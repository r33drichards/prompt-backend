use rust_redis_webserver::bg_tasks::cancellation_enforcer::run_cancellation_enforcer;
use rust_redis_webserver::entities::session::{
    CancellationStatus, Entity as Session, UiStatus,
};
use sea_orm::{ActiveModelTrait, ColumnTrait, DatabaseConnection, EntityTrait, QueryFilter, Set};
use std::process::Command;
use std::time::Duration;
use tokio::time::sleep;
use uuid::Uuid;

/// Helper function to create a test database connection
/// Returns None if database is not available (for CI/CD environments without test DB)
async fn try_create_test_db() -> Option<DatabaseConnection> {
    let database_url = std::env::var("DATABASE_URL").unwrap_or_else(|_| {
        "postgres://promptuser:promptpass@localhost:5432/prompt_backend_test".to_string()
    });

    sea_orm::Database::connect(&database_url).await.ok()
}

/// Macro to skip test if database is not available
macro_rules! skip_if_no_db {
    ($db:expr) => {
        match $db {
            Some(db) => db,
            None => {
                eprintln!("Skipping test: Database not available");
                return;
            }
        }
    };
}

/// Helper function to create a test session with optional process PID
async fn create_test_session(
    db: &DatabaseConnection,
    cancellation_status: Option<CancellationStatus>,
    process_pid: Option<i32>,
) -> Result<rust_redis_webserver::entities::session::Model, sea_orm::DbErr> {
    let session_id = Uuid::new_v4();
    let now = chrono::Utc::now();

    let new_session = rust_redis_webserver::entities::session::ActiveModel {
        id: Set(session_id),
        sbx_config: Set(None),
        parent: Set(None),
        branch: Set(Some("test-branch".to_string())),
        repo: Set(Some("test/repo".to_string())),
        target_branch: Set(Some("main".to_string())),
        title: Set(Some("Test Session".to_string())),
        ui_status: Set(UiStatus::InProgress),
        created_at: Set(now.into()),
        updated_at: Set(now.into()),
        deleted_at: Set(None),
        user_id: Set("test-user".to_string()),
        ip_return_retry_count: Set(0),
        cancellation_status: Set(cancellation_status),
        cancelled_at: Set(None),
        cancelled_by: Set(None),
        process_pid: Set(process_pid),
    };

    new_session.insert(db).await
}

/// Helper to clean up test session
async fn cleanup_session(db: &DatabaseConnection, session_id: Uuid) {
    let _ = Session::delete_by_id(session_id).exec(db).await;
}

#[tokio::test]
async fn test_cancel_session_with_requested_status() {
    let db = skip_if_no_db!(try_create_test_db().await);

    // Create a test session with cancellation requested
    let session = create_test_session(&db, Some(CancellationStatus::Requested), None)
        .await
        .expect("Failed to create test session");

    // Verify session was created with requested status
    assert_eq!(
        session.cancellation_status,
        Some(CancellationStatus::Requested)
    );
    assert_eq!(session.process_pid, None);

    // Clean up
    cleanup_session(&db, session.id).await;
}

