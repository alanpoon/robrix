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

/// Classify a 21-landmark hand configuration into a discrete [`GestureAction`].
///
/// Returns `None` when the configuration is ambiguous (e.g. diagonal pointing,
/// partial open hand) or when the palm size is degenerate.
///
/// Landmark layout (MediaPipe canonical):
/// - `0`       wrist
/// - `1‑4`     thumb  (tip = 4)
/// - `5‑8`     index  (tip = 8,  mcp = 5)
/// - `9‑12`    middle (tip = 12, mcp = 9)
/// - `13‑16`   ring   (tip = 16, mcp = 13)
/// - `17‑20`   pinky  (tip = 20, mcp = 17)
pub fn classify(lm: &[Vec2; 21]) -> Option<GestureAction> {
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
    // Closed fist: no fingers extended.
    if !idx_ext && !mid_ext && !ring_ext && !pinky_ext {
        return Some(GestureAction::Catch);
    }
    // Index-only pointing → direction from wrist to index tip.
    if idx_ext && !mid_ext && !ring_ext && !pinky_ext {
        let v = (lm[8] - wrist) / palm_size;
        // Camera is mirrored (selfie view): flip X so the user's right hand
        // pointing right reads as `Right` rather than `Left`.
        let dx = -v.x;
        let dy = v.y;

        if dy.abs() > dx.abs() * AXIS_DOMINANCE_RATIO {
            return Some(if dy < 0.0 { GestureAction::Forward } else { GestureAction::Back });
        }
        if dx.abs() > dy.abs() * AXIS_DOMINANCE_RATIO {
            return Some(if dx < 0.0 { GestureAction::Left } else { GestureAction::Right });
        }
        // Ambiguous diagonal — refuse to guess.
        return None;
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Build a landmark fixture by composing a wrist position with a per-finger
    /// configuration. `finger_extended[i]` means the i-th finger (index/middle/
    /// ring/pinky) is extended; `tip_direction` is added to the wrist for the
    /// index tip when index_only is true.
    fn fixture(
        finger_extended: [bool; 4],
        thumb_extended: bool,
        index_tip_direction: Vec2,
    ) -> [Vec2; 21] {
        let wrist = Vec2::new(0.5, 1.0);
        let palm_size = 0.3_f32;
        // MCP joints arrayed along a horizontal line across the palm.
        let mcps = [
            Vec2::new(0.50, 0.70), // 5  index
            Vec2::new(0.45, 0.70), // 9  middle
            Vec2::new(0.40, 0.70), // 13 ring
            Vec2::new(0.35, 0.70), // 17 pinky
        ];
        let mut lm = [Vec2::default(); 21];
        lm[0] = wrist;
        // Thumb (indices 1..=4). Thumb extended → tip far from IP joint.
        lm[1] = Vec2::new(0.60, 0.85);
        lm[2] = Vec2::new(0.65, 0.80);
        lm[3] = Vec2::new(0.70, 0.75);
        lm[4] = if thumb_extended {
            Vec2::new(0.70 + 0.6 * palm_size, 0.75) // far from lm[2]
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
        assert_eq!(classify(&lm), None);
    }

    #[test]
    fn closed_fist_yields_catch() {
        let lm = fixture([false, false, false, false], false, Vec2::new(0.0, -0.6));
        assert_eq!(classify(&lm), Some(GestureAction::Catch));
    }

    #[test]
    fn open_palm_yields_drop() {
        let lm = fixture([true, true, true, true], true, Vec2::new(0.0, -0.6));
        assert_eq!(classify(&lm), Some(GestureAction::Drop));
    }

    #[test]
    fn index_pointing_up_yields_forward() {
        // Negative y = up in image coordinates.
        let lm = fixture([true, false, false, false], false, Vec2::new(0.0, -0.6));
        assert_eq!(classify(&lm), Some(GestureAction::Forward));
    }

    #[test]
    fn index_pointing_down_yields_back() {
        let lm = fixture([true, false, false, false], false, Vec2::new(0.0, 0.6));
        assert_eq!(classify(&lm), Some(GestureAction::Back));
    }

    #[test]
    fn index_pointing_user_left_yields_left() {
        // In image coords the user's left hand pointing left moves the tip in
        // the +x direction (camera is mirrored), so we feed +x here and expect
        // `Left` after mirror correction.
        let lm = fixture([true, false, false, false], false, Vec2::new(0.6, 0.0));
        assert_eq!(classify(&lm), Some(GestureAction::Left));
    }

    #[test]
    fn index_pointing_user_right_yields_right() {
        let lm = fixture([true, false, false, false], false, Vec2::new(-0.6, 0.0));
        assert_eq!(classify(&lm), Some(GestureAction::Right));
    }

    #[test]
    fn diagonal_pointing_yields_none() {
        // Equal magnitudes on both axes → no axis dominates → ambiguous.
        let lm = fixture([true, false, false, false], false, Vec2::new(0.6, 0.6));
        assert_eq!(classify(&lm), None);
    }

    #[test]
    fn partial_open_hand_yields_none() {
        // Index + middle extended, ring + pinky curled — not a recognized shape.
        let lm = fixture([true, true, false, false], false, Vec2::new(0.0, -0.6));
        assert_eq!(classify(&lm), None);
    }

    #[test]
    fn wire_name_round_trip() {
        assert_eq!(GestureAction::Forward.wire_name(), Some("forward"));
        assert_eq!(GestureAction::Back.wire_name(), Some("back"));
        assert_eq!(GestureAction::Left.wire_name(), Some("left"));
        assert_eq!(GestureAction::Right.wire_name(), Some("right"));
        assert_eq!(GestureAction::Catch.wire_name(), Some("catch"));
        assert_eq!(GestureAction::Drop.wire_name(), Some("drop"));
        assert_eq!(GestureAction::None.wire_name(), None);
    }
}
