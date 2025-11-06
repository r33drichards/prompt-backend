use apalis::prelude::*;
use sea_orm::{ActiveModelTrait, DatabaseConnection, EntityTrait, NotSet, Set};
use serde::{Deserialize, Serialize};
use tracing::{error, info};

use sandbox_client::types::ShellExecRequest;

use crate::entities::message;
use crate::entities::prompt::Entity as Prompt;
use crate::entities::session::Entity as Session;

/// Job that reads from PostgreSQL outbox and publishes to Redis
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OutboxJob {
    pub prompt_id: String,
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

    // Fire-and-forget task to run Claude Code CLI
    let session_id = _session_model.id;
    let prompt_id_clone = prompt_id;
    let borrowed_ip_item = borrowed_ip.item.clone();
    let ip_allocator_url_clone = ip_allocator_url.clone();
    let db_clone = ctx.db.clone();
    let prompt_content_clone = prompt_content.clone();
    let api_url_clone = api_url.to_string();
    let repo_clone = _session_model.repo.clone();
    let target_branch_clone = _session_model.target_branch.clone();
    let branch_clone = branch.clone();
    let title_clone = _session_model.title.clone();
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
        const SYSTEM_PROMPT_TEMPLATE: &str = include_str!("../../prompts/outbox_handler_system_prompt.md");

        // Construct system prompt with context about the task by replacing placeholders
        let system_prompt = SYSTEM_PROMPT_TEMPLATE
            .replace("{REPO_PATH}", &repo_path_clone)
            .replace("{REPO}", &repo_clone.clone().unwrap_or_else(|| "unknown/repo".to_string()))
            .replace("{BRANCH}", &branch_clone);

        // Spawn the Claude CLI process with piped stdout/stderr for streaming
        let result = tokio::task::spawn_blocking(move || {
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

        // Handle the result
        match result {
            Ok(Ok(status)) => {
                info!("Claude Code CLI completed for session {}", session_id);
                if !status.success() {
                    error!(
                        "Claude Code CLI failed with exit status: {:?} for session {}",
                        status, session_id
                    );
                } else {
                    // Claude CLI succeeded - now deterministically commit, push, and create PR
                    info!("Claude Code CLI succeeded - executing deterministic git workflow for session {}", session_id);

                    // Recreate sandbox client
                    let sbx = sandbox_client::Client::new(&api_url_clone);

                    // Step 1: Configure git user
                    info!("Configuring git user for session {}", session_id);
                    let git_config_name = sbx
                        .exec_command_v1_shell_exec_post(&ShellExecRequest {
                            command: String::from("git config user.name 'claude-code-bot[bot]'"),
                            async_mode: false,
                            id: None,
                            timeout: Some(30.0_f64),
                            exec_dir: Some(repo_path_clone.clone()),
                        })
                        .await;

                    if let Err(e) = git_config_name {
                        error!(
                            "Failed to configure git user.name for session {}: {}",
                            session_id, e
                        );
                    }

                    let git_config_email = sbx.exec_command_v1_shell_exec_post(&ShellExecRequest {
                        command: String::from("git config user.email 'claude-code-bot[bot]@users.noreply.github.com'"),
                        async_mode: false,
                        id: None,
                        timeout: Some(30.0_f64),
                        exec_dir: Some(repo_path_clone.clone()),
                    }).await;

                    if let Err(e) = git_config_email {
                        error!(
                            "Failed to configure git user.email for session {}: {}",
                            session_id, e
                        );
                    }

                    // Step 2: Add all changes
                    info!("Adding all changes for session {}", session_id);
                    let git_add = sbx
                        .exec_command_v1_shell_exec_post(&ShellExecRequest {
                            command: String::from("git add ."),
                            async_mode: false,
                            id: None,
                            timeout: Some(30.0_f64),
                            exec_dir: Some(repo_path_clone.clone()),
                        })
                        .await;

                    if let Err(e) = git_add {
                        error!("Failed to add changes for session {}: {}", session_id, e);
                    }

                    // Step 3: Commit if there are staged changes
                    info!("Committing changes for session {}", session_id);
                    let commit_message = format!(
                        "Claude Code: {}",
                        title_clone
                            .as_deref()
                            .unwrap_or(&format!("Session {}", session_id))
                    );

                    // Use git diff --staged --quiet to check if there are changes, if not, exit 0, else commit
                    let git_commit_cmd = format!(
                        "git diff --staged --quiet || git commit -m \"{}\"",
                        commit_message.replace("\"", "\\\"")
                    );

                    let git_commit = sbx
                        .exec_command_v1_shell_exec_post(&ShellExecRequest {
                            command: git_commit_cmd,
                            async_mode: false,
                            id: None,
                            timeout: Some(30.0_f64),
                            exec_dir: Some(repo_path_clone.clone()),
                        })
                        .await;

                    match git_commit {
                        Ok(_) => {
                            info!("Successfully committed changes for session {}", session_id);

                            // Step 4: Push to remote
                            info!("Pushing changes to remote for session {}", session_id);
                            let git_push = sbx
                                .exec_command_v1_shell_exec_post(&ShellExecRequest {
                                    command: format!("git push -u origin {}", branch_clone),
                                    async_mode: false,
                                    id: None,
                                    timeout: Some(60.0_f64),
                                    exec_dir: Some(repo_path_clone.clone()),
                                })
                                .await;

                            match git_push {
                                Ok(_) => {
                                    info!("Successfully pushed changes for session {}", session_id);

                                    // Step 5: Create PR using gh CLI
                                    if let (Some(_repo), Some(target_branch)) =
                                        (&repo_clone, &target_branch_clone)
                                    {
                                        info!("Creating PR for session {}", session_id);

                                        let default_title =
                                            format!("Claude Code Session {}", session_id);
                                        let pr_title =
                                            title_clone.as_deref().unwrap_or(&default_title);
                                        let pr_body = format!(
                                            "## Summary\n\nAutomated changes from Claude Code session.\n\n**Prompt**: {}\n\n**Session ID**: {}\n\n---\n*This PR was automatically created by Claude Code Bot*",
                                            prompt_content_clone.chars().take(200).collect::<String>(),
                                            session_id
                                        );

                                        let gh_pr_create_cmd = format!(
                                            "gh pr create --title \"{}\" --body \"{}\" --base \"{}\" --head \"{}\"",
                                            pr_title.replace("\"", "\\\""),
                                            pr_body.replace("\"", "\\\"").replace("\n", "\\n"),
                                            target_branch,
                                            branch_clone
                                        );

                                        let pr_result = sbx
                                            .exec_command_v1_shell_exec_post(&ShellExecRequest {
                                                command: gh_pr_create_cmd,
                                                async_mode: false,
                                                id: None,
                                                timeout: Some(60.0_f64),
                                                exec_dir: Some(repo_path_clone.clone()),
                                            })
                                            .await;

                                        match pr_result {
                                            Ok(pr_response) => {
                                                info!(
                                                    "Successfully created PR for session {}: {:?}",
                                                    session_id, pr_response
                                                );
                                            }
                                            Err(e) => {
                                                error!(
                                                    "Failed to create PR for session {}: {}",
                                                    session_id, e
                                                );
                                            }
                                        }
                                    } else {
                                        error!("Cannot create PR for session {}: missing repo or target_branch", session_id);
                                    }
                                }
                                Err(e) => {
                                    error!(
                                        "Failed to push changes for session {}: {}",
                                        session_id, e
                                    );
                                }
                            }
                        }
                        Err(e) => {
                            error!("Failed to commit changes for session {}: {}", session_id, e);
                        }
                    }
                }
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

    info!("Completed outbox job for prompt_id: {}", job.prompt_id);

    Ok(())
}
