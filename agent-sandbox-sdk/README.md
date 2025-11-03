# Sandbox API Rust SDK

This is an auto-generated Rust SDK for the Sandbox API, created using [progenitor](https://github.com/oxidecomputer/progenitor).

## Installation

Add this to your `Cargo.toml`:

```toml
[dependencies]
sandbox-client = { path = "agent-sandbox-sdk" }
```

## Usage

```rust
use sandbox_client::Client;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let client = Client::new("http://localhost:8080")?;

    // Use the client to make API calls
    // Example methods will depend on your API specification

    Ok(())
}
```

## How It Works

This SDK uses [progenitor](https://github.com/oxidecomputer/progenitor) to generate a Rust client from the OpenAPI specification at build time.

The `openapi.json` file is automatically updated by the GitHub Action workflow when triggered. When you build this crate (`cargo build`), the `build.rs` script reads the OpenAPI spec and generates the Rust client code.

## Updating the SDK

The OpenAPI specification is automatically updated by the GitHub Action workflow "Generate Rust SDK from Sandbox". To trigger an update:

1. Go to the Actions tab in GitHub
2. Select "Generate Rust SDK from Sandbox"
3. Click "Run workflow"
4. Enter the sandbox image tag (e.g., `latest`, `v1.0.0`)
5. The workflow will fetch the OpenAPI spec and create a PR with the updated `openapi.json`

## Documentation

Run `cargo doc --open` in the `agent-sandbox-sdk` directory to view the generated documentation.
