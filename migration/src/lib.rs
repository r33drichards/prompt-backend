pub use sea_orm_migration::prelude::*;

mod m20250102_000001_create_session_table;
mod m20250103_000001_add_title_to_session;
mod m20250104_000001_add_session_status_to_session;
mod m20250105_000001_add_timestamp_fields;
mod m20250106_000001_add_git_fields_to_session;
mod m20251103_000001_add_user_id_to_sessions;
mod m20251106_000001_create_prompt_table;
mod m20251106_000002_create_message_table;
mod m20251106_000003_drop_messages_from_session;
mod m20251106_000004_add_inbox_status_to_prompt;
mod m20251106_000005_drop_inbox_status_from_session;
mod m20251107_000001_add_status_message_to_session;
mod m20251107_000002_create_dead_letter_queue_table;
mod m20251107_000003_add_ip_return_retry_count_to_session;

pub struct Migrator;

#[async_trait::async_trait]
impl MigratorTrait for Migrator {
    fn migrations() -> Vec<Box<dyn MigrationTrait>> {
        vec![
            Box::new(m20250102_000001_create_session_table::Migration),
            Box::new(m20250103_000001_add_title_to_session::Migration),
            Box::new(m20250104_000001_add_session_status_to_session::Migration),
            Box::new(m20250105_000001_add_timestamp_fields::Migration),
            Box::new(m20250106_000001_add_git_fields_to_session::Migration),
            Box::new(m20251103_000001_add_user_id_to_sessions::Migration),
            Box::new(m20251106_000001_create_prompt_table::Migration),
            Box::new(m20251106_000002_create_message_table::Migration),
            Box::new(m20251106_000003_drop_messages_from_session::Migration),
            Box::new(m20251106_000004_add_inbox_status_to_prompt::Migration),
            Box::new(m20251106_000005_drop_inbox_status_from_session::Migration),
            Box::new(m20251107_000001_add_status_message_to_session::Migration),
            Box::new(m20251107_000002_create_dead_letter_queue_table::Migration),
            Box::new(m20251107_000003_add_ip_return_retry_count_to_session::Migration),
        ]
    }
}
