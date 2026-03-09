//! Simple Crew integration that sends messages to a local HTTP endpoint.

use makepad_widgets::*;

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
        HttpMethod::Post,
    );

    request.set_header("Authorization".to_string(), "Bearer my-secret".to_string());
    request.set_header("Content-Type".to_string(), "application/json".to_string());
    request.set_json_body(request_body);

    cx.http_request(id!(CREW_HTTP_REQUEST), request);
}

/// Handle HTTP response for Crew messages.
/// Call this from your widget's event handler.
pub fn handle_crew_response(cx: &mut Cx, event: &Event) {
    if let Event::NetworkResponse(response) = event {
        if response.request_id != id!(CREW_HTTP_REQUEST) {
            return;
        }

        if response.status_code >= 200 && response.status_code < 300 {
            match response.get_string_body() {
                Ok(body) => {
                    log!("Crew response: {}", body.chars().take(100).collect::<String>());
                    enqueue_popup_notification(
                        format!("Crew response received:\n{}", body.chars().take(200).collect::<String>()),
                        PopupKind::Info,
                        Some(5.0),
                    );
                }
                Err(e) => {
                    error!("Failed to parse Crew response: {:?}", e);
                    enqueue_popup_notification(
                        format!("Failed to parse Crew response: {:?}", e),
                        PopupKind::Error,
                        None,
                    );
                }
            }
        } else {
            let error_body = response.get_string_body().unwrap_or_else(|_| "(no body)".to_string());
            error!("Crew error {}: {}", response.status_code, error_body);
            enqueue_popup_notification(
                format!("Crew error {}: {}", response.status_code, error_body),
                PopupKind::Error,
                None,
            );
        }

        cx.redraw_all();
    }
}
