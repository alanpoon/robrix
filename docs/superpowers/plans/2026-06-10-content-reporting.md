# Content Reporting Implementation Plan

> **For agentic workers:** REQUIRED: Use superpowers:subagent-driven-development (if subagents available) or superpowers:executing-plans to implement this plan. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add content reporting to room message context menus — right-click opens a modal with reason input and optional "Ignore user" checkbox, submitting via the Matrix `report_content` API.

**Architecture:** Uncomment the already-scaffolded `report_button` in the context menu, create a new `ReportContentModal` widget in its own file following the `ReportRoomModal` pattern, add two new `MatrixRequest` variants (`ReportContent`, `IgnoreUserById`), and wire everything together in `room_screen.rs`.

**Tech Stack:** Rust, Makepad 2.0 (`script_mod!`, `#[derive(Script, ScriptHook, Widget)]`), matrix-sdk, bitflags crate.

**Spec:** `docs/superpowers/specs/2026-06-10-content-reporting-design.md`

---

## Chunk 1: Data model changes

### Task 1: Add `sender_id` to `MessageDetails` and update construction site

**Files:**
- Modify: `src/home/new_message_context_menu.rs` (struct definition ~line 282)
- Modify: `src/home/room_screen.rs` (construction site ~line 10545)

- [ ] **Step 1: Add `sender_id` field to `MessageDetails`**

In `src/home/new_message_context_menu.rs`, add to the `MessageDetails` struct after `should_be_highlighted`:

```rust
/// The user ID of the sender of this message.
pub sender_id: OwnedUserId,
```

Also add the import at the top if not present:
```rust
use matrix_sdk::ruma::OwnedUserId;
```

- [ ] **Step 2: Populate `sender_id` at the construction site**

In `src/home/room_screen.rs` around line 10545, in the `MessageDetails { ... }` struct literal, add:

```rust
sender_id: event_tl_item.sender().to_owned(),
```

- [ ] **Step 3: Verify it compiles**

```bash
cargo build 2>&1 | head -40
```

Expected: compile succeeds (or only errors about missing `sender_id` in any other `MessageDetails { ... }` literals — fix those too if they exist).

- [ ] **Step 4: Commit**

```bash
git add src/home/new_message_context_menu.rs src/home/room_screen.rs
git commit -m "feat: add sender_id to MessageDetails for content reporting"
```

---

### Task 2: Add `CanReport` to `MessageAbilities` (u8 → u16)

**Files:**
- Modify: `src/home/new_message_context_menu.rs` (~line 217)

- [ ] **Step 1: Change backing type and add flag**

In `src/home/new_message_context_menu.rs`, find the `bitflags!` block (~line 217):

```rust
#[derive(Copy, Clone, Debug)]
pub struct MessageAbilities: u8 {
```

Change `u8` to `u16`, then add at the end of the flag list after `CanForward = 1 << 7`:

```rust
/// Whether the user can report this message (i.e., it is not their own).
const CanReport = 1 << 8;
```

- [ ] **Step 2: Set `CanReport` in `from_user_power_and_event`**

In the same file, in `MessageAbilities::from_user_power_and_event` (~line 244), add after `abilities.set(Self::CanForward, ...)`:

```rust
abilities.set(Self::CanReport, !event_tl_item.is_own());
```

- [ ] **Step 3: Verify it compiles**

```bash
cargo build 2>&1 | head -40
```

Expected: clean compile.

- [ ] **Step 4: Commit**

```bash
git add src/home/new_message_context_menu.rs
git commit -m "feat: add CanReport flag to MessageAbilities (u8 -> u16)"
```

---

### Task 3: Replace scaffolded report button in context menu

**Files:**
- Modify: `src/home/new_message_context_menu.rs`

- [ ] **Step 1: Replace the commented-out DSL block**

In `src/home/new_message_context_menu.rs`, find the commented-out `report_button` DSL block (lines ~178–193). **Delete the entire comment block** and replace it with:

```
report_button := mod.widgets.NewMessageContextMenuButton {
    draw_icon +: {
        svg: (ICON_WARNING)
        color: (COLOR_FG_DANGER_RED),
    }
    draw_bg +: {
        border_color: (COLOR_FG_DANGER_RED),
        color: (COLOR_BG_DANGER_RED)
    }
    draw_text.color: (COLOR_FG_DANGER_RED),
    text: "Report"
}
```

