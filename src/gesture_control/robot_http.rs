//! Async HTTP sender for the robotic arm car.
//!
//! Owns a single long-lived tokio task with a `reqwest::Client` and an unbounded
//! receiver. UI thread calls [`RobotHttpSender::send`] with the gesture and the
//! current IP — each request is POSTed to `http://{ip}/cmd` with body
//! `{"action":"<wire>"}`, `Content-Type: application/json`, 2-second timeout.
//! The outcome is forwarded back on `result_rx` for the UI's status indicator
//! and recent-commands log.
//!
//! Uses `matrix_sdk::reqwest` (reqwest 0.13, already a transitive dep) so no
//! new HTTP crate is added — the project's existing `reqwest` declaration is
//! gated on the `tsp` feature and not available to non-TSP code.

use std::time::Instant;

use crossbeam_channel::{Receiver, Sender, unbounded};
use makepad_widgets::log;
use matrix_sdk::reqwest;
use tokio::sync::mpsc;

use crate::gesture_control::GestureAction;

/// Outcome of a single HTTP attempt — drives the connection indicator color.
#[derive(Clone, Debug)]
pub enum HttpOutcome {
    /// 2xx response, latency in milliseconds.
    Ok { status: u16, latency_ms: u32 },
    /// Non-2xx response (5xx, 4xx, …).
    HttpStatus { status: u16 },
    /// Request timed out within the 2-second window.
    Timeout,
    /// Other transport error (DNS, connection refused, TLS, …).
    Error(String),
}

impl HttpOutcome {
    /// True if this outcome means "talked to the car successfully".
    pub fn is_ok(&self) -> bool {
        matches!(self, HttpOutcome::Ok { .. })
    }
}

/// A request to send a single gesture command.
#[derive(Clone, Debug)]
pub struct HttpRequest {
    pub action: GestureAction,
    /// Pre-validated IP string (e.g. `"192.168.4.1"` or `"192.168.4.1:8080"`).
    /// The URL `http://{ip}/cmd` is built inside the tokio task.
    pub ip: String,
}

/// A delivered result for the UI.
#[derive(Clone, Debug)]
pub struct HttpResult {
    pub action: GestureAction,
    pub outcome: HttpOutcome,
}

/// Handle to the HTTP-sending tokio task. Hold one of these per `RobotScreen`
/// instance. Dropping the handle drops the request sender, which causes the
/// task to shut down cleanly.
pub struct RobotHttpSender {
    request_tx: mpsc::UnboundedSender<HttpRequest>,
    result_rx: Receiver<HttpResult>,
}

impl RobotHttpSender {
    /// Spawn the HTTP task on the given tokio runtime handle.
    pub fn spawn(rt: tokio::runtime::Handle) -> Self {
        let (request_tx, mut request_rx) = mpsc::unbounded_channel::<HttpRequest>();
        let (result_tx, result_rx): (Sender<HttpResult>, Receiver<HttpResult>) = unbounded();

        rt.spawn(async move {
            let client = match reqwest::Client::builder()
                .timeout(std::time::Duration::from_secs(2))
                .build()
            {
                Ok(c) => c,
                Err(e) => {
                    log!("RobotHttpSender: failed to build reqwest client: {e}");
                    return;
                }
            };

            while let Some(req) = request_rx.recv().await {
                let Some(wire) = req.action.wire_name() else {
                    continue;
                };
                let url = format!("http://{}/cmd", req.ip);
                // Manual JSON: matrix_sdk's re-exported reqwest does not
                // necessarily enable the `json` Cargo feature, so we serialize
                // by hand and set Content-Type ourselves.
                let body_str = format!("{{\"action\":\"{}\"}}", wire);

                let start = Instant::now();
                let resp = client
                    .post(&url)
                    .header("Content-Type", "application/json")
                    .body(body_str)
                    .send()
                    .await;
                let elapsed_ms = start.elapsed().as_millis() as u32;

                let outcome = match resp {
                    Ok(r) => {
                        let status = r.status().as_u16();
                        if r.status().is_success() {
                            HttpOutcome::Ok { status, latency_ms: elapsed_ms }
                        } else {
                            HttpOutcome::HttpStatus { status }
                        }
                    }
                    Err(e) if e.is_timeout() => HttpOutcome::Timeout,
                    Err(e) => HttpOutcome::Error(e.to_string()),
                };

                // try_send: if the UI dropped the receiver (tab closed), we
                // silently discard — the tokio task will exit on the next
                // request_rx.recv() when the request sender is dropped.
                let _ = result_tx.send(HttpResult { action: req.action, outcome });
            }
        });

        Self { request_tx, result_rx }
    }

