spec: task
name: "User Moderation Actions in Profile Pane"
inherits: project
tags: [feature, moderation, profile, matrix, ui]
estimate: 1d
---

## Intent

Add moderation actions to the user profile sliding pane (`UserProfileSlidingPane` in `src/profile/user_profile.rs`): Disinvite/Kick, Ban, Unban, and Mute/Unmute. Buttons appear conditionally based on the viewing user's power level and the target's current room membership state. Destructive actions (Kick/Ban/Unban) route through a new confirmation modal that supports an optional reason string. Mute is implemented by setting the target's room power level to `-1` and is non-destructive, so it dispatches directly without confirmation.

## Decisions

- New `MatrixRequest` variants in `src/sliding_sync.rs`: `KickUser`, `BanUser`, `UnbanUser`, each with `{ room_id, user_id, reason: Option<String> }`
- Mute/Unmute reuses `MatrixRequest::SetRoomMemberPowerLevel`, widened to accept a raw `i64` power level via new field `raw_power_level: Option<i64>` (when `Some`, it overrides the role-derived value)
- New widget `ModerationActionModal` (file `src/profile/moderation_action_modal.rs`), modeled on `JoinLeaveRoomModal`. `ModerationActionKind` enum: `Kick { is_invite }`, `Ban`, `Unban`
- New action `ShowModerationActionModal { kind }` emitted from `user_profile.rs` and handled at the App level (registered alongside `JoinLeaveRoomModal`)
- `UserProfilePaneInfo` extended with `user_power: UserPowerLevels` (full bitflags from `sliding_sync.rs:8317`) so the pane can call `can_kick()`, `can_ban()`, `can_unban()`, and `can_change_room_power_levels()` directly
- Button visibility, computed in `draw_walk`:
  - Disinvite/Kick: `user_power.can_kick()` AND target's membership is `Invite` or `Join`. Label = "Disinvite from room" if `Invite`, "Kick from room" if `Join`
  - Ban: `user_power.can_ban()` AND target's membership is not `Ban`
  - Unban: `user_power.can_unban()` AND target's membership is `Ban`
  - Mute/Unmute: `user_power.can_change_room_power_levels()` AND target's role is not `Creator`/`Administrator`. Label = "Unmute" if target's `power_level() <= -1`, else "Mute"
- Buttons use `RobrixNegativeIconButton` (red) styling matching the Element reference
- Failures bubble up via `enqueue_popup_notification(..., PopupKind::Error, ...)` from the async handlers, matching `IgnoreUser`'s error path

## Boundaries

### Allowed Changes
- src/profile/user_profile.rs
- src/profile/moderation_action_modal.rs (new)
- src/profile/mod.rs
- src/sliding_sync.rs
- src/home/room_screen.rs (to thread `UserPowerLevels` into `UserProfilePaneInfo`)
- src/app.rs (register `ModerationActionModal`)

### Forbidden
- Do not modify `JoinLeaveRoomModal` — copy its pattern, do not generalize it
- Do not rename or repurpose existing `MatrixRequest` variants
- Do not add "Remove messages" (redaction) — out of scope this round

## Out of Scope

- Redact/remove user messages
- Power-level slider UI for arbitrary roles (already exists via `SetRoomMemberPowerLevel`)
- Bulk moderation across rooms
- Banned-user list view
- Audit logging of moderation actions

## Completion Criteria

Scenario: Kick button visible only with permission
  Given the viewing user has kick power in the room
  And the profile pane is opened for a joined member
  Then a red "Kick from room" button is visible
  And clicking it opens the moderation confirmation modal

Scenario: Kick button labeled "Disinvite" for invited members
  Given the profile pane is opened for a member whose membership state is Invite
  And the viewing user has kick power
  Then the button reads "Disinvite from room"

Scenario: Kick button hidden without permission
  Given the viewing user does not have kick power
  When the profile pane is opened for any member
  Then no Kick/Disinvite button is rendered

Scenario: Ban button visible only with permission
  Given the viewing user has ban power
  And the target's membership is not Ban
  Then a red "Ban from room" button is visible

Scenario: Ban replaced by Unban when target is banned
  Given the target's membership is Ban
  And the viewing user has unban power
  Then the Ban button is hidden
  And an "Unban from room" button is visible

Scenario: Mute toggles based on target power level
  Given the viewing user can change power levels
  And the target's power level is 0
  Then the button reads "Mute"
  When clicked, the target's power level is set to -1
  And subsequently the button reads "Unmute"
  When clicked again, the target's power level is set to 0

Scenario: Mute hidden for admins
  Given the target's role is Administrator or Creator
  Then no Mute button is rendered

Scenario: Confirmation modal sends optional reason
  When the user types a reason and confirms a Kick
  Then `MatrixRequest::KickUser` is dispatched with `reason: Some("…")`
  When the reason field is empty and the user confirms
  Then `reason: None` is sent

Scenario: Cancel closes modal without action
  When the user clicks Cancel in the moderation modal
  Then no `MatrixRequest` is dispatched
  And the modal closes
