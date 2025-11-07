use apalis::prelude::*;
use sea_orm::{ActiveModelTrait, DatabaseConnection, EntityTrait, NotSet, Set};
use serde::{Deserialize, Serialize};
use tracing::{error, info, warn};

use sandbox_client::types::ShellExecRequest;

use crate::entities::message;
use crate::entities::prompt::{Entity as Prompt, InboxStatus};
use crate::entities::session::{Entity as Session, SessionStatus};

/// Maximum number of retry attempts for a prompt
const MAX_RETRY_ATTEMPTS: i32 = 3;

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

/// Error types for better error handling
#[derive(Debug)]
enum ProcessingError {
    Transient(String),   // Retryable errors (network, timeout, etc.)
    Permanent(String),   // Non-retryable errors (invalid data, etc.)
    DatabaseError(String), // Database-related errors
}

impl std::fmt::Display for ProcessingError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ProcessingError::Transient(msg) => write!(f, "Transient error: {}", msg),
            ProcessingError::Permanent(msg) => write!(f, "Permanent error: {}", msg),
            ProcessingError::DatabaseError(msg) => write!(f, "Database error: {}", msg),
        }
    }
}

impl std::error::Error for ProcessingError {}

/// Process an outbox job: read prompt by ID, get related session, set up sandbox, and run Claude Code
/// This implementation is idempotent and fault-tolerant
pub async fn process_outbox_job(job: OutboxJob, ctx: Data<OutboxContext>) -> Result<(), Error> {
    info!("Processing outbox job for prompt_id: {}", job.prompt_id);

    // Parse prompt ID from job
    let prompt_id = uuid::Uuid::parse_str(&job.prompt_id).map_err(|e| {
        error!("Invalid prompt ID format: {}", e);
        Error::Failed(Box::new(e))
    })?;

    // Try to acquire lock and process the prompt
    match try_process_prompt(prompt_id, &ctx.db).await {
        Ok(_) => {
            info!("Successfully processed prompt {}", prompt_id);
            Ok(())
        }
        Err(e) => {
            error!("Failed to process prompt {}: {}", prompt_id, e);
            // Apalis will handle retries based on the error
            Err(Error::Failed(Box::new(std::io::Error::new(
                std::io::ErrorKind::Other,
                e.to_string(),
            ))))
        }
    }
}

/// Try to acquire processing lock and process the prompt
async fn try_process_prompt(
    prompt_id: uuid::Uuid,
    db: &DatabaseConnection,
) -> Result<(), ProcessingError> {
    // Step 1: Try to acquire the prompt with idempotency check
    let prompt_model = acquire_prompt_for_processing(prompt_id, db).await?;

    // Check if already completed (idempotency)
    if prompt_model.inbox_status == InboxStatus::Completed {
        info!("Prompt {} already completed, skipping", prompt_id);
        return Ok(());
    }

    // Check if max retries exceeded
    if prompt_model.processing_attempts >= MAX_RETRY_ATTEMPTS {
        warn!(
            "Prompt {} exceeded max retry attempts ({}), marking as failed",
            prompt_id, MAX_RETRY_ATTEMPTS
        );
        mark_prompt_as_failed(
            prompt_id,
            db,
            &format!("Exceeded maximum retry attempts ({})", MAX_RETRY_ATTEMPTS),
        )
        .await?;
        return Err(ProcessingError::Permanent(
            "Max retry attempts exceeded".to_string(),
        ));
    }

    // Step 2: Increment processing attempts and update timestamp
    increment_processing_attempt(prompt_id, db).await?;

    // Step 3: Get session information
    let session_id = prompt_model.session_id;
    let session_model = Session::find_by_id(session_id)
        .one(db)
        .await
        .map_err(|e| {
            ProcessingError::DatabaseError(format!("Failed to query session {}: {}", session_id, e))
        })?
        .ok_or_else(|| {
            ProcessingError::Permanent(format!("Session {} not found", session_id))
        })?;

    info!("Processing prompt {} for session {}", prompt_id, session_id);

    // Step 4: Extract prompt content
    let prompt_content = extract_prompt_content(&prompt_model.data);
    info!(
        "Extracted prompt content (first 100 chars): {}",
        prompt_content.chars().take(100).collect::<String>()
    );

    // Step 5: Get sandbox configuration
    let (mcp_json_string, api_url) = get_sandbox_config(&session_model, session_id)?;

    // Step 6: Create sandbox client
    let sbx = sandbox_client::Client::new(&api_url);

    // Step 7: Setup git authentication (with retry)
    setup_git_auth(&sbx, session_id).await?;

    // Step 8: Clone repository (idempotent - skip if already exists)
    let repo_dir = format!("repo_{}", session_id);
    let repo_path = format!("/home/gem/{}", repo_dir);
    
    clone_repository_idempotent(
        &sbx,
        &session_model.repo.clone().unwrap_or_default(),
        &repo_dir,
        &repo_path,
    )
    .await?;

    // Step 9: Checkout branches (idempotent)
    checkout_branches_idempotent(
        &sbx,
        &repo_path,
        &session_model.target_branch.clone().unwrap_or_else(|| "main".to_string()),
        &session_model
            .branch
            .clone()
            .unwrap_or_else(|| format!("claude/{}", session_id)),
    )
    .await?;

    // Step 10: Run Claude Code CLI in background with cleanup guarantee
    spawn_claude_cli_with_cleanup(
        prompt_id,
        session_id,
        db.clone(),
        mcp_json_string,
        prompt_content,
        repo_path.clone(),
        session_model.repo.clone(),
        session_model
            .branch
            .clone()
            .unwrap_or_else(|| format!("claude/{}", session_id)),
    );

    info!("Successfully enqueued Claude CLI execution for prompt {}", prompt_id);
    Ok(())
}

