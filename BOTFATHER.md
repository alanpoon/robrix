# BotFather - Matrix Bot Manager

BotFather is a multi-bot manager that reads a configuration file and spawns multiple Matrix bots in parallel. Each bot responds to `!crew` messages by calling the Crew API.

## Overview

BotFather:
1. Reads `bots.json` configuration file
2. Spawns a separate bot instance for each configuration entry
3. Each bot runs independently in its own async task
4. Dynamically adds new bots when they're added to the configuration
5. Stops bots when they're removed from the configuration
6. All bots respond to `!crew` messages using the Crew API
7. Auto-accepts DM invitations (if `room_id` is not specified)

## Building

```bash
cargo build --bin botfather
```

Or for optimized builds:

```bash
cargo build --bin botfather --profile debug-opt
```

## Usage

### 1. Create bots.json Configuration

Create a `bots.json` file in the project root with your bot configurations:

```json
[
  {
    "username": "testuser2",
    "password": "testpassword",
    "homeserver": "http://localhost:8008",
    "room_id": "!abc123:localhost",
    "crew_api_url": "http://localhost:8080",
    "crew_api_token": "my-secret"
  },
  {
    "username": "bot2",
    "password": "bot2password",
    "homeserver": "http://localhost:8008",
    "crew_api_url": "http://localhost:8080",
    "crew_api_token": "my-secret"
  }
]
```

**Note:** The `room_id` field is optional. If omitted, the bot will automatically accept all Direct Message invitations.

### 2. Run BotFather

```bash
cargo run --bin botfather
```

BotFather will:
- Read the `bots.json` file
- Start all configured bots in parallel
- Watch for file changes
- Automatically update running bots when the file changes

### 3. Managing Bots

**Add a new bot:** Edit `bots.json` and add a new configuration object. BotFather will detect the change and start the new bot.

**Remove a bot:** Remove the configuration from `bots.json`. BotFather will detect the change and stop that bot.