- [ ] **Step 2: Update `set_button_visibility` — show logic**

In `set_button_visibility` (~line 590), find:

```rust
// let show_report = true;
let show_delete = details.abilities.contains(MessageAbilities::CanDelete);
let show_divider_before_report_delete = show_delete; // || show_report;
```

Replace with:

```rust
let show_report = details.abilities.contains(MessageAbilities::CanReport);
let show_delete = details.abilities.contains(MessageAbilities::CanDelete);
let show_divider_before_report_delete = show_delete || show_report;
```

- [ ] **Step 3: Update `set_button_visibility` — apply visibility**

Find the block that calls `.set_visible(cx, ...)`. Find `// report_button.set_visible(cx, show_report);` and replace it with:

```rust
self.view.button(cx, ids!(report_button)).set_visible(cx, show_report);
```

Also find `// report_button.reset_hover(cx);` and replace with:

```rust
self.view.button(cx, ids!(report_button)).reset_hover(cx);
```

- [ ] **Step 4: Update height calculation**

Find `// + show_report as u8` in the `num_visible_buttons` calculation (~line 668) and uncomment it:

```rust
+ show_report as u8
```

- [ ] **Step 5: Add click handler**

In `handle_actions` (~line 488), find the commented-out report handler block (lines ~488–499). **Delete the entire comment block** and replace it with:

```rust
else if self.button(cx, ids!(report_button)).clicked(actions) {
    cx.widget_action(
        details.room_screen_widget_uid,
        MessageAction::Report(details.clone()),
    );
    close_menu = true;
}
```

(Place this `else if` before the existing `else if self.button(cx, ids!(delete_button))...` block.)

- [ ] **Step 6: Verify it compiles**

```bash
cargo build 2>&1 | head -40
```

Expected: error about `MessageAction::Report` not existing — that's fine for now, comment out the new handler body temporarily if needed to get a clean build, or proceed to the next task immediately.

- [ ] **Step 7: Commit**

```bash
git add src/home/new_message_context_menu.rs
git commit -m "feat: wire report button in message context menu"
```

---

## Chunk 2: ReportContentModal widget

### Task 4: Create `src/home/report_content_modal.rs`

**Files:**
- Create: `src/home/report_content_modal.rs`

- [ ] **Step 1: Create the file with widget DSL and structs**

Create `/Users/alanpoon/Documents/rust/robius/robrix3/src/home/report_content_modal.rs` with this content:

