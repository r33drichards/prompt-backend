pub mod outbox_publisher;
pub mod session_handler;

use anyhow::Result;
use apalis::prelude::*;
use apalis_redis::RedisStorage;
use apalis_sql::postgres::{PgListen, PgPool, PostgresStorage};
use redis::aio::ConnectionManager;
use std::time::Duration;
use tracing::info;

/// Available background task names
pub const OUTBOX_PUBLISHER: &str = "outbox-publisher";
pub const SESSION_HANDLER: &str = "session-handler";

/// Get all available task names
pub fn all_tasks() -> Vec<&'static str> {
    vec![OUTBOX_PUBLISHER, SESSION_HANDLER]
}

/// Determine which backends are needed based on task names
fn needs_redis(task_names: &[String]) -> bool {
    task_names.iter().any(|t| t == SESSION_HANDLER)
}

fn needs_postgres(task_names: &[String]) -> bool {
    task_names.iter().any(|t| t == OUTBOX_PUBLISHER)
}

/// Register a specific task with the monitor
async fn register_task(
    task_name: &str,
    monitor: Monitor<TokioExecutor>,
    redis_conn: Option<&ConnectionManager>,
    pg_pool: Option<&PgPool>,
) -> Result<Monitor<TokioExecutor>> {
    info!("Registering {} worker", task_name);

    match task_name {
        OUTBOX_PUBLISHER => {
            let pool = pg_pool
                .ok_or_else(|| anyhow::anyhow!("PostgreSQL connection required for {}", task_name))?
                .clone();

            // Setup storage
            PostgresStorage::setup(&pool).await?;
            let storage = PostgresStorage::new(pool.clone());

            // Create listener for PostgreSQL notifications
            let mut listener = PgListen::new(pool).await?;
            listener.subscribe::<outbox_publisher::OutboxJob>();

            tokio::spawn(async move {
                listener.listen().await.unwrap();
            });

            let worker = WorkerBuilder::new(OUTBOX_PUBLISHER)
                .data(())
                .with_storage(storage)
                .build_fn(outbox_publisher::process_outbox_job);

            Ok(monitor.register(worker))
        }
        SESSION_HANDLER => {
            let conn = redis_conn
                .ok_or_else(|| anyhow::anyhow!("Redis connection required for {}", task_name))?
                .clone();

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

/// Run background tasks based on the provided task names
pub async fn run_bg_tasks(
    task_names: Vec<String>,
    redis_url: String,
    database_url: String,
) -> Result<()> {
    info!("Starting background tasks: {:?}", task_names);

    // Initialize connections based on which tasks need them
    let redis_conn = if needs_redis(&task_names) {
        Some(apalis_redis::connect(redis_url).await?)
    } else {
        None
    };

    let pg_pool = if needs_postgres(&task_names) {
        Some(PgPool::connect(&database_url).await?)
    } else {
        None
    };

    // Register all requested tasks
    let mut monitor = Monitor::<TokioExecutor>::new();

    for task_name in &task_names {
        monitor = register_task(
            task_name,
            monitor,
            redis_conn.as_ref(),
            pg_pool.as_ref(),
        )
        .await?;
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
