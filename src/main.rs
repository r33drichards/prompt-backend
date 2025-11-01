#[macro_use]
extern crate rocket;

use dotenv::dotenv;

use crate::store::Store;

use rocket_okapi::settings::UrlObject;
use rocket_okapi::swagger_ui::make_swagger_ui;
use rocket_okapi::{openapi_get_routes, rapidoc::*, swagger_ui::*};

use std::env;
use tokio::sync::Mutex;

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
        ](&settings);
        println!("{}", serde_json::to_string_pretty(&spec).unwrap());
        return;
    }

    let redis_url = env::var("REDIS_URL").unwrap_or_else(|_| "redis://127.0.0.1/".to_string());

    let store = Store::new(redis_url.clone());

    let _ = rocket::build()
        .configure(rocket::Config {
            address: "0.0.0.0".parse().expect("valid IP address"),
            port: 8000,
            ..rocket::Config::default()
        })
        .manage(Mutex::new(store))
        .mount(
            "/",
            openapi_get_routes![
                handlers::items::create,
                handlers::items::read,
                handlers::items::list,
                handlers::items::update,
                handlers::items::delete,
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
