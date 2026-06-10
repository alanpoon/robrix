# Pinned Messages Panel Design

**Date:** 2026-06-10
**Branch:** feat/user_moderation_actions

## Overview

Add a "Pinned Messages" entry to the room list context menu (right-click a room). Clicking it opens a 320 px right-side sliding pane that lists the room's pinned messages — sender name, body preview, and timestamp per item. The panel follows the existing `RoomInfoSlidingPane` pattern exactly.

## Constraints

- `RoomContextMenuAction` is defined in `src/home/room_context_menu.rs` (line 140). Add `ViewPinnedMessages(OwnedRoomId)` as a new variant. App.rs already handles `RoomContextMenuAction::OpenRoomSettings` at line 1785; add a new handler for `ViewPinnedMessages` directly after it.
- The routing chain from room context menu to room_screen uses global actions, the same pattern as `RoomsListAction::Selected` and `AppStateAction::RoomFocused` which are dispatched globally and caught in room_screen's `for action in actions` loop (lines 5130–5148 of room_screen.rs). App.rs dispatches `cx.action(PinnedMessagesPanelAction::Open)` and room_screen catches it — this is proven by the existing global-action patterns in this codebase.
- `self.pinned_events: Vec<OwnedEventId>` is already maintained on `RoomScreen` (line 4798–4799), updated by `TimelineUpdate::PinnedEvents` (lines 7383–7384). Use this as the source of event IDs when opening the panel.
- If `pinned_events` is empty when the panel opens, skip the fetch and show the panel immediately in an empty state ("No pinned messages in this room.").
- The async handler fetches each pinned event individually via the Matrix SDK. The implementer must verify the exact SDK call; the candidate is `room.event(&event_id, None).await` returning `Result<TimelineEvent>`. Extract sender, body text, and timestamp from the deserialized event.
- **Animator direction (critical):** `slide: 1.0` = fully hidden (off-screen right); `slide: 0.0` = fully shown. The `show` animator state applies `slide: 0.0`; the `hide` state applies `slide: 1.0`. Default is `slide: 1.0`. `draw_walk` computes `right_margin = -(self.slide * 320.0)` — when `slide = 1.0`, `right_margin = -320` (panel off-screen); when `slide = 0.0`, `right_margin = 0` (fully visible). Background alpha: `(1.0 - self.slide) * 0.733`. Apply both via `script_apply_eval!` in `draw_walk`, matching the exact pattern in `RoomInfoSlidingPane::draw_walk` at lines 4535–4546.
- **Icon:** Use `ICON_THREADS` for `pinned_messages_button`. Do NOT use `ICON_PIN` (used by `favorite_button`, line 69) or `ICON_INFO` (used by `notifications_button`, line 95) — both would cause visual collisions. `ICON_THREADS` is unused in this menu and visually represents "messages".
- **Error type:** `PinnedMessagesFetchResult::Failed` uses `error: matrix_sdk::Error`, matching the convention established by `ReportContentResultAction::Failed` in `report_content_modal.rs`. `pinned_messages_panel.rs` must import `use matrix_sdk;` (or the specific error type).
- `PinnedMessagesPanel::register_widget(vm)` is called from `pub fn script_mod(vm: &mut ScriptVm)` in `pinned_messages_panel.rs`, which is called from `src/home/mod.rs`'s `script_mod` function — the same pattern as `report_content_modal::script_mod(vm)` at line 114. It must be added **before** `room_screen::script_mod(vm)` (line 120) since room_screen's DSL references `PinnedMessagesPanel`.
- **Height calculation** in `update_buttons`: current formula is `((if details.app_service_enabled { 9.0 } else { 8.0 }) * BUTTON_HEIGHT) + 20.0 + 10.0`. Adding one always-visible button gives `((if details.app_service_enabled { 10.0 } else { 9.0 }) * BUTTON_HEIGHT) + 20.0 + 10.0`.
- Close the panel on the same three branches that already call `close_report_content_modal`: `RoomsListAction::Selected` (line 5132), `AppStateAction::RoomFocused` (line 5139), and `AppStateAction::FocusNone` (line 5145). Add `self.close_pinned_messages_panel(cx)` alongside the existing close calls in each branch.
- Add `self.close_pinned_messages_panel(cx)` to `reset_app_service_ui` (line 6657) alongside existing close calls.

