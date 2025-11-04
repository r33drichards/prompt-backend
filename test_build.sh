#!/bin/bash
set -e

echo "Building agent-sandbox-sdk..."
cd /Users/robertwendt/prompt-backend
cargo build --package sandbox-client

echo "Running tests..."
cargo test --all-targets

echo "All tests passed!"