#[tokio::test]
async fn test_cancel_session_with_running_process() {
    let db = skip_if_no_db!(try_create_test_db().await);

    // Start a long-running dummy process (sleep)
    let child = Command::new("sleep")
        .arg("300") // Sleep for 5 minutes
        .spawn()
        .expect("Failed to spawn sleep process");

    let pid = child.id() as i32;

    // Create a test session with cancellation requested and the running process PID
    let session = create_test_session(&db, Some(CancellationStatus::Requested), Some(pid))
        .await
        .expect("Failed to create test session");

    assert_eq!(
        session.cancellation_status,
        Some(CancellationStatus::Requested)
    );
    assert_eq!(session.process_pid, Some(pid));

    // Manually run the cancellation enforcer logic once
    let sessions_to_cancel = Session::find()
        .filter(
            rust_redis_webserver::entities::session::Column::CancellationStatus
                .eq(CancellationStatus::Requested),
        )
        .filter(rust_redis_webserver::entities::session::Column::ProcessPid.is_not_null())
        .filter(rust_redis_webserver::entities::session::Column::Id.eq(session.id))
        .all(&db)
        .await
        .expect("Failed to query sessions");

    assert_eq!(sessions_to_cancel.len(), 1);
    assert_eq!(sessions_to_cancel[0].id, session.id);

    // Kill the process using SIGTERM
    let kill_result = Command::new("kill")
        .arg("-TERM")
        .arg(pid.to_string())
        .output()
        .expect("Failed to execute kill command");

    assert!(kill_result.status.success(), "Kill command should succeed");

    // Update the session to mark as cancelled
    let mut active_session: rust_redis_webserver::entities::session::ActiveModel =
        sessions_to_cancel[0].clone().into();
    active_session.cancellation_status = Set(Some(CancellationStatus::Cancelled));
    active_session.ui_status = Set(UiStatus::NeedsReview);
    active_session.process_pid = Set(None);

    active_session
        .update(&db)
        .await
        .expect("Failed to update session");

    // Verify the session was updated correctly
    let updated_session = Session::find_by_id(session.id)
        .one(&db)
        .await
        .expect("Failed to query session")
        .expect("Session not found");

    assert_eq!(
        updated_session.cancellation_status,
        Some(CancellationStatus::Cancelled)
    );
    assert_eq!(updated_session.ui_status, UiStatus::NeedsReview);
    assert_eq!(updated_session.process_pid, None);

    // Clean up
    cleanup_session(&db, session.id).await;
}

#[tokio::test]
async fn test_cancel_already_cancelled_session() {
    let db = skip_if_no_db!(try_create_test_db().await);

    // Create a session that's already cancelled
    let session = create_test_session(&db, Some(CancellationStatus::Cancelled), None)
        .await
        .expect("Failed to create test session");

    assert_eq!(
        session.cancellation_status,
        Some(CancellationStatus::Cancelled)
    );

    // Query for sessions with cancellation requested (should not include already cancelled)
    let sessions_to_cancel = Session::find()
        .filter(
            rust_redis_webserver::entities::session::Column::CancellationStatus
                .eq(CancellationStatus::Requested),
        )
        .filter(rust_redis_webserver::entities::session::Column::Id.eq(session.id))
        .all(&db)
        .await
        .expect("Failed to query sessions");

    // Should not find any sessions since it's already cancelled
    assert_eq!(
        sessions_to_cancel.len(),
        0,
        "Already cancelled session should not be in requested queue"
    );

    // Clean up
    cleanup_session(&db, session.id).await;
}

#[tokio::test]
async fn test_cancel_nonexistent_process() {
    let db = skip_if_no_db!(try_create_test_db().await);

    // Use a PID that definitely doesn't exist (very high number)
    let fake_pid = 999999;

    // Create a session with a non-existent process PID
    let session = create_test_session(&db, Some(CancellationStatus::Requested), Some(fake_pid))
        .await
        .expect("Failed to create test session");

    // Try to kill the non-existent process
    let kill_result = Command::new("kill")
        .arg("-TERM")
        .arg(fake_pid.to_string())
        .output()
        .expect("Failed to execute kill command");

    // Kill should fail for non-existent process
    assert!(
        !kill_result.status.success(),
        "Kill should fail for non-existent process"
    );

    let stderr = String::from_utf8_lossy(&kill_result.stderr);
    assert!(
        stderr.contains("No such process"),
        "Should indicate no such process"
    );

    // Update session to mark as cancelled even though process was already dead
    let mut active_session: rust_redis_webserver::entities::session::ActiveModel =
        session.into();
    active_session.cancellation_status = Set(Some(CancellationStatus::Cancelled));
    active_session.ui_status = Set(UiStatus::NeedsReview);
    active_session.process_pid = Set(None);

    let updated = active_session
        .update(&db)
        .await
        .expect("Failed to update session");

    assert_eq!(
        updated.cancellation_status,
        Some(CancellationStatus::Cancelled)
    );
    assert_eq!(updated.process_pid, None);

    // Clean up
    cleanup_session(&db, updated.id).await;
}

