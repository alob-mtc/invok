use axum::body::Body;
use axum::extract::{Multipart, Path, Query, State};
use axum::http::{HeaderMap, Request, StatusCode};
use axum::response::sse::{Event, KeepAlive, Sse};
use axum::response::IntoResponse;
use futures_util::stream::StreamExt;
use runtime::core::logs::LogMessage;

use crate::api_controller::middlewares::jwt::AuthenticatedUser;
use crate::api_controller::AppState;
use crate::db::function::FunctionDBRepo;
use crate::db::models::DeployableFunction;
use crate::lifecycle_manager::deploy::deploy_function;
use crate::lifecycle_manager::invoke::{check_function_status, start_function};
use crate::utils::utils::{generate_hash, make_request};
use std::collections::HashMap;
use std::convert::Infallible;
use tracing::{error, info, warn};
use uuid::Uuid;

/// Handles uploading a function as a ZIP file with authentication.
///
/// This endpoint expects a multipart request with one or more files and an Authorization header.
/// If a file with a name ending in ".zip" is found, it reads its content
/// and deploys the function for the authenticated user.
///
/// Returns an HTTP response indicating success or an appropriate error.
pub(crate) async fn upload_function(
    State(state): State<AppState>,
    AuthenticatedUser(user_uuid): AuthenticatedUser,
    mut multipart: Multipart,
) -> impl IntoResponse {
    // Get configuration from state
    let supported_archive_ext = ".zip"; // Currently we only support ZIP
    let max_size = state.config.function_config.max_function_size;

    // Iterate over the fields in the multipart request.
    while let Ok(Some(mut field)) = multipart.next_field().await {
        // Check if the field has a file name.
        if let Some(file_name) = field.file_name() {
            let file_name = file_name.to_owned();
            // Process only archive files.
            if file_name.ends_with(supported_archive_ext) {
                // Read file content in chunks.
                let buffer = match read_field_chunks(&mut field, max_size).await {
                    Ok(buffer) => buffer,
                    Err(e) => {
                        error!("Error reading file chunk: {}", e);
                        return (
                            StatusCode::INTERNAL_SERVER_ERROR,
                            format!("Error reading file: {}", e),
                        )
                            .into_response();
                    }
                };

                let function_name = file_name
                    .strip_suffix(supported_archive_ext)
                    .unwrap_or(&file_name);
                info!("Received service: {}", function_name);

                let function = DeployableFunction {
                    name: function_name.to_string(),
                    content: buffer,
                    user_uuid,
                };

                // Deploy the function
                return match deploy_function(&state.db_conn, function).await {
                    Ok(res) => (
                        StatusCode::OK,
                        format!(
                            "{}\nFunction: {}\nUser UUID: {}",
                            res, function_name, user_uuid
                        ),
                    )
                        .into_response(),
                    Err(e) => {
                        error!("Error deploying function {}: {}", function_name, e);
                        (
                            StatusCode::INTERNAL_SERVER_ERROR,
                            format!("Failed to deploy function: {}", e),
                        )
                            .into_response()
                    }
                };
            }
        } else {
            error!("Encountered a multipart field without a filename");
        }
    }
    (StatusCode::BAD_REQUEST, "Unexpected request").into_response()
}

/// List functions for an authenticated user
pub(crate) async fn list_functions(
    State(state): State<AppState>,
    AuthenticatedUser(user_uuid): AuthenticatedUser,
) -> impl IntoResponse {
    // Get functions for this user
    match FunctionDBRepo::find_functions_by_user_uuid(&state.db_conn, user_uuid).await {
        Ok(functions) => {
            // Convert to a simpler representation
            let function_list = functions
                .into_iter()
                .map(|f| {
                    serde_json::json!({
                        "uuid": f.uuid.to_string(),
                        "name": f.name,
                        "runtime": f.runtime
                    })
                })
                .collect::<Vec<_>>();

            (StatusCode::OK, axum::Json(function_list)).into_response()
        }
        Err(e) => {
            error!("Error listing functions: {}", e);
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("Error listing functions: {}", e),
            )
                .into_response()
        }
    }
}

