//! Standalone Matrix-agnostic video surface for Robrix video messages.

use std::{path::PathBuf, time::Instant};

use makepad_widgets::*;

script_mod! {
    use mod.prelude.widgets.*
    use mod.widgets.*

    mod.widgets.RobrixVideo = #(RobrixVideo::register_widget(vm)) {
        width: Fill
        height: Fill
        flow: Overlay
        show_bg: true
        draw_bg +: {
            color: #x222222
        }

        // poster_image := Image {
        //     width: Fill
        //     height: Fill
        //     // Biggest = cover: fill the Fill x Fill rect so the poster's
        //     // drawn rect matches video_surface's allocated rect. Preserves
        //     // aspect, cropping any overflow when poster and video aspects
        //     // disagree. Smallest (the previous setting) letterboxed the
        //     // poster smaller than video_surface, which made the composer's
        //     // controls land outside the visible poster rect.
        //     fit: ImageFit.Biggest
        // }

        video_surface := Video {
            width: Fill
            height: Fill
            autoplay: false
            is_looping: false
            // The VideoMessagePlayer composer renders its own controls
            // (play, mute, maximise, slider) anchored to the outer surface.
            // Letting the inner Video widget render its own controls too
            // creates a duplicate overlay positioned relative to the
            // letterboxed video rect — which differs from the poster's
            // letterboxed rect, so Makepad's overlay button can end up
            // outside the visible poster when paused.
            show_controls: true
            show_idle_thumbnail: true
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Default)]
pub enum BlurhashState {
    #[default]
    NoSource,
    NotYetStarted,
    AwaitingFirstFrame,
    Playing,
    Stopped,
}

pub fn should_show_blurhash(state: BlurhashState) -> bool {
    matches!(
        state,
        BlurhashState::NoSource
            | BlurhashState::NotYetStarted
            | BlurhashState::AwaitingFirstFrame
            | BlurhashState::Stopped
    )
}

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

#[derive(Script, Widget, ScriptHook)]
pub struct RobrixVideo {
    #[deref]
    view: View,
    #[rust]
    source_url: Option<PathBuf>,
    #[rust]
    blurhash: Option<String>,
    #[rust]
    current_position_ms: u64,
    #[rust]
    blurhash_state: BlurhashState,
    #[rust]
    playing_since: Option<Instant>,
    #[rust]
    poster_texture: Option<Texture>,
}

impl Widget for RobrixVideo {
    fn handle_event(&mut self, cx: &mut Cx, event: &Event, scope: &mut Scope) {
        if let Event::Actions(actions) = event {
            let video = self.view.video(cx, ids!(video_surface));
            if let Some(action) = actions.find_widget_action(video.widget_uid()) {
                match action.cast() {
                    VideoAction::PlaybackPrepared => {
                        self.blurhash_state = BlurhashState::AwaitingFirstFrame;
                    }
                    VideoAction::PlaybackBegan => {
                        if self.blurhash_state != BlurhashState::Playing {
                            video.show_thumbnail(cx, false);
                        }
                        self.blurhash_state = BlurhashState::Playing;
                    }
                    VideoAction::TextureUpdated => {
                        if self.blurhash_state != BlurhashState::Playing {
                            video.show_thumbnail(cx, false);
                        }
                        self.blurhash_state = BlurhashState::Playing;
                    }
                    VideoAction::PlaybackCompleted | VideoAction::PlayerReset => {
                        self.blurhash_state = BlurhashState::Stopped;
                    }
                    _ => {}
                }
            }
        }
        self.view.handle_event(cx, event, scope);

        let video_area = self.view.video(cx, ids!(video_surface)).area();
        if let Hit::FingerHoverOut(_) = event.hits(cx, video_area) {
            if self.blurhash_state != BlurhashState::Playing {
                if let Some(texture) = self.poster_texture.clone() {
                    let video_surface = self.view.video(cx, ids!(video_surface));
                    video_surface.set_thumbnail_texture(cx, Some(texture));
                    video_surface.show_thumbnail(cx, true);
                }
            }
        }
    }

    fn draw_walk(&mut self, cx: &mut Cx2d, scope: &mut Scope, walk: Walk) -> DrawStep {
        self.view.draw_walk(cx, scope, walk)
    }
}

impl RobrixVideo {
    pub fn begin_playback(&mut self, cx: &mut Cx) {
        self.view.video(cx, ids!(video_surface)).begin_playback(cx);
        self.blurhash_state = BlurhashState::AwaitingFirstFrame;
        self.playing_since = Some(Instant::now());
    }

