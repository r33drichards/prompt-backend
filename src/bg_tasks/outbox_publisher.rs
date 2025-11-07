use apalis::prelude::*;
use sea_orm::{ActiveModelTrait, DatabaseConnection, EntityTrait, NotSet, Set};
use serde::{Deserialize, Serialize};
use tracing::{error, info, warn};

use sandbox_client::types::ShellExecRequest;

use crate::entities::message;
use crate::entities::prompt::{Entity as Prompt, InboxStatus};
use crate::entities::session::{Entity as Session, SessionStatus};
use crate::services::dead_letter_queue;

/// Maximum number of retries for transient failures
const MAX_RETRIES: u32 = 3;
/// Delay between retries in milliseconds
const RETRY_DELAY_MS: u64 = 1000;
/// Maximum retry count before moving to DLQ
const MAX_DLQ_RETRY_COUNT: i32 = 5;

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

/// Helper function to execute sandbox command with retry logic
async fn execute_sandbox_command_with_retry(
    sbx: &sandbox_client::Client,
    request: &ShellExecRequest,
    operation_name: &str,
) -> Result<sandbox_client::types::ShellExecResponse, String> {
    let mut last_error = String::new();
    
    for attempt in 1..=MAX_RETRIES {
        match sbx.exec_command_v1_shell_exec_post(request).await {
            Ok(response) => {
                if attempt > 1 {
                    info!("{} succeeded on retry attempt {}", operation_name, attempt);
                }
                return Ok(response);
            }
            Err(e) => {
                last_error = format!("{}", e);
                warn!(
                    "{} failed on attempt {}/{}: {}",
                    operation_name, attempt, MAX_RETRIES, last_error
                );
                
                if attempt < MAX_RETRIES {
                    let delay = std::time::Duration::from_millis(RETRY_DELAY_MS * attempt as u64);
                    info!("Retrying {} after {:?}", operation_name, delay);
                    tokio::time::sleep(delay).await;
                }
            }
        }
    }
    
    Err(last_error)
}

/// Update prompt status to handle failures
async fn update_prompt_status(
    db: &DatabaseConnection,
    prompt_id: uuid::Uuid,
    status: InboxStatus,
) -> Result<(), sea_orm::DbErr> {
    let prompt = Prompt::find_by_id(prompt_id)
        .one(db)
        .await?
        .ok_or_else(|| sea_orm::DbErr::RecordNotFound("Prompt not found".to_string()))?;
    
    let mut active_prompt: crate::entities::prompt::ActiveModel = prompt.into();
    active_prompt.inbox_status = Set(status);
    active_prompt.update(db).await?;
    
    Ok(())
}

/// Update session status with error handling
async fn update_session_status_safe(
    db: &DatabaseConnection,
    session_id: uuid::Uuid,
    status: SessionStatus,
    message: Option<String>,
) {
    match Session::find_by_id(session_id).one(db).await {
        Ok(Some(session_model)) => {
            let mut active_session: crate::entities::session::ActiveModel = session_model.into();
            active_session.session_status = Set(status);
            active_session.status_message = Set(message);

            if let Err(e) = active_session.update(db).await {
                error!("Failed to update session {} status: {}", session_id, e);
            } else {
                info!("Updated session {} status to {:?}", session_id, status);
            }
        }
        Ok(None) => {
            error!("Session {} not found when trying to update status", session_id);
        }
        Err(e) => {
            error!("Failed to query session {} for status update: {}", session_id, e);
        }
    }
}

/// Count existing DLQ entries for this prompt
async fn get_dlq_retry_count(
    db: &DatabaseConnection,
    prompt_id: uuid::Uuid,
) -> Result<i32, sea_orm::DbErr> {
    // Check if prompt already exists in DLQ and get retry count
    if let Ok(exists) = dead_letter_queue::exists_in_dlq(db, "outbox_job", prompt_id).await {
        if exists {
            // Query to get the actual retry count
            use crate::entities::dead_letter_queue::{Column, Entity as DeadLetterQueue};
            use sea_orm::{ColumnTrait, QueryFilter};
            
            if let Some(dlq_entry) = DeadLetterQueue::find()
                .filter(Column::TaskType.eq("outbox_job"))
                .filter(Column::EntityId.eq(prompt_id))
                .one(db)
                .await?
            {
                return Ok(dlq_entry.retry_count);
            }
        }
    }
    Ok(0)
}

