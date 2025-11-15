use chrono::Utc;
use rust_redis_webserver::entities::session::{
    CancellationStatus, Entity as Session, UiStatus, Model as SessionModel,
};
use sea_orm::{ActiveModelTrait, ColumnTrait, DatabaseConnection, EntityTrait, NotSet, QueryFilter, Set};
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

/// Helper function to create a test session
async fn create_test_session(
    db: &DatabaseConnection,
    user_id: &str,
    process_pid: Option<i32>,
) -> Result<SessionModel, sea_orm::DbErr> {
    let session_id = Uuid::new_v4();
    let new_session = rust_redis_webserver::entities::session::ActiveModel {
        id: Set(session_id),
        sbx_config: Set(None),
        parent: Set(None),
        branch: Set(Some(format!("test-branch-{}", session_id))),
        repo: Set(Some("test/repo".to_string())),
        target_branch: Set(Some("main".to_string())),
        title: Set(Some("Test Session".to_string())),
        ui_status: Set(UiStatus::InProgress),
        user_id: Set(user_id.to_string()),
        ip_return_retry_count: Set(0),
        created_at: NotSet,
        updated_at: NotSet,
        deleted_at: Set(None),
        cancellation_status: Set(None),
        cancelled_at: Set(None),
        cancelled_by: Set(None),
        process_pid: Set(process_pid),
    };

    new_session.insert(db).await
}

/// Helper function to cleanup test session
async fn cleanup_session(db: &DatabaseConnection, session_id: Uuid) {
    let _ = Session::delete_by_id(session_id).exec(db).await;
}

#[tokio::test]
async fn test_cancel_session_marks_as_requested() {
    let db = skip_if_no_db!(try_create_test_db().await);
    let user_id = "test-user-1";

    // Create a test session with a mock process PID
    let session = create_test_session(&db, user_id, Some(99999))
        .await
        .expect("Failed to create test session");

    // Verify initial state
    assert_eq!(session.cancellation_status, None);
    assert_eq!(session.cancelled_at, None);
    assert_eq!(session.cancelled_by, None);

    // Simulate cancellation request by updating the session
    let mut active_session: rust_redis_webserver::entities::session::ActiveModel = session.into();
    active_session.cancellation_status = Set(Some(CancellationStatus::Requested));
    active_session.cancelled_at = Set(Some(Utc::now().into()));
    active_session.cancelled_by = Set(Some(user_id.to_string()));

    let updated_session = active_session
        .update(&db)
        .await
        .expect("Failed to update session");

    // Verify cancellation was requested
    assert_eq!(
        updated_session.cancellation_status,
        Some(CancellationStatus::Requested)
    );
    assert!(updated_session.cancelled_at.is_some());
    assert_eq!(updated_session.cancelled_by.as_deref(), Some(user_id));
    assert_eq!(updated_session.process_pid, Some(99999));

    // Cleanup
    cleanup_session(&db, updated_session.id).await;
}

#[tokio::test]
async fn test_cancel_session_without_process_pid() {
    let db = skip_if_no_db!(try_create_test_db().await);
    let user_id = "test-user-2";

    // Create a test session without a process PID
    let session = create_test_session(&db, user_id, None)
        .await
        .expect("Failed to create test session");

    assert_eq!(session.process_pid, None);

    // Request cancellation
    let mut active_session: rust_redis_webserver::entities::session::ActiveModel = session.into();
    active_session.cancellation_status = Set(Some(CancellationStatus::Requested));
    active_session.cancelled_at = Set(Some(Utc::now().into()));
    active_session.cancelled_by = Set(Some(user_id.to_string()));

    let updated_session = active_session
        .update(&db)
        .await
        .expect("Failed to update session");

    // Verify cancellation was requested even without PID
    assert_eq!(
        updated_session.cancellation_status,
        Some(CancellationStatus::Requested)
    );
    assert!(updated_session.cancelled_at.is_some());
    assert_eq!(updated_session.process_pid, None);

    // Cleanup
    cleanup_session(&db, updated_session.id).await;
}

