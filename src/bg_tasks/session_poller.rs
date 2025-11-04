use apalis::prelude::Storage;
use apalis_sql::postgres::{PgPool, PostgresStorage};
use sea_orm::{ColumnTrait, DatabaseConnection, EntityTrait, QueryFilter};
use std::time::Duration;
use tracing::{error, info};

use super::outbox_publisher::OutboxJob;
use crate::entities::session::{self, Entity as Session, InboxStatus};

/// Periodic poller that checks for active sessions every second
/// and pushes them to the outbox queue for processing
pub async fn run_session_poller(db: DatabaseConnection, pool: PgPool) -> anyhow::Result<()> {
    info!("Starting session poller - checking every 1 second");

    let mut storage = PostgresStorage::new(pool);

    loop {
        tokio::time::sleep(Duration::from_secs(1)).await;

        match poll_and_enqueue_sessions(&db, &mut storage).await {
            Ok(count) => {
                if count > 0 {
                    info!("Enqueued {} active sessions for processing", count);
                }
            }
            Err(e) => {
                error!("Failed to poll and enqueue sessions: {}", e);
            }
        }
    }
}

/// Query for ACTIVE sessions and push them to the outbox queue
async fn poll_and_enqueue_sessions(
    db: &DatabaseConnection,
    storage: &mut PostgresStorage<OutboxJob>,
) -> anyhow::Result<usize> {
    // Query all sessions with Active inbox_status
    let active_sessions = Session::find()
        .filter(session::Column::InboxStatus.eq(InboxStatus::Active))
        .all(db)
        .await?;

    let count = active_sessions.len();

    // Push each active session to the outbox queue
    for session in active_sessions {
        let job = OutboxJob {
            session_id: session.id.to_string(),
            payload: serde_json::json!({}),
        };

        storage.push(job).await.map_err(|e| {
            anyhow::anyhow!("Failed to push job to storage: {}", e)
        })?;
    }

    Ok(count)
}
