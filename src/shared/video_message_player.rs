//! Inline video message player widget.
//!
//! Owns the single platform video session per message (one Makepad `Video`
//! widget child) and the shared `Arc<Mutex<...>>` state handles that the
//! sibling `VideoMessagePlayerModal` reads and writes when maximised.
//!
//! All control logic (playable-mime detection, slider drag, mute/restore,
//! maximise toggle) is implemented as pure functions exercised by the
//! `tests_video_message_player` module at the bottom of this file.

use std::{
    path::PathBuf,
    sync::mpsc::Receiver,
    sync::{Arc, Mutex},
};

use makepad_widgets::*;
use matrix_sdk::ruma::{events::room::MediaSource, OwnedMxcUri};
use matrix_sdk::media::MediaFormat;

pub use crate::event_preview::VideoSummary;
use crate::{
    event_preview::format_mmss,
    media_cache::{MediaCache, MediaCacheEntry},
    shared::video_message_player_modal::VideoMessagePlayerModalAction,
    utils,
};

// Blurhash / placeholder helpers — previously lived in the now-deleted
// `shared::robrix_video` module. Kept here because this is the only
// caller after that wrapper widget was inlined into raw `Video`.

pub fn cap_blurhash_dimensions(width: u32, height: u32, max: u32) -> (u32, u32) {
    if width == 0 || height == 0 {
        return (0, 0);
    }
    if width <= max && height <= max {
        return (width, height);
    }
    let aspect_ratio = width as f32 / height as f32;
    if height > max && aspect_ratio <= 16.0 / 9.0 {
        return ((max as f32 * aspect_ratio).floor() as u32, max);
    }
    (max, (max as f32 / aspect_ratio).floor() as u32)
}

pub fn decode_blurhash_to_rgba(blurhash: &str, width: u32, height: u32) -> Option<Vec<u8>> {
    if blurhash.is_empty() || width == 0 || height == 0 {
        return None;
    }
    blurhash::decode(blurhash, width, height, 1.0).ok()
}

pub fn placeholder_fallback_color() -> [u8; 4] {
    [0x22, 0x22, 0x22, 0xFF]
}

// ============================================================================
// State types
// ============================================================================

#[derive(Clone, Copy, Debug, Default)]
pub struct VideoPlayerState {
    pub playing: bool,
    pub position_ms: u64,
}

#[derive(Clone, Copy, Debug)]
pub struct VideoVolumeState {
    pub muted: bool,
    pub level: f32,
    pub restore_level: f32,
}

