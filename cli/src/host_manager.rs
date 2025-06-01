/*!
This module serves as the base for host management for the CLI.
- Handles injecting the correct host at build time.
*/

/// HOST_BASE is the base URL for the API server
///
/// TODO: dynamically configure the host
const HOST_BASE: &str = "https://freeserverless.com";

// const HOST_BASE: &str = "http://localhost:3000";

/// Generates the URL for the login endpoint
pub fn auth_login_url() -> String {
    format!("{}/auth/login", HOST_BASE)
}
/// Generates the URL for the register endpoint
pub fn auth_register_url() -> String {
    format!("{}/auth/register", HOST_BASE)
}
/// Generates the URL for the function upload endpoint
pub fn function_upload_url() -> String {
    format!("{}/invok/deploy", HOST_BASE)
}
/// Generates the URL for the function list endpoint
pub fn function_list_url() -> String {
    format!("{}/invok/list", HOST_BASE)
}
