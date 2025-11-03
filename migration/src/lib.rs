pub use sea_orm_migration::prelude::*;

mod m20250102_000001_create_session_table;
mod m20250103_000001_add_title_to_session;
mod m20250104_000001_add_session_status_to_session;

pub struct Migrator;

#[async_trait::async_trait]
impl MigratorTrait for Migrator {
    fn migrations() -> Vec<Box<dyn MigrationTrait>> {
        vec![
            Box::new(m20250102_000001_create_session_table::Migration),
            Box::new(m20250103_000001_add_title_to_session::Migration),
            Box::new(m20250104_000001_add_session_status_to_session::Migration),
        ]
    }
}
