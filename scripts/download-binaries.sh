#!/usr/bin/env bash
# Baixa os binários externos necessários para o bundle do mptube.
# Execute uma vez antes de `pnpm tauri build`.
#
# Requer: curl, unzip (macOS já tem ambos por padrão)

set -e

DEST="src-tauri/binaries"
mkdir -p "$DEST"

# ── yt-dlp ────────────────────────────────────────────────────────────────────
YT_DLP_VERSION=$(curl -s "https://api.github.com/repos/yt-dlp/yt-dlp/releases/latest" | grep '"tag_name"' | cut -d'"' -f4)
echo "→ yt-dlp versão: $YT_DLP_VERSION"

BASE="https://github.com/yt-dlp/yt-dlp/releases/download/${YT_DLP_VERSION}"

echo "→ yt-dlp macOS aarch64 (Apple Silicon)..."
curl -L "$BASE/yt-dlp_macos" -o "$DEST/yt-dlp-aarch64-apple-darwin"
chmod +x "$DEST/yt-dlp-aarch64-apple-darwin"

echo "→ yt-dlp macOS x86_64 (Intel)..."
curl -L "$BASE/yt-dlp_macos" -o "$DEST/yt-dlp-x86_64-apple-darwin"
chmod +x "$DEST/yt-dlp-x86_64-apple-darwin"

echo "→ yt-dlp Windows x86_64..."
curl -L "$BASE/yt-dlp.exe" -o "$DEST/yt-dlp-x86_64-pc-windows-msvc.exe"

# ── ffmpeg ────────────────────────────────────────────────────────────────────
# Usa o ffmpeg universal do evermeet.cx (compilado para macOS, universal binary)
FFMPEG_VERSION=$(curl -s "https://evermeet.cx/ffmpeg/info/ffmpeg/release" | grep '"version"' | head -1 | cut -d'"' -f4)
echo ""
echo "→ ffmpeg versão: ${FFMPEG_VERSION:-latest}"

FFMPEG_URL="https://evermeet.cx/ffmpeg/getrelease/ffmpeg/zip"

echo "→ ffmpeg macOS (universal)..."
TMP_ZIP="$DEST/ffmpeg-mac.zip"
curl -L "$FFMPEG_URL" -o "$TMP_ZIP"
unzip -o "$TMP_ZIP" -d "$DEST/ffmpeg-tmp"
# Copia para os dois targets macOS
cp "$DEST/ffmpeg-tmp/ffmpeg" "$DEST/ffmpeg-aarch64-apple-darwin"
cp "$DEST/ffmpeg-tmp/ffmpeg" "$DEST/ffmpeg-x86_64-apple-darwin"
chmod +x "$DEST/ffmpeg-aarch64-apple-darwin"
chmod +x "$DEST/ffmpeg-x86_64-apple-darwin"
# Limpeza
rm -f "$TMP_ZIP"
rm -rf "$DEST/ffmpeg-tmp"

echo ""
echo "→ yt-dlp Linux x86_64..."
curl -L "$BASE/yt-dlp_linux" -o "$DEST/yt-dlp-x86_64-unknown-linux-gnu"
chmod +x "$DEST/yt-dlp-x86_64-unknown-linux-gnu"

# ffmpeg estático para Linux (John Van Sickle builds)
echo "→ ffmpeg Linux x86_64..."
TMP_TAR="$DEST/ffmpeg-linux.tar.xz"
curl -L "https://johnvansickle.com/ffmpeg/releases/ffmpeg-release-amd64-static.tar.xz" -o "$TMP_TAR"
mkdir -p "$DEST/ffmpeg-linux-tmp"
tar -xf "$TMP_TAR" -C "$DEST/ffmpeg-linux-tmp" --strip-components=1
cp "$DEST/ffmpeg-linux-tmp/ffmpeg" "$DEST/ffmpeg-x86_64-unknown-linux-gnu"
chmod +x "$DEST/ffmpeg-x86_64-unknown-linux-gnu"
rm -f "$TMP_TAR"
rm -rf "$DEST/ffmpeg-linux-tmp"

echo ""
echo "✓ Binários prontos em $DEST/"
ls -lh "$DEST/"
