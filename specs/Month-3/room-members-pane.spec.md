spec: task
name: "Month 3 — Room Members Pane"
inherits: project
tags: [month-3, rooms, members, moderation, power-levels]
---

## Intent

Add a `RoomMembersPane` widget that lists every member of the currently
open room with their power level, lets the user search/filter the list,
and exposes per-member moderation actions: **invite**, **kick**, and **ban**.
The pane reuses the existing `MatrixRequest::GetRoomMembers`
(`src/sliding_sync.rs:702`) and `MatrixRequest::InviteUser`
(`src/sliding_sync.rs:678`) backends; Month 3 adds two new variants —
`KickUser` and `BanUser` — for the destructive actions. The
`RoomMember`-shaped list and the per-action enable/disable logic are
implemented as pure functions in `src/room/room_members_pane.rs` so the
filter and permission rules are unit-testable in
`#[cfg(test)] mod tests_room_members_pane` without a `Client`.

## Decisions

### Module layout

- New module `src/room/room_members_pane.rs` registered in `src/room/mod.rs`.
- Exposes widget `RoomMembersPane` and these pure types/helpers:
  - `pub struct MemberListEntry { pub user_id: OwnedUserId, pub display_name: Option<String>, pub avatar: Option<OwnedMxcUri>, pub power_level: i64, pub membership: MembershipState }`
  - `pub fn filter_members(entries: &[MemberListEntry], query: &str) -> Vec<MemberListEntry>`
  - `pub fn can_invite(actor_power: i64, levels: &RoomPowerLevels) -> bool`
  - `pub fn can_kick(actor_power: i64, target: &MemberListEntry, levels: &RoomPowerLevels) -> bool`
  - `pub fn can_ban(actor_power: i64, target: &MemberListEntry, levels: &RoomPowerLevels) -> bool`

### Filter rule

- `filter_members` matches the trimmed lowercase `query` against:
  1. `display_name.as_deref().unwrap_or("")` (lowercased), and
  2. the user ID's localpart (the substring between `@` and `:`,
     lowercased).
- An entry is included iff *any* of those fields contains the query as a
  substring. An empty / all-whitespace query returns every entry, in
  input order.
- Ordering of matched results equals input order. Stable sort, no
  re-ranking by score.

### Permission rules (per matrix-sdk convention)

- `can_invite(actor_power, levels)` returns `actor_power >= levels.invite`.
- `can_kick(actor_power, target, levels)` returns
  `actor_power >= levels.kick && actor_power > target.power_level`
  (note: strictly greater than target, equal-power admins cannot kick each
  other). For an actor acting on themselves it returns `false`.
- `can_ban(actor_power, target, levels)` returns
  `actor_power >= levels.ban && actor_power > target.power_level`. Self-ban
  returns `false`.

### Backend variants

- Append to `MatrixRequest`:
  ```
  KickUser {
      room_id: OwnedRoomId,
      user_id: OwnedUserId,
      reason: Option<String>,
  },
  BanUser {
      room_id: OwnedRoomId,
      user_id: OwnedUserId,
      reason: Option<String>,
  }
  ```
- Each maps to `Joined::kick_user` / `Joined::ban_user` inside
  `async_main_loop` and posts back `MemberActionResult::Kicked { room_id, user_id }`
  / `MemberActionResult::Banned { room_id, user_id }` on success, or
  `MemberActionResult::Failed { room_id, user_id, action, reason }` on
  error, where `action` is the enum `MemberAction::{Invite, Kick, Ban}`.

### UI behavior

- The pane is opened from a "Members" button in the existing room
  header (`src/home/room_screen.rs`). The list re-renders whenever a
  `TimelineUpdate::UserPowerLevels` or a `RoomMembersUpdate` arrives.
- Each row shows the avatar, display name, full user ID, and a numeric
  badge with the power level. A right-aligned overflow menu offers
  "Invite", "Kick", "Ban" — each entry is shown iff the corresponding
  `can_*` helper returns `true`.
- The search box debounces by exactly **one** UI frame; the helper
  `filter_members` is called once per debounced frame and the result
  replaces the rendered list.

## Constraints

- Must NOT call `Joined::kick_user`, `Joined::ban_user`, or
  `Joined::invite_user_by_id` from widget code. The widget always
  dispatches a `MatrixRequest` variant.
- Must NOT show a "Kick" or "Ban" menu entry for a target whose
  `power_level >= actor_power`. The menu is hidden, not just disabled,
  to avoid an obvious request that the server will refuse.
- Must NOT panic on a member entry whose `display_name` is `None`. The
  filter uses `display_name.as_deref().unwrap_or("")` and the row UI
  falls back to the user ID's localpart for display.
- Must NOT support multi-select / bulk-kick / bulk-ban operations this
  month. One target per action.