    pub fn stop_and_cleanup_resources(&mut self, cx: &mut Cx) {
        self.view
            .video(cx, ids!(video_surface))
            .stop_and_cleanup_resources(cx);
        self.blurhash_state = BlurhashState::Stopped;
        self.current_position_ms = 0;
        self.playing_since = None;
    }

    pub fn pause_playback(&mut self, cx: &mut Cx) {
        self.update_current_position();
        self.view.video(cx, ids!(video_surface)).pause_playback(cx);
        self.playing_since = None;
    }

    pub fn resume_playback(&mut self, cx: &mut Cx) {
        self.view.video(cx, ids!(video_surface)).resume_playback(cx);
        self.blurhash_state = BlurhashState::Playing;
        self.playing_since = Some(Instant::now());
    }

    pub fn mute_playback(&mut self, cx: &mut Cx) {
        self.view.video(cx, ids!(video_surface)).mute_playback(cx);
    }

    pub fn unmute_playback(&mut self, cx: &mut Cx) {
        self.view.video(cx, ids!(video_surface)).unmute_playback(cx);
    }

    pub fn is_playing(&self, cx: &mut Cx) -> bool {
        self.view.video(cx, ids!(video_surface)).is_playing()
    }

    pub fn current_position_ms(&self) -> u64 {
        self.playing_since
            .map(|started| {
                self.current_position_ms
                    .saturating_add(started.elapsed().as_millis() as u64)
            })
            .unwrap_or(self.current_position_ms)
    }

    pub fn seek_to(&mut self, cx: &mut Cx, position_ms: u64) {
        self.current_position_ms = position_ms;
        if self.playing_since.is_some() {
            self.playing_since = Some(Instant::now());
        }
        let _ = cx;
        // Makepad's public VideoRef API currently does not expose an absolute seek method.
        // Keep Robrix's position state in sync so the deferred modal seek path has a
        // single call site; when the upstream API exposes seek, it belongs here.
    }

    pub fn set_source_url(&mut self, cx: &mut Cx, path: PathBuf) {
        if self.source_url.as_ref() == Some(&path) {
            return;
        }
        self.view
            .video(cx, ids!(video_surface))
            .stop_and_cleanup_resources(cx);
        self.source_url = Some(path.clone());
        self.blurhash_state = BlurhashState::NotYetStarted;
        let video = self.view.video(cx, ids!(video_surface));
        video.set_source(VideoDataSource::Filesystem {
            path: path.to_string_lossy().into_owned(),
        });
        video.should_dispatch_texture_updates(true);
    }

    pub fn set_blurhash(&mut self, cx: &mut Cx, blurhash: Option<String>) {
        self.blurhash = blurhash;
    }

    pub fn set_poster_texture(&mut self, cx: &mut Cx, texture: Texture) {
        let video_surface = self.view
            .video(cx, ids!(video_surface));
        video_surface.set_thumbnail_texture(cx, Some(texture.clone()));
        video_surface.show_thumbnail(cx, true);
        self.poster_texture = Some(texture);
    }

    pub fn set_blurhash_texture(&mut self, cx: &mut Cx, texture: Texture) {
        let video_surface = self.view
            .video(cx, ids!(video_surface));
        video_surface.set_thumbnail_texture(cx, Some(texture));
        video_surface.show_thumbnail(cx, true);
    }

    pub fn set_poster_to_solid_color(&mut self, cx: &mut Cx, color: [u8; 4]) {
        if let Ok(buffer) = ImageBuffer::new(&color, 1, 1) {
            let texture = buffer.into_new_texture(cx);
            self.set_poster_texture(cx, texture);
        }
    }

    fn update_current_position(&mut self) {
        if let Some(started) = self.playing_since {
            self.current_position_ms = self
                .current_position_ms
                .saturating_add(started.elapsed().as_millis() as u64);
        }
    }
}

impl RobrixVideoRef {
    pub fn begin_playback(&self, cx: &mut Cx) {
        if let Some(mut inner) = self.borrow_mut() {
            inner.begin_playback(cx);
        }
    }

    pub fn stop_and_cleanup_resources(&self, cx: &mut Cx) {
        if let Some(mut inner) = self.borrow_mut() {
            inner.stop_and_cleanup_resources(cx);
        }
    }

    pub fn pause_playback(&self, cx: &mut Cx) {
        if let Some(mut inner) = self.borrow_mut() {
            inner.pause_playback(cx);
        }
    }

