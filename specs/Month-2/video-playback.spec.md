spec: task
name: "Month 2 — Video Message Rendering"
inherits: project
tags: [month-2, media, video, playback, controls]
---

## Intent

Replace the static "Video playback not yet supported" HTML placeholder
produced by `populate_video_message_content` in
`src/home/room_screen.rs:4604` with a **standalone Makepad widget** —
`VideoMessagePlayer` — that renders the video's poster frame, the audio
player's familiar control row, and a real frame surface driven by
Makepad's existing platform video primitive
(`AppleVideoPlayer` in
`/Users/alanpoon/Documents/rust/makepad/platform/src/os/apple/apple_video_playback.rs`,
or its platform-abstracted equivalent). The widget owns four visible
controls: a centered **Play / Pause** toggle, a **draggable seek slider**
along the bottom of the surface, a **Maximise** button in the top-left
corner, and a **Volume / Mute** button in the top-right corner. When the
video's mime type is not playable on the current platform, the widget
overlays an **unplayable** icon (`resources/icons/forbidden.svg`) in
front of the surface and disables Play, Mute, and the slider — Maximise
remains usable so the user can still inspect the poster frame at full
size.

The video metadata helpers from the previous revision of this spec —
`summarize_video_message` and `video_summary_html` in
`src/event_preview.rs` — are preserved verbatim and continue to render
the textual summary above or alongside the player surface. All new
control logic (playable-mime detection, mute/restore, slider drag,
maximise toggle) is implemented as **pure functions** under
`src/shared/video_message_player.rs` so the per-rule behavior is
exercised by `#[cfg(test)] mod tests_video_message_player` without
spinning up Makepad's video session.

## Decisions

### Module layout

- New widget `VideoMessagePlayer` at `src/shared/video_message_player.rs`,
  registered in `src/shared/mod.rs` alongside `audio_message_player.rs`.
  This widget owns the single Makepad `Video` widget (i.e. the single
  platform video session) per message and renders the inline overlay
  stack.
- New widget `VideoMessagePlayerModal` at
  `src/shared/video_message_player_modal.rs`, registered in
  `src/shared/mod.rs` after `video_message_player.rs` (it imports the
  shared state types from that module). This widget owns the centered
  card layout, the modal-only controls, and the close button — but it
  does NOT own a Makepad `Video` widget, and it does NOT render the
  scrim or its own overlay draw-list. Both of those are provided by
  the outer Makepad `Modal { content +: { ... } }` that wraps this
  widget (mirroring the `EventSourceModal` pattern at
  `src/home/event_source_modal.rs` and its `Modal` wrapper at
  `src/app.rs:124`).
- Exactly one `VideoMessagePlayerModal` instance lives at the
  `RoomScreen` level (not per-message), placed inside an outer
  `video_message_player_modal := Modal { content +: { ... } }` in the
  `RoomScreen` live_design block. When a maximise is triggered, the
  inline player emits `VideoMessagePlayerModalAction::Open { ... }`
  carrying the shared state handles and the `VideoSummary`; `RoomScreen`
  receives that action, calls `inner.show(...)` on the modal content
  widget, and opens the outer `Modal`. A single modal slot serves
  every video message in the room.
- Exposes `VideoMessagePlayerRef`, `VideoMessagePlayerWidgetRefExt`,
  `VideoMessagePlayerModalRef`, and
  `VideoMessagePlayerModalWidgetRefExt`, following the existing widget
  conventions used by `AudioMessagePlayer`.
- The textual summary helpers stay in `src/event_preview.rs`:
  - `summarize_video_message(&VideoMessageEventContent) -> VideoSummary`
  - `video_summary_html(&VideoSummary) -> String`
- `VideoSummary` is unchanged: `pub struct` with fields `filename: String`,
  `mime: Option<String>`, `duration_secs: Option<f64>`,
  `size_bytes: Option<u64>`, `dimensions: Option<(u64, u64)>`,
  `caption_html: Option<String>`.

### Layout (single overlay stack)

