use crate::db::cache::FunctionCacheRepo;
use crate::db::function::FunctionDBRepo;
use crate::lifecycle_manager::lib::error::{ServelessCoreError, ServelessCoreResult};
use crate::utils::utils::random_port;
use redis::aio::MultiplexedConnection;
use runtime::core::runner::runner;
use sea_orm::DatabaseConnection;
use std::time::Duration;
use tracing::{error, info};
use uuid::Uuid;

/// Checks if a function is registered in the database.
///
/// Returns `Ok(())` if the function exists; otherwise, returns an error
/// indicating that the function is not registered.
///
/// # Arguments
///
/// * `conn` - A reference to the database connection.
/// * `name` - The name of the function to check.
pub async fn check_function_status(
    conn: &DatabaseConnection,
    name: &str,
    user_uuid: Uuid,
) -> ServelessCoreResult<()> {
    let function = FunctionDBRepo::find_function_by_name(conn, name, user_uuid).await;
    if function.is_none() {
        error!("Function '{}' not registered", name);
        return Err(ServelessCoreError::FunctionNotRegistered(name.to_string()));
    }
    Ok(())
}

/// Starts a function service if it's not already running.
///
/// This function first checks if the function is already running by querying the
/// cache repository. If a running instance is found, it returns the cached address.
/// Otherwise, it generates a random port, starts the function container using the
/// Docker runner, caches the new function's address, and returns it.
///
/// # Arguments
///
/// * `cache_conn` - A mutable reference to the Redis multiplexed connection.
/// * `name` - The name of the function to start.
///
/// # Returns
///
/// A `Result` containing the function's address (e.g., "localhost:PORT") on success,
/// or an error if the function fails to start.
pub async fn start_function(
    cache_conn: &mut MultiplexedConnection,
    name: &str,
    user_uuid: Uuid,
) -> ServelessCoreResult<String> {
    // Check if the function is already running.
    let function_name = format!("{name}-{user_uuid}");
    if let Some(addr) = FunctionCacheRepo::get_function(cache_conn, &function_name).await {
        info!("Function '{}' already running at: {}", name, addr);
        return Ok(addr);
    }

    // Generate a random port and prepare the service address.
    let port = random_port();
    let addr = format!("localhost:{}", port);
    let timeout = 10;

    // Attempt to run the function container with a timeout slightly longer than the cache TTL.
    match runner(
        &function_name,
        &format!("{port}:8080"),
        Some(Duration::from_secs(timeout + 2)),
    )
    .await
    {
        Err(e) => {
            error!("Error starting function '{}': {:?}", function_name, e);
            Err(ServelessCoreError::FunctionFailedToStart(name.to_string()))
        }
        Ok(_) => {
            // Register the function in the cache.
            let _ =
                FunctionCacheRepo::add_function(cache_conn, &function_name, &addr, timeout).await;
            info!("Function '{}' started at: {}", name, addr);
            Ok(addr)
        }
    }
}
