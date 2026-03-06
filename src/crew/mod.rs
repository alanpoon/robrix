//! Integration with Crew AI chat agent system.
//!
//! This module provides the ability to send messages to Crew's chat server
//! alongside Matrix messages.

use std::sync::{Mutex, OnceLock};

use makepad_widgets::*;
use serde::{Deserialize, Serialize};
use tokio::{runtime::Handle, sync::mpsc::{unbounded_channel, UnboundedReceiver, UnboundedSender}};

use crate::shared::popup_list::{enqueue_popup_notification, PopupKind};

pub mod crew_send_button;

pub fn live_design(cx: &mut Cx) {
    crew_send_button::live_design(cx);
}

/// The sender used by [`submit_crew_request()`] to send Crew requests to the async worker thread.
static CREW_REQUEST_SENDER: OnceLock<UnboundedSender<CrewRequest>> = OnceLock::new();

/// Submits a Crew request to the worker thread to be executed asynchronously.
///
/// If an error occurs, a popup notification will be displayed to the user
/// informing them of the error with a recommendation to restart the app.
pub fn submit_crew_request(req: CrewRequest) {
    let Some(sender) = CREW_REQUEST_SENDER.get() else {
        enqueue_popup_notification(
            "Failed to submit Crew request: Crew request sender was not initialized.\n\n\
                Please restart Robrix to continue using Crew features.",
            PopupKind::Error,
            None,
        );
        return;
    };
    if sender.send(req).is_err() {
        enqueue_popup_notification(
            "Failed to submit Crew request: the background Crew worker task has died.\n\n\
                Please restart Robrix to continue using Crew features.",
            PopupKind::Error,
            None,
        );
    }
}

/// The global singleton Crew state.
static CREW_STATE: Mutex<CrewState> = Mutex::new(CrewState::new());

pub fn crew_state_ref() -> &'static Mutex<CrewState> {
    &CREW_STATE
}

/// Default channel for Crew messages.
pub const DEFAULT_CREW_CHANNEL: &str = "matrix";
/// Default chat ID for Crew messages.
pub const DEFAULT_CREW_CHAT_ID: &str = "default";
/// Default gateway URL for Crew.
pub const DEFAULT_GATEWAY_URL: &str = "http://localhost:8080";

/// The current state of the Crew integration.
#[derive(Debug)]
pub struct CrewState {
    /// The channel to send messages to (e.g., "matrix", "telegram", etc.).
    pub channel: String,
    /// The chat ID to send messages to.
    pub chat_id: String,
    /// The gateway URL to connect to.
    pub gateway_url: String,
    /// Optional auth token for the gateway.
    pub auth_token: Option<String>,
    /// Whether the Crew integration is currently connected/active.
    pub is_connected: bool,
}

impl Default for CrewState {
    fn default() -> Self {
        Self {
            channel: DEFAULT_CREW_CHANNEL.to_string(),
            chat_id: DEFAULT_CREW_CHAT_ID.to_string(),
            gateway_url: DEFAULT_GATEWAY_URL.to_string(),
            auth_token: None,
            is_connected: true,
        }
    }
}

impl CrewState {
    const fn new() -> Self {
        Self {
            channel: String::new(),
            chat_id: String::new(),
            gateway_url: String::new(),
            auth_token: None,
            is_connected: false,
        }
    }

    /// Configures the Crew state with channel and chat information.
    pub fn configure(&mut self, channel: String, chat_id: String) {
        self.channel = channel;
        self.chat_id = chat_id;
        self.is_connected = true;
    }

    /// Sets the gateway URL.
    pub fn set_gateway_url(&mut self, url: String) {
        self.gateway_url = url;
    }

    /// Sets the auth token for the gateway.
    pub fn set_auth_token(&mut self, token: Option<String>) {
        self.auth_token = token;
    }
}

/// Request body for the crew-cli gateway `/api/chat` endpoint.
#[derive(Debug, Clone, Serialize)]
pub struct GatewayChatRequest {
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub session_id: Option<String>,
}

