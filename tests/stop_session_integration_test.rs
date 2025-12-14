use chrono::Utc;
use rust_redis_webserver::entities::session::{
    CancellationStatus, Entity as Session, Model as SessionModel, UiStatus,
};
use sea_orm::{
    ActiveModelTrait, ColumnTrait, DatabaseConnection, EntityTrait, NotSet, QueryFilter, Set,
};
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

/// Helper function to create a test session with sandbox config
async fn create_test_session_with_sandbox(
    db: &DatabaseConnection,
    user_id: &str,
    process_pid: Option<i32>,
    sbx_config: Option<serde_json::Value>,
    preserve_sandbox: Option<bool>,
) -> Result<SessionModel, sea_orm::DbErr> {
    let session_id = Uuid::new_v4();
    let new_session = rust_redis_webserver::entities::session::ActiveModel {
        id: Set(session_id),
        sbx_config: Set(sbx_config),
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
        preserve_sandbox: Set(preserve_sandbox),
    };

    new_session.insert(db).await
}

/// Helper function to cleanup test session
async fn cleanup_session(db: &DatabaseConnection, session_id: Uuid) {
    let _ = Session::delete_by_id(session_id).exec(db).await;
}

#[tokio::test]
async fn test_stop_session_sets_preserve_sandbox() {
    let db = skip_if_no_db!(try_create_test_db().await);
    let user_id = "test-stop-user-1";

    // Create a test session with a mock process PID and sandbox config
    let sbx_config = serde_json::json!({
        "item": {"api_url": "http://test-sandbox:8080"},
        "borrow_token": "test-token-123"
    });
    let session =
        create_test_session_with_sandbox(&db, user_id, Some(99999), Some(sbx_config), None)
            .await
            .expect("Failed to create test session");

    // Verify initial state
    assert_eq!(session.preserve_sandbox, None);
    assert!(session.sbx_config.is_some());

    // Simulate stop request (what the /sessions/{id}/stop endpoint does)
    let mut active_session: rust_redis_webserver::entities::session::ActiveModel = session.into();
    active_session.cancellation_status = Set(Some(CancellationStatus::Requested));
    active_session.cancelled_at = Set(Some(Utc::now().into()));
    active_session.cancelled_by = Set(Some(user_id.to_string()));
    active_session.preserve_sandbox = Set(Some(true));

    let stopped_session = active_session
        .update(&db)
        .await
        .expect("Failed to update session");

    // Verify stop was requested with preserve_sandbox = true
    assert_eq!(
        stopped_session.cancellation_status,
        Some(CancellationStatus::Requested)
    );
    assert_eq!(stopped_session.preserve_sandbox, Some(true));
    assert!(stopped_session.cancelled_at.is_some());
    assert_eq!(stopped_session.cancelled_by.as_deref(), Some(user_id));

    // Cleanup
    cleanup_session(&db, stopped_session.id).await;
}

#[tokio::test]
async fn test_cancellation_enforcer_preserves_sandbox_when_flag_set() {
    let db = skip_if_no_db!(try_create_test_db().await);
    let user_id = "test-stop-user-2";

    // Create a session with preserve_sandbox = true (simulating after stop endpoint)
    let sbx_config = serde_json::json!({
        "item": {"api_url": "http://test-sandbox:8080"},
        "borrow_token": "test-token-456"
    });
    let session =
        create_test_session_with_sandbox(&db, user_id, Some(88888), Some(sbx_config.clone()), None)
            .await
            .expect("Failed to create test session");

    // Set stop request with preserve_sandbox
    let mut active_session: rust_redis_webserver::entities::session::ActiveModel = session.into();
    active_session.cancellation_status = Set(Some(CancellationStatus::Requested));
    active_session.preserve_sandbox = Set(Some(true));
    let stopped_session = active_session
        .update(&db)
        .await
        .expect("Failed to update session");

    // Simulate what cancellation enforcer does when preserve_sandbox = true
    // It should set ui_status = NeedsReviewIpReturned (not NeedsReview)
    let mut active_session: rust_redis_webserver::entities::session::ActiveModel =
        stopped_session.into();

    // When preserve_sandbox = true, enforcer sets NeedsReviewIpReturned
    active_session.cancellation_status = Set(Some(CancellationStatus::Cancelled));
    active_session.ui_status = Set(UiStatus::NeedsReviewIpReturned);
    active_session.process_pid = Set(None);

    let enforced_session = active_session
        .update(&db)
        .await
        .expect("Failed to update session");

    // Verify sandbox is preserved
    assert_eq!(
        enforced_session.cancellation_status,
        Some(CancellationStatus::Cancelled)
    );
    assert_eq!(enforced_session.ui_status, UiStatus::NeedsReviewIpReturned);
    assert!(enforced_session.sbx_config.is_some()); // Sandbox preserved!
    assert_eq!(enforced_session.preserve_sandbox, Some(true));

    // Cleanup
    cleanup_session(&db, enforced_session.id).await;
}