/// Acquire the prompt for processing with optimistic locking
async fn acquire_prompt_for_processing(
    prompt_id: uuid::Uuid,
    db: &DatabaseConnection,
) -> Result<crate::entities::prompt::Model, ProcessingError> {
    let prompt_model = Prompt::find_by_id(prompt_id)
        .one(db)
        .await
        .map_err(|e| {
            ProcessingError::DatabaseError(format!("Failed to query prompt {}: {}", prompt_id, e))
        })?
        .ok_or_else(|| ProcessingError::Permanent(format!("Prompt {} not found", prompt_id)))?;

    Ok(prompt_model)
}

/// Increment processing attempt counter
async fn increment_processing_attempt(
    prompt_id: uuid::Uuid,
    db: &DatabaseConnection,
) -> Result<(), ProcessingError> {
    let prompt = Prompt::find_by_id(prompt_id)
        .one(db)
        .await
        .map_err(|e| ProcessingError::DatabaseError(e.to_string()))?
        .ok_or_else(|| ProcessingError::Permanent("Prompt not found".to_string()))?;

    let mut active_prompt: crate::entities::prompt::ActiveModel = prompt.into();
    active_prompt.processing_attempts = Set(active_prompt.processing_attempts.unwrap() + 1);
    active_prompt.last_attempt_at = Set(Some(chrono::Utc::now().into()));

    active_prompt
        .update(db)
        .await
        .map_err(|e| ProcessingError::DatabaseError(e.to_string()))?;

    Ok(())
}

/// Mark prompt as failed
async fn mark_prompt_as_failed(
    prompt_id: uuid::Uuid,
    db: &DatabaseConnection,
    error_msg: &str,
) -> Result<(), ProcessingError> {
    let prompt = Prompt::find_by_id(prompt_id)
        .one(db)
        .await
        .map_err(|e| ProcessingError::DatabaseError(e.to_string()))?
        .ok_or_else(|| ProcessingError::Permanent("Prompt not found".to_string()))?;

    let mut active_prompt: crate::entities::prompt::ActiveModel = prompt.into();
    active_prompt.inbox_status = Set(InboxStatus::Failed);
    active_prompt.last_error = Set(Some(error_msg.to_string()));

    active_prompt
        .update(db)
        .await
        .map_err(|e| ProcessingError::DatabaseError(e.to_string()))?;

    Ok(())
}

/// Extract prompt content from the data field
fn extract_prompt_content(data: &serde_json::Value) -> String {
    match data {
        serde_json::Value::String(s) => s.clone(),
        serde_json::Value::Object(obj) => {
            obj.get("content")
                .or_else(|| obj.get("prompt"))
                .or_else(|| obj.get("text"))
                .or_else(|| obj.get("message"))
                .and_then(|v| v.as_str())
                .map(|s| s.to_string())
                .unwrap_or_else(|| serde_json::to_string(data).unwrap_or_default())
        }
        _ => serde_json::to_string(data).unwrap_or_default(),
    }
}

