use apalis::prelude::*;
use sea_orm::{ColumnTrait, DatabaseConnection, EntityTrait, QueryFilter};
use serde::{Deserialize, Serialize};
use tracing::{error, info};

use sandbox_client::types::ShellExecRequest;

use crate::entities::session::{self, Entity as Session, InboxStatus};

/// Job that reads from PostgreSQL outbox and publishes to Redis
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OutboxJob {
    pub session_id: String,
    pub payload: serde_json::Value,
}

impl Job for OutboxJob {
    const NAME: &'static str = "OutboxJob";
}

/// Context for the outbox publisher containing database and Redis connections
#[derive(Clone)]
pub struct OutboxContext {
    pub db: DatabaseConnection,
}

/// Process an outbox job: read from PostgreSQL sessions with active inbox_status,
/// publish to Redis, and mark as pending
pub async fn process_outbox_job(
    job: OutboxJob,
    ctx: Data<OutboxContext>,
) -> Result<(), Error> {
    info!(
        "Processing outbox job for session_id: {}",
        job.session_id
    );

    // Query sessions with Active inbox_status
    let active_sessions = Session::find()
        .filter(session::Column::InboxStatus.eq(InboxStatus::Active))
        .all(&ctx.db)
        .await
        .map_err(|e| {
            error!("Failed to query active sessions: {}", e);
            Error::Failed(Box::new(e))
        })?;

    info!("Found {} active sessions to process", active_sessions.len());

    // Process each active session
    for _session_model in active_sessions {
        // get sbx config from ip-allocator
        // Read IP_ALLOCATOR_URL from environment, e.g., "http://localhost:8000"
        let ip_allocator_url = std::env::var("IP_ALLOCATOR_URL")
            .unwrap_or_else(|_| "http://localhost:8000".to_string());

        let ip_client = ip_allocator_client::Client::new(&ip_allocator_url);
        let borrowed_ip = ip_client.handlers_ip_borrow().await.map_err(|e| {
            error!("Failed to borrow IP: {}", e);
            Error::Failed(Box::new(e))
        })?;

        // Parse the response JSON to extract mcp_url and api_url
        let mcp_json_string = borrowed_ip.item["mcp_json_string"].as_str()
            .ok_or_else(|| Error::Failed("Missing mcp_json_string in response".into()))?;

        info!("Borrowed sandbox - mcp_json_string: {}", mcp_json_string);

        let api_url = borrowed_ip.item["api_url"].as_str().ok_or_else(|| Error::Failed("Missing api_url in response".into()))?;

        info!("Borrowed sandbox - api_url: {}", api_url);

        // Create sandbox client using the api_url
        let sbx = sandbox_client::Client::new(api_url);

        sbx.exec_command_v1_shell_exec_post(&ShellExecRequest {
            command: "gh auth login --with-token TODO".to_string(),
            async_mode: false,
            id: None,
            timeout: Some(30.0 as f64),
            exec_dir: Some(String::from("/home/gem")),
        }).await.map_err(|e| {
            error!("Failed to execute command: {}", e);
            Error::Failed(Box::new(e))
        })?;

        // clone the repo 
        sbx.exec_command_v1_shell_exec_post(&ShellExecRequest {
            command: format!("git clone https://github.com/{}.git repo", _session_model.repo.unwrap()),
            async_mode: false,
            id: None,
            timeout: Some(30.0 as f64),
            exec_dir: Some(String::from("/home/gem")),
        }).await.map_err(|e| {
            error!("Failed to execute command: {}", e);
            Error::Failed(Box::new(e))
        })?;

        // checkout the target branch
        sbx.exec_command_v1_shell_exec_post(&ShellExecRequest {
            command: format!("git checkout {}", _session_model.target_branch.unwrap()),
            async_mode: false,
            id: None,
            timeout: Some(30.0 as f64),
            exec_dir: Some(String::from("/home/gem/repo")),
        }).await.map_err(|e| {
            error!("Failed to execute command: {}", e);
            Error::Failed(Box::new(e))
        })?;

        let branch = _session_model
            .branch
            .unwrap_or_else(|| format!("claude/{}", _session_model.id));
        // if branch exists, checkout the branch, else switch -c the branch
        sbx.exec_command_v1_shell_exec_post(&ShellExecRequest {
            command: format!(
                "git checkout {} || git switch -c {}",
                branch,
                branch
            ),
            async_mode: false,
            id: None,
            timeout: Some(30.0 as f64),
            exec_dir: Some(String::from("/home/gem/repo")),
        }).await.map_err(|e| {
            error!("Failed to execute command: {}", e);
            Error::Failed(Box::new(e))
        })?;

        // Fire-and-forget task to run Claude Code CLI
        let session_id = _session_model.id;
        let mcp_json_string_owned = mcp_json_string.to_string();
        let borrowed_ip_item = borrowed_ip.item.clone();
        let ip_allocator_url_clone = ip_allocator_url.clone();

        tokio::spawn(async move {
            // Run npx command in blocking thread pool
            let result = tokio::task::spawn_blocking(move || {
                // Write MCP config to temporary file
                let config_path = format!("/tmp/borrow-{}.mcp-config", session_id);
                if let Err(e) = std::fs::write(&config_path, &mcp_json_string_owned) {
                    error!("Failed to write MCP config for session {}: {}", session_id, e);
                    return Err(e);
                }

                info!("Running Claude Code CLI for session {}", session_id);

                // Execute npx command locally (not in sandbox)
                let output = std::process::Command::new("npx")
                    .args([
                        "-y",
                        "@anthropic-ai/claude-code",
                        "--append-system-prompt",
                        "you are running as a disposable task agent with a git repo checked out in a feature branch. when you completed with your task, commit and push the changes upstream",
                        "--dangerously-skip-permissions",
                        "--print",
                        "--output-format=stream-json",
                        "--session-id",
                        &session_id.to_string(),
                        "--allowedTools",
                        "WebSearch",
                        "mcp__*",
                        "ListMcpResourcesTool",
                        "ReadMcpResourceTool",
                        "--disallowedTools",
                        "Bash",
                        "Edit",
                        "Write",
                        "NotebookEdit",
                        "Read",
                        "Glob",
                        "Grep",
                        "KillShell",
                        "BashOutput",
                        "TodoWrite",
                        "-p",
                        "what are your available tools?",
                        "--verbose",
                        "--strict-mcp-config",
                        "--mcp-config",
                        &config_path,
                    ])
                    .output();

                // Cleanup temp file
                let _ = std::fs::remove_file(&config_path);

                output
            })
            .await;

            // Handle the result
            match result {
                Ok(Ok(output)) => {
                    info!("Claude Code CLI completed for session {}", session_id);

                    // Parse stream-json output
                    let stdout = String::from_utf8_lossy(&output.stdout);
                    let stderr = String::from_utf8_lossy(&output.stderr);

                    if !stderr.is_empty() {
                        info!("Claude Code stderr for session {}: {}", session_id, stderr);
                    }

                    // Log each line of stream-json output
                    for line in stdout.lines() {
                        info!("Claude Code output for session {}: {}", session_id, line);

                        // TODO: Parse JSON and extract messages
                        // TODO: Append to session.messages in database
                    }

                    // TODO: Update session.messages in database
                    // For now, just log that we would update
                    info!("Would update session {} messages in database", session_id);
                }
                Ok(Err(e)) => {
                    error!("Failed to execute Claude Code CLI for session {}: {}", session_id, e);
                }
                Err(e) => {
                    error!("Failed to spawn blocking task for session {}: {}", session_id, e);
                }
            }

            // Return borrowed IP (always, even on failure)
            info!("Returning borrowed IP for session {}", session_id);
            let ip_client = ip_allocator_client::Client::new(&ip_allocator_url_clone);
            let return_input = ip_allocator_client::types::ReturnInput {
                item: borrowed_ip_item,
            };
            if let Err(e) = ip_client.handlers_ip_return_item(&return_input).await {
                error!("Failed to return IP for session {}: {}", session_id, e);
            } else {
                info!("Successfully returned IP for session {}", session_id);
            }
        });
    }

    info!("Completed outbox job for session_id: {}", job.session_id);

    Ok(())
}
