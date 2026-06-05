//! `tract`-based two-stage hand detection: palm bounding box → 21 landmarks.
//!
//! Structural stub for layer 1 — concrete tensor shape wiring is delivered in
//! a later layer of the same spec, once the canonical ONNX model files are
//! committed to the spec's `MODEL_BASE_URL` location and their I/O signatures
//! are known. Until then this struct exists so the rest of the module tree
//! compiles.

#![allow(dead_code)]

use crate::gesture_control::gesture_classifier::Vec2;

/// One inference result: 21 hand landmarks plus the model's overall confidence
/// for the hand-vs-no-hand head.
#[derive(Clone, Debug)]
pub struct HandLandmarks {
    pub landmarks: [Vec2; 21],
    pub confidence: f32,
}

/// Loaded ONNX models, ready to be invoked on a 256×256 RGB frame.
pub struct HandModel {
    // tract::SimplePlan<F32, _, _> for each stage, stored after model load.
    // Stub for now.
    _private: (),
}

impl HandModel {
    /// Load both ONNX files from the standard app_data_dir location.
    pub fn load() -> anyhow::Result<Self> {
        anyhow::bail!("HandModel::load not yet implemented — fill in once ONNX files are pinned")
    }

    /// Run inference on a 256×256 RGB frame (3 channels, row-major, 0-255 u8).
    pub fn run(&self, _frame_rgb_256: &[u8]) -> anyhow::Result<Option<HandLandmarks>> {
        anyhow::bail!("HandModel::run not yet implemented")
    }
}
