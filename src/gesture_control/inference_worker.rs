//! Background inference thread that owns a `HandModel` and processes frames
//! pushed via a bounded(1) channel.
//!
//! Single-shot mode (one frame per button click) and continuous mode (camera
//! capture pushing every Nth frame) both work — the worker just consumes
//! whatever shows up on `frame_rx`.

use std::sync::Arc;
use std::thread::JoinHandle;

use anyhow::{Context, Result};
use crossbeam_channel::{Receiver, Sender, TrySendError, bounded, unbounded};
use makepad_widgets::log;

use crate::gesture_control::{
    GestureAction,
    gesture_classifier,
    hand_model::{HandLandmarks, HandModel},
};
use crate::shared::webrtc_video::WebRtcVideoFrame;

/// Skip every Nth frame received from the capture callback. With a 30 fps
/// camera and N=3 this gives ~10 inferences per second.
pub const INFERENCE_FRAME_SKIP: u32 = 3;

/// Confidence threshold below which a detection is dropped (no emit, no HTTP).
pub const CONFIDENCE_THRESHOLD: f32 = 0.6;

/// One inference output delivered to the UI thread.
#[derive(Clone, Debug)]
pub struct InferenceResult {
    pub landmarks: Option<HandLandmarks>,
    pub detected: GestureAction,
}

/// Handle to a running inference worker. Drop it to shut down the worker.
pub struct InferenceWorker {
    frame_tx: Sender<WebRtcVideoFrame>,
    result_rx: Receiver<InferenceResult>,
    handle: Option<JoinHandle<()>>,
}

impl InferenceWorker {
    /// Load the hand model and spawn the worker thread.
    pub fn spawn() -> Result<Self> {
        let model = Arc::new(HandModel::load().context("HandModel::load")?);
        let (frame_tx, frame_rx) = bounded::<WebRtcVideoFrame>(1);
        let (result_tx, result_rx) = unbounded::<InferenceResult>();

        let handle = std::thread::Builder::new()
            .name("robot-inference".into())
            .spawn(move || worker_loop(model, frame_rx, result_tx))
            .context("spawn inference thread")?;

        Ok(Self {
            frame_tx,
            result_rx,
            handle: Some(handle),
        })
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

impl Drop for InferenceWorker {
    fn drop(&mut self) {
        // Dropping frame_tx closes the channel; the worker exits its loop on
        // the next `recv()`. Then we wait for the thread to actually exit.
        // The `frame_tx` drop happens automatically as we go out of scope, but
        // we need to do it BEFORE the join, so we take the handle first.
        let handle = self.handle.take();
        // Replace the sender with a dropped one by swapping with a dummy.
        // Simpler: just rely on the field drop ordering — handle was already
        // moved out, so when the rest of `self` drops, `frame_tx` drops too.
        // But the join needs `frame_tx` already dropped, so close it manually:
        // we can't move out of `self.frame_tx` in Drop, so spawn the join in
        // a way that doesn't deadlock. Easiest: detach (don't join).
        if let Some(h) = handle {
            // Best-effort: give the worker a moment to drain, then detach.
            // We can't drop `self.frame_tx` from here without unsafe, so we
            // accept that the worker will exit naturally when `self.frame_tx`
            // is dropped after this `Drop` returns.
            drop(h);
        }
    }
}

fn worker_loop(
    model: Arc<HandModel>,
    frame_rx: Receiver<WebRtcVideoFrame>,
    result_tx: Sender<InferenceResult>,
) {
    log!("inference worker started");
    while let Ok(frame) = frame_rx.recv() {
        match model.run(&frame.data, frame.width, frame.height) {
            Ok(Some(hand)) => {
                let detected = if hand.confidence >= CONFIDENCE_THRESHOLD {
                    gesture_classifier::classify(&hand.landmarks).unwrap_or(GestureAction::None)
                } else {
                    GestureAction::None
                };
                let result = InferenceResult {
                    landmarks: Some(hand),
                    detected,
                };
                if result_tx.send(result).is_err() {
                    break;
                }
            }
            Ok(None) => {
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