```rust
//! Modal dialog for reporting a message to the homeserver.

use makepad_widgets::*;
use matrix_sdk::ruma::{OwnedEventId, OwnedRoomId, OwnedUserId};

script_mod! {
    use mod.prelude.widgets.*
    use mod.widgets.*

    mod.widgets.ReportContentModalLabel = Label {
        width: Fill
        height: Fit
        draw_text +: {
            text_style: REGULAR_TEXT { font_size: 10.5 }
            color: #333
        }
        text: ""
    }

    mod.widgets.ReportContentModal = #(ReportContentModal::register_widget(vm)) {
        width: Fit
        height: Fit

        RoundedView {
            width: 430
            height: Fit
            align: Align{x: 0.5}
            flow: Down
            padding: Inset{top: 26, right: 22, bottom: 18, left: 22}
            spacing: 14

            show_bg: true
            draw_bg +: {
                color: (COLOR_PRIMARY)
                border_radius: 6.0
            }

            title := Label {
                width: Fill
                height: Fit
                draw_text +: {
                    text_style: TITLE_TEXT { font_size: 13 }
                    color: #000
                }
                text: "Report Message"
            }

            body := mod.widgets.ReportContentModalLabel {
                text: "Report this message to your homeserver administrators. Please provide a reason."
            }

            reason_input := RobrixTextInput {
                width: Fill
                height: Fit
                padding: 10
                draw_text +: {
                    text_style: REGULAR_TEXT { font_size: 11.5 }
                    color: #000
                }
                empty_text: "Describe why you are reporting this message"
            }

            ignore_row := View {
                width: Fill
                height: Fit
                flow: Down
                spacing: 4

                ignore_checkbox := CheckBoxFlat {
                    text: "Ignore user"
                    active: false
                    draw_text +: {
                        color: (COLOR_TEXT)
                        color_hover: (COLOR_TEXT)
                        color_focus: (COLOR_TEXT)
                        color_down: (COLOR_TEXT)
                    }
                }

                ignore_label := mod.widgets.ReportContentModalLabel {
                    text: "Check if you want to hide all current and future messages from this user."
                }
            }

            status_label := Label {
                width: Fill
                height: Fit
                draw_text +: {
                    text_style: REGULAR_TEXT { font_size: 10.2 }
                    color: #000
                }
                text: ""
            }

            buttons := View {
                width: Fill
                height: Fit
                flow: Right
                align: Align{x: 1.0, y: 0.5}
                spacing: 16

                cancel_button := RobrixNeutralIconButton {
                    width: 110
                    align: Align{x: 0.5, y: 0.5}
                    padding: 12
                    draw_icon.svg: (ICON_FORBIDDEN)
                    icon_walk: Walk{width: 16, height: 16, margin: Inset{left: -2, right: -1}}
                    text: "Cancel"
                }

                report_button := RobrixNegativeIconButton {
                    width: 130
                    align: Align{x: 0.5, y: 0.5}
                    padding: 12
                    draw_icon.svg: (ICON_WARNING)
                    icon_walk: Walk{width: 16, height: 16, margin: Inset{left: -2, right: -1}}
                    text: "Report"
                }
            }
        }
    }
}

#[derive(Debug)]
pub enum ReportContentModalAction {
    Close,
    Submit {
        event_id: OwnedEventId,
        reason: String,
        /// `Some(user_id)` if the "Ignore user" checkbox was checked.
        ignore_sender: Option<OwnedUserId>,
    },
}

#[derive(Debug)]
pub enum ReportContentResultAction {
    Sent {
        room_id: OwnedRoomId,
        event_id: OwnedEventId,
    },
    Failed {
        room_id: OwnedRoomId,
        event_id: OwnedEventId,
        error: matrix_sdk::Error,
    },
}

#[derive(Script, ScriptHook, Widget)]
pub struct ReportContentModal {
    #[deref]
    view: View,
    #[rust]
    event_id: Option<OwnedEventId>,
    #[rust]
    sender_id: Option<OwnedUserId>,
    #[rust]
    is_showing_error: bool,
}

impl Widget for ReportContentModal {
    fn handle_event(&mut self, cx: &mut Cx, event: &Event, scope: &mut Scope) {
        self.view.handle_event(cx, event, scope);
        self.widget_match_event(cx, event, scope);
    }

    fn draw_walk(&mut self, cx: &mut Cx2d, scope: &mut Scope, walk: Walk) -> DrawStep {
        self.view.draw_walk(cx, scope, walk)
    }
}

impl WidgetMatchEvent for ReportContentModal {
    fn handle_actions(&mut self, cx: &mut Cx, actions: &Actions, _scope: &mut Scope) {
        let cancel_button = self.view.button(cx, ids!(buttons.cancel_button));
        let report_button = self.view.button(cx, ids!(buttons.report_button));
        let reason_input = self.view.text_input(cx, ids!(reason_input));
        let mut status_label = self.view.label(cx, ids!(status_label));

        if cancel_button.clicked(actions)
            || actions
                .iter()
                .any(|a| matches!(a.downcast_ref(), Some(ModalAction::Dismissed)))
        {
            cx.action(ReportContentModalAction::Close);
            return;
        }

        if self.is_showing_error && reason_input.changed(actions).is_some() {
            self.is_showing_error = false;
            status_label.set_text(cx, "");
            self.view.redraw(cx);
        }

        if report_button.clicked(actions) || reason_input.returned(actions).is_some() {
            let reason = reason_input.text().trim().to_string();
            if reason.is_empty() {
                self.is_showing_error = true;
                script_apply_eval!(cx, status_label, {
                    text: "Please enter a reason before reporting."
                    draw_text +: {
                        color: mod.widgets.COLOR_FG_DANGER_RED
                    }
                });
                self.view.redraw(cx);
                return;
            }
            let Some(event_id) = self.event_id.clone() else { return };
            let ignore_sender = if self.view.check_box(cx, ids!(ignore_row.ignore_checkbox)).active(cx) {
                self.sender_id.clone()
            } else {
                None
            };
            cx.action(ReportContentModalAction::Submit { event_id, reason, ignore_sender });
        }
    }
}

impl ReportContentModal {
    pub fn show(&mut self, cx: &mut Cx, event_id: OwnedEventId, sender_id: OwnedUserId) {
        self.event_id = Some(event_id);
        self.sender_id = Some(sender_id);
        self.is_showing_error = false;
        self.view.label(cx, ids!(status_label)).set_text(cx, "");
        self.view.text_input(cx, ids!(reason_input)).set_text(cx, "");
        self.view.check_box(cx, ids!(ignore_row.ignore_checkbox)).set_active(cx, false, Animate::No);
        self.view.button(cx, ids!(buttons.report_button)).set_enabled(cx, true);
        self.view.button(cx, ids!(buttons.cancel_button)).set_enabled(cx, true);
        self.view.button(cx, ids!(buttons.report_button)).reset_hover(cx);
        self.view.button(cx, ids!(buttons.cancel_button)).reset_hover(cx);
        self.view.redraw(cx);
    }
}

impl ReportContentModalRef {
    pub fn show(&self, cx: &mut Cx, event_id: OwnedEventId, sender_id: OwnedUserId) {
        let Some(mut inner) = self.borrow_mut() else { return };
        inner.show(cx, event_id, sender_id);
    }
}
```

