# Content Reporting Design

**Date:** 2026-06-10
**Branch:** feat/user_moderation_actions

## Overview

Add the ability for a user to report a message (event) from another user in a room. Right-clicking a message opens the existing context menu; a new "Report" item opens a modal where the user enters a reason and submits it to the homeserver.

## Constraints

- Report button is only shown for messages from **other users** (not the current user's own messages).
- A reason is **required** — the submit button is blocked until the user enters non-empty text.
- The feature uses the existing `room.report_content(event_id, reason)` Matrix SDK API.
- Follows the `ReportRoomModal` pattern exactly.

## Components

### 1. `src/home/report_content_modal.rs` (new file)

A new `ReportContentModal` widget with:
- Title: "Report Message"
- Body: "Report this message to your homeserver administrators. Please provide a reason."
- `reason_input`: required text input; shows inline error if submitted empty
- `cancel_button` + `report_button` (danger red)
- Stores `OwnedEventId` of the targeted message internally

**Actions:**
- `ReportContentModalAction::Close`
- `ReportContentModalAction::Submit { event_id: OwnedEventId, reason: String }`

### 2. `src/sliding_sync.rs`

New `MatrixRequest` variant:
```rust
ReportContent {
    room_id: OwnedRoomId,
    event_id: OwnedEventId,
    reason: String,
}
```

Handler spawns an async task calling `room.report_content(event_id, Some(reason)).await` and dispatches:
- `ReportContentResultAction::Sent { room_id, event_id }`
- `ReportContentResultAction::Failed { room_id, event_id, error }`

### 3. `src/home/new_message_context_menu.rs`

- Add `CanReport` bit to the `MessageAbilities` bitflags.
- Set `CanReport` when `!event_tl_item.is_own()` in `MessageAbilities::from_user_power_and_event`.
- Uncomment the `report_button` DSL block.
- Show it when `details.abilities.contains(MessageAbilities::CanReport)`.
- On click: dispatch `MessageAction::Report(details)` and close menu.

### 4. `src/home/room_screen.rs`

- Uncomment `MessageAction::Report(MessageDetails)` variant.
- Add `report_content_modal` (Modal wrapper) and `report_content_modal_inner` (ReportContentModal) to the DSL overlay stack alongside `report_room_modal`.
- In `handle_message_actions`: on `Report(details)`, call `open_report_content_modal(cx, details)`.
- Add `open_report_content_modal` and `close_report_content_modal` helper methods.
- Handle `ReportContentModalAction::Submit`: call `submit_async_request(MatrixRequest::ReportContent {...})` and close modal.
- Handle `ReportContentResultAction::Sent`: show success popup notification ("Message reported").
- Handle `ReportContentResultAction::Failed`: show error popup notification.
- Close modal on room focus change / `AppStateAction::FocusNone`.

## Data Flow

```
right-click message
  → NewMessageContextMenu shows report_button (only if !is_own())
  → user clicks "Report"
  → MessageAction::Report(details) dispatched to RoomScreen
  → open_report_content_modal(cx, details) called
  → modal opens with event_id stored
  → user types reason and clicks "Report"
  → ReportContentModalAction::Submit { event_id, reason }
  → MatrixRequest::ReportContent { room_id, event_id, reason } submitted
  → async: room.report_content(event_id, Some(reason)).await
  → ReportContentResultAction::Sent → popup: "Message reported"
  → ReportContentResultAction::Failed → popup: error message
```

## Files Changed

| File | Change |
|------|--------|
| `src/home/report_content_modal.rs` | New file |
| `src/sliding_sync.rs` | Add `ReportContent` variant + handler + result action |
| `src/home/new_message_context_menu.rs` | Add `CanReport` flag, uncomment button |
| `src/home/room_screen.rs` | Wire modal, uncomment `MessageAction::Report`, handle result |
| `src/home/mod.rs` | Add `pub mod report_content_modal` |
