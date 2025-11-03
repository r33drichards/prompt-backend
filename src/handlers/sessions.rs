use rocket::serde::json::Json;
use rocket::State;
use rocket_okapi::openapi;
use rocket_okapi::okapi::schemars::JsonSchema;
use rocket::serde::{Deserialize, Serialize};
use sea_orm::{ActiveModelTrait, DatabaseConnection, EntityTrait, Set, QueryOrder};
use sea_orm::prelude::DateTimeWithTimeZone;
use uuid::Uuid;

use crate::entities::session::{self, Entity as Session, Model as SessionModel, InboxStatus, SessionStatus};
use crate::error::{Error, OResult};
use crate::services::anthropic;

#[derive(Serialize, Deserialize, JsonSchema, Clone)]
pub struct CreateSessionInput {
    pub messages: Option<serde_json::Value>,
    pub parent: Option<String>,
    pub repo: String,
    pub target_branch: String,
}

#[derive(Serialize, Deserialize, JsonSchema, Clone)]
pub struct CreateSessionOutput {
    pub success: bool,
    pub message: String,
    pub id: String,
}

#[derive(Serialize, Deserialize, JsonSchema, Clone)]
pub struct SessionDto {
    pub id: String,
    pub messages: Option<serde_json::Value>,
    pub inbox_status: InboxStatus,
    pub sbx_config: Option<serde_json::Value>,
    pub parent: Option<String>,
    pub branch: Option<String>,
    pub repo: Option<String>,
    pub target_branch: Option<String>,
    pub title: Option<String>,
    pub session_status: SessionStatus,
    pub created_at: String,
    pub updated_at: String,
    pub deleted_at: Option<String>,
}

impl From<SessionModel> for SessionDto {
    fn from(model: SessionModel) -> Self {
        SessionDto {
            id: model.id.to_string(),
            messages: model.messages,
            inbox_status: model.inbox_status,
            sbx_config: model.sbx_config,
            parent: model.parent.map(|p| p.to_string()),
            branch: model.branch,
            repo: model.repo,
            target_branch: model.target_branch,
            title: model.title,
            session_status: model.session_status,
            created_at: model.created_at.to_string(),
            updated_at: model.updated_at.to_string(),
            deleted_at: model.deleted_at.map(|d| d.to_string()),
        }
    }
}

#[derive(Serialize, Deserialize, JsonSchema, Clone)]
pub struct ReadSessionOutput {
    pub session: SessionDto,
}

#[derive(Serialize, Deserialize, JsonSchema, Clone)]
pub struct ListSessionsOutput {
    pub sessions: Vec<SessionDto>,
}

#[derive(Serialize, Deserialize, JsonSchema, Clone)]
pub struct UpdateSessionInput {
    pub id: String,
    pub messages: Option<serde_json::Value>,
    pub inbox_status: InboxStatus,
    pub sbx_config: Option<serde_json::Value>,
    pub parent: Option<String>,
    pub branch: Option<String>,
    pub repo: Option<String>,
    pub target_branch: Option<String>,
    pub title: Option<String>,
    pub session_status: Option<SessionStatus>,
}

#[derive(Serialize, Deserialize, JsonSchema, Clone)]
pub struct UpdateSessionOutput {
    pub success: bool,
    pub message: String,
}

#[derive(Serialize, Deserialize, JsonSchema, Clone)]
pub struct DeleteSessionOutput {
    pub success: bool,
    pub message: String,
}

/// Create a new session
#[openapi]
#[post("/sessions", data = "<input>")]
pub async fn create(
    db: &State<DatabaseConnection>,
    input: Json<CreateSessionInput>,
) -> OResult<CreateSessionOutput> {
    let id = Uuid::new_v4();

    let parent = match &input.parent {
        Some(p) => Some(Uuid::parse_str(p)
            .map_err(|_| Error::bad_request("Invalid parent UUID format".to_string()))?),
        None => None,
    };

    let prompt = "todo";

    // Generate title using Anthropic Haiku
    let title = anthropic::generate_session_title(
        &input.repo,
        &input.target_branch,
        &prompt,
    )
    .await
    .unwrap_or_else(|e| {
        tracing::warn!("Failed to generate session title: {}", e);
        "Untitled Session".to_string()
    });

    // Generate branch name 
    let generated_branch = anthropic::generate_branch_name(
        &input.repo,
        &input.target_branch,
        &prompt,
        &id.to_string(),
    )
    .await
    .unwrap_or_else(|e| {
        tracing::warn!("Failed to generate branch name: {}", e);
        format!("claude/session-{}", &id.to_string()[..24])
    });

    let new_session = session::ActiveModel {
        id: Set(id),
        messages: Set(input.messages.clone()),
        inbox_status: Set(InboxStatus::Active),
        sbx_config: Set(None),
        parent: Set(parent),
        branch: Set(Some(generated_branch)),
        repo: Set(Some(input.repo)),
        target_branch: Set(Some(input.target_branch)),
        title: Set(Some(title)),
        session_status: Set(SessionStatus::Active),
        created_at: Set(DateTimeWithTimeZone::now()),
        updated_at: Set(DateTimeWithTimeZone::now()),
        deleted_at: Set(None),
    };

    match new_session.insert(db.inner()).await {
        Ok(_) => Ok(Json(CreateSessionOutput {
            success: true,
            message: "Session created successfully".to_string(),
            id: id.to_string(),
        })),
        Err(e) => Err(Error::database_error(e.to_string())),
    }
}

