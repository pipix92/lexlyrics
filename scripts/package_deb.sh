#!/usr/bin/env bash
# =====================================================================
# LexLyrics Debian & Portable Linux Packaging Script
# Generates .deb and .tar.gz release bundles for Linux (x86_64)
# =====================================================================
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
VERSION="$(sed -n 's/^version = "\([^"]*\)"/\1/p' "$ROOT_DIR/Cargo.toml" | head -n1)"
PKG_DIR="/tmp/lexlyrics_${VERSION}-1_amd64"
DEB_FILE="$ROOT_DIR/lexlyrics_${VERSION}-1_amd64.deb"
TAR_NAME="lexlyrics-v${VERSION}-linux-x86_64"
TAR_FILE="$ROOT_DIR/${TAR_NAME}.tar.gz"

echo "==> Packaging LexLyrics v${VERSION} for Linux..."
rm -rf "$PKG_DIR"
mkdir -p "$PKG_DIR/DEBIAN"
mkdir -p "$PKG_DIR/usr/bin"
mkdir -p "$PKG_DIR/usr/share/applications"
mkdir -p "$PKG_DIR/usr/share/icons/hicolor/512x512/apps"
mkdir -p "$PKG_DIR/usr/share/lexlyrics/themes"
mkdir -p "$PKG_DIR/usr/share/pixmaps"
mkdir -p "$PKG_DIR/usr/share/doc/lexlyrics"
mkdir -p "$PKG_DIR/usr/share/glib-2.0/schemas"

echo "==> Compiling release binary in rootless Podman container..."
podman run --rm \
  -v "$HOME/.cargo:/root/.cargo" \
  -v "$HOME/.rustup:/root/.rustup:ro" \
  -v "$ROOT_DIR:/workspace" \
  -w /workspace \
  -e WAYLYRICS_THEME_PRESETS_DIR=/usr/share/lexlyrics/themes \
  lexlyrics-builder \
  bash -c 'export PATH="/root/.cargo/bin:$PATH"; cargo build --release'

# 1. Binary & Symlink
cp "$ROOT_DIR/target/release/lexlyrics" "$PKG_DIR/usr/bin/lexlyrics"
chmod 755 "$PKG_DIR/usr/bin/lexlyrics"
ln -sf lexlyrics "$PKG_DIR/usr/bin/waylyrics"

# 2. Icons
ICON_SRC="$ROOT_DIR/res/icons/lexlyrics.png"
cp "$ICON_SRC" "$PKG_DIR/usr/share/icons/hicolor/512x512/apps/lexlyrics.png"
cp "$ICON_SRC" "$PKG_DIR/usr/share/pixmaps/lexlyrics.png"
ln -sf lexlyrics.png "$PKG_DIR/usr/share/icons/hicolor/512x512/apps/io.github.waylyrics.Waylyrics.png"

for icon in "$ROOT_DIR"/res/icons/hicolor/*/apps/lexlyrics-tray.*; do
    rel="${icon#"$ROOT_DIR/res/icons/"}"
    install -Dm644 "$icon" "$PKG_DIR/usr/share/icons/$rel"
done

# 2b. Customization TUI (opened from the tray icon)
install -Dm755 "$ROOT_DIR/scripts/lexlyrics-theme" "$PKG_DIR/usr/bin/lexlyrics-theme"

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

# 5. Documentation & GSettings Schema
cp "$ROOT_DIR/README.md" "$PKG_DIR/usr/share/doc/lexlyrics/"
cp "$ROOT_DIR/LICENSE" "$PKG_DIR/usr/share/doc/lexlyrics/copyright"
cp "$ROOT_DIR/metainfo/io.github.waylyrics.Waylyrics.gschema.xml" "$PKG_DIR/usr/share/glib-2.0/schemas/"

# 6. Control File
cat << EOF > "$PKG_DIR/DEBIAN/control"
Package: lexlyrics
Version: ${VERSION}-1
Section: sound
Priority: optional
Architecture: amd64
Depends: libgtk-4-1, libcairo2, libglib2.0-0, libssl3, libdbus-1-3, libx11-6
Recommends: python3 (>= 3.11), xwayland
Maintainer: Lex <lex@debian-pc>
Description: LexLyrics - Desktop Karaoke Lyrics with Live Neural Translation
 Fork di Waylyrics con traduzione italiana in tempo reale (NLLB-200 + LLM).
 Caratteristiche:
  - Barra OSD compatta in vetro (spostabile, ridimensionabile, zoom Alt+rotella)
  - HUD a schermo intero non intrusivo con click-through fuori dal testo ed Esc/F11
  - Traduzione veloce offline NLLB-200 con fallback automatico GPU -> CPU
  - Rifinitura naturale via LLM (agy, hermes, claude) e salto brani gia' in italiano
  - Pulizia titoli Spotify/LRCLib, filtro crediti cinesi NetEase e stima tempi
  - Icona tray a doppia nota con editor TUI lexlyrics-theme
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
TAR_TMP="/tmp/${TAR_NAME}"
rm -rf "$TAR_TMP"
mkdir -p "$TAR_TMP/share/glib-2.0/schemas"
cp "$PKG_DIR/usr/bin/lexlyrics" "$TAR_TMP/"
cp "$PKG_DIR/usr/bin/lexlyrics-theme" "$TAR_TMP/"
cp -r "$ROOT_DIR/res/icons" "$TAR_TMP/icons"
cp -r "$PKG_DIR/usr/share/lexlyrics/themes" "$TAR_TMP/"
cp -r "$ROOT_DIR/contrib" "$TAR_TMP/"
cp "$ROOT_DIR/metainfo/io.github.waylyrics.Waylyrics.gschema.xml" "$TAR_TMP/share/glib-2.0/schemas/"
glib-compile-schemas "$TAR_TMP/share/glib-2.0/schemas"
cp "$ICON_SRC" "$TAR_TMP/lexlyrics.png"
cp "$PKG_DIR/usr/share/applications/lexlyrics.desktop" "$TAR_TMP/"
cp "$ROOT_DIR/README.md" "$TAR_TMP/"
cp "$ROOT_DIR/LICENSE" "$TAR_TMP/"
tar -czf "$TAR_FILE" -C /tmp "$TAR_NAME"

echo "==> Packaging complete:"
echo "    - Debian Package:   $DEB_FILE ($(du -h "$DEB_FILE" | cut -f1))"
echo "    - Portable Tarball: $TAR_FILE ($(du -h "$TAR_FILE" | cut -f1))"
