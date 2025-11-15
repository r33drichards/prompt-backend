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
    git_repo: &str,
    target_branch: &str,
    prompt: &str,
) -> Result<String, String> {
    let api_key = env::var("ANTHROPIC_API_KEY")
        .map_err(|_| "ANTHROPIC_API_KEY not set in environment".to_string())?;

    let user_message = format!(
        "Generate a specific, descriptive title (max 60 characters) for a coding task.\n\nUser's request: {}\n\nIMPORTANT RULES:\n1. Extract the CORE TASK from the user's prompt - what specific thing are they asking for?\n2. Start with an action verb: Improve, Fix, Add, Implement, Refactor, Update, Remove, etc.\n3. Include the specific component/feature being modified\n4. NEVER use generic phrases like \"Code Session\", \"Update Master Branch\", \"Work on [repo name]\"\n5. If the request is vague, make your best guess about the specific work being done\n\nGOOD title examples:\n- User says \"the auto title generation could use some improvement\" → \"Improve Auto Title Generation Prompt\"\n- User says \"fix the memory leak\" → \"Fix Memory Leak in Session Handler\"\n- User says \"add authentication\" → \"Implement User Authentication\"\n- User says \"refactor the database code\" → \"Refactor Database Connection Layer\"\n\nBAD title examples (NEVER generate these):\n- \"Prompt-Backend Code Session: Update Master Branch\" ❌ Too generic\n- \"Update Code\" ❌ Not specific\n- \"Code Session\" ❌ Meaningless\n- \"Work on Repository\" ❌ Too vague\n\nRepository context: {}\nTarget branch: {}\n\nRespond with ONLY the title, nothing else. Make it specific to the actual task!",
        prompt,
        git_repo,
        target_branch
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
        let error_text = response
            .text()
            .await
            .unwrap_or_else(|_| "Unknown error".to_string());
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

pub async fn generate_branch_name(
    git_repo: &str,
    target_branch: &str,
    prompt: &str,
    session_id: &str,
) -> Result<String, String> {
    let api_key = env::var("ANTHROPIC_API_KEY")
        .map_err(|_| "ANTHROPIC_API_KEY not set in environment".to_string())?;

    // Build context from available information
    let mut context_parts = Vec::new();
    context_parts.push(format!("Git repository: {}", git_repo));
    context_parts.push(format!("target branch: {}", target_branch));
    context_parts.push(format!("Prompt: {}", prompt));
    let context = context_parts.join("\n");

    let user_message = format!(
        "Generate a concise, descriptive git branch name (max 50 characters) for a coding session based on this context:\n\n{}\n\nThe branch name should be:\n- Descriptive of the task/feature\n- In kebab-case (lowercase with hyphens)\n- Git-safe (only alphanumeric characters and hyphens)\n\nRespond with ONLY the branch name, nothing else. Do NOT include 'claude/' prefix.",
        context
    );

    let request_body = AnthropicRequest {
        model: "claude-3-haiku-20240307".to_string(),
        max_tokens: 50,
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
        let error_text = response
            .text()
            .await
            .unwrap_or_else(|_| "Unknown error".to_string());
        return Err(format!("Anthropic API error ({}): {}", status, error_text));
    }

    let anthropic_response: AnthropicResponse = response
        .json()
        .await
        .map_err(|e| format!("Failed to parse Anthropic API response: {}", e))?;

    let mut branch_name = anthropic_response
        .content
        .first()
        .map(|block| block.text.trim().to_string())
        .unwrap_or_else(|| "untitled-session".to_string());

    // Clean up the branch name to ensure it's git-safe
    branch_name = branch_name
        .to_lowercase()
        .chars()
        .map(|c| {
            if c.is_alphanumeric() || c == '-' {
                c
            } else {
                '-'
            }
        })
        .collect::<String>()
        .split('-')
        .filter(|s| !s.is_empty())
        .collect::<Vec<&str>>()
        .join("-");

    // Add claude/ prefix and session ID suffix
    let full_branch_name = format!("claude/{}-{}", branch_name, &session_id[..24]);

    Ok(full_branch_name)
}
