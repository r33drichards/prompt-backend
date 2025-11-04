use rocket::serde::json::Json;
use rocket_okapi::openapi;
use rocket_okapi::okapi::schemars::JsonSchema;
use rocket::serde::{Deserialize, Serialize};

use crate::error::{Error, OResult};
use crate::auth::AuthenticatedUser;

#[derive(Serialize, Deserialize, JsonSchema, Clone)]
pub struct SearchRepositoriesInput {
    pub query: Option<String>,
}

#[derive(Serialize, Deserialize, JsonSchema, Clone, Debug)]
pub struct Repository {
    pub id: i64,
    pub name: String,
    pub full_name: String,
    pub private: bool,
    pub html_url: String,
    pub description: Option<String>,
    pub fork: bool,
    pub created_at: String,
    pub updated_at: String,
    pub pushed_at: Option<String>,
    pub stargazers_count: i64,
    pub watchers_count: i64,
    pub language: Option<String>,
    pub default_branch: String,
}

#[derive(Serialize, Deserialize, JsonSchema, Clone)]
pub struct SearchRepositoriesOutput {
    pub repositories: Vec<Repository>,
    pub total_count: usize,
}

/// Search authenticated user's repositories
#[openapi]
#[post("/github/search-repos", data = "<input>")]
pub async fn search_repositories(
    user: AuthenticatedUser,
    input: Json<SearchRepositoriesInput>,
) -> OResult<SearchRepositoriesOutput> {
    // Get GitHub token from user's session
    // Note: This requires the GitHub token to be included in the JWT or stored separately
    // For now, we'll use the GITHUB_TOKEN environment variable as a fallback
    let github_token = std::env::var("GITHUB_TOKEN")
        .map_err(|_| Error::bad_request(
            "GitHub token not available. Please authenticate with GitHub.".to_string()
        ))?;

    let client = reqwest::Client::new();

    // Build the search query
    // Search user's repos by prepending "user:<username>" to the query
    let search_url = if let Some(query) = &input.query {
        // Search user's repos with a query
        format!("https://api.github.com/user/repos?q={}&per_page=100", query)
    } else {
        // List all user's repos
        "https://api.github.com/user/repos?per_page=100&sort=updated".to_string()
    };

    tracing::info!("Searching GitHub repos for user: {} with URL: {}", user.user_id, search_url);

    let response = client
        .get(&search_url)
        .header("Authorization", format!("Bearer {}", github_token))
        .header("Accept", "application/vnd.github+json")
        .header("User-Agent", "prompt-backend")
        .header("X-GitHub-Api-Version", "2022-11-28")
        .send()
        .await
        .map_err(|e| Error::database_error(format!("Failed to connect to GitHub API: {}", e)))?;

    if !response.status().is_success() {
        let status = response.status();
        let error_text = response.text().await.unwrap_or_else(|_| "Unknown error".to_string());
        tracing::error!("GitHub API error ({}): {}", status, error_text);
        return Err(Error::database_error(format!("GitHub API error ({}): {}", status, error_text)));
    }

    let repositories: Vec<Repository> = response
        .json()
        .await
        .map_err(|e| Error::database_error(format!("Failed to parse GitHub API response: {}", e)))?;

    let total_count = repositories.len();

    Ok(Json(SearchRepositoriesOutput {
        repositories,
        total_count,
    }))
}
