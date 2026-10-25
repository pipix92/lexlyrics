#!/usr/bin/env bash
# =====================================================================
# LexLyrics Debian Packaging Script
# Generates .deb and .tar.gz release bundles for Linux
# =====================================================================
set -e

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
PKG_DIR="/tmp/lexlyrics_0.5.0-1_amd64"
DEB_FILE="$ROOT_DIR/lexlyrics_0.5.0-1_amd64.deb"
TAR_FILE="$ROOT_DIR/lexlyrics-v0.5.0-linux-x86_64.tar.gz"

echo "==> Preparing package directory structure..."
rm -rf "$PKG_DIR"
mkdir -p "$PKG_DIR/DEBIAN"
mkdir -p "$PKG_DIR/usr/bin"
mkdir -p "$PKG_DIR/usr/share/applications"
mkdir -p "$PKG_DIR/usr/share/icons/hicolor/512x512/apps"
mkdir -p "$PKG_DIR/usr/share/lexlyrics/themes"
mkdir -p "$PKG_DIR/usr/share/pixmaps"
mkdir -p "$PKG_DIR/usr/share/doc/lexlyrics"
mkdir -p "$PKG_DIR/usr/share/glib-2.0/schemas"

# Compile release binary incrementally in container
echo "==> Compiling release binary in container..."
podman run --rm \
  -v /home/lex/.cargo:/root/.cargo \
  -v /home/lex/.rustup:/root/.rustup:ro \
  -v "$ROOT_DIR:/workspace" \
  -w /workspace \
  lexlyrics-builder \
  bash -c 'export PATH="/root/.cargo/bin:$PATH"; cargo build --release'

# 1. Binary & Symlink
cp "$ROOT_DIR/target/release/lexlyrics" "$PKG_DIR/usr/bin/lexlyrics"
chmod 755 "$PKG_DIR/usr/bin/lexlyrics"
ln -sf lexlyrics "$PKG_DIR/usr/bin/waylyrics"

# 2. Icons
ICON_SRC="/home/lex/.local/share/icons/lexlyrics-cosmic-mononoki-title.png"
if [ ! -f "$ICON_SRC" ]; then
    ICON_SRC="/home/lex/.local/share/icons/lexlyrics.png"
fi
cp "$ICON_SRC" "$PKG_DIR/usr/share/icons/hicolor/512x512/apps/lexlyrics.png"
cp "$ICON_SRC" "$PKG_DIR/usr/share/pixmaps/lexlyrics.png"
ln -sf lexlyrics.png "$PKG_DIR/usr/share/icons/hicolor/512x512/apps/io.github.waylyrics.Waylyrics.png"

# 3. Themes
cp -r "$ROOT_DIR/themes/"* "$PKG_DIR/usr/share/lexlyrics/themes/"

# 4. Desktop Entries
cat << "EOF" > "$PKG_DIR/usr/share/applications/lexlyrics.desktop"
[Desktop Entry]
Version=1.0
Type=Application
Name=LexLyrics
GenericName=Desktop Lyrics & Karaoke
Comment=Real-time karaoke lyrics for Linux with NLLB-200 offline neural translation
Exec=/usr/bin/lexlyrics
Icon=lexlyrics
Terminal=false
Categories=AudioVideo;Audio;Player;Utility;
StartupWMClass=io.github.waylyrics.Waylyrics
Keywords=lyrics;karaoke;music;translation;nllb;
EOF

cp "$PKG_DIR/usr/share/applications/lexlyrics.desktop" "$PKG_DIR/usr/share/applications/io.github.waylyrics.Waylyrics.desktop"

# 5. Documentation
cp "$ROOT_DIR/README.md" "$PKG_DIR/usr/share/doc/lexlyrics/"
cp "$ROOT_DIR/LICENSE" "$PKG_DIR/usr/share/doc/lexlyrics/copyright"

cp "$ROOT_DIR/metainfo/io.github.waylyrics.Waylyrics.gschema.xml" "$PKG_DIR/usr/share/glib-2.0/schemas/"

# 6. Control File
cat << "EOF" > "$PKG_DIR/DEBIAN/control"
Package: lexlyrics
Version: 0.5.0-1
Section: sound
Priority: optional
Architecture: amd64
Depends: libgtk-4-1, libcairo2, libglib2.0-0, libssl3, libdbus-1-3
Maintainer: Lex <lex@debian-pc>
Description: LexLyrics - Desktop Karaoke Lyrics for Linux with Neural Offline Translation
 Fork avanzato di Waylyrics per Debian/Linux.
 Caratteristiche:
  - Scorrimento karaoke a 4 righe verso l alto
  - Traduzione neurale offline multilingue NLLB-200 (Meta 600M)
  - VRAM tiering dinamico con accelerazione CUDA RTX 3050
  - Filtro anti-cinese automatico su LRCLib e NetEase
  - Dimensioni a schermo intero con persistenza coordinate
  - Modalita passthrough e spostamento con Alt+Mouse Drag
  - Icona cosmica con Mononoki Nerd Font
EOF

# 7. Postinst script
cat << "EOF" > "$PKG_DIR/DEBIAN/postinst"
#!/bin/sh
set -e
if [ -x /usr/bin/update-desktop-database ]; then
    update-desktop-database -q /usr/share/applications || true
fi
if [ -x /usr/bin/gtk-update-icon-cache ]; then
    gtk-update-icon-cache -q /usr/share/icons/hicolor || true
fi
if [ -x /usr/bin/glib-compile-schemas ]; then
    glib-compile-schemas /usr/share/glib-2.0/schemas || true
fi
exit 0
EOF
chmod 755 "$PKG_DIR/DEBIAN/postinst"

echo "==> Building .deb package..."
dpkg-deb --root-owner-group --build "$PKG_DIR" "$DEB_FILE"

echo "==> Building standalone .tar.gz bundle..."
TAR_TMP="/tmp/lexlyrics-v0.5.0-linux-x86_64"
rm -rf "$TAR_TMP"
mkdir -p "$TAR_TMP"
cp "$PKG_DIR/usr/bin/lexlyrics" "$TAR_TMP/"
cp -r "$PKG_DIR/usr/share/lexlyrics/themes" "$TAR_TMP/"
cp "$ICON_SRC" "$TAR_TMP/lexlyrics.png"
cp "$PKG_DIR/usr/share/applications/lexlyrics.desktop" "$TAR_TMP/"
cp "$ROOT_DIR/README.md" "$TAR_TMP/"
cp "$ROOT_DIR/LICENSE" "$TAR_TMP/"
tar -czf "$TAR_FILE" -C /tmp lexlyrics-v0.5.0-linux-x86_64

echo "==> Packaging complete:"
echo "    - Debian Package: $DEB_FILE ($(du -h "$DEB_FILE" | cut -f1))"
echo "    - Portable Tarball: $TAR_FILE ($(du -h "$TAR_FILE" | cut -f1))"
