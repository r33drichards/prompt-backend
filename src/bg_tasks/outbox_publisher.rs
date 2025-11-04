use apalis::prelude::*;
use sea_orm::{ColumnTrait, DatabaseConnection, EntityTrait, QueryFilter};
use serde::{Deserialize, Serialize};
use tracing::{error, info};

use sandbox_client::types::ShellExecRequest;
use crate::entities::session::Model as SessionModel;

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


        // if branch exists, checkout the branch, else switch -c the branch
        sbx.exec_command_v1_shell_exec_post(&ShellExecRequest {
            command: format!("git checkout {} || git switch -c {}", _session_model.branch.unwrap(), _session_model.branch.unwrap()),
            async_mode: false,
            id: None,
            timeout: Some(30.0 as f64),
            exec_dir: Some(String::from("/home/gem/repo")),
        }).await.map_err(|e| {
            error!("Failed to execute command: {}", e);
            Error::Failed(Box::new(e))
        })?;

        // TODO: Store borrowed_ip in session_model.sbx_config
        // TODO: Use sandbox_client to interact with the sandbox
        // TODO: Call ip_client.handlers_ip_return() when done

        // run gh auth login sbx sdk using gh auth token, hard code for initial testing 
        // clone target branch
        // create a new branch with name session_model.branch
        // run   npx -y @anthropic-ai/claude-code \
            // --append-system-prompt "you are running as a disposable task agent with a git repo checked out in a feature branch. when you completed with your task, commit and push the changes upstream" \
            // --dangerously-skip-permissions \
            // --print \
            // --output-format=stream-json \
            // --session-id `uuidgen` \
            // --allowedTools "WebSearch" "mcp__*" "ListMcpResourcesTool" "ReadMcpResourceTool" \
            // --disallowedTools "Bash" "Edit" "Write" "NotebookEdit" "Read" "Glob" "Grep" "KillShell" "BashOutput" "TodoWrite" \
            // -p "what are your available tools?" \
            // --verbose \ 
            // --strict-mcp-config \
            // --mcp-config borrow.mcp-config 
            // locally 
        // check ~/claude for the session id messages 
        // update session model with messages and inbox status
    }

    info!("Completed outbox job for session_id: {}", job.session_id);

    Ok(())
}