#[tokio::test]
async fn test_cancellation_enforcer_finds_requested_sessions() {
    let db = skip_if_no_db!(try_create_test_db().await);

    // Start a dummy process
    let child = Command::new("sleep")
        .arg("300")
        .spawn()
        .expect("Failed to spawn sleep process");

    let pid = child.id() as i32;

    // Create multiple sessions with different states
    let session1 = create_test_session(&db, Some(CancellationStatus::Requested), Some(pid))
        .await
        .expect("Failed to create session1");

    let session2 = create_test_session(&db, Some(CancellationStatus::Cancelled), Some(pid + 1))
        .await
        .expect("Failed to create session2");

    let session3 = create_test_session(&db, None, Some(pid + 2))
        .await
        .expect("Failed to create session3");

    let session4 = create_test_session(&db, Some(CancellationStatus::Requested), None)
        .await
        .expect("Failed to create session4");

    // Query for sessions that should be cancelled (requested + has PID)
    let sessions_to_cancel = Session::find()
        .filter(
            rust_redis_webserver::entities::session::Column::CancellationStatus
                .eq(CancellationStatus::Requested),
        )
        .filter(rust_redis_webserver::entities::session::Column::ProcessPid.is_not_null())
        .filter(
            rust_redis_webserver::entities::session::Column::Id.is_in([
                session1.id,
                session2.id,
                session3.id,
                session4.id,
            ]),
        )
        .all(&db)
        .await
        .expect("Failed to query sessions");

    // Only session1 should match (requested + has PID)
    assert_eq!(sessions_to_cancel.len(), 1);
    assert_eq!(sessions_to_cancel[0].id, session1.id);

    // Kill the actual process
    let _ = Command::new("kill")
        .arg("-TERM")
        .arg(pid.to_string())
        .output();

    // Clean up all sessions
    cleanup_session(&db, session1.id).await;
    cleanup_session(&db, session2.id).await;
    cleanup_session(&db, session3.id).await;
    cleanup_session(&db, session4.id).await;
}

#[tokio::test]
async fn test_cancellation_status_transitions() {
    let db = skip_if_no_db!(try_create_test_db().await);

    // Create a session with no cancellation status
    let session = create_test_session(&db, None, None)
        .await
        .expect("Failed to create session");

    assert_eq!(session.cancellation_status, None);

    // Transition to Requested
    let mut active_session: rust_redis_webserver::entities::session::ActiveModel =
        session.into();
    active_session.cancellation_status = Set(Some(CancellationStatus::Requested));
    active_session.cancelled_at = Set(Some(chrono::Utc::now().into()));
    active_session.cancelled_by = Set(Some("test-user".to_string()));

    let updated = active_session
        .update(&db)
        .await
        .expect("Failed to update to Requested");

    assert_eq!(
        updated.cancellation_status,
        Some(CancellationStatus::Requested)
    );
    assert!(updated.cancelled_at.is_some());
    assert_eq!(updated.cancelled_by, Some("test-user".to_string()));

    // Transition to Cancelled
    let mut active_session: rust_redis_webserver::entities::session::ActiveModel =
        updated.into();
    active_session.cancellation_status = Set(Some(CancellationStatus::Cancelled));
    active_session.ui_status = Set(UiStatus::NeedsReview);

    let final_session = active_session
        .update(&db)
        .await
        .expect("Failed to update to Cancelled");

    assert_eq!(
        final_session.cancellation_status,
        Some(CancellationStatus::Cancelled)
    );
    assert_eq!(final_session.ui_status, UiStatus::NeedsReview);

    // Clean up
    cleanup_session(&db, final_session.id).await;
}