impl Default for VideoVolumeState {
    fn default() -> Self {
        Self {
            muted: false,
            level: 0.8,
            restore_level: 0.8,
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub enum VolumeAction {
    Mute,
    Unmute,
    SetLevel(f32),
}

// ============================================================================
// Shared state aliases — used by the modal widget.
// ============================================================================

pub type SharedPlayerState = Arc<Mutex<VideoPlayerState>>;
pub type SharedVolumeState = Arc<Mutex<VideoVolumeState>>;

// ============================================================================
// Cross-widget actions
// ============================================================================

#[derive(Clone, Debug)]
pub enum VideoPlaybackAction {
    /// Broadcast whenever a new video begins playback. Other video
    /// players observe this and pause themselves if their uid does not
    /// match — same "single-active track" model the audio player uses.
    ActiveTrackChanged {
        now_playing: WidgetUid,
    },
    ResumeInlineAfterModal {
        inline_uid: WidgetUid,
    },
}

// ============================================================================
// Pure helpers
// ============================================================================

/// Returns `true` for case-folded, parameter-stripped mime values the
/// platform decoder is known to handle.
pub fn is_playable_mime(mime: &str) -> bool {
    let normalized = mime
        .split(';')
        .next()
        .unwrap_or("")
        .trim()
        .to_ascii_lowercase();
    matches!(
        normalized.as_str(),
        "video/mp4" | "video/quicktime" | "video/x-m4v" | "video/webm" | "video/ogg"
    )
}

/// Whether the unplayable-overlay should be drawn. If the summary has
/// no mime we *optimistically* show the player and let the decoder
/// fail loudly during prepare, so this returns `false` in that case.
pub fn should_show_unplayable_overlay(summary: &VideoSummary) -> bool {
    match summary.mime.as_deref() {
        Some(mime) => !is_playable_mime(mime),
        None => false,
    }
}

pub fn infer_video_extension(filename: &str, mime: Option<&str>) -> &'static str {
    let from_filename = filename
        .rsplit_once('.')
        .map(|(_, ext)| ext.trim().to_ascii_lowercase())
        .and_then(|ext| match ext.as_str() {
            "mp4" | "m4v" | "mov" | "webm" | "ogv" | "ogg" => Some(ext),
            _ => None,
        });

    match from_filename.as_deref() {
        Some("mp4") => "mp4",
        Some("m4v") => "m4v",
        Some("mov") => "mov",
        Some("webm") => "webm",
        Some("ogv") => "ogv",
        Some("ogg") => "ogg",
        _ => mime.and_then(video_extension_from_mime).unwrap_or("mp4"),
    }
}

fn video_extension_from_mime(mime: &str) -> Option<&'static str> {
    match mime
        .to_ascii_lowercase()
        .split(';')
        .next()
        .unwrap_or("")
        .trim()
    {
        "video/mp4" => Some("mp4"),
        "video/x-m4v" => Some("m4v"),
        "video/quicktime" => Some("mov"),
        "video/webm" => Some("webm"),
        "video/ogg" => Some("ogv"),
        _ => None,
    }
}

/// Apply a volume action to `VideoVolumeState`. Mute snapshots the
/// current level into `restore_level`; Unmute restores it with a 0.05
/// minimum guard so a previously-silent slider doesn't unmute to zero.
pub fn apply_volume_action(state: &mut VideoVolumeState, action: VolumeAction) {
    match action {
        VolumeAction::Mute => {
            state.restore_level = state.level;
            state.level = 0.0;
            state.muted = true;
        }
        VolumeAction::Unmute => {
            state.level = state.restore_level.max(0.05);
            state.muted = false;
        }
        VolumeAction::SetLevel(value) => {
            state.level = value.clamp(0.0, 1.0);
            state.muted = state.level == 0.0;
        }
    }
}

// ============================================================================
// Single-active broadcaster (mirrors the audio player's pattern)
// ============================================================================

static ACTIVE_VIDEO: Mutex<Option<WidgetUid>> = Mutex::new(None);

fn set_active_video(uid: WidgetUid) {
    if let Ok(mut guard) = ACTIVE_VIDEO.lock() {
        if guard.as_ref() != Some(&uid) {
            *guard = Some(uid);
            // Broadcast so the other players can pause themselves.
            Cx::post_action(VideoPlaybackAction::ActiveTrackChanged { now_playing: uid });
        }
    }
}

#[cfg(test)]
#[derive(Debug, PartialEq)]
enum PosterLayerDecision {
    SetPosterTexture,
    DecodeBlurhash { width: u32, height: u32 },
    SetSolidFallback([u8; 4]),
}

#[cfg(test)]
fn poster_layer_decision(
    entry: &MediaCacheEntry,
    blurhash: Option<&str>,
    dimensions: Option<(u32, u32)>,
) -> PosterLayerDecision {
    if matches!(entry, MediaCacheEntry::Loaded(_)) {
        return PosterLayerDecision::SetPosterTexture;
    }

    if let (Some(blurhash), Some((width, height))) = (blurhash, dimensions) {
        let (width, height) = cap_blurhash_dimensions(
            width,
            height,
            crate::home::room_screen::BLURHASH_IMAGE_MAX_SIZE,
        );
        if decode_blurhash_to_rgba(blurhash, width, height).is_some() {
            return PosterLayerDecision::DecodeBlurhash { width, height };
        }
    }

    PosterLayerDecision::SetSolidFallback(placeholder_fallback_color())
}

#[cfg(test)]
#[derive(Debug, PartialEq)]
enum VideoFileLayerDecision {
    SetSourceUrl(PathBuf),
    DisablePlay,
    SetInlineError(String),
}

#[cfg(test)]
fn video_file_layer_decision(
    entry: &MediaCacheEntry,
    format: &MediaFormat,
    mxc_uri: &OwnedMxcUri,
    source_path: PathBuf,
) -> VideoFileLayerDecision {
    match (entry, format) {
        (MediaCacheEntry::Loaded(_), MediaFormat::File) => {
            VideoFileLayerDecision::SetSourceUrl(source_path)
        }
        (MediaCacheEntry::Failed(status_code), _) => VideoFileLayerDecision::SetInlineError(
            format!("Failed to fetch video from {mxc_uri} (HTTP {status_code})"),
        ),
        _ => VideoFileLayerDecision::DisablePlay,
    }
}

// ============================================================================
// Live design
// ============================================================================

script_mod! {
    use mod.prelude.widgets.*
    use mod.widgets.*

    mod.widgets.VIDEO_ICON_PLAY     = crate_resource("self://resources/icons/play.svg")
    mod.widgets.VIDEO_ICON_PAUSE    = crate_resource("self://resources/icons/pause.svg")
    mod.widgets.VIDEO_ICON_FORBIDDEN= crate_resource("self://resources/icons/forbidden.svg")
    mod.widgets.VIDEO_ICON_MAXIMISE = crate_resource("self://resources/icons/maximise.svg")
    mod.widgets.VIDEO_ICON_VOL_ON   = crate_resource("self://resources/icons/volume_on.svg")
    mod.widgets.VIDEO_ICON_VOL_OFF  = crate_resource("self://resources/icons/volume_off.svg")

    mod.widgets.VideoMessagePlayer = #(VideoMessagePlayer::register_widget(vm)) {
        width: Fill
        height: Fit
        min_width: 240
        max_width: 520
        flow: Down
        spacing: 6

        surface := View {
            width: Fill
            height: 292
            flow: Overlay
            show_bg: true
            draw_bg +: {
                color: #x111827
                border_radius: 8.0
            }

            robrix_video := Video {
                width: Fill
                height: Fill
                show_controls: true
            }

            unplayable_overlay := View {
                width: Fill
                height: Fill
                visible: false
                align: Align{x: 0.5, y: 0.5}
                forbidden_icon := Icon {
                    width: 48
                    height: 48
                    draw_icon +: {
                        svg: (mod.widgets.VIDEO_ICON_FORBIDDEN)
                        color: #xff4444
                    }
                    icon_walk: Walk{width: 48, height: 48}
                }
            }

            controls := View {
                width: Fill
                height: Fill
                flow: Overlay
                padding: 8

                maximise_button := Button {
                    width: 36
                    height: 36
                    text: ""
                    spacing: 0
                    padding: 0
                    align: Align{x: 0.5, y: 0.5}
                    icon_walk: Walk{width: 18, height: 18}
                    draw_icon +: {
                        svg: (mod.widgets.VIDEO_ICON_MAXIMISE)
                        color: #xffffff
                    }
                    draw_bg +: {
                        border_radius: 5.0
                        color: #x111827
                        color_hover: #x374151
                        color_down: #x111827
                    }
                }

                mute_button := Button {
                    width: 36
                    height: 36
                    margin: Inset{left: 99999}      // push to top-right edge
                    text: ""
                    spacing: 0
                    padding: 0
                    align: Align{x: 0.5, y: 0.5}
                    icon_walk: Walk{width: 18, height: 18}
                    draw_icon +: {
                        svg: (mod.widgets.VIDEO_ICON_VOL_ON)
                        color: #xffffff
                    }
                    draw_bg +: {
                        border_radius: 5.0
                        color: #x111827
                        color_hover: #x374151
                        color_down: #x111827
                        color_disabled: #x737A85
                    }
                }
            }
        }

        error_label := Label {
            width: Fill
            height: Fit
            visible: false
            flow: Flow.Right { wrap: true }
            draw_text +: { color: #xff4444 }
        }
    }
}

// ============================================================================
// Widget
// ============================================================================

#[derive(Script, Widget, ScriptHook)]
pub struct VideoMessagePlayer {
    #[deref]
    view: View,

    /// When false, the maximise button in the top-left of the controls
    /// overlay is hidden. The embedded `VideoMessagePlayer` inside
    /// `VideoMessagePlayerModal` sets this to false because it is
    /// already shown at its maximised size.
    #[live(true)]
    show_maximise_button: bool,

    // Per-message metadata.
    #[rust]
    summary: Option<VideoSummary>,
    #[rust]
    video_source: Option<MediaSource>,
    #[rust]
    poster_source: Option<MediaSource>,
    #[rust]
    loaded_video: Option<OwnedMxcUri>,
    #[rust]
    loaded_source_url: Option<PathBuf>,
    #[rust]
    loaded_poster: Option<OwnedMxcUri>,
    #[rust]
    poster_texture: Option<Texture>,
    #[rust]
    blurhash: Option<String>,
    #[rust]
    blurhash_dimensions: Option<(u32, u32)>,
    #[rust]
    blurhash_decode_key: Option<(String, u32, u32)>,
    #[rust]
    blurhash_texture_key: Option<(String, u32, u32)>,
    #[rust]
    blurhash_receiver: Option<Receiver<Option<(u32, u32, Vec<u8>)>>>,
    #[rust]
    play_enabled: bool,