#[tokio::test]
async fn test_cancellation_enforcer_query() {
    let db = skip_if_no_db!(try_create_test_db().await);
    let user_id = "test-user-3";

    // Create a session with cancellation requested and a PID
    let session = create_test_session(&db, user_id, Some(88888))
        .await
        .expect("Failed to create test session");

    let mut active_session: rust_redis_webserver::entities::session::ActiveModel = session.into();
    active_session.cancellation_status = Set(Some(CancellationStatus::Requested));
    active_session.cancelled_at = Set(Some(Utc::now().into()));
    active_session.cancelled_by = Set(Some(user_id.to_string()));

    let updated_session = active_session
        .update(&db)
        .await
        .expect("Failed to update session");

    // Query sessions that should be picked up by cancellation enforcer
    let sessions_to_cancel = Session::find()
        .filter(
            rust_redis_webserver::entities::session::Column::CancellationStatus
                .eq(CancellationStatus::Requested),
        )
        .filter(rust_redis_webserver::entities::session::Column::ProcessPid.is_not_null())
        .all(&db)
        .await
        .expect("Failed to query sessions");

    // Verify our session is in the result set
    let found = sessions_to_cancel
        .iter()
        .any(|s| s.id == updated_session.id);
    assert!(
        found,
        "Session should be found by cancellation enforcer query"
    );

    // Cleanup
    cleanup_session(&db, updated_session.id).await;
}

#[tokio::test]
async fn test_cancel_already_cancelled_session() {
    let db = skip_if_no_db!(try_create_test_db().await);
    let user_id = "test-user-4";

    // Create a session that's already cancelled
    let session = create_test_session(&db, user_id, None)
        .await
        .expect("Failed to create test session");

    let mut active_session: rust_redis_webserver::entities::session::ActiveModel = session.into();
    active_session.cancellation_status = Set(Some(CancellationStatus::Cancelled));
    active_session.cancelled_at = Set(Some(Utc::now().into()));
    active_session.cancelled_by = Set(Some(user_id.to_string()));

    let cancelled_session = active_session
        .update(&db)
        .await
        .expect("Failed to update session");

    // Verify it's already cancelled
    assert_eq!(
        cancelled_session.cancellation_status,
        Some(CancellationStatus::Cancelled)
    );

    // Attempting to cancel again should recognize it's already cancelled
    // This simulates the handler's check
    if let Some(CancellationStatus::Cancelled) = cancelled_session.cancellation_status {
        // This is the expected path - session is already cancelled
        assert!(true, "Session correctly identified as already cancelled");
    } else {
        panic!("Session should be marked as Cancelled");
    }

    // Cleanup
    cleanup_session(&db, cancelled_session.id).await;
}

#[tokio::test]
async fn test_cancellation_enforcer_marks_as_cancelled() {
    let db = skip_if_no_db!(try_create_test_db().await);
    let user_id = "test-user-5";

    // Create a session with cancellation requested
    let session = create_test_session(&db, user_id, Some(77777))
        .await
        .expect("Failed to create test session");

    let mut active_session: rust_redis_webserver::entities::session::ActiveModel = session.into();
    active_session.cancellation_status = Set(Some(CancellationStatus::Requested));
    active_session.cancelled_at = Set(Some(Utc::now().into()));
    active_session.cancelled_by = Set(Some(user_id.to_string()));

    let requested_session = active_session
        .update(&db)
        .await
        .expect("Failed to update session");

    // Simulate what the cancellation enforcer does after killing the process
    let mut active_session: rust_redis_webserver::entities::session::ActiveModel =
        requested_session.into();
    active_session.cancellation_status = Set(Some(CancellationStatus::Cancelled));
    active_session.ui_status = Set(UiStatus::NeedsReview);
    active_session.process_pid = Set(None); // Clear PID after killing

    let cancelled_session = active_session
        .update(&db)
        .await
        .expect("Failed to update session");

    // Verify final state
    assert_eq!(
        cancelled_session.cancellation_status,
        Some(CancellationStatus::Cancelled)
    );
    assert_eq!(cancelled_session.ui_status, UiStatus::NeedsReview);
    assert_eq!(cancelled_session.process_pid, None);

    // Cleanup
    cleanup_session(&db, cancelled_session.id).await;
}

