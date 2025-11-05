use reqwest::Client;
use serde::{Deserialize, Serialize};
use std::env;
use tracing::{error, info, warn};

/// Response from Keycloak's token endpoint
#[derive(Debug, Deserialize, Serialize)]
pub struct TokenResponse {
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
    #[error("Invalid configuration: {0}")]
    InvalidConfig(String),
    #[error("Unauthorized: {0}")]
    Unauthorized(String),
    #[error("Admin authentication failed: {0}")]
    AdminAuthFailed(String),
}

/// Client for interacting with Keycloak Admin API
#[derive(Clone)]
pub struct KeycloakClient {
    http_client: Client,
    keycloak_base_url: String,
    realm: String,
    admin_username: String,
    admin_password: String,
}

impl KeycloakClient {
    /// Create a new Keycloak client
    pub fn new() -> Result<Self, KeycloakError> {
        let keycloak_base_url = env::var("KEYCLOAK_URL")
            .map_err(|_| KeycloakError::InvalidConfig("KEYCLOAK_URL not set".to_string()))?;

        let realm = env::var("KEYCLOAK_REALM")
            .map_err(|_| KeycloakError::InvalidConfig("KEYCLOAK_REALM not set".to_string()))?;

        // Get admin credentials from environment
        let admin_username = env::var("KEYCLOAK_ADMIN_USERNAME").map_err(|_| {
            KeycloakError::InvalidConfig("KEYCLOAK_ADMIN_USERNAME not set".to_string())
        })?;

        let admin_password = env::var("KEYCLOAK_ADMIN_PASSWORD").map_err(|_| {
            KeycloakError::InvalidConfig("KEYCLOAK_ADMIN_PASSWORD not set".to_string())
        })?;

        info!(
            "Initialized Keycloak Admin client for realm '{}' at '{}'",
            realm, keycloak_base_url
        );

        Ok(Self {
            http_client: Client::new(),
            keycloak_base_url,
            realm,
            admin_username,
            admin_password,
        })
    }

    /// Authenticate with Keycloak as admin and get an access token
    async fn get_admin_token(&self) -> Result<String, KeycloakError> {
        let url = format!(
            "{}/realms/master/protocol/openid-connect/token",
            self.keycloak_base_url
        );

        info!("Authenticating as Keycloak admin");

        let params = [
            ("grant_type", "password"),
            ("client_id", "admin-cli"),
            ("username", &self.admin_username),
            ("password", &self.admin_password),
        ];

        let response = self.http_client.post(&url).form(&params).send().await?;

        let status = response.status();
        if !status.is_success() {
            let error_text = response.text().await.unwrap_or_default();
            error!(
                "Failed to authenticate as admin: status={}, body={}",
                status, error_text
            );

            return Err(KeycloakError::AdminAuthFailed(format!(
                "Status: {}, Body: {}",
                status, error_text
            )));
        }

        let token_response: TokenResponse = response.json().await?;
        Ok(token_response.access_token)
    }

}

impl Default for KeycloakClient {
    fn default() -> Self {
        Self::new().expect("Failed to create KeycloakClient")
    }
}
