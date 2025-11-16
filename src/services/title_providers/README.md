# Title Generation Providers

This module provides a pluggable architecture for generating session titles and branch names using different AI providers.

## Overview

The title provider system allows you to configure which AI service is used to generate:
- Session titles: Descriptive names for coding sessions based on user prompts
- Branch names: Git-safe branch names derived from session context

## Available Providers

### 1. Anthropic Haiku 4.5 (Default)
- **Model**: `claude-haiku-4-5`
- **Configuration Values**: `anthropic`, `anthropic-haiku`, `anthropic-haiku-4-5`

### 2. Google Gemini 2.5 Flash
- **Model**: `gemini-2.5-flash`
- **Configuration Values**: `gemini`, `gemini-flash`, `google-gemini-2-5`, `google-gemini-2.5-flash`

## Configuration

Set the `TITLE_PROVIDER` environment variable to choose which provider to use:

```bash
# Use Anthropic Haiku (default)
export TITLE_PROVIDER=anthropic-haiku

# Use Google Gemini
export TITLE_PROVIDER=gemini-flash
```

If `TITLE_PROVIDER` is not set, the system defaults to `anthropic-haiku`.

## Required Environment Variables

All providers use a single `TITLE_API_KEY` environment variable for authentication:

```bash
# Set this to your Anthropic API key if using anthropic-haiku provider
# OR set this to your Google Gemini API key if using gemini-flash provider
export TITLE_API_KEY=your_api_key_here
```

This unified approach prevents accidental shadowing of provider-specific API key variables.

## Usage

The providers are used automatically through the `anthropic.rs` service functions:

```rust
use crate::services::anthropic;

// Generate a session title
let title = anthropic::generate_session_title(
    "owner/repo",
    "main",
    "Fix memory leak in session handler"
).await?;

// Generate a branch name
let branch = anthropic::generate_branch_name(
    "owner/repo",
    "main",
    "Fix memory leak in session handler",
    "session-id-123"
).await?;
```

The provider is selected automatically based on the `TITLE_PROVIDER` environment variable.

## Adding a New Provider

To add a new AI provider:

1. Create a new file in `src/services/title_providers/` (e.g., `my_provider.rs`)
2. Implement the `TitleProvider` trait:

```rust
use async_trait::async_trait;
use super::TitleProvider;

pub struct MyProvider {
    api_key: String,
    client: reqwest::Client,
}

impl MyProvider {
    pub fn new() -> Result<Self, String> {
        let api_key = std::env::var("MY_PROVIDER_API_KEY")
            .map_err(|_| "MY_PROVIDER_API_KEY not set")?;
        
        Ok(Self {
            api_key,
            client: reqwest::Client::new(),
        })
    }
}

#[async_trait]
impl TitleProvider for MyProvider {
    async fn generate_title(
        &self,
        _git_repo: &str,
        _target_branch: &str,
        prompt: &str,
    ) -> Result<String, String> {
        // Implementation here
    }

    async fn generate_branch_name(
        &self,
        _git_repo: &str,
        _target_branch: &str,
        prompt: &str,
        session_id: &str,
    ) -> Result<String, String> {
        // Implementation here
    }

    fn name(&self) -> &'static str {
        "my-provider"
    }
}
```

3. Add your provider to the `ProviderType` enum in `mod.rs`:

```rust
pub enum ProviderType {
    AnthropicHaiku,
    GeminiFlash,
    MyProvider,  // Add this
}
```

4. Update the `from_str` method to recognize your provider:

```rust
impl ProviderType {
    pub fn from_str(s: &str) -> Result<Self, String> {
        match s.to_lowercase().as_str() {
            // ... existing matches ...
            "my-provider" => Ok(ProviderType::MyProvider),
            _ => Err(format!("Unknown provider type: {}", s)),
        }
    }
}
```

5. Update the factory's `create` method:

```rust
impl ProviderFactory {
    pub fn create(provider_type: ProviderType) -> Result<Box<dyn TitleProvider>, String> {
        match provider_type {
            // ... existing providers ...
            ProviderType::MyProvider => {
                Ok(Box::new(my_provider::MyProvider::new()?))
            }
        }
    }
}
```

6. Add your module to `mod.rs`:

```rust
pub mod my_provider;
```

## Architecture

### Trait: `TitleProvider`

The core abstraction that all providers must implement:

```rust
#[async_trait]
pub trait TitleProvider: Send + Sync {
    async fn generate_title(&self, git_repo: &str, target_branch: &str, prompt: &str) -> Result<String, String>;
    async fn generate_branch_name(&self, git_repo: &str, target_branch: &str, prompt: &str, session_id: &str) -> Result<String, String>;
    fn name(&self) -> &'static str;
}
```

### Factory Pattern

The `ProviderFactory` handles provider instantiation:

- `create_from_env()`: Creates a provider based on the `TITLE_PROVIDER` environment variable
- `create(provider_type)`: Creates a specific provider by type

### Backward Compatibility

The existing `anthropic.rs` API remains unchanged. Callers don't need to be aware of the provider abstraction - they continue to use the same functions, which now delegate to the configured provider.

## Testing

Run the unit tests to verify provider configuration:

```bash
cargo test --lib title_providers
```

The tests verify:
- Provider type string parsing
- Factory provider creation
- Environment-based configuration

Note: Integration tests that actually call AI APIs require valid API keys to be set in the environment.
