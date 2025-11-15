use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .create_table(
                Table::create()
                    .table(Session::Table)
                    .if_not_exists()
                    .col(ColumnDef::new(Session::Id).uuid().not_null().primary_key())
                    .col(ColumnDef::new(Session::Messages).json_binary().null())
                    .col(
                        ColumnDef::new(Session::InboxStatus)
                            .string_len(50)
                            .not_null(),
                    )
                    .col(ColumnDef::new(Session::SbxConfig).json_binary().null())
                    .col(ColumnDef::new(Session::Parent).uuid().null())
                    .to_owned(),
            )
            .await
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .drop_table(Table::drop().table(Session::Table).to_owned())
            .await
    }
}

#[derive(DeriveIden)]
enum Session {
    Table,
    Id,
    Messages,
    InboxStatus,
    SbxConfig,
    Parent,
}
