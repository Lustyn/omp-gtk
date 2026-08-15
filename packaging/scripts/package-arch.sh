#!/usr/bin/env bash
set -euo pipefail
source "$(dirname "${BASH_SOURCE[0]}")/common.sh"

build_release

WORK_DIR="$(mktemp -d)"
trap 'rm -rf "$WORK_DIR"' EXIT
install -m755 "$BINARY" "$WORK_DIR/omp-native"
install -m644 "$DESKTOP_FILE" "$WORK_DIR/dev.omp.Native.desktop"
install -m644 "$ICON_FILE" "$WORK_DIR/dev.omp.Native.svg"

ARCH_VERSION="${PACKAGE_VERSION//-/.}"
ARCH="$(uname -m)"
cat >"$WORK_DIR/PKGBUILD" <<EOF
pkgname=omp-native
pkgver=$ARCH_VERSION
pkgrel=1
pkgdesc='$PACKAGE_DESCRIPTION'
arch=('$ARCH')
license=('LicenseRef-Unknown')
depends=('gtk4>=4.22' 'libadwaita>=1.9' 'fontconfig')
options=('!debug')
source=('omp-native' 'dev.omp.Native.desktop' 'dev.omp.Native.svg')
sha256sums=('SKIP' 'SKIP' 'SKIP')

package() {
    install -Dm755 "\$srcdir/omp-native" "\$pkgdir/usr/bin/omp-native"
    install -Dm644 "\$srcdir/dev.omp.Native.desktop" \
        "\$pkgdir/usr/share/applications/dev.omp.Native.desktop"
    install -Dm644 "\$srcdir/dev.omp.Native.svg" \
        "\$pkgdir/usr/share/icons/hicolor/scalable/apps/dev.omp.Native.svg"
}
EOF

(
    cd "$WORK_DIR"
    PKGDEST="$DIST_DIR" makepkg --force --nodeps --noconfirm
)
