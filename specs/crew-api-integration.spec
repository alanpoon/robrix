spec: task
name: "matrix crew bot"
tags: [api, contract]
---

## Intent

Create a new matrix bot binary using matrix sdk to send Crew message to the crew api, and then post the crew response back to the matrix room as a message.

## Decisions

- Create a new matrix bot binary using matrix sdk, that is able to differentiate Crew message from normal message. See Tsp example.
- Create a crew message with a message, but will fully display the crew response inside the matrix message. 
- Use `POST /api/chat` using http library that is not reqwest.
- create test matrix client that can send message to the matrix bot within the same room.

## Boundaries

### Allowed Changes
- src folder

### Forbidden


## Completion Criteria

Scenario: Matrix bot receive normal message
  Test: While matrix bot is running, send a normal message to the matrix bot
  Given normal message is not crew:
  Then display normal message.

Scenario: Matrix bot receive crew message
  Test: While matrix bot is running, send a crew message to the matrix bot
  Given it is a crew message:
  Then matrix bot will post the crew message to the crew api
  And then matrix bot will post the crew response back to the matrix room as a crew message.

