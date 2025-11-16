pub mod anthropic_provider;
pub mod gemini_provider;

#[cfg(test)]
mod tests;

use async_trait::async_trait;

/// Trait for title generation providers
#[async_trait]
pub trait TitleProvider: Send + Sync {
    /// Generate a session title based on the user's prompt
    async fn generate_title(
        &self,
        git_repo: &str,
        target_branch: &str,
        prompt: &str,
    ) -> Result<String, String>;

    /// Generate a git branch name based on the user's prompt
    async fn generate_branch_name(
        &self,
        git_repo: &str,
        target_branch: &str,
        prompt: &str,
        session_id: &str,
    ) -> Result<String, String>;

    /// Get the name of this provider
    fn name(&self) -> &'static str;
}

/// Provider types available in the system
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProviderType {
    AnthropicHaiku,
    GeminiFlash,
}

impl ProviderType {
    /// Parse provider type from string
    pub fn from_str(s: &str) -> Result<Self, String> {
        match s.to_lowercase().as_str() {
            "anthropic" | "anthropic-haiku" | "anthropic-haiku-4-5" => {
                Ok(ProviderType::AnthropicHaiku)
            }
            "gemini" | "gemini-flash" | "google-gemini-2-5" | "google-gemini-2.5-flash" => {
                Ok(ProviderType::GeminiFlash)
            }
            _ => Err(format!("Unknown provider type: {}", s)),
        }
    }
}

/// Factory for creating title providers
pub struct ProviderFactory;

impl ProviderFactory {
    /// Create a provider based on environment configuration
    /// Reads from TITLE_PROVIDER env var, defaults to anthropic-haiku
    /// API key is read from TITLE_API_KEY environment variable
    pub fn create_from_env() -> Result<Box<dyn TitleProvider>, String> {
        let provider_name = std::env::var("TITLE_PROVIDER")
            .unwrap_or_else(|_| "anthropic-haiku".to_string());

        let provider_type = ProviderType::from_str(&provider_name)?;

        match provider_type {
            ProviderType::AnthropicHaiku => {
                Ok(Box::new(anthropic_provider::AnthropicProvider::new()?))
            }
            ProviderType::GeminiFlash => {
                Ok(Box::new(gemini_provider::GeminiProvider::new()?))
            }
        }
    }
}
