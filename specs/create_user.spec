spec: task
name: Crew User
tags: [api, contract]
---

## Intent
Create a matrix user in local homeserver matrixdotorg/synapse:latest:8008:8008⁠

## Decisions
- Create random name and password for matrix user
- save the matrix user id, and password in bots.json

## Boundaries

### Allowed Changes
- src folder

### Forbidden


## Completion Criteria

Scenario: Create matrix user
  Test: Create matrix user
  Given matrix user does not exist:
  Then create the matrix user.
