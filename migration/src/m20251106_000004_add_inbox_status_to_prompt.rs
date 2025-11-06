use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .alter_table(
                Table::alter()
                    .table(Prompt::Table)
                    .add_column(
                        ColumnDef::new(Prompt::InboxStatus)
                            .string_len(50)
                            .not_null()
                            .default("pending"),
                    )
                    .to_owned(),
            )
            .await
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .alter_table(
                Table::alter()
                    .table(Prompt::Table)
                    .drop_column(Prompt::InboxStatus)
                    .to_owned(),
            )
            .await
    }
}

#[derive(DeriveIden)]
enum Prompt {
    Table,
    InboxStatus,
}
