//! A sliding panel that shows the pinned messages for the currently-displayed room.

use makepad_widgets::*;
use matrix_sdk::ruma::{MilliSecondsSinceUnixEpoch, OwnedEventId, OwnedRoomId, OwnedUserId};

script_mod! {
    use mod.prelude.widgets.*
    use mod.widgets.*

    mod.widgets.PinnedMessagesPanel = #(PinnedMessagesPanel::register_widget(vm)) {
        visible: false,
        flow: Overlay,
        width: Fill,
        height: Fill,
        align: Align{x: 1.0, y: 0}

        bg_view := SolidView {
            width: Fill, height: Fill,
            visible: false,
            show_bg: true
            draw_bg.color: #000000BB
        }

        main_content := SolidView {
            width: 320,
            height: Fill,
            flow: Down,
            show_bg: true,
            draw_bg.color: (COLOR_PRIMARY)

            header := View {
                width: Fill, height: Fit,
                flow: Right,
                align: Align{y: 0.5},
                padding: Inset{top: 12, right: 10, bottom: 12, left: 15}

                title := Label {
                    width: Fill, height: Fit,
                    draw_text +: {
                        text_style: USERNAME_TEXT_STYLE { font_size: 12.5 }
                        color: #000
                    }
                    text: "Pinned Messages"
                }

                close_button := RobrixNeutralIconButton {
                    width: Fit, height: Fit,
                    spacing: 0, padding: 15,
                    draw_icon.svg: (ICON_CLOSE)
                    icon_walk: Walk{width: 14, height: 14}
                    text: ""
                }
            }

            content_scroll := ScrollYView {
                width: Fill, height: Fill,
                flow: Down,
                padding: Inset{left: 12, right: 12, top: 8, bottom: 12}

                empty_label := Label {
                    visible: false,
                    width: Fill, height: Fit,
                    draw_text +: { color: #888, text_style: REGULAR_TEXT { font_size: 11.0 } }
                    text: "No pinned messages in this room."
                }

                items_content := Label {
                    visible: false,
                    width: Fill, height: Fit,
                    draw_text +: {
                        wrap: Word,
                        color: #333,
                        text_style: REGULAR_TEXT { font_size: 10.5 }
                    }
                    text: ""
                }
            }
        }

        slide: 1.0,

        animator: Animator {
            panel: {
                default: @hide
                show: AnimatorState {
                    redraw: true,
                    from: {all: Forward {duration: 0.5}}
                    ease: Ease.ExpDecay {d1: 0.80, d2: 0.97}
                    apply: { slide: 0.0 }
                }
                hide: AnimatorState {
                    redraw: true,
                    from: {all: Forward {duration: 0.5}}
                    ease: Ease.ExpDecay {d1: 0.80, d2: 0.97}
                    apply: { slide: 1.0 }
                }
            }
        }
    }
}

/// Content of a single pinned event, fetched from the homeserver.
#[derive(Clone, Debug)]
pub struct PinnedEventContent {
    pub event_id: OwnedEventId,
    pub sender_id: OwnedUserId,
    pub display_name: Option<String>,
    pub body: String,
    pub timestamp: MilliSecondsSinceUnixEpoch,
}

/// Actions dispatched by the PinnedMessagesPanel widget or its fetch result.
#[derive(Clone, Default, Debug)]
pub enum PinnedMessagesPanelAction {
    Open,
    #[default]
    None,
}

/// Result of fetching pinned message content from the homeserver.
#[derive(Clone, Debug)]
pub enum PinnedMessagesFetchResult {
    Fetched { room_id: OwnedRoomId, items: Vec<PinnedEventContent> },
    Failed { room_id: OwnedRoomId, error: String },
}

#[derive(Script, ScriptHook, Widget, Animator)]
pub struct PinnedMessagesPanel {
    #[deref] view: View,
    #[source] source: ScriptObjectRef,
    #[apply_default] animator: Animator,
    #[live] slide: f32,
    #[rust] is_animating_out: bool,
    #[rust] items: Vec<PinnedEventContent>,
}