/// Get sandbox configuration from session
fn get_sandbox_config(
    session_model: &crate::entities::session::Model,
    session_id: uuid::Uuid,
) -> Result<(String, String), ProcessingError> {
    let borrowed_ip_json = session_model.sbx_config.as_ref().ok_or_else(|| {
        ProcessingError::Permanent(format!(
            "Session {} has no sbx_config - IP should have been borrowed during enqueue",
            session_id
        ))
    })?;

    let mcp_json_string = borrowed_ip_json["mcp_json_string"]
        .as_str()
        .ok_or_else(|| {
            ProcessingError::Permanent("Missing mcp_json_string in sbx_config".to_string())
        })?
        .to_string();

    let api_url = borrowed_ip_json["api_url"]
        .as_str()
        .ok_or_else(|| ProcessingError::Permanent("Missing api_url in sbx_config".to_string()))?
        .to_string();

    info!("Using sandbox - api_url: {}", api_url);
    Ok((mcp_json_string, api_url))
}

/// Setup git authentication with retry logic
async fn setup_git_auth(
    sbx: &sandbox_client::Client,
    session_id: uuid::Uuid,
) -> Result<(), ProcessingError> {
    info!("Setting up GitHub authentication for session {}", session_id);

    let github_token = std::env::var("GITHUB_TOKEN").map_err(|e| {
        ProcessingError::Permanent(format!("GITHUB_TOKEN environment variable not set: {}", e))
    })?;

    // Authenticate with GitHub
    let auth_command = format!("echo '{}' | gh auth login --with-token", github_token);
    sbx.exec_command_v1_shell_exec_post(&ShellExecRequest {
        command: auth_command,
        async_mode: false,
        id: None,
        timeout: Some(30.0_f64),
        exec_dir: Some(String::from("/home/gem")),
    })
    .await
    .map_err(|e| ProcessingError::Transient(format!("Failed to authenticate with GitHub: {}", e)))?;

    // Setup git credentials
    sbx.exec_command_v1_shell_exec_post(&ShellExecRequest {
        command: "gh auth setup-git".to_string(),
        async_mode: false,
        id: None,
        timeout: Some(30.0_f64),
        exec_dir: Some(String::from("/home/gem")),
    })
    .await
    .map_err(|e| ProcessingError::Transient(format!("Failed to setup git credentials: {}", e)))?;

    info!("Successfully set up GitHub authentication");
    Ok(())
}

/// Clone repository (idempotent - checks if already exists)
async fn clone_repository_idempotent(
    sbx: &sandbox_client::Client,
    repo: &str,
    repo_dir: &str,
    repo_path: &str,
) -> Result<(), ProcessingError> {
    info!("Checking if repository already cloned at {}", repo_path);

    // Check if repo already exists
    let check_result = sbx
        .exec_command_v1_shell_exec_post(&ShellExecRequest {
            command: format!("test -d {}", repo_path),
            async_mode: false,
            id: None,
            timeout: Some(10.0_f64),
            exec_dir: Some(String::from("/home/gem")),
        })
        .await;

    match check_result {
        Ok(_) => {
            info!("Repository already exists at {}, skipping clone", repo_path);
            return Ok(());
        }
        Err(_) => {
            info!("Repository not found, proceeding with clone");
        }
    }

    // Clone the repository
    sbx.exec_command_v1_shell_exec_post(&ShellExecRequest {
        command: format!("git clone https://github.com/{}.git {}", repo, repo_dir),
        async_mode: false,
        id: None,
        timeout: Some(60.0_f64),
        exec_dir: Some(String::from("/home/gem")),
    })
    .await
    .map_err(|e| ProcessingError::Transient(format!("Failed to clone repository: {}", e)))?;

    info!("Successfully cloned repository to {}", repo_path);
    Ok(())
}

