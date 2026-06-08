# Two-Stage Hand Pose Design

Date: 2026-06-06
Status: Draft for review
Scope: Replace the single-stage center-crop landmark inference in `src/gesture_control/` with a MediaPipe-style two-stage pipeline (palm detector → rotated ROI → landmark).

## Goal

Fix the Left/Right gesture failure observed on live webcam by replacing the current center-square-crop landmark inference with a two-stage palm-detection → landmark pipeline. The new pipeline preserves the public `HandLandmarks { landmarks: [Vec2; 21], confidence }` contract so the existing classifier, overlay, debounce, and HTTP path do not change.

## Context

The shipped code in `src/gesture_control/` runs a single-stage MediaPipe hand-landmark ONNX (`handpose_estimation_mediapipe_2023feb.onnx`) on a center-square crop of the webcam frame. The original task spec `specs/task-gesture-robot-control.spec.md` always called for a two-stage pipeline; the single-stage was a deferred stand-in (see `src/gesture_control/model_downloader.rs:9-12`).

User testing has confirmed:

- Forward, Back, Catch, and Drop classify correctly.
- Left and Right fail.

Most-likely cause, in order:

1. **Center-square-crop chops off horizontally-extended fingertips.** On a 640×480 webcam center-cropped to 480×480, ~80 source columns on each side are discarded. A sideways-pointing index fingertip lands in the chopped region and the landmark model emits a "fingertip" landmark mid-finger, weakening the wrist→tip X component.
2. **`AXIS_DOMINANCE_RATIO = 1.3`** then rejects the weakened X component, classifying as `None` or mis-classifying as Forward/Back when Y dominates by accident.

The two-stage pipeline addresses both: letterboxing the source frame into the palm detector input preserves full horizontal extent, the palm detector finds the rotated palm bbox even when the hand is sideways, and the landmark model sees a tight, upright crop where the fingertip is always inside the ROI.

## Architecture

### Pipeline data flow per inference frame

```
RGBA frame (W×H)
    │
    ▼ palm_model.run(rgba, W, H)
        ├─ Letterbox into [1, 192, 192, 3] f32, normalize (pixel/127.5 − 1.0,
        │    subject to first-load verification — see Open Verification Items)
        ├─ tract.run → regressors [1, 2016, 18] + scores [1, 2016, 1]
        ├─ Decode against 2016 anchors → list of (cx, cy, w, h, 7 keypoints, score)
        ├─ Sigmoid scores, threshold ≥ PALM_SCORE_THRESHOLD
        └─ NMS (IoU PALM_NMS_IOU_THRESHOLD) → top-1 candidate (or None)
    │
    ▼ hand_roi.derive_roi(candidate)
        ├─ rotation derived from wrist (kp0) → middle-MCP (kp2)
        ├─ scale = palm bbox dimension × 2.6
        ├─ center shifted 0.5 box-sides toward fingertips
        └─ RotatedRoi { center, size, rotation } in normalized source-frame coords
    │
    ▼ hand_roi.warp_to_landmark_input(rgba, W, H, roi)
        → bilinear-sampled [1, 224, 224, 3] f32 tensor, normalized /255
    │
    ▼ hand_model.run(roi_tensor)
        ├─ tract.run → [1, 63] landmarks (in 224-px space) + [1, 1] sigmoid presence score
        └─ hand_roi.unwarp_landmark(...) maps each landmark back to normalized full-frame coords
    │
    ▼ HandLandmarks { landmarks: [Vec2; 21], confidence }   // unchanged public shape
```

When the palm stage returns no candidate above threshold, the orchestrator returns `Ok(None)` and the landmark stage is skipped entirely. The center-crop fallback is explicitly removed.

### Key contract preservations

