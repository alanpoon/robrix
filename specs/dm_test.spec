spec: task
name: "DM TEST"
tags: [api, contract]
---

## Intent
Create a test of two matrix clients in the same room to test crew api.

## Decisions
- One matrix Client (testclient) invites the other (testclient2) into the direct message room.
- testclient send a message to the testclient2, testclient2 then forward the message to crew api.
- testclient2 then send back the response to testclient.

## Boundaries

### Allowed Changes
- src folder

### Forbidden


## Completion Criteria

Scenario: testclient receive Crew response
  Test: test_test_client_receive_crew_response 
  Given testclient2 forward the message to crew api:
  Then testclient2 receive response.
