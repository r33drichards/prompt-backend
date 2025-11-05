use prometheus::{Encoder, TextEncoder};
use rocket::http::Status;
use rocket::response::content::RawText;
use rocket::State;

/// Shared Prometheus registry
pub type PrometheusRegistry = prometheus::Registry;

/// Metrics endpoint for Prometheus scraping
#[get("/metrics")]
pub fn metrics(registry: &State<PrometheusRegistry>) -> Result<RawText<String>, Status> {
    let encoder = TextEncoder::new();
    let metric_families = registry.gather();

    let mut buffer = vec![];
    encoder
        .encode(&metric_families, &mut buffer)
        .map_err(|_| Status::InternalServerError)?;

    String::from_utf8(buffer)
        .map(RawText)
        .map_err(|_| Status::InternalServerError)
}
