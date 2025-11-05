use reqwest::Client;
use serde::{Deserialize, Serialize};
use std::env;
use tracing::{error, info};

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
    #[error("User not found or not linked to GitHub")]
    UserNotLinked,
    #[error("Invalid configuration: {0}")]
    InvalidConfig(String),
    #[error("Unauthorized: {0}")]
    Unauthorized(String),
    #[error("Admin authentication failed: {0}")]
    AdminAuthFailed(String),
}

/// Client for interacting with Keycloak Admin API to retrieve external IdP tokens
#[derive(Clone)]
pub struct KeycloakClient {
    http_client: Client,
    keycloak_base_url: String,
    realm: String,
    admin_username: String,
    admin_password: String,
    client_id: String,
    client_secret: String,
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

        // Get client credentials for token exchange
        let client_id = env::var("KEYCLOAK_CLIENT_ID").map_err(|_| {
            KeycloakError::InvalidConfig("KEYCLOAK_CLIENT_ID not set".to_string())
        })?;

        let client_secret = env::var("KEYCLOAK_CLIENT_SECRET").map_err(|_| {
            KeycloakError::InvalidConfig("KEYCLOAK_CLIENT_SECRET not set".to_string())
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
            client_id,
            client_secret,
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

    /// Impersonate a user using token exchange to get their access token
    /// This uses the admin token to exchange for a user's token
    async fn impersonate_user(&self, user_id: &str) -> Result<String, KeycloakError> {
        let admin_token = self.get_admin_token().await?;

        let url = format!(
            "{}/realms/{}/protocol/openid-connect/token",
            self.keycloak_base_url, self.realm
        );

        info!("Impersonating user {} via token exchange", user_id);

        let params = [
            ("grant_type", "urn:ietf:params:oauth:grant-type:token-exchange"),
            ("client_id", &self.client_id),
            ("client_secret", &self.client_secret),
            ("subject_token", &admin_token),
            ("requested_subject", user_id),
            ("requested_token_type", "urn:ietf:params:oauth:token-type:access_token"),
        ];

        let response = self.http_client.post(&url).form(&params).send().await?;

        let status = response.status();
        if !status.is_success() {
            let error_text = response.text().await.unwrap_or_default();
            error!(
                "Failed to impersonate user: status={}, body={}",
                status, error_text
            );

            return Err(KeycloakError::Unauthorized(format!(
                "Status: {}, Body: {}. Note: Ensure token-exchange is enabled and client has impersonation permissions",
                status, error_text
            )));
        }

        let token_response: TokenResponse = response.json().await?;
        info!("Successfully impersonated user {}", user_id);
        Ok(token_response.access_token)
    }

    /// Get external IdP token using the broker endpoint with user's token
    async fn get_broker_token(
        &self,
        user_token: &str,
        provider_alias: &str,
    ) -> Result<String, KeycloakError> {
        let url = format!(
            "{}/realms/{}/broker/{}/token",
            self.keycloak_base_url, self.realm, provider_alias
        );

        info!("Fetching {} token from broker endpoint", provider_alias);

        let response = self
            .http_client
            .get(&url)
            .bearer_auth(user_token)
            .send()
            .await?;

        let status = response.status();
        if !status.is_success() {
            let error_text = response.text().await.unwrap_or_default();
            error!(
                "Failed to retrieve IdP token from broker: status={}, body={}",
                status, error_text
            );

            if status.as_u16() == 400 {
                return Err(KeycloakError::UserNotLinked);
            }

            return Err(KeycloakError::Unauthorized(format!(
                "Status: {}, Body: {}. Note: Ensure storeToken=true and 'Stored Tokens Readable' is enabled in Keycloak IdP config",
                status, error_text
            )));
        }

        let token_response: TokenResponse = response.json().await?;
        Ok(token_response.access_token)
    }

    /// Retrieve GitHub token for a user using Token Exchange
    ///
    /// This method uses a two-step token exchange process:
    /// 1. Impersonate the user using admin credentials (get user's access token)
    /// 2. Use the impersonated token to retrieve the external IdP token from the broker endpoint
    ///
    /// # Arguments
    /// * `user_id` - The Keycloak user ID (subject from JWT)
    /// * `provider_alias` - The identity provider alias (e.g., "github")
    ///
    /// # Returns
    /// The GitHub access token that can be used for GitHub API calls and git operations
    pub async fn get_idp_token_for_user(
        &self,
        user_id: &str,
        provider_alias: &str,
    ) -> Result<String, KeycloakError> {
        info!(
            "Retrieving {} token for user {} via token exchange",
            provider_alias, user_id
        );

        // Step 1: Impersonate the user to get their access token
        let user_token = self.impersonate_user(user_id).await?;

        // Step 2: Use the user's token to get the external IdP token from the broker endpoint
        let idp_token = self.get_broker_token(&user_token, provider_alias).await?;

        info!(
            "Successfully retrieved {} token for user {}",
            provider_alias, user_id
        );

        Ok(idp_token)
    }

    /// Get GitHub token for a user (convenience method)
    ///
    /// # Arguments
    /// * `user_id` - The Keycloak user ID (from JWT's 'sub' claim)
    pub async fn get_github_token_for_user(&self, user_id: &str) -> Result<String, KeycloakError> {
        self.get_idp_token_for_user(user_id, "github").await
    }
}

impl Default for KeycloakClient {
    fn default() -> Self {
        Self::new().expect("Failed to create KeycloakClient")
    }
}
