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

/// Federated identity information from Keycloak Admin API
#[derive(Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FederatedIdentity {
    pub identity_provider: String,
    pub user_id: String,
    pub user_name: String,
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

    /// Get federated identities for a user
    async fn get_federated_identities(
        &self,
        admin_token: &str,
        user_id: &str,
    ) -> Result<Vec<FederatedIdentity>, KeycloakError> {
        let url = format!(
            "{}/admin/realms/{}/users/{}/federated-identity",
            self.keycloak_base_url, self.realm, user_id
        );

        info!("Fetching federated identities for user {}", user_id);

        let response = self
            .http_client
            .get(&url)
            .bearer_auth(admin_token)
            .send()
            .await?;

        let status = response.status();
        if !status.is_success() {
            let error_text = response.text().await.unwrap_or_default();
            error!(
                "Failed to retrieve federated identities: status={}, body={}",
                status, error_text
            );

            if status.as_u16() == 404 {
                return Err(KeycloakError::UserNotLinked);
            }

            return Err(KeycloakError::Unauthorized(format!(
                "Status: {}, Body: {}",
                status, error_text
            )));
        }

        let identities: Vec<FederatedIdentity> = response.json().await?;
        Ok(identities)
    }

    /// Retrieve GitHub token for a user using Keycloak Admin API
    ///
    /// This method uses admin credentials to query the user's federated identity
    /// and retrieve the stored GitHub token from Keycloak.
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
        // Step 1: Get admin access token
        let admin_token = self.get_admin_token().await?;

        // Step 2: Get user's federated identities
        let identities = self.get_federated_identities(&admin_token, user_id).await?;

        info!(
            "Found {} federated identities for user {}: {:?}",
            identities.len(),
            user_id,
            identities
                .iter()
                .map(|id| &id.identity_provider)
                .collect::<Vec<_>>()
        );

        // Step 3: Find the GitHub identity
        let github_identity = identities
            .iter()
            .find(|id| id.identity_provider == provider_alias)
            .ok_or_else(|| {
                warn!(
                    "User {} not linked to identity provider '{}'. Available providers: {:?}",
                    user_id,
                    provider_alias,
                    identities
                        .iter()
                        .map(|id| &id.identity_provider)
                        .collect::<Vec<_>>()
                );
                KeycloakError::UserNotLinked
            })?;

        info!(
            "Found {} identity for user {}: {}",
            provider_alias, user_id, github_identity.user_name
        );

        // Step 4: Use broker token endpoint to get the actual token
        // We need to use the admin token to impersonate the user
        let token_url = format!(
            "{}/admin/realms/{}/users/{}/federated-identity/{}/token",
            self.keycloak_base_url, self.realm, user_id, provider_alias
        );

        info!(
            "Fetching stored token for {} identity from URL: {}",
            provider_alias, token_url
        );

        let response = self
            .http_client
            .get(&token_url)
            .bearer_auth(&admin_token)
            .send()
            .await?;

        let status = response.status();
        if !status.is_success() {
            let error_text = response.text().await.unwrap_or_default();
            error!(
                "Failed to retrieve IdP token: status={}, body={}, url={}",
                status, error_text, token_url
            );

            let error_msg = if status.as_u16() == 404 {
                format!(
                    "Status: {}, Body: {}. Possible causes:\n\
                    1. User needs to log out and log back in after storeToken was enabled\n\
                    2. Keycloak version may not support token retrieval (requires v18+)\n\
                    3. IdP configuration may be missing 'Store Tokens' setting\n\
                    4. Token may have expired and needs refresh\n\
                    Troubleshooting:\n\
                    - Verify storeToken=true in Keycloak IdP settings\n\
                    - Check Keycloak version: {} endpoint requires Keycloak 18+\n\
                    - Ask user to re-authenticate with GitHub\n\
                    - Check Keycloak server logs for more details",
                    status, error_text, token_url
                )
            } else {
                format!(
                    "Status: {}, Body: {}. Note: Ensure storeToken=true in Keycloak IdP config",
                    status, error_text
                )
            };

            return Err(KeycloakError::Unauthorized(error_msg));
        }

        let token_response: TokenResponse = response.json().await?;

        info!(
            "Successfully retrieved {} token for user {}",
            provider_alias, user_id
        );

        Ok(token_response.access_token)
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
