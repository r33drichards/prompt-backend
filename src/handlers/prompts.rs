use rocket::serde::json::Json;
use rocket::serde::{Deserialize, Serialize};
use rocket::State;
use rocket_okapi::okapi::schemars::JsonSchema;
use rocket_okapi::openapi;
use sea_orm::{
    ActiveModelTrait, ColumnTrait, DatabaseConnection, EntityTrait, NotSet, QueryFilter,
    QueryOrder, Set,
};
use uuid::Uuid;

use crate::auth::AuthenticatedUser;
use crate::entities::prompt::{self, Entity as Prompt, InboxStatus, Model as PromptModel};
use crate::entities::session::{self, Entity as Session};
use crate::error::{Error, OResult};

#[derive(Serialize, Deserialize, JsonSchema, Clone)]
pub struct CreatePromptInput {
    pub session_id: String,
    pub data: serde_json::Value,
}

#[derive(Serialize, Deserialize, JsonSchema, Clone)]
pub struct CreatePromptOutput {
    pub success: bool,
    pub message: String,
    pub id: String,
}

#[derive(Serialize, Deserialize, JsonSchema, Clone)]
pub struct PromptDto {
    pub id: String,
    pub session_id: String,
    pub data: serde_json::Value,
    pub inbox_status: InboxStatus,
    pub created_at: String,
    pub updated_at: String,
}

impl From<PromptModel> for PromptDto {
    fn from(model: PromptModel) -> Self {
        PromptDto {
            id: model.id.to_string(),
            session_id: model.session_id.to_string(),
            data: model.data.clone(),
            inbox_status: model.inbox_status,
            created_at: model.created_at.to_string(),
            updated_at: model.updated_at.to_string(),
        }
    }
}

#[derive(Serialize, Deserialize, JsonSchema, Clone)]
pub struct ReadPromptOutput {
    pub prompt: PromptDto,
}

#[derive(Serialize, Deserialize, JsonSchema, Clone)]
pub struct ListPromptsOutput {
    pub prompts: Vec<PromptDto>,
}

#[derive(Serialize, Deserialize, JsonSchema, Clone)]
pub struct UpdatePromptInput {
    pub data: serde_json::Value,
}

#[derive(Serialize, Deserialize, JsonSchema, Clone)]
pub struct UpdatePromptOutput {
    pub success: bool,
    pub message: String,
}

#[derive(Serialize, Deserialize, JsonSchema, Clone)]
pub struct DeletePromptOutput {
    pub success: bool,
    pub message: String,
}

/// Create a new prompt
#[openapi]
#[post("/prompts", data = "<input>")]
pub async fn create(
    user: AuthenticatedUser,
    db: &State<DatabaseConnection>,
    input: Json<CreatePromptInput>,
) -> OResult<CreatePromptOutput> {
    let session_id = Uuid::parse_str(&input.session_id)
        .map_err(|_| Error::bad_request("Invalid session_id UUID format".to_string()))?;

    // Verify session exists and belongs to user
    let _session = Session::find_by_id(session_id)
        .filter(session::Column::UserId.eq(&user.user_id))
        .one(db.inner())
        .await
        .map_err(|e| Error::database_error(e.to_string()))?
        .ok_or_else(|| Error::not_found("Session not found".to_string()))?;

    let id = Uuid::new_v4();

    let new_prompt = prompt::ActiveModel {
        id: Set(id),
        session_id: Set(session_id),
        data: Set(input.data.clone()),
        inbox_status: Set(InboxStatus::Pending),
        created_at: NotSet,
        updated_at: NotSet,
    };

    match new_prompt.insert(db.inner()).await {
        Ok(_) => Ok(Json(CreatePromptOutput {
            success: true,
            message: "Prompt created successfully".to_string(),
            id: id.to_string(),
        })),
        Err(e) => Err(Error::database_error(e.to_string())),
    }
}

/// Read (retrieve) a prompt by ID
#[openapi]
#[get("/prompts/<id>")]
pub async fn read(
    user: AuthenticatedUser,
    db: &State<DatabaseConnection>,
    id: String,
) -> OResult<ReadPromptOutput> {
    let uuid =
        Uuid::parse_str(&id).map_err(|_| Error::bad_request("Invalid UUID format".to_string()))?;

    let prompt = Prompt::find_by_id(uuid)
        .one(db.inner())
        .await
        .map_err(|e| Error::database_error(e.to_string()))?
        .ok_or_else(|| Error::not_found("Prompt not found".to_string()))?;

    // Verify prompt's session belongs to user
    let _session = Session::find_by_id(prompt.session_id)
        .filter(session::Column::UserId.eq(&user.user_id))
        .one(db.inner())
        .await
        .map_err(|e| Error::database_error(e.to_string()))?
        .ok_or_else(|| Error::not_found("Session not found".to_string()))?;

    Ok(Json(ReadPromptOutput {
        prompt: prompt.into(),
    }))
}

