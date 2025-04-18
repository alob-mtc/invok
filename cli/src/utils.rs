use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use std::fs::File;
use std::io::Write;
use std::path::Path;
use std::{fs, io};

#[derive(Serialize, Deserialize, Debug)]
pub struct GlobalConfig {
    pub function_name: Vec<String>,
    pub runtime: String,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct FuncConfig {
    pub function_name: String,
    pub runtime: String,
    pub env: Value,
}

pub fn create_fn_project_file(name: &str, runtime: &str) -> io::Result<File> {
    create_global_config_file(name, runtime)?;

    let path = Path::new(name);
    if path.exists() {
        return Err(io::Error::new(
            io::ErrorKind::AlreadyExists,
            format!("Folder '{}' already exists.", name),
        ));
    }

    fs::create_dir(path)?;
    create_fn_config(name, runtime)?;

    let routes_file_path = path.join("function.go");
    let routes_file = File::create(&routes_file_path)?;

    Ok(routes_file)
}

fn create_fn_config(name: &str, runtime: &str) -> io::Result<()> {
    let mut f = File::create(format!("{name}/config.json"))?;
    let config = FuncConfig {
        function_name: name.to_string(),
        runtime: runtime.to_string(),
        env: Value::Object(Map::new()),
    };
    let serialized = serde_json::to_string(&config)?;
    f.write_all(serialized.as_bytes())
}

fn create_global_config_file(name: &str, runtime: &str) -> io::Result<()> {
    if Path::new("./config.json").exists() {
        let f = File::open("./config.json")?;
        let mut content: GlobalConfig = serde_json::from_reader(&f)?;
        if content.function_name.contains(&name.to_string()) {
            return Err(std::io::Error::new(
                std::io::ErrorKind::AlreadyExists,
                format!("Function '{}' already exists.", name),
            ));
        }
        content.function_name.push(name.to_string());
        let mut f = File::create("./config.json")?;
        f.write_all(serde_json::to_string(&content)?.as_bytes())
    } else {
        let mut f = File::create("./config.json")?;
        let config = GlobalConfig {
            function_name: vec![name.to_string()],
            runtime: runtime.to_string(),
        };
        let serialized = serde_json::to_string(&config)?;
        f.write_all(serialized.as_bytes())
    }
}

pub fn init_go_mod(function_name: &str) -> std::io::Result<()> {
    println!("Initializing go mod...");
    let mut mod_file = File::create(format!("{}/go.mod", function_name))?;
    mod_file.write_all(shared_utils::template::FUNCTION_MODULE_TEMPLATE.as_bytes())
}