- The widget renders three z-layers, back-to-front, inside a single
  `flow: Overlay` parent View:
  1. **Surface layer** (back): the platform video frame texture while
     playing; the cached poster thumbnail otherwise. Sized at the video's
     dimensions if known, else `16 / 9` aspect at the parent width.
  2. **Unplayable layer** (middle): a centered `Icon` showing
     `resources/icons/forbidden.svg`, visible iff
     `should_show_unplayable_overlay(&summary)` returns `true`.
  3. **Controls layer** (front): four positioned controls.
- Controls positions on the controls layer (using absolute alignment
  inside the overlay):
  - `maximise_button`: top-left, 8 px from top and 8 px from left
  - `mute_button`: top-right, 8 px from top and 8 px from right
  - `play_button`: centered horizontally and vertically over the surface
  - `slider_row` (slider + elapsed `mm:ss` + total `mm:ss`): bottom-edge
    full-width strip, 6 px above the bottom of the surface

### Pure helpers (testable without a Makepad runtime)

- `pub fn is_playable_mime(mime: &str) -> bool` in
  `src/shared/video_message_player.rs`. Returns `true` for case-folded,
  parameter-stripped values:
  `"video/mp4"`, `"video/quicktime"`, `"video/x-m4v"`, `"video/webm"`,
  `"video/ogg"`. Returns `false` for anything else, including `""` and
  unknown codecs.
- `pub fn should_show_unplayable_overlay(summary: &VideoSummary) -> bool`
  in the same module. Returns `true` iff `summary.mime.as_deref()`
  exists and `is_playable_mime` returns `false` for it. If `summary.mime`
  is `None`, returns `false` (we optimistically show the player and let
  the platform decoder fail loudly during prepare).
- `pub fn apply_video_slider_drag(state: &mut VideoPlayerState, normalized_pos: f64, total_ms: u64, phase: DragPhase)`
  in the same module. Mirrors the audio implementation: `Start` and
  `Move` set `state.playing = false` and recompute `state.position_ms`;
  `End { was_playing }` resumes playback only if `was_playing` was true
  **and** `state.position_ms < total_ms`. `DragPhase` is re-exported
  from `audio_message_player` to avoid a duplicate enum.
- `pub fn apply_volume_action(state: &mut VideoVolumeState, action: VolumeAction)`
  where `VideoVolumeState` is `pub struct { pub muted: bool, pub level: f32, pub restore_level: f32 }`
  and `VolumeAction` is `pub enum { Mute, Unmute, SetLevel(f32) }`. Rules:
  - `Mute`: `state.restore_level = state.level; state.level = 0.0; state.muted = true;`
  - `Unmute`: `state.level = state.restore_level.max(0.05); state.muted = false;`
    (the `.max(0.05)` guard prevents an unmute that lands on silence
     because the user had dragged level to zero before muting).
  - `SetLevel(v)`: clamps `v` to `[0.0, 1.0]`, sets `state.level`, and
    sets `state.muted = state.level == 0.0` without touching
    `restore_level`.
- `pub fn toggle_maximise(state: &mut VideoUiState)` where
  `VideoUiState` is `pub struct { pub maximised: bool }`. Flips
  `maximised`.

### Backend mapping

- `VideoMessagePlayer` calls into Makepad's existing platform video
  primitive — `AppleVideoPlayer` on macOS/iOS (per
  `apple_video_playback.rs:34`), whose public surface already exposes
  `play()`, `pause()`, `resume()`, `mute()`, `unmute()`,
  `set_volume(volume: f64)`, `seek_to(position_ms: u64)`,
  `current_position_ms()`, and `poll_frame(textures: &mut CxTexturePool)`.
- The widget never instantiates `AppleVideoPlayer` directly. It goes
  through Makepad's `video_session` registration
  (`platform/src/video_session.rs`) so the same indirection covers Linux
  and Windows when their backends land — the platform layer is the
  abstraction boundary, not this widget.
- The capability check uses
  `makepad_platform::os::apple::apple_video_playback::can_play_type(mime)`
  *only as a secondary* signal; `is_playable_mime` is the primary,
  platform-agnostic check used to decide whether to draw the unplayable
  overlay. If the platform check disagrees at prepare time, the widget
  falls back to the unplayable overlay and surfaces the prepare error
  in the existing inline error label (which replaces the slider row).

### Single-active video playback

