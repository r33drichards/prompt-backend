#[macro_use]
extern crate rocket;

use clap::{Parser, Subcommand};
use dotenv::dotenv;
use tracing::info;

use crate::db::establish_connection;

use rocket_okapi::settings::UrlObject;
use rocket_okapi::swagger_ui::make_swagger_ui;
use rocket_okapi::{openapi_get_routes, rapidoc::*, swagger_ui::*};
use rocket_cors::{AllowedOrigins, CorsOptions};

use sea_orm_migration::prelude::*;

mod bg_tasks;
mod db;
mod entities;
mod error;
mod handlers;
mod services;

/// CLI application for the prompt backend server
#[derive(Parser)]
#[command(name = "prompt-backend")]
#[command(about = "A Rocket web server with Redis and PostgreSQL support", long_about = None)]
struct Cli {
    #[command(subcommand)]
    command: Option<Commands>,

    /// Run the web server
    #[arg(long)]
    server: bool,

    /// Enable background tasks. Use -A/--all to run all tasks, or specify task names
    #[arg(long = "bg-tasks", value_name = "TASKS")]
    bg_tasks: Vec<String>,
}

#[derive(Subcommand, PartialEq)]
enum Commands {
    /// Print the OpenAPI specification in JSON format
    PrintOpenapi,
}

/// Generate OpenAPI specification
fn generate_openapi_spec() -> String {
    let settings = rocket_okapi::settings::OpenApiSettings::new();
    let spec = rocket_okapi::openapi_spec![
        handlers::sessions::create,
        handlers::sessions::read,
        handlers::sessions::list,
        handlers::sessions::update,
        handlers::sessions::delete,
    ](&settings);
    serde_json::to_string_pretty(&spec).unwrap()
}

#[rocket::main]
async fn main() -> anyhow::Result<()> {
    dotenv().ok();

    // Initialize tracing
    tracing_subscriber::fmt::init();

    let cli = Cli::parse();

    // Handle print-openapi command
    match cli.command {
        Some(Commands::PrintOpenapi) => {
            println!("{}", generate_openapi_spec());
            return Ok(());
        }
        _ => {}
    }

    let redis_url = std::env::var("REDIS_URL").unwrap_or_else(|_| "redis://127.0.0.1/".to_string());
    let database_url = std::env::var("DATABASE_URL").expect("DATABASE_URL must be set");

    // Determine which background tasks to run (empty vec if none specified)
    let bg_task_names: Vec<String> = if cli.bg_tasks.contains(&"-A".to_string())
        || cli.bg_tasks.contains(&"--all".to_string())
    {
        bg_tasks::all_tasks()
            .into_iter()
            .map(|s| s.to_string())
            .collect()
    } else {
        cli.bg_tasks
    };

    let mut handles = vec![];

    // Spawn server if --server flag is present
    if cli.server {
        let server_redis_url = redis_url.clone();
        let server_database_url = database_url.clone();

        let server_handle = tokio::spawn(async move {
            info!("Starting web server");
            run_server(server_redis_url, server_database_url).await
        });

        handles.push(server_handle);
    }

    // Spawn background tasks if --bg-tasks flag is present
    if !bg_task_names.is_empty() {
        // Determine which connections are needed based on task names
        let needs_redis = bg_task_names.iter().any(|t| t == bg_tasks::SESSION_HANDLER);
        let needs_postgres = bg_task_names.iter().any(|t| t == bg_tasks::OUTBOX_PUBLISHER);

        let task_redis_url = if needs_redis { Some(redis_url) } else { None };
        let task_database_url = if needs_postgres { Some(database_url) } else { None };

        let bg_tasks_handle = tokio::spawn(async move {
            info!("Starting background tasks");
            let task_context = bg_tasks::TaskContext::new(task_redis_url, task_database_url)
                .await
                .expect("Failed to create task context");
            task_context.run_bg_tasks(bg_task_names).await
        });

        handles.push(bg_tasks_handle);
    }

    // If no services specified, error out
    if handles.is_empty() {
        eprintln!("No services specified. Use --server and/or --bg-tasks, or --help for usage.");
        return Err(anyhow::anyhow!("No services specified"));
    }

    // Wait for all services to complete
    for handle in handles {
        handle.await??;
    }

    Ok(())
}

/// Run the Rocket web server
async fn run_server(_redis_url: String, database_url: String) -> anyhow::Result<()> {
    let db = establish_connection(&database_url)
        .await
        .expect("Failed to connect to database");

    // Run database migrations
    println!("Running database migrations...");
    migration::Migrator::up(&db, None)
        .await
        .expect("Failed to run migrations");
    println!("Migrations completed successfully");

    // Configure CORS to allow all origins
    let cors = CorsOptions::default()
        .allowed_origins(AllowedOrigins::all())
        .to_cors()
        .expect("Failed to create CORS fairing");

    let _ = rocket::build()
        .configure(rocket::Config {
            address: "0.0.0.0".parse().expect("valid IP address"),
            port: 8000,
            ..rocket::Config::default()
        })
        .attach(cors)
        .manage(db)
        .mount(
            "/",
            openapi_get_routes![
                handlers::sessions::create,
                handlers::sessions::read,
                handlers::sessions::list,
                handlers::sessions::update,
                handlers::sessions::delete,
            ],
        )
        .mount(
            "/swagger-ui/",
            make_swagger_ui(&SwaggerUIConfig {
                url: "../openapi.json".to_owned(),
                ..Default::default()
            }),
        )
        .mount(
            "/rapidoc/",
            make_rapidoc(&RapiDocConfig {
                general: GeneralConfig {
                    spec_urls: vec![UrlObject::new("General", "../openapi.json")],
                    ..Default::default()
                },
                hide_show: HideShowConfig {
                    allow_spec_url_load: false,
                    allow_spec_file_load: false,
                    ..Default::default()
                },
                ..Default::default()
            }),
        )
        .launch()
        .await?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_openapi_spec_snapshot() {
        let spec = generate_openapi_spec();
        insta::assert_snapshot!(spec);
    }
}
