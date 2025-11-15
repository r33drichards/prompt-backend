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
                    .add_column(ColumnDef::new(Session::ProcessPid).integer().null())
                    .to_owned(),
            )
            .await
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .alter_table(
                Table::alter()
                    .table(Session::Table)
                    .drop_column(Session::ProcessPid)
                    .to_owned(),
            )
            .await
    }
}

#[derive(DeriveIden)]
enum Session {
    Table,
    ProcessPid,
}
