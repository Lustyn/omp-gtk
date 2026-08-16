#!/usr/bin/env bash
set -euo pipefail
source "$(dirname "${BASH_SOURCE[0]}")/common.sh"

build_release

WORK_DIR="$(mktemp -d)"
STAGING="$WORK_DIR/debian/omp-gtk"
trap 'rm -rf "$WORK_DIR"' EXIT
install_payload "$STAGING"
install -Dm644 "$LICENSE_FILE" "$STAGING/usr/share/doc/omp-gtk/copyright"

DEB_VERSION="${PACKAGE_VERSION//-/~}"
DEB_ARCH="$(dpkg --print-architecture)"
mkdir -p "$WORK_DIR/debian" "$STAGING/DEBIAN"
cat >"$WORK_DIR/debian/control" <<EOF
Source: omp-gtk
Section: devel
Priority: optional
Maintainer: omp-gtk Contributors
Standards-Version: 4.7.2

Package: omp-gtk
Architecture: any
Description: $PACKAGE_DESCRIPTION
 Native GTK desktop frontend for omp sessions.
EOF
SHLIBS_DEPENDS="$(
    cd "$WORK_DIR"
    dpkg-shlibdeps -O -edebian/omp-gtk/usr/bin/omp-gtk |
        sed -n 's/^shlibs:Depends=//p'
)"
INSTALLED_SIZE="$(du -sk "$STAGING/usr" | cut -f1)"

cat >"$STAGING/DEBIAN/control" <<EOF
Package: omp-gtk
Version: $DEB_VERSION
Section: devel
Priority: optional
Architecture: $DEB_ARCH
Maintainer: omp-gtk Contributors
Installed-Size: $INSTALLED_SIZE
Depends: $SHLIBS_DEPENDS
Description: $PACKAGE_DESCRIPTION
 Native GTK desktop frontend for omp sessions.
EOF

dpkg-deb --root-owner-group --build "$STAGING" \
    "$DIST_DIR/omp-gtk_${DEB_VERSION}_${DEB_ARCH}.deb"