    pub fn resume_playback(&self, cx: &mut Cx) {
        if let Some(mut inner) = self.borrow_mut() {
            inner.resume_playback(cx);
        }
    }

    pub fn mute_playback(&self, cx: &mut Cx) {
        if let Some(mut inner) = self.borrow_mut() {
            inner.mute_playback(cx);
        }
    }

    pub fn unmute_playback(&self, cx: &mut Cx) {
        if let Some(mut inner) = self.borrow_mut() {
            inner.unmute_playback(cx);
        }
    }

    pub fn is_playing(&self, cx: &mut Cx) -> bool {
        self.borrow().is_some_and(|inner| inner.is_playing(cx))
    }

    pub fn current_position_ms(&self) -> u64 {
        self.borrow()
            .map(|inner| inner.current_position_ms())
            .unwrap_or_default()
    }

    pub fn seek_to(&self, cx: &mut Cx, position_ms: u64) {
        if let Some(mut inner) = self.borrow_mut() {
            inner.seek_to(cx, position_ms);
        }
    }

    pub fn set_source_url(&self, cx: &mut Cx, path: PathBuf) {
        if let Some(mut inner) = self.borrow_mut() {
            inner.set_source_url(cx, path);
        }
    }

    pub fn set_blurhash(&self, cx: &mut Cx, blurhash: Option<String>) {
        if let Some(mut inner) = self.borrow_mut() {
            inner.set_blurhash(cx, blurhash);
        }
    }

    pub fn set_poster_texture(&self, cx: &mut Cx, texture: Texture) {
        if let Some(mut inner) = self.borrow_mut() {
            inner.set_poster_texture(cx, texture);
        }
    }

    pub fn set_blurhash_texture(&self, cx: &mut Cx, texture: Texture) {
        if let Some(mut inner) = self.borrow_mut() {
            inner.set_blurhash_texture(cx, texture);
        }
    }

    pub fn set_poster_to_solid_color(&self, cx: &mut Cx, color: [u8; 4]) {
        if let Some(mut inner) = self.borrow_mut() {
            inner.set_poster_to_solid_color(cx, color);
        }
    }
}

#[cfg(test)]
mod tests_robrix_video {
    use super::*;

    #[test]
    fn test_cap_blurhash_dimensions_no_op_when_below_cap() {
        assert_eq!(cap_blurhash_dimensions(320, 240, 500), (320, 240));
    }

    #[test]
    fn test_cap_blurhash_dimensions_caps_height() {
        assert_eq!(cap_blurhash_dimensions(1920, 1080, 500), (888, 500));
    }

    #[test]
    fn test_cap_blurhash_dimensions_caps_width() {
        assert_eq!(cap_blurhash_dimensions(2000, 800, 500), (500, 200));
    }

    #[test]
    fn test_cap_blurhash_dimensions_zero_returns_zero() {
        assert_eq!(cap_blurhash_dimensions(0, 480, 500), (0, 0));
    }

    #[test]
    fn test_decode_blurhash_to_rgba_valid() {
        let buf = decode_blurhash_to_rgba("LEHV6nWB2yk8pyo0adR*.7kCMdnj", 32, 18)
            .expect("valid blurhash should decode");
        assert_eq!(buf.len(), 32 * 18 * 4);
    }

    #[test]
    fn test_decode_blurhash_to_rgba_empty_returns_none() {
        assert_eq!(decode_blurhash_to_rgba("", 32, 18), None);
    }

    #[test]
    fn test_decode_blurhash_to_rgba_malformed_returns_none() {
        assert_eq!(decode_blurhash_to_rgba("not a real blurhash", 32, 18), None);
    }

    #[test]
    fn test_decode_blurhash_to_rgba_zero_width_returns_none() {
        assert_eq!(
            decode_blurhash_to_rgba("LEHV6nWB2yk8pyo0adR*.7kCMdnj", 0, 18),
            None
        );
    }

    #[test]
    fn test_placeholder_fallback_color_value() {
        assert_eq!(placeholder_fallback_color(), [0x22, 0x22, 0x22, 0xFF]);
    }

    #[test]
    fn test_should_show_blurhash_before_first_frame() {
        assert!(should_show_blurhash(BlurhashState::NoSource));
        assert!(should_show_blurhash(BlurhashState::NotYetStarted));
        assert!(should_show_blurhash(BlurhashState::AwaitingFirstFrame));
    }

    #[test]
    fn test_should_show_blurhash_false_while_playing() {
        assert!(!should_show_blurhash(BlurhashState::Playing));
    }
}
