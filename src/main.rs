#[macro_use]
extern crate rocket;

use clap::{Parser, Subcommand};
use dotenv::dotenv;

use crate::store::Store;
use crate::db::establish_connection;

use rocket_okapi::settings::UrlObject;
use rocket_okapi::swagger_ui::make_swagger_ui;
use rocket_okapi::{openapi_get_routes, rapidoc::*, swagger_ui::*};

use sea_orm_migration::prelude::*;

use tokio::sync::Mutex;

mod bg_tasks;
mod db;
mod entities;
mod error;
mod handlers;
mod store;

/// CLI application for the prompt backend server
#[derive(Parser)]
#[command(name = "prompt-backend")]
#[command(about = "A Rocket web server with Redis and PostgreSQL support", long_about = None)]
struct Cli {
    #[command(subcommand)]
    command: Option<Commands>,

    /// Enable background tasks. Use -A/--all to run all tasks, or specify task names
    #[arg(long = "bg-tasks", value_name = "TASKS")]
    bg_tasks: Vec<String>,
}

#[derive(Subcommand)]
enum Commands {
    /// Print the OpenAPI specification in JSON format
    PrintOpenapi,
    /// Run the server (default)
    Serve,
}

/// Generate OpenAPI specification
fn generate_openapi_spec() -> String {
    let settings = rocket_okapi::settings::OpenApiSettings::new();
    let spec = rocket_okapi::openapi_spec![
        handlers::items::create,
        handlers::items::read,
        handlers::items::list,
        handlers::items::update,
        handlers::items::delete,
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

    // Determine which background tasks to run
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

    let should_run_server = match cli.command {
        Some(Commands::Serve) => true,
        None => bg_task_names.is_empty(), // Default: run server only if no bg tasks specified
        _ => false,
    };

    let should_run_bg_tasks = !bg_task_names.is_empty();

    // Run both server and background tasks if needed
    if should_run_server && should_run_bg_tasks {
        // Run both concurrently
        let server_redis_url = redis_url.clone();
        let server_database_url = database_url.clone();

        let server_handle = tokio::spawn(async move {
            run_server(server_redis_url, server_database_url).await
        });

        let bg_tasks_handle = tokio::spawn(async move {
            bg_tasks::run_bg_tasks(bg_task_names, redis_url, database_url)
                .await
                .expect("Background tasks failed");
        });

        // Wait for both to complete (or shutdown signal)
        tokio::select! {
            _ = server_handle => {
                println!("Server stopped");
            }
            _ = bg_tasks_handle => {
                println!("Background tasks stopped");
            }
        }
    } else if should_run_server {
        // Run only server
        run_server(redis_url, database_url).await?;
    } else if should_run_bg_tasks {
        // Run only background tasks
        bg_tasks::run_bg_tasks(bg_task_names, redis_url, database_url).await?;
    } else {
        eprintln!("No operation specified. Use --help for usage information.");
        return Err(anyhow::anyhow!("No operation specified"));
    }

    Ok(())
}

/// Run the Rocket web server
async fn run_server(redis_url: String, database_url: String) -> anyhow::Result<()> {
    let store = Store::new(redis_url.clone());
    let db = establish_connection(&database_url)
        .await
        .expect("Failed to connect to database");

    // Run database migrations
    println!("Running database migrations...");
    migration::Migrator::up(&db, None)
        .await
        .expect("Failed to run migrations");
    println!("Migrations completed successfully");

    let _ = rocket::build()
        .configure(rocket::Config {
            address: "0.0.0.0".parse().expect("valid IP address"),
            port: 8000,
            ..rocket::Config::default()
        })
        .manage(Mutex::new(store))
        .manage(db)
        .mount(
            "/",
            openapi_get_routes![
                handlers::items::create,
                handlers::items::read,
                handlers::items::list,
                handlers::items::update,
                handlers::items::delete,
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
