//! HTTP sender for the robotic arm car, built on Makepad's `cx.http_request`.
//!
//! Each [`RobotHttpSender::send`] call fires a GET to
//! `http://{ip}/api/control?action={wire}&speed=50` via `cx.http_request`.
//! Responses arrive on the standard `Event::NetworkResponses` event bus and
//! are matched back to the originating action via a per-request `metadata_id`
//! counter we stash in the request.
//!
//! No tokio task, no channels — this struct is just a pending-request map
//! plus a sequence counter.

use std::collections::HashMap;
use std::time::Instant;

use makepad_widgets::*;

use crate::gesture_control::GestureAction;

/// Request-id tag used for every robot-control call. Each request also carries
/// a unique `metadata_id` (a monotonically increasing sequence number wrapped
/// in a `LiveId`) so concurrent requests can be matched to their pending entry
/// on response.
pub const ROBOT_CONTROL_REQUEST_ID: LiveId = live_id!(robot_control);

/// Outcome of a single HTTP attempt — drives the connection indicator color.
#[derive(Clone, Debug)]
pub enum HttpOutcome {
    /// 2xx response, latency in milliseconds.
    Ok { status: u16, latency_ms: u32 },
    /// Non-2xx response (5xx, 4xx, …).
    HttpStatus { status: u16 },
    /// Transport error (DNS, connection refused, TLS, …).
    Error(String),
}

impl HttpOutcome {
    /// True if this outcome means "talked to the car successfully".
    pub fn is_ok(&self) -> bool {
        matches!(self, HttpOutcome::Ok { .. })
    }
}

/// A delivered result for the UI to render in its recent-commands log.
#[derive(Clone, Debug)]
pub struct HttpResult {
    pub action: GestureAction,
    pub outcome: HttpOutcome,
}

struct PendingRequest {
    action: GestureAction,
    started: Instant,
}

/// Stateless-ish helper that fires control requests through Makepad's HTTP
/// pipeline and matches responses back to their originating gesture.
#[derive(Default)]
pub struct RobotHttpSender {
    /// Monotonically increasing counter used to build per-request metadata_id.
    next_seq: u64,
    /// In-flight requests keyed by the metadata_id we stamped on them.
    pending: HashMap<u64, PendingRequest>,
}

impl RobotHttpSender {
    pub fn new() -> Self {
        Self::default()
    }

    /// Fire a control request. Returns `false` if `action` has no wire name
    /// (i.e. `GestureAction::None`) — the call is silently a no-op in that
    /// case.
    pub fn send(&mut self, cx: &mut Cx, action: GestureAction, ip: &str) -> bool {
        let Some(wire) = action.wire_name() else {
            return false;
        };
        let url = format!("http://{}/api/control?action={}&speed=50", ip, wire);
        let mut req = HttpRequest::new(url.clone(), HttpMethod::GET);

        let seq = self.next_seq;
        self.next_seq = self.next_seq.wrapping_add(1);
        req.set_metadata_id(LiveId(seq));

        log!("RobotHttpSender: GET {} (seq={})", url, seq);
        self.pending
            .insert(seq, PendingRequest { action, started: Instant::now() });
        cx.http_request(ROBOT_CONTROL_REQUEST_ID, req);
        true
    }

    /// Inspect a `NetworkResponse` and, if it belongs to one of our pending
    /// requests, return the matched [`HttpResult`]. Returns `None` for
    /// unrelated responses or for in-flight chunks/progress events.
    pub fn handle_network_response(&mut self, response: &NetworkResponse) -> Option<HttpResult> {
        match response {
            NetworkResponse::HttpResponse { request_id, response: r }
                if *request_id == ROBOT_CONTROL_REQUEST_ID =>
            {
                let pending = self.pending.remove(&r.metadata_id.0)?;
                let latency_ms = pending.started.elapsed().as_millis() as u32;
                let status = r.status_code;
                let outcome = if (200..300).contains(&status) {
                    HttpOutcome::Ok { status, latency_ms }
                } else {
                    HttpOutcome::HttpStatus { status }
                };
                Some(HttpResult { action: pending.action, outcome })
            }
            NetworkResponse::HttpError { request_id, error }
                if *request_id == ROBOT_CONTROL_REQUEST_ID =>
            {
                let pending = self.pending.remove(&error.metadata_id.0)?;
                Some(HttpResult {
                    action: pending.action,
                    outcome: HttpOutcome::Error(error.message.clone()),
                })
            }
            _ => None,
        }
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
