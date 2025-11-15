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
use crate::entities::prompt;
use crate::entities::session::{
    self, CancellationStatus, Entity as Session, Model as SessionModel, UiStatus,
};
use crate::entities::session_repository;
use crate::error::{Error, OResult};
use crate::services::anthropic;
use chrono::Utc;

#[derive(Serialize, Deserialize, JsonSchema, Clone)]
pub struct RepositoryInput {
    pub repo: String,
    pub target_branch: String,
}

#[derive(Serialize, Deserialize, JsonSchema, Clone)]
pub struct CreateSessionInput {
    pub parent: Option<String>,
    // Support both single repo (backward compatibility) and multiple repos
    #[serde(skip_serializing_if = "Option::is_none")]
    pub repo: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub target_branch: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub repositories: Option<Vec<RepositoryInput>>,
}

#[derive(Serialize, Deserialize, JsonSchema, Clone)]
pub struct CreateSessionOutput {
    pub success: bool,
    pub message: String,
    pub id: String,
}

#[derive(Serialize, Deserialize, JsonSchema, Clone)]
pub struct CreateSessionWithPromptInput {
    // Support both single repo (backward compatibility) and multiple repos
    #[serde(skip_serializing_if = "Option::is_none")]
    pub repo: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub target_branch: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub repositories: Option<Vec<RepositoryInput>>,
    pub messages: serde_json::Value,
    pub parent_id: Option<String>,
}

#[derive(Serialize, Deserialize, JsonSchema, Clone)]
#[serde(rename_all = "camelCase")]
pub struct CreateSessionWithPromptOutput {
    pub success: bool,
    pub message: String,
    pub session_id: String,
    pub prompt_id: String,
}

#[derive(Serialize, Deserialize, JsonSchema, Clone)]
#[serde(rename_all = "camelCase")]
pub struct RepositoryDto {
    pub id: String,
    pub repo: String,
    pub target_branch: String,
}

#[derive(Serialize, Deserialize, JsonSchema, Clone)]
#[serde(rename_all = "camelCase")]
pub struct SessionDto {
    pub id: String,
    pub sbx_config: Option<serde_json::Value>,
    pub parent: Option<String>,
    pub branch: Option<String>,
    // Keep for backward compatibility
    pub repo: Option<String>,
    pub target_branch: Option<String>,
    // New field for multiple repositories
    #[serde(skip_serializing_if = "Option::is_none")]
    pub repositories: Option<Vec<RepositoryDto>>,
    pub title: Option<String>,
    pub ui_status: UiStatus,
    pub created_at: String,
    pub updated_at: String,
    pub deleted_at: Option<String>,
    pub cancellation_status: Option<CancellationStatus>,
    pub cancelled_at: Option<String>,
    pub cancelled_by: Option<String>,
}