- [ ] **Step 2: Register module in `src/home/mod.rs`**

Open `src/home/mod.rs` and add alongside the other `pub mod` declarations:

```rust
pub mod report_content_modal;
```

- [ ] **Step 3: Verify it compiles**

```bash
cargo build 2>&1 | head -50
```

Expected: compiles (or errors only about the widget not yet being registered in room_screen — acceptable at this stage).

- [ ] **Step 4: Commit**

```bash
git add src/home/report_content_modal.rs src/home/mod.rs
git commit -m "feat: add ReportContentModal widget"
```

---

## Chunk 3: Matrix API

### Task 5: Add `MatrixRequest::ReportContent` and `IgnoreUserById` to sliding_sync

**Files:**
- Modify: `src/sliding_sync.rs`

- [ ] **Step 1: Add import for `ReportContentResultAction`**

In `src/sliding_sync.rs`, find line 51 (the large `use crate::home::{ ... }` block). It currently contains:

```
room_screen::{ActionResponseResultAction, InviteResultAction, ReportRoomResultAction, TimelineUpdate},
```

Add `report_content_modal::ReportContentResultAction` as a separate import inside the `home::{ ... }` block, next to the other modal imports:

```
report_content_modal::ReportContentResultAction,
```

- [ ] **Step 2: Add `ReportContent` variant to `MatrixRequest` enum**

Find `MatrixRequest::ReportRoom { ... }` (~line 1179) and add directly after it:

```rust
/// Request to report a specific message/event in a room.
ReportContent {
    room_id: OwnedRoomId,
    event_id: OwnedEventId,
    reason: String,
},
/// Request to ignore a user by their user ID (without needing their RoomMember object).
IgnoreUserById {
    user_id: OwnedUserId,
    room_id: OwnedRoomId,
},
```

- [ ] **Step 3: Add handlers in the `MatrixRequest` dispatch loop**

Find the `MatrixRequest::ReportRoom { room_id, reason }` handler (~line 3100) and add directly after its closing `}`:

```rust
MatrixRequest::ReportContent { room_id, event_id, reason } => {
    let Some(client) = get_client() else { continue };
    let _report_content_task = Handle::current().spawn(async move {
        log!("Sending request to report event {event_id} in room {room_id}...");
        let result = if let Some(room) = client.get_room(&room_id) {
            match room.report_content(event_id.clone(), Some(reason)).await {
                Ok(_) => ReportContentResultAction::Sent { room_id, event_id },
                Err(e) => {
                    error!("Error reporting event {event_id} in room {room_id}: {e:?}");
                    ReportContentResultAction::Failed { room_id, event_id, error: e }
                }
            }
        } else {
            ReportContentResultAction::Failed {
                room_id,
                event_id,
                error: matrix_sdk::Error::UnknownError(
                    "Client couldn't locate room to report content.".into()
                ),
            }
        };
        Cx::post_action(result);
    });
}

MatrixRequest::IgnoreUserById { user_id, room_id } => {
    let Some(client) = get_client() else { continue };
    let _ignore_task = Handle::current().spawn(async move {
        log!("Sending request to ignore user {user_id}...");
        let result = if let Some(room) = client.get_room(&room_id) {
            match room.get_member(&user_id).await {
                Ok(Some(member)) => member.ignore().await,
                Ok(None) => Err(matrix_sdk::Error::UnknownError(
                    format!("User {user_id} not found in room {room_id}").into()
                )),
                Err(e) => Err(e),
            }
        } else {
            Err(matrix_sdk::Error::UnknownError(
                format!("Room {room_id} not found").into()
            ))
        };
        if let Err(e) = result {
            error!("Error ignoring user {user_id}: {e:?}");
            return;
        }
        log!("Successfully ignored user {user_id}.");
        // Re-paginate the room so ignored messages are hidden.
        submit_async_request(MatrixRequest::PaginateTimeline {
            timeline_kind: TimelineKind::MainRoom { room_id },
            num_events: 50,
            direction: PaginationDirection::Backwards,
        });
    });
}
```

