use anyhow::{Context, Result};
use matrix_sdk::{
    config::SyncSettings,
    ruma::{
        events::room::{
            encrypted::OriginalSyncRoomEncryptedEvent,
            member::StrippedRoomMemberEvent,
            message::{
                MessageType, OriginalSyncRoomMessageEvent, RoomMessageEventContent,
            },
        },
        OwnedRoomId, OwnedUserId,
    },
    Client, Room, RoomState,
};
use robrix::crew::matrix_handler::{self, CrewConfig};
use serde::Deserialize;
use std::collections::HashMap;
use std::fs;
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::RwLock;
use tokio::task::JoinHandle;
use tokio::time::sleep;

/// Configuration for a single bot
#[derive(Debug, Clone, Deserialize, PartialEq, Eq, Hash)]
struct BotConfig {
    username: String,
    password: String,
    homeserver: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    room_id: Option<String>,
    #[serde(default = "default_crew_api_url")]
    crew_api_url: String,
    #[serde(default = "default_crew_api_token")]
    crew_api_token: String,
}

fn default_crew_api_url() -> String {
    "http://localhost:8080".to_string()
}

fn default_crew_api_token() -> String {
    "my-secret".to_string()
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

/// Handle incoming room messages for a bot
async fn handle_message(
    event: OriginalSyncRoomMessageEvent,
    room: Room,
    bot_user_id: &OwnedUserId,
    crew_config: &CrewConfig,
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

    // Get room name for display
    let room_name = room
        .display_name()
        .await
        .ok()
        .map(|n| n.to_string())
        .unwrap_or_else(|| format!("{}", room.room_id()));

    println!(
        "[{}] 📨 [{}] {} → {}",
        bot_username, room_name, event.sender, body
    );

    // Check if it's a crew message
    if is_crew_message(body) {
        let crew_content = extract_crew_content(body);
        println!("[{}] 🔍 Detected crew message: {}", bot_username, crew_content);

        // Call crew API using matrix_handler
        match matrix_handler::call_crew_api(crew_config, &crew_content).await {
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
    if let Some(ref room_id) = config.room_id {
        println!("[{}]    Room ID: {}", bot_username, room_id);
    } else {
        println!("[{}]    Mode: Auto-accept DM invitations", bot_username);
    }
    println!("[{}]    Crew API: {}", bot_username, config.crew_api_url);

    // Create Matrix client (without persistent storage for now)
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

    // If a specific room_id is provided, try to get it
    if let Some(ref room_id_str) = config.room_id {
        let room_id: OwnedRoomId = room_id_str
            .parse()
            .context("Invalid room ID format")?;

        if let Some(room) = client.get_room(&room_id) {
            let room_name = room
                .display_name()
                .await
                .ok()
                .map(|n| n.to_string())
                .unwrap_or_else(|| "Unknown".to_string());
            println!("[{}] ✅ Found room: {}", bot_username, room_name);
        } else {
            println!("[{}] ⚠️  Room {} not found, will wait for invitations", bot_username, room_id);
        }
    }

    // Create crew config
    let crew_config = Arc::new(CrewConfig {
        api_url: config.crew_api_url,
        api_token: config.crew_api_token,
    });

    let bot_username_arc = Arc::new(bot_username.clone());

    // Register event handler for room invitations (auto-accept)
    let bot_username_invite = bot_username.clone();
    client.add_event_handler(move |_event: StrippedRoomMemberEvent, room: Room| {
        let username = bot_username_invite.clone();
        async move {
            if room.state() == RoomState::Invited {
                println!("[{}] 📨 Received invitation to room {}", username, room.room_id());
                match room.join().await {
                    Ok(_) => {
                        println!("[{}] ✅ Automatically accepted invitation", username);
                    }
                    Err(e) => {
                        eprintln!("[{}] ❌ Failed to accept invitation: {}", username, e);
                    }
                }
            }
        }
    });

    // Register event handler for encrypted room messages (for debugging)
    let bot_username_encrypted = bot_username.clone();
    client.add_event_handler(
        move |event: OriginalSyncRoomEncryptedEvent, room: Room| {
            let username = bot_username_encrypted.clone();
            async move {
                println!(
                    "[{}] 🔐 Encrypted event received in room {} from {} (still encrypted - decryption may have failed)",
                    username,
                    room.room_id(),
                    event.sender
                );
            }
        },
    );

    // Register event handler for room messages
    client.add_event_handler(
        move |event: OriginalSyncRoomMessageEvent, room: Room| {
            let bot_user_id = bot_user_id.clone();
            let crew_config = Arc::clone(&crew_config);
            let bot_username = Arc::clone(&bot_username_arc);

            async move {
                if let Err(e) = handle_message(
                    event,
                    room,
                    &bot_user_id,
                    &crew_config,
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

    // Start syncing - use sync_once in a loop for better control
    let settings = SyncSettings::default().timeout(Duration::from_secs(30));

    loop {
        match client.sync_once(settings.clone()).await {
            Ok(response) => {
                // Log if we received any room events
                for (room_id, room_info) in &response.rooms.joined {
                    if !room_info.timeline.events.is_empty() {
                        println!(
                            "[{}] 📬 Received {} timeline event(s) in room {}",
                            bot_username,
                            room_info.timeline.events.len(),
                            room_id
                        );
                    }
                }
            }
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

        println!("[{}] ▶️  Starting bot... {:?}", username, config);

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
    // Set NO_PROXY to bypass proxy for localhost
    unsafe {
        std::env::set_var("NO_PROXY", "localhost,127.0.0.1");
        std::env::set_var("no_proxy", "localhost,127.0.0.1");
    }

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