/// Handle job failure by moving to DLQ if retry limit exceeded
async fn handle_job_failure(
    db: &DatabaseConnection,
    prompt_id: uuid::Uuid,
    session_id: uuid::Uuid,
    error: &str,
) {
    info!("Handling job failure for prompt {}: {}", prompt_id, error);
    
    // Get current retry count
    let retry_count = match get_dlq_retry_count(db, prompt_id).await {
        Ok(count) => count + 1,
        Err(e) => {
            error!("Failed to get DLQ retry count for prompt {}: {}", prompt_id, e);
            1
        }
    };
    
    info!("Prompt {} has {} retry attempts", prompt_id, retry_count);
    
    if retry_count >= MAX_DLQ_RETRY_COUNT {
        // Move to DLQ permanently
        error!(
            "Prompt {} exceeded max retries ({}), moving to DLQ permanently",
            prompt_id, MAX_DLQ_RETRY_COUNT
        );
        
        // Update prompt status to archived
        if let Err(e) = update_prompt_status(db, prompt_id, InboxStatus::Archived).await {
            error!("Failed to update prompt {} status to Archived: {}", prompt_id, e);
        }
        
        // Update session status
        update_session_status_safe(
            db,
            session_id,
            SessionStatus::Archived,
            Some(format!("Failed after {} retries: {}", MAX_DLQ_RETRY_COUNT, error)),
        )
        .await;
        
        // Insert or update DLQ entry
        let entity_data = serde_json::json!({
            "prompt_id": prompt_id.to_string(),
            "session_id": session_id.to_string(),
        });
        
        if let Err(e) = dead_letter_queue::insert_dlq_entry(
            db,
            "outbox_job",
            prompt_id,
            Some(entity_data),
            retry_count,
            error,
            chrono::Utc::now().into(),
        )
        .await
        {
            error!("Failed to insert DLQ entry for prompt {}: {}", prompt_id, e);
        }
    } else {
        // Just log the failure and let Apalis retry
        warn!(
            "Prompt {} failed (attempt {}/ {}), will retry",
            prompt_id, retry_count, MAX_DLQ_RETRY_COUNT
        );
        
        // Update session status to indicate retry
        update_session_status_safe(
            db,
            session_id,
            SessionStatus::Active,
            Some(format!("Retry attempt {}: {}", retry_count, error)),
        )
        .await;
    }
}

/// Process an outbox job: read prompt by ID, get related session, set up sandbox, and run Claude Code
/// This is the main entry point with comprehensive error handling
pub async fn process_outbox_job(job: OutboxJob, ctx: Data<OutboxContext>) -> Result<(), Error> {
    info!("Processing outbox job for prompt_id: {}", job.prompt_id);

    // Parse prompt ID from job
    let prompt_id = match uuid::Uuid::parse_str(&job.prompt_id) {
        Ok(id) => id,
        Err(e) => {
            error!("Invalid prompt ID format: {}", e);
            // This is a permanent failure - don't retry
            return Err(Error::Abort(Box::new(e)));
        }
    };

    // Execute the core logic and handle failures
    match process_outbox_job_core(prompt_id, &ctx).await {
        Ok(()) => {
            info!("Successfully processed outbox job for prompt_id: {}", prompt_id);
            Ok(())
        }
        Err(e) => {
            error!("Failed to process outbox job for prompt {}: {}", prompt_id, e);
            
            // Try to get session ID for error handling
            if let Ok(Some(prompt)) = Prompt::find_by_id(prompt_id).one(&ctx.db).await {
                handle_job_failure(&ctx.db, prompt_id, prompt.session_id, &e.to_string()).await;
            }
            
            // Return a retriable error to let Apalis retry
            Err(Error::Failed(Box::new(std::io::Error::new(
                std::io::ErrorKind::Other,
                e.to_string(),
            ))))
        }
    }
}