- Only one `VideoMessagePlayer` may have an active platform video
  session at a time. The same controller pattern as audio applies:
  starting playback on widget B pauses widget A. This reuses
  `audio_playback_controller`'s `AudioPlaybackAction::ActiveTrackChanged`
  pattern; the video equivalent is
  `VideoPlaybackAction::ActiveTrackChanged { now_playing: WidgetUid }`
  emitted via `Cx::post_action`. Audio and video active-track slots are
  independent — playing a video does not pause an audio message in
  another timeline cell.

### Maximise modal

`VideoMessagePlayerModal` follows the **same pattern as
`EventSourceModal`** (see `src/home/event_source_modal.rs` and its
outer-Modal wiring at `src/app.rs:124-131` / `src/app.rs:868-881`).
The pattern in one line: an outer Makepad `Modal { content +: {
inner := VideoMessagePlayerModal {} } }` provides the scrim, the
overlay draw-list, and the scrim-click / Escape / back-press
dismissal; the inner widget renders only the card body.

- **Outer wrapper, owned by `RoomScreen`'s live_design block.** A
  `video_message_player_modal := Modal { content +: {
  video_message_player_modal_inner := VideoMessagePlayerModal {} } }`
  block sits at the same level as the timeline. The outer `Modal`
  (Makepad's built-in widget) is the one with the `DrawList2d` and
  the scrim. The inner widget never draws a scrim itself.
- **Inner widget body, rendered by `VideoMessagePlayerModal`.** No
  scrim. No overlay draw-list. Just the card body:
  1. **Card** layer: a centered, rounded view that defines the rect
     into which the inline player's surface will be drawn. Sized to
     fill 90% of the viewport width *or* 90% of the viewport height
     — whichever bound is reached first — while preserving the
     source's aspect ratio. The card itself does NOT contain a
     Makepad `Video` widget; it writes the card's screen-space rect
     into `ui_state.lock().unwrap().card_rect` on every draw, and
     the inline `VideoMessagePlayer` reads that rect on its next
     draw to position its single `Video` surface into the card area
     via the inline player's own elevated `DrawList2d`.
  2. **Controls** layer drawn on top of the card, mirroring the
     inline control positions: centered Play / Pause, bottom slider
     strip with elapsed / total `mm:ss` labels, and the same Mute
     button in the card's top-right. Each control reads/writes the
     shared `Arc<Mutex<...>>` state — never a copy.
  3. **close_button** at the card's top-right corner showing
     `resources/icons/close.svg`. The modal does NOT show a
     Maximise button — its role is fulfilled by `close_button`.
- **Action protocol** (mirrors `EventSourceModalAction`):
  - `VideoMessagePlayerModalAction::Open { player_state,
    volume_state, ui_state, summary }` — emitted by the inline
    player's `maximise_button` handler. `RoomScreen` receives this
    action, calls `inner.show(cx, player_state, volume_state,
    ui_state, summary)`, then calls `outer.open(cx)`.
  - `VideoMessagePlayerModalAction::Close` — emitted by the inner
    widget when its `close_button` is clicked. `RoomScreen` receives
    this action and calls `outer.close(cx)`.
- **Dismissal paths and `ModalAction::Dismissed`.** When the user
  dismisses the outer `Modal` via the scrim, the Escape key, or
  back-press, the outer `Modal` self-closes and emits
  `ModalAction::Dismissed`. The inner widget observes this in its
  `WidgetMatchEvent::handle_actions` implementation and uses it to
  flip `ui_state.lock().unwrap().maximised = false` so the inline
  player exits its elevated-draw mode. The inner widget MUST NOT
  re-emit `VideoMessagePlayerModalAction::Close` in response to
  `ModalAction::Dismissed` (this would create an infinite action
  feedback loop — see the comment at
  `src/home/event_source_modal.rs:293-297`).
- **`ui_state.maximised` is flipped in exactly two places** so the
  invariant "one writer at a time" is preserved:
  1. `RoomScreen` sets it to `true` when it handles
     `VideoMessagePlayerModalAction::Open`.
  2. The inner widget sets it to `false` when it observes either a
     `close_button` click or a `ModalAction::Dismissed`.
- Opening or closing the modal NEVER auto-pauses or auto-plays. The
  shared `VideoPlayerState.playing` is unchanged across the
  transition.

### Shared playback state

- `VideoMessagePlayer` owns exactly one
  `player_state: Arc<Mutex<VideoPlayerState>>` field. The inline view
  and the modal view both bind their slider and Play / Pause button
  through this same `Arc` — by `Arc::clone`, not by copying the inner
  state. Mutations from either view are observable in the other on the
  next paint.
- `VideoMessagePlayer` owns exactly one platform video session — one
  Makepad `Video` widget (which in turn owns one `AppleVideoPlayer`
  handle on macOS/iOS, or its registered
  `register_video_frame_session` equivalent on non-Apple platforms).
  When the modal opens, the inline player's `Video` widget is the
  same one whose frames the user sees inside the modal card; no
  second decode pipeline and no second audio sink is instantiated. The
  inline player conditionally promotes its surface draw into its own
  elevated `DrawList2d` while `ui_state.maximised == true`, drawing the
  same `Video` widget into the card rect that the modal writes.
- The volume state behaves the same way: one
  `volume_state: Arc<Mutex<VideoVolumeState>>` is shared between the
  inline Mute button and the modal Mute button. Muting in the modal
  mutes the inline view, and vice versa.
- The UI state is also shared: one
  `ui_state: Arc<Mutex<VideoUiState>>` is held by both the inline
  player and the modal. `VideoUiState` is
  `pub struct { pub maximised: bool, pub card_rect: Option<Rect> }`.
  The `maximised` flag is the single source of truth for "modal
  visible." The `card_rect` field is written by the modal on each
  draw and read by the inline player on its next draw so the elevated
  surface lands inside the card.
- For convenience, the module exposes type aliases
  `pub type SharedPlayerState = Arc<Mutex<VideoPlayerState>>`,
  `pub type SharedVolumeState = Arc<Mutex<VideoVolumeState>>`, and
  `pub type SharedUiState = Arc<Mutex<VideoUiState>>`.
  `VideoMessagePlayerModal::show(...)` (the inner-widget entry point,
  named to match the `EventSourceModal::show` convention) takes these
  three handles plus the `VideoSummary` so the modal can render
  `total_ms` labels without reaching back into the inline player.
- Closing the modal NEVER resets `player_state.position_ms`,
  `player_state.playing`, `volume_state.muted`, or
  `volume_state.level`. The inline view resumes from wherever the
  modal left it (and vice versa).

### Unplayable visual state

- When `should_show_unplayable_overlay` returns `true`:
  - The `forbidden.svg` icon is rendered centered, at 48 × 48 dp.
  - `play_button` is `set_enabled(cx, false)`.
  - `slider_row` is `set_visible(cx, false)`.
  - `mute_button` is `set_enabled(cx, false)`.
  - `maximise_button` remains enabled so the user can still expand the
    poster frame.

### Icon resources

- `resources/icons/play.svg` — already in tree (used by audio player).
- `resources/icons/pause.svg` — already in tree.
- `resources/icons/forbidden.svg` — already in tree; reused as the
  unplayable overlay.
- `resources/icons/close.svg` — already in tree; reused as the
  modal `close_button`.
- `resources/icons/maximise.svg` — new SVG, a 24 × 24 outlined "expand
  to full screen" glyph.
- `resources/icons/volume_on.svg` — new SVG, a 24 × 24 speaker glyph.
- `resources/icons/volume_off.svg` — new SVG, a 24 × 24 muted-speaker
  glyph.

### Thumbnail / poster frame

- Thumbnail fetch is dispatched through
  `MediaCache::try_get_media_or_fetch(&mxc, MediaFormat::Thumbnail(_))`
  whenever the message's `MediaSource` is `MediaSource::Plain` and
  `info.thumbnail_source` is `None` — reusing the existing thumbnail
  negotiation in `src/media_cache.rs`. The fetched thumbnail is rendered
  on the surface layer until the first decoded video frame arrives.

### `populate_video_message_content` rewrite

- Signature changes to accept a `VideoMessagePlayerRef` (a new
  live_design slot in the per-message template) **in addition to** the
  existing `HtmlOrPlaintextRef`. The textual summary built by
  `video_summary_html` continues to be written into the
  `HtmlOrPlaintextRef`; the player widget is fed the `VideoSummary`,
  the `MediaSource`, and the `MediaCache`. The `bool` return contract is
  preserved: returns `true` once both the summary and the player
  scaffolding have been written.
- The per-message live_design template in `src/home/room_screen.rs`
  gains a `video_player = <VideoMessagePlayer>` slot adjacent to the
  existing `message_content` slot, hidden by default and made visible
  only for `MessageType::Video`.

### Cargo

- No new dependency. Frame decode goes through Makepad's existing
  platform video primitive; the project-level constraint "no full media
  decoder, no ffmpeg binding" continues to apply.

## Constraints

- Must NOT auto-play on render. The widget loads the poster thumbnail
  and disabled controls until the user clicks Play.
- Must NOT crash when the mime is unsupported. When
  `should_show_unplayable_overlay` returns `true`, the widget shows
  `forbidden.svg`, disables Play/Mute/slider, and never invokes
  `AppleVideoPlayer::play()`.
- Must NOT change the signature of `summarize_video_message` or
  `video_summary_html`.
- Must NOT unmute on slider drag. `apply_video_slider_drag` only
  touches `VideoPlayerState.position_ms` and `playing`; mute state is
  owned by `VideoVolumeState` and is preserved across drags.
- Must NOT decode video frames on the UI thread. Frame production goes
  through `AppleVideoPlayer::poll_frame` (or its registered video
  session callback), which the platform layer drives off the UI thread.
- Must NOT call `.unwrap()` on `info.width` / `info.height`; missing
  dimensions still produce `VideoSummary.dimensions = None`.
- Must NOT register more than one platform video session per widget
  instance. The widget owns at most one `AppleVideoPlayer` handle and
  drops it on `Widget::handle_event(Destruct)`.
- Must NOT play two videos simultaneously. The single-active
  `VideoPlaybackAction::ActiveTrackChanged` broadcast pauses every
  other `VideoMessagePlayer` exactly as audio does.
- Must NOT instantiate a second `AppleVideoPlayer` or any second
  platform video session when the modal opens. The inline view and the
  modal view share exactly one session and one decoded-frame stream.
- Must NOT clone `VideoPlayerState` or `VideoVolumeState` by value
  into the modal. Both views bind to the same `Arc<Mutex<...>>` handles
  so a mutation in one view is immediately observable in the other.
- Must NOT auto-pause or auto-play when the modal opens or closes.
  Transitioning between inline and modal is a pure presentation change;
  `state.playing` is left untouched.
- Must NOT reset `state.position_ms` or `volume.muted` on modal close.
  The inline view continues from the position and mute state the modal
  left behind.

## Boundaries

### Allowed Changes

- src/home/room_screen.rs
- src/event_preview.rs
- src/media_cache.rs
- src/shared/text_or_image.rs
- src/shared/video_message_player.rs (new)
- src/shared/video_message_player_modal.rs (new)
- src/shared/mod.rs
- src/lib.rs
- resources/icons/maximise.svg (new)
- resources/icons/volume_on.svg (new)
- resources/icons/volume_off.svg (new)
- Cargo.toml

### Forbidden

- Do not invent a new "video" template id; reuse `id!(Message)` and
  `id!(CondensedMessage)` exactly as today (see
  `src/home/room_screen.rs:4001`).
- Do not duplicate `summarize_audio_message` logic — `VideoSummary` is
  a separate struct with its own helpers.
- Do not add a video-decoding crate (no ffmpeg, no gstreamer). The
  frame pipeline goes through Makepad's existing platform primitive.
- Do not introduce a second active-track controller. The video
  controller mirrors the audio controller's shape (one mutex-guarded
  active track, one broadcast action), not a new orchestration layer.
