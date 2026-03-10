use std::sync::Arc;

use anyhow::{Context, Result};
use clap::Parser;
use matrix_sdk::{
    config::SyncSettings,
    ruma::{
        events::room::message::{
            MessageType, OriginalSyncRoomMessageEvent, RoomMessageEventContent,
        },
        OwnedRoomId, OwnedUserId,
    },
    Client, Room, RoomState,
};
use serde::{Deserialize, Serialize};
use tokio::time::{sleep, Duration};

/// Matrix Crew Bot - Forwards crew messages to Crew API and posts responses back
#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
struct Args {
    /// Matrix homeserver URL
    #[arg(short = 's', long)]
    homeserver: String,

    /// Bot username
    #[arg(short = 'u', long)]
    username: String,

    /// Bot password
    #[arg(short = 'p', long)]
    password: String,

    /// Room ID to monitor (e.g., !abc123:matrix.org)
    #[arg(short = 'r', long)]
    room_id: String,

    /// Crew API base URL
    #[arg(short = 'c', long, default_value = "http://localhost:8080")]
    crew_api_url: String,

    /// Crew API authorization token
    #[arg(short = 'a', long, default_value = "Bearer my-secret")]
    crew_api_token: String,
}

#[derive(Debug, Serialize)]
struct CrewRequest {
    message: String,
}

#[derive(Debug, Deserialize)]
struct CrewResponse {
    response: String,
}

/// Check if a message is a crew message (starts with "!crew ")
fn is_crew_message(body: &str) -> bool {
    body.trim_start().starts_with("!crew ")
}

/// Extract the crew message content (removes the "!crew " prefix)
fn extract_crew_content(body: &str) -> String {
    body.trim_start()
        .strip_prefix("!crew ")
        .unwrap_or("")
        .to_string()
}

/// Send a message to the Crew API and get the response
fn send_to_crew_api(
    api_url: &str,
    auth_token: &str,
    message: &str,
) -> Result<String> {
    let endpoint = format!("{}/api/chat", api_url);
    let request_body = CrewRequest {
        message: message.to_string(),
    };

    println!("🤖 Sending to Crew API: {}", message);

    let response = ureq::post(&endpoint)
        .set("Authorization", auth_token)
        .set("Content-Type", "application/json")
        .send_json(&request_body)
        .context("Failed to send request to Crew API")?;

    let crew_response: CrewResponse = response
        .into_json()
        .context("Failed to parse Crew API response")?;

    println!("✅ Received from Crew API: {}", crew_response.response);

    Ok(crew_response.response)
}

/// Handle incoming room messages
async fn handle_message(
    event: OriginalSyncRoomMessageEvent,
    room: Room,
    bot_user_id: &OwnedUserId,
    crew_api_url: &str,
    crew_api_token: &str,
) -> Result<()> {
    // Ignore messages from the bot itself to prevent loops
    if event.sender == *bot_user_id {
        return Ok(());
    }

    let MessageType::Text(text_content) = &event.content.msgtype else {
        // Ignore non-text messages
        return Ok(());
    };

    let body = &text_content.body;
    println!("📨 Received message from {}: {}", event.sender, body);

    // Check if it's a crew message
    if is_crew_message(body) {
        let crew_content = extract_crew_content(body);
        println!("🔍 Detected crew message: {}", crew_content);

        // Send to Crew API
        match send_to_crew_api(crew_api_url, crew_api_token, &crew_content) {
            Ok(response) => {
                // Format the response with "Crew:" prefix to indicate it's from the bot
                let formatted_response = format!("🤖 Crew Response:\n{}", response);

                // Send the response back to the room
                let content = RoomMessageEventContent::text_plain(&formatted_response);
                room.send(content).await?;

                println!("✅ Posted response to room");
            }
            Err(e) => {
                eprintln!("❌ Error calling Crew API: {}", e);

                // Send error message to room
                let error_msg = format!("❌ Error contacting Crew API: {}", e);
                let content = RoomMessageEventContent::text_plain(&error_msg);
                room.send(content).await?;
            }
        }
    } else {
        println!("💬 Normal message (not a crew message)");
    }

    Ok(())
}

#[tokio::main]
async fn main() -> Result<()> {
    // Initialize tracing
    tracing_subscriber::fmt::init();

    let args = Args::parse();

    println!("🚀 Starting Matrix Crew Bot");
    println!("   Homeserver: {}", args.homeserver);
    println!("   Username: {}", args.username);
    println!("   Room ID: {}", args.room_id);
    println!("   Crew API: {}", args.crew_api_url);

    // Parse room ID
    let room_id: OwnedRoomId = args
        .room_id
        .parse()
        .context("Invalid room ID format")?;

    // Create Matrix client
    let client = Client::builder()
        .homeserver_url(&args.homeserver)
        .build()
        .await
        .context("Failed to create Matrix client")?;

    // Login
    println!("🔐 Logging in...");
    client
        .matrix_auth()
        .login_username(&args.username, &args.password)
        .initial_device_display_name("Crew Bot")
        .await
        .context("Failed to login")?;

    println!("✅ Logged in as {}", client.user_id().unwrap());

    let bot_user_id = client.user_id().unwrap().to_owned();

    // Perform initial sync to get rooms
    println!("🔄 Performing initial sync...");
    client
        .sync_once(SyncSettings::default())
        .await
        .context("Initial sync failed")?;

    // Get the room
    let room = client
        .get_room(&room_id)
        .context("Room not found - make sure the bot has joined the room")?;

    if room.state() != RoomState::Joined {
        anyhow::bail!("Bot is not in the joined state for this room. Please invite and join the bot first.");
    }

    let room_name = room.display_name().await.ok().map(|n| n.to_string()).unwrap_or_else(|| "Unknown".to_string());
    println!("✅ Found room: {}", room_name);

    // Clone values for use in the closure
    let crew_api_url = Arc::new(args.crew_api_url);
    let crew_api_token = Arc::new(args.crew_api_token);

    // Register event handler for room messages
    client.add_event_handler(move |event: OriginalSyncRoomMessageEvent, room: Room| {
        let bot_user_id = bot_user_id.clone();
        let crew_api_url = Arc::clone(&crew_api_url);
        let crew_api_token = Arc::clone(&crew_api_token);

        async move {
            if let Err(e) = handle_message(
                event,
                room,
                &bot_user_id,
                &crew_api_url,
                &crew_api_token,
            )
            .await
            {
                eprintln!("❌ Error handling message: {}", e);
            }
        }
    });

    println!("👂 Listening for messages... (Press Ctrl+C to quit)");
    println!("   Send messages starting with '!crew ' to trigger the bot");

    // Start syncing
    let settings = SyncSettings::default().timeout(Duration::from_secs(30));

    loop {
        match client.sync_once(settings.clone()).await {
            Ok(_) => {
                // Sync successful, continue
            }
            Err(e) => {
                eprintln!("❌ Sync error: {}", e);
                println!("⏳ Retrying in 5 seconds...");
                sleep(Duration::from_secs(5)).await;
            }
        }
    }
}
