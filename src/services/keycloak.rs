use reqwest::Client;
use serde::{Deserialize, Serialize};

/// Keycloak Admin API client for realm management
pub struct KeycloakAdmin {
    base_url: String,
    admin_username: String,
    admin_password: String,
    client: Client,
}

#[derive(Debug, Serialize, Deserialize)]
struct TokenResponse {
    access_token: String,
}

impl KeycloakAdmin {
    /// Create a new Keycloak Admin client from environment variables
    pub fn from_env() -> Result<Self, String> {
        let base_url =
            std::env::var("KEYCLOAK_URL").map_err(|_| "KEYCLOAK_URL must be set".to_string())?;
        let admin_username = std::env::var("KEYCLOAK_ADMIN_USERNAME")
            .map_err(|_| "KEYCLOAK_ADMIN_USERNAME must be set".to_string())?;
        let admin_password = std::env::var("KEYCLOAK_ADMIN_PASSWORD")
            .map_err(|_| "KEYCLOAK_ADMIN_PASSWORD must be set".to_string())?;

        Ok(Self {
            base_url,
            admin_username,
            admin_password,
            client: Client::new(),
        })
    }

    /// Get an admin access token from Keycloak
    async fn get_admin_token(&self) -> Result<String, String> {
        let token_url = format!(
            "{}/realms/master/protocol/openid-connect/token",
            self.base_url
        );

        let response = self
            .client
            .post(&token_url)
            .form(&[
                ("grant_type", "password"),
                ("client_id", "admin-cli"),
                ("username", &self.admin_username),
                ("password", &self.admin_password),
            ])
            .send()
            .await
            .map_err(|e| format!("Failed to request admin token: {}", e))?;

        let status = response.status();
        if !status.is_success() {
            let body = response
                .text()
                .await
                .unwrap_or_else(|_| "unable to read response body".to_string());
            return Err(format!(
                "Failed to get admin token (HTTP {}): {}",
                status, body
            ));
        }

        let token_response: TokenResponse = response
            .json()
            .await
            .map_err(|e| format!("Failed to parse token response: {}", e))?;

        Ok(token_response.access_token)
    }

    /// Check if a realm exists
    pub async fn realm_exists(&self, realm_name: &str) -> Result<bool, String> {
        let token = self.get_admin_token().await?;
        let realm_url = format!("{}/admin/realms/{}", self.base_url, realm_name);

        let response = self
            .client
            .get(&realm_url)
            .bearer_auth(&token)
            .send()
            .await
            .map_err(|e| format!("Failed to check realm: {}", e))?;

        match response.status().as_u16() {
            200 => Ok(true),
            404 => Ok(false),
            status => {
                let body = response
                    .text()
                    .await
                    .unwrap_or_else(|_| "unable to read response body".to_string());
                Err(format!("Failed to check realm (HTTP {}): {}", status, body))
            }
        }
    }

    /// Create a realm from a JSON configuration
    pub async fn create_realm(&self, realm_json: &str) -> Result<(), String> {
        let token = self.get_admin_token().await?;
        let realms_url = format!("{}/admin/realms", self.base_url);

        let response = self
            .client
            .post(&realms_url)
            .bearer_auth(&token)
            .header("Content-Type", "application/json")
            .body(realm_json.to_string())
            .send()
            .await
            .map_err(|e| format!("Failed to create realm: {}", e))?;

        let status = response.status();
        if !status.is_success() {
            let body = response
                .text()
                .await
                .unwrap_or_else(|_| "unable to read response body".to_string());
            return Err(format!(
                "Failed to create realm (HTTP {}): {}",
                status, body
            ));
        }

        Ok(())
    }
}

/// Setup the Keycloak realm if it doesn't exist
/// Uses the embedded realm configuration
pub async fn setup_keycloak_realm() -> Result<(), String> {
    let realm_name =
        std::env::var("KEYCLOAK_REALM").map_err(|_| "KEYCLOAK_REALM must be set".to_string())?;

    println!("Checking if Keycloak realm '{}' exists...", realm_name);

    let admin = KeycloakAdmin::from_env()?;

    if admin.realm_exists(&realm_name).await? {
        println!("Keycloak realm '{}' already exists", realm_name);
        return Ok(());
    }

    println!(
        "Keycloak realm '{}' does not exist, creating...",
        realm_name
    );

    // Embedded realm configuration
    let realm_json = include_str!("../../keycloak/oauth2-realm.json");

    admin.create_realm(realm_json).await?;

    println!("Keycloak realm '{}' created successfully", realm_name);

    Ok(())
}