- Do not auto-pause audio when video plays, and do not auto-pause
  video when audio plays. The two media types have independent
  active-track slots.
- Do not use cargo fmt

## Completion Criteria

Scenario: Video summary captures dimensions when width and height are both set
  Test:
    Package: robrix
    Filter: test_video_summary_full_info
  Given a `VideoMessageEventContent` whose `body` is `"clip.mp4"`
  And whose `info.mimetype` is `Some("video/mp4")`
  And whose `info.width` is `Some(1920_u64.into())`
  And whose `info.height` is `Some(1080_u64.into())`
  And whose `info.duration` is `Some(Duration::from_millis(7_250))`
  And whose `info.size` is `Some(2_048_000_u64.into())`
  When `summarize_video_message` is called
  Then `VideoSummary.dimensions` equals `Some((1920, 1080))`
  And `VideoSummary.duration_secs` equals `Some(7.25)`
  And `VideoSummary.size_bytes` equals `Some(2_048_000)`
  And `VideoSummary.mime` equals `Some("video/mp4".to_string())`

Scenario: Video summary drops dimensions when only one of width or height is set
  Test:
    Package: robrix
    Filter: test_video_summary_partial_dimensions
  Given a `VideoMessageEventContent` whose `info.width` is `Some(1280_u64.into())`
  And whose `info.height` is `None`
  When `summarize_video_message` is called
  Then `VideoSummary.dimensions` equals `None`

