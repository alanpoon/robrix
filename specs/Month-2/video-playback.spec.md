spec: task
name: "Element-style Video Player"
tags: [video, playback, blurhash, modal, fullscreen, media, element]
---

## Intent

Implement the video-message rendering path in robrix's room timeline so
that it matches the progressive-display UX already used for image
messages by `populate_image_message_content` in
`src/home/room_screen.rs:4432`. The image populator runs through three
states driven by `MediaCache::try_get_media_or_fetch`:

1. `MediaCacheEntry::Loaded(data)` — render the decoded media.
2. `MediaCacheEntry::Requested` — decode the message's blurhash into a
   placeholder texture (capped at `BLURHASH_IMAGE_MAX_SIZE = 500 px`
   per `src/home/room_screen.rs:56`) while the network fetch is in
   flight.
3. `MediaCacheEntry::Failed(_status_code)` — render an error string in
   place of the media.

The video path applied by `populate_video_message_content` in
`src/home/room_screen.rs:4652` adopts the same three-state shape, but
because video has two separate cached artefacts (a thumbnail JPEG/PNG
and the playable video file), the lifecycle is layered:

- Layer A (poster): `MediaCacheEntry::Requested` for the thumbnail
  source → render decoded blurhash; `MediaCacheEntry::Loaded(data)` →
  render the decoded thumbnail PNG/JPG; `MediaCacheEntry::Failed` →
  fall back to the blurhash placeholder (or solid `#222` if no
  blurhash is available).
- Layer B (playable): `MediaCacheEntry::Requested` for the full video
  file → keep showing Layer A and disable the play button;
  `MediaCacheEntry::Loaded(data)` → write the cached file path into the
  `RobrixVideo` widget and enable the play button;
  `MediaCacheEntry::Failed` → keep Layer A and surface the error in
  the existing inline error label.

A **maximise button** is overlaid in the top-left corner of every
video message. Clicking it opens a Makepad `Modal` that fills the host
window via OS-level fullscreen (mirrors
`/Users/alanpoon/Documents/rust/makepad/examples/media_player/src/lib.rs`).
The modal carries a second `RobrixVideo` instance bound to the same
source; the two instances swap which one holds the active platform
video session (inline stops, modal begins). Position is preserved
inline → modal via a deferred `seek_to` on
`VideoAction::PlaybackPrepared`. The modal closes via four equivalent
paths — **close-button click**, **modal scrim click**, **Escape key**,
and **back-press** — all routed through a single
`close_video_modal(&mut self, cx: &mut Cx)` helper so the cleanup
order cannot drift.

## Decisions

### Three-state lifecycle (mirror of `populate_image_message_content`)

- `populate_video_message_content(cx, message_content_widget,
  video_player, video, media_cache)` in `src/home/room_screen.rs:4652`
  is extended to consult the `MediaCache` for both the poster and the
  playable file. The function's `bool` return contract is preserved:
  it returns `true` once every layer that can be drawn has been
  drawn this frame.
- For Layer A (poster), the same `try_get_media_or_fetch` call pattern
  used by `populate_image_message_content` is invoked. The match arm
  shapes mirror lines `4492-4564` exactly:
  - `(MediaCacheEntry::Loaded(data), _media_format)` — decode the PNG
    or JPG via `utils::load_png_or_jpg(&img, cx, &data)` and call
    `video_player.set_poster_texture(cx, texture)`.
  - `(MediaCacheEntry::Requested, _media_format)` — when
    `(Some(blurhash), Some(width), Some(height))` are all present on
    `video.info`, decode the blurhash via `blurhash::decode(blurhash,
    capped_width, capped_height, 1.0)` and pass the resulting
    `ImageBuffer` into `video_player.set_blurhash_texture(cx,
    texture)`. The width/height capping logic is copy-equivalent to
    `src/home/room_screen.rs:4519-4529`: capped at
    `BLURHASH_IMAGE_MAX_SIZE`, aspect-preserving.
  - `(MediaCacheEntry::Failed(_status_code), _media_format)` — fall
    back to the blurhash placeholder if one is available; otherwise
    set the poster to the solid-color fallback returned by
    `placeholder_fallback_color()`.
- For Layer B (playable file), a second `try_get_media_or_fetch` call
  with `MediaFormat::File` is invoked. The match arm shapes mirror
  the same three branches, dispatching to:
  - `Loaded(file_bytes)` — write the cached file path returned by
    `MediaCache::path_for(&mxc)` into the `RobrixVideo`'s
    `source_url` and call
    `video_player.set_play_enabled(cx, true)`.
  - `Requested` — keep Layer A visible, call
    `video_player.set_play_enabled(cx, false)`, return `false` so the
    caller re-polls on the next paint.
  - `Failed(status_code)` — show the inline error label (replacing
    the slider row) with `Failed to fetch video from {mxc} (HTTP
    {status_code})` and leave Layer A visible.

### Blurhash decode

- Reuse the existing `blurhash` crate (version pinned in `Cargo.toml`
  alongside the `populate_image_message_content` usage). Do not add a
  second blurhash crate version.
- The `BLURHASH_IMAGE_MAX_SIZE` constant at
  `src/home/room_screen.rs:56` is reused verbatim — do not redefine
  it in the video module.
- Pure helper `pub fn cap_blurhash_dimensions(width: u32, height: u32,
  max: u32) -> (u32, u32)` lives in `src/shared/robrix_video.rs`. It
  returns the capped (width, height) preserving aspect ratio,
  matching the logic at `src/home/room_screen.rs:4519-4529`. Returns
  `(0, 0)` when `width == 0 || height == 0`. Returns the input
  unchanged when both dimensions are already `<= max`.
- Pure helper `pub fn decode_blurhash_to_rgba(blurhash: &str, width:
  u32, height: u32) -> Option<Vec<u8>>` in the same module. Returns
  `Some(buf)` of length `width * height * 4` for a valid blurhash and
  positive dimensions. Returns `None` for an empty string, a
  malformed blurhash, or when `width == 0 || height == 0`.
