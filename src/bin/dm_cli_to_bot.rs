use anyhow::{Context, Result};
use matrix_sdk::{
    config::SyncSettings,
    ruma::{
        events::room::message::{
            MessageType, OriginalSyncRoomMessageEvent, RoomMessageEventContent,
        },
        OwnedRoomId, UserId,
    },
    Client, Room,
};
use serde::Deserialize;
use std::fs;
use std::io::{self, BufRead, Write};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use tokio::sync::mpsc;
use tokio::time::{Duration, sleep};

const HOMESERVER: &str = "http://localhost:8008";
const USERNAME: &str = "testuser";
const PASSWORD: &str = "testpassword";
const TARGET_USER: &str = "@testuser2:localhost";

/// Configuration for a bot from bots.json
#[derive(Debug, Deserialize)]
struct BotConfig {
    username: String,
    #[allow(dead_code)]
    password: String,
    #[allow(dead_code)]
    homeserver: String,
    room_id: Option<String>,
    #[allow(dead_code)]
    #[serde(default)]
    crew_api_url: String,
    #[allow(dead_code)]
    #[serde(default)]
    crew_api_token: String,
}

/// Read bots.json and find room_id for the target user
fn get_room_id_from_config(target_username: &str) -> Result<Option<String>> {
    let config_path = "bots.json";

    // Check if file exists
    if !std::path::Path::new(config_path).exists() {
        return Ok(None);
    }

    let content = fs::read_to_string(config_path)
        .context("Failed to read bots.json")?;

    let configs: Vec<BotConfig> = serde_json::from_str(&content)
        .context("Failed to parse bots.json")?;

    // Find config where username matches target
    for config in configs {
        if config.username == target_username {
            return Ok(config.room_id);
        }
    }

    Ok(None)
}

/// Handle incoming messages and print them to stdout
async fn handle_message(
    event: OriginalSyncRoomMessageEvent,
    _room: Room,
    my_user_id: &UserId,
    target_user_id: &UserId,
    response_received: Arc<AtomicBool>,
) {
    // Don't print messages from ourselves
    if event.sender == my_user_id {
        return;
    }

    // Only print messages from testuser2
    if event.sender == target_user_id {
        if let MessageType::Text(text_content) = &event.content.msgtype {
            let message = &text_content.body;
            println!("\n📨 Message received from {}: {}", event.sender, message);
            response_received.store(true, Ordering::SeqCst);
            print!("> ");
            io::stdout().flush().unwrap();
        }
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    // Set NO_PROXY to bypass proxy for localhost
    unsafe {
        std::env::set_var("NO_PROXY", "localhost,127.0.0.1");
        std::env::set_var("no_proxy", "localhost,127.0.0.1");
    }

    tracing_subscriber::fmt::init();

    println!("🚀 Matrix DM CLI to Bot");
    println!("   Sending to: {}", TARGET_USER);
    println!("   Logging in as {}...", USERNAME);

    // Create client (without persistent storage for now)
    let client = Client::builder()
        .homeserver_url(HOMESERVER)
        .build()
        .await
        .context("Failed to create Matrix client")?;

    // Login
    client
        .matrix_auth()
        .login_username(USERNAME, PASSWORD)
        .initial_device_display_name("DM CLI to Bot")
        .await
        .context("Failed to login")?;

    let my_user_id = client.user_id().unwrap().to_owned();
    println!("✅ Logged in as {}", my_user_id);

    // Perform initial sync
    println!("🔄 Performing initial sync...");
    client.sync_once(SyncSettings::default()).await?;

    // Get target user ID
    let target_user_id: Box<UserId> = TARGET_USER.parse()
        .context("Invalid target user ID format")?;

    // Extract username from target user ID (e.g., "@testuser2:localhost" -> "testuser2")
    let target_username = target_user_id.localpart();

    // Try to get room_id from bots.json
    println!("🔍 Looking for room_id in bots.json for user '{}'...", target_username);
    let room_id_str = get_room_id_from_config(target_username)?
        .context("No room_id found in bots.json for target user. Please add an entry with username and room_id.")?;

    println!("✅ Found room_id in config: {}", room_id_str);

    // Parse room ID and get the room
    let room_id: OwnedRoomId = room_id_str
        .parse()
        .context("Invalid room ID format in bots.json")?;

    let room = client
        .get_room(&room_id)
        .context("Room not found. Make sure the bot has joined this room.")?;

    println!("✅ Using room: {}", room_id);

    // Flag to track if we received a response
    let response_received = Arc::new(AtomicBool::new(false));

    // Set up event handler for incoming messages from testuser2
    let my_user_id_clone = my_user_id.clone();
    let target_user_id_clone = target_user_id.clone();
    let response_received_clone = Arc::clone(&response_received);
    client.add_event_handler(move |event: OriginalSyncRoomMessageEvent, room: Room| {
        let user_id = my_user_id_clone.clone();
        let target_id = target_user_id_clone.clone();
        let response_flag = Arc::clone(&response_received_clone);
        async move {
            handle_message(event, room, &user_id, &target_id, response_flag).await;
        }
    });

    // Channel to send messages from stdin thread to async runtime
    let (message_tx, mut message_rx) = mpsc::unbounded_channel::<String>();

    // Start sync loop in background
    let client_clone = client.clone();
    let _sync_handle = tokio::spawn(async move {
        let settings = SyncSettings::default().timeout(Duration::from_secs(30));
        loop {
            match client_clone.sync_once(settings.clone()).await {
                Ok(_) => {},
                Err(e) => {
                    eprintln!("\n❌ Sync error: {}", e);
                }
            }
        }
    });

    // Start stdin reader in blocking thread
    let _stdin_handle = std::thread::spawn(move || {
        let stdin = io::stdin();
        let reader = stdin.lock();

        for line in reader.lines() {
            match line {
                Ok(message) => {
                    if message_tx.send(message).is_err() {
                        break;
                    }
                }
                Err(e) => {
                    eprintln!("Error reading stdin: {}", e);
                    break;
                }
            }
        }
    });

    println!("\n💬 Chat ready! Type your message and press Enter to send to {}.", TARGET_USER);
    println!("📨 Listening for messages from {}...", TARGET_USER);
    println!("   (Press Ctrl+C to quit)\n");
    print!("> ");
    io::stdout().flush()?;

    // Track if we sent a message (for waiting for response)
    let mut sent_message = false;

    // Main message sending loop
    while let Some(message) = message_rx.recv().await {
        let message = message.trim();

        if message.is_empty() {
            print!("> ");
            io::stdout().flush()?;
            continue;
        }

        // Send message to room
        let content = RoomMessageEventContent::text_plain(message);
        match room.send(content).await {
            Ok(_) => {
                println!("✅ Message sent: {}", message);
                sent_message = true;
            }
            Err(e) => {
                eprintln!("❌ Failed to send message: {}", e);
            }
        }

        print!("> ");
        io::stdout().flush()?;
    }

    // If we sent a message, wait for a response (up to 30 seconds)
    if sent_message {
        println!("\n⏳ Waiting for response from {}...", TARGET_USER);
        let timeout_secs = 30;
        for i in 0..timeout_secs {
            if response_received.load(Ordering::SeqCst) {
                println!("✅ Response received!");
                sleep(Duration::from_secs(1)).await; // Give time to print the message
                break;
            }
            sleep(Duration::from_secs(1)).await;
            if (i + 1) % 5 == 0 {
                println!("   Still waiting... ({}/{}s)", i + 1, timeout_secs);
            }
        }
        if !response_received.load(Ordering::SeqCst) {
            println!("⏰ Timeout waiting for response");
        }
    }

    Ok(())
}
