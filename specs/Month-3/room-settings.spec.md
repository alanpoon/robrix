spec: task
name: "Month 3 — Room Settings Screen"
inherits: project
tags: [month-3, rooms, settings, power-levels, encryption]
---

## Intent

Replace the missing room-settings entry point with a `RoomSettingsScreen`
widget that lets the user edit settings for an already-joined room. The
screen exposes four sub-sections in a vertical tab strip: **General**
(name, topic, avatar), **Security & Privacy** (join rule, history
visibility, enable-encryption toggle), **Roles & Permissions** (per-action
power levels), and **Advanced** (room ID, canonical alias, leave room).
Settings flow through new `MatrixRequest` variants that map onto the
corresponding `matrix_sdk::Room` methods inside `async_main_loop`
(`src/sliding_sync.rs:962`). Pure helpers under
`src/home/room_settings.rs` validate each setting change and answer the
permission question "can the current user perform this change?" without
needing a live `Client`, so the behavior is unit-testable in
`#[cfg(test)] mod tests_room_settings`.

## Decisions

### Module layout

- New module `src/home/room_settings.rs` registered in `src/home/mod.rs`.
- Exposes widget `RoomSettingsScreen` plus the following pure types:
  - `pub enum RoomSettingUpdate { Name(String), Topic(Option<String>), Avatar(AvatarUpdate), JoinRule(JoinRuleChoice), HistoryVisibility(HistoryVisibilityChoice), EnableEncryption, PowerLevels(PowerLevelChanges) }`
  - `pub enum AvatarUpdate { Set { bytes: Vec<u8>, mime: Option<String> }, Clear }`
  - `pub enum JoinRuleChoice { Public, Invite, Knock }`
  - `pub enum HistoryVisibilityChoice { WorldReadable, Shared, Invited, Joined }`
  - `pub struct PowerLevelChanges { pub events_default: Option<i64>, pub state_default: Option<i64>, pub invite: Option<i64>, pub kick: Option<i64>, pub ban: Option<i64>, pub redact: Option<i64>, pub per_user: BTreeMap<OwnedUserId, i64> }`

### Validation and permission helpers

- `pub fn validate_room_setting_update(update: &RoomSettingUpdate) -> Result<(), RoomSettingUpdateError>`
  where the error enum has variants `NameEmpty`, `NameTooLong { len: usize, max: usize }`
  (max = 255), `TopicTooLong { len: usize, max: usize }` (max = 2048),
  `AvatarTooLarge { bytes: usize, max: usize }` (max = 4 MiB),
  `PowerLevelOutOfRange { field: &'static str, value: i64 }` (any field
  whose value is outside the closed range `[0, 100]` is rejected).
- `pub fn can_apply_setting(
       update: &RoomSettingUpdate,
       current_user_power: i64,
       current_levels: &RoomPowerLevels,
   ) -> bool`
  returns `true` iff the current user's power level is at least the level
  required by the room to perform the change. Mapping:
  - `Name`, `Topic`, `Avatar` → `current_levels.events.get("m.room.name" / "m.room.topic" / "m.room.avatar").copied().unwrap_or(current_levels.state_default)`
  - `JoinRule` → required level for `m.room.join_rules`
  - `HistoryVisibility` → required level for `m.room.history_visibility`
  - `EnableEncryption` → required level for `m.room.encryption`
  - `PowerLevels` → required level for `m.room.power_levels`

### Backend variant

- Append to `MatrixRequest` (after `MatrixRequest::CreateRoom` from
  `room-creation.spec.md`):
  ```
  UpdateRoomSetting {
      room_id: OwnedRoomId,
      update: RoomSettingUpdate,
  }
  ```
- Result type: `RoomSettingsAction::Applied { room_id, kind: RoomSettingKind }`
  on success, `RoomSettingsAction::Failed { room_id, kind, reason }` on error.
  `kind` is a discriminant-only enum so the UI can re-enable the right form
  fields without re-cloning the full `update`.

### Encryption is one-way

- The UI exposes "Enable encryption" only when the room is unencrypted.
  Once `MatrixRequest::UpdateRoomSetting { update: EnableEncryption }`
  succeeds, the toggle is hidden and replaced by the static text
  "Encryption is enabled and cannot be disabled". The handler explicitly
  refuses to send a `disable` action because matrix-sdk does not support
  unencrypting a room.

### Leave-room button (Advanced)

- The Leave button dispatches the **existing** `MatrixRequest::LeaveRoom`
  variant from `src/sliding_sync.rs:693`; Month 3 does not introduce a new
  leave path.

## Constraints

- Must NOT call `Room::set_name` / `set_topic` / `upload_avatar` /
  `enable_encryption` / `apply_power_levels_changes` from widget code. The
  widget only dispatches `MatrixRequest::UpdateRoomSetting`.
- Must NOT offer a UI control to *disable* encryption — the toggle is
  hidden once the room is encrypted.
- Must NOT silently clamp a power-level value into `[0, 100]`. Out-of-range
  values must return `Err(PowerLevelOutOfRange { .. })` from validation;
  the submit button stays disabled.
- Must NOT modify the existing `MatrixRequest::LeaveRoom` handler at
  `src/sliding_sync.rs:1393` — the new Leave button reuses it as-is.
- Must NOT add any new state-event type beyond
  `m.room.name`, `m.room.topic`, `m.room.avatar`, `m.room.join_rules`,
  `m.room.history_visibility`, `m.room.encryption`, and `m.room.power_levels`.

## Boundaries