## Components

### 1. `src/home/pinned_messages_panel.rs` (new file)

**Data struct:**
```rust
#[derive(Clone, Debug)]
pub struct PinnedEventContent {
    pub event_id: OwnedEventId,
    pub sender_id: OwnedUserId,
    pub display_name: Option<String>,
    pub body: String,
    pub timestamp: MilliSecondsSinceUnixEpoch,
}
```

**Actions:**
```rust
#[derive(Clone, Default, Debug)]
pub enum PinnedMessagesPanelAction {
    Open,
    #[default]
    None,
}

#[derive(Clone, Debug)]
pub enum PinnedMessagesFetchResult {
    Fetched { room_id: OwnedRoomId, items: Vec<PinnedEventContent> },
    Failed { room_id: OwnedRoomId, error: matrix_sdk::Error },
}
```

**Widget struct:**
```rust
#[derive(Script, ScriptHook, Widget, Animator)]
pub struct PinnedMessagesPanel {
    #[deref] view: View,
    #[source] source: ScriptObjectRef,
    #[apply_default] animator: Animator,
    #[live] slide: f32,
    #[rust] is_animating_out: bool,
    #[rust] items: Vec<PinnedEventContent>,
}
```

**DSL template** (`mod.widgets.PinnedMessagesPanel`):
```
mod.widgets.PinnedMessagesPanel = #(PinnedMessagesPanel::register_widget(vm)) {
    visible: false,
    flow: Overlay,
    width: Fill,
    height: Fill,
    align: Align{x: 1.0, y: 0}

    bg_view := SolidView {
        width: Fill, height: Fill,
        visible: false,
        show_bg: true
        draw_bg.color: #000000BB
    }

    main_content := SolidView {
        width: 320,
        height: Fill,
        flow: Down,
        show_bg: true,
        draw_bg.color: (COLOR_PRIMARY)

        header := View {
            width: Fill, height: Fit,
            flow: Right,
            align: Align{y: 0.5},
            padding: Inset{top: 12, right: 10, bottom: 12, left: 15}

            title := Label {
                width: Fill, height: Fit,
                draw_text +: {
                    text_style: USERNAME_TEXT_STYLE { font_size: 12.5 }
                    color: #000
                }
                text: "Pinned Messages"
            }

            close_button := RobrixNeutralIconButton {
                width: Fit, height: Fit,
                spacing: 0, padding: 15,
                draw_icon.svg: (ICON_CLOSE)
                icon_walk: Walk{width: 14, height: 14}
                text: ""
            }
        }

        content_scroll := ScrollYView {
            width: Fill, height: Fill,
            flow: Down,
            padding: Inset{left: 12, right: 12, top: 8, bottom: 12}

            empty_label := Label {
                visible: false,
                width: Fill, height: Fit,
                draw_text +: { color: #888, text_style: REGULAR_TEXT { font_size: 11.0 } }
                text: "No pinned messages in this room."
            }

            items_list := View {
                width: Fill, height: Fit,
                flow: Down,
                spacing: 8
            }
        }
    }

    slide: 1.0,

    animator: Animator {
        panel: {
            default: @hide
            show: AnimatorState {
                redraw: true,
                from: {all: Forward {duration: 0.5}}
                ease: Ease.ExpDecay {d1: 0.80, d2: 0.97}
                apply: { slide: 0.0 }
            }
            hide: AnimatorState {
                redraw: true,
                from: {all: Forward {duration: 0.5}}
                ease: Ease.ExpDecay {d1: 0.80, d2: 0.97}
                apply: { slide: 1.0 }
            }
        }
    }
}
```

