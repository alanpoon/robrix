//! Camera handling and video management

use makepad_widgets::*;
use makepad_widgets::makepad_platform::video::{VideoInputId, VideoFormatId, VideoInputsEvent, VideoPixelFormat};
use makepad_widgets::video::VideoCameraPreviewMode;

/// Camera format choice
#[derive(Clone)]
pub struct CameraChoice {
    pub input_id: VideoInputId,
    pub format_id: VideoFormatId,
    pub name: String,
    pub width: usize,
    pub height: usize,
    pub pixel_format: VideoPixelFormat,
}

/// Camera manager handles camera selection and video playback
pub struct CameraManager;

impl CameraManager {
    /// Pick the best camera format from available options.
    ///
    /// Prefers the front-facing camera (selfie) by name — Android's
    /// Camera2 NDK exposes devices named "Front Camera" / "Back Camera",
    /// and the front one is what both the VoIP lobby (video call self-view)
    /// and the Robot tab (gesture recognition of the user's hand in front
    /// of the screen) want. We match on case-insensitive substring so
    /// other platforms' naming variants ("FaceTime HD Camera", "User
    /// Facing Camera", etc.) get a fair shot too — falls back to the first
    /// enumerated device if no front match exists (e.g. external webcam,
    /// OBS Virtual Camera).
    pub fn pick_camera_choice(ev: &VideoInputsEvent) -> Option<CameraChoice> {
        fn looks_like_front(name: &str) -> bool {
            let n = name.to_ascii_lowercase();
            n.contains("front")
                || n.contains("user")
                || n.contains("facetime")
                || n.contains("selfie")
        }
        let desc = ev
            .descs
            .iter()
            .find(|d| looks_like_front(&d.name))
            .or_else(|| ev.descs.first())?;

        fn pixel_rank(pixel_format: VideoPixelFormat) -> usize {
            match pixel_format {
                VideoPixelFormat::NV12 => 4,
                VideoPixelFormat::YUY2 => 3,
                VideoPixelFormat::YUV420 => 2,
                VideoPixelFormat::RGB24 => 1,
                _ => 0,
            }
        }

        fn better(
            a: &makepad_widgets::makepad_platform::video::VideoFormat,
            b: &makepad_widgets::makepad_platform::video::VideoFormat,
        ) -> bool {
            let a_rank = pixel_rank(a.pixel_format);
            let b_rank = pixel_rank(b.pixel_format);
            if a_rank != b_rank {
                return a_rank > b_rank;
            }
            let a_pixels = a.width * a.height;
            let b_pixels = b.width * b.height;
            if a_pixels != b_pixels {
                return a_pixels > b_pixels;
            }
            let a_fps = a.frame_rate.unwrap_or(0.0);
            let b_fps = b.frame_rate.unwrap_or(0.0);
            a_fps > b_fps
        }

        let mut best: Option<makepad_widgets::makepad_platform::video::VideoFormat> = None;
        for fmt in &desc.formats {
            if best.as_ref().is_none_or(|b| better(fmt, b)) {
                best = Some(*fmt);
            }
        }
        let format = best?;

        if pixel_rank(format.pixel_format) == 0 {
            log!(
                "Camera: NOTE — selected format on '{}' is {:?}, which Makepad's \
                 frame callback does not dispatch. Display via Native preview \
                 still works; the Robot tab's Infer button will report \
                 \"(no frame yet)\" until BGRA capture is wired in.",
                desc.name, format.pixel_format
            );
        }
        log!(
            "Camera: selected '{}' {}x{} {:?}",
            desc.name, format.width, format.height, format.pixel_format
        );

        Some(CameraChoice {
            input_id: desc.input_id,
            format_id: format.format_id,
            name: desc.name.clone(),
            width: format.width,
            height: format.height,
            pixel_format: format.pixel_format,
        })
    }

    /// Start camera for lobby preview
    pub fn start_lobby_camera(ui: &View, cx: &mut Cx, choice: &CameraChoice) -> bool {
        let video = ui.video(cx, &[live_id!(lobby_camera_video)]);

        if !video.is_unprepared() {
            return false;
        }

        log!("Starting lobby camera: {} ({}x{} {:?})",
            choice.name, choice.width, choice.height, choice.pixel_format);

        ui.view(cx, ids!(lobby_video_host)).set_visible(cx, true);
        ui.view(cx, ids!(lobby_camera_placeholder)).set_visible(cx, false);

        video.set_camera_preview_mode(cx, VideoCameraPreviewMode::Native);
        video.set_source_camera(cx, choice.input_id, choice.format_id);
        video.begin_playback(cx);
        true
    }

    /// Start camera for in-call video
    pub fn start_call_camera(ui: &View, cx: &mut Cx, choice: &CameraChoice) -> bool {
        let video = ui.video(cx, &[live_id!(local_camera_video)]);

        if !video.is_unprepared() {
            return false;
        }

        log!("Starting call camera...");

        ui.view(cx, ids!(local_video_host)).set_visible(cx, true);
        ui.view(cx, ids!(local_avatar_view)).set_visible(cx, false);

        video.set_camera_preview_mode(cx, VideoCameraPreviewMode::Native);
        video.set_source_camera(cx, choice.input_id, choice.format_id);
        video.begin_playback(cx);
        true
    }

    /// Stop lobby camera
    pub fn stop_lobby_camera(ui: &View, cx: &mut Cx) {
        let video = ui.video(cx, &[live_id!(lobby_camera_video)]);
        if !video.is_unprepared() && !video.is_cleaning_up() {
            video.stop_and_cleanup_resources(cx);
        }
        ui.view(cx, ids!(lobby_video_host)).set_visible(cx, false);
        ui.view(cx, ids!(lobby_camera_placeholder)).set_visible(cx, true);
        ui.view(cx, ids!(join_call_button_view)).set_visible(cx, true);
    }

    /// Stop in-call camera
    pub fn stop_call_camera(ui: &View, cx: &mut Cx) {
        let video = ui.video(cx, &[live_id!(local_camera_video)]);
        if !video.is_unprepared() && !video.is_cleaning_up() {
            video.stop_and_cleanup_resources(cx);
        }
        ui.view(cx, ids!(local_video_host)).set_visible(cx, false);
        ui.view(cx, ids!(local_avatar_view)).set_visible(cx, true);
    }

    /// Show video view for lobby
    pub fn show_lobby_video(ui: &View, cx: &mut Cx) {
        ui.view(cx, ids!(lobby_video_host)).set_visible(cx, true);
        ui.view(cx, ids!(lobby_camera_placeholder)).set_visible(cx, false);
    }

    /// Show video view for call
    pub fn show_call_video(ui: &View, cx: &mut Cx) {
        ui.view(cx, ids!(local_video_host)).set_visible(cx, true);
        ui.view(cx, ids!(local_avatar_view)).set_visible(cx, false);
    }
}
