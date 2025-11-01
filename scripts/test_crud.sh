#!/usr/bin/env bash
set -e

echo "Building and starting services..."
docker compose build
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
echo "All CRUD tests passed successfully! ✓"
echo "========================================="

docker compose down -v
