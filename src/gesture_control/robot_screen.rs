//! Top-level Robot tab screen.
//!
//! This is the user-visible surface for the gesture-control feature:
//! - IP textbox (left side) — driven by `AppPreferences::robot_control_ip`
//! - Status indicator (grey/yellow/green/red dot)
//! - Last-gesture readout
//! - Continuous inference: while the camera is open, every fresh frame is
//!   pushed to the background `InferenceWorker`; recognized gestures are
//!   emitted automatically.
//! - Manual-control buttons (D-pad cross + Catch/Release) for firing each
//!   `GestureAction` by hand, bypassing the ML pipeline.
//! - Recent commands log (rolling, ~8 lines)
//! - Camera preview area — Makepad `Video` widget in Native preview mode.

use std::time::Instant;

use makepad_widgets::*;
use makepad_widgets::video::VideoCameraPreviewMode;

use crossbeam_channel::Receiver;

use crate::gesture_control::{
    GestureAction,
    inference_worker::InferenceWorker,
    model_downloader,
    robot_http::{HttpOutcome, HttpResult, RobotHttpSender, validate_ip},
};

// Platform-conditional inference frame source. On macOS we open a parallel
// `AVCaptureSession` (BGRA-aware) because Makepad's `camera_frame_input`
// dispatcher drops BGRA. Everywhere else we ride Makepad's path.
#[cfg(target_os = "macos")]
type InferenceCapture = crate::gesture_control::avf_capture::AvfCapture;
#[cfg(not(target_os = "macos"))]
type InferenceCapture = crate::gesture_control::camera_capture::CameraCapture;
use crate::settings::app_preferences::AppPreferencesGlobal;
use crate::shared::webrtc_video::WebRtcVideoFrame;
use crate::voip::{CameraConsumer, VoipGlobalState};

