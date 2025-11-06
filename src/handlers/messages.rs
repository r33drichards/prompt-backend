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
use crate::entities::message::{self, Entity as Message, Model as MessageModel};
use crate::entities::prompt::Entity as Prompt;
use crate::entities::session::{self, Entity as Session};
use crate::error::{Error, OResult};

#[derive(Serialize, Deserialize, JsonSchema, Clone)]
pub struct CreateMessageInput {
    pub prompt_id: String,
    pub data: serde_json::Value,
}

#[derive(Serialize, Deserialize, JsonSchema, Clone)]
pub struct CreateMessageOutput {
    pub success: bool,
    pub message: String,
    pub id: String,
}

#[derive(Serialize, Deserialize, JsonSchema, Clone)]
pub struct MessageDto {
    pub id: String,
    pub prompt_id: String,
    pub data: serde_json::Value,
    pub created_at: String,
    pub updated_at: String,
}

impl From<MessageModel> for MessageDto {
    fn from(model: MessageModel) -> Self {
        MessageDto {
            id: model.id.to_string(),
            prompt_id: model.prompt_id.to_string(),
            data: model.data.clone(),
            created_at: model.created_at.to_string(),
            updated_at: model.updated_at.to_string(),
        }
    }
}

#[derive(Serialize, Deserialize, JsonSchema, Clone)]
pub struct ReadMessageOutput {
    pub message: MessageDto,
}

#[derive(Serialize, Deserialize, JsonSchema, Clone)]
pub struct ListMessagesOutput {
    pub messages: Vec<MessageDto>,
}

#[derive(Serialize, Deserialize, JsonSchema, Clone)]
pub struct UpdateMessageInput {
    pub data: serde_json::Value,
}

#[derive(Serialize, Deserialize, JsonSchema, Clone)]
pub struct UpdateMessageOutput {
    pub success: bool,
    pub message: String,
}

#[derive(Serialize, Deserialize, JsonSchema, Clone)]
pub struct DeleteMessageOutput {
    pub success: bool,
    pub message: String,
}

/// Create a new message
#[openapi]
#[post("/messages", data = "<input>")]
pub async fn create(
    user: AuthenticatedUser,
    db: &State<DatabaseConnection>,
    input: Json<CreateMessageInput>,
) -> OResult<CreateMessageOutput> {
    let prompt_id = Uuid::parse_str(&input.prompt_id)
        .map_err(|_| Error::bad_request("Invalid prompt_id UUID format".to_string()))?;

    // Verify prompt exists
    let prompt = Prompt::find_by_id(prompt_id)
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

    let id = Uuid::new_v4();

    let new_message = message::ActiveModel {
        id: Set(id),
        prompt_id: Set(prompt_id),
        data: Set(input.data.clone()),
        created_at: NotSet,
        updated_at: NotSet,
    };

    match new_message.insert(db.inner()).await {
        Ok(_) => Ok(Json(CreateMessageOutput {
            success: true,
            message: "Message created successfully".to_string(),
            id: id.to_string(),
        })),
        Err(e) => Err(Error::database_error(e.to_string())),
    }
}

/// Read (retrieve) a message by ID
#[openapi]
#[get("/messages/<id>")]
pub async fn read(
    user: AuthenticatedUser,
    db: &State<DatabaseConnection>,
    id: String,
) -> OResult<ReadMessageOutput> {
    let uuid =
        Uuid::parse_str(&id).map_err(|_| Error::bad_request("Invalid UUID format".to_string()))?;

    let message = Message::find_by_id(uuid)
        .one(db.inner())
        .await
        .map_err(|e| Error::database_error(e.to_string()))?
        .ok_or_else(|| Error::not_found("Message not found".to_string()))?;

    // Verify message's prompt's session belongs to user
    let prompt = Prompt::find_by_id(message.prompt_id)
        .one(db.inner())
        .await
        .map_err(|e| Error::database_error(e.to_string()))?
        .ok_or_else(|| Error::not_found("Prompt not found".to_string()))?;

    let _session = Session::find_by_id(prompt.session_id)
        .filter(session::Column::UserId.eq(&user.user_id))
        .one(db.inner())
        .await
        .map_err(|e| Error::database_error(e.to_string()))?
        .ok_or_else(|| Error::not_found("Session not found".to_string()))?;

    Ok(Json(ReadMessageOutput {
        message: message.into(),
    }))
}

