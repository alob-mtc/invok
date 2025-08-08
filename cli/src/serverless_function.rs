use crate::auth::{load_session, AuthError};
use crate::host_manager;
use crate::utils::{create_fn_project_file, init_function_module, FuncConfig};
use futures_util::stream::TryStreamExt;
use reqwest::blocking::{multipart, Client};
use reqwest::header::{self, HeaderMap, HeaderValue};
use serde_json::Value;
use shared_utils::{compress_dir_with_excludes, to_camel_case_handler};
use std::fs::File;
use std::io::{self, Cursor, Read, Write};
use std::path::Path;
use std::time::Duration;
use templates::{go_template, nodejs_template};
use thiserror::Error;

// Constants
const REQUEST_TIMEOUT_SECS: u64 = 120;
const CONFIG_FILE_PATH: &str = "config.json";

/// Errors that can occur during serverless function operations
#[derive(Debug, Error)]
pub enum FunctionError {
    #[error("IO error: {0}")]
    IoError(#[from] io::Error),

    #[error("JSON parsing error: {0}")]
    JsonError(#[from] serde_json::Error),

    #[error("Network request error: {0}")]
    RequestError(#[from] reqwest::Error),

    #[error("Function not found: {0}")]
    FunctionNotFound(String),

    #[error("Compression error: {0}")]
    CompressionError(String),

    #[error("Authentication error: {0}")]
    AuthError(#[from] AuthError),
}

/// Creates a new serverless function project with the specified name and runtime.
///
/// # Arguments
///
/// * `name` - The name of the function to create
/// * `runtime` - The runtime to use (e.g., "go")
///
/// # Returns
///
/// A Result indicating success or containing an error
pub fn create_new_project(name: &str, runtime: &str) -> Result<(), FunctionError> {
    // Validate runtime
    let normalized_runtime = match runtime.to_lowercase().as_str() {
        "go" => "go",
        "nodejs" | "node" | "typescript" | "ts" => "nodejs",
        _ => {
            return Err(FunctionError::CompressionError(format!(
                "Unsupported runtime: '{}'. Supported runtimes: go, nodejs",
                runtime
            )))
        }
    };

    println!("Creating service... '{name}' [RUNTIME:'{normalized_runtime}']");
    // Create project file
    let file = create_fn_project_file(name, normalized_runtime)?;
    let mut file = io::BufWriter::new(&file);

    match normalized_runtime {
        "go" => {
            let handler_name = to_camel_case_handler(name);
            // Write template with replacements
            file.write_all(
                go_template::ROUTES_TEMPLATE
                    .replace("{{ROUTE}}", name)
                    .replace("{{HANDLER}}", &handler_name)
                    .as_bytes(),
            )?;
        }
        "nodejs" => {
            // Write template with replacements
            file.write_all(
                nodejs_template::ROUTE_TEMPLATE
                    .replace("{{ROUTE}}", name)
                    .as_bytes(),
            )?;
        }
        _ => {}
    }

    // Initialize function module
    init_function_module(name, normalized_runtime)?;
    println!("Function created");

    Ok(())
}

/// List all functions
pub fn list_functions() -> Result<(), FunctionError> {
    // Load authentication session
    let session = load_session()?;

    // Set up authorization headers
    let mut headers = HeaderMap::new();
    headers.insert(
        header::AUTHORIZATION,
        HeaderValue::from_str(&format!("Bearer {}", session.token))
            .map_err(|_| FunctionError::CompressionError("Invalid token format".to_string()))?,
    );

    // Build client with timeout
    let client = Client::builder()
        .timeout(Duration::from_secs(REQUEST_TIMEOUT_SECS))
        .default_headers(headers)
        .build()?;

    // Send request to API
    let response = client.get(host_manager::function_list_url()).send()?;

    // Check the response
    if response.status().is_success() {
        let response_text = response.text()?;
        let functions: Vec<Value> = serde_json::from_str(&response_text)?;

        if functions.is_empty() {
            println!("No functions found.");
            return Ok(());
        }

        // Print table header
        println!("+--------------------------------------+----------------------+---------+");
        println!("| UUID                                 | Name                 | Runtime |");
        println!("+--------------------------------------+----------------------+---------+");

        // Print each function as a table row
        for function in functions {
            let uuid = function["uuid"].as_str().unwrap_or("N/A");
            let name = function["name"].as_str().unwrap_or("N/A");
            let runtime = function["runtime"].as_str().unwrap_or("N/A");

            // Format the row with proper alignment
            println!("| {:<36} | {:<20} | {:<7} |", uuid, name, runtime);
        }

        // Print table footer
        println!("+--------------------------------------+----------------------+---------+");

        Ok(())
    } else {
        let status = response.status();
        let error_text = response
            .text()
            .unwrap_or_else(|_| "Unknown error".to_string());

        Err(FunctionError::CompressionError(format!(
            "API error: Status code {}. {}",
            status, error_text
        )))
    }
}

/// Deploys an existing function to the serverless platform using authentication.
///
/// # Arguments
///
/// * `name` - The name of the function to deploy
///
/// # Returns
///
/// A Result indicating success or containing an error
pub fn deploy_function(name: &str) -> Result<(), FunctionError> {
    // Read configuration file
    let mut config_file = File::open(format!("{name}/{CONFIG_FILE_PATH}"))?;
    let mut contents = String::new();
    config_file.read_to_string(&mut contents)?;

    let config: FuncConfig = serde_json::from_str(&contents)?;

    // Validate function exists in config
    if !config.function_name.contains(&name.to_string()) {
        return Err(FunctionError::FunctionNotFound(name.to_string()));
    }

    let runtime = config.runtime;
    println!("🚀 Deploying service... '{}'", name);

    // Create ZIP archive with runtime-specific exclusions
    let mut dest_zip = Cursor::new(Vec::new());
    let exclude_files = match runtime.to_lowercase().as_str() {
        "go" => vec!["go.mod", "go.sum", ".git", ".gitignore"],
        "nodejs" | "node" | "typescript" | "ts" => {
            vec!["node_modules", ".git", ".gitignore", "dist", "*.log"]
        }
        _ => vec![],
    };

    compress_dir_with_excludes(Path::new(name), &mut dest_zip, &exclude_files)
        .map_err(|e| FunctionError::CompressionError(e.to_string()))?;

    // Reset the cursor to the beginning of the buffer
    dest_zip.set_position(0);

    println!("📦 Zipped up the folder service... '{}'", name);

    deploy_with_auth(name, dest_zip)?;

    Ok(())
}

/// Deploy a function using authentication
fn deploy_with_auth(name: &str, dest_zip: Cursor<Vec<u8>>) -> Result<String, FunctionError> {
    // Load authentication session
    let session = load_session()?;

    // Create multipart form
    let form = multipart::Form::new().part(
        "file",
        multipart::Part::reader(dest_zip)
            .file_name(format!("{name}.zip"))
            .mime_str("application/zip")?,
    );

    // Set up authorization headers
    let mut headers = HeaderMap::new();
    headers.insert(
        header::AUTHORIZATION,
        HeaderValue::from_str(&format!("Bearer {}", session.token))
            .map_err(|_| FunctionError::CompressionError("Invalid token format".to_string()))?,
    );

    // Build client with timeout
    let client = Client::builder()
        .timeout(Duration::from_secs(REQUEST_TIMEOUT_SECS))
        .default_headers(headers)
        .build()?;

    // Send request to API
    let response = client
        .post(host_manager::function_upload_url())
        .multipart(form)
        .send()?;

    // Check the response
    if response.status().is_success() {
        let response_text = response.text()?;

        // Generate function URL
        let function_url = generate_function_url(name, &session.user_uuid);

        // Print deployment success message with URL
        println!("✅ Function deployed successfully!");
        println!("📝 Function name: {}", name);
        println!("🌐 Function URL: {}", function_url);
        println!("🔗 You can invoke your function by making requests to the URL above");

        Ok(response_text)
    } else {
        let status = response.status();
        let error_text = response
            .text()
            .unwrap_or_else(|_| "Unknown error".to_string());

        Err(FunctionError::CompressionError(format!(
            "API error: Status code {}. {}",
            status, error_text
        )))
    }
}

/// Generate the function URL for a deployed function
fn generate_function_url(function_name: &str, user_uuid: &str) -> String {
    format!(
        "{}/invok/{}/{}",
        host_manager::base_url(),
        user_uuid,
        function_name
    )
}

/// Stream logs from a deployed function
///
/// # Arguments
///
/// * `name` - The name of the function to stream logs from
///
/// # Returns
///
/// A Result indicating success or containing an error
pub fn stream_logs(name: &str) -> Result<(), FunctionError> {
    // Load authentication session
    let session = load_session()?;

    // Build the logs URL
    let logs_url = host_manager::function_logs_url(&session.user_uuid, name);

    // Set up authorization headers
    let mut headers = HeaderMap::new();
    headers.insert(
        header::AUTHORIZATION,
        HeaderValue::from_str(&format!("Bearer {}", session.token))
            .map_err(|_| FunctionError::CompressionError("Invalid token format".to_string()))?,
    );

    // Use minimal single-threaded runtime for streaming
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_io()
        .enable_time()
        .build()
        .map_err(|e| FunctionError::IoError(io::Error::new(io::ErrorKind::Other, e)))?;

    rt.block_on(async { stream_logs_async(&logs_url, headers).await })
}

/// Async function to handle log streaming
async fn stream_logs_async(url: &str, headers: HeaderMap) -> Result<(), FunctionError> {
    // Build async client
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(300)) // 5 minute timeout for streaming
        .default_headers(headers)
        .build()
        .map_err(|e| FunctionError::RequestError(e))?;

    println!("🔍 Connecting to function logs...");

    // Send request to logs endpoint
    let response = client
        .get(url)
        .send()
        .await
        .map_err(|e| FunctionError::RequestError(e))?;

    if !response.status().is_success() {
        let status = response.status();
        let error_text = response
            .text()
            .await
            .unwrap_or_else(|_| "Unknown error".to_string());

        return Err(FunctionError::CompressionError(format!(
            "Failed to connect to logs: Status code {}. {}",
            status, error_text
        )));
    }

    println!("📡 Connected! Streaming logs... (Press Ctrl+C to stop)\n");

    // Stream the response
    let mut stream = response.bytes_stream();

    while let Some(chunk) = TryStreamExt::try_next(&mut stream)
        .await
        .map_err(|e| FunctionError::RequestError(e))?
    {
        let text = String::from_utf8_lossy(&chunk);

        // Filter out empty lines and just print the log content
        for line in text.lines() {
            if !line.trim().is_empty() {
                // Parse Server-Sent Events format if needed
                if line.starts_with("data:") {
                    let log_content = &line[5..]; // Remove "data:" prefix
                    if !log_content.trim().is_empty() {
                        println!("{}", log_content);
                    }
                } else if !line.starts_with(":")
                    && !line.starts_with("event:")
                    && !line.starts_with("id:")
                {
                    // Print non-SSE control lines directly
                    println!("{}", line);
                }
            }
        }

        // Flush stdout to ensure real-time output
        io::stdout()
            .flush()
            .map_err(|e| FunctionError::IoError(e))?;
    }

    println!("\n📴 Log stream ended");
    Ok(())
}