- Pure helper `pub fn placeholder_fallback_color() -> [u8; 4]` returns
  `[0x22, 0x22, 0x22, 0xFF]` — the solid color used when no blurhash
  is available and the thumbnail has not yet loaded.

### `RobrixVideo` standalone widget

- New widget `RobrixVideo` at `src/shared/robrix_video.rs`. It is the
  reusable, Matrix-agnostic surface that wraps Makepad's built-in
  `Video` widget. It accepts:
  - `#[live] source_url: PathBuf` — local filesystem path to a video
    file already cached on disk. The widget never fetches.
  - `#[live] blurhash: Option<String>` — placeholder string rendered
    behind the video surface until the first decoded frame arrives.
- `RobrixVideo` constructs Makepad's `Video` widget internally with
  `autoplay: false`, `is_looping: false`, `show_controls: false`. The
  inner `Video`'s `source: VideoDataSource.File` is bound to
  `self.source_url`.
- Public methods (delegated to the inner Makepad `Video`):
  - `pub fn begin_playback(&mut self, cx: &mut Cx)`
  - `pub fn stop_and_cleanup_resources(&mut self, cx: &mut Cx)`
  - `pub fn pause_playback(&mut self, cx: &mut Cx)`
  - `pub fn resume_playback(&mut self, cx: &mut Cx)`
  - `pub fn is_playing(&self) -> bool`
  - `pub fn current_position_ms(&self) -> u64`
  - `pub fn seek_to(&mut self, cx: &mut Cx, position_ms: u64)`
  - `pub fn set_source_url(&mut self, cx: &mut Cx, path: PathBuf)`
  - `pub fn set_blurhash(&mut self, cx: &mut Cx, blurhash: Option<String>)`
  - `pub fn set_poster_texture(&mut self, cx: &mut Cx, texture: Texture)`
  - `pub fn set_blurhash_texture(&mut self, cx: &mut Cx, texture: Texture)`
- Visibility rule (pure function):
  - `pub fn should_show_blurhash(state: BlurhashState) -> bool` where
    `BlurhashState` is `pub enum { NoSource, NotYetStarted,
    AwaitingFirstFrame, Playing, Stopped }`. Returns `true` for
    `NoSource`, `NotYetStarted`, `AwaitingFirstFrame`, and `Stopped`.
    Returns `false` for `Playing`.

### Inline `VideoMessagePlayer` composer

- `VideoMessagePlayer` at `src/shared/video_message_player.rs` is the
  message-aware composer. It owns ONE `RobrixVideo` instance plus the
  control overlay.
- Controls (`flow: Overlay` parent View):
  - `maximise_button`: top-left, 8 px from top and 8 px from left.
    Uses `resources/icons/maximise.svg` (24 × 24).
  - `play_button`: centered horizontally and vertically over the
    surface. Disabled until Layer B reports `Loaded`.
  - `mute_button`: top-right, 8 px from top and 8 px from right. Uses
    `resources/icons/volume_on.svg` or `volume_off.svg`.
  - `slider_row` (slider + elapsed `mm:ss` + total `mm:ss`):
    bottom-edge full-width strip, 6 px above the bottom of the
    surface.
- `pub fn populate_from_summary(&self, cx, summary, source, poster,
  media_cache)` is the entry point called by
  `populate_video_message_content`. It runs the two-layer cache
  lookups described above, updates the inner `RobrixVideo`, and
  returns whether everything that can be drawn has been drawn.

### `maximise_button` always-visible invariant

- The `maximise_button` MUST be present in the `VideoMessagePlayer`
  live_design tree at the slot path
  `surface.controls.maximise_button` (a top-left-anchored `Button`
  inside the controls layer of the overlay). A `VideoMessagePlayer`
  instance whose live_design tree omits this slot does not satisfy
  the spec.
- The `maximise_button` MUST be visible at all times once the
  composer has rendered at least once — including:
  - Before Layer A (poster) has loaded, while only the solid-color
    fallback is on screen.
  - Before Layer B (playable file) has loaded, when the play button
    is disabled.
  - When `should_show_unplayable_overlay(&summary)` returns `true`
    and the play / mute / slider controls are disabled. In the
    unplayable state, the user can still maximise the poster /
    placeholder for inspection — so `maximise_button` is the ONLY
    control that remains enabled.
  - While the modal is open (the inline composer is hidden by the
    modal, but the maximise_button visibility flag on the inline
    composer itself remains `true`).
- `populate_from_summary` MUST NOT call `set_visible(cx, false)` on
  the `maximise_button` for any reason. The composer's draw cycle
  must not toggle the button's visibility.
- The `maximise_button.clicked(actions)` handler emits exactly one
  `VideoMessagePlayerModalAction::Open { source_url, blurhash,
  summary }` action via `Cx::post_action` per click. The payload
  carries the same `source_url` (a `PathBuf`) that the inline
  `RobrixVideo` is bound to, the same `Option<String>` blurhash, and
  a clone of the `VideoSummary`.

### Play-button toggle (three-branch dispatch, mirrors makepad media_player)

- The inline `play_button.clicked(actions)` handler dispatches on the
  inner Makepad `Video` widget's current state via THREE branches —
  matching `examples/media_player/src/lib.rs:303-323`. Two-branch
  dispatch (only `is_playing? pause : begin_playback`) is forbidden
  because `begin_playback` is a no-op on a session that is already
  prepared-and-paused; the user-visible symptom is "video_message_player
  does not play video when play button is clicked" after the first
  pause.
- The three branches, in order:
  1. `if video.is_playing(cx)` → call `video.pause_playback(cx)` and
     set `player_state.playing = false`.
  2. `else if video.is_paused(cx)` → call `video.resume_playback(cx)`
     and set `player_state.playing = true`. This is the branch the
     current implementation is missing; without it, every click after
     the first pause hits the `else` branch and calls
     `begin_playback` on a prepared session, which the platform
     `Video` widget ignores.
  3. `else` → call `video.begin_playback(cx)` and set
     `player_state.playing = true`. This branch fires on the very
     first click (when the session has not yet been prepared) and
     after any `stop_and_cleanup_resources` (when the session was
     released by the maximise / close swap).
