#[macro_use]
extern crate rocket;

use dotenv::dotenv;

use crate::store::Store;
use crate::db::establish_connection;

use rocket_okapi::settings::UrlObject;
use rocket_okapi::swagger_ui::make_swagger_ui;
use rocket_okapi::{openapi_get_routes, rapidoc::*, swagger_ui::*};

use std::env;
use tokio::sync::Mutex;

mod db;
mod entities;
mod error;
mod handlers;
mod store;

#[rocket::main]
async fn main() {
    dotenv().ok();
    let args: Vec<String> = env::args().collect();

    // Support --print-openapi flag for generating OpenAPI spec
    if args.contains(&"--print-openapi".to_string()) {
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
        println!("{}", serde_json::to_string_pretty(&spec).unwrap());
        return;
    }

    let redis_url = env::var("REDIS_URL").unwrap_or_else(|_| "redis://127.0.0.1/".to_string());
    let database_url = env::var("DATABASE_URL").expect("DATABASE_URL must be set");

    let store = Store::new(redis_url.clone());
    let db = establish_connection(&database_url)
        .await
        .expect("Failed to connect to database");

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
