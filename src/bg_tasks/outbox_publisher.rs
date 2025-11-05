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
    let sbx_clone = sbx.clone();

    tokio::spawn(async move {
        info!("Starting Claude Code CLI execution for session {}", session_id);

        // Check if npx is available in the sandbox
        info!("Checking if npx is available in sandbox for session {}", session_id);
        let npx_check_result = sbx_clone.exec_command_v1_shell_exec_post(&ShellExecRequest {
            command: String::from("which npx && npx --version && node --version"),
            async_mode: false,
            id: None,
            timeout: Some(10.0_f64),
            exec_dir: Some(String::from("/home/gem")),
        })
        .await;

        match npx_check_result {
            Ok(response) => {
                if let Some(data) = &response.data {
                    info!("npx availability check for session {}: output={:?}, exit_code={:?}",
                        session_id,
                        data.output,
                        data.exit_code
                    );
                } else {
                    error!("npx availability check returned no data for session {}", session_id);
                }
            }
            Err(e) => {
                error!("Failed to check npx availability for session {}: {}", session_id, e);
            }
        }

        // Write MCP config to sandbox filesystem
        let mcp_config_path = format!("/home/gem/.config/claude/mcp_config_{}.json", session_id);
        info!("Writing MCP config to sandbox at: {}", mcp_config_path);

        let write_config_result = sbx_clone.exec_command_v1_shell_exec_post(&ShellExecRequest {
            command: format!("mkdir -p /home/gem/.config/claude && cat > {} << 'EOF'\n{}\nEOF", mcp_config_path, mcp_json_string),
            async_mode: false,
            id: None,
            timeout: Some(10.0_f64),
            exec_dir: Some(String::from("/home/gem")),
        })
        .await;

        match write_config_result {
            Ok(_) => {
                info!("Successfully wrote MCP config to sandbox for session {}", session_id);
            }
            Err(e) => {
                error!("Failed to write MCP config to sandbox for session {}: {}", session_id, e);
                // Continue anyway, the command might fail but we want to see the error
            }
        }

        // Build the Claude Code CLI command
        let claude_code_command = format!(
            "npx -y @anthropic-ai/claude-code \
            --append-system-prompt 'you are running as a disposable task agent with a git repo checked out in a feature branch. when you completed with your task, commit and push the changes upstream' \
            --dangerously-skip-permissions \
            --print \
            --output-format=stream-json \
            --session-id {} \
            --allowedTools WebSearch mcp__* ListMcpResourcesTool ReadMcpResourceTool \
            --disallowedTools Bash Edit Write NotebookEdit Read Glob Grep KillShell BashOutput TodoWrite \
            -p 'what are your available tools?' \
            --verbose \
            --strict-mcp-config \
            --mcp-config {}",
            session_id,
            mcp_config_path
        );

        info!("Executing Claude Code CLI in sandbox for session {}", session_id);
        info!("Working directory: /home/gem/repo");
        info!("Command: {}", claude_code_command);

        // Execute npx command in sandbox
        let result = sbx_clone.exec_command_v1_shell_exec_post(&ShellExecRequest {
            command: claude_code_command,
            async_mode: false,
            id: None,
            timeout: Some(300.0_f64), // 5 minutes timeout
            exec_dir: Some(String::from("/home/gem/repo")),
        })
        .await;

        // Handle the result
        match result {
            Ok(response) => {
                info!("Claude Code CLI completed for session {}", session_id);
                info!("Response success: {}, message: {}", response.success, response.message);

                if let Some(data) = &response.data {
                    let exit_code = data.exit_code.unwrap_or(-1);
                    let output = data.output.as_deref().unwrap_or("");

                    info!("Exit code: {}", exit_code);
                    info!("Status: {:?}", data.status);

                    if !output.is_empty() {
                        info!("Claude Code output length: {} bytes", output.len());

                        // Extract existing messages from the nested messages.messages field
                        let mut msgs: Vec<serde_json::Value> = session_model_clone
                            .messages
                            .as_ref()
                            .and_then(|v| v.get("messages"))
                            .and_then(|v| v.as_array())
                            .cloned()
                            .unwrap_or_default();

                        // Log each line of stream-json output
                        for line in output.lines() {
                            info!("Claude Code output for session {}: {}", session_id, line);

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
                                    error!("Failed to parse JSON from Claude Code output for session {}: {}", session_id, e);
                                    error!("Line was: {}", line);
                                }
                            }
                        }
                    } else {
                        error!("Claude Code produced no output for session {}", session_id);
                    }

                    if exit_code != 0 {
                        error!("Claude Code CLI exited with non-zero code {} for session {}", exit_code, session_id);
                    }

                    // Log console records if any
                    if !data.console.is_empty() {
                        info!("Console records for session {}: {} records", session_id, data.console.len());
                        for (i, record) in data.console.iter().enumerate() {
                            info!("Console record {}: {:?}", i, record);
                        }
                    }
                } else {
                    error!("Claude Code CLI returned no data for session {}", session_id);
                }
            }
            Err(e) => {
                error!(
                    "Failed to execute Claude Code CLI for session {}: {}",
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