- All three branches end by calling `set_active_video(self.widget_uid())`
  ONLY when the new state is "playing" (branches 2 and 3), so the
  cross-message `VideoPlaybackAction::ActiveTrackChanged` broadcast
  fires exactly when the active track changes — never on a pause.
- The single helper `fn toggle_playback(&mut self, cx: &mut Cx)`
  in `src/shared/video_message_player.rs` is the only place the
  three-branch logic lives. Both `play_button.clicked` and
  `pause_button.clicked` route through `toggle_playback` so the same
  branch table is exercised by either button.
- `RobrixVideo` must expose `pub fn is_paused(&self, cx: &Cx) -> bool`
  alongside the existing `is_playing(&self, cx: &Cx) -> bool` so the
  composer can read both states without reaching into the inner
  Makepad `Video` widget. Both methods delegate to the inner widget's
  identically-named accessors (`makepad_video::Video::is_paused`,
  `makepad_video::Video::is_playing`).

### Maximise modal (fullscreen window, mirrors makepad media_player)

- New widget `VideoMessagePlayerModal` at
  `src/shared/video_message_player_modal.rs`. It composes a SECOND
  `RobrixVideo` instance plus the modal-only controls. It does not
  render the scrim — that is provided by the outer Makepad `Modal`.
- The outer wrapper lives in `RoomScreen`'s live_design block:
  `video_message_player_modal := Modal { content +: {
  video_message_player_modal_inner := VideoMessagePlayerModal {} } }`.
  A single modal instance serves every video message in the room.
- The inner card uses `width: Fill, height: Fill` so it stretches to
  the full size of the fullscreened window. The modal's `RobrixVideo`
  also sizes `width: Fill, height: Fill` (the inner Makepad `Video`
  widget preserves aspect via letterboxing).
- `RoomScreen` carries three deferred-state fields (mirroring
  `examples/media_player/src/lib.rs:186-190`):
  - `pending_fullscreen: Option<NextFrame>`
  - `pending_normalize: Option<NextFrame>`
  - `pending_modal_seek_ms: Option<u64>`
- Action protocol:
  - `VideoMessagePlayerModalAction::Open { source_url, blurhash,
    summary }` — emitted by the inline player's `maximise_button`
    handler. `RoomScreen` runs the open sequence below.
  - `VideoMessagePlayerModalAction::Close` — emitted by the inner
    widget when its `close_button` is clicked. `RoomScreen` routes
    through `close_video_modal`.
- **Open sequence** (mirrors
  `examples/media_player/src/lib.rs:325-351`). On
  `VideoMessagePlayerModalAction::Open`, `RoomScreen` runs, in order:
  1. `let main_pos_ms = inline_player.robrix_video().current_position_ms();`
  2. `self.pending_modal_seek_ms = Some(main_pos_ms);`
  3. `inline_player.robrix_video().stop_and_cleanup_resources(cx)`.
  4. `inline_player.set_play_button_text(cx, "Play")`.
  5. `modal_inner.show(cx, source_url, blurhash, summary)`.
  6. `outer_modal.open(cx)`.
  7. `modal_inner.robrix_video().begin_playback(cx)`.
  8. `modal_inner.set_play_button_text(cx, "Pause")`.
  9. `self.pending_fullscreen = Some(cx.new_next_frame())`.
- **PlaybackPrepared seek** (mirrors
  `examples/media_player/src/lib.rs:352-363`). In `handle_actions`,
  `RoomScreen` watches for `VideoAction::PlaybackPrepared` from the
  modal's `RobrixVideo`. When that action arrives and
  `self.pending_modal_seek_ms.take()` returns `Some(ms)`, the modal
  calls `seek_to(cx, ms)`.
- **NextFrame application** (mirrors
  `examples/media_player/src/lib.rs:269-284`). In
  `MatchEvent::handle_next_frame`, `RoomScreen`:
  - If `pending_fullscreen` fires for the current frame, calls
    `self.window_ref(cx).fullscreen(cx)` and clears the field.
  - If `pending_normalize` fires for the current frame, calls
    `self.window_ref(cx).disable_fullscreen(cx)` and clears the field.
- **`close_video_modal` helper** (mirrors
  `examples/media_player/src/lib.rs:210-222`). A single method on
  `RoomScreen` performs, in order:
  1. `modal_inner.robrix_video().stop_and_cleanup_resources(cx)`.
  2. `inline_player.robrix_video().begin_playback(cx)`.
  3. `inline_player.set_play_button_text(cx, "Pause")`.
  4. `self.pending_normalize = Some(cx.new_next_frame())`.
  5. `outer_modal.close(cx)` (idempotent on already-closed `Modal`).
- **Close entry points**, all routed through `close_video_modal`:
  1. `VideoMessagePlayerModalAction::Close` from the inner widget's
     close-button click.
  2. `outer_modal.dismissed(actions)` returning `true` — Makepad's
     built-in scrim / back-press signal.
  3. `MatchEvent::handle_key_down` matching
     `e.key_code == KeyCode::Escape && outer_modal.is_open()`
     (mirrors `examples/media_player/src/lib.rs:393-400`).
  4. Any future programmatic close.
- **Position-preservation asymmetry** (matches reference):
  - inline → modal: PRESERVED via `pending_modal_seek_ms` +
    `PlaybackPrepared` seek.
  - modal → inline: NOT PRESERVED. The inline `begin_playback` call
    in `close_video_modal` restarts from `position_ms = 0`.

### Cargo

- No new dependency. The `blurhash` crate is already pinned for the
  image populator at `src/home/room_screen.rs:4531`.
- No new media-decoder dependency. Frame decode goes through
  Makepad's existing platform `Video` widget.

## Constraints

- Must NOT auto-play on render. The widget shows the blurhash
  placeholder (or `#222` fallback) and the play button is disabled
  until Layer B reports `Loaded`.
