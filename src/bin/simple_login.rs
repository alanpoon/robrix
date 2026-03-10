use anyhow::{Context, Result};
use clap::Parser;
use serde::{Deserialize, Serialize};

/// Simple Matrix login using raw HTTP
#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
struct Args {
    /// Matrix homeserver URL
    #[arg(short = 's', long, default_value = "http://localhost:8008")]
    homeserver: String,

    /// Username (without @ prefix)
    #[arg(short = 'u', long, default_value = "testuser")]
    username: String,

    /// Password
    #[arg(short = 'p', long, default_value = "testpassword")]
    password: String,
}

#[derive(Debug, Serialize)]
struct LoginRequest {
    #[serde(rename = "type")]
    login_type: String,
    identifier: Identifier,
    password: String,
}

#[derive(Debug, Serialize)]
struct Identifier {
    #[serde(rename = "type")]
    id_type: String,
    user: String,
}

#[derive(Debug, Deserialize)]
struct LoginResponse {
    user_id: String,
    access_token: String,
    home_server: String,
    device_id: String,
}

fn main() -> Result<()> {
    let args = Args::parse();

    println!("🚀 Simple Matrix Login");
    println!("   Homeserver: {}", args.homeserver);
    println!("   Username: {}", args.username);

    let login_url = format!("{}/_matrix/client/v3/login", args.homeserver);

    let request = LoginRequest {
        login_type: "m.login.password".to_string(),
        identifier: Identifier {
            id_type: "m.id.user".to_string(),
            user: args.username.clone(),
        },
        password: args.password.clone(),
    };

    println!("🔐 Logging in...");
    let response = ureq::post(&login_url)
        .set("Content-Type", "application/json")
        .send_json(&request)
        .context("Failed to send login request")?;

    let login_response: LoginResponse = response
        .into_json()
        .context("Failed to parse login response")?;

    println!("\n✅ Successfully logged in!");
    println!("   User ID: {}", login_response.user_id);
    println!("   Device ID: {}", login_response.device_id);
    println!("   Access Token: {}", &login_response.access_token[..20]);
    println!("   Home Server: {}", login_response.home_server);

    Ok(())
}
