//! A confirmation modal for destructive moderation actions
//! (kick/disinvite, ban, unban) on a user in a room.
//!
//! The modal asks the moderator to confirm and lets them enter an optional
//! reason string that is included in the Matrix kick/ban/unban request.

use makepad_widgets::*;
use matrix_sdk::ruma::{OwnedRoomId, OwnedUserId};

use crate::sliding_sync::{submit_async_request, MatrixRequest};

script_mod! {
    use mod.prelude.widgets.*
    use mod.widgets.*


    mod.widgets.ModerationActionModal = #(ModerationActionModal::register_widget(vm)) {
        width: Fit
        height: Fit

        RoundedView {
            flow: Down
            width: 420
            height: Fit
            padding: Inset{top: 30, right: 40, bottom: 20, left: 40}

            show_bg: true
            draw_bg.color: (COLOR_PRIMARY)
            draw_bg.border_radius: 4.0

            title_view := View {
                width: Fill,
                height: Fit,
                padding: Inset{top: 0, bottom: 15}
                align: Align{x: 0.5, y: 0.0}

                title := Label {
                    flow: Flow.Right{wrap: true},
                    draw_text +: {
                        text_style: TITLE_TEXT {font_size: 13},
                        color: #000
                    }
                }
            }

            body := View {
                width: Fill,
                height: Fit,
                flow: Down,
                spacing: 10,

                description := Label {
                    width: Fill
                    flow: Flow.Right{wrap: true}
                    draw_text +: {
                        text_style: REGULAR_TEXT {
                            font_size: 11.5,
                        },
                        color: #000
                    }
                }

                reason_label := Label {
                    width: Fill, height: Fit
                    margin: Inset{top: 10}
                    draw_text +: {
                        text_style: REGULAR_TEXT { font_size: 10.5 },
                        color: (MESSAGE_TEXT_COLOR)
                    }
                    text: "Reason (optional)"
                }

                reason_input := RobrixTextInput {
                    width: Fill,
                    empty_text: "e.g. spam, harassment"
                }

                View {
                    width: Fill, height: Fit
                    flow: Right,
                    padding: Inset{top: 20, bottom: 10}
                    align: Align{x: 1.0, y: 0.5}
                    spacing: 20

                    cancel_button := RobrixNeutralIconButton {
                        width: 120,
                        align: Align{x: 0.5, y: 0.5}
                        padding: 15,
                        draw_icon.svg: (ICON_CLOSE)
                        icon_walk: Walk{width: 14, height: 14, margin: Inset{left: -2, right: -1} }
                        text: "Cancel"
                    }

                    confirm_button := RobrixNegativeIconButton {
                        width: 140,
                        align: Align{x: 0.5, y: 0.5}
                        padding: 15,
                        draw_icon.svg: (ICON_FORBIDDEN)
                        icon_walk: Walk{width: 16, height: 16, margin: Inset{left: -2, right: -1} }
                        text: "Confirm"
                    }
                }
            }
        }
    }
}

/// Which moderation action this modal is currently confirming.
#[derive(Clone, Debug)]
pub enum ModerationActionKind {
    /// Forcibly remove a member from the room.
    /// `is_invite` distinguishes a pending invitation (label: "Disinvite")
    /// from a joined member (label: "Kick").
    Kick {
        room_id: OwnedRoomId,
        user_id: OwnedUserId,
        user_display_name: String,
        room_name: String,
        is_invite: bool,
    },
    /// Ban a user from the room. They cannot rejoin until unbanned.
    Ban {
        room_id: OwnedRoomId,
        user_id: OwnedUserId,
        user_display_name: String,
        room_name: String,
    },
    /// Lift a previously-applied ban so the user may rejoin.
    Unban {
        room_id: OwnedRoomId,
        user_id: OwnedUserId,
        user_display_name: String,
        room_name: String,
    },
}

/// Actions handled by the App-level parent of this modal.
#[derive(Clone, Debug)]
pub enum ModerationActionModalAction {
    /// Open the modal with the given action context.
    Open(ModerationActionKind),
    /// The modal has been closed (by Cancel, Confirm, or backdrop dismiss).
    Close,
}


#[derive(Script, ScriptHook, Widget)]
pub struct ModerationActionModal {
    #[deref] view: View,
    #[rust] kind: Option<ModerationActionKind>,
}