**`draw_walk`:** In Rust, before calling `self.view.draw_walk(cx, scope, walk)`:
```rust
let panel_width = 320.0;
let right_margin = -(self.slide * panel_width);
let mut main_content = self.view(cx, ids!(main_content));
script_apply_eval!(cx, main_content, { margin.right: #(right_margin) });
let bg_alpha = (1.0 - self.slide) * 0.733;
let bg_color = vec4(0.0, 0.0, 0.0, bg_alpha);
let mut bg_view = self.view(cx, ids!(bg_view));
script_apply_eval!(cx, bg_view, { draw_bg +: { color: #(bg_color) } });

// Show empty_label or items_list
let has_items = !self.items.is_empty();
self.view(cx, ids!(content_scroll.empty_label)).set_visible(cx, !has_items);
self.view(cx, ids!(content_scroll.items_list)).set_visible(cx, has_items);
// Draw each item inline via label set_text on named children,
// or by drawing a simple card per item using a PortalList.
// For v1, use Label widgets set via set_text for simplicity.
```

Each item is rendered as a simple `RoundedView` card with:
- A `Label` for sender (bold, display_name or sender_id localpart)
- A `Label` for body (truncated to ~2 lines if too long — use `body.chars().take(200).collect::<String>()`)
- A `Label` for formatted timestamp

Because item count is small (pinned lists are typically <20 items), draw them as a fixed `View { flow: Down }` in `items_list`, not a `PortalList`.

**`handle_event`:**
- Call `self.view.handle_event(cx, event, scope)`.
- If not visible, return early.
- Call `self.animator_handle_event(cx, event)` and redraw if needed.
- If `is_animating_out && !self.animator.is_track_animating(id!(panel))`: set `visible = false`, `is_animating_out = false`, revert key focus, hide bg_view, redraw.
- Close conditions: `close_button` clicked, `Escape` key, `FingerUp` outside `main_content` area, `back_pressed()`. On ANY close condition: call `self.hide(cx)` directly — do NOT dispatch `PinnedMessagesPanelAction::Close`. The panel manages its own visibility. Room_screen closes the panel only via `close_pinned_messages_panel(cx)` on room-switch and fetch-failure paths (not via a Close action). This matches the `RoomInfoSlidingPane` pattern exactly.

**`show(cx, items: Vec<PinnedEventContent>)`:**
```rust
pub fn show(&mut self, cx: &mut Cx, items: Vec<PinnedEventContent>) {
    self.items = items;
    self.visible = true;
    self.is_animating_out = false;
    cx.set_key_focus(self.view.area());
    self.animator_play(cx, ids!(panel.show));
    self.view(cx, ids!(bg_view)).set_visible(cx, true);
    self.view.button(cx, ids!(close_button)).reset_hover(cx);
    self.redraw(cx);
}
```

**`hide(cx)`:**
```rust
pub fn hide(&mut self, cx: &mut Cx) {
    if !self.visible { return; }
    self.is_animating_out = true;
    self.animator_play(cx, ids!(panel.hide));
    self.redraw(cx);
}
```

**`pub fn script_mod(vm: &mut ScriptVm)`** — calls `PinnedMessagesPanel::register_widget(vm)`.

**`PinnedMessagesPanelRef`** wrapper: `show(cx, items)` and `hide(cx)` methods delegating to inner.

### 2. `src/home/room_context_menu.rs`

- Add `ViewPinnedMessages(OwnedRoomId)` to `RoomContextMenuAction` (after `OpenRoomSettings`).
- Add `pinned_messages_button` DSL item between `copy_link_button` and `divider1`:
  ```
  pinned_messages_button := mod.widgets.RoomContextMenuButton {
      draw_icon +: { svg: (ICON_THREADS) }
      text: "Pinned Messages"
  }
  ```
  Use `ICON_THREADS` — `ICON_PIN` is taken by `favorite_button` and `ICON_INFO` is taken by `notifications_button`.