- Must render a `maximise_button` at the live_design slot path
  `surface.controls.maximise_button`, anchored top-left at 8 px from
  the top and 8 px from the left of the surface, on every
  `VideoMessagePlayer` instance. A composer instance whose draw tree
  has no `maximise_button` at this path does not satisfy the spec.
- Must keep `maximise_button.visible == true` at all times after the
  first draw, regardless of poster-load state, playable-file load
  state, unplayable-mime state, or modal-open state. `set_visible(cx,
  false)` on the maximise_button is forbidden.
- Must emit exactly one `VideoMessagePlayerModalAction::Open` per
  `maximise_button.clicked(actions)` call. The action payload must
  carry the same `source_url`, `blurhash`, and `summary` that the
  inline `RobrixVideo` is bound to.
- Must dispatch the play-button click through the three-branch
  toggle: `is_playing → pause_playback`, `is_paused →
  resume_playback`, else `begin_playback`. A two-branch toggle that
  omits the `is_paused → resume_playback` arm is forbidden — it
  produces the observed bug where the play button stops working after
  the first pause because `begin_playback` is a no-op on a prepared
  session. Matches `examples/media_player/src/lib.rs:303-323`.
- Must call `set_active_video(self.widget_uid())` only on the two
  branches that transition into "playing" (resume and begin), never
  on the pause branch. The cross-message
  `VideoPlaybackAction::ActiveTrackChanged` broadcast fires only when
  the active track actually changes.
- Must NOT block the UI thread on blurhash decode. The decode runs
  via `Cx::spawn_thread(...)` and uploads to a `Texture`
  asynchronously, matching the image populator's behavior.
- Must NOT decode a blurhash larger than `BLURHASH_IMAGE_MAX_SIZE`
  pixels in either dimension. The `cap_blurhash_dimensions` helper is
  the single enforcement point.
- Must NOT call `blurhash::decode` when any of
  `info.blurhash.is_some()`, `info.width.is_some()`,
  `info.height.is_some()` is false — match the gating condition at
  `src/home/room_screen.rs:4508` exactly.
- Must NOT call `.unwrap()` on `info.width` / `info.height`; missing
  dimensions silently skip the blurhash decode and fall through to
  the solid-color fallback.
- Must NOT instantiate a second `Video` widget on maximise. Each
  composer (`VideoMessagePlayer`, `VideoMessagePlayerModal`) owns
  exactly one `RobrixVideo`, which owns exactly one platform video
  session.
- Must NOT play both the inline `RobrixVideo` and the modal
  `RobrixVideo` of the SAME video message simultaneously. The
  open/close sequences enforce this by calling
  `stop_and_cleanup_resources` on the outgoing widget before
  `begin_playback` on the incoming one.
- Must NOT play two videos from DIFFERENT messages simultaneously.
  The cross-widget `VideoPlaybackAction::ActiveTrackChanged`
  broadcast pauses every other inline `VideoMessagePlayer` via
  `RobrixVideo::pause_playback` (NOT `stop_and_cleanup_resources`).
- Must preserve `position_ms` on maximise (inline → modal) via the
  `pending_modal_seek_ms` + `PlaybackPrepared` seek path.
- Must NOT preserve `position_ms` on close (modal → inline). The
  inline `begin_playback` call in `close_video_modal` restarts from
  `position_ms = 0`.
- Must defer the OS fullscreen / exit-fullscreen calls by exactly
  one frame via `pending_fullscreen` / `pending_normalize`
  `Option<NextFrame>` fields, applied inside `handle_next_frame`. A
  direct, non-deferred call to `Window::fullscreen(cx)` from
  `handle_actions` is forbidden because it races the modal's first
  layout pass.
- Must route every modal close path through the single
  `close_video_modal(&mut self, cx: &mut Cx)` helper on `RoomScreen`.
  No close-step (modal stop, inline begin_playback,
  pending_normalize, outer_modal.close) is allowed outside this
  helper.
- Must wire the Escape key via `MatchEvent::handle_key_down` with the
  guard `e.key_code == KeyCode::Escape && outer_modal.is_open()`. A
  bare `KeyCode::Escape` handler without the `is_open()` guard is
  forbidden.
- Must NOT couple `RobrixVideo` to any Matrix type. The widget takes
  only `PathBuf` and `Option<String>`.
- Must NOT change the signature of `summarize_video_message` or
  `video_summary_html` in `src/event_preview.rs`.

## Boundaries

### Allowed Changes

- src/home/room_screen.rs
- src/event_preview.rs
- src/media_cache.rs
- src/shared/text_or_image.rs
- src/shared/robrix_video.rs (new)
- src/shared/video_message_player.rs
- src/shared/video_message_player_modal.rs
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
- Do not redefine `BLURHASH_IMAGE_MAX_SIZE`. Reuse the constant at
  `src/home/room_screen.rs:56`.
- Do not add a media-decoder dependency (no ffmpeg, no gstreamer).
- Do not couple `RobrixVideo` to Matrix types. The standalone widget
  takes only `PathBuf` and `Option<String>`; Matrix-aware resolution
  lives in `VideoMessagePlayer`.
- Do not introduce a second active-track controller. The video
  controller mirrors the audio controller's shape (one mutex-guarded
  active track, one broadcast action).
- Do not auto-pause audio when video plays, and do not auto-pause
  video when audio plays.
- Do not share `Arc<Mutex<VideoPlayerState>>` between the inline
  composer and the modal composer. Each composer owns its state by
  value; the maximise / close handlers drive the platform sessions
  directly.
- Do not bypass `close_video_modal` for any close path.
- Do not use `cargo fmt`. Verification is `cargo test -p robrix` only;
  no formatter pass is permitted to touch the working tree.

## Completion Criteria

Scenario: cap_blurhash_dimensions returns input unchanged when both axes are within the cap
  Test:
    Package: robrix
    Filter: test_cap_blurhash_dimensions_no_op_when_below_cap
  Given `width = 320` and `height = 240`
  When `cap_blurhash_dimensions(width, height, 500)` is called
  Then the result equals `(320, 240)`

