spec: task
name: "Hand-Gesture Robot Arm Car Control"
inherits: project
tags: [feature, gesture, webcam, ml, http, ui]
estimate: 5d
---

## Intent

Add a new "Robot" dock tab that captures the local webcam, runs a two-stage hand-landmark ML pipeline (MediaPipe-equivalent palm detection → 21-point hand landmarks) on CPU using the pure-Rust `tract` ONNX runtime, classifies each detection into one of six discrete gestures (index up/down/left/right, closed fist, open palm), and emits both an in-app `GestureAction` plus a JSON HTTP POST to a user-configured robotic-arm-car endpoint on the local network. Models are downloaded on first use into the app data directory. Target platforms: macOS, Linux, Windows, Android — not web.

## Decisions

- Module location: `src/gesture_control/` with files `mod.rs`, `hand_model.rs`, `gesture_classifier.rs`, `inference_worker.rs`, `robot_http.rs`, `gesture_webcam_view.rs`, `robot_screen.rs`, `model_downloader.rs`
- Action enum (publicly exported from `mod.rs`): `pub enum GestureAction { Forward, Back, Left, Right, Catch, Drop, None }` — emitted via `cx.action(...)` on the standard Makepad action bus
- Tab integration: Robot tab is a **permanent** dock tab registered in `src/home/main_desktop_ui.rs` alongside `home_tab` — initial dock `tabs: [@home_tab, @robot_tab]`, template `@PermanentTab` (not closeable), kind id `robot_screen`. Not represented as a `SelectedRoom` variant because it is a singleton non-room tab.
- Inference engine: `tract-onnx` crate (pure Rust, no native libs), pinned to a single version chosen during implementation
- Model format: ONNX, two stages: `palm_detection_lite.onnx` (input 192×192 RGB) and `hand_landmark_lite.onnx` (input 224×224 RGB, output 63 landmark coords + score + handedness)
- Model storage: `<app_data_dir>/models/palm_detection_lite.onnx` and `<app_data_dir>/models/hand_landmark_lite.onnx`, downloaded via `reqwest::blocking::Client` on first tab open, verified by SHA-256 from constants in `model_downloader.rs`
- Model download URLs: stored as `const` in `model_downloader.rs`; concrete URLs filled in during implementation, pointing to a fixed CDN location
- Frame capture: `cx.video_input(0, |buf| …)` registered by `gesture_webcam_view.rs` — `VideoCameraPreviewMode::Native` is NOT used so the preview and the inference share the same buffer path
- Display: NV12 → RGBA conversion in the callback, uploaded to a `Texture2d` rendered by a `WebRtcVideo`-style shader, 21 landmark dots drawn as a Sdf2d overlay on top
- Inference rate: hardcoded ~10 Hz (every 3rd captured frame at 30 fps), tuning constant `INFERENCE_FRAME_SKIP` in `inference_worker.rs`
- Threading: one dedicated `std::thread` for inference; bounded `crossbeam_channel::bounded(1)` for frame ingress (drop on full, never block capture); `crossbeam_channel::unbounded` for results
- Result polling: `RobotScreen::handle_event` polls `result_rx.try_recv()` on each `Event::NextFrame` — no extra timer
- Gesture classifier: pure function `classify(&[Vec2; 21]) -> Option<GestureAction>` in `gesture_classifier.rs`, no state, no allocation, uses constants `EXTENSION_RATIO=1.6` and `AXIS_DOMINANCE_RATIO=1.3`
- Direction inference: index-fingertip-vs-wrist vector after camera-mirror X flip; Up=Forward, Down=Back, Left=Left, Right=Right
- Open palm → `Drop`; Closed fist → `Catch`; diagonal or partial gestures → `None` (no emit)
- Confidence gate: hand landmark score must be ≥ 0.6 before classification runs
- Debounce: same `GestureAction` is NOT re-emitted within 500 ms of the last emit; a different `GestureAction` resets the cooldown immediately
- HTTP shape: `POST http://{ip}/cmd` with body `{"action":"forward"}` (lowercase, one of `forward|back|left|right|catch|drop`), `Content-Type: application/json`, request timeout 2 s
- HTTP runtime: dedicated tokio task using `reqwest` with `rustls-tls` feature, reusing the Matrix SDK's existing tokio runtime via `crate::sliding_sync::start_matrix_tokio` handle
- IP field: `TextInput` widget in `RobotControlPanel`, validated on every keystroke as IPv4 or IPv4:port (regex-free, manual parse), persisted on Return key or focus loss
- IP persistence: new field `robot_control_ip: Option<String>` on `AppPreferences` (`src/settings/app_preferences.rs`), serialized with the existing app prefs JSON
- IP URL build: `http://{ip}/cmd` constructed only when sending; no `http://` allowed in the textbox itself
- Connection indicator states: grey (no IP / invalid), yellow (timeout or first launch with valid IP), green (last HTTP returned 2xx within 5 s), red (last HTTP returned non-2xx or non-timeout error); driven by `LastHttpResult { at: Instant, outcome: Outcome }` field, no background polling
- Camera coordination: opening the Robot tab calls `VoipGlobalState::release_lobby_camera_for_other_consumer(cx)` (new function); switching back to a VoIP lobby calls the inverse — only one consumer of `video_input(0,…)` at a time
- New cargo dependencies (approved as part of this task spec, overriding `project.spec.md` constraint): `tract-onnx`, `crossbeam-channel`, `sha2`; `reqwest` is already a transitive dep but the `rustls-tls` feature must be enabled if not already
- Android: `<uses-permission android:name="android.permission.INTERNET"/>` and `android:usesCleartextTraffic="true"` must be present in the Android manifest; cargo-apk config updated under `[package.metadata.android]` in `Cargo.toml`
- DSL syntax: Robot screen uses Makepad 2.0 `script_mod!`, named children via `:=`, property merge via `+:`, per `project.spec.md`

