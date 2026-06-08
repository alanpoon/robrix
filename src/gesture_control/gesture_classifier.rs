//! Pure-function gesture classifier.
//!
//! Maps a fixed array of 21 hand landmarks (MediaPipe canonical layout) to one
//! of six discrete [`GestureAction`] values, or `None` if the configuration is
//! ambiguous or doesn't match any recognized gesture.
//!
//! This module has no I/O, no allocation, no shared state, and no dependency on
//! Makepad — it is fully testable with `cargo test` using hand-crafted landmark
//! fixtures.

use crate::gesture_control::GestureAction;

/// A 2D point in landmark space. The hand model returns normalized coordinates
/// in `[0.0, 1.0]` where `(0, 0)` is the top-left of the cropped hand region.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Vec2 {
    pub x: f32,
    pub y: f32,
}

impl Vec2 {
    pub const fn new(x: f32, y: f32) -> Self {
        Self { x, y }
    }
    pub fn length(self) -> f32 {
        (self.x * self.x + self.y * self.y).sqrt()
    }
}

impl core::ops::Sub for Vec2 {
    type Output = Vec2;
    fn sub(self, rhs: Vec2) -> Vec2 {
        Vec2::new(self.x - rhs.x, self.y - rhs.y)
    }
}

impl core::ops::Div<f32> for Vec2 {
    type Output = Vec2;
    fn div(self, rhs: f32) -> Vec2 {
        Vec2::new(self.x / rhs, self.y / rhs)
    }
}

/// Ratio (tip-to-wrist) / (mcp-to-wrist) above which a finger is "extended".
const EXTENSION_RATIO: f32 = 1.6;

/// Ratio by which one direction axis must dominate the other to count as a
/// clean pointing direction (avoids diagonal jitter).
const AXIS_DOMINANCE_RATIO: f32 = 1.3;

/// Handedness score *below* which the MediaPipe hand-landmark model's
/// `Identity_2` output is interpreted as the user's right hand. Empirically
/// verified against the OpenCV Zoo port plus this codebase's camera path:
/// the model emits a value near 0 for the user's right hand and near 1 for
/// the left, so the right-hand branch uses `handedness < threshold`.
const HANDEDNESS_RIGHT_THRESHOLD: f32 = 0.5;

