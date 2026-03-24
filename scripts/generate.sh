#!/usr/bin/env bash
set -euo pipefail

REPO_ROOT="$(git rev-parse --show-toplevel)"

echo "Generating CRD from Rust types..."
cargo run -q -- crd > "${REPO_ROOT}/deploy/crd.yaml"

echo "Building install.yaml via kustomize..."
mkdir -p "${REPO_ROOT}/deploy/manifests"
kustomize build "${REPO_ROOT}/deploy" > "${REPO_ROOT}/deploy/manifests/install.yaml"

echo "Done. Generated:"
echo "  deploy/crd.yaml"
echo "  deploy/manifests/install.yaml"
