//! A modal pane for viewing room members with power levels, searching, and
//! performing administrative actions (invite, kick, ban).

use std::sync::Arc;

use makepad_widgets::*;
use matrix_sdk::{
    RoomMemberships,
    room::{RoomMember, RoomMemberRole},
    ruma::OwnedUserId,
};
use ruma::OwnedRoomId;

use crate::avatar_cache::{self, AvatarCacheEntry};
use crate::home::room_screen::{
    MemberActionKind, MemberActionResultAction,
};
use crate::shared::avatar::AvatarWidgetRefExt;
use crate::shared::popup_list::{PopupKind, enqueue_popup_notification};
use crate::sliding_sync::{MatrixRequest, current_user_id, submit_async_request};
use crate::utils::{RoomNameId, load_png_or_jpg};

script_mod! {
    use mod.prelude.widgets.*
    use mod.widgets.*

    // Template for a single member row.
    let MemberRow = View {
        visible: false
        width: Fill
        height: 56
        flow: Overlay

        row := View {
            width: Fill
            height: Fill
            flow: Right
            align: Align{y: 0.5}
            spacing: 10
            padding: Inset{left: 10, right: 8, top: 6, bottom: 6}

            show_bg: true
            draw_bg +: {
                color: #F8FAFD
                color_hover: #EEF3FA
                border_radius: 4.0
                border_size: 1.0
                border_color: #D8E0EA
            }

            avatar := Avatar {
                width: 34
                height: 34
            }

            text_col := View {
                width: Fill
                height: Fit
                flow: Down
                spacing: 0

                name_label := Label {
                    width: Fill
                    height: Fit
                    flow: Flow.Right{wrap: true}
                    draw_text +: {
                        color: #1F1F1F
                        text_style: REGULAR_TEXT {font_size: 11}
                    }
                    text: ""
                }

                id_label := Label {
                    width: Fill
                    height: Fit
                    flow: Flow.Right{wrap: true}
                    draw_text +: {
                        color: (COLOR_TEXT_INPUT_IDLE)
                        text_style: REGULAR_TEXT {font_size: 9}
                    }
                    text: ""
                }
            }

            role_badge := Label {
                width: Fit
                height: Fit
                margin: Inset{left: 4, right: 4}
                padding: Inset{left: 6, right: 6, top: 2, bottom: 2}
                show_bg: true
                draw_bg +: {
                    color: #E8EEF5
                    border_radius: 3.0
                }
                draw_text +: {
                    color: #4A5568
                    text_style: REGULAR_TEXT {font_size: 9}
                }
                text: ""
            }

            kick_button := RobrixNeutralIconButton {
                width: 30
                height: 30
                padding: 4
                draw_icon.svg: (ICON_LOGOUT)
                icon_walk: Walk{width: 14, height: 14}
                text: ""
            }

            ban_button := RobrixNegativeIconButton {
                width: 30
                height: 30
                padding: 4
                draw_icon.svg: (ICON_FORBIDDEN)
                icon_walk: Walk{width: 14, height: 14}
                text: ""
            }
        }
    }

    mod.widgets.RoomMembersPane = #(RoomMembersPane::register_widget(vm)) {
        width: Fit
        height: Fit

        RoundedView {
            width: 560
            height: Fit
            flow: Down
            padding: Inset{top: 0, right: 0, bottom: 0, left: 0}
            show_bg: true
            draw_bg +: {
                color: (COLOR_PRIMARY)
                border_radius: 6.0
            }

            // Title bar
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
                    text: "Members"
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

            View {
                width: Fill
                height: 1
                show_bg: true
                draw_bg +: { color: (COLOR_SECONDARY) }
            }

            // Search + invite row
            toolbar := View {
                width: Fill
                height: Fit
                flow: Right
                align: Align{y: 0.5}
                padding: Inset{left: 16, right: 16, top: 12, bottom: 8}
                spacing: 10

                search_input := RobrixTextInput {
                    width: Fill
                    height: 36
                    empty_text: "Search members…"
                }

                invite_button := RobrixIconButton {
                    width: Fit
                    height: 36
                    padding: Inset{left: 12, right: 12, top: 6, bottom: 6}
                    draw_icon.svg: (ICON_ADD_USER)
                    icon_walk: Walk{width: 14, height: 14, margin: Inset{right: 4}}
                    text: "Invite"
                }
            }

            // Status / count label
            status_label := Label {
                width: Fill
                height: Fit
                padding: Inset{left: 18, right: 18, top: 0, bottom: 8}
                draw_text +: {
                    text_style: REGULAR_TEXT {font_size: 10}
                    color: #6D7682
                }
                text: "Loading members…"
            }

            // Member list (scrollable)
            list_scroll := ScrollYView {
                width: Fill
                height: 460
                padding: Inset{left: 12, right: 12, top: 0, bottom: 12}

                list_view := View {
                    width: Fill
                    height: Fit
                    flow: Down
                    spacing: 4

                    member_row_0  := MemberRow {}
                    member_row_1  := MemberRow {}
                    member_row_2  := MemberRow {}
                    member_row_3  := MemberRow {}
                    member_row_4  := MemberRow {}
                    member_row_5  := MemberRow {}
                    member_row_6  := MemberRow {}
                    member_row_7  := MemberRow {}
                    member_row_8  := MemberRow {}
                    member_row_9  := MemberRow {}
                    member_row_10 := MemberRow {}
                    member_row_11 := MemberRow {}
                    member_row_12 := MemberRow {}
                    member_row_13 := MemberRow {}
                    member_row_14 := MemberRow {}
                    member_row_15 := MemberRow {}
                    member_row_16 := MemberRow {}
                    member_row_17 := MemberRow {}
                    member_row_18 := MemberRow {}
                    member_row_19 := MemberRow {}
                    member_row_20 := MemberRow {}
                    member_row_21 := MemberRow {}
                    member_row_22 := MemberRow {}
                    member_row_23 := MemberRow {}
                    member_row_24 := MemberRow {}
                    member_row_25 := MemberRow {}
                    member_row_26 := MemberRow {}
                    member_row_27 := MemberRow {}
                    member_row_28 := MemberRow {}
                    member_row_29 := MemberRow {}
                    member_row_30 := MemberRow {}
                    member_row_31 := MemberRow {}
                }
            }
        }
    }
}