/// Core processing logic with proper error propagation
async fn process_outbox_job_core(
    prompt_id: uuid::Uuid,
    ctx: &OutboxContext,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    // Query the specific prompt
    let prompt_model = Prompt::find_by_id(prompt_id)
        .one(&ctx.db)
        .await?
        .ok_or_else(|| format!("Prompt {} not found", prompt_id))?;

    // Query the related session
    let session_id = prompt_model.session_id;
    let session_model = Session::find_by_id(session_id)
        .one(&ctx.db)
        .await?
        .ok_or_else(|| format!("Session {} not found", session_id))?;

    info!("Processing prompt {} for session {}", prompt_id, session_id);

    // Update prompt status to Active
    update_prompt_status(&ctx.db, prompt_id, InboxStatus::Active).await?;

    // Extract prompt content from the data field
    let prompt_content = match &prompt_model.data {
        serde_json::Value::String(s) => s.clone(),
        serde_json::Value::Object(obj) => {
            obj.get("content")
                .or_else(|| obj.get("prompt"))
                .or_else(|| obj.get("text"))
                .or_else(|| obj.get("message"))
                .and_then(|v| v.as_str())
                .map(|s| s.to_string())
                .unwrap_or_else(|| serde_json::to_string(&prompt_model.data).unwrap_or_default())
        }
        _ => serde_json::to_string(&prompt_model.data).unwrap_or_default(),
    };

    info!(
        "Extracted prompt content (first 100 chars): {}",
        prompt_content.chars().take(100).collect::<String>()
    );

    // Read borrowed IP from session's sbx_config
    let borrowed_ip_json = session_model
        .sbx_config
        .as_ref()
        .ok_or("Session missing sbx_config")?;

    let item = borrowed_ip_json["item"]
        .as_object()
        .ok_or("Missing item object in sbx_config")?;

    let mcp_json_string = item["mcp_json_string"]
        .as_str()
        .ok_or("Missing mcp_json_string in sbx_config.item")?
        .to_string();

    let api_url = item["api_url"]
        .as_str()
        .ok_or("Missing api_url in sbx_config.item")?;

    info!("Using sandbox at {}", api_url);

    // Create sandbox client
    let sbx = sandbox_client::Client::new(api_url);

    // Read GitHub token
    let github_token = std::env::var("GITHUB_TOKEN")
        .map_err(|_| "GITHUB_TOKEN environment variable not set")?;

    info!("Authenticating with GitHub");

    // Authenticate with GitHub (with retry)
    let auth_command = format!("echo '{}' | gh auth login --with-token", github_token);
    execute_sandbox_command_with_retry(
        &sbx,
        &ShellExecRequest {
            command: auth_command,
            async_mode: false,
            id: None,
            timeout: Some(30.0),
            exec_dir: Some(String::from("/home/gem")),
        },
        "GitHub auth login",
    )
    .await
    .map_err(|e| format!("GitHub auth failed: {}", e))?;

    // Setup git (with retry)
    execute_sandbox_command_with_retry(
        &sbx,
        &ShellExecRequest {
            command: "gh auth setup-git".to_string(),
            async_mode: false,
            id: None,
            timeout: Some(30.0),
            exec_dir: Some(String::from("/home/gem")),
        },
        "GitHub setup-git",
    )
    .await
    .map_err(|e| format!("GitHub setup-git failed: {}", e))?;

    // Clone the repo (with retry)
    let repo_dir = format!("repo_{}", session_id);
    let repo_url = session_model
        .repo
        .as_ref()
        .ok_or("Session missing repo field")?;
    
    execute_sandbox_command_with_retry(
        &sbx,
        &ShellExecRequest {
            command: format!("git clone https://github.com/{}.git {}", repo_url, repo_dir),
            async_mode: false,
            id: None,
            timeout: Some(60.0), // Longer timeout for clone
            exec_dir: Some(String::from("/home/gem")),
        },
        "Git clone",
    )
    .await
    .map_err(|e| format!("Git clone failed: {}", e))?;

    // Checkout target branch (with retry)
    let repo_path = format!("/home/gem/{}", repo_dir);
    let target_branch = session_model
        .target_branch
        .as_ref()
        .ok_or("Session missing target_branch field")?;
    
    execute_sandbox_command_with_retry(
        &sbx,
        &ShellExecRequest {
            command: format!("git checkout {}", target_branch),
            async_mode: false,
            id: None,
            timeout: Some(30.0),
            exec_dir: Some(repo_path.clone()),
        },
        "Git checkout target branch",
    )
    .await
    .map_err(|e| format!("Git checkout target branch failed: {}", e))?;

    // Create/checkout feature branch (with retry)
    let branch = session_model
        .branch
        .clone()
        .unwrap_or_else(|| format!("claude/{}", session_id));
    
    execute_sandbox_command_with_retry(
        &sbx,
        &ShellExecRequest {
            command: format!("git checkout {} || git switch -c {}", branch, branch),
            async_mode: false,
            id: None,
            timeout: Some(30.0),
            exec_dir: Some(repo_path.clone()),
        },
        "Git checkout/create feature branch",
    )
    .await
    .map_err(|e| format!("Git checkout/create feature branch failed: {}", e))?;

    // Spawn the Claude Code CLI process in the background
    spawn_claude_cli_task(
        session_id,
        prompt_id,
        prompt_content,
        mcp_json_string,
        repo_path,
        session_model.repo.clone(),
        branch,
        ctx.db.clone(),
    );

    info!("Successfully initiated Claude Code CLI for prompt {}", prompt_id);
    Ok(())
}

