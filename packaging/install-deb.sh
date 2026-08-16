#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
PACKAGE="${1:-}"

if [[ -z "$PACKAGE" ]]; then
    packages=("$ROOT_DIR"/dist/omp-gtk_*.deb)
    if [[ ${#packages[@]} -ne 1 || ! -f "${packages[0]}" ]]; then
        echo "Usage: packaging/install-deb.sh path/to/omp-gtk.deb" >&2
        exit 2
    fi
    PACKAGE="${packages[0]}"
fi

if [[ ! -f "$PACKAGE" ]]; then
    echo "error: package not found: $PACKAGE" >&2
    exit 1
fi

INSTALL_DIR="$(mktemp -d /tmp/omp-gtk-install.XXXXXX)"
trap 'rm -rf "$INSTALL_DIR"' EXIT
chmod 755 "$INSTALL_DIR"
install -m644 "$PACKAGE" "$INSTALL_DIR/omp-gtk.deb"

sudo apt-get install --reinstall "$INSTALL_DIR/omp-gtk.deb"