/// Response from the crew-cli gateway `/api/chat` endpoint.
#[derive(Debug, Clone, Deserialize)]
pub struct GatewayChatResponse {
    pub content: String,
    pub input_tokens: u32,
    pub output_tokens: u32,
}

/// Requests that can be sent to the Crew async worker thread.
pub enum CrewRequest {
    /// Request to send a message to the Crew chat server.
    SendMessage {
        content: String,
        session_id: Option<String>,
    },
    /// Request to configure the Crew connection.
    Configure {
        gateway_url: Option<String>,
        auth_token: Option<String>,
        channel: Option<String>,
        chat_id: Option<String>,
    },
}

/// Actions related to Crew operations.
#[derive(Debug, Clone)]
pub enum CrewAction {
    /// A message was successfully sent to Crew and a response was received.
    ResponseReceived {
        content: String,
        input_tokens: u32,
        output_tokens: u32,
    },
    /// Failed to send a message to Crew.
    MessageSendFailed { error: String },
    /// Crew connection was configured.
    Configured { gateway_url: String },
}

/// Initializes the Crew integration.
pub fn crew_init(rt_handle: tokio::runtime::Handle) -> anyhow::Result<()> {
    // Initialize the Crew state with default configuration.
    {
        let mut state = crew_state_ref().lock().unwrap();
        state.channel = DEFAULT_CREW_CHANNEL.to_string();
        state.chat_id = DEFAULT_CREW_CHAT_ID.to_string();
        state.gateway_url = DEFAULT_GATEWAY_URL.to_string();
        state.is_connected = true;
    }
    log!("Crew initialized with default configuration: gateway='{}', channel='{}', chat_id='{}'",
        DEFAULT_GATEWAY_URL, DEFAULT_CREW_CHANNEL, DEFAULT_CREW_CHAT_ID);

    // Create a channel to be used between UI thread(s) and the Crew async worker thread.
    let (sender, receiver) = unbounded_channel::<CrewRequest>();
    CREW_REQUEST_SENDER.set(sender).expect("BUG: CREW_REQUEST_SENDER already set!");

    // Start the async worker task.
    let _monitor = rt_handle.spawn(async move {
        log!("Starting Crew async worker task.");
        match async_crew_worker(receiver).await {
            Ok(()) => log!("Crew async worker task ended normally."),
            Err(e) => {
                error!("Crew async worker task ended with error: {e:?}");
                enqueue_popup_notification(
                    format!("Crew background worker error: {e}"),
                    PopupKind::Error,
                    None,
                );
            }
        }
    });

    Ok(())
}