## Boundaries

### Allowed Changes
- src/gesture_control/** (new module)
- src/home/main_desktop_ui.rs (register `robot_screen` tab kind and add `@robot_tab` to the initial `main_tabs` list)
- src/settings/app_preferences.rs (add `robot_control_ip` field + serde)
- src/voip/mod.rs (add `release_lobby_camera_for_other_consumer` helper, no behavioural change to existing call paths)
- src/lib.rs (`mod gesture_control;`)
- Cargo.toml (add `tract-onnx`, `crossbeam-channel`, `sha2` under the existing desktop and Android cfg blocks; enable `rustls-tls` on `reqwest` if needed; update `[package.metadata.android]` permissions)
- specs/task-gesture-robot-control.spec.md (this file)

### Forbidden
- Do NOT add `mediapipe-rs`, `tflite`, `tflitec`, `ort`, `candle`, `wasmtime`, `wasmer`, or any non-`tract` ML runtime — engine choice is fixed by Decisions
- Do NOT add `live_design!` syntax to any new file — use Makepad 2.0 `script_mod!` per `project.spec.md`
- Do NOT make blocking HTTP calls on the UI thread — all `reqwest` calls live in the tokio task
- Do NOT run model inference on the UI thread — all `tract` calls live in the dedicated inference thread
- Do NOT use `VideoCameraPreviewMode::Native` for the Robot tab's preview — frames must flow through the `video_input(0,…)` callback so the same buffer feeds both display and inference
- Do NOT spam the car: holding a gesture for one second must NOT produce more than 2 HTTP requests of the same action (500 ms debounce)
- Do NOT auto-probe the IP with background pings — the connection indicator is driven only by real command results
- Do NOT make the inference rate, confidence threshold, debounce window, or classifier ratio constants user-facing settings in v1 — they live as `const` in code
- Do NOT bundle ONNX model weights into the repo — they are downloaded on first use
- Do NOT open both the Robot tab and a VoIP lobby simultaneously — the camera coordination handoff is mandatory
- Do NOT use `cx.request_permission` for a separate "robot camera" — reuse the camera permission already granted at app startup by `VoipGlobalState::initialize`

## Out of Scope

- Right-hand vs left-hand differentiation (handedness field is read for diagnostics only, not used in classification)
- Two-handed gestures
- Custom or user-trained models
- A second car or multi-robot fan-out
- HTTPS / mTLS to the car
- Authentication tokens in the HTTP request
- Per-gesture enable toggle, configurable debounce/threshold, configurable inference rate (all hardcoded in v1)
- Video recording of the gesture session
- Showing the remote car's camera feed
- iOS support (Robrix's iOS build is out of scope per existing repo state)
- WebAssembly / browser target — explicitly excluded by user requirement
- Gesture-driven control of Robrix's own UI (rooms list, timeline, etc.) — only the HTTP-to-car path is wired

## Completion Criteria

Scenario: Open Robot tab starts webcam preview
  Test: manual_test_robot_tab_preview_starts
  Given the app is logged in and the home screen is visible
  And camera permission was granted at startup
  When the user opens the Robot tab from the home or sidebar
  Then the webcam preview displays the live camera image within 2 seconds
  And the 21-landmark overlay layer is allocated but invisible until a hand is detected

Scenario: Index finger pointing up emits Forward and posts JSON
  Test: manual_test_index_up_emits_forward
  Given the Robot tab is open with a valid IP "192.168.4.1" set
  And the gesture classifier has been initialized
  When the user shows an index-finger-up gesture for 600 ms
  Then exactly one POST request is sent to "http://192.168.4.1/cmd"
  And the request body is "{\"action\":\"forward\"}"
  And the request Content-Type is "application/json"
  And the GestureAction emitted on the action bus is "Forward"
  And the "Last gesture" label in the panel reads "Forward"

Scenario: Closed fist emits Catch
  Test: manual_test_closed_fist_emits_catch
  Given the Robot tab is open with a valid IP set
  When the user shows a closed-fist gesture for 600 ms
  Then the request body is "{\"action\":\"catch\"}"
  And the GestureAction emitted is "Catch"

Scenario: Open palm emits Drop
  Test: manual_test_open_palm_emits_drop
  Given the Robot tab is open with a valid IP set
  When the user shows an open-palm gesture for 600 ms
  Then the request body is "{\"action\":\"drop\"}"
  And the GestureAction emitted is "Drop"

Scenario: Classifier unit test for each canonical gesture
  Test: gesture_control::gesture_classifier::tests
  Given a fixture array of 21 Vec2 landmarks for each of: index_up, index_down, index_left, index_right, fist, palm, diagonal, partial_open
  When `classify(&landmarks)` runs on each fixture
  Then `index_up` returns "Some(Forward)"
  And `index_down` returns "Some(Back)"
  And `index_left` returns "Some(Left)"
  And `index_right` returns "Some(Right)"
  And `fist` returns "Some(Catch)"
  And `palm` returns "Some(Drop)"
  And `diagonal` returns "None"
  And `partial_open` returns "None"

Scenario: Same gesture held does not spam HTTP requests
  Test: manual_test_debounce_holds_gesture
  Given the Robot tab is open with a valid IP set
  When the user holds an index-up gesture continuously for 2 seconds
  Then the number of POST requests sent is "4" or fewer
  And no two POST requests for "forward" arrive within 500 ms of each other

Scenario: IP textbox persists across app restart
  Test: manual_test_ip_persists
  Given the Robot tab is open with an empty IP field
  When the user types "192.168.4.1" and presses Return
  And the app is closed and reopened
  And the Robot tab is opened again
  Then the IP field displays "192.168.4.1"
  And the connection indicator is grey or yellow (never red until a command is attempted)

Scenario: Invalid IP blocks all HTTP requests
  Test: manual_test_invalid_ip_blocks_requests
  Given the Robot tab is open
  When the user types "not.an.ip" into the IP field
  Then the IP textbox border turns red
  And the connection indicator stays grey
  When the user then shows any valid gesture for 600 ms
  Then no POST request is sent
  And the GestureAction is still emitted on the local action bus

Scenario: Empty IP blocks HTTP requests but allows local actions
  Test: manual_test_empty_ip_emits_local_only
  Given the Robot tab is open with the IP field empty
  When the user shows an index-up gesture
  Then no HTTP request is attempted
  And the GestureAction "Forward" is still emitted on the local action bus
  And the "Last gesture" label still updates

Scenario: HTTP timeout flips indicator to yellow
  Test: manual_test_http_timeout_yellow
  Given the Robot tab is open with IP "192.0.2.1" (TEST-NET unreachable)
  When the user shows an index-up gesture
  And 3 seconds elapse
  Then the connection indicator is yellow
  And the gesture log shows "timeout" for the last entry

Scenario: HTTP 5xx flips indicator to red
  Test: manual_test_http_5xx_red
  Given the Robot tab is open with an IP pointing to a test server that returns 503
  When the user shows an index-up gesture
  Then the connection indicator is red within 2 seconds
  And the gesture log shows "503" for the last entry

Scenario: Model files missing on first launch trigger download
  Test: manual_test_model_download_first_run
  Given the app data dir contains no files under "models/"
  When the user opens the Robot tab for the first time
  Then a "Downloading hand model…" status appears in the panel
  And after the download completes the webcam preview becomes interactive
  And both "palm_detection_lite.onnx" and "hand_landmark_lite.onnx" exist under "<app_data_dir>/models/" with matching SHA-256

Scenario: Model download failure shows retry option
  Test: manual_test_model_download_failure_retry
  Given the device is offline
  When the user opens the Robot tab for the first time
  Then a "Model download failed" message appears with a "Retry" button
  And the webcam preview does NOT start
  When the device comes online and the user clicks Retry
  Then the download completes and the preview starts

Scenario: Ambiguous diagonal pointing emits nothing
  Test: gesture_control::gesture_classifier::tests::diagonal_pointing_yields_none
  Given the index finger is extended and the wrist→tip vector is "(0.6, 0.6)" normalized
  When `classify` runs
  Then it returns "None"

Scenario: Confidence below threshold suppresses emission
  Test: manual_test_low_confidence_suppressed
  Given the Robot tab is open with a valid IP set
  When the user shows a hand with the model returning confidence "0.4"
  Then no GestureAction is emitted
  And no POST request is sent
  And the "Last gesture" label is unchanged

Scenario: Opening Robot tab releases VoIP lobby camera
  Test: manual_test_camera_handoff_voip_to_robot
  Given a VoIP lobby tab is open with the lobby camera running
  When the user opens the Robot tab
  Then the VoIP lobby's camera preview shows the offline placeholder within 1 second
  And the Robot tab's webcam preview becomes live within 2 seconds

Scenario: Opening VoIP lobby releases Robot camera
  Test: manual_test_camera_handoff_robot_to_voip
  Given the Robot tab is open with the webcam running
  When the user opens a VoIP lobby tab
  Then the Robot tab's preview shows "Camera released" placeholder
  And the VoIP lobby's preview becomes live within 2 seconds

Scenario: Closing Robot tab shuts down inference thread
  Test: manual_test_tab_close_shuts_down_worker
  Given the Robot tab is open with the inference worker running
  When the user closes the Robot tab
  Then the inference thread exits within 1 second
  And no further `Event::NextFrame` polls attempt `result_rx.try_recv`
  And the worker's input channel is dropped