- In `handle_actions`: add click handler:
  ```rust
  else if self.button(cx, ids!(pinned_messages_button)).clicked(actions) {
      cx.action(RoomContextMenuAction::ViewPinnedMessages(
          details.room_name_id.room_id().clone()
      ));
      close_menu = true;
  }
  ```
- In `update_buttons`: add:
  ```rust
  self.button(cx, ids!(pinned_messages_button))
      .set_text(cx, "Pinned Messages");
  self.button(cx, ids!(pinned_messages_button)).reset_hover(cx);
  ```
- Update height formula to: `((if details.app_service_enabled { 10.0 } else { 9.0 }) * BUTTON_HEIGHT) + 20.0 + 10.0`.

### 3. `src/home/mod.rs`

- Add `pub mod pinned_messages_panel;` to the module declarations.
- Add `pinned_messages_panel::script_mod(vm);` in the `script_mod` function, before `room_screen::script_mod(vm)` (currently at line 120).

### 4. `src/sliding_sync.rs`

New `MatrixRequest` variant:
```rust
FetchPinnedMessageContent {
    room_id: OwnedRoomId,
    event_ids: Vec<OwnedEventId>,
}
```

Handler (spawned async task):
- Fetch each event: verify the exact SDK call (candidate: `room.event(&event_id, None).await`).
- Deserialize to extract: `sender_id` (from event sender field), `display_name` (try `room.get_member(&sender_id).await` → `.display_name().map(str::to_owned)`; use `None` on failure without propagating), `body` (from `m.room.message` `body` field; fallback `"[non-text event]"` for other types), `timestamp` (from `origin_server_ts`).
- Collect all into `Vec<PinnedEventContent>`.
- On complete success: `Cx::post_action(PinnedMessagesFetchResult::Fetched { room_id, items })`.
- On any event fetch error: `Cx::post_action(PinnedMessagesFetchResult::Failed { room_id, error })`.

Import `PinnedMessagesFetchResult` and `PinnedEventContent` from `crate::home::pinned_messages_panel` in the import block at line 51, alongside `ReportContentResultAction`.

### 5. `src/home/room_screen.rs`

**Import:**
```rust
use crate::home::pinned_messages_panel::{
    PinnedMessagesPanel, PinnedMessagesPanelAction,
    PinnedMessagesPanelWidgetRefExt, PinnedMessagesFetchResult,
};
```

**DSL:** Add `pinned_messages_panel := mod.widgets.PinnedMessagesPanel {}` to the overlay stack, after `room_info_sliding_pane` (line 3891).

**Helpers:**
```rust
fn open_pinned_messages_panel(&mut self, cx: &mut Cx) {
    let panel = self.view.pinned_messages_panel(cx, ids!(pinned_messages_panel));
    if self.pinned_events.is_empty() {
        panel.show(cx, vec![]);
    } else {
        let Some(room_id) = self.room_id().cloned() else { return };
        submit_async_request(MatrixRequest::FetchPinnedMessageContent {
            room_id,
            event_ids: self.pinned_events.clone(),
        });
        panel.show(cx, vec![]);  // opens immediately; content fills in on Fetched result
    }
}

fn close_pinned_messages_panel(&mut self, cx: &mut Cx) {
    self.view.pinned_messages_panel(cx, ids!(pinned_messages_panel)).hide(cx);
}
```

**Action handling** (in the `for action in actions` loop):
- `PinnedMessagesPanelAction::Open` → `self.open_pinned_messages_panel(cx)`.
- `PinnedMessagesFetchResult::Fetched { room_id, items }` — guard `if Some(room_id) == self.room_id()` → `self.view.pinned_messages_panel(cx, ids!(pinned_messages_panel)).show(cx, items)`.
- `PinnedMessagesFetchResult::Failed { room_id: _, error }` → `enqueue_popup_notification(format!("Failed to load pinned messages: {error}"), PopupKind::Error, Some(4.0))` and call `self.close_pinned_messages_panel(cx)`.

