use rocket_okapi::okapi::schemars::{self, JsonSchema};
use sea_orm::entity::prelude::*;
use serde::{Deserialize, Serialize};

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
