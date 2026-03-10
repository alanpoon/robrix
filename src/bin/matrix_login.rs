use anyhow::{Context, Result};
use clap::Parser;
use matrix_sdk::{config::SyncSettings, Client};
use tokio::time::{timeout, Duration};
use std::path::PathBuf;

/// Simple Matrix login tool
#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
struct Args {
    /// Matrix homeserver URL
    #[arg(short = 's', long, default_value = "http://localhost:8008")]
    homeserver: String,

    /// Username (e.g., testuser or @testuser:localhost)
    #[arg(short = 'u', long, default_value = "testuser")]
    username: String,

    /// Password
    #[arg(short = 'p', long, default_value = "testpassword")]
    password: String,

    /// Login timeout in seconds
    #[arg(short = 't', long, default_value = "30")]
    timeout_secs: u64,
}

#[tokio::main]
async fn main() -> Result<()> {
    // Initialize tracing
    tracing_subscriber::fmt::init();

    let args = Args::parse();

    // Set NO_PROXY to bypass proxy for localhost
    // SAFETY: We're setting env vars at startup before any threads are spawned
    unsafe {
        std::env::set_var("NO_PROXY", "localhost,127.0.0.1");
        std::env::set_var("no_proxy", "localhost,127.0.0.1");
    }

    println!("🚀 Matrix Login Tool");
    println!("   Homeserver: {}", args.homeserver);
    println!("   Username: {}", args.username);
    println!("   Timeout: {}s", args.timeout_secs);

    // Create Matrix client with explicit SQLite path
    println!("📡 Creating Matrix client...");
    let db_path = PathBuf::from("/tmp/matrix_login_test.db");
    println!("   Using database: {:?}", db_path);

    let client_builder = Client::builder()
        .homeserver_url(&args.homeserver)
        .sqlite_store(&db_path, None);

    println!("📡 Building client...");
    let client = timeout(Duration::from_secs(10), client_builder.build()).await
        .context("Client build timed out")?
        .context("Failed to create Matrix client")?;
    println!("✅ Client created");

    // Login with timeout
    println!("🔐 Logging in...");
    let login_future = client
        .matrix_auth()
        .login_username(&args.username, &args.password)
        .initial_device_display_name("Matrix Login Tool");

    match timeout(Duration::from_secs(args.timeout_secs), login_future).await {
        Ok(Ok(_)) => println!("✅ Login request completed"),
        Ok(Err(e)) => {
            eprintln!("❌ Login failed: {}", e);
            return Err(e.into());
        }
        Err(_) => {
            eprintln!("❌ Login timed out after {}s", args.timeout_secs);
            anyhow::bail!("Login timed out");
        }
    }

    println!("✅ Successfully logged in!");
    println!("   User ID: {}", client.user_id().unwrap());
    println!("   Device ID: {:?}", client.device_id());

    // Perform initial sync to get account data
    println!("🔄 Performing initial sync...");
    client
        .sync_once(SyncSettings::default())
        .await
        .context("Initial sync failed")?;

    println!("✅ Initial sync completed!");

    // Get and display joined rooms
    let rooms = client.rooms();
    println!("\n📋 Joined rooms: {}", rooms.len());
    for room in rooms.iter().take(10) {
        let room_name = room
            .display_name()
            .await
            .ok()
            .map(|n| n.to_string())
            .unwrap_or_else(|| "Unknown".to_string());
        println!("   • {} ({})", room_name, room.room_id());
    }

    if rooms.len() > 10 {
        println!("   ... and {} more", rooms.len() - 10);
    }

    println!("\n✅ Login successful! Session details above.");

    Ok(())
}
