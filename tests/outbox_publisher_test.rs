use apalis::prelude::*;
use apalis_redis::RedisStorage;
use sea_orm::{
    ActiveModelTrait, Database, DatabaseConnection, EntityTrait, Set,
};
use std::sync::Arc;
use tokio::sync::Mutex;
use uuid::Uuid;

// Import modules from the main crate
use rust_redis_webserver::bg_tasks::outbox_publisher::{process_outbox_job, OutboxContext, OutboxJob};
use rust_redis_webserver::bg_tasks::session_handler::SessionJob;
use rust_redis_webserver::entities::session::{
    ActiveModel, Entity as Session, InboxStatus, SessionStatus,
};

/// Helper function to setup test database
async fn setup_test_db() -> DatabaseConnection {
    let database_url = std::env::var("TEST_DATABASE_URL")
        .unwrap_or_else(|_| "postgres://postgres:postgres@localhost/test_db".to_string());

    Database::connect(&database_url)
        .await
        .expect("Failed to connect to test database")
}

/// Helper function to setup Redis storage
async fn setup_redis_storage() -> Arc<Mutex<RedisStorage<SessionJob>>> {
    let redis_url = std::env::var("REDIS_URL")
        .unwrap_or_else(|_| "redis://127.0.0.1/".to_string());

    let conn = apalis_redis::connect(redis_url)
        .await
        .expect("Failed to connect to Redis");

    Arc::new(Mutex::new(RedisStorage::new(conn)))
}

/// Helper function to create a test session
async fn create_test_session(
    db: &DatabaseConnection,
    inbox_status: InboxStatus,
    session_status: SessionStatus,
) -> Uuid {
    let id = Uuid::new_v4();

    let new_session = ActiveModel {
        id: Set(id),
        messages: Set(Some(serde_json::json!({"test": "data"}))),
        inbox_status: Set(inbox_status),
        sbx_config: Set(Some(serde_json::json!({"config": "test"}))),
        parent: Set(None),
        branch: Set(Some("test-branch".to_string())),
        repo: Set(Some("test-repo".to_string())),
        target_branch: Set(Some("main".to_string())),
        title: Set(Some("Test Session".to_string())),
        session_status: Set(session_status),
        created_at: Set(chrono::Utc::now().into()),
        updated_at: Set(chrono::Utc::now().into()),
        deleted_at: Set(None),
    };

    new_session
        .insert(db)
        .await
        .expect("Failed to insert test session");

    id
}

/// Helper function to clean up test sessions
async fn cleanup_test_sessions(db: &DatabaseConnection) {
    Session::delete_many()
        .exec(db)
        .await
        .expect("Failed to clean up test sessions");
}

#[tokio::test]
async fn test_outbox_publisher_no_sessions() {
    let db = setup_test_db().await;
    let redis_storage = setup_redis_storage().await;

    // Clean up any existing test data
    cleanup_test_sessions(&db).await;

    // Create context
    let ctx = OutboxContext {
        db: db.clone(),
        redis_storage: redis_storage.clone(),
    };

    // Create a test job
    let job = OutboxJob {
        session_id: "test".to_string(),
        payload: serde_json::json!({}),
    };

    // Process the job
    let result = process_outbox_job(job, Data::new(ctx)).await;

    // Should succeed even with no sessions
    assert!(result.is_ok());

    // Verify no sessions exist
    let sessions = Session::find().all(&db).await.unwrap();
    assert_eq!(sessions.len(), 0);
}

#[tokio::test]
async fn test_outbox_publisher_single_active_session() {
    let db = setup_test_db().await;
    let redis_storage = setup_redis_storage().await;

    // Clean up any existing test data
    cleanup_test_sessions(&db).await;

    // Create a single active session
    let session_id = create_test_session(&db, InboxStatus::Active, SessionStatus::Active).await;

    // Create context
    let ctx = OutboxContext {
        db: db.clone(),
        redis_storage: redis_storage.clone(),
    };

    // Create a test job
    let job = OutboxJob {
        session_id: "test".to_string(),
        payload: serde_json::json!({}),
    };

    // Process the job
    let result = process_outbox_job(job, Data::new(ctx)).await;
    assert!(result.is_ok());

    // Verify session inbox_status was updated to Pending
    let updated_session = Session::find_by_id(session_id)
        .one(&db)
        .await
        .unwrap()
        .expect("Session should exist");

    assert_eq!(updated_session.inbox_status, InboxStatus::Pending);
}

#[tokio::test]
async fn test_outbox_publisher_multiple_active_sessions() {
    let db = setup_test_db().await;
    let redis_storage = setup_redis_storage().await;

    // Clean up any existing test data
    cleanup_test_sessions(&db).await;

    // Create multiple active sessions
    let session_id_1 = create_test_session(&db, InboxStatus::Active, SessionStatus::Active).await;
    let session_id_2 = create_test_session(&db, InboxStatus::Active, SessionStatus::Active).await;
    let session_id_3 = create_test_session(&db, InboxStatus::Active, SessionStatus::Active).await;

    // Create context
    let ctx = OutboxContext {
        db: db.clone(),
        redis_storage: redis_storage.clone(),
    };

    // Create a test job
    let job = OutboxJob {
        session_id: "test".to_string(),
        payload: serde_json::json!({}),
    };

    // Process the job
    let result = process_outbox_job(job, Data::new(ctx)).await;
    assert!(result.is_ok());

    // Verify all sessions were updated to Pending
    for session_id in [session_id_1, session_id_2, session_id_3] {
        let updated_session = Session::find_by_id(session_id)
            .one(&db)
            .await
            .unwrap()
            .expect("Session should exist");

        assert_eq!(updated_session.inbox_status, InboxStatus::Pending);
    }
}

