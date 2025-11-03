use serde::{Deserialize, Serialize};
use std::env;

#[derive(Debug, Serialize)]
struct AnthropicRequest {
    model: String,
    max_tokens: u32,
    messages: Vec<Message>,
}

#[derive(Debug, Serialize)]
struct Message {
    role: String,
    content: String,
}

#[derive(Debug, Deserialize)]
struct AnthropicResponse {
    content: Vec<ContentBlock>,
}

#[derive(Debug, Deserialize)]
struct ContentBlock {
    text: String,
}

pub async fn generate_session_title(
    git_repo: Option<&str>,
    target_branch: Option<&str>,
    prompt: Option<&str>,
) -> Result<String, String> {
    let api_key = env::var("ANTHROPIC_API_KEY")
        .map_err(|_| "ANTHROPIC_API_KEY not set in environment".to_string())?;

    // Build context from available information
    let mut context_parts = Vec::new();

    if let Some(repo) = git_repo {
        context_parts.push(format!("Git repository: {}", repo));
    }

    if let Some(branch) = target_branch {
        context_parts.push(format!("Target branch: {}", branch));
    }

    if let Some(p) = prompt {
        context_parts.push(format!("Prompt: {}", p));
    }

    let context = if context_parts.is_empty() {
        "No context provided".to_string()
    } else {
        context_parts.join("\n")
    };

    let user_message = format!(
        "Generate a concise, descriptive title (max 60 characters) for a coding session based on this context:\n\n{}\n\nRespond with ONLY the title, nothing else.",
        context
    );

    let request_body = AnthropicRequest {
        model: "claude-3-haiku-20240307".to_string(),
        max_tokens: 100,
        messages: vec![Message {
            role: "user".to_string(),
            content: user_message,
        }],
    };

    let client = reqwest::Client::new();
    let response = client
        .post("https://api.anthropic.com/v1/messages")
        .header("x-api-key", api_key)
        .header("anthropic-version", "2023-06-01")
        .header("content-type", "application/json")
        .json(&request_body)
        .send()
        .await
        .map_err(|e| format!("Failed to send request to Anthropic API: {}", e))?;

    if !response.status().is_success() {
        let status = response.status();
        let error_text = response.text().await.unwrap_or_else(|_| "Unknown error".to_string());
        return Err(format!("Anthropic API error ({}): {}", status, error_text));
    }

    let anthropic_response: AnthropicResponse = response
        .json()
        .await
        .map_err(|e| format!("Failed to parse Anthropic API response: {}", e))?;

    let title = anthropic_response
        .content
        .first()
        .map(|block| block.text.trim().to_string())
        .unwrap_or_else(|| "Untitled Session".to_string());

    Ok(title)
}
