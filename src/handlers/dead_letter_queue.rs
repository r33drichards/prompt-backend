use rocket::serde::json::Json;
use rocket::serde::{Deserialize, Serialize};
use rocket::State;
use rocket_okapi::okapi::schemars::JsonSchema;
use rocket_okapi::openapi;
use sea_orm::{ColumnTrait, DatabaseConnection, EntityTrait, QueryFilter, QueryOrder};
use uuid::Uuid;

use crate::auth::AuthenticatedUser;
use crate::entities::dead_letter_queue::{
    self, DlqStatus, Entity as DeadLetterQueue, Model as DlqModel,
};
use crate::error::{Error, OResult};
use crate::services::dead_letter_queue::{abandon_dlq_entry, resolve_dlq_entry};

#[derive(Serialize, Deserialize, JsonSchema, Clone)]
pub struct DlqDto {
    pub id: String,
    pub task_type: String,
    pub entity_id: String,
    pub entity_data: Option<serde_json::Value>,
    pub retry_count: i32,
    pub last_error: String,
    pub last_error_at: String,
    pub first_failed_at: String,
    pub status: DlqStatus,
    pub resolution_notes: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

impl From<DlqModel> for DlqDto {
    fn from(model: DlqModel) -> Self {
        DlqDto {
            id: model.id.to_string(),
            task_type: model.task_type,
            entity_id: model.entity_id.to_string(),
            entity_data: model.entity_data,
            retry_count: model.retry_count,
            last_error: model.last_error,
            last_error_at: model.last_error_at.to_string(),
            first_failed_at: model.first_failed_at.to_string(),
            status: model.status,
            resolution_notes: model.resolution_notes,
            created_at: model.created_at.to_string(),
            updated_at: model.updated_at.to_string(),
        }
    }
}

#[derive(Serialize, Deserialize, JsonSchema, Clone)]
pub struct ListDlqOutput {
    pub entries: Vec<DlqDto>,
}

#[derive(Serialize, Deserialize, JsonSchema, Clone)]
pub struct ResolveDlqInput {
    pub resolution_notes: Option<String>,
}

#[derive(Serialize, Deserialize, JsonSchema, Clone)]
pub struct ResolveDlqOutput {
    pub success: bool,
    pub message: String,
}

#[derive(Serialize, Deserialize, JsonSchema, Clone)]
pub struct AbandonDlqInput {
    pub resolution_notes: Option<String>,
}

#[derive(Serialize, Deserialize, JsonSchema, Clone)]
pub struct AbandonDlqOutput {
    pub success: bool,
    pub message: String,
}

/// List all dead letter queue entries
///
/// Returns all entries in the dead letter queue, optionally filtered by status
#[openapi(tag = "Dead Letter Queue")]
#[get("/dead-letter-queue?<status>")]
pub async fn list_dlq_entries(
    db: &State<DatabaseConnection>,
    _user: AuthenticatedUser,
    status: Option<String>,
) -> OResult<ListDlqOutput> {
    let mut query = DeadLetterQueue::find();

    // Filter by status if provided
    if let Some(status_str) = status {
        let dlq_status = match status_str.as_str() {
            "pending" => DlqStatus::Pending,
            "resolved" => DlqStatus::Resolved,
            "abandoned" => DlqStatus::Abandoned,
            _ => {
                return Err(Error::bad_request(format!(
                    "Invalid status: {}. Valid values: pending, resolved, abandoned",
                    status_str
                )));
            }
        };
        query = query.filter(dead_letter_queue::Column::Status.eq(dlq_status));
    }

    let entries = query
        .order_by_desc(dead_letter_queue::Column::CreatedAt)
        .all(db.inner())
        .await
        .map_err(|e| Error::internal_server_error(format!("Failed to list DLQ entries: {}", e)))?;

    let dto_entries: Vec<DlqDto> = entries.into_iter().map(|e| e.into()).collect();

    Ok(Json(ListDlqOutput {
        entries: dto_entries,
    }))
}

/// Get a specific dead letter queue entry
///
/// Returns details of a single DLQ entry by ID
#[openapi(tag = "Dead Letter Queue")]
#[get("/dead-letter-queue/<id>")]
pub async fn get_dlq_entry(
    db: &State<DatabaseConnection>,
    _user: AuthenticatedUser,
    id: String,
) -> OResult<DlqDto> {
    let uuid =
        Uuid::parse_str(&id).map_err(|_| Error::bad_request(format!("Invalid UUID: {}", id)))?;

    let entry = DeadLetterQueue::find_by_id(uuid)
        .one(db.inner())
        .await
        .map_err(|e| Error::internal_server_error(format!("Failed to get DLQ entry: {}", e)))?
        .ok_or_else(|| Error::not_found(format!("DLQ entry not found: {}", id)))?;

    Ok(Json(entry.into()))
}

/// Mark a DLQ entry as resolved
///
/// Marks a dead letter queue entry as resolved with optional resolution notes
#[openapi(tag = "Dead Letter Queue")]
#[post("/dead-letter-queue/<id>/resolve", data = "<input>")]
pub async fn resolve_dlq(
    db: &State<DatabaseConnection>,
    _user: AuthenticatedUser,
    id: String,
    input: Json<ResolveDlqInput>,
) -> OResult<ResolveDlqOutput> {
    let uuid =
        Uuid::parse_str(&id).map_err(|_| Error::bad_request(format!("Invalid UUID: {}", id)))?;

    resolve_dlq_entry(db.inner(), uuid, input.resolution_notes.clone())
        .await
        .map_err(|e| Error::internal_server_error(format!("Failed to resolve DLQ entry: {}", e)))?;

    Ok(Json(ResolveDlqOutput {
        success: true,
        message: format!("DLQ entry {} marked as resolved", id),
    }))
}

/// Mark a DLQ entry as abandoned
///
/// Marks a dead letter queue entry as abandoned with optional resolution notes
#[openapi(tag = "Dead Letter Queue")]
#[post("/dead-letter-queue/<id>/abandon", data = "<input>")]
pub async fn abandon_dlq(
    db: &State<DatabaseConnection>,
    _user: AuthenticatedUser,
    id: String,
    input: Json<AbandonDlqInput>,
) -> OResult<AbandonDlqOutput> {
    let uuid =
        Uuid::parse_str(&id).map_err(|_| Error::bad_request(format!("Invalid UUID: {}", id)))?;

    abandon_dlq_entry(db.inner(), uuid, input.resolution_notes.clone())
        .await
        .map_err(|e| Error::internal_server_error(format!("Failed to abandon DLQ entry: {}", e)))?;

    Ok(Json(AbandonDlqOutput {
        success: true,
        message: format!("DLQ entry {} marked as abandoned", id),
    }))
}
