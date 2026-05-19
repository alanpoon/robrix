spec: task
name: "Month 3 — Room Creation Flow"
inherits: project
tags: [month-3, rooms, create, e2ee, invite]
---

## Intent

Add a first-class "Create room" flow to `robrix`. Today `src/home/add_room.rs`
can resolve an alias or matrix-link and dispatch
`MatrixRequest::JoinRoom`/`Knock`, but there is no path to create a new room.
Month 3 introduces a `CreateRoomScreen` widget that lets the user enter a
**name**, **topic**, and **avatar**; pick a **visibility** (public or private);
flip an **end-to-end-encryption toggle**; and supply an optional list of
**initial invitees**. The screen submits a new
`MatrixRequest::CreateRoom { config: CreateRoomConfig }` variant that maps onto
`matrix_sdk::Client::create_room` inside the existing `async_main_loop`
(`src/sliding_sync.rs:962`), and surfaces a `CreateRoomAction::Created { room_id }`
or `CreateRoomAction::Failed { reason }` back to the UI.

All input validation, mxc-uri parsing of an uploaded avatar response, and
power-level defaulting are implemented as **pure functions** under
`src/home/create_room.rs` so that the per-rule behavior is exercised by
`#[cfg(test)] mod tests_room_creation` without spinning up a `Client`.

## Decisions

### Config struct (pure, serializable)

- New module `src/home/create_room.rs`. Public struct:
  ```
  pub struct CreateRoomConfig {
      pub name: String,
      pub topic: Option<String>,
      pub avatar_bytes: Option<Vec<u8>>,
      pub avatar_mime: Option<String>,
      pub visibility: RoomVisibilityChoice,
      pub e2ee_enabled: bool,
      pub initial_invitees: Vec<OwnedUserId>,
  }
  pub enum RoomVisibilityChoice { Public, Private }
  ```
- `RoomVisibilityChoice::Public` maps to ruma `Visibility::Public` plus
  `JoinRule::Public`; `RoomVisibilityChoice::Private` maps to
  `Visibility::Private` plus `JoinRule::Invite` (the matrix-sdk defaults).
- `e2ee_enabled = true` sets `initial_state` to include an `m.room.encryption`
  state event with algorithm `m.megolm.v1.aes-sha2`. `e2ee_enabled = false`
  leaves `initial_state` empty so the server's default applies.

### Pure validation helpers

- `pub fn validate_create_room_config(config: &CreateRoomConfig) -> Result<(), CreateRoomConfigError>`
  in `src/home/create_room.rs`. Error enum has variants `EmptyName`,
  `NameTooLong { len: usize, max: usize }` (max = 255 chars after trim),
  `InvalidInvitee { raw: String }`, `AvatarTooLarge { bytes: usize, max: usize }`
  (max = 4 MiB = `4 * 1024 * 1024`), and `EncryptedPublicRoom` (refuses
  `e2ee_enabled = true` combined with `Public` visibility because public-room
  encryption is not supported by robrix this month).
- `pub fn parse_invitee_list(raw: &str) -> (Vec<OwnedUserId>, Vec<String>)`
  in `src/home/create_room.rs`. Splits on whitespace and commas. The first
  tuple element holds successfully parsed `OwnedUserId`s; the second holds
  raw substrings that failed to parse. Order is preserved.

### Backend variant

- Append to `MatrixRequest` (after `GetUrlPreview` at `src/sliding_sync.rs:916`):
  ```
  CreateRoom {
      config: CreateRoomConfig,
  }
  ```
- The new handler in `async_main_loop` calls `Client::create_room` with a
  ruma `create_room::v3::Request` built from the config, then on success
  also iterates `config.initial_invitees` and calls
  `Joined::invite_user_by_id` for each invitee. Per-invitee failures do not
  fail the overall create; they are surfaced as
  `CreateRoomAction::PartialInvite { room_id, failed: Vec<OwnedUserId> }`.
- On error from `Client::create_room`, the handler posts
  `CreateRoomAction::Failed { reason: String }`. On success with all
  invites accepted, it posts `CreateRoomAction::Created { room_id }`.