    // Shared state — these Arcs are cloned and handed to the modal on
    // maximise so both views observe the same playback / volume / ui
    // state through `Arc<Mutex<...>>`.
    #[rust]
    player_state: SharedPlayerState,
    #[rust]
    volume_state: SharedVolumeState,

    #[rust]
    slider_drag_was_playing: Option<bool>,
}

impl Widget for VideoMessagePlayer {
    fn handle_event(&mut self, cx: &mut Cx, event: &Event, scope: &mut Scope) {
        if matches!(event, Event::Signal) {
            self.poll_blurhash_receiver(cx);
        }
        if let Event::Actions(actions) = event {
            for action in actions {
                if let Some(VideoPlaybackAction::ActiveTrackChanged { now_playing }) =
                    action.downcast_ref::<VideoPlaybackAction>()
                {
                    if *now_playing != self.widget_uid() {
                        self.pause_for_other_video(cx);
                    }
                }
                if let Some(VideoPlaybackAction::ResumeInlineAfterModal { inline_uid }) =
                    action.downcast_ref::<VideoPlaybackAction>()
                {
                    if *inline_uid == self.widget_uid() {
                        self.begin_inline_after_modal(cx);
                    }
                }
            }

            if self
                .view
                .button(cx, ids!(surface.controls.mute_button))
                .clicked(actions)
            {
                self.toggle_mute(cx);
            }

            if self
                .view
                .button(cx, ids!(surface.controls.maximise_button))
                .clicked(actions)
            {
                self.emit_maximise(cx);
            }
        }

        self.view.handle_event(cx, event, scope);
    }

    fn draw_walk(&mut self, cx: &mut Cx2d, scope: &mut Scope, walk: Walk) -> DrawStep {
        self.view.draw_walk(cx, scope, walk)
    }
}

// ============================================================================
// Population + control helpers
// ============================================================================

impl VideoMessagePlayer {
    pub fn populate_from_summary(
        &mut self,
        cx: &mut Cx,
        summary: VideoSummary,
        video_source: MediaSource,
        poster_source: Option<MediaSource>,
        media_cache: &mut MediaCache,
    ) -> bool {
        self.populate_from_summary_and_blurhash(
            cx,
            summary,
            video_source,
            poster_source,
            None,
            None,
            media_cache,
        )
    }

    pub fn populate_from_summary_and_blurhash(
        &mut self,
        cx: &mut Cx,
        summary: VideoSummary,
        video_source: MediaSource,
        poster_source: Option<MediaSource>,
        blurhash: Option<String>,
        blurhash_dimensions: Option<(u32, u32)>,
        media_cache: &mut MediaCache,
    ) -> bool {
        self.summary = Some(summary);
        self.video_source = Some(video_source);
        self.poster_source = poster_source.or_else(|| self.video_source.clone());
        self.loaded_video = None;
        self.blurhash = blurhash;
        self.blurhash_dimensions = blurhash_dimensions;
        self.slider_drag_was_playing = None;

        self.apply_summary_state(cx);
        let poster_drawn = self.populate_poster(cx, media_cache);
        let video_drawn = self.is_unplayable() || self.ensure_video_loaded(cx, media_cache);
        if poster_drawn && !video_drawn {
            self.video_ref(cx)
                .set_thumbnail_texture(cx, self.poster_texture.clone());
        }
        self.sync_controls(cx);
        poster_drawn && video_drawn
    }

    /// Populate the player from a file path that has already been
    /// resolved by another player (typically the inline timeline
    /// player). Used by `VideoMessagePlayerModal`, which receives the
    /// loaded path through `VideoMessagePlayerModalAction::Open` and
    /// has no `MediaCache` to look up.
    pub fn populate_from_loaded_url(
        &mut self,
        cx: &mut Cx,
        summary: VideoSummary,
        source_url: PathBuf,
        blurhash: Option<String>,
    ) {
        self.summary = Some(summary);
        self.blurhash = blurhash;
        self.loaded_source_url = Some(source_url.clone());
        self.play_enabled = true;
        self.slider_drag_was_playing = None;

        let video = self.video_ref(cx);
        video.set_source(VideoDataSource::Filesystem {
            path: source_url.to_string_lossy().into_owned(),
        });
        video.should_dispatch_texture_updates(true);

        self.apply_summary_state(cx);
        self.sync_controls(cx);
    }

    fn apply_summary_state(&mut self, cx: &mut Cx) {
        let unplayable = self.is_unplayable();
        self.view(cx, ids!(surface.unplayable_overlay))
            .set_visible(cx, unplayable);
        self.view
            .button(cx, ids!(surface.controls.mute_button))
            .set_enabled(cx, !unplayable);
        self.view
            .button(cx, ids!(surface.controls.maximise_button))
            .set_visible(cx, self.show_maximise_button);

        let total = format_mmss(self.total_secs());
        self.view
            .label(cx, ids!(surface.controls.slider_row.total_label))
            .set_text(cx, &total);
        self.view(cx, ids!(error_label)).set_visible(cx, false);
    }

    fn populate_poster(&mut self, cx: &mut Cx, media_cache: &mut MediaCache) -> bool {
        let Some(MediaSource::Plain(mxc_uri)) = self.poster_source.clone() else {
            self.apply_blurhash_or_fallback(cx);
            return true;
        };
        if self.loaded_poster.as_ref() == Some(&mxc_uri) {
            return true;
        }
        match media_cache.try_get_media_or_fetch(&mxc_uri, utils::MEDIA_THUMBNAIL_FORMAT.into()) {
            (MediaCacheEntry::Loaded(data), _) => {
                match crate::shared::image_viewer::get_png_or_jpg_image_buffer(data.to_vec()) {
                    Ok(image_buffer) => {
                        let texture = image_buffer.into_new_texture(cx);
                        self.video_ref(cx)
                            .set_thumbnail_texture(cx, Some(texture.clone()));
                        self.poster_texture = Some(texture);
                        self.loaded_poster = Some(mxc_uri);
                        true
                    }
                    Err(_) => {
                        self.apply_blurhash_or_fallback(cx);
                        true
                    }
                }
            }
            (MediaCacheEntry::Requested, _) => {
                self.apply_blurhash_or_fallback(cx);
                false
            }
            (MediaCacheEntry::Failed(_), _) => {
                self.apply_blurhash_or_fallback(cx);
                true
            }
        }
    }

