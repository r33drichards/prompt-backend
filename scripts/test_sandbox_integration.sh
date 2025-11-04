#!/usr/bin/env bash
set -e

echo "=== Sandbox Integration Test ==="
echo

# Check if Docker is running
if ! docker info > /dev/null 2>&1; then
    echo "Error: Docker is not running"
    exit 1
fi

echo "1. Starting sandbox container..."
docker rm -f sandbox 2>/dev/null || true
docker run -d --name sandbox -p 8080:8080 wholelottahoopla/sandbox:latest

# Wait for container to be ready
echo "2. Waiting for sandbox to be ready..."
max_attempts=30
attempt=0
while [ $attempt -lt $max_attempts ]; do
    if curl -s http://localhost:8080/v1/sandbox > /dev/null 2>&1; then
        echo "   Sandbox is ready!"
        break
    fi
    attempt=$((attempt + 1))
    sleep 1
done

if [ $attempt -eq $max_attempts ]; then
    echo "Error: Sandbox failed to start within 30 seconds"
    docker logs sandbox
    docker stop sandbox
    docker rm sandbox
    exit 1
fi

echo "3. Running integration test with Nix..."
# Use nix develop if available, otherwise try plain cargo
if command -v nix > /dev/null 2>&1; then
    echo "   Using Nix to run cargo test..."
    nix develop --command cargo test --test sandbox_integration_test -- --ignored --nocapture
else
    echo "   Nix not found, using system cargo..."
    cargo test --test sandbox_integration_test -- --ignored --nocapture
fi

test_result=$?

echo "4. Cleaning up..."
docker stop sandbox
docker rm sandbox

if [ $test_result -eq 0 ]; then
    echo
    echo "✓ Integration test passed!"
    exit 0
else
    echo
    echo "✗ Integration test failed"
    exit 1
fi