impl Widget for PinnedMessagesPanel {
    fn draw_walk(&mut self, cx: &mut Cx2d, scope: &mut Scope, walk: Walk) -> DrawStep {
        let panel_width = 320.0;
        let right_margin = -(self.slide * panel_width);
        let mut main_content = self.view(cx, ids!(main_content));
        script_apply_eval!(cx, main_content, { margin.right: #(right_margin) });
        let bg_alpha = (1.0 - self.slide) * 0.733;
        let bg_color = vec4(0.0, 0.0, 0.0, bg_alpha);
        let mut bg_view = self.view(cx, ids!(bg_view));
        script_apply_eval!(cx, bg_view, { draw_bg +: { color: #(bg_color) } });

        let has_items = !self.items.is_empty();
        self.view(cx, ids!(content_scroll.empty_label)).set_visible(cx, !has_items);
        self.view(cx, ids!(content_scroll.items_content)).set_visible(cx, has_items);

        if has_items {
            let text = self.items.iter().enumerate().map(|(i, item)| {
                let sender = item.display_name.as_deref()
                    .unwrap_or_else(|| item.sender_id.localpart());
                let ts = crate::utils::relative_format(item.timestamp)
                    .unwrap_or_default();
                let body = if item.body.chars().count() > 200 {
                    format!("{}…", item.body.chars().take(200).collect::<String>())
                } else {
                    item.body.clone()
                };
                if i == 0 {
                    format!("{sender}  {ts}\n{body}")
                } else {
                    format!("{sender}  {ts}\n{body}")
                }
            }).collect::<Vec<_>>().join("\n\n");
            self.label(cx, ids!(content_scroll.items_content)).set_text(cx, &text);
        }

        self.view.draw_walk(cx, scope, walk)
    }

    fn handle_event(&mut self, cx: &mut Cx, event: &Event, scope: &mut Scope) {
        self.view.handle_event(cx, event, scope);

        if !self.visible { return; }

        let animator_action = self.animator_handle_event(cx, event);
        if animator_action.must_redraw() {
            self.redraw(cx);
        }

        if self.is_animating_out && !self.animator.is_track_animating(id!(panel)) {
            self.visible = false;
            self.is_animating_out = false;
            cx.revert_key_focus();
            self.view(cx, ids!(bg_view)).set_visible(cx, false);
            self.redraw(cx);
            return;
        }

        let area = self.view.area();
        let close_pane = event.back_pressed()
            || match event.hits_with_capture_overload(cx, area, true) {
                Hit::KeyUp(key) => key.key_code == KeyCode::Escape,
                Hit::FingerDown(_) => {
                    cx.set_key_focus(area);
                    false
                }
                Hit::FingerUp(fue) if fue.is_over => {
                    fue.mouse_button().is_some_and(|b| b.is_back())
                    || !self.view(cx, ids!(main_content)).area().rect(cx).contains(fue.abs)
                }
                _ => false,
            };

        if let Event::Actions(actions) = event {
            if self.button(cx, ids!(close_button)).clicked(actions) {
                self.hide(cx);
                return;
            }
        }

        if close_pane {
            self.hide(cx);
        }
    }
}

impl PinnedMessagesPanel {
    pub fn show(&mut self, cx: &mut Cx, items: Vec<PinnedEventContent>) {
        self.items = items;
        self.visible = true;
        self.is_animating_out = false;
        cx.set_key_focus(self.view.area());
        self.animator_play(cx, ids!(panel.show));
        self.view(cx, ids!(bg_view)).set_visible(cx, true);
        self.view.button(cx, ids!(close_button)).reset_hover(cx);
        self.redraw(cx);
    }

    pub fn hide(&mut self, cx: &mut Cx) {
        if !self.visible { return; }
        self.is_animating_out = true;
        self.animator_play(cx, ids!(panel.hide));
        self.redraw(cx);
    }
}

impl PinnedMessagesPanelRef {
    pub fn show(&self, cx: &mut Cx, items: Vec<PinnedEventContent>) {
        let Some(mut inner) = self.borrow_mut() else { return };
        inner.show(cx, items);
    }

    pub fn hide(&self, cx: &mut Cx) {
        let Some(mut inner) = self.borrow_mut() else { return };
        inner.hide(cx);
    }
}
