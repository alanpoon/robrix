//! Parallel Camera2 NDK capture for the Robot tab on Android.
//!
//! Why this exists: Makepad's `cx.camera_frame_input(...)` path on Android
//! is wired so that a *plain* registration is invisible to the dispatch
//! table unless `slot_streams[index]` was previously bound by
//! `use_video_input`. Even after binding, Makepad rebuilds the Camera2
//! session whenever the Video widget attaches its preview surface, which
//! caused rapid back-to-back session restarts and `device error 4` on
//! the user's device (logcat 2026-06-09 13:50:06).
//!
//! Solution: open a *parallel* Camera2 session via the NDK directly. The
//! same convention the macOS port uses (`avf_capture.rs` opens a parallel
//! `AVCaptureSession`). The Video widget's session keeps rendering frames
//! to its SurfaceView; this one delivers `YUV_420_888` images into an
//! `AImageReader` whose listener converts them to RGBA and pushes them
//! through a single-slot crossbeam channel.
//!
//! Lifecycle:
//! 1. `AcameraCapture::start()` → open front camera → create ImageReader →
//!    create CaptureSession → install ImageReader listener → start
//!    repeating preview request.
//! 2. Each delivered image: listener pulls Y/U/V plane data, builds
//!    a `WebRtcVideoFrame` with packed RGBA bytes, `try_send`s to the
//!    receiver (drops on full so the freshest frame always wins).
//! 3. Drop: stop repeating, close session, close device, free all
//!    NDK objects.
//!
//! The struct is `Send + Sync` in practice — all the heavy state is owned
//! by a `Box<ListenerContext>` that the listener accesses via raw pointer.
//! Lifetime is enforced by `Drop` running in reverse order of resource
//! creation.

#![allow(non_snake_case, non_camel_case_types, dead_code, clippy::missing_safety_doc)]

use std::ffi::{CStr, CString};
use std::os::raw::{c_char, c_int, c_void};
use std::ptr::null_mut;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

use crossbeam_channel::{Receiver, Sender, TrySendError, bounded};
use makepad_widgets::log;

use crate::shared::webrtc_video::WebRtcVideoFrame;

// ───────────────────────────── FFI bindings ─────────────────────────────
//
// Minimal subset of Android's `camera2ndk` + `mediandk` exports that we
// need. Same shape as Makepad's `acamera_sys.rs`; copied here so this
// module is self-contained.

#[repr(C)] pub struct ACameraManager { _p: [u8; 0] }
#[repr(C)] pub struct ACameraDevice { _p: [u8; 0] }
#[repr(C)] pub struct ACameraIdList { numCameras: c_int, cameraIds: *mut *const c_char }
#[repr(C)] pub struct ACameraMetadata { _p: [u8; 0] }
#[repr(C)] pub struct ACameraCaptureSession { _p: [u8; 0] }
#[repr(C)] pub struct ACaptureRequest { _p: [u8; 0] }
#[repr(C)] pub struct ACameraOutputTarget { _p: [u8; 0] }
#[repr(C)] pub struct ACaptureSessionOutput { _p: [u8; 0] }
#[repr(C)] pub struct ACaptureSessionOutputContainer { _p: [u8; 0] }
#[repr(C)] pub struct AImageReader { _p: [u8; 0] }
#[repr(C)] pub struct AImage { _p: [u8; 0] }
#[repr(C)] pub struct ANativeWindow { _p: [u8; 0] }

type camera_status_t = c_int;
type media_status_t = c_int;
const ACAMERA_OK: camera_status_t = 0;
const AMEDIA_OK: media_status_t = 0;

const ACAMERA_LENS_FACING: u32 = 524293;
const ACAMERA_LENS_FACING_FRONT: u8 = 0;
const ACAMERA_LENS_FACING_BACK: u8 = 1;

const AIMAGE_FORMAT_YUV_420_888: i32 = 35;

const TEMPLATE_PREVIEW: c_int = 1;

#[repr(u8)]
#[derive(Copy, Clone)]
enum ACameraMetadataType {
    Byte = 0,
    Int32 = 1,
    Float = 2,
    Int64 = 3,
    Double = 4,
    Rational = 5,
}

