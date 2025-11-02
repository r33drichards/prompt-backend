use apalis::prelude::*;
use serde::{Deserialize, Serialize};
use tracing::info;

/// Job that reads from PostgreSQL outbox and publishes to Redis
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OutboxJob {
    pub session_id: String,
    pub payload: serde_json::Value,
}

impl Job for OutboxJob {
    const NAME: &'static str = "OutboxJob";
}

/// Process an outbox job: read from PostgreSQL and publish to Redis
pub async fn process_outbox_job(job: OutboxJob) -> Result<(), Error> {
    info!(
        "Processing outbox job for session_id: {}",
        job.session_id
    );

    // TODO: Implement actual logic to:
    // 1. Read from PostgreSQL outbox table
    // 2. Publish to Redis
    // 3. Mark as processed in PostgreSQL

    // For now, just log the job
    info!("Outbox job payload: {:?}", job.payload);

    // Simulate some work
    tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

    info!("Completed outbox job for session_id: {}", job.session_id);

    Ok(())
}
