#!/usr/bin/env bash
# Baixa os binários externos necessários para o bundle do mptube.
# Execute uma vez antes de `pnpm tauri build`.

set -e

DEST="src-tauri/binaries"
mkdir -p "$DEST"

YT_DLP_VERSION=$(curl -s "https://api.github.com/repos/yt-dlp/yt-dlp/releases/latest" | grep '"tag_name"' | cut -d'"' -f4)
echo "→ yt-dlp versão: $YT_DLP_VERSION"

BASE="https://github.com/yt-dlp/yt-dlp/releases/download/${YT_DLP_VERSION}"

echo "→ macOS aarch64 (Apple Silicon)..."
curl -L "$BASE/yt-dlp_macos" -o "$DEST/yt-dlp-aarch64-apple-darwin"
chmod +x "$DEST/yt-dlp-aarch64-apple-darwin"

echo "→ macOS x86_64 (Intel)..."
curl -L "$BASE/yt-dlp_macos" -o "$DEST/yt-dlp-x86_64-apple-darwin"
chmod +x "$DEST/yt-dlp-x86_64-apple-darwin"

echo "→ Windows x86_64..."
curl -L "$BASE/yt-dlp.exe" -o "$DEST/yt-dlp-x86_64-pc-windows-msvc.exe"

echo ""
echo "✓ Binários prontos em $DEST/"
ls -lh "$DEST/"