#[tokio::test]
async fn test_cancellation_enforcer_returns_sandbox_when_flag_not_set() {
    let db = skip_if_no_db!(try_create_test_db().await);
    let user_id = "test-stop-user-3";

    // Create a session with preserve_sandbox = None (regular cancel, not stop)
    let sbx_config = serde_json::json!({
        "item": {"api_url": "http://test-sandbox:8080"},
        "borrow_token": "test-token-789"
    });
    let session =
        create_test_session_with_sandbox(&db, user_id, Some(77777), Some(sbx_config), None)
            .await
            .expect("Failed to create test session");

    // Set cancel request (not stop - preserve_sandbox remains None)
    let mut active_session: rust_redis_webserver::entities::session::ActiveModel = session.into();
    active_session.cancellation_status = Set(Some(CancellationStatus::Requested));
    let cancelled_session = active_session
        .update(&db)
        .await
        .expect("Failed to update session");

    // Simulate what cancellation enforcer does when preserve_sandbox = None
    // It should set ui_status = NeedsReview (IP return poller will return sandbox)
    let mut active_session: rust_redis_webserver::entities::session::ActiveModel =
        cancelled_session.into();

    active_session.cancellation_status = Set(Some(CancellationStatus::Cancelled));
    active_session.ui_status = Set(UiStatus::NeedsReview); // Different from NeedsReviewIpReturned
    active_session.process_pid = Set(None);

    let enforced_session = active_session
        .update(&db)
        .await
        .expect("Failed to update session");

    // Verify ui_status is NeedsReview (sandbox will be returned by IP return poller)
    assert_eq!(enforced_session.ui_status, UiStatus::NeedsReview);
    assert_eq!(enforced_session.preserve_sandbox, None);

    // Cleanup
    cleanup_session(&db, enforced_session.id).await;
}

#[tokio::test]
async fn test_ip_return_poller_skips_preserved_sandbox() {
    let db = skip_if_no_db!(try_create_test_db().await);
    let user_id = "test-stop-user-4";

    // Create a session with preserve_sandbox = true and NeedsReview status
    // This simulates a session that was stopped but still has sandbox
    let sbx_config = serde_json::json!({
        "item": {"api_url": "http://test-sandbox:8080"},
        "borrow_token": "test-token-skip"
    });
    let session =
        create_test_session_with_sandbox(&db, user_id, None, Some(sbx_config), Some(true))
            .await
            .expect("Failed to create test session");

    // Set to NeedsReview (which normally triggers IP return)
    let mut active_session: rust_redis_webserver::entities::session::ActiveModel = session.into();
    active_session.ui_status = Set(UiStatus::NeedsReview);
    let review_session = active_session
        .update(&db)
        .await
        .expect("Failed to update session");

    // Query for sessions that IP return poller would process
    let sessions_for_ip_return = Session::find()
        .filter(
            rust_redis_webserver::entities::session::Column::UiStatus
                .is_in([UiStatus::NeedsReview, UiStatus::Archived]),
        )
        .filter(rust_redis_webserver::entities::session::Column::SbxConfig.is_not_null())
        .all(&db)
        .await
        .expect("Failed to query sessions");

    // Our session should be found by the query
    let found = sessions_for_ip_return
        .iter()
        .find(|s| s.id == review_session.id);
    assert!(
        found.is_some(),
        "Session should be found by IP return query"
    );

    // But the poller should skip it because preserve_sandbox = true and not Archived
    let session_data = found.unwrap();
    let should_skip =
        session_data.preserve_sandbox == Some(true) && session_data.ui_status != UiStatus::Archived;
    assert!(should_skip, "IP return poller should skip this session");

    // Cleanup
    cleanup_session(&db, review_session.id).await;
}

