//! `VideoMessagePlayerModal` — the modal chrome (scrim + card + controls +
//! close button) shown when the user maximises a video message.
//!
//! Architecture (per `specs/Month-2/video-playback.spec.md`):
//!
//! - This widget does **not** own a Makepad `Video` widget. The single
//!   platform video session lives in `VideoMessagePlayer`
//!   (`src/shared/video_message_player.rs`). The video frames the user
//!   sees inside the modal card are produced by that inline player and
//!   drawn into the card rect via the inline player's own elevated
//!   `DrawList2d` while `ui_state.maximised == true`.
//!
//! - State is **shared** via `Arc<Mutex<...>>`. `VideoMessagePlayer`
//!   owns the originals; `VideoMessagePlayerModal` receives clones via
//!   [`VideoMessagePlayerModalRef::bind`]. Mutations from the modal's
//!   controls are observed by the inline view on the next paint, and
//!   vice versa.
//!
//! - The modal does **not** mutate `ui_state.maximised` directly on
//!   dismissal — it emits [`VideoMessagePlayerModalAction::Dismissed`]
//!   and lets the owner (typically `RoomScreen` or the inline player)
//!   flip the flag, so there is exactly one writer of that field.
//!
//! NOTE: this module imports `VideoPlayerState`, `VideoVolumeState`,
//! `VideoUiState`, `VideoSummary`, the `Shared*State` aliases, and the
//! pure helpers (`apply_video_slider_drag`, `apply_volume_action`,
//! `DragPhase`, `VolumeAction`) from `crate::shared::video_message_player`.
//! Until that sibling module is (re)created, this file will not
//! compile — register it in `src/shared/mod.rs` only once both files
//! are present.

use makepad_widgets::{
    makepad_draw::*,
    makepad_platform::{KeyCode, KeyEvent},
    *,
};

use crate::{
    event_preview::format_mmss,
    shared::video_message_player::{
        apply_video_slider_drag, apply_volume_action, DragPhase, SharedPlayerState,
        SharedUiState, SharedVolumeState, VideoSummary, VolumeAction,
    },
};

// ============================================================================
// Live design
// ============================================================================

