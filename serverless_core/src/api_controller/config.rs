use std::env;

use function::InvokFunctionConfig;
use server::InvokServerConfig;
use thiserror::Error;

mod function;
mod server;

/// Error that can occur during configuration loading
#[derive(Debug, Error)]
pub enum InvokConfigError {
    #[error("Missing environment variable: {0}")]
    MissingVar(String),

    #[error("The port {0} provided is invalid")]
    InvalidPort(String),

    #[error("Invalid configuration value: {0}")]
    InvalidValue(String),

    #[error("Environment error: {0}")]
    EnvError(#[from] env::VarError),
}

/// Complete application configuration
#[derive(Debug, Clone)]
pub struct InvokConfig {
    /// Server configuration
    pub server_config: InvokServerConfig,

    /// Function configuration
    pub function_config: InvokFunctionConfig,
}

impl InvokConfig {
    /// Load complete configuration from environment
    pub fn load() -> Result<Self, InvokConfigError> {
        let server_config = InvokServerConfig::from_env()?;
        let function_config = InvokFunctionConfig::from_env();

        Ok(Self {
            server_config,
            function_config,
        })
    }
}
