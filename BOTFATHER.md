# BotFather - Matrix Bot Manager

BotFather is a Matrix bot manager that watches a configuration file and automatically starts, stops, and manages multiple Matrix crew bots in parallel.

## Overview

BotFather monitors a `bots.json` file and:
1. Starts a separate bot instance for each configuration entry
2. Each bot runs independently in its own thread
3. Dynamically adds new bots when they're added to the configuration
4. Stops bots when they're removed from the configuration
5. All bots apply the crew bot logic (respond to `!crew` messages)

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
    "username": "crew_bot_1",
    "password": "your_password_here_1",
    "homeserver": "https://matrix.org",
    "room_id": "!roomid1:matrix.org",
    "crew_api_url": "http://localhost:8080",
    "crew_api_token": "Bearer my-secret"
  },
  {
    "username": "crew_bot_2",
    "password": "your_password_here_2",
    "homeserver": "https://matrix.org",
    "room_id": "!roomid2:matrix.org",
    "crew_api_url": "http://localhost:8080",
    "crew_api_token": "Bearer my-secret"
  }
]
```

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

## Configuration Fields

Each bot configuration requires:

- `username` (required): Bot's Matrix username (with or without @ prefix)
- `password` (required): Bot's password for Matrix login
- `homeserver` (required): Matrix homeserver URL (e.g., "https://matrix.org")
- `room_id` (required): Room ID where the bot should listen (e.g., "!abc123:matrix.org")
- `crew_api_url` (optional): Crew API base URL (default: "http://localhost:8080")
- `crew_api_token` (optional): Authorization token for Crew API (default: "Bearer my-secret")

## Setting Up Bot Accounts

To create a bot account, you'll need to register it on your Matrix homeserver:

### Method 1: Using matrix-sdk (for Synapse)
```bash
docker exec <synapse-container> register_new_matrix_user \
  -u bot_username \
  -p bot_password \
  -c /data/homeserver.yaml \
  http://localhost:8008
```

### Method 2: Using matrix-commander
```bash
matrix-commander --login password --user @botname:matrix.org
# Follow the prompts, then find the access token in the credentials file
```

### Method 3: Using curl
```bash
curl -X POST "https://matrix.org/_matrix/client/v3/login" \
  -H "Content-Type: application/json" \
  -d '{
    "type": "m.login.password",
    "identifier": {
      "type": "m.id.user",
      "user": "botname"
    },
    "password": "bot_password"
  }'
```

The response will include the `access_token`.

## How It Works

### File Watching
- BotFather checks `bots.json` every 2 seconds for changes
- When a change is detected, it compares the new configuration with running bots
- Bots not in the new configuration are stopped
- New bots in the configuration are started

### Bot Lifecycle
1. Each bot connects to its configured homeserver using the access token
2. Performs initial sync to load room state
3. Listens for room messages in the configured room
4. When a message starts with `!crew `, sends it to the Crew API
5. Posts the Crew API response back to the room

### Crew Message Format

Users send messages in Matrix rooms:
```
!crew What is the weather today?
```

The bot:
1. Extracts the content: "What is the weather today?"
2. Sends POST to Crew API: `{"message": "What is the weather today?"}`
3. Receives response from Crew API: `{"response": "..."}`
4. Posts to room: `🤖 Crew Response:\n...`

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
  {"username": "bot1", "access_token": "token1", ...},
  {"username": "bot2", "access_token": "token2", ...},
  {"username": "bot3", "access_token": "token3", ...}
]
```
**Result:** All three bots run in parallel, each in its own thread, each monitoring its configured room.

## Logging

Each bot logs with a prefix indicating its username:
```
[crew_bot_1] 🚀 Starting bot
[crew_bot_1] 📨 Received message from @user:matrix.org: !crew hello
[crew_bot_2] 🔍 Detected crew message: hello
```

## Error Handling

- If a bot's sync fails, it retries after 5 seconds
- If the Crew API is unreachable, the bot posts an error message to the room
- If `bots.json` is malformed, BotFather logs the error and continues watching
- Individual bot failures don't affect other running bots

## Limitations

- Access tokens must be obtained manually (no `/new_bot` command yet)
- Bots cannot be updated in-place (must remove and re-add)
- All bots must use the same Crew API endpoint (or configure separately)
- No database for storing bot configurations (file-based only)

## Future Enhancements

According to the spec, future versions should include:
- `/new_bot` command to create new bots and return tokens
- Database storage for bot configurations
- Docker setup with local Synapse homeserver for testing
- Hot-reloading of bot configurations without restart

## Troubleshooting

**Bot doesn't start:**
- Verify the access token is valid
- Check that the homeserver URL is correct
- Ensure the bot account has joined the room

**Bot doesn't respond to messages:**
- Verify messages start with `!crew ` (with space)
- Check that the Crew API is running and accessible
- Look for error messages in bot logs

**File changes not detected:**
- BotFather checks every 2 seconds; wait a moment
- Verify `bots.json` is in the current directory
- Check file permissions

## Related Documentation

- [CREW_BOT.md](./CREW_BOT.md) - Documentation for the standalone crew-bot
- [specs/bot-father.spec](./specs/bot-father.spec) - Original specification
