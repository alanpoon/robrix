# Content Reporting Design

**Date:** 2026-06-10
**Branch:** feat/user_moderation_actions

## Overview

Add the ability for a user to report a message (event) from another user in a room. Right-clicking a message opens the existing context menu; a new "Report" item opens a modal where the user enters a reason and submits it to the homeserver.

## Constraints

- Report button is only shown for messages from **other users** (not the current user's own messages). Since local echo messages (`TransactionId`) are always the current user's own, `event_id()` will always be `Some` when `CanReport` is true — but the submit path must still guard with `if let Some(event_id) = details.event_id()` defensively.
- A reason is **required** — the submit button is blocked until the user enters non-empty text.
- The feature uses the existing `room.report_content(event_id, reason)` Matrix SDK API. The correct call is `room.report_content(event_id, Some(reason)).await` (2 parameters only — no score parameter).
- Follows the `ReportRoomModal` pattern exactly, but in its own file `src/home/report_content_modal.rs`. Because the DSL template referencing `ReportContentModal` lives in `room_screen.rs`, `ReportContentModal::register_widget(vm)` must be called from the `script_mod!` block in `room_screen.rs`.

## Components

### 1. `src/home/report_content_modal.rs` (new file)

A new `ReportContentModal` widget with:
- Title: "Report Message"
- Body: "Report this message to your homeserver administrators. Please provide a reason."
- `reason_input`: required text input; shows inline error if submitted empty
- `cancel_button` + `report_button` (danger red, use `ICON_WARNING` as stopgap icon)
- Does **not** store `room_id` — it is retrieved from `self.room_id()` on the `RoomScreen` at submit time, following `ReportRoomModal`'s pattern

**Actions:**
- `ReportContentModalAction::Close`
- `ReportContentModalAction::Submit { event_id: OwnedEventId, reason: String }`

`ReportContentResultAction` is defined in this same file and imported into `sliding_sync.rs`:
```rust
pub enum ReportContentResultAction {
    Sent { room_id: OwnedRoomId, event_id: OwnedEventId },
    Failed { room_id: OwnedRoomId, event_id: OwnedEventId, error: matrix_sdk::Error },
}
```

### 2. `src/sliding_sync.rs`

New `MatrixRequest` variant:
```rust
ReportContent {
    room_id: OwnedRoomId,
    event_id: OwnedEventId,
    reason: String,
}
```

Handler spawns an async task calling:
```rust
room.report_content(event_id, Some(reason)).await
```
and dispatches `ReportContentResultAction::Sent` or `::Failed`.

Import `ReportContentResultAction` from `home::report_content_modal` in `sliding_sync.rs` (not from `room_screen`) — add it to the imports at line 51 as a separate entry: `home::report_content_modal::ReportContentResultAction`.

### 3. `src/home/new_message_context_menu.rs`

- Change the `MessageAbilities` backing type from `u8` to `u16`, then add `CanReport = 1 << 8` (the next available bit — `1 << 8 = 256` overflows `u8` but fits `u16`).
- Set `CanReport` when `!event_tl_item.is_own()` in `MessageAbilities::from_user_power_and_event`.
- Replace (do not simply uncomment) the scaffolded `report_button` DSL block — the existing commented body uses an obsolete `details.event_id` field name and struct-variant shape that no longer matches.
- Show `report_button` when `details.abilities.contains(MessageAbilities::CanReport)`.
- Update `show_divider_before_report_delete` to `show_delete || show_report` so the separator appears when report is visible but delete is not.
- On click: dispatch `MessageAction::Report(details)` using the 2-argument form `cx.widget_action(details.room_screen_widget_uid, MessageAction::Report(details.clone()))` — consistent with all other live callers; the old 3-arg form with `&scope.path` is obsolete.
- Also uncomment `+ show_report as u8` in the `num_visible_buttons` height calculation so the menu height is computed correctly when report is visible but delete is not.

### 4. `src/home/room_screen.rs`

- Add `use super::report_content_modal::{ReportContentModal, ReportContentModalAction, ReportContentResultAction};` to `room_screen.rs` (note: `pub mod report_content_modal;` goes in `mod.rs` — Section 5; `room_screen.rs` needs only the `use` import).
- Uncomment and update `MessageAction::Report(MessageDetails)` variant (tuple variant wrapping `MessageDetails`, not the old struct-variant scaffold).
- Add `report_content_modal` (Modal wrapper) and `report_content_modal_inner` (ReportContentModal) to the DSL overlay stack alongside `report_room_modal`.
- Call `ReportContentModal::register_widget(vm)` in the `script_mod!` block.
- In `handle_message_actions`: on `Report(details)`, call `open_report_content_modal(cx, details)`.
- Add `open_report_content_modal(cx, details: MessageDetails)` helper: guard with `if let Some(event_id) = details.event_id()`, show modal.
- Add `close_report_content_modal(cx)` helper.
- Handle `ReportContentModalAction::Submit { event_id, reason }`: get `room_id` from `self.room_id()`, call `submit_async_request(MatrixRequest::ReportContent {...})`, close modal.
- Handle `ReportContentResultAction::Sent`: show success popup ("Message reported").
- Handle `ReportContentResultAction::Failed`: show error popup.
- Add `self.close_report_content_modal(cx)` to `reset_app_service_ui` (called from `set_displayed_room` on room switch).
- Close modal on `RoomsListAction::Selected`, `AppStateAction::RoomFocused`, and `AppStateAction::FocusNone` — matching the identical three cases used for `close_report_room_modal`.

### 5. `src/home/mod.rs`

- Add `pub mod report_content_modal;`

## Data Flow

```
right-click message
  → NewMessageContextMenu shows report_button (only if CanReport, i.e. !is_own())
  → user clicks "Report"
  → MessageAction::Report(details) dispatched to RoomScreen
  → open_report_content_modal(cx, details) called
    → guard: if let Some(event_id) = details.event_id()
  → modal opens
  → user types reason and clicks "Report"
  → ReportContentModalAction::Submit { event_id, reason }
  → room_id from self.room_id()
  → MatrixRequest::ReportContent { room_id, event_id, reason } submitted
  → async: room.report_content(event_id, Some(reason)).await
  → ReportContentResultAction::Sent  → popup: "Message reported"
  → ReportContentResultAction::Failed → popup: error message
```

## Files Changed

| File | Change |
|------|--------|
| `src/home/report_content_modal.rs` | New file: widget, modal/result actions |
| `src/sliding_sync.rs` | Add `ReportContent` variant + handler; import `ReportContentResultAction` |
| `src/home/new_message_context_menu.rs` | Add `CanReport` flag, replace scaffolded button, fix divider logic |
| `src/home/room_screen.rs` | Register widget, wire modal, uncomment `MessageAction::Report`, handle result, update `reset_app_service_ui` |
| `src/home/mod.rs` | Add `pub mod report_content_modal` |
