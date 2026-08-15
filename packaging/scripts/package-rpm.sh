#!/usr/bin/env bash
set -euo pipefail
source "$(dirname "${BASH_SOURCE[0]}")/common.sh"

build_release

TOPDIR="$(mktemp -d)"
trap 'rm -rf "$TOPDIR"' EXIT
mkdir -p "$TOPDIR"/{BUILD,BUILDROOT,RPMS,SOURCES,SPECS,SRPMS}
install -m755 "$BINARY" "$TOPDIR/SOURCES/omp-native"
install -m644 "$DESKTOP_FILE" "$TOPDIR/SOURCES/dev.omp.Native.desktop"
install -m644 "$ICON_FILE" "$TOPDIR/SOURCES/dev.omp.Native.svg"

RPM_VERSION="${PACKAGE_VERSION//-/_}"
cat >"$TOPDIR/SPECS/omp-native.spec" <<EOF
Name:           omp-native
Version:        $RPM_VERSION
Release:        1%{?dist}
Summary:        $PACKAGE_DESCRIPTION
License:        LicenseRef-Unknown
Requires:        alsa-lib
Requires:        gtk4 >= 4.22
Requires:        libadwaita >= 1.9
Source0:        omp-native
Source1:        dev.omp.Native.desktop
Source2:        dev.omp.Native.svg

%global debug_package %{nil}

%description
Native GTK desktop client for working with Oh My Pi sessions.

%prep

%build

%install
install -Dm755 %{SOURCE0} %{buildroot}%{_bindir}/omp-native
install -Dm644 %{SOURCE1} %{buildroot}%{_datadir}/applications/dev.omp.Native.desktop
install -Dm644 %{SOURCE2} %{buildroot}%{_datadir}/icons/hicolor/scalable/apps/dev.omp.Native.svg

%files
%{_bindir}/omp-native
%{_datadir}/applications/dev.omp.Native.desktop
%{_datadir}/icons/hicolor/scalable/apps/dev.omp.Native.svg
EOF

rpmbuild --define "_topdir $TOPDIR" -bb "$TOPDIR/SPECS/omp-native.spec"
RPM_FILES=("$TOPDIR"/RPMS/*/*.rpm)
if [[ ! -f "${RPM_FILES[0]}" ]]; then
    echo "error: rpmbuild did not produce an RPM" >&2
    exit 1
fi
cp "${RPM_FILES[@]}" "$DIST_DIR/"