/// List all messages for a prompt
#[openapi]
#[get("/prompts/<prompt_id>/messages")]
pub async fn list(
    user: AuthenticatedUser,
    db: &State<DatabaseConnection>,
    prompt_id: String,
) -> OResult<ListMessagesOutput> {
    let prompt_uuid = Uuid::parse_str(&prompt_id)
        .map_err(|_| Error::bad_request("Invalid prompt_id UUID format".to_string()))?;

    // Verify prompt exists
    let prompt = Prompt::find_by_id(prompt_uuid)
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

    match Message::find()
        .filter(message::Column::PromptId.eq(prompt_uuid))
        .order_by_asc(message::Column::CreatedAt)
        .all(db.inner())
        .await
    {
        Ok(messages) => Ok(Json(ListMessagesOutput {
            messages: messages.into_iter().map(|m| m.into()).collect(),
        })),
        Err(e) => Err(Error::database_error(e.to_string())),
    }
}

/// Update an existing message (PUT - full replacement)
#[openapi]
#[put("/messages/<id>", data = "<input>")]
pub async fn update(
    user: AuthenticatedUser,
    db: &State<DatabaseConnection>,
    id: String,
    input: Json<UpdateMessageInput>,
) -> OResult<UpdateMessageOutput> {
    let uuid =
        Uuid::parse_str(&id).map_err(|_| Error::bad_request("Invalid UUID format".to_string()))?;

    let message = Message::find_by_id(uuid)
        .one(db.inner())
        .await
        .map_err(|e| Error::database_error(e.to_string()))?
        .ok_or_else(|| Error::not_found("Message not found".to_string()))?;

    // Verify message's prompt's session belongs to user
    let prompt = Prompt::find_by_id(message.prompt_id)
        .one(db.inner())
        .await
        .map_err(|e| Error::database_error(e.to_string()))?
        .ok_or_else(|| Error::not_found("Prompt not found".to_string()))?;

    let _session = Session::find_by_id(prompt.session_id)
        .filter(session::Column::UserId.eq(&user.user_id))
        .one(db.inner())
        .await
        .map_err(|e| Error::database_error(e.to_string()))?
        .ok_or_else(|| Error::not_found("Session not found".to_string()))?;

    let mut active_message: message::ActiveModel = message.into();
    active_message.data = Set(input.data.clone());

    match active_message.update(db.inner()).await {
        Ok(_) => Ok(Json(UpdateMessageOutput {
            success: true,
            message: "Message updated successfully".to_string(),
        })),
        Err(e) => Err(Error::database_error(e.to_string())),
    }
}

/// Delete a message by ID
#[openapi]
#[delete("/messages/<id>")]
pub async fn delete(
    user: AuthenticatedUser,
    db: &State<DatabaseConnection>,
    id: String,
) -> OResult<DeleteMessageOutput> {
    let uuid =
        Uuid::parse_str(&id).map_err(|_| Error::bad_request("Invalid UUID format".to_string()))?;

    let message = Message::find_by_id(uuid)
        .one(db.inner())
        .await
        .map_err(|e| Error::database_error(e.to_string()))?
        .ok_or_else(|| Error::not_found("Message not found".to_string()))?;

    // Verify message's prompt's session belongs to user
    let prompt = Prompt::find_by_id(message.prompt_id)
        .one(db.inner())
        .await
        .map_err(|e| Error::database_error(e.to_string()))?
        .ok_or_else(|| Error::not_found("Prompt not found".to_string()))?;

    let _session = Session::find_by_id(prompt.session_id)
        .filter(session::Column::UserId.eq(&user.user_id))
        .one(db.inner())
        .await
        .map_err(|e| Error::database_error(e.to_string()))?
        .ok_or_else(|| Error::not_found("Session not found".to_string()))?;

    let active_message: message::ActiveModel = message.into();

    match active_message.delete(db.inner()).await {
        Ok(_) => Ok(Json(DeleteMessageOutput {
            success: true,
            message: "Message deleted successfully".to_string(),
        })),
        Err(e) => Err(Error::database_error(e.to_string())),
    }
}