    /// Enqueue a gesture for sending. Non-blocking. Returns `false` if the
    /// HTTP task has shut down (should not happen during normal operation).
    pub fn send(&self, action: GestureAction, ip: String) -> bool {
        self.request_tx.send(HttpRequest { action, ip }).is_ok()
    }

    /// Drain pending HTTP results — called by the UI on each `Event::NextFrame`.
    pub fn try_recv(&self) -> Option<HttpResult> {
        self.result_rx.try_recv().ok()
    }
}

/// Validate that a string is an IPv4 address optionally followed by `:port`.
/// Returns `Some(canonicalized_ip)` on success, `None` on parse failure.
///
/// Accepts: `"192.168.4.1"`, `"192.168.4.1:8080"`, `"10.0.0.5:80"`.
/// Rejects: empty, hostnames, IPv6, missing octets, port out of range.
pub fn validate_ip(s: &str) -> Option<String> {
    let s = s.trim();
    if s.is_empty() {
        return None;
    }
    let (host, port) = match s.split_once(':') {
        Some((h, p)) => {
            let port: u16 = p.parse().ok()?;
            if port == 0 {
                return None;
            }
            (h, Some(port))
        }
        None => (s, None),
    };
    // IPv4: exactly 4 dot-separated octets, each 0..=255.
    let mut octets = host.split('.');
    let mut count = 0;
    for o in &mut octets {
        let v: u32 = o.parse().ok()?;
        if v > 255 {
            return None;
        }
        count += 1;
    }
    if count != 4 {
        return None;
    }
    Some(match port {
        Some(p) => format!("{host}:{p}"),
        None => host.to_string(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validate_ip_accepts_plain_ipv4() {
        assert_eq!(validate_ip("192.168.4.1"), Some("192.168.4.1".to_string()));
        assert_eq!(validate_ip(" 10.0.0.5 "), Some("10.0.0.5".to_string()));
    }

    #[test]
    fn validate_ip_accepts_ipv4_with_port() {
        assert_eq!(validate_ip("192.168.4.1:8080"), Some("192.168.4.1:8080".to_string()));
    }

    #[test]
    fn validate_ip_rejects_empty() {
        assert_eq!(validate_ip(""), None);
        assert_eq!(validate_ip("   "), None);
    }

    #[test]
    fn validate_ip_rejects_hostname() {
        assert_eq!(validate_ip("car.local"), None);
        assert_eq!(validate_ip("example.com"), None);
    }

    #[test]
    fn validate_ip_rejects_octet_overflow() {
        assert_eq!(validate_ip("256.0.0.1"), None);
        assert_eq!(validate_ip("192.168.4.999"), None);
    }

    #[test]
    fn validate_ip_rejects_too_few_octets() {
        assert_eq!(validate_ip("192.168.4"), None);
        assert_eq!(validate_ip("192"), None);
    }

    #[test]
    fn validate_ip_rejects_port_zero() {
        assert_eq!(validate_ip("192.168.4.1:0"), None);
    }

    #[test]
    fn validate_ip_rejects_port_overflow() {
        assert_eq!(validate_ip("192.168.4.1:99999"), None);
    }
}
