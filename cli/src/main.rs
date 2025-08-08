mod auth;
mod host_manager;
mod serverless_function;
mod utils;

use crate::auth::{login, logout, register};
use crate::serverless_function::{
    create_new_project, deploy_function, list_functions, stream_logs,
};
use clap::{Arg, Command};
use std::process;

fn main() {
    let matches = Command::new("CLI")
        .version("1.0")
        .author("Akinlua Bolamigbe <bolamigbeakinlua@gmail.com>")
        .about("Serverless Function Platform CLI - Create and deploy functions to the cloud")
        .subcommand(
            Command::new("create")
                .about("Creates a new function")
                .args([
                    Arg::new("name")
                        .short('n')
                        .long("name")
                        .value_name("NAME")
                        .required(true)
                        .help("The name of the function to create"),
                    Arg::new("runtime")
                        .short('r')
                        .default_value("go")
                        .long("runtime")
                        .value_name("RUNTIME")
                        .required(false)
                        .help("The runtime for the function (supported: go, nodejs)"),
                ]),
        )
        .subcommand(
            Command::new("deploy")
                .about("Deploys an existing function")
                .arg(
                    Arg::new("name")
                        .short('n')
                        .long("name")
                        .value_name("NAME")
                        .required(true)
                        .help("The name of the function to deploy"),
                ),
        )
        .subcommand(Command::new("list").about("Lists all functions"))
        .subcommand(
            Command::new("logs")
                .about("Stream logs from a function")
                .arg(
                    Arg::new("name")
                        .short('n')
                        .long("name")
                        .value_name("NAME")
                        .required(true)
                        .help("The name of the function to get logs from"),
                ),
        )
        .subcommand(
            Command::new("login")
                .about("Login to the serverless platform")
                .args([
                    Arg::new("email")
                        .short('e')
                        .long("email")
                        .value_name("EMAIL")
                        .required(true)
                        .help("The email to login with"),
                    Arg::new("password")
                        .short('p')
                        .long("password")
                        .value_name("PASSWORD")
                        .required(true)
                        .help("The password to login with"),
                ]),
        )
        .subcommand(
            Command::new("register").about("Register a new user").args([
                Arg::new("email")
                    .short('e')
                    .long("email")
                    .value_name("EMAIL")
                    .required(true)
                    .help("The email to register with"),
                Arg::new("password")
                    .short('p')
                    .long("password")
                    .value_name("PASSWORD")
                    .required(true)
                    .help("The password to register with"),
            ]),
        )
        .subcommand(Command::new("logout").about("Logout from the serverless platform"))
        .get_matches();

    match matches.subcommand() {
        Some(("create", sub_matches)) => {
            if let Some(name) = sub_matches.get_one::<String>("name") {
                if let Some(runtime) = sub_matches.get_one::<String>("runtime") {
                    if let Err(err) = create_new_project(name, runtime) {
                        eprintln!("Error creating function: {}", err);
                        process::exit(1);
                    }
                } else {
                    eprintln!("Runtime parameter is required");
                    process::exit(1);
                }
            } else {
                eprintln!("Name parameter is required");
                process::exit(1);
            }
        }
        Some(("deploy", sub_matches)) => {
            if let Some(name) = sub_matches.get_one::<String>("name") {
                match deploy_function(name) {
                    Ok(_) => {
                        println!("🎉 Deployment completed successfully!");
                    }
                    Err(err) => {
                        eprintln!("❌ Error deploying function: {}", err);
                        process::exit(1);
                    }
                }
            } else {
                eprintln!("Name parameter is required");
                process::exit(1);
            }
        }
        Some(("list", _)) => {
            if let Err(err) = list_functions() {
                eprintln!("Error getting function: {}", err);
                process::exit(1);
            }
        }
        Some(("logs", sub_matches)) => {
            if let Some(name) = sub_matches.get_one::<String>("name") {
                match stream_logs(name) {
                    Ok(_) => {
                        println!("Log streaming ended");
                    }
                    Err(err) => {
                        eprintln!("❌ Error streaming logs: {}", err);
                        process::exit(1);
                    }
                }
            } else {
                eprintln!("Name parameter is required");
                process::exit(1);
            }
        }
        Some(("login", sub_matches)) => {
            if let (Some(email), Some(password)) = (
                sub_matches.get_one::<String>("email"),
                sub_matches.get_one::<String>("password"),
            ) {
                match login(email, password) {
                    Ok(session) => {
                        println!(
                            "Logged in successfully as {} (User ID: {})",
                            session.email, session.user_uuid
                        );
                    }
                    Err(err) => {
                        eprintln!("Login failed: {}", err);
                        process::exit(1);
                    }
                }
            } else {
                eprintln!("Email and password are required");
                process::exit(1);
            }
        }
        Some(("register", sub_matches)) => {
            if let (Some(email), Some(password)) = (
                sub_matches.get_one::<String>("email"),
                sub_matches.get_one::<String>("password"),
            ) {
                match register(email, password) {
                    Ok(session) => {
                        println!(
                            "Registered and logged in successfully as {} (User ID: {})",
                            session.email, session.user_uuid
                        );
                    }
                    Err(err) => {
                        eprintln!("Registration failed: {}", err);
                        process::exit(1);
                    }
                }
            } else {
                eprintln!("Email and password are required");
                process::exit(1);
            }
        }
        Some(("logout", _)) => match logout() {
            Ok(_) => {
                println!("Logged out successfully");
            }
            Err(err) => {
                eprintln!("Logout failed: {}", err);
                process::exit(1);
            }
        },
        _ => {
            eprintln!("Please use a valid subcommand. Run with --help for more information.");
            process::exit(1);
        }
    }
}
