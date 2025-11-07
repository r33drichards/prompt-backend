use rocket_okapi::okapi::schemars::{self, JsonSchema};
use sea_orm::entity::prelude::*;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel, Serialize, Deserialize)]
#[sea_orm(table_name = "prompt")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    pub id: Uuid,
    #[sea_orm(column_name = "session_id")]
    pub session_id: Uuid,
    #[sea_orm(column_type = "JsonBinary")]
    pub data: Json,
    #[sea_orm(column_name = "inbox_status")]
    pub inbox_status: InboxStatus,
    pub processing_attempts: i32,
    #[sea_orm(nullable)]
    pub last_error: Option<String>,
    #[sea_orm(nullable)]
    pub last_attempt_at: Option<DateTimeWithTimeZone>,
    #[sea_orm(nullable)]
    pub completed_at: Option<DateTimeWithTimeZone>,
    pub created_at: DateTimeWithTimeZone,
    pub updated_at: DateTimeWithTimeZone,
}

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
    #[sea_orm(string_value = "failed")]
    Failed,
    #[sea_orm(string_value = "archived")]
    Archived,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {
    #[sea_orm(
        belongs_to = "super::session::Entity",
        from = "Column::SessionId",
        to = "super::session::Column::Id"
    )]
    Session,
    #[sea_orm(has_many = "super::message::Entity")]
    Message,
}

impl Related<super::session::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::Session.def()
    }
}

impl Related<super::message::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::Message.def()
    }
}

impl ActiveModelBehavior for ActiveModel {}
