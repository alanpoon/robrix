# DM CLI - Direct Message Command Line Interface

A simple command-line tool for sending and receiving Matrix direct messages via stdin/stdout.

## Overview

DM CLI is a terminal-based Matrix chat client that:
1. Logs in as testuser
2. Creates or gets a DM room with another user (default: botfather)
3. Allows typing messages and sending them by pressing Enter
4. Displays incoming messages from the other user

## Building

```bash
cargo build --bin dm-cli
```

Or for optimized builds:

```bash
cargo build --bin dm-cli --profile debug-opt
```

## Usage

### Basic Usage

```bash
cargo run --bin dm-cli
```

The CLI will:
1. Login as `testuser` with password `testpassword` to `http://localhost:8008`
2. Prompt you for a user ID to invite (default: `@botfather:localhost`)
3. Create or get a DM room with that user
4. Start a chat interface where you can type messages

### Example Session

```
🚀 Matrix Direct Message CLI
   Logging in as testuser...
✅ Logged in as @testuser:localhost
🔄 Performing initial sync...

📨 Who would you like to invite? (default: @botfather:localhost)
User ID: [press Enter for default]
📤 Creating/getting DM room with @botfather:localhost...
✅ DM room ready: !abc123:localhost

💬 Chat ready! Type your message and press Enter to send.
   (Press Ctrl+C to quit)

> Hello botfather
> !crew What is the weather today?
[@botfather:localhost]: 🤖 Crew Response:
I don't have access to real-time weather data...
>
```

## Features

- **Stdin/Stdout Interface**: Simple terminal interface for chatting
- **Auto-reconnect**: Automatically handles Matrix sync in the background
- **Real-time Messages**: Displays incoming messages as they arrive
- **Clean Database**: Automatically cleans up old database on startup to avoid device conflicts

## Integration with BotFather

This CLI is designed to work seamlessly with the botfather bot:

1. Start botfather with the configuration in `bots.json`:
   ```bash
   cargo run --bin botfather
   ```

2. In another terminal, start dm-cli:
   ```bash
   cargo run --bin dm-cli
   ```

3. When prompted, press Enter to invite `@botfather:localhost` (default)

4. BotFather will automatically accept the invitation

5. Type `!crew <your message>` and press Enter to interact with the Crew API through the bot

## Configuration

The binary uses hardcoded defaults:
- **Homeserver**: `http://localhost:8008`
- **Username**: `testuser`
- **Password**: `testpassword`
- **Default Invite**: `@botfather:localhost`

To change these, edit the constants in `src/bin/dm_cli.rs`.

## Troubleshooting

**Login fails:**
- Verify the homeserver is running at `http://localhost:8008`
- Ensure the testuser account exists with the correct password
- Check that NO_PROXY is bypassing localhost

**Bot doesn't auto-accept invitation:**
- Verify botfather is running
- Check botfather logs for error messages
- Ensure botfather's `bots.json` does not have a `room_id` field (for auto-accept mode)

**Messages not appearing:**
- Wait a moment for sync to complete
- Check network connectivity
- Verify both users are in the same room

## Technical Details

- Uses Matrix SDK with SQLite storage
- Database stored at `/tmp/dm_cli.db`
- Runs sync loop in background tokio task
- Reads stdin in blocking thread and sends via mpsc channel
- Handles incoming messages via Matrix SDK event handlers

## Related Documentation

- [BOTFATHER.md](./BOTFATHER.md) - Documentation for the botfather bot manager
- [CREW_BOT.md](./CREW_BOT.md) - Documentation for the standalone crew-bot
