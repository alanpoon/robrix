use std::collections::HashMap;
use std::fs;
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use anyhow::{Context, Result};
use matrix_sdk::{
    config::SyncSettings,
    ruma::{
        events::room::message::{
            MessageType, OriginalSyncRoomMessageEvent, RoomMessageEventContent,
        },
        OwnedRoomId, OwnedUserId,
    },
    Client, Room,
};
use serde::{Deserialize, Serialize};
use tokio::sync::RwLock;
use tokio::task::JoinHandle;
use tokio::time::sleep;

/// Configuration for a single bot
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
struct BotConfig {
    username: String,
    password: String,
    homeserver: String,
    room_id: String,
    #[serde(default = "default_crew_api_url")]
    crew_api_url: String,
    #[serde(default = "default_crew_api_token")]
    crew_api_token: String,
}

fn default_crew_api_url() -> String {
    "http://localhost:8080".to_string()
}

fn default_crew_api_token() -> String {
    "Bearer my-secret".to_string()
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
fn send_to_crew_api(api_url: &str, auth_token: &str, message: &str) -> Result<String> {
    let endpoint = format!("{}/api/chat", api_url);
    let request_body = CrewRequest {
        message: message.to_string(),
    };

    let response = ureq::post(&endpoint)
        .set("Authorization", auth_token)
        .set("Content-Type", "application/json")
        .send_json(&request_body)
        .context("Failed to send request to Crew API")?;

    let crew_response: CrewResponse = response
        .into_json()
        .context("Failed to parse Crew API response")?;

    Ok(crew_response.response)
}

/// Handle incoming room messages for a bot
async fn handle_message(
    event: OriginalSyncRoomMessageEvent,
    room: Room,
    bot_user_id: &OwnedUserId,
    crew_api_url: &str,
    crew_api_token: &str,
    bot_username: &str,
) -> Result<()> {
    // Ignore messages from the bot itself to prevent loops
    if event.sender == *bot_user_id {
        return Ok(());
    }

    let MessageType::Text(text_content) = &event.content.msgtype else {
        return Ok(());
    };

    let body = &text_content.body;
    println!(
        "[{}] 📨 Received message from {}: {}",
        bot_username, event.sender, body
    );

    // Check if it's a crew message
    if is_crew_message(body) {
        let crew_content = extract_crew_content(body);
        println!("[{}] 🔍 Detected crew message: {}", bot_username, crew_content);

        // Send to Crew API
        match send_to_crew_api(crew_api_url, crew_api_token, &crew_content) {
            Ok(response) => {
                let formatted_response = format!("🤖 Crew Response:\n{}", response);
                let content = RoomMessageEventContent::text_plain(&formatted_response);
                room.send(content).await?;
                println!("[{}] ✅ Posted response to room", bot_username);
            }
            Err(e) => {
                eprintln!("[{}] ❌ Error calling Crew API: {}", bot_username, e);
                let error_msg = format!("❌ Error contacting Crew API: {}", e);
                let content = RoomMessageEventContent::text_plain(&error_msg);
                room.send(content).await?;
            }
        }
    }

    Ok(())
}

/// Run a single bot instance
async fn run_bot(config: BotConfig) -> Result<()> {
    let bot_username = config.username.clone();

    println!("[{}] 🚀 Starting bot", bot_username);
    println!("[{}]    Homeserver: {}", bot_username, config.homeserver);
    println!("[{}]    Room ID: {}", bot_username, config.room_id);
    println!("[{}]    Crew API: {}", bot_username, config.crew_api_url);

    // Create Matrix client
    let client = Client::builder()
        .homeserver_url(&config.homeserver)
        .build()
        .await
        .context("Failed to create Matrix client")?;

    // Login with username and password
    println!("[{}] 🔐 Logging in with password...", bot_username);

    client
        .matrix_auth()
        .login_username(&config.username, &config.password)
        .initial_device_display_name(&format!("Botfather - {}", bot_username))
        .await
        .context("Failed to login")?;

    println!("[{}] ✅ Logged in as {}", bot_username, client.user_id().unwrap());

    let bot_user_id = client.user_id().unwrap().to_owned();

    // Perform initial sync
    println!("[{}] 🔄 Performing initial sync...", bot_username);
    client
        .sync_once(SyncSettings::default())
        .await
        .context("Initial sync failed")?;

    // Get the room
    let room_id: OwnedRoomId = config.room_id
        .parse()
        .context("Invalid room ID format")?;
    let room = client
        .get_room(&room_id)
        .context("Room not found")?;

    let room_name = room
        .display_name()
        .await
        .ok()
        .map(|n| n.to_string())
        .unwrap_or_else(|| "Unknown".to_string());
    println!("[{}] ✅ Found room: {}", bot_username, room_name);

    // Clone values for use in the closure
    let crew_api_url = Arc::new(config.crew_api_url);
    let crew_api_token = Arc::new(config.crew_api_token);
    let bot_username_arc = Arc::new(bot_username.clone());

    // Register event handler for room messages
    client.add_event_handler(
        move |event: OriginalSyncRoomMessageEvent, room: Room| {
            let bot_user_id = bot_user_id.clone();
            let crew_api_url = Arc::clone(&crew_api_url);
            let crew_api_token = Arc::clone(&crew_api_token);
            let bot_username = Arc::clone(&bot_username_arc);

            async move {
                if let Err(e) = handle_message(
                    event,
                    room,
                    &bot_user_id,
                    &crew_api_url,
                    &crew_api_token,
                    &bot_username,
                )
                .await
                {
                    eprintln!("[{}] ❌ Error handling message: {}", bot_username, e);
                }
            }
        },
    );

    println!("[{}] 👂 Listening for messages...", bot_username);

    // Start syncing
    let settings = SyncSettings::default().timeout(Duration::from_secs(30));

    loop {
        match client.sync_once(settings.clone()).await {
            Ok(_) => {}
            Err(e) => {
                eprintln!("[{}] ❌ Sync error: {}", bot_username, e);
                sleep(Duration::from_secs(5)).await;
            }
        }
    }
}

/// Read and parse the bots.json file
fn read_bots_config(path: &Path) -> Result<Vec<BotConfig>> {
    let content = fs::read_to_string(path)
        .context("Failed to read bots.json")?;
    let configs: Vec<BotConfig> = serde_json::from_str(&content)
        .context("Failed to parse bots.json")?;
    Ok(configs)
}

/// BotManager manages the lifecycle of all bots
struct BotManager {
    running_bots: Arc<RwLock<HashMap<String, JoinHandle<()>>>>,
}

impl BotManager {
    fn new() -> Self {
        Self {
            running_bots: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Start a new bot
    async fn start_bot(&self, config: BotConfig) {
        let username = config.username.clone();

        // Check if bot is already running
        {
            let bots = self.running_bots.read().await;
            if bots.contains_key(&username) {
                println!("[{}] ⚠️  Bot already running, skipping", username);
                return;
            }
        }

        println!("[{}] ▶️  Starting bot...", username);

        let handle = tokio::spawn(async move {
            if let Err(e) = run_bot(config).await {
                eprintln!("Bot error: {}", e);
            }
        });

        let mut bots = self.running_bots.write().await;
        bots.insert(username, handle);
    }

    /// Stop a bot by username
    async fn stop_bot(&self, username: &str) {
        let mut bots = self.running_bots.write().await;

        if let Some(handle) = bots.remove(username) {
            println!("[{}] ⏸️  Stopping bot...", username);
            handle.abort();
        }
    }

    /// Update bots based on new configuration
    async fn update_bots(&self, new_configs: Vec<BotConfig>) {
        let new_usernames: std::collections::HashSet<_> =
            new_configs.iter().map(|c| c.username.clone()).collect();

        // Stop bots that are no longer in the config
        let bots_to_stop = {
            let bots = self.running_bots.read().await;
            let current_usernames: Vec<_> = bots.keys().cloned().collect();

            current_usernames
                .into_iter()
                .filter(|username| !new_usernames.contains(username))
                .collect::<Vec<_>>()
        };

        for username in bots_to_stop {
            self.stop_bot(&username).await;
        }

        // Start new bots
        for config in new_configs {
            self.start_bot(config).await;
        }
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt::init();

    println!("🤖 BotFather - Matrix Bot Manager");
    println!("   Watching: bots.json");
    println!();

    let bots_path = Path::new("bots.json");
    let manager = BotManager::new();

    // Initial load
    match read_bots_config(bots_path) {
        Ok(configs) => {
            if configs.is_empty() {
                println!("📝 bots.json is empty - no bots to start");
            } else {
                println!("📋 Found {} bot(s) in configuration", configs.len());
                manager.update_bots(configs).await;
            }
        }
        Err(e) => {
            eprintln!("❌ Error reading bots.json: {}", e);
            eprintln!("   Create a bots.json file with bot configurations");
            return Ok(());
        }
    }

    // Watch for file changes
    println!();
    println!("👀 Watching for changes to bots.json...");
    println!("   (Press Ctrl+C to quit)");

    let mut last_modified = fs::metadata(bots_path)
        .ok()
        .and_then(|m| m.modified().ok());

    loop {
        sleep(Duration::from_secs(2)).await;

        // Check if file was modified
        if let Ok(metadata) = fs::metadata(bots_path) {
            if let Ok(modified) = metadata.modified() {
                if last_modified.map_or(true, |last| modified > last) {
                    last_modified = Some(modified);

                    println!();
                    println!("🔄 Detected change in bots.json, reloading...");

                    match read_bots_config(bots_path) {
                        Ok(configs) => {
                            if configs.is_empty() {
                                println!("📝 bots.json is now empty - stopping all bots");
                            } else {
                                println!("📋 Found {} bot(s) in updated configuration", configs.len());
                            }
                            manager.update_bots(configs).await;
                        }
                        Err(e) => {
                            eprintln!("❌ Error parsing updated bots.json: {}", e);
                        }
                    }
                }
            }
        }
    }
}