Scenario: Video summary handles a missing info block
  Test:
    Package: robrix
    Filter: test_video_summary_missing_info
  Given a `VideoMessageEventContent` whose `info` is `None`
  When `summarize_video_message` is called
  Then `VideoSummary.dimensions` equals `None`
  And `VideoSummary.duration_secs` equals `None`
  And `VideoSummary.size_bytes` equals `None`
  And `VideoSummary.mime` equals `None`

Scenario: Video summary HTML renders dimensions in the WIDTHxHEIGHT shape
  Test:
    Package: robrix
    Filter: test_video_summary_html_includes_dimensions
  Given a `VideoSummary` whose `dimensions` is `Some((640, 480))`
  When `video_summary_html` is called
  Then the returned string contains the substring `"640x480"`

Scenario: Video summary HTML omits the dimensions line when none are known
  Test:
    Package: robrix
    Filter: test_video_summary_html_omits_dimensions_when_none
  Given a `VideoSummary` whose `dimensions` is `None`
  When `video_summary_html` is called
  Then the returned string does NOT contain the character `'x'` between two digit runs
  And the returned string does NOT contain the substring `"None"`

Scenario: Video summary HTML escapes a hostile filename
  Test:
    Package: robrix
    Filter: test_video_summary_html_escapes_filename
  Given a `VideoSummary` whose `filename` is `"<img src=x onerror=1>.mp4"`
  When `video_summary_html` is called
  Then the returned string contains `"&lt;img"`
  And the returned string does NOT contain `"<img "`

