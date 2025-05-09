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
    functions::{call_function, list_functions, upload_function},
};
use redis::aio::MultiplexedConnection;
use sea_orm::{Database, DatabaseConnection};
use std::net::SocketAddr;
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
}

/// Custom error type for server initialization.
#[derive(Debug, Error)]
pub enum InvokAppError {
    #[error("Configuration error: {0}")]
    ConfigError(#[from] InvokConfigError),

    #[error("Redis connection error: {0}")]
    RedisError(#[from] redis::RedisError),

    #[error("Database connection error: {0}")]
    DatabaseError(#[from] sea_orm::DbErr),

    #[error("Server error: {0}")]
    ServerError(#[from] std::io::Error),

    #[error("HTTP server error: {0}")]
    HttpError(#[from] hyper::Error),
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

    let app_state = AppState {
        db_conn,
        cache_conn,
        config: config.clone(),
    };

    // Create a router with all our routes
    let app = Router::new()
        // Auth routes
        .route("/auth/register", post(register))
        .route("/auth/login", post(login))
        // Function management routes
        .route("/invok/list", get(list_functions))
        .route("/invok/deploy", post(upload_function))
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
