use super::title_providers::ProviderFactory;

/// Generate a session title using the configured title provider
/// Reads TITLE_PROVIDER env var to determine which provider to use
/// Defaults to Anthropic Haiku if not specified
/// API keys are read from environment variables (ANTHROPIC_API_KEY or GEMINI_API_KEY)
pub async fn generate_session_title(
    git_repo: &str,
    target_branch: &str,
    prompt: &str,
) -> Result<String, String> {
    let provider = ProviderFactory::create_from_env()?;
    provider.generate_title(git_repo, target_branch, prompt).await
}

/// Generate a git branch name using the configured title provider
/// Reads TITLE_PROVIDER env var to determine which provider to use
/// Defaults to Anthropic Haiku if not specified
/// API keys are read from environment variables (ANTHROPIC_API_KEY or GEMINI_API_KEY)
pub async fn generate_branch_name(
    git_repo: &str,
    target_branch: &str,
    prompt: &str,
    session_id: &str,
) -> Result<String, String> {
    let provider = ProviderFactory::create_from_env()?;
    provider
        .generate_branch_name(git_repo, target_branch, prompt, session_id)
        .await
}
