//! Background inference thread that owns a `HandModel` and processes frames
//! pushed via a bounded(1) channel.
//!
//! Single-shot mode (one frame per button click) and continuous mode (camera
//! capture pushing every Nth frame) both work — the worker just consumes
//! whatever shows up on `frame_rx`.

use std::collections::VecDeque;
use std::sync::Arc;
use std::time::{Duration, Instant};

use crossbeam_channel::{Receiver, Sender, TrySendError, bounded, unbounded};
use makepad_widgets::{Cx, log};

use crate::gesture_control::{
    GestureAction,
    gesture_classifier,
    hand_model::{HandLandmarks, HandModel},
};
use crate::shared::webrtc_video::WebRtcVideoFrame;

/// Lifecycle signal posted by the background `spawn_blocking` task that
/// owns the `HandModel`. The Robot tab listens for this on its Actions
/// loop and only promotes a pending `InferenceWorker` into the active
/// `self.inference` slot once `Ready` has fired — that way callers can
/// assume the worker is actually loaded and looping before they start
/// counting on frame results.
#[derive(Clone, Debug)]
pub enum InferenceWorkerAction {
    /// Model loaded successfully and `worker_loop` is now running.
    Ready,
    /// `HandModel::load` returned an error; the worker exited without
    /// entering its processing loop.
    LoadFailed(String),
}

/// Skip every Nth frame received from the capture callback. With a 30 fps
/// camera and N=3 this gives ~10 inferences per second.
pub const INFERENCE_FRAME_SKIP: u32 = 3;

/// Confidence threshold below which a detection is dropped (no emit, no HTTP).
pub const CONFIDENCE_THRESHOLD: f32 = 0.6;

/// Number of recent frames the motion tracker averages over for the
/// stationarity and swipe-direction checks. At ~10 inferences per second
/// this is roughly a half-second window — enough to integrate a deliberate
/// swipe without lagging behind quick hand movements.
const MOTION_WINDOW: usize = 5;

/// Net horizontal wrist displacement (last frame minus first frame in
/// window) required to register a horizontal swipe, expressed as a
/// fraction of the minimum palm size observed in the window. Sized so a
/// deliberate sideways swipe (~one palm width across the window) clears
/// the bar while incidental wrist drift during a static pose does not.
const SWIPE_MIN_NET_DX_PALM_FRACTION: f32 = 0.8;

/// How strongly the horizontal motion must dominate the vertical motion to
/// count as a clean left/right swipe rather than a diagonal one. Mirrors
/// `gesture_classifier::AXIS_DOMINANCE_RATIO`'s 1.3.
const SWIPE_HORIZONTAL_DOMINANCE: f32 = 1.3;

/// Minimum gap between two consecutive Left/Right emissions from the
/// motion-based detector. Without this, a sustained sideways swipe would
/// fire Left every inference frame (~10 / s) and flood the robot's command
/// queue. One second matches the natural "one discrete command per swipe"
/// pacing — long enough to count as a separate gesture, short enough that
/// the user doesn't feel a noticeable latency between deliberate
/// back-to-back swipes.
const LR_DEBOUNCE_MS: u64 = 1000;

/// One inference output delivered to the UI thread.
#[derive(Clone, Debug)]
pub struct InferenceResult {
    pub landmarks: Option<HandLandmarks>,
    pub detected: GestureAction,
}

/// Handle to a running inference worker. Drop it to shut down the worker —
/// `frame_tx` closes when the struct goes out of scope, which makes the
/// worker's blocking `recv()` return `Err` and the loop exits naturally.
pub struct InferenceWorker {
    frame_tx: Sender<WebRtcVideoFrame>,
    result_rx: Receiver<InferenceResult>,
}