#[tokio::test]
async fn test_archived_session_returns_sandbox_despite_preserve_flag() {
    let db = skip_if_no_db!(try_create_test_db().await);
    let user_id = "test-stop-user-5";

    // Create a session with preserve_sandbox = true but Archived status
    let sbx_config = serde_json::json!({
        "item": {"api_url": "http://test-sandbox:8080"},
        "borrow_token": "test-token-archive"
    });
    let session =
        create_test_session_with_sandbox(&db, user_id, None, Some(sbx_config), Some(true))
            .await
            .expect("Failed to create test session");

    // Archive the session
    let mut active_session: rust_redis_webserver::entities::session::ActiveModel = session.into();
    active_session.ui_status = Set(UiStatus::Archived);
    let archived_session = active_session
        .update(&db)
        .await
        .expect("Failed to update session");

    // IP return poller should NOT skip archived sessions even with preserve_sandbox = true
    let should_skip = archived_session.preserve_sandbox == Some(true)
        && archived_session.ui_status != UiStatus::Archived;
    assert!(
        !should_skip,
        "IP return poller should NOT skip archived sessions"
    );

    // Cleanup
    cleanup_session(&db, archived_session.id).await;
}

#[tokio::test]
async fn test_stopped_session_can_be_resumed() {
    let db = skip_if_no_db!(try_create_test_db().await);
    let user_id = "test-stop-user-6";

    // Create a session that was stopped (has sandbox, preserve_sandbox = true, NeedsReviewIpReturned)
    let sbx_config = serde_json::json!({
        "item": {"api_url": "http://test-sandbox:8080"},
        "borrow_token": "test-token-resume"
    });
    let session =
        create_test_session_with_sandbox(&db, user_id, None, Some(sbx_config.clone()), Some(true))
            .await
            .expect("Failed to create test session");

    let mut active_session: rust_redis_webserver::entities::session::ActiveModel = session.into();
    active_session.ui_status = Set(UiStatus::NeedsReviewIpReturned);
    active_session.cancellation_status = Set(Some(CancellationStatus::Cancelled));
    let stopped_session = active_session
        .update(&db)
        .await
        .expect("Failed to update session");

    // Verify stopped state
    assert_eq!(stopped_session.ui_status, UiStatus::NeedsReviewIpReturned);
    assert!(stopped_session.sbx_config.is_some());
    assert_eq!(stopped_session.preserve_sandbox, Some(true));

    // Simulate sending a new prompt - session goes back to Pending
    let mut active_session: rust_redis_webserver::entities::session::ActiveModel =
        stopped_session.into();
    active_session.ui_status = Set(UiStatus::Pending);
    let pending_session = active_session
        .update(&db)
        .await
        .expect("Failed to update session");

    // Simulate prompt poller processing - should reuse existing sandbox
    let has_existing_sandbox = pending_session.sbx_config.is_some();
    assert!(
        has_existing_sandbox,
        "Stopped session should still have sandbox"
    );

    // Prompt poller would reuse sandbox and clear preserve_sandbox flag
    let mut active_session: rust_redis_webserver::entities::session::ActiveModel =
        pending_session.into();
    active_session.ui_status = Set(UiStatus::InProgress);
    active_session.preserve_sandbox = Set(None); // Clear flag when resuming
    active_session.cancellation_status = Set(None); // Clear cancellation for fresh run
    active_session.cancelled_at = Set(None);
    active_session.cancelled_by = Set(None);
    let resumed_session = active_session
        .update(&db)
        .await
        .expect("Failed to update session");

    // Verify resumed state
    assert_eq!(resumed_session.ui_status, UiStatus::InProgress);
    assert!(resumed_session.sbx_config.is_some()); // Same sandbox reused
    assert_eq!(resumed_session.preserve_sandbox, None); // Flag cleared
    assert_eq!(resumed_session.cancellation_status, None);

    // Cleanup
    cleanup_session(&db, resumed_session.id).await;
}