#[tokio::test]
async fn test_multiple_sessions_cancellation() {
    let db = skip_if_no_db!(try_create_test_db().await);
    let user_id = "test-user-6";

    // Create multiple sessions
    let session1 = create_test_session(&db, user_id, Some(11111))
        .await
        .expect("Failed to create session 1");
    let session2 = create_test_session(&db, user_id, Some(22222))
        .await
        .expect("Failed to create session 2");
    let session3 = create_test_session(&db, user_id, None)
        .await
        .expect("Failed to create session 3");

    // Mark session1 and session2 for cancellation (with PIDs)
    let mut active_session1: rust_redis_webserver::entities::session::ActiveModel =
        session1.into();
    active_session1.cancellation_status = Set(Some(CancellationStatus::Requested));
    let updated1 = active_session1.update(&db).await.expect("Failed to update");

    let mut active_session2: rust_redis_webserver::entities::session::ActiveModel =
        session2.into();
    active_session2.cancellation_status = Set(Some(CancellationStatus::Requested));
    let updated2 = active_session2.update(&db).await.expect("Failed to update");

    // Mark session3 for cancellation (without PID)
    let mut active_session3: rust_redis_webserver::entities::session::ActiveModel =
        session3.into();
    active_session3.cancellation_status = Set(Some(CancellationStatus::Requested));
    let updated3 = active_session3.update(&db).await.expect("Failed to update");

    // Query for sessions the enforcer would process
    let sessions_to_cancel = Session::find()
        .filter(
            rust_redis_webserver::entities::session::Column::CancellationStatus
                .eq(CancellationStatus::Requested),
        )
        .filter(rust_redis_webserver::entities::session::Column::ProcessPid.is_not_null())
        .all(&db)
        .await
        .expect("Failed to query sessions");

    // Should find sessions 1 and 2, but not 3 (no PID)
    let ids: Vec<Uuid> = sessions_to_cancel.iter().map(|s| s.id).collect();
    assert!(ids.contains(&updated1.id), "Session 1 should be found");
    assert!(ids.contains(&updated2.id), "Session 2 should be found");
    assert!(
        !ids.contains(&updated3.id),
        "Session 3 should not be found (no PID)"
    );

    // Cleanup
    cleanup_session(&db, updated1.id).await;
    cleanup_session(&db, updated2.id).await;
    cleanup_session(&db, updated3.id).await;
}

#[tokio::test]
async fn test_cancellation_state_transitions() {
    let db = skip_if_no_db!(try_create_test_db().await);
    let user_id = "test-user-7";

    // Create session
    let session = create_test_session(&db, user_id, Some(55555))
        .await
        .expect("Failed to create session");

    // State 1: Initial state (no cancellation)
    assert_eq!(session.cancellation_status, None);
    assert_eq!(session.ui_status, UiStatus::InProgress);

    // State 2: Cancellation requested
    let mut active: rust_redis_webserver::entities::session::ActiveModel = session.into();
    active.cancellation_status = Set(Some(CancellationStatus::Requested));
    active.cancelled_at = Set(Some(Utc::now().into()));
    active.cancelled_by = Set(Some(user_id.to_string()));
    let requested = active.update(&db).await.expect("Failed to update");

    assert_eq!(
        requested.cancellation_status,
        Some(CancellationStatus::Requested)
    );
    assert!(requested.cancelled_at.is_some());
    assert_eq!(requested.process_pid, Some(55555));

    // State 3: Cancelled (after enforcer kills process)
    let mut active: rust_redis_webserver::entities::session::ActiveModel = requested.into();
    active.cancellation_status = Set(Some(CancellationStatus::Cancelled));
    active.ui_status = Set(UiStatus::NeedsReview);
    active.process_pid = Set(None);
    let cancelled = active.update(&db).await.expect("Failed to update");

    assert_eq!(
        cancelled.cancellation_status,
        Some(CancellationStatus::Cancelled)
    );
    assert_eq!(cancelled.ui_status, UiStatus::NeedsReview);
    assert_eq!(cancelled.process_pid, None);

    // Cleanup
    cleanup_session(&db, cancelled.id).await;
}

