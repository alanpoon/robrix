spec: task
name: "Botfather"
tags: [api, contract]
---

## Intent
Replicate the @botfather from telegram backend server in rust but for matrix bot client.

## Decisions
- create a new binary "botfather" that watches a file containing a list of bot's username and access_token.
- /new_bot command to create a new bot and return the bot token.
- /new_bot command will create a new bot in the database.
- /new_bot command will create a new bot in the matrix server.
- /new_bot command will return the bot token.
- setup a new docker with synapse to test in local matrix homeserver
- botfather will start all the bots in the file.
- botfather will watch the file for changes and add / remove the bots if the file (bots.json) changes.
- botfather will start the bots in parallel.
- Create a module for crew bots that contains its logic.

## Boundaries

### Allowed Changes
- src folder

### Forbidden


## Completion Criteria

Scenario: bots.json is empty
  Test: bots.json is empty
  Given bots.json is empty:
  Then do nothing.

Scenario: bots.json is changed
  Test: continuously edit botfather.rs and dm_cli_to_bot.rs, and run them. 
  Given stdin dm_cli_to_bot.rs with "!crew hello".
  Then the testuser2 bot in botfather should be able to call crew api and then send back response to testuser in               
  dm_cli_to_bot.rs