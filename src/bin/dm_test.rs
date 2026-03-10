use anyhow::{Context, Result};
use matrix_sdk::{
    config::SyncSettings,
    ruma::{
        events::room::message::{
            MessageType, OriginalSyncRoomMessageEvent, RoomMessageEventContent,
        },
        UserId,
    },
    Client, Room, RoomState,
};
use robrix::crew::matrix_handler::{self, CrewConfig};
use tokio::sync::mpsc;
use tokio::time::{sleep, timeout, Duration};

const HOMESERVER: &str = "http://localhost:8008";
const USER1_NAME: &str = "testuser";
const USER2_NAME: &str = "testuser2";
const PASSWORD: &str = "testpassword";
const TEST_MESSAGE: &str = "Hello from testuser! This is a test message.";

/// Create and login a Matrix client
async fn create_client(username: &str, password: &str, db_path: &str) -> Result<Client> {
    let client = Client::builder()
        .homeserver_url(HOMESERVER)
        .sqlite_store(db_path, None)
        .build()
        .await
        .context("Failed to create Matrix client")?;

    println!("[{}] Logging in...", username);
    client
        .matrix_auth()
        .login_username(username, password)
        .initial_device_display_name(&format!("DM Test - {}", username))
        .await
        .context("Failed to login")?;

    println!("[{}] ✅ Logged in as {}", username, client.user_id().unwrap());

    Ok(client)
}

