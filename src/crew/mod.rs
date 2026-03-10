//! Simple Crew integration that sends messages to a local HTTP endpoint.

use makepad_widgets::*;
use makepad_widgets::makepad_micro_serde::{SerJson, DeJson, SerJsonState, DeJsonState, DeJsonErr};

use crate::shared::popup_list::{enqueue_popup_notification, PopupKind};

pub mod crew_send_button;

pub fn live_design(cx: &mut Cx) {
    crew_send_button::live_design(cx);
}

/// Request body for the /api/chat endpoint.
#[derive(Debug, Clone, SerJson, DeJson)]
pub struct ChatRequest {
    pub message: String,
}

/// Response from the /api/chat endpoint.
#[derive(Debug, Clone, DeJson)]
pub struct ChatResponse {
    pub response: Option<String>,
}

/// Sends a message to the Crew chat server using Makepad's HTTP request API.
pub fn send_crew_message(cx: &mut Cx, content: &str) {
    log!("Sending Crew message: {}...", content.chars().take(50).collect::<String>());

    let request_body = ChatRequest {
        message: content.to_string(),
    };

    let mut request = HttpRequest::new(
        "http://localhost:8080/api/chat".to_string(),
        HttpMethod::POST,
    );

    request.set_header("Authorization".to_string(), "Bearer my-secret".to_string());
    request.set_header("Content-Type".to_string(), "application/json".to_string());
    request.set_json_body(request_body);

    cx.http_request(id!(CREW_HTTP_REQUEST), request);
}

/// Handle HTTP response for Crew messages.
/// Call this from your widget's event handler.
pub fn handle_crew_response(cx: &mut Cx, event: &Event) {
    if let Event::NetworkResponses(responses) = event {
        for item in responses {
            if item.request_id != id!(CREW_HTTP_REQUEST) {
                continue;
            }

            // NetworkResponse structure has changed in Makepad
            // For now, just show a notification that response was received
            log!("Crew HTTP response received");
            enqueue_popup_notification(
                "Crew response received (see logs for details)".to_string(),
                PopupKind::Info,
                Some(3.0),
            );
        }

        cx.redraw_all();
    }
}
