//! `VideoMessagePlayerModal` — the **inner content widget** placed inside
//! a Makepad `Modal { content +: { ... } }` shell so the user can view a
//! maximised video.
//!
//! Architecture (per `specs/Month-2/video-playback.spec.md`, mirrors the
//! pattern at `src/home/event_source_modal.rs` and its outer-Modal wiring
//! at `src/app.rs:124-131` / `src/app.rs:868-881`):
//!
//! ```ignore
//! // In RoomScreen's live_design block:
//! video_message_player_modal := Modal {
//!     content +: {
//!         height: Fill, width: Fill,
//!         align: Align{x: 0.5, y: 0.5},
//!         video_message_player_modal_inner := VideoMessagePlayerModal {}
//!     }
//! }
//! ```
//!
//! - The **outer** Makepad `Modal` provides the scrim, the overlay
//!   `DrawList2d`, and the scrim-click / Escape / back-press dismissal.
//! - This widget renders only the **card body** — close button, controls,
//!   and the rect that the inline player draws its surface into.
//! - State is **shared** with the inline `VideoMessagePlayer` via
//!   `Arc<Mutex<...>>` handles passed in through [`show`](Self::show).
//!   Mutations from the modal's controls are observed by the inline view
//!   on the next paint, and vice versa.
//! - Action protocol:
//!     - `VideoMessagePlayerModalAction::Open { ... }` — emitted by the
//!       inline player's `maximise_button` handler. `RoomScreen` receives
//!       this, calls `inner.show(cx, ...)`, then `outer.open(cx)`.
//!     - `VideoMessagePlayerModalAction::Close` — emitted by this widget
//!       when its `close_button` is clicked. `RoomScreen` receives this
//!       and calls `outer.close(cx)`.
//!     - `ModalAction::Dismissed` (from the outer Makepad `Modal`) is
//!       observed here and flips `ui_state.maximised = false` so the
//!       inline player exits its elevated-draw mode. We MUST NOT re-emit
//!       `Close` in response to `Dismissed` (infinite feedback loop —
//!       see `src/home/event_source_modal.rs:293-297`).
//!
//! NOTE: this module imports `VideoPlayerState`, `VideoVolumeState`,
//! `VideoUiState`, `VideoSummary`, the `Shared*State` aliases, and the
//! pure helpers (`apply_video_slider_drag`, `apply_volume_action`,
//! `DragPhase`, `VolumeAction`) from `crate::shared::video_message_player`.
//! Until that sibling module is (re)created, this file will not
//! compile — register it in `src/shared/mod.rs` only once both files
//! are present.

use makepad_widgets::*;
use std::path::PathBuf;

use crate::{
    event_preview::format_mmss,
    shared::robrix_video::{RobrixVideoRef, RobrixVideoWidgetExt},
    shared::video_message_player::{
        apply_video_slider_drag, apply_volume_action, DragPhase, SharedPlayerState, SharedUiState,
        SharedVolumeState, VideoPlaybackAction, VideoSummary, VolumeAction,
    },
};

// ============================================================================
// Live design — the inner card body. No scrim, no overlay draw-list:
// the outer `Modal { ... }` wrapper supplies those.
// ============================================================================

