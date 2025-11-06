use apalis::prelude::Storage;
use apalis_sql::postgres::{PgPool, PostgresStorage};
use sea_orm::{
    ActiveModelTrait, ActiveValue, ColumnTrait, DatabaseConnection, EntityTrait, QueryFilter,
};
use std::time::Duration;
use tracing::{error, info};
use uuid::Uuid;

use super::outbox_publisher::OutboxJob;
use crate::entities::prompt::{self, Entity as Prompt, InboxStatus};

/// Periodic poller that checks for active prompts every second
/// and pushes them to the outbox queue for processing
pub async fn run_session_poller(db: DatabaseConnection, pool: PgPool) -> anyhow::Result<()> {
    info!("Starting session poller - checking every 1 second");

    let mut storage = PostgresStorage::new(pool);

    loop {
        tokio::time::sleep(Duration::from_secs(1)).await;

        match poll_and_enqueue_prompts(&db, &mut storage).await {
            Ok(count) => {
                if count > 0 {
                    info!("Enqueued {} active prompts for processing", count);
                }
            }
            Err(e) => {
                error!("Failed to poll and enqueue prompts: {}", e);
            }
        }
    }
}

/// Query for ACTIVE prompts and push them to the outbox queue
async fn poll_and_enqueue_prompts(
    db: &DatabaseConnection,
    storage: &mut PostgresStorage<OutboxJob>,
) -> anyhow::Result<usize> {
    // Query all prompts with Active inbox_status
    let active_prompts = Prompt::find()
        .filter(prompt::Column::InboxStatus.eq(InboxStatus::Active))
        .all(db)
        .await?;

    let count = active_prompts.len();

    // Push each active prompt to the outbox queue
    for prompt in active_prompts {
        let job = OutboxJob {
            prompt_id: prompt.id.to_string(),
            payload: serde_json::json!({}),
        };

        storage
            .push(job)
            .await
            .map_err(|e| anyhow::anyhow!("Failed to push job to storage: {}", e))?;

        // Mark prompt as Pending after successfully enqueueing
        update_prompt_status_to_pending(db, prompt.id).await?;
    }

    Ok(count)
}

/// Update a prompt's inbox_status to Pending
async fn update_prompt_status_to_pending(
    db: &DatabaseConnection,
    prompt_id: Uuid,
) -> anyhow::Result<()> {
    let prompt = Prompt::find_by_id(prompt_id)
        .one(db)
        .await?
        .ok_or_else(|| anyhow::anyhow!("Prompt not found"))?;

    let mut active_prompt: prompt::ActiveModel = prompt.into();
    active_prompt.inbox_status = ActiveValue::Set(InboxStatus::Pending);
    active_prompt.update(db).await?;

    Ok(())
}