/// Reads all chunks from a multipart field into a buffer.
async fn read_field_chunks(
    field: &mut axum::extract::multipart::Field<'_>,
    max_size: usize,
) -> Result<Vec<u8>, String> {
    let mut buffer = Vec::new();
    let mut total_size = 0;

    while let Some(chunk_result) = field.next().await {
        match chunk_result {
            Ok(chunk) => {
                total_size += chunk.len();
                if total_size > max_size {
                    return Err(format!(
                        "File too large, maximum size is {} bytes",
                        max_size
                    ));
                }
                buffer.extend_from_slice(&chunk);
            }
            Err(e) => return Err(e.to_string()),
        }
    }
    Ok(buffer)
}

/// Handles calling a function service based on a provided key.
///
/// This endpoint:
/// - Validates the namespace (user UUID) format and function name
/// - Checks if the function exists in the user's namespace
/// - Determines the appropriate runtime version (v1 or v2)
/// - Starts the function if needed using the appropriate runtime
/// - Forwards the incoming request to the service with proper error handling
///
/// # Parameters
///
/// * `namespace` - The user's UUID serving as a namespace for their functions
/// * `function_name` - The name of the function to invoke
/// * `query` - Query parameters to forward to the function
/// * `headers` - HTTP headers to forward to the function
/// * `request` - The complete HTTP request to forward
///
/// # Returns
///
/// The service's response or an appropriate error response
pub(crate) async fn call_function(
    mut state: State<AppState>,
    Path((namespace, function_name)): Path<(String, String)>,
    Query(query): Query<HashMap<String, String>>,
    headers: HeaderMap,
    request: Request<Body>,
) -> impl IntoResponse {
    // Validate input parameters
    if let Err(response) = validate_function_call_inputs(&namespace, &function_name) {
        return response;
    }

    // Parse and validate namespace UUID early
    let user_uuid = match namespace.parse() {
        Ok(uuid) => uuid,
        Err(e) => {
            error!(
                namespace = %namespace,
                function = %function_name,
                error = %e,
                "Invalid function namespace format"
            );
            return (
                StatusCode::BAD_REQUEST,
                format!("Invalid function namespace format: {}", e),
            )
                .into_response();
        }
    };

    // Check function existence and authorization
    if let Err(e) = check_function_status(&mut state, &function_name, user_uuid).await {
        error!(
            namespace = %namespace,
            function = %function_name,
            user_uuid = %user_uuid,
            error = %e,
            "Function status check failed"
        );
        return e.into_response();
    }

    info!(
        namespace = %namespace,
        function = %function_name,
        user_uuid = %user_uuid,
        "Starting function invocation"
    );

    let start_time = std::time::Instant::now();
    let function_address =
        start_function(state.autoscaler.clone(), &function_name, user_uuid).await;

    let addr = match function_address {
        Ok(addr) => {
            let duration = start_time.elapsed();
            info!(
                namespace = %namespace,
                function = %function_name,
                user_uuid = %user_uuid,
                address = %addr,
                startup_duration_ms = duration.as_millis(),
                "Function started successfully"
            );
            addr
        }
        Err(e) => {
            let duration = start_time.elapsed();
            error!(
                namespace = %namespace,
                function = %function_name,
                user_uuid = %user_uuid,
                error = ?e,
                startup_duration_ms = duration.as_millis(),
                "Failed to start function"
            );

            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("Failed to start function: {}", e),
            )
                .into_response();
        }
    };

    info!(
        namespace = %namespace,
        function = %function_name,
        user_uuid = %user_uuid,
        address = %addr,
        "Function started successfully, forwarding request"
    );

    // Forward the request to the service
    make_request(&addr, &function_name, query, headers, request)
        .await
        .into_response()
}