### UI

- New widget `CreateRoomScreen` at `src/home/create_room.rs`, registered in
  `src/home/mod.rs` alongside `add_room::script_mod(vm)` so it is reachable
  from the same nav surface as the existing Add-Room screen.
- The screen renders, top to bottom: name input, topic input, avatar
  preview + "Upload" button, visibility radio (Public / Private — Private
  is the default), an E2EE toggle (default off), an "Invite people"
  textarea, and a "Create room" submit button. The submit button is
  disabled while `validate_create_room_config` returns `Err`.

### Cargo

- No new dependency: `matrix_sdk` already pulls in
  `ruma_client_api::room::create_room::v3` and `ruma::events::room::encryption`.

## Constraints

- Must NOT call `Client::create_room` from widget code. The widget always
  goes through `submit_async_request(MatrixRequest::CreateRoom { config })`.
- Must NOT `.unwrap()` on a parsed `OwnedUserId`; `parse_invitee_list`
  returns the failed raw strings so the UI can show them inline.
- Must NOT allow E2EE on a Public room — `validate_create_room_config`
  returns `Err(EncryptedPublicRoom)` and the submit button stays disabled.
- Must NOT silently truncate the room name. A name longer than 255
  characters after trim must return `Err(NameTooLong { .. })` from
  validation; the UI never sends an over-length name to the server.
- Must NOT send an empty `initial_state` `m.room.encryption` event when
  `e2ee_enabled = false`. The state event is added only when the toggle is
  on, so the server's default visibility policy continues to apply otherwise.
- Must NOT block the UI on the `Joined::invite_user_by_id` loop. Per-invitee
  failures are collected and surfaced as `PartialInvite`, not awaited
  serially in a way that stalls subsequent invites.

## Boundaries

### Allowed Changes

- specs/Month-3/room-creation.spec.md
- src/home/create_room.rs (new)
- src/home/mod.rs
- src/sliding_sync.rs
- src/home/add_room.rs
- src/lib.rs

### Forbidden

- Do not move the create-room widget out of `src/home/`. The room-management
  widgets live there alongside `add_room.rs`, `invite_modal.rs`, etc.
- Do not introduce a new `Cargo.toml` dependency for room creation. The
  existing `matrix_sdk` re-export of `ruma_client_api::room::create_room`
  is sufficient.
- Do not silently drop invitees that fail to parse — failed raw strings
  must be returned from `parse_invitee_list` and rendered in the UI.
- Do not add room-upgrade ("upgrade to v11"), room-tombstone, or
  cross-room linking flows here; those are explicit out-of-scope.

## Completion Criteria

Scenario: Create-room config rejects an empty name
  Test:
    Package: robrix
    Filter: test_create_room_config_rejects_empty_name
  Given a `CreateRoomConfig` whose `name` is `""`
  When `validate_create_room_config` is called
  Then the result equals `Err(CreateRoomConfigError::EmptyName)`

Scenario: Create-room config rejects a name longer than 255 characters
  Test:
    Package: robrix
    Filter: test_create_room_config_rejects_overlong_name
  Given a `CreateRoomConfig` whose `name` is `"a".repeat(256)`
  When `validate_create_room_config` is called
  Then the result equals `Err(CreateRoomConfigError::NameTooLong { len: 256, max: 255 })`

Scenario: Create-room config rejects E2EE on a Public room
  Test:
    Package: robrix
    Filter: test_create_room_config_rejects_e2ee_on_public_room
  Given a `CreateRoomConfig` with `name = "test"`, `visibility = Public`, and `e2ee_enabled = true`
  When `validate_create_room_config` is called
  Then the result equals `Err(CreateRoomConfigError::EncryptedPublicRoom)`

Scenario: Create-room config rejects an avatar payload larger than 4 MiB
  Test:
    Package: robrix
    Filter: test_create_room_config_rejects_oversize_avatar
  Given a `CreateRoomConfig` with a 5 MiB `avatar_bytes` blob
  When `validate_create_room_config` is called
  Then the result equals `Err(CreateRoomConfigError::AvatarTooLarge { bytes: 5_242_880, max: 4_194_304 })`

