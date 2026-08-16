#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
DIST_DIR="${DIST_DIR:-$ROOT_DIR/dist}"
TARGET_DIR="${CARGO_TARGET_DIR:-$ROOT_DIR/target}"
APP_NAME="omp-gtk.app"

if [[ "$(uname -s)" != Darwin ]]; then
    echo "error: macOS packaging must run on macOS" >&2
    exit 1
fi

for command in brew cargo codesign dylibbundler hdiutil iconutil install_name_tool otool plutil python3 rsvg-convert; do
    if ! command -v "$command" >/dev/null 2>&1; then
        echo "error: required command '$command' was not found" >&2
        exit 1
    fi
done

PACKAGE_VERSION="$(
    cargo metadata --locked --no-deps --format-version 1 --manifest-path "$ROOT_DIR/Cargo.toml" |
        python3 -c 'import json, sys; print(json.load(sys.stdin)["packages"][0]["version"])'
)"
ARCH="$(uname -m)"
ARTIFACT_NAME="omp-gtk-${PACKAGE_VERSION}-macos-${ARCH}.dmg"
BINARY="$TARGET_DIR/release/omp-gtk"
BREW_PREFIX="$(brew --prefix)"
SCHEMA_DIR="$BREW_PREFIX/share/glib-2.0/schemas"
ADWAITA_ICONS="$(brew --prefix adwaita-icon-theme)/share/icons/Adwaita"
HICOLOR_ICONS="$(brew --prefix hicolor-icon-theme)/share/icons/hicolor"
PIXBUF_LOADERS="$BREW_PREFIX/lib/gdk-pixbuf-2.0/2.10.0/loaders"
PIXBUF_CACHE="$BREW_PREFIX/lib/gdk-pixbuf-2.0/2.10.0/loaders.cache"

for path in \
    "$SCHEMA_DIR/gschemas.compiled" \
    "$ADWAITA_ICONS" \
    "$HICOLOR_ICONS" \
    "$PIXBUF_LOADERS" \
    "$PIXBUF_CACHE"; do
    if [[ ! -e "$path" ]]; then
        echo "error: required runtime resource '$path' was not found" >&2
        exit 1
    fi
done

mkdir -p "$DIST_DIR" "$TARGET_DIR"
export CARGO_TARGET_DIR="$TARGET_DIR"
cargo build --locked --release --manifest-path "$ROOT_DIR/Cargo.toml"

WORK_DIR="$(mktemp -d "${TMPDIR:-/tmp}/omp-gtk-package.XXXXXX")"
trap 'rm -rf "$WORK_DIR"' EXIT
APP_DIR="$WORK_DIR/$APP_NAME"
CONTENTS_DIR="$APP_DIR/Contents"
MACOS_DIR="$CONTENTS_DIR/MacOS"
RESOURCES_DIR="$CONTENTS_DIR/Resources"
SHARE_DIR="$RESOURCES_DIR/share"
PIXBUF_BUNDLE_PATH="lib/gdk-pixbuf-2.0/2.10.0/loaders"
PIXBUF_BUNDLE_DIR="$RESOURCES_DIR/$PIXBUF_BUNDLE_PATH"

mkdir -p \
    "$MACOS_DIR" \
    "$SHARE_DIR/glib-2.0/schemas" \
    "$SHARE_DIR/icons" \
    "$PIXBUF_BUNDLE_DIR"
install -m 755 "$BINARY" "$MACOS_DIR/omp-gtk"
sed "s/@VERSION@/$PACKAGE_VERSION/g" \
    "$ROOT_DIR/packaging/macos/Info.plist.in" > "$CONTENTS_DIR/Info.plist"
plutil -lint "$CONTENTS_DIR/Info.plist" >/dev/null
install -m 644 "$SCHEMA_DIR/gschemas.compiled" "$SHARE_DIR/glib-2.0/schemas/gschemas.compiled"
ditto "$ADWAITA_ICONS" "$SHARE_DIR/icons/Adwaita"
ditto "$HICOLOR_ICONS" "$SHARE_DIR/icons/hicolor"

