#!/usr/bin/env bash
set -e

echo "Building and starting services..."
docker compose build
docker compose up -d postgres
docker compose up -d redis
docker compose up -d webserver

echo "Waiting for webserver to be ready..."
for i in {1..30}; do
  if curl -s http://localhost:8000/swagger-ui/ > /dev/null 2>&1; then
    echo "Webserver is ready!"
    break
  fi
  echo "Waiting... ($i/30)"
  sleep 1
done

# Test 1: Create an item
echo "Test 1: Creating an item..."
CREATE_RESPONSE=$(curl -s -X POST http://localhost:8000/items \
  -H "Content-Type: application/json" \
  -d '{"item": {"name": "test-item", "value": 42}}')
echo "Create response: $CREATE_RESPONSE"

if echo "$CREATE_RESPONSE" | grep -q '"success":true'; then
  echo "✓ Test 1 passed: Item created successfully"
else
  echo "✗ Test 1 failed: Failed to create item"
  docker compose down -v
  exit 1
fi

# Test 2: List items
echo ""
echo "Test 2: Listing items..."
LIST_RESPONSE=$(curl -s http://localhost:8000/items)
echo "List response: $LIST_RESPONSE"

if echo "$LIST_RESPONSE" | grep -q 'test-item'; then
  echo "✓ Test 2 passed: Item found in list"
else
  echo "✗ Test 2 failed: Item not found in list"
  docker compose down -v
  exit 1
fi

# Test 3: Update an item
echo ""
echo "Test 3: Updating an item..."
UPDATE_RESPONSE=$(curl -s -X PUT http://localhost:8000/items \
  -H "Content-Type: application/json" \
  -d '{"old_item": {"name": "test-item", "value": 42}, "new_item": {"name": "test-item", "value": 100}}')
echo "Update response: $UPDATE_RESPONSE"

if echo "$UPDATE_RESPONSE" | grep -q '"success":true'; then
  echo "✓ Test 3 passed: Item updated successfully"
else
  echo "✗ Test 3 failed: Failed to update item"
  docker compose down -v
  exit 1
fi

# Test 4: Verify update by listing
echo ""
echo "Test 4: Verifying update..."
LIST_RESPONSE2=$(curl -s http://localhost:8000/items)
echo "List response after update: $LIST_RESPONSE2"

if echo "$LIST_RESPONSE2" | grep -q '"value":100'; then
  echo "✓ Test 4 passed: Update verified"
else
  echo "✗ Test 4 failed: Update not reflected"
  docker compose down -v
  exit 1
fi

# Test 5: Read (pop) an item
echo ""
echo "Test 5: Reading (popping) an item..."
READ_RESPONSE=$(curl -s http://localhost:8000/items/read)
echo "Read response: $READ_RESPONSE"

if echo "$READ_RESPONSE" | grep -q 'test-item'; then
  echo "✓ Test 5 passed: Item read successfully"
else
  echo "✗ Test 5 failed: Failed to read item"
  docker compose down -v
  exit 1
fi

# Test 6: Verify item was removed by listing
echo ""
echo "Test 6: Verifying item was removed..."
LIST_RESPONSE3=$(curl -s http://localhost:8000/items)
echo "List response after read: $LIST_RESPONSE3"

if echo "$LIST_RESPONSE3" | grep -q '"items":\[\]'; then
  echo "✓ Test 6 passed: Item removed after read"
else
  echo "✗ Test 6 failed: Item still present after read"
  docker compose down -v
  exit 1
fi

# Test 7: Create and delete an item
echo ""
echo "Test 7: Creating item for deletion test..."
CREATE_RESPONSE2=$(curl -s -X POST http://localhost:8000/items \
  -H "Content-Type: application/json" \
  -d '{"item": {"name": "delete-me", "id": 123}}')
echo "Create response: $CREATE_RESPONSE2"

echo "Test 8: Deleting the item..."
DELETE_RESPONSE=$(curl -s -X DELETE http://localhost:8000/items \
  -H "Content-Type: application/json" \
  -d '{"item": {"name": "delete-me", "id": 123}}')
echo "Delete response: $DELETE_RESPONSE"

if echo "$DELETE_RESPONSE" | grep -q '"success":true'; then
  echo "✓ Test 8 passed: Item deleted successfully"
else
  echo "✗ Test 8 failed: Failed to delete item"
  docker compose down -v
  exit 1
fi

# Test 9: Verify deletion
echo ""
echo "Test 9: Verifying deletion..."
LIST_RESPONSE4=$(curl -s http://localhost:8000/items)
echo "List response after delete: $LIST_RESPONSE4"

if echo "$LIST_RESPONSE4" | grep -q '"items":\[\]'; then
  echo "✓ Test 9 passed: Deletion verified"
else
  echo "✗ Test 9 failed: Item still present after deletion"
  docker compose down -v
  exit 1
fi

echo ""
echo "========================================="
echo "Testing Session CRUD Operations"
echo "========================================="

# Test 10: Create a session
echo ""
echo "Test 10: Creating a session..."
SESSION_CREATE_RESPONSE=$(curl -s -X POST http://localhost:8000/sessions \
  -H "Content-Type: application/json" \
  -d '{"messages": {"chat": ["hello", "world"]}, "inbox_status": "pending", "sbx_config": {"timeout": 30}, "parent": null}')
echo "Session create response: $SESSION_CREATE_RESPONSE"

if echo "$SESSION_CREATE_RESPONSE" | grep -q '"success":true'; then
  echo "✓ Test 10 passed: Session created successfully"
  SESSION_ID=$(echo "$SESSION_CREATE_RESPONSE" | grep -o '"id":"[^"]*"' | cut -d'"' -f4)
  echo "Session ID: $SESSION_ID"
else
  echo "✗ Test 10 failed: Failed to create session"
  docker compose down -v
  exit 1
fi

# Test 11: List sessions
echo ""
echo "Test 11: Listing sessions..."
SESSION_LIST_RESPONSE=$(curl -s http://localhost:8000/sessions)
echo "Session list response: $SESSION_LIST_RESPONSE"

if echo "$SESSION_LIST_RESPONSE" | grep -q "$SESSION_ID"; then
  echo "✓ Test 11 passed: Session found in list"
else
  echo "✗ Test 11 failed: Session not found in list"
  docker compose down -v
  exit 1
fi

# Test 12: Read a specific session
echo ""
echo "Test 12: Reading session by ID..."
SESSION_READ_RESPONSE=$(curl -s http://localhost:8000/sessions/$SESSION_ID)
echo "Session read response: $SESSION_READ_RESPONSE"

if echo "$SESSION_READ_RESPONSE" | grep -q "$SESSION_ID" && echo "$SESSION_READ_RESPONSE" | grep -q '"inbox_status":"pending"'; then
  echo "✓ Test 12 passed: Session retrieved successfully"
else
  echo "✗ Test 12 failed: Failed to retrieve session"
  docker compose down -v
  exit 1
fi

# Test 13: Update a session
echo ""
echo "Test 13: Updating session..."
SESSION_UPDATE_RESPONSE=$(curl -s -X PUT http://localhost:8000/sessions/$SESSION_ID \
  -H "Content-Type: application/json" \
  -d "{\"id\": \"$SESSION_ID\", \"messages\": {\"chat\": [\"updated\", \"messages\"]}, \"inbox_status\": \"active\", \"sbx_config\": {\"timeout\": 60}, \"parent\": null}")
echo "Session update response: $SESSION_UPDATE_RESPONSE"

if echo "$SESSION_UPDATE_RESPONSE" | grep -q '"success":true'; then
  echo "✓ Test 13 passed: Session updated successfully"
else
  echo "✗ Test 13 failed: Failed to update session"
  docker compose down -v
  exit 1
fi

# Test 14: Verify session update
echo ""
echo "Test 14: Verifying session update..."
SESSION_VERIFY_RESPONSE=$(curl -s http://localhost:8000/sessions/$SESSION_ID)
echo "Session verify response: $SESSION_VERIFY_RESPONSE"

if echo "$SESSION_VERIFY_RESPONSE" | grep -q '"inbox_status":"active"'; then
  echo "✓ Test 14 passed: Session update verified"
else
  echo "✗ Test 14 failed: Session update not reflected"
  docker compose down -v
  exit 1
fi

# Test 15: Create a child session (testing parent relationship)
echo ""
echo "Test 15: Creating a child session with parent..."
CHILD_SESSION_CREATE_RESPONSE=$(curl -s -X POST http://localhost:8000/sessions \
  -H "Content-Type: application/json" \
  -d "{\"messages\": {\"chat\": [\"child\"]}, \"inbox_status\": \"pending\", \"sbx_config\": null, \"parent\": \"$SESSION_ID\"}")
echo "Child session create response: $CHILD_SESSION_CREATE_RESPONSE"

if echo "$CHILD_SESSION_CREATE_RESPONSE" | grep -q '"success":true'; then
  echo "✓ Test 15 passed: Child session created successfully"
  CHILD_SESSION_ID=$(echo "$CHILD_SESSION_CREATE_RESPONSE" | grep -o '"id":"[^"]*"' | cut -d'"' -f4)
  echo "Child Session ID: $CHILD_SESSION_ID"
else
  echo "✗ Test 15 failed: Failed to create child session"
  docker compose down -v
  exit 1
fi

# Test 16: Verify child session has correct parent
echo ""
echo "Test 16: Verifying child session parent relationship..."
CHILD_VERIFY_RESPONSE=$(curl -s http://localhost:8000/sessions/$CHILD_SESSION_ID)
echo "Child session verify response: $CHILD_VERIFY_RESPONSE"

if echo "$CHILD_VERIFY_RESPONSE" | grep -q "\"parent\":\"$SESSION_ID\""; then
  echo "✓ Test 16 passed: Child session parent relationship verified"
else
  echo "✗ Test 16 failed: Parent relationship not correct"
  docker compose down -v
  exit 1
fi

# Test 17: Delete child session
echo ""
echo "Test 17: Deleting child session..."
CHILD_DELETE_RESPONSE=$(curl -s -X DELETE http://localhost:8000/sessions/$CHILD_SESSION_ID)
echo "Child session delete response: $CHILD_DELETE_RESPONSE"

if echo "$CHILD_DELETE_RESPONSE" | grep -q '"success":true'; then
  echo "✓ Test 17 passed: Child session deleted successfully"
else
  echo "✗ Test 17 failed: Failed to delete child session"
  docker compose down -v
  exit 1
fi

# Test 18: Delete parent session
echo ""
echo "Test 18: Deleting parent session..."
SESSION_DELETE_RESPONSE=$(curl -s -X DELETE http://localhost:8000/sessions/$SESSION_ID)
echo "Session delete response: $SESSION_DELETE_RESPONSE"

if echo "$SESSION_DELETE_RESPONSE" | grep -q '"success":true'; then
  echo "✓ Test 18 passed: Session deleted successfully"
else
  echo "✗ Test 18 failed: Failed to delete session"
  docker compose down -v
  exit 1
fi

# Test 19: Verify sessions were deleted
echo ""
echo "Test 19: Verifying sessions were deleted..."
FINAL_SESSION_LIST=$(curl -s http://localhost:8000/sessions)
echo "Final session list: $FINAL_SESSION_LIST"

if echo "$FINAL_SESSION_LIST" | grep -q '"sessions":\[\]'; then
  echo "✓ Test 19 passed: All sessions deleted successfully"
else
  echo "✗ Test 19 failed: Sessions still present after deletion"
  docker compose down -v
  exit 1
fi

echo ""
echo "========================================="
echo "All CRUD tests passed successfully! ✓"
echo "Items: 9 tests passed"
echo "Sessions: 10 tests passed"
echo "Total: 19 tests passed"
echo "========================================="

docker compose down -v
