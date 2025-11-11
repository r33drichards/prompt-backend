use sea_orm::{ActiveModelTrait, ColumnTrait, DatabaseConnection, EntityTrait, QueryFilter, Set};
use std::time::Duration;
use tracing::{error, info, warn};

use crate::entities::session::{self, CancellationStatus, Entity as Session, UiStatus};

/// Periodic poller that checks for sessions with cancellation requested
/// and running processes, then kills those processes
pub async fn run_cancellation_enforcer(db: DatabaseConnection) -> anyhow::Result<()> {
    info!("Starting cancellation enforcer - checking every 2 seconds");

    loop {
        tokio::time::sleep(Duration::from_secs(2)).await;

        match enforce_cancellations(&db).await {
            Ok(count) => {
                if count > 0 {
                    info!("Killed {} running processes for cancelled sessions", count);
                }
            }
            Err(e) => {
                error!("Failed to enforce cancellations: {}", e);
            }
        }
    }
}

/// Find sessions with cancellation requested and a running process, then kill those processes
async fn enforce_cancellations(db: &DatabaseConnection) -> anyhow::Result<usize> {
    // Query all sessions with cancellation requested and a process PID
    let sessions_to_cancel = Session::find()
        .filter(session::Column::CancellationStatus.eq(CancellationStatus::Requested))
        .filter(session::Column::ProcessPid.is_not_null())
        .all(db)
        .await?;

    let mut count = 0;

    for session_model in sessions_to_cancel {
        let session_id = session_model.id;
        let pid = match session_model.process_pid {
            Some(p) => p,
            None => {
                warn!(
                    "Session {} has cancellation requested but no PID - skipping",
                    session_id
                );
                continue;
            }
        };

        info!(
            "Attempting to kill process {} for cancelled session {}",
            pid, session_id
        );

        // Kill the process using the kill command
        // First try SIGTERM (graceful shutdown)
        let kill_result = std::process::Command::new("kill")
            .arg("-TERM")
            .arg(pid.to_string())
            .output();

        match kill_result {
            Ok(output) if output.status.success() => {
                info!(
                    "Successfully sent SIGTERM to process {} for session {}",
                    pid, session_id
                );
                count += 1;

                // Update session to mark as cancelled and clear PID
                let mut active_session: session::ActiveModel = session_model.into();
                active_session.cancellation_status = Set(Some(CancellationStatus::Cancelled));
                active_session.ui_status = Set(UiStatus::NeedsReview);
                active_session.process_pid = Set(None);

                if let Err(e) = active_session.update(db).await {
                    error!(
                        "Failed to update session {} after killing process: {}",
                        session_id, e
                    );
                } else {
                    info!(
                        "Session {} marked as cancelled after killing process {}",
                        session_id, pid
                    );
                }
            }
            Ok(output) => {
                // Check stderr for "No such process" error
                let stderr = String::from_utf8_lossy(&output.stderr);
                if stderr.contains("No such process") {
                    info!(
                        "Process {} for session {} already terminated",
                        pid, session_id
                    );

                    // Update session anyway to clear the PID and mark as cancelled
                    let mut active_session: session::ActiveModel = session_model.into();
                    active_session.cancellation_status = Set(Some(CancellationStatus::Cancelled));
                    active_session.ui_status = Set(UiStatus::NeedsReview);
                    active_session.process_pid = Set(None);

                    if let Err(e) = active_session.update(db).await {
                        error!(
                            "Failed to update session {} after process already dead: {}",
                            session_id, e
                        );
                    } else {
                        info!(
                            "Session {} marked as cancelled (process was already dead)",
                            session_id
                        );
                    }
                    count += 1;
                } else {
                    warn!(
                        "Failed to kill process {} for session {}: {}",
                        pid, session_id, stderr
                    );
                }
            }
            Err(e) => {
                error!(
                    "Failed to execute kill command for process {} (session {}): {}",
                    pid, session_id, e
                );
            }
        }
    }

    Ok(count)
}
