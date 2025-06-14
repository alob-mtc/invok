# Docker Usage for invok CLI

The `invok` CLI is available as a Docker image for easy distribution and consistent execution across different environments.

## Quick Start

### Pull the Image
```bash
docker pull bolamigbe/invok:latest
```

### Basic Usage

```bash
# Show help
docker run --rm -v $(pwd):/app -w /app bolamigbe/invok:latest --help

# Login (auth file will be saved in current directory)
docker run --rm -v $(pwd):/app -w /app bolamigbe/invok:latest login -e your@email.com -p yourpassword

# Create a function
docker run --rm -v $(pwd):/app -w /app bolamigbe/invok:latest create -n my-function -r go

# List functions
docker run --rm -v $(pwd):/app -w /app bolamigbe/invok:latest list

# Deploy a function
docker run --rm -v $(pwd):/app -w /app bolamigbe/invok:latest deploy -n my-function
```

## Authentication with Docker

The CLI automatically detects when it's running in Docker and saves the authentication file (`.serverless-cli-auth`) in the current working directory instead of the home directory. This means authentication persists between Docker runs without needing additional volume mounts!

### Simple Authentication Flow

```bash
# Login (creates .serverless-cli-auth in current directory)
docker run --rm -v $(pwd):/app -w /app bolamigbe/invok:latest login -e user@example.com -p your_password

# Subsequent commands automatically use the saved auth
docker run --rm -v $(pwd):/app -w /app bolamigbe/invok:latest list
docker run --rm -v $(pwd):/app -w /app bolamigbe/invok:latest create -n my-function
```

### Optional: Use the Wrapper Script
For even more convenience, use the provided wrapper script:

```bash
# Make the wrapper executable
chmod +x docker-wrapper.sh

# Use it like the native CLI
./docker-wrapper.sh login -e user@example.com -p your_password
./docker-wrapper.sh create -n my-function
./docker-wrapper.sh deploy -n my-function
```

## Shell Alias (Optional)

For convenience, you can create a shell alias:

```bash
# Add to your ~/.bashrc or ~/.zshrc
alias invok='docker run --rm -v $(pwd):/app -w /app bolamigbe/invok:latest'

# Then use it like a native command
invok --help
invok create -n my-function
invok deploy -n my-function
```

## Building the Image Locally

If you want to build the image yourself:

```bash
# Build latest version
make build-cli

# Build specific version
make build-cli-version VERSION=v1.0.0

# Test the build
make test-cli
```

## Publishing to Registry

```bash
# Push latest
make push-cli

# Push specific version
make push-cli-version VERSION=v1.0.0
```

## Image Details

- **Base Image**: `debian:bookworm-slim`
- **Architecture**: Multi-arch support (amd64, arm64)
- **User**: Runs as non-root user `cliuser` (UID 1000)
- **Working Directory**: `/app`
- **Entry Point**: `invok`
- **Image Size**: ~50MB (optimized with multi-stage build)

## Troubleshooting

### Permission Issues
If you encounter permission issues, ensure your user ID matches the container user:
```bash
# Check your user ID
id -u

# If different from 1000, you may need to build with custom UID
docker build --build-arg UID=$(id -u) -f Dockerfile.cli -t bolamigbe/invok:latest .
```

### Network Issues
Ensure Docker can access the internet for CLI operations:
```bash
# Test network connectivity
docker run --rm bolamigbe/invok:latest --help
```

### Authentication File Location
The CLI automatically chooses the authentication file location:
1. **In Docker**: `./.serverless-cli-auth` (current working directory)
2. **Native**: `~/.serverless-cli-auth` (home directory)