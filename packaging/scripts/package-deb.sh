#!/usr/bin/env bash
set -euo pipefail
source "$(dirname "${BASH_SOURCE[0]}")/common.sh"

build_release

WORK_DIR="$(mktemp -d)"
STAGING="$WORK_DIR/debian/omp-native"
trap 'rm -rf "$WORK_DIR"' EXIT
install_payload "$STAGING"

DEB_VERSION="${PACKAGE_VERSION//-/~}"
DEB_ARCH="$(dpkg --print-architecture)"
mkdir -p "$WORK_DIR/debian" "$STAGING/DEBIAN"
cat >"$WORK_DIR/debian/control" <<EOF
Source: omp-native
Section: devel
Priority: optional
Maintainer: Oh My Pi Contributors
Standards-Version: 4.7.2

Package: omp-native
Architecture: any
Description: $PACKAGE_DESCRIPTION
 Native GTK desktop client for working with Oh My Pi sessions.
EOF
SHLIBS_DEPENDS="$(
    cd "$WORK_DIR"
    dpkg-shlibdeps -O -edebian/omp-native/usr/bin/omp-native |
        sed -n 's/^shlibs:Depends=//p'
)"
INSTALLED_SIZE="$(du -sk "$STAGING/usr" | cut -f1)"

cat >"$STAGING/DEBIAN/control" <<EOF
Package: omp-native
Version: $DEB_VERSION
Section: devel
Priority: optional
Architecture: $DEB_ARCH
Maintainer: Oh My Pi Contributors
Installed-Size: $INSTALLED_SIZE
Depends: $SHLIBS_DEPENDS
Description: $PACKAGE_DESCRIPTION
 Native GTK desktop client for working with Oh My Pi sessions.
EOF

dpkg-deb --root-owner-group --build "$STAGING" \
    "$DIST_DIR/omp-native_${DEB_VERSION}_${DEB_ARCH}.deb"