impl From<SessionModel> for SessionDto {
    fn from(model: SessionModel) -> Self {
        SessionDto {
            id: model.id.to_string(),
            sbx_config: model.sbx_config,
            parent: model.parent.map(|p| p.to_string()),
            branch: model.branch,
            repo: model.repo,
            target_branch: model.target_branch,
            repositories: None, // Will be populated by handler if needed
            title: model.title,
            ui_status: model.ui_status,
            created_at: model.created_at.to_string(),
            updated_at: model.updated_at.to_string(),
            deleted_at: model.deleted_at.map(|d| d.to_string()),
            cancellation_status: model.cancellation_status,
            cancelled_at: model.cancelled_at.map(|d| d.to_string()),
            cancelled_by: model.cancelled_by,
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
    pub sbx_config: Option<serde_json::Value>,
    pub parent: Option<String>,
    pub branch: Option<String>,
    pub repo: Option<String>,
    pub target_branch: Option<String>,
    pub title: Option<String>,
    pub ui_status: Option<UiStatus>,
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

#[derive(Serialize, Deserialize, JsonSchema, Clone)]
pub struct CancelSessionOutput {
    pub success: bool,
    pub message: String,
}

/// Create a new session
#[openapi]
#[post("/sessions", data = "<input>")]
pub async fn create(
    user: AuthenticatedUser,
    db: &State<DatabaseConnection>,
    input: Json<CreateSessionInput>,
) -> OResult<CreateSessionOutput> {
    let id = Uuid::new_v4();

    let parent = match &input.parent {
        Some(p) => Some(
            Uuid::parse_str(p)
                .map_err(|_| Error::bad_request("Invalid parent UUID format".to_string()))?,
        ),
        None => None,
    };

    let prompt = "todo";

    // Generate title using Anthropic Haiku
    let title = anthropic::generate_session_title(&input.repo, &input.target_branch, prompt)
        .await
        .unwrap_or_else(|e| {
            tracing::warn!("Failed to generate session title: {}", e);
            "Untitled Session".to_string()
        });

    // Generate branch name
    let generated_branch =
        anthropic::generate_branch_name(&input.repo, &input.target_branch, prompt, &id.to_string())
            .await
            .unwrap_or_else(|e| {
                tracing::warn!("Failed to generate branch name: {}", e);
                format!("claude/session-{}", &id.to_string()[..24])
            });

    let new_session = session::ActiveModel {
        id: Set(id),
        sbx_config: Set(None),
        parent: Set(parent),
        branch: Set(Some(generated_branch)),
        repo: Set(Some(input.repo.clone())),
        target_branch: Set(Some(input.target_branch.clone())),
        title: Set(Some(title)),
        ui_status: Set(UiStatus::Pending),
        user_id: Set(user.user_id.clone()),
        ip_return_retry_count: Set(0),
        created_at: NotSet,
        updated_at: NotSet,
        deleted_at: Set(None),
        cancellation_status: Set(None),
        cancelled_at: Set(None),
        cancelled_by: Set(None),
        process_pid: Set(None),
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

/// Create a new session with an initial prompt
#[openapi]
#[post("/sessions/with-prompt", data = "<input>")]
pub async fn create_with_prompt(
    user: AuthenticatedUser,
    db: &State<DatabaseConnection>,
    input: Json<CreateSessionWithPromptInput>,
) -> OResult<CreateSessionWithPromptOutput> {
    let session_id = Uuid::new_v4();

    let parent = match &input.parent_id {
        Some(p) => Some(
            Uuid::parse_str(p)
                .map_err(|_| Error::bad_request("Invalid parent UUID format".to_string()))?,
        ),
        None => None,
    };

    // Extract prompt content for title/branch generation
    // Try to get "content" field from messages, or use the entire JSON as string
    let prompt_content = input
        .messages
        .get("content")
        .and_then(|v| v.as_str())
        .unwrap_or("New session");

    // Determine which repositories to use
    // Priority: repositories array > single repo/target_branch
    let repos_to_create: Vec<RepositoryInput> = if let Some(repos) = &input.repositories {
        // Use the new multi-repo array
        repos.clone()
    } else if let (Some(repo), Some(target_branch)) = (&input.repo, &input.target_branch) {
        // Fall back to legacy single repo
        vec![RepositoryInput {
            repo: repo.clone(),
            target_branch: target_branch.clone(),
        }]
    } else {
        return Err(Error::bad_request(
            "Either 'repositories' array or 'repo'+'target_branch' must be provided".to_string(),
        ));
    };

    if repos_to_create.is_empty() {
        return Err(Error::bad_request(
            "At least one repository must be provided".to_string(),
        ));
    }

    // Use first repo for title/branch generation and legacy fields
    let first_repo = &repos_to_create[0];

    // Generate title using Anthropic Haiku
    let title = anthropic::generate_session_title(
        &Some(first_repo.repo.clone()),
        &Some(first_repo.target_branch.clone()),
        prompt_content,
    )
    .await
    .unwrap_or_else(|e| {
        tracing::warn!("Failed to generate session title: {}", e);
        "Untitled Session".to_string()
    });

    // Generate branch name
    let generated_branch = anthropic::generate_branch_name(
        &Some(first_repo.repo.clone()),
        &Some(first_repo.target_branch.clone()),
        prompt_content,
        &session_id.to_string(),
    )
    .await
    .unwrap_or_else(|e| {
        tracing::warn!("Failed to generate branch name: {}", e);
        format!("claude/session-{}", &session_id.to_string()[..24])
    });

    // Create session with legacy fields populated from first repo (backward compatibility)
    let new_session = session::ActiveModel {
        id: Set(session_id),
        sbx_config: Set(None),
        parent: Set(parent),
        branch: Set(Some(generated_branch)),
        repo: Set(Some(first_repo.repo.clone())),
        target_branch: Set(Some(first_repo.target_branch.clone())),
        title: Set(Some(title)),
        ui_status: Set(UiStatus::Pending),
        user_id: Set(user.user_id.clone()),
        ip_return_retry_count: Set(0),
        created_at: NotSet,
        updated_at: NotSet,
        deleted_at: Set(None),
        cancellation_status: Set(None),
        cancelled_at: Set(None),
        cancelled_by: Set(None),
        process_pid: Set(None),
    };

    // Insert the session
    new_session
        .insert(db.inner())
        .await
        .map_err(|e| Error::database_error(e.to_string()))?;

    // Create session_repository entries for all repos
    for repo_input in repos_to_create {
        let repo_id = Uuid::new_v4();
        let new_repo = session_repository::ActiveModel {
            id: Set(repo_id),
            session_id: Set(session_id),
            repo: Set(repo_input.repo),
            target_branch: Set(repo_input.target_branch),
            created_at: NotSet,
            updated_at: NotSet,
        };

        new_repo
            .insert(db.inner())
            .await
            .map_err(|e| Error::database_error(e.to_string()))?;
    }

    // Create the initial prompt
    let prompt_id = Uuid::new_v4();
    let new_prompt = prompt::ActiveModel {
        id: Set(prompt_id),
        session_id: Set(session_id),
        data: Set(input.messages.clone()),
        created_at: NotSet,
        updated_at: NotSet,
    };

    new_prompt
        .insert(db.inner())
        .await
        .map_err(|e| Error::database_error(e.to_string()))?;

    Ok(Json(CreateSessionWithPromptOutput {
        success: true,
        message: "Session and prompt created successfully".to_string(),
        session_id: session_id.to_string(),
        prompt_id: prompt_id.to_string(),
    }))
}

/// Read (retrieve) a session by ID
#[openapi]
#[get("/sessions/<id>")]
pub async fn read(
    user: AuthenticatedUser,
    db: &State<DatabaseConnection>,
    id: String,
) -> OResult<ReadSessionOutput> {
    let uuid =
        Uuid::parse_str(&id).map_err(|_| Error::bad_request("Invalid UUID format".to_string()))?;

    let session = Session::find_by_id(uuid)
        .filter(session::Column::UserId.eq(&user.user_id))
        .one(db.inner())
        .await
        .map_err(|e| Error::database_error(e.to_string()))?
        .ok_or_else(|| Error::not_found("Session not found".to_string()))?;

    // Load related repositories
    let repositories = session_repository::Entity::find()
        .filter(session_repository::Column::SessionId.eq(uuid))
        .all(db.inner())
        .await
        .map_err(|e| Error::database_error(e.to_string()))?;

    // Convert to DTO
    let mut session_dto: SessionDto = session.into();
    
    // Populate repositories array if any exist
    if !repositories.is_empty() {
        session_dto.repositories = Some(
            repositories
                .into_iter()
                .map(|r| RepositoryDto {
                    id: r.id.to_string(),
                    repo: r.repo,
                    target_branch: r.target_branch,
                })
                .collect(),
        );
    }

    Ok(Json(ReadSessionOutput {
        session: session_dto,
    }))
}

/// List all sessions
#[openapi]
#[get("/sessions")]
pub async fn list(
    user: AuthenticatedUser,
    db: &State<DatabaseConnection>,
) -> OResult<ListSessionsOutput> {
    let sessions = Session::find()
        .filter(session::Column::UserId.eq(&user.user_id))
        .order_by_asc(session::Column::Id)
        .all(db.inner())
        .await
        .map_err(|e| Error::database_error(e.to_string()))?;

    // Collect all session IDs
    let session_ids: Vec<Uuid> = sessions.iter().map(|s| s.id).collect();

    // Batch query all repositories for these sessions
    let all_repositories = if !session_ids.is_empty() {
        session_repository::Entity::find()
            .filter(session_repository::Column::SessionId.is_in(session_ids))
            .all(db.inner())
            .await
            .map_err(|e| Error::database_error(e.to_string()))?
    } else {
        vec![]
    };

    // Group repositories by session_id
    let mut repos_by_session: std::collections::HashMap<Uuid, Vec<RepositoryDto>> =
        std::collections::HashMap::new();
    
    for repo in all_repositories {
        repos_by_session
            .entry(repo.session_id)
            .or_insert_with(Vec::new)
            .push(RepositoryDto {
                id: repo.id.to_string(),
                repo: repo.repo,
                target_branch: repo.target_branch,
            });
    }

    // Convert sessions to DTOs and populate repositories
    let session_dtos: Vec<SessionDto> = sessions
        .into_iter()
        .map(|s| {
            let session_id = s.id;
            let mut dto: SessionDto = s.into();
            
            // Populate repositories if they exist for this session
            if let Some(repos) = repos_by_session.get(&session_id) {
                dto.repositories = Some(repos.clone());
            }
            
            dto
        })
        .collect();

    Ok(Json(ListSessionsOutput {
        sessions: session_dtos,
    }))
}

/// Update an existing session (PUT - partial update, only provided fields are updated)
#[openapi]
#[put("/sessions/<id>", data = "<input>")]
pub async fn update(
    user: AuthenticatedUser,
    db: &State<DatabaseConnection>,
    id: String,
    input: Json<UpdateSessionInput>,
) -> OResult<UpdateSessionOutput> {
    let uuid =
        Uuid::parse_str(&id).map_err(|_| Error::bad_request("Invalid UUID format".to_string()))?;

    let parent = match &input.parent {
        Some(p) => Some(
            Uuid::parse_str(p)
                .map_err(|_| Error::bad_request("Invalid parent UUID format".to_string()))?,
        ),
        None => None,
    };

    // Verify session exists and belongs to user
    let existing_session = Session::find_by_id(uuid)
        .filter(session::Column::UserId.eq(&user.user_id))
        .one(db.inner())
        .await
        .map_err(|e| Error::database_error(e.to_string()))?
        .ok_or_else(|| Error::not_found("Session not found".to_string()))?;

    let mut active_session: session::ActiveModel = existing_session.into();

    // Only update fields that are provided (Some)
    if input.sbx_config.is_some() {
        active_session.sbx_config = Set(input.sbx_config.clone());
    }
    if parent.is_some() || input.parent.is_some() {
        active_session.parent = Set(parent);
    }
    if input.branch.is_some() {
        active_session.branch = Set(input.branch.clone());
    }
    if input.repo.is_some() {
        active_session.repo = Set(input.repo.clone());
    }
    if input.target_branch.is_some() {
        active_session.target_branch = Set(input.target_branch.clone());
    }
    if input.title.is_some() {
        active_session.title = Set(input.title.clone());
    }
    if let Some(ui_status) = &input.ui_status {
        active_session.ui_status = Set(ui_status.clone());
    }

    // Explicitly update the updated_at timestamp
    active_session.updated_at = Set(Utc::now().into());

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
    user: AuthenticatedUser,
    db: &State<DatabaseConnection>,
    id: String,
) -> OResult<DeleteSessionOutput> {
    let uuid =
        Uuid::parse_str(&id).map_err(|_| Error::bad_request("Invalid UUID format".to_string()))?;

    // Verify session exists and belongs to user before deleting
    let existing_session = Session::find_by_id(uuid)
        .filter(session::Column::UserId.eq(&user.user_id))
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

/// Cancel a session by ID
#[openapi]
#[post("/sessions/<id>/cancel")]
pub async fn cancel(
    user: AuthenticatedUser,
    db: &State<DatabaseConnection>,
    id: String,
) -> OResult<CancelSessionOutput> {
    let uuid =
        Uuid::parse_str(&id).map_err(|_| Error::bad_request("Invalid UUID format".to_string()))?;

    // Verify session exists and belongs to user
    let existing_session = Session::find_by_id(uuid)
        .filter(session::Column::UserId.eq(&user.user_id))
        .one(db.inner())
        .await
        .map_err(|e| Error::database_error(e.to_string()))?
        .ok_or_else(|| Error::not_found("Session not found".to_string()))?;

    // Check if already cancelled
    if let Some(CancellationStatus::Cancelled) = existing_session.cancellation_status {
        return Ok(Json(CancelSessionOutput {
            success: true,
            message: "Session is already cancelled".to_string(),
        }));
    }

    // Update session to mark as cancellation requested
    let mut active_session: session::ActiveModel = existing_session.into();
    active_session.cancellation_status = Set(Some(CancellationStatus::Requested));
    active_session.cancelled_at = Set(Some(Utc::now().into()));
    active_session.cancelled_by = Set(Some(user.user_id.clone()));

    match active_session.update(db.inner()).await {
        Ok(_) => Ok(Json(CancelSessionOutput {
            success: true,
            message: "Session cancellation requested successfully".to_string(),
        })),
        Err(e) => Err(Error::database_error(e.to_string())),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_create_session_with_prompt_output_serialization() {
        let output = CreateSessionWithPromptOutput {
            success: true,
            message: "Test message".to_string(),
            session_id: "550e8400-e29b-41d4-a716-446655440000".to_string(),
            prompt_id: "660e8400-e29b-41d4-a716-446655440001".to_string(),
        };

        let json = serde_json::to_string(&output).expect("Failed to serialize");

        // Verify the fields are in camelCase
        assert!(
            json.contains("\"sessionId\""),
            "Expected sessionId in camelCase, got: {}",
            json
        );
        assert!(
            json.contains("\"promptId\""),
            "Expected promptId in camelCase, got: {}",
            json
        );

        // Verify no snake_case fields
        assert!(
            !json.contains("\"session_id\""),
            "Found snake_case session_id, expected camelCase"
        );
        assert!(
            !json.contains("\"prompt_id\""),
            "Found snake_case prompt_id, expected camelCase"
        );
    }
}
