pub mod outbox_publisher;
pub mod session_handler;

use anyhow::Result;
use apalis::prelude::*;
use apalis_redis::RedisStorage;
use apalis_sql::postgres::{PgListen, PgPool, PostgresStorage};
use std::collections::HashSet;
use std::time::Duration;
use tracing::info;

/// Available background task names
pub const OUTBOX_PUBLISHER: &str = "outbox-publisher";
pub const SESSION_HANDLER: &str = "session-handler";

/// Get all available task names
pub fn all_tasks() -> Vec<&'static str> {
    vec![OUTBOX_PUBLISHER, SESSION_HANDLER]
}

/// Run background tasks based on the provided task names
pub async fn run_bg_tasks(
    task_names: Vec<String>,
    redis_url: String,
    database_url: String,
) -> Result<()> {
    info!("Starting background tasks: {:?}", task_names);

    let task_set: HashSet<String> = task_names.into_iter().collect();

    // Initialize Redis connection if needed
    let redis_conn = if task_set.contains(SESSION_HANDLER) {
        Some(apalis_redis::connect(redis_url).await?)
    } else {
        None
    };

    // Initialize PostgreSQL connection if needed
    let pg_pool = if task_set.contains(OUTBOX_PUBLISHER) {
        Some(PgPool::connect(&database_url).await?)
    } else {
        None
    };

    let mut monitor = Monitor::<TokioExecutor>::new();

    // Register outbox-publisher worker
    if task_set.contains(OUTBOX_PUBLISHER) {
        info!("Registering {} worker", OUTBOX_PUBLISHER);
        let pool = pg_pool.as_ref().unwrap().clone();

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

        monitor = monitor.register(worker);
    }

    // Register session-handler worker
    if task_set.contains(SESSION_HANDLER) {
        info!("Registering {} worker", SESSION_HANDLER);
        let conn = redis_conn.as_ref().unwrap().clone();
        let storage = RedisStorage::new(conn);

        let worker = WorkerBuilder::new(SESSION_HANDLER)
            .data(())
            .with_storage(storage)
            .build_fn(session_handler::process_session_job);

        monitor = monitor.register(worker);
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
