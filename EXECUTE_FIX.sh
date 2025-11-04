#!/bin/bash
# Script to apply fix and verify it works for PR #25

set -e  # Exit on error
set -x  # Print commands

cd /Users/robertwendt/prompt-backend

echo "=== Step 1: Verify current branch ==="
git branch --show-current
git status

echo "=== Step 2: Clean build artifacts ==="
cargo clean

echo "=== Step 3: Build agent-sandbox-sdk ==="
cargo build --package sandbox-client

echo "=== Step 4: Run all tests (including SDK smoke tests) ==="
cargo test --all-targets

echo "=== Step 5: Check for any warnings ==="
cargo clippy --all-targets -- -D warnings

echo "=== Step 6: Commit the fix ==="
git add agent-sandbox-sdk/src/lib.rs
git commit -m "Fix: Allow irrefutable_let_patterns in generated SDK code

Add compiler directives to allow irrefutable_let_patterns and
unreachable_patterns in the auto-generated sandbox-client code.

The progenitor code generator creates FromStr implementations for
anyOf schemas that result in irrefutable patterns. For example,
LocationItem can be either String or i64, and parsing &str to String
always succeeds, making the if let pattern irrefutable.

Since this is generated code, we suppress the warnings rather than
attempting to modify the code generator.

Fixes failing unit tests in PR #25.

🤖 Generated with [Claude Code](https://claude.com/claude-code)

Co-Authored-By: Claude <noreply@anthropic.com>"

echo "=== Step 7: Push to remote ==="
git push origin cargo-workspace-sdk

echo "=== Fix applied successfully! ==="
echo "Check GitHub Actions at: https://github.com/r33drichards/prompt-backend/pull/25"
