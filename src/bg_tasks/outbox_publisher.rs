use apalis::prelude::*;
use sea_orm::{ActiveModelTrait, DatabaseConnection, EntityTrait, NotSet, Set};
use serde::{Deserialize, Serialize};
use std::time::Duration;
use tracing::{error, info, warn};

use sandbox_client::types::ShellExecRequest;

use crate::entities::message;
use crate::entities::prompt::{Entity as Prompt, InboxStatus};
use crate::entities::session::{Entity as Session, SessionStatus};
use crate::services::dead_letter_queue::{exists_in_dlq, insert_dlq_entry, MAX_RETRY_COUNT};

/// Job that reads from PostgreSQL outbox and publishes to Redis
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OutboxJob {
    pub prompt_id: String,
    pub payload: serde_json::Value,
}

impl Job for OutboxJob {
    const NAME: &'static str = "OutboxJob";
}

/// Context for the outbox publisher containing database connection
#[derive(Clone)]
pub struct OutboxContext {
    pub db: DatabaseConnection,
}

/// Error classification for retry logic
#[derive(Debug, Clone)]
enum ErrorType {
    /// Transient errors that should be retried (network issues, timeouts, etc.)
    Transient,
    /// Permanent errors that should not be retried (validation errors, not found, etc.)
    Permanent,
}

/// Classify an error to determine if it should be retried
fn classify_error(error: &str) -> ErrorType {
    let error_lower = error.to_lowercase();
    
    // Transient errors - should retry
    if error_lower.contains("timeout")
        || error_lower.contains("connection refused")
        || error_lower.contains("connection reset")
        || error_lower.contains("network")
        || error_lower.contains("temporarily unavailable")
        || error_lower.contains("503")
        || error_lower.contains("502")
        || error_lower.contains("504")
        || error_lower.contains("ECONNRESET")
        || error_lower.contains("ETIMEDOUT")
    {
        return ErrorType::Transient;
    }
    
    // Permanent errors - don't retry
    if error_lower.contains("not found")
        || error_lower.contains("404")
        || error_lower.contains("401")
        || error_lower.contains("403")
        || error_lower.contains("invalid")
        || error_lower.contains("parse")
        || error_lower.contains("missing")
    {
        return ErrorType::Permanent;
    }
    
    // Default to transient for unknown errors (better to retry than fail permanently)
    ErrorType::Transient
}

/// Execute a sandbox command with retry logic and timeout
async fn exec_sandbox_command_with_retry(
    sbx: &sandbox_client::Client,
    request: &ShellExecRequest,
    max_retries: u32,
) -> Result<sandbox_client::types::ShellExecResponse, Box<dyn std::error::Error + Send + Sync>> {
    let mut last_error = None;
    
    for attempt in 0..=max_retries {
        if attempt > 0 {
            let backoff_ms = 1000 * 2_u64.pow(attempt - 1).min(16); // Exponential backoff: 1s, 2s, 4s, 8s, 16s max
            warn!(
                "Retrying sandbox command (attempt {}/{}) after {}ms backoff",
                attempt + 1,
                max_retries + 1,
                backoff_ms
            );
            tokio::time::sleep(Duration::from_millis(backoff_ms)).await;
        }
        
        match sbx.exec_command_v1_shell_exec_post(request).await {
            Ok(response) => {
                info!("Sandbox command succeeded on attempt {}", attempt + 1);
                return Ok(response);
            }
            Err(e) => {
                let error_str = e.to_string();
                warn!("Sandbox command failed on attempt {}: {}", attempt + 1, error_str);
                
                // Check if error is permanent
                if let ErrorType::Permanent = classify_error(&error_str) {
                    error!("Permanent error detected, stopping retries: {}", error_str);
                    return Err(Box::new(e));
                }
                
                last_error = Some(e);
            }
        }
    }
    
    Err(Box::new(
        last_error.unwrap_or_else(|| sandbox_client::Error::UnexpectedResponse("Max retries exceeded".to_string())),
    ))
}

