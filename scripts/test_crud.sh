#!/usr/bin/env bash
set -e

echo "Building Docker image with Nix..."
nix build .#docker

echo "Loading Docker image..."
docker load < result

echo "Starting services..."
docker compose up -d

echo "Waiting for webserver to be ready..."
for i in {1..30}; do
  if curl -s http://localhost:8000/swagger-ui/ > /dev/null 2>&1; then
    echo "Webserver is ready!"
    break
  fi
  echo "Waiting... ($i/30)"
  sleep 1
done

# Function to get access token from Keycloak
get_access_token() {
  local USERNAME="testuser"
  local PASSWORD="testpass"
  local CLIENT_ID="prompt-backend"
  local KEYCLOAK_URL="http://localhost:8080/realms/oauth2-realm/protocol/openid-connect/token"

  local TOKEN_RESPONSE=$(curl -s -X POST "$KEYCLOAK_URL" \
    -H "Content-Type: application/x-www-form-urlencoded" \
    -d "grant_type=password" \
    -d "client_id=$CLIENT_ID" \
    -d "username=$USERNAME" \
    -d "password=$PASSWORD")

  local ACCESS_TOKEN=$(echo "$TOKEN_RESPONSE" | grep -o '"access_token":"[^"]*"' | cut -d'"' -f4)

  if [ -z "$ACCESS_TOKEN" ]; then
    echo "Failed to get access token. Response: $TOKEN_RESPONSE"
    return 1
  fi

  echo "$ACCESS_TOKEN"
}

# Test 0: Verify authentication works
echo ""
echo "Test 0: Verifying authentication..."
ACCESS_TOKEN=$(get_access_token)
if [ -z "$ACCESS_TOKEN" ]; then
  echo "✗ Test 0 failed: Could not obtain access token"
  docker compose down -v
  exit 1
fi
echo "✓ Test 0 passed: Successfully obtained access token"
echo "Token (first 20 chars): ${ACCESS_TOKEN:0:20}..."

# Test 1: Create a session
echo ""
echo "Test 1: Creating a session..."
CREATE_RESPONSE=$(curl -s -X POST http://localhost:8000/sessions \
  -H "Content-Type: application/json" \
  -H "Authorization: Bearer $ACCESS_TOKEN" \
  -d '{"repo": "test-repo", "target_branch": "main", "messages": {"content": "test message"}}')
echo "Create response: $CREATE_RESPONSE"

if echo "$CREATE_RESPONSE" | grep -q '"success":true'; then
  echo "✓ Test 1 passed: Session created successfully"
  SESSION_ID=$(echo "$CREATE_RESPONSE" | grep -o '"id":"[^"]*"' | cut -d'"' -f4)
  echo "Session ID: $SESSION_ID"
else
  echo "✗ Test 1 failed: Failed to create session"
  docker compose down -v
  exit 1
fi

# Test 2: List sessions
echo ""
echo "Test 2: Listing sessions..."
LIST_RESPONSE=$(curl -s http://localhost:8000/sessions \
  -H "Authorization: Bearer $ACCESS_TOKEN")
echo "List response: $LIST_RESPONSE"

if echo "$LIST_RESPONSE" | grep -q "$SESSION_ID"; then
  echo "✓ Test 2 passed: Session found in list"
else
  echo "✗ Test 2 failed: Session not found in list"
  docker compose down -v
  exit 1
fi

# Test 3: Read a specific session
echo ""
echo "Test 3: Reading session by ID..."
READ_RESPONSE=$(curl -s http://localhost:8000/sessions/$SESSION_ID \
  -H "Authorization: Bearer $ACCESS_TOKEN")
echo "Read response: $READ_RESPONSE"

if echo "$READ_RESPONSE" | grep -q "$SESSION_ID"; then
  echo "✓ Test 3 passed: Session read successfully"
else
  echo "✗ Test 3 failed: Failed to read session"
  docker compose down -v
  exit 1
fi