impl InferenceWorker {
    /// Create the channels and schedule the background worker. Both
    /// `HandModel::load` (CPU-bound 4 MB ONNX parse) and the per-frame
    /// inference loop run inside a single `tokio::task::spawn_blocking`
    /// task on the shared matrix runtime — that way the UI thread never
    /// stalls waiting on the model parse, and there's a single managed
    /// blocking pool for all background CPU work instead of an ad-hoc
    /// thread-per-feature sprawl.
    ///
    /// The returned `Self` is "pending": its `frame_tx` channel exists,
    /// but `worker_loop` hasn't necessarily started consuming frames yet.
    /// Callers should hold the value off to the side until they observe
    /// an [`InferenceWorkerAction::Ready`] on the Actions bus, then
    /// promote it into the active slot. If the load fails, an
    /// [`InferenceWorkerAction::LoadFailed`] is posted instead and the
    /// pending value can be dropped.
    pub fn spawn() -> Self {
        let (frame_tx, frame_rx) = bounded::<WebRtcVideoFrame>(1);
        let (result_tx, result_rx) = unbounded::<InferenceResult>();
        log!("InferenceWorker::spawning");
        crate::sliding_sync::spawn_async_task(async move {
            let _ = tokio::task::spawn_blocking(move || {
                match HandModel::load() {
                    Ok(model) => {
                        log!("InferenceWorker: hand-model loaded; posting Ready");
                        Cx::post_action(InferenceWorkerAction::Ready);
                        worker_loop(Arc::new(model), frame_rx, result_tx);
                    }
                    Err(e) => {
                        let msg = format!("{e:#}");
                        log!("InferenceWorker: hand-model load failed: {msg}");
                        Cx::post_action(InferenceWorkerAction::LoadFailed(msg));
                        // Drop frame_rx / result_tx implicitly; pending host
                        // can release the `Self` once it sees LoadFailed.
                    }
                }
            })
            .await;
        });

        Self {
            frame_tx,
            result_rx,
        }
    }

    /// Submit a frame for inference. Returns `false` if the channel is full
    /// (worker is still processing the previous frame) — caller can ignore.
    pub fn submit(&self, frame: WebRtcVideoFrame) -> bool {
        match self.frame_tx.try_send(frame) {
            Ok(()) => true,
            Err(TrySendError::Full(_)) => false,
            Err(TrySendError::Disconnected(_)) => false,
        }
    }

    /// Pull the next inference result if available without blocking.
    pub fn try_recv(&self) -> Option<InferenceResult> {
        self.result_rx.try_recv().ok()
    }
}

fn worker_loop(
    model: Arc<HandModel>,
    frame_rx: Receiver<WebRtcVideoFrame>,
    result_tx: Sender<InferenceResult>,
) {
    log!("inference worker started");
    let mut tracker = MotionTracker::new();
    // Wall-clock of the most recent Left/Right we emitted, used to enforce
    // `LR_DEBOUNCE_MS`. `None` until the first L/R fires.
    let mut last_lr_emit_at: Option<Instant> = None;
    while let Ok(frame) = frame_rx.recv() {
        match model.run(&frame.data, frame.width, frame.height) {
            Ok(Some(hand)) => {
                // Record this frame's wrist position + palm size for the
                // motion test below. We do this even when confidence is
                // too low for a classify call — the tracker still wants
                // the kinematic history, so a brief confidence dip doesn't
                // appear as "hand suddenly teleported" once confidence
                // recovers.
                tracker.record_from(&hand);

                let mut detected = if hand.confidence >= CONFIDENCE_THRESHOLD {
                    gesture_classifier::classify(&hand.landmarks, hand.handedness)
                        .unwrap_or(GestureAction::None)
                } else {
                    GestureAction::None
                };
                // Horizontal hand motion takes priority over whatever
                // static pose the classifier matched — the user is in the
                // middle of a swipe and we should emit Left/Right rather
                // than whatever transitional finger shape the model
                // happens to see mid-motion. Debounced by
                // `LR_DEBOUNCE_MS` so a sustained swipe produces ONE
                // discrete command instead of a per-frame flood.
                if let Some(swipe) = tracker.horizontal_swipe() {
                    let now = Instant::now();
                    let recently_emitted = last_lr_emit_at
                        .map(|t| now.duration_since(t) < Duration::from_millis(LR_DEBOUNCE_MS))
                        .unwrap_or(false);
                    if !recently_emitted {
                        detected = swipe;
                        last_lr_emit_at = Some(now);
                    }
                    // else: still mid-swipe, but we already emitted this
                    // command — leave `detected` as the classifier's
                    // output (typically None mid-motion).
                }
                // Grab fires whenever the classifier detects a closed
                // fist — no stationarity gate. The motion-based Left/Right
                // detector above already wins over Catch during a swipe
                // (it overrides `detected` first), so a hand moving
                // sideways with a closed fist still emits Left/Right
                // rather than Grab.
                let result = InferenceResult {
                    landmarks: Some(hand),
                    detected,
                };
                if result_tx.send(result).is_err() {
                    break;
                }
            }
            Ok(None) => {
                // No hand visible this frame. Clear the kinematic history
                // so a hand that reappears later doesn't get spuriously
                // labeled "stationary" by comparing a fresh sample to
                // stale samples from before it left the frame.
                tracker.reset();
                if result_tx
                    .send(InferenceResult {
                        landmarks: None,
                        detected: GestureAction::None,
                    })
                    .is_err()
                {
                    break;
                }
            }
            Err(e) => {
                log!("inference error: {e:#}");
            }
        }
    }
    log!("inference worker exiting");
}

