#!/usr/bin/env bash
# =====================================================================
# LexLyrics Portable Windows x86_64 Cross-Build & Packaging Script
# Uses rootless Podman (Fedora MinGW-w64 + GTK4) to build a standalone
# lexlyrics-v<VERSION>-windows-x86_64.zip with GTK4 runtime DLLs,
# compiled GSettings schema, themes, and NLLB translator server.
# =====================================================================
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
VERSION="$(sed -n 's/^version = "\([^"]*\)"/\1/p' "$ROOT_DIR/Cargo.toml" | head -n1)"
IMAGE_NAME="lexlyrics-win64-builder"
BUNDLE_NAME="lexlyrics-v${VERSION}-windows-x86_64"
ZIP_FILE="$ROOT_DIR/${BUNDLE_NAME}.zip"

if ! podman image exists "$IMAGE_NAME"; then
    echo "==> Building rootless Podman cross-compiler image ($IMAGE_NAME)..."
    EMPTY_CTX="$(mktemp -d)"
    trap 'rm -rf "$EMPTY_CTX"' EXIT
    podman build -t "$IMAGE_NAME" -f - "$EMPTY_CTX" << 'Containerfile'
FROM registry.fedoraproject.org/fedora:41
RUN dnf install -y -q --setopt=install_weak_deps=False \
        gcc \
        gcc-c++ \
        make \
        mingw64-gcc \
        mingw64-gcc-c++ \
        mingw64-binutils \
        mingw64-winpthreads-static \
        mingw64-gtk4 \
        mingw64-gettext \
        mingw64-librsvg2 \
        glib2-devel \
        pkgconf-pkg-config \
        zip \
        findutils \
    && dnf clean all
Containerfile
fi

echo "==> Cross-compiling LexLyrics v${VERSION} for Windows (x86_64-pc-windows-gnu)..."
mkdir -p /tmp/lexlyrics-win64-target
podman run --rm \
  -v "$HOME/.cargo:/root/.cargo" \
  -v "$HOME/.rustup:/root/.rustup" \
  -v "$ROOT_DIR:/workspace" \
  -v "/tmp/lexlyrics-win64-target:/win-target" \
  -w /workspace \
  "$IMAGE_NAME" \
  bash -e -c '
    export PATH="/root/.cargo/bin:$PATH"
    export CARGO_TARGET_DIR="/win-target"
    rustup target add x86_64-pc-windows-gnu

    MINGW_ROOT="/usr/x86_64-w64-mingw32/sys-root/mingw"
    export PKG_CONFIG_ALLOW_CROSS=1
    export PKG_CONFIG_LIBDIR_x86_64_pc_windows_gnu="$MINGW_ROOT/lib/pkgconfig:$MINGW_ROOT/share/pkgconfig"
    export PKG_CONFIG_PATH_x86_64_pc_windows_gnu="$MINGW_ROOT/lib/pkgconfig:$MINGW_ROOT/share/pkgconfig"
    export CC_x86_64_pc_windows_gnu="x86_64-w64-mingw32-gcc"
    export CXX_x86_64_pc_windows_gnu="x86_64-w64-mingw32-g++"
    export AR_x86_64_pc_windows_gnu="x86_64-w64-mingw32-ar"
    export CARGO_TARGET_X86_64_PC_WINDOWS_GNU_LINKER="x86_64-w64-mingw32-gcc"
    export CARGO_TARGET_X86_64_PC_WINDOWS_GNU_RUSTFLAGS="-C link-arg=-Wl,--allow-multiple-definition"
    export MINGW_PREFIX="$MINGW_ROOT"
    export X86_64_PC_WINDOWS_GNU_MINGW_PREFIX="$MINGW_ROOT"
    export GETTEXT_DIR="$MINGW_ROOT"
    export GETTEXT_BIN_DIR="$MINGW_ROOT/bin"
    export GETTEXT_LIB_DIR="$MINGW_ROOT/lib"
    export GETTEXT_INCLUDE_DIR="$MINGW_ROOT/include"
    export WAYLYRICS_THEME_PRESETS_DIR="themes"

    cargo build --release --target x86_64-pc-windows-gnu

    STAGE="/tmp/'"$BUNDLE_NAME"'"
    rm -rf "$STAGE"
    mkdir -p "$STAGE/themes" "$STAGE/share/glib-2.0/schemas" "$STAGE/share/icons" "$STAGE/lib"

    cp "$CARGO_TARGET_DIR/x86_64-pc-windows-gnu/release/lexlyrics.exe" "$STAGE/"

    # Copy required MinGW runtime + GTK4 DLLs
    MINGW_ROOT="/usr/x86_64-w64-mingw32/sys-root/mingw"
    cp "$MINGW_ROOT"/bin/*.dll "$STAGE/"
    if [ -d "$MINGW_ROOT/lib/gdk-pixbuf-2.0" ]; then
        cp -r "$MINGW_ROOT/lib/gdk-pixbuf-2.0" "$STAGE/lib/"
    fi
    if [ -d "$MINGW_ROOT/share/icons/Adwaita" ]; then
        cp -r "$MINGW_ROOT/share/icons/Adwaita" "$STAGE/share/icons/"
    fi

    # Themes, schemas, docs, NLLB server
    cp -r themes/* "$STAGE/themes/"
    cp metainfo/io.github.waylyrics.Waylyrics.gschema.xml "$STAGE/share/glib-2.0/schemas/"
    glib-compile-schemas "$STAGE/share/glib-2.0/schemas"
    cp -r contrib "$STAGE/"
    cp README.md LICENSE "$STAGE/"

    rm -f "/workspace/'"$BUNDLE_NAME"'.zip"
    (cd /tmp && zip -qr "/workspace/'"$BUNDLE_NAME"'.zip" "'"$BUNDLE_NAME"'")
  '

echo "==> Windows portable bundle ready:"
echo "    - $ZIP_FILE ($(du -h "$ZIP_FILE" | cut -f1))"