- `HandLandmarks.landmarks[i]` stays in `[0, 1]` source-frame coords. The classifier (`gesture_classifier::classify`) is unchanged.
- `HandLandmarks.confidence` continues to be the landmark model's hand-presence score (sigmoid-applied output `Identity_1`). The existing `CONFIDENCE_THRESHOLD = 0.6` semantics carry over unchanged.
- The on-video 21-dot overlay is unchanged.
- The HTTP path, debounce, action enum, IP textbox, and connection indicator are unchanged.

## File Layout

| File | Status | Responsibility | Approx LOC |
|------|--------|----------------|------------|
| `palm_anchors.rs` | new | Generate the 2016-entry anchor table at startup from MediaPipe's SSD anchor config. Pure function, no I/O. | ~80 |
| `palm_model.rs` | new | Owns the palm ONNX `RunnablePlan`. Loads, runs, preprocesses (letterbox + normalize), decodes (sigmoid + threshold + NMS), returns top-1 `PalmCandidate` in normalized source-frame coords. | ~180 |
| `hand_roi.rs` | new | Pure math. Derives `RotatedRoi` from candidate keypoints, warps source RGBA to landmark input via bilinear sampling, unwarps landmark points back to source-frame coords. | ~120 |
| `hand_model.rs` | modify | Strip the center-crop logic and the NHWC buffer build. Change `run(...)` to take the pre-built warped `Vec<f32>` of length `224*224*3` instead of raw RGBA. Keeps ownership of `INPUT_SIZE = 224`, the tract plan, and the output-tensor parsing (`Identity` → 21 × (x, y, z) divided by 224, `Identity_1` → sigmoid confidence). Returns landmarks in 224-space; caller unwarps. | ~90 (was ~130) |
| `hand_pipeline.rs` | new | Composition root. Owns both models and the anchor table. `run(rgba, w, h) → Result<Option<HandLandmarks>>` — same signature today's `HandModel::run` exposes to the worker. | ~80 |
| `model_downloader.rs` | modify | Add `PALM_DETECTION_FILENAME`, `PALM_DETECTION_SHA256`, `palm_detection_path()`. Rename `landmark_model_present_and_valid()` → `models_present_and_valid()` (single caller in `robot_screen.rs` updated). `ensure_downloaded()` now downloads both files. | +60 |
| `inference_worker.rs` | modify | Replace `Arc<HandModel>` with `Arc<HandPipeline>`. Worker loop body unchanged. | ~5 lines |
| `mod.rs` | modify | Add `pub mod palm_anchors;`, `pub mod palm_model;`, `pub mod hand_roi;`, `pub mod hand_pipeline;`. | +4 lines |

### Boundary discipline

- `palm_anchors.rs` knows nothing about ONNX or tract — pure anchor geometry.
- `hand_roi.rs` knows nothing about ONNX — pure pixel sampling math, testable with synthetic frames.
- `palm_model.rs` and `hand_model.rs` are thin tract wrappers, no knowledge of each other.
- `hand_pipeline.rs` is the only file aware that two stages exist.

## Component Detail

### Palm detector (`palm_model.rs`)

**Model file:**

```rust
pub const PALM_DETECTION_FILENAME: &str = "palm_detection_mediapipe_2023feb.onnx";
pub const PALM_DETECTION_SHA256: &str = "";   // empty: `file_valid` (model_downloader.rs:58) short-
                                              // circuits to a presence-only check when the expected
                                              // SHA is empty. This is the existing intentional helper
                                              // behavior — reused so the empty constant means
                                              // "downloaded file is trusted on first run, then pinned
                                              // by a follow-up commit". Same pattern used historically
                                              // for the landmark model.
```

URL pattern: `{MODEL_BASE_URL}/{PALM_DETECTION_FILENAME}` — reuses the existing constant pointing at `github.com/opencv/opencv_zoo/raw/main/models/handpose_estimation_mediapipe`.

**Expected ONNX I/O** (verified at implementation time via `tract` introspection, not assumed):