Use `Cx::post_action(result)` — this is the dispatch mechanism used by the existing `ReportRoom` handler at `sliding_sync.rs` line ~3121. Replace every `cx_dispatch_main(result)` placeholder above with `Cx::post_action(result)`.

- [ ] **Step 4: Verify it compiles**

```bash
cargo build 2>&1 | head -50
```

Expected: clean compile, or errors only about room_screen not yet handling the new action types.

- [ ] **Step 5: Commit**

```bash
git add src/sliding_sync.rs
git commit -m "feat: add ReportContent and IgnoreUserById MatrixRequest variants"
```

---

## Chunk 4: room_screen wiring

### Task 6: Wire modal DSL and `MessageAction::Report` in room_screen

**Files:**
- Modify: `src/home/room_screen.rs`

- [ ] **Step 1: Add import**

In `room_screen.rs`, inside the large `use crate::home::{ ... }` block at line ~30 (alongside `create_bot_modal::{CreateBotModalAction, CreateBotModalWidgetExt}` etc.), add:

```rust
report_content_modal::{
    ReportContentModal, ReportContentModalAction,
    ReportContentModalWidgetRefExt, ReportContentResultAction,
},
```

`ReportContentModalWidgetRefExt` is the trait auto-generated by `#[derive(Widget)]` that provides the `self.view.report_content_modal(cx, ids!(...))` accessor on `View`. Without this import the accessor will not be found.

- [ ] **Step 2: Register widget in `script_mod!`**

Inside the `script_mod! { ... }` block in `room_screen.rs`, find `#(ReportRoomModal::register_widget(vm))` and add directly after it:

```
#(ReportContentModal::register_widget(vm))
```

- [ ] **Step 3: Add modal DSL to overlay stack**

Find the `report_room_modal := Modal { ... }` DSL block (~line 3908) and add directly after it:

```
report_content_modal := Modal {
    content +: {
        report_content_modal_inner := mod.widgets.ReportContentModal {}
    }
}
```

- [ ] **Step 4: Uncomment `MessageAction::Report`**

Find (~line 8034):
```rust
// MessageAction::Report(details) => {
//     // TODO
// }
```
Replace with:
```rust
MessageAction::Report(details) => {
    self.open_report_content_modal(cx, &details);
}
```

- [ ] **Step 5: Also uncomment `MessageAction::Report` variant in the enum**

Find (~line 12086):
```rust
// /// The user clicked the "report" button on a message.
// Report(MessageDetails),
```
Uncomment to:
```rust
/// The user clicked the "report" button on a message.
Report(MessageDetails),
```

- [ ] **Step 6: Verify it compiles**

```bash
cargo build 2>&1 | head -50
```

Expected: clean compile (or errors about result action handlers not yet added — acceptable).

- [ ] **Step 7: Commit**

```bash
git add src/home/room_screen.rs
git commit -m "feat: wire ReportContentModal DSL and MessageAction::Report handler"
```

---

### Task 7: Handle modal actions and result actions

**Files:**
- Modify: `src/home/room_screen.rs`

- [ ] **Step 1: Add helper methods**

Find `fn close_report_room_modal` (~line 6529) and add directly after it:

```rust
fn close_report_content_modal(&self, cx: &mut Cx) {
    self.view.modal(cx, ids!(report_content_modal)).close(cx);
}

fn open_report_content_modal(&mut self, cx: &mut Cx, details: &MessageDetails) {
    let Some(event_id) = details.event_id().cloned() else { return };
    let sender_id = details.sender_id.clone();
    self.view
        .report_content_modal(cx, ids!(report_content_modal_inner))
        .show(cx, event_id, sender_id);
    self.view.modal(cx, ids!(report_content_modal)).open(cx);
}
```

(Place `open_report_content_modal` next to `open_report_room_modal` ~line 6559 for consistency.)

