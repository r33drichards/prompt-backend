use rocket::http::Status;
use rocket::request::{FromRequest, Outcome, Request};
use rocket::State;
use rocket_okapi::request::{OpenApiFromRequest, RequestHeaderInput};
use rocket_okapi::gen::OpenApiGenerator;
use rocket_okapi::okapi::openapi3::{Responses, SecurityRequirement, SecurityScheme, SecuritySchemeData};

use super::jwks::JwksCache;

#[derive(Debug, Clone)]
pub struct AuthenticatedUser {
    pub user_id: String,
    pub email: Option<String>,
    pub name: Option<String>,
}

#[rocket::async_trait]
impl<'r> FromRequest<'r> for AuthenticatedUser {
    type Error = String;

    async fn from_request(request: &'r Request<'_>) -> Outcome<Self, Self::Error> {
        tracing::debug!("AuthenticatedUser::from_request called");

        let jwks_cache = request
            .guard::<&State<JwksCache>>()
            .await
            .succeeded()
            .ok_or("JWKS cache not available".to_string());

        if let Err(e) = jwks_cache {
            tracing::error!("JWKS cache not available: {}", e);
            return Outcome::Error((Status::InternalServerError, e));
        }

        let jwks_cache = jwks_cache.unwrap();

        let auth_header = request.headers().get_one("Authorization");
        if auth_header.is_none() {
            tracing::warn!("Missing Authorization header");
            return Outcome::Error((
                Status::Unauthorized,
                "Missing Authorization header".to_string(),
            ));
        }

        let auth_header = auth_header.unwrap();
        tracing::debug!("Authorization header: {}", &auth_header[..20.min(auth_header.len())]);

        if !auth_header.starts_with("Bearer ") {
            tracing::warn!("Invalid Authorization header format: {}", auth_header);
            return Outcome::Error((
                Status::Unauthorized,
                "Invalid Authorization header format".to_string(),
            ));
        }

        let token = &auth_header[7..];

        match jwks_cache.validate_token(token).await {
            Ok(claims) => {
                tracing::info!("Token validated successfully for user: {}", claims.sub);
                Outcome::Success(AuthenticatedUser {
                    user_id: claims.sub,
                    email: claims.email,
                    name: claims.name,
                })
            },
            Err(e) => {
                tracing::error!("Token validation failed: {}", e);
                Outcome::Error((Status::Unauthorized, e))
            },
        }
    }
}

impl<'a> OpenApiFromRequest<'a> for AuthenticatedUser {
    fn from_request_input(
        _gen: &mut OpenApiGenerator,
        _name: String,
        _required: bool,
    ) -> rocket_okapi::Result<RequestHeaderInput> {
        let mut security_req = SecurityRequirement::new();
        security_req.insert("Bearer".to_string(), vec![]);

        Ok(RequestHeaderInput::Security(
            "Bearer".to_string(),
            SecurityScheme {
                description: Some("JWT Bearer token from Keycloak".to_string()),
                data: SecuritySchemeData::Http {
                    scheme: "bearer".to_string(),
                    bearer_format: Some("JWT".to_string()),
                },
                extensions: Default::default(),
            },
            security_req,
        ))
    }

    fn get_responses(_gen: &mut OpenApiGenerator) -> rocket_okapi::Result<Responses> {
        Ok(Responses::default())
    }
}
