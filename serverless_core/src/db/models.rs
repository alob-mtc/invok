use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use uuid::Uuid;

/// Represents a deployable function.
///
/// # Fields
/// - `name`: The unique name of the function.
/// - `runtime`: The runtime environment required by the function (e.g., "go").
/// - `content`: The zipped binary content of the function.
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct DeployableFunction {
    pub name: String,
    pub content: Vec<u8>,
    pub user_uuid: Uuid,
}

/// Represents the configuration for a function.
///
/// This configuration is typically extracted from a JSON file
/// bundled with the function's package.
///
/// # Fields
/// - `function_name`: The name of the function (should correspond to the `Function`'s name).
/// - `runtime`: The runtime environment for the function.
/// - `env`: Optional key-value pairs representing environment variables.
#[derive(Serialize, Deserialize, Debug)]
pub struct DeployableFunctionConfig {
    function_name: String,
    pub(crate) runtime: String,
    pub(crate) env: Option<HashMap<String, String>>,
}