/// Spawn Claude Code CLI as a background task with proper error handling
fn spawn_claude_cli_task(
    session_id: uuid::Uuid,
    prompt_id: uuid::Uuid,
    prompt_content: String,
    mcp_json_string: String,
    repo_path: String,
    repo: Option<String>,
    branch: String,
    db: DatabaseConnection,
) {
    tokio::spawn(async move {
        info!("Running Claude Code CLI for session {}", session_id);

        // Create temporary directory
        let temp_base_dir = std::env::var("TMPDIR")
            .or_else(|_| std::env::var("TEMP_DIR"))
            .unwrap_or_else(|_| {
                std::env::var("HOME")
                    .map(|home| format!("{}/.tmp", home))
                    .unwrap_or_else(|_| ".".to_string())
            });

        if let Err(e) = std::fs::create_dir_all(&temp_base_dir) {
            error!("Failed to create temp base directory: {}", e);
            update_session_status_safe(
                &db,
                session_id,
                SessionStatus::ReturningIp,
                Some(format!("Failed to create temp directory: {}", e)),
            )
            .await;
            return;
        }

        let temp_dir = match tempfile::Builder::new()
            .prefix(&format!("claude_session_{}_", session_id))
            .tempdir_in(&temp_base_dir)
        {
            Ok(dir) => dir,
            Err(e) => {
                error!("Failed to create temp directory: {}", e);
                update_session_status_safe(
                    &db,
                    session_id,
                    SessionStatus::ReturningIp,
                    Some(format!("Failed to create temp directory: {}", e)),
                )
                .await;
                return;
            }
        };

        // Write MCP config
        let mcp_config_path = temp_dir.path().join("mcp_config.json");
        if let Err(e) = std::fs::write(&mcp_config_path, &mcp_json_string) {
            error!("Failed to write MCP config: {}", e);
            update_session_status_safe(
                &db,
                session_id,
                SessionStatus::ReturningIp,
                Some(format!("Failed to write MCP config: {}", e)),
            )
            .await;
            return;
        }

        // Load system prompt template
        const SYSTEM_PROMPT_TEMPLATE: &str =
            include_str!("../../prompts/outbox_handler_system_prompt.md");

        let system_prompt = SYSTEM_PROMPT_TEMPLATE
            .replace("{REPO_PATH}", &repo_path)
            .replace(
                "{REPO}",
                &repo.unwrap_or_else(|| "unknown/repo".to_string()),
            )
            .replace("{BRANCH}", &branch);

        // Run Claude CLI
        let cli_result = run_claude_cli(
            session_id,
            prompt_id,
            &prompt_content,
            &system_prompt,
            &mcp_config_path,
            &temp_dir,
            &db,
        )
        .await;

        match cli_result {
            Ok(_) => {
                info!("Claude CLI completed successfully for session {}", session_id);
            }
            Err(e) => {
                error!("Claude CLI failed for session {}: {}", session_id, e);
            }
        }

        // Update session status to ReturningIp
        update_session_status_safe(
            &db,
            session_id,
            SessionStatus::ReturningIp,
            Some("Claude CLI completed, returning IP".to_string()),
        )
        .await;
    });
}

