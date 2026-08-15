#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
DIST_DIR="$ROOT_DIR/dist"
TARGET="${1:-all}"

usage() {
    cat <<'EOF'
Usage: packaging/build-packages.sh [all|ubuntu|fedora|arch]

Builds installable Linux packages in isolated distro containers and writes them
to dist/. Set CONTAINER_RUNTIME=docker or podman to choose the runtime. Install
the Ubuntu package with packaging/install-deb.sh to preserve apt sandboxing.
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

for distro in "${TARGETS[@]}"; do
    image="omp-native-package-$distro"
    case "$distro" in
        ubuntu) package_script=package-deb.sh ;;
        fedora) package_script=package-rpm.sh ;;
        arch) package_script=package-arch.sh ;;
    esac

    "$RUNTIME" build \
        --file "$ROOT_DIR/packaging/containers/$distro.Dockerfile" \
        --tag "$image" \
        "$ROOT_DIR"

    "$RUNTIME" run --rm \
        --user "$(id -u):$(id -g)" \
        --env HOME=/tmp/omp-native-home \
        --env CARGO_HOME=/tmp/omp-native-home/cargo \
        --env SOURCE_DATE_EPOCH="$SOURCE_DATE_EPOCH" \
        --volume "$ROOT_DIR:/workspace:ro" \
        --volume "$DIST_DIR:/dist" \
        --workdir /workspace \
        "$image" "/workspace/packaging/scripts/$package_script"
done
