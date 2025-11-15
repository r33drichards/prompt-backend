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
                    .add_column(ColumnDef::new(Session::Branch).string().null())
                    .add_column(ColumnDef::new(Session::Repo).string().null())
                    .add_column(ColumnDef::new(Session::TargetBranch).string().null())
                    .to_owned(),
            )
            .await
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .alter_table(
                Table::alter()
                    .table(Session::Table)
                    .drop_column(Session::Branch)
                    .drop_column(Session::Repo)
                    .drop_column(Session::TargetBranch)
                    .to_owned(),
            )
            .await
    }
}

#[derive(DeriveIden)]
enum Session {
    Table,
    Branch,
    Repo,
    TargetBranch,
}
