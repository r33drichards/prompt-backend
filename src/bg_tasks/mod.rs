pub mod outbox_publisher;
pub mod session_handler;

use anyhow::Result;
use apalis::prelude::*;
use apalis_redis::RedisStorage;
use apalis_sql::postgres::{PgListen, PgPool, PostgresStorage};
use sea_orm::DatabaseConnection;
use std::time::Duration;
use tracing::info;

/// Available background task names
pub const OUTBOX_PUBLISHER: &str = "outbox-publisher";
pub const SESSION_HANDLER: &str = "session-handler";

/// Get all available task names
pub fn all_tasks() -> Vec<&'static str> {
    vec![OUTBOX_PUBLISHER, SESSION_HANDLER]
}

/// Context for running background tasks, holds optional connections to backends
pub struct TaskContext {
    pub redis_url: Option<String>,
    pub db: Option<PgPool>,
    pub sea_orm_db: Option<DatabaseConnection>,
}

impl TaskContext {
    /// Create a new TaskContext with optional Redis and PostgreSQL connections
    pub async fn new(
        redis_url: Option<String>,
        database_url: Option<String>,
    ) -> Result<Self> {
        let db = if let Some(url) = &database_url {
            Some(
                PgPool::connect(url)
                    .await
                    .map_err(|e| anyhow::anyhow!("Failed to connect to PostgreSQL: {}", e))?,
            )
        } else {
            None
        };

        let sea_orm_db = if let Some(url) = database_url {
            Some(
                crate::db::establish_connection(&url)
                    .await
                    .map_err(|e| anyhow::anyhow!("Failed to connect to PostgreSQL with SeaORM: {}", e))?,
            )
        } else {
            None
        };

        Ok(Self { redis_url, db, sea_orm_db })
    }

    /// Run background tasks based on the provided task names
    pub async fn run_bg_tasks(self, task_names: Vec<String>) -> Result<()> {
        info!("Starting background tasks: {:?}", task_names);

        // Register all requested tasks
        let mut monitor = Monitor::<TokioExecutor>::new();

        for task_name in &task_names {
            monitor = self.register_task(task_name, monitor).await?;
        }

        // Run monitor with graceful shutdown
        monitor
            .on_event(|e| {
                let worker_id = e.id();
                match e.inner() {
                    Event::Start => {
                        info!("Worker [{worker_id}] started");
                    }
                    Event::Error(e) => {
                        tracing::error!("Worker [{worker_id}] encountered an error: {e}");
                    }
                    Event::Exit => {
                        info!("Worker [{worker_id}] exited");
                    }
                    _ => {}
                }
            })
            .shutdown_timeout(Duration::from_millis(5000))
            .run_with_signal(async {
                info!("Background tasks monitor started");
                tokio::signal::ctrl_c().await?;
                info!("Background tasks monitor starting shutdown");
                Ok(())
            })
            .await?;

        info!("Background tasks monitor shutdown complete");
        Ok(())
    }

    /// Register a specific task with the monitor
    async fn register_task(
        &self,
        task_name: &str,
        monitor: Monitor<TokioExecutor>,
    ) -> Result<Monitor<TokioExecutor>> {
        info!("Registering {} worker", task_name);

        match task_name {
            OUTBOX_PUBLISHER => {
                let pool = self
                    .db
                    .as_ref()
                    .ok_or_else(|| {
                        anyhow::anyhow!("PostgreSQL connection required for {}", task_name)
                    })?
                    .clone();

                let sea_orm_db = self
                    .sea_orm_db
                    .as_ref()
                    .ok_or_else(|| {
                        anyhow::anyhow!("SeaORM database connection required for {}", task_name)
                    })?
                    .clone();

                let redis_url = self
                    .redis_url
                    .as_ref()
                    .ok_or_else(|| anyhow::anyhow!("Redis connection required for {}", task_name))?;

                // Setup PostgreSQL storage for outbox jobs
                PostgresStorage::setup(&pool).await?;
                let storage = PostgresStorage::new(pool.clone());

                // Create listener for PostgreSQL notifications
                let mut listener = PgListen::new(pool).await?;
                listener.subscribe::<outbox_publisher::OutboxJob>();

                tokio::spawn(async move {
                    listener.listen().await.unwrap();
                });

                // Create context with database connection
                let ctx = outbox_publisher::OutboxContext {
                    db: sea_orm_db,
                };

                let worker = WorkerBuilder::new(OUTBOX_PUBLISHER)
                    .data(ctx)
                    .with_storage(storage)
                    .build_fn(outbox_publisher::process_outbox_job);

                Ok(monitor.register(worker))
            }
            SESSION_HANDLER => {
                let redis_url = self
                    .redis_url
                    .as_ref()
                    .ok_or_else(|| anyhow::anyhow!("Redis connection required for {}", task_name))?;

                let conn = apalis_redis::connect(redis_url.clone()).await?;
                let storage = RedisStorage::new(conn);

                let worker = WorkerBuilder::new(SESSION_HANDLER)
                    .data(())
                    .with_storage(storage)
                    .build_fn(session_handler::process_session_job);

                Ok(monitor.register(worker))
            }
            _ => Err(anyhow::anyhow!("Unknown task: {}", task_name)),
        }
    }
}
