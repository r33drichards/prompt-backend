use super::title_providers::{ProviderFactory, ProviderType, TitleProvider};

/// Generate a session title using the configured title provider
/// Reads TITLE_PROVIDER env var to determine which provider to use
/// Defaults to Anthropic Haiku if not specified
pub async fn generate_session_title(
    git_repo: &str,
    target_branch: &str,
    prompt: &str,
) -> Result<String, String> {
    generate_session_title_with_key(git_repo, target_branch, prompt, None).await
}

/// Generate a session title using the configured title provider with an optional API key
/// If api_key is provided, it will be used instead of the environment variable
/// Reads TITLE_PROVIDER env var to determine which provider to use
/// Defaults to Anthropic Haiku if not specified
pub async fn generate_session_title_with_key(
    git_repo: &str,
    target_branch: &str,
    prompt: &str,
    api_key: Option<String>,
) -> Result<String, String> {
    let provider = if let Some(key) = api_key {
        // If a custom API key is provided, use it
        let provider_name = std::env::var("TITLE_PROVIDER")
            .unwrap_or_else(|_| "anthropic-haiku".to_string());
        let provider_type = ProviderType::from_str(&provider_name)?;
        ProviderFactory::create(provider_type, Some(key))?
    } else {
        ProviderFactory::create_from_env()?
    };
    provider.generate_title(git_repo, target_branch, prompt).await
}

/// Generate a git branch name using the configured title provider
/// Reads TITLE_PROVIDER env var to determine which provider to use
/// Defaults to Anthropic Haiku if not specified
pub async fn generate_branch_name(
    git_repo: &str,
    target_branch: &str,
    prompt: &str,
    session_id: &str,
) -> Result<String, String> {
    generate_branch_name_with_key(git_repo, target_branch, prompt, session_id, None).await
}

/// Generate a git branch name using the configured title provider with an optional API key
/// If api_key is provided, it will be used instead of the environment variable
/// Reads TITLE_PROVIDER env var to determine which provider to use
/// Defaults to Anthropic Haiku if not specified
pub async fn generate_branch_name_with_key(
    git_repo: &str,
    target_branch: &str,
    prompt: &str,
    session_id: &str,
    api_key: Option<String>,
) -> Result<String, String> {
    let provider = if let Some(key) = api_key {
        // If a custom API key is provided, use it
        let provider_name = std::env::var("TITLE_PROVIDER")
            .unwrap_or_else(|_| "anthropic-haiku".to_string());
        let provider_type = ProviderType::from_str(&provider_name)?;
        ProviderFactory::create(provider_type, Some(key))?
    } else {
        ProviderFactory::create_from_env()?
    };
    provider
        .generate_branch_name(git_repo, target_branch, prompt, session_id)
        .await
}
