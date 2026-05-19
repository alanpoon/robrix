spec: task
name: "Month 3 — Public Rooms Directory Browser"
inherits: project
tags: [month-3, rooms, directory, search, public]
---

## Intent

Add a `PublicRoomsDirectory` widget that lets the user browse and search
the public-rooms directory of a homeserver, and join a discovered room
with one click. Today `src/home/add_room.rs` can join a room only when
the user already knows the alias or ID. Month 3 closes that gap by
adding a paginated, searchable directory powered by a new
`MatrixRequest::SearchPublicRooms` variant that wraps
`matrix_sdk::Client::public_rooms_filtered`, surfaces results via
`PublicRoomsAction::Loaded { rooms, next_batch }`, and joins via the
**existing** `MatrixRequest::JoinRoom` (`src/sliding_sync.rs:689`).
Pagination state, query normalization, and the
`PublicRoomsChunk → PublicRoomEntry` projection are pure functions in
`src/home/room_directory.rs` so the behavior is unit-testable in
`#[cfg(test)] mod tests_room_directory` without a `Client`.

## Decisions

### Module layout

- New module `src/home/room_directory.rs` registered in `src/home/mod.rs`.
- Exposes widget `PublicRoomsDirectory` and these pure types/helpers:
  - `pub struct PublicRoomEntry { pub room_id: OwnedRoomId, pub canonical_alias: Option<OwnedRoomAliasId>, pub name: Option<String>, pub topic: Option<String>, pub avatar_url: Option<OwnedMxcUri>, pub num_joined_members: u64, pub world_readable: bool, pub guest_can_join: bool }`
  - `pub struct PublicRoomsQuery { pub server: Option<OwnedServerName>, pub search_term: Option<String>, pub limit: u32, pub since: Option<String> }`
  - `pub fn normalize_query(raw: &str) -> Option<String>`
  - `pub fn merge_page(existing: &mut Vec<PublicRoomEntry>, page: Vec<PublicRoomEntry>) -> usize`
  - `pub fn project_chunk(chunk: &ruma::api::client::directory::get_public_rooms_filtered::v3::PublicRoomsChunk) -> PublicRoomEntry`

### Query normalization

- `normalize_query("   foo  bar  ")` returns `Some("foo bar")` — leading
  and trailing whitespace stripped, internal runs of whitespace
  collapsed to a single space.
- `normalize_query("")` and `normalize_query("   ")` both return `None`
  so the UI can omit the `filter` field entirely on a blank search box.
- The lowercase / unicode-fold of the term is *not* applied here; the
  server handles case-folding.

### Pagination

- `limit` defaults to `30`. The UI may raise it to `100` (matrix spec
  hard ceiling) but never higher. The limit is set at the
  `PublicRoomsDirectory` widget level, not per-helper.
- `merge_page` appends each entry from `page` to `existing` *only if*
  no existing entry has the same `room_id`. It returns the number of
  entries actually inserted. This makes incremental fetch idempotent
  against double-fired pagination requests.

### Backend variant

- Append to `MatrixRequest`:
  ```
  SearchPublicRooms {
      query: PublicRoomsQuery,
  }
  ```
- The handler in `async_main_loop` calls
  `Client::public_rooms_filtered` with a request built from
  `PublicRoomsQuery`, projects each `PublicRoomsChunk` via
  `project_chunk`, and posts back:
  - `PublicRoomsAction::Loaded { rooms: Vec<PublicRoomEntry>, next_batch: Option<String>, total_room_count_estimate: Option<u64> }`
    on success, or
  - `PublicRoomsAction::Failed { reason: String }` on error.

### Join from the directory

- Each row carries a "Join" button. Clicking it dispatches the
  **existing** `MatrixRequest::JoinRoom { room_id }` variant
  (`src/sliding_sync.rs:689`); no new join variant is introduced.
- A row whose `room_id` is already in the joined-rooms list shows
  "Already joined" instead of a Join button. The check uses the same
  joined-rooms snapshot already maintained by `src/home/rooms_list.rs`.

## Constraints

- Must NOT call `Client::public_rooms_filtered` from widget code. The
  widget always dispatches `MatrixRequest::SearchPublicRooms`.
- Must NOT silently de-dupe by `canonical_alias` — only `room_id`
  determines identity. Two entries with the same `name` but distinct
  `room_id` are two rooms.
- Must NOT raise the `limit` field above the matrix spec ceiling of
  `100`. Any caller value greater than `100` must be clamped to `100`
  by `PublicRoomsQuery::sanitized()` before sending.
- Must NOT block the search box on the in-flight request. New keystrokes
  cancel the pending UI debounce timer; the most-recent debounced
  query is the one dispatched.
- Must NOT cache results across server selections — switching the
  `server` field clears the in-memory list and resets `since` to `None`.

## Boundaries

### Allowed Changes

- specs/Month-3/room-directory-browser.spec.md
- src/home/room_directory.rs (new)
- src/home/mod.rs
- src/home/add_room.rs
- src/sliding_sync.rs
- src/lib.rs

### Forbidden

- Do not introduce server-selection autocomplete with WHOIS / well-known
  lookups. The server field is a free-text input that accepts an
  already-parsed `OwnedServerName` or `None` (defaulting to the
  homeserver of the signed-in account).
- Do not introduce streaming pagination via WebSocket. The directory
  uses standard HTTP `public_rooms_filtered` with the `since` token.
- Do not introduce a "favorite room" / "pinned room" surface in the
  directory — joining a public room from the directory promotes the
  row out of the list, nothing more.
