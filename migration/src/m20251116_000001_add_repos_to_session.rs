use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        // Step 1: Add the new repos column
        manager
            .alter_table(
                Table::alter()
                    .table(Session::Table)
                    .add_column(ColumnDef::new(Session::Repos).json_binary().null())
                    .to_owned(),
            )
            .await?;

        // Step 2: Migrate data from repo and target_branch to repos
        // This SQL will create a JSONB object with the repos array containing the old repo and branch
        let sql = r#"
            UPDATE session
            SET repos = jsonb_build_object(
                'repos', jsonb_build_array(
                    jsonb_build_object(
                        'url', repo,
                        'branch', COALESCE(target_branch, 'main')
                    )
                )
            )
            WHERE repo IS NOT NULL
        "#;

        manager.get_connection().execute_unprepared(sql).await?;

        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .alter_table(
                Table::alter()
                    .table(Session::Table)
                    .drop_column(Session::Repos)
                    .to_owned(),
            )
            .await
    }
}

#[derive(DeriveIden)]
enum Session {
    Table,
    Repos,
    Repo,
    TargetBranch,
}