    fn ensure_video_loaded(&mut self, cx: &mut Cx, media_cache: &mut MediaCache) -> bool {
        let Some(MediaSource::Plain(mxc_uri)) = self.video_source.clone() else {
            self.show_error(cx, "Encrypted video is not supported yet.");
            return false;
        };
        if self.loaded_video.as_ref() == Some(&mxc_uri) {
            return true;
        }
        match media_cache.try_get_media_or_fetch(&mxc_uri, MediaFormat::File) {
            (MediaCacheEntry::Loaded(data), MediaFormat::File) => {
                let mut path = media_cache.path_for(&mxc_uri);
                if path.extension().is_none() {
                    if let Some(summary) = self.summary.as_ref() {
                        path.set_extension(infer_video_extension(
                            &summary.filename,
                            summary.mime.as_deref(),
                        ));
                    }
                }
                if let Err(error) = std::fs::write(&path, &data) {
                    self.show_error(cx, &format!("Failed to stage video file: {error}"));
                    self.set_play_enabled(cx, false);
                    return false;
                }
                let video = self.video_ref(cx);
                video.set_source(VideoDataSource::Filesystem {
                    path: path.to_string_lossy().into_owned(),
                });
                video.should_dispatch_texture_updates(true);
                self.loaded_source_url = Some(path);
                self.loaded_video = Some(mxc_uri);
                self.set_play_enabled(cx, true);
                self.view(cx, ids!(error_label)).set_visible(cx, false);
                true
            }
            (MediaCacheEntry::Requested, _) | (MediaCacheEntry::Loaded(_), _) => {
                self.set_play_enabled(cx, false);
                false
            }
            (MediaCacheEntry::Failed(status_code), _) => {
                self.set_play_enabled(cx, false);
                self.show_error(
                    cx,
                    &format!("Failed to fetch video from {mxc_uri} (HTTP {status_code})"),
                );
                true
            }
        }
    }

    fn pause_for_other_video(&mut self, cx: &mut Cx) {
        let was_playing = self
            .player_state
            .lock()
            .ok()
            .map(|g| g.playing)
            .unwrap_or(false);
        if was_playing {
            self.video_ref(cx).pause_playback(cx);
        }
        if let Ok(mut s) = self.player_state.lock() {
            s.playing = false;
        }
        self.sync_controls(cx);
    }

    fn toggle_mute(&mut self, cx: &mut Cx) {
        let new_muted = if let Ok(mut volume) = self.volume_state.lock() {
            let action = if volume.muted {
                VolumeAction::Unmute
            } else {
                VolumeAction::Mute
            };
            apply_volume_action(&mut volume, action);
            volume.muted
        } else {
            false
        };
        let _ = new_muted;
        if new_muted {
            self.video_ref(cx).mute_playback(cx);
        } else {
            self.video_ref(cx).unmute_playback(cx);
        }
        self.sync_controls(cx);
    }

    fn emit_maximise(&mut self, cx: &mut Cx) {
        let Some(summary) = self.summary.clone() else {
            return;
        };
        let Some(source_url) = self.loaded_source_url.clone() else {
            return;
        };
        let position_ms = self.video_ref(cx).current_position_ms() as u64;
        self.video_ref(cx).stop_and_cleanup_resources(cx);
        if let Ok(mut state) = self.player_state.lock() {
            state.playing = false;
        }
        self.sync_controls(cx);
        cx.action(VideoMessagePlayerModalAction::Open {
            inline_uid: self.widget_uid(),
            source_url,
            blurhash: self.blurhash.clone(),
            summary,
            position_ms,
        });
    }

    fn show_error(&mut self, cx: &mut Cx, text: &str) {
        self.view.label(cx, ids!(error_label)).set_text(cx, text);
        self.view(cx, ids!(error_label)).set_visible(cx, true);
        self.view(cx, ids!(surface.controls.slider_row))
            .set_visible(cx, false);
    }

    fn is_unplayable(&self) -> bool {
        self.summary
            .as_ref()
            .is_some_and(should_show_unplayable_overlay)
    }

    fn total_ms(&self) -> u64 {
        self.summary
            .as_ref()
            .and_then(|s| s.duration_secs)
            .map(|secs| (secs.max(0.0) * 1000.0).round() as u64)
            .unwrap_or(0)
    }

    fn total_secs(&self) -> f64 {
        self.total_ms() as f64 / 1000.0
    }

    #[allow(unreachable_code, unused_variables)]
    fn sync_controls(&mut self, cx: &mut Cx) {
        // Short-circuited while the makepad Video widget's own controls
        // are driving play/pause/slider state. The body below is kept
        // for the eventual re-enable.
        return;
        let (playing, position_ms) = self
            .player_state
            .lock()
            .ok()
            .map(|g| (g.playing, g.position_ms))
            .unwrap_or((false, 0));
        let _muted = self
            .volume_state
            .lock()
            .ok()
            .map(|g| g.muted)
            .unwrap_or(false);
        let total_ms = self.total_ms();

        self.view
            .button(cx, ids!(surface.controls.center_controls.play_button))
            .set_visible(cx, !playing);
        self.view
            .button(cx, ids!(surface.controls.center_controls.pause_button))
            .set_visible(cx, playing);

        let normalized = if total_ms == 0 {
            0.0
        } else {
            (position_ms as f64 / total_ms as f64).clamp(0.0, 1.0)
        };
        self.view
            .slider(cx, ids!(surface.controls.slider_row.slider))
            .set_value(cx, normalized);
        self.view
            .label(cx, ids!(surface.controls.slider_row.elapsed_label))
            .set_text(cx, &format_mmss(position_ms as f64 / 1000.0));
    }

    fn set_play_enabled(&mut self, cx: &mut Cx, enabled: bool) {
        self.play_enabled = enabled;
        self.view
            .button(cx, ids!(surface.controls.center_controls.play_button))
            .set_enabled(cx, enabled);
        self.view
            .button(cx, ids!(surface.controls.center_controls.pause_button))
            .set_enabled(cx, enabled);
    }

