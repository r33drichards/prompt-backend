use rocket::serde::json::Json;
use rocket::serde::{Deserialize, Serialize};
use rocket_okapi::okapi::schemars::JsonSchema;
use rocket_okapi::openapi;

use crate::error::{Error, OResult};

#[derive(Serialize, Deserialize, JsonSchema, Clone, Debug)]
pub struct ReturnItemInput {
    pub item: serde_json::Value,
}

#[derive(Serialize, Deserialize, JsonSchema, Clone)]
pub struct ReturnItemOutput {
    pub success: bool,
    pub message: String,
}

#[derive(Serialize, Deserialize, Debug)]
struct RailwayGraphQLRequest {
    query: String,
    variables: RailwayVariables,
    #[serde(rename = "operationName")]
    operation_name: String,
}

#[derive(Serialize, Deserialize, Debug)]
struct RailwayVariables {
    id: String,
}

#[derive(Serialize, Deserialize, Debug)]
struct RailwayGraphQLResponse {
    data: Option<serde_json::Value>,
    errors: Option<Vec<serde_json::Value>>,
}

/// Webhook endpoint for IP allocator to trigger Railway redeployment
/// This endpoint receives item return notifications from the IP allocator
/// and triggers a Railway deployment redeploy to refresh the deployment state
#[openapi]
#[post("/webhook/return", data = "<input>")]
pub async fn return_item(input: Json<ReturnItemInput>) -> OResult<ReturnItemOutput> {
    tracing::info!("Received return item webhook: {:?}", input.item);

    // Get Railway API configuration from environment
    let railway_api_key = std::env::var("RAILWAY_API_KEY")
        .map_err(|_| Error::internal_server_error("RAILWAY_API_KEY not configured".to_string()))?;

    let deployment_id = std::env::var("RAILWAY_DEPLOYMENT_ID").map_err(|_| {
        Error::internal_server_error("RAILWAY_DEPLOYMENT_ID not configured".to_string())
    })?;

    // Prepare the GraphQL mutation
    let graphql_request = RailwayGraphQLRequest {
        query: "mutation deploymentRedeploy($id: String!) {\n  deploymentRedeploy(id: $id) {\n    id\n  }\n}".to_string(),
        variables: RailwayVariables {
            id: deployment_id.clone(),
        },
        operation_name: "deploymentRedeploy".to_string(),
    };

    tracing::info!(
        "Triggering Railway redeployment for deployment: {}",
        deployment_id
    );

    // Make blocking HTTP request to Railway GraphQL API
    let client = reqwest::Client::new();
    let response = client
        .post("https://backboard.railway.app/graphql/v2")
        .header("Authorization", format!("Bearer {}", railway_api_key))
        .header("Content-Type", "application/json")
        .json(&graphql_request)
        .send()
        .await
        .map_err(|e| {
            Error::internal_server_error(format!("Failed to send Railway request: {}", e))
        })?;

    let status = response.status();
    let response_text = response.text().await.map_err(|e| {
        Error::internal_server_error(format!("Failed to read Railway response: {}", e))
    })?;

    tracing::info!(
        "Railway API response status: {}, body: {}",
        status,
        response_text
    );

    if !status.is_success() {
        return Err(Error::internal_server_error(format!(
            "Railway API request failed with status {}: {}",
            status, response_text
        )));
    }

    // Parse the response to check for GraphQL errors
    let graphql_response: RailwayGraphQLResponse =
        serde_json::from_str(&response_text).map_err(|e| {
            Error::internal_server_error(format!("Failed to parse Railway response: {}", e))
        })?;

    if let Some(errors) = graphql_response.errors {
        let error_messages: Vec<String> = errors.iter().map(|e| e.to_string()).collect();
        return Err(Error::internal_server_error(format!(
            "Railway GraphQL errors: {}",
            error_messages.join(", ")
        )));
    }

    tracing::info!("Railway redeployment triggered successfully");

    Ok(Json(ReturnItemOutput {
        success: true,
        message: "Railway redeployment triggered successfully".to_string(),
    }))
}