- [ ] **Step 2: Add to `reset_app_service_ui`**

Find `fn reset_app_service_ui` (~line 6585). Inside it, find `self.close_report_room_modal(cx);` and add directly after:

```rust
self.close_report_content_modal(cx);
```

- [ ] **Step 3: Handle `ReportContentModalAction` in the action loop**

Find the block that handles `ReportRoomModalAction` (~line 5717):
```rust
match action.downcast_ref::<ReportRoomModalAction>() {
    ...
}
```
Add directly after its closing `}`:

```rust
match action.downcast_ref::<ReportContentModalAction>() {
    Some(ReportContentModalAction::Close) => {
        self.close_report_content_modal(cx);
        return false;
    }
    Some(ReportContentModalAction::Submit { event_id, reason, ignore_sender }) => {
        let Some(room_id) = self.room_id().cloned() else {
            self.close_report_content_modal(cx);
            return false;
        };
        submit_async_request(MatrixRequest::ReportContent {
            room_id: room_id.clone(),
            event_id: event_id.clone(),
            reason: reason.clone(),
        });
        if let Some(user_id) = ignore_sender.clone() {
            submit_async_request(MatrixRequest::IgnoreUserById {
                user_id,
                room_id,
            });
        }
        self.close_report_content_modal(cx);
        return false;
    }
    None => {}
}
```

- [ ] **Step 4: Handle `ReportContentResultAction`**

Find the block that handles `ReportRoomResultAction::Sent` (~line 5196):
```rust
if let Some(ReportRoomResultAction::Sent { room_id }) = action.downcast_ref() {
    ...
}
```
Add directly after the `ReportRoomResultAction::Failed` block:

```rust
if let Some(ReportContentResultAction::Sent { room_id, .. }) = action.downcast_ref() {
    if self.room_name_id.as_ref().is_some_and(|rn| rn.room_id() == room_id) {
        enqueue_popup_notification(
            "Message reported successfully.",
            PopupKind::Success,
            Some(4.0),
        );
    }
}
if let Some(ReportContentResultAction::Failed { room_id, error, .. }) = action.downcast_ref() {
    if self.room_name_id.as_ref().is_some_and(|rn| rn.room_id() == room_id) {
        enqueue_popup_notification(
            format!("Failed to report message.\n\nError: {error}"),
            PopupKind::Error,
            Some(5.0),
        );
    }
}
```

- [ ] **Step 5: Close modal on room-change actions**

Find the three `close_report_room_modal` calls triggered by room focus changes (~lines 5121, 5127, 5132). After each `self.close_report_room_modal(cx);`, add:

```rust
self.close_report_content_modal(cx);
```

The three locations are:
- `RoomsListAction::Selected` branch (~line 5121)
- `AppStateAction::RoomFocused` branch (~line 5127)  
- `AppStateAction::FocusNone` branch (~line 5132)

- [ ] **Step 6: Final build**

```bash
cargo build 2>&1
```

Expected: clean compile with zero errors.

- [ ] **Step 7: Commit**

```bash
git add src/home/room_screen.rs
git commit -m "feat: handle ReportContentModal actions and result notifications"
```

---

## Chunk 5: Manual testing

### Task 8: Test the feature end-to-end

- [ ] **Step 1: Run the app**

```bash
cargo run
```

- [ ] **Step 2: Test report button visibility**

Log in and navigate to a room with messages from other users.
- Right-click a message from **another user** → "Report" should appear in the context menu (below the divider, before "Delete" if visible).
- Right-click **your own message** → "Report" should NOT appear.

- [ ] **Step 3: Test modal flow**

Click "Report" on another user's message:
- Modal opens titled "Report Message".
- Submit without entering a reason → error label appears: "Please enter a reason before reporting."
- Enter a reason, leave "Ignore user" unchecked → click "Report" → modal closes, popup: "Message reported successfully."

- [ ] **Step 4: Test ignore checkbox**

Click "Report" again on a different message:
- Enter a reason, check "Ignore user" → click "Report" → modal closes.
- The ignored user's messages should be hidden (Matrix SDK clears and re-paginates the timeline automatically after ignore).

- [ ] **Step 5: Test cancel**

Click "Report", then click "Cancel" → modal closes with no action taken.

- [ ] **Step 6: Test room-switch cleanup**

Open the report modal, then switch to a different room → modal closes automatically.
