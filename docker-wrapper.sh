#!/bin/bash

# Docker wrapper script for invok CLI
# This script handles authentication file mounting and provides a clean interface

# Configuration
REGISTRY="bolamigbe"
IMAGE_NAME="invok"
VERSION="latest"
FULL_IMAGE_NAME="${REGISTRY}/${IMAGE_NAME}:${VERSION}"

# Run the Docker command with simple volume mount
docker run --rm \
    -v "$(pwd):/app" \
    -w /app \
    "$FULL_IMAGE_NAME" \
    "$@" 