#[tokio::test]
async fn test_cancellation_preserves_metadata() {
    let db = skip_if_no_db!(try_create_test_db().await);
    let user_id = "test-user-8";

    // Create a session with specific metadata
    let session_id = Uuid::new_v4();
    let repo = "owner/test-repo";
    let branch = "feature/test-branch";
    let title = "Important Test Session";

    let new_session = rust_redis_webserver::entities::session::ActiveModel {
        id: Set(session_id),
        sbx_config: Set(Some(serde_json::json!({"test": "config"}))),
        parent: Set(None),
        branch: Set(Some(branch.to_string())),
        repo: Set(Some(repo.to_string())),
        target_branch: Set(Some("main".to_string())),
        title: Set(Some(title.to_string())),
        ui_status: Set(UiStatus::InProgress),
        user_id: Set(user_id.to_string()),
        ip_return_retry_count: Set(0),
        created_at: NotSet,
        updated_at: NotSet,
        deleted_at: Set(None),
        cancellation_status: Set(None),
        cancelled_at: Set(None),
        cancelled_by: Set(None),
        process_pid: Set(Some(44444)),
    };

    let session = new_session
        .insert(&db)
        .await
        .expect("Failed to create session");

    // Cancel the session
    let mut active: rust_redis_webserver::entities::session::ActiveModel = session.into();
    active.cancellation_status = Set(Some(CancellationStatus::Cancelled));
    active.ui_status = Set(UiStatus::NeedsReview);
    active.process_pid = Set(None);
    let cancelled = active.update(&db).await.expect("Failed to update");

    // Verify metadata is preserved
    assert_eq!(cancelled.repo.as_deref(), Some(repo));
    assert_eq!(cancelled.branch.as_deref(), Some(branch));
    assert_eq!(cancelled.title.as_deref(), Some(title));
    assert_eq!(cancelled.target_branch.as_deref(), Some("main"));
    assert!(cancelled.sbx_config.is_some());

    // Cleanup
    cleanup_session(&db, cancelled.id).await;
}

#[test]
fn test_cancellation_status_enum_values() {
    // Unit test to verify CancellationStatus enum values
    use std::mem::discriminant;

    // Ensure both statuses are distinct
    assert_ne!(
        discriminant(&CancellationStatus::Requested),
        discriminant(&CancellationStatus::Cancelled)
    );
}

#[tokio::test]
async fn test_query_sessions_by_user_and_cancellation_status() {
    let db = skip_if_no_db!(try_create_test_db().await);
    let user1 = "test-user-9";
    let user2 = "test-user-10";

    // Create sessions for different users
    let session1 = create_test_session(&db, user1, Some(11111))
        .await
        .expect("Failed to create session 1");
    let session2 = create_test_session(&db, user2, Some(22222))
        .await
        .expect("Failed to create session 2");

    // Mark both for cancellation
    let mut active1: rust_redis_webserver::entities::session::ActiveModel = session1.into();
    active1.cancellation_status = Set(Some(CancellationStatus::Requested));
    let updated1 = active1.update(&db).await.expect("Failed to update");

    let mut active2: rust_redis_webserver::entities::session::ActiveModel = session2.into();
    active2.cancellation_status = Set(Some(CancellationStatus::Requested));
    let updated2 = active2.update(&db).await.expect("Failed to update");

    // Query for user1's cancelled sessions
    let user1_sessions = Session::find()
        .filter(rust_redis_webserver::entities::session::Column::UserId.eq(user1))
        .filter(
            rust_redis_webserver::entities::session::Column::CancellationStatus
                .eq(CancellationStatus::Requested),
        )
        .all(&db)
        .await
        .expect("Failed to query");

    // Should only find user1's session
    assert_eq!(user1_sessions.len(), 1);
    assert_eq!(user1_sessions[0].id, updated1.id);
    assert_eq!(user1_sessions[0].user_id, user1);

    // Cleanup
    cleanup_session(&db, updated1.id).await;
    cleanup_session(&db, updated2.id).await;
}
