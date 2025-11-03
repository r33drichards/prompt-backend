use jsonwebtoken::{decode, decode_header, Algorithm, DecodingKey, Validation};
use reqwest;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tokio::sync::RwLock;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Jwk {
    pub kty: String,
    pub kid: String,
    pub n: String,
    pub e: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Jwks {
    pub keys: Vec<Jwk>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Claims {
    pub sub: String,
    pub iss: String,
    pub aud: Option<String>,
    pub exp: u64,
    pub iat: u64,
    pub email: Option<String>,
    pub name: Option<String>,
}

pub struct JwksCache {
    jwks_uri: String,
    issuer: String,
    cache: Arc<RwLock<Option<Jwks>>>,
}

impl JwksCache {
    pub fn new(jwks_uri: String, issuer: String) -> Self {
        Self {
            jwks_uri,
            issuer,
            cache: Arc::new(RwLock::new(None)),
        }
    }

    pub async fn fetch_jwks(&self) -> Result<Jwks, String> {
        let response = reqwest::get(&self.jwks_uri)
            .await
            .map_err(|e| format!("Failed to fetch JWKS: {}", e))?;

        let jwks: Jwks = response
            .json()
            .await
            .map_err(|e| format!("Failed to parse JWKS: {}", e))?;

        let mut cache = self.cache.write().await;
        *cache = Some(jwks.clone());

        Ok(jwks)
    }

    pub async fn get_jwks(&self) -> Result<Jwks, String> {
        let cache = self.cache.read().await;
        if let Some(jwks) = cache.as_ref() {
            return Ok(jwks.clone());
        }
        drop(cache);

        self.fetch_jwks().await
    }

    pub async fn validate_token(&self, token: &str) -> Result<Claims, String> {
        let header = decode_header(token)
            .map_err(|e| format!("Invalid token header: {}", e))?;

        let kid = header.kid.ok_or("Missing kid in token header")?;

        let jwks = self.get_jwks().await?;
        let jwk = jwks
            .keys
            .iter()
            .find(|k| k.kid == kid)
            .ok_or("Key not found in JWKS")?;

        let decoding_key = DecodingKey::from_rsa_components(&jwk.n, &jwk.e)
            .map_err(|e| format!("Invalid RSA key: {}", e))?;

        let mut validation = Validation::new(Algorithm::RS256);
        validation.set_issuer(&[&self.issuer]);
        validation.validate_exp = true;

        let token_data = decode::<Claims>(token, &decoding_key, &validation)
            .map_err(|e| format!("Token validation failed: {}", e))?;

        Ok(token_data.claims)
    }
}