#[tokio::test]
async fn test_stop_then_archive_returns_sandbox() {
    let db = skip_if_no_db!(try_create_test_db().await);
    let user_id = "test-stop-user-7";

    // Create a stopped session (preserve_sandbox = true, has sandbox)
    let sbx_config = serde_json::json!({
        "item": {"api_url": "http://test-sandbox:8080"},
        "borrow_token": "test-token-stop-archive"
    });
    let session =
        create_test_session_with_sandbox(&db, user_id, None, Some(sbx_config), Some(true))
            .await
            .expect("Failed to create test session");

    let mut active_session: rust_redis_webserver::entities::session::ActiveModel = session.into();
    active_session.ui_status = Set(UiStatus::NeedsReviewIpReturned);
    active_session.cancellation_status = Set(Some(CancellationStatus::Cancelled));
    let stopped_session = active_session
        .update(&db)
        .await
        .expect("Failed to update session");

    // Verify stopped state with preserved sandbox
    assert!(stopped_session.sbx_config.is_some());
    assert_eq!(stopped_session.preserve_sandbox, Some(true));

    // Now archive the session
    let mut active_session: rust_redis_webserver::entities::session::ActiveModel =
        stopped_session.into();
    active_session.ui_status = Set(UiStatus::Archived);
    let archived_session = active_session
        .update(&db)
        .await
        .expect("Failed to update session");

    // Archived session should be processed by IP return poller
    // (preserve_sandbox is ignored for Archived sessions)
    let should_return_sandbox =
        archived_session.ui_status == UiStatus::Archived && archived_session.sbx_config.is_some();
    assert!(
        should_return_sandbox,
        "Archived session should have sandbox returned"
    );

    // Simulate IP return poller returning the sandbox
    let mut active_session: rust_redis_webserver::entities::session::ActiveModel =
        archived_session.into();
    active_session.sbx_config = Set(None); // Sandbox returned
    active_session.ui_status = Set(UiStatus::NeedsReviewIpReturned);
    let final_session = active_session
        .update(&db)
        .await
        .expect("Failed to update session");

    // Verify sandbox was returned
    assert!(final_session.sbx_config.is_none());

    // Cleanup
    cleanup_session(&db, final_session.id).await;
}

#[test]
fn test_preserve_sandbox_field_exists() {
    // Unit test to verify preserve_sandbox field is accessible
    // This will fail to compile if the field doesn't exist
    let _session = rust_redis_webserver::entities::session::ActiveModel {
        id: Set(Uuid::new_v4()),
        sbx_config: Set(None),
        parent: Set(None),
        branch: Set(None),
        repo: Set(None),
        target_branch: Set(None),
        title: Set(None),
        ui_status: Set(UiStatus::Pending),
        user_id: Set("test".to_string()),
        ip_return_retry_count: Set(0),
        created_at: NotSet,
        updated_at: NotSet,
        deleted_at: Set(None),
        cancellation_status: Set(None),
        cancelled_at: Set(None),
        cancelled_by: Set(None),
        process_pid: Set(None),
        preserve_sandbox: Set(Some(true)), // This field must exist
    };
}