LOADER_FIX_ARGS=()
for loader in "$PIXBUF_LOADERS"/*.so; do
    [[ -e "$loader" ]] || continue
    loader_name="$(basename "$loader")"
    install -m 755 "$loader" "$PIXBUF_BUNDLE_DIR/$loader_name"
    LOADER_FIX_ARGS+=(--fix-file "Resources/$PIXBUF_BUNDLE_PATH/$loader_name")
done
if [[ "${#LOADER_FIX_ARGS[@]}" -eq 0 ]]; then
    echo "error: no GdkPixbuf loader modules were found in '$PIXBUF_LOADERS'" >&2
    exit 1
fi
sed "s|$PIXBUF_LOADERS|@GDK_PIXBUF_MODULEDIR@|g" \
    "$PIXBUF_CACHE" > "$PIXBUF_BUNDLE_DIR/loaders.cache.in"

ICONSET_DIR="$WORK_DIR/omp.iconset"
mkdir -p "$ICONSET_DIR"
for size in 16 32 128 256 512; do
    rsvg-convert --width "$size" --height "$size" \
        --output "$ICONSET_DIR/icon_${size}x${size}.png" "$ROOT_DIR/src/assets/omp.svg"
    doubled_size="$((size * 2))"
    rsvg-convert --width "$doubled_size" --height "$doubled_size" \
        --output "$ICONSET_DIR/icon_${size}x${size}@2x.png" "$ROOT_DIR/src/assets/omp.svg"
done
iconutil --convert icns --output "$RESOURCES_DIR/omp.icns" "$ICONSET_DIR"

(
    cd "$CONTENTS_DIR"
    dylibbundler \
        --no-codesign \
        --overwrite-dir \
        --bundle-deps \
        --fix-file "MacOS/omp-gtk" \
        "${LOADER_FIX_ARGS[@]}" \
        --dest-dir "Frameworks" \
        --install-path "@executable_path/../Frameworks/" \
        --search-path "$BREW_PREFIX/lib"
)

for loader in "$PIXBUF_BUNDLE_DIR"/*.so; do
    if [[ -n "$(otool -D "$loader" | sed -n '2p')" ]]; then
        install_name_tool -id "@rpath/$(basename "$loader")" "$loader"
    fi
    framework_rpaths="$(
        otool -l "$loader" |
            awk '$1 == "path" && $2 == "@executable_path/../Frameworks/" { count++ } END { print count + 0 }'
    )"
    while ((framework_rpaths > 1)); do
        install_name_tool -delete_rpath "@executable_path/../Frameworks/" "$loader"
        ((framework_rpaths -= 1))
    done
    codesign --force --sign - "$loader"
done

while IFS= read -r macho; do
    if otool -L "$macho" | awk '
        /\/opt\/homebrew\// || /\/usr\/local\/Cellar\// || /\/usr\/local\/opt\// { found = 1 }
        END { exit found ? 0 : 1 }
    '; then
        echo "error: Homebrew dependency remains in $macho" >&2
        otool -L "$macho" >&2
        exit 1
    fi
done < <(find "$MACOS_DIR" "$CONTENTS_DIR/Frameworks" "$PIXBUF_BUNDLE_DIR" -type f)

# Apple Silicon requires modified Mach-O files to carry a valid loadable signature.
# An ad-hoc signature has no identity and does not provide Gatekeeper trust.
codesign --force --deep --sign - "$APP_DIR"
codesign --verify --deep --strict "$APP_DIR"

DMG_ROOT="$WORK_DIR/dmg"
mkdir -p "$DMG_ROOT"
ditto "$APP_DIR" "$DMG_ROOT/$APP_NAME"
ln -s /Applications "$DMG_ROOT/Applications"
rm -f "$DIST_DIR/$ARTIFACT_NAME"
hdiutil create \
    -quiet \
    -volname "omp-gtk $PACKAGE_VERSION" \
    -srcfolder "$DMG_ROOT" \
    -format UDZO \
    -ov \
    "$DIST_DIR/$ARTIFACT_NAME"

echo "$DIST_DIR/$ARTIFACT_NAME"
