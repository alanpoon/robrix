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
    rc::Rc,
    sync::{Arc, Mutex},
};

use makepad_widgets::*;
use matrix_sdk::ruma::{events::room::MediaSource, OwnedMxcUri};
use matrix_sdk::media::MediaFormat;

pub use crate::event_preview::VideoSummary;
pub use crate::shared::audio_message_player::DragPhase;
use crate::{
    event_preview::format_mmss,
    media_cache::{MediaCache, MediaCacheEntry},
    shared::video_message_player_modal::VideoMessagePlayerModalAction,
    utils,
};

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

#[derive(Clone, Copy, Debug, Default)]
pub struct VideoUiState {
    pub maximised: bool,
    pub card_rect: Option<Rect>,
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
pub type SharedUiState = Arc<Mutex<VideoUiState>>;

// ============================================================================
// Cross-widget actions
// ============================================================================

#[derive(Clone, Debug)]
pub enum VideoPlaybackAction {
    /// Broadcast whenever a new video begins playback. Other video
    /// players observe this and pause themselves if their uid does not
    /// match — same "single-active track" model the audio player uses.
    ActiveTrackChanged { now_playing: WidgetUid },
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

/// Apply a slider-drag update to `VideoPlayerState`. `Start` and `Move`
/// pause playback (so the user sees the seeked frame); `End { was_playing }`
/// resumes only if `was_playing` was true AND we haven't scrubbed to the
/// very end.
pub fn apply_video_slider_drag(
    state: &mut VideoPlayerState,
    normalized_pos: f64,
    total_ms: u64,
    phase: DragPhase,
) {
    let normalized_pos = normalized_pos.clamp(0.0, 1.0);
    state.position_ms = (normalized_pos * total_ms as f64).round() as u64;
    match phase {
        DragPhase::Start { .. } | DragPhase::Move => {
            state.playing = false;
        }
        DragPhase::End { was_playing } => {
            state.playing = was_playing && state.position_ms < total_ms;
        }
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

/// Flip `VideoUiState.maximised`. The single mutator for that field —
/// both the inline maximise button and the modal close path go
/// through this helper.
pub fn toggle_maximise(state: &mut VideoUiState) {
    state.maximised = !state.maximised;
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

            poster_image := Image {
                width: Fill
                height: Fill
                fit: ImageFit.Smallest
            }

            video_surface := Video {
                width: Fill
                height: Fill
                show_controls: false
                show_idle_thumbnail: true
                autoplay: false
                is_looping: false
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
                    margin: Inset{left: 99999}     // top-right
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

                center_controls := View {
                    width: Fill
                    height: Fill
                    align: Align{x: 0.5, y: 0.5}

                    play_button := Button {
                        width: 54
                        height: 54
                        text: ""
                        spacing: 0
                        padding: 0
                        align: Align{x: 0.5, y: 0.5}
                        icon_walk: Walk{width: 22, height: 22, margin: Inset{left: 3}}
                        draw_icon +: {
                            svg: (mod.widgets.VIDEO_ICON_PLAY)
                            color: #xffffff
                        }
                        draw_bg +: {
                            border_radius: 7.0
                            color: #x111827
                            color_hover: #x374151
                            color_down: #x111827
                            color_disabled: #x737A85
                        }
                    }
                    pause_button := Button {
                        width: 54
                        height: 54
                        visible: false
                        text: ""
                        spacing: 0
                        padding: 0
                        align: Align{x: 0.5, y: 0.5}
                        icon_walk: Walk{width: 20, height: 22}
                        draw_icon +: {
                            svg: (mod.widgets.VIDEO_ICON_PAUSE)
                            color: #xffffff
                        }
                        draw_bg +: {
                            border_radius: 7.0
                            color: #x111827
                            color_hover: #x374151
                            color_down: #x111827
                            color_disabled: #x737A85
                        }
                    }
                }

                slider_row := View {
                    width: Fill
                    height: Fit
                    margin: Inset{top: 99999}      // bottom strip
                    flow: Right
                    spacing: 8
                    padding: Inset{top: 4, bottom: 4, left: 8, right: 8}
                    align: Align{y: 0.5}
                    show_bg: true
                    draw_bg +: {
                        color: #x111827
                        border_radius: 5.0
                    }
                    elapsed_label := Label {
                        width: 46
                        height: Fit
                        text: "00:00"
                        draw_text +: { color: #xffffff }
                    }
                    slider := SliderMinimal {
                        width: Fill
                        height: 20
                        min: 0.0
                        max: 1.0
                        step: 0.0
                        default: 0.0
                        precision: 2
                        hover_actions_enabled: false
                        text_input: TextInput { visible: false, width: 0, height: 0 }
                    }
                    total_label := Label {
                        width: 46
                        height: Fit
                        text: "00:00"
                        draw_text +: { color: #xffffff }
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
    #[deref] view: View,

    // Per-message metadata.
    #[rust] summary: Option<VideoSummary>,
    #[rust] video_source: Option<MediaSource>,
    #[rust] poster_source: Option<MediaSource>,
    #[rust] loaded_video: Option<OwnedMxcUri>,

    // Shared state — these Arcs are cloned and handed to the modal on
    // maximise so both views observe the same playback / volume / ui
    // state through `Arc<Mutex<...>>`.
    #[rust] player_state: SharedPlayerState,
    #[rust] volume_state: SharedVolumeState,
    #[rust] ui_state: SharedUiState,

    #[rust] slider_drag_was_playing: Option<bool>,
}

impl Widget for VideoMessagePlayer {
    fn handle_event(&mut self, cx: &mut Cx, event: &Event, scope: &mut Scope) {
        if let Event::Actions(actions) = event {
            for action in actions {
                if let Some(VideoPlaybackAction::ActiveTrackChanged { now_playing }) =
                    action.downcast_ref::<VideoPlaybackAction>()
                {
                    if *now_playing != self.widget_uid() {
                        self.pause_for_other_video(cx);
                    }
                }
            }

            let play_button = self
                .view
                .button(cx, ids!(surface.controls.center_controls.play_button));
            let pause_button = self
                .view
                .button(cx, ids!(surface.controls.center_controls.pause_button));
            if play_button.clicked(actions) || pause_button.clicked(actions) {
                self.toggle_playback(cx);
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

            let slider = self
                .view
                .slider(cx, ids!(surface.controls.slider_row.slider));
            if let Some(action) = actions.find_widget_action(slider.widget_uid()) {
                let value_now = slider.value().unwrap_or(0.0);
                let total_ms = self.total_ms();
                if let Ok(mut state) = self.player_state.lock() {
                    match action.cast() {
                        SliderAction::StartSlide => {
                            let was_playing = state.playing;
                            self.slider_drag_was_playing = Some(was_playing);
                            apply_video_slider_drag(
                                &mut state,
                                value_now,
                                total_ms,
                                DragPhase::Start { was_playing },
                            );
                        }
                        SliderAction::Slide(v) | SliderAction::TextSlide(v) => {
                            apply_video_slider_drag(&mut state, v, total_ms, DragPhase::Move);
                        }
                        SliderAction::EndSlide(v) => {
                            let was = self.slider_drag_was_playing.take().unwrap_or(false);
                            apply_video_slider_drag(
                                &mut state,
                                v,
                                total_ms,
                                DragPhase::End { was_playing: was },
                            );
                        }
                        _ => {}
                    }
                }
                self.sync_controls(cx);
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
    ) {
        self.summary = Some(summary);
        self.video_source = Some(video_source);
        self.poster_source = poster_source.or_else(|| self.video_source.clone());
        self.loaded_video = None;
        self.slider_drag_was_playing = None;

        self.apply_summary_state(cx);
        self.populate_poster(cx, media_cache);
        if !self.is_unplayable() {
            self.ensure_video_loaded(cx, media_cache);
        }
        self.sync_controls(cx);
    }

    fn apply_summary_state(&mut self, cx: &mut Cx) {
        let unplayable = self.is_unplayable();
        self.view(cx, ids!(surface.unplayable_overlay))
            .set_visible(cx, unplayable);
        self.view(cx, ids!(surface.controls.slider_row))
            .set_visible(cx, !unplayable);
        self.view
            .button(cx, ids!(surface.controls.center_controls.play_button))
            .set_enabled(cx, !unplayable);
        self.view
            .button(cx, ids!(surface.controls.center_controls.pause_button))
            .set_enabled(cx, !unplayable);
        self.view
            .button(cx, ids!(surface.controls.mute_button))
            .set_enabled(cx, !unplayable);

        let total = format_mmss(self.total_secs());
        self.view
            .label(cx, ids!(surface.controls.slider_row.total_label))
            .set_text(cx, &total);
        self.view(cx, ids!(error_label)).set_visible(cx, false);
    }

    fn populate_poster(&mut self, cx: &mut Cx, media_cache: &mut MediaCache) {
        let Some(MediaSource::Plain(mxc_uri)) = self.poster_source.clone() else {
            return;
        };
        if let (MediaCacheEntry::Loaded(data), MediaFormat::Thumbnail(_)) =
            media_cache.try_get_media_or_fetch(&mxc_uri, utils::MEDIA_THUMBNAIL_FORMAT.into())
        {
            let image = self.view.image(cx, ids!(surface.poster_image));
            let _ = utils::load_png_or_jpg(&image, cx, &data);
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
                self.view
                    .video(cx, ids!(surface.video_surface))
                    .set_source_in_memory(Rc::new(data.to_vec()));
                self.loaded_video = Some(mxc_uri);
                self.view(cx, ids!(error_label)).set_visible(cx, false);
                true
            }
            (MediaCacheEntry::Requested, _) | (MediaCacheEntry::Loaded(_), _) => false,
            (MediaCacheEntry::Failed(_), _) => {
                self.show_error(cx, "Failed to fetch video.");
                false
            }
        }
    }

    fn toggle_playback(&mut self, cx: &mut Cx) {
        if self.is_unplayable() {
            return;
        }
        let video = self.view.video(cx, ids!(surface.video_surface));
        let was_playing = self
            .player_state
            .lock()
            .ok()
            .map(|g| g.playing)
            .unwrap_or(false);
        if was_playing || video.is_playing() {
            video.pause_playback(cx);
            if let Ok(mut s) = self.player_state.lock() {
                s.playing = false;
            }
        } else {
            if video.is_paused() {
                video.resume_playback(cx);
            } else {
                video.begin_playback(cx);
            }
            if let Ok(mut s) = self.player_state.lock() {
                s.playing = true;
            }
            set_active_video(self.widget_uid());
        }
        self.sync_controls(cx);
    }

    fn pause_for_other_video(&mut self, cx: &mut Cx) {
        let was_playing = self
            .player_state
            .lock()
            .ok()
            .map(|g| g.playing)
            .unwrap_or(false);
        if was_playing {
            self.view
                .video(cx, ids!(surface.video_surface))
                .pause_playback(cx);
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
        let video = self.view.video(cx, ids!(surface.video_surface));
        if new_muted {
            video.mute_playback(cx);
        } else {
            video.unmute_playback(cx);
        }
        self.sync_controls(cx);
    }

    fn emit_maximise(&mut self, cx: &mut Cx) {
        let Some(summary) = self.summary.clone() else {
            return;
        };
        if let Ok(mut ui) = self.ui_state.lock() {
            ui.maximised = true;
        }
        cx.action(VideoMessagePlayerModalAction::Open {
            player_state: Arc::clone(&self.player_state),
            volume_state: Arc::clone(&self.volume_state),
            ui_state: Arc::clone(&self.ui_state),
            summary,
        });
    }

    fn show_error(&mut self, cx: &mut Cx, text: &str) {
        self.view.label(cx, ids!(error_label)).set_text(cx, text);
        self.view(cx, ids!(error_label)).set_visible(cx, true);
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

    fn sync_controls(&mut self, cx: &mut Cx) {
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
    ) {
        if let Some(mut inner) = self.borrow_mut() {
            inner.populate_from_summary(cx, summary, video_source, poster_source, media_cache);
        }
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests_video_message_player {
    use super::*;
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

    // ---- apply_video_slider_drag ----

    #[test]
    fn test_apply_video_slider_drag_start_pauses_playback() {
        let mut state = VideoPlayerState {
            playing: true,
            position_ms: 1_000,
        };
        apply_video_slider_drag(
            &mut state,
            0.5,
            4_000,
            DragPhase::Start { was_playing: true },
        );
        assert!(!state.playing);
        assert_eq!(state.position_ms, 2_000);
    }

    #[test]
    fn test_apply_video_slider_drag_end_resumes_when_was_playing() {
        let mut state = VideoPlayerState {
            playing: false,
            position_ms: 500,
        };
        apply_video_slider_drag(
            &mut state,
            0.25,
            4_000,
            DragPhase::End { was_playing: true },
        );
        assert!(state.playing);
        assert_eq!(state.position_ms, 1_000);
    }

    #[test]
    fn test_apply_video_slider_drag_end_does_not_resume_at_end() {
        let mut state = VideoPlayerState {
            playing: false,
            position_ms: 0,
        };
        apply_video_slider_drag(
            &mut state,
            1.0,
            4_000,
            DragPhase::End { was_playing: true },
        );
        assert!(!state.playing);
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

    // ---- toggle_maximise ----

    #[test]
    fn test_toggle_maximise_round_trip() {
        let mut state = VideoUiState::default();
        toggle_maximise(&mut state);
        assert!(state.maximised);
        toggle_maximise(&mut state);
        assert!(!state.maximised);
    }

    #[test]
    fn test_close_button_closes_via_toggle_maximise() {
        let mut state = VideoUiState {
            maximised: true,
            card_rect: None,
        };
        toggle_maximise(&mut state);
        assert!(!state.maximised);

        let mut scrim_state = VideoUiState {
            maximised: true,
            card_rect: None,
        };
        toggle_maximise(&mut scrim_state);
        assert!(!scrim_state.maximised);
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

    #[test]
    fn test_modal_slider_drag_updates_shared_state() {
        let state = Arc::new(Mutex::new(VideoPlayerState {
            playing: true,
            position_ms: 0,
        }));
        let modal_binding = Arc::clone(&state);
        apply_video_slider_drag(
            &mut modal_binding.lock().unwrap(),
            0.5,
            4_000,
            DragPhase::Start { was_playing: true },
        );
        let guard = state.lock().unwrap();
        assert_eq!(guard.position_ms, 2_000);
        assert!(!guard.playing);
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

    #[test]
    fn test_close_modal_preserves_playback_state() {
        let state = Arc::new(Mutex::new(VideoPlayerState {
            playing: true,
            position_ms: 3_500,
        }));
        let volume = Arc::new(Mutex::new(VideoVolumeState {
            muted: true,
            level: 0.0,
            restore_level: 0.6,
        }));
        let mut ui = VideoUiState {
            maximised: true,
            card_rect: None,
        };

        toggle_maximise(&mut ui);

        assert!(!ui.maximised);
        let player = state.lock().unwrap();
        assert_eq!(player.position_ms, 3_500);
        assert!(player.playing);
        let vol = volume.lock().unwrap();
        assert!(vol.muted);
        assert_eq!(vol.level, 0.0);
    }
}
