use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .create_table(
                Table::create()
                    .table(SessionRepository::Table)
                    .if_not_exists()
                    .col(
                        ColumnDef::new(SessionRepository::Id)
                            .uuid()
                            .not_null()
                            .primary_key(),
                    )
                    .col(
                        ColumnDef::new(SessionRepository::SessionId)
                            .uuid()
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(SessionRepository::Repo)
                            .string()
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(SessionRepository::TargetBranch)
                            .string()
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(SessionRepository::CreatedAt)
                            .timestamp_with_time_zone()
                            .not_null()
                            .default(Expr::current_timestamp()),
                    )
                    .col(
                        ColumnDef::new(SessionRepository::UpdatedAt)
                            .timestamp_with_time_zone()
                            .not_null()
                            .default(Expr::current_timestamp()),
                    )
                    .foreign_key(
                        ForeignKey::create()
                            .name("fk_session_repositories_session_id")
                            .from(SessionRepository::Table, SessionRepository::SessionId)
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
                    .name("idx_session_repositories_session_id")
                    .table(SessionRepository::Table)
                    .col(SessionRepository::SessionId)
                    .to_owned(),
            )
            .await?;

        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .drop_table(Table::drop().table(SessionRepository::Table).to_owned())
            .await
    }
}

#[derive(DeriveIden)]
enum SessionRepository {
    Table,
    Id,
    SessionId,
    Repo,
    TargetBranch,
    CreatedAt,
    UpdatedAt,
}

#[derive(DeriveIden)]
enum Session {
    Table,
    Id,
}