/// The entry point for an async worker thread that processes Crew-related async tasks.
async fn async_crew_worker(
    mut request_receiver: UnboundedReceiver<CrewRequest>,
) -> anyhow::Result<()> {
    log!("Started async_crew_worker task.");

    // Create an HTTP client for communicating with the gateway.
    let http_client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(120))
        .build()
        .expect("Failed to create HTTP client for Crew gateway");

    while let Some(req) = request_receiver.recv().await {
        match req {
            CrewRequest::SendMessage { content, session_id } => {
                log!("Received CrewRequest::SendMessage(content: {}...)",
                    content.chars().take(50).collect::<String>());

                let client = http_client.clone();
                Handle::current().spawn(async move {
                    let (gateway_url, auth_token, chat_id) = {
                        let state = crew_state_ref().lock().unwrap();
                        (
                            state.gateway_url.clone(),
                            state.auth_token.clone(),
                            state.chat_id.clone(),
                        )
                    };

                    if gateway_url.is_empty() {
                        enqueue_popup_notification(
                            "Crew gateway URL is not configured.",
                            PopupKind::Warning,
                            None,
                        );
                        Cx::post_action(CrewAction::MessageSendFailed {
                            error: "Gateway URL not configured".to_string(),
                        });
                        return;
                    }

                    let chat_endpoint = format!("{}/api/chat", gateway_url.trim_end_matches('/'));
                    let session = session_id.or_else(|| {
                        if chat_id.is_empty() { None } else { Some(chat_id) }
                    });

                    let request_body = GatewayChatRequest {
                        message: content.clone(),
                        session_id: session,
                    };

                    log!("Sending Crew message to gateway '{}': {}...",
                        chat_endpoint,
                        content.chars().take(50).collect::<String>()
                    );

                    let mut request = client
                        .post(&chat_endpoint)
                        .header("Content-Type", "application/json")
                        .json(&request_body);

                    // Add auth token if configured
                    if let Some(token) = auth_token {
                        request = request.header("Authorization", format!("Bearer {}", token));
                    }

                    match request.send().await {
                        Ok(response) => {
                            if response.status().is_success() {
                                match response.json::<GatewayChatResponse>().await {
                                    Ok(chat_response) => {
                                        log!("Crew gateway response received: {}...",
                                            chat_response.content.chars().take(100).collect::<String>());

                                        Cx::post_action(CrewAction::ResponseReceived {
                                            content: chat_response.content,
                                            input_tokens: chat_response.input_tokens,
                                            output_tokens: chat_response.output_tokens,
                                        });
                                    }
                                    Err(e) => {
                                        let error_msg = format!("Failed to parse gateway response: {}", e);
                                        error!("{}", error_msg);
                                        enqueue_popup_notification(error_msg.clone(), PopupKind::Error, None);
                                        Cx::post_action(CrewAction::MessageSendFailed { error: error_msg });
                                    }
                                }
                            } else {
                                let status = response.status();
                                let error_body = response.text().await.unwrap_or_default();
                                let error_msg = format!("Gateway returned error {}: {}", status, error_body);
                                error!("{}", error_msg);
                                enqueue_popup_notification(error_msg.clone(), PopupKind::Error, None);
                                Cx::post_action(CrewAction::MessageSendFailed { error: error_msg });
                            }
                        }
                        Err(e) => {
                            let error_msg = format!("Failed to connect to gateway: {}", e);
                            error!("{}", error_msg);
                            enqueue_popup_notification(error_msg.clone(), PopupKind::Error, None);
                            Cx::post_action(CrewAction::MessageSendFailed { error: error_msg });
                        }
                    }
                });
            }

            CrewRequest::Configure { gateway_url, auth_token, channel, chat_id } => {
                log!("Received CrewRequest::Configure(gateway_url: {:?})", gateway_url);

                let configured_url = {
                    let mut state = crew_state_ref().lock().unwrap();
                    if let Some(url) = gateway_url {
                        state.set_gateway_url(url);
                    }
                    if let Some(token) = auth_token {
                        state.set_auth_token(Some(token));
                    }
                    if let Some(ch) = channel {
                        state.channel = ch;
                    }
                    if let Some(cid) = chat_id {
                        state.chat_id = cid;
                    }
                    state.is_connected = true;
                    state.gateway_url.clone()
                };

                enqueue_popup_notification(
                    format!("Crew gateway configured: {}", configured_url),
                    PopupKind::Info,
                    Some(3.0),
                );

                Cx::post_action(CrewAction::Configured { gateway_url: configured_url });
            }
        }
    }

    error!("async_crew_worker task ended unexpectedly");
    anyhow::bail!("async_crew_worker task ended unexpectedly")
}

/// Sends a message to the Crew chat server.
///
/// This is a convenience function that submits a `CrewRequest::SendMessage`.
pub fn send_crew_message(content: &str) {
    submit_crew_request(CrewRequest::SendMessage {
        content: content.to_string(),
        session_id: None,
    });
}

/// Sends a message to the Crew chat server with a specific session ID.
pub fn send_crew_message_with_session(content: &str, session_id: &str) {
    submit_crew_request(CrewRequest::SendMessage {
        content: content.to_string(),
        session_id: Some(session_id.to_string()),
    });
}

/// Configures the Crew gateway connection.
pub fn configure_crew_gateway(gateway_url: Option<&str>, auth_token: Option<&str>) {
    submit_crew_request(CrewRequest::Configure {
        gateway_url: gateway_url.map(|s| s.to_string()),
        auth_token: auth_token.map(|s| s.to_string()),
        channel: None,
        chat_id: None,
    });
}