# Test 4: Update the session
echo ""
echo "Test 4: Updating the session..."
UPDATE_RESPONSE=$(curl -s -X PUT http://localhost:8000/sessions/$SESSION_ID \
  -H "Content-Type: application/json" \
  -H "Authorization: Bearer $ACCESS_TOKEN" \
  -d "{\"id\": \"$SESSION_ID\", \"session_status\": \"Archived\", \"sbx_config\": {\"setting\": \"new_value\"}}")
echo "Update response: $UPDATE_RESPONSE"

if echo "$UPDATE_RESPONSE" | grep -q '"success":true'; then
  echo "✓ Test 4 passed: Session updated successfully"
else
  echo "✗ Test 4 failed: Failed to update session"
  docker compose down -v
  exit 1
fi

# Test 5: Verify update by reading
echo ""
echo "Test 5: Verifying update..."
READ_RESPONSE2=$(curl -s http://localhost:8000/sessions/$SESSION_ID \
  -H "Authorization: Bearer $ACCESS_TOKEN")
echo "Read response after update: $READ_RESPONSE2"

if echo "$READ_RESPONSE2" | grep -qi '"session_status":"[Aa]rchived"'; then
  echo "✓ Test 5 passed: Update verified"
else
  echo "✗ Test 5 failed: Update not reflected"
  docker compose down -v
  exit 1
fi

# Test 6: Create another session
echo ""
echo "Test 6: Creating another session for deletion test..."
CREATE_RESPONSE2=$(curl -s -X POST http://localhost:8000/sessions \
  -H "Content-Type: application/json" \
  -H "Authorization: Bearer $ACCESS_TOKEN" \
  -d '{"repo": "test-repo", "target_branch": "main", "messages": {"content": "delete me"}}')
echo "Create response: $CREATE_RESPONSE2"

if echo "$CREATE_RESPONSE2" | grep -q '"success":true'; then
  SESSION_ID2=$(echo "$CREATE_RESPONSE2" | grep -o '"id":"[^"]*"' | cut -d'"' -f4)
  echo "✓ Test 6 passed: Second session created (ID: $SESSION_ID2)"
else
  echo "✗ Test 6 failed: Failed to create second session"
  docker compose down -v
  exit 1
fi

# Test 7: Delete the second session
echo ""
echo "Test 7: Deleting the second session..."
DELETE_RESPONSE=$(curl -s -X DELETE http://localhost:8000/sessions/$SESSION_ID2 \
  -H "Authorization: Bearer $ACCESS_TOKEN")
echo "Delete response: $DELETE_RESPONSE"

if echo "$DELETE_RESPONSE" | grep -q '"success":true'; then
  echo "✓ Test 7 passed: Session deleted successfully"
else
  echo "✗ Test 7 failed: Failed to delete session"
  docker compose down -v
  exit 1
fi

# Test 8: Verify deletion
echo ""
echo "Test 8: Verifying deletion..."
READ_DELETED=$(curl -s -w "\n%{http_code}" http://localhost:8000/sessions/$SESSION_ID2 \
  -H "Authorization: Bearer $ACCESS_TOKEN")
HTTP_CODE=$(echo "$READ_DELETED" | tail -n1)
echo "HTTP code when reading deleted session: $HTTP_CODE"

if [ "$HTTP_CODE" = "404" ] || [ "$HTTP_CODE" = "500" ]; then
  echo "✓ Test 8 passed: Deleted session not found (HTTP $HTTP_CODE)"
else
  echo "✗ Test 8 failed: Deleted session still accessible"
  docker compose down -v
  exit 1
fi

# Test 9: Verify first session still exists
echo ""
echo "Test 9: Verifying first session still exists..."
READ_RESPONSE3=$(curl -s http://localhost:8000/sessions/$SESSION_ID \
  -H "Authorization: Bearer $ACCESS_TOKEN")
echo "Read response for first session: $READ_RESPONSE3"

if echo "$READ_RESPONSE3" | grep -q "$SESSION_ID"; then
  echo "✓ Test 9 passed: First session still exists"