Scenario: Create-room config accepts a minimal valid private room
  Test:
    Package: robrix
    Filter: test_create_room_config_accepts_minimal_private_room
  Given a `CreateRoomConfig` with `name = "Project Alpha"`, `visibility = Private`, `e2ee_enabled = false`, and no invitees
  When `validate_create_room_config` is called
  Then the result equals `Ok(())`

Scenario: Create-room config accepts a valid private encrypted room with invitees
  Test:
    Package: robrix
    Filter: test_create_room_config_accepts_private_encrypted
  Given a `CreateRoomConfig` with `name = "Secrets"`, `topic = Some("plans")`, `visibility = Private`, `e2ee_enabled = true`, and one invitee `"@alice:matrix.org"`
  When `validate_create_room_config` is called
  Then the result equals `Ok(())`

Scenario: Invitee parser separates valid and invalid Matrix user IDs
  Test:
    Package: robrix
    Filter: test_parse_invitee_list_splits_valid_and_invalid
  Given the raw string `"@alice:matrix.org, bob@example.com  @carol:matrix.org"`
  When `parse_invitee_list` is called
  Then the parsed list contains `OwnedUserId` values for `"@alice:matrix.org"` and `"@carol:matrix.org"`
  And the failed list equals `vec!["bob@example.com".to_string()]`

Scenario: Invitee parser preserves order across mixed delimiters
  Test:
    Package: robrix
    Filter: test_parse_invitee_list_preserves_order
  Given the raw string `"@a:m.org\n@b:m.org , @c:m.org"`
  When `parse_invitee_list` is called
  Then the parsed list equals `[OwnedUserId("@a:m.org"), OwnedUserId("@b:m.org"), OwnedUserId("@c:m.org")]`

Scenario: Invitee parser returns empty lists for a blank string
  Test:
    Package: robrix
    Filter: test_parse_invitee_list_returns_empty_for_blank
  Given the raw string `"   \n  "`
  When `parse_invitee_list` is called
  Then the parsed list equals `vec![]`
  And the failed list equals `vec![]`

Scenario: CreateRoomConfig builds a ruma request with encryption state when E2EE is on
  Test:
    Package: robrix
    Filter: test_create_room_config_to_ruma_request_includes_encryption_state
  Given a `CreateRoomConfig` with `e2ee_enabled = true`
  When `CreateRoomConfig::to_ruma_request` is called
  Then the resulting `initial_state` contains exactly one entry whose `state_event_type` equals `"m.room.encryption"`
  And the encryption algorithm equals `"m.megolm.v1.aes-sha2"`

Scenario: CreateRoomConfig builds a ruma request without encryption state when E2EE is off
  Test:
    Package: robrix
    Filter: test_create_room_config_to_ruma_request_omits_encryption_when_off
  Given a `CreateRoomConfig` with `e2ee_enabled = false`
  When `CreateRoomConfig::to_ruma_request` is called
  Then the resulting `initial_state` is empty

Scenario: CreateRoomConfig maps Public visibility to Public+Public-join-rule
  Test:
    Package: robrix
    Filter: test_create_room_config_to_ruma_request_public_visibility
  Given a `CreateRoomConfig` with `visibility = Public` and `e2ee_enabled = false`
  When `CreateRoomConfig::to_ruma_request` is called
  Then `visibility` equals `Visibility::Public`
  And `preset` is `Some(RoomPreset::PublicChat)`

## Out of Scope

- Room creation via a chosen room version newer than the server's default
  (no `--version` selector this month).
- Tombstoning or upgrading an existing room.
- Federation alias reservation (`#alias:server`) — the create flow only
  accepts a localpart-less name and a visibility; aliases are deferred.
- Avatar cropping / resizing UX. The avatar bytes are uploaded as-is (the
  4 MiB limit is the only constraint).
- Public-room directory submission — creating a Public room exposes it via
  the standard matrix-sdk default; explicit directory publishing toggles
  are part of `room-directory-browser.spec.md` if at all.
- Auto-creation of a "Random" topic, sample message, or default integrations.
