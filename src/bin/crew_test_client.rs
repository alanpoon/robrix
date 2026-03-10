use anyhow::{Context, Result};
use clap::Parser;
use matrix_sdk::{
    config::SyncSettings,
    ruma::{
        events::room::message::RoomMessageEventContent,
        OwnedRoomId,
    },
    Client, RoomState,
};

/// Matrix Test Client - Send messages to test the Crew Bot
#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
struct Args {
    /// Matrix homeserver URL
    #[arg(short = 's', long)]
    homeserver: String,

    /// Test user username
    #[arg(short = 'u', long)]
    username: String,

    /// Test user password
    #[arg(short = 'p', long)]
    password: String,

    /// Room ID to send messages to (e.g., !abc123:matrix.org)
    #[arg(short = 'r', long)]
    room_id: String,

    /// Message to send
    #[arg(short = 'm', long)]
    message: String,

    /// Send as a crew message (adds "!crew " prefix)
    #[arg(long)]
    crew: bool,
}

#[tokio::main]
async fn main() -> Result<()> {
    // Initialize tracing
    tracing_subscriber::fmt::init();

    let args = Args::parse();

    println!("🚀 Starting Matrix Test Client");
    println!("   Homeserver: {}", args.homeserver);
    println!("   Username: {}", args.username);
    println!("   Room ID: {}", args.room_id);

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
        .initial_device_display_name("Crew Test Client")
        .await
        .context("Failed to login")?;

    println!("✅ Logged in as {}", client.user_id().unwrap());

    // Perform initial sync to get rooms
    println!("🔄 Performing initial sync...");
    client
        .sync_once(SyncSettings::default())
        .await
        .context("Initial sync failed")?;

    // Get the room
    let room = client
        .get_room(&room_id)
        .context("Room not found - make sure you've joined the room")?;

    if room.state() != RoomState::Joined {
        anyhow::bail!("You are not in the joined state for this room. Please join the room first.");
    }

    let room_name = room.display_name().await.ok().map(|n| n.to_string()).unwrap_or_else(|| "Unknown".to_string());
    println!("✅ Found room: {}", room_name);

    // Prepare the message
    let message_text = if args.crew {
        format!("!crew {}", args.message)
    } else {
        args.message.clone()
    };

    println!("📤 Sending message: {}", message_text);

    // Send the message
    let content = RoomMessageEventContent::text_plain(&message_text);
    room.send(content).await
        .context("Failed to send message")?;

    println!("✅ Message sent successfully!");

    Ok(())
}
