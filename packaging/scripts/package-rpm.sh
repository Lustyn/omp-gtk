#!/usr/bin/env bash
set -euo pipefail
source "$(dirname "${BASH_SOURCE[0]}")/common.sh"

build_release

TOPDIR="$(mktemp -d)"
trap 'rm -rf "$TOPDIR"' EXIT
mkdir -p "$TOPDIR"/{BUILD,BUILDROOT,RPMS,SOURCES,SPECS,SRPMS}
install -m755 "$BINARY" "$TOPDIR/SOURCES/omp-gtk"
install -m644 "$DESKTOP_FILE" "$TOPDIR/SOURCES/dev.omp.Gtk.desktop"
install -m644 "$ICON_FILE" "$TOPDIR/SOURCES/dev.omp.Gtk.svg"
install -m644 "$LICENSE_FILE" "$TOPDIR/SOURCES/LICENSE"

RPM_VERSION="${PACKAGE_VERSION//-/_}"
cat >"$TOPDIR/SPECS/omp-gtk.spec" <<EOF
Name:           omp-gtk
Version:        $RPM_VERSION
Release:        1%{?dist}
Summary:        $PACKAGE_DESCRIPTION
License:        MIT
Requires:        alsa-lib
Requires:        gtk4 >= 4.22
Requires:        libadwaita >= 1.9
Source0:        omp-gtk
Source1:        dev.omp.Gtk.desktop
Source2:        dev.omp.Gtk.svg
Source3:        LICENSE

%global debug_package %{nil}

%description
Native GTK desktop frontend for omp sessions.

%prep

%build

%install
install -Dm755 %{SOURCE0} %{buildroot}%{_bindir}/omp-gtk
install -Dm644 %{SOURCE1} %{buildroot}%{_datadir}/applications/dev.omp.Gtk.desktop
install -Dm644 %{SOURCE2} %{buildroot}%{_datadir}/icons/hicolor/scalable/apps/dev.omp.Gtk.svg
install -Dm644 %{SOURCE3} %{buildroot}%{_licensedir}/omp-gtk/LICENSE

%files
%{_bindir}/omp-gtk
%{_datadir}/applications/dev.omp.Gtk.desktop
%{_datadir}/icons/hicolor/scalable/apps/dev.omp.Gtk.svg
%license %{_licensedir}/omp-gtk/LICENSE
EOF

rpmbuild --define "_topdir $TOPDIR" -bb "$TOPDIR/SPECS/omp-gtk.spec"
RPM_FILES=("$TOPDIR"/RPMS/*/*.rpm)
if [[ ! -f "${RPM_FILES[0]}" ]]; then
    echo "error: rpmbuild did not produce an RPM" >&2
    exit 1
fi
cp "${RPM_FILES[@]}" "$DIST_DIR/"
