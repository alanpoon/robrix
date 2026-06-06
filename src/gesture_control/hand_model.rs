//! Single-stage MediaPipe hand-landmark inference via `tract`.
//!
//! Loads the OpenCV Zoo handpose ONNX file and runs 21-landmark inference on a
//! center-square crop of the source frame.
//!
//! Model I/O (verified by inspecting the file):
//! - input  `input_1`     NHWC f32 `[1, 224, 224, 3]`, pixel values in `[0, 1]`
//! - output `Identity`    f32 `[1, 63]`  — 21 landmarks × (x, y, z) in 224-px space
//! - output `Identity_1`  f32 `[1, 1]`   — hand presence score, **sigmoid applied**
//! - output `Identity_2`  f32 `[1, 1]`   — handedness score (unused)
//! - output `Identity_3`  f32 `[1, 63]`  — 3D world landmarks (unused)
//!
//! Coordinate convention exposed to the rest of the module: x and y in `[0, 1]`
//! relative to the cropped square — matches `gesture_classifier::Vec2` contract.

use anyhow::{Context, Result};
use tract_onnx::prelude::*;

use crate::gesture_control::gesture_classifier::Vec2;

const INPUT_SIZE: usize = 224;

/// One inference result: 21 hand landmarks plus the model's overall confidence
/// for the hand-vs-no-hand head.
#[derive(Clone, Debug)]
pub struct HandLandmarks {
    pub landmarks: [Vec2; 21],
    pub confidence: f32,
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
    /// Center-square crops the frame, resizes to 224×224 with nearest-neighbour,
    /// normalizes to `[0, 1]` float32, runs the model, and returns 21 landmarks
    /// in `[0, 1]` cropped-square coordinates plus the hand-presence score.
    ///
    /// Returns `None` if the frame is too small to crop a square out of.
    pub fn run(&self, rgba: &[u8], width: u32, height: u32) -> Result<Option<HandLandmarks>> {
        if width < 4 || height < 4 {
            return Ok(None);
        }
        let side = width.min(height) as usize;
        let off_x = ((width as usize) - side) / 2;
        let off_y = ((height as usize) - side) / 2;
        let row_stride = (width as usize) * 4;

        // NHWC f32 buffer: [1, 224, 224, 3].
        let mut data = vec![0f32; INPUT_SIZE * INPUT_SIZE * 3];
        for ty in 0..INPUT_SIZE {
            let sy = off_y + (ty * side / INPUT_SIZE);
            let row_off = sy * row_stride;
            let dst_row = ty * INPUT_SIZE * 3;
            for tx in 0..INPUT_SIZE {
                let sx = off_x + (tx * side / INPUT_SIZE);
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

        let confidence = score_t.as_slice().map(|s| s[0]).unwrap_or(0.0);

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

        Ok(Some(HandLandmarks { landmarks, confidence }))
    }
}