Scenario: is_playable_mime accepts the common mp4 mime
  Test:
    Package: robrix
    Filter: test_is_playable_mime_accepts_mp4
  Given the mime string `"video/mp4"`
  When `is_playable_mime` is called
  Then the result equals `true`

Scenario: is_playable_mime accepts webm and ogg
  Test:
    Package: robrix
    Filter: test_is_playable_mime_accepts_webm_and_ogg
  Given each of the mime strings `"video/webm"` and `"video/ogg"`
  When `is_playable_mime` is called on each
  Then the result equals `true` for both

Scenario: is_playable_mime is case-insensitive and strips parameters
  Test:
    Package: robrix
    Filter: test_is_playable_mime_normalizes_input
  Given the mime string `"VIDEO/MP4; codecs=avc1.42E01E"`
  When `is_playable_mime` is called
  Then the result equals `true`

Scenario: is_playable_mime rejects unsupported codecs
  Test:
    Package: robrix
    Filter: test_is_playable_mime_rejects_unsupported
  Given the mime string `"video/x-matroska"`
  When `is_playable_mime` is called
  Then the result equals `false`

Scenario: is_playable_mime rejects an empty mime
  Test:
    Package: robrix
    Filter: test_is_playable_mime_rejects_empty
  Given the mime string `""`
  When `is_playable_mime` is called
  Then the result equals `false`

Scenario: Unplayable overlay shows when summary mime is unsupported
  Test:
    Package: robrix
    Filter: test_should_show_unplayable_overlay_for_unsupported_mime
  Given a `VideoSummary` whose `mime` equals `Some("video/x-matroska".to_string())`
  When `should_show_unplayable_overlay` is called
  Then the result equals `true`

