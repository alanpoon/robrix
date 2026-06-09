//! Hand-gesture robot-arm-car control.
//!
//! Captures the local webcam, runs a single-stage 21-point hand-landmark ONNX
//! model on a background thread via `tract` (center-square crop of the source
//! frame fed straight to the landmark model — palm-detection stage deferred),
//! classifies the landmarks into one of six discrete gestures, and emits
//! both an in-app [`GestureAction`] and an HTTP POST to a user-configured
//! robotic-arm-car endpoint on the local network.
//!
//! Target platforms: desktop (macOS / Linux / Windows) and Android. Web is
//! explicitly out of scope.
//!
//! See `specs/task-gesture-robot-control.spec.md` for the full contract.

pub mod gesture_classifier;
pub mod robot_http;
pub mod model_downloader;
pub mod hand_model;
pub mod inference_worker;
pub mod gesture_webcam_view;
pub mod robot_control_panel;
pub mod robot_screen;
pub mod camera_capture;
#[cfg(target_os = "macos")]
pub mod avf_capture;
#[cfg(target_os = "android")]
pub mod acamera_capture;
pub mod frame_analyzer;

/// The six discrete gesture actions the classifier can emit, plus `None`
/// for "no recognized gesture this frame".
///
/// These are emitted on Makepad's action bus by `RobotScreen` once per debounced
/// detection, and also drive the HTTP POST body (`{"action": "forward"}` etc.).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum GestureAction {
    /// No gesture detected this frame, or below confidence threshold.
    #[default]
    None,
    /// Thumbs up (thumb extended upward, other fingers curled).
    Forward,
    /// Thumbs down (thumb extended downward, other fingers curled).
    Back,
    /// Two-finger peace sign (index + middle extended) shown with the user's
    /// left hand, identified via the hand-landmark model's handedness output.
    Left,
    /// Two-finger peace sign (index + middle extended) shown with the user's
    /// right hand, identified via the hand-landmark model's handedness output.
    Right,
    /// Closed fist (no fingers extended).
    Catch,
    /// Open palm (all five fingers extended).
    Drop,
    /// Stop command — emitted when a movement button is released, never by
    /// the ML classifier. Not displayed in the on-video overlay or "Last
    /// gesture" readout; only logged via the HTTP recent-commands feed.
    Stop,
}

impl GestureAction {
    /// Lowercase wire name used as the `"action"` query parameter against
    /// `http://{ip}/api/control?action=…&speed=50`. Returns `None` for
    /// `GestureAction::None` — callers should not send a request when no
    /// gesture is detected.
    pub fn wire_name(self) -> Option<&'static str> {
        match self {
            GestureAction::None => None,
            GestureAction::Forward => Some("up"),
            GestureAction::Back => Some("down"),
            GestureAction::Left => Some("left"),
            GestureAction::Right => Some("right"),
            GestureAction::Catch => Some("grab"),
            GestureAction::Drop => Some("release"),
            GestureAction::Stop => Some("stop"),
        }
    }

    /// Short human-readable label for the UI (`Last gesture: …`).
    pub fn display_label(self) -> &'static str {
        match self {
            GestureAction::None => "—",
            GestureAction::Forward => "Forward",
            GestureAction::Back => "Back",
            GestureAction::Left => "Left",
            GestureAction::Right => "Right",
            GestureAction::Catch => "Grab",
            GestureAction::Drop => "Release",
            GestureAction::Stop => "Stop",
        }
    }

    /// Single-glyph icon used in the on-video overlay pill. Big-text-friendly.
    pub fn icon(self) -> &'static str {
        match self {
            GestureAction::None => "—",
            GestureAction::Forward => "▲",
            GestureAction::Back => "▼",
            GestureAction::Left => "◀",
            GestureAction::Right => "▶",
            GestureAction::Catch => "✊",
            GestureAction::Drop => "🖐",
            GestureAction::Stop => "⏹",
        }
    }
}

/// Register all `script_mod!` widgets that belong to this module.
///
/// Called from `App::register_widgets()` in `src/app.rs`.
pub fn script_mod(vm: &mut makepad_widgets::ScriptVm) {
    gesture_webcam_view::script_mod(vm);
    robot_control_panel::script_mod(vm);
    robot_screen::script_mod(vm);
}
