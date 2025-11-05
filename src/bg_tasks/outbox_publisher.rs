use apalis::prelude::*;
use sea_orm::{ActiveModelTrait, DatabaseConnection, EntityTrait, Set};
use serde::{Deserialize, Serialize};
use tracing::{error, info};

use sandbox_client::types::ShellExecRequest;

use crate::entities::session::{self, Entity as Session};

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

/// Process an outbox job: read session by ID, verify it's ACTIVE, set up sandbox, and run Claude Code
pub async fn process_outbox_job(job: OutboxJob, ctx: Data<OutboxContext>) -> Result<(), Error> {
    info!("Processing outbox job for session_id: {}", job.session_id);

    // Parse session ID from job
    let session_id = uuid::Uuid::parse_str(&job.session_id).map_err(|e| {
        error!("Invalid session ID format: {}", e);
        Error::Failed(Box::new(e))
    })?;

    // Query the specific session
    let _session_model = Session::find_by_id(session_id)
        .one(&ctx.db)
        .await
        .map_err(|e| {
            error!("Failed to query session {}: {}", session_id, e);
            Error::Failed(Box::new(e))
        })?
        .ok_or_else(|| {
            error!("Session {} not found", session_id);
            Error::Failed("Session not found".into())
        })?;

    info!("Processing active session {}", session_id);

    // get sbx config from ip-allocator
    // Read IP_ALLOCATOR_URL from environment, e.g., "http://localhost:8000"
    let ip_allocator_url =
        std::env::var("IP_ALLOCATOR_URL").unwrap_or_else(|_| "http://localhost:8000".to_string());

    let ip_client = ip_allocator_client::Client::new(&ip_allocator_url);
    let borrowed_ip = ip_client.handlers_ip_borrow(None).await.map_err(|e| {
        error!("Failed to borrow IP: {}", e);
        Error::Failed(Box::new(e))
    })?;

    // Parse the response JSON to extract mcp_url and api_url
    let mcp_json_string = borrowed_ip.item["mcp_json_string"]
        .as_str()
        .ok_or_else(|| Error::Failed("Missing mcp_json_string in response".into()))?
        .to_string();

    info!("Borrowed sandbox - mcp_json_string: {}", mcp_json_string);

    let api_url = borrowed_ip.item["api_url"]
        .as_str()
        .ok_or_else(|| Error::Failed("Missing api_url in response".into()))?;

    info!("Borrowed sandbox - api_url: {}", api_url);

    // Create sandbox client using the api_url
    let sbx = sandbox_client::Client::new(api_url);

    // Read GitHub token from environment variable
    info!("Reading GitHub token from environment variable");

    let github_token = std::env::var("GITHUB_TOKEN").map_err(|e| {
        error!("Failed to read GITHUB_TOKEN from environment: {}", e);
        Error::Failed(Box::new(std::io::Error::new(
            std::io::ErrorKind::NotFound,
            "GITHUB_TOKEN environment variable not set",
        )))
    })?;

    info!("Successfully read GitHub token from environment");

    // Authenticate with GitHub using the fetched token
    info!(
        "Authenticating with GitHub for session {}",
        _session_model.id
    );

    // Pass the token to gh auth login via stdin
    let auth_command = format!("echo '{}' | gh auth login --with-token", github_token);
    sbx.exec_command_v1_shell_exec_post(&ShellExecRequest {
        command: auth_command,
        async_mode: false,
        id: None,
        timeout: Some(30.0_f64),
        exec_dir: Some(String::from("/home/gem")),
    })
    .await
    .map_err(|e| {
        error!("Failed to authenticate with GitHub: {}", e);
        Error::Failed(Box::new(e))
    })?;

    // clone the repo
    sbx.exec_command_v1_shell_exec_post(&ShellExecRequest {
        command: format!(
            "git clone https://github.com/{}.git repo",
            _session_model.repo.clone().unwrap()
        ),
        async_mode: false,
        id: None,
        timeout: Some(30.0_f64),
        exec_dir: Some(String::from("/home/gem")),
    })
    .await
    .map_err(|e| {
        error!("Failed to execute command: {}", e);
        Error::Failed(Box::new(e))
    })?;

    // checkout the target branch
    sbx.exec_command_v1_shell_exec_post(&ShellExecRequest {
        command: format!(
            "git checkout {}",
            _session_model.target_branch.clone().unwrap()
        ),
        async_mode: false,
        id: None,
        timeout: Some(30.0_f64),
        exec_dir: Some(String::from("/home/gem/repo")),
    })
    .await
    .map_err(|e| {
        error!("Failed to execute command: {}", e);
        Error::Failed(Box::new(e))
    })?;

    let branch = _session_model
        .branch
        .clone()
        .unwrap_or_else(|| format!("claude/{}", _session_model.id));
    // if branch exists, checkout the branch, else switch -c the branch
    sbx.exec_command_v1_shell_exec_post(&ShellExecRequest {
        command: format!("git checkout {} || git switch -c {}", branch, branch),
        async_mode: false,
        id: None,
        timeout: Some(30.0_f64),
        exec_dir: Some(String::from("/home/gem/repo")),
    })
    .await
    .map_err(|e| {
        error!("Failed to execute command: {}", e);
        Error::Failed(Box::new(e))
    })?;

    // Fire-and-forget task to run Claude Code CLI
    let session_id = _session_model.id;
    let borrowed_ip_item = borrowed_ip.item.clone();
    let ip_allocator_url_clone = ip_allocator_url.clone();
    let session_model_clone = _session_model.clone();
    let db_clone = ctx.db.clone();

    tokio::spawn(async move {
        // Run npx command in blocking thread pool
        let result = tokio::task::spawn_blocking(move || {
            info!("Running Claude Code CLI for session {}", session_id);

            // Create a temporary directory for this session using tempfile
            let temp_dir = tempfile::tempdir().map_err(|e| {
                error!("Failed to create temp directory for session {}: {}", session_id, e);
                std::io::Error::other(format!("Failed to create temp directory: {}", e))
            })?;

            // Write MCP config to a file
            let mcp_config_path = temp_dir.path().join("mcp_config.json");
            if let Err(e) = std::fs::write(&mcp_config_path, &mcp_json_string) {
                error!("Failed to write MCP config for session {}: {}", session_id, e);
                return Err(e);
            }

            // Execute npx command locally (not in sandbox)
            let output = std::process::Command::new("claude")
                .args([
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
                    mcp_config_path.to_str().unwrap(),
                ])
                .current_dir(temp_dir.path())
                .output();

            // Temp directory will be automatically cleaned up when temp_dir is dropped

            output
        })
        .await;

        // Handle the result
        match result {
            Ok(Ok(output)) => {
                info!("Claude Code CLI completed for session {}", session_id);
                info!("Claude Code CLI exit status: {:?}", output.status);

                // Parse stream-json output
                let stdout = String::from_utf8_lossy(&output.stdout);
                let stderr = String::from_utf8_lossy(&output.stderr);

                // Log stderr line by line to avoid truncation
                if !stderr.is_empty() {
                    error!("Claude Code stderr for session {} (start)", session_id);
                    for (i, line) in stderr.lines().enumerate() {
                        error!(
                            "Claude Code stderr[{}] for session {}: {}",
                            i, session_id, line
                        );
                    }
                    error!("Claude Code stderr for session {} (end)", session_id);
                }

                // Check if the command failed
                if !output.status.success() {
                    error!(
                        "Claude Code CLI failed with exit status: {:?} for session {}",
                        output.status, session_id
                    );

                    // Log first 20 lines of stdout for debugging
                    info!(
                        "Claude Code stdout (first 20 lines) for session {}:",
                        session_id
                    );
                    for (i, line) in stdout.lines().take(20).enumerate() {
                        info!("stdout[{}]: {}", i, line);
                    }

                    // Don't process the output further if command failed
                    return;
                }

                // Extract existing messages from the nested messages.messages field
                let mut msgs: Vec<serde_json::Value> = session_model_clone
                    .messages
                    .as_ref()
                    .and_then(|v| v.get("messages"))
                    .and_then(|v| v.as_array())
                    .cloned()
                    .unwrap_or_default();

                // Log each line of stream-json output
                let mut line_count = 0;
                for line in stdout.lines() {
                    line_count += 1;

                    // Skip empty lines
                    if line.trim().is_empty() {
                        continue;
                    }

                    info!(
                        "Claude Code output line {} for session {}: {}",
                        line_count, session_id, line
                    );

                    // Parse JSON with error handling
                    match serde_json::from_str::<serde_json::Value>(line) {
                        Ok(json) => {
                            msgs.push(json);

                            // Update session messages in database, wrapping in messages.messages structure
                            let mut active_session: session::ActiveModel =
                                session_model_clone.clone().into();
                            let messages_wrapper = serde_json::json!({
                                "messages": msgs.clone()
                            });
                            active_session.messages = Set(Some(messages_wrapper));

                            if let Err(e) = active_session.update(&db_clone).await {
                                error!(
                                    "Failed to update session {} messages in database: {}",
                                    session_id, e
                                );
                            }
                        }
                        Err(e) => {
                            error!(
                                "Failed to parse JSON at line {} for session {}: {}. Line content: {}",
                                line_count, session_id, e, line
                            );
                        }
                    }
                }

                info!(
                    "Processed {} lines of output for session {}",
                    line_count, session_id
                );
            }
            Ok(Err(e)) => {
                error!(
                    "Failed to execute Claude Code CLI for session {}: {}",
                    session_id, e
                );
            }
            Err(e) => {
                error!(
                    "Failed to spawn blocking task for session {}: {}",
                    session_id, e
                );
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

    info!("Completed outbox job for session_id: {}", job.session_id);

    Ok(())
}