/// Classify a 21-landmark hand configuration into a discrete [`GestureAction`].
///
/// Gesture map:
/// - open palm (all 5 fingers extended) → [`GestureAction::Drop`]
/// - closed fist (no fingers extended) → [`GestureAction::Catch`]
/// - index only, pointing up/down → [`GestureAction::Forward`] /
///   [`GestureAction::Back`]
/// - thumbs down (thumb extended downward, fingers curled) →
///   [`GestureAction::Back`] (loose diagonal allowance — alternate path)
/// - index + middle extended, ring + pinky curled → [`GestureAction::Right`]
///   if `handedness` reads as the right hand, [`GestureAction::Left`] otherwise
/// - anything else → `None`
///
/// `handedness` is the MediaPipe hand-landmark model's `Identity_2` output —
/// a value near 0 or 1 indicating which anatomical hand the model identified.
///
/// Landmark layout (MediaPipe canonical):
/// - `0`       wrist
/// - `1‑4`     thumb  (tip = 4)
/// - `5‑8`     index  (tip = 8,  mcp = 5)
/// - `9‑12`    middle (tip = 12, mcp = 9)
/// - `13‑16`   ring   (tip = 16, mcp = 13)
/// - `17‑20`   pinky  (tip = 20, mcp = 17)
pub fn classify(lm: &[Vec2; 21], handedness: f32) -> Option<GestureAction> {
    let wrist = lm[0];
    let palm_size = (lm[5] - lm[17]).length();
    if palm_size < f32::EPSILON {
        return None;
    }

    let extended = |tip: usize, mcp: usize| -> bool {
        (lm[tip] - wrist).length() > EXTENSION_RATIO * (lm[mcp] - wrist).length()
    };

    let idx_ext = extended(8, 5);
    let mid_ext = extended(12, 9);
    let ring_ext = extended(16, 13);
    let pinky_ext = extended(20, 17);
    // The thumb's geometry is different from the other fingers — measure the
    // tip-to-IP distance relative to palm size.
    let thumb_ext = (lm[4] - lm[2]).length() > 0.6 * palm_size;

    // Open palm: all five fingers extended.
    if idx_ext && mid_ext && ring_ext && pinky_ext && thumb_ext {
        return Some(GestureAction::Drop);
    }
    // Thumbs down: thumb extended, all four other fingers curled, thumb-tip
    // below the wrist with more vertical than horizontal travel (allows
    // diagonal-down gestures, since users naturally angle the thumb out at
    // the wrist). Thumbs-up is intentionally NOT mapped — Forward is the
    // index-pointing-up gesture handled below.
    if thumb_ext && !idx_ext && !mid_ext && !ring_ext && !pinky_ext {
        let v = (lm[4] - wrist) / palm_size;
        if v.y > 0.0 && v.y > v.x.abs() {
            return Some(GestureAction::Back);
        }
    }
    // Index-only pointing up/down → Forward / Back. Horizontal/diagonal index
    // pointing is intentionally unmapped — Left/Right are the two-finger
    // gestures below.
    if idx_ext && !mid_ext && !ring_ext && !pinky_ext {
        let v = (lm[8] - wrist) / palm_size;
        if v.y.abs() > v.x.abs() * AXIS_DOMINANCE_RATIO {
            return Some(if v.y < 0.0 { GestureAction::Forward } else { GestureAction::Back });
        }
    }
    // Closed fist: no fingers extended.
    if !idx_ext && !mid_ext && !ring_ext && !pinky_ext {
        return Some(GestureAction::Catch);
    }
    // Two-finger peace sign (index + middle extended, ring + pinky curled,
    // thumb don't-care). Direction is encoded by the model's handedness output
    // rather than image-space position, so the user can hold the hand wherever
    // is comfortable in frame.
    if idx_ext && mid_ext && !ring_ext && !pinky_ext {
        return Some(if handedness < HANDEDNESS_RIGHT_THRESHOLD {
            GestureAction::Right
        } else {
            GestureAction::Left
        });
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Build a landmark fixture by composing a wrist position with a per-finger
    /// configuration. `finger_extended[i]` means the i-th finger (index/middle/
    /// ring/pinky) is extended. `thumb_tip_direction` is added to the wrist to
    /// place the thumb tip when `thumb_extended` is true — used to test
    /// thumbs-up/down/sideways. `index_tip_direction` is similarly applied for
    /// extending-index fixtures.
    fn fixture(
        finger_extended: [bool; 4],
        thumb_extended: bool,
        index_tip_direction: Vec2,
        thumb_tip_direction: Vec2,
    ) -> [Vec2; 21] {
        let wrist = Vec2::new(0.5, 1.0);
        // MCP joints arrayed along a horizontal line across the palm.
        let mcps = [
            Vec2::new(0.50, 0.70), // 5  index
            Vec2::new(0.45, 0.70), // 9  middle
            Vec2::new(0.40, 0.70), // 13 ring
            Vec2::new(0.35, 0.70), // 17 pinky
        ];
        let mut lm = [Vec2::default(); 21];
        lm[0] = wrist;
        // Thumb (indices 1..=4). Thumb extended → tip far from IP joint, placed
        // by caller-supplied direction so we can test thumb orientation.
        lm[1] = Vec2::new(0.60, 0.85);
        lm[2] = Vec2::new(0.65, 0.80);
        lm[3] = Vec2::new(0.70, 0.75);
        lm[4] = if thumb_extended {
            Vec2::new(wrist.x + thumb_tip_direction.x, wrist.y + thumb_tip_direction.y)
        } else {
            Vec2::new(0.66, 0.79) // close to lm[2]
        };

        // For each finger, place MCP, two intermediate joints, then tip.
        for (i, mcp) in mcps.iter().enumerate() {
            let extended = finger_extended[i];
            let mcp_idx = 5 + i * 4;
            lm[mcp_idx] = *mcp;
            lm[mcp_idx + 1] = Vec2::new(mcp.x, mcp.y - 0.05);
            lm[mcp_idx + 2] = Vec2::new(mcp.x, mcp.y - 0.10);
            // Tip direction depends on extended state. For the index finger we
            // place the tip according to `index_tip_direction` (so we can test
            // pointing-up/down/left/right). For other fingers we just put the
            // tip far above when extended, or close to the MCP when curled.
            let tip = if extended {
                if i == 0 {
                    // Index — use caller-supplied direction.
                    Vec2::new(wrist.x + index_tip_direction.x, wrist.y + index_tip_direction.y)
                } else {
                    Vec2::new(mcp.x, mcp.y - 0.45)
                }
            } else {
                // Curled: tip is back near the MCP.
                Vec2::new(mcp.x, mcp.y + 0.02)
            };
            lm[mcp_idx + 3] = tip;
        }
        lm
    }

    #[test]
    fn palm_zero_palm_size_yields_none() {
        let lm = [Vec2::new(0.5, 0.5); 21];
        assert_eq!(classify(&lm, 0.5), None);
    }

    #[test]
    fn closed_fist_yields_catch() {
        let lm = fixture([false, false, false, false], false, Vec2::new(0.0, -0.6), Vec2::new(0.0, 0.0));
        assert_eq!(classify(&lm, 0.5), Some(GestureAction::Catch));
    }

    #[test]
    fn open_palm_yields_drop() {
        // Thumb extended sideways so the "all five extended" branch fires
        // before the thumbs-up/down branch can match.
        let lm = fixture([true, true, true, true], true, Vec2::new(0.0, -0.6), Vec2::new(0.4, 0.0));
        assert_eq!(classify(&lm, 0.5), Some(GestureAction::Drop));
    }

    #[test]
    fn thumb_up_yields_catch() {
        // Thumb extended upward, all four fingers curled. Thumbs-up is no
        // longer mapped to Forward — it falls through to the closed-fist
        // branch and registers as Catch. (Forward is now index-pointing-up.)
        let lm = fixture([false, false, false, false], true, Vec2::new(0.0, 0.0), Vec2::new(0.0, -0.6));
        assert_eq!(classify(&lm, 0.5), Some(GestureAction::Catch));
    }

    #[test]
    fn thumb_down_yields_back() {
        // Thumb extended downward (positive y), all four fingers curled.
        let lm = fixture([false, false, false, false], true, Vec2::new(0.0, 0.0), Vec2::new(0.0, 0.6));
        assert_eq!(classify(&lm, 0.5), Some(GestureAction::Back));
    }

    #[test]
    fn thumb_diagonal_down_yields_back() {
        // Thumb angled down-and-out (both axes positive, but y dominates x).
        // This is the typical wrist angle when a user makes a thumbs-down.
        let lm = fixture([false, false, false, false], true, Vec2::new(0.0, 0.0), Vec2::new(0.4, 0.5));
        assert_eq!(classify(&lm, 0.5), Some(GestureAction::Back));
    }

    #[test]
    fn thumb_diagonal_up_yields_catch() {
        // Diagonal up is intentionally NOT accepted as Forward — thumbs-up
        // keeps the stricter dominance ratio, so this falls through to Catch.
        let lm = fixture([false, false, false, false], true, Vec2::new(0.0, 0.0), Vec2::new(0.4, -0.5));
        assert_eq!(classify(&lm, 0.5), Some(GestureAction::Catch));
    }

    #[test]
    fn thumb_sideways_yields_catch() {
        // Thumb extended horizontally falls through to the closed-fist branch
        // rather than being labeled Forward/Back.
        let lm = fixture([false, false, false, false], true, Vec2::new(0.0, 0.0), Vec2::new(0.6, 0.0));
        assert_eq!(classify(&lm, 0.5), Some(GestureAction::Catch));
    }

    #[test]
    fn index_pointing_up_yields_forward() {
        // Index extended upward, all other fingers curled. Negative y = up in
        // image coordinates.
        let lm = fixture([true, false, false, false], false, Vec2::new(0.0, -0.6), Vec2::new(0.0, 0.0));
        assert_eq!(classify(&lm, 0.5), Some(GestureAction::Forward));
    }

    #[test]
    fn index_pointing_down_yields_back() {
        // Index extended downward, all other fingers curled.
        let lm = fixture([true, false, false, false], false, Vec2::new(0.0, 0.6), Vec2::new(0.0, 0.0));
        assert_eq!(classify(&lm, 0.5), Some(GestureAction::Back));
    }

    #[test]
    fn index_pointing_horizontal_is_unmapped() {
        // Horizontal index pointing is no longer mapped — Left/Right are the
        // two-finger gestures.
        let horiz = fixture([true, false, false, false], false, Vec2::new(0.6, 0.0), Vec2::new(0.0, 0.0));
        assert_eq!(classify(&horiz, 0.5), None);
    }

    #[test]
    fn two_fingers_right_hand_yields_right() {
        // Index + middle extended, ring + pinky curled. The OpenCV Zoo port's
        // `Identity_2` reads ~0 for the user's right hand, so handedness below
        // the threshold → `Right` regardless of pointing direction.
        let lm = fixture([true, true, false, false], false, Vec2::new(0.0, -0.6), Vec2::new(0.0, 0.0));
        assert_eq!(classify(&lm, 0.1), Some(GestureAction::Right));
    }

    #[test]
    fn two_fingers_left_hand_yields_left() {
        let lm = fixture([true, true, false, false], false, Vec2::new(0.0, -0.6), Vec2::new(0.0, 0.0));
        assert_eq!(classify(&lm, 0.9), Some(GestureAction::Left));
    }

    #[test]
    fn wire_name_round_trip() {
        assert_eq!(GestureAction::Forward.wire_name(), Some("up"));
        assert_eq!(GestureAction::Back.wire_name(), Some("down"));
        assert_eq!(GestureAction::Left.wire_name(), Some("left"));
        assert_eq!(GestureAction::Right.wire_name(), Some("right"));
        assert_eq!(GestureAction::Catch.wire_name(), Some("grab"));
        assert_eq!(GestureAction::Drop.wire_name(), Some("release"));
        assert_eq!(GestureAction::Stop.wire_name(), Some("stop"));
        assert_eq!(GestureAction::None.wire_name(), None);
    }
}