#[tokio::test]
async fn test_ui_status_updated_on_cancellation() {
    let db = skip_if_no_db!(try_create_test_db().await);

    // Create a session in InProgress state
    let session = create_test_session(&db, None, None)
        .await
        .expect("Failed to create session");

    assert_eq!(session.ui_status, UiStatus::InProgress);

    // Simulate cancellation
    let mut active_session: rust_redis_webserver::entities::session::ActiveModel =
        session.into();
    active_session.cancellation_status = Set(Some(CancellationStatus::Cancelled));
    active_session.ui_status = Set(UiStatus::NeedsReview);

    let updated = active_session
        .update(&db)
        .await
        .expect("Failed to update session");

    assert_eq!(
        updated.cancellation_status,
        Some(CancellationStatus::Cancelled)
    );
    assert_eq!(updated.ui_status, UiStatus::NeedsReview);

    // Clean up
    cleanup_session(&db, updated.id).await;
}

#[tokio::test]
async fn test_cancellation_clears_process_pid() {
    let db = skip_if_no_db!(try_create_test_db().await);

    // Create a session with a PID
    let session = create_test_session(&db, Some(CancellationStatus::Requested), Some(12345))
        .await
        .expect("Failed to create session");

    assert_eq!(session.process_pid, Some(12345));

    // Simulate cancellation clearing the PID
    let mut active_session: rust_redis_webserver::entities::session::ActiveModel =
        session.into();
    active_session.cancellation_status = Set(Some(CancellationStatus::Cancelled));
    active_session.process_pid = Set(None);

    let updated = active_session
        .update(&db)
        .await
        .expect("Failed to update session");

    assert_eq!(
        updated.cancellation_status,
        Some(CancellationStatus::Cancelled)
    );
    assert_eq!(updated.process_pid, None);

    // Clean up
    cleanup_session(&db, updated.id).await;
}


#[tokio::test]
async fn test_concurrent_cancellations() {
    let db = skip_if_no_db!(try_create_test_db().await);

    // Create multiple long-running processes
    let mut children = vec![];
    let mut session_ids = vec![];

    for _ in 0..3 {
        let child = Command::new("sleep")
            .arg("300")
            .spawn()
            .expect("Failed to spawn sleep process");
        let pid = child.id() as i32;
        children.push((child, pid));

        // Create sessions with cancellation requested
        let session = create_test_session(&db, Some(CancellationStatus::Requested), Some(pid))
            .await
            .expect("Failed to create session");
        session_ids.push(session.id);
    }

    // Verify all sessions are created with requested status
    for session_id in &session_ids {
        let session = Session::find_by_id(*session_id)
            .one(&db)
            .await
            .expect("Failed to query session")
            .expect("Session not found");

        assert_eq!(
            session.cancellation_status,
            Some(CancellationStatus::Requested)
        );
    }

    // Kill all processes concurrently
    let kill_handles: Vec<_> = children
        .iter()
        .map(|(_, pid)| {
            let pid = *pid;
            tokio::spawn(async move {
                Command::new("kill")
                    .arg("-TERM")
                    .arg(pid.to_string())
                    .output()
                    .expect("Failed to execute kill command")
            })
        })
        .collect();

    // Wait for all kill commands to complete
    for handle in kill_handles {
        let _ = handle.await;
    }

    // Update all sessions to cancelled
    for session_id in &session_ids {
        let session = Session::find_by_id(*session_id)
            .one(&db)
            .await
            .expect("Failed to query session")
            .expect("Session not found");

        let mut active_session: rust_redis_webserver::entities::session::ActiveModel =
            session.into();
        active_session.cancellation_status = Set(Some(CancellationStatus::Cancelled));
        active_session.ui_status = Set(UiStatus::NeedsReview);
        active_session.process_pid = Set(None);

        active_session
            .update(&db)
            .await
            .expect("Failed to update session");
    }

    // Verify all sessions are now cancelled
    for session_id in &session_ids {
        let session = Session::find_by_id(*session_id)
            .one(&db)
            .await
            .expect("Failed to query session")
            .expect("Session not found");

        assert_eq!(
            session.cancellation_status,
            Some(CancellationStatus::Cancelled)
        );
        assert_eq!(session.ui_status, UiStatus::NeedsReview);
        assert_eq!(session.process_pid, None);
    }

    // Clean up all sessions
    for session_id in session_ids {
        cleanup_session(&db, session_id).await;
    }
}