/// Checkout branches (idempotent)
async fn checkout_branches_idempotent(
    sbx: &sandbox_client::Client,
    repo_path: &str,
    target_branch: &str,
    work_branch: &str,
) -> Result<(), ProcessingError> {
    info!("Checking out target branch: {}", target_branch);

    // Checkout target branch
    sbx.exec_command_v1_shell_exec_post(&ShellExecRequest {
        command: format!("git checkout {}", target_branch),
        async_mode: false,
        id: None,
        timeout: Some(30.0_f64),
        exec_dir: Some(repo_path.to_string()),
    })
    .await
    .map_err(|e| {
        ProcessingError::Transient(format!("Failed to checkout target branch: {}", e))
    })?;

    // Checkout or create work branch
    info!("Checking out work branch: {}", work_branch);
    sbx.exec_command_v1_shell_exec_post(&ShellExecRequest {
        command: format!("git checkout {} || git switch -c {}", work_branch, work_branch),
        async_mode: false,
        id: None,
        timeout: Some(30.0_f64),
        exec_dir: Some(repo_path.to_string()),
    })
    .await
    .map_err(|e| ProcessingError::Transient(format!("Failed to checkout work branch: {}", e)))?;

    info!("Successfully checked out branches");
    Ok(())
}

/// Spawn Claude CLI with guaranteed cleanup
fn spawn_claude_cli_with_cleanup(
    prompt_id: uuid::Uuid,
    session_id: uuid::Uuid,
    db: DatabaseConnection,
    mcp_json_string: String,
    prompt_content: String,
    repo_path: String,
    repo: Option<String>,
    branch: String,
) {
    tokio::spawn(async move {
        let result = run_claude_cli(
            prompt_id,
            session_id,
            &db,
            &mcp_json_string,
            &prompt_content,
            &repo_path,
            &repo,
            &branch,
        )
        .await;

        // Always mark as completed or failed
        match result {
            Ok(_) => {
                info!("Claude CLI completed successfully for prompt {}", prompt_id);
                if let Err(e) = mark_prompt_as_completed(prompt_id, &db).await {
                    error!("Failed to mark prompt {} as completed: {}", prompt_id, e);
                }
            }
            Err(e) => {
                error!("Claude CLI failed for prompt {}: {}", prompt_id, e);
                if let Err(e) = mark_prompt_as_failed(prompt_id, &db, &e.to_string()).await {
                    error!("Failed to mark prompt {} as failed: {}", prompt_id, e);
                }
            }
        }

        // Update session status to ReturningIp (poller will handle IP return)
        info!("Updating session {} status to ReturningIp", session_id);
        if let Err(e) = update_session_to_returning_ip(session_id, &db).await {
            error!(
                "Failed to update session {} to ReturningIp: {}",
                session_id, e
            );
        }
    });
}