### Allowed Changes

- specs/Month-3/room-settings.spec.md
- src/home/room_settings.rs (new)
- src/home/mod.rs
- src/sliding_sync.rs
- src/home/room_screen.rs
- src/lib.rs

### Forbidden

- Do not split the four sub-sections into four separate widgets in
  separate files. They live as inner `View`s of `RoomSettingsScreen` in
  one module so the form state stays local.
- Do not introduce new ruma feature flags. The state-event types listed
  above are already enabled by the existing `matrix-sdk` feature set.
- Do not add per-user mention / notification settings here; user-scoped
  notification routing is its own roadmap item.
- Do not add a "transfer room ownership" flow this month — power-level
  edits via `PowerLevelChanges` are sufficient.

## Completion Criteria

Scenario: Setting validator rejects an empty name
  Test:
    Package: robrix
    Filter: test_validate_setting_rejects_empty_name
  Given an update `RoomSettingUpdate::Name(String::new())`
  When `validate_room_setting_update` is called
  Then the result equals `Err(RoomSettingUpdateError::NameEmpty)`

Scenario: Setting validator accepts a non-empty room name
  Test:
    Package: robrix
    Filter: test_validate_setting_accepts_short_name
  Given an update `RoomSettingUpdate::Name("Project Beta".to_string())`
  When `validate_room_setting_update` is called
  Then the result equals `Ok(())`

Scenario: Setting validator rejects a topic longer than 2048 characters
  Test:
    Package: robrix
    Filter: test_validate_setting_rejects_overlong_topic
  Given an update `RoomSettingUpdate::Topic(Some("x".repeat(2049)))`
  When `validate_room_setting_update` is called
  Then the result equals `Err(RoomSettingUpdateError::TopicTooLong { len: 2049, max: 2048 })`

Scenario: Setting validator allows clearing the topic with None
  Test:
    Package: robrix
    Filter: test_validate_setting_allows_clearing_topic
  Given an update `RoomSettingUpdate::Topic(None)`
  When `validate_room_setting_update` is called
  Then the result equals `Ok(())`

Scenario: Setting validator rejects an avatar payload larger than 4 MiB
  Test:
    Package: robrix
    Filter: test_validate_setting_rejects_oversize_avatar
  Given an update `RoomSettingUpdate::Avatar(AvatarUpdate::Set { bytes: vec![0u8; 5 * 1024 * 1024], mime: Some("image/png".into()) })`
  When `validate_room_setting_update` is called
  Then the result equals `Err(RoomSettingUpdateError::AvatarTooLarge { bytes: 5_242_880, max: 4_194_304 })`

Scenario: Setting validator rejects a power-level value above 100
  Test:
    Package: robrix
    Filter: test_validate_setting_rejects_power_level_above_100
  Given an update `RoomSettingUpdate::PowerLevels(changes)` whose `kick = Some(101)`
  When `validate_room_setting_update` is called
  Then the result equals `Err(RoomSettingUpdateError::PowerLevelOutOfRange { field: "kick", value: 101 })`

Scenario: Setting validator rejects a negative power-level value
  Test:
    Package: robrix
    Filter: test_validate_setting_rejects_negative_power_level
  Given an update `RoomSettingUpdate::PowerLevels(changes)` whose `ban = Some(-1)`
  When `validate_room_setting_update` is called
  Then the result equals `Err(RoomSettingUpdateError::PowerLevelOutOfRange { field: "ban", value: -1 })`

Scenario: Permission helper allows a moderator to change the name when state_default permits it
  Test:
    Package: robrix
    Filter: test_can_apply_setting_allows_moderator_to_set_name
  Given a `RoomPowerLevels` where `state_default = 50` and `events` lacks `"m.room.name"`
  And the current user's power level equals `50`
  When `can_apply_setting` is called with `RoomSettingUpdate::Name("X".to_string())`
  Then the result equals `true`

Scenario: Permission helper forbids a non-admin from changing power levels
  Test:
    Package: robrix
    Filter: test_can_apply_setting_forbids_non_admin_power_level_change
  Given a `RoomPowerLevels` where the required level for `"m.room.power_levels"` is `100`
  And the current user's power level equals `50`
  When `can_apply_setting` is called with a `RoomSettingUpdate::PowerLevels(_)`
  Then the result equals `false`

Scenario: Permission helper forbids enabling encryption when the user lacks the required level
  Test:
    Package: robrix
    Filter: test_can_apply_setting_forbids_enable_encryption_without_power
  Given a `RoomPowerLevels` where the required level for `"m.room.encryption"` is `100`
  And the current user's power level equals `0`
  When `can_apply_setting` is called with `RoomSettingUpdate::EnableEncryption`
  Then the result equals `false`

Scenario: EnableEncryption is one-way — the UI cannot construct a disable update
  Test:
    Package: robrix
    Filter: test_enable_encryption_is_one_way
  Given the `RoomSettingUpdate` enum
  When the test inspects the variants
  Then there exists exactly one encryption-related variant named `EnableEncryption`
  And there is no `DisableEncryption` or equivalent variant

## Out of Scope

- Per-room notification rules (push/keyword/highlight). Notification rules
  are user-scoped, not room-scoped, and not part of this month.
- Room version upgrade (sending `m.room.tombstone`).
- Server-side ACL editing (`m.room.server_acl`).
- Cross-signing key reset, device verification, or any e2ee key-management
  flow beyond enabling encryption.
- Per-user moderation badges, custom roles, or non-integer power levels.
- Room export / import.