/// Update prompt inbox status
async fn update_prompt_status(
    db: &DatabaseConnection,
    prompt_id: uuid::Uuid,
    status: InboxStatus,
) -> Result<(), sea_orm::DbErr> {
    let prompt = Prompt::find_by_id(prompt_id)
        .one(db)
        .await?
        .ok_or_else(|| sea_orm::DbErr::RecordNotFound(format!("Prompt {} not found", prompt_id)))?;
    
    let mut active_prompt: crate::entities::prompt::ActiveModel = prompt.into();
    active_prompt.inbox_status = Set(status);
    active_prompt.update(db).await?;
    
    Ok(())
}

/// Main fault-tolerant wrapper for process_outbox_job
/// This wraps the actual processing logic with retry and DLQ handling
pub async fn process_outbox_job(job: OutboxJob, ctx: Data<OutboxContext>) -> Result<(), Error> {
    let prompt_id = match uuid::Uuid::parse_str(&job.prompt_id) {
        Ok(id) => id,
        Err(e) => {
            error!("Invalid prompt ID format: {}", e);
            return Err(Error::Failed(Box::new(e)));
        }
    };
    
    // Check if this prompt is already in the DLQ
    match exists_in_dlq(&ctx.db, "outbox_job", prompt_id).await {
        Ok(true) => {
            warn!("Prompt {} already in DLQ, skipping processing", prompt_id);
            return Ok(());
        }
        Ok(false) => {
            // Continue processing
        }
        Err(e) => {
            error!("Failed to check DLQ status for prompt {}: {}", prompt_id, e);
            // Continue anyway - better to potentially duplicate than skip
        }
    }
    
    // Try to process the job
    let result = process_outbox_job_internal(job.clone(), ctx.clone()).await;
    
    match result {
        Ok(_) => {
            info!("Successfully processed outbox job for prompt {}", prompt_id);
            
            // Update prompt status to completed on success
            if let Err(e) = update_prompt_status(&ctx.db, prompt_id, InboxStatus::Completed).await {
                error!("Failed to update prompt {} status to completed: {}", prompt_id, e);
                // Don't fail the job if status update fails
            }
            
            Ok(())
        }
        Err(e) => {
            let error_str = e.to_string();
            error!("Error processing outbox job for prompt {}: {}", prompt_id, error_str);
            
            // Classify the error
            match classify_error(&error_str) {
                ErrorType::Transient => {
                    warn!("Transient error detected for prompt {}, will retry", prompt_id);
                    // Return error to trigger Apalis retry
                    Err(e)
                }
                ErrorType::Permanent => {
                    error!("Permanent error detected for prompt {}, moving to DLQ", prompt_id);
                    
                    // Try to get prompt data for DLQ
                    let entity_data = match Prompt::find_by_id(prompt_id).one(&ctx.db).await {
                        Ok(Some(prompt)) => Some(serde_json::json!({
                            "prompt_id": prompt_id.to_string(),
                            "session_id": prompt.session_id.to_string(),
                            "data": prompt.data,
                            "inbox_status": format!("{:?}", prompt.inbox_status),
                        })),
                        _ => None,
                    };
                    
                    // Insert into DLQ
                    let now = chrono::Utc::now().into();
                    if let Err(dlq_err) = insert_dlq_entry(
                        &ctx.db,
                        "outbox_job",
                        prompt_id,
                        entity_data,
                        0, // Will be incremented by Apalis
                        &error_str,
                        now,
                    )
                    .await
                    {
                        error!("Failed to insert prompt {} into DLQ: {}", prompt_id, dlq_err);
                    } else {
                        info!("Moved prompt {} to DLQ after permanent error", prompt_id);
                    }
                    
                    // Update prompt status to archived
                    if let Err(status_err) = update_prompt_status(&ctx.db, prompt_id, InboxStatus::Archived).await {
                        error!("Failed to archive prompt {} after DLQ insertion: {}", prompt_id, status_err);
                    }
                    
                    // Return Ok to prevent further retries since we've moved to DLQ
                    Ok(())
                }
            }
        }
    }
}