Scenario: cap_blurhash_dimensions caps height and scales width by aspect ratio
  Test:
    Package: robrix
    Filter: test_cap_blurhash_dimensions_caps_height
  Given `width = 1920` and `height = 1080` and `max = 500`
  When `cap_blurhash_dimensions(width, height, max)` is called
  Then the returned tuple equals `(888, 500)`

Scenario: cap_blurhash_dimensions caps width and scales height by aspect ratio
  Test:
    Package: robrix
    Filter: test_cap_blurhash_dimensions_caps_width
  Given `width = 2000` and `height = 800` and `max = 500`
  When `cap_blurhash_dimensions(width, height, max)` is called
  Then the returned tuple equals `(500, 200)`

Scenario: cap_blurhash_dimensions returns zero for zero input
  Test:
    Package: robrix
    Filter: test_cap_blurhash_dimensions_zero_returns_zero
  Given `width = 0` and `height = 480`
  When `cap_blurhash_dimensions(width, height, 500)` is called
  Then the returned tuple equals `(0, 0)`

Scenario: decode_blurhash_to_rgba produces a width*height*4 buffer for a valid blurhash
  Test:
    Package: robrix
    Filter: test_decode_blurhash_to_rgba_valid
  Given the blurhash string `"LEHV6nWB2yk8pyo0adR*.7kCMdnj"`
  When `decode_blurhash_to_rgba(blurhash, 32, 18)` is called
  Then the returned `Option<Vec<u8>>` is `Some(buf)`
  And `buf.len()` equals `32 * 18 * 4`

Scenario: decode_blurhash_to_rgba returns None for an empty string
  Test:
    Package: robrix
    Filter: test_decode_blurhash_to_rgba_empty_returns_none
  Given the blurhash string `""`
  When `decode_blurhash_to_rgba(blurhash, 32, 18)` is called
  Then the returned value equals `None`

Scenario: decode_blurhash_to_rgba returns None for a malformed blurhash
  Test:
    Package: robrix
    Filter: test_decode_blurhash_to_rgba_malformed_returns_none
  Given the blurhash string `"not a real blurhash"`
  When `decode_blurhash_to_rgba(blurhash, 32, 18)` is called
  Then the returned value equals `None`

Scenario: decode_blurhash_to_rgba returns None for zero width
  Test:
    Package: robrix
    Filter: test_decode_blurhash_to_rgba_zero_width_returns_none
  Given the blurhash string `"LEHV6nWB2yk8pyo0adR*.7kCMdnj"`
  When `decode_blurhash_to_rgba(blurhash, 0, 18)` is called
  Then the returned value equals `None`

Scenario: placeholder_fallback_color is the documented dark gray
  Test:
    Package: robrix
    Filter: test_placeholder_fallback_color_value
  When `placeholder_fallback_color()` is called
  Then the returned `[u8; 4]` equals `[0x22, 0x22, 0x22, 0xFF]`

Scenario: should_show_blurhash returns true before the first frame
  Test:
    Package: robrix
    Filter: test_should_show_blurhash_before_first_frame
  Given each of `BlurhashState::NoSource`, `BlurhashState::NotYetStarted`, and `BlurhashState::AwaitingFirstFrame`
  When `should_show_blurhash(state)` is called for each
  Then the result equals `true` for all three

Scenario: should_show_blurhash returns false while playing
  Test:
    Package: robrix
    Filter: test_should_show_blurhash_false_while_playing
  Given `BlurhashState::Playing`
  When `should_show_blurhash(state)` is called
  Then the result equals `false`

Scenario: Requested poster with blurhash + width + height decodes the blurhash
  Test:
    Package: robrix
    Filter: test_requested_poster_with_blurhash_decodes
  Given a stub `MediaCache` that returns `(MediaCacheEntry::Requested, MediaFormat::Thumbnail(_))` for the poster
  And a `VideoMessageEventContent` whose `info.blurhash = Some("LEHV6nWB2yk8pyo0adR*.7kCMdnj")`, `info.width = Some(640)`, `info.height = Some(480)`
  When `populate_video_message_content` is called with these inputs
  Then the recording `video_player` test double records exactly one `set_blurhash_texture` call
  And no `set_poster_texture` call is recorded

Scenario: Requested poster with no blurhash sets the solid-color fallback
  Test:
    Package: robrix
    Filter: test_requested_poster_without_blurhash_falls_back_to_solid
  Given a stub `MediaCache` that returns `(MediaCacheEntry::Requested, MediaFormat::Thumbnail(_))` for the poster
  And a `VideoMessageEventContent` whose `info.blurhash = None`
  When `populate_video_message_content` is called
  Then the recording `video_player` records exactly one `set_poster_to_solid_color(0x22, 0x22, 0x22, 0xFF)` call

Scenario: Requested poster with blurhash but missing width skips decode
  Test:
    Package: robrix
    Filter: test_requested_poster_missing_width_skips_decode
  Given a stub `MediaCache` that returns `(MediaCacheEntry::Requested, _)` for the poster
  And a `VideoMessageEventContent` whose `info.blurhash = Some("...")`, `info.width = None`, `info.height = Some(480)`
  When `populate_video_message_content` is called
  Then the recording `video_player` records zero `set_blurhash_texture` calls
  And records exactly one `set_poster_to_solid_color(...)` call

Scenario: Loaded poster decodes the cached PNG and sets the poster texture
  Test:
    Package: robrix
    Filter: test_loaded_poster_sets_poster_texture
  Given a stub `MediaCache` that returns `(MediaCacheEntry::Loaded(bytes), MediaFormat::Thumbnail(_))` for the poster
  When `populate_video_message_content` is called
  Then the recording `video_player` records exactly one `set_poster_texture` call

