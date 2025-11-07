use rust_redis_webserver::entities::dead_letter_queue::{DlqStatus, Entity as DeadLetterQueue};
use rust_redis_webserver::services::dead_letter_queue::{
    exists_in_dlq, insert_dlq_entry, MAX_RETRY_COUNT,
};
use sea_orm::{ColumnTrait, DatabaseConnection, EntityTrait, QueryFilter};
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

#[tokio::test]
async fn test_dlq_insert_and_exists() {
    let db = skip_if_no_db!(try_create_test_db().await);
    let entity_id = Uuid::new_v4();
    let task_type = "test_task";

    // Verify entity doesn't exist in DLQ initially
    let exists_before = exists_in_dlq(&db, task_type, entity_id)
        .await
        .expect("Failed to check DLQ");
    assert!(!exists_before, "Entity should not exist in DLQ initially");

    // Insert entity into DLQ
    let now = chrono::Utc::now();
    let result = insert_dlq_entry(
        &db,
        task_type,
        entity_id,
        None,
        MAX_RETRY_COUNT,
        "Test error",
        now.into(),
    )
    .await;

    assert!(result.is_ok(), "Failed to insert DLQ entry");

    // Verify entity exists in DLQ
    let exists_after = exists_in_dlq(&db, task_type, entity_id)
        .await
        .expect("Failed to check DLQ");
    assert!(exists_after, "Entity should exist in DLQ after insertion");

    // Clean up
    let entry = result.unwrap();
    let _ = DeadLetterQueue::delete_by_id(entry.id).exec(&db).await;
}

#[tokio::test]
async fn test_dlq_prevents_infinite_retry() {
    let db = skip_if_no_db!(try_create_test_db().await);
    let entity_id = Uuid::new_v4();
    let task_type = "ip_return_poller";

    // Simulate MAX_RETRY_COUNT failures
    for i in 1..=MAX_RETRY_COUNT {
        let exists = exists_in_dlq(&db, task_type, entity_id)
            .await
            .expect("Failed to check DLQ");

        if i < MAX_RETRY_COUNT {
            // Should not be in DLQ yet
            assert!(!exists, "Should not be in DLQ before max retries");
        } else {
            // On the last iteration, insert into DLQ
            let now = chrono::Utc::now();
            let result = insert_dlq_entry(
                &db,
                task_type,
                entity_id,
                None,
                i,
                &format!("Error attempt {}", i),
                now.into(),
            )
            .await;

            assert!(result.is_ok(), "Failed to insert DLQ entry");

            // Verify it's now in DLQ
            let exists_after = exists_in_dlq(&db, task_type, entity_id)
                .await
                .expect("Failed to check DLQ");
            assert!(exists_after, "Should be in DLQ after max retries");

            // Clean up
            let entry = result.unwrap();
            let _ = DeadLetterQueue::delete_by_id(entry.id).exec(&db).await;
        }
    }
}

#[tokio::test]
async fn test_dlq_entry_has_correct_status() {
    let db = skip_if_no_db!(try_create_test_db().await);
    let entity_id = Uuid::new_v4();
    let task_type = "test_task";

    let now = chrono::Utc::now();
    let entry = insert_dlq_entry(
        &db,
        task_type,
        entity_id,
        Some(serde_json::json!({"test": "data"})),
        MAX_RETRY_COUNT,
        "Test error message",
        now.into(),
    )
    .await
    .expect("Failed to insert DLQ entry");

    // Verify entry has correct initial status
    assert_eq!(entry.status, DlqStatus::Pending);
    assert_eq!(entry.task_type, task_type);
    assert_eq!(entry.entity_id, entity_id);
    assert_eq!(entry.retry_count, MAX_RETRY_COUNT);
    assert_eq!(entry.last_error, "Test error message");
    assert!(entry.entity_data.is_some());

    // Clean up
    let _ = DeadLetterQueue::delete_by_id(entry.id).exec(&db).await;
}

#[test]
fn test_max_retry_count_is_five() {
    // This test ensures MAX_RETRY_COUNT hasn't been accidentally changed
    // This is a unit test that doesn't need database
    assert_eq!(MAX_RETRY_COUNT, 5, "MAX_RETRY_COUNT should be 5");
}

#[tokio::test]
async fn test_dlq_filters_by_status() {
    let db = skip_if_no_db!(try_create_test_db().await);
    let entity_id = Uuid::new_v4();
    let task_type = "test_task";

    let now = chrono::Utc::now();
    let entry = insert_dlq_entry(
        &db,
        task_type,
        entity_id,
        None,
        MAX_RETRY_COUNT,
        "Test error",
        now.into(),
    )
    .await
    .expect("Failed to insert DLQ entry");

    // Query for pending entries
    let pending_entries = DeadLetterQueue::find()
        .filter(
            rust_redis_webserver::entities::dead_letter_queue::Column::Status
                .eq(DlqStatus::Pending),
        )
        .filter(rust_redis_webserver::entities::dead_letter_queue::Column::EntityId.eq(entity_id))
        .all(&db)
        .await
        .expect("Failed to query DLQ");

    assert!(!pending_entries.is_empty(), "Should find pending entry");

    // Query for resolved entries (should be empty)
    let resolved_entries = DeadLetterQueue::find()
        .filter(
            rust_redis_webserver::entities::dead_letter_queue::Column::Status
                .eq(DlqStatus::Resolved),
        )
        .filter(rust_redis_webserver::entities::dead_letter_queue::Column::EntityId.eq(entity_id))
        .all(&db)
        .await
        .expect("Failed to query DLQ");

    assert!(
        resolved_entries.is_empty(),
        "Should not find resolved entry"
    );

    // Clean up
    let _ = DeadLetterQueue::delete_by_id(entry.id).exec(&db).await;
}

#[test]
fn test_dlq_status_enum_values() {
    // Unit test to verify DLQ status enum values
    use std::mem::discriminant;

    // Ensure all three statuses are distinct
    assert_ne!(
        discriminant(&DlqStatus::Pending),
        discriminant(&DlqStatus::Resolved)
    );
    assert_ne!(
        discriminant(&DlqStatus::Pending),
        discriminant(&DlqStatus::Abandoned)
    );
    assert_ne!(
        discriminant(&DlqStatus::Resolved),
        discriminant(&DlqStatus::Abandoned)
    );
}
