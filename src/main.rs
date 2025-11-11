#[macro_use]
extern crate rocket;

use clap::{Parser, Subcommand};
use dotenv::dotenv;
use tracing::info;

use crate::auth::JwksCache;
use crate::db::establish_connection;

use rocket_cors::{AllowedOrigins, CorsOptions};
use rocket_okapi::settings::UrlObject;
use rocket_okapi::swagger_ui::make_swagger_ui;
use rocket_okapi::{openapi_get_routes, rapidoc::*, swagger_ui::*};

use sea_orm_migration::prelude::*;

mod auth;
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

    /// Run the web server and background tasks
    #[arg(long)]
    server: bool,
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
        handlers::health::health,
        handlers::sessions::create,
        handlers::sessions::create_with_prompt,
        handlers::sessions::read,
        handlers::sessions::list,
        handlers::sessions::update,
        handlers::sessions::delete,
        handlers::sessions::cancel,
        handlers::prompts::create,
        handlers::prompts::read,
        handlers::prompts::list,
        handlers::prompts::update,
        handlers::prompts::delete,
        handlers::messages::create,
        handlers::messages::read,
        handlers::messages::list,
        handlers::messages::update,
        handlers::messages::delete,
        handlers::webhooks::return_item,
        handlers::dead_letter_queue::list_dlq_entries,
        handlers::dead_letter_queue::get_dlq_entry,
        handlers::dead_letter_queue::resolve_dlq,
        handlers::dead_letter_queue::abandon_dlq,
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
    if let Some(Commands::PrintOpenapi) = cli.command {
        println!("{}", generate_openapi_spec());
        return Ok(());
    }

    let redis_url = std::env::var("REDIS_URL").unwrap_or_else(|_| "redis://127.0.0.1/".to_string());
    let database_url = std::env::var("DATABASE_URL").expect("DATABASE_URL must be set");

    let mut handles = vec![];

    // Spawn server and background tasks if --server flag is present
    if cli.server {
        let server_redis_url = redis_url.clone();
        let server_database_url = database_url.clone();

        let server_handle = tokio::spawn(async move {
            info!("Starting web server");
            run_server(server_redis_url, server_database_url).await
        });

        handles.push(server_handle);

        // Spawn all background tasks
        let bg_task_names: Vec<String> = bg_tasks::all_tasks()
            .into_iter()
            .map(|s| s.to_string())
            .collect();

        let task_database_url = Some(database_url.clone());
        let bg_tasks_handle = tokio::spawn(async move {
            info!("Starting background tasks");
            let task_context = bg_tasks::TaskContext::new(task_database_url)
                .await
                .expect("Failed to create task context");
            task_context.run_bg_tasks(bg_task_names).await
        });

        handles.push(bg_tasks_handle);

        // Spawn prompt poller
        let poller_database_url = database_url.clone();
        let poller_handle = tokio::spawn(async move {
            info!("Starting prompt poller");

            // Create SeaORM database connection for the poller
            let db = establish_connection(&poller_database_url).await?;

            // Create PostgreSQL pool for apalis storage
            let pool = apalis_sql::postgres::PgPool::connect(&poller_database_url).await?;

            bg_tasks::prompt_poller::run_prompt_poller(db, pool).await
        });

        handles.push(poller_handle);

        // Spawn IP return poller
        let ip_return_database_url = database_url.clone();
        let ip_return_handle = tokio::spawn(async move {
            info!("Starting IP return poller");

            // Create SeaORM database connection for the poller
            let db = establish_connection(&ip_return_database_url).await?;

            bg_tasks::ip_return_poller::run_ip_return_poller(db).await
        });

        handles.push(ip_return_handle);
    }

    // If no services specified, error out
    if handles.is_empty() {
        eprintln!("No services specified. Use --server to start the web server and background tasks, or --help for usage.");
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

    // Initialize JWKS cache
    let keycloak_issuer = std::env::var("KEYCLOAK_ISSUER").expect("KEYCLOAK_ISSUER must be set");
    let keycloak_jwks_uri =
        std::env::var("KEYCLOAK_JWKS_URI").expect("KEYCLOAK_JWKS_URI must be set");

    let jwks_cache = JwksCache::new(keycloak_jwks_uri, keycloak_issuer);

    // Pre-fetch JWKS on startup
    println!("Fetching JWKS from Keycloak...");
    jwks_cache.fetch_jwks().await.expect("Failed to fetch JWKS");
    println!("JWKS fetched successfully");

    // Configure CORS to allow all origins, methods, and headers
    let cors = CorsOptions::default()
        .allowed_origins(AllowedOrigins::all())
        .allowed_methods(
            vec![
                rocket::http::Method::Get,
                rocket::http::Method::Post,
                rocket::http::Method::Put,
                rocket::http::Method::Delete,
                rocket::http::Method::Options,
            ]
            .into_iter()
            .map(From::from)
            .collect(),
        )
        .allowed_headers(rocket_cors::AllowedHeaders::all())
        .allow_credentials(true)
        .to_cors()
        .expect("Failed to create CORS fairing");

    // Create Prometheus registry
    let prometheus_registry = prometheus::Registry::new();

    let _ = rocket::build()
        .configure(rocket::Config {
            address: "0.0.0.0".parse().expect("valid IP address"),
            port: 8000,
            ..rocket::Config::default()
        })
        .attach(cors)
        .manage(db)
        .manage(jwks_cache)
        .manage(prometheus_registry)
        .mount(
            "/",
            openapi_get_routes![
                handlers::health::health,
                handlers::sessions::create,
                handlers::sessions::create_with_prompt,
                handlers::sessions::read,
                handlers::sessions::list,
                handlers::sessions::update,
                handlers::sessions::delete,
                handlers::sessions::cancel,
                handlers::prompts::create,
                handlers::prompts::read,
                handlers::prompts::list,
                handlers::prompts::update,
                handlers::prompts::delete,
                handlers::messages::create,
                handlers::messages::read,
                handlers::messages::list,
                handlers::messages::update,
                handlers::messages::delete,
                handlers::webhooks::return_item,
                handlers::dead_letter_queue::list_dlq_entries,
                handlers::dead_letter_queue::get_dlq_entry,
                handlers::dead_letter_queue::resolve_dlq,
                handlers::dead_letter_queue::abandon_dlq,
            ],
        )
        .mount("/", routes![handlers::metrics::metrics])
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