else
  echo "✗ Test 9 failed: First session was incorrectly deleted"
  docker compose down -v
  exit 1
fi

# Test 10: Create session with prompt (new combined endpoint)
echo ""
echo "Test 10: Creating session with initial prompt using combined endpoint..."
CREATE_WITH_PROMPT_RESPONSE=$(curl -s -X POST http://localhost:8000/sessions/with-prompt \
  -H "Content-Type: application/json" \
  -H "Authorization: Bearer $ACCESS_TOKEN" \
  -d '{"repo": "test-repo", "target_branch": "main", "messages": {"content": "Fix authentication bug"}, "parent_id": null}')
echo "Create with prompt response: $CREATE_WITH_PROMPT_RESPONSE"

if echo "$CREATE_WITH_PROMPT_RESPONSE" | grep -q '"success":true'; then
  echo "✓ Test 10 passed: Session with prompt created successfully"
  SESSION_WITH_PROMPT_ID=$(echo "$CREATE_WITH_PROMPT_RESPONSE" | grep -o '"sessionId":"[^"]*"' | cut -d'"' -f4)
  PROMPT_ID=$(echo "$CREATE_WITH_PROMPT_RESPONSE" | grep -o '"promptId":"[^"]*"' | cut -d'"' -f4)
  echo "Session ID: $SESSION_WITH_PROMPT_ID"
  echo "Prompt ID: $PROMPT_ID"
else
  echo "✗ Test 10 failed: Failed to create session with prompt"
  docker compose down -v
  exit 1
fi

# Test 11: Verify session was created
echo ""
echo "Test 11: Verifying session was created..."
READ_SESSION_WITH_PROMPT=$(curl -s http://localhost:8000/sessions/$SESSION_WITH_PROMPT_ID \
  -H "Authorization: Bearer $ACCESS_TOKEN")
echo "Read session response: $READ_SESSION_WITH_PROMPT"

if echo "$READ_SESSION_WITH_PROMPT" | grep -q "$SESSION_WITH_PROMPT_ID"; then
  echo "✓ Test 11 passed: Session verified"
else
  echo "✗ Test 11 failed: Session not found"
  docker compose down -v
  exit 1
fi

# Test 12: Verify prompt was created and associated with session
echo ""
echo "Test 12: Verifying prompt was created..."
READ_PROMPT=$(curl -s http://localhost:8000/prompts/$PROMPT_ID \
  -H "Authorization: Bearer $ACCESS_TOKEN")
echo "Read prompt response: $READ_PROMPT"

if echo "$READ_PROMPT" | grep -q "$PROMPT_ID" && echo "$READ_PROMPT" | grep -q "$SESSION_WITH_PROMPT_ID"; then
  echo "✓ Test 12 passed: Prompt verified and associated with session"
else
  echo "✗ Test 12 failed: Prompt not found or not associated with session"
  docker compose down -v
  exit 1
fi

# Test 13: Verify prompt appears in session's prompt list
echo ""
echo "Test 13: Listing prompts for session..."
LIST_PROMPTS=$(curl -s http://localhost:8000/sessions/$SESSION_WITH_PROMPT_ID/prompts \
  -H "Authorization: Bearer $ACCESS_TOKEN")
echo "List prompts response: $LIST_PROMPTS"

if echo "$LIST_PROMPTS" | grep -q "$PROMPT_ID"; then
  echo "✓ Test 13 passed: Prompt found in session's prompt list"
else
  echo "✗ Test 13 failed: Prompt not in session's prompt list"
  docker compose down -v
  exit 1
fi

# Test 14: Verify prompt data is correct
echo ""
echo "Test 14: Verifying prompt data..."
if echo "$READ_PROMPT" | grep -q '"content":"Fix authentication bug"'; then
  echo "✓ Test 14 passed: Prompt data is correct"
else
  echo "✗ Test 14 failed: Prompt data is incorrect"
  docker compose down -v
  exit 1
