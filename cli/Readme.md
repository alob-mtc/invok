# CLI

This CLI tool provides functionality to create and deploy serverless functions.

## Installation

To install the CLI, clone the repository and build it using Cargo:

```sh
git clone <repository-url>
cd cli
cargo build --release
```

## Usage

The CLI provides the following subcommands:

### Functions

#### Create function

Creates a new serverless function.

```sh
cli create-function -n <n> [-r <RUNTIME>]
```

Arguments

- `-n, --name <n>`: The name of the function to create (required).
- `-r, --runtime <RUNTIME>`: The runtime for the function (optional, default: go).

Example

```sh
cli create-function -n my-function -r go
```

#### Deploy function

Deploys an existing serverless function

```sh
cli deploy-function -n <n>
```

Arguments

- `-n, --name <n>`: The name of the function to deploy (required).

Example

```sh
cli deploy-function -n my-function
```

#### List functions

Lists all your deployed functions.

```sh
cli list
```

### Authentication

The CLI now supports user authentication for secure function deployment.

#### Register

Register a new user account:

```sh
cli register --email user@example.com --password your_password
```

Arguments

- `-e, --email <EMAIL>`: The email to register with (required).
- `-p, --password <PASSWORD>`: The password to register with (required).

#### Login

Log in to an existing account:

```sh
cli login --email user@example.com --password your_password
```

Arguments

- `-e, --email <EMAIL>`: The email to login with (required).
- `-p, --password <PASSWORD>`: The password to login with (required).

#### Logout

Remove your locally stored authentication:

```sh
cli logout
```

## Session Management

Your authentication token is stored in a file named `.serverless-cli-auth` in your home directory. This file contains your user ID, email, and authentication token.

When you're logged in, the CLI will automatically use your authentication token when deploying functions. If you're not logged in, it will fail.

## Function Namespacing

All functions are automatically namespaced by your user UUID, which means:

- You can have functions with the same name as other users without conflicts
- Only you can access and invoke your own functions
- The system enforces isolation between users' functions

Function URLs follow the pattern `/functions/{user-uuid}/invoke/{function-name}`, but the CLI handles this transparently.

## Security Considerations

- Do not share your authentication token with others
- Use a strong password for your account
- If you suspect your token has been compromised, logout and login again to get a new token