Scenario: Failed poster falls back to blurhash when one is available
  Test:
    Package: robrix
    Filter: test_failed_poster_falls_back_to_blurhash
  Given a stub `MediaCache` that returns `(MediaCacheEntry::Failed(404), MediaFormat::Thumbnail(_))` for the poster
  And a `VideoMessageEventContent` whose `info.blurhash = Some("L#...")`, `info.width = Some(640)`, `info.height = Some(480)`
  When `populate_video_message_content` is called
  Then the recording `video_player` records exactly one `set_blurhash_texture` call

Scenario: Requested video file disables the play button and keeps Layer A
  Test:
    Package: robrix
    Filter: test_requested_video_file_disables_play
  Given a stub `MediaCache` that returns `(MediaCacheEntry::Requested, MediaFormat::File)` for the video file
  When `populate_video_message_content` is called
  Then the recording `video_player` records `set_play_enabled(false)`
  And does NOT record any `set_source_url` call
  And `populate_video_message_content` returns `false`

Scenario: Loaded video file writes the source path and enables the play button
  Test:
    Package: robrix
    Filter: test_loaded_video_file_enables_play
  Given a stub `MediaCache` that returns `(MediaCacheEntry::Loaded(_), MediaFormat::File)` for the video file
  And the cache reports `path_for(&mxc) == PathBuf::from("/tmp/clip.mp4")`
  When `populate_video_message_content` is called
  Then the recording `video_player` records `set_source_url(PathBuf::from("/tmp/clip.mp4"))`
  And records `set_play_enabled(true)`

Scenario: Failed video file surfaces the error label while keeping Layer A
  Test:
    Package: robrix
    Filter: test_failed_video_file_shows_inline_error
  Given a stub `MediaCache` that returns `(MediaCacheEntry::Failed(500), MediaFormat::File)` for the video file
  When `populate_video_message_content` is called
  Then the recording `video_player` records exactly one `set_inline_error("Failed to fetch video from mxc:... (HTTP 500)")` call
  And does NOT record any `set_source_url` call

Scenario: First play-button click on a fresh widget calls begin_playback
  Test:
    Package: robrix
    Filter: test_play_button_first_click_begins_playback
  Given a recording test double `video: RecordingVideo` reporting `is_playing() == false` and `is_paused() == false`
  When the test invokes `toggle_playback(&mut self, &mut cx)`
  Then `video.calls` equals `vec!["is_playing", "is_paused", "begin_playback"]`
  And `player_state.playing` equals `true`
  And the cross-message `ActiveTrackChanged` broadcast was fired exactly once with `now_playing == self.widget_uid()`

Scenario: Play-button click on a paused session calls resume_playback (not begin_playback)
  Test:
    Package: robrix
    Filter: test_play_button_click_on_paused_resumes
  Given a recording test double `video: RecordingVideo` reporting `is_playing() == false` and `is_paused() == true`
  When the test invokes `toggle_playback(&mut self, &mut cx)`
  Then `video.calls` equals `vec!["is_playing", "is_paused", "resume_playback"]`
  And `video.calls` does NOT contain `"begin_playback"`
  And `player_state.playing` equals `true`
  And the cross-message `ActiveTrackChanged` broadcast was fired exactly once

Scenario: Play-button click on a playing session calls pause_playback
  Test:
    Package: robrix
    Filter: test_play_button_click_on_playing_pauses
  Given a recording test double `video: RecordingVideo` reporting `is_playing() == true`
  When the test invokes `toggle_playback(&mut self, &mut cx)`
  Then `video.calls` equals `vec!["is_playing", "pause_playback"]`
  And `video.calls` does NOT contain `"resume_playback"`
  And `video.calls` does NOT contain `"begin_playback"`
  And `player_state.playing` equals `false`

Scenario: Pause-then-play round trip uses begin then resume (not begin twice)
  Test:
    Package: robrix
    Filter: test_play_button_round_trip_uses_resume_on_second_play
  Given a recording test double `video: RecordingVideo` starting in `is_playing() == false`, `is_paused() == false`
  When the test invokes `toggle_playback` (first click — begins)
  And the recording double transitions to `is_playing() == true`, `is_paused() == false`
  And the test invokes `toggle_playback` (second click — pauses)
  And the recording double transitions to `is_playing() == false`, `is_paused() == true`
  And the test invokes `toggle_playback` (third click — must resume)
  Then `video.calls` (filtered to playback verbs only) equals `vec!["begin_playback", "pause_playback", "resume_playback"]`
  And `video.calls` does NOT contain a second `"begin_playback"` entry

Scenario: Pause branch does NOT fire ActiveTrackChanged
  Test:
    Package: robrix
    Filter: test_pause_branch_does_not_broadcast_active_track
  Given a recording test double `video: RecordingVideo` reporting `is_playing() == true`
  And a recording broadcast double `broadcaster: RecordingBroadcast`
  When the test invokes `toggle_playback(&mut self, &mut cx)`
  Then `broadcaster.calls` is empty

Scenario: pause_button.clicked routes through the same three-branch toggle
  Test:
    Package: robrix
    Filter: test_pause_button_routes_through_toggle_playback
  Given a recording test double `video: RecordingVideo` reporting `is_playing() == true`
  And the test wires the `pause_button.clicked(actions)` action into the handler
  When the action fires
  Then `toggle_playback_invocations` equals `1`
  And `video.calls` equals `vec!["is_playing", "pause_playback"]`

Scenario: Begin branch after stop_and_cleanup_resources (post-close swap) restarts cleanly
  Test:
    Package: robrix
    Filter: test_play_button_after_stop_calls_begin
  Given a recording test double `video: RecordingVideo` whose previous state was Playing and which was just sent `stop_and_cleanup_resources` (so is_playing == false, is_paused == false)
  When the test invokes `toggle_playback(&mut self, &mut cx)`
  Then `video.calls` ends with `"begin_playback"` (not `"resume_playback"`)
  And `player_state.playing` equals `true`