/// Run Claude CLI process and stream output to database
async fn run_claude_cli(
    session_id: uuid::Uuid,
    prompt_id: uuid::Uuid,
    prompt_content: &str,
    system_prompt: &str,
    mcp_config_path: &std::path::Path,
    temp_dir: &tempfile::TempDir,
    db: &DatabaseConnection,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    use std::io::{BufRead, BufReader};
    use std::process::{Command, Stdio};

    tokio::task::spawn_blocking({
        let session_id = session_id;
        let prompt_id = prompt_id;
        let prompt_content = prompt_content.to_string();
        let system_prompt = system_prompt.to_string();
        let mcp_config_path = mcp_config_path.to_path_buf();
        let temp_dir_path = temp_dir.path().to_path_buf();
        let db = db.clone();

        move || {
            let mut child = Command::new("claude")
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
                    &prompt_content,
                    "--verbose",
                    "--strict-mcp-config",
                    "--mcp-config",
                    mcp_config_path.to_str().unwrap(),
                ])
                .current_dir(temp_dir_path)
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .spawn()?;

            let stdout = child.stdout.take().expect("Failed to capture stdout");
            let stderr = child.stderr.take().expect("Failed to capture stderr");

            // Spawn thread for stderr
            let session_id_clone = session_id;
            std::thread::spawn(move || {
                let stderr_reader = BufReader::new(stderr);
                for (i, line) in stderr_reader.lines().enumerate() {
                    if let Ok(line) = line {
                        error!("Claude stderr[{}] session {}: {}", i, session_id_clone, line);
                    }
                }
            });

            // Process stdout
            let stdout_reader = BufReader::new(stdout);
            let mut line_count = 0;

            for line in stdout_reader.lines() {
                if let Ok(line) = line {
                    line_count += 1;

                    if line.trim().is_empty() {
                        continue;
                    }

                    info!("Claude output line {} session {}: {}", line_count, session_id, line);

                    // Parse and store message
                    if let Ok(json) = serde_json::from_str::<serde_json::Value>(&line) {
                        let new_message = message::ActiveModel {
                            id: Set(uuid::Uuid::new_v4()),
                            prompt_id: Set(prompt_id),
                            data: Set(json),
                            created_at: NotSet,
                            updated_at: NotSet,
                        };

                        let handle = tokio::runtime::Handle::current();
                        let db_clone = db.clone();
                        
                        if let Err(e) = handle.block_on(async move {
                            new_message.insert(&db_clone).await
                        }) {
                            error!("Failed to insert message for prompt {}: {}", prompt_id, e);
                        }
                    }
                }
            }

            info!("Processed {} lines for session {}", line_count, session_id);

            let status = child.wait()?;
            info!("Claude CLI exit status for session {}: {:?}", session_id, status);

            Ok::<_, std::io::Error>(status)
        }
    })
    .await??;

    Ok(())
}