/// Actions emitted by, or directed at, the `RoomMembersPane`.
#[derive(Clone, Debug)]
pub enum RoomMembersPaneAction {
    /// Open the members pane for the given room.
    Open { room_id: OwnedRoomId, room_name_id: RoomNameId },
    /// Close the members pane.
    Close,
    /// User clicked the Invite button; the parent should open the InviteModal.
    InviteRequested { room_name_id: RoomNameId },
}

#[derive(Script, ScriptHook, Widget)]
pub struct RoomMembersPane {
    #[deref] view: View,
    #[source] source: ScriptObjectRef,
    #[rust] room_id: Option<OwnedRoomId>,
    #[rust] room_name_id: Option<RoomNameId>,
    #[rust] all_members: Arc<Vec<RoomMember>>,
    /// Indices into `all_members` that match the current search filter.
    #[rust] filtered_indices: Vec<usize>,
    #[rust] search_query: String,
}

impl Widget for RoomMembersPane {
    fn handle_event(&mut self, cx: &mut Cx, event: &Event, scope: &mut Scope) {
        self.view.handle_event(cx, event, scope);
        self.widget_match_event(cx, event, scope);
    }

    fn draw_walk(&mut self, cx: &mut Cx2d, scope: &mut Scope, walk: Walk) -> DrawStep {
        self.view.draw_walk(cx, scope, walk)
    }
}

/// Number of member rows in the DSL pool. Members beyond this count are
/// trimmed from the visible list; the search box is the primary way to find
/// specific members in larger rooms.
const MEMBER_ROW_COUNT: usize = 32;