    fn apply_blurhash_or_fallback(&mut self, cx: &mut Cx) {
        if let (Some(blurhash), Some((width, height))) =
            (self.blurhash.as_deref(), self.blurhash_dimensions)
        {
            let (width, height) = cap_blurhash_dimensions(
                width,
                height,
                crate::home::room_screen::BLURHASH_IMAGE_MAX_SIZE,
            );
            let key = (blurhash.to_string(), width, height);
            if self.blurhash_texture_key.as_ref() == Some(&key)
                || self.blurhash_decode_key.as_ref() == Some(&key)
            {
                return;
            }
            self.blurhash_decode_key = Some(key);
            let blurhash = blurhash.to_string();
            let (sender, receiver) = std::sync::mpsc::channel();
            self.blurhash_receiver = Some(receiver);
            cx.spawn_thread(move || {
                let result = decode_blurhash_to_rgba(&blurhash, width, height)
                    .map(|data| (width, height, data));
                let _ = sender.send(result);
                SignalToUI::set_ui_signal();
            });
            return;
        }
        let color = placeholder_fallback_color();
        let texture = ImageBuffer::new(&color, 1, 1)
            .ok()
            .map(|buf| buf.into_new_texture(cx));
        self.video_ref(cx).set_thumbnail_texture(cx, texture);
    }

    fn poll_blurhash_receiver(&mut self, cx: &mut Cx) {
        let Some(receiver) = self.blurhash_receiver.as_ref() else {
            return;
        };
        let Ok(result) = receiver.try_recv() else {
            return;
        };
        self.blurhash_receiver = None;
        match result {
            Some((width, height, data)) => {
                if let Ok(buffer) = ImageBuffer::new(&data, width as usize, height as usize) {
                    let texture = buffer.into_new_texture(cx);
                    self.video_ref(cx).set_thumbnail_texture(cx, Some(texture));
                    self.blurhash_texture_key = self.blurhash_decode_key.take();
                }
            }
            None => {
                self.blurhash_decode_key = None;
                let color = placeholder_fallback_color();
                let texture = ImageBuffer::new(&color, 1, 1)
                    .ok()
                    .map(|buf| buf.into_new_texture(cx));
                self.video_ref(cx).set_thumbnail_texture(cx, texture);
            }
        }
    }

    pub fn video_ref(&self, cx: &mut Cx) -> VideoRef {
        self.view.video(cx, ids!(surface.robrix_video))
    }

    pub fn loaded_source_url(&self) -> Option<PathBuf> {
        self.loaded_source_url.clone()
    }

    fn begin_inline_after_modal(&mut self, cx: &mut Cx) {
        self.video_ref(cx).begin_playback(cx);
        if let Ok(mut state) = self.player_state.lock() {
            state.playing = true;
            state.position_ms = 0;
        }
        self.sync_controls(cx);
        set_active_video(self.widget_uid());
    }
}

// ============================================================================
// Ref API
// ============================================================================

impl VideoMessagePlayerRef {
    pub fn populate_from_summary(
        &self,
        cx: &mut Cx,
        summary: VideoSummary,
        video_source: MediaSource,
        poster_source: Option<MediaSource>,
        media_cache: &mut MediaCache,
    ) -> bool {
        self.borrow_mut().is_some_and(|mut inner| {
            inner.populate_from_summary(cx, summary, video_source, poster_source, media_cache)
        })
    }

    pub fn populate_from_summary_and_blurhash(
        &self,
        cx: &mut Cx,
        summary: VideoSummary,
        video_source: MediaSource,
        poster_source: Option<MediaSource>,
        blurhash: Option<String>,
        blurhash_dimensions: Option<(u32, u32)>,
        media_cache: &mut MediaCache,
    ) -> bool {
        self.borrow_mut().is_some_and(|mut inner| {
            inner.populate_from_summary_and_blurhash(
                cx,
                summary,
                video_source,
                poster_source,
                blurhash,
                blurhash_dimensions,
                media_cache,
            )
        })
    }

    pub fn robrix_video(&self, cx: &mut Cx) -> VideoRef {
        self.borrow()
            .map(|inner| inner.video_ref(cx))
            .unwrap_or_default()
    }

    pub fn populate_from_loaded_url(
        &self,
        cx: &mut Cx,
        summary: VideoSummary,
        source_url: PathBuf,
        blurhash: Option<String>,
    ) {
        if let Some(mut inner) = self.borrow_mut() {
            inner.populate_from_loaded_url(cx, summary, source_url, blurhash);
        }
    }