/// Run Claude CLI and stream output to database
async fn run_claude_cli(
    prompt_id: uuid::Uuid,
    session_id: uuid::Uuid,
    db: &DatabaseConnection,
    mcp_json_string: &str,
    prompt_content: &str,
    repo_path: &str,
    repo: &Option<String>,
    branch: &str,
) -> Result<(), ProcessingError> {
    info!("Running Claude Code CLI for session {}", session_id);

    // Create temporary directory
    let temp_base_dir = std::env::var("TMPDIR")
        .or_else(|_| std::env::var("TEMP_DIR"))
        .unwrap_or_else(|_| {
            std::env::var("HOME")
                .map(|home| format!("{}/.tmp", home))
                .unwrap_or_else(|_| ".".to_string())
        });

    std::fs::create_dir_all(&temp_base_dir).map_err(|e| {
        ProcessingError::Permanent(format!("Failed to create temp directory: {}", e))
    })?;

    let temp_dir = tempfile::Builder::new()
        .prefix(&format!("claude_session_{}_", session_id))
        .tempdir_in(&temp_base_dir)
        .map_err(|e| ProcessingError::Permanent(format!("Failed to create temp dir: {}", e)))?;

    // Write MCP config
    let mcp_config_path = temp_dir.path().join("mcp_config.json");
    std::fs::write(&mcp_config_path, mcp_json_string).map_err(|e| {
        ProcessingError::Permanent(format!("Failed to write MCP config: {}", e))
    })?;

    // Load and prepare system prompt
    const SYSTEM_PROMPT_TEMPLATE: &str =
        include_str!("../../prompts/outbox_handler_system_prompt.md");
    let system_prompt = SYSTEM_PROMPT_TEMPLATE
        .replace("{REPO_PATH}", repo_path)
        .replace(
            "{REPO}",
            &repo.clone().unwrap_or_else(|| "unknown/repo".to_string()),
        )
        .replace("{BRANCH}", branch);

    // Clone variables for spawn_blocking
    let prompt_for_cli = prompt_content.to_string();
    let session_id_str = session_id.to_string();
    let mcp_config_path_str = mcp_config_path.to_str().unwrap().to_string();
    let temp_dir_path = temp_dir.path().to_path_buf();
    let db_clone = db.clone();

    // Spawn blocking task to run Claude CLI
    let result = tokio::task::spawn_blocking(move || {
        use std::io::{BufRead, BufReader};
        use std::process::{Command, Stdio};

        let child = Command::new("claude")
            .args([
                "--dangerously-skip-permissions",
                "--print",
                "--output-format=stream-json",
                "--session-id",
                &session_id_str,
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
                &mcp_config_path_str,
            ])
            .current_dir(&temp_dir_path)
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

        let stdout = child.stdout.take().expect("Failed to capture stdout");
        let stderr = child.stderr.take().expect("Failed to capture stderr");

        // Handle stderr in separate thread
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

        // Read and process stdout
        let stdout_reader = BufReader::new(stdout);
        let mut line_count = 0;

        for line in stdout_reader.lines() {
            match line {
                Ok(line) => {
                    line_count += 1;
                    if line.trim().is_empty() {
                        continue;
                    }

                    info!("Claude Code output line {} for session {}: {}", line_count, session_id, line);

                    // Parse and insert message
                    match serde_json::from_str::<serde_json::Value>(&line) {
                        Ok(json) => {
                            let message_id = uuid::Uuid::new_v4();
                            let new_message = message::ActiveModel {
                                id: Set(message_id),
                                prompt_id: Set(prompt_id),
                                data: Set(json),
                                created_at: NotSet,
                                updated_at: NotSet,
                            };

                            let handle = tokio::runtime::Handle::current();
                            let db_clone2 = db_clone.clone();
                            match handle.block_on(async move {
                                new_message.insert(&db_clone2).await
                            }) {
                                Ok(_) => {
                                    info!("Created message {} for prompt {}", message_id, prompt_id);
                                }
                                Err(e) => {
                                    error!("Failed to create message {}: {}", message_id, e);
                                }
                            }
                        }
                        Err(e) => {
                            error!("Failed to parse JSON at line {}: {}. Content: {}", line_count, e, line);
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

        let status = child.wait()?;
        info!("Claude Code CLI exit status for session {}: {:?}", session_id, status);

        Ok(status)
    })
    .await
    .map_err(|e| ProcessingError::Transient(format!("Failed to join blocking task: {}", e)))?;

    result.map_err(|e| ProcessingError::Transient(format!("Claude CLI execution failed: {}", e)))?;

    Ok(())
}

/// Mark prompt as completed
async fn mark_prompt_as_completed(
    prompt_id: uuid::Uuid,
    db: &DatabaseConnection,
) -> Result<(), ProcessingError> {
    let prompt = Prompt::find_by_id(prompt_id)
        .one(db)
        .await
        .map_err(|e| ProcessingError::DatabaseError(e.to_string()))?
        .ok_or_else(|| ProcessingError::Permanent("Prompt not found".to_string()))?;

    let mut active_prompt: crate::entities::prompt::ActiveModel = prompt.into();
    active_prompt.inbox_status = Set(InboxStatus::Completed);
    active_prompt.completed_at = Set(Some(chrono::Utc::now().into()));
    active_prompt.last_error = Set(None);

    active_prompt
        .update(db)
        .await
        .map_err(|e| ProcessingError::DatabaseError(e.to_string()))?;

    Ok(())
}

/// Update session status to ReturningIp
async fn update_session_to_returning_ip(
    session_id: uuid::Uuid,
    db: &DatabaseConnection,
) -> Result<(), ProcessingError> {
    let session = Session::find_by_id(session_id)
        .one(db)
        .await
        .map_err(|e| ProcessingError::DatabaseError(e.to_string()))?
        .ok_or_else(|| ProcessingError::Permanent("Session not found".to_string()))?;

    let mut active_session: crate::entities::session::ActiveModel = session.into();
    active_session.session_status = Set(SessionStatus::ReturningIp);
    active_session.status_message = Set(Some("Returning IP".to_string()));

    active_session
        .update(db)
        .await
        .map_err(|e| ProcessingError::DatabaseError(e.to_string()))?;

    Ok(())
}