/// Handle incoming messages for user2
async fn handle_message(
    event: OriginalSyncRoomMessageEvent,
    _room: Room,
    expected_sender: &UserId,
    received_tx: mpsc::UnboundedSender<String>,
) {
    if event.sender == expected_sender {
        if let MessageType::Text(text_content) = &event.content.msgtype {
            let message = &text_content.body;
            println!("\n[testuser2] 📨 Received message from {}: \"{}\"", event.sender, message);
            let _ = received_tx.send(message.clone());
        }
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    // Set NO_PROXY to bypass proxy for localhost - MUST be first
    unsafe {
        std::env::set_var("NO_PROXY", "localhost,127.0.0.1");
        std::env::set_var("no_proxy", "localhost,127.0.0.1");
    }

    tracing_subscriber::fmt::init();

    println!("🚀 Starting Direct Message Test");
    println!("   User 1: {} (sender)", USER1_NAME);
    println!("   User 2: {} (receiver)", USER2_NAME);
    println!();

    // Clean up old databases to avoid device mismatch errors
    println!("🧹 Cleaning up old databases...");
    let _ = std::fs::remove_dir_all("/tmp/dm_test_user1.db");
    let _ = std::fs::remove_dir_all("/tmp/dm_test_user2.db");
    println!();

    // Create clients for both users
    let client1 = create_client(USER1_NAME, PASSWORD, "/tmp/dm_test_user1.db").await?;
    let client2 = create_client(USER2_NAME, PASSWORD, "/tmp/dm_test_user2.db").await?;

    let user1_id = client1.user_id().unwrap().to_owned();
    let user2_id = client2.user_id().unwrap().to_owned();

    println!();
    println!("[testuser] Performing initial sync...");
    client1.sync_once(SyncSettings::default()).await?;

    println!("[testuser2] Performing initial sync...");
    client2.sync_once(SyncSettings::default()).await?;

    // Channels to signal when messages are received
    let (user2_received_tx, mut user2_received_rx) = mpsc::unbounded_channel::<String>();
    let (user1_received_tx, mut user1_received_rx) = mpsc::unbounded_channel::<String>();

    // Set up event handler for user2 to receive messages from user1
    let user1_id_clone = user1_id.clone();
    client2.add_event_handler(move |event: OriginalSyncRoomMessageEvent, room: Room| {
        let tx = user2_received_tx.clone();
        let sender_id = user1_id_clone.clone();
        async move {
            handle_message(event, room, &sender_id, tx).await;
        }
    });

    // Set up event handler for user1 to receive messages from user2
    let user2_id_clone = user2_id.clone();
    client1.add_event_handler(move |event: OriginalSyncRoomMessageEvent, room: Room| {
        let tx = user1_received_tx.clone();
        let sender_id = user2_id_clone.clone();
        async move {
            handle_message(event, room, &sender_id, tx).await;
        }
    });

    // Start syncing for user2 in background
    let client2_clone = client2.clone();
    let _sync_handle2 = tokio::spawn(async move {
        println!("[testuser2] 👂 Started listening for messages...");
        let settings = SyncSettings::default().timeout(Duration::from_secs(30));
        loop {
            match client2_clone.sync_once(settings.clone()).await {
                Ok(_) => {},
                Err(e) => {
                    eprintln!("[testuser2] Sync error: {}", e);
                    sleep(Duration::from_secs(2)).await;
                }
            }
        }
    });

    // Start syncing for user1 in background
    let client1_clone = client1.clone();
    let _sync_handle1 = tokio::spawn(async move {
        println!("[testuser] 👂 Started listening for messages...");
        let settings = SyncSettings::default().timeout(Duration::from_secs(30));
        loop {
            match client1_clone.sync_once(settings.clone()).await {
                Ok(_) => {},
                Err(e) => {
                    eprintln!("[testuser] Sync error: {}", e);
                    sleep(Duration::from_secs(2)).await;
                }
            }
        }
    });

    // Give both users time to start listening
    sleep(Duration::from_secs(1)).await;

    // Create or get direct message room
    println!();
    println!("[testuser] Creating direct message room with testuser2...");

    let room = client1
        .create_dm(&user2_id)
        .await
        .context("Failed to create DM room")?;

    let room_id = room.room_id().to_owned();
    println!("[testuser] ✅ DM room created: {}", room_id);

    // Wait for user2 to join
    println!("[testuser2] Waiting for room invitation...");
    sleep(Duration::from_secs(2)).await;

    // Sync user2 to get the invitation
    client2.sync_once(SyncSettings::default()).await?;

    // Get the room from user2's perspective and join if invited
    if let Some(room2) = client2.get_room(&room_id) {
        match room2.state() {
            RoomState::Invited => {
                println!("[testuser2] Accepting invitation...");
                room2.join().await.context("Failed to join room")?;
                println!("[testuser2] ✅ Joined the room");

                // Sync to confirm join
                sleep(Duration::from_secs(1)).await;
                client2.sync_once(SyncSettings::default()).await?;
            }
            RoomState::Joined => {
                println!("[testuser2] ✅ Already in the room");
            }
            _ => {
                println!("[testuser2] ⚠️ Room state: {:?}", room2.state());
            }
        }
    } else {
        println!("[testuser2] ⏳ Waiting for room to appear...");
        sleep(Duration::from_secs(2)).await;
        client2.sync_once(SyncSettings::default()).await?;

        if let Some(room2) = client2.get_room(&room_id) {
            if room2.state() == RoomState::Invited {
                println!("[testuser2] Accepting invitation...");
                room2.join().await.context("Failed to join room")?;
                println!("[testuser2] ✅ Joined the room");
                sleep(Duration::from_secs(1)).await;
                client2.sync_once(SyncSettings::default()).await?;
            }
        }
    }

    // Wait for room keys to be established and encryption to be ready
    println!("[testuser] ⏳ Waiting for encryption to be ready...");
    sleep(Duration::from_secs(5)).await;

    // Do additional syncs to ensure keys are exchanged
    println!("[testuser] 🔄 Syncing to establish encryption keys...");
    client1.sync_once(SyncSettings::default()).await?;
    client2.sync_once(SyncSettings::default()).await?;
    sleep(Duration::from_secs(2)).await;

    // Send message from user1
    println!();
    println!("[testuser] 📤 Sending message: \"{}\"", TEST_MESSAGE);
    let content = RoomMessageEventContent::text_plain(TEST_MESSAGE);
    room.send(content).await.context("Failed to send message")?;
    println!("[testuser] ✅ Message sent");

    // Give time for the message and keys to propagate
    println!("[testuser2] ⏳ Allowing time for key exchange and message delivery...");
    sleep(Duration::from_secs(3)).await;

    // Wait for user2 to receive the message
    println!("[testuser2] ⏳ Waiting for message...");

    match timeout(Duration::from_secs(20), user2_received_rx.recv()).await {
        Ok(Some(message)) => {
            println!();
            println!("✅ SUCCESS! Message received by testuser2");
            println!("   Message content: \"{}\"", message);

            // Get the room from user2's perspective to send reply
            let room2 = client2.get_room(&room_id)
                .context("Failed to get room for user2")?;

            println!();
            println!("[testuser2] 📡 Calling Crew API at http://127.0.0.1:8080/api/chat...");

            // Use the crew module to call API and send response
            let crew_config = CrewConfig {
                api_url: "http://127.0.0.1:8080".to_string(),
                api_token: "my-secret".to_string(),
            };

            match matrix_handler::call_crew_api(&crew_config, "give me streaming response").await {
                Ok(content) => {
                    println!("[testuser2] ✅ API returned content: \"{}\"", content);

                    // Send the content back to testuser as a Matrix message
                    println!("[testuser2] 📤 Sending API response back to testuser...");
                    let reply_content = RoomMessageEventContent::text_plain(&content);
                    room2.send(reply_content).await.context("Failed to send reply message")?;
                    println!("[testuser2] ✅ Reply sent");

                    // Wait for testuser to receive the reply
                    println!();
                    println!("[testuser] ⏳ Waiting for reply message...");

                    match timeout(Duration::from_secs(20), user1_received_rx.recv()).await {
                        Ok(Some(reply_message)) => {
                            println!();
                            println!("✅ SUCCESS! Reply received by testuser");
                            println!("   Reply content: \"{}\"", reply_message);
                        }
                        Ok(None) => {
                            eprintln!("❌ Channel closed without receiving reply");
                        }
                        Err(_) => {
                            eprintln!("❌ Timeout waiting for reply");
                        }
                    }
                }
                Err(e) => {
                    eprintln!("[testuser2] ❌ Crew API error: {}", e);
                }
            }
        }
        Ok(None) => {
            eprintln!("❌ Channel closed without receiving message");
        }
        Err(_) => {
            eprintln!("❌ Timeout waiting for message");
        }
    }

    // Teardown
    println!();
    println!("🧹 Tearing down...");

    // Note: Sync handles will be automatically dropped when they go out of scope

    println!("   Cleaning up databases...");
    drop(client1);
    drop(client2);

    sleep(Duration::from_secs(1)).await;

    // Clean up database files
    let _ = std::fs::remove_dir_all("/tmp/dm_test_user1.db");
    let _ = std::fs::remove_dir_all("/tmp/dm_test_user2.db");

    println!("✅ Teardown complete");
    println!();
    println!("🎉 Direct message test completed successfully!");

    Ok(())
}
