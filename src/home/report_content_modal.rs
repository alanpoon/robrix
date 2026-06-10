//! Modal dialog for reporting a message to the homeserver.

use makepad_widgets::*;
use matrix_sdk::ruma::{OwnedEventId, OwnedRoomId, OwnedUserId};

script_mod! {
    use mod.prelude.widgets.*
    use mod.widgets.*

    mod.widgets.ReportContentModalLabel = Label {
        width: Fill
        height: Fit
        draw_text +: {
            text_style: REGULAR_TEXT { font_size: 10.5 }
            color: #333
        }
        text: ""
    }

    mod.widgets.ReportContentModal = #(ReportContentModal::register_widget(vm)) {
        width: Fit
        height: Fit

        RoundedView {
            width: 430
            height: Fit
            align: Align{x: 0.5}
            flow: Down
            padding: Inset{top: 26, right: 22, bottom: 18, left: 22}
            spacing: 14

            show_bg: true
            draw_bg +: {
                color: (COLOR_PRIMARY)
                border_radius: 6.0
            }

            title := Label {
                width: Fill
                height: Fit
                draw_text +: {
                    text_style: TITLE_TEXT { font_size: 13 }
                    color: #000
                }
                text: "Report Message"
            }

            body := mod.widgets.ReportContentModalLabel {
                text: "Report this message to your homeserver administrators. Please provide a reason."
            }

            reason_input := RobrixTextInput {
                width: Fill
                height: Fit
                padding: 10
                draw_text +: {
                    text_style: REGULAR_TEXT { font_size: 11.5 }
                    color: #000
                }
                empty_text: "Describe why you are reporting this message"
            }

            ignore_row := View {
                width: Fill
                height: Fit
                flow: Down
                spacing: 4

                ignore_checkbox := CheckBoxFlat {
                    text: "Ignore user"
                    active: false
                    draw_text +: {
                        color: (COLOR_TEXT)
                        color_hover: (COLOR_TEXT)
                        color_focus: (COLOR_TEXT)
                        color_down: (COLOR_TEXT)
                    }
                }

                ignore_label := mod.widgets.ReportContentModalLabel {
                    text: "Check if you want to hide all current and future messages from this user."
                }
            }

            status_label := Label {
                width: Fill
                height: Fit
                draw_text +: {
                    text_style: REGULAR_TEXT { font_size: 10.2 }
                    color: #000
                }
                text: ""
            }

            buttons := View {
                width: Fill
                height: Fit
                flow: Right
                align: Align{x: 1.0, y: 0.5}
                spacing: 16

                cancel_button := RobrixNeutralIconButton {
                    width: 110
                    align: Align{x: 0.5, y: 0.5}
                    padding: 12
                    draw_icon.svg: (ICON_FORBIDDEN)
                    icon_walk: Walk{width: 16, height: 16, margin: Inset{left: -2, right: -1}}
                    text: "Cancel"
                }

                report_button := RobrixNegativeIconButton {
                    width: 130
                    align: Align{x: 0.5, y: 0.5}
                    padding: 12
                    draw_icon.svg: (ICON_WARNING)
                    icon_walk: Walk{width: 16, height: 16, margin: Inset{left: -2, right: -1}}
                    text: "Report"
                }
            }
        }
    }
}

#[derive(Debug)]
pub enum ReportContentModalAction {
    Close,
    Submit {
        event_id: OwnedEventId,
        reason: String,
        /// `Some(user_id)` if the "Ignore user" checkbox was checked.
        ignore_sender: Option<OwnedUserId>,
    },
}

#[derive(Debug)]
pub enum ReportContentResultAction {
    Sent {
        room_id: OwnedRoomId,
        event_id: OwnedEventId,
    },
    Failed {
        room_id: OwnedRoomId,
        event_id: OwnedEventId,
        error: matrix_sdk::Error,
    },
}

#[derive(Script, ScriptHook, Widget)]
pub struct ReportContentModal {
    #[deref]
    view: View,
    #[rust]
    event_id: Option<OwnedEventId>,
    #[rust]
    sender_id: Option<OwnedUserId>,
    #[rust]
    is_showing_error: bool,
}

impl Widget for ReportContentModal {
    fn handle_event(&mut self, cx: &mut Cx, event: &Event, scope: &mut Scope) {
        self.view.handle_event(cx, event, scope);
        self.widget_match_event(cx, event, scope);
    }

    fn draw_walk(&mut self, cx: &mut Cx2d, scope: &mut Scope, walk: Walk) -> DrawStep {
        self.view.draw_walk(cx, scope, walk)
    }
}

impl WidgetMatchEvent for ReportContentModal {
    fn handle_actions(&mut self, cx: &mut Cx, actions: &Actions, _scope: &mut Scope) {
        let cancel_button = self.view.button(cx, ids!(buttons.cancel_button));
        let report_button = self.view.button(cx, ids!(buttons.report_button));
        let reason_input = self.view.text_input(cx, ids!(reason_input));
        let mut status_label = self.view.label(cx, ids!(status_label));

        if cancel_button.clicked(actions)
            || actions
                .iter()
                .any(|a| matches!(a.downcast_ref(), Some(ModalAction::Dismissed)))
        {
            cx.action(ReportContentModalAction::Close);
            return;
        }

        if self.is_showing_error && reason_input.changed(actions).is_some() {
            self.is_showing_error = false;
            status_label.set_text(cx, "");
            self.view.redraw(cx);
        }

        if report_button.clicked(actions) || reason_input.returned(actions).is_some() {
            let reason = reason_input.text().trim().to_string();
            if reason.is_empty() {
                self.is_showing_error = true;
                script_apply_eval!(cx, status_label, {
                    text: "Please enter a reason before reporting."
                    draw_text +: {
                        color: mod.widgets.COLOR_FG_DANGER_RED
                    }
                });
                self.view.redraw(cx);
                return;
            }
            let Some(event_id) = self.event_id.clone() else { return };
            let ignore_sender = if self.view.check_box(cx, ids!(ignore_row.ignore_checkbox)).active(cx) {
                self.sender_id.clone()
            } else {
                None
            };
            self.view.button(cx, ids!(buttons.report_button)).set_enabled(cx, false);
            self.view.button(cx, ids!(buttons.cancel_button)).set_enabled(cx, false);
            cx.action(ReportContentModalAction::Submit { event_id, reason, ignore_sender });
        }
    }
}

impl ReportContentModal {
    pub fn show(&mut self, cx: &mut Cx, event_id: OwnedEventId, sender_id: OwnedUserId) {
        self.event_id = Some(event_id);
        self.sender_id = Some(sender_id);
        self.is_showing_error = false;
        self.view.label(cx, ids!(status_label)).set_text(cx, "");
        self.view.text_input(cx, ids!(reason_input)).set_text(cx, "");
        self.view.check_box(cx, ids!(ignore_row.ignore_checkbox)).set_active(cx, false, Animate::No);
        self.view.button(cx, ids!(buttons.report_button)).set_enabled(cx, true);
        self.view.button(cx, ids!(buttons.cancel_button)).set_enabled(cx, true);
        self.view.button(cx, ids!(buttons.report_button)).reset_hover(cx);
        self.view.button(cx, ids!(buttons.cancel_button)).reset_hover(cx);
        self.view.redraw(cx);
    }
}

impl ReportContentModalRef {
    pub fn show(&self, cx: &mut Cx, event_id: OwnedEventId, sender_id: OwnedUserId) {
        let Some(mut inner) = self.borrow_mut() else { return };
        inner.show(cx, event_id, sender_id);
    }
}
