//! Parallel AVFoundation camera capture for the Robot tab's inference path.
//!
//! Why this exists: Makepad's `cx.camera_frame_input(0, …)` callback on macOS
//! only fires when the camera delivers a known planar layout (NV12 / I420 /
//! YUY2). OBS Virtual Camera's CMIO DAL plugin advertises only
//! `kCVPixelFormatType_32BGRA`, which Makepad's dispatcher treats as
//! `CameraFrameLayout::Unknown` and silently drops. To still drive the
//! `frame_analyzer` skin-color heuristic when OBS is the active camera, we
//! open a *second* `AVCaptureSession` against the default video device,
//! explicitly request BGRA output, swizzle to RGBA on the dispatch queue, and
//! push the result into a 1-slot crossbeam channel.
//!
//! Makepad's own Video widget continues to drive the display in parallel —
//! macOS allows multiple `AVCaptureSession` instances to share a device, so
//! this doesn't disturb the existing preview.
//!
//! Scope: macOS only. On other platforms the original
//! `gesture_control::camera_capture::CameraCapture` is the right path.

#![cfg(target_os = "macos")]

use std::os::raw::{c_char, c_void};
use std::sync::Mutex;

use crossbeam_channel::{Receiver, Sender, TrySendError, bounded};
use makepad_widgets::log;
use objc::declare::ClassDecl;
use objc::runtime::{BOOL, Class, Object, Sel, YES};
use objc::{class, msg_send, sel, sel_impl};

use crate::shared::webrtc_video::WebRtcVideoFrame;

type Id = *mut Object;

/// Bridge between the static delegate callback (which has no `&self` we can
/// stash state on) and the active `AvfCapture` instance's sender.
static FRAME_TX_GLOBAL: Mutex<Option<Sender<WebRtcVideoFrame>>> = Mutex::new(None);

/// Handle to a running parallel AVFoundation capture session. Drop to stop.
pub struct AvfCapture {
    session: Id,
    queue: *mut c_void,
    frame_rx: Receiver<WebRtcVideoFrame>,
}

// AVCaptureSession is meant to be operated from any thread per Apple docs;
// we only call start/stopRunning on it from the owning thread, and the
// delegate callback dispatches via the global Mutex.
unsafe impl Send for AvfCapture {}

impl AvfCapture {
    /// Start a parallel BGRA capture against the system's default video
    /// device.
    pub fn start() -> anyhow::Result<Self> {
        let (frame_tx, frame_rx) = bounded::<WebRtcVideoFrame>(1);
        *FRAME_TX_GLOBAL.lock().unwrap() = Some(frame_tx);

        let session = unsafe { build_session() }?;
        let queue = unsafe { create_serial_queue("rs.robius.robrix.gesture_capture") };
        let output = unsafe { build_video_data_output(queue) }?;
        unsafe { attach_output(session, output) }?;

        unsafe {
            let _: () = msg_send![session, commitConfiguration];
            let _: () = msg_send![session, startRunning];
        }
        log!("AvfCapture: AVCaptureSession running");

        Ok(Self {
            session,
            queue,
            frame_rx,
        })
    }

    pub fn try_recv(&self) -> Option<WebRtcVideoFrame> {
        self.frame_rx.try_recv().ok()
    }
}

impl Drop for AvfCapture {
    fn drop(&mut self) {
        *FRAME_TX_GLOBAL.lock().unwrap() = None;
        unsafe {
            let _: () = msg_send![self.session, stopRunning];
            let _: () = msg_send![self.session, release];
            if !self.queue.is_null() {
                dispatch_release(self.queue);
            }
        }
        log!("AvfCapture: session stopped");
    }
}

// ───────────────────────── AVFoundation FFI glue ────────────────────────