impl Widget for ModerationActionModal {
    fn handle_event(&mut self, cx: &mut Cx, event: &Event, scope: &mut Scope) {
        self.view.handle_event(cx, event, scope);
        self.widget_match_event(cx, event, scope);
    }

    fn draw_walk(&mut self, cx: &mut Cx2d, scope: &mut Scope, walk: Walk) -> DrawStep {
        self.view.draw_walk(cx, scope, walk)
    }
}

impl WidgetMatchEvent for ModerationActionModal {
    fn handle_actions(&mut self, cx: &mut Cx, actions: &Actions, _scope: &mut Scope) {
        let cancel_clicked = self.view.button(cx, ids!(cancel_button)).clicked(actions);
        let dismissed = actions
            .iter()
            .any(|a| matches!(a.downcast_ref(), Some(ModalAction::Dismissed)));
        if cancel_clicked || dismissed {
            cx.action(ModerationActionModalAction::Close);
            self.reset_state(cx);
            return;
        }

        if self.view.button(cx, ids!(confirm_button)).clicked(actions) {
            if let Some(kind) = self.kind.take() {
                let reason_text = self.view.text_input(cx, ids!(reason_input)).text();
                let reason = if reason_text.trim().is_empty() {
                    None
                } else {
                    Some(reason_text)
                };

                match kind {
                    ModerationActionKind::Kick { room_id, user_id, .. } => {
                        log!("Submitting kick request for user {user_id} in {room_id}");
                        submit_async_request(MatrixRequest::KickUser { room_id, user_id, reason });
                    }
                    ModerationActionKind::Ban { room_id, user_id, .. } => {
                        log!("Submitting ban request for user {user_id} in {room_id}");
                        submit_async_request(MatrixRequest::BanUser { room_id, user_id, reason });
                    }
                    ModerationActionKind::Unban { room_id, user_id, .. } => {
                        log!("Submitting unban request for user {user_id} in {room_id}");
                        submit_async_request(MatrixRequest::UnbanUser { room_id, user_id, reason });
                    }
                }
            }
            cx.action(ModerationActionModalAction::Close);
            self.reset_state(cx);
        }
    }
}

impl ModerationActionModal {
    fn reset_state(&mut self, cx: &mut Cx) {
        self.kind = None;
        self.view.text_input(cx, ids!(reason_input)).set_text(cx, "");
    }

    fn set_kind(&mut self, cx: &mut Cx, kind: ModerationActionKind) {
        let (title, description, confirm_text) = match &kind {
            ModerationActionKind::Kick { user_display_name, room_name, is_invite, .. } => {
                if *is_invite {
                    (
                        format!("Disinvite {user_display_name}?"),
                        format!(
                            "This will cancel the pending invitation for {user_display_name} to join {room_name}. They can be re-invited later."
                        ),
                        "Disinvite",
                    )
                } else {
                    (
                        format!("Kick {user_display_name}?"),
                        format!(
                            "This will remove {user_display_name} from {room_name}. They can be re-invited or rejoin later if the room allows it."
                        ),
                        "Kick",
                    )
                }
            }
            ModerationActionKind::Ban { user_display_name, room_name, .. } => (
                format!("Ban {user_display_name}?"),
                format!(
                    "This will remove {user_display_name} from {room_name} and prevent them from rejoining until they are unbanned."
                ),
                "Ban",
            ),
            ModerationActionKind::Unban { user_display_name, room_name, .. } => (
                format!("Unban {user_display_name}?"),
                format!(
                    "This will lift the ban on {user_display_name} in {room_name}, allowing them to rejoin."
                ),
                "Unban",
            ),
        };

        self.view.label(cx, ids!(title)).set_text(cx, &title);
        self.view.label(cx, ids!(description)).set_text(cx, &description);
        self.view.text_input(cx, ids!(reason_input)).set_text(cx, "");
        let confirm_button = self.view.button(cx, ids!(confirm_button));
        confirm_button.set_text(cx, confirm_text);
        confirm_button.set_enabled(cx, true);
        confirm_button.reset_hover(cx);

        self.kind = Some(kind);
    }
}

impl ModerationActionModalRef {
    pub fn set_kind(&self, cx: &mut Cx, kind: ModerationActionKind) {
        let Some(mut inner) = self.borrow_mut() else { return };
        inner.set_kind(cx, kind);
    }
}
