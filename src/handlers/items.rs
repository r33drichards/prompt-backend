use rocket::serde::json::Json;
use rocket::State;
use rocket_okapi::openapi;
use rocket_okapi::okapi::schemars::JsonSchema;
use rocket::serde::{Deserialize, Serialize};
use tokio::sync::Mutex;
use serde_json::Value;

use crate::error::{Error, OResult};
use crate::store::Store;

#[derive(Serialize, Deserialize, JsonSchema, Clone)]
pub struct CreateInput {
    pub item: Value,
}

#[derive(Serialize, Deserialize, JsonSchema, Clone)]
pub struct CreateOutput {
    pub success: bool,
    pub message: String,
}

#[derive(Serialize, Deserialize, JsonSchema, Clone)]
pub struct ReadOutput {
    pub item: Value,
}

#[derive(Serialize, Deserialize, JsonSchema, Clone)]
pub struct ListOutput {
    pub items: Vec<Value>,
}

#[derive(Serialize, Deserialize, JsonSchema, Clone)]
pub struct UpdateInput {
    pub old_item: Value,
    pub new_item: Value,
}

#[derive(Serialize, Deserialize, JsonSchema, Clone)]
pub struct UpdateOutput {
    pub success: bool,
    pub message: String,
}

#[derive(Serialize, Deserialize, JsonSchema, Clone)]
pub struct DeleteInput {
    pub item: Value,
}

#[derive(Serialize, Deserialize, JsonSchema, Clone)]
pub struct DeleteOutput {
    pub success: bool,
    pub message: String,
}

/// Create a new item in the store
#[openapi]
#[post("/items", data = "<input>")]
pub async fn create(
    store: &State<Mutex<Store>>,
    input: Json<CreateInput>,
) -> OResult<CreateOutput> {
    let store = store.lock().await;
    match store.create(&input.item) {
        Ok(_) => Ok(Json(CreateOutput {
            success: true,
            message: "Item created successfully".to_string(),
        })),
        Err(e) => Err(Error::from(e)),
    }
}

/// Read (retrieve and remove) an item from the store
#[openapi]
#[get("/items/read")]
pub async fn read(store: &State<Mutex<Store>>) -> OResult<ReadOutput> {
    let store = store.lock().await;
    match store.read() {
        Ok(item) => Ok(Json(ReadOutput { item })),
        Err(e) => Err(Error::from(e)),
    }
}

/// List all items in the store
#[openapi]
#[get("/items")]
pub async fn list(store: &State<Mutex<Store>>) -> OResult<ListOutput> {
    let store = store.lock().await;
    match store.list() {
        Ok(items) => Ok(Json(ListOutput { items })),
        Err(e) => Err(Error::from(e)),
    }
}

/// Update an existing item in the store
#[openapi]
#[put("/items", data = "<input>")]
pub async fn update(
    store: &State<Mutex<Store>>,
    input: Json<UpdateInput>,
) -> OResult<UpdateOutput> {
    let store = store.lock().await;
    match store.update(&input.old_item, &input.new_item) {
        Ok(_) => Ok(Json(UpdateOutput {
            success: true,
            message: "Item updated successfully".to_string(),
        })),
        Err(e) => Err(Error::from(e)),
    }
}

/// Delete an item from the store
#[openapi]
#[delete("/items", data = "<input>")]
pub async fn delete(
    store: &State<Mutex<Store>>,
    input: Json<DeleteInput>,
) -> OResult<DeleteOutput> {
    let store = store.lock().await;
    match store.delete(&input.item) {
        Ok(_) => Ok(Json(DeleteOutput {
            success: true,
            message: "Item deleted successfully".to_string(),
        })),
        Err(e) => Err(Error::from(e)),
    }
}
