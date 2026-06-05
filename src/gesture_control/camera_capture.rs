//! Camera frame capture via Makepad's native `cx.camera_frame_input` callback.
//!
//! Makepad's AV capture (on macOS / Linux / Android) delivers planar camera
//! frames to a `CameraFrameInputFn` we register via
//! [`makepad_widgets::Cx::camera_frame_input`]. We convert each frame's
//! native pixel layout (NV12 / I420 / YUY2) into RGBA on the camera thread,
//! then push the result into a single-slot crossbeam channel. The UI thread
//! reads from the channel each `Event::NextFrame` and (a) forwards the frame
//! to the on-screen `WebRtcVideo` widget for display, and (b) caches it for
//! the "Infer gesture" button to feed `frame_analyzer::classify_frame`.
//!
//! This replaces the previous `nokhwa`-based path which crashed on macOS
//! because nokhwa 0.10's AVFoundation backend bails out during format
//! enumeration if the camera advertises `kCVPixelFormatType_32BGRA`
//! (FourCharCode `'BGRA' = 0x42475241`). Makepad's own backend handles BGRA
//! and the other AVFoundation formats fine — it was already opening this
//! exact camera for the VoIP lobby's Native preview.

use crossbeam_channel::{Receiver, Sender, TrySendError, bounded};
use makepad_widgets::makepad_platform::CxMediaApi;
use makepad_widgets::makepad_platform::video::{
    CameraColorMatrix, CameraFrameLayout, CameraFrameRef,
};
use makepad_widgets::{Cx, log};

use crate::shared::webrtc_video::WebRtcVideoFrame;

/// Handle to the camera-frame stream registration. Dropping it releases the
/// `Receiver`; the camera callback closure stays registered with Makepad but
/// its `try_send` calls fail silently, so it becomes effectively a no-op until
/// `CameraCapture::start` is called again (which installs a fresh closure).
pub struct CameraCapture {
    frame_rx: Receiver<WebRtcVideoFrame>,
}

impl CameraCapture {
    /// Register a frame callback for camera slot 0.
    ///
    /// Note: the camera must already be opened and streaming for frames to
    /// arrive. In this codebase that's done by either the VoIP lobby's
    /// `Video` widget or the Robot tab's hidden `Video` widget calling
    /// `begin_playback` — Makepad's AV capture starts the camera the first
    /// time a Video widget asks for it.
    pub fn start(cx: &mut Cx) -> Self {
        let (frame_tx, frame_rx) = bounded::<WebRtcVideoFrame>(1);
        let mut tx_holder = FrameSender { tx: frame_tx };
        cx.camera_frame_input(0, move |frame: CameraFrameRef| {
            if let Some(rgba) = convert_to_rgba(&frame) {
                tx_holder.try_send_drop(rgba);
            }
        });
        Self { frame_rx }
    }

    /// Try to receive the latest frame. Returns `None` if no new frame is
    /// available — the channel only holds the most recent.
    pub fn try_recv(&self) -> Option<WebRtcVideoFrame> {
        self.frame_rx.try_recv().ok()
    }
}

/// Wraps the sender so we can give it FnMut behaviour inside the camera
/// callback without leaking the channel-full case as visible errors.
struct FrameSender {
    tx: Sender<WebRtcVideoFrame>,
}

impl FrameSender {
    fn try_send_drop(&mut self, f: WebRtcVideoFrame) {
        match self.tx.try_send(f) {
            Ok(()) => {}
            // Channel full: drop newest. UI hasn't consumed the last frame yet.
            Err(TrySendError::Full(_)) => {}
            // Receiver dropped (CameraCapture was dropped): we'll keep being
            // called by Makepad but every send fails. That's fine — nothing
            // to do until a new CameraCapture installs a fresh closure.
            Err(TrySendError::Disconnected(_)) => {}
        }
    }
}

// ─────────────────────────── pixel conversion ────────────────────────────

fn convert_to_rgba(frame: &CameraFrameRef) -> Option<WebRtcVideoFrame> {
    if frame.width == 0 || frame.height == 0 {
        return None;
    }
    match frame.layout {
        CameraFrameLayout::NV12 => convert_nv12(frame),
        CameraFrameLayout::I420 => convert_i420(frame),
        CameraFrameLayout::YUY2 => convert_yuy2(frame),
        CameraFrameLayout::Mjpeg => {
            // MJPEG would need libjpeg/turbojpeg — out of scope for the
            // first iteration. On macOS the AV capture typically delivers
            // NV12 so we shouldn't hit this path.
            log!("camera_capture: MJPEG frame received — decoding not implemented");
            None
        }
        CameraFrameLayout::Unknown => None,
    }
}

/// BT.601 vs BT.709 coefficient pair, scaled by 256 for integer math.
struct YuvCoeffs {
    r_v: i32,
    g_u: i32,
    g_v: i32,
    b_u: i32,
}

const BT709: YuvCoeffs = YuvCoeffs { r_v: 459, g_u: 55,  g_v: 137, b_u: 541 };
const BT601: YuvCoeffs = YuvCoeffs { r_v: 409, g_u: 100, g_v: 208, b_u: 516 };