- Do not add a "preview without joining" affordance this month. The
  existing `MatrixRequest::GetRoomPreview` flow already covers preview
  via a paste-link, and the directory does not duplicate it.

## Completion Criteria

Scenario: Query normalizer collapses internal whitespace
  Test:
    Package: robrix
    Filter: test_normalize_query_collapses_internal_whitespace
  Given the raw query `"   rust   makepad  "`
  When `normalize_query` is called
  Then the result equals `Some("rust makepad".to_string())`

Scenario: Query normalizer rejects a blank string by returning None
  Test:
    Package: robrix
    Filter: test_normalize_query_rejects_blank
  Given the raw query `"   \t  \n  "`
  When `normalize_query` is called
  Then the result equals `None`
  And the caller treats this as an invalid query that must not be dispatched

Scenario: Query normalizer leaves a single-word query untouched
  Test:
    Package: robrix
    Filter: test_normalize_query_preserves_single_word
  Given the raw query `"rust"`
  When `normalize_query` is called
  Then the result equals `Some("rust".to_string())`

Scenario: PublicRoomsQuery sanitizer clamps an over-large limit to 100
  Test:
    Package: robrix
    Filter: test_public_rooms_query_clamps_limit
  Given a `PublicRoomsQuery` whose `limit` equals `250`
  When `PublicRoomsQuery::sanitized` is called
  Then the returned query's `limit` equals `100`

Scenario: PublicRoomsQuery sanitizer leaves a valid limit untouched
  Test:
    Package: robrix
    Filter: test_public_rooms_query_preserves_valid_limit
  Given a `PublicRoomsQuery` whose `limit` equals `30`
  When `PublicRoomsQuery::sanitized` is called
  Then the returned query's `limit` equals `30`

Scenario: merge_page appends a new entry and reports one insertion
  Test:
    Package: robrix
    Filter: test_merge_page_appends_new_entry
  Given `existing` containing one entry with `room_id = "!a:m.org"`
  And `page` containing one entry with `room_id = "!b:m.org"`
  When `merge_page` is called
  Then `existing.len()` equals `2`
  And the return value equals `1`

Scenario: merge_page skips an entry whose room_id is already present
  Test:
    Package: robrix
    Filter: test_merge_page_skips_duplicate_room_id
  Given `existing` containing one entry with `room_id = "!a:m.org"`
  And `page` containing one entry with `room_id = "!a:m.org"` and a different name
  When `merge_page` is called
  Then `existing.len()` equals `1`
  And the return value equals `0`
  And the kept entry is the original `existing` entry, not the duplicate

Scenario: merge_page handles an empty incoming page
  Test:
    Package: robrix
    Filter: test_merge_page_empty_page_inserts_zero
  Given `existing` containing two entries
  And `page` equal to `vec![]`
  When `merge_page` is called
  Then `existing.len()` equals `2`
  And the return value equals `0`

Scenario: project_chunk maps every relevant ruma field to PublicRoomEntry
  Test:
    Package: robrix
    Filter: test_project_chunk_maps_all_fields
  Given a `PublicRoomsChunk` with `room_id = "!a:m.org"`, `name = Some("Rust")`, `topic = Some("blazing fast")`, `num_joined_members = 42`, `world_readable = true`, `guest_can_join = false`, `canonical_alias = Some("#rust:m.org")`
  When `project_chunk` is called
  Then the returned `PublicRoomEntry.room_id` equals `OwnedRoomId("!a:m.org")`
  And `name` equals `Some("Rust".to_string())`
  And `topic` equals `Some("blazing fast".to_string())`
  And `num_joined_members` equals `42`
  And `world_readable` equals `true`
  And `guest_can_join` equals `false`
  And `canonical_alias` equals `Some(OwnedRoomAliasId("#rust:m.org"))`

Scenario: project_chunk preserves None for absent optional fields
  Test:
    Package: robrix
    Filter: test_project_chunk_preserves_none_fields
  Given a `PublicRoomsChunk` whose `name`, `topic`, `canonical_alias`, and `avatar_url` are all `None`
  When `project_chunk` is called
  Then the returned `PublicRoomEntry.name` equals `None`
  And `topic` equals `None`
  And `canonical_alias` equals `None`
  And `avatar_url` equals `None`

Scenario: Joining from the directory dispatches the existing JoinRoom variant
  Test:
    Package: robrix
    Filter: test_directory_join_uses_existing_join_room_variant
  Given a directory row whose `room_id` equals `"!a:matrix.org"`
  When the user clicks the Join button on that row
  Then the dispatched request equals `MatrixRequest::JoinRoom { room_id: OwnedRoomId("!a:matrix.org") }`
  And no new directory-specific join variant is dispatched

Scenario: PublicRoomsAction exposes a Failed variant for surfacing search errors
  Test:
    Package: robrix
    Filter: test_public_rooms_action_has_failed_variant
  Given the `PublicRoomsAction` enum defined in `src/home/room_directory.rs`
  When the test inspects the enum variants by name
  Then a variant named `Failed` exists
  And the `Failed` variant carries a `reason: String` field
  And constructing `PublicRoomsAction::Failed { reason: "homeserver returned 502".into() }` succeeds

## Out of Scope

- Cross-server federated directory aggregation (querying multiple
  servers in one request).
- Persistent caching of directory results across app restarts.
- Sorting / filtering by member count, recency, or language.
- Room preview *inside* the directory row (the existing
  `MatrixRequest::GetRoomPreview` flow handles preview by alias).
- Federated suggestions / recommendations / trending rooms.
- Reporting / flagging a public room.

## Forbidden
- Do not use cargo fmt