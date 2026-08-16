#!/usr/bin/env bash
set -euo pipefail
source "$(dirname "${BASH_SOURCE[0]}")/common.sh"

build_release

WORK_DIR="$(mktemp -d)"
trap 'rm -rf "$WORK_DIR"' EXIT
install -m755 "$BINARY" "$WORK_DIR/omp-gtk"
install -m644 "$DESKTOP_FILE" "$WORK_DIR/dev.omp.Gtk.desktop"
install -m644 "$ICON_FILE" "$WORK_DIR/dev.omp.Gtk.svg"
install -m644 "$LICENSE_FILE" "$WORK_DIR/LICENSE"

ARCH_VERSION="${PACKAGE_VERSION//-/.}"
ARCH="$(uname -m)"
cat >"$WORK_DIR/PKGBUILD" <<EOF
pkgname=omp-gtk
pkgver=$ARCH_VERSION
pkgrel=1
pkgdesc='$PACKAGE_DESCRIPTION'
arch=('$ARCH')
license=('MIT')
depends=('alsa-lib' 'gtk4>=4.22' 'libadwaita>=1.9' 'fontconfig')
options=('!debug')
source=('omp-gtk' 'dev.omp.Gtk.desktop' 'dev.omp.Gtk.svg' 'LICENSE')
sha256sums=('SKIP' 'SKIP' 'SKIP' 'SKIP')

package() {
    install -Dm755 "\$srcdir/omp-gtk" "\$pkgdir/usr/bin/omp-gtk"
    install -Dm644 "\$srcdir/dev.omp.Gtk.desktop" \
        "\$pkgdir/usr/share/applications/dev.omp.Gtk.desktop"
    install -Dm644 "\$srcdir/dev.omp.Gtk.svg" \
        "\$pkgdir/usr/share/icons/hicolor/scalable/apps/dev.omp.Gtk.svg"
    install -Dm644 "\$srcdir/LICENSE" \
        "\$pkgdir/usr/share/licenses/omp-gtk/LICENSE"
}
EOF

(
    cd "$WORK_DIR"
    PKGDEST="$DIST_DIR" makepkg --force --nodeps --noconfirm
)
