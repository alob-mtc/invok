mod config;
mod handlers;
mod middlewares;

use axum::{
    extract::FromRef,
    routing::{any, get, post},
    Router,
};
use config::{InvokConfig, InvokConfigError};
use db_migrations::{Migrator, MigratorTrait};
use handlers::{
    auth::{login, register},
    functions::{call_function, list_functions, stream_function_logs, upload_function},
};
use redis::aio::MultiplexedConnection;
use runtime::core::autoscaler::Autoscaler;
use runtime::core::builder::AutoscalingRuntimeBuilder;
use sea_orm::{Database, DatabaseConnection};
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;
use thiserror::Error;
use tracing::{error, info};

/// Application state shared across handlers.
#[derive(Clone, FromRef)]
pub struct AppState {
    /// Database connection for persisting data.
    pub db_conn: DatabaseConnection,
    /// Redis connection for caching.
    pub cache_conn: MultiplexedConnection,
    /// Application configuration
    pub config: InvokConfig,
    // TODO: added autoscaler runtime
    pub autoscaler: Arc<Autoscaler>,
}

/// Custom error type for server initialization.
#[derive(Debug, Error)]
pub enum InvokAppError {
    #[error("Configuration error: {0}")]
    Config(#[from] InvokConfigError),

    #[error("Redis connection error: {0}")]
    Redis(#[from] redis::RedisError),

    #[error("Database connection error: {0}")]
    Database(#[from] sea_orm::DbErr),

    #[error("Server error: {0}")]
    Server(#[from] std::io::Error),

    #[error("HTTP server error: {0}")]
    Http(#[from] hyper::Error),
}

/// Starts the server and sets up the necessary connections and routes.
///
/// This function performs the following:
/// - Initializes structured logging.
/// - Loads application configuration
/// - Connects to Redis and the database.
/// - Runs database migrations.
/// - Sets up the Axum router with defined routes.
/// - Binds the server to a socket address and starts serving requests.
pub async fn start_server() -> Result<(), InvokAppError> {
    tracing_subscriber::fmt::init();

    // Load application configuration
    let config = InvokConfig::load()?;

    // Connect to Redis.
    let client = redis::Client::open(config.server_config.redis_url.clone())?;
    let cache_conn = client.get_multiplexed_async_connection().await?;

    // Connect to the database.
    let db_conn = Database::connect(config.server_config.database_url.clone()).await?;

    // Run database migrations.
    Migrator::up(&db_conn, None).await?;

    // Configure autoscaling runtime
    let runtime = AutoscalingRuntimeBuilder::new()
        .cpu_overload_threshold(config.function_config.autoscaling.cpu_overload_threshold)
        .memory_overload_threshold(config.function_config.autoscaling.memory_overload_threshold)
        .docker_compose_network_host(config.server_config.docker_compose_network_host.to_string())
        .min_containers_per_function(
            config
                .function_config
                .autoscaling
                .min_containers_per_function,
        )
        .max_containers_per_function(
            config
                .function_config
                .autoscaling
                .max_containers_per_function,
        )
        .cooldown_duration(Duration::from_secs(
            config.function_config.autoscaling.cooldown_duration_secs,
        ))
        .cooldown_cpu_threshold(config.function_config.autoscaling.cooldown_cpu_threshold)
        .scale_check_interval(Duration::from_secs(
            config.function_config.autoscaling.poll_interval_secs,
        ))
        .persistence_enabled(config.function_config.autoscaling.persistence_enabled)
        .redis_url(config.server_config.redis_url.clone())
        .persistence_batch_size(20) // Load 20 pools at a time during recovery
        .build()
        .await
        .map_err(|e| {
            error!("Failed to build autoscaling runtime: {}", e);
            InvokAppError::Config(InvokConfigError::InvalidValue(format!(
                "Runtime build error: {}",
                e
            )))
        })?;

    // Start runtime
    runtime.start().await.map_err(|e| {
        error!("Failed to start autoscaling runtime: {}", e);
        InvokAppError::Config(InvokConfigError::InvalidValue(format!(
            "Runtime start error: {}",
            e
        )))
    })?;

    let app_state = AppState {
        db_conn,
        cache_conn,
        config: config.clone(),
        autoscaler: runtime.autoscaler().clone(),
    };

    // Create a router with all our routes
    let app = Router::new()
        // Auth routes
        .route("/auth/register", post(register))
        .route("/auth/login", post(login))
        // Function management routes
        .route("/invok/list", get(list_functions))
        .route("/invok/deploy", post(upload_function))
        // Function logs route
        .route(
            "/invok/logs/:namespace/:function_name",
            get(stream_function_logs),
        )
        // Function invocation routes
        .route("/invok/:namespace/:function_name", any(call_function))
        .with_state(app_state);

    // Build socket address from configuration
    let addr = SocketAddr::new(
        config
            .server_config
            .host
            .parse()
            .unwrap_or_else(|_| "0.0.0.0".parse().unwrap()),
        config.server_config.port,
    );

    info!("Server listening on {}", addr);

    axum::Server::bind(&addr)
        .serve(app.into_make_service())
        .await?;

    Ok(())
}
