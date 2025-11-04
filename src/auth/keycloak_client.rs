use reqwest::Client;
use serde::{Deserialize, Serialize};
use std::env;
use tracing::{error, info};

/// Response from Keycloak's broker token endpoint
#[derive(Debug, Deserialize, Serialize)]
pub struct BrokerTokenResponse {
    pub access_token: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub expires_in: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub refresh_token: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub token_type: Option<String>,
}

/// Error types for Keycloak operations
#[derive(Debug, thiserror::Error)]
pub enum KeycloakError {
    #[error("HTTP request failed: {0}")]
    RequestFailed(#[from] reqwest::Error),
    #[error("User not found or not linked to GitHub")]
    UserNotLinked,
    #[error("Invalid configuration: {0}")]
    InvalidConfig(String),
    #[error("Unauthorized: {0}")]
    Unauthorized(String),
}

/// Client for interacting with Keycloak to retrieve external IdP tokens
#[derive(Clone)]
pub struct KeycloakClient {
    http_client: Client,
    keycloak_base_url: String,
    realm: String,
}

impl KeycloakClient {
    /// Create a new Keycloak client
    pub fn new() -> Result<Self, KeycloakError> {
        let keycloak_issuer = env::var("KEYCLOAK_ISSUER").map_err(|_| {
            KeycloakError::InvalidConfig("KEYCLOAK_ISSUER not set".to_string())
        })?;

        // Extract base URL and realm from issuer
        // Expected format: https://keycloak.example.com/realms/realm-name
        let parts: Vec<&str> = keycloak_issuer.rsplitn(2, "/realms/").collect();
        if parts.len() != 2 {
            return Err(KeycloakError::InvalidConfig(
                "Invalid KEYCLOAK_ISSUER format".to_string(),
            ));
        }

        let realm = parts[0].to_string();
        let keycloak_base_url = parts[1].to_string();

        info!(
            "Initialized Keycloak client for realm '{}' at '{}'",
            realm, keycloak_base_url
        );

        Ok(Self {
            http_client: Client::new(),
            keycloak_base_url,
            realm,
        })
    }

    /// Retrieve GitHub token for a user using Keycloak's broker token endpoint
    ///
    /// This endpoint requires the user's Keycloak access token (the one your backend validates)
    /// and returns the external identity provider's (GitHub's) access token.
    ///
    /// # Arguments
    /// * `user_access_token` - The Keycloak access token for the user (from Authorization header)
    /// * `provider_alias` - The identity provider alias (e.g., "github")
    ///
    /// # Returns
    /// The GitHub access token that can be used for GitHub API calls and git operations
    pub async fn get_idp_token(
        &self,
        user_access_token: &str,
        provider_alias: &str,
    ) -> Result<String, KeycloakError> {
        // Keycloak broker token endpoint:
        // GET /realms/{realm}/broker/{provider}/token
        let url = format!(
            "{}/realms/{}/broker/{}/token",
            self.keycloak_base_url, self.realm, provider_alias
        );

        info!(
            "Fetching IdP token for provider '{}' from Keycloak",
            provider_alias
        );

        let response = self
            .http_client
            .get(&url)
            .bearer_auth(user_access_token)
            .send()
            .await?;

        let status = response.status();
        if !status.is_success() {
            let error_text = response.text().await.unwrap_or_default();
            error!(
                "Failed to retrieve IdP token: status={}, body={}",
                status, error_text
            );

            if status.as_u16() == 404 || status.as_u16() == 400 {
                return Err(KeycloakError::UserNotLinked);
            }

            return Err(KeycloakError::Unauthorized(format!(
                "Status: {}, Body: {}",
                status, error_text
            )));
        }

        let token_response: BrokerTokenResponse = response.json().await?;

        info!("Successfully retrieved IdP token for provider '{}'", provider_alias);

        Ok(token_response.access_token)
    }

    /// Get GitHub token specifically (convenience method)
    pub async fn get_github_token(
        &self,
        user_access_token: &str,
    ) -> Result<String, KeycloakError> {
        self.get_idp_token(user_access_token, "github").await
    }
}

impl Default for KeycloakClient {
    fn default() -> Self {
        Self::new().expect("Failed to create KeycloakClient")
    }
}