impl RoomMembersPane {
    const MEMBER_ROW_IDS: [LiveId; MEMBER_ROW_COUNT] = [
        live_id!(member_row_0),  live_id!(member_row_1),
        live_id!(member_row_2),  live_id!(member_row_3),
        live_id!(member_row_4),  live_id!(member_row_5),
        live_id!(member_row_6),  live_id!(member_row_7),
        live_id!(member_row_8),  live_id!(member_row_9),
        live_id!(member_row_10), live_id!(member_row_11),
        live_id!(member_row_12), live_id!(member_row_13),
        live_id!(member_row_14), live_id!(member_row_15),
        live_id!(member_row_16), live_id!(member_row_17),
        live_id!(member_row_18), live_id!(member_row_19),
        live_id!(member_row_20), live_id!(member_row_21),
        live_id!(member_row_22), live_id!(member_row_23),
        live_id!(member_row_24), live_id!(member_row_25),
        live_id!(member_row_26), live_id!(member_row_27),
        live_id!(member_row_28), live_id!(member_row_29),
        live_id!(member_row_30), live_id!(member_row_31),
    ];

    /// Populate the pane with room data and trigger a member fetch.
    pub fn show(
        &mut self,
        cx: &mut Cx,
        room_id: OwnedRoomId,
        room_name_id: RoomNameId,
    ) {
        let title = format!("Members – {}", room_name_id);
        self.view.label(cx, ids!(title_label)).set_text(cx, &title);

        self.room_id = Some(room_id.clone());
        self.room_name_id = Some(room_name_id);
        self.all_members = Arc::new(Vec::new());
        self.filtered_indices.clear();
        self.search_query.clear();

        self.view.text_input(cx, ids!(search_input)).set_text(cx, "");
        self.view.label(cx, ids!(status_label))
            .set_text(cx, "Loading members…");

        // Hide all rows until results arrive.
        for row_id in Self::MEMBER_ROW_IDS.iter() {
            self.view.view(cx, &[*row_id]).set_visible(cx, false);
        }

        submit_async_request(MatrixRequest::FetchMembersForPane {
            room_id,
            memberships: RoomMemberships::ACTIVE,
        });

        self.view.redraw(cx);
    }

    /// Apply fetched member list and rebuild the visible rows.
    pub fn apply_members(
        &mut self,
        cx: &mut Cx,
        members: Arc<Vec<RoomMember>>,
    ) {
        self.all_members = members;
        self.recompute_filter(cx);
    }

    /// Filter `all_members` by the current search query, sort by role then name,
    /// and refresh the visible row widgets.
    fn recompute_filter(&mut self, cx: &mut Cx) {
        let query = self.search_query.trim().to_lowercase();
        self.filtered_indices.clear();

        for (idx, member) in self.all_members.iter().enumerate() {
            if query.is_empty() {
                self.filtered_indices.push(idx);
                continue;
            }
            let name = member.display_name().unwrap_or("").to_lowercase();
            let id = member.user_id().as_str().to_lowercase();
            if name.contains(&query) || id.contains(&query) {
                self.filtered_indices.push(idx);
            }
        }

        // Sort: admins first, then mods, then users; secondary sort by display name.
        let members = Arc::clone(&self.all_members);
        self.filtered_indices.sort_by(|&a, &b| {
            let ma = &members[a];
            let mb = &members[b];
            let ra = role_rank(ma.suggested_role_for_power_level());
            let rb = role_rank(mb.suggested_role_for_power_level());
            ra.cmp(&rb).then_with(|| {
                let na = ma.display_name().unwrap_or(ma.user_id().as_str()).to_lowercase();
                let nb = mb.display_name().unwrap_or(mb.user_id().as_str()).to_lowercase();
                na.cmp(&nb)
            })
        });

        let total = self.all_members.len();
        let shown = self.filtered_indices.len().min(MEMBER_ROW_COUNT);
        let status_text = if total == 0 {
            "No members loaded.".to_string()
        } else if self.filtered_indices.is_empty() {
            format!("No members match \"{}\".", self.search_query)
        } else if self.filtered_indices.len() > MEMBER_ROW_COUNT {
            format!(
                "Showing {shown} of {} matches ({} members total). Refine search to see more.",
                self.filtered_indices.len(),
                total,
            )
        } else {
            format!(
                "Showing {} member(s){}.",
                self.filtered_indices.len(),
                if !self.search_query.trim().is_empty() {
                    format!(" out of {total}")
                } else {
                    String::new()
                },
            )
        };
        self.view.label(cx, ids!(status_label)).set_text(cx, &status_text);

        let current_user = current_user_id();
        let can_moderate = self.current_user_can_moderate(current_user.as_ref());

        let list_view = self.view.view(cx, ids!(list_scroll.list_view));
        for (slot, row_id) in Self::MEMBER_ROW_IDS.iter().enumerate() {
            let row = list_view.view(cx, &[*row_id]);
            if let Some(&member_idx) = self.filtered_indices.get(slot) {
                let member = &self.all_members[member_idx];
                let display_name = member
                    .display_name()
                    .map(|s| s.to_string())
                    .unwrap_or_else(|| member.user_id().to_string());
                row.label(cx, ids!(row.text_col.name_label))
                    .set_text(cx, &display_name);
                row.label(cx, ids!(row.text_col.id_label))
                    .set_text(cx, member.user_id().as_str());

                let role_label = match member.suggested_role_for_power_level() {
                    RoomMemberRole::Creator => "Creator",
                    RoomMemberRole::Administrator => "Admin",
                    RoomMemberRole::Moderator => "Moderator",
                    RoomMemberRole::User => "User",
                };
                row.label(cx, ids!(row.role_badge)).set_text(cx, role_label);

                self.set_row_avatar(cx, &row, member);

                let is_self = current_user
                    .as_ref()
                    .is_some_and(|u| u.as_str() == member.user_id().as_str());
                let show_actions = can_moderate && !is_self;
                row.button(cx, ids!(row.kick_button)).set_visible(cx, show_actions);
                row.button(cx, ids!(row.ban_button)).set_visible(cx, show_actions);

                row.set_visible(cx, true);
            } else {
                row.set_visible(cx, false);
            }
        }

        self.view.redraw(cx);
    }