#[repr(C)]
struct ACameraMetadata_const_entry {
    tag: u32,
    typ: u8,
    count: u32,
    data: ACameraMetadata_data_union,
}

#[repr(C)]
union ACameraMetadata_data_union {
    u8: *const u8,
    i32: *const i32,
    f32: *const f32,
    i64: *const i64,
    f64: *const f64,
}

#[repr(C)]
struct ACameraDevice_StateCallbacks {
    context: *mut c_void,
    on_disconnected: unsafe extern "C" fn(*mut c_void, *mut ACameraDevice),
    on_error: unsafe extern "C" fn(*mut c_void, *mut ACameraDevice, c_int),
}

#[repr(C)]
struct ACameraCaptureSession_stateCallbacks {
    context: *mut c_void,
    on_closed: unsafe extern "C" fn(*mut c_void, *mut ACameraCaptureSession),
    on_ready: unsafe extern "C" fn(*mut c_void, *mut ACameraCaptureSession),
    on_active: unsafe extern "C" fn(*mut c_void, *mut ACameraCaptureSession),
}

#[repr(C)]
struct AImageReader_ImageListener {
    context: *mut c_void,
    on_image_available: unsafe extern "C" fn(*mut c_void, *mut AImageReader),
}

#[link(name = "camera2ndk")]
unsafe extern "C" {
    fn ACameraManager_create() -> *mut ACameraManager;
    fn ACameraManager_delete(m: *mut ACameraManager);
    fn ACameraManager_getCameraIdList(m: *mut ACameraManager, list: *mut *mut ACameraIdList) -> camera_status_t;
    fn ACameraManager_deleteCameraIdList(list: *mut ACameraIdList);
    fn ACameraManager_getCameraCharacteristics(m: *mut ACameraManager, id: *const c_char, out: *mut *mut ACameraMetadata) -> camera_status_t;
    fn ACameraManager_openCamera(m: *mut ACameraManager, id: *const c_char, cb: *mut ACameraDevice_StateCallbacks, out: *mut *mut ACameraDevice) -> camera_status_t;
    fn ACameraMetadata_getConstEntry(md: *const ACameraMetadata, tag: u32, entry: *mut ACameraMetadata_const_entry) -> camera_status_t;
    fn ACameraMetadata_free(md: *mut ACameraMetadata);

    fn ACameraDevice_close(d: *mut ACameraDevice) -> camera_status_t;
    fn ACameraDevice_createCaptureRequest(d: *const ACameraDevice, t: c_int, req: *mut *mut ACaptureRequest) -> camera_status_t;
    fn ACameraDevice_createCaptureSession(d: *mut ACameraDevice, container: *const ACaptureSessionOutputContainer, cb: *const ACameraCaptureSession_stateCallbacks, out: *mut *mut ACameraCaptureSession) -> camera_status_t;

    fn ACameraCaptureSession_setRepeatingRequest(s: *mut ACameraCaptureSession, cb: *mut c_void, n: c_int, reqs: *mut *mut ACaptureRequest, seq: *mut c_int) -> camera_status_t;
    fn ACameraCaptureSession_stopRepeating(s: *mut ACameraCaptureSession) -> camera_status_t;
    fn ACameraCaptureSession_close(s: *mut ACameraCaptureSession);

    fn ACameraOutputTarget_create(w: *mut ANativeWindow, out: *mut *mut ACameraOutputTarget) -> camera_status_t;
    fn ACameraOutputTarget_free(o: *mut ACameraOutputTarget);

    fn ACaptureRequest_addTarget(r: *mut ACaptureRequest, t: *const ACameraOutputTarget) -> camera_status_t;
    fn ACaptureRequest_free(r: *mut ACaptureRequest);

    fn ACaptureSessionOutput_create(w: *mut ANativeWindow, out: *mut *mut ACaptureSessionOutput) -> camera_status_t;
    fn ACaptureSessionOutput_free(o: *mut ACaptureSessionOutput);

    fn ACaptureSessionOutputContainer_create(out: *mut *mut ACaptureSessionOutputContainer) -> camera_status_t;
    fn ACaptureSessionOutputContainer_add(c: *mut ACaptureSessionOutputContainer, o: *const ACaptureSessionOutput) -> camera_status_t;
    fn ACaptureSessionOutputContainer_free(c: *mut ACaptureSessionOutputContainer);
}

