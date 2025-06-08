mod api;
mod auth;
mod function_runner;
mod metrics;

use crate::auth::JwtKeys;
use actix_web::{web, App, HttpServer};
use dotenvy::dotenv;
use runtime::core::builder::AutoscalingRuntimeBuilder;
use std::env;
use std::sync::Arc;
use tracing::info;

#[actix_web::main]
async fn main() -> std::io::Result<()> {
    dotenv().ok();
    tracing_subscriber::fmt::init();

    info!("Starting serverless runtime...");

    // Build the autoscaling runtime with persistence enabled
    let runtime = AutoscalingRuntimeBuilder::new()
        .docker_compose_network_host("host.docker.internal".to_string())
        .port(8080)
        .metrics_port(9090)
        .min_containers_per_function(1)
        .max_containers_per_function(5)
        .persistence_enabled(true)
        .redis_url("redis://localhost:6379".to_string())
        .persistence_key_prefix("autoscaler".to_string())
        .persistence_batch_size(50)  // Load 50 pools at a time during recovery
        .build()
        .await
        .expect("Failed to build autoscaling runtime");

    // Start the autoscaler
    runtime.start().await.expect("Failed to start autoscaler");

    let autoscaler = runtime.autoscaler().clone();
    let jwt_keys = Arc::new(JwtKeys::new());

    info!("Starting HTTP server on port 8080...");

    HttpServer::new(move || {
        App::new()
            .app_data(web::Data::new(autoscaler.clone()))
            .app_data(web::Data::new(jwt_keys.clone()))
            .service(
                web::scope("/api")
                    .service(api::health)
                    .service(api::invoke_function)
                    .service(api::autoscaler_status),
            )
    })
    .bind("0.0.0.0:8080")?
    .run()
    .await
} 