Scenario: VideoMessagePlayer live_design tree contains a maximise_button at the expected slot
  Test:
    Package: robrix
    Filter: test_video_message_player_has_maximise_button_slot
  Given a freshly-constructed `VideoMessagePlayer` widget
  When the test resolves `widget.button_ref(ids!(surface.controls.maximise_button))`
  Then the returned `ButtonRef` is NOT empty (`button_ref.borrow().is_some()` equals `true`)

Scenario: maximise_button is visible before any media has loaded
  Test:
    Package: robrix
    Filter: test_maximise_button_visible_before_load
  Given a freshly-constructed `VideoMessagePlayer` widget that has been drawn once
  And neither Layer A (poster) nor Layer B (playable file) has reported Loaded
  When the test reads `widget.button_ref(ids!(surface.controls.maximise_button)).visible()`
  Then the returned value equals `true`

Scenario: maximise_button is visible while the play button is disabled
  Test:
    Package: robrix
    Filter: test_maximise_button_visible_when_play_disabled
  Given a `VideoMessagePlayer` whose Layer B reported `set_play_enabled(false)`
  When the test reads `widget.button_ref(ids!(surface.controls.maximise_button)).visible()`
  Then the returned value equals `true`
  And `widget.button_ref(ids!(surface.controls.play_button)).enabled()` equals `false`

Scenario: maximise_button is visible when summary mime is unplayable
  Test:
    Package: robrix
    Filter: test_maximise_button_visible_when_unplayable
  Given a `VideoMessagePlayer` populated from a `VideoSummary` whose `mime = Some("video/x-matroska")`
  When the test reads `widget.button_ref(ids!(surface.controls.maximise_button)).visible()`
  Then the returned value equals `true`
  And `widget.button_ref(ids!(surface.controls.play_button)).enabled()` equals `false`
  And `widget.button_ref(ids!(surface.controls.mute_button)).enabled()` equals `false`

Scenario: populate_from_summary never hides the maximise_button
  Test:
    Package: robrix
    Filter: test_populate_from_summary_does_not_hide_maximise
  Given a recording `VideoMessagePlayer` test double that records every `set_visible` call
  When the test invokes `populate_from_summary(cx, summary, source, poster, media_cache)` with arbitrary inputs
  Then the recording does NOT contain any `set_visible(maximise_button, false)` entry

Scenario: maximise_button is anchored top-left at 8 px from top and 8 px from left
  Test:
    Package: robrix
    Filter: test_maximise_button_anchor_is_top_left
  Given a `VideoMessagePlayer` drawn into a surface of size `400 x 300`
  When the test reads `widget.button_ref(ids!(surface.controls.maximise_button)).area().rect(cx)`
  Then `rect.pos.x` equals `8.0`
  And `rect.pos.y` equals `8.0`

Scenario: Clicking maximise_button emits VideoMessagePlayerModalAction::Open
  Test:
    Package: robrix
    Filter: test_maximise_button_click_emits_open_action
  Given a `VideoMessagePlayer` populated with `source_url = PathBuf::from("/tmp/clip.mp4")`, `blurhash = Some("L#...")`, and a `VideoSummary { filename: "clip.mp4", ... }`
  And a captured `Cx` action queue
  When the test fires the `maximise_button.clicked` action through the handler
  Then exactly one `VideoMessagePlayerModalAction::Open { source_url, blurhash, summary }` action is posted
  And `source_url` equals `PathBuf::from("/tmp/clip.mp4")`
  And `blurhash` equals `Some("L#...".to_string())`
  And `summary.filename` equals `"clip.mp4"`

Scenario: Repeated maximise clicks emit one Open action per click
  Test:
    Package: robrix
    Filter: test_maximise_button_click_emits_one_open_per_click
  Given a `VideoMessagePlayer` and a captured `Cx` action queue
  When the test fires `maximise_button.clicked` three times in sequence
  Then exactly three `VideoMessagePlayerModalAction::Open` actions are posted

Scenario: Maximise captures inline position BEFORE stopping inline
  Test:
    Package: robrix
    Filter: test_maximise_captures_inline_position_before_stop
  Given a recording test double `inline: RecordingVideo` with `current_position_ms = 3_500` and currently playing
  And `let mut pending_modal_seek_ms: Option<u64> = None;`
  When the test invokes `open_modal_sequence(&mut inline, &mut modal, &mut pending_modal_seek_ms)`
  Then `pending_modal_seek_ms` equals `Some(3_500)`
  And the recorded call order on `inline.calls` shows `"current_position_ms"` appearing BEFORE `"stop_and_cleanup_resources"`

Scenario: Modal seeks to pending_modal_seek_ms on VideoAction::PlaybackPrepared
  Test:
    Package: robrix
    Filter: test_modal_seeks_on_playback_prepared
  Given `let mut pending_modal_seek_ms: Option<u64> = Some(3_500);`
  And a recording test double `modal: RecordingVideo`
  When the test invokes `handle_modal_playback_prepared(&mut modal, &mut pending_modal_seek_ms)`
  Then `modal.calls` equals `vec!["seek_to(3500)"]`
  And `pending_modal_seek_ms` equals `None`

Scenario: PlaybackPrepared with no pending seek is a no-op
  Test:
    Package: robrix
    Filter: test_playback_prepared_without_pending_seek_is_noop
  Given `let mut pending_modal_seek_ms: Option<u64> = None;`
  And a recording test double `modal: RecordingVideo`
  When the test invokes `handle_modal_playback_prepared(&mut modal, &mut pending_modal_seek_ms)`
  Then `modal.calls` is empty

Scenario: Close does NOT preserve position - inline restarts from zero
  Test:
    Package: robrix
    Filter: test_close_does_not_preserve_position
  Given a recording test double `inline: RecordingVideo` and `modal: RecordingVideo` where `modal.current_position_ms = 2_000`
  When the test invokes `close_video_modal_sequence(&mut inline, &mut modal)`
  Then `modal.calls` equals `vec!["stop_and_cleanup_resources"]`
  And `inline.calls` equals `vec!["begin_playback"]`
  And the recording does NOT include any `"current_position_ms"` call on `modal`

