use rocket_okapi::okapi::schemars::{self, JsonSchema};
use sea_orm::entity::prelude::*;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel, Serialize, Deserialize)]
#[sea_orm(table_name = "session")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    pub id: Uuid,
    #[sea_orm(column_type = "JsonBinary", nullable)]
    pub messages: Option<Json>,
    #[sea_orm(column_name = "inbox_status")]
    pub inbox_status: InboxStatus,
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
    pub session_status: SessionStatus,
    pub created_at: DateTimeWithTimeZone,
    pub updated_at: DateTimeWithTimeZone,
    #[sea_orm(nullable)]
    pub deleted_at: Option<DateTimeWithTimeZone>,
    #[sea_orm(column_name = "user_id")]
    pub user_id: String,
    #[sea_orm(nullable)]
    pub github_token: Option<String>,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}

#[derive(
    Debug, Clone, PartialEq, Eq, Serialize, Deserialize, EnumIter, DeriveActiveEnum, JsonSchema,
)]
#[sea_orm(rs_type = "String", db_type = "String(Some(50))")]
pub enum InboxStatus {
    #[sea_orm(string_value = "pending")]
    Pending,
    #[sea_orm(string_value = "active")]
    Active,
    #[sea_orm(string_value = "completed")]
    Completed,
    #[sea_orm(string_value = "archived")]
    Archived,
}

#[derive(
    Debug, Clone, PartialEq, Eq, Serialize, Deserialize, EnumIter, DeriveActiveEnum, JsonSchema,
)]
#[sea_orm(rs_type = "String", db_type = "String(Some(50))")]
pub enum SessionStatus {
    #[sea_orm(string_value = "active")]
    Active,
    #[sea_orm(string_value = "archived")]
    Archived,
}
