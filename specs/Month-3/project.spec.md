spec: project
name: "Month 3 — Room Management"
tags: [month-3, rooms, settings, members, directory, matrix]
---

## Intent

Deliver the README roadmap item "Room creation and settings" between
2026-07-11 and 2026-08-10. Today the `robrix` crate can only *join* or *leave*
rooms — `src/home/add_room.rs` accepts an alias/ID and dispatches
`MatrixRequest::JoinRoom`/`Knock`, but there is no UI or backend path to
*create* a room, no settings screen for an already-joined room, no dedicated
member-management pane, and no public-rooms directory browser. Month 3 closes
these four gaps. Every new public helper ships with a `#[cfg(test)] mod tests_*`
block in the same style already used by `src/utils.rs` (`tests_human_readable_list`,
`tests_linkify`, `tests_room_name`) and `src/event_preview.rs`
(`tests_audio_summary`, `tests_format_mmss`) — pure-Rust unit tests, no
network, no `tokio::test`, no live homeserver.

## Decisions

- Target crate: single binary crate `robrix` (see `Cargo.toml:2`); no workspace
  split this month either.
- All four task specs (`room-creation.spec.md`, `room-settings.spec.md`,
  `room-members-pane.spec.md`, `room-directory-browser.spec.md`) inherit from
  this `project` spec.
- Backend transport is the existing `MatrixRequest` enum in
  `src/sliding_sync.rs:624`. New variants are added at the end of the enum so
  serialization order is preserved; reply types flow back as `Action`s posted
  via `Cx::post_action`, matching the existing
  `RoomPreviewAction`/`JoinRoomResultAction` pattern.
- Matrix types come from `matrix_sdk::ruma` (already a dependency). Room
  creation uses `ruma_client_api::room::create_room::v3::Request` via
  `Client::create_room`. Power levels use
  `ruma::events::room::power_levels::RoomPowerLevels` (already imported at
  `src/sliding_sync.rs:21`). Public rooms use `Client::public_rooms_filtered`.
- Verification is `cargo test -p robrix` only. Acceptance scenarios bind to
  pure functions (config validators, member-filter helpers, permission
  checks, response parsers) that take ruma/sdk inputs and return plain
  values — no `Cx`, no `Client`, no widget event loop in any test.
- Every new task spec ships with at least one happy-path test **and** at least
  one exception-path test, so the exception-vs-happy ratio in `src/` does not
  regress from Month 2.

## Constraints

- Must NOT delete or rename any existing `#[test] fn test_*` in `src/utils.rs`,
  `src/event_preview.rs`, `src/shared/audio_message_player.rs`, or anywhere
  else under `src/`. The Month-2 baseline remains the reinforcement floor.
- Must NOT change the existing `MatrixRequest::JoinRoom`,
  `MatrixRequest::LeaveRoom`, `MatrixRequest::InviteUser`,
  `MatrixRequest::Knock`, `MatrixRequest::GetRoomMembers`,
  `MatrixRequest::GetRoomPreview`, or `MatrixRequest::GetRoomPowerLevels`
  variants in `src/sliding_sync.rs:624` — Month 3 adds new variants, it does
  not rewrite old ones.
- Must NOT introduce a new heavyweight runtime dependency at the project
  layer. Per-task specs may add a narrowly scoped dependency if their own
  `Decisions` block names it; today no such dependency is foreseen.
- Must NOT regress the existing add-room flow at `src/home/add_room.rs` —
  pasting an alias or matrix-link must keep dispatching the same join/knock
  request it does today.
- Must NOT block the UI thread on any HTTP or matrix-sdk call. Every new
  backend code path runs inside the existing `async_main_loop`
  (`src/sliding_sync.rs:962`) and returns through a `Cx::post_action`.

## Boundaries

### Allowed Changes

- specs/Month-3/**
- src/sliding_sync.rs
- src/home/add_room.rs
- src/home/mod.rs
- src/home/rooms_list.rs
- src/home/rooms_sidebar.rs
- src/home/invite_modal.rs
- src/room/mod.rs
- src/settings/mod.rs
- src/settings/settings_screen.rs
- src/shared/mod.rs
- src/utils.rs
- src/lib.rs
- Cargo.toml

Per-task specs declare additional file paths in their own
`### Allowed Changes` block — those are additive, not overriding.

### Forbidden

- Do not add a new top-level crate or split `robrix` into a workspace.
- Do not introduce `.unwrap()` on user-supplied strings (room name, topic,
  search query, member user-ids). Every parse failure must surface as a
  typed error returned to the UI layer.
- Do not call `matrix_sdk::Client` methods from widget code. The widget layer
  builds a typed config struct, submits a `MatrixRequest`, and waits for an
  action — no direct sdk usage outside `src/sliding_sync.rs`.
- Do not silently swallow matrix-sdk errors. Each new backend handler returns
  a typed `Action::Failed { reason }` variant; UI may then surface a popup.

## Completion Criteria

Scenario: Existing Month-2 utility tests still pass after Month 3 edits
  Test:
    Package: robrix
    Filter: tests_human_readable_list
  Given the Month 3 code changes are applied to `src/`
  When `cargo test -p robrix tests_human_readable_list` runs
  Then every test in the `tests_human_readable_list` module passes
  And the count of tests in that module is unchanged from the pre-Month-3 baseline

Scenario: Existing audio-player tests still pass after Month 3 edits
  Test:
    Package: robrix
    Filter: tests_audio_message_player
  Given the Month 3 code changes are applied to `src/`
  When `cargo test -p robrix tests_audio_message_player` runs
  Then every test in the `tests_audio_message_player` module passes

Scenario: Each Month-3 task ships at least one exception-path test
  Test:
    Package: robrix
    Filter: tests_room_creation
  Given the four Month-3 task specs are stamped
  When `cargo test -p robrix tests_room_creation` runs
  Then the run includes at least one test whose name contains the substring "rejects"
  And the run includes at least one test whose name contains the substring "missing"

Scenario: Existing add-room join flow is unchanged for a valid alias
  Test:
    Package: robrix
    Filter: test_add_room_alias_dispatch_unchanged
  Given a user pastes `"#robrix:matrix.org"` into the add-room input
  When the add-room handler resolves the input
  Then the dispatched request is `MatrixRequest::JoinRoom` with the resolved `OwnedRoomId`
  And no new request variant is dispatched on this path

## Out of Scope

- Spaces (parent/child room relationships, `m.space.parent`,
  `m.space.child`) — Month 3 covers rooms, not spaces.
- Server-side admin actions (server notices, room shutdown). Robrix is a
  client; server admin lives elsewhere.
- Voice/video call setup inside a room (no SFU integration this month).
- Migrating an existing unencrypted room to encrypted — only the
  *create-time* encryption toggle is in scope; once encryption is enabled,
  there is no UI to disable or rotate it.
- Sticker packs, custom emoji packs, or room-level integrations.
- Federation discovery / well-known fallback — the existing matrix-sdk
  client config is used as-is.
- Multi-account room management — Month 3 operates on the single signed-in
  account, matching Month 2.
