# Matrix Crew Bot

A Matrix bot that forwards crew messages to a Crew API and posts responses back to the Matrix room.

## Overview

The Crew Bot monitors a Matrix room for messages starting with `!crew ` prefix. When detected:
1. Extracts the message content (removing the `!crew ` prefix)
2. Sends it to the Crew API endpoint (`POST /api/chat`)
3. Receives the response from the Crew API
4. Posts the response back to the Matrix room

## Building

Build the bot and test client:

```bash
cargo build --bin crew-bot
cargo build --bin crew-test-client
```

Or for optimized builds:

```bash
cargo build --bin crew-bot --profile debug-opt
cargo build --bin crew-test-client --profile debug-opt
```

## Usage

### Running the Crew Bot

The bot requires several configuration parameters:

```bash
cargo run --bin crew-bot -- \
  --homeserver "https://matrix.org" \
  --username "crew_bot" \
  --password "bot_password" \
  --room-id "!abc123:matrix.org" \
  --crew-api-url "http://localhost:8080" \
  --crew-api-token "Bearer my-secret"
```

#### Command-line Options

- `-s, --homeserver <URL>` - Matrix homeserver URL
- `-u, --username <USER>` - Bot username
- `-p, --password <PASS>` - Bot password
- `-r, --room-id <ID>` - Room ID to monitor
- `-c, --crew-api-url <URL>` - Crew API base URL (default: `http://localhost:8080`)
- `-a, --crew-api-token <TOKEN>` - Authorization token (default: `Bearer my-secret`)

### Running the Test Client

The test client can send messages to test the bot:

#### Send a normal message:

```bash
cargo run --bin crew-test-client -- \
  --homeserver "https://matrix.org" \
  --username "test_user" \
  --password "test_password" \
  --room-id "!abc123:matrix.org" \
  --message "Hello, this is a normal message"
```

#### Send a crew message:

```bash
cargo run --bin crew-test-client -- \
  --homeserver "https://matrix.org" \
  --username "test_user" \
  --password "test_password" \
  --room-id "!abc123:matrix.org" \
  --message "What is the weather?" \
  --crew
```

The `--crew` flag automatically adds the `!crew ` prefix.

## Setup Instructions

### 1. Create a Bot Account

1. Register a new Matrix account for the bot
2. Join the bot to the room where it should operate
3. Note the room ID (found in room settings or by using a Matrix client)

### 2. Set Up the Crew API

Ensure your Crew API is running and accessible at the configured URL. The API should:
- Accept POST requests to `/api/chat`
- Accept JSON body: `{"message": "user message"}`
- Require Authorization header with the configured token
- Return JSON response: `{"response": "api response"}`

### 3. Run the Bot

Start the bot with the appropriate credentials and configuration.

## Message Format

### Input (from users in Matrix room):

- **Normal message**: Any text → Bot displays it but doesn't process
- **Crew message**: `!crew <content>` → Bot sends `<content>` to Crew API

### Output (from bot to Matrix room):

```
🤖 Crew Response:
<API response text>
```

## Testing Scenarios

### Scenario 1: Normal Message

**Test:**
```bash
cargo run --bin crew-test-client -- \
  -s "https://matrix.org" \
  -u "test_user" \
  -p "password" \
  -r "!room:matrix.org" \
  -m "Hello bot"
```

**Expected:** Bot receives the message, prints "Normal message (not a crew message)", does not call Crew API.

### Scenario 2: Crew Message

**Test:**
```bash
cargo run --bin crew-test-client -- \
  -s "https://matrix.org" \
  -u "test_user" \
  -p "password" \
  -r "!room:matrix.org" \
  -m "Explain quantum computing" \
  --crew
```

**Expected:**
1. Bot receives message `!crew Explain quantum computing`
2. Bot posts to Crew API with `{"message": "Explain quantum computing"}`
3. Bot receives response from API
4. Bot posts response to room as: `🤖 Crew Response:\n<API response>`

## Implementation Details

### HTTP Client

The bot uses `ureq` (not reqwest) as specified in the requirements. This is a synchronous HTTP client that's simple and reliable.

### Message Differentiation

Messages are identified as crew messages by checking if they start with `!crew ` (case-sensitive). This follows the pattern used in the TSP implementation for custom message types.

### Error Handling

If the Crew API call fails:
- The error is logged to stderr
- An error message is posted to the Matrix room
- The bot continues running

### Bot Behavior

- The bot ignores its own messages to prevent loops
- Only text messages are processed (images, files, etc. are ignored)
- The bot requires an initial sync to load room state
- Syncing continues in a loop with 30-second timeouts

## Troubleshooting

### "Room not found" error

Ensure the bot account has been invited to and has joined the room.

### "Sync error" messages

The bot will automatically retry after 5 seconds if sync fails.

### Crew API connection errors

Check that:
- The Crew API URL is correct and accessible
- The authorization token is correct
- The API is running and responding to requests

## Technical Notes

### Dependencies

- `matrix-sdk` - Matrix protocol implementation
- `ureq` - HTTP client (synchronous, not reqwest)
- `tokio` - Async runtime
- `anyhow` - Error handling
- `serde` / `serde_json` - JSON serialization

### Architecture

The bot follows the async worker pattern used in Robrix's TSP implementation:
- Main thread runs Matrix sync loop
- Event handlers process messages asynchronously
- HTTP calls to Crew API are synchronous (using ureq)
- Responses are posted back through Matrix SDK's async API