script_mod! {
    use mod.prelude.widgets.*
    use mod.widgets.*

    mod.widgets.RobotScreen = #(RobotScreen::register_widget(vm)) {
        width: Fill, height: Fill
        // Stack vertically (preview on top, control panel below) so the layout
        // works on narrow mobile screens. Side-by-side flow:Right with the
        // fixed 300dp panel degenerates the preview to a sliver on phones
        // (~24dp wide after padding/spacing) and stalls Makepad's shadow
        // shader during the StackNavigation push animation.
        flow: Down
        show_bg: true
        draw_bg.color: (COLOR_PRIMARY)
        padding: Inset{top: 12, bottom: 12, left: 12, right: 12}
        spacing: 12

        // Single control panel wrapped in a vertical scroll view so the
        // contents (IP entry, status, gestures, D-pad, recent log) can
        // overflow the viewport on phones without clipping. The panel
        // itself is `height: Fill` so it claims the full RobotScreen body;
        // its children stack with their natural heights and become
        // scrollable when their sum exceeds Fill.
        panel_col := ScrollYView {
            width: Fill, height: Fill
            flow: Down
            spacing: 10

            panel_title := Label {
                text: "Robot"
                draw_text +: {
                    color: #x202020
                    text_style: theme.font_bold { font_size: 18.0 }
                }
            }

            // Top row — Robot IP entry on the left, live webcam thumbnail on
            // the right. `top_left_col` is `width: Fill` so it consumes
            // whatever horizontal space the fixed-width webcam leaves over.
            // The IP input itself is fixed at 200dp so it stays "short"
            // even if the column gets wider, which matches the desktop
            // layout intent.
            top_row := View {
                width: Fill, height: Fit
                flow: Right
                align: Align{y: 0.0}
                spacing: 12

                top_left_col := View {
                    width: Fill, height: Fit
                    flow: Down
                    spacing: 6

                    ip_label := Label {
                        text: "Robot IP"
                        draw_text +: {
                            color: #x404040
                            text_style: theme.font_regular { font_size: 12.0 }
                        }
                    }
                    robot_ip_input := RobrixTextInput {
                        width: 200, height: Fit
                        empty_text: "192.168.4.1"
                        padding: Inset{top: 6, bottom: 6, left: 10, right: 10}
                    }
                }

                // Live webcam thumbnail pinned to the top-right of the
                // panel. Always visible (background = dark gray) so the
                // user sees the slot before they tap Open camera; the
                // `preview_video` inside is the one that gets toggled on/off
                // by start_camera_preview / stop_camera_preview.
                // Children stack via flow:Overlay so the gesture pill
                // floats on top of the live frames.
                webcam_view := RoundedView {
                    width: 160, height: 120
                    flow: Overlay
                    show_bg: true
                    draw_bg +: { color: #x1a1a1a, border_radius: 8.0 }

                    preview_video := Video {
                        width: Fill, height: Fill
                        visible: false
                    }

                    gesture_overlay_layer := View {
                        width: Fill, height: Fill
                        align: Align{x: 0.5, y: 0.5}

                        gesture_overlay_pill := RoundedView {
                            visible: false
                            width: Fit, height: Fit
                            flow: Down
                            align: Align{x: 0.5, y: 0.5}
                            show_bg: true
                            draw_bg +: { color: #x000000DD, border_radius: 14.0 }
                            padding: Inset{top: 8, bottom: 8, left: 14, right: 14}
                            spacing: 2

                            gesture_overlay_icon := Label {
                                text: "—"
                                align: Align{x: 0.5, y: 0.5}
                                draw_text +: {
                                    color: #xFFFFFF
                                    text_style: theme.font_bold { font_size: 32.0 }
                                }
                            }
                            gesture_overlay_label := Label {
                                text: ""
                                align: Align{x: 0.5, y: 0.5}
                                draw_text +: {
                                    color: #xFFFFFF
                                    text_style: theme.font_bold { font_size: 12.0 }
                                }
                            }
                        }
                    }
                }
            }

            // Status row: colored dot + text label
            status_row := View {
                width: Fill, height: Fit, flow: Right
                align: Align{y: 0.5}, spacing: 8

                status_dot := RoundedView {
                    width: 14, height: 14
                    show_bg: true
                    draw_bg +: { color: #x888888, border_radius: 7.0 }
                }
                status_label := Label {
                    text: "Not connected"
                    draw_text +: {
                        color: #x606060
                        text_style: theme.font_regular { font_size: 12.0 }
                    }
                }
            }

            spacer1 := View { width: Fill, height: 8 }

            last_gesture_caption := Label {
                text: "Last gesture"
                draw_text +: {
                    color: #x404040
                    text_style: theme.font_regular { font_size: 12.0 }
                }
            }
            last_gesture_value := Label {
                text: "—"
                draw_text +: {
                    color: #x202020
                    text_style: theme.font_bold { font_size: 18.0 }
                }
            }

            spacer2 := View { width: Fill, height: 8 }

            // Camera toggle. Inference runs continuously while the camera is
            // open — no manual "Infer" button.
            btn_camera := Button {
                text: "Open camera"
                width: Fill, height: 36
                draw_text +: { color: #x000000 }
            }

            inference_caption := Label {
                text: "Inference"
                draw_text +: {
                    color: #x404040
                    text_style: theme.font_regular { font_size: 12.0 }
                }
            }
            inference_value := Label {
                text: "—"
                draw_text +: {
                    color: #x1B5E20
                    text_style: theme.font_bold { font_size: 18.0 }
                }
            }

            spacer_cam := View { width: Fill, height: 8 }

            // Manual control buttons — fire each gesture without the camera.
            // Directional buttons are arrayed in a cross (D-pad) layout; the
            // catch/release pair sits below.
            test_label := Label {
                text: "Manual control"
                draw_text +: {
                    color: #x404040
                    text_style: theme.font_regular { font_size: 12.0 }
                }
            }
            cross_pad := View {
                width: Fill, height: Fit, flow: Down, spacing: 6
                align: Align{x: 0.5}

                cross_row_up := View {
                    width: Fit, height: Fit, flow: Right
                    btn_forward := Button {
                        text: "▲"
                        width: 56, height: 36
                        draw_bg +: { color: #xCCCCCC }
                        draw_text +: { color: #x000000 }
                    }
                }
                cross_row_mid := View {
                    width: Fit, height: Fit, flow: Right, spacing: 6
                    align: Align{y: 0.5}
                    btn_left := Button {
                        text: "◀"
                        width: 56, height: 36
                        draw_bg +: { color: #xCCCCCC }
                        draw_text +: { color: #x000000 }
                    }
                    btn_stop := Button {
                        text: "⏹"
                        width: 56, height: 36
                        draw_bg +: { color: #xCCCCCC }
                        draw_text +: { color: #x000000 }
                    }
                    btn_right := Button {
                        text: "▶"
                        width: 56, height: 36
                        draw_bg +: { color: #xCCCCCC }
                        draw_text +: { color: #x000000 }
                    }
                }
                cross_row_down := View {
                    width: Fit, height: Fit, flow: Right
                    btn_back := Button {
                        text: "▼"
                        width: 56, height: 36
                        draw_bg +: { color: #xCCCCCC }
                        draw_text +: { color: #x000000 }
                    }
                }
            }
            test_row3 := View {
                width: Fill, height: Fit, flow: Right, spacing: 6
                btn_catch := Button {
                    text: "✊ Catch"
                    width: Fill, height: 32
                    draw_bg +: { color: #xCCCCCC }
                    draw_text +: { color: #x000000 }
                }
                btn_drop := Button {
                    text: "🖐 Release"
                    width: Fill, height: 32
                    draw_bg +: { color: #xCCCCCC }
                    draw_text +: { color: #x000000 }
                }
            }

            spacer3 := View { width: Fill, height: 8 }

            recent_label := Label {
                text: "Recent commands"
                draw_text +: {
                    color: #x404040
                    text_style: theme.font_regular { font_size: 12.0 }
                }
            }
            recent_log := Label {
                width: Fill, height: Fit
                text: ""
                draw_text +: {
                    color: #x505050
                    text_style: theme.font_regular { font_size: 11.0 }
                }
            }
        }
    }
}

/// Indicator states for the status dot.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
enum ConnState {
    /// No IP set, or IP invalid.
    #[default]
    Grey,
    /// Valid IP, no successful send yet, or last command timed out.
    Yellow,
    /// Last command returned 2xx within the last 5 s.
    Green,
    /// Last command returned non-2xx or transport error.
    Red,
}

impl ConnState {
    fn color(self) -> u32 {
        match self {
            Self::Grey => 0x888888,
            Self::Yellow => 0xD4A017,
            Self::Green => 0x33A852,
            Self::Red => 0xCC3333,
        }
    }
    fn label(self) -> &'static str {
        match self {
            Self::Grey => "Not connected",
            Self::Yellow => "Connecting…",
            Self::Green => "Connected",
            Self::Red => "Error",
        }
    }
}

#[derive(Script, ScriptHook, Widget)]
pub struct RobotScreen {
    #[deref] view: View,

    /// Validated IP (or `ip:port`) string. `None` means no valid value set.
    #[rust] valid_ip: Option<String>,
    /// Connection indicator state.
    #[rust] conn: ConnState,
    /// Time of the last green-flipping HTTP success (for the 5-s green window).
    #[rust] last_green_at: Option<Instant>,
    /// Last detected/sent gesture, for the "Last gesture" readout.
    #[rust] last_gesture: GestureAction,
    /// Rolling buffer of recent HTTP outcomes, newest first.
    #[rust] recent_lines: Vec<String>,
    /// Lazy: spawns the tokio HTTP task on first need.
    #[rust] http: Option<RobotHttpSender>,
    /// One-shot init flag so we only pull initial IP from app prefs once.
    #[rust] initialized: bool,
    /// Wall-clock instant at which the on-video gesture pill should auto-hide.
    /// `None` means the pill is currently hidden.
    #[rust] overlay_hide_at: Option<Instant>,
    /// True while we hold the shared `video_input(0,…)` callback. Released
    /// when the user switches to another tab (signalled by a `CameraAcquired`
    /// action whose consumer is not `Robot`).
    #[rust] camera_held: bool,
    /// True when the nokhwa capture worker is running.
    /// Drives the camera button label and the placeholder/video visibility.
    #[rust] camera_running: bool,
    /// Active background camera worker. `None` when camera is off.
    #[rust] capture: Option<InferenceCapture>,
    /// Hand-landmark ONNX inference worker. `Some` once the model has been
    /// successfully loaded; `None` if load failed or hasn't been attempted.
    #[rust] inference: Option<InferenceWorker>,
    /// Set once we've tried to load the model and failed — keeps us from
    /// re-attempting load on every click. Cleared automatically when a
    /// pending model download succeeds (see `pump_inference_results`).
    #[rust] inference_load_failed: bool,
    /// True once we've kicked off the async model download. Stays true
    /// even on failure so we don't spam the network with retries.
    #[rust] model_download_started: bool,
    /// Result channel from the background `ensure_downloaded()` task.
    /// `Some` only while a download is in flight; cleared once the result
    /// has been consumed in `pump_inference_results`.
    #[rust] model_download_rx: Option<Receiver<Result<(), String>>>,
    /// One-shot diagnostic: log the dimensions of the first camera frame
    /// that reaches inference so we can verify `cx.camera_frame_input`
    /// is actually firing on Android.
    #[rust] logged_first_camera_frame: bool,
    /// Which manual-control button is currently lit. `None` means all six are
    /// at their idle colour. Used to avoid redundant `script_apply_eval!` calls
    /// when continuous inference keeps emitting the same gesture.
    #[rust] highlighted_action: Option<GestureAction>,
    /// Wall-clock instant at which the current button highlight should fade.
    /// `None` means no active highlight.
    #[rust] highlight_clear_at: Option<Instant>,
    /// Wall-clock instant at which an inference-detected movement gesture
    /// should be auto-stopped (we send a one-shot `GestureAction::Stop`
    /// `AUTO_STOP_AFTER_MS` after the gesture was detected). Each new
    /// inference-detected movement resets the timer. `None` means no
    /// movement is currently active. Manual control buttons bypass this —
    /// they already use the press/release pattern for stop.
    #[rust] auto_stop_at: Option<Instant>,
}

const MAX_RECENT_LINES: usize = 8;
const GREEN_WINDOW_SECS: u64 = 5;
/// How long the on-video gesture pill stays visible after a detection.
const OVERLAY_HOLD_MS: u64 = 1_500;
/// How long a manual-control button stays highlighted after the most recent
/// gesture emit (continuous inference refreshes the timer each frame).
const HIGHLIGHT_HOLD_MS: u64 = 600;
/// How long after an inference-detected movement (Forward/Back/Left/Right)
/// we automatically issue a `Stop` to the robot. Each new movement detection
/// resets the timer — so holding a pose keeps the robot moving — but the
/// moment the inference stops emitting that gesture for `AUTO_STOP_AFTER_MS`,
/// we cut motion. Avoids runaway robots when the user lowers their hand
/// without immediately transitioning to a different gesture.
const AUTO_STOP_AFTER_MS: u64 = 500;
/// Idle background colour for the six manual-control buttons. Matches the
/// `#xCCCCCC` constant set on each `draw_bg` in the DSL.
const COLOR_BTN_DEFAULT: Vec4 = Vec4 { x: 0.8, y: 0.8, z: 0.8, w: 1.0 };
/// Highlight background colour applied to the button whose `GestureAction`
/// the inference most recently emitted.
const COLOR_BTN_HIGHLIGHT: Vec4 = Vec4 { x: 1.0, y: 0.8, z: 0.0, w: 1.0 };

impl Widget for RobotScreen {
    fn draw_walk(&mut self, cx: &mut Cx2d, scope: &mut Scope, walk: Walk) -> DrawStep {
        if !self.initialized {
            self.initialize_from_prefs(cx);
        }
        self.view.draw_walk(cx, scope, walk)
    }

    fn handle_event(&mut self, cx: &mut Cx, event: &Event, scope: &mut Scope) {
        if !self.initialized {
            self.initialize_from_prefs(cx);
        }

        // Each NextFrame: refresh the green-window (so the dot drops back
        // from green → yellow after 5 s of silence), auto-hide the gesture
        // overlay pill after its hold window expires, and pull the latest
        // camera frame from the worker into the WebRtcVideo widget.
        if matches!(event, Event::NextFrame(_)) {
            self.refresh_green_window(cx);
            self.refresh_overlay_visibility(cx);
            self.refresh_button_highlight(cx);
            self.refresh_auto_stop(cx);
            self.pump_camera_frames(cx);
            self.pump_inference_results(cx);
            cx.new_next_frame();
        }

        // HTTP results arrive on the standard NetworkResponses bus (no tokio
        // task — we use Makepad's cx.http_request now). Filter by request_id
        // inside the sender, then update UI state for each matched result.
        if let Event::NetworkResponses(responses) = event {
            for response in responses {
                let result = self
                    .http
                    .as_mut()
                    .and_then(|s| s.handle_network_response(response));
                if let Some(result) = result {
                    self.apply_http_result(cx, result);
                }
            }
        }

        self.view.handle_event(cx, event, scope);

        if let Event::Actions(actions) = event {
            self.handle_actions(cx, actions, scope);
        }
    }
}

impl RobotScreen {
    fn initialize_from_prefs(&mut self, cx: &mut Cx) {
        // Pull current IP value from app preferences global, if set.
        if cx.has_global::<AppPreferencesGlobal>() {
            let initial = cx.global::<AppPreferencesGlobal>().0.robot_control_ip.clone();
            if let Some(ref ip) = initial {
                if let Some(valid) = validate_ip(ip) {
                    self.valid_ip = Some(valid.clone());
                    self.view
                        .text_input(cx, ids!(robot_ip_input))
                        .set_text(cx, &valid);
                    self.set_conn_state(cx, ConnState::Yellow);
                }
            }
        }
        // Tell other camera consumers we're taking over.
        VoipGlobalState::acquire_camera_for(cx, CameraConsumer::Robot);
        self.camera_held = true;
        // Subscribe to NextFrame so we can poll HTTP results.
        cx.new_next_frame();
        self.initialized = true;
    }

    fn handle_actions(&mut self, cx: &mut Cx, actions: &Actions, _scope: &mut Scope) {
        // Some other tab acquired the camera (or the user switched to a tab
        // that uses no camera): release our preview / inference worker.
        // Some other tab pressed re-acquires Robot if it's us — typical when
        // returning to the Robot tab via the dock.
        for action in actions {
            if let Some(crate::voip::VoipAction::CameraAcquired { consumer }) = action.downcast_ref() {
                if *consumer == CameraConsumer::Robot {
                    if !self.camera_held {
                        self.acquire_camera(cx);
                    }
                } else if self.camera_held {
                    self.release_camera(cx);
                }
            }
        }

        // IP textbox: validate on Returned (commit on Enter key).
        // Note: `.returned(actions)` returns `Option<&(String, KeyModifiers)>`,
        // and `.changed(actions)` returns `Option<&String>` — destructure
        // before reading.
        let ip_input = self.view.text_input(cx, ids!(robot_ip_input));
        let returned_text = ip_input.returned(actions).map(|(t, _)| t.clone());
        let changed_text = ip_input.changed(actions);
        if let Some(text) = returned_text {
            self.commit_ip(cx, &text);
        }
        if let Some(text) = changed_text {
            // Live validation for the connection indicator AND for the
            // send-time IP cache: invalid IP → grey + clear self.valid_ip,
            // valid IP keeps whatever live state we're in and updates the
            // cache so movement-button clicks fire without requiring the
            // user to press Enter first. We do NOT persist to AppPreferences
            // here — persistence still happens on Returned via commit_ip.
            let parsed = validate_ip(&text);
            self.valid_ip = parsed.clone();
            let next = match (parsed.is_some(), self.conn) {
                (false, _) => ConnState::Grey,
                (true, ConnState::Green) => ConnState::Green,
                (true, _) => ConnState::Yellow,
            };
            self.set_conn_state(cx, next);
        }

        // Camera open/close button.
        if self.view.button(cx, ids!(btn_camera)).clicked(actions) {
            if self.camera_running {
                self.stop_camera_preview(cx);
            } else {
                self.start_camera_preview(cx);
            }
        }

        // Movement buttons follow a press-and-hold pattern: pressing fires
        // the directional command, releasing fires Stop. Catch/Drop remain
        // one-shot (clicked → command, no Stop on release) because the robot
        // treats grab/release as latching actions.
        let movement_map = [
            (ids!(btn_forward), GestureAction::Forward),
            (ids!(btn_back),    GestureAction::Back),
            (ids!(btn_left),    GestureAction::Left),
            (ids!(btn_right),   GestureAction::Right),
        ];
        let mut any_movement_released = false;
        for (id_path, action) in movement_map {
            let btn = self.view.button(cx, id_path);
            if btn.pressed(actions) {
                self.emit_gesture(cx, action);
            }
            if btn.clicked(actions) || btn.released(actions) {
                any_movement_released = true;
            }
        }
        if any_movement_released {
            self.fire_command(cx, GestureAction::Stop);
        }

        // Center button — fires Stop without going through emit_gesture
        // (no overlay flash, no "Last gesture" overwrite, no D-pad highlight).
        if self.view.button(cx, ids!(btn_stop)).clicked(actions) {
            self.fire_command(cx, GestureAction::Stop);
        }

        let oneshot_map = [
            (ids!(btn_catch), GestureAction::Catch),
            (ids!(btn_drop),  GestureAction::Drop),
        ];
        for (id_path, action) in oneshot_map {
            if self.view.button(cx, id_path).clicked(actions) {
                self.emit_gesture(cx, action);
            }
        }
    }

    /// Commit a new IP from the textbox: validate, persist to AppPreferences,
    /// refresh state.
    fn commit_ip(&mut self, cx: &mut Cx, raw: &str) {
        match validate_ip(raw) {
            Some(valid) => {
                self.valid_ip = Some(valid.clone());
                if cx.has_global::<AppPreferencesGlobal>() {
                    cx.global::<AppPreferencesGlobal>().0.robot_control_ip = Some(valid.clone());
                    cx.action(
                        crate::settings::app_preferences::AppPreferencesAction::RobotControlIpChanged(
                            Some(valid),
                        ),
                    );
                }
                self.set_conn_state(cx, ConnState::Yellow);
            }
            None => {
                self.valid_ip = None;
                self.set_conn_state(cx, ConnState::Grey);
            }
        }
    }

    /// Emit a gesture: action bus + HTTP send + update last_gesture +
    /// light up the matching manual-control button.
    fn emit_gesture(&mut self, cx: &mut Cx, action: GestureAction) {
        self.last_gesture = action;
        self.view
            .label(cx, ids!(last_gesture_value))
            .set_text(cx, action.display_label());
        cx.action(action);

        // Flash the gesture pill on top of the webcam preview.
        self.show_overlay(cx, action);
        // Light up the corresponding manual-control button (and unlight any
        // previously-lit one).
        self.highlight_action_button(cx, action);

        // Grab and Release are gripper actions — halt any in-flight movement
        // before actuating so the arm isn't being driven while it grabs.
        if matches!(action, GestureAction::Catch | GestureAction::Drop) {
            self.fire_command(cx, GestureAction::Stop);
        }
        self.fire_command(cx, action);
        self.view.redraw(cx);
    }

    /// Fire an HTTP control command without touching gesture-display state.
    /// Used both by `emit_gesture` and by the movement-button-release path,
    /// which sends `GestureAction::Stop` but should not overwrite the
    /// "Last gesture" readout or flash the overlay.
    fn fire_command(&mut self, cx: &mut Cx, action: GestureAction) {
        println!("valid_ip {:?}", self.valid_ip);
        let Some(ip) = self.valid_ip.clone() else { return };
        self.ensure_http_sender();
        if let Some(sender) = self.http.as_mut() {
            sender.send(cx, action, &ip);
        }
    }

    /// Apply the highlight colour to the button matching `action`, default
    /// colour to all others. Skips work if `action` is already highlighted —
    /// continuous inference repeatedly emits the same gesture and we don't
    /// want to re-run six `script_apply_eval!`s every frame.
    fn highlight_action_button(&mut self, cx: &mut Cx, action: GestureAction) {
        let new_highlight = match action {
            GestureAction::None => None,
            other => Some(other),
        };
        // Refresh the fade-out timer on every call, even if the highlighted
        // button hasn't changed — the timer is what keeps the light on while
        // the user holds a pose against continuous inference.
        self.highlight_clear_at =
            Some(Instant::now() + std::time::Duration::from_millis(HIGHLIGHT_HOLD_MS));
        if self.highlighted_action == new_highlight {
            return;
        }
        self.apply_button_colors(cx, new_highlight);
        self.highlighted_action = new_highlight;
    }

    /// Walk all six manual-control buttons and set each `draw_bg.color` to
    /// either `COLOR_BTN_HIGHLIGHT` (if it matches `current`) or
    /// `COLOR_BTN_DEFAULT`. Called from `highlight_action_button` and from
    /// the timer-driven fade-out path.
    fn apply_button_colors(&mut self, cx: &mut Cx, current: Option<GestureAction>) {
        let buttons = [
            (GestureAction::Forward, ids!(btn_forward)),
            (GestureAction::Back,    ids!(btn_back)),
            (GestureAction::Left,    ids!(btn_left)),
            (GestureAction::Right,   ids!(btn_right)),
            (GestureAction::Catch,   ids!(btn_catch)),
            (GestureAction::Drop,    ids!(btn_drop)),
        ];
        for (act, id_path) in buttons {
            let color = if Some(act) == current {
                COLOR_BTN_HIGHLIGHT
            } else {
                COLOR_BTN_DEFAULT
            };
            let mut btn = self.view.button(cx, id_path);
            script_apply_eval!(cx, btn, {
                draw_bg +: { color: #(color) }
            });
        }
    }

    /// Drop the highlight back to idle once the hold window expires.
    fn refresh_button_highlight(&mut self, cx: &mut Cx) {
        if let Some(deadline) = self.highlight_clear_at {
            if Instant::now() >= deadline {
                self.apply_button_colors(cx, None);
                self.highlighted_action = None;
                self.highlight_clear_at = None;
                self.view.redraw(cx);
            }
        }
    }

    /// Lazily build the HTTP sender on first use. Now just allocates the
    /// pending-request map — no tokio task involved.
    fn ensure_http_sender(&mut self) {
        if self.http.is_none() {
            self.http = Some(RobotHttpSender::new());
        }
    }

    fn apply_http_result(&mut self, cx: &mut Cx, result: HttpResult) {
        let (state, line) = match &result.outcome {
            HttpOutcome::Ok { status, latency_ms } => {
                self.last_green_at = Some(Instant::now());
                (
                    ConnState::Green,
                    format!("{} {} {}ms", result.action.display_label(), status, latency_ms),
                )
            }
            HttpOutcome::HttpStatus { status } => (
                ConnState::Red,
                format!("{} HTTP {}", result.action.display_label(), status),
            ),
            HttpOutcome::Error(e) => (
                ConnState::Red,
                format!("{} error: {}", result.action.display_label(), short_err(e)),
            ),
        };
        self.set_conn_state(cx, state);
        self.recent_lines.insert(0, line);
        self.recent_lines.truncate(MAX_RECENT_LINES);
        self.view
            .label(cx, ids!(recent_log))
            .set_text(cx, &self.recent_lines.join("\n"));
        self.view.redraw(cx);
    }

    /// Acquire exclusive use of the shared camera callback. Called when the
    /// Robot tab is (re-)selected.
    fn acquire_camera(&mut self, cx: &mut Cx) {
        log!("RobotScreen: acquiring camera");
        self.camera_held = true;
        self.view.redraw(cx);
    }

    /// Release the camera — called when the user switches to a non-Robot tab.
    /// Stops the video preview and hides the gesture overlay. The webcam
    /// view itself is hidden by `stop_camera_preview` below.
    fn release_camera(&mut self, cx: &mut Cx) {
        log!("RobotScreen: releasing camera");
        self.camera_held = false;
        if self.camera_running {
            self.stop_camera_preview(cx);
        }
        // Hide the on-video gesture overlay immediately.
        self.view
            .view(cx, ids!(gesture_overlay_pill))
            .set_visible(cx, false);
        self.overlay_hide_at = None;
        self.view.redraw(cx);
    }

    /// Submit a fresh camera frame to the background `InferenceWorker` for
    /// continuous classification. The worker's bounded(1) input channel
    /// naturally throttles us — if it's still busy with the previous frame,
    /// `try_send` returns Full and we drop this one, keeping the latency low.
    ///
    /// The model is lazy-loaded on the first frame so a missing/corrupt ONNX
    /// surfaces as a one-time log line rather than blocking tab open.
    ///
    /// We horizontally flip the RGBA buffer before submitting because
    /// `pick_camera_choice` now prefers the front (selfie) camera, which
    /// delivers a mirrored image. The hand-model and `gesture_classifier`'s
    /// handedness threshold were calibrated against the un-mirrored back-
    /// camera orientation, so without this flip a user's right hand reads
    /// as left and the two-finger Left/Right gestures come out swapped.
    fn submit_frame_for_inference(&mut self, mut frame: WebRtcVideoFrame) {
        mirror_rgba_horizontal_in_place(&mut frame.data, frame.width, frame.height);
        if !self.logged_first_camera_frame {
            log!(
                "RobotScreen: first inference frame received — {}x{} RGBA, {} bytes",
                frame.width, frame.height, frame.data.len()
            );
            self.logged_first_camera_frame = true;
        }
        if self.inference.is_none() && !self.inference_load_failed {
            // Don't even try to load if the ONNX file isn't on disk —
            // `ensure_model_download_started` (driven from the per-frame
            // pump) will fetch it asynchronously and clear
            // `inference_load_failed` on success so the next frame retries.
            if !model_downloader::landmark_model_present_and_valid() {
                return;
            }
            match InferenceWorker::spawn() {
                Ok(w) => self.inference = Some(w),
                Err(e) => {
                    log!("RobotScreen: hand-model load failed: {e:#}");
                    self.inference_load_failed = true;
                }
            }
        }
        if let Some(worker) = self.inference.as_ref() {
            let _ = worker.submit(frame);
        }
    }

    /// Kick off the async model download via Robrix's tokio runtime if we
    /// haven't already. Result lands on `self.model_download_rx`, which is
    /// drained on every NextFrame in `pump_inference_results`.
    fn ensure_model_download_started(&mut self) {
        if self.model_download_started {
            return;
        }
        if model_downloader::landmark_model_present_and_valid() {
            return;
        }
        self.model_download_started = true;
        let (tx, rx) = crossbeam_channel::bounded::<Result<(), String>>(1);
        self.model_download_rx = Some(rx);
        log!("RobotScreen: hand-model ONNX missing — starting async download");
        let handle = match crate::sliding_sync::start_matrix_tokio() {
            Ok(h) => h,
            Err(e) => {
                log!("RobotScreen: tokio runtime unavailable for model download: {e:#}");
                let _ = tx.try_send(Err(format!("no tokio runtime: {e}")));
                return;
            }
        };
        handle.spawn(async move {
            let result = model_downloader::ensure_downloaded()
                .await
                .map_err(|e| format!("{e:#}"));
            let _ = tx.try_send(result);
        });
    }

    /// Drain any inference results delivered by the background worker, update
    /// the inference label, and emit any recognized gesture. Also polls the
    /// async model-download channel so the inference can come online once
    /// the ONNX file lands on disk.
    fn pump_inference_results(&mut self, cx: &mut Cx) {
        // First — if camera frames are flowing but inference can't load
        // because the ONNX is missing, start the async download. Idempotent.
        if self.camera_running
            && self.inference.is_none()
            && !self.inference_load_failed
            && !self.model_download_started
            && !model_downloader::landmark_model_present_and_valid()
        {
            self.ensure_model_download_started();
            self.view
                .label(cx, ids!(inference_value))
                .set_text(cx, "Downloading model…");
            self.view.redraw(cx);
        }

        // Second — drain the download result if one is pending. Clearing
        // `inference_load_failed` lets the next frame retry `HandModel::load`.
        if let Some(rx) = self.model_download_rx.as_ref() {
            if let Ok(result) = rx.try_recv() {
                match result {
                    Ok(()) => {
                        log!("RobotScreen: model download succeeded; will retry inference");
                        self.inference_load_failed = false;
                        self.view
                            .label(cx, ids!(inference_value))
                            .set_text(cx, "Model ready");
                    }
                    Err(e) => {
                        log!("RobotScreen: model download failed: {e}");
                        self.inference_load_failed = true;
                        self.view
                            .label(cx, ids!(inference_value))
                            .set_text(cx, "Model download failed");
                    }
                }
                self.model_download_rx = None;
                self.view.redraw(cx);
            }
        }

        let results: Vec<_> = {
            let Some(worker) = self.inference.as_ref() else { return };
            std::iter::from_fn(|| worker.try_recv()).collect()
        };
        for result in results {
            if matches!(result.detected, GestureAction::None) {
                let text = if result.landmarks.is_some() {
                    "(no gesture)"
                } else {
                    "(no hand detected)"
                };
                self.view
                    .label(cx, ids!(inference_value))
                    .set_text(cx, text);
                self.view.redraw(cx);
                // No gesture detected → halt the robot. Dedup against the
                // previously-emitted gesture so a hand-out-of-frame stretch
                // doesn't flood the wire with stop requests — we only fire
                // Stop on the first frame of "no gesture" after movement.
                if !matches!(self.last_gesture, GestureAction::None | GestureAction::Stop) {
                    self.fire_command(cx, GestureAction::Stop);
                    self.last_gesture = GestureAction::Stop;
                }
            } else {
                log!("RobotScreen: inferred gesture: {:?}", result.detected);
                self.view
                    .label(cx, ids!(inference_value))
                    .set_text(cx, result.detected.display_label());
                self.emit_gesture(cx, result.detected);
                // For inference-detected movements only, schedule an
                // automatic Stop one second from now (refreshed on each new
                // movement detection, so holding the pose keeps the robot
                // going). Manual control buttons bypass this; they already
                // emit Stop on release.
                if matches!(
                    result.detected,
                    GestureAction::Forward
                        | GestureAction::Back
                        | GestureAction::Left
                        | GestureAction::Right
                ) {
                    self.auto_stop_at = Some(
                        Instant::now() + std::time::Duration::from_millis(AUTO_STOP_AFTER_MS),
                    );
                } else {
                    // Grab/Release/Stop reset the timer too (no auto-stop
                    // needed; those are one-shot or already-Stop actions).
                    self.auto_stop_at = None;
                }
            }
        }
    }

    /// Fire the auto-stop if its timer has elapsed. Called on every NextFrame.
    fn refresh_auto_stop(&mut self, cx: &mut Cx) {
        let Some(deadline) = self.auto_stop_at else { return };
        if Instant::now() < deadline {
            return;
        }
        log!("RobotScreen: auto-stopping inference-driven movement after timeout");
        self.auto_stop_at = None;
        // Only send Stop if we haven't already (e.g. via the no-gesture
        // transition path). Last-gesture dedup keeps us from spamming.
        if !matches!(self.last_gesture, GestureAction::None | GestureAction::Stop) {
            self.fire_command(cx, GestureAction::Stop);
            self.last_gesture = GestureAction::Stop;
        }
    }

    /// Start the camera: kick the Makepad `Video` widget in Native mode so
    /// the OS stream starts, and register a parallel `camera_frame_input`
    /// callback so we get RGBA frames for inference.
    fn start_camera_preview(&mut self, cx: &mut Cx) {
        if self.capture.is_some() {
            return;
        }
        let choice = VoipGlobalState::get_camera_choice(cx);
        let Some(choice) = choice else {
            log!("RobotScreen: no camera available — refusing to start");
            return;
        };
        let video = self.view.video(cx, ids!(preview_video));
        if !video.is_unprepared() {
            log!("RobotScreen: video already prepared, skipping start");
            return;
        }
        log!(
            "RobotScreen: starting camera {} ({}x{} {:?})",
            choice.name, choice.width, choice.height, choice.pixel_format
        );

        // The webcam_view container is always visible (it's pinned to the
        // top-right of the panel); we only flip the Video widget itself on
        // BEFORE begin_playback so Makepad's native preview can attach its
        // overlay layer to the now-visible draw list.
        let video = self.view.video(cx, ids!(preview_video));
        video.set_visible(cx, true);

        // Native preview mode. Texture mode is not implemented on macOS in
        // this Makepad version and falls back to Native anyway — leaving the
        // explicit Native here keeps the launch log clean.
        video.set_camera_preview_mode(cx, VideoCameraPreviewMode::Native);
        video.set_source_camera(cx, choice.input_id, choice.format_id);
        video.begin_playback(cx);

        // Spin up the inference capture path. On macOS this opens a parallel
        // AVCaptureSession (BGRA-aware) so OBS Virtual Camera frames still
        // reach the heuristic. On other platforms it rides Makepad's
        // camera_frame_input callback.
        self.capture = start_inference_capture(cx);
        self.camera_running = true;

        self.view
            .button(cx, ids!(btn_camera))
            .set_text(cx, "Close camera");
        self.view.redraw(cx);
    }

    /// Stop the camera and collapse the inline webcam view so the panel goes
    /// back to its compact "camera off" layout.
    fn stop_camera_preview(&mut self, cx: &mut Cx) {
        log!("RobotScreen: stopping camera preview");
        let video = self.view.video(cx, ids!(preview_video));
        if !video.is_unprepared() && !video.is_cleaning_up() {
            video.stop_and_cleanup_resources(cx);
        }
        // Drop the receiver — the Makepad-side callback stays registered but
        // its try_send becomes a silent no-op.
        self.capture = None;
        self.camera_running = false;
        // Hide only the Video widget — the webcam_view container itself
        // stays visible (just shows the dark gray background) so the slot
        // doesn't collapse when the camera is closed.
        self.view.video(cx, ids!(preview_video)).set_visible(cx, false);
        self.view
            .button(cx, ids!(btn_camera))
            .set_text(cx, "Open camera");
        self.view.redraw(cx);
    }

    /// Pull pending frames from the camera callback and forward the newest to
    /// the inference worker. Display is handled by Makepad's Video widget
    /// directly — we don't push frames to it ourselves.
    fn pump_camera_frames(&mut self, _cx: &mut Cx) {
        let Some(cap) = self.capture.as_ref() else { return };
        // Drain anything backed up; we only keep the newest so inference always
        // runs on the freshest possible frame.
        let mut newest: Option<WebRtcVideoFrame> = None;
        while let Some(f) = cap.try_recv() {
            newest = Some(f);
        }
        if let Some(frame) = newest {
            self.submit_frame_for_inference(frame);
        }
    }

    /// Show the gesture pill on top of the webcam preview.
    /// `GestureAction::None` hides the pill immediately.
    fn show_overlay(&mut self, cx: &mut Cx, action: GestureAction) {
        let pill = self.view.view(cx, ids!(gesture_overlay_pill));
        if matches!(action, GestureAction::None) {
            pill.set_visible(cx, false);
            self.overlay_hide_at = None;
            self.view.redraw(cx);
            return;
        }
        self.view
            .label(cx, ids!(gesture_overlay_icon))
            .set_text(cx, action.icon());
        self.view
            .label(cx, ids!(gesture_overlay_label))
            .set_text(cx, action.display_label());
        pill.set_visible(cx, true);
        self.overlay_hide_at =
            Some(Instant::now() + std::time::Duration::from_millis(OVERLAY_HOLD_MS));
        self.view.redraw(cx);
    }

    /// Hide the overlay pill once its hold window expires.
    fn refresh_overlay_visibility(&mut self, cx: &mut Cx) {
        if let Some(deadline) = self.overlay_hide_at {
            if Instant::now() >= deadline {
                self.view
                    .view(cx, ids!(gesture_overlay_pill))
                    .set_visible(cx, false);
                self.overlay_hide_at = None;
                self.view.redraw(cx);
            }
        }
    }

    fn refresh_green_window(&mut self, cx: &mut Cx) {
        if self.conn == ConnState::Green {
            if let Some(t) = self.last_green_at {
                if t.elapsed().as_secs() >= GREEN_WINDOW_SECS {
                    self.set_conn_state(cx, ConnState::Yellow);
                }
            }
        }
    }

    fn set_conn_state(&mut self, cx: &mut Cx, state: ConnState) {
        if self.conn == state {
            return;
        }
        self.conn = state;
        self.view
            .label(cx, ids!(status_label))
            .set_text(cx, state.label());
        let _ = state.color(); // colour is applied via draw_bg in a follow-up
                                // shader-instance update; for now we keep the
                                // dot a neutral fixed colour and rely on the
                                // text label to convey state.
        self.view.redraw(cx);
    }
}

/// Horizontally flip a packed-RGBA buffer in place. Used to undo the selfie
/// mirror on front-camera frames before they go to the hand-landmark model —
/// see `RobotScreen::submit_frame_for_inference` for context.
fn mirror_rgba_horizontal_in_place(data: &mut [u8], width: u32, height: u32) {
    let w = width as usize;
    let h = height as usize;
    let row_stride = w * 4;
    if data.len() < row_stride * h || w < 2 {
        return;
    }
    let half = w / 2;
    for y in 0..h {
        let row = &mut data[y * row_stride..(y + 1) * row_stride];
        for c in 0..half {
            let left = c * 4;
            let right = (w - 1 - c) * 4;
            for k in 0..4 {
                row.swap(left + k, right + k);
            }
        }
    }
}

/// Trim a transport error message so it fits in the log row.
fn short_err(e: &str) -> String {
    let trimmed: String = e.chars().take(60).collect();
    if e.len() > trimmed.len() {
        format!("{trimmed}…")
    } else {
        trimmed
    }
}

#[cfg(target_os = "macos")]
fn start_inference_capture(_cx: &mut makepad_widgets::Cx) -> Option<InferenceCapture> {
    match crate::gesture_control::avf_capture::AvfCapture::start() {
        Ok(cap) => Some(cap),
        Err(e) => {
            makepad_widgets::log!("RobotScreen: AvfCapture start failed: {e}");
            None
        }
    }
}

#[cfg(not(target_os = "macos"))]
fn start_inference_capture(cx: &mut makepad_widgets::Cx) -> Option<InferenceCapture> {
    Some(crate::gesture_control::camera_capture::CameraCapture::start(cx))
}

