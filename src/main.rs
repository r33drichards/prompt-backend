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
async fn main() {
    dotenv().ok();

    let cli = Cli::parse();

    match cli.command {
        Some(Commands::PrintOpenapi) => {
            println!("{}", generate_openapi_spec());
            return;
        }
        Some(Commands::Serve) | None => {
            // Run the server (default behavior)
        }
    }

    let redis_url = std::env::var("REDIS_URL").unwrap_or_else(|_| "redis://127.0.0.1/".to_string());
    let database_url = std::env::var("DATABASE_URL").expect("DATABASE_URL must be set");

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
        .await;
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
