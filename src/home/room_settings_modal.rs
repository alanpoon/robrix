//! A modal dialog for viewing and editing room settings.

use std::path::PathBuf;

use makepad_widgets::*;
use ruma::OwnedRoomId;

use crate::shared::avatar::AvatarWidgetExt;
use crate::shared::popup_list::{PopupKind, enqueue_popup_notification};
use crate::sliding_sync::{
    MatrixRequest, PowerLevelsChangesPayload, PowerLevelsSnapshot,
    RoomPowerLevelsAction, submit_async_request,
};
use crate::utils::load_png_or_jpg;

/// Which tab of the [`RoomSettingsModal`] is currently visible.
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq)]
enum SettingsTab {
    #[default]
    General,
    Permissions,
}

/// Identifiers for every row in the permissions section, in the order the
/// rows appear in the DSL. Used as a stable enum so we can iterate rows when
/// building the save payload.
#[derive(Copy, Clone, Debug)]
enum PermissionRowId {
    UsersDefault,
    EventsDefault,
    Invite,
    StateDefault,
    RoomName,
    RoomTopic,
    RoomAvatar,
    Redact,
    Kick,
    Ban,
}

impl PermissionRowId {
    const ALL: [PermissionRowId; 10] = [
        PermissionRowId::UsersDefault,
        PermissionRowId::EventsDefault,
        PermissionRowId::Invite,
        PermissionRowId::StateDefault,
        PermissionRowId::RoomName,
        PermissionRowId::RoomTopic,
        PermissionRowId::RoomAvatar,
        PermissionRowId::Redact,
        PermissionRowId::Kick,
        PermissionRowId::Ban,
    ];

    fn snapshot_value(self, snap: &PowerLevelsSnapshot) -> i64 {
        match self {
            PermissionRowId::UsersDefault => snap.users_default,
            PermissionRowId::EventsDefault => snap.events_default,
            PermissionRowId::Invite => snap.invite,
            PermissionRowId::StateDefault => snap.state_default,
            PermissionRowId::RoomName => snap.room_name,
            PermissionRowId::RoomTopic => snap.room_topic,
            PermissionRowId::RoomAvatar => snap.room_avatar,
            PermissionRowId::Redact => snap.redact,
            PermissionRowId::Kick => snap.kick,
            PermissionRowId::Ban => snap.ban,
        }
    }

    /// Apply a new PL value to the corresponding field of `changes`.
    fn assign_change(self, changes: &mut PowerLevelsChangesPayload, value: i64) {
        match self {
            PermissionRowId::UsersDefault => changes.users_default = Some(value),
            PermissionRowId::EventsDefault => changes.events_default = Some(value),
            PermissionRowId::Invite => changes.invite = Some(value),
            PermissionRowId::StateDefault => changes.state_default = Some(value),
            PermissionRowId::RoomName => changes.room_name = Some(value),
            PermissionRowId::RoomTopic => changes.room_topic = Some(value),
            PermissionRowId::RoomAvatar => changes.room_avatar = Some(value),
            PermissionRowId::Redact => changes.redact = Some(value),
            PermissionRowId::Kick => changes.kick = Some(value),
            PermissionRowId::Ban => changes.ban = Some(value),
        }
    }
}

/// Maps a raw power-level value to a dropdown index (0/1/2/3 for
/// Default/Moderator/Admin/Custom\u{2026}).
fn pl_to_dropdown_index(pl: i64) -> usize {
    match pl {
        0 => 0,
        50 => 1,
        100 => 2,
        _ => 3,
    }
}