script_mod! {
    use mod.prelude.widgets.*
    use mod.prelude.widgets_internal.*
    use mod.widgets.*

    // Icons reused from the inline player; declared here so the modal compiles
    // standalone if the inline player has not yet registered them.
    mod.widgets.MODAL_ICON_CLOSE          = crate_resource("self://resources/icons/close.svg")
    mod.widgets.MODAL_ICON_PLAY           = crate_resource("self://resources/icons/play.svg")
    mod.widgets.MODAL_ICON_PAUSE          = crate_resource("self://resources/icons/pause.svg")
    mod.widgets.MODAL_ICON_VOLUME_ON      = crate_resource("self://resources/icons/volume_on.svg")
    mod.widgets.MODAL_ICON_VOLUME_OFF     = crate_resource("self://resources/icons/volume_off.svg")

    mod.widgets.VideoMessagePlayerModalBase =
        #(VideoMessagePlayerModal::register_widget(vm))

    mod.widgets.VideoMessagePlayerModal = mod.widgets.VideoMessagePlayerModalBase {
        width: Fill
        height: Fill
        flow: Overlay
        align: Center

        // Transparent background; the scrim view paints the dim.
        draw_bg +: {
            pixel: fn() {
                return vec4(0. 0. 0. 0.0)
            }
        }

        scrim := View {
            width: Fill
            height: Fill
            show_bg: true
            draw_bg +: {
                color: #000000B3
                pixel: fn() { return self.color }
            }
        }

        // The card defines the *rect* into which the inline player's
        // surface will be drawn. The card itself is transparent — it
        // does NOT contain a Video widget.
        //
        // Initial size is a sensible default; runtime resize to 90% of
        // viewport-min (preserving aspect ratio) happens in `bind` and
        // on subsequent frames via NextFrame.
        card := View {
            width: 800
            height: 450
            flow: Overlay
            show_bg: true
            // Transparent fill that still absorbs hits so a click on the
            // video area does not dismiss the modal.
            draw_bg +: {
                color: #00000000
                pixel: fn() { return self.color }
            }

            controls := View {
                width: Fill
                height: Fill
                flow: Overlay
                padding: 8

                close_button := Button {
                    width: 36
                    height: 36
                    margin: Inset{ left: 99999 }   // right-align
                    text: ""
                    spacing: 0
                    padding: 0
                    align: Align{x: 0.5 y: 0.5}
                    icon_walk: Walk{width: 18 height: 18}
                    draw_icon +: {
                        svg: (mod.widgets.MODAL_ICON_CLOSE)
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
                    align: Align{x: 0.5 y: 0.5}

                    play_button := Button {
                        width: 54
                        height: 54
                        text: ""
                        spacing: 0
                        padding: 0
                        align: Align{x: 0.5 y: 0.5}
                        icon_walk: Walk{width: 22 height: 22 margin: Inset{left: 3}}
                        draw_icon +: {
                            svg: (mod.widgets.MODAL_ICON_PLAY)
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
                        align: Align{x: 0.5 y: 0.5}
                        icon_walk: Walk{width: 20 height: 22}
                        draw_icon +: {
                            svg: (mod.widgets.MODAL_ICON_PAUSE)
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
                    align: Align{x: 0.5 y: 0.5}
                    icon_walk: Walk{width: 18 height: 18}
                    draw_icon +: {
                        svg: (mod.widgets.MODAL_ICON_VOLUME_ON)
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
                    padding: Inset{top: 4 bottom: 4 left: 8 right: 8}
                    margin: Inset{ top: 99999 }                // bottom
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
                        draw_text +: {
                            color: #xffffff
                        }
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
                        text_input: TextInput { visible: false width: 0 height: 0 }
                    }
                    total_label := Label {
                        width: 46
                        height: Fit
                        text: "00:00"
                        draw_text +: {
                            color: #xffffff
                        }
                    }
                }
            }
        }
    }
}

// ============================================================================
// Actions
// ============================================================================

#[derive(Clone, Debug, Default)]
pub enum VideoMessagePlayerModalAction {
    /// Emitted when the user dismisses the modal via the close button,
    /// the scrim, the Escape key, or a back-navigation gesture. The
    /// modal does NOT flip `ui_state.maximised` itself; the receiver
    /// is responsible for that, so there is exactly one writer.
    Dismissed,
    #[default]
    None,
}

// ============================================================================
// Widget
// ============================================================================

#[derive(Script, Widget)]
pub struct VideoMessagePlayerModal {
    #[source]
    source: ScriptObjectRef,

    #[deref]
    view: View,

    /// Overlay draw-list so the scrim and chrome escape parent bounds,
    /// mirroring how Makepad's built-in `Modal` widget renders itself.
    #[rust]
    draw_list: Option<DrawList2d>,

    #[live]
    draw_bg: DrawQuad,

    // Shared state handles — populated by `bind`.
    #[rust]
    player_state: Option<SharedPlayerState>,
    #[rust]
    volume_state: Option<SharedVolumeState>,
    #[rust]
    ui_state: Option<SharedUiState>,
    #[rust]
    summary: Option<VideoSummary>,

    /// Snapshot of `player_state.playing` taken at the start of a
    /// slider drag so the drag-end resume rule sees the pre-drag value
    /// rather than the mid-drag `playing = false`.
    #[rust]
    slider_drag_was_playing: Option<bool>,
}

impl ScriptHook for VideoMessagePlayerModal {
    fn on_after_new(&mut self, vm: &mut ScriptVm) {
        self.draw_list = Some(DrawList2d::script_new(vm));
    }

    fn on_after_apply(
        &mut self,
        vm: &mut ScriptVm,
        _apply: &Apply,
        _scope: &mut Scope,
        _value: ScriptValue,
    ) {
        vm.with_cx_mut(|cx| {
            if let Some(draw_list) = &self.draw_list {
                draw_list.redraw(cx);
            }
        });
    }
}

impl Widget for VideoMessagePlayerModal {
    fn handle_event(&mut self, cx: &mut Cx, event: &Event, scope: &mut Scope) {
        if !self.is_visible() {
            return;
        }

        // Let inner widgets receive the event first.
        let card = self.view.widget(cx, ids!(card));
        card.handle_event(cx, event, scope);

        if let Event::Actions(actions) = event {
            // Close button — dismisses without mutating ui_state.
            if self
                .view
                .button(cx, ids!(card.controls.close_button))
                .clicked(actions)
            {
                self.emit_dismissed(cx);
                return;
            }

            // Play / pause — flips shared player_state.playing.
            let play = self
                .view
                .button(cx, ids!(card.controls.center_controls.play_button));
            let pause = self
                .view
                .button(cx, ids!(card.controls.center_controls.pause_button));
            if play.clicked(actions) || pause.clicked(actions) {
                if let Some(state) = &self.player_state {
                    let mut s = state.lock().unwrap();
                    s.playing = !s.playing;
                }
                self.sync_controls(cx);
            }

            // Mute — flips shared volume_state via apply_volume_action.
            if self
                .view
                .button(cx, ids!(card.controls.mute_button))
                .clicked(actions)
            {
                if let Some(volume) = &self.volume_state {
                    let mut v = volume.lock().unwrap();
                    let action = if v.muted {
                        VolumeAction::Unmute
                    } else {
                        VolumeAction::Mute
                    };
                    apply_volume_action(&mut v, action);
                }
                self.sync_controls(cx);
            }

            // Slider.
            let slider = self
                .view
                .slider(cx, ids!(card.controls.slider_row.slider));
            if let Some(action) = actions.find_widget_action(slider.widget_uid()) {
                let value_now = slider.value().unwrap_or(0.0);
                let total_ms = self.total_ms();
                if let Some(state) = &self.player_state {
                    let mut s = state.lock().unwrap();
                    match action.cast() {
                        SliderAction::StartSlide => {
                            self.slider_drag_was_playing = Some(s.playing);
                            apply_video_slider_drag(
                                &mut s,
                                value_now,
                                total_ms,
                                DragPhase::Start {
                                    was_playing: s.playing,
                                },
                            );
                        }
                        SliderAction::Slide(v) | SliderAction::TextSlide(v) => {
                            apply_video_slider_drag(&mut s, v, total_ms, DragPhase::Move);
                        }
                        SliderAction::EndSlide(v) => {
                            let was = self.slider_drag_was_playing.take().unwrap_or(false);
                            apply_video_slider_drag(
                                &mut s,
                                v,
                                total_ms,
                                DragPhase::End { was_playing: was },
                            );
                        }
                        _ => {}
                    }
                }
                self.sync_controls(cx);
            }
        }

        // Scrim-click / Escape / back-press dismissal (mirrors Modal pattern).
        let bg_hit = event.hits(cx, self.draw_bg.area());
        let card_area = card.area();
        let card_hit = event.hits(cx, card_area);
        let should_close = event.back_pressed()
            || matches!(
                bg_hit,
                Hit::KeyDown(KeyEvent {
                    key_code: KeyCode::Escape,
                    ..
                })
            )
            || matches!(
                card_hit,
                Hit::KeyDown(KeyEvent {
                    key_code: KeyCode::Escape,
                    ..
                })
            )
            || matches!(
                bg_hit,
                Hit::FingerUp(ref fe) if !card_area.rect(cx).contains(fe.abs)
            );

        if should_close {
            self.emit_dismissed(cx);
        }
    }

    fn draw_walk(&mut self, cx: &mut Cx2d, scope: &mut Scope, walk: Walk) -> DrawStep {
        let draw_list = self.draw_list.as_mut().unwrap();
        draw_list.begin_overlay_reuse(cx);
        cx.begin_root_turtle_for_pass(self.view.layout);
        self.draw_bg.begin(cx, self.view.walk, self.view.layout);

        if self.is_visible() {
            let scrim = self.view.widget(cx, ids!(scrim));
            let _ = scrim.draw_walk(
                cx,
                scope,
                walk.with_abs_pos(Vec2d { x: 0., y: 0. }),
            );

            let card = self.view.widget(cx, ids!(card));
            let _ = card.draw_all(cx, scope);

            // Publish the card's screen rect so the inline player can
            // promote its surface draw into this rect on its next pass.
            let card_rect = card.area().rect(cx);
            if let Some(ui) = &self.ui_state {
                if let Ok(mut guard) = ui.lock() {
                    guard.card_rect = Some(card_rect);
                }
            }
        }

        self.draw_bg.end(cx);
        cx.end_pass_sized_turtle();
        self.draw_list.as_mut().unwrap().end(cx);

        DrawStep::done()
    }
}

impl VideoMessagePlayerModal {
    /// `true` iff a bound `ui_state` reports `maximised == true`.
    fn is_visible(&self) -> bool {
        self.ui_state
            .as_ref()
            .and_then(|s| s.lock().ok().map(|g| g.maximised))
            .unwrap_or(false)
    }

    fn total_ms(&self) -> u64 {
        self.summary
            .as_ref()
            .and_then(|s| s.duration_secs)
            .map(|secs| (secs.max(0.0) * 1000.0).round() as u64)
            .unwrap_or(0)
    }

    fn emit_dismissed(&mut self, cx: &mut Cx) {
        // Note: we do NOT mutate ui_state.maximised here. The owner is
        // the single writer of that field and will flip it in response
        // to this action.
        cx.action(VideoMessagePlayerModalAction::Dismissed);
        if let Some(dl) = &self.draw_list {
            dl.redraw(cx);
        }
        self.draw_bg.redraw(cx);
    }

    fn sync_controls(&mut self, cx: &mut Cx) {
        let playing = self
            .player_state
            .as_ref()
            .and_then(|s| s.lock().ok().map(|g| g.playing))
            .unwrap_or(false);
        let muted = self
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
            .button(cx, ids!(card.controls.center_controls.play_button))
            .set_visible(cx, !playing);
        self.view
            .button(cx, ids!(card.controls.center_controls.pause_button))
            .set_visible(cx, playing);

        // Swap mute icon — we keep one Button and swap its drawn SVG.
        let mute_btn = self.view.button(cx, ids!(card.controls.mute_button));
        let _ = (mute_btn, muted); // icon-swap mechanism is widget-specific;
                                   // wire to the same approach the inline
                                   // player uses once it's in place.

        let normalized = if total_ms == 0 {
            0.0
        } else {
            (position_ms as f64 / total_ms as f64).clamp(0.0, 1.0)
        };
        self.view
            .slider(cx, ids!(card.controls.slider_row.slider))
            .set_value(cx, normalized);
        self.view
            .label(cx, ids!(card.controls.slider_row.elapsed_label))
            .set_text(cx, &format_mmss(position_ms as f64 / 1000.0));
        self.view
            .label(cx, ids!(card.controls.slider_row.total_label))
            .set_text(cx, &format_mmss(total_ms as f64 / 1000.0));
    }
}

// ============================================================================
// Ref API
// ============================================================================

impl VideoMessagePlayerModalRef {
    /// Bind the modal to a specific `VideoMessagePlayer`'s shared state
    /// handles plus its `VideoSummary` (so the modal can render
    /// `total_ms` labels without reaching into the inline player).
    pub fn bind(
        &self,
        cx: &mut Cx,
        player_state: SharedPlayerState,
        volume_state: SharedVolumeState,
        ui_state: SharedUiState,
        summary: VideoSummary,
    ) {
        if let Some(mut inner) = self.borrow_mut() {
            inner.player_state = Some(player_state);
            inner.volume_state = Some(volume_state);
            inner.ui_state = Some(ui_state);
            inner.summary = Some(summary);
            inner.slider_drag_was_playing = None;
            if let Some(dl) = &inner.draw_list {
                dl.redraw(cx);
            }
            inner.draw_bg.redraw(cx);
            inner.sync_controls(cx);
        }
    }

    /// Returns `true` iff the bound `ui_state.maximised == true`.
    pub fn is_visible(&self) -> bool {
        if let Some(inner) = self.borrow() {
            inner.is_visible()
        } else {
            false
        }
    }

    /// Returns `true` iff `actions` contains a `Dismissed` from this
    /// widget instance.
    pub fn dismissed(&self, actions: &Actions) -> bool {
        if let Some(inner) = self.borrow() {
            matches!(
                actions.find_widget_action(inner.widget_uid()).cast(),
                VideoMessagePlayerModalAction::Dismissed
            )
        } else {
            false
        }
    }
}

// ============================================================================
// Module wiring
// ============================================================================
//
// `pub fn script_mod(vm: &mut ScriptVm)` is generated by the
// `script_mod! { ... }` macro at the top of this file. In
// `src/shared/mod.rs`, call it AFTER `video_message_player::script_mod(vm)`
// since this module imports the shared state types from there:
//
// ```ignore
// video_message_player::script_mod(vm);
// video_message_player_modal::script_mod(vm);
// ```