/// Read (retrieve) a session by ID
#[openapi]
#[get("/sessions/<id>")]
pub async fn read(
    db: &State<DatabaseConnection>,
    id: String,
) -> OResult<ReadSessionOutput> {
    let uuid = Uuid::parse_str(&id)
        .map_err(|_| Error::bad_request("Invalid UUID format".to_string()))?;

    match Session::find_by_id(uuid).one(db.inner()).await {
        Ok(Some(session)) => Ok(Json(ReadSessionOutput {
            session: session.into()
        })),
        Ok(None) => Err(Error::not_found("Session not found".to_string())),
        Err(e) => Err(Error::database_error(e.to_string())),
    }
}

/// List all sessions
#[openapi]
#[get("/sessions")]
pub async fn list(db: &State<DatabaseConnection>) -> OResult<ListSessionsOutput> {
    match Session::find()
        .order_by_asc(session::Column::Id)
        .all(db.inner())
        .await
    {
        Ok(sessions) => Ok(Json(ListSessionsOutput {
            sessions: sessions.into_iter().map(|s| s.into()).collect()
        })),
        Err(e) => Err(Error::database_error(e.to_string())),
    }
}

/// Update an existing session (PUT - full replacement)
#[openapi]
#[put("/sessions/<id>", data = "<input>")]
pub async fn update(
    db: &State<DatabaseConnection>,
    id: String,
    input: Json<UpdateSessionInput>,
) -> OResult<UpdateSessionOutput> {
    let uuid = Uuid::parse_str(&id)
        .map_err(|_| Error::bad_request("Invalid UUID format".to_string()))?;

    let parent = match &input.parent {
        Some(p) => Some(Uuid::parse_str(p)
            .map_err(|_| Error::bad_request("Invalid parent UUID format".to_string()))?),
        None => None,
    };

    // First check if the session exists
    let existing_session = Session::find_by_id(uuid)
        .one(db.inner())
        .await
        .map_err(|e| Error::database_error(e.to_string()))?
        .ok_or_else(|| Error::not_found("Session not found".to_string()))?;

    let mut active_session: session::ActiveModel = existing_session.into();
    active_session.messages = Set(input.messages.clone());
    active_session.inbox_status = Set(input.inbox_status.clone());
    active_session.sbx_config = Set(input.sbx_config.clone());
    active_session.parent = Set(parent);
    active_session.branch = Set(input.branch.clone());
    active_session.repo = Set(input.repo.clone());
    active_session.target_branch = Set(input.target_branch.clone());
    active_session.title = Set(input.title.clone());

    // Only update session_status if provided
    if let Some(status) = &input.session_status {
        active_session.session_status = Set(status.clone());
    }

    match active_session.update(db.inner()).await {
        Ok(_) => Ok(Json(UpdateSessionOutput {
            success: true,
            message: "Session updated successfully".to_string(),
        })),
        Err(e) => Err(Error::database_error(e.to_string())),
    }
}

/// Delete a session by ID
#[openapi]
#[delete("/sessions/<id>")]
pub async fn delete(
    db: &State<DatabaseConnection>,
    id: String,
) -> OResult<DeleteSessionOutput> {
    let uuid = Uuid::parse_str(&id)
        .map_err(|_| Error::bad_request("Invalid UUID format".to_string()))?;

    // First check if the session exists
    let existing_session = Session::find_by_id(uuid)
        .one(db.inner())
        .await
        .map_err(|e| Error::database_error(e.to_string()))?
        .ok_or_else(|| Error::not_found("Session not found".to_string()))?;

    let active_session: session::ActiveModel = existing_session.into();

    match active_session.delete(db.inner()).await {
        Ok(_) => Ok(Json(DeleteSessionOutput {
            success: true,
            message: "Session deleted successfully".to_string(),
        })),
        Err(e) => Err(Error::database_error(e.to_string())),
    }
}
