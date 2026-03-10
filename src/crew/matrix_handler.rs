//! Matrix bot handler for Crew API integration.
//!
//! This module provides functions for Matrix bots to:
//! - Call the Crew API with messages
//! - Handle incoming Matrix messages
//! - Send Crew API responses back as Matrix messages

use anyhow::{Context, Result};
use matrix_sdk::{
    ruma::events::room::message::{
        MessageType, OriginalSyncRoomMessageEvent, RoomMessageEventContent,
    },
    Room,
};
use serde::{Deserialize, Serialize};

/// Request body for the Crew API /api/chat endpoint.
#[derive(Debug, Clone, Serialize)]
pub struct CrewRequest {
    pub message: String,
}

/// Response from the Crew API /api/chat endpoint.
#[derive(Debug, Clone, Deserialize)]
pub struct CrewResponse {
    pub content: String,
}

/// Configuration for Crew API calls.
#[derive(Debug, Clone)]
pub struct CrewConfig {
    /// Base URL for the Crew API (e.g., "http://localhost:8080")
    pub api_url: String,
    /// Authorization token (e.g., "my-secret")
    pub api_token: String,
}

impl Default for CrewConfig {
    fn default() -> Self {
        Self {
            api_url: "http://localhost:8080".to_string(),
            api_token: "my-secret".to_string(),
        }
    }
}

/// Call the Crew API with a message and return the response content.
///
/// # Arguments
/// * `config` - Crew API configuration
/// * `message` - The message to send to the Crew API
///
/// # Returns
/// The content field from the Crew API response, or an error if the request fails.
pub async fn call_crew_api(config: &CrewConfig, message: &str) -> Result<String> {
    let endpoint = format!("{}/api/chat", config.api_url);

    let request_body = CrewRequest {
        message: message.to_string(),
    };

    // Create HTTP client with no proxy to avoid localhost proxy issues
    let http_client = reqwest::Client::builder()
        .no_proxy()
        .build()
        .context("Failed to create HTTP client")?;

    // Send POST request to Crew API
    let response = http_client
        .post(&endpoint)
        .bearer_auth(&config.api_token)
        .json(&request_body)
        .send()
        .await
        .context("Failed to send request to Crew API")?;

    // Check status code
    let status = response.status();

    if status == reqwest::StatusCode::OK {
        // Parse JSON response
        let crew_response: CrewResponse = response
            .json()
            .await
            .context("Failed to parse Crew API response as JSON")?;

        Ok(crew_response.content)
    } else {
        let error_text = response
            .text()
            .await
            .unwrap_or_else(|_| "Unable to read response".to_string());

        anyhow::bail!(
            "Crew API returned status {}: {}",
            status,
            error_text
        )
    }
}

/// Handle an incoming Matrix message and send Crew API response back to the room.
///
/// This function:
/// 1. Extracts the text content from the Matrix message
/// 2. Calls the Crew API with the message content
/// 3. Sends the Crew API response back to the Matrix room
///
/// # Arguments
/// * `event` - The Matrix room message event
/// * `room` - The Matrix room to send the response to
/// * `config` - Crew API configuration
/// * `bot_username` - Username for logging (optional)
///
/// # Returns
/// Ok(()) if the message was handled successfully, or an error if something failed.
pub async fn handle_matrix_message_with_crew(
    event: OriginalSyncRoomMessageEvent,
    room: Room,
    config: &CrewConfig,
    bot_username: Option<&str>,
) -> Result<()> {
    let username = bot_username.unwrap_or("bot");

    // Extract text content from the message
    let MessageType::Text(text_content) = &event.content.msgtype else {
        return Ok(()); // Ignore non-text messages
    };

    let message = &text_content.body;
    println!("[{}] 📨 Received message from {}: \"{}\"", username, event.sender, message);

    // Call Crew API
    println!("[{}] 📡 Calling Crew API...", username);

    match call_crew_api(config, message).await {
        Ok(response_content) => {
            println!("[{}] ✅ Crew API returned content: \"{}\"", username, response_content);

            // Send response back to the Matrix room
            println!("[{}] 📤 Sending Crew API response back to room...", username);
            let reply_content = RoomMessageEventContent::text_plain(&response_content);
            room.send(reply_content)
                .await
                .context("Failed to send reply message")?;

            println!("[{}] ✅ Reply sent", username);
            Ok(())
        }
        Err(e) => {
            eprintln!("[{}] ❌ Crew API error: {}", username, e);

            // Send error message to room
            let error_msg = format!("❌ Error contacting Crew API: {}", e);
            let error_content = RoomMessageEventContent::text_plain(&error_msg);
            room.send(error_content)
                .await
                .context("Failed to send error message")?;

            Err(e)
        }
    }
}
