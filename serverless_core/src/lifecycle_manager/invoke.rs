use crate::api_controller::AppState;
use crate::db::cache::FunctionCacheRepo;
use crate::db::function::FunctionDBRepo;
use crate::lifecycle_manager::error::ServelessCoreError::FunctionFailedToStart;
use crate::lifecycle_manager::error::{ServelessCoreError, ServelessCoreResult};
use crate::utils::utils::generate_hash;
use axum::extract::State;
use runtime::core::autoscaler::Autoscaler;
use std::sync::Arc;
use tracing::{error, info};
use uuid::Uuid;

const TIMEOUT_DEFAULT_IN_SECONDS: u64 = 1 * 60 * 60; // 1 hour timeout for function cache

/// Checks if a function is registered in the database.
///
/// Returns `Ok(())` if the function exists; otherwise, returns an error
/// indicating that the function is not registered.
///
/// # Arguments
///
/// * `conn` - A reference to the database connection.
/// * `name` - The name of the function to check.
/// * `user_uuid` - The UUID of the user (namespace) to verify function ownership.
pub async fn check_function_status(
    state: &mut State<AppState>,
    name: &str,
    user_uuid: Uuid,
) -> ServelessCoreResult<()> {
    if FunctionCacheRepo::get_function(&mut state.cache_conn, name)
        .await
        .is_some()
    {
        return Ok(());
    }

    let function = FunctionDBRepo::find_function_by_name(&state.db_conn, name, user_uuid).await;
    if function.is_none() {
        error!("Function '{}' not found in namespace '{}'", name, user_uuid);
        return Err(ServelessCoreError::FunctionNotRegistered(format!(
            "Function '{}' not found in namespace '{}'",
            name, user_uuid
        )));
    }

    // If the function exists in the database, add it to the cache with a TTL.
    if let Err(e) =
        FunctionCacheRepo::add_function(&mut state.cache_conn, name, TIMEOUT_DEFAULT_IN_SECONDS)
            .await
    {
        error!("Failed to cache function '{}': {}", name, e);
        return Err(ServelessCoreError::SystemError(format!(
            "Failed to cache function '{}': {}",
            name, e
        )));
    }

    Ok(())
}

/// Starts a function service if it's not already running.
///
///
/// # Arguments
///
/// * `runtime` - An `Arc` reference to the `Autoscaler` runtime, which manages function execution.
/// * `name` - The name of the function to start.
/// * `user_uuid` - The UUID of the user (namespace) who owns this function.
///
/// # Returns
///
/// A `Result` containing the function's address (e.g., "localhost:PORT") on success,
/// or an error if the function fails to start.
pub async fn start_function(
    runtime: Arc<Autoscaler>,
    name: &str,
    user_uuid: Uuid,
) -> ServelessCoreResult<String> {
    // Generate a shorter hash of the UUID for better container names
    let uuid_short = generate_hash(user_uuid);

    // Create a unique function name based on function name and user's UUID hash
    let function_key = format!("{name}-{uuid_short}");

    if let Some(container_details) = runtime.get_container_for_invocation(&function_key).await {
        // Register the function in the cache.
        let function_address = format!(
            "{}:{}",
            &container_details.container_name, &container_details.container_port
        );

        info!(
            "Function '{}' for user '{}' started at: {}",
            name, user_uuid, function_address
        );

        return Ok(function_address);
    }

    Err(FunctionFailedToStart("Function did not start".to_string()))
}