fi

# Test 15: Create session with prompt including parent_id
echo ""
echo "Test 15: Creating session with prompt and parent_id..."
CREATE_WITH_PARENT=$(curl -s -X POST http://localhost:8000/sessions/with-prompt \
  -H "Content-Type: application/json" \
  -H "Authorization: Bearer $ACCESS_TOKEN" \
  -d "{\"repo\": \"test-repo\", \"target_branch\": \"main\", \"messages\": {\"content\": \"Refactor based on previous work\"}, \"parent_id\": \"$SESSION_WITH_PROMPT_ID\"}")
echo "Create with parent response: $CREATE_WITH_PARENT"

if echo "$CREATE_WITH_PARENT" | grep -q '"success":true'; then
  SESSION_WITH_PARENT_ID=$(echo "$CREATE_WITH_PARENT" | grep -o '"sessionId":"[^"]*"' | cut -d'"' -f4)
  echo "✓ Test 15 passed: Session with parent created successfully (ID: $SESSION_WITH_PARENT_ID)"
else
  echo "✗ Test 15 failed: Failed to create session with parent"
  docker compose down -v
  exit 1
fi

# Test 16: Verify parent relationship
echo ""
echo "Test 16: Verifying parent relationship..."
READ_CHILD_SESSION=$(curl -s http://localhost:8000/sessions/$SESSION_WITH_PARENT_ID \
  -H "Authorization: Bearer $ACCESS_TOKEN")
echo "Read child session response: $READ_CHILD_SESSION"

if echo "$READ_CHILD_SESSION" | grep -q "\"parent\":\"$SESSION_WITH_PROMPT_ID\""; then
  echo "✓ Test 16 passed: Parent relationship verified"
else
  echo "✗ Test 16 failed: Parent relationship not found"
  docker compose down -v
  exit 1
fi

# Test 17: Test with complex message structure
echo ""
echo "Test 17: Creating session with complex message structure..."
CREATE_COMPLEX=$(curl -s -X POST http://localhost:8000/sessions/with-prompt \
  -H "Content-Type: application/json" \
  -H "Authorization: Bearer $ACCESS_TOKEN" \
  -d '{"repo": "complex-repo", "target_branch": "develop", "messages": {"content": "Add feature X", "role": "user", "metadata": {"priority": "high"}}}')
echo "Create with complex data response: $CREATE_COMPLEX"

if echo "$CREATE_COMPLEX" | grep -q '"success":true'; then
  COMPLEX_SESSION_ID=$(echo "$CREATE_COMPLEX" | grep -o '"sessionId":"[^"]*"' | cut -d'"' -f4)
  COMPLEX_PROMPT_ID=$(echo "$CREATE_COMPLEX" | grep -o '"promptId":"[^"]*"' | cut -d'"' -f4)
  echo "✓ Test 17 passed: Complex message structure handled (Session: $COMPLEX_SESSION_ID, Prompt: $COMPLEX_PROMPT_ID)"
else
  echo "✗ Test 17 failed: Failed to handle complex message structure"
  docker compose down -v
  exit 1
fi

# Test 18: Verify complex prompt data
echo ""
echo "Test 18: Verifying complex prompt data..."
READ_COMPLEX_PROMPT=$(curl -s http://localhost:8000/prompts/$COMPLEX_PROMPT_ID \
  -H "Authorization: Bearer $ACCESS_TOKEN")
echo "Read complex prompt response: $READ_COMPLEX_PROMPT"

if echo "$READ_COMPLEX_PROMPT" | grep -q '"priority":"high"' && echo "$READ_COMPLEX_PROMPT" | grep -q '"role":"user"'; then
  echo "✓ Test 18 passed: Complex prompt data preserved correctly"
else
  echo "✗ Test 18 failed: Complex prompt data not preserved"
  docker compose down -v
  exit 1
fi

echo ""
echo "========================================="
echo "All CRUD tests passed successfully! ✓"
echo "========================================="

docker compose down -v
