use apalis::prelude::Storage;
use apalis_sql::postgres::{PgPool, PostgresStorage};
use sea_orm::{
    ActiveModelTrait, ActiveValue, ColumnTrait, DatabaseConnection, EntityTrait, QueryFilter, Set,
};
use std::time::Duration;
use tracing::{error, info};
use uuid::Uuid;

use super::outbox_publisher::OutboxJob;
use crate::entities::prompt::{self, Entity as Prompt, InboxStatus};
use crate::entities::session::{self, Entity as Session};

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

/// Query for PENDING prompts and push them to the outbox queue
async fn poll_and_enqueue_prompts(
    db: &DatabaseConnection,
    storage: &mut PostgresStorage<OutboxJob>,
) -> anyhow::Result<usize> {
    // Query all prompts with Pending inbox_status
    let pending_prompts = Prompt::find()
        .filter(prompt::Column::InboxStatus.eq(InboxStatus::Pending))
        .all(db)
        .await?;

    let count = pending_prompts.len();

    // Get IP allocator URL from environment
    let ip_allocator_url =
        std::env::var("IP_ALLOCATOR_URL").unwrap_or_else(|_| "http://localhost:8000".to_string());
    let ip_client = ip_allocator_client::Client::new(&ip_allocator_url);

    // Push each pending prompt to the outbox queue
    for prompt in pending_prompts {
        // Get the session for this prompt
        let session_model = Session::find_by_id(prompt.session_id)
            .one(db)
            .await?
            .ok_or_else(|| anyhow::anyhow!("Session {} not found", prompt.session_id))?;

        // Borrow an IP for this session
        info!(
            "Borrowing IP for session {} before enqueuing prompt {}",
            prompt.session_id, prompt.id
        );

        let borrowed_ip = ip_client.handlers_ip_borrow(None).await.map_err(|e| {
            anyhow::anyhow!(
                "Failed to borrow IP for session {}: {}",
                prompt.session_id,
                e
            )
        })?;

        info!(
            "Successfully borrowed IP for session {}: {:?}",
            prompt.session_id, borrowed_ip.item
        );

        // Update session's sbx_config with the borrowed IP data (including borrow_token)
        let mut active_session: session::ActiveModel = session_model.into();
        let sbx_config_data = serde_json::json!({
            "item": borrowed_ip.item,
            "borrow_token": borrowed_ip.borrow_token,
        });
        active_session.sbx_config = Set(Some(sbx_config_data));
        active_session.status_message = Set(Some("Found Sandbox".to_string()));
        active_session.update(db).await?;

        info!(
            "Updated session {} sbx_config with borrowed IP",
            prompt.session_id
        );

        // Now enqueue the prompt
        let job = OutboxJob {
            prompt_id: prompt.id.to_string(),
            payload: serde_json::json!({}),
        };

        storage
            .push(job)
            .await
            .map_err(|e| anyhow::anyhow!("Failed to push job to storage: {}", e))?;

        // Mark prompt as Active after successfully enqueueing
        update_prompt_status_to_active(db, prompt.id).await?;
    }

    Ok(count)
}

/// Update a prompt's inbox_status to Active
async fn update_prompt_status_to_active(
    db: &DatabaseConnection,
    prompt_id: Uuid,
) -> anyhow::Result<()> {
    let prompt = Prompt::find_by_id(prompt_id)
        .one(db)
        .await?
        .ok_or_else(|| anyhow::anyhow!("Prompt not found"))?;

    let mut active_prompt: prompt::ActiveModel = prompt.into();
    active_prompt.inbox_status = ActiveValue::Set(InboxStatus::Active);
    active_prompt.update(db).await?;

    Ok(())
}