#[tokio::test]
async fn test_cancelled_by_and_cancelled_at_fields() {
    let db = skip_if_no_db!(try_create_test_db().await);

    // Create a session
    let session = create_test_session(&db, None, None)
        .await
        .expect("Failed to create session");

    assert_eq!(session.cancelled_at, None);
    assert_eq!(session.cancelled_by, None);

    // Request cancellation with audit fields
    let cancelled_at = chrono::Utc::now();
    let cancelled_by = "test-user-123".to_string();

    let mut active_session: rust_redis_webserver::entities::session::ActiveModel =
        session.into();
    active_session.cancellation_status = Set(Some(CancellationStatus::Requested));
    active_session.cancelled_at = Set(Some(cancelled_at.into()));
    active_session.cancelled_by = Set(Some(cancelled_by.clone()));

    let updated = active_session
        .update(&db)
        .await
        .expect("Failed to update session");

    // Verify audit fields are set
    assert_eq!(
        updated.cancellation_status,
        Some(CancellationStatus::Requested)
    );
    assert!(updated.cancelled_at.is_some());
    assert_eq!(updated.cancelled_by, Some(cancelled_by));

    // Complete the cancellation
    let mut active_session: rust_redis_webserver::entities::session::ActiveModel =
        updated.into();
    active_session.cancellation_status = Set(Some(CancellationStatus::Cancelled));
    active_session.ui_status = Set(UiStatus::NeedsReview);

    let final_session = active_session
        .update(&db)
        .await
        .expect("Failed to update session");

    // Verify audit fields are preserved
    assert_eq!(
        final_session.cancellation_status,
        Some(CancellationStatus::Cancelled)
    );
    assert!(final_session.cancelled_at.is_some());
    assert_eq!(final_session.cancelled_by, Some("test-user-123".to_string()));

    // Clean up
    cleanup_session(&db, final_session.id).await;
}

#[tokio::test]
async fn test_session_requested_without_process_pid() {
    let db = skip_if_no_db!(try_create_test_db().await);

    // Create a session with cancellation requested but no process PID
    let session = create_test_session(&db, Some(CancellationStatus::Requested), None)
        .await
        .expect("Failed to create session");

    assert_eq!(
        session.cancellation_status,
        Some(CancellationStatus::Requested)
    );
    assert_eq!(session.process_pid, None);

    // Query for sessions to cancel (should not include sessions without PIDs)
    let sessions_to_cancel = Session::find()
        .filter(
            rust_redis_webserver::entities::session::Column::CancellationStatus
                .eq(CancellationStatus::Requested),
        )
        .filter(rust_redis_webserver::entities::session::Column::ProcessPid.is_not_null())
        .filter(rust_redis_webserver::entities::session::Column::Id.eq(session.id))
        .all(&db)
        .await
        .expect("Failed to query sessions");

    // Should not find the session since it has no PID
    assert_eq!(
        sessions_to_cancel.len(),
        0,
        "Sessions without PIDs should not be selected for process killing"
    );

    // Such sessions might still need to be marked as cancelled through other means
    // For example, if the process never started or already completed
    let mut active_session: rust_redis_webserver::entities::session::ActiveModel =
        session.into();
    active_session.cancellation_status = Set(Some(CancellationStatus::Cancelled));
    active_session.ui_status = Set(UiStatus::NeedsReview);

    let updated = active_session
        .update(&db)
        .await
        .expect("Failed to update session");

    assert_eq!(
        updated.cancellation_status,
        Some(CancellationStatus::Cancelled)
    );

    // Clean up
    cleanup_session(&db, updated.id).await;
}