/// Rolling-window tracker over the wrist's recent positions.
///
/// Used for two related checks:
///
/// 1. **Stationarity** ([`is_stationary`]) — the hand has stayed roughly put
///    for the whole window, so it's safe to fire a one-shot gesture like
///    Grab without picking up a transitional pose mid-motion.
/// 2. **Horizontal swipe** ([`horizontal_swipe`]) — the wrist has moved
///    decisively left or right over the window, so we emit
///    [`GestureAction::Left`] / [`GestureAction::Right`].
///
/// Distances are normalized to the smallest palm size observed in the
/// window so the thresholds are scale-invariant: a hand close to the
/// camera and a hand far away both get the same physical-motion budget.
struct MotionTracker {
    /// (wrist_x, wrist_y, palm_size) for recent frames, newest at the back.
    history: VecDeque<(f32, f32, f32)>,
}

impl MotionTracker {
    fn new() -> Self {
        Self {
            history: VecDeque::with_capacity(MOTION_WINDOW),
        }
    }

    fn record_from(&mut self, hand: &HandLandmarks) {
        let wrist = hand.landmarks[0];
        // Palm size — distance between the index MCP (5) and pinky MCP (17),
        // matching the convention used inside `gesture_classifier`.
        let palm = (hand.landmarks[5] - hand.landmarks[17]).length();
        if self.history.len() == MOTION_WINDOW {
            self.history.pop_front();
        }
        self.history.push_back((wrist.x, wrist.y, palm));
    }

    fn reset(&mut self) {
        self.history.clear();
    }

    // fn is_stationary(&self) -> bool {
    //     // Need a full window before we'll vouch for stationarity — that
    //     // way Grab requires sustained stillness rather than firing on the
    //     // very first frame the hand appears.
    //     if self.history.len() < MOTION_WINDOW {
    //         return false;
    //     }
    //     let mut min_palm = f32::INFINITY;
    //     let mut min_x = f32::INFINITY;
    //     let mut max_x = f32::NEG_INFINITY;
    //     let mut min_y = f32::INFINITY;
    //     let mut max_y = f32::NEG_INFINITY;
    //     for &(x, y, palm) in &self.history {
    //         min_palm = min_palm.min(palm);
    //         min_x = min_x.min(x);
    //         max_x = max_x.max(x);
    //         min_y = min_y.min(y);
    //         max_y = max_y.max(y);
    //     }
    //     if min_palm < f32::EPSILON {
    //         return false;
    //     }
    //     let dx = max_x - min_x;
    //     let dy = max_y - min_y;
    //     let spread = (dx * dx + dy * dy).sqrt();
    //     spread < STATIONARY_MAX_DRIFT_PALM_FRACTION * min_palm
    // }