unsafe fn build_session() -> anyhow::Result<Id> {
    let video_media_type: Id = unsafe {
        msg_send![class!(NSString), stringWithUTF8String: c"vide".as_ptr()]
    };
    let device: Id = unsafe {
        msg_send![
            class!(AVCaptureDevice),
            defaultDeviceWithMediaType: video_media_type
        ]
    };
    if device.is_null() {
        return Err(anyhow::anyhow!("no default video device"));
    }
    let device_name: Id = unsafe { msg_send![device, localizedName] };
    let device_name_c: *const c_char = unsafe { msg_send![device_name, UTF8String] };
    if !device_name_c.is_null() {
        let cstr = unsafe { std::ffi::CStr::from_ptr(device_name_c) };
        log!("AvfCapture: opening device '{}'", cstr.to_string_lossy());
    }

    let mut error: Id = std::ptr::null_mut();
    let input: Id = unsafe {
        msg_send![
            class!(AVCaptureDeviceInput),
            deviceInputWithDevice: device error: &mut error
        ]
    };
    if input.is_null() {
        return Err(anyhow::anyhow!("AVCaptureDeviceInput::deviceInputWithDevice failed"));
    }

    let session: Id = unsafe { msg_send![class!(AVCaptureSession), new] };
    unsafe {
        let _: () = msg_send![session, beginConfiguration];
    }

    let can_add_input: BOOL = unsafe { msg_send![session, canAddInput: input] };
    if can_add_input != YES {
        unsafe {
            let _: () = msg_send![session, release];
        }
        return Err(anyhow::anyhow!("session refused input"));
    }
    unsafe {
        let _: () = msg_send![session, addInput: input];
    }
    Ok(session)
}

unsafe fn build_video_data_output(queue: *mut c_void) -> anyhow::Result<Id> {
    let output: Id = unsafe { msg_send![class!(AVCaptureVideoDataOutput), new] };

    // videoSettings = @{ (id)kCVPixelBufferPixelFormatTypeKey: @(kCVPixelFormatType_32BGRA) }
    let key: Id = unsafe {
        msg_send![
            class!(NSString),
            stringWithUTF8String: c"PixelFormatType".as_ptr()
        ]
    };
    let value: Id = unsafe { msg_send![class!(NSNumber), numberWithUnsignedInt: 0x42475241u32] };
    let mut keys = [key];
    let mut values = [value];
    let settings: Id = unsafe {
        msg_send![
            class!(NSDictionary),
            dictionaryWithObjects: values.as_mut_ptr()
            forKeys: keys.as_mut_ptr()
            count: 1usize
        ]
    };
    unsafe {
        let _: () = msg_send![output, setVideoSettings: settings];
        let _: () = msg_send![output, setAlwaysDiscardsLateVideoFrames: YES];
    }

    let delegate_class = unsafe { ensure_delegate_class() };
    let delegate: Id = unsafe { msg_send![delegate_class, new] };
    unsafe {
        let _: () = msg_send![output, setSampleBufferDelegate: delegate queue: queue];
    }
    Ok(output)
}

unsafe fn attach_output(session: Id, output: Id) -> anyhow::Result<()> {
    let can_add: BOOL = unsafe { msg_send![session, canAddOutput: output] };
    if can_add != YES {
        return Err(anyhow::anyhow!("session refused output"));
    }
    unsafe {
        let _: () = msg_send![session, addOutput: output];
    }
    Ok(())
}

// ──────────────────────────── delegate class ────────────────────────────

unsafe fn ensure_delegate_class() -> &'static Class {
    use std::sync::Once;
    static INIT: Once = Once::new();
    static mut CLASS: Option<&'static Class> = None;

    INIT.call_once(|| {
        let superclass = class!(NSObject);
        let mut decl =
            ClassDecl::new("RobrixGestureSampleBufferDelegate", superclass).unwrap();
        unsafe {
            decl.add_method(
                sel!(captureOutput:didOutputSampleBuffer:fromConnection:),
                capture_output
                    as extern "C" fn(&Object, Sel, Id, *mut c_void, Id),
            );
            CLASS = Some(decl.register());
        }
    });
    // SAFETY: `CLASS` is written exactly once inside Once::call_once before
    // any read, and is read-only afterwards.
    unsafe { CLASS.unwrap() }
}

