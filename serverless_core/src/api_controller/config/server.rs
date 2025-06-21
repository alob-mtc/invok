use super::InvokConfigError;
use std::env;

// Env variables
const REDIS_URL_ENV_VARIABLE: &str = "REDIS_URL";
const DATABASE_URL_ENV_VARIABLE: &str = "DATABASE_URL";
const PORT_ENV_VARIABLE: &str = "SERVER_PORT";
const SERVER_HOST_ENV_VARIABLE: &str = "SERVER_HOST";
const AUTH_JWT_SECRET_ENV_VARIABLE: &str = "AUTH_JWT_SECRET";

const DOCKER_COMPOSE_NETWORK_ENV_VARIABLE: &str = "DOCKER_COMPOSE_NETWORK";
const DOCKER_HOST_ENV_VARIABLE: &str = "DOCKER_HOST";

/// Default port to use if not configured
const DEFAULT_PORT_VALUE: u16 = 3000;

/// Default host to bind to if not configured
const DEFAULT_HOST_VALUE: &str = "0.0.0.0";

/// Server configuration
#[derive(Debug, Clone)]
pub struct InvokServerConfig {
    /// Redis connection URL
    pub redis_url: String,

    /// Database connection URL
    pub database_url: String,

    /// JWT auth secret
    pub jwt_auth_secret: String,

    /// Server listen address
    pub host: String,

    /// Docker network  address
    pub docker_compose_network_host: String,

    /// Server listen port
    pub port: u16,
}

impl InvokServerConfig {
    /// Load configuration from environment variables
    pub fn from_env() -> Result<Self, InvokConfigError> {
        // Required variables
        let redis_url = env::var(REDIS_URL_ENV_VARIABLE)
            .map_err(|_| InvokConfigError::MissingVar(REDIS_URL_ENV_VARIABLE.to_string()))?;

        let database_url = env::var(DATABASE_URL_ENV_VARIABLE)
            .map_err(|_| InvokConfigError::MissingVar(DATABASE_URL_ENV_VARIABLE.to_string()))?;

        env::var(DOCKER_HOST_ENV_VARIABLE)
            .map_err(|_| InvokConfigError::MissingVar(DOCKER_HOST_ENV_VARIABLE.to_string()))?;

        let docker_compose_network_host =
            env::var(DOCKER_COMPOSE_NETWORK_ENV_VARIABLE).map_err(|_| {
                InvokConfigError::MissingVar(DOCKER_COMPOSE_NETWORK_ENV_VARIABLE.to_string())
            })?;

        let jwt_auth_secret = env::var(AUTH_JWT_SECRET_ENV_VARIABLE)
            .map_err(|_| InvokConfigError::MissingVar(AUTH_JWT_SECRET_ENV_VARIABLE.to_string()))?;

        let host =
            env::var(SERVER_HOST_ENV_VARIABLE).unwrap_or_else(|_| DEFAULT_HOST_VALUE.to_string());

        let port = match env::var(PORT_ENV_VARIABLE) {
            Ok(port_str) => port_str
                .parse::<u16>()
                .map_err(|_| InvokConfigError::InvalidPort(port_str))?,
            Err(_) => DEFAULT_PORT_VALUE,
        };

        Ok(Self {
            redis_url,
            database_url,
            jwt_auth_secret,
            docker_compose_network_host,
            host,
            port,
        })
    }
}
