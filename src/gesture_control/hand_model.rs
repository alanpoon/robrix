//! Single-stage MediaPipe hand-landmark inference via `tract`.
//!
//! Loads the OpenCV Zoo handpose ONNX file and runs 21-landmark inference on a
//! letterboxed 224×224 view of the full source frame (grey padding on the
//! narrower axis). Letterboxing — vs the previous center-square crop — preserves
//! horizontally-extended index fingertips that on a 640×480 webcam used to fall
//! into the ~80 columns discarded on each side. The change keeps the model's
//! input contract identical while restoring a strong wrist→tip X component for
//! the Left/Right pointing gestures.
//!
//! Model I/O (verified by inspecting the file):
//! - input  `input_1`     NHWC f32 `[1, 224, 224, 3]`, pixel values in `[0, 1]`
//! - output `Identity`    f32 `[1, 63]`  — 21 landmarks × (x, y, z) in 224-px space
//! - output `Identity_1`  f32 `[1, 1]`   — hand presence score, **sigmoid applied**
//! - output `Identity_2`  f32 `[1, 1]`   — handedness score
//! - output `Identity_3`  f32 `[1, 63]`  — 3D world landmarks (unused)
//!
//! Coordinate convention exposed to the rest of the module: x and y in `[0, 1]`
//! relative to the cropped square — matches `gesture_classifier::Vec2` contract.

use anyhow::{Context, Result};
use tract_onnx::prelude::*;

use crate::gesture_control::gesture_classifier::Vec2;

const INPUT_SIZE: usize = 224;

/// One inference result: 21 hand landmarks, the model's overall confidence
/// for the hand-vs-no-hand head, and the handedness score (close to 0 or 1
/// depending on which anatomical hand the model identifies).
#[derive(Clone, Debug)]
pub struct HandLandmarks {
    pub landmarks: [Vec2; 21],
    pub confidence: f32,
    pub handedness: f32,
}

type RunnablePlan = SimplePlan<
    TypedFact,
    Box<dyn TypedOp>,
    Graph<TypedFact, Box<dyn TypedOp>>,
>;

/// Loaded ONNX model, ready to be invoked on an RGBA frame.
pub struct HandModel {
    plan: RunnablePlan,
}

impl HandModel {
    /// Load the landmark ONNX from the standard app_data_dir location.
    pub fn load() -> Result<Self> {
        let path = crate::gesture_control::model_downloader::hand_landmark_path();
        let plan = tract_onnx::onnx()
            .model_for_path(&path)
            .with_context(|| format!("load ONNX {}", path.display()))?
            .into_optimized()
            .context("optimize ONNX graph")?
            .into_runnable()
            .context("build runnable plan")?;
        Ok(Self { plan })
    }

    /// Run inference on an RGBA frame.
    ///
    /// Letterboxes the full frame into a 224×224 NHWC buffer with neutral grey
    /// padding on the narrower axis, normalizes to `[0, 1]` float32, runs the
    /// model, and returns 21 landmarks in `[0, 1]` of the 224 buffer plus the
    /// hand-presence score. Horizontal aspect is preserved so the wrist→tip
    /// vector direction stays accurate for Left/Right pointing.
    ///
    /// Returns `None` if the frame is too small to sample.
    pub fn run(&self, rgba: &[u8], width: u32, height: u32) -> Result<Option<HandLandmarks>> {
        if width < 4 || height < 4 {
            return Ok(None);
        }
        let w = width as usize;
        let h = height as usize;
        let row_stride = w * 4;

        // Letterbox: scale the longer axis to INPUT_SIZE, pad the shorter axis
        // with neutral grey. Using floating-point math for the scale keeps the
        // visible content centered to within a pixel on common aspect ratios.
        let longer = w.max(h);
        let scale = INPUT_SIZE as f32 / longer as f32;
        let new_w = ((w as f32) * scale).round() as usize;
        let new_h = ((h as f32) * scale).round() as usize;
        let pad_x = (INPUT_SIZE - new_w.min(INPUT_SIZE)) / 2;
        let pad_y = (INPUT_SIZE - new_h.min(INPUT_SIZE)) / 2;

        // Neutral mid-grey for padded pixels, matching MediaPipe's letterbox fill.
        const PAD_VALUE: f32 = 0.5;
        let mut data = vec![PAD_VALUE; INPUT_SIZE * INPUT_SIZE * 3];
        for ty in 0..INPUT_SIZE {
            if ty < pad_y || ty >= pad_y + new_h {
                continue;
            }
            let sy = (((ty - pad_y) as f32 + 0.5) / scale) as usize;
            if sy >= h {
                continue;
            }
            let row_off = sy * row_stride;
            let dst_row = ty * INPUT_SIZE * 3;
            for tx in 0..INPUT_SIZE {
                if tx < pad_x || tx >= pad_x + new_w {
                    continue;
                }
                let sx = (((tx - pad_x) as f32 + 0.5) / scale) as usize;
                if sx >= w {
                    continue;
                }
                let i = row_off + sx * 4;
                if i + 2 >= rgba.len() {
                    continue;
                }
                let dst = dst_row + tx * 3;
                data[dst]     = rgba[i] as f32 / 255.0;
                data[dst + 1] = rgba[i + 1] as f32 / 255.0;
                data[dst + 2] = rgba[i + 2] as f32 / 255.0;
            }
        }

        let input: Tensor = tract_ndarray::Array4::from_shape_vec(
            (1, INPUT_SIZE, INPUT_SIZE, 3),
            data,
        )
        .context("build input tensor")?
        .into();

        let outputs = self
            .plan
            .run(tvec!(input.into()))
            .context("run hand landmark inference")?;

        // Output 0: [1, 63] landmark coords in 224-px space (x, y, z per joint).
        let landmarks_t = outputs[0]
            .to_array_view::<f32>()
            .context("read landmark output")?;
        // Output 1: [1, 1] sigmoid hand-presence score.
        let score_t = outputs[1]
            .to_array_view::<f32>()
            .context("read score output")?;
        // Output 2: [1, 1] handedness score. The MediaPipe convention is
        // ~0 for one anatomical hand and ~1 for the other; the polarity for
        // the OpenCV Zoo port fed with this codebase's camera orientation is
        // empirically pinned by `HANDEDNESS_RIGHT_THRESHOLD` in the classifier.
        let handedness_t = outputs[2]
            .to_array_view::<f32>()
            .context("read handedness output")?;

        let confidence = score_t.as_slice().map(|s| s[0]).unwrap_or(0.0);
        let handedness = handedness_t.as_slice().map(|s| s[0]).unwrap_or(0.5);

        let lm_slice = landmarks_t
            .as_slice()
            .context("landmark tensor not contiguous")?;
        if lm_slice.len() < 21 * 3 {
            anyhow::bail!("landmark output too short: {}", lm_slice.len());
        }
        let mut landmarks = [Vec2::default(); 21];
        let scale = INPUT_SIZE as f32;
        for i in 0..21 {
            let x = lm_slice[i * 3] / scale;
            let y = lm_slice[i * 3 + 1] / scale;
            landmarks[i] = Vec2::new(x, y);
        }

        Ok(Some(HandLandmarks { landmarks, confidence, handedness }))
    }
}
