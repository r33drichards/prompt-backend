use crate::entities::dead_letter_queue::{
    self, ActiveModel, DlqStatus, Entity as DeadLetterQueue, Model,
};
use sea_orm::entity::prelude::DateTimeWithTimeZone;
use sea_orm::{
    ActiveModelTrait, ColumnTrait, DatabaseConnection, EntityTrait, NotSet, PaginatorTrait,
    QueryFilter, Set,
};
use serde_json::Value as JsonValue;
use uuid::Uuid;

/// Maximum number of retries before moving to DLQ
pub const MAX_RETRY_COUNT: i32 = 5;

/// Insert a new entry into the dead letter queue
pub async fn insert_dlq_entry(
    db: &DatabaseConnection,
    task_type: &str,
    entity_id: Uuid,
    entity_data: Option<JsonValue>,
    retry_count: i32,
    error: &str,
    first_failed_at: DateTimeWithTimeZone,
) -> Result<Model, sea_orm::DbErr> {
    let dlq_entry = ActiveModel {
        id: Set(Uuid::new_v4()),
        task_type: Set(task_type.to_string()),
        entity_id: Set(entity_id),
        entity_data: Set(entity_data),
        retry_count: Set(retry_count),
        last_error: Set(error.to_string()),
        last_error_at: Set(chrono::Utc::now().into()),
        first_failed_at: Set(first_failed_at),
        status: Set(DlqStatus::Pending),
        resolution_notes: Set(None),
        created_at: NotSet, // Use database default (current_timestamp)
        updated_at: NotSet, // Use database default (current_timestamp)
    };

    dlq_entry.insert(db).await
}

/// Check if an entity already exists in the DLQ for a given task type
pub async fn exists_in_dlq(
    db: &DatabaseConnection,
    task_type: &str,
    entity_id: Uuid,
) -> Result<bool, sea_orm::DbErr> {
    let count = DeadLetterQueue::find()
        .filter(dead_letter_queue::Column::TaskType.eq(task_type))
        .filter(dead_letter_queue::Column::EntityId.eq(entity_id))
        .filter(dead_letter_queue::Column::Status.eq(DlqStatus::Pending))
        .count(db)
        .await?;

    Ok(count > 0)
}

/// Mark a DLQ entry as resolved
pub async fn resolve_dlq_entry(
    db: &DatabaseConnection,
    dlq_id: Uuid,
    resolution_notes: Option<String>,
) -> Result<Model, sea_orm::DbErr> {
    let dlq_entry = DeadLetterQueue::find_by_id(dlq_id).one(db).await?.ok_or(
        sea_orm::DbErr::RecordNotFound("DLQ entry not found".to_string()),
    )?;

    let mut active_entry: ActiveModel = dlq_entry.into();
    active_entry.status = Set(DlqStatus::Resolved);
    active_entry.resolution_notes = Set(resolution_notes);
    active_entry.updated_at = NotSet; // Will be updated by database trigger or default

    active_entry.update(db).await
}

/// Mark a DLQ entry as abandoned
pub async fn abandon_dlq_entry(
    db: &DatabaseConnection,
    dlq_id: Uuid,
    resolution_notes: Option<String>,
) -> Result<Model, sea_orm::DbErr> {
    let dlq_entry = DeadLetterQueue::find_by_id(dlq_id).one(db).await?.ok_or(
        sea_orm::DbErr::RecordNotFound("DLQ entry not found".to_string()),
    )?;

    let mut active_entry: ActiveModel = dlq_entry.into();
    active_entry.status = Set(DlqStatus::Abandoned);
    active_entry.resolution_notes = Set(resolution_notes);
    active_entry.updated_at = NotSet; // Will be updated by database trigger or default

    active_entry.update(db).await
}
