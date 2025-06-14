#!/bin/bash

# Build script for invok CLI Docker image
set -e

# Configuration
REGISTRY="bolamigbe"
IMAGE_NAME="invok"
VERSION=${1:-"latest"}
DOCKERFILE="Dockerfile.cli"

# Full image name
FULL_IMAGE_NAME="${REGISTRY}/${IMAGE_NAME}:${VERSION}"

echo "Building invok CLI Docker image..."
echo "Image: ${FULL_IMAGE_NAME}"
echo "Dockerfile: ${DOCKERFILE}"

# Build the Docker image
docker build -f "${DOCKERFILE}" -t "${FULL_IMAGE_NAME}" .

# Also tag as latest if building a specific version
if [ "${VERSION}" != "latest" ]; then
    docker tag "${FULL_IMAGE_NAME}" "${REGISTRY}/${IMAGE_NAME}:latest"
    echo "Also tagged as: ${REGISTRY}/${IMAGE_NAME}:latest"
fi

echo "Build completed successfully!"
echo ""
echo "To push to registry:"
echo "  docker push ${FULL_IMAGE_NAME}"
if [ "${VERSION}" != "latest" ]; then
    echo "  docker push ${REGISTRY}/${IMAGE_NAME}:latest"
fi
echo ""
echo "To test locally:"
echo "  docker run --rm -v \$(pwd):/app -w /app ${FULL_IMAGE_NAME} --help"
echo ""
echo "Example usage:"
echo "  docker run --rm -v \$(pwd):/app -w /app ${FULL_IMAGE_NAME} create -n my-function" 