use apalis::prelude::*;
use sea_orm::{
    ActiveModelTrait, ColumnTrait, DatabaseConnection, EntityTrait, NotSet, Order, QueryFilter,
    QueryOrder, Set,
};
use serde::{Deserialize, Serialize};
use serde_json::json;
use tracing::{error, info};

use sandbox_client::types::FileContentEncoding;
use sandbox_client::types::FileWriteRequest;
use sandbox_client::types::ShellExecRequest;

use crate::entities::message;
use crate::entities::message::Entity as Message;
use crate::entities::prompt::Entity as Prompt;
use crate::entities::session::{CancellationStatus, Entity as Session, UiStatus};

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

/// Fetch all previous prompts in the session and format them using toon-format
async fn get_formatted_session_history(
    db: &DatabaseConnection,
    session_id: uuid::Uuid,
    current_prompt_id: uuid::Uuid,
) -> Result<String, Error> {
    // Fetch all prompts for this session, excluding the current prompt, ordered by creation time
    let prompts = Prompt::find()
        .filter(crate::entities::prompt::Column::SessionId.eq(session_id))
        .filter(crate::entities::prompt::Column::Id.ne(current_prompt_id))
        .order_by(crate::entities::prompt::Column::CreatedAt, Order::Asc)
        .all(db)
        .await
        .map_err(|e| {
            error!("Failed to fetch prompts for session {}: {}", session_id, e);
            Error::Failed(Box::new(e))
        })?;

    if prompts.is_empty() {
        info!("No previous prompts found for session {}", session_id);
        return Ok(String::new());
    }

    info!(
        "Found {} previous prompts for session {}",
        prompts.len(),
        session_id
    );

    // Build the session history structure
    let mut session_data = Vec::new();

    for prompt in prompts {
        // Fetch messages for this prompt
        let messages = Message::find()
            .filter(crate::entities::message::Column::PromptId.eq(prompt.id))
            .order_by(crate::entities::message::Column::CreatedAt, Order::Asc)
            .all(db)
            .await
            .map_err(|e| {
                error!("Failed to fetch messages for prompt {}: {}", prompt.id, e);
                Error::Failed(Box::new(e))
            })?;

        let mut messages_data = Vec::new();
        for message in messages {
            messages_data.push(message.data);
        }

        session_data.push(json!({
            "prompt_id": prompt.id.to_string(),
            "prompt_data": prompt.data,
            "messages": messages_data,
        }));
    }

    // Create the final JSON structure
    let history_json = json!({
        "session_id": session_id.to_string(),
        "previous_prompts": session_data,
    });

    // Use toon-format to encode the history
    let formatted_history = toon_format::encode_default(&history_json).map_err(|e| {
        error!("Failed to format session history with toon-format: {}", e);
        Error::Failed(format!("Toon format error: {}", e).into())
    })?;

    Ok(formatted_history)
}

/// Process an outbox job: read prompt by ID, get related session, set up sandbox, and run Claude Code
pub async fn process_outbox_job(job: OutboxJob, ctx: Data<OutboxContext>) -> Result<(), Error> {
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

    // Check if session has cancellation requested
    if let Some(CancellationStatus::Requested) = _session_model.cancellation_status {
        info!(
            "Session {} has cancellation requested - marking as cancelled and skipping processing",
            session_id
        );

        // Update session to mark as cancelled
        let mut active_session: crate::entities::session::ActiveModel = _session_model.into();
        active_session.cancellation_status = Set(Some(CancellationStatus::Cancelled));
        active_session.ui_status = Set(UiStatus::NeedsReview);

        active_session.update(&ctx.db).await.map_err(|e| {
            error!(
                "Failed to update session {} to cancelled status: {}",
                session_id, e
            );
            Error::Failed(Box::new(e))
        })?;

        info!("Session {} marked as cancelled", session_id);
        return Ok(());
    }

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

    // Fetch and format session history using toon-format
    let formatted_history: String =
        get_formatted_session_history(&ctx.db, session_id, prompt_id).await?;

    // Prepend the formatted history to the current prompt if there is history
    let prompt_content = if !formatted_history.is_empty() {
        format!(
            "# Previous Session History\n\n{}\n\n# Current Prompt\n\n{}",
            formatted_history, prompt_content
        )
    } else {
        prompt_content
    };

    // Read borrowed IP from session's sbx_config (already allocated by prompt_poller)
    let borrowed_ip_json = _session_model.sbx_config.as_ref().ok_or_else(|| {
        error!(
            "Session {} has no sbx_config - IP should have been borrowed during enqueue",
            session_id
        );
        Error::Failed("Session missing sbx_config".into())
    })?;

    // Parse the sbx_config JSON to extract mcp_json_string and api_url
    // Note: The data is nested under "item" key from prompt_poller
    let item = borrowed_ip_json["item"]
        .as_object()
        .ok_or_else(|| Error::Failed("Missing item object in sbx_config".into()))?;

    let mcp_json_string = item["mcp_json_string"]
        .as_str()
        .ok_or_else(|| Error::Failed("Missing mcp_json_string in sbx_config.item".into()))?
        .to_string();

    let api_url = item["api_url"]
        .as_str()
        .ok_or_else(|| Error::Failed("Missing api_url in sbx_config.item".into()))?;

    // Create sandbox client using the api_url
    let sbx = sandbox_client::Client::new(api_url);

    let uuid = uuid::Uuid::new_v4();
    let prompt_file_path = format!("/home/gem/prompt_{}.md", uuid);
    let prompt_file_path_for_cli = prompt_file_path.clone();
    // upload formatted history to a file in the sandbox
    sbx.write_file(&FileWriteRequest {
        content: prompt_content.to_string(),
        file: prompt_file_path.clone(),
        append: false,
        sudo: false,
        encoding: FileContentEncoding::Utf8,
        leading_newline: false,
        trailing_newline: true,
    })
    .await
    .map_err(|e| {
        error!("Failed to upload formatted history to sandbox: {}", e);
        Error::Failed(Box::new(e))
    })?;

    // Read GitHub token from environment variable
    let github_token = std::env::var("GITHUB_TOKEN").map_err(|e| {
        error!("Failed to read GITHUB_TOKEN from environment: {}", e);
        Error::Failed(Box::new(std::io::Error::new(
            std::io::ErrorKind::NotFound,
            "GITHUB_TOKEN environment variable not set",
        )))
    })?;

    // Authenticate with GitHub using the fetched token

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
    // Pass the token to gh auth login via stdin
    sbx.exec_command_v1_shell_exec_post(&ShellExecRequest {
        command: "gh auth setup-git".to_string(),
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
    // clone the repo using session_id as directory name
    let repo_dir = format!("repo_{}", session_id);
    sbx.exec_command_v1_shell_exec_post(&ShellExecRequest {
        command: format!(
            "git clone https://github.com/{}.git {}",
            _session_model.repo.clone().unwrap(),
            repo_dir
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
    let repo_path = format!("/home/gem/{}", repo_dir);
    sbx.exec_command_v1_shell_exec_post(&ShellExecRequest {
        command: format!(
            "git checkout {}",
            _session_model.target_branch.clone().unwrap()
        ),
        async_mode: false,
        id: None,
        timeout: Some(30.0_f64),
        exec_dir: Some(repo_path.clone()),
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
        exec_dir: Some(repo_path.clone()),
    })
    .await
    .map_err(|e| {
        error!("Failed to execute command: {}", e);
        Error::Failed(Box::new(e))
    })?;

    // Run Claude Code CLI directly in the job (not fire-and-forget)
    let session_id = _session_model.id;
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
        return Err(Error::Failed(Box::new(e)));
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
            return Err(Error::Failed(Box::new(e)));
        }
    };

    // Write MCP config to a file
    let mcp_config_path = temp_dir.path().join("mcp_config.json");
    if let Err(e) = std::fs::write(&mcp_config_path, &mcp_json_string) {
        error!(
            "Failed to write MCP config for session {}: {}",
            session_id, e
        );
        return Err(Error::Failed(Box::new(e)));
    }

    // Load system prompt template from embedded markdown file
    const SYSTEM_PROMPT_TEMPLATE: &str =
        include_str!("../../prompts/outbox_handler_system_prompt.md");

    // Construct system prompt with context about the task by replacing placeholders
    let system_prompt = SYSTEM_PROMPT_TEMPLATE
        .replace("{REPO_PATH}", &repo_path)
        .replace(
            "{REPO}",
            &_session_model
                .repo
                .clone()
                .unwrap_or_else(|| "unknown/repo".to_string()),
        )
        .replace("{BRANCH}", &branch)
        .replace(
            "{TARGET_BRANCH}",
            &_session_model
                .target_branch
                .clone()
                .unwrap_or_else(|| "main".to_string()),
        );

    // Create clones for spawn_blocking
    let prompt_id_clone = prompt_id;
    let db_clone = ctx.db.clone();
    let session_id_clone = session_id;
    let db_for_pid = ctx.db.clone();

    // Spawn the Claude CLI process with piped stdout/stderr for streaming
    let cli_result = tokio::task::spawn_blocking(move || {
        use std::io::{BufRead, BufReader};
        use std::process::{Command, Stdio};

        let child = Command::new("claude")
            .args([
                "--dangerously-skip-permissions",
                "--print",
                "--output-format=stream-json",
                "--session-id",
                &session_id_clone.to_string(),
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
                &format!("`cat {}`", prompt_file_path_for_cli),
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
                error!("Failed to spawn Claude CLI for session {}: {}", session_id_clone, e);
                return Err(e);
            }
        };

        // Store the process PID in the database
        let pid = child.id();
        info!("Claude CLI process spawned with PID {} for session {}", pid, session_id_clone);

        // Update session with PID using tokio runtime handle
        let handle = tokio::runtime::Handle::current();
        let update_result = handle.block_on(async {
            let session = Session::find_by_id(session_id_clone)
                .one(&db_for_pid)
                .await
                .map_err(|e| {
                    error!("Failed to query session {} for PID update: {}", session_id_clone, e);
                    e
                })?
                .ok_or_else(|| {
                    error!("Session {} not found for PID update", session_id_clone);
                    sea_orm::DbErr::RecordNotFound(format!("Session {} not found", session_id_clone))
                })?;

            let mut active_session: crate::entities::session::ActiveModel = session.into();
            active_session.process_pid = Set(Some(pid as i32));

            active_session.update(&db_for_pid).await.map_err(|e| {
                error!("Failed to update session {} with PID: {}", session_id_clone, e);
                e
            })
        });

        if let Err(e) = update_result {
            error!("Failed to store PID for session {}: {}", session_id_clone, e);
            // Continue anyway - the process is already running
        } else {
            info!("Successfully stored PID {} for session {}", pid, session_id_clone);
        }

        // Take stdout and stderr handles
        let stdout = child.stdout.take().expect("Failed to capture stdout");
        let stderr = child.stderr.take().expect("Failed to capture stderr");

        // Spawn a thread to handle stderr
        let session_id_for_stderr = session_id_clone;
        std::thread::spawn(move || {
            let stderr_reader = BufReader::new(stderr);
            let mut stderr_lines = Vec::new();
            for line in stderr_reader.lines() {
                match line {
                    Ok(line) => {
                        stderr_lines.push(line);
                    }
                    Err(e) => {
                        error!("Error reading stderr for session {}: {}", session_id_for_stderr, e);
                        break;
                    }
                }
            }
            if !stderr_lines.is_empty() {
                error!("Claude Code stderr for session {} ({} lines total). First/last lines: [{} ... {}]",
                    session_id_for_stderr,
                    stderr_lines.len(),
                    stderr_lines.first().unwrap_or(&String::new()),
                    stderr_lines.last().unwrap_or(&String::new())
                );
            }
        });

        // Read stdout line by line and send to channel
        let stdout_reader = BufReader::new(stdout);
        let mut line_count = 0;
        let mut message_count = 0;
        let mut error_count = 0;

        for line in stdout_reader.lines() {
            match line {
                Ok(line) => {
                    line_count += 1;

                    // Skip empty lines
                    if line.trim().is_empty() {
                        continue;
                    }

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
                                    message_count += 1;
                                }
                                Err(e) => {
                                    error_count += 1;
                                    error!("Failed to create message for session {}: {}", session_id_clone, e);
                                }
                            }
                        }
                        Err(e) => {
                            error_count += 1;
                            // Only log first few parse errors to avoid spam
                            if error_count <= 3 {
                                error!("Failed to parse JSON at line {} for session {}: {}", line_count, session_id_clone, e);
                            }
                        }
                    }
                }
                Err(e) => {
                    error!("Error reading stdout for session {}: {}", session_id_clone, e);
                    break;
                }
            }
        }

        info!("Processed {} lines of output for session {} ({} messages created, {} errors)", line_count, session_id_clone, message_count, error_count);

        // Wait for process to complete and get exit status
        let status = child.wait()?;
        info!("Claude Code CLI exit status for session {}: {:?}", session_id_clone, status);

        Ok(status)
    })
    .await
    .map_err(|e| {
        error!("Failed to join spawn_blocking task: {}", e);
        Error::Failed(Box::new(e))
    })?;

    // Log the CLI result
    match cli_result {
        Ok(status) => {
            info!("Claude CLI completed with status: {:?}", status);
        }
        Err(e) => {
            error!("Claude CLI process failed: {}", e);
            return Err(Error::Failed(Box::new(e)));
        }
    }

    // Update session ui_status to NeedsReview (poller will handle IP return)
    info!("Updating session {} ui_status to NeedsReview", session_id);

    let session_result = Session::find_by_id(session_id).one(&ctx.db).await;
    match session_result {
        Ok(Some(session_model)) => {
            let mut active_session: crate::entities::session::ActiveModel = session_model.into();
            active_session.ui_status = Set(UiStatus::NeedsReview);
            active_session.process_pid = Set(None); // Clear PID now that process is complete

            if let Err(e) = active_session.update(&ctx.db).await {
                error!(
                    "Failed to update session {} ui_status to NeedsReview: {}",
                    session_id, e
                );
                return Err(Error::Failed(Box::new(e)));
            } else {
                info!(
                    "Updated session {} ui_status to NeedsReview - poller will handle IP return",
                    session_id
                );
            }
        }
        Ok(None) => {
            error!(
                "Session {} not found when trying to update status",
                session_id
            );
            return Err(Error::Failed("Session not found".into()));
        }
        Err(e) => {
            error!(
                "Failed to query session {} for status update: {}",
                session_id, e
            );
            return Err(Error::Failed(Box::new(e)));
        }
    }

    info!("Completed outbox job for prompt_id: {}", job.prompt_id);

    Ok(())
}
