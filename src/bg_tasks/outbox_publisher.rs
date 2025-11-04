use apalis::prelude::*;
use sea_orm::{ActiveModelTrait, ColumnTrait, DatabaseConnection, EntityTrait, QueryFilter, Set};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tokio::sync::Mutex;
use tracing::{error, info};

use crate::entities::session::{self, Entity as Session, InboxStatus};
use crate::bg_tasks::session_handler::SessionJob;

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
    for session_model in active_sessions {
        // get sbx config from ip-allocator 
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
