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

echo ""
echo "========================================="
echo "All CRUD tests passed successfully! ✓"
echo "========================================="

docker compose down -v
