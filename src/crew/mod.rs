//! Simple Crew integration that sends messages to a local HTTP endpoint.

use std::sync::OnceLock;

use makepad_widgets::*;
use serde::Serialize;
use tokio::{runtime::Handle, sync::mpsc::{unbounded_channel, UnboundedReceiver, UnboundedSender}};

use crate::shared::popup_list::{enqueue_popup_notification, PopupKind};

pub mod crew_send_button;

pub fn live_design(cx: &mut Cx) {
    crew_send_button::live_design(cx);
}

/// The sender used by [`send_crew_message()`] to send requests to the async worker thread.
static CREW_REQUEST_SENDER: OnceLock<UnboundedSender<String>> = OnceLock::new();

/// Request body for the /api/chat endpoint.
#[derive(Debug, Clone, Serialize)]
struct ChatRequest {
    message: String,
}

/// Sends a message to the Crew chat server.
pub fn send_crew_message(content: &str) {
    let Some(sender) = CREW_REQUEST_SENDER.get() else {
        enqueue_popup_notification(
            "Failed to send Crew message: sender not initialized.\n\n\
                Please restart Robrix.",
            PopupKind::Error,
            None,
        );
        return;
    };
    if sender.send(content.to_string()).is_err() {
        enqueue_popup_notification(
            "Failed to send Crew message: background worker has stopped.\n\n\
                Please restart Robrix.",
            PopupKind::Error,
            None,
        );
    }
}

/// Initializes the Crew integration.
pub fn crew_init(rt_handle: Handle) -> anyhow::Result<()> {
    // Create a channel for communication between UI and async worker.
    let (sender, receiver) = unbounded_channel::<String>();
    CREW_REQUEST_SENDER.set(sender).expect("BUG: CREW_REQUEST_SENDER already set!");

    // Start the async worker task.
    rt_handle.spawn(async move {
        log!("Starting Crew async worker task.");
        async_crew_worker(receiver).await;
    });

    log!("Crew initialized.");
    Ok(())
}

/// The async worker thread that processes Crew requests.
async fn async_crew_worker(mut receiver: UnboundedReceiver<String>) {
    // Create an HTTP client.
    let http_client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(120))
        .build()
        .expect("Failed to create HTTP client for Crew");

    while let Some(message) = receiver.recv().await {
        log!("Sending Crew message: {}...", message.chars().take(50).collect::<String>());

        let request_body = ChatRequest { message: message.clone() };

        let result = http_client
            .post("http://localhost:8080/api/chat")
            .header("Authorization", "Bearer my-secret")
            .header("Content-Type", "application/json")
            .json(&request_body)
            .send()
            .await;

        match result {
            Ok(response) => {
                if response.status().is_success() {
                    let body = response.text().await.unwrap_or_default();
                    log!("Crew response: {}", body.chars().take(100).collect::<String>());
                    enqueue_popup_notification(
                        format!("Crew response received:\n{}", body.chars().take(200).collect::<String>()),
                        PopupKind::Info,
                        Some(5.0),
                    );
                } else {
                    let status = response.status();
                    let error_body = response.text().await.unwrap_or_default();
                    error!("Crew error {}: {}", status, error_body);
                    enqueue_popup_notification(
                        format!("Crew error {}: {}", status, error_body),
                        PopupKind::Error,
                        None,
                    );
                }
            }
            Err(e) => {
                error!("Failed to connect to Crew: {}", e);
                enqueue_popup_notification(
                    format!("Failed to connect to Crew: {}", e),
                    PopupKind::Error,
                    None,
                );
            }
        }
    }

    error!("Crew async worker ended unexpectedly");
}
