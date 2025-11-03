# Cargo Workspace SDK Integration Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Integrate agent-sandbox-sdk as a library dependency using Cargo workspaces so server code in src/ can import and use the SDK.

**Architecture:** Convert root Cargo.toml to workspace + package configuration, add SDK as path dependency. Workspace build automatically includes both crates and makes SDK available to server.

**Tech Stack:** Cargo workspaces, Rust, Nix (buildRustPackage)

---

## Task 1: Update Root Cargo.toml for Workspace

**Files:**
- Modify: `Cargo.toml` (root)

**Step 1: Add workspace declaration to root Cargo.toml**

Add workspace section at the very top of the file, before `[package]`:

```toml
[workspace]
members = ["agent-sandbox-sdk"]
resolver = "2"

[package]
name = "rust-redis-webserver"
version = "0.1.0"
edition = "2021"

[dependencies]
rocket = "0.5.0-rc.1"
redis = { version = "0.25.4", features = ["aio", "tokio-comp", "connection-manager"] }
tokio = { version = "1", features = ["full", "macros"] }
serde = { version = "1.0", features = ["derive"] }
serde_json = "1.0"
dotenv = "0.15.0"
rocket_okapi = { version = "0.8.0", features = [ "swagger", "rapidoc" ] }
rocket_cors = "0.6.0"
schemars = { version = "0.8" }
sea-orm = { version = "0.12", features = ["sqlx-postgres", "runtime-tokio-rustls", "macros"] }
sea-orm-migration = { version = "0.12", features = ["runtime-tokio-rustls", "sqlx-postgres"] }
uuid = { version = "1.0", features = ["serde", "v4"] }
migration = { path = "migration" }
clap = { version = "4.5", features = ["derive"] }
apalis = { version = "0.5", features = ["tokio-comp"] }
apalis-redis = "0.5"
apalis-sql = { version = "0.5", features = ["postgres"] }
anyhow = "1.0"
tracing = "0.1"
tracing-subscriber = { version = "0.3", features = ["env-filter"] }
reqwest = { version = "0.11", features = ["json"] }
sandbox-client = { path = "agent-sandbox-sdk" }

# Pin base64ct to avoid edition 2024 requirement (not yet stable in Rust 1.84)
[dependencies.base64ct]
version = "=1.6.0"

[dependencies.home]
version = "=0.5.9"

# Pin regex to avoid compatibility issues with sea-orm-cli in Rust 1.84
[dependencies.regex]
version = "=1.10.6"

[dev-dependencies]
insta = { version = "1.34", features = ["json"] }
```

**Key changes:**
- Add `[workspace]` section at top with `members = ["agent-sandbox-sdk"]`
- Add `resolver = "2"` for modern dependency resolution
- Add `sandbox-client = { path = "agent-sandbox-sdk" }` to `[dependencies]`

**Step 2: Verify file syntax**

Check that the file is valid TOML and has no syntax errors:

```bash
cargo metadata --format-version=1 > /dev/null
```

Expected: No errors

---

## Task 2: Regenerate Cargo.lock

**Files:**
- Modify: `Cargo.lock` (root, will be regenerated)

**Step 1: Clean existing build artifacts**

```bash
cargo clean
```

Expected: Removes `target/` directory

**Step 2: Generate new Cargo.lock with workspace structure**

```bash
cargo generate-lockfile
```

Expected: Creates unified Cargo.lock for entire workspace

**Step 3: Verify workspace structure**

```bash
cargo metadata --format-version=1 | jq '.workspace_members | length'
```

Expected: Output `2` (two workspace members: server and SDK)

**Step 4: Commit workspace configuration**

```bash
git add Cargo.toml Cargo.lock
git commit -m "feat: convert to Cargo workspace and add SDK dependency

Add workspace declaration with agent-sandbox-sdk as member.
Add sandbox-client as path dependency to main server.
Unified Cargo.lock for entire workspace."
```

---

## Task 3: Verify Workspace Build (Cargo)

**Step 1: Build entire workspace**

```bash
cargo build
```

Expected: Builds both `sandbox-client` (library) and `rust-redis-webserver` (binary)
- Should see "Compiling sandbox-client" before "Compiling rust-redis-webserver"
- Should complete without errors

**Step 2: Build SDK independently**

```bash
cargo build -p sandbox-client
```

Expected: Builds only the SDK crate, produces `libsandbox_client.rlib`

**Step 3: Build server independently**

```bash
cargo build -p rust-redis-webserver
```

Expected: Builds server and its dependency (SDK)

**Step 4: Verify SDK is linked**

Check that server binary was built with SDK:

```bash
cargo tree -p rust-redis-webserver | grep sandbox-client
```

Expected: Output shows `sandbox-client` in dependency tree

---

## Task 4: Verify Nix Build

**Step 1: Clean Nix build cache (optional)**

```bash
nix-store --delete /nix/store/*rust-redis-webserver* 2>/dev/null || true
```

Expected: Clears old builds (may show warnings if none exist, that's fine)

**Step 2: Build with Nix**

```bash
nix build
```

Expected:
- Builds workspace successfully
- Creates `result` symlink pointing to server binary
- No errors about missing Cargo.lock or workspace members

**Step 3: Verify Nix build output**

```bash
ls -lh result/bin/
```

Expected: Shows `rust-redis-webserver` binary (SDK is linked into it, not separate)

**Step 4: Commit verification note**

```bash
git add -A
git commit -m "test: verify Nix build works with workspace" --allow-empty
```

Note: `--allow-empty` used since this is verification step

---

## Task 5: Add Smoke Test Using SDK

**Files:**
- Create: `tests/sdk_smoke_test.rs`

**Step 1: Write failing integration test**

Create new integration test file:

```rust
// tests/sdk_smoke_test.rs

/// Smoke test to verify SDK can be imported and Client can be instantiated
#[test]
fn test_sdk_client_instantiation() {
    use sandbox_client::Client;

    // Just verify we can create a client - don't actually make requests
    let _client = Client::new("http://localhost:8000");

    // If we got here, SDK is properly linked
    assert!(true, "SDK client successfully instantiated");
}
```

**Step 2: Run test to verify it compiles**

```bash
cargo test --test sdk_smoke_test
```

Expected: Test passes (client instantiation succeeds)

**Step 3: Add test with type checking**

Update the test to verify more SDK types are accessible:

```rust
// tests/sdk_smoke_test.rs

/// Smoke test to verify SDK can be imported and Client can be instantiated
#[test]
fn test_sdk_client_instantiation() {
    use sandbox_client::Client;

    // Just verify we can create a client - don't actually make requests
    let _client = Client::new("http://localhost:8000");

    // If we got here, SDK is properly linked
    assert!(true, "SDK client successfully instantiated");
}

/// Verify SDK types are accessible
#[test]
fn test_sdk_types_accessible() {
    // This test just needs to compile to verify types are available
    // We're not testing the API behavior, just that SDK exports work

    let _ = std::any::type_name::<sandbox_client::Client>();

    assert!(true, "SDK types are accessible");
}
```

**Step 4: Run tests again**

```bash
cargo test --test sdk_smoke_test
```

Expected: Both tests pass

**Step 5: Run all tests**

```bash
cargo test
```

Expected: All tests pass (including existing main.rs test and new SDK tests)

**Step 6: Commit smoke tests**

```bash
git add tests/sdk_smoke_test.rs
git commit -m "test: add SDK integration smoke tests

Verify SDK can be imported and Client can be instantiated.
Confirms workspace integration is working correctly."
```

---

## Task 6: Final Verification

**Step 1: Clean build from scratch**

```bash
cargo clean
cargo build
```

Expected: Clean build succeeds

**Step 2: Run all tests with Nix environment**

```bash
nix develop -c cargo test
```

Expected: All tests pass (1 from main.rs, 2 from SDK smoke test)

**Step 3: Verify flake still works**

```bash
nix flake check
```

Expected: No errors

**Step 4: Document usage in README (optional)**

If there's a README or development docs, add a note about the workspace structure:

```markdown
## Project Structure

This project uses a Cargo workspace:
- `agent-sandbox-sdk/` - Generated Rust SDK client (from OpenAPI spec)
- Root - Main server application

The SDK is automatically available to server code via workspace dependency.
```

---

## Testing Strategy

**What we're testing:**
1. Workspace configuration is valid (cargo metadata)
2. Both crates build independently and together
3. SDK is properly linked into server binary
4. Nix build still works with workspace structure
5. SDK types are accessible in server code (smoke test)

**What we're NOT testing:**
- Actual SDK functionality (that's generated code)
- Server API behavior (covered by existing tests)
- End-to-end API calls (not needed for workspace integration)

---

## Success Criteria

- [ ] Root Cargo.toml has workspace declaration
- [ ] `sandbox-client` is listed in dependencies
- [ ] `cargo build` builds both workspace members
- [ ] `cargo test` passes all tests
- [ ] `nix build` produces working server binary
- [ ] SDK types can be imported in server code
- [ ] Cargo.lock is unified for entire workspace

---

## Rollback Plan

If integration fails:

```bash
# Remove workspace changes
git reset --hard HEAD~N  # where N is number of commits made

# Clean build artifacts
cargo clean
rm -rf target/

# Rebuild original state
cargo build
```

---

## Future Work

After this integration:
1. Use SDK in actual server handlers
2. Add integration tests that use SDK to test server endpoints
3. Consider generating openapi.json from server during build (ensures sync)
4. Add pre-commit hook to regenerate SDK when OpenAPI spec changes

---

## References

- Design document: `docs/plans/2025-11-03-cargo-workspace-sdk-integration-design.md`
- Cargo workspaces: https://doc.rust-lang.org/cargo/reference/workspaces.html
- Nix Rust build: https://nixos.org/manual/nixpkgs/stable/#rust
