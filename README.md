# mptube

Downloader de vídeo/áudio (YouTube, Instagram, TikTok, Twitter/X, Facebook,
Vimeo, SoundCloud) com yt-dlp + ffmpeg por baixo. Existe em duas versões:

- **App desktop** (Tauri + React) — Windows, macOS e Linux.
- **Versão web** (servidor Rust/Axum) — para self-host, com Docker.

## 1. App desktop (instalador pronto)

Baixe o instalador da sua plataforma na página de
[Releases](https://github.com/sandrocarvalho10/mptube/releases) do repositório:

- **Windows** → `.msi` ou `.exe` — clique e instale.
- **macOS Apple Silicon** → `.dmg` (aarch64).
- **macOS Intel** → `.dmg` (x86_64).
- **Linux** → `.deb`, `.rpm` ou `.AppImage` (x86_64).

O yt-dlp e o ffmpeg já vêm embutidos no instalador — não é preciso instalar
nada além disso.

> No macOS/Linux o instalador não é assinado; pode ser necessário liberar a
> execução manualmente (ex: macOS → "Abrir mesmo assim" nas Preferências de
> Segurança; Linux/AppImage → `chmod +x` antes de rodar).

## 2. Versão web (self-host)

Requer um servidor com Docker + Docker Compose (VPS, por exemplo). O passo a
passo completo — incluindo configuração do nginx, HTTPS e variáveis de
ambiente — está em **[DEPLOY.md](DEPLOY.md)**. Resumo:

```bash
git clone git@github.com:sandrocarvalho10/mptube.git
cd mptube
cp .env.example .env
docker compose pull
docker compose up -d
```

Isso baixa a imagem já pronta (publicada pelo CI no GHCR) e sobe o servidor
em `127.0.0.1:8080`.

## 3. Rodando a partir do código-fonte (desenvolvimento)

### Pré-requisitos

- [Node.js](https://nodejs.org/) 20+ e [pnpm](https://pnpm.io/) 9+
- [Rust](https://www.rust-lang.org/tools/install) (toolchain stable)
- Dependências de sistema do Tauri — veja o guia oficial por plataforma:
  [tauri.app/start/prerequisites](https://tauri.app/start/prerequisites/)
  (no Linux: `libwebkit2gtk-4.1-dev`, `libgtk-3-dev`,
  `libayatana-appindicator3-dev`, `librsvg2-dev`, `libssl-dev`, `libxdo-dev`,
  `libdbus-1-dev`, `build-essential`)

Instale as dependências JS:

```bash
pnpm install
```

### App desktop (Tauri)

O bundle final embute yt-dlp/ffmpeg, então antes de gerar um build baixe os
binários externos (não é necessário para `tauri dev`, que usa os do sistema
se existirem no PATH, ou os baixados aqui):

```bash
bash scripts/download-binaries.sh   # baixa yt-dlp/ffmpeg para src-tauri/binaries
```

```bash
pnpm tauri dev     # roda o app em modo desenvolvimento
pnpm tauri build   # gera o instalador (.msi/.dmg/.deb/.rpm/.AppImage)
```

### Versão web (servidor Rust)

```bash
pnpm build                          # gera dist/ (frontend)
cp .env.example .env                # ajuste se quiser
cargo run --release -p mptube-server
```

O servidor lê a configuração de variáveis de ambiente — veja `.env.example`
para a lista completa (porta, diretório de downloads, limites de taxa,
domínios permitidos, etc.).

Para rodar via Docker localmente (build local em vez de baixar a imagem do
GHCR):

```bash
docker compose build
docker compose up
```