Scenario: Unplayable overlay does NOT show when summary mime is playable
  Test:
    Package: robrix
    Filter: test_should_show_unplayable_overlay_false_for_mp4
  Given a `VideoSummary` whose `mime` equals `Some("video/mp4".to_string())`
  When `should_show_unplayable_overlay` is called
  Then the result equals `false`

Scenario: Unplayable overlay does NOT show when mime is missing
  Test:
    Package: robrix
    Filter: test_should_show_unplayable_overlay_false_when_mime_none
  Given a `VideoSummary` whose `mime` equals `None`
  When `should_show_unplayable_overlay` is called
  Then the result equals `false`

Scenario: Slider drag start pauses an already-playing video
  Test:
    Package: robrix
    Filter: test_apply_video_slider_drag_start_pauses_playback
  Given a `VideoPlayerState { playing: true, position_ms: 1_000 }`
  When `apply_video_slider_drag` is called with `phase = Start`, `normalized_pos = 0.5`, and `total_ms = 4_000`
  Then `state.playing` equals `false`
  And `state.position_ms` equals `2_000`

Scenario: Slider drag end resumes playback when the track was playing before drag
  Test:
    Package: robrix
    Filter: test_apply_video_slider_drag_end_resumes_when_was_playing
  Given a `VideoPlayerState { playing: false, position_ms: 500 }` and `was_playing = true`
  When `apply_video_slider_drag` is called with `phase = End`, `normalized_pos = 0.25`, and `total_ms = 4_000`
  Then `state.playing` equals `true`
  And `state.position_ms` equals `1_000`

Scenario: Slider drag end does NOT resume when scrubbed to the very end
  Test:
    Package: robrix
    Filter: test_apply_video_slider_drag_end_does_not_resume_at_end
  Given `was_playing = true`
  When `apply_video_slider_drag` is called with `phase = End`, `normalized_pos = 1.0`, and `total_ms = 4_000`
  Then `state.playing` equals `false`

Scenario: Mute snapshots the current level into restore_level
  Test:
    Package: robrix
    Filter: test_apply_volume_action_mute_snapshots_level
  Given a `VideoVolumeState { muted: false, level: 0.6, restore_level: 0.0 }`
  When `apply_volume_action` is called with `VolumeAction::Mute`
  Then `state.muted` equals `true`
  And `state.level` equals `0.0`
  And `state.restore_level` equals `0.6`

Scenario: Unmute restores the snapshotted level
  Test:
    Package: robrix
    Filter: test_apply_volume_action_unmute_restores_level
  Given a `VideoVolumeState { muted: true, level: 0.0, restore_level: 0.6 }`
  When `apply_volume_action` is called with `VolumeAction::Unmute`
  Then `state.muted` equals `false`
  And `state.level` equals `0.6`

Scenario: Unmute from a zero restore_level uses the 0.05 minimum guard
  Test:
    Package: robrix
    Filter: test_apply_volume_action_unmute_uses_min_guard
  Given a `VideoVolumeState { muted: true, level: 0.0, restore_level: 0.0 }`
  When `apply_volume_action` is called with `VolumeAction::Unmute`
  Then `state.muted` equals `false`
  And `state.level` equals `0.05`

Scenario: SetLevel clamps to the range zero through one
  Test:
    Package: robrix
    Filter: test_apply_volume_action_set_level_clamps
  Given a `VideoVolumeState { muted: false, level: 0.5, restore_level: 0.0 }`
  When `apply_volume_action` is called with `VolumeAction::SetLevel(1.7)`
  Then `state.level` equals `1.0`
  And calling it again with `VolumeAction::SetLevel(-0.2)` results in `state.level == 0.0`

Scenario: SetLevel to zero infers muted without touching restore_level
  Test:
    Package: robrix
    Filter: test_apply_volume_action_set_level_zero_implies_muted
  Given a `VideoVolumeState { muted: false, level: 0.5, restore_level: 0.2 }`
  When `apply_volume_action` is called with `VolumeAction::SetLevel(0.0)`
  Then `state.muted` equals `true`
  And `state.restore_level` equals `0.2`