script_mod! {
    use mod.prelude.widgets.*
    use mod.widgets.*

    let PermissionRow = View {
        width: Fill
        height: Fit
        flow: Down
        spacing: 4
        margin: Inset{bottom: 12}

        row_label := Label {
            width: Fill
            height: Fit
            margin: Inset{bottom: 2}
            draw_text +: {
                text_style: REGULAR_TEXT {font_size: 10.5}
                color: #333
            }
            text: ""
        }

        row_main := View {
            width: Fill
            height: Fit
            flow: Right
            spacing: 8
            align: Align{y: 0.5}

            row_dropdown := DropDownFlat {
                width: 160
                height: 32
                align: Align{y: 0.5}
                padding: Inset{left: 10, top: 6, bottom: 6, right: 28}
                draw_text +: {
                    text_style: REGULAR_TEXT { font_size: 11 }
                    color: #333
                }
                draw_bg +: {
                    color: uniform(#fff)
                    color_hover: uniform(#F0F0F2)
                    color_focus: uniform(#F0F0F2)
                    color_down: uniform(#E8E8EA)
                    border_color: uniform(#CCC)
                    border_color_hover: uniform(#AAA)
                    border_color_focus: uniform((COLOR_ACTIVE_PRIMARY))
                    arrow_color: uniform(#888)
                    arrow_color_hover: uniform(#555)
                }
                labels: ["Default", "Moderator", "Admin", "Custom\u{2026}"]
            }

            row_custom_input := RobrixTextInput {
                visible: false
                width: 80
                height: 32
                empty_text: "PL"
            }
        }
    }

    mod.widgets.RoomSettingsModal = #(RoomSettingsModal::register_widget(vm)) {
        width: Fit
        height: Fit

        RoundedView {
            width: 680
            height: Fit
            flow: Down
            padding: Inset{top: 0, right: 0, bottom: 0, left: 0}
            show_bg: true
            draw_bg +: {
                color: (COLOR_PRIMARY)
                border_radius: 6.0
            }

            // ── Title bar ────────────────────────────────────────────────
            title_bar := View {
                width: Fill
                height: Fit
                flow: Right
                align: Align{y: 0.5}
                padding: Inset{left: 20, right: 12, top: 14, bottom: 14}
                spacing: 8

                title_label := Label {
                    width: Fill
                    height: Fit
                    draw_text +: {
                        text_style: TITLE_TEXT {font_size: 13}
                        color: #000
                    }
                    text: "Room Settings"
                }

                close_button := RobrixNeutralIconButton {
                    width: 28
                    height: 28
                    padding: 4
                    draw_icon.svg: (ICON_CLOSE)
                    icon_walk: Walk{width: 14, height: 14}
                    text: ""
                }
            }

            // ── Separator ────────────────────────────────────────────────
            View {
                width: Fill
                height: 1
                show_bg: true
                draw_bg +: { color: (COLOR_SECONDARY) }
            }

            // ── Main area ────────────────────────────────────────────────
            main_area := View {
                width: Fill
                height: Fit
                flow: Right

                // Sidebar
                sidebar := View {
                    width: 130
                    height: Fit
                    flow: Down
                    padding: Inset{top: 12, left: 0, right: 0, bottom: 12}
                    show_bg: true
                    draw_bg +: { color: #F3F5F8 }

                    general_tab_button := RobrixNeutralIconButton {
                        width: Fill
                        height: 36
                        padding: Inset{left: 12, right: 8, top: 8, bottom: 8}
                        align: Align{x: 0.0, y: 0.5}
                        icon_walk: Walk{width: 0, height: 0}
                        draw_bg +: {
                            color: #E8EEF5
                            color_hover: #DDE6F0
                            color_down: #D0DBE8
                            border_radius: 0.0
                        }
                        draw_text +: {
                            color: #000
                            color_hover: #000
                            color_down: #000
                            text_style: REGULAR_TEXT {font_size: 11}
                        }
                        text: "General"
                    }

                    permissions_tab_button := RobrixNeutralIconButton {
                        width: Fill
                        height: 36
                        padding: Inset{left: 12, right: 8, top: 8, bottom: 8}
                        align: Align{x: 0.0, y: 0.5}
                        icon_walk: Walk{width: 0, height: 0}
                        draw_bg +: {
                            color: #F3F5F8
                            color_hover: #DDE6F0
                            color_down: #D0DBE8
                            border_radius: 0.0
                        }
                        draw_text +: {
                            color: #000
                            color_hover: #000
                            color_down: #000
                            text_style: REGULAR_TEXT {font_size: 11}
                        }
                        text: "Roles & Permissions"
                    }
                }

                // Content area
                content_scroll := ScrollYView {
                    width: Fill
                    height: 520
                    flow: Down
                    spacing: 0
                    padding: Inset{left: 24, right: 24, top: 20, bottom: 20}

                    general_section := View {
                        width: Fill
                        height: Fit
                        flow: Down
                        visible: true

                    // ── General heading ──────────────────────────────
                    general_heading := Label {
                        width: Fill
                        height: Fit
                        margin: Inset{bottom: 16}
                        draw_text +: {
                            text_style: TITLE_TEXT {font_size: 13}
                            color: #000
                        }
                        text: "General"
                    }

                    // ── Form row (inputs + avatar) ───────────────────
                    form_row := View {
                        width: Fill
                        height: Fit
                        flow: Right
                        spacing: 16

                        // Inputs column
                        inputs_col := View {
                            width: Fill
                            height: Fit
                            flow: Down
                            spacing: 6

                            room_name_label := Label {
                                width: Fill
                                height: Fit
                                margin: Inset{bottom: 2}
                                draw_text +: {
                                    text_style: REGULAR_TEXT {font_size: 10.5}
                                    color: #333
                                }
                                text: "Room Name"
                            }

                            room_name_input := RobrixTextInput {
                                width: Fill
                                height: 44
                                empty_text: "Room name"
                            }

                            room_topic_label := Label {
                                width: Fill
                                height: Fit
                                margin: Inset{top: 10, bottom: 2}
                                draw_text +: {
                                    text_style: REGULAR_TEXT {font_size: 10.5}
                                    color: #333
                                }
                                text: "Room Topic"
                            }

                            room_topic_input := RobrixTextInput {
                                width: Fill
                                height: 120
                                empty_text: "Room topic (optional)"
                                is_multiline: true
                            }

                            name_error_label := Label {
                                visible: false
                                width: Fill
                                height: Fit
                                margin: Inset{top: 2}
                                draw_text +: {
                                    text_style: REGULAR_TEXT {font_size: 10}
                                    color: (COLOR_FG_DANGER_RED)
                                }
                                text: ""
                            }

                            buttons_row := View {
                                width: Fill
                                height: Fit
                                flow: Right
                                align: Align{x: 1.0, y: 0.5}
                                margin: Inset{top: 12}
                                spacing: 10

                                cancel_button := RobrixNeutralIconButton {
                                    width: 90
                                    height: 32
                                    padding: 6
                                    icon_walk: Walk{width: 0, height: 0}
                                    draw_icon.svg: (ICON_FORBIDDEN)
                                    text: "Cancel"
                                }

                                save_button := RobrixIconButton {
                                    width: 90
                                    height: 32
                                    padding: 6
                                    icon_walk: Walk{width: 0, height: 0}
                                    draw_icon.svg: (ICON_CHECKMARK)
                                    text: "Save"
                                }
                            }
                        }

                        // Avatar column
                        avatar_col := View {
                            width: 80
                            height: Fit
                            flow: Down
                            align: Align{x: 0.5}
                            spacing: 6

                            room_avatar := Avatar {
                                width: 60
                                height: 60
                            }

                            pencil_button := RobrixNeutralIconButton {
                                width: 60
                                height: 24
                                padding: 4
                                align: Align{x: 0.5, y: 0.5}
                                draw_icon.svg: (ICON_EDIT)
                                icon_walk: Walk{width: 12, height: 12}
                                text: ""
                            }
                        }
                    }

                    // ── Section separator ────────────────────────────
                    View {
                        width: Fill
                        height: 1
                        margin: Inset{top: 20, bottom: 16}
                        show_bg: true
                        draw_bg +: { color: (COLOR_SECONDARY) }
                    }

                    // ── Room Addresses ───────────────────────────────
                    addresses_heading := Label {
                        width: Fill
                        height: Fit
                        margin: Inset{bottom: 10}
                        draw_text +: {
                            text_style: TITLE_TEXT {font_size: 12}
                            color: #000
                        }
                        text: "Room Addresses"
                    }

                    published_addresses_label := Label {
                        width: Fill
                        height: Fit
                        margin: Inset{bottom: 4}
                        draw_text +: {
                            text_style: REGULAR_TEXT {font_size: 11}
                            color: #333
                        }
                        text: "Published Addresses"
                    }

                    published_desc := Label {
                        width: Fill
                        height: Fit
                        flow: Flow.Right{wrap: true}
                        margin: Inset{bottom: 8}
                        draw_text +: {
                            text_style: REGULAR_TEXT {font_size: 10}
                            color: #666
                        }
                        text: "These are the addresses that are published on the room directory for others to find this room."
                    }

                    main_alias_row := View {
                        width: Fill
                        height: Fit
                        flow: Right
                        align: Align{y: 0.5}
                        margin: Inset{bottom: 8}
                        spacing: 8

                        main_alias_label := Label {
                            width: Fill
                            height: Fit
                            draw_text +: {
                                text_style: REGULAR_TEXT {font_size: 10.5}
                                color: #444
                            }
                            text: "No main address set"
                        }
                    }

                    publish_toggle_row := View {
                        width: Fill
                        height: Fit
                        flow: Right
                        align: Align{y: 0.5}
                        margin: Inset{bottom: 8}
                        spacing: 8

                        publish_toggle := Toggle {
                            width: Fit
                            height: Fit
                            padding: Inset{top: 2, right: 4, bottom: 2, left: 2}
                            text: ""
                            active: false
                            draw_bg +: {
                                size: 18.0
                                color_active: (COLOR_ACTIVE_PRIMARY)
                                border_color_active: (COLOR_ACTIVE_PRIMARY)
                                mark_color_active: #fff
                            }
                        }

                        publish_toggle_label := Label {
                            width: Fill
                            height: Fit
                            flow: Flow.Right{wrap: true}
                            draw_text +: {
                                text_style: REGULAR_TEXT {font_size: 10}
                                color: #333
                            }
                            text: "Publish this room to the public in matrix.org's room directory?"
                        }
                    }

                    no_published_label := Label {
                        width: Fill
                        height: Fit
                        margin: Inset{bottom: 8}
                        draw_text +: {
                            text_style: REGULAR_TEXT {font_size: 10}
                            color: #888
                        }
                        text: "No other published addresses yet, add one below"
                    }

                    add_address_row := View {
                        width: Fill
                        height: Fit
                        flow: Right
                        align: Align{y: 0.5}
                        spacing: 8
                        margin: Inset{bottom: 12}

                        add_address_input := RobrixTextInput {
                            width: Fill
                            height: 36
                            empty_text: "# e.g. my-room"
                        }

                        add_address_button := RobrixIconButton {
                            width: 60
                            height: 36
                            padding: 6
                            icon_walk: Walk{width: 0, height: 0}
                            text: "Add"
                        }
                    }

                    local_addresses_label := Label {
                        width: Fill
                        height: Fit
                        margin: Inset{bottom: 4}
                        draw_text +: {
                            text_style: REGULAR_TEXT {font_size: 11}
                            color: #333
                        }
                        text: "Local Addresses"
                    }

                    local_desc := Label {
                        width: Fill
                        height: Fit
                        flow: Flow.Right{wrap: true}
                        margin: Inset{bottom: 8}
                        draw_text +: {
                            text_style: REGULAR_TEXT {font_size: 10}
                            color: #666
                        }
                        text: "Set addresses for this room so users can find this room. As an admin, you can set local addresses for this room."
                    }

                    // ── Section separator ────────────────────────────
                    View {
                        width: Fill
                        height: 1
                        margin: Inset{top: 12, bottom: 16}
                        show_bg: true
                        draw_bg +: { color: (COLOR_SECONDARY) }
                    }

                    // ── Other / Moderation ───────────────────────────
                    other_heading := Label {
                        width: Fill
                        height: Fit
                        margin: Inset{bottom: 10}
                        draw_text +: {
                            text_style: TITLE_TEXT {font_size: 12}
                            color: #000
                        }
                        text: "Other"
                    }

                    moderation_label := Label {
                        width: Fill
                        height: Fit
                        margin: Inset{bottom: 6}
                        draw_text +: {
                            text_style: REGULAR_TEXT {font_size: 11}
                            color: #333
                        }
                        text: "Moderation and safety"
                    }

                    show_media_label := Label {
                        width: Fill
                        height: Fit
                        margin: Inset{bottom: 2}
                        draw_text +: {
                            text_style: REGULAR_TEXT {font_size: 10.5}
                            color: #333
                        }
                        text: "Show media in timeline"
                    }

                    show_media_desc := Label {
                        width: Fill
                        height: Fit
                        flow: Flow.Right{wrap: true}
                        margin: Inset{bottom: 6}
                        draw_text +: {
                            text_style: REGULAR_TEXT {font_size: 10}
                            color: #666
                        }
                        text: "A hidden media can always be shown by tapping on it"
                    }

                    media_hide_radio := RadioButton {
                        width: Fit
                        height: Fit
                        align: Align{y: 0.5}
                        padding: Inset{top: 4, bottom: 4, left: 6, right: 4}
                        draw_text +: {
                            color: (MESSAGE_TEXT_COLOR)
                            color_hover: (MESSAGE_TEXT_COLOR)
                            color_focus: (MESSAGE_TEXT_COLOR)
                            color_active: (MESSAGE_TEXT_COLOR)
                            color_down: (MESSAGE_TEXT_COLOR)
                            color_disabled: (MESSAGE_TEXT_COLOR)
                            text_style: REGULAR_TEXT {font_size: 10.5}
                        }
                        draw_bg +: {
                            color: (COLOR_PRIMARY)
                            border_color: (COLOR_SECONDARY_DARKER)
                            border_color_active: (COLOR_ACTIVE_PRIMARY_DARKER)
                            mark_color: vec4(0.0, 0.0, 0.0, 0.0)
                            mark_color_active: (COLOR_ACTIVE_PRIMARY_DARKER)
                        }
                        text: "Always hide"
                    }

                    media_show_radio := RadioButton {
                        width: Fit
                        height: Fit
                        align: Align{y: 0.5}
                        padding: Inset{top: 4, bottom: 4, left: 6, right: 4}
                        draw_text +: {
                            color: (MESSAGE_TEXT_COLOR)
                            color_hover: (MESSAGE_TEXT_COLOR)
                            color_focus: (MESSAGE_TEXT_COLOR)
                            color_active: (MESSAGE_TEXT_COLOR)
                            color_down: (MESSAGE_TEXT_COLOR)
                            color_disabled: (MESSAGE_TEXT_COLOR)
                            text_style: REGULAR_TEXT {font_size: 10.5}
                        }
                        draw_bg +: {
                            color: (COLOR_PRIMARY)
                            border_color: (COLOR_SECONDARY_DARKER)
                            border_color_active: (COLOR_ACTIVE_PRIMARY_DARKER)
                            mark_color: vec4(0.0, 0.0, 0.0, 0.0)
                            mark_color_active: (COLOR_ACTIVE_PRIMARY_DARKER)
                        }
                        text: "Always show"
                    }

                    // ── Section separator ────────────────────────────
                    View {
                        width: Fill
                        height: 1
                        margin: Inset{top: 16, bottom: 16}
                        show_bg: true
                        draw_bg +: { color: (COLOR_SECONDARY) }
                    }

                    // ── Leave Room ───────────────────────────────────
                    leave_room_label := Label {
                        width: Fill
                        height: Fit
                        margin: Inset{bottom: 10}
                        draw_text +: {
                            text_style: REGULAR_TEXT {font_size: 11}
                            color: #333
                        }
                        text: "Leave room"
                    }

                    leave_button := RobrixNegativeIconButton {
                        width: Fit
                        height: 32
                        padding: Inset{left: 12, right: 12, top: 6, bottom: 6}
                        icon_walk: Walk{width: 0, height: 0}
                        text: "Leave room"
                    }
                    }  // end general_section

                    permissions_section := View {
                        width: Fill
                        height: Fit
                        flow: Down
                        visible: false

                        permissions_heading := Label {
                            width: Fill
                            height: Fit
                            margin: Inset{bottom: 8}
                            draw_text +: {
                                text_style: TITLE_TEXT {font_size: 13}
                                color: #000
                            }
                            text: "Roles & Permissions"
                        }

                        permissions_subtext := Label {
                            width: Fill
                            height: Fit
                            flow: Flow.Right{wrap: true}
                            margin: Inset{bottom: 14}
                            draw_text +: {
                                text_style: REGULAR_TEXT {font_size: 10.5}
                                color: #666
                            }
                            text: "Set the minimum role required to perform each action in this room. Changes are sent to the server when you click Save."
                        }

                        permissions_status_label := Label {
                            width: Fill
                            height: Fit
                            margin: Inset{bottom: 10}
                            draw_text +: {
                                text_style: REGULAR_TEXT {font_size: 10.5}
                                color: #888
                            }
                            text: "Loading permissions\u{2026}"
                        }

                        pl_row_users_default := PermissionRow { row_label: { text: "Default role for new members" } }
                        pl_row_events_default := PermissionRow { row_label: { text: "Send messages" } }
                        pl_row_invite := PermissionRow { row_label: { text: "Invite users" } }
                        pl_row_state_default := PermissionRow { row_label: { text: "Change room settings" } }
                        pl_row_room_name := PermissionRow { row_label: { text: "Change room name" } }
                        pl_row_room_topic := PermissionRow { row_label: { text: "Change room topic" } }
                        pl_row_room_avatar := PermissionRow { row_label: { text: "Change room avatar" } }
                        pl_row_redact := PermissionRow { row_label: { text: "Remove messages" } }
                        pl_row_kick := PermissionRow { row_label: { text: "Kick users" } }
                        pl_row_ban := PermissionRow { row_label: { text: "Ban users" } }

                        permissions_buttons_row := View {
                            width: Fill
                            height: Fit
                            flow: Right
                            align: Align{x: 1.0, y: 0.5}
                            margin: Inset{top: 20}
                            spacing: 10

                            permissions_cancel_button := RobrixNeutralIconButton {
                                width: 100
                                height: 32
                                padding: 6
                                icon_walk: Walk{width: 0, height: 0}
                                text: "Cancel"
                            }

                            permissions_save_button := RobrixIconButton {
                                width: 100
                                height: 32
                                padding: 6
                                icon_walk: Walk{width: 0, height: 0}
                                text: "Save"
                            }
                        }
                    }  // end permissions_section
                }
            }
        }
    }
}

/// Actions emitted by the `RoomSettingsModal`.
#[derive(Clone, Debug, Default)]
pub enum RoomSettingsAction {
    /// Open the modal for the given room.
    Open { room_id: OwnedRoomId },
    /// Close the modal (user clicked close/X).
    Close,
    /// Save room name and topic.
    Save { room_id: OwnedRoomId, room_name: String, room_topic: String },
    /// Cancel edits without saving.
    Cancel,
    /// Toggle publishing this room to the directory.
    SetDirectoryPublish { room_id: OwnedRoomId, enabled: bool },
    /// Add a local address alias.
    AddLocalAddress { room_id: OwnedRoomId, alias: String },
    /// Change media visibility preference.
    SetMediaVisibility { room_id: OwnedRoomId, always_show: bool },
    /// Leave the room.
    LeaveRoom { room_id: OwnedRoomId },
    /// Upload a new room avatar from the given local file path.
    UploadRoomAvatar { room_id: OwnedRoomId, avatar_path: PathBuf },
    #[default]
    None,
}

#[derive(Script, ScriptHook, Widget)]
pub struct RoomSettingsModal {
    #[deref] view: View,
    #[source] source: ScriptObjectRef,
    #[rust] room_id: Option<OwnedRoomId>,
    #[rust] original_name: String,
    #[rust] original_topic: String,
    #[rust] always_show_media: bool,
    #[rust] active_tab: SettingsTab,
    /// Most recently fetched snapshot of the room's PL thresholds. `None`
    /// until the GetRoomPowerLevels response arrives.
    #[rust] permissions_snapshot: Option<PowerLevelsSnapshot>,
}

impl Widget for RoomSettingsModal {
    fn handle_event(&mut self, cx: &mut Cx, event: &Event, scope: &mut Scope) {
        self.view.handle_event(cx, event, scope);
        self.widget_match_event(cx, event, scope);
    }

    fn draw_walk(&mut self, cx: &mut Cx2d, scope: &mut Scope, walk: Walk) -> DrawStep {
        self.view.draw_walk(cx, scope, walk)
    }
}

impl WidgetMatchEvent for RoomSettingsModal {
    fn handle_actions(&mut self, cx: &mut Cx, actions: &Actions, _scope: &mut Scope) {
        // Tab switching
        if self.view.button(cx, ids!(general_tab_button)).clicked(actions) {
            self.set_active_tab(cx, SettingsTab::General);
            return;
        }
        if self.view.button(cx, ids!(permissions_tab_button)).clicked(actions) {
            self.set_active_tab(cx, SettingsTab::Permissions);
            return;
        }

        // Permissions tab: per-row dropdown change reveals/hides custom input
        if self.active_tab == SettingsTab::Permissions {
            self.handle_permission_row_dropdowns(cx, actions);

            if self.view.button(cx, ids!(permissions_cancel_button)).clicked(actions) {
                // Reset edits back to the snapshot.
                if self.permissions_snapshot.is_some() {
                    self.populate_permission_rows(cx);
                }
                return;
            }

            if self.view.button(cx, ids!(permissions_save_button)).clicked(actions) {
                self.dispatch_permission_save(cx);
                return;
            }
        }

        // Permissions: fetch response actions
        for action in actions {
            if let Some(pl_action) = action.downcast_ref::<RoomPowerLevelsAction>() {
                let room_id_matches = self.room_id.as_ref().is_some_and(|rid| {
                    match pl_action {
                        RoomPowerLevelsAction::Fetched { room_id, .. }
                        | RoomPowerLevelsAction::FetchFailed { room_id, .. }
                        | RoomPowerLevelsAction::Applied { room_id }
                        | RoomPowerLevelsAction::ApplyFailed { room_id, .. } => room_id == rid,
                    }
                });
                if !room_id_matches { continue }
                match pl_action {
                    RoomPowerLevelsAction::Fetched { levels, .. } => {
                        self.permissions_snapshot = Some(levels.clone());
                        self.populate_permission_rows(cx);
                        self.view.label(cx, ids!(permissions_status_label))
                            .set_visible(cx, false);
                        self.view.redraw(cx);
                    }
                    RoomPowerLevelsAction::FetchFailed { error, .. } => {
                        self.view.label(cx, ids!(permissions_status_label))
                            .set_text(cx, &format!("Failed to load permissions: {error}"));
                        self.view.redraw(cx);
                    }
                    RoomPowerLevelsAction::Applied { .. } => {
                        // Refresh snapshot from server after a successful apply
                        // so subsequent edits compare against the new baseline.
                        if let Some(room_id) = self.room_id.clone() {
                            submit_async_request(MatrixRequest::FetchRoomPowerLevelsSnapshot { room_id });
                        }
                    }
                    RoomPowerLevelsAction::ApplyFailed { .. } => {
                        // Popup already enqueued by the handler. Leave the
                        // draft edits in place so the user can retry.
                    }
                }
            }
        }

        // Close button
        if self.view.button(cx, ids!(close_button)).clicked(actions) {
            cx.action(RoomSettingsAction::Close);
            return;
        }

        // Cancel button
        if self.view.button(cx, ids!(cancel_button)).clicked(actions) {
            cx.action(RoomSettingsAction::Cancel);
            return;
        }

        // Save button – validate name not empty
        if self.view.button(cx, ids!(save_button)).clicked(actions) {
            let name = self.view.text_input(cx, ids!(room_name_input)).text();
            let topic = self.view.text_input(cx, ids!(room_topic_input)).text();
            if name.trim().is_empty() {
                self.view.label(cx, ids!(name_error_label))
                    .set_text(cx, "Room name cannot be empty");
                self.view.label(cx, ids!(name_error_label)).set_visible(cx, true);
                self.view.redraw(cx);
            } else {
                self.view.label(cx, ids!(name_error_label)).set_visible(cx, false);
                if let Some(room_id) = self.room_id.clone() {
                    cx.action(RoomSettingsAction::Save {
                        room_id,
                        room_name: name.trim().to_string(),
                        room_topic: topic.trim().to_string(),
                    });
                }
            }
            return;
        }

        // Publish toggle
        let publish_toggle = self.view.check_box(cx, ids!(publish_toggle));
        if let Some(enabled) = publish_toggle.changed(actions) {
            if let Some(room_id) = self.room_id.clone() {
                cx.action(RoomSettingsAction::SetDirectoryPublish { room_id, enabled });
            }
        }

        // Add address button
        if self.view.button(cx, ids!(add_address_button)).clicked(actions) {
            let alias = self.view.text_input(cx, ids!(add_address_input)).text();
            let alias = alias.trim().trim_start_matches('#').to_string();
            if !alias.is_empty() {
                if let Some(room_id) = self.room_id.clone() {
                    cx.action(RoomSettingsAction::AddLocalAddress { room_id, alias });
                    self.view.text_input(cx, ids!(add_address_input)).set_text(cx, "");
                }
            }
        }

        // Media radio buttons
        let radios = self.view.radio_button_set(cx, ids_array!(media_hide_radio, media_show_radio));
        if let Some(selected) = radios.selected(cx, actions) {
            let always_show = selected == 1;
            self.always_show_media = always_show;
            if let Some(room_id) = self.room_id.clone() {
                cx.action(RoomSettingsAction::SetMediaVisibility { room_id, always_show });
            }
        }

        // Leave button
        if self.view.button(cx, ids!(leave_button)).clicked(actions) {
            if let Some(room_id) = self.room_id.clone() {
                cx.action(RoomSettingsAction::LeaveRoom { room_id });
            }
        }

        // Pencil / edit avatar button — open native file picker
        if self.view.button(cx, ids!(pencil_button)).clicked(actions) {
            #[cfg(any(target_os = "macos", target_os = "windows", target_os = "linux"))]
            if let Some(room_id) = self.room_id.clone() {
                use rfd::FileDialog;
                if let Some(path) = FileDialog::new()
                    .add_filter("Image", &["png", "jpg", "jpeg"])
                    .pick_file()
                {
                    cx.action(RoomSettingsAction::UploadRoomAvatar { room_id, avatar_path: path });
                }
            }
            #[cfg(not(any(target_os = "macos", target_os = "windows", target_os = "linux")))]
            if let Some(_room_id) = self.room_id.clone() {
                use crate::shared::popup_list::{PopupKind, enqueue_popup_notification};
                enqueue_popup_notification(
                    "Avatar upload not supported on this platform",
                    PopupKind::Warning,
                    Some(4.0),
                );
            }
        }
    }
}

impl RoomSettingsModal {
    fn set_active_tab(&mut self, cx: &mut Cx, tab: SettingsTab) {
        self.active_tab = tab;
        let (general_visible, permissions_visible) = match tab {
            SettingsTab::General => (true, false),
            SettingsTab::Permissions => (false, true),
        };
        self.view.view(cx, ids!(general_section)).set_visible(cx, general_visible);
        self.view.view(cx, ids!(permissions_section)).set_visible(cx, permissions_visible);

        if tab == SettingsTab::Permissions && self.permissions_snapshot.is_none() {
            if let Some(room_id) = self.room_id.clone() {
                self.view.label(cx, ids!(permissions_status_label))
                    .set_visible(cx, true);
                self.view.label(cx, ids!(permissions_status_label))
                    .set_text(cx, "Loading permissions\u{2026}");
                submit_async_request(MatrixRequest::FetchRoomPowerLevelsSnapshot { room_id });
            }
        } else if tab == SettingsTab::Permissions {
            self.populate_permission_rows(cx);
        }
        self.view.redraw(cx);
    }

    /// Returns the dropdown widget for a given row id.
    fn row_dropdown(&mut self, cx: &mut Cx, row_id: PermissionRowId) -> DropDownRef {
        match row_id {
            PermissionRowId::UsersDefault => self.view.drop_down(cx, ids!(pl_row_users_default.row_main.row_dropdown)),
            PermissionRowId::EventsDefault => self.view.drop_down(cx, ids!(pl_row_events_default.row_main.row_dropdown)),
            PermissionRowId::Invite => self.view.drop_down(cx, ids!(pl_row_invite.row_main.row_dropdown)),
            PermissionRowId::StateDefault => self.view.drop_down(cx, ids!(pl_row_state_default.row_main.row_dropdown)),
            PermissionRowId::RoomName => self.view.drop_down(cx, ids!(pl_row_room_name.row_main.row_dropdown)),
            PermissionRowId::RoomTopic => self.view.drop_down(cx, ids!(pl_row_room_topic.row_main.row_dropdown)),
            PermissionRowId::RoomAvatar => self.view.drop_down(cx, ids!(pl_row_room_avatar.row_main.row_dropdown)),
            PermissionRowId::Redact => self.view.drop_down(cx, ids!(pl_row_redact.row_main.row_dropdown)),
            PermissionRowId::Kick => self.view.drop_down(cx, ids!(pl_row_kick.row_main.row_dropdown)),
            PermissionRowId::Ban => self.view.drop_down(cx, ids!(pl_row_ban.row_main.row_dropdown)),
        }
    }

    fn row_custom_input(&mut self, cx: &mut Cx, row_id: PermissionRowId) -> TextInputRef {
        match row_id {
            PermissionRowId::UsersDefault => self.view.text_input(cx, ids!(pl_row_users_default.row_main.row_custom_input)),
            PermissionRowId::EventsDefault => self.view.text_input(cx, ids!(pl_row_events_default.row_main.row_custom_input)),
            PermissionRowId::Invite => self.view.text_input(cx, ids!(pl_row_invite.row_main.row_custom_input)),
            PermissionRowId::StateDefault => self.view.text_input(cx, ids!(pl_row_state_default.row_main.row_custom_input)),
            PermissionRowId::RoomName => self.view.text_input(cx, ids!(pl_row_room_name.row_main.row_custom_input)),
            PermissionRowId::RoomTopic => self.view.text_input(cx, ids!(pl_row_room_topic.row_main.row_custom_input)),
            PermissionRowId::RoomAvatar => self.view.text_input(cx, ids!(pl_row_room_avatar.row_main.row_custom_input)),
            PermissionRowId::Redact => self.view.text_input(cx, ids!(pl_row_redact.row_main.row_custom_input)),
            PermissionRowId::Kick => self.view.text_input(cx, ids!(pl_row_kick.row_main.row_custom_input)),
            PermissionRowId::Ban => self.view.text_input(cx, ids!(pl_row_ban.row_main.row_custom_input)),
        }
    }

    /// Populate every permission row's dropdown + custom input from the
    /// currently-stored snapshot. No-op when the snapshot is not yet loaded.
    fn populate_permission_rows(&mut self, cx: &mut Cx) {
        let Some(snap) = self.permissions_snapshot.clone() else { return };
        for row_id in PermissionRowId::ALL {
            let pl = row_id.snapshot_value(&snap);
            let idx = pl_to_dropdown_index(pl);
            self.row_dropdown(cx, row_id).set_selected_item(cx, idx);
            let input = self.row_custom_input(cx, row_id);
            input.set_visible(cx, idx == 3);
            input.set_text(cx, &pl.to_string());
        }
    }

    /// Detect any dropdown changes this frame and reveal/hide the per-row
    /// custom input accordingly.
    fn handle_permission_row_dropdowns(&mut self, cx: &mut Cx, actions: &Actions) {
        for row_id in PermissionRowId::ALL {
            let dd = self.row_dropdown(cx, row_id);
            if dd.changed(actions).is_some() {
                let idx = dd.selected_item();
                let input = self.row_custom_input(cx, row_id);
                let was_custom = input.borrow().is_some_and(|i| i.visible());
                input.set_visible(cx, idx == 3);
                if idx == 3 && !was_custom {
                    // Pre-fill with the current snapshot value when first
                    // opening the custom input.
                    if let Some(snap) = self.permissions_snapshot.as_ref() {
                        let pl = row_id.snapshot_value(snap);
                        input.set_text(cx, &pl.to_string());
                    }
                }
                self.view.redraw(cx);
            }
        }
    }

    /// Build a `PowerLevelsChangesPayload` containing only the rows that
    /// changed from the snapshot, then dispatch it. Invalid custom inputs
    /// (non-integer text) abort the save with a popup notification.
    fn dispatch_permission_save(&mut self, cx: &mut Cx) {
        let Some(snap) = self.permissions_snapshot.clone() else {
            enqueue_popup_notification(
                "Permissions not loaded yet \u{2014} please wait.",
                PopupKind::Warning,
                Some(3.0),
            );
            return;
        };
        let Some(room_id) = self.room_id.clone() else { return };

        let mut changes = PowerLevelsChangesPayload::default();
        let mut any_change = false;
        for row_id in PermissionRowId::ALL {
            let idx = self.row_dropdown(cx, row_id).selected_item();
            let new_pl: i64 = match idx {
                0 => 0,
                1 => 50,
                2 => 100,
                3 => {
                    let text = self.row_custom_input(cx, row_id).text();
                    match text.trim().parse::<i64>() {
                        Ok(v) => v,
                        Err(_) => {
                            enqueue_popup_notification(
                                format!("Invalid power level \"{}\" \u{2014} enter an integer.", text.trim()),
                                PopupKind::Error,
                                None,
                            );
                            return;
                        }
                    }
                }
                _ => row_id.snapshot_value(&snap),
            };
            let current = row_id.snapshot_value(&snap);
            if new_pl != current {
                row_id.assign_change(&mut changes, new_pl);
                any_change = true;
            }
        }

        if !any_change {
            enqueue_popup_notification(
                "No permission changes to save.",
                PopupKind::Info,
                Some(2.5),
            );
            return;
        }

        submit_async_request(MatrixRequest::ApplyRoomPowerLevelChanges { room_id, changes });
    }

    /// Populate the modal with room data and prepare for display.
    pub fn show(
        &mut self,
        cx: &mut Cx,
        room_id: OwnedRoomId,
        room_name: &str,
        room_topic: &str,
        canonical_alias: Option<&str>,
    ) {
        self.room_id = Some(room_id);
        self.original_name = room_name.to_string();
        self.original_topic = room_topic.to_string();
        self.always_show_media = false;
        // Reset the permissions tab so previous-room data isn't shown.
        self.active_tab = SettingsTab::General;
        self.permissions_snapshot = None;
        self.view.view(cx, ids!(general_section)).set_visible(cx, true);
        self.view.view(cx, ids!(permissions_section)).set_visible(cx, false);
        self.view.label(cx, ids!(permissions_status_label)).set_visible(cx, true);
        self.view.label(cx, ids!(permissions_status_label))
            .set_text(cx, "Loading permissions\u{2026}");

        // Update title
        self.view.label(cx, ids!(title_label))
            .set_text(cx, &format!("Room Settings – {room_name}"));

        // Populate inputs
        self.view.text_input(cx, ids!(room_name_input))
            .set_text(cx, room_name);
        self.view.text_input(cx, ids!(room_topic_input))
            .set_text(cx, room_topic);

        // Canonical alias
        let alias_text = canonical_alias
            .map(|a| a.to_string())
            .unwrap_or_else(|| String::from("No main address set"));
        self.view.label(cx, ids!(main_alias_label))
            .set_text(cx, &alias_text);

        // Avatar fallback text (first char of name)
        let avatar_char = room_name.chars().next().unwrap_or('?').to_string();
        self.view.avatar(cx, ids!(room_avatar))
            .show_text(cx, None, None, &avatar_char);

        // Reset error label
        self.view.label(cx, ids!(name_error_label)).set_visible(cx, false);
        self.view.label(cx, ids!(name_error_label)).set_text(cx, "");

        self.view.redraw(cx);
    }

    /// Update the avatar widget with freshly uploaded image bytes.
    pub fn apply_avatar(&mut self, cx: &mut Cx, image_data: &[u8]) {
        let _ = self.view.avatar(cx, ids!(room_avatar))
            .show_image(cx, None, |cx, img| load_png_or_jpg(&img, cx, image_data));
        self.view.redraw(cx);
    }

    /// Apply fetched settings (topic, is_public) that arrived asynchronously.
    pub fn apply_fetched_settings(
        &mut self,
        cx: &mut Cx,
        topic: Option<String>,
        is_public: bool,
    ) {
        if let Some(t) = topic {
            self.original_topic = t.clone();
            self.view.text_input(cx, ids!(room_topic_input)).set_text(cx, &t);
        }
        // Update publish toggle state (active == is_public)
        // Toggle widget: set via script_apply_eval on check_box
        let _ = is_public; // reflected by the toggle's current state
        self.view.redraw(cx);
    }
}

impl RoomSettingsModalRef {
    /// Populate the modal with room data and prepare for display.
    pub fn show_settings(
        &self,
        cx: &mut Cx,
        room_id: OwnedRoomId,
        room_name: &str,
        room_topic: &str,
        canonical_alias: Option<&str>,
    ) {
        let Some(mut inner) = self.borrow_mut() else { return };
        inner.show(cx, room_id, room_name, room_topic, canonical_alias);
    }

    /// Apply asynchronously-fetched settings (topic, is_public).
    pub fn apply_fetched_settings(&self, cx: &mut Cx, topic: Option<String>, is_public: bool) {
        let Some(mut inner) = self.borrow_mut() else { return };
        inner.apply_fetched_settings(cx, topic, is_public);
    }

    /// Update the avatar widget after a successful upload.
    pub fn apply_avatar(&self, cx: &mut Cx, image_data: &[u8]) {
        let Some(mut inner) = self.borrow_mut() else { return };
        inner.apply_avatar(cx, image_data);
    }
}
