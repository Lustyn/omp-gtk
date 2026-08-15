#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
DIST_DIR="${DIST_DIR:-/dist}"
TARGET_DIR="${CARGO_TARGET_DIR:-/tmp/omp-native-target}"
BINARY="$TARGET_DIR/release/omp-native"
DESKTOP_FILE="$ROOT_DIR/packaging/dev.omp.Native.desktop"
ICON_FILE="$ROOT_DIR/src/assets/omp.svg"

PACKAGE_VERSION="$(
    cargo metadata --locked --no-deps --format-version 1 --manifest-path "$ROOT_DIR/Cargo.toml" |
        python3 -c 'import json, sys; print(json.load(sys.stdin)["packages"][0]["version"])'
)"
PACKAGE_DESCRIPTION="$(
    cargo metadata --locked --no-deps --format-version 1 --manifest-path "$ROOT_DIR/Cargo.toml" |
        python3 -c 'import json, sys; print(json.load(sys.stdin)["packages"][0]["description"])'
)"

build_release() {
    mkdir -p "$DIST_DIR" "${CARGO_HOME:-/tmp/omp-native-cargo}" "$TARGET_DIR"
    export CARGO_TARGET_DIR="$TARGET_DIR"
    export SOURCE_DATE_EPOCH="${SOURCE_DATE_EPOCH:-$(stat -c %Y "$ROOT_DIR/Cargo.lock")}"
    cargo build --locked --release --manifest-path "$ROOT_DIR/Cargo.toml"
}

install_payload() {
    local root="$1"
    install -Dm755 "$BINARY" "$root/usr/bin/omp-native"
    install -Dm644 "$DESKTOP_FILE" "$root/usr/share/applications/dev.omp.Native.desktop"
    install -Dm644 "$ICON_FILE" "$root/usr/share/icons/hicolor/scalable/apps/dev.omp.Native.svg"
}
