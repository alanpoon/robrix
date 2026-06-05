//! Top-level Robot tab screen.
//!
//! This is the user-visible surface for the gesture-control feature:
//! - IP textbox (left side) — driven by `AppPreferences::robot_control_ip`
//! - Status indicator (grey/yellow/green/red dot)
//! - Last-gesture readout
//! - Recent commands log (rolling, ~8 lines)
//! - Six test buttons (one per `GestureAction`) so the HTTP pipeline can be
//!   exercised on the desk without the webcam pipeline being live yet.
//! - Camera preview area — currently a "Camera offline" placeholder.
//!   The real `GestureWebcamView` (NV12→texture upload + landmark overlay) is
//!   wired in once the ONNX models are pinned, per the task spec.

use std::time::Instant;

use makepad_widgets::*;
use makepad_widgets::video::VideoCameraPreviewMode;

use crate::gesture_control::{
    GestureAction,
    frame_analyzer,
    robot_http::{HttpOutcome, RobotHttpSender, validate_ip},
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
        flow: Right
        show_bg: true
        draw_bg.color: (COLOR_PRIMARY)
        padding: Inset{top: 12, bottom: 12, left: 12, right: 12}
        spacing: 12

        // ── Left: webcam preview (placeholder) + gesture overlay ─────────
        // Overlay flow stacks children — placeholder/video on the bottom,
        // gesture pill floats on top.
        //
        // Both child layers are explicit Views with `width: Fill, height: Fill`
        // so the Overlay-flow parent's height isn't pulled down to the natural
        // height of a single Label's text. (Bare `Label { height: Fill }` was
        // collapsing the parent to ~60 px tall in earlier screenshots.)
        preview_col := RoundedView {
            width: Fill, height: Fill
            flow: Overlay
            show_bg: true
            draw_bg +: { color: #x1a1a1a, border_radius: 8.0 }

            // Layer 0a — live camera preview.
            // Display uses Makepad's native `Video` widget in Native preview
            // mode (camera → GPU directly). Inference frames come through a
            // separate `cx.camera_frame_input(0, …)` callback registered in
            // `camera_capture.rs` — same camera, parallel pixel path.
            preview_video := Video {
                width: Fill, height: Fill
                visible: false
            }

            // Layer 0b — placeholder / "camera off" message.
            placeholder_layer := View {
                width: Fill, height: Fill
                align: Align{x: 0.5, y: 0.5}

                preview_placeholder := Label {
                    text: "Camera offline\n(click \"Open camera\" to start)"
                    align: Align{x: 0.5, y: 0.5}
                    draw_text +: {
                        color: #x808080
                        text_style: theme.font_regular { font_size: 14.0 }
                    }
                }
            }

            // Layer 1 — gesture overlay pill, centered. Visibility toggled
            // from Rust on each `emit_gesture`.
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
                    padding: Inset{top: 14, bottom: 14, left: 28, right: 28}
                    spacing: 4

                    gesture_overlay_icon := Label {
                        text: "—"
                        align: Align{x: 0.5, y: 0.5}
                        draw_text +: {
                            color: #xFFFFFF
                            text_style: theme.font_bold { font_size: 56.0 }
                        }
                    }
                    gesture_overlay_label := Label {
                        text: ""
                        align: Align{x: 0.5, y: 0.5}
                        draw_text +: {
                            color: #xFFFFFF
                            text_style: theme.font_bold { font_size: 18.0 }
                        }
                    }
                }
            }
        }

        // ── Right: control panel ─────────────────────────────────────────
        panel_col := View {
            width: 300, height: Fill
            flow: Down
            spacing: 10

            panel_title := Label {
                text: "Robot"
                draw_text +: {
                    color: #x202020
                    text_style: theme.font_bold { font_size: 18.0 }
                }
            }

            // IP row
            ip_label := Label {
                text: "Robot IP"
                draw_text +: {
                    color: #x404040
                    text_style: theme.font_regular { font_size: 12.0 }
                }
            }
            robot_ip_input := RobrixTextInput {
                width: Fill, height: Fit
                empty_text: "192.168.4.1"
                padding: Inset{top: 6, bottom: 6, left: 10, right: 10}
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

            // Camera toggle.
            btn_camera := Button {
                text: "Open camera"
                width: Fill, height: 36
            }

            // One-shot gesture inference on the current webcam frame.
            btn_infer := Button {
                text: "Infer gesture"
                width: Fill, height: 36
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

            // Six test buttons — fire each gesture without the camera.
            test_label := Label {
                text: "Test"
                draw_text +: {
                    color: #x404040
                    text_style: theme.font_regular { font_size: 12.0 }
                }
            }
            test_row1 := View {
                width: Fill, height: Fit, flow: Right, spacing: 6
                btn_forward := Button { text: "▲" width: Fill height: 32 }
                btn_back    := Button { text: "▼" width: Fill height: 32 }
            }
            test_row2 := View {
                width: Fill, height: Fit, flow: Right, spacing: 6
                btn_left  := Button { text: "◀" width: Fill height: 32 }
                btn_right := Button { text: "▶" width: Fill height: 32 }
            }
            test_row3 := View {
                width: Fill, height: Fit, flow: Right, spacing: 6
                btn_catch := Button { text: "✊ Catch" width: Fill height: 32 }
                btn_drop  := Button { text: "🖐 Drop"  width: Fill height: 32 }
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
    /// Most recent RGBA frame received from the worker. Reused by the
    /// "Infer gesture" button — we never re-grab from the camera on click.
    #[rust] latest_frame: Option<WebRtcVideoFrame>,
}

const MAX_RECENT_LINES: usize = 8;
const GREEN_WINDOW_SECS: u64 = 5;
/// How long the on-video gesture pill stays visible after a detection.
const OVERLAY_HOLD_MS: u64 = 1_500;

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

        // Each NextFrame: drain HTTP results, refresh the green-window (so
        // the dot drops back from green → yellow after 5 s of silence),
        // auto-hide the gesture overlay pill after its hold window expires,
        // and pull the latest camera frame from the worker into the
        // WebRtcVideo widget.
        if matches!(event, Event::NextFrame(_)) {
            self.drain_http_results(cx);
            self.refresh_green_window(cx);
            self.refresh_overlay_visibility(cx);
            self.pump_camera_frames(cx);
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
            // Live validation for the connection indicator: invalid IP → grey,
            // valid IP keeps whatever live state we're in. We do NOT persist
            // here — persistence happens on Returned.
            let next = match (validate_ip(&text).is_some(), self.conn) {
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

        // Run a one-shot inference against the current webcam frame.
        if self.view.button(cx, ids!(btn_infer)).clicked(actions) {
            self.infer_gesture(cx);
        }

        // Test buttons fire gestures directly, bypassing the (not-yet-wired)
        // ML pipeline so the HTTP path can be exercised on the desk.
        let fire_map = [
            (ids!(btn_forward), GestureAction::Forward),
            (ids!(btn_back),    GestureAction::Back),
            (ids!(btn_left),    GestureAction::Left),
            (ids!(btn_right),   GestureAction::Right),
            (ids!(btn_catch),   GestureAction::Catch),
            (ids!(btn_drop),    GestureAction::Drop),
        ];
        for (id_path, action) in fire_map {
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

    /// Emit a gesture: action bus + HTTP send + update last_gesture.
    fn emit_gesture(&mut self, cx: &mut Cx, action: GestureAction) {
        self.last_gesture = action;
        self.view
            .label(cx, ids!(last_gesture_value))
            .set_text(cx, action.display_label());
        cx.action(action);

        // Flash the gesture pill on top of the webcam preview.
        self.show_overlay(cx, action);

        // Send over HTTP if we have a valid IP. We bind the IP up-front so the
        // borrow of `self` for `http_sender()` doesn't overlap with `self.view`.
        if let Some(ip) = self.valid_ip.clone() {
            self.ensure_http_sender();
            if let Some(sender) = self.http.as_ref() {
                sender.send(action, ip);
            }
        }
        self.view.redraw(cx);
    }

    /// Lazily spawn the tokio HTTP task on first use.
    fn ensure_http_sender(&mut self) {
        if self.http.is_some() {
            return;
        }
        match crate::sliding_sync::start_matrix_tokio() {
            Ok(handle) => self.http = Some(RobotHttpSender::spawn(handle)),
            Err(e) => {
                log!("RobotScreen: failed to start tokio for HTTP sender: {e}");
            }
        }
    }

    fn drain_http_results(&mut self, cx: &mut Cx) {
        // Drain into a local buffer first so the immutable borrow of
        // `self.http` is released before we call `self.apply_http_result`,
        // which needs `&mut self`.
        let drained: Vec<crate::gesture_control::robot_http::HttpResult> = if let Some(s) = self.http.as_ref() {
            std::iter::from_fn(|| s.try_recv()).collect()
        } else {
            Vec::new()
        };
        for result in drained {
            self.apply_http_result(cx, result);
        }
        // Keep polling each frame.
        cx.new_next_frame();
    }

    fn apply_http_result(
        &mut self,
        cx: &mut Cx,
        result: crate::gesture_control::robot_http::HttpResult,
    ) {
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
            HttpOutcome::Timeout => (
                ConnState::Yellow,
                format!("{} timeout", result.action.display_label()),
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
        // Refresh the placeholder so it reads "Camera offline" again instead
        // of the "Camera released" text from a previous deselection.
        if !self.camera_running {
            self.view
                .label(cx, ids!(preview_placeholder))
                .set_text(cx, "Camera offline\n(click \"Open camera\" to start)");
        }
        self.view.redraw(cx);
    }

    /// Release the camera — called when the user switches to a non-Robot tab.
    /// Stops the video preview, hides the gesture overlay, swaps the
    /// placeholder text so the user can see we've let go.
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
        self.view
            .label(cx, ids!(preview_placeholder))
            .set_text(cx, "Camera released\n(another tab is using it)");
        self.view.redraw(cx);
    }

    /// Run a one-shot gesture inference against the most recently received
    /// webcam frame.
    ///
    /// The current implementation runs the skin-color centroid heuristic in
    /// `frame_analyzer::classify_frame`. It returns a `GestureAction` based on
    /// where the largest skin-tone blob sits in the frame and what shape its
    /// bounding box has. This is *not* hand-landmark ML — but it responds to
    /// actual hand position and is the wired-in seam for when a real
    /// `hand_model::run` + `gesture_classifier::classify` chain replaces it.
    fn infer_gesture(&mut self, cx: &mut Cx) {
        if !self.camera_running {
            self.view
                .label(cx, ids!(inference_value))
                .set_text(cx, "(open camera first)");
            self.view.redraw(cx);
            return;
        }
        let Some(frame) = self.latest_frame.as_ref() else {
            self.view
                .label(cx, ids!(inference_value))
                .set_text(cx, "(no frame yet)");
            self.view.redraw(cx);
            return;
        };

        match frame_analyzer::classify_frame(frame) {
            Some(action) => {
                log!("RobotScreen: inferred gesture: {action:?}");
                self.view
                    .label(cx, ids!(inference_value))
                    .set_text(cx, action.display_label());
                self.emit_gesture(cx, action);
            }
            None => {
                self.view
                    .label(cx, ids!(inference_value))
                    .set_text(cx, "(no hand detected)");
                self.view.redraw(cx);
            }
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
            self.view
                .label(cx, ids!(preview_placeholder))
                .set_text(cx, "No camera detected");
            self.view.redraw(cx);
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

        // Order matters: the Video widget must be visible (and the placeholder
        // hidden) BEFORE begin_playback, otherwise Makepad's macOS native
        // preview can't attach its camera layer to the widget's draw list.
        // The VoIP lobby uses the same ordering.
        self.view.view(cx, ids!(placeholder_layer)).set_visible(cx, false);
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

    /// Stop the camera and swap back to the placeholder.
    fn stop_camera_preview(&mut self, cx: &mut Cx) {
        log!("RobotScreen: stopping camera preview");
        let video = self.view.video(cx, ids!(preview_video));
        if !video.is_unprepared() && !video.is_cleaning_up() {
            video.stop_and_cleanup_resources(cx);
        }
        // Drop the receiver — the Makepad-side callback stays registered but
        // its try_send becomes a silent no-op.
        self.capture = None;
        self.latest_frame = None;
        self.camera_running = false;
        self.view.video(cx, ids!(preview_video)).set_visible(cx, false);
        self.view.view(cx, ids!(placeholder_layer)).set_visible(cx, true);
        self.view
            .label(cx, ids!(preview_placeholder))
            .set_text(cx, "Camera offline\n(click \"Open camera\" to start)");
        self.view
            .button(cx, ids!(btn_camera))
            .set_text(cx, "Open camera");
        self.view.redraw(cx);
    }

    /// Pull pending frames from the camera callback into the latest_frame
    /// cache for the Infer button. Display is handled by Makepad's Video
    /// widget directly — we don't push frames to it ourselves.
    fn pump_camera_frames(&mut self, _cx: &mut Cx) {
        let Some(cap) = self.capture.as_ref() else { return };
        // Drain anything backed up; we only keep the newest.
        let mut newest: Option<WebRtcVideoFrame> = None;
        while let Some(f) = cap.try_recv() {
            newest = Some(f);
        }
        if let Some(frame) = newest {
            self.latest_frame = Some(frame);
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