#[tokio::test]
async fn test_outbox_publisher_ignores_non_active_sessions() {
    let db = setup_test_db().await;
    let redis_storage = setup_redis_storage().await;

    // Clean up any existing test data
    cleanup_test_sessions(&db).await;

    // Create sessions with various inbox_status values
    let active_id = create_test_session(&db, InboxStatus::Active, SessionStatus::Active).await;
    let pending_id = create_test_session(&db, InboxStatus::Pending, SessionStatus::Active).await;
    let completed_id = create_test_session(&db, InboxStatus::Completed, SessionStatus::Active).await;
    let archived_id = create_test_session(&db, InboxStatus::Archived, SessionStatus::Active).await;

    // Create context
    let ctx = OutboxContext {
        db: db.clone(),
        redis_storage: redis_storage.clone(),
    };

    // Create a test job
    let job = OutboxJob {
        session_id: "test".to_string(),
        payload: serde_json::json!({}),
    };

    // Process the job
    let result = process_outbox_job(job, Data::new(ctx)).await;
    assert!(result.is_ok());

    // Verify only the active session was updated
    let active_session = Session::find_by_id(active_id).one(&db).await.unwrap().unwrap();
    assert_eq!(active_session.inbox_status, InboxStatus::Pending);

    let pending_session = Session::find_by_id(pending_id).one(&db).await.unwrap().unwrap();
    assert_eq!(pending_session.inbox_status, InboxStatus::Pending); // Should remain Pending

    let completed_session = Session::find_by_id(completed_id).one(&db).await.unwrap().unwrap();
    assert_eq!(completed_session.inbox_status, InboxStatus::Completed); // Should remain Completed

    let archived_session = Session::find_by_id(archived_id).one(&db).await.unwrap().unwrap();
    assert_eq!(archived_session.inbox_status, InboxStatus::Archived); // Should remain Archived
}

#[tokio::test]
async fn test_outbox_publisher_with_null_data() {
    let db = setup_test_db().await;
    let redis_storage = setup_redis_storage().await;

    // Clean up any existing test data
    cleanup_test_sessions(&db).await;

    // Create a session with null/minimal data
    let id = Uuid::new_v4();
    let new_session = ActiveModel {
        id: Set(id),
        messages: Set(None),
        inbox_status: Set(InboxStatus::Active),
        sbx_config: Set(None),
        parent: Set(None),
        branch: Set(None),
        repo: Set(None),
        target_branch: Set(None),
        title: Set(None),
        session_status: Set(SessionStatus::Active),
        created_at: Set(chrono::Utc::now().into()),
        updated_at: Set(chrono::Utc::now().into()),
        deleted_at: Set(None),
    };

    new_session.insert(&db).await.expect("Failed to insert test session");

    // Create context
    let ctx = OutboxContext {
        db: db.clone(),
        redis_storage: redis_storage.clone(),
    };

    // Create a test job
    let job = OutboxJob {
        session_id: "test".to_string(),
        payload: serde_json::json!({}),
    };

    // Process the job - should handle null data gracefully
    let result = process_outbox_job(job, Data::new(ctx)).await;
    assert!(result.is_ok());

    // Verify session was still updated
    let updated_session = Session::find_by_id(id).one(&db).await.unwrap().unwrap();
    assert_eq!(updated_session.inbox_status, InboxStatus::Pending);
}

#[tokio::test]
async fn test_outbox_publisher_mixed_session_statuses() {
    let db = setup_test_db().await;
    let redis_storage = setup_redis_storage().await;

    // Clean up any existing test data
    cleanup_test_sessions(&db).await;

    // Create active sessions with different session_status values
    let active_active = create_test_session(&db, InboxStatus::Active, SessionStatus::Active).await;
    let active_archived = create_test_session(&db, InboxStatus::Active, SessionStatus::Archived).await;

    // Create context
    let ctx = OutboxContext {
        db: db.clone(),
        redis_storage: redis_storage.clone(),
    };

    // Create a test job
    let job = OutboxJob {
        session_id: "test".to_string(),
        payload: serde_json::json!({}),
    };

    // Process the job
    let result = process_outbox_job(job, Data::new(ctx)).await;
    assert!(result.is_ok());

    // Verify both sessions were updated (inbox_status is independent of session_status)
    let session1 = Session::find_by_id(active_active).one(&db).await.unwrap().unwrap();
    assert_eq!(session1.inbox_status, InboxStatus::Pending);

    let session2 = Session::find_by_id(active_archived).one(&db).await.unwrap().unwrap();
    assert_eq!(session2.inbox_status, InboxStatus::Pending);
}
