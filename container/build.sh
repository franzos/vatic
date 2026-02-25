#!/bin/sh
set -e

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
RUNTIME="${CONTAINER_RUNTIME:-podman}"
IMAGE="vatic-agent"
TAG="${1:-latest}"

${RUNTIME} build -t "${IMAGE}:${TAG}" "$SCRIPT_DIR"

echo "Built ${IMAGE}:${TAG}"