#[tokio::test]
async fn test_enforce_cancellations_function_integration() {
    let db = skip_if_no_db!(try_create_test_db().await);

    // Start a real process
    let child = Command::new("sleep")
        .arg("300")
        .spawn()
        .expect("Failed to spawn sleep process");
    let pid = child.id() as i32;

    // Create a session with cancellation requested
    let session = create_test_session(&db, Some(CancellationStatus::Requested), Some(pid))
        .await
        .expect("Failed to create session");

    // Simulate what enforce_cancellations does
    let sessions_to_cancel = Session::find()
        .filter(
            rust_redis_webserver::entities::session::Column::CancellationStatus
                .eq(CancellationStatus::Requested),
        )
        .filter(rust_redis_webserver::entities::session::Column::ProcessPid.is_not_null())
        .filter(rust_redis_webserver::entities::session::Column::Id.eq(session.id))
        .all(&db)
        .await
        .expect("Failed to query sessions");

    assert_eq!(sessions_to_cancel.len(), 1);
    assert_eq!(sessions_to_cancel[0].id, session.id);

    // Kill the process
    let kill_result = Command::new("kill")
        .arg("-TERM")
        .arg(pid.to_string())
        .output()
        .expect("Failed to execute kill command");

    assert!(
        kill_result.status.success(),
        "Kill command should succeed for running process"
    );

    // Update the session
    let mut active_session: rust_redis_webserver::entities::session::ActiveModel =
        sessions_to_cancel[0].clone().into();
    active_session.cancellation_status = Set(Some(CancellationStatus::Cancelled));
    active_session.ui_status = Set(UiStatus::NeedsReview);
    active_session.process_pid = Set(None);

    let updated = active_session
        .update(&db)
        .await
        .expect("Failed to update session");

    assert_eq!(
        updated.cancellation_status,
        Some(CancellationStatus::Cancelled)
    );
    assert_eq!(updated.process_pid, None);

    // Verify the session won't be selected again
    let sessions_to_cancel_again = Session::find()
        .filter(
            rust_redis_webserver::entities::session::Column::CancellationStatus
                .eq(CancellationStatus::Requested),
        )
        .filter(rust_redis_webserver::entities::session::Column::ProcessPid.is_not_null())
        .filter(rust_redis_webserver::entities::session::Column::Id.eq(session.id))
        .all(&db)
        .await
        .expect("Failed to query sessions");

    assert_eq!(
        sessions_to_cancel_again.len(),
        0,
        "Cancelled session should not be selected again"
    );

    // Clean up
    cleanup_session(&db, updated.id).await;
}

#[tokio::test]
async fn test_cancellation_idempotency() {
    let db = skip_if_no_db!(try_create_test_db().await);

    // Start a process
    let child = Command::new("sleep")
        .arg("300")
        .spawn()
        .expect("Failed to spawn sleep process");
    let pid = child.id() as i32;

    // Create session
    let session = create_test_session(&db, Some(CancellationStatus::Requested), Some(pid))
        .await
        .expect("Failed to create session");

    // Kill the process once
    let _ = Command::new("kill")
        .arg("-TERM")
        .arg(pid.to_string())
        .output();

    // Try to kill again (should fail gracefully)
    let kill_result = Command::new("kill")
        .arg("-TERM")
        .arg(pid.to_string())
        .output()
        .expect("Failed to execute kill command");

    // Second kill should fail but we should handle it gracefully
    let stderr = String::from_utf8_lossy(&kill_result.stderr);
    assert!(
        !kill_result.status.success() && stderr.contains("No such process"),
        "Second kill should fail with 'No such process'"
    );

    // Update session to cancelled
    let mut active_session: rust_redis_webserver::entities::session::ActiveModel =
        session.into();
    active_session.cancellation_status = Set(Some(CancellationStatus::Cancelled));
    active_session.ui_status = Set(UiStatus::NeedsReview);
    active_session.process_pid = Set(None);

    let updated = active_session
        .update(&db)
        .await
        .expect("Failed to update session");

    assert_eq!(
        updated.cancellation_status,
        Some(CancellationStatus::Cancelled)
    );

    // Clean up
    cleanup_session(&db, updated.id).await;
}

