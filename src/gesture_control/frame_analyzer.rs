//! Skin-color centroid heuristic for the on-camera gesture inference button.
//!
//! This is *not* hand-landmark ML — it's a pure-CPU per-pixel analysis that:
//!
//! 1. Scans the RGBA frame with a stride to keep it cheap.
//! 2. Marks each sampled pixel as "skin" iff its YCbCr falls inside a
//!    conservative skin-tone box (Cb 77..127, Cr 133..173, Y 40..235).
//! 3. Builds the centroid + axis-aligned bounding box of the skin blob.
//! 4. Returns a `GestureAction` based on blob shape and centroid position:
//!
//!    - very wide + large blob → `Drop` (open palm)
//!    - small + roughly square blob → `Catch` (closed fist)
//!    - tall blob, centroid above frame center → `Forward` (finger up)
//!    - tall blob, centroid below frame center → `Back` (finger down)
//!    - centroid clearly left/right of center (after mirror flip) → `Left`/`Right`
//!    - otherwise → `None` (ambiguous)
//!
//! The heuristic is intentionally simple. It produces real per-frame output
//! based on actual hand position, but it does NOT understand finger
//! configurations. It exists as a stepping stone until a real hand-landmark
//! ONNX model is wired in — at which point this module is replaced by
//! `hand_model::run` + `gesture_classifier::classify`.

use crate::gesture_control::GestureAction;
use crate::shared::webrtc_video::WebRtcVideoFrame;

/// Sub-sample step (in pixels). Larger = faster + less accurate.
const SCAN_STEP: usize = 4;
/// Minimum number of skin-classified pixels to consider the result valid.
const MIN_SKIN_PIXELS: i64 = 80;

/// Run skin-color blob analysis on the frame and return a gesture, or `None`
/// if no usable blob is detected.
pub fn classify_frame(frame: &WebRtcVideoFrame) -> Option<GestureAction> {
    if frame.width == 0 || frame.height == 0 {
        return None;
    }
    let stride = (frame.width as usize) * 4;
    let w = frame.width as i32;
    let h = frame.height as i32;

    let mut sum_x: i64 = 0;
    let mut sum_y: i64 = 0;
    let mut count: i64 = 0;
    let mut min_x = w;
    let mut min_y = h;
    let mut max_x = 0;
    let mut max_y = 0;

    let mut y = 0i32;
    while y < h {
        let row_start = (y as usize) * stride;
        let mut x = 0i32;
        while x < w {
            let i = row_start + (x as usize) * 4;
            // Bounds-check guard against malformed buffers.
            if i + 2 >= frame.data.len() {
                break;
            }
            let r = frame.data[i];
            let g = frame.data[i + 1];
            let b = frame.data[i + 2];
            if is_skin(r, g, b) {
                sum_x += x as i64;
                sum_y += y as i64;
                count += 1;
                if x < min_x { min_x = x; }
                if y < min_y { min_y = y; }
                if x > max_x { max_x = x; }
                if y > max_y { max_y = y; }
            }
            x += SCAN_STEP as i32;
        }
        y += SCAN_STEP as i32;
    }

    if count < MIN_SKIN_PIXELS {
        return None;
    }

    let cx = (sum_x / count) as i32;
    let cy = (sum_y / count) as i32;
    let bw = (max_x - min_x).max(1);
    let bh = (max_y - min_y).max(1);
    let aspect = bw as f32 / bh as f32; // wide > 1, tall < 1
    let area_ratio = (bw * bh) as f32 / (w * h) as f32;

    // Big, wide blob ≈ open palm with fingers spread.
    if area_ratio > 0.25 && aspect > 1.3 {
        return Some(GestureAction::Drop);
    }
    // Small, roughly square blob ≈ closed fist.
    if area_ratio < 0.10 && aspect > 0.7 && aspect < 1.4 {
        return Some(GestureAction::Catch);
    }

    let dx = cx - w / 2;
    let dy = cy - h / 2;
    // Camera is mirrored (selfie view): user's right hand appears on left of
    // the image. Flip x so "user pointing right" classifies as `Right`.
    let dx_user = -dx;

    // Require one axis to clearly dominate the other (×2) to avoid diagonal
    // jitter — same idea as in `gesture_classifier::classify`.
    if dy.abs() > dx.abs() * 2 {
        return Some(if dy < 0 { GestureAction::Forward } else { GestureAction::Back });
    }
    if dx.abs() > dy.abs() * 2 {
        return Some(if dx_user < 0 { GestureAction::Left } else { GestureAction::Right });
    }
    None
}

/// Conservative YCbCr-based skin-tone test. Integer math, no allocations.
fn is_skin(r: u8, g: u8, b: u8) -> bool {
    let rf = r as i32;
    let gf = g as i32;
    let bf = b as i32;
    let y = (77 * rf + 150 * gf + 29 * bf) >> 8;
    let cb = 128 + ((-43 * rf - 85 * gf + 128 * bf) >> 8);
    let cr = 128 + ((128 * rf - 107 * gf - 21 * bf) >> 8);
    y > 40 && y < 235 && cb > 77 && cb < 127 && cr > 133 && cr < 173
}

#[cfg(test)]
mod tests {
    use super::*;

    fn solid_frame(width: u32, height: u32, r: u8, g: u8, b: u8) -> WebRtcVideoFrame {
        let mut data = Vec::with_capacity((width * height * 4) as usize);
        for _ in 0..(width * height) {
            data.push(r);
            data.push(g);
            data.push(b);
            data.push(255);
        }
        WebRtcVideoFrame { data, width, height, participant_id: None }
    }

    #[test]
    fn empty_frame_returns_none() {
        let frame = WebRtcVideoFrame {
            data: vec![],
            width: 0,
            height: 0,
            participant_id: None,
        };
        assert_eq!(classify_frame(&frame), None);
    }

    #[test]
    fn solid_black_returns_none() {
        let frame = solid_frame(64, 64, 0, 0, 0);
        assert_eq!(classify_frame(&frame), None);
    }

    #[test]
    fn skin_test_accepts_typical_skin_pixel() {
        // RGB ≈ 200,150,120 is a common pale-skin tone.
        assert!(is_skin(200, 150, 120));
    }

    #[test]
    fn skin_test_rejects_pure_red() {
        assert!(!is_skin(255, 0, 0));
    }

    #[test]
    fn skin_test_rejects_pure_blue() {
        assert!(!is_skin(0, 0, 255));
    }
}
