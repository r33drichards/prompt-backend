pub mod outbox_publisher;
pub mod session_poller;

use anyhow::Result;
use apalis::layers::prometheus::PrometheusLayer;
use apalis::prelude::*;
use apalis_sql::postgres::{PgListen, PgPool, PostgresStorage};
use std::time::Duration;
use tracing::info;

/// Available background task names
pub const OUTBOX_PUBLISHER: &str = "outbox-publisher";

/// Get all available task names
pub fn all_tasks() -> Vec<&'static str> {
    vec![OUTBOX_PUBLISHER]
}

/// Context for running background tasks, holds optional connections to backends
pub struct TaskContext {
    pub db: Option<PgPool>,
}

impl TaskContext {
    /// Create a new TaskContext with optional Redis and PostgreSQL connections
    pub async fn new(database_url: Option<String>) -> Result<Self> {
        let db = if let Some(url) = database_url {
            Some(
                PgPool::connect(&url)
                    .await
                    .map_err(|e| anyhow::anyhow!("Failed to connect to PostgreSQL: {}", e))?,
            )
        } else {
            None
        };

        Ok(Self { db })
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

                // Setup storage
                PostgresStorage::setup(&pool).await?;
                let storage = PostgresStorage::new(pool.clone());

                // Create listener for PostgreSQL notifications
                let mut listener = PgListen::new(pool.clone()).await?;
                listener.subscribe::<outbox_publisher::OutboxJob>();

                tokio::spawn(async move {
                    listener.listen().await.unwrap();
                });

                // Create OutboxContext with database connection
                let database_url = std::env::var("DATABASE_URL")
                    .map_err(|_| anyhow::anyhow!("DATABASE_URL must be set"))?;
                let db = crate::db::establish_connection(&database_url).await?;
                let ctx = outbox_publisher::OutboxContext { db };

                let worker = WorkerBuilder::new(OUTBOX_PUBLISHER)
                    .layer(PrometheusLayer)
                    .data(ctx)
                    .with_storage(storage)
                    .build_fn(outbox_publisher::process_outbox_job);

                Ok(monitor.register(worker))
            }
            _ => Err(anyhow::anyhow!("Unknown task: {}", task_name)),
        }
    }
}