/// List all prompts for a session
#[openapi]
#[get("/sessions/<session_id>/prompts")]
pub async fn list(
    user: AuthenticatedUser,
    db: &State<DatabaseConnection>,
    session_id: String,
) -> OResult<ListPromptsOutput> {
    let session_uuid = Uuid::parse_str(&session_id)
        .map_err(|_| Error::bad_request("Invalid session_id UUID format".to_string()))?;

    // Verify session belongs to user
    let _session = Session::find_by_id(session_uuid)
        .filter(session::Column::UserId.eq(&user.user_id))
        .one(db.inner())
        .await
        .map_err(|e| Error::database_error(e.to_string()))?
        .ok_or_else(|| Error::not_found("Session not found".to_string()))?;

    match Prompt::find()
        .filter(prompt::Column::SessionId.eq(session_uuid))
        .order_by_asc(prompt::Column::CreatedAt)
        .all(db.inner())
        .await
    {
        Ok(prompts) => Ok(Json(ListPromptsOutput {
            prompts: prompts.into_iter().map(|p| p.into()).collect(),
        })),
        Err(e) => Err(Error::database_error(e.to_string())),
    }
}

/// Update an existing prompt (PUT - full replacement)
#[openapi]
#[put("/prompts/<id>", data = "<input>")]
pub async fn update(
    user: AuthenticatedUser,
    db: &State<DatabaseConnection>,
    id: String,
    input: Json<UpdatePromptInput>,
) -> OResult<UpdatePromptOutput> {
    let uuid =
        Uuid::parse_str(&id).map_err(|_| Error::bad_request("Invalid UUID format".to_string()))?;

    let prompt = Prompt::find_by_id(uuid)
        .one(db.inner())
        .await
        .map_err(|e| Error::database_error(e.to_string()))?
        .ok_or_else(|| Error::not_found("Prompt not found".to_string()))?;

    // Verify prompt's session belongs to user
    let _session = Session::find_by_id(prompt.session_id)
        .filter(session::Column::UserId.eq(&user.user_id))
        .one(db.inner())
        .await
        .map_err(|e| Error::database_error(e.to_string()))?
        .ok_or_else(|| Error::not_found("Session not found".to_string()))?;

    let mut active_prompt: prompt::ActiveModel = prompt.into();
    active_prompt.data = Set(input.data.clone());

    match active_prompt.update(db.inner()).await {
        Ok(_) => Ok(Json(UpdatePromptOutput {
            success: true,
            message: "Prompt updated successfully".to_string(),
        })),
        Err(e) => Err(Error::database_error(e.to_string())),
    }
}

/// Delete a prompt by ID
#[openapi]
#[delete("/prompts/<id>")]
pub async fn delete(
    user: AuthenticatedUser,
    db: &State<DatabaseConnection>,
    id: String,
) -> OResult<DeletePromptOutput> {
    let uuid =
        Uuid::parse_str(&id).map_err(|_| Error::bad_request("Invalid UUID format".to_string()))?;

    let prompt = Prompt::find_by_id(uuid)
        .one(db.inner())
        .await
        .map_err(|e| Error::database_error(e.to_string()))?
        .ok_or_else(|| Error::not_found("Prompt not found".to_string()))?;

    // Verify prompt's session belongs to user
    let _session = Session::find_by_id(prompt.session_id)
        .filter(session::Column::UserId.eq(&user.user_id))
        .one(db.inner())
        .await
        .map_err(|e| Error::database_error(e.to_string()))?
        .ok_or_else(|| Error::not_found("Session not found".to_string()))?;

    let active_prompt: prompt::ActiveModel = prompt.into();

    match active_prompt.delete(db.inner()).await {
        Ok(_) => Ok(Json(DeletePromptOutput {
            success: true,
            message: "Prompt deleted successfully".to_string(),
        })),
        Err(e) => Err(Error::database_error(e.to_string())),
    }
}
