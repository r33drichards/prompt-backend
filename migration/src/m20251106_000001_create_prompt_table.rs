use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .create_table(
                Table::create()
                    .table(Prompt::Table)
                    .if_not_exists()
                    .col(ColumnDef::new(Prompt::Id).uuid().not_null().primary_key())
                    .col(ColumnDef::new(Prompt::SessionId).uuid().not_null())
                    .col(ColumnDef::new(Prompt::Data).json_binary().not_null())
                    .col(
                        ColumnDef::new(Prompt::CreatedAt)
                            .timestamp_with_time_zone()
                            .not_null()
                            .default(Expr::current_timestamp()),
                    )
                    .col(
                        ColumnDef::new(Prompt::UpdatedAt)
                            .timestamp_with_time_zone()
                            .not_null()
                            .default(Expr::current_timestamp()),
                    )
                    .foreign_key(
                        ForeignKey::create()
                            .name("fk_prompt_session_id")
                            .from(Prompt::Table, Prompt::SessionId)
                            .to(Session::Table, Session::Id)
                            .on_delete(ForeignKeyAction::Cascade)
                            .on_update(ForeignKeyAction::Cascade),
                    )
                    .to_owned(),
            )
            .await?;

        // Create index on session_id for faster lookups
        manager
            .create_index(
                Index::create()
                    .name("idx_prompt_session_id")
                    .table(Prompt::Table)
                    .col(Prompt::SessionId)
                    .to_owned(),
            )
            .await
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .drop_table(Table::drop().table(Prompt::Table).to_owned())
            .await
    }
}

#[derive(DeriveIden)]
enum Prompt {
    Table,
    Id,
    SessionId,
    Data,
    CreatedAt,
    UpdatedAt,
}

#[derive(DeriveIden)]
enum Session {
    Table,
    Id,
}