extern "C" fn capture_output(
    _this: &Object,
    _cmd: Sel,
    _output: Id,
    sample_buffer: *mut c_void,
    _connection: Id,
) {
    if sample_buffer.is_null() {
        return;
    }
    let frame = unsafe { sample_buffer_to_rgba(sample_buffer) };
    if let Some(frame) = frame {
        if let Ok(guard) = FRAME_TX_GLOBAL.lock() {
            if let Some(tx) = guard.as_ref() {
                match tx.try_send(frame) {
                    Ok(()) | Err(TrySendError::Full(_)) | Err(TrySendError::Disconnected(_)) => {}
                }
            }
        }
    }
}

// ─────────────────── CVPixelBuffer → RGBA conversion ────────────────────

unsafe fn sample_buffer_to_rgba(sample_buffer: *mut c_void) -> Option<WebRtcVideoFrame> {
    let image_buffer = unsafe { CMSampleBufferGetImageBuffer(sample_buffer) };
    if image_buffer.is_null() {
        return None;
    }
    if unsafe { CVPixelBufferLockBaseAddress(image_buffer, 1) } != 0 {
        return None;
    }

    let width = unsafe { CVPixelBufferGetWidth(image_buffer) };
    let height = unsafe { CVPixelBufferGetHeight(image_buffer) };
    let bytes_per_row = unsafe { CVPixelBufferGetBytesPerRow(image_buffer) };
    let base = unsafe { CVPixelBufferGetBaseAddress(image_buffer) };

    let result = if width > 0 && height > 0 && !base.is_null() && bytes_per_row >= width * 4 {
        let src = unsafe { std::slice::from_raw_parts(base as *const u8, bytes_per_row * height) };
        let mut rgba = vec![0u8; width * height * 4];
        for row in 0..height {
            let src_row_off = row * bytes_per_row;
            let dst_row_off = row * width * 4;
            for col in 0..width {
                let s = src_row_off + col * 4;
                let d = dst_row_off + col * 4;
                // Source is BGRA. WebRtcVideo's shader reads input bytes as
                // R G B A so we swap channels 0 and 2.
                rgba[d] = src[s + 2];
                rgba[d + 1] = src[s + 1];
                rgba[d + 2] = src[s];
                rgba[d + 3] = 255;
            }
        }
        Some(WebRtcVideoFrame {
            data: rgba,
            width: width as u32,
            height: height as u32,
            participant_id: None,
        })
    } else {
        None
    };

    unsafe {
        CVPixelBufferUnlockBaseAddress(image_buffer, 1);
    }
    result
}

// ───────────────────────── extern declarations ──────────────────────────

#[link(name = "CoreMedia", kind = "framework")]
unsafe extern "C" {
    fn CMSampleBufferGetImageBuffer(buffer: *mut c_void) -> *mut c_void;
}

#[link(name = "CoreVideo", kind = "framework")]
unsafe extern "C" {
    fn CVPixelBufferLockBaseAddress(buffer: *mut c_void, lock_flags: u64) -> i32;
    fn CVPixelBufferUnlockBaseAddress(buffer: *mut c_void, lock_flags: u64) -> i32;
    fn CVPixelBufferGetWidth(buffer: *mut c_void) -> usize;
    fn CVPixelBufferGetHeight(buffer: *mut c_void) -> usize;
    fn CVPixelBufferGetBytesPerRow(buffer: *mut c_void) -> usize;
    fn CVPixelBufferGetBaseAddress(buffer: *mut c_void) -> *const c_void;
}

#[link(name = "System", kind = "framework")]
unsafe extern "C" {
    fn dispatch_queue_create(label: *const c_char, attr: *mut c_void) -> *mut c_void;
    fn dispatch_release(object: *mut c_void);
}

unsafe fn create_serial_queue(label: &str) -> *mut c_void {
    let c_label = std::ffi::CString::new(label).unwrap();
    unsafe { dispatch_queue_create(c_label.as_ptr(), std::ptr::null_mut()) }
}
