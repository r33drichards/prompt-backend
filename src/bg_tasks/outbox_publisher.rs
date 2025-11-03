use apalis::prelude::*;
use apalis_redis::RedisStorage;
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
    pub redis_storage: Arc<Mutex<RedisStorage<SessionJob>>>,
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
        let session_id = session_model.id.to_string();

        // Create SessionJob for this session
        let session_job = SessionJob {
            session_id: session_id.clone(),
            action: "process".to_string(),
            data: serde_json::json!({
                "messages": session_model.messages,
                "sbx_config": session_model.sbx_config,
                "branch": session_model.branch,
                "repo": session_model.repo,
                "target_branch": session_model.target_branch,
            }),
        };

        // Push job to Redis queue
        let mut storage = ctx.redis_storage.lock().await;
        match storage.push(session_job).await {
            Ok(job_id) => {
                info!("Pushed SessionJob to Redis for session {}: {:?}", session_id, job_id);
                drop(storage); // Release lock before database update

                // Update inbox_status to Pending
                let mut active_model: session::ActiveModel = session_model.into();
                active_model.inbox_status = Set(InboxStatus::Pending);

                match active_model.update(&ctx.db).await {
                    Ok(_) => {
                        info!("Updated session {} inbox_status to Pending", session_id);
                    }
                    Err(e) => {
                        error!("Failed to update session {} inbox_status: {}", session_id, e);
                        // Note: Job is already in Redis, so we log error but continue
                    }
                }
            }
            Err(e) => {
                error!("Failed to push SessionJob to Redis for session {}: {}", session_id, e);
                // Continue with other sessions
            }
        }
    }

    info!("Completed outbox job for session_id: {}", job.session_id);

    Ok(())
}