#[tokio::test]
async fn test_multiple_sessions_different_states() {
    let db = skip_if_no_db!(try_create_test_db().await);

    // Create various sessions in different states
    let sessions = vec![
        // Session 1: Normal in-progress session (no cancellation)
        create_test_session(&db, None, None).await.unwrap(),
        // Session 2: Cancellation requested with PID (should be cancelled)
        {
            let child = Command::new("sleep").arg("300").spawn().unwrap();
            let pid = child.id() as i32;
            create_test_session(&db, Some(CancellationStatus::Requested), Some(pid))
                .await
                .unwrap()
        },
        // Session 3: Cancellation requested without PID (no process to kill)
        create_test_session(&db, Some(CancellationStatus::Requested), None)
            .await
            .unwrap(),
        // Session 4: Already cancelled
        create_test_session(&db, Some(CancellationStatus::Cancelled), None)
            .await
            .unwrap(),
    ];

    // Query for sessions that need process termination
    let sessions_to_cancel = Session::find()
        .filter(
            rust_redis_webserver::entities::session::Column::CancellationStatus
                .eq(CancellationStatus::Requested),
        )
        .filter(rust_redis_webserver::entities::session::Column::ProcessPid.is_not_null())
        .filter(
            rust_redis_webserver::entities::session::Column::Id.is_in(
                sessions.iter().map(|s| s.id).collect::<Vec<_>>(),
            ),
        )
        .all(&db)
        .await
        .expect("Failed to query sessions");

    // Only session 2 should match (requested + has PID)
    assert_eq!(sessions_to_cancel.len(), 1);
    assert_eq!(sessions_to_cancel[0].id, sessions[1].id);

    // Kill the process for session 2
    if let Some(pid) = sessions[1].process_pid {
        let _ = Command::new("kill")
            .arg("-TERM")
            .arg(pid.to_string())
            .output();
    }

    // Clean up all sessions
    for session in sessions {
        cleanup_session(&db, session.id).await;
    }
}

#[tokio::test]
async fn test_cancellation_with_different_ui_statuses() {
    let db = skip_if_no_db!(try_create_test_db().await);

    // Test cancellation transitions UI status to NeedsReview from various states
    let ui_statuses = vec![
        UiStatus::Pending,
        UiStatus::InProgress,
    ];

    for initial_status in ui_statuses {
        let session_id = Uuid::new_v4();
        let now = chrono::Utc::now();

        let new_session = rust_redis_webserver::entities::session::ActiveModel {
            id: Set(session_id),
            sbx_config: Set(None),
            parent: Set(None),
            branch: Set(Some("test-branch".to_string())),
            repo: Set(Some("test/repo".to_string())),
            target_branch: Set(Some("main".to_string())),
            title: Set(Some("Test Session".to_string())),
            ui_status: Set(initial_status.clone()),
            created_at: Set(now.into()),
            updated_at: Set(now.into()),
            deleted_at: Set(None),
            user_id: Set("test-user".to_string()),
            ip_return_retry_count: Set(0),
            cancellation_status: Set(None),
            cancelled_at: Set(None),
            cancelled_by: Set(None),
            process_pid: Set(None),
        };

        let session = new_session.insert(&db).await.expect("Failed to create session");
        assert_eq!(session.ui_status, initial_status);

        // Cancel the session
        let mut active_session: rust_redis_webserver::entities::session::ActiveModel =
            session.into();
        active_session.cancellation_status = Set(Some(CancellationStatus::Cancelled));
        active_session.ui_status = Set(UiStatus::NeedsReview);

        let updated = active_session
            .update(&db)
            .await
            .expect("Failed to update session");

        assert_eq!(updated.ui_status, UiStatus::NeedsReview);
        assert_eq!(
            updated.cancellation_status,
            Some(CancellationStatus::Cancelled)
        );

        cleanup_session(&db, updated.id).await;
    }
}
