use crate::host_manager;
use reqwest::blocking::Client;
use serde::{Deserialize, Serialize};
use std::fs::File;
use std::io::{self, Read, Write};
use std::path::Path;
use thiserror::Error;

// File to store auth token
const AUTH_FILE: &str = ".serverless-cli-auth";

/// Authentication errors
#[derive(Debug, Error)]
pub enum AuthError {
    #[error("Network error: {0}")]
    NetworkError(#[from] reqwest::Error),

    #[error("IO error: {0}")]
    IoError(#[from] io::Error),

    #[error("JSON error: {0}")]
    JsonError(#[from] serde_json::Error),

    #[error("Authentication error: {0}")]
    AuthenticationError(String),
}

/// User credentials for login/registration
#[derive(Serialize)]
pub struct Credentials {
    pub email: String,
    pub password: String,
}

/// Auth token response from the server
#[derive(Deserialize)]
pub struct AuthResponse {
    pub token: String,
    pub user: UserResponse,
}

/// User information response
#[derive(Deserialize)]
pub struct UserResponse {
    pub uuid: String,
    pub email: String,
}

/// Authentication session stored locally
#[derive(Serialize, Deserialize)]
pub struct AuthSession {
    pub token: String,
    pub user_uuid: String,
    pub email: String,
}

/// Registers a new user
///
/// # Arguments
///
/// * `email` - Email address for the new user
/// * `password` - Password for the new user
///
/// # Returns
///
/// An AuthSession on success or AuthError on failure
pub fn register(email: &str, password: &str) -> Result<AuthSession, AuthError> {
    let client = Client::new();
    let credentials = Credentials {
        email: email.to_string(),
        password: password.to_string(),
    };

    let response = client
        .post(host_manager::auth_register_url())
        .json(&credentials)
        .send()?;

    if !response.status().is_success() {
        let error_text = response.text()?;
        return Err(AuthError::AuthenticationError(error_text));
    }

    let auth_response: AuthResponse = response.json()?;

    // Save the session locally
    let session = AuthSession {
        token: auth_response.token,
        user_uuid: auth_response.user.uuid,
        email: auth_response.user.email,
    };

    save_session(&session)?;

    Ok(session)
}

/// Login a user
///
/// # Arguments
///
/// * `email` - Email address of the user
/// * `password` - Password of the user
///
/// # Returns
///
/// An AuthSession on success or AuthError on failure
pub fn login(email: &str, password: &str) -> Result<AuthSession, AuthError> {
    let client = Client::new();
    let credentials = Credentials {
        email: email.to_string(),
        password: password.to_string(),
    };

    let response = client
        .post(host_manager::auth_login_url())
        .json(&credentials)
        .send()?;

    if !response.status().is_success() {
        let error_text = response.text()?;
        return Err(AuthError::AuthenticationError(error_text));
    }

    let auth_response: AuthResponse = response.json()?;

    // Save the session locally
    let session = AuthSession {
        token: auth_response.token,
        user_uuid: auth_response.user.uuid,
        email: auth_response.user.email,
    };

    save_session(&session)?;

    Ok(session)
}

/// Save authentication session to a local file
fn save_session(session: &AuthSession) -> Result<(), AuthError> {
    let auth_file_path = get_auth_file_path();
    let serialized = serde_json::to_string_pretty(session)?;

    let mut file = File::create(auth_file_path)?;
    file.write_all(serialized.as_bytes())?;

    Ok(())
}

/// Load authentication session from the local file
pub fn load_session() -> Result<AuthSession, AuthError> {
    let auth_file_path = get_auth_file_path();

    if !auth_file_path.exists() {
        return Err(AuthError::AuthenticationError(
            "Not logged in. Please run 'cli login' first.".to_string(),
        ));
    }

    let mut file = File::open(auth_file_path)?;
    let mut contents = String::new();
    file.read_to_string(&mut contents)?;

    let session: AuthSession = serde_json::from_str(&contents)?;

    Ok(session)
}

/// Get the path to the auth file
fn get_auth_file_path() -> std::path::PathBuf {
    // Check if we're running in Docker environment
    if std::env::var("ENV").unwrap_or_default() == "DOCKER" {
        return Path::new(".").join(AUTH_FILE);
    }

    // For native execution, use home directory
    let home_dir = dirs::home_dir().unwrap_or_else(|| Path::new(".").to_path_buf());
    home_dir.join(AUTH_FILE)
}

/// Logout (remove saved session)
pub fn logout() -> Result<(), AuthError> {
    let auth_file_path = get_auth_file_path();

    if auth_file_path.exists() {
        std::fs::remove_file(auth_file_path)?;
    }

    Ok(())
}