- Must NOT cache the member list across rooms — when the open room
  changes, the pane discards its filtered list and re-issues
  `MatrixRequest::GetRoomMembers` for the new room.

## Boundaries

### Allowed Changes

- specs/Month-3/room-members-pane.spec.md
- src/room/room_members_pane.rs (new)
- src/room/mod.rs
- src/sliding_sync.rs
- src/home/room_screen.rs
- src/lib.rs

### Forbidden

- Do not relocate the members pane to `src/home/`. Per-room widgets that
  scope to a single open room live under `src/room/`, alongside
  `room_input_bar.rs` and `reply_preview.rs`.
- Do not add an "unban" action this month — banned-user management is
  its own concern and is out of scope.
- Do not surface power-level *editing* controls in this pane. Editing
  power levels is exclusively the responsibility of the Roles &
  Permissions section in `room-settings.spec.md`.
- Do not add a presence / online-status indicator in this pane.

## Completion Criteria

Scenario: Filter returns every entry for an empty query
  Test:
    Package: robrix
    Filter: test_filter_members_empty_query_returns_all
  Given a member list of three entries `["@alice", "@bob", "@carol"]`
  When `filter_members` is called with `""`
  Then the result has length `3`
  And the order equals the input order

Scenario: Filter matches the user-id localpart
  Test:
    Package: robrix
    Filter: test_filter_members_matches_localpart
  Given a member list containing `MemberListEntry { user_id: OwnedUserId("@alice:matrix.org"), display_name: None, .. }`
  When `filter_members` is called with `"ali"`
  Then the result contains that entry

Scenario: Filter matches a lowercased display name even when the query is mixed case
  Test:
    Package: robrix
    Filter: test_filter_members_matches_display_name_case_insensitive
  Given a member list containing one entry whose `display_name` equals `Some("Alice Wonderland".to_string())`
  When `filter_members` is called with `"WONDER"`
  Then the result contains that entry

Scenario: Filter returns no entries when nothing matches
  Test:
    Package: robrix
    Filter: test_filter_members_returns_empty_when_no_match
  Given a member list of three entries none of whose display names or localparts contain `"zzz"`
  When `filter_members` is called with `"zzz"`
  Then the result equals `vec![]`

Scenario: Invite permission requires actor power >= levels.invite
  Test:
    Package: robrix
    Filter: test_can_invite_requires_invite_power
  Given a `RoomPowerLevels` whose `invite` equals `50`
  When `can_invite` is called with `actor_power = 49`
  Then the result equals `false`
  And calling it with `actor_power = 50` returns `true`

Scenario: Kick requires actor power strictly greater than target power
  Test:
    Package: robrix
    Filter: test_can_kick_requires_strict_power_dominance
  Given a `RoomPowerLevels` whose `kick` equals `50`
  And a target whose `power_level` equals `50`
  When `can_kick` is called with `actor_power = 50`
  Then the result equals `false`

Scenario: Kick is permitted when actor outranks the target
  Test:
    Package: robrix
    Filter: test_can_kick_permitted_when_actor_outranks_target
  Given a `RoomPowerLevels` whose `kick` equals `50`
  And a target whose `power_level` equals `0`
  When `can_kick` is called with `actor_power = 50`
  Then the result equals `true`

Scenario: Self-kick is rejected even for an admin
  Test:
    Package: robrix
    Filter: test_can_kick_rejects_self_target
  Given a target whose `user_id` equals the actor's `user_id`
  When `can_kick` is called with `actor_power = 100`
  Then the result equals `false`

Scenario: Ban requires actor power strictly greater than target power
  Test:
    Package: robrix
    Filter: test_can_ban_requires_strict_power_dominance
  Given a `RoomPowerLevels` whose `ban` equals `50`
  And a target whose `power_level` equals `50`
  When `can_ban` is called with `actor_power = 50`
  Then the result equals `false`

Scenario: Self-ban is rejected even for an admin
  Test:
    Package: robrix
    Filter: test_can_ban_rejects_self_target
  Given a target whose `user_id` equals the actor's `user_id`
  When `can_ban` is called with `actor_power = 100`
  Then the result equals `false`

Scenario: Filter preserves input order across multiple matches
  Test:
    Package: robrix
    Filter: test_filter_members_preserves_order
  Given a member list whose display names are `["zoe", "alice", "ali"]` in that order
  When `filter_members` is called with `"al"`
  Then the result equals the entries `["alice", "ali"]` in that order

## Out of Scope

- Unban / list-of-banned-users management.
- Multi-select / bulk moderation actions.
- Presence (online/offline) indicators.
- Direct-message creation from a row (the existing
  `MatrixRequest::OpenOrCreateDirectMessage` already covers this; no new
  affordance is added here).
- Display-name and avatar editing of *other* members.
- Cross-room member search.
