use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .create_table(
                Table::create()
                    .table(DeadLetterQueue::Table)
                    .if_not_exists()
                    .col(
                        ColumnDef::new(DeadLetterQueue::Id)
                            .uuid()
                            .not_null()
                            .primary_key(),
                    )
                    .col(
                        ColumnDef::new(DeadLetterQueue::TaskType)
                            .string()
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(DeadLetterQueue::EntityId)
                            .uuid()
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(DeadLetterQueue::EntityData)
                            .json_binary()
                            .null(),
                    )
                    .col(
                        ColumnDef::new(DeadLetterQueue::RetryCount)
                            .integer()
                            .not_null()
                            .default(0),
                    )
                    .col(
                        ColumnDef::new(DeadLetterQueue::LastError)
                            .text()
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(DeadLetterQueue::LastErrorAt)
                            .timestamp_with_time_zone()
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(DeadLetterQueue::FirstFailedAt)
                            .timestamp_with_time_zone()
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(DeadLetterQueue::Status)
                            .string()
                            .not_null()
                            .default("pending"),
                    )
                    .col(
                        ColumnDef::new(DeadLetterQueue::ResolutionNotes)
                            .text()
                            .null(),
                    )
                    .col(
                        ColumnDef::new(DeadLetterQueue::CreatedAt)
                            .timestamp_with_time_zone()
                            .not_null()
                            .default(Expr::current_timestamp()),
                    )
                    .col(
                        ColumnDef::new(DeadLetterQueue::UpdatedAt)
                            .timestamp_with_time_zone()
                            .not_null()
                            .default(Expr::current_timestamp()),
                    )
                    .to_owned(),
            )
            .await?;

        // Create index on entity_id for faster lookups
        manager
            .create_index(
                Index::create()
                    .name("idx_dlq_entity_id")
                    .table(DeadLetterQueue::Table)
                    .col(DeadLetterQueue::EntityId)
                    .to_owned(),
            )
            .await?;

        // Create index on task_type for filtering by task
        manager
            .create_index(
                Index::create()
                    .name("idx_dlq_task_type")
                    .table(DeadLetterQueue::Table)
                    .col(DeadLetterQueue::TaskType)
                    .to_owned(),
            )
            .await?;

        // Create index on status for filtering by status
        manager
            .create_index(
                Index::create()
                    .name("idx_dlq_status")
                    .table(DeadLetterQueue::Table)
                    .col(DeadLetterQueue::Status)
                    .to_owned(),
            )
            .await
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .drop_table(Table::drop().table(DeadLetterQueue::Table).to_owned())
            .await
    }
}

#[derive(DeriveIden)]
enum DeadLetterQueue {
    Table,
    Id,
    TaskType,
    EntityId,
    EntityData,
    RetryCount,
    LastError,
    LastErrorAt,
    FirstFailedAt,
    Status,
    ResolutionNotes,
    CreatedAt,
    UpdatedAt,
}
