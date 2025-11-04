# Cargo Workspace and SDK Integration Design

**Date:** 2025-11-03
**Status:** Approved

## Overview

Integrate `agent-sandbox-sdk` as a library dependency within the main server codebase using Cargo workspaces, enabling `src/` code to import and use the SDK without exposing it as a separate binary.

## Requirements

1. Build agent-sandbox-sdk as a library (not a binary)
2. Make SDK available as a dependency to main server code in `src/`
3. Support building with Nix (`nix build`)
4. Maintain existing flake.nix structure and outputs
5. Keep existing directory structure (no major reorganization)

## Architecture

### Cargo Workspace Structure

**Option B: Root is server + workspace**
```
prompt-backend/
├── Cargo.toml              (workspace + server package)
├── agent-sandbox-sdk/      (workspace member - library)
│   ├── Cargo.toml
│   ├── build.rs            (generates client from openapi.json)
│   └── src/lib.rs
├── src/                    (server source - unchanged)
├── migration/              (unchanged)
└── flake.nix              (minimal changes)
```

This approach keeps the existing repository structure intact while adding workspace capabilities.

## Implementation Details

### 1. Root Cargo.toml Changes

Add workspace declaration at the top of the existing file:

```toml
[workspace]
members = ["agent-sandbox-sdk"]
resolver = "2"

[package]
name = "rust-redis-webserver"
version = "0.1.0"
edition = "2021"

[dependencies]
# ... existing dependencies
sandbox-client = { path = "agent-sandbox-sdk" }
```

**Key points:**
- Workspace declaration comes first
- Package configuration remains unchanged
- Add `sandbox-client` as path dependency
- `resolver = "2"` ensures modern dependency resolution

### 2. agent-sandbox-sdk/Cargo.toml

No changes needed. Already configured as:
```toml
[package]
name = "sandbox-client"
version = "0.1.0"
edition = "2021"

[lib]  # Implicit, but this is a library crate
```

### 3. Nix Flake Changes

**Current state:**
- Single `rustPlatform.buildRustPackage` that builds the server
- Uses root `Cargo.lock`

**Required changes:**
- Minimal - workspace builds automatically include all members
- `buildRustPackage` will build both the SDK library and server binary
- Only the server binary gets installed to output (standard behavior)
- `cargoLock.lockFile` already points to root `./Cargo.lock`

**No changes needed to flake.nix** - Cargo workspaces are transparent to `buildRustPackage`.

### 4. Using SDK in Server Code

After integration, server code can import the SDK:

```rust
// In src/main.rs or src/handlers/*.rs
use sandbox_client::{Client, types::*};

async fn example() {
    let client = Client::new("http://localhost:8000");
    // Use the generated client...
}
```

## Build Process

### Workspace Build Flow

1. Cargo reads root `Cargo.toml`, discovers workspace members
2. Builds `agent-sandbox-sdk`:
   - Runs `build.rs` to generate client from `openapi.json`
   - Compiles to `libsandbox_client.rlib`
3. Builds `rust-redis-webserver`:
   - Links against compiled SDK library
   - Produces server binary

### Nix Build Flow

```bash
# Build entire workspace (server binary)
nix build

# Development shell includes workspace dependencies
nix develop
cargo build  # Builds both workspace members
```

## Migration Steps

1. Update root `Cargo.toml` with workspace declaration and SDK dependency
2. Regenerate `Cargo.lock` with workspace structure
3. Update `.gitignore` if needed (Cargo.lock should be committed)
4. Test builds:
   - `cargo build` - Workspace build
   - `nix build` - Nix build
5. Update main server code to use SDK (future work)

## Testing Strategy

1. **Cargo workspace validation:**
   - `cargo build` succeeds
   - `cargo build -p sandbox-client` builds SDK only
   - `cargo build -p rust-redis-webserver` builds server

2. **Nix build validation:**
   - `nix build` produces working server binary
   - `nix develop` provides correct environment

3. **Smoke test:**
   - Add simple SDK import in server code
   - Verify compilation succeeds

## Trade-offs and Considerations

**Advantages:**
- Minimal disruption to existing structure
- Standard Rust monorepo pattern
- Shared dependency resolution
- Single `Cargo.lock` for entire project
- SDK stays in sync with server

**Limitations:**
- SDK cannot be built as standalone Nix package (not needed per requirements)
- Workspace rebuilds all members on dependency changes (acceptable for small workspace)

## Future Enhancements

1. Generate `openapi.json` from server during build (ensures SDK stays in sync)
2. Add integration tests using the SDK to test the server
3. Split into multiple workspace crates if needed (handlers, services, etc.)

## References

- Cargo workspaces: https://doc.rust-lang.org/cargo/reference/workspaces.html
- Nix buildRustPackage: https://nixos.org/manual/nixpkgs/stable/#rust
- Progenitor (SDK generator): https://github.com/oxidecomputer/progenitor