- Input: name discovered at load time via `tract` introspection (see Open Verification Items #1). Expected NHWC `[1, 192, 192, 3]` f32, RGB, values in `[-1, 1]` (i.e. `pixel/127.5 − 1.0`).
- Output 0: regressors `[1, 2016, 18]` f32 — per anchor, `(dx, dy, dw, dh, kp0.x, kp0.y, …, kp6.x, kp6.y)` in input-pixel units relative to the anchor center.
- Output 1: scores `[1, 2016, 1]` f32 — raw logits, sigmoid not applied.

**The implementation must verify these shapes at runtime** by printing `model.input_outlets()` and `model.output_outlets()` on first load and failing loudly if they diverge. Decode constants in dependent code are then adjusted to match reality, not the spec.

**Preprocess — letterbox into 192×192:**

Source frames are typically 640×480 or 1280×720 (not square). The current single-stage path center-square-crops first; for two-stage we drop the center-square-crop entirely. The palm detector runs on the full frame letterboxed into 192×192 with neutral grey padding.

A `LetterboxTransform { scale: f32, pad_x: f32, pad_y: f32 }` is computed during preprocess and stored on the returned `PalmCandidate` so the decoder maps detector-space coordinates back to normalized source-frame coordinates.

This is a behavior change vs today: **hands at the edges of a wide-aspect frame will now be detected.** The existing single-stage path silently ignored those frames.

**Decode constants:**

```rust
pub const PALM_SCORE_THRESHOLD: f32 = 0.5;       // sigmoid score gate
pub const PALM_NMS_IOU_THRESHOLD: f32 = 0.3;
pub const PALM_BOX_SCALE: f32 = 1.0;             // regressor → bbox scale (divided by input_size)
pub const PALM_KEYPOINT_SCALE: f32 = 1.0;        // same for the 7 keypoints
pub const PALM_SCORE_CLIP: f32 = 100.0;          // clamp logits before sigmoid to avoid overflow
```

These are the standard MediaPipe values. If the OpenCV Zoo build emits sigmoid scores already (some forks do), threshold semantics change. The first-run smoke test (described in Testing) gates whether these constants stay or get adjusted before downstream tasks proceed.

**Multi-detection:** the detector emits multiple candidates after NMS. Per the existing task spec ("no two-handed gestures"), the pipeline keeps only the top-scoring candidate.

### Anchor table (`palm_anchors.rs`)

MediaPipe's SSD anchors are deterministic from these constants (from `mediapipe/calculators/tflite/ssd_anchors_calculator.proto` for the palm detector):

```rust
pub struct AnchorOptions {
    pub num_layers: usize,                       // 4
    pub min_scale: f32,                          // 0.1484375
    pub max_scale: f32,                          // 0.75
    pub input_size: usize,                       // 192
    pub anchor_offset_x: f32,                    // 0.5
    pub anchor_offset_y: f32,                    // 0.5
    pub strides: [usize; 4],                     // [8, 16, 16, 16]
    pub aspect_ratios: &'static [f32],           // &[1.0]
    pub interpolated_scale_aspect_ratio: f32,    // 1.0
}

pub struct Anchor { pub cx: f32, pub cy: f32, pub w: f32, pub h: f32 }
pub fn build_anchors() -> Vec<Anchor>;
```

The 2016 count derives from `Σ over layers of (feature_map_size² × num_anchors_per_position)` with the strides above. Unit test `anchor_count_is_2016` pins this. If the OpenCV Zoo port retrained with different anchor config, this test fails immediately and the constants get re-derived before any further task proceeds.

### ROI math (`hand_roi.rs`)

**Palm-keypoint convention** (MediaPipe palm detection, same 7 keypoints emitted by the OpenCV Zoo model):

| Index | Anatomical point |
|-------|------------------|
| kp0 | wrist center |
| kp1 | thumb base (CMC area) |
| kp2 | middle finger MCP — **used for orientation** |
| kp3 | index finger PIP |
| kp4 | pinky base |
| kp5 | thumb tip approximation |
| kp6 | index/middle MCP midpoint |

Only `kp0` and `kp2` are used in ROI derivation. The others are ignored.

**ROI derivation:**

```rust
pub struct RotatedRoi {
    pub center: Vec2,    // normalized [0,1] source-frame coords
    pub size: f32,       // normalized side length of the square ROI
    pub rotation: f32,   // radians, 0 = upright, positive = CCW
}

pub fn derive_roi(c: &PalmCandidate) -> RotatedRoi {
    // Target: wrist→middle-MCP should point "up" in image coords (-π/2).
    let dx = c.keypoints[2].x - c.keypoints[0].x;
    let dy = c.keypoints[2].y - c.keypoints[0].y;
    let current_angle = dy.atan2(dx);
    let target_angle = -std::f32::consts::FRAC_PI_2;
    let rotation = target_angle - current_angle;

    // Box scale: palm bbox dim × 2.6 (MediaPipe published; pads so all fingers
    // stay in-frame even when extended).
    let box_side = c.w.max(c.h);
    let size = box_side * 2.6;

    // Center shifted 0.5 box-sides along wrist→MCP axis — places the palm at
    // the ROI's lower portion, matching the landmark model's training crop.
    let shift = 0.5 * box_side;
    let norm = dx.hypot(dy).max(1e-6);
    let cx = c.cx + shift * dx / norm;
    let cy = c.cy + shift * dy / norm;

    RotatedRoi { center: Vec2::new(cx, cy), size, rotation }
}
```

Constants `2.6` and `0.5` are MediaPipe-published. If end-to-end smoke shows systematic landmark bias, these are tunable as a follow-up task (not in scope for v1 unless a regression is observed on the smoke fixture).

**Bilinear warp to landmark input:**

For each of the 224×224 destination pixels, compute the source-frame sample location through inverse rotation, sample bilinearly, normalize to `[0, 1]`. Out-of-frame samples return zero.

```rust
pub fn warp_to_landmark_input(rgba: &[u8], w: u32, h: u32, roi: &RotatedRoi) -> Vec<f32>;
```

`sample_bilinear_rgb` is a ~20-line private helper: four nearest pixels, weighted by fractional offset, with bounds-checked zero-fill outside the frame.

**Note on aspect:** `roi.size` is normalized against `w.min(h)` so the ROI is geometrically square in source pixels regardless of frame aspect.

**Inverse landmark transform:**

```rust
pub fn unwarp_landmark(roi_point: Vec2, roi: &RotatedRoi, w: u32, h: u32) -> Vec2;
```

`roi_point` is in `[0, 1]` of the 224×224 input. Returns normalized `[0, 1]` full-frame coords. Matches today's contract with `gesture_classifier.rs`.

### Pipeline orchestrator (`hand_pipeline.rs`)

```rust
pub struct HandPipeline {
    palm: PalmModel,
    landmark: HandModel,
    anchors: Vec<Anchor>,
}

impl HandPipeline {
    pub fn load() -> Result<Self>;
    pub fn run(&self, rgba: &[u8], w: u32, h: u32) -> Result<Option<HandLandmarks>>;
}
```

The `run` signature matches today's `HandModel::run` so the worker change is mechanical. Internally:

1. `palm.run(rgba, w, h)` → `Option<PalmCandidate>`. If `None`, return `Ok(None)`.
2. `hand_roi::derive_roi(&candidate)` → `RotatedRoi`.
3. `hand_roi::warp_to_landmark_input(rgba, w, h, &roi)` → warped tensor.
4. `landmark.run(&warped)` → 21 landmarks in 224-space + confidence.
5. Map each landmark through `unwarp_landmark(...)` → normalized full-frame coords.
6. Return `Ok(Some(HandLandmarks { landmarks, confidence }))`.

### Worker integration (`inference_worker.rs`)

Minimal change:

```rust
// In InferenceWorker::spawn:
let pipeline = Arc::new(HandPipeline::load().context("HandPipeline::load")?);

fn worker_loop(
    pipeline: Arc<HandPipeline>,        // was: Arc<HandModel>
    frame_rx: Receiver<WebRtcVideoFrame>,
    result_tx: Sender<InferenceResult>,
) {
    while let Ok(frame) = frame_rx.recv() {
        match pipeline.run(&frame.data, frame.width, frame.height) {   // was: model.run(...)
            // body unchanged
        }
    }
}
```

`INFERENCE_FRAME_SKIP=3` and `CONFIDENCE_THRESHOLD=0.6` are unchanged. The `InferenceWorker::Drop` shutdown semantics (detached join via channel close) are unchanged — swapping `Arc<HandModel>` for `Arc<HandPipeline>` does not touch the worker's lifecycle.

**Why the landmark-model confidence (not the palm-detector score) is the one we threshold:** the palm score signals "this looks like a hand region"; the landmark model's confidence signals "the 21 joints I produced are trustworthy". The classifier consumes joints, not regions, so the landmark confidence is the correct gate.

### Model download (`model_downloader.rs`)

```rust
pub const PALM_DETECTION_FILENAME: &str = "palm_detection_mediapipe_2023feb.onnx";
pub const PALM_DETECTION_SHA256: &str = "";  // pinned after first successful download

pub fn palm_detection_path() -> PathBuf {
    models_dir().join(PALM_DETECTION_FILENAME)
}

pub fn models_present_and_valid() -> bool {
    file_valid(&hand_landmark_path(), HAND_LANDMARK_SHA256)
        && file_valid(&palm_detection_path(), PALM_DETECTION_SHA256)
}

pub async fn ensure_downloaded() -> Result<()> {
    // ... existing setup ...
    download_if_invalid(&client, HAND_LANDMARK_FILENAME, HAND_LANDMARK_SHA256, &hand_landmark_path()).await?;
    download_if_invalid(&client, PALM_DETECTION_FILENAME, PALM_DETECTION_SHA256, &palm_detection_path()).await?;
    Ok(())
}
```

The existing `landmark_model_present_and_valid()` function is renamed to `models_present_and_valid()`; the single caller in `robot_screen.rs` is updated. The user-facing "Downloading hand model…" status string is unchanged — users don't need to know there are two files.

## Testing

### Unit tests (`#[cfg(test)]`, automated)

| Test | File | What it pins |
|------|------|--------------|
| `anchor_count_is_2016` | `palm_anchors.rs` | `build_anchors().len() == 2016`. Regression guard for anchor config drift. |
| `anchor_centers_are_in_unit_square` | `palm_anchors.rs` | All anchors satisfy `0 ≤ cx, cy ≤ 1`. Catches stride/offset errors. |
| `anchor_first_layer_count` | `palm_anchors.rs` | Layer 0 (stride 8) produces `(192/8)² = 576` anchors. Catches per-layer breakdown. |
| `letterbox_roundtrip` | `palm_model.rs` | A point at known source-frame coords letterboxed then inverse-mapped lands at the original (within 1 px). |
| `nms_keeps_highest_drops_overlap` | `palm_model.rs` | Two synthetic candidates at IoU 0.5 with different scores: NMS keeps the higher. |
| `roi_derivation_upright_hand` | `hand_roi.rs` | `kp0=(0.5, 0.8)`, `kp2=(0.5, 0.4)` (pointing straight up) → `rotation ≈ 0`. |
| `roi_derivation_sideways_hand` | `hand_roi.rs` | `kp0=(0.3, 0.5)`, `kp2=(0.7, 0.5)` (pointing right) → `rotation ≈ -π/2`. **This test pins the L/R failure mode at the math layer.** |
| `warp_identity_when_no_rotation` | `hand_roi.rs` | ROI with `rotation=0`, centered, size matching source: warped tensor equals downsampled source within bilinear tolerance. |
| `warp_corner_rotation` | `hand_roi.rs` | Synthetic vertical-stripe image warped through 90° rotation produces horizontal stripes at expected positions. |
| `unwarp_inverse_of_warp` | `hand_roi.rs` | Point at known ROI-local coords → warp → unwarp returns to within 1 px of the original. |

### Integration / smoke tests (manual, run during implementation)

| Test | Gates which task | What it pins |
|------|------------------|--------------|
| ONNX I/O smoke | Task 1 — palm-model wrapper | Loads palm ONNX, prints input/output names and shapes, fails loudly on any divergence from the expected I/O documented above. Decode constants get adjusted before downstream tasks start. |
| End-to-end on a known-hand still | After pipeline composes | One palm image, hand approximately centered, vertical. Pipeline must return `Some(HandLandmarks)` with the index-finger-tip landmark within ±0.05 of expected `[0,1]` coords. Failure means anchor params, decode constants, or ROI math are wrong. |
| **End-to-end Left/Right pointing** | Before declaring complete | The motivating regression. Live webcam: point index finger horizontally left, then right. Pipeline must emit `GestureAction::Left` and `GestureAction::Right` over a 5-second hold each. |
| End-to-end Forward/Back/Catch/Drop preserved | Before declaring complete | The previously-working gestures still work — no regression from the architecture change. |

## Open Verification Items

These items are documented as expected values in this design but must be confirmed against the real ONNX file at implementation time, not assumed:

1. **Palm ONNX input/output tensor names** — confirmed via `tract` introspection in the first implementation task.
2. **Palm input normalization** — `(pixel/127.5 − 1.0)` is the standard MediaPipe value. Confirmed from the model's introspectable params or from running a known image through it.
3. **Score format** — pre-sigmoid logits vs post-sigmoid probabilities. Determined by inspecting the first few output values from a real frame; `PALM_SCORE_THRESHOLD` adjusted accordingly.
4. **Anchor config constants** — listed values are MediaPipe-published. If the OpenCV Zoo port retrained with different config, the `anchor_count_is_2016` test fails and constants get re-derived before further work.
5. **ROI shift/scale constants** (`2.6`, `0.5`) — published values. If end-to-end smoke shows systematic landmark bias, a tuning sub-step is added.
6. **Palm model SHA-256** — empty in first commit, pinned in a follow-up commit after first successful download.

## Out of Scope

- Tracking-mode / detection-skip optimization. The vanilla design runs both stages every inference frame for simplicity and to avoid drift failure modes.
- Center-crop fallback when palm detection fails. The old single-stage center-crop path is removed entirely; "no palm detected" emits `None`.
- Multi-hand processing. The task spec already excludes two-handed gestures; the pipeline keeps only the top-scoring palm candidate.
- Changing classifier thresholds (`EXTENSION_RATIO`, `AXIS_DOMINANCE_RATIO`). If Left/Right still fails after two-stage is correctly wired, that is a follow-up tuning task.
- New ML runtime dependencies. The existing `tract-onnx`, `crossbeam-channel`, `sha2` set is sufficient; no `imageproc`, `image`-crate filters, or affine-warp libraries are added.
- UI changes to the Robot screen. The on-video overlay, panel layout, IP textbox, and connection indicator are unchanged.
- iOS and Web (existing spec exclusions).

## Behavior Changes vs Today

These are observable changes a user could detect:

1. **Hands at the wide-aspect edges of the frame are detected.** The single-stage center-crop silently ignored ~80 columns on each side of a 640×480 frame; letterboxing preserves them.
2. **Left and Right pointing classifies correctly.** The motivating regression.
3. **"No palm detected" emits `GestureAction::None` rather than running landmark on a center-crop and emitting a possibly-incorrect classification.** Slightly fewer spurious emits expected.

## Follow-ups Outside This Spec

- Pin `PALM_DETECTION_SHA256` after the first successful download.
- Update `specs/task-gesture-robot-control.spec.md` to remove the "deferred" language for the palm detection stage.
- If the smoke fixture reveals decode-constant divergence (e.g., scores already sigmoided in the OpenCV Zoo port), commit the adjusted constants with rationale in the commit body.
