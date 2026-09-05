# mptube-server — build multi-stage: frontend (Vite) + backend (Rust/Axum) +
# imagem final com yt-dlp/ffmpeg instalados. Ver DEPLOY.md para o passo a passo de deploy.

# ── Stage 1: frontend (dist/ usado tanto pelo Tauri quanto pelo server) ──────
FROM node:22-slim AS frontend
WORKDIR /app
RUN corepack enable
COPY package.json pnpm-lock.yaml pnpm-workspace.yaml ./
RUN pnpm install --frozen-lockfile
COPY index.html tsconfig.json tsconfig.node.json vite.config.ts ./
COPY public ./public
COPY src ./src
RUN pnpm build

# ── Stage 2: backend (só compila mptube-server + mptube-core, sem GTK/webkit) ─
FROM rust:1-slim-bookworm AS backend
WORKDIR /app
RUN apt-get update && apt-get install -y --no-install-recommends \
      pkg-config libssl-dev \
    && rm -rf /var/lib/apt/lists/*
COPY Cargo.toml Cargo.lock ./
COPY crates ./crates
COPY server ./server
# `src-tauri` precisa existir para o workspace resolver, mas seu build.rs/tauri.conf.json
# (que exigem toolchain GTK) não são necessários — só compilamos `-p mptube-server`.
COPY src-tauri/Cargo.toml ./src-tauri/Cargo.toml
COPY src-tauri/src ./src-tauri/src
RUN cargo build --release -p mptube-server

# ── Stage 3: imagem final ─────────────────────────────────────────────────────
FROM debian:bookworm-slim
RUN apt-get update && apt-get install -y --no-install-recommends \
      ffmpeg curl ca-certificates python3 \
    && rm -rf /var/lib/apt/lists/* \
    && curl -fL https://github.com/yt-dlp/yt-dlp/releases/latest/download/yt-dlp_linux -o /usr/local/bin/yt-dlp \
    && chmod +x /usr/local/bin/yt-dlp

WORKDIR /app
COPY --from=backend /app/target/release/mptube-server ./mptube-server
COPY --from=frontend /app/dist ./dist

ENV FRONTEND_DIST=/app/dist \
    DOWNLOAD_DIR=/data/downloads \
    YTDLP_BIN=/usr/local/bin/yt-dlp \
    FFMPEG_BIN=/usr/bin/ffmpeg \
    PORT=8080

EXPOSE 8080
VOLUME ["/data"]

CMD ["./mptube-server"]
