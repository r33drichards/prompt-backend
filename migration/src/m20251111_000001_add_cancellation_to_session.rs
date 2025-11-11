use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .alter_table(
                Table::alter()
                    .table(Session::Table)
                    .add_column(
                        ColumnDef::new(Session::CancellationStatus)
                            .string_len(50)
                            .null(),
                    )
                    .add_column(
                        ColumnDef::new(Session::CancelledAt)
                            .timestamp_with_time_zone()
                            .null(),
                    )
                    .add_column(
                        ColumnDef::new(Session::CancelledBy)
                            .string()
                            .null(),
                    )
                    .to_owned(),
            )
            .await
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .alter_table(
                Table::alter()
                    .table(Session::Table)
                    .drop_column(Session::CancellationStatus)
                    .drop_column(Session::CancelledAt)
                    .drop_column(Session::CancelledBy)
                    .to_owned(),
            )
            .await
    }
}

#[derive(DeriveIden)]
enum Session {
    Table,
    CancellationStatus,
    CancelledAt,
    CancelledBy,
}
