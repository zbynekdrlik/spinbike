#!/usr/bin/env bash
set -euo pipefail
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT_DIR="$(dirname "$SCRIPT_DIR")"
VERSION_FILE="$ROOT_DIR/VERSION"
if [[ ! -f "$VERSION_FILE" ]]; then echo "ERROR: VERSION file not found" >&2; exit 1; fi
VERSION="$(cat "$VERSION_FILE" | tr -d '[:space:]')"
if [[ -z "$VERSION" ]]; then echo "ERROR: VERSION file is empty" >&2; exit 1; fi
echo "Syncing version: $VERSION"
sed -i "s/^version = \"[^\"]*\"/version = \"$VERSION\"/" "$ROOT_DIR/Cargo.toml"
UI_CARGO="$ROOT_DIR/spinbike-ui/Cargo.toml"
if [[ -f "$UI_CARGO" ]]; then sed -i "s/^version = \"[^\"]*\"/version = \"$VERSION\"/" "$UI_CARGO"; fi
echo "Done. All version fields set to $VERSION"