    /// Report a horizontal swipe if the wrist has decisively moved left or
    /// right over the window. Uses **net** displacement (newest minus
    /// oldest sample) rather than range so a U-shaped back-and-forth path
    /// doesn't spuriously register — only a sustained one-way sweep does.
    ///
    /// In the inference frame's image-space coordinates, positive x = right,
    /// negative x = left. If the polarity reads reversed on a given device
    /// (depending on sensor orientation + the selfie-mirror un-do in
    /// `RobotScreen::submit_frame_for_inference`), swap the two branches.
    fn horizontal_swipe(&self) -> Option<GestureAction> {
        if self.history.len() < MOTION_WINDOW {
            return None;
        }
        let &(first_x, first_y, _) = self.history.front()?;
        let &(last_x, last_y, _) = self.history.back()?;
        let net_dx = last_x - first_x;
        let net_dy = last_y - first_y;
        // Use the smallest palm size observed in the window for
        // normalization — stricter threshold when the hand is far away
        // and harder to track precisely.
        let min_palm = self
            .history
            .iter()
            .map(|(_, _, p)| *p)
            .fold(f32::INFINITY, f32::min);
        if min_palm < f32::EPSILON {
            return None;
        }
        let abs_dx = net_dx.abs();
        let abs_dy = net_dy.abs();
        // Require both: (1) enough horizontal travel relative to the
        // hand's apparent size and (2) horizontal motion clearly dominating
        // vertical motion so a diagonal "lower-hand" gesture doesn't fire
        // a phantom Left or Right.
        if abs_dx < SWIPE_MIN_NET_DX_PALM_FRACTION * min_palm {
            return None;
        }
        if abs_dx < abs_dy * SWIPE_HORIZONTAL_DOMINANCE {
            return None;
        }
        Some(if net_dx > 0.0 {
            GestureAction::Right
        } else {
            GestureAction::Left
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::gesture_control::gesture_classifier::Vec2;

    /// Build a fake `HandLandmarks` with a configurable wrist position and
    /// fixed palm-size landmark separation. Only wrist + the two palm-MCP
    /// landmarks are populated meaningfully; the rest are zeroed (they
    /// don't matter for the stationarity check).
    fn hand_at(wrist: Vec2) -> HandLandmarks {
        let mut lm = [Vec2::default(); 21];
        lm[0] = wrist;
        lm[5] = Vec2::new(wrist.x + 0.10, wrist.y); // index MCP
        lm[17] = Vec2::new(wrist.x - 0.10, wrist.y); // pinky MCP — palm_size = 0.20
        HandLandmarks {
            landmarks: lm,
            confidence: 1.0,
            handedness: 0.5,
        }
    }

    #[test]
    fn fewer_than_window_samples_is_not_stationary() {
        let mut t = MotionTracker::new();
        for _ in 0..MOTION_WINDOW - 1 {
            t.record_from(&hand_at(Vec2::new(0.5, 0.5)));
        }
        assert!(!t.is_stationary(), "needs full window before vouching for stationarity");
    }

    #[test]
    fn perfectly_still_hand_is_stationary() {
        let mut t = MotionTracker::new();
        for _ in 0..MOTION_WINDOW {
            t.record_from(&hand_at(Vec2::new(0.5, 0.5)));
        }
        assert!(t.is_stationary());
    }

    #[test]
    fn drifting_hand_is_not_stationary() {
        let mut t = MotionTracker::new();
        // Palm size = 0.20 (see hand_at); threshold = 0.20 * 0.20 = 0.04.
        // Drift the wrist by 0.10 (well past threshold) across the window.
        for i in 0..MOTION_WINDOW {
            let x = 0.5 + (i as f32) * 0.025;
            t.record_from(&hand_at(Vec2::new(x, 0.5)));
        }
        assert!(!t.is_stationary());
    }

    #[test]
    fn reset_clears_history() {
        let mut t = MotionTracker::new();
        for _ in 0..MOTION_WINDOW {
            t.record_from(&hand_at(Vec2::new(0.5, 0.5)));
        }
        assert!(t.is_stationary());
        t.reset();
        assert!(!t.is_stationary());
    }

    #[test]
    fn rightward_swipe_yields_right() {
        // Palm size = 0.20; swipe threshold = 0.20 * 0.80 = 0.16.
        // Move the wrist from 0.30 → 0.50 across the window — net dx
        // = +0.20, which clears the threshold.
        let mut t = MotionTracker::new();
        for i in 0..MOTION_WINDOW {
            let frac = i as f32 / (MOTION_WINDOW - 1) as f32;
            let x = 0.30 + 0.20 * frac;
            t.record_from(&hand_at(Vec2::new(x, 0.5)));
        }
        assert_eq!(t.horizontal_swipe(), Some(GestureAction::Right));
    }

    #[test]
    fn leftward_swipe_yields_left() {
        let mut t = MotionTracker::new();
        for i in 0..MOTION_WINDOW {
            let frac = i as f32 / (MOTION_WINDOW - 1) as f32;
            let x = 0.50 - 0.20 * frac;
            t.record_from(&hand_at(Vec2::new(x, 0.5)));
        }
        assert_eq!(t.horizontal_swipe(), Some(GestureAction::Left));
    }

    #[test]
    fn stationary_hand_does_not_swipe() {
        let mut t = MotionTracker::new();
        for _ in 0..MOTION_WINDOW {
            t.record_from(&hand_at(Vec2::new(0.5, 0.5)));
        }
        assert_eq!(t.horizontal_swipe(), None);
    }

    #[test]
    fn small_drift_does_not_swipe() {
        // Net dx = +0.05, well below the 0.16 swipe threshold — this is
        // incidental drift, not a deliberate swipe.
        let mut t = MotionTracker::new();
        for i in 0..MOTION_WINDOW {
            let frac = i as f32 / (MOTION_WINDOW - 1) as f32;
            let x = 0.50 + 0.05 * frac;
            t.record_from(&hand_at(Vec2::new(x, 0.5)));
        }
        assert_eq!(t.horizontal_swipe(), None);
    }

    #[test]
    fn diagonal_swipe_is_not_left_right() {
        // Equal horizontal and vertical net motion — horizontal does not
        // dominate by SWIPE_HORIZONTAL_DOMINANCE, so no swipe fires.
        let mut t = MotionTracker::new();
        for i in 0..MOTION_WINDOW {
            let frac = i as f32 / (MOTION_WINDOW - 1) as f32;
            let x = 0.30 + 0.20 * frac;
            let y = 0.30 + 0.20 * frac;
            t.record_from(&hand_at(Vec2::new(x, y)));
        }
        assert_eq!(t.horizontal_swipe(), None);
    }

    #[test]
    fn u_shaped_path_does_not_swipe() {
        // Wrist moves out then back — net displacement ≈ 0 even though
        // max-minus-min would be large. Using net displacement protects
        // against this false positive.
        let mut t = MotionTracker::new();
        for i in 0..MOTION_WINDOW {
            // Sample positions: 0.50, 0.40, 0.30, 0.40, 0.50 (down then up).
            let x = if i <= MOTION_WINDOW / 2 {
                0.50 - 0.10 * (i as f32)
            } else {
                0.50 - 0.10 * ((MOTION_WINDOW - 1 - i) as f32)
            };
            t.record_from(&hand_at(Vec2::new(x, 0.5)));
        }
        assert_eq!(t.horizontal_swipe(), None);
    }
}