#[link(name = "mediandk")]
unsafe extern "C" {
    fn AImageReader_new(w: c_int, h: c_int, format: i32, maxImages: c_int, reader: *mut *mut AImageReader) -> media_status_t;
    fn AImageReader_delete(reader: *mut AImageReader);
    fn AImageReader_getWindow(reader: *mut AImageReader, window: *mut *mut ANativeWindow) -> media_status_t;
    fn AImageReader_setImageListener(reader: *mut AImageReader, listener: *mut AImageReader_ImageListener) -> media_status_t;
    fn AImageReader_acquireLatestImage(reader: *mut AImageReader, image: *mut *mut AImage) -> media_status_t;

    fn AImage_delete(image: *mut AImage);
    fn AImage_getWidth(image: *const AImage, w: *mut i32) -> media_status_t;
    fn AImage_getHeight(image: *const AImage, h: *mut i32) -> media_status_t;
    fn AImage_getPlaneData(image: *const AImage, plane: c_int, data: *mut *mut u8, length: *mut c_int) -> media_status_t;
    fn AImage_getPlaneRowStride(image: *const AImage, plane: c_int, stride: *mut c_int) -> media_status_t;
    fn AImage_getPlanePixelStride(image: *const AImage, plane: c_int, stride: *mut c_int) -> media_status_t;
}

// ─────────────────────────── Capture state machine ───────────────────────

/// Owned by the `AImageReader` listener callback; lives behind a stable
/// `Box<ListenerContext>` pointer that's also the `context` field of the
/// `AImageReader_ImageListener`.
struct ListenerContext {
    sender: Sender<WebRtcVideoFrame>,
    alive: AtomicBool,
    width: usize,
    height: usize,
    frame_count: AtomicUsize,
}

unsafe extern "C" fn on_image_available(context: *mut c_void, reader: *mut AImageReader) {
    let ctx = unsafe { &*(context as *const ListenerContext) };
    if !ctx.alive.load(Ordering::Relaxed) {
        return;
    }
    let mut image: *mut AImage = null_mut();
    if unsafe { AImageReader_acquireLatestImage(reader, &mut image) } != AMEDIA_OK || image.is_null() {
        return;
    }

    let frame = unsafe { extract_rgba_from_yuv_420_888(image, ctx.width, ctx.height) };
    unsafe { AImage_delete(image) };

    if let Some(rgba) = frame {
        if ctx.frame_count.fetch_add(1, Ordering::Relaxed) == 0 {
            log!(
                "AcameraCapture: first frame delivered to channel — {}x{}",
                ctx.width, ctx.height
            );
        }
        match ctx.sender.try_send(rgba) {
            Ok(_) => {}
            Err(TrySendError::Full(_)) => {} // drop newest; UI thread hasn't drained the last
            Err(TrySendError::Disconnected(_)) => {} // capture dropped; benign
        }
    }
}

unsafe extern "C" fn on_disconnected(_: *mut c_void, _: *mut ACameraDevice) {
    log!("AcameraCapture: camera disconnected");
}
unsafe extern "C" fn on_error(_: *mut c_void, _: *mut ACameraDevice, err: c_int) {
    log!("AcameraCapture: camera device error {err}");
}
unsafe extern "C" fn on_session_closed(_: *mut c_void, _: *mut ACameraCaptureSession) {}
unsafe extern "C" fn on_session_ready(_: *mut c_void, _: *mut ACameraCaptureSession) {}
unsafe extern "C" fn on_session_active(_: *mut c_void, _: *mut ACameraCaptureSession) {}