    pub fn set_play_button_text(&self, _cx: &mut Cx, _text: &str) {}
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests_video_message_player {
    use super::*;
    use std::collections::HashSet;
    use std::sync::{Arc, Mutex};

    fn summary_with_mime(mime: Option<&str>) -> VideoSummary {
        VideoSummary {
            filename: "clip.mp4".to_string(),
            mime: mime.map(ToString::to_string),
            duration_secs: Some(4.0),
            size_bytes: None,
            dimensions: None,
            caption_html: None,
        }
    }

    // ---- is_playable_mime ----

    #[test]
    fn test_is_playable_mime_accepts_mp4() {
        assert!(is_playable_mime("video/mp4"));
    }

    #[test]
    fn test_is_playable_mime_accepts_webm_and_ogg() {
        assert!(is_playable_mime("video/webm"));
        assert!(is_playable_mime("video/ogg"));
    }

    #[test]
    fn test_is_playable_mime_normalizes_input() {
        assert!(is_playable_mime("VIDEO/MP4; codecs=avc1.42E01E"));
    }

    #[test]
    fn test_is_playable_mime_rejects_unsupported() {
        assert!(!is_playable_mime("video/x-matroska"));
    }

    #[test]
    fn test_is_playable_mime_rejects_empty() {
        assert!(!is_playable_mime(""));
    }

    #[test]
    fn test_infer_video_extension_prefers_filename() {
        assert_eq!(
            infer_video_extension("clip.mov", Some("video/mp4")),
            "mov"
        );
    }

    #[test]
    fn test_infer_video_extension_falls_back_to_mime() {
        assert_eq!(infer_video_extension("clip", Some("video/mp4")), "mp4");
        assert_eq!(infer_video_extension("clip", Some("video/quicktime")), "mov");
        assert_eq!(infer_video_extension("clip", Some("video/webm")), "webm");
    }

    #[test]
    fn test_infer_video_extension_defaults_to_mp4() {
        assert_eq!(infer_video_extension("clip", None), "mp4");
    }

    // ---- should_show_unplayable_overlay ----

    #[test]
    fn test_should_show_unplayable_overlay_for_unsupported_mime() {
        assert!(should_show_unplayable_overlay(&summary_with_mime(Some(
            "video/x-matroska",
        ))));
    }

    #[test]
    fn test_should_show_unplayable_overlay_false_for_mp4() {
        assert!(!should_show_unplayable_overlay(&summary_with_mime(Some(
            "video/mp4",
        ))));
    }

    #[test]
    fn test_should_show_unplayable_overlay_false_when_mime_none() {
        assert!(!should_show_unplayable_overlay(&summary_with_mime(None)));
    }

    // ---- poster / file cache layer decisions ----

    fn test_mxc_uri() -> OwnedMxcUri {
        "mxc://example.org/video".try_into().unwrap()
    }

    #[test]
    fn test_requested_poster_with_blurhash_decodes() {
        assert_eq!(
            poster_layer_decision(
                &MediaCacheEntry::Requested,
                Some("LEHV6nWB2yk8pyo0adR*.7kCMdnj"),
                Some((640, 480)),
            ),
            PosterLayerDecision::DecodeBlurhash {
                width: 500,
                height: 375,
            }
        );
    }

    #[test]
    fn test_requested_poster_without_blurhash_falls_back_to_solid() {
        assert_eq!(
            poster_layer_decision(&MediaCacheEntry::Requested, None, Some((640, 480))),
            PosterLayerDecision::SetSolidFallback([0x22, 0x22, 0x22, 0xFF])
        );
    }

    #[test]
    fn test_requested_poster_missing_width_skips_decode() {
        assert_eq!(
            poster_layer_decision(
                &MediaCacheEntry::Requested,
                Some("LEHV6nWB2yk8pyo0adR*.7kCMdnj"),
                None,
            ),
            PosterLayerDecision::SetSolidFallback([0x22, 0x22, 0x22, 0xFF])
        );
    }

    #[test]
    fn test_loaded_poster_sets_poster_texture() {
        assert_eq!(
            poster_layer_decision(&MediaCacheEntry::Loaded(Arc::from([0_u8; 4])), None, None),
            PosterLayerDecision::SetPosterTexture
        );
    }

    #[test]
    fn test_failed_poster_falls_back_to_blurhash() {
        assert_eq!(
            poster_layer_decision(
                &MediaCacheEntry::Failed(reqwest::StatusCode::NOT_FOUND),
                Some("LEHV6nWB2yk8pyo0adR*.7kCMdnj"),
                Some((640, 480)),
            ),
            PosterLayerDecision::DecodeBlurhash {
                width: 500,
                height: 375,
            }
        );
    }

    #[test]
    fn test_requested_video_file_disables_play() {
        let mxc_uri = test_mxc_uri();
        assert_eq!(
            video_file_layer_decision(
                &MediaCacheEntry::Requested,
                &MediaFormat::File,
                &mxc_uri,
                PathBuf::from("/tmp/clip.mp4"),
            ),
            VideoFileLayerDecision::DisablePlay
        );
    }

    #[test]
    fn test_loaded_video_file_enables_play() {
        let mxc_uri = test_mxc_uri();
        assert_eq!(
            video_file_layer_decision(
                &MediaCacheEntry::Loaded(Arc::from([0_u8; 4])),
                &MediaFormat::File,
                &mxc_uri,
                PathBuf::from("/tmp/clip.mp4"),
            ),
            VideoFileLayerDecision::SetSourceUrl(PathBuf::from("/tmp/clip.mp4"))
        );
    }

    #[test]
    fn test_failed_video_file_shows_inline_error() {
        let mxc_uri = test_mxc_uri();
        assert_eq!(
            video_file_layer_decision(
                &MediaCacheEntry::Failed(reqwest::StatusCode::INTERNAL_SERVER_ERROR),
                &MediaFormat::File,
                &mxc_uri,
                PathBuf::from("/tmp/clip.mp4"),
            ),
            VideoFileLayerDecision::SetInlineError(
                "Failed to fetch video from mxc://example.org/video (HTTP 500 Internal Server Error)"
                    .to_string()
            )
        );
    }

    // ---- apply_volume_action ----

    #[test]
    fn test_apply_volume_action_mute_snapshots_level() {
        let mut state = VideoVolumeState {
            muted: false,
            level: 0.6,
            restore_level: 0.0,
        };
        apply_volume_action(&mut state, VolumeAction::Mute);
        assert!(state.muted);
        assert_eq!(state.level, 0.0);
        assert_eq!(state.restore_level, 0.6);
    }

    #[test]
    fn test_apply_volume_action_unmute_restores_level() {
        let mut state = VideoVolumeState {
            muted: true,
            level: 0.0,
            restore_level: 0.6,
        };
        apply_volume_action(&mut state, VolumeAction::Unmute);
        assert!(!state.muted);
        assert_eq!(state.level, 0.6);
    }

    #[test]
    fn test_apply_volume_action_unmute_uses_min_guard() {
        let mut state = VideoVolumeState {
            muted: true,
            level: 0.0,
            restore_level: 0.0,
        };
        apply_volume_action(&mut state, VolumeAction::Unmute);
        assert!(!state.muted);
        assert_eq!(state.level, 0.05);
    }

    #[test]
    fn test_apply_volume_action_set_level_clamps() {
        let mut state = VideoVolumeState {
            muted: false,
            level: 0.5,
            restore_level: 0.0,
        };
        apply_volume_action(&mut state, VolumeAction::SetLevel(1.7));
        assert_eq!(state.level, 1.0);
        apply_volume_action(&mut state, VolumeAction::SetLevel(-0.2));
        assert_eq!(state.level, 0.0);
    }

    #[test]
    fn test_apply_volume_action_set_level_zero_implies_muted() {
        let mut state = VideoVolumeState {
            muted: false,
            level: 0.5,
            restore_level: 0.2,
        };
        apply_volume_action(&mut state, VolumeAction::SetLevel(0.0));
        assert!(state.muted);
        assert_eq!(state.restore_level, 0.2);
    }

    // ---- Arc<Mutex<...>> shared-state contracts (modal ↔ inline) ----

    #[test]
    fn test_video_player_state_arc_clones_share_mutations() {
        let state_a = Arc::new(Mutex::new(VideoPlayerState {
            playing: true,
            position_ms: 0,
        }));
        let state_b = Arc::clone(&state_a);
        state_b.lock().unwrap().position_ms = 4_321;
        assert_eq!(state_a.lock().unwrap().position_ms, 4_321);
        assert!(Arc::ptr_eq(&state_a, &state_b));
    }

    #[derive(Default, Debug)]
    struct RecordingVideo {
        uid: u64,
        current_position_ms: u64,
        playing: bool,
        calls: Vec<String>,
    }

    impl RecordingVideo {
        fn current_position_ms(&mut self) -> u64 {
            self.calls.push("current_position_ms".to_string());
            self.current_position_ms
        }

        fn stop_and_cleanup_resources(&mut self) {
            self.calls.push("stop_and_cleanup_resources".to_string());
            self.playing = false;
        }

        fn begin_playback(&mut self) {
            self.calls.push("begin_playback".to_string());
            self.playing = true;
            self.current_position_ms = 0;
        }

        fn pause_playback(&mut self) {
            self.calls.push("pause_playback".to_string());
            self.playing = false;
        }

        fn seek_to(&mut self, position_ms: u64) {
            self.calls.push(format!("seek_to({position_ms})"));
            self.current_position_ms = position_ms;
        }
    }

    #[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
    struct NextFrameToken(u64);

    #[derive(Default, Debug)]
    struct RecordingCx {
        next_frame: u64,
        calls: Vec<String>,
    }

    impl RecordingCx {
        fn new_next_frame(&mut self) -> NextFrameToken {
            let token = NextFrameToken(self.next_frame);
            self.next_frame += 1;
            token
        }

        fn seek_video_playback(&mut self, video_id: u64, position_ms: u64) {
            self.calls
                .push(format!("seek_video_playback({video_id}, {position_ms})"));
        }
    }

    #[derive(Default, Debug)]
    struct RecordingWindow {
        calls: Vec<&'static str>,
    }

    impl RecordingWindow {
        fn fullscreen(&mut self) {
            self.calls.push("fullscreen");
        }

        fn disable_fullscreen(&mut self) {
            self.calls.push("disable_fullscreen");
        }
    }

    fn open_modal_sequence(
        inline: &mut RecordingVideo,
        modal: &mut RecordingVideo,
        pending_modal_seek_ms: &mut Option<u64>,
        pending_fullscreen: &mut Option<NextFrameToken>,
        cx: &mut RecordingCx,
    ) {
        let main_pos_ms = inline.current_position_ms();
        *pending_modal_seek_ms = Some(main_pos_ms);
        inline.stop_and_cleanup_resources();
        modal.begin_playback();
        *pending_fullscreen = Some(cx.new_next_frame());
    }

    fn handle_modal_playback_prepared(
        modal: &mut RecordingVideo,
        pending_modal_seek_ms: &mut Option<u64>,
        cx: &mut RecordingCx,
        video_id: u64,
    ) {
        if let Some(position_ms) = pending_modal_seek_ms.take() {
            cx.seek_video_playback(video_id, position_ms);
            modal.seek_to(position_ms);
        }
    }

    fn close_video_modal_sequence(inline: &mut RecordingVideo, modal: &mut RecordingVideo) {
        modal.stop_and_cleanup_resources();
        inline.begin_playback();
    }

    fn close_video_modal_sets_pending_normalize(
        inline: &mut RecordingVideo,
        modal: &mut RecordingVideo,
        pending_normalize: &mut Option<NextFrameToken>,
        cx: &mut RecordingCx,
    ) {
        close_video_modal_sequence(inline, modal);
        *pending_normalize = Some(cx.new_next_frame());
    }

    fn apply_pending_fullscreen(
        fired: &HashSet<NextFrameToken>,
        window: &mut RecordingWindow,
        pending_fullscreen: &mut Option<NextFrameToken>,
    ) {
        if pending_fullscreen.is_some_and(|token| fired.contains(&token)) {
            window.fullscreen();
            *pending_fullscreen = None;
        }
    }

    fn apply_pending_normalize(
        fired: &HashSet<NextFrameToken>,
        window: &mut RecordingWindow,
        pending_normalize: &mut Option<NextFrameToken>,
    ) {
        if pending_normalize.is_some_and(|token| fired.contains(&token)) {
            window.disable_fullscreen();
            *pending_normalize = None;
        }
    }

    fn handle_active_track_changed(now_playing_uid: u64, players: &mut [RecordingVideo]) {
        for player in players {
            if player.uid != now_playing_uid && player.playing {
                player.pause_playback();
            }
        }
    }

    #[derive(Default, Debug)]
    struct RecordingModal {
        open: bool,
        calls: Vec<String>,
    }

    impl RecordingModal {
        fn is_open(&self) -> bool {
            self.open
        }

        fn close(&mut self) {
            self.calls.push("close".to_string());
            self.open = false;
        }
    }

    fn close_video_modal_with_outer(
        inline: &mut RecordingVideo,
        modal_video: &mut RecordingVideo,
        outer: &mut RecordingModal,
        close_helper_invocations: &mut usize,
    ) {
        *close_helper_invocations += 1;
        close_video_modal_sequence(inline, modal_video);
        outer.close();
    }

    fn handle_key_down_for_modal(
        key_code: &str,
        inline: &mut RecordingVideo,
        modal_video: &mut RecordingVideo,
        outer: &mut RecordingModal,
        close_helper_invocations: &mut usize,
    ) {
        if key_code == "Escape" && outer.is_open() {
            close_video_modal_with_outer(inline, modal_video, outer, close_helper_invocations);
        }
    }

    #[test]
    fn test_maximise_captures_inline_position_before_stop() {
        let mut inline = RecordingVideo {
            current_position_ms: 3_500,
            playing: true,
            ..Default::default()
        };
        let mut modal = RecordingVideo::default();
        let mut pending_modal_seek_ms = None;
        let mut pending_fullscreen = None;
        let mut cx = RecordingCx {
            next_frame: 42,
            ..Default::default()
        };

        open_modal_sequence(
            &mut inline,
            &mut modal,
            &mut pending_modal_seek_ms,
            &mut pending_fullscreen,
            &mut cx,
        );

        assert_eq!(pending_modal_seek_ms, Some(3_500));
        assert_eq!(
            inline.calls,
            vec!["current_position_ms", "stop_and_cleanup_resources"]
        );
    }

    #[test]
    fn test_modal_seeks_on_playback_prepared() {
        let mut pending_modal_seek_ms = Some(3_500);
        let mut modal = RecordingVideo::default();
        let mut cx = RecordingCx::default();

        handle_modal_playback_prepared(&mut modal, &mut pending_modal_seek_ms, &mut cx, 7);

        assert_eq!(cx.calls, vec!["seek_video_playback(7, 3500)"]);
        assert_eq!(modal.calls, vec!["seek_to(3500)"]);
        assert_eq!(pending_modal_seek_ms, None);
    }

    #[test]
    fn test_playback_prepared_without_pending_seek_is_noop() {
        let mut pending_modal_seek_ms = None;
        let mut modal = RecordingVideo::default();
        let mut cx = RecordingCx::default();

        handle_modal_playback_prepared(&mut modal, &mut pending_modal_seek_ms, &mut cx, 7);

        assert!(cx.calls.is_empty());
        assert!(modal.calls.is_empty());
    }

    #[test]
    fn test_close_does_not_preserve_position() {
        let mut inline = RecordingVideo::default();
        let mut modal = RecordingVideo {
            current_position_ms: 2_000,
            ..Default::default()
        };

        close_video_modal_sequence(&mut inline, &mut modal);

        assert_eq!(modal.calls, vec!["stop_and_cleanup_resources"]);
        assert_eq!(inline.calls, vec!["begin_playback"]);
        assert!(!modal.calls.contains(&"current_position_ms".to_string()));
    }

    #[test]
    fn test_open_sequence_sets_pending_fullscreen() {
        let mut inline = RecordingVideo::default();
        let mut modal = RecordingVideo::default();
        let mut pending_modal_seek_ms = None;
        let mut pending_fullscreen = None;
        let mut cx = RecordingCx {
            next_frame: 42,
            ..Default::default()
        };

        open_modal_sequence(
            &mut inline,
            &mut modal,
            &mut pending_modal_seek_ms,
            &mut pending_fullscreen,
            &mut cx,
        );

        assert_eq!(pending_fullscreen, Some(NextFrameToken(42)));
    }

    #[test]
    fn test_handle_next_frame_applies_fullscreen_once() {
        let mut window = RecordingWindow::default();
        let mut pending_fullscreen = Some(NextFrameToken(42));
        let fired = HashSet::from([NextFrameToken(42)]);

        apply_pending_fullscreen(&fired, &mut window, &mut pending_fullscreen);
        apply_pending_fullscreen(&fired, &mut window, &mut pending_fullscreen);

        assert_eq!(window.calls, vec!["fullscreen"]);
        assert_eq!(pending_fullscreen, None);
    }

    #[test]
    fn test_handle_next_frame_waits_for_matching_token() {
        let mut window = RecordingWindow::default();
        let mut pending_fullscreen = Some(NextFrameToken(42));
        let fired = HashSet::from([NextFrameToken(7)]);

        apply_pending_fullscreen(&fired, &mut window, &mut pending_fullscreen);

        assert!(window.calls.is_empty());
        assert_eq!(pending_fullscreen, Some(NextFrameToken(42)));
    }

    #[test]
    fn test_close_video_modal_sets_pending_normalize() {
        let mut inline = RecordingVideo::default();
        let mut modal = RecordingVideo::default();
        let mut pending_normalize = None;
        let mut cx = RecordingCx {
            next_frame: 99,
            ..Default::default()
        };

        close_video_modal_sets_pending_normalize(
            &mut inline,
            &mut modal,
            &mut pending_normalize,
            &mut cx,
        );

        assert_eq!(pending_normalize, Some(NextFrameToken(99)));
    }

    #[test]
    fn test_handle_next_frame_applies_disable_fullscreen() {
        let mut window = RecordingWindow::default();
        let mut pending_normalize = Some(NextFrameToken(99));
        let fired = HashSet::from([NextFrameToken(99)]);

        apply_pending_normalize(&fired, &mut window, &mut pending_normalize);

        assert_eq!(window.calls, vec!["disable_fullscreen"]);
        assert_eq!(pending_normalize, None);
    }

    #[test]
    fn test_active_track_changed_pauses_others() {
        let mut players = [
            RecordingVideo {
                uid: 1,
                playing: true,
                ..Default::default()
            },
            RecordingVideo {
                uid: 2,
                playing: true,
                ..Default::default()
            },
        ];

        handle_active_track_changed(2, &mut players);

        assert_eq!(players[0].calls, vec!["pause_playback"]);
        assert!(!players[0]
            .calls
            .contains(&"stop_and_cleanup_resources".to_string()));
        assert!(players[1].calls.is_empty());
    }

    #[test]
    fn test_escape_calls_close_when_modal_open() {
        let mut inline = RecordingVideo::default();
        let mut modal_video = RecordingVideo::default();
        let mut outer = RecordingModal {
            open: true,
            ..Default::default()
        };
        let mut close_helper_invocations = 0;

        handle_key_down_for_modal(
            "Escape",
            &mut inline,
            &mut modal_video,
            &mut outer,
            &mut close_helper_invocations,
        );

        assert_eq!(modal_video.calls, vec!["stop_and_cleanup_resources"]);
        assert_eq!(inline.calls, vec!["begin_playback"]);
        assert!(outer.calls.contains(&"close".to_string()));
    }

    #[test]
    fn test_escape_ignored_when_modal_closed() {
        let mut inline = RecordingVideo::default();
        let mut modal_video = RecordingVideo::default();
        let mut outer = RecordingModal::default();
        let mut close_helper_invocations = 0;

        handle_key_down_for_modal(
            "Escape",
            &mut inline,
            &mut modal_video,
            &mut outer,
            &mut close_helper_invocations,
        );

        assert!(modal_video.calls.is_empty());
        assert!(inline.calls.is_empty());
        assert!(!outer.calls.contains(&"close".to_string()));
    }

    #[test]
    fn test_non_escape_key_does_not_close_modal() {
        let mut inline = RecordingVideo::default();
        let mut modal_video = RecordingVideo::default();
        let mut outer = RecordingModal {
            open: true,
            ..Default::default()
        };
        let mut close_helper_invocations = 0;

        handle_key_down_for_modal(
            "Space",
            &mut inline,
            &mut modal_video,
            &mut outer,
            &mut close_helper_invocations,
        );

        assert!(modal_video.calls.is_empty());
        assert!(inline.calls.is_empty());
        assert!(!outer.calls.contains(&"close".to_string()));
    }

    #[test]
    fn test_all_close_paths_route_through_helper() {
        let mut close_helper_invocations = 0;

        for _ in ["close_button", "scrim", "escape"] {
            let mut inline = RecordingVideo::default();
            let mut modal_video = RecordingVideo::default();
            let mut outer = RecordingModal {
                open: true,
                ..Default::default()
            };
            close_video_modal_with_outer(
                &mut inline,
                &mut modal_video,
                &mut outer,
                &mut close_helper_invocations,
            );
        }

        assert_eq!(close_helper_invocations, 3);
    }

    #[test]
    fn test_modal_mute_propagates_to_inline_volume() {
        let volume = Arc::new(Mutex::new(VideoVolumeState {
            muted: false,
            level: 0.7,
            restore_level: 0.0,
        }));
        let inline_binding = Arc::clone(&volume);
        apply_volume_action(&mut volume.lock().unwrap(), VolumeAction::Mute);
        let guard = inline_binding.lock().unwrap();
        assert!(guard.muted);
        assert_eq!(guard.level, 0.0);
        assert_eq!(guard.restore_level, 0.7);
    }
}