/// Internal implementation of outbox job processing with detailed error handling
async fn process_outbox_job_internal(job: OutboxJob, ctx: Data<OutboxContext>) -> Result<(), Error> {
    info!("Processing outbox job for prompt_id: {}", job.prompt_id);

    // Parse prompt ID from job
    let prompt_id = uuid::Uuid::parse_str(&job.prompt_id).map_err(|e| {
        error!("Invalid prompt ID format: {}", e);
        Error::Failed(Box::new(e))
    })?;

    // Query the specific prompt
    let prompt_model = Prompt::find_by_id(prompt_id)
        .one(&ctx.db)
        .await
        .map_err(|e| {
            error!("Failed to query prompt {}: {}", prompt_id, e);
            Error::Failed(Box::new(e))
        })?
        .ok_or_else(|| {
            error!("Prompt {} not found", prompt_id);
            Error::Failed("Prompt not found".into())
        })?;

    // Query the related session
    let session_id = prompt_model.session_id;
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

    info!("Processing prompt {} for session {}", prompt_id, session_id);

    // Extract prompt content from the data field
    let prompt_content = match &prompt_model.data {
        serde_json::Value::String(s) => s.clone(),
        serde_json::Value::Object(obj) => {
            // Try to extract from common field names: "content", "prompt", "text", "message"
            obj.get("content")
                .or_else(|| obj.get("prompt"))
                .or_else(|| obj.get("text"))
                .or_else(|| obj.get("message"))
                .and_then(|v| v.as_str())
                .map(|s| s.to_string())
                .unwrap_or_else(|| {
                    // If no common field found, serialize the entire object as a string
                    serde_json::to_string(&prompt_model.data).unwrap_or_default()
                })
        }
        _ => serde_json::to_string(&prompt_model.data).unwrap_or_default(),
    };

    info!(
        "Extracted prompt content (first 100 chars): {}",
        prompt_content.chars().take(100).collect::<String>()
    );

    // Read borrowed IP from session's sbx_config (already allocated by prompt_poller)
    let borrowed_ip_json = _session_model.sbx_config.as_ref().ok_or_else(|| {
        error!(
            "Session {} has no sbx_config - IP should have been borrowed during enqueue",
            session_id
        );
        Error::Failed("Session missing sbx_config".into())
    })?;

    info!("Using pre-allocated sandbox from session sbx_config");

    // Parse the sbx_config JSON to extract mcp_json_string and api_url
    // Note: The data is nested under "item" key from prompt_poller
    let item = borrowed_ip_json["item"]
        .as_object()
        .ok_or_else(|| Error::Failed("Missing item object in sbx_config".into()))?;

    let mcp_json_string = item["mcp_json_string"]
        .as_str()
        .ok_or_else(|| Error::Failed("Missing mcp_json_string in sbx_config.item".into()))?
        .to_string();

    info!("Sandbox mcp_json_string: {}", mcp_json_string);

    let api_url = item["api_url"]
        .as_str()
        .ok_or_else(|| Error::Failed("Missing api_url in sbx_config.item".into()))?;

    info!("Sandbox api_url: {}", api_url);

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

    // Pass the token to gh auth login via stdin - with retry
    let auth_command = format!("echo '{}' | gh auth login --with-token", github_token);
    exec_sandbox_command_with_retry(
        &sbx,
        &ShellExecRequest {
            command: auth_command,
            async_mode: false,
            id: None,
            timeout: Some(30.0_f64),
            exec_dir: Some(String::from("/home/gem")),
        },
        3, // Max 3 retries
    )
    .await
    .map_err(|e| {
        error!("Failed to authenticate with GitHub after retries: {}", e);
        Error::Failed(e)
    })?;
    
    // Setup git authentication - with retry
    exec_sandbox_command_with_retry(
        &sbx,
        &ShellExecRequest {
            command: "gh auth setup-git".to_string(),
            async_mode: false,
            id: None,
            timeout: Some(30.0_f64),
            exec_dir: Some(String::from("/home/gem")),
        },
        3, // Max 3 retries
    )
    .await
    .map_err(|e| {
        error!("Failed to setup git authentication after retries: {}", e);
        Error::Failed(e)
    })?;
    
    // Clone the repo using session_id as directory name - with retry
    let repo_dir = format!("repo_{}", session_id);
    exec_sandbox_command_with_retry(
        &sbx,
        &ShellExecRequest {
            command: format!(
                "git clone https://github.com/{}.git {}",
                _session_model.repo.clone().unwrap(),
                repo_dir
            ),
            async_mode: false,
            id: None,
            timeout: Some(60.0_f64), // Longer timeout for clone
            exec_dir: Some(String::from("/home/gem")),
        },
        3, // Max 3 retries
    )
    .await
    .map_err(|e| {
        error!("Failed to clone repository after retries: {}", e);
        Error::Failed(e)
    })?;

    // Checkout the target branch - with retry
    let repo_path = format!("/home/gem/{}", repo_dir);
    exec_sandbox_command_with_retry(
        &sbx,
        &ShellExecRequest {
            command: format!(
                "git checkout {}",
                _session_model.target_branch.clone().unwrap()
            ),
            async_mode: false,
            id: None,
            timeout: Some(30.0_f64),
            exec_dir: Some(repo_path.clone()),
        },
        3, // Max 3 retries
    )
    .await
    .map_err(|e| {
        error!("Failed to checkout target branch after retries: {}", e);
        Error::Failed(e)
    })?;

    let branch = _session_model
        .branch
        .clone()
        .unwrap_or_else(|| format!("claude/{}", _session_model.id));
    
    // If branch exists, checkout the branch, else switch -c the branch - with retry
    exec_sandbox_command_with_retry(
        &sbx,
        &ShellExecRequest {
            command: format!("git checkout {} || git switch -c {}", branch, branch),
            async_mode: false,
            id: None,
            timeout: Some(30.0_f64),
            exec_dir: Some(repo_path.clone()),
        },
        3, // Max 3 retries
    )
    .await
    .map_err(|e| {
        error!("Failed to checkout/create working branch after retries: {}", e);
        Error::Failed(e)
    })?;

    // Fire-and-forget task to run Claude Code CLI
    let session_id = _session_model.id;
    let prompt_id_clone = prompt_id;
    let db_clone = ctx.db.clone();
    let db_clone_for_return = ctx.db.clone();
    let prompt_content_clone = prompt_content.clone();
    let repo_clone = _session_model.repo.clone();
    let branch_clone = branch.clone();
    let repo_path_clone = repo_path.clone();

    tokio::spawn(async move {
        info!("Running Claude Code CLI for session {}", session_id);

        // Create a temporary directory for this session using tempfile
        // Use environment variable TMPDIR if set, otherwise use user's home directory
        let temp_base_dir = std::env::var("TMPDIR")
            .or_else(|_| std::env::var("TEMP_DIR"))
            .unwrap_or_else(|_| {
                // Fall back to user's home directory
                std::env::var("HOME")
                    .map(|home| format!("{}/.tmp", home))
                    .unwrap_or_else(|_| ".".to_string())
            });

        info!("Using temp base directory: {}", temp_base_dir);

        // Ensure the base directory exists
        if let Err(e) = std::fs::create_dir_all(&temp_base_dir) {
            error!(
                "Failed to create base temp directory {}: {}",
                temp_base_dir, e
            );
            return;
        }

        let temp_dir = match tempfile::Builder::new()
            .prefix(&format!("claude_session_{}_", session_id))
            .tempdir_in(&temp_base_dir)
        {
            Ok(dir) => dir,
            Err(e) => {
                error!(
                    "Failed to create temp directory for session {} in {}: {}",
                    session_id, temp_base_dir, e
                );
                return;
            }
        };

        // Write MCP config to a file
        let mcp_config_path = temp_dir.path().join("mcp_config.json");
        if let Err(e) = std::fs::write(&mcp_config_path, &mcp_json_string) {
            error!(
                "Failed to write MCP config for session {}: {}",
                session_id, e
            );
            return;
        }

        // Clone prompt_content for use in spawn_blocking
        let prompt_for_cli = prompt_content_clone.clone();

        // Load system prompt template from embedded markdown file
        const SYSTEM_PROMPT_TEMPLATE: &str =
            include_str!("../../prompts/outbox_handler_system_prompt.md");

        // Construct system prompt with context about the task by replacing placeholders
        let system_prompt = SYSTEM_PROMPT_TEMPLATE
            .replace("{REPO_PATH}", &repo_path_clone)
            .replace(
                "{REPO}",
                &repo_clone
                    .clone()
                    .unwrap_or_else(|| "unknown/repo".to_string()),
            )
            .replace("{BRANCH}", &branch_clone);

        // Spawn the Claude CLI process with piped stdout/stderr for streaming
        let _ = tokio::task::spawn_blocking(move || {
            use std::io::{BufRead, BufReader};
            use std::process::{Command, Stdio};

            let child = Command::new("claude")
                .args([
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
                    "--append-system-prompt",
                    &system_prompt,
                    "-p",
                    &prompt_for_cli,
                    "--verbose",
                    "--strict-mcp-config",
                    "--mcp-config",
                    mcp_config_path.to_str().unwrap(),
                ])
                .current_dir(temp_dir.path())
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .spawn();

            let mut child = match child {
                Ok(c) => c,
                Err(e) => {
                    error!("Failed to spawn Claude CLI for session {}: {}", session_id, e);
                    return Err(e);
                }
            };

            // Take stdout and stderr handles
            let stdout = child.stdout.take().expect("Failed to capture stdout");
            let stderr = child.stderr.take().expect("Failed to capture stderr");

            // Spawn a thread to handle stderr
            let session_id_clone = session_id;
            std::thread::spawn(move || {
                let stderr_reader = BufReader::new(stderr);
                for (i, line) in stderr_reader.lines().enumerate() {
                    match line {
                        Ok(line) => {
                            error!("Claude Code stderr[{}] for session {}: {}", i, session_id_clone, line);
                        }
                        Err(e) => {
                            error!("Error reading stderr for session {}: {}", session_id_clone, e);
                            break;
                        }
                    }
                }
            });

            // Read stdout line by line and send to channel
            let stdout_reader = BufReader::new(stdout);
            let mut line_count = 0;

            for line in stdout_reader.lines() {
                match line {
                    Ok(line) => {
                        line_count += 1;

                        // Skip empty lines
                        if line.trim().is_empty() {
                            continue;
                        }

                        info!("Claude Code output line {} for session {}: {}", line_count, session_id, line);

                        // Parse JSON and insert into database
                        match serde_json::from_str::<serde_json::Value>(&line) {
                            Ok(json) => {
                                let message_id = uuid::Uuid::new_v4();
                                let new_message = message::ActiveModel {
                                    id: Set(message_id),
                                    prompt_id: Set(prompt_id_clone),
                                    data: Set(json),
                                    created_at: NotSet,
                                    updated_at: NotSet,
                                };

                                // Use tokio runtime handle to insert from blocking context
                                let handle = tokio::runtime::Handle::current();
                                let db_clone2 = db_clone.clone();
                                match handle.block_on(async move {
                                    new_message.insert(&db_clone2).await
                                }) {
                                    Ok(_) => {
                                        info!("Created message {} for prompt {} in session {}", message_id, prompt_id_clone, session_id);
                                    }
                                    Err(e) => {
                                        error!("Failed to create message {} for prompt {} in session {}: {}", message_id, prompt_id_clone, session_id, e);
                                    }
                                }
                            }
                            Err(e) => {
                                error!("Failed to parse JSON at line {} for session {}: {}. Line content: {}", line_count, session_id, e, line);
                            }
                        }
                    }
                    Err(e) => {
                        error!("Error reading stdout for session {}: {}", session_id, e);
                        break;
                    }
                }
            }

            info!("Processed {} lines of output for session {}", line_count, session_id);

            // Wait for process to complete and get exit status
            let status = child.wait()?;
            info!("Claude Code CLI exit status for session {}: {:?}", session_id, status);

            Ok(status)
        })
        .await;

        // Update session status to ReturningIp (poller will handle IP return)
        info!("Updating session {} status to ReturningIp", session_id);

        let session_result = Session::find_by_id(session_id)
            .one(&db_clone_for_return)
            .await;
        match session_result {
            Ok(Some(session_model)) => {
                let mut active_session: crate::entities::session::ActiveModel =
                    session_model.into();
                active_session.session_status = Set(SessionStatus::ReturningIp);
                active_session.status_message = Set(Some("Returning IP".to_string()));

                if let Err(e) = active_session.update(&db_clone_for_return).await {
                    error!(
                        "Failed to update session {} status to ReturningIp: {}",
                        session_id, e
                    );
                } else {
                    info!(
                        "Updated session {} status to ReturningIp - poller will handle IP return",
                        session_id
                    );
                }
            }
            Ok(None) => {
                error!(
                    "Session {} not found when trying to update status",
                    session_id
                );
            }
            Err(e) => {
                error!(
                    "Failed to query session {} for status update: {}",
                    session_id, e
                );
            }
        }
    });

    info!("Completed outbox job for prompt_id: {}", job.prompt_id);

    Ok(())
}