/// Convert a `YUV_420_888` image into packed RGBA8888. Uses BT.601 limited
/// range — same coefficients `camera_capture.rs::convert_i420` uses for
/// the equivalent macOS / desktop path.
unsafe fn extract_rgba_from_yuv_420_888(
    image: *mut AImage,
    expected_w: usize,
    expected_h: usize,
) -> Option<WebRtcVideoFrame> {
    let mut w: i32 = 0;
    let mut h: i32 = 0;
    if unsafe { AImage_getWidth(image, &mut w) } != AMEDIA_OK
        || unsafe { AImage_getHeight(image, &mut h) } != AMEDIA_OK
        || w <= 0 || h <= 0
    {
        return None;
    }
    let w = w as usize;
    let h = h as usize;
    if w != expected_w || h != expected_h {
        // Camera2 occasionally returns sub-region images. Use the actual
        // dimensions; downstream consumers don't care which axis is which.
    }

    let mut y_data: *mut u8 = null_mut();
    let mut y_len: c_int = 0;
    let mut y_row_stride: c_int = 0;
    if unsafe { AImage_getPlaneData(image, 0, &mut y_data, &mut y_len) } != AMEDIA_OK
        || unsafe { AImage_getPlaneRowStride(image, 0, &mut y_row_stride) } != AMEDIA_OK
        || y_data.is_null()
    {
        return None;
    }

    let mut u_data: *mut u8 = null_mut();
    let mut u_len: c_int = 0;
    let mut u_row_stride: c_int = 0;
    let mut u_pixel_stride: c_int = 0;
    if unsafe { AImage_getPlaneData(image, 1, &mut u_data, &mut u_len) } != AMEDIA_OK
        || unsafe { AImage_getPlaneRowStride(image, 1, &mut u_row_stride) } != AMEDIA_OK
        || unsafe { AImage_getPlanePixelStride(image, 1, &mut u_pixel_stride) } != AMEDIA_OK
        || u_data.is_null()
    {
        return None;
    }

    let mut v_data: *mut u8 = null_mut();
    let mut v_len: c_int = 0;
    let mut v_row_stride: c_int = 0;
    let mut v_pixel_stride: c_int = 0;
    if unsafe { AImage_getPlaneData(image, 2, &mut v_data, &mut v_len) } != AMEDIA_OK
        || unsafe { AImage_getPlaneRowStride(image, 2, &mut v_row_stride) } != AMEDIA_OK
        || unsafe { AImage_getPlanePixelStride(image, 2, &mut v_pixel_stride) } != AMEDIA_OK
        || v_data.is_null()
    {
        return None;
    }

    let y_slice = unsafe { std::slice::from_raw_parts(y_data, y_len.max(0) as usize) };
    let u_slice = unsafe { std::slice::from_raw_parts(u_data, u_len.max(0) as usize) };
    let v_slice = unsafe { std::slice::from_raw_parts(v_data, v_len.max(0) as usize) };

    let mut rgba = vec![0u8; w * h * 4];
    let y_row_stride = y_row_stride.max(0) as usize;
    let u_row_stride = u_row_stride.max(0) as usize;
    let v_row_stride = v_row_stride.max(0) as usize;
    let u_pixel_stride = u_pixel_stride.max(1) as usize;
    let v_pixel_stride = v_pixel_stride.max(1) as usize;

    for row in 0..h {
        let y_off = row * y_row_stride;
        let uv_row = row / 2;
        let u_off = uv_row * u_row_stride;
        let v_off = uv_row * v_row_stride;
        let out_off = row * w * 4;
        for col in 0..w {
            let uv_col = col / 2;
            let y_val = *y_slice.get(y_off + col).unwrap_or(&0);
            let u_val = *u_slice.get(u_off + uv_col * u_pixel_stride).unwrap_or(&128);
            let v_val = *v_slice.get(v_off + uv_col * v_pixel_stride).unwrap_or(&128);

            // BT.601 limited range YUV → RGB (integer math, factor of 256).
            let yc = (y_val as i32 - 16) * 298;
            let uc = u_val as i32 - 128;
            let vc = v_val as i32 - 128;
            let r = ((yc + 409 * vc + 128) >> 8).clamp(0, 255) as u8;
            let g = ((yc - 100 * uc - 208 * vc + 128) >> 8).clamp(0, 255) as u8;
            let b = ((yc + 516 * uc + 128) >> 8).clamp(0, 255) as u8;

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

/// Owned Camera2 NDK resources. Cleanup in `Drop` runs in reverse order.
pub struct AcameraCapture {
    frame_rx: Receiver<WebRtcVideoFrame>,
    // Held in this struct so Drop order is deterministic.
    listener_ctx: Box<ListenerContext>,
    listener: Box<AImageReader_ImageListener>,
    device_cb: Box<ACameraDevice_StateCallbacks>,
    session_cb: Box<ACameraCaptureSession_stateCallbacks>,

    session: *mut ACameraCaptureSession,
    request: *mut ACaptureRequest,
    target: *mut ACameraOutputTarget,
    session_output: *mut ACaptureSessionOutput,
    session_output_container: *mut ACaptureSessionOutputContainer,
    reader: *mut AImageReader,
    device: *mut ACameraDevice,
    manager: *mut ACameraManager,
}

// All the raw NDK pointers are owned by `Self` and only mutated through
// `Drop`. Sending them across threads is safe because we never share them
// concurrently.
unsafe impl Send for AcameraCapture {}
unsafe impl Sync for AcameraCapture {}

impl AcameraCapture {
    /// Open the front-facing camera and start delivering frames into a
    /// channel. Errors out if Camera2 enumeration fails or no front-facing
    /// camera exists; callers can fall back to whatever else they want.
    pub fn start() -> Result<Self, String> {
        let manager = unsafe { ACameraManager_create() };
        if manager.is_null() {
            return Err("ACameraManager_create returned null".into());
        }

        let camera_id = match find_front_camera_id(manager) {
            Ok(id) => id,
            Err(e) => {
                unsafe { ACameraManager_delete(manager) };
                return Err(e);
            }
        };

        let mut device_cb = Box::new(ACameraDevice_StateCallbacks {
            context: null_mut(),
            on_disconnected,
            on_error,
        });
        let mut device: *mut ACameraDevice = null_mut();
        let cid = match CString::new(camera_id.clone()) {
            Ok(c) => c,
            Err(e) => {
                unsafe { ACameraManager_delete(manager) };
                return Err(format!("camera id contains nul byte: {e}"));
            }
        };
        let device_cb_ptr: *mut ACameraDevice_StateCallbacks = &mut *device_cb;
        let status = unsafe {
            ACameraManager_openCamera(manager, cid.as_ptr(), device_cb_ptr, &mut device)
        };
        if status != ACAMERA_OK || device.is_null() {
            unsafe { ACameraManager_delete(manager) };
            return Err(format!("ACameraManager_openCamera failed: {status}"));
        }

        // Reasonable inference resolution — matches MediaPipe's expected
        // input letterbox source.
        let cap_w: i32 = 640;
        let cap_h: i32 = 480;

        let mut reader: *mut AImageReader = null_mut();
        let s = unsafe {
            AImageReader_new(cap_w, cap_h, AIMAGE_FORMAT_YUV_420_888, 2, &mut reader)
        };
        if s != AMEDIA_OK || reader.is_null() {
            unsafe { ACameraDevice_close(device); ACameraManager_delete(manager); }
            return Err(format!("AImageReader_new failed: {s}"));
        }

        let (frame_tx, frame_rx) = bounded::<WebRtcVideoFrame>(1);
        let listener_ctx = Box::new(ListenerContext {
            sender: frame_tx,
            alive: AtomicBool::new(true),
            width: cap_w as usize,
            height: cap_h as usize,
            frame_count: AtomicUsize::new(0),
        });
        let listener_ctx_ptr: *const ListenerContext = &*listener_ctx;
        let mut listener = Box::new(AImageReader_ImageListener {
            context: listener_ctx_ptr as *mut c_void,
            on_image_available,
        });
        let listener_ptr: *mut AImageReader_ImageListener = &mut *listener;
        let s = unsafe { AImageReader_setImageListener(reader, listener_ptr) };
        if s != AMEDIA_OK {
            unsafe { AImageReader_delete(reader); ACameraDevice_close(device); ACameraManager_delete(manager); }
            return Err(format!("AImageReader_setImageListener failed: {s}"));
        }

        let mut window: *mut ANativeWindow = null_mut();
        let s = unsafe { AImageReader_getWindow(reader, &mut window) };
        if s != AMEDIA_OK || window.is_null() {
            unsafe { AImageReader_delete(reader); ACameraDevice_close(device); ACameraManager_delete(manager); }
            return Err(format!("AImageReader_getWindow failed: {s}"));
        }

        let mut session_output: *mut ACaptureSessionOutput = null_mut();
        if unsafe { ACaptureSessionOutput_create(window, &mut session_output) } != ACAMERA_OK {
            unsafe { AImageReader_delete(reader); ACameraDevice_close(device); ACameraManager_delete(manager); }
            return Err("ACaptureSessionOutput_create failed".into());
        }

        let mut session_output_container: *mut ACaptureSessionOutputContainer = null_mut();
        if unsafe { ACaptureSessionOutputContainer_create(&mut session_output_container) } != ACAMERA_OK
            || unsafe { ACaptureSessionOutputContainer_add(session_output_container, session_output) } != ACAMERA_OK
        {
            unsafe { ACaptureSessionOutput_free(session_output); AImageReader_delete(reader); ACameraDevice_close(device); ACameraManager_delete(manager); }
            return Err("ACaptureSessionOutputContainer setup failed".into());
        }

        let mut target: *mut ACameraOutputTarget = null_mut();
        if unsafe { ACameraOutputTarget_create(window, &mut target) } != ACAMERA_OK {
            unsafe { ACaptureSessionOutputContainer_free(session_output_container); ACaptureSessionOutput_free(session_output); AImageReader_delete(reader); ACameraDevice_close(device); ACameraManager_delete(manager); }
            return Err("ACameraOutputTarget_create failed".into());
        }

        let mut request: *mut ACaptureRequest = null_mut();
        if unsafe { ACameraDevice_createCaptureRequest(device, TEMPLATE_PREVIEW, &mut request) } != ACAMERA_OK
            || unsafe { ACaptureRequest_addTarget(request, target) } != ACAMERA_OK
        {
            unsafe {
                if !request.is_null() { ACaptureRequest_free(request); }
                ACameraOutputTarget_free(target);
                ACaptureSessionOutputContainer_free(session_output_container);
                ACaptureSessionOutput_free(session_output);
                AImageReader_delete(reader);
                ACameraDevice_close(device);
                ACameraManager_delete(manager);
            }
            return Err("createCaptureRequest / addTarget failed".into());
        }

        let session_cb = Box::new(ACameraCaptureSession_stateCallbacks {
            context: null_mut(),
            on_closed: on_session_closed,
            on_ready: on_session_ready,
            on_active: on_session_active,
        });
        let session_cb_ptr: *const ACameraCaptureSession_stateCallbacks = &*session_cb;

        let mut session: *mut ACameraCaptureSession = null_mut();
        if unsafe { ACameraDevice_createCaptureSession(device, session_output_container, session_cb_ptr, &mut session) } != ACAMERA_OK
            || session.is_null()
        {
            unsafe {
                ACaptureRequest_free(request);
                ACameraOutputTarget_free(target);
                ACaptureSessionOutputContainer_free(session_output_container);
                ACaptureSessionOutput_free(session_output);
                AImageReader_delete(reader);
                ACameraDevice_close(device);
                ACameraManager_delete(manager);
            }
            return Err("ACameraDevice_createCaptureSession failed".into());
        }

        let mut req_ptr = request;
        let mut sequence_id: c_int = 0;
        if unsafe {
            ACameraCaptureSession_setRepeatingRequest(
                session, null_mut(), 1, &mut req_ptr, &mut sequence_id,
            )
        } != ACAMERA_OK
        {
            unsafe {
                ACameraCaptureSession_close(session);
                ACaptureRequest_free(request);
                ACameraOutputTarget_free(target);
                ACaptureSessionOutputContainer_free(session_output_container);
                ACaptureSessionOutput_free(session_output);
                AImageReader_delete(reader);
                ACameraDevice_close(device);
                ACameraManager_delete(manager);
            }
            return Err("setRepeatingRequest failed".into());
        }

        log!(
            "AcameraCapture: opened camera '{}' at {}x{} YUV_420_888 (seq={})",
            camera_id, cap_w, cap_h, sequence_id
        );

        Ok(Self {
            frame_rx,
            listener_ctx,
            listener,
            device_cb,
            session_cb,
            session,
            request,
            target,
            session_output,
            session_output_container,
            reader,
            device,
            manager,
        })
    }

    /// Pull the latest frame from the camera if one is buffered. Returns
    /// `None` if no frame has arrived since the last call.
    pub fn try_recv(&self) -> Option<WebRtcVideoFrame> {
        self.frame_rx.try_recv().ok()
    }
}

impl Drop for AcameraCapture {
    fn drop(&mut self) {
        // Mark the listener inactive so any in-flight image callback
        // returns immediately and doesn't touch the soon-to-be-freed
        // resources.
        self.listener_ctx.alive.store(false, Ordering::Release);
        unsafe {
            if !self.session.is_null() {
                ACameraCaptureSession_stopRepeating(self.session);
                ACameraCaptureSession_close(self.session);
            }
            if !self.request.is_null() {
                ACaptureRequest_free(self.request);
            }
            if !self.target.is_null() {
                ACameraOutputTarget_free(self.target);
            }
            if !self.session_output_container.is_null() {
                ACaptureSessionOutputContainer_free(self.session_output_container);
            }
            if !self.session_output.is_null() {
                ACaptureSessionOutput_free(self.session_output);
            }
            if !self.reader.is_null() {
                AImageReader_delete(self.reader);
            }
            if !self.device.is_null() {
                let _ = ACameraDevice_close(self.device);
            }
            if !self.manager.is_null() {
                ACameraManager_delete(self.manager);
            }
        }
    }
}

fn find_front_camera_id(manager: *mut ACameraManager) -> Result<String, String> {
    let mut list: *mut ACameraIdList = null_mut();
    if unsafe { ACameraManager_getCameraIdList(manager, &mut list) } != ACAMERA_OK || list.is_null() {
        return Err("ACameraManager_getCameraIdList failed".into());
    }
    let n = unsafe { (*list).numCameras };
    let ids = unsafe { std::slice::from_raw_parts((*list).cameraIds, n.max(0) as usize) };
    let mut first_id: Option<String> = None;
    let mut front_id: Option<String> = None;
    for &id_ptr in ids {
        if id_ptr.is_null() { continue; }
        let cstr = unsafe { CStr::from_ptr(id_ptr) };
        let id = cstr.to_string_lossy().into_owned();
        if first_id.is_none() { first_id = Some(id.clone()); }
        let mut md: *mut ACameraMetadata = null_mut();
        if unsafe { ACameraManager_getCameraCharacteristics(manager, id_ptr, &mut md) } != ACAMERA_OK || md.is_null() {
            continue;
        }
        let mut entry = ACameraMetadata_const_entry {
            tag: 0,
            typ: 0,
            count: 0,
            data: ACameraMetadata_data_union { u8: null_mut() },
        };
        if unsafe { ACameraMetadata_getConstEntry(md, ACAMERA_LENS_FACING, &mut entry) } == ACAMERA_OK
            && entry.count > 0
        {
            let facing = unsafe { *entry.data.u8 };
            if facing == ACAMERA_LENS_FACING_FRONT {
                front_id = Some(id);
                unsafe { ACameraMetadata_free(md); }
                break;
            }
        }
        unsafe { ACameraMetadata_free(md); }
    }
    unsafe { ACameraManager_deleteCameraIdList(list); }
    front_id.or(first_id).ok_or_else(|| "no usable camera found".into())
}

// Compatibility shim so `robot_screen.rs` can keep using the same
// `start_inference_capture(cx)` / `cap.try_recv()` shape it uses on macOS.
// The `cx` parameter is unused — Camera2 doesn't need a Makepad handle.
pub fn start_for_inference(_cx: &mut makepad_widgets::Cx) -> Option<AcameraCapture> {
    match AcameraCapture::start() {
        Ok(c) => Some(c),
        Err(e) => {
            log!("AcameraCapture::start failed: {e}");
            None
        }
    }
}

// `Arc<...>` reachable from background thread because the listener
// holds a raw pointer into `listener_ctx`. The Box must outlive the
// listener callbacks; this is enforced by `Drop` running in declaration
// order (Rust drops fields in declaration order; we list `session` first
// so it stops the callbacks before anything is freed).
unsafe impl Send for ListenerContext {}
unsafe impl Sync for ListenerContext {}

// Allow ordering tests to ignore arc traversal warnings.
#[doc(hidden)]
#[allow(dead_code)]
fn _phantom_arc<T: Send + Sync>(_: Arc<T>) {}
