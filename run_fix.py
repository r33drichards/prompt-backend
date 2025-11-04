#!/usr/bin/env python3
"""
Script to apply the fix and verify it works
"""
import subprocess
import os
import sys

os.chdir('/Users/robertwendt/prompt-backend')

def run(cmd, description):
    print(f"\n{'='*60}")
    print(f"{description}")
    print(f"{'='*60}")
    print(f"Running: {cmd}")
    result = subprocess.run(cmd, shell=True, capture_output=True, text=True)
    print(result.stdout)
    if result.stderr:
        print("STDERR:", result.stderr, file=sys.stderr)
    if result.returncode != 0:
        print(f"ERROR: Command failed with exit code {result.returncode}")
        sys.exit(1)
    return result

# Step 1: Check current state
run("git branch --show-current", "Check current branch")
run("git status --short", "Check git status")

# Step 2: Run tests
run("cargo test --all-targets", "Run all tests")

# Step 3: Commit
run("git add agent-sandbox-sdk/src/lib.rs", "Stage changes")
commit_msg = """Fix: Allow irrefutable_let_patterns in generated SDK code

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

Co-Authored-By: Claude <noreply@anthropic.com>"""

run(f'git commit -m "{commit_msg}"', "Commit changes")

# Step 4: Push
run("git push origin cargo-workspace-sdk", "Push to remote")

print("\n" + "="*60)
print("SUCCESS! Fix applied and pushed")
print("Check: https://github.com/r33drichards/prompt-backend/pull/25")
print("="*60)