Scenario: Open sequence sets pending_fullscreen to a NextFrame token
  Test:
    Package: robrix
    Filter: test_open_sequence_sets_pending_fullscreen
  Given `let mut pending_fullscreen: Option<NextFrameToken> = None;`
  When the test invokes `open_modal_sequence(...)` with a stub `cx` that issues `NextFrameToken(42)`
  Then `pending_fullscreen` equals `Some(NextFrameToken(42))`

Scenario: handle_next_frame applies pending_fullscreen exactly once
  Test:
    Package: robrix
    Filter: test_handle_next_frame_applies_fullscreen_once
  Given a recording test double `window: RecordingWindow`
  And `let mut pending_fullscreen: Option<NextFrameToken> = Some(NextFrameToken(42));`
  And a `NextFrameEvent { set: HashSet::from([NextFrameToken(42)]) }`
  When the test invokes `apply_pending_fullscreen(&event, &mut window, &mut pending_fullscreen)` twice
  Then `window.calls` equals `vec!["fullscreen"]`
  And `pending_fullscreen` equals `None` after the first call

Scenario: handle_next_frame ignores pending_fullscreen until its token fires
  Test:
    Package: robrix
    Filter: test_handle_next_frame_waits_for_matching_token
  Given a recording test double `window: RecordingWindow`
  And `let mut pending_fullscreen: Option<NextFrameToken> = Some(NextFrameToken(42));`
  And a `NextFrameEvent { set: HashSet::from([NextFrameToken(7)]) }`
  When the test invokes `apply_pending_fullscreen(&event, &mut window, &mut pending_fullscreen)`
  Then `window.calls` is empty
  And `pending_fullscreen` equals `Some(NextFrameToken(42))`

Scenario: close_video_modal sets pending_normalize for deferred exit-fullscreen
  Test:
    Package: robrix
    Filter: test_close_video_modal_sets_pending_normalize
  Given `let mut pending_normalize: Option<NextFrameToken> = None;`
  And recording doubles `inline: RecordingVideo`, `modal: RecordingVideo`, `outer: RecordingModal`
  When the test invokes `close_video_modal(...)` with a stub `cx` that issues `NextFrameToken(99)`
  Then `pending_normalize` equals `Some(NextFrameToken(99))`

Scenario: handle_next_frame applies pending_normalize by calling disable_fullscreen
  Test:
    Package: robrix
    Filter: test_handle_next_frame_applies_disable_fullscreen
  Given a recording test double `window: RecordingWindow`
  And `let mut pending_normalize: Option<NextFrameToken> = Some(NextFrameToken(99));`
  And a `NextFrameEvent { set: HashSet::from([NextFrameToken(99)]) }`
  When the test invokes `apply_pending_normalize(&event, &mut window, &mut pending_normalize)`
  Then `window.calls` equals `vec!["disable_fullscreen"]`
  And `pending_normalize` equals `None`

Scenario: Escape key fires close_video_modal when outer modal is open
  Test:
    Package: robrix
    Filter: test_escape_calls_close_when_modal_open
  Given a recording outer modal double `outer: RecordingModal` reporting `is_open() == true`
  And recording doubles `inline: RecordingVideo`, `modal: RecordingVideo`
  And a `KeyEvent { key_code: KeyCode::Escape }`
  When the test invokes `handle_key_down_for_modal(&event, &mut self_state)`
  Then `modal.calls` equals `vec!["stop_and_cleanup_resources"]`
  And `inline.calls` equals `vec!["begin_playback"]`
  And `outer.calls` contains `"close"`

Scenario: Escape key is ignored when the outer modal is closed
  Test:
    Package: robrix
    Filter: test_escape_ignored_when_modal_closed
  Given a recording outer modal double `outer: RecordingModal` reporting `is_open() == false`
  And recording doubles `inline: RecordingVideo`, `modal: RecordingVideo`
  And a `KeyEvent { key_code: KeyCode::Escape }`
  When the test invokes `handle_key_down_for_modal(&event, &mut self_state)`
  Then `modal.calls` is empty
  And `inline.calls` is empty
  And `outer.calls` does NOT contain `"close"`

Scenario: Non-Escape key events do NOT trigger close_video_modal
  Test:
    Package: robrix
    Filter: test_non_escape_key_does_not_close_modal
  Given a recording outer modal double `outer: RecordingModal` reporting `is_open() == true`
  And recording doubles `inline: RecordingVideo`, `modal: RecordingVideo`
  And a `KeyEvent { key_code: KeyCode::Space }`
  When the test invokes `handle_key_down_for_modal(&event, &mut self_state)`
  Then `modal.calls` is empty
  And `inline.calls` is empty
  And `outer.calls` does NOT contain `"close"`

Scenario: close_button, scrim dismiss, and Escape all route through close_video_modal
  Test:
    Package: robrix
    Filter: test_all_close_paths_route_through_helper
  Given a counter `close_helper_invocations: usize = 0;`
  When the test fires `VideoMessagePlayerModalAction::Close` and increments on each `close_video_modal` entry
  And fires `outer_modal.dismissed(...)` returning `true`
  And fires `KeyEvent { key_code: KeyCode::Escape }` with `outer.is_open() == true`
  Then `close_helper_invocations` equals `3`

Scenario: Cross-message active-track broadcast pauses (does not stop) other inline players
  Test:
    Package: robrix
    Filter: test_active_track_changed_pauses_others
  Given two `RecordingVideo` doubles `a` and `b`, both currently playing
  When the test invokes `handle_active_track_changed(now_playing_uid = b.uid, &mut [a, b])`
  Then `a.calls` equals `vec!["pause_playback"]`
  And `a.calls` does NOT contain `"stop_and_cleanup_resources"`
  And `b.calls` is empty

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
  thumbnail and file fetch paths.
- Encrypted video sources (`MediaSource::Encrypted`). The function
  shows a `[TODO] fetch encrypted video at ...` placeholder, matching
  the existing image handling at `src/home/room_screen.rs:4569-4575`.