**Room-switch cleanup** — in the three existing branches (lines 5130–5148), add `self.close_pinned_messages_panel(cx)` alongside the existing modal close calls:
```rust
// RoomsListAction::Selected branch:
self.close_report_room_modal(cx);
self.close_report_content_modal(cx);
self.close_leave_room_confirm_modal(cx);
self.close_pinned_messages_panel(cx);  // ADD

// AppStateAction::RoomFocused branch: same additions
// AppStateAction::FocusNone branch: same additions
```

**`reset_app_service_ui`** (line 6657): add `self.close_pinned_messages_panel(cx);`.

### 6. `src/app.rs`

- Add `PinnedMessagesPanelAction` to the import from `crate::home::pinned_messages_panel`.
- After the existing `RoomContextMenuAction::OpenRoomSettings` handler (line 1785–1788):
  ```rust
  if let Some(RoomContextMenuAction::ViewPinnedMessages(_room_id)) = action.downcast_ref::<RoomContextMenuAction>() {
      cx.action(PinnedMessagesPanelAction::Open);
      continue;
  }
  ```

## Data Flow

```
right-click room in rooms list
  → RoomContextMenu shows "Pinned Messages" button
  → user clicks it
  → cx.action(RoomContextMenuAction::ViewPinnedMessages(room_id))
  → room_context_menu closes

  → app.rs catches RoomContextMenuAction::ViewPinnedMessages
  → cx.action(PinnedMessagesPanelAction::Open)  [global action]

  → room_screen's action loop catches PinnedMessagesPanelAction::Open
  → self.open_pinned_messages_panel(cx)
    → if pinned_events.is_empty(): panel.show(cx, vec![]) → empty state
    → else: submit MatrixRequest::FetchPinnedMessageContent { room_id, event_ids }
            panel.show(cx, vec![])  → panel opens in loading state

  → async handler: for each event_id, room.event(&event_id, ...).await
      → extract sender_id, display_name, body, timestamp
  → Cx::post_action(PinnedMessagesFetchResult::Fetched { room_id, items })

  → room_screen catches Fetched → panel.show(cx, items) → list rendered

Close paths:
  → close_button / Escape / back / click outside → panel calls self.hide(cx) directly
                                                   (no action dispatched — panel manages its own visibility,
                                                    matching RoomInfoSlidingPane pattern)
  → room switch (3 branches) → room_screen calls close_pinned_messages_panel(cx) → panel.hide(cx)
  → reset_app_service_ui → close_pinned_messages_panel(cx) → panel.hide(cx)
  → fetch failed → error popup + close_pinned_messages_panel(cx) → panel.hide(cx)
```

## Files Changed

| File | Change |
|------|--------|
| `src/home/pinned_messages_panel.rs` | New file: widget, DSL, show/hide, draw_walk, item rendering, actions |
| `src/home/room_context_menu.rs` | Add `ViewPinnedMessages` variant, `pinned_messages_button` DSL + click handler + update_buttons, height formula |
| `src/home/mod.rs` | `pub mod pinned_messages_panel;` + `pinned_messages_panel::script_mod(vm)` (before room_screen) |
| `src/sliding_sync.rs` | `FetchPinnedMessageContent` variant + async handler; import `PinnedEventContent`, `PinnedMessagesFetchResult` |
| `src/home/room_screen.rs` | DSL overlay entry, `open_/close_pinned_messages_panel` helpers, action handlers, room-switch + reset_app_service_ui cleanup |
| `src/app.rs` | Handle `ViewPinnedMessages` → `cx.action(PinnedMessagesPanelAction::Open)`; import `PinnedMessagesPanelAction` |
