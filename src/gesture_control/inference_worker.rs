//! Background inference thread that owns a `HandModel` and processes frames
//! at ~10 Hz from a bounded channel.
//!
//! Structural stub for layer 1.

#![allow(dead_code)]

use crate::gesture_control::{
    GestureAction,
    hand_model::HandLandmarks,
};

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
