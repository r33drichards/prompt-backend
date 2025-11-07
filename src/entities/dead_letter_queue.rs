use rocket_okapi::okapi::schemars::{self, JsonSchema};
use sea_orm::entity::prelude::*;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel, Serialize, Deserialize)]
#[sea_orm(table_name = "dead_letter_queue")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    pub id: Uuid,
    pub task_type: String,
    pub entity_id: Uuid,
    #[sea_orm(column_type = "JsonBinary", nullable)]
    pub entity_data: Option<Json>,
    pub retry_count: i32,
    pub last_error: String,
    pub last_error_at: DateTimeWithTimeZone,
    pub first_failed_at: DateTimeWithTimeZone,
    pub status: DlqStatus,
    #[sea_orm(nullable)]
    pub resolution_notes: Option<String>,
    pub created_at: DateTimeWithTimeZone,
    pub updated_at: DateTimeWithTimeZone,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}

#[derive(
    Debug, Clone, PartialEq, Eq, Serialize, Deserialize, EnumIter, DeriveActiveEnum, JsonSchema,
)]
#[sea_orm(rs_type = "String", db_type = "String(Some(50))")]
pub enum DlqStatus {
    #[sea_orm(string_value = "pending")]
    Pending,
    #[sea_orm(string_value = "resolved")]
    Resolved,
    #[sea_orm(string_value = "abandoned")]
    Abandoned,
}
