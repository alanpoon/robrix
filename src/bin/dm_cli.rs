use anyhow::{Context, Result};
use matrix_sdk::{
    config::SyncSettings,
    ruma::{
        events::room::message::{
            MessageType, OriginalSyncRoomMessageEvent, RoomMessageEventContent,
        },
        UserId,
    },
    Client, Room,
};
use std::io::{self, BufRead, Write};
use tokio::sync::mpsc;
use tokio::time::Duration;

const HOMESERVER: &str = "http://localhost:8008";
const USERNAME: &str = "testuser";
const PASSWORD: &str = "testpassword";

/// Handle incoming messages and print them to stdout
async fn handle_message(
    event: OriginalSyncRoomMessageEvent,
    _room: Room,
    my_user_id: &UserId,
) {
    // Don't print messages from ourselves
    if event.sender == my_user_id {
        return;
    }

    if let MessageType::Text(text_content) = &event.content.msgtype {
        let message = &text_content.body;
        println!("\n[{}]: {}", event.sender, message);
        print!("> ");
        io::stdout().flush().unwrap();
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

    println!("🚀 Matrix Direct Message CLI");
    println!("   Logging in as {}...", USERNAME);

    // Clean up old database to avoid device mismatch
    let db_path = "/tmp/dm_cli.db";
    let _ = std::fs::remove_dir_all(db_path);

    // Create client
    let client = Client::builder()
        .homeserver_url(HOMESERVER)
        .sqlite_store(db_path, None)
        .build()
        .await
        .context("Failed to create Matrix client")?;

    // Login
    client
        .matrix_auth()
        .login_username(USERNAME, PASSWORD)
        .initial_device_display_name("DM CLI")
        .await
        .context("Failed to login")?;

    let my_user_id = client.user_id().unwrap().to_owned();
    println!("✅ Logged in as {}", my_user_id);

    // Perform initial sync
    println!("🔄 Performing initial sync...");
    client.sync_once(SyncSettings::default()).await?;

    // Get botfather's user ID
    println!("\n📨 Who would you like to invite? (default: @botfather:localhost)");
    print!("User ID: ");
    io::stdout().flush()?;

    let mut input = String::new();
    io::stdin().read_line(&mut input)?;
    let invite_user = input.trim();
    let invite_user = if invite_user.is_empty() {
        "@botfather:localhost"
    } else {
        invite_user
    };

    let botfather_id: Box<UserId> = invite_user.parse()
        .context("Invalid user ID format")?;

    println!("📤 Creating/getting DM room with {}...", botfather_id);

    let room = client
        .create_dm(&botfather_id)
        .await
        .context("Failed to create DM room")?;

    let room_id = room.room_id().to_owned();
    println!("✅ DM room ready: {}", room_id);

    // Set up event handler for incoming messages
    let my_user_id_clone = my_user_id.clone();
    client.add_event_handler(move |event: OriginalSyncRoomMessageEvent, room: Room| {
        let user_id = my_user_id_clone.clone();
        async move {
            handle_message(event, room, &user_id).await;
        }
    });

    // Channel to send messages from stdin thread to async runtime
    let (message_tx, mut message_rx) = mpsc::unbounded_channel::<String>();

    // Start sync loop in background
    let client_clone = client.clone();
    let sync_handle = tokio::spawn(async move {
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
    let stdin_handle = std::thread::spawn(move || {
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

    println!("\n💬 Chat ready! Type your message and press Enter to send.");
    println!("   (Press Ctrl+C to quit)\n");
    print!("> ");
    io::stdout().flush()?;

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
                // Message sent successfully
            }
            Err(e) => {
                eprintln!("❌ Failed to send message: {}", e);
            }
        }

        print!("> ");
        io::stdout().flush()?;
    }

    // Cleanup
    sync_handle.abort();
    drop(stdin_handle);

    Ok(())
}