    fn current_user_can_moderate(&self, current_user: Option<&OwnedUserId>) -> bool {
        let Some(uid) = current_user else { return false };
        self.all_members
            .iter()
            .find(|m| m.user_id().as_str() == uid.as_str())
            .map(|m| matches!(
                m.suggested_role_for_power_level(),
                RoomMemberRole::Administrator | RoomMemberRole::Creator | RoomMemberRole::Moderator,
            ))
            .unwrap_or(false)
    }

    fn set_row_avatar(&self, cx: &mut Cx, row: &ViewRef, member: &RoomMember) {
        let avatar = row.avatar(cx, ids!(row.avatar));
        let fallback = member
            .display_name()
            .unwrap_or_else(|| member.user_id().as_str());

        let owned_uri = member.avatar_url().map(|u| u.to_owned());
        if let Some(uri) = owned_uri.as_ref()
            && let AvatarCacheEntry::Loaded(image_data) = avatar_cache::get_or_fetch_avatar(cx, uri)
        {
            let res = avatar.show_image(
                cx,
                None,
                |cx, img_ref| load_png_or_jpg(&img_ref, cx, &image_data),
            );
            if res.is_ok() {
                return;
            }
        }
        avatar.show_text(cx, None, None, fallback);
    }

    /// Returns `(slot_index, kind)` if a kick or ban button was clicked.
    fn clicked_action(&self, cx: &mut Cx, actions: &Actions) -> Option<(usize, MemberActionKind)> {
        let list_view = self.view.view(cx, ids!(list_scroll.list_view));
        for (slot, row_id) in Self::MEMBER_ROW_IDS.iter().enumerate() {
            let kick = list_view.button(cx, &[*row_id, live_id!(row), live_id!(kick_button)]);
            if kick.clicked(actions) {
                return Some((slot, MemberActionKind::Kick));
            }
            let ban = list_view.button(cx, &[*row_id, live_id!(row), live_id!(ban_button)]);
            if ban.clicked(actions) {
                return Some((slot, MemberActionKind::Ban));
            }
        }
        None
    }
}

