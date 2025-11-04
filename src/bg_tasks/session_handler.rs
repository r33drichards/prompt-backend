use apalis::prelude::*;
use serde::{Deserialize, Serialize};
use tracing::info;

/// Job that reads from Redis and processes session data
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionJob {
    pub session_id: String,
    pub action: String,
    pub data: serde_json::Value,
}

impl Job for SessionJob {
    const NAME: &'static str = "SessionJob";
}

/// Process a session job: read from Redis and do work
pub async fn process_session_job(job: SessionJob) -> Result<(), Error> {
    info!(
        "Processing session job for session_id: {}, action: {}",
        job.session_id, job.action
    );

    // TODO: Implement actual logic to:
    // 1. Read session data from Redis
    // 2. Process the session based on action
    // 3. Update state as needed

    // For now, just log the job
    info!("Session job data: {:?}", job.data);

    // Simulate some work
    tokio::time::sleep(tokio::time::Duration::from_millis(200)).await;

    info!("Completed session job for session_id: {}", job.session_id);

    Ok(())
}