Scenario: Toggle maximise flips the state and back
  Test:
    Package: robrix
    Filter: test_toggle_maximise_round_trip
  Given a `VideoUiState { maximised: false }`
  When `toggle_maximise` is called once
  Then `state.maximised` equals `true`
  And calling `toggle_maximise` again results in `state.maximised == false`

Scenario: Close button shares the same toggle_maximise path as the maximise button
  Test:
    Package: robrix
    Filter: test_close_button_closes_via_toggle_maximise
  Given a `VideoUiState { maximised: true }` representing an open modal
  When the modal's `close_button` handler invokes `toggle_maximise`
  Then `state.maximised` equals `false`
  And calling `toggle_maximise` from the scrim handler on a fresh
       `VideoUiState { maximised: true }` also produces `maximised == false`

Scenario: VideoPlayerState clones via Arc<Mutex<...>> share mutations
  Test:
    Package: robrix
    Filter: test_video_player_state_arc_clones_share_mutations
  Given `let state_a = Arc::new(Mutex::new(VideoPlayerState { playing: true, position_ms: 0 }));`
  And `let state_b = Arc::clone(&state_a);`
  When the test writes `state_b.lock().unwrap().position_ms = 4_321`
  Then `state_a.lock().unwrap().position_ms` equals `4_321`
  And `Arc::ptr_eq(&state_a, &state_b)` equals `true`

Scenario: Slider drag inside the modal updates the shared VideoPlayerState
  Test:
    Package: robrix
    Filter: test_modal_slider_drag_updates_shared_state
  Given `let state = Arc::new(Mutex::new(VideoPlayerState { playing: true, position_ms: 0 }));`
  And `let modal_binding = Arc::clone(&state);`
  When the test calls `apply_video_slider_drag(&mut modal_binding.lock().unwrap(), DragPhase::Start, 0.5, 4_000)`
  Then `state.lock().unwrap().position_ms` equals `2_000`
  And `state.lock().unwrap().playing` equals `false`

Scenario: Muting from the modal mutes the inline view via the shared VideoVolumeState
  Test:
    Package: robrix
    Filter: test_modal_mute_propagates_to_inline_volume
  Given `let volume = Arc::new(Mutex::new(VideoVolumeState { muted: false, level: 0.7, restore_level: 0.0 }));`
  And `let inline_binding = Arc::clone(&volume);`
  When the test calls `apply_volume_action(&mut volume.lock().unwrap(), VolumeAction::Mute)`
  Then `inline_binding.lock().unwrap().muted` equals `true`
  And `inline_binding.lock().unwrap().level` equals `0.0`
  And `inline_binding.lock().unwrap().restore_level` equals `0.7`

Scenario: Closing the modal preserves position, play state, and mute
  Test:
    Package: robrix
    Filter: test_close_modal_preserves_playback_state
  Given a shared `state: Arc<Mutex<VideoPlayerState>>` whose inner value is `VideoPlayerState { playing: true, position_ms: 3_500 }`
  And a shared `volume: Arc<Mutex<VideoVolumeState>>` whose inner value has `muted = true`, `level = 0.0`, `restore_level = 0.6`
  And `let ui = VideoUiState { maximised: true };`
  When `toggle_maximise(&mut ui)` is called from the close_button handler
  Then `ui.maximised` equals `false`
  And `state.lock().unwrap().position_ms` equals `3_500`
  And `state.lock().unwrap().playing` equals `true`
  And `volume.lock().unwrap().muted` equals `true`
  And `volume.lock().unwrap().level` equals `0.0`

## Out of Scope

- Transcoding or re-encoding video.
- Subtitle / closed-caption rendering.
- Frame extraction or scrubbing previews independent of the platform
  video session.
- Multi-active video playback (only one video plays at a time).
- Per-widget custom playback rate beyond 1.0×.
- Picture-in-picture mode separate from the in-timeline / fullscreen
  toggle.
- Persisting playback position or mute state across app restarts.
- Auto-advance to the next video message after the current one ends.
- Any change to `MediaCache` semantics beyond calling its existing
  thumbnail fetch path.
