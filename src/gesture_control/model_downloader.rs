//! ONNX model download + SHA-256 verification.
//!
//! The hand-landmark ONNX file lives under `<app_data_dir>/models/`. On
//! startup of the Robot tab we check the file exists and matches its pinned
//! SHA-256. If it's missing or corrupt we fetch the canonical copy from
//! [`MODEL_BASE_URL`] over `matrix_sdk::reqwest`.
//!
//! This is currently a **single-stage landmark-only** pipeline: the original
//! two-stage spec (palm-detection → landmark with rotated ROI) is deferred.
//! The landmark model is fed a center-square crop of the webcam frame and
//! produces 21 landmarks plus a hand-presence score directly.

use std::path::PathBuf;

use anyhow::{Context, Result, anyhow};
use makepad_widgets::log;
use sha2::{Digest, Sha256};

/// Filename of the hand-landmark ONNX model on disk and in the URL.
///
/// This is the OpenCV Zoo MediaPipe Hands port (input NHWC f32 [1,224,224,3],
/// outputs [1,63] landmarks in 224-px space, [1,1] hand-presence score,
/// [1,1] handedness, [1,63] world landmarks).
pub const HAND_LANDMARK_FILENAME: &str = "handpose_estimation_mediapipe_2023feb.onnx";

/// Base URL the model is fetched from. The full URL is `{MODEL_BASE_URL}/{filename}`.
pub const MODEL_BASE_URL: &str =
    "https://github.com/opencv/opencv_zoo/raw/main/models/handpose_estimation_mediapipe";

/// Expected SHA-256 of the landmark file (hex-encoded, lowercase, 64 chars).
/// Empty string disables verification.
pub const HAND_LANDMARK_SHA256: &str =
    "db0898ae717b76b075d9bf563af315b29562e11f8df5027a1ef07b02bef6d81c";

/// Directory under `app_data_dir` where ONNX files live.
pub fn models_dir() -> PathBuf {
    crate::app_data_dir().join("models")
}

pub fn hand_landmark_path() -> PathBuf {
    models_dir().join(HAND_LANDMARK_FILENAME)
}

/// True if the landmark ONNX file is on disk and matches its pinned SHA-256.
///
/// If the SHA-256 constant is empty, we only check file presence (bootstrap
/// mode for when the hash hasn't been pinned).
pub fn landmark_model_present_and_valid() -> bool {
    file_valid(&hand_landmark_path(), HAND_LANDMARK_SHA256)
}

fn file_valid(path: &std::path::Path, expected_sha: &str) -> bool {
    if !path.is_file() {
        return false;
    }
    if expected_sha.is_empty() {
        return true;
    }
    match sha256_of_file(path) {
        Ok(actual) => actual == expected_sha,
        Err(e) => {
            log!("sha256 read failed for {}: {e}", path.display());
            false
        }
    }
}

fn sha256_of_file(path: &std::path::Path) -> Result<String> {
    use std::io::Read;
    let mut f = std::fs::File::open(path).with_context(|| format!("open {}", path.display()))?;
    let mut hasher = Sha256::new();
    let mut buf = [0u8; 64 * 1024];
    loop {
        let n = f.read(&mut buf)?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    Ok(hex_encode(&hasher.finalize()))
}

fn hex_encode(bytes: &[u8]) -> String {
    let mut s = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        s.push_str(&format!("{:02x}", b));
    }
    s
}

/// Full URL the landmark model is fetched from.
///
/// Used by the caller (currently `RobotScreen`) which fires the actual GET
/// through Makepad's `cx.http_request` — that path goes through the
/// platform's native HTTPS stack (Android's `HttpURLConnection`, macOS's
/// NSURLSession, etc.) and follows the GitHub-raw → media.githubusercontent
/// redirect chain automatically, sidestepping reqwest's
/// rustls-platform-verifier dependency which panics on Android without the
/// JNI/Kotlin shim.
pub fn landmark_download_url() -> String {
    format!("{}/{}", MODEL_BASE_URL.trim_end_matches('/'), HAND_LANDMARK_FILENAME)
}

/// Verify the SHA-256 of the freshly-downloaded body and atomically install
/// it at `hand_landmark_path()`. Writes to `<dest>.onnx.tmp` first then
/// renames so a crashed download never leaves a corrupt file in place.
pub fn install_landmark_model(bytes: &[u8]) -> Result<()> {
    if !HAND_LANDMARK_SHA256.is_empty() {
        let mut hasher = Sha256::new();
        hasher.update(bytes);
        let actual = hex_encode(&hasher.finalize());
        if actual != HAND_LANDMARK_SHA256 {
            return Err(anyhow!(
                "SHA-256 mismatch for {HAND_LANDMARK_FILENAME}: expected {HAND_LANDMARK_SHA256}, got {actual}"
            ));
        }
    }

    std::fs::create_dir_all(models_dir())
        .with_context(|| format!("create dir {}", models_dir().display()))?;

    let dest = hand_landmark_path();
    let tmp = dest.with_extension("onnx.tmp");
    std::fs::write(&tmp, bytes)
        .with_context(|| format!("write {}", tmp.display()))?;
    std::fs::rename(&tmp, &dest)
        .with_context(|| format!("rename {} -> {}", tmp.display(), dest.display()))?;

    Ok(())
}