**Update a bot:** Remove and re-add the configuration (BotFather doesn't support hot-reloading existing bots).

### 4. Example Output

```
🤖 BotFather - Matrix Bot Manager
   Watching: bots.json

📋 Found 2 bot(s) in configuration
[testuser2] ▶️  Starting bot...
[bot2] ▶️  Starting bot...
[testuser2] 🚀 Starting bot
[testuser2]    Homeserver: http://localhost:8008
[testuser2]    Room ID: !abc123:localhost
[testuser2]    Crew API: http://localhost:8080
[testuser2] 🔐 Logging in with password...
[testuser2] ✅ Logged in as @testuser2:localhost
[testuser2] 🔄 Performing initial sync...
[testuser2] ✅ Found room: Direct Chat with testuser
[testuser2] 👂 Listening for messages...

[bot2] 🚀 Starting bot
[bot2]    Homeserver: http://localhost:8008
[bot2]    Mode: Auto-accept DM invitations
[bot2]    Crew API: http://localhost:8080
[bot2] 🔐 Logging in with password...
[bot2] ✅ Logged in as @bot2:localhost
[bot2] 👂 Listening for messages...

👀 Watching for changes to bots.json...
   (Press Ctrl+C to quit)

[testuser2] 📨 [Direct Chat with testuser] @testuser:localhost → !crew Hello
[testuser2] 🔍 Detected crew message: Hello
[testuser2] ✅ Posted response to room
```

## Configuration Fields

Each bot configuration requires:

- `username` (required): Bot's Matrix username (with or without @ prefix)
- `password` (required): Bot's password for Matrix login
- `homeserver` (required): Matrix homeserver URL (e.g., "http://localhost:8008")
- `room_id` (optional): Room ID where the bot should listen (e.g., "!abc123:localhost"). If omitted, the bot will automatically accept all Direct Message invitations.
- `crew_api_url` (optional): Crew API base URL (default: "http://localhost:8080")
- `crew_api_token` (optional): Authorization token for Crew API (default: "my-secret")

## Features

### Multi-Bot Management
- Spawns separate bot instance for each configuration entry
- Each bot runs independently in its own async task
- Hot-reload: Automatically detects changes to bots.json
- Dynamic bot lifecycle: Add/remove bots by editing the config file

### Crew API Integration
Each bot:
- Monitors messages in all joined rooms (or specific room if configured)
- Detects messages starting with `!crew `
- Extracts the message content after `!crew `
- Calls the Crew API using `matrix_handler::call_crew_api()`
- Posts the Crew API response back to the room

### Auto-Accept Invitations
When `room_id` is not specified, bots automatically:
1. Detect incoming invitations
2. Join the room
3. Start monitoring messages
4. Respond to `!crew` messages

### Message Format
Users send messages in Matrix rooms:
```
!crew What is the weather today?
```

The bot:
1. Extracts the content: "What is the weather today?"
2. Calls Crew API via `matrix_handler::call_crew_api()`
3. Receives response from Crew API
4. Posts to room: `🤖 Crew Response:\n...`

## How It Works

### File Watching
- BotFather checks `bots.json` every 2 seconds for changes
- When a change is detected, it compares the new configuration with running bots
- Bots not in the new configuration are stopped
- New bots in the configuration are started

### Bot Lifecycle
1. Each bot connects to its configured homeserver
2. Logs in with username and password
3. Performs initial sync to load room state
4. Registers event handlers for invitations and messages
5. Enters continuous sync loop

### Event Handlers

**Invitation Handler:**
- Triggered when bot is invited to a room
- Automatically accepts if `room_id` is not specified
- Prints confirmation message

**Message Handler:**
- Triggered for every message in joined rooms
- Ignores messages from the bot itself (prevents echo)
- Checks if message starts with `!crew `
- If yes:
  - Extracts crew content
  - Calls `matrix_handler::call_crew_api()` with content
  - Posts response back to room
- Prints all messages to stdout with bot username prefix

## Example Scenarios

### Scenario 1: Empty Configuration
```json
[]
```
**Result:** BotFather starts but doesn't run any bots.

### Scenario 2: Adding a Bot While Running
1. BotFather is running with an empty `bots.json`
2. You edit `bots.json` and add a bot configuration
3. BotFather detects the change
4. BotFather starts the new bot
5. The bot begins listening for `!crew` messages

### Scenario 3: Multiple Bots
```json
[
  {"username": "bot1", "password": "pass1", "homeserver": "http://localhost:8008"},
  {"username": "bot2", "password": "pass2", "homeserver": "http://localhost:8008"},
  {"username": "bot3", "password": "pass3", "homeserver": "http://localhost:8008"}
]
```
**Result:** All three bots run in parallel, each in its own async task, each monitoring its rooms and responding to `!crew` messages.

## Logging

Each bot logs with a prefix indicating its username:
```
[testuser2] 🚀 Starting bot
[testuser2] 📨 [Direct Chat with testuser] @user:localhost → !crew hello
[testuser2] 🔍 Detected crew message: hello
[testuser2] ✅ Posted response to room
```

## Error Handling

- If a bot's sync fails, it retries after 5 seconds
- If the Crew API is unreachable, the bot posts an error message to the room
- If `bots.json` is malformed, BotFather logs the error and continues watching
- Individual bot failures don't affect other running bots

## Use Cases

### Crew API Gateway
Run multiple bots that serve as Matrix gateways to the Crew API:
- Each bot monitors different rooms or users
- Users send `!crew` messages to interact with AI
- Bots forward requests to Crew API and return responses

### Integration with DM CLI
1. Terminal 1: Configure and run `cargo run --bin botfather`
2. Terminal 2: Run `cargo run --bin dm-cli-to-bot`
3. Send `!crew` messages from dm-cli-to-bot
4. Bot receives, calls Crew API, and responds

### Multi-Room Bot Deployment
Deploy bots across multiple rooms:
- Support bot in #support room
- Dev bot in #development room
- General bot accepting all DM invitations

## Troubleshooting

**Bot doesn't start:**
- Verify the username/password is correct
- Check that the homeserver URL is correct
- Ensure the bot account exists on the homeserver

**Bot doesn't respond to messages:**
- Verify messages start with `!crew ` (with space)
- Check that the Crew API is running and accessible
- Look for error messages in bot logs

**File changes not detected:**
- BotFather checks every 2 seconds; wait a moment
- Verify `bots.json` is in the current directory
- Check file permissions

**Crew API errors:**
- Verify crew_api_url is correct
- Check that crew_api_token matches the API server's token
- Ensure Crew API server is running

## Related Documentation

- [DM_CLI.md](./DM_CLI.md) - Interactive command-line Matrix client
- [CREW_BOT.md](./CREW_BOT.md) - Standalone crew-bot that responds to !crew messages
