use apalis::prelude::Storage;
use apalis_sql::postgres::{PgPool, PostgresStorage};
use sea_orm::{ActiveModelTrait, ColumnTrait, DatabaseConnection, EntityTrait, QueryFilter, Set};
use std::time::Duration;
use tracing::{error, info};

use super::outbox_publisher::OutboxJob;
use crate::entities::prompt::{self, Entity as Prompt};
use crate::entities::session::{self, Entity as Session, UiStatus};

/// Periodic poller that checks for pending prompts every second
/// and pushes them to the outbox queue for processing
pub async fn run_prompt_poller(db: DatabaseConnection, pool: PgPool) -> anyhow::Result<()> {
    info!("Starting prompt poller - checking every 1 second");

    let mut storage = PostgresStorage::new(pool);

    loop {
        tokio::time::sleep(Duration::from_secs(1)).await;

        match poll_and_enqueue_prompts(&db, &mut storage).await {
            Ok(count) => {
                if count > 0 {
                    info!("Enqueued {} pending prompts for processing", count);
                }
            }
            Err(e) => {
                error!("Failed to poll and enqueue prompts: {}", e);
            }
        }
    }
}

/// Query for prompts that belong to sessions with Pending UI status and push them to the outbox queue
async fn poll_and_enqueue_prompts(
    db: &DatabaseConnection,
    storage: &mut PostgresStorage<OutboxJob>,
) -> anyhow::Result<usize> {
    // Query all sessions with Pending UI status
    let pending_sessions = Session::find()
        .filter(session::Column::UiStatus.eq(UiStatus::Pending))
        .all(db)
        .await?;

    let mut count = 0;

    // Get IP allocator URL from environment
    let ip_allocator_url =
        std::env::var("IP_ALLOCATOR_URL").unwrap_or_else(|_| "http://localhost:8000".to_string());
    let ip_client = ip_allocator_client::Client::new(&ip_allocator_url);

    // Process each pending session
    for session_model in pending_sessions {
        // Find prompts for this session
        let prompts = Prompt::find()
            .filter(prompt::Column::SessionId.eq(session_model.id))
            .all(db)
            .await?;

        if prompts.is_empty() {
            continue;
        }

        // Borrow an IP for this session
        info!(
            "Borrowing IP for session {} with {} prompts",
            session_model.id,
            prompts.len()
        );

        let borrowed_ip = ip_client.handlers_ip_borrow(None).await.map_err(|e| {
            anyhow::anyhow!(
                "Failed to borrow IP for session {}: {}",
                session_model.id,
                e
            )
        })?;

        info!(
            "Successfully borrowed IP for session {}: {:?}",
            session_model.id, borrowed_ip.item
        );

        // Save session_id before moving session_model
        let session_id = session_model.id;

        // Update session's sbx_config with the borrowed IP data (including borrow_token)
        let mut active_session: session::ActiveModel = session_model.into();
        let sbx_config_data = serde_json::json!({
            "item": borrowed_ip.item,
            "borrow_token": borrowed_ip.borrow_token,
        });
        active_session.sbx_config = Set(Some(sbx_config_data));
        active_session.ui_status = Set(UiStatus::InProgress);
        active_session.update(db).await?;

        info!("Updated session {} sbx_config with borrowed IP", session_id);

        // Enqueue each prompt for this session
        for prompt in prompts {
            let job = OutboxJob {
                prompt_id: prompt.id.to_string(),
                payload: serde_json::json!({}),
            };

            storage
                .push(job)
                .await
                .map_err(|e| anyhow::anyhow!("Failed to push job to storage: {}", e))?;

            count += 1;
        }
    }

    Ok(count)
}