/// Validates the input parameters for function calls
fn validate_function_call_inputs(
    namespace: &str,
    function_name: &str,
) -> Result<(), axum::response::Response> {
    // Validate namespace format (should be a valid UUID string)
    if namespace.is_empty() {
        warn!("Empty namespace provided");
        return Err((
            StatusCode::BAD_REQUEST,
            "Namespace cannot be empty".to_string(),
        )
            .into_response());
    }

    // Validate function name
    if function_name.is_empty() {
        warn!(namespace = %namespace, "Empty function name provided");
        return Err((
            StatusCode::BAD_REQUEST,
            "Function name cannot be empty".to_string(),
        )
            .into_response());
    }

    // Check for potentially dangerous characters in function name
    if function_name.contains("..") || function_name.contains('/') || function_name.contains('\\') {
        warn!(
            namespace = %namespace,
            function = %function_name,
            "Function name contains invalid characters"
        );
        return Err((
            StatusCode::BAD_REQUEST,
            "Function name contains invalid characters".to_string(),
        )
            .into_response());
    }

    // Check function name length (reasonable limits)
    if function_name.len() > 25 {
        warn!(
            namespace = %namespace,
            function = %function_name,
            function_name_length = function_name.len(),
            "Function name too long"
        );
        return Err((
            StatusCode::BAD_REQUEST,
            "Function name is too long (max 25 characters)".to_string(),
        )
            .into_response());
    }

    Ok(())
}

/// Stream logs from a deployed function in real-time
///
/// This endpoint:
/// - Validates the namespace (user UUID) format and function name  
/// - Checks if the function exists in the user's namespace
/// - Uses the runtime module to stream container logs
/// - Returns logs via Server-Sent Events
///
/// # Parameters
///
/// * `namespace` - The user's UUID serving as a namespace for their functions
/// * `function_name` - The name of the function to get logs from
///
/// # Returns
///
/// A Server-Sent Events stream of container logs
pub(crate) async fn stream_function_logs(
    mut state: State<AppState>,
    Path((namespace, function_name)): Path<(String, String)>,
    AuthenticatedUser(user_uuid): AuthenticatedUser,
) -> impl IntoResponse {
    // Validate input parameters
    if let Err(response) = validate_function_call_inputs(&namespace, &function_name) {
        return response;
    }

    // Validate namespace matches authenticated user
    let namespace_uuid: Uuid = match namespace.parse() {
        Ok(uuid) => uuid,
        Err(e) => {
            error!(
                namespace = %namespace,
                function = %function_name,
                error = %e,
                "Invalid function namespace format"
            );
            return (
                StatusCode::BAD_REQUEST,
                format!("Invalid function namespace format: {}", e),
            )
                .into_response();
        }
    };

    if namespace_uuid != user_uuid {
        error!(
            namespace = %namespace,
            function = %function_name,
            user_uuid = %user_uuid,
            "Namespace doesn't match authenticated user"
        );
        return (
            StatusCode::FORBIDDEN,
            "You can only access logs for your own functions".to_string(),
        )
            .into_response();
    }

    // Check function existence
    if let Err(e) = check_function_status(&mut state, &function_name, user_uuid).await {
        error!(
            namespace = %namespace,
            function = %function_name,
            user_uuid = %user_uuid,
            error = %e,
            "Function status check failed"
        );
        return e.into_response();
    }

    info!(
        namespace = %namespace,
        function = %function_name,
        user_uuid = %user_uuid,
        "Starting log stream for function"
    );

    // Generate function key and get log stream from runtime
    let uuid_short = generate_hash(user_uuid);
    let function_key = format!("{function_name}-{uuid_short}");

    let log_stream = match state.autoscaler.get_function_logs(&function_key).await {
        Some(stream) => stream,
        None => {
            warn!(
                namespace = %namespace,
                function = %function_name,
                user_uuid = %user_uuid,
                function_key = %function_key,
                "No running container found for function"
            );
            return (
                StatusCode::NOT_FOUND,
                "No running container found for this function. Try invoking the function first."
                    .to_string(),
            )
                .into_response();
        }
    };

    info!(
        namespace = %namespace,
        function = %function_name,
        user_uuid = %user_uuid,
        "Log stream established successfully"
    );

    // Convert LogMessage stream to Server-Sent Events
    let sse_stream = log_stream.map(|log_msg| {
        let event_data = match log_msg {
            LogMessage::Content(content) => content,
            LogMessage::Error(error) => format!("ERROR: {}", error),
            LogMessage::End => "Log stream ended".to_string(),
        };

        Ok::<Event, Infallible>(Event::default().data(event_data))
    });

    Sse::new(sse_stream)
        .keep_alive(KeepAlive::default())
        .into_response()
}