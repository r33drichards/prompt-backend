use rocket_okapi::okapi::schemars::{self, JsonSchema};
use sea_orm::entity::prelude::*;
use serde::{Deserialize, Serialize};

/// Repository configuration for a session
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct RepoConfig {
    pub url: String,
    pub branch: String,
}

/// Repositories configuration
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct ReposConfig {
    pub repos: Vec<RepoConfig>,
}

#[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel, Serialize, Deserialize)]
#[sea_orm(table_name = "session")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    pub id: Uuid,
    #[sea_orm(column_type = "JsonBinary", nullable)]
    pub sbx_config: Option<Json>,
    #[sea_orm(nullable)]
    pub parent: Option<Uuid>,
    #[sea_orm(nullable)]
    pub branch: Option<String>,
    /// Deprecated: Use `repos` field instead
    #[sea_orm(nullable)]
    pub repo: Option<String>,
    #[sea_orm(nullable)]
    pub target_branch: Option<String>,
    #[sea_orm(nullable)]
    pub title: Option<String>,
    pub ui_status: UiStatus,
    pub created_at: DateTimeWithTimeZone,
    pub updated_at: DateTimeWithTimeZone,
    #[sea_orm(nullable)]
    pub deleted_at: Option<DateTimeWithTimeZone>,
    #[sea_orm(column_name = "user_id")]
    pub user_id: String,
    #[sea_orm(default_value = 0)]
    pub ip_return_retry_count: i32,
    #[sea_orm(nullable)]
    pub cancellation_status: Option<CancellationStatus>,
    #[sea_orm(nullable)]
    pub cancelled_at: Option<DateTimeWithTimeZone>,
    #[sea_orm(nullable)]
    pub cancelled_by: Option<String>,
    #[sea_orm(nullable)]
    pub process_pid: Option<i32>,
    #[sea_orm(nullable)]
    pub preserve_sandbox: Option<bool>,
    /// New field: supports multiple repositories
    #[sea_orm(column_type = "JsonBinary", nullable)]
    pub repos: Option<Json>,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {
    #[sea_orm(has_many = "super::prompt::Entity")]
    Prompt,
}

impl Related<super::prompt::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::Prompt.def()
    }
}

impl ActiveModelBehavior for ActiveModel {}

impl Model {
    /// Get the primary repository URL, supporting both old and new formats
    /// Returns the first repo from `repos` field if available, otherwise falls back to `repo` field
    pub fn get_repo_url(&self) -> Option<String> {
        // Try new format first
        if let Some(repos_json) = &self.repos {
            if let Ok(repos_config) = serde_json::from_value::<ReposConfig>(repos_json.clone()) {
                if let Some(first_repo) = repos_config.repos.first() {
                    return Some(first_repo.url.clone());
                }
            }
        }
        
        // Fall back to old format
        self.repo.clone()
    }

    /// Get the primary repository branch
    /// Returns the first repo's branch from `repos` field if available, otherwise falls back to `target_branch` field
    pub fn get_repo_branch(&self) -> Option<String> {
        // Try new format first
        if let Some(repos_json) = &self.repos {
            if let Ok(repos_config) = serde_json::from_value::<ReposConfig>(repos_json.clone()) {
                if let Some(first_repo) = repos_config.repos.first() {
                    return Some(first_repo.branch.clone());
                }
            }
        }
        
        // Fall back to old format
        self.target_branch.clone()
    }

    /// Get all repositories
    /// Returns vector of RepoConfig, converting from old format if necessary
    pub fn get_all_repos(&self) -> Vec<RepoConfig> {
        // Try new format first
        if let Some(repos_json) = &self.repos {
            if let Ok(repos_config) = serde_json::from_value::<ReposConfig>(repos_json.clone()) {
                return repos_config.repos;
            }
        }
        
        // Fall back to old format - create single repo from repo and target_branch
        if let (Some(url), Some(branch)) = (&self.repo, &self.target_branch) {
            return vec![RepoConfig {
                url: url.clone(),
                branch: branch.clone(),
            }];
        }
        
        vec![]
    }
}

#[derive(
    Debug, Clone, PartialEq, Eq, Serialize, Deserialize, EnumIter, DeriveActiveEnum, JsonSchema,
)]
#[sea_orm(rs_type = "String", db_type = "String(Some(50))")]
pub enum UiStatus {
    #[sea_orm(string_value = "pending")]
    Pending,
    #[sea_orm(string_value = "in_progress")]
    InProgress,
    #[sea_orm(string_value = "needs_review")]
    NeedsReview,
    #[sea_orm(string_value = "needs_review_ip_returned")]
    NeedsReviewIpReturned,
    #[sea_orm(string_value = "archived")]
    Archived,
}

#[derive(
    Debug, Clone, PartialEq, Eq, Serialize, Deserialize, EnumIter, DeriveActiveEnum, JsonSchema,
)]
#[sea_orm(rs_type = "String", db_type = "String(Some(50))")]
pub enum CancellationStatus {
    #[sea_orm(string_value = "requested")]
    Requested,
    #[sea_orm(string_value = "cancelled")]
    Cancelled,
}
