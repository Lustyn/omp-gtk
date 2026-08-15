#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
DIST_DIR="$ROOT_DIR/dist"
TARGET="${1:-all}"

usage() {
    cat <<'EOF'
Usage: packaging/build-packages.sh [all|ubuntu|fedora|arch]

Builds installable Linux packages in isolated distro containers and writes them
to dist/. Cargo downloads and release artifacts persist under the user cache
directory; set PACKAGING_CACHE_DIR to override it. Set CONTAINER_RUNTIME=docker
or podman to choose the runtime. Install the Ubuntu package with
packaging/install-deb.sh to preserve apt sandboxing.
EOF
}

case "$TARGET" in
    all) TARGETS=(ubuntu fedora arch) ;;
    ubuntu|fedora|arch) TARGETS=("$TARGET") ;;
    -h|--help) usage; exit 0 ;;
    *) usage >&2; exit 2 ;;
esac

if [[ -n "${CONTAINER_RUNTIME:-}" ]]; then
    RUNTIME="$CONTAINER_RUNTIME"
elif command -v docker >/dev/null 2>&1; then
    RUNTIME=docker
elif command -v podman >/dev/null 2>&1; then
    RUNTIME=podman
else
    echo "error: docker or podman is required" >&2
    exit 1
fi

mkdir -p "$DIST_DIR"
SOURCE_DATE_EPOCH="${SOURCE_DATE_EPOCH:-$(stat -c %Y "$ROOT_DIR/Cargo.lock")}"
CACHE_DIR="${PACKAGING_CACHE_DIR:-${XDG_CACHE_HOME:-$HOME/.cache}/omp-native/packaging}"
mkdir -p "$CACHE_DIR/cargo"
CACHE_DIR="$(realpath "$CACHE_DIR")"
RUN_OPTIONS=(
    --rm
    --user "$(id -u):$(id -g)"
    --env HOME=/tmp/omp-native-home
    --env CARGO_HOME=/cache/cargo
    --env SOURCE_DATE_EPOCH="$SOURCE_DATE_EPOCH"
    --volume "$ROOT_DIR:/workspace:ro"
    --volume "$DIST_DIR:/dist"
    --workdir /workspace
)
if [[ "$(basename "$RUNTIME")" == podman ]]; then
    RUN_OPTIONS+=(--userns=keep-id)
fi

for distro in "${TARGETS[@]}"; do
    image="omp-native-package-$distro"
    case "$distro" in
        ubuntu) package_script=package-deb.sh ;;
        fedora) package_script=package-rpm.sh ;;
        arch) package_script=package-arch.sh ;;
    esac

    target_cache="$CACHE_DIR/target/$distro"
    mkdir -p "$target_cache"
    DISTRO_RUN_OPTIONS=(
        "${RUN_OPTIONS[@]}"
        --env CARGO_TARGET_DIR=/cache/target
        --volume "$CACHE_DIR/cargo:/cache/cargo"
        --volume "$target_cache:/cache/target"
    )

    if [[ "${PACKAGING_SKIP_IMAGE_BUILD:-0}" == 1 ]]; then
        if ! "$RUNTIME" image inspect "$image" >/dev/null 2>&1; then
            echo "error: prebuilt package image '$image' is not loaded" >&2
            exit 1
        fi
    else
        "$RUNTIME" build \
            --file "$ROOT_DIR/packaging/containers/$distro.Dockerfile" \
            --tag "$image" \
            "$ROOT_DIR"
    fi

    "$RUNTIME" run "${DISTRO_RUN_OPTIONS[@]}" \
        "$image" "/workspace/packaging/scripts/$package_script"
done