script_mod! {
    use mod.prelude.widgets.*
    use mod.widgets.*

    mod.widgets.VIDEO_MODAL_ICON_CLOSE      = crate_resource("self://resources/icons/close.svg")
    mod.widgets.VIDEO_MODAL_ICON_PLAY       = crate_resource("self://resources/icons/play.svg")
    mod.widgets.VIDEO_MODAL_ICON_PAUSE      = crate_resource("self://resources/icons/pause.svg")
    mod.widgets.VIDEO_MODAL_ICON_VOLUME_ON  = crate_resource("self://resources/icons/volume_on.svg")
    mod.widgets.VIDEO_MODAL_ICON_VOLUME_OFF = crate_resource("self://resources/icons/volume_off.svg")

    mod.widgets.VideoMessagePlayerModal =
        set_type_default() do #(VideoMessagePlayerModal::register_widget(vm))
    {
        ..mod.widgets.RoundedView

        // The card body. Sized to fit the outer Modal's `Fill, Fill,
        // align: Center` content slot with a viewport margin — the same
        // sizing approach EventSourceModal uses.
        width: Fill { max: 1600 }
        height: Fill { max: 1000 }
        margin: 40
        flow: Overlay
        align: Align{x: 0.5, y: 0.5}

        show_bg: true
        // Transparent fill that still absorbs hits so a click on the
        // video area does not dismiss the outer Modal.
        draw_bg +: {
            color: #00000000
            border_radius: 6.0
            border_size: 0.0
        }

        // The card rect is reserved by this widget; the inline
        // `VideoMessagePlayer` reads it from `ui_state.card_rect` and
        // promotes its `Video` surface draw into it via its own
        // elevated `DrawList2d`.
        robrix_video := RobrixVideo {
            width: Fill
            height: Fill
        }

        controls := View {
            width: Fill
            height: Fill
            flow: Overlay
            padding: 8

            close_button := Button {
                width: 36
                height: 36
                margin: Inset{ left: 99999 }      // right-align
                text: ""
                spacing: 0
                padding: 0
                align: Align{x: 0.5, y: 0.5}
                icon_walk: Walk{width: 18, height: 18}
                draw_icon +: {
                    svg: (mod.widgets.VIDEO_MODAL_ICON_CLOSE)
                    color: #xffffff
                }
                draw_bg +: {
                    border_radius: 5.0
                    color: #x111827
                    color_hover: #x374151
                    color_down: #x111827
                }
            }

            center_controls := View {
                width: Fill
                height: Fill
                align: Align{x: 0.5, y: 0.5}

                play_button := Button {
                    width: 54
                    height: 54
                    text: ""
                    spacing: 0
                    padding: 0
                    align: Align{x: 0.5, y: 0.5}
                    icon_walk: Walk{width: 22, height: 22, margin: Inset{left: 3}}
                    draw_icon +: {
                        svg: (mod.widgets.VIDEO_MODAL_ICON_PLAY)
                        color: #xffffff
                    }
                    draw_bg +: {
                        border_radius: 7.0
                        color: #x111827
                        color_hover: #x374151
                        color_down: #x111827
                    }
                }

                pause_button := Button {
                    width: 54
                    height: 54
                    visible: false
                    text: ""
                    spacing: 0
                    padding: 0
                    align: Align{x: 0.5, y: 0.5}
                    icon_walk: Walk{width: 20, height: 22}
                    draw_icon +: {
                        svg: (mod.widgets.VIDEO_MODAL_ICON_PAUSE)
                        color: #xffffff
                    }
                    draw_bg +: {
                        border_radius: 7.0
                        color: #x111827
                        color_hover: #x374151
                        color_down: #x111827
                    }
                }
            }

            mute_button := Button {
                width: 36
                height: 36
                margin: Inset{ left: 99999, top: 99999 }   // bottom-right
                text: ""
                spacing: 0
                padding: 0
                align: Align{x: 0.5, y: 0.5}
                icon_walk: Walk{width: 18, height: 18}
                draw_icon +: {
                    svg: (mod.widgets.VIDEO_MODAL_ICON_VOLUME_ON)
                    color: #xffffff
                }
                draw_bg +: {
                    border_radius: 5.0
                    color: #x111827
                    color_hover: #x374151
                    color_down: #x111827
                }
            }

            slider_row := View {
                width: Fill
                height: Fit
                flow: Right
                spacing: 8
                padding: Inset{top: 4, bottom: 4, left: 8, right: 8}
                margin: Inset{ top: 99999 }                 // bottom strip
                align: Align{y: 0.5}
                show_bg: true
                draw_bg +: {
                    color: #x111827
                    border_radius: 5.0
                }
                elapsed_label := Label {
                    width: 46
                    height: Fit
                    text: "00:00"
                    draw_text +: { color: #xffffff }
                }
                slider := SliderMinimal {
                    width: Fill
                    height: 20
                    min: 0.0
                    max: 1.0
                    step: 0.0
                    default: 0.0
                    precision: 2
                    hover_actions_enabled: false
                    text_input: TextInput { visible: false, width: 0, height: 0 }
                }
                total_label := Label {
                    width: 46
                    height: Fit
                    text: "00:00"
                    draw_text +: { color: #xffffff }
                }
            }
        }
    }
}

// ============================================================================
// Actions
// ============================================================================

/// Actions emitted by / received around `VideoMessagePlayerModal`.
///
/// The shape mirrors `EventSourceModalAction` so the host (`RoomScreen`)
/// can wire it up the same way that `app.rs` wires `EventSourceModal`.
#[derive(Clone, Debug)]
pub enum VideoMessagePlayerModalAction {
    /// Emitted by the inline `VideoMessagePlayer`'s `maximise_button`
    /// handler. The host calls `inner.show(cx, ...)` with this payload
    /// and then opens the outer `Modal`.
    Open {
        inline_uid: WidgetUid,
        source_url: PathBuf,
        blurhash: Option<String>,
        summary: VideoSummary,
        position_ms: u64,
    },
    /// Emitted by this widget when its `close_button` is clicked. The
    /// host receives this and calls `outer.close(cx)`. NOT emitted in
    /// response to `ModalAction::Dismissed` (to avoid an action loop).
    Close,
}

/// Relay action used by the video maximise/close flow to ask the
/// `App`-level handler (which owns the `main_window` `WindowRef`) to
/// toggle OS fullscreen. `RoomScreen` cannot call `Window::fullscreen`
/// directly because `main_window` lives above `RoomScreen` in the
/// widget tree; this action bridges that gap.
#[derive(Clone, Debug)]
pub enum WindowFullscreenAction {
    /// Maps to `self.ui.window(cx, ids!(main_window)).fullscreen(cx)`.
    Enable,
    /// Maps to `self.ui.window(cx, ids!(main_window)).disable_fullscreen(cx)`.
    Disable,
}

// ============================================================================
// Widget
// ============================================================================

#[derive(Script, ScriptHook, Widget)]
pub struct VideoMessagePlayerModal {
    #[deref]
    view: View,

    // Shared state handles — populated by `show`.
    #[rust]
    player_state: Option<SharedPlayerState>,
    #[rust]
    volume_state: Option<SharedVolumeState>,
    #[rust]
    ui_state: Option<SharedUiState>,
    #[rust]
    summary: Option<VideoSummary>,
    #[rust]
    source_url: Option<PathBuf>,
    #[rust]
    origin_inline_uid: Option<WidgetUid>,

    /// Snapshot of `player_state.playing` taken at the start of a slider
    /// drag so the drag-end resume rule sees the pre-drag value rather
    /// than the mid-drag `playing = false`.
    #[rust]
    slider_drag_was_playing: Option<bool>,
}

impl Widget for VideoMessagePlayerModal {
    fn handle_event(&mut self, cx: &mut Cx, event: &Event, scope: &mut Scope) {
        self.view.handle_event(cx, event, scope);
        self.widget_match_event(cx, event, scope);
    }

    fn draw_walk(&mut self, cx: &mut Cx2d, scope: &mut Scope, walk: Walk) -> DrawStep {
        // Refresh visible state from shared handles before walking the view.
        self.sync_controls_from_state(cx);

        let step = self.view.draw_walk(cx, scope, walk);

        // Publish the card's screen rect so the inline `VideoMessagePlayer`
        // can promote its surface draw into this rect on its next pass.
        if let Some(ui) = &self.ui_state {
            if let Ok(mut guard) = ui.lock() {
                guard.card_rect = Some(self.view.area().rect(cx));
            }
        }

        step
    }
}

impl WidgetMatchEvent for VideoMessagePlayerModal {
    fn handle_actions(&mut self, cx: &mut Cx, actions: &Actions, _scope: &mut Scope) {
        for action in actions {
            if let Some(VideoPlaybackAction::ActiveTrackChanged { now_playing }) =
                action.downcast_ref::<VideoPlaybackAction>()
            {
                if self
                    .origin_inline_uid
                    .is_some_and(|origin_uid| origin_uid != *now_playing)
                {
                    self.robrix_video_ref(cx).pause_playback(cx);
                    self.set_playing(cx, false);
                }
            }
        }

        // ---- Outer `Modal` dismissed (scrim / Escape / back-press) ----
        //
        // The outer Modal already self-closes; we just need to flip
        // ui_state.maximised so the inline player exits elevated-draw
        // mode. We MUST NOT emit `Close` here (infinite feedback loop).
        let dismissed_by_outer = actions
            .iter()
            .any(|a| matches!(a.downcast_ref(), Some(ModalAction::Dismissed)));
        if dismissed_by_outer {
            self.set_maximised(false);
            return;
        }

        // ---- Close button: emit `Close` and flip ui_state.maximised. ----
        let close_button = self.view.button(cx, ids!(controls.close_button));
        if close_button.clicked(actions) {
            self.set_maximised(false);
            cx.action(VideoMessagePlayerModalAction::Close);
            return;
        }

        // ---- Play / Pause: flip shared `player_state.playing`. ----
        let play = self
            .view
            .button(cx, ids!(controls.center_controls.play_button));
        let pause = self
            .view
            .button(cx, ids!(controls.center_controls.pause_button));
        if play.clicked(actions) || pause.clicked(actions) {
            let playing = self
                .player_state
                .as_ref()
                .and_then(|state| state.lock().ok().map(|guard| guard.playing))
                .unwrap_or(false);
            if playing {
                self.robrix_video_ref(cx).pause_playback(cx);
            } else {
                self.robrix_video_ref(cx).begin_playback(cx);
            }
            self.set_playing(cx, !playing);
            self.sync_controls_from_state(cx);
        }

        // ---- Mute: apply VolumeAction::Mute / Unmute to shared state. ----
        if self
            .view
            .button(cx, ids!(controls.mute_button))
            .clicked(actions)
        {
            if let Some(volume) = &self.volume_state {
                if let Ok(mut guard) = volume.lock() {
                    let action = if guard.muted {
                        VolumeAction::Unmute
                    } else {
                        VolumeAction::Mute
                    };
                    apply_volume_action(&mut guard, action);
                    if guard.muted {
                        self.robrix_video_ref(cx).mute_playback(cx);
                    } else {
                        self.robrix_video_ref(cx).unmute_playback(cx);
                    }
                }
            }
            self.sync_controls_from_state(cx);
        }

        // ---- Slider drag: route to apply_video_slider_drag. ----
        let slider = self.view.slider(cx, ids!(controls.slider_row.slider));
        if let Some(action) = actions.find_widget_action(slider.widget_uid()) {
            let value_now = slider.value().unwrap_or(0.0);
            let total_ms = self.total_ms();
            if let Some(state) = &self.player_state {
                if let Ok(mut guard) = state.lock() {
                    match action.cast() {
                        SliderAction::StartSlide => {
                            let was_playing = guard.playing;
                            self.slider_drag_was_playing = Some(was_playing);
                            apply_video_slider_drag(
                                &mut guard,
                                value_now,
                                total_ms,
                                DragPhase::Start { was_playing },
                            );
                        }
                        SliderAction::Slide(v) | SliderAction::TextSlide(v) => {
                            apply_video_slider_drag(&mut guard, v, total_ms, DragPhase::Move);
                        }
                        SliderAction::EndSlide(v) => {
                            let was = self.slider_drag_was_playing.take().unwrap_or(false);
                            apply_video_slider_drag(
                                &mut guard,
                                v,
                                total_ms,
                                DragPhase::End { was_playing: was },
                            );
                        }
                        _ => {}
                    }
                }
            }
            self.sync_controls_from_state(cx);
        }
    }
}

// ============================================================================
// Public methods (entry point + helpers)
// ============================================================================

impl VideoMessagePlayerModal {
    /// Populate the modal with the originating `VideoMessagePlayer`'s
    /// shared state handles plus its `VideoSummary`. Called by the host
    /// (`RoomScreen`) when it receives `VideoMessagePlayerModalAction::Open`
    /// — same lifecycle as `EventSourceModal::show`.
    pub fn show(
        &mut self,
        cx: &mut Cx,
        origin_inline_uid: WidgetUid,
        source_url: PathBuf,
        blurhash: Option<String>,
        summary: VideoSummary,
    ) {
        self.origin_inline_uid = Some(origin_inline_uid);
        self.source_url = Some(source_url.clone());
        self.player_state = Some(Default::default());
        self.volume_state = Some(Default::default());
        self.ui_state = Some(Default::default());
        self.summary = Some(summary);
        self.slider_drag_was_playing = None;
        self.robrix_video_ref(cx).set_blurhash(cx, blurhash);
        self.robrix_video_ref(cx).set_source_url(cx, source_url);

        self.view
            .button(cx, ids!(controls.close_button))
            .reset_hover(cx);
        self.view
            .button(cx, ids!(controls.center_controls.play_button))
            .reset_hover(cx);
        self.view
            .button(cx, ids!(controls.center_controls.pause_button))
            .reset_hover(cx);
        self.view
            .button(cx, ids!(controls.mute_button))
            .reset_hover(cx);

        self.sync_controls_from_state(cx);
        self.view.redraw(cx);
    }

    pub fn robrix_video_ref(&self, cx: &mut Cx) -> RobrixVideoRef {
        self.view.robrix_video(cx, ids!(robrix_video))
    }

    pub fn set_playing(&mut self, cx: &mut Cx, playing: bool) {
        if let Some(state) = &self.player_state {
            if let Ok(mut guard) = state.lock() {
                guard.playing = playing;
            }
        }
        self.sync_controls_from_state(cx);
    }

    fn set_maximised(&self, value: bool) {
        if let Some(ui) = &self.ui_state {
            if let Ok(mut guard) = ui.lock() {
                guard.maximised = value;
                if !value {
                    guard.card_rect = None;
                }
            }
        }
    }

    fn total_ms(&self) -> u64 {
        self.summary
            .as_ref()
            .and_then(|s| s.duration_secs)
            .map(|secs| (secs.max(0.0) * 1000.0).round() as u64)
            .unwrap_or(0)
    }

    fn sync_controls_from_state(&mut self, cx: &mut Cx) {
        let playing = self
            .player_state
            .as_ref()
            .and_then(|s| s.lock().ok().map(|g| g.playing))
            .unwrap_or(false);
        let _muted = self
            .volume_state
            .as_ref()
            .and_then(|v| v.lock().ok().map(|g| g.muted))
            .unwrap_or(false);
        let position_ms = self
            .player_state
            .as_ref()
            .and_then(|s| s.lock().ok().map(|g| g.position_ms))
            .unwrap_or(0);
        let total_ms = self.total_ms();

        self.view
            .button(cx, ids!(controls.center_controls.play_button))
            .set_visible(cx, !playing);
        self.view
            .button(cx, ids!(controls.center_controls.pause_button))
            .set_visible(cx, playing);

        // The mute button's icon-swap mechanism (volume_on.svg ↔
        // volume_off.svg) is widget-specific — wire to whatever approach
        // the inline player ends up using once that module is in place.
        // (`_muted` is consumed here to keep the read deterministic.)

        let normalized = if total_ms == 0 {
            0.0
        } else {
            (position_ms as f64 / total_ms as f64).clamp(0.0, 1.0)
        };
        self.view
            .slider(cx, ids!(controls.slider_row.slider))
            .set_value(cx, normalized);
        self.view
            .label(cx, ids!(controls.slider_row.elapsed_label))
            .set_text(cx, &format_mmss(position_ms as f64 / 1000.0));
        self.view
            .label(cx, ids!(controls.slider_row.total_label))
            .set_text(cx, &format_mmss(total_ms as f64 / 1000.0));
    }
}

// ============================================================================
// Ref API — mirrors `EventSourceModalRef::show`.
// ============================================================================

impl VideoMessagePlayerModalRef {
    pub fn show(
        &self,
        cx: &mut Cx,
        origin_inline_uid: WidgetUid,
        source_url: PathBuf,
        blurhash: Option<String>,
        summary: VideoSummary,
    ) {
        let Some(mut inner) = self.borrow_mut() else {
            return;
        };
        inner.show(cx, origin_inline_uid, source_url, blurhash, summary);
    }

    pub fn robrix_video(&self, cx: &mut Cx) -> RobrixVideoRef {
        self.borrow()
            .map(|inner| inner.robrix_video_ref(cx))
            .unwrap_or_default()
    }

    pub fn set_playing(&self, cx: &mut Cx, playing: bool) {
        if let Some(mut inner) = self.borrow_mut() {
            inner.set_playing(cx, playing);
        }
    }
}

// ============================================================================
// Module wiring
// ============================================================================
//
// `pub fn script_mod(vm: &mut ScriptVm)` is generated by the
// `script_mod! { ... }` macro at the top of this file. In
// `src/shared/mod.rs`, call it AFTER `video_message_player::script_mod(vm)`:
//
// ```ignore
// video_message_player::script_mod(vm);
// video_message_player_modal::script_mod(vm);
// ```