fn coeffs_for(matrix: CameraColorMatrix) -> &'static YuvCoeffs {
    match matrix {
        CameraColorMatrix::BT709 | CameraColorMatrix::BT2020 => &BT709,
        CameraColorMatrix::BT601 | CameraColorMatrix::Unknown => &BT601,
    }
}

#[inline(always)]
fn yuv_to_rgb(y: u8, u: u8, v: u8, c: &YuvCoeffs) -> (u8, u8, u8) {
    let yc = (y as i32 - 16) * 298;
    let uc = u as i32 - 128;
    let vc = v as i32 - 128;
    let r = ((yc + c.r_v * vc + 128) >> 8).clamp(0, 255) as u8;
    let g = ((yc - c.g_u * uc - c.g_v * vc + 128) >> 8).clamp(0, 255) as u8;
    let b = ((yc + c.b_u * uc + 128) >> 8).clamp(0, 255) as u8;
    (r, g, b)
}

fn convert_nv12(frame: &CameraFrameRef) -> Option<WebRtcVideoFrame> {
    if frame.plane_count < 2 {
        return None;
    }
    let w = frame.width;
    let h = frame.height;
    let y_plane = frame.planes[0];
    let uv_plane = frame.planes[1];
    if y_plane.bytes.len() < y_plane.row_stride * h
        || uv_plane.bytes.len() < uv_plane.row_stride * (h / 2)
    {
        return None;
    }
    let c = coeffs_for(frame.matrix);
    let mut rgba = vec![0u8; w * h * 4];
    for row in 0..h {
        let y_off = row * y_plane.row_stride;
        let uv_row = row / 2;
        let uv_off = uv_row * uv_plane.row_stride;
        let out_off = row * w * 4;
        for col in 0..w {
            let y_val = y_plane.bytes[y_off + col];
            let uv_col = col & !1;
            let u_val = uv_plane.bytes[uv_off + uv_col];
            let v_val = uv_plane.bytes[uv_off + uv_col + 1];
            let (r, g, b) = yuv_to_rgb(y_val, u_val, v_val, c);
            let i = out_off + col * 4;
            rgba[i] = r;
            rgba[i + 1] = g;
            rgba[i + 2] = b;
            rgba[i + 3] = 255;
        }
    }
    Some(WebRtcVideoFrame {
        data: rgba,
        width: w as u32,
        height: h as u32,
        participant_id: None,
    })
}

fn convert_i420(frame: &CameraFrameRef) -> Option<WebRtcVideoFrame> {
    if frame.plane_count < 3 {
        return None;
    }
    let w = frame.width;
    let h = frame.height;
    let y_p = frame.planes[0];
    let u_p = frame.planes[1];
    let v_p = frame.planes[2];
    let c = coeffs_for(frame.matrix);
    let mut rgba = vec![0u8; w * h * 4];
    for row in 0..h {
        let y_off = row * y_p.row_stride;
        let uv_row = row / 2;
        let u_off = uv_row * u_p.row_stride;
        let v_off = uv_row * v_p.row_stride;
        let out_off = row * w * 4;
        for col in 0..w {
            let y_val = y_p.bytes[y_off + col];
            let uv_col = col / 2;
            let u_val = u_p.bytes[u_off + uv_col];
            let v_val = v_p.bytes[v_off + uv_col];
            let (r, g, b) = yuv_to_rgb(y_val, u_val, v_val, c);
            let i = out_off + col * 4;
            rgba[i] = r;
            rgba[i + 1] = g;
            rgba[i + 2] = b;
            rgba[i + 3] = 255;
        }
    }
    Some(WebRtcVideoFrame {
        data: rgba,
        width: w as u32,
        height: h as u32,
        participant_id: None,
    })
}

fn convert_yuy2(frame: &CameraFrameRef) -> Option<WebRtcVideoFrame> {
    if frame.plane_count < 1 {
        return None;
    }
    let w = frame.width;
    let h = frame.height;
    let p = frame.planes[0];
    // YUYV: 2 pixels per 4 bytes (Y0 U Y1 V).
    let c = coeffs_for(frame.matrix);
    let mut rgba = vec![0u8; w * h * 4];
    for row in 0..h {
        let row_off = row * p.row_stride;
        let out_off = row * w * 4;
        let mut col = 0;
        while col < w {
            let i = row_off + col * 2;
            if i + 3 >= p.bytes.len() {
                break;
            }
            let y0 = p.bytes[i];
            let u = p.bytes[i + 1];
            let y1 = p.bytes[i + 2];
            let v = p.bytes[i + 3];
            let (r0, g0, b0) = yuv_to_rgb(y0, u, v, c);
            let (r1, g1, b1) = yuv_to_rgb(y1, u, v, c);
            let o0 = out_off + col * 4;
            rgba[o0] = r0;
            rgba[o0 + 1] = g0;
            rgba[o0 + 2] = b0;
            rgba[o0 + 3] = 255;
            let o1 = o0 + 4;
            rgba[o1] = r1;
            rgba[o1 + 1] = g1;
            rgba[o1 + 2] = b1;
            rgba[o1 + 3] = 255;
            col += 2;
        }
    }
    Some(WebRtcVideoFrame {
        data: rgba,
        width: w as u32,
        height: h as u32,
        participant_id: None,
    })
}
