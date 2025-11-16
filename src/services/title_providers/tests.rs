#[cfg(test)]
mod tests {
    use super::super::{ProviderFactory, ProviderType};

    #[test]
    fn test_provider_type_parsing() {
        // Test Anthropic variants
        assert_eq!(
            ProviderType::from_str("anthropic").unwrap(),
            ProviderType::AnthropicHaiku
        );
        assert_eq!(
            ProviderType::from_str("anthropic-haiku").unwrap(),
            ProviderType::AnthropicHaiku
        );
        assert_eq!(
            ProviderType::from_str("anthropic-haiku-4-5").unwrap(),
            ProviderType::AnthropicHaiku
        );

        // Test Gemini variants
        assert_eq!(
            ProviderType::from_str("gemini").unwrap(),
            ProviderType::GeminiFlash
        );
        assert_eq!(
            ProviderType::from_str("gemini-flash").unwrap(),
            ProviderType::GeminiFlash
        );
        assert_eq!(
            ProviderType::from_str("google-gemini-2-5").unwrap(),
            ProviderType::GeminiFlash
        );
        assert_eq!(
            ProviderType::from_str("google-gemini-2.5-flash").unwrap(),
            ProviderType::GeminiFlash
        );

        // Test unknown provider
        assert!(ProviderType::from_str("unknown").is_err());
    }

    #[test]
    fn test_provider_factory_create() {
        // Note: These tests will fail if the required API key is not set
        // They're here to verify the structure compiles correctly

        // Test creating Anthropic provider
        let result = ProviderFactory::create(ProviderType::AnthropicHaiku);
        // We expect this to fail without TITLE_API_KEY
        if std::env::var("TITLE_API_KEY").is_ok() {
            assert!(result.is_ok());
        } else {
            assert!(result.is_err());
        }

        // Test creating Gemini provider
        let result = ProviderFactory::create(ProviderType::GeminiFlash);
        // We expect this to fail without TITLE_API_KEY
        if std::env::var("TITLE_API_KEY").is_ok() {
            assert!(result.is_ok());
        } else {
            assert!(result.is_err());
        }
    }
}
