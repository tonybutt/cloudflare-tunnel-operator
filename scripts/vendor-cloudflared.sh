#!/usr/bin/env bash
set -euo pipefail

VERSION="${1:?Usage: vendor-cloudflared.sh <version>}"
REPO_ROOT="$(git rev-parse --show-toplevel)"

curl -sL "https://raw.githubusercontent.com/cloudflare/cloudflared/refs/tags/${VERSION}/config/configuration.go" \
    -o "${REPO_ROOT}/codegen/configuration.go"

echo "Vendored cloudflared ${VERSION} configuration.go"
