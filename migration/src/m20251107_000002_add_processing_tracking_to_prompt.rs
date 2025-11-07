use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        // Add processing_attempts field
        manager
            .alter_table(
                Table::alter()
                    .table(Prompt::Table)
                    .add_column(
                        ColumnDef::new(Prompt::ProcessingAttempts)
                            .integer()
                            .not_null()
                            .default(0),
                    )
                    .to_owned(),
            )
            .await?;

        // Add last_error field
        manager
            .alter_table(
                Table::alter()
                    .table(Prompt::Table)
                    .add_column(ColumnDef::new(Prompt::LastError).text().null())
                    .to_owned(),
            )
            .await?;

        // Add last_attempt_at field
        manager
            .alter_table(
                Table::alter()
                    .table(Prompt::Table)
                    .add_column(
                        ColumnDef::new(Prompt::LastAttemptAt)
                            .timestamp_with_time_zone()
                            .null(),
                    )
                    .to_owned(),
            )
            .await?;

        // Add completed_at field
        manager
            .alter_table(
                Table::alter()
                    .table(Prompt::Table)
                    .add_column(
                        ColumnDef::new(Prompt::CompletedAt)
                            .timestamp_with_time_zone()
                            .null(),
                    )
                    .to_owned(),
            )
            .await?;

        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .alter_table(
                Table::alter()
                    .table(Prompt::Table)
                    .drop_column(Prompt::ProcessingAttempts)
                    .to_owned(),
            )
            .await?;

        manager
            .alter_table(
                Table::alter()
                    .table(Prompt::Table)
                    .drop_column(Prompt::LastError)
                    .to_owned(),
            )
            .await?;

        manager
            .alter_table(
                Table::alter()
                    .table(Prompt::Table)
                    .drop_column(Prompt::LastAttemptAt)
                    .to_owned(),
            )
            .await?;

        manager
            .alter_table(
                Table::alter()
                    .table(Prompt::Table)
                    .drop_column(Prompt::CompletedAt)
                    .to_owned(),
            )
            .await?;

        Ok(())
    }
}

#[derive(DeriveIden)]
enum Prompt {
    Table,
    ProcessingAttempts,
    LastError,
    LastAttemptAt,
    CompletedAt,
}