impl WidgetMatchEvent for RoomMembersPane {
    fn handle_actions(&mut self, cx: &mut Cx, actions: &Actions, _scope: &mut Scope) {
        if self.view.button(cx, ids!(close_button)).clicked(actions) {
            cx.action(RoomMembersPaneAction::Close);
            return;
        }

        if self.view.button(cx, ids!(invite_button)).clicked(actions)
            && let Some(room_name_id) = &self.room_name_id
        {
            cx.action(RoomMembersPaneAction::InviteRequested {
                room_name_id: room_name_id.clone(),
            });
            return;
        }

        if let Some(new_query) = self.view.text_input(cx, ids!(search_input)).changed(actions) {
            self.search_query = new_query;
            self.recompute_filter(cx);
        }

        if let Some((slot, kind)) = self.clicked_action(cx, actions)
            && let Some(&member_idx) = self.filtered_indices.get(slot)
            && let Some(room_id) = self.room_id.clone()
        {
            let user_id = self.all_members[member_idx].user_id().to_owned();
            let display = self.all_members[member_idx]
                .display_name()
                .map(|s| s.to_string())
                .unwrap_or_else(|| user_id.to_string());
            match kind {
                MemberActionKind::Kick => {
                    submit_async_request(MatrixRequest::KickUser {
                        room_id,
                        user_id,
                        reason: None,
                    });
                    enqueue_popup_notification(
                        format!("Kicking {display}…"),
                        PopupKind::Info,
                        Some(3.0),
                    );
                }
                MemberActionKind::Ban => {
                    submit_async_request(MatrixRequest::BanUser {
                        room_id,
                        user_id,
                        reason: None,
                    });
                    enqueue_popup_notification(
                        format!("Banning {display}…"),
                        PopupKind::Info,
                        Some(3.0),
                    );
                }
                MemberActionKind::Unban => {
                    submit_async_request(MatrixRequest::UnbanUser { room_id, user_id });
                }
            }
        }

        // Apply member-action results so the row is removed from view.
        for action in actions {
            if let Some(result) = action.downcast_ref::<MemberActionResultAction>() {
                self.handle_member_action_result(cx, result);
            }
        }
    }
}

impl RoomMembersPane {
    fn handle_member_action_result(
        &mut self,
        cx: &mut Cx,
        result: &MemberActionResultAction,
    ) {
        let our_room = self.room_id.as_ref();
        match result {
            MemberActionResultAction::Kicked { room_id, user_id }
                if our_room == Some(room_id) =>
            {
                enqueue_popup_notification(
                    format!("Kicked {user_id}."),
                    PopupKind::Success,
                    Some(4.0),
                );
                self.drop_member(cx, user_id);
            }
            MemberActionResultAction::Banned { room_id, user_id }
                if our_room == Some(room_id) =>
            {
                enqueue_popup_notification(
                    format!("Banned {user_id}."),
                    PopupKind::Success,
                    Some(4.0),
                );
                self.drop_member(cx, user_id);
            }
            MemberActionResultAction::Unbanned { room_id, user_id }
                if our_room == Some(room_id) =>
            {
                enqueue_popup_notification(
                    format!("Unbanned {user_id}."),
                    PopupKind::Success,
                    Some(4.0),
                );
            }
            MemberActionResultAction::Failed {
                room_id, user_id, kind, error,
            } if our_room == Some(room_id) => {
                enqueue_popup_notification(
                    format!("Failed to {} {user_id}: {error}", kind.as_str()),
                    PopupKind::Error,
                    Some(6.0),
                );
            }
            _ => {}
        }
    }

    fn drop_member(&mut self, cx: &mut Cx, user_id: &OwnedUserId) {
        // Rebuild the Arc<Vec<...>> without the kicked/banned user.
        let filtered: Vec<RoomMember> = self
            .all_members
            .iter()
            .filter(|m| m.user_id().as_str() != user_id.as_str())
            .cloned()
            .collect();
        self.all_members = Arc::new(filtered);
        self.recompute_filter(cx);
    }
}

fn role_rank(role: RoomMemberRole) -> u8 {
    match role {
        RoomMemberRole::Creator => 0,
        RoomMemberRole::Administrator => 1,
        RoomMemberRole::Moderator => 2,
        RoomMemberRole::User => 3,
    }
}

impl RoomMembersPaneRef {
    pub fn show(
        &self,
        cx: &mut Cx,
        room_id: OwnedRoomId,
        room_name_id: RoomNameId,
    ) {
        let Some(mut inner) = self.borrow_mut() else { return };
        inner.show(cx, room_id, room_name_id);
    }

    pub fn apply_members(
        &self,
        cx: &mut Cx,
        members: Arc<Vec<RoomMember>>,
    ) {
        let Some(mut inner) = self.borrow_mut() else { return };
        inner.apply_members(cx, members);
    }
}
