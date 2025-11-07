use apalis::prelude::*;
use sea_orm::{ActiveModelTrait, DatabaseConnection, EntityTrait, Set};
use serde::{Deserialize, Serialize};
use tracing::{error, info};

use crate::entities::session::{Entity as Session, SessionStatus};

/// Job that returns borrowed IPs for sessions
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IpReturnJob {
    pub session_id: String,
}

impl Job for IpReturnJob {
    const NAME: &'static str = "IpReturnJob";
}

/// Context for the IP returner containing database connection
#[derive(Clone)]
pub struct IpReturnContext {
    pub db: DatabaseConnection,
}

/// Process an IP return job: set sbx_config to null and return the IP to the allocator
pub async fn process_ip_return_job(
    job: IpReturnJob,
    ctx: Data<IpReturnContext>,
) -> Result<(), Error> {
    info!(
        "Processing IP return job for session_id: {}",
        job.session_id
    );

    // Parse session ID from job
    let session_id = uuid::Uuid::parse_str(&job.session_id).map_err(|e| {
        error!("Invalid session ID format: {}", e);
        Error::Failed(Box::new(e))
    })?;

    // Query the session
    let session_model = Session::find_by_id(session_id)
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

    info!("Found session {} for IP return", session_id);

    // Extract the borrowed IP from sbx_config
    let borrowed_ip_json = session_model.sbx_config.as_ref().ok_or_else(|| {
        error!(
            "Session {} has no sbx_config - nothing to return",
            session_id
        );
        Error::Failed("Session missing sbx_config".into())
    })?;

    info!("Returning IP for session {}", session_id);

    // Get IP allocator URL from environment
    let ip_allocator_url =
        std::env::var("IP_ALLOCATOR_URL").unwrap_or_else(|_| "http://localhost:8000".to_string());
    let ip_client = ip_allocator_client::Client::new(&ip_allocator_url);

    // Return the IP
    let return_input = ip_allocator_client::types::ReturnInput {
        item: borrowed_ip_json.clone(),
    };

    if let Err(e) = ip_client.handlers_ip_return_item(&return_input).await {
        error!("Failed to return IP for session {}: {}", session_id, e);
        return Err(Error::Failed(Box::new(e)));
    }

    info!("Successfully returned IP for session {}", session_id);

    // Set sbx_config to null and update session status to Archived
    let mut active_session: crate::entities::session::ActiveModel = session_model.into();
    active_session.sbx_config = Set(None);
    active_session.session_status = Set(SessionStatus::Archived);
    active_session.status_message = Set(Some("IP returned successfully".to_string()));

    active_session.update(&ctx.db).await.map_err(|e| {
        error!(
            "Failed to update session {} after IP return: {}",
            session_id, e
        );
        Error::Failed(Box::new(e))
    })?;

    info!(
        "Updated session {} - set sbx_config to null and status to Archived",
        session_id
    );

    Ok(())
}
