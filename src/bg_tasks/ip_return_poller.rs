use sea_orm::{ActiveModelTrait, ColumnTrait, DatabaseConnection, EntityTrait, QueryFilter, Set};
use std::time::Duration;
use tracing::{error, info, warn};

use crate::entities::session::{self, Entity as Session, SessionStatus};

/// Periodic poller that checks for sessions in ReturningIp status every 5 seconds
/// and returns their IPs to the allocator
pub async fn run_ip_return_poller(db: DatabaseConnection) -> anyhow::Result<()> {
    info!("Starting IP return poller - checking every 5 seconds");

    loop {
        tokio::time::sleep(Duration::from_secs(5)).await;

        match poll_and_return_ips(&db).await {
            Ok(count) => {
                if count > 0 {
                    info!("Processed {} sessions for IP return", count);
                }
            }
            Err(e) => {
                error!("Failed to poll and return IPs: {}", e);
            }
        }
    }
}

/// Query for sessions in ReturningIp status and return their IPs
async fn poll_and_return_ips(db: &DatabaseConnection) -> anyhow::Result<usize> {
    // Query all sessions with ReturningIp status that still have sbx_config
    let returning_sessions = Session::find()
        .filter(session::Column::SessionStatus.eq(SessionStatus::ReturningIp))
        .filter(session::Column::SbxConfig.is_not_null())
        .all(db)
        .await?;

    let count = returning_sessions.len();

    // Get IP allocator URL from environment
    let ip_allocator_url =
        std::env::var("IP_ALLOCATOR_URL").unwrap_or_else(|_| "http://localhost:8000".to_string());
    let ip_client = ip_allocator_client::Client::new(&ip_allocator_url);

    // Process each session
    for session in returning_sessions {
        let session_id = session.id;

        // Extract the borrowed IP and token from sbx_config
        let (item, borrow_token) = match &session.sbx_config {
            Some(config) => {
                let item = config
                    .get("item")
                    .cloned()
                    .unwrap_or_else(|| config.clone());
                let borrow_token = config
                    .get("borrow_token")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string();
                (item, borrow_token)
            }
            None => {
                warn!(
                    "Session {} in ReturningIp status but sbx_config is None, archiving anyway",
                    session_id
                );
                // Archive the session without returning IP
                archive_session(db, session).await?;
                continue;
            }
        };

        info!("Returning IP for session {}", session_id);

        // Return the IP
        let return_input = ip_allocator_client::types::ReturnInput { item, borrow_token };

        match ip_client.handlers_ip_return_item(&return_input).await {
            Ok(_) => {
                info!("Successfully returned IP for session {}", session_id);

                // Set sbx_config to null and update session status to Archived
                let mut active_session: session::ActiveModel = session.into();
                active_session.sbx_config = Set(None);
                active_session.session_status = Set(SessionStatus::Archived);
                active_session.status_message = Set(Some("IP returned successfully".to_string()));

                if let Err(e) = active_session.update(db).await {
                    error!(
                        "Failed to update session {} after IP return: {}",
                        session_id, e
                    );
                    // Continue processing other sessions
                } else {
                    info!(
                        "Updated session {} - set sbx_config to null and status to Archived",
                        session_id
                    );
                }
            }
            Err(e) => {
                error!("Failed to return IP for session {}: {}", session_id, e);
                // Will retry on next poll cycle
            }
        }
    }

    Ok(count)
}

/// Archive a session without returning IP (when sbx_config is already None)
async fn archive_session(db: &DatabaseConnection, session: session::Model) -> anyhow::Result<()> {
    let session_id = session.id;
    let mut active_session: session::ActiveModel = session.into();
    active_session.sbx_config = Set(None);
    active_session.session_status = Set(SessionStatus::Archived);
    active_session.status_message = Set(Some("Archived (no IP to return)".to_string()));

    active_session.update(db).await?;
    info!("Archived session {} without IP return", session_id);

    Ok(())
}
