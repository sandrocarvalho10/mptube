# Deploy do mptube-server na VPS

Pré-requisitos na VPS: Docker + Docker Compose, e nginx já rodando (com certbot,
se você quiser HTTPS).

## 1. Primeira vez

A imagem (frontend + servidor Rust + yt-dlp/ffmpeg) é buildada pelo CI
(`.github/workflows/web.yml`) a cada push na `main` e publicada no GHCR — a
VPS só baixa a imagem pronta, **sem compilar nada localmente** (importante em
VPS com pouca RAM: compilar o Rust ali pode não caber).

O pacote no GHCR nasce privado por padrão, mesmo com o repositório público.
Antes do primeiro `pull`, faça login na VPS uma vez com um
[Personal Access Token](https://github.com/settings/tokens) com escopo
`read:packages` (ou torne o pacote público em
`github.com/users/sandrocarvalho10/packages/container/mptube-server/settings`
e pule o login):

```bash
echo "<seu-PAT>" | docker login ghcr.io -u sandrocarvalho10 --password-stdin
```

```bash
git clone <seu-repositorio> mptube
cd mptube
cp .env.example .env
docker compose pull
docker compose up -d
```

Isso baixa a imagem já pronta e sobe o container escutando em
`127.0.0.1:8080` — não exposto direto na internet.

Confira que subiu:

```bash
curl -I http://127.0.0.1:8080/
docker compose logs -f mptube-server
```

## 2. Configurar o nginx

```bash
cp deploy/nginx.mptube.conf.example /etc/nginx/sites-available/mptube
# edite /etc/nginx/sites-available/mptube e troque SEUDOMINIO.com pelo seu domínio
ln -s /etc/nginx/sites-available/mptube /etc/nginx/sites-enabled/mptube
nginx -t && systemctl reload nginx
```

Para HTTPS (se ainda não tiver certificado para esse domínio):

```bash
certbot --nginx -d SEUDOMINIO.com
```

## 3. Atualizar depois de mudanças no código

Espere o workflow "Build web (Docker)" terminar no GitHub Actions (builda,
testa e publica a imagem), depois:

```bash
git pull
docker compose pull
docker compose up -d
```

## 4. Atualizar só o yt-dlp (quando o YouTube muda algo e passa a falhar)

O `yt-dlp` é baixado direto do GitHub na hora do build da imagem no CI. Rode o
workflow "Build web (Docker)" manualmente (aba Actions → Run workflow) pra
gerar uma imagem nova com a versão mais recente, depois `docker compose pull
&& docker compose up -d` na VPS.

## Variáveis de ambiente

Veja `.env.example` — todas têm um padrão razoável. As mais relevantes para
operar em produção:

- `MAX_CONCURRENT_DOWNLOADS` / `MAX_CONCURRENT_PER_IP`: controlam quanto de
  CPU/banda o servidor pode gastar de uma vez.
- `RATE_LIMIT_PER_MINUTE`: limite de requisições por IP nas rotas de
  formatos/downloads (proteção contra abuso, já que o site é público).
- `RETENTION_MINUTES`: por quanto tempo um arquivo baixado fica disponível
  antes de ser apagado automaticamente do disco da VPS.
- `ALLOWED_DOMAINS` / `ALLOW_ANY_DOMAIN`: por padrão só links de
  YouTube/Instagram/TikTok/Twitter-X/Facebook/Vimeo/SoundCloud são aceitos,
  pra evitar que o servidor vire um proxy de download genérico.

## Monitorando

```bash
docker compose logs -f mptube-server   # logs em tempo real
du -sh data/                            # espaço usado pelos downloads temporários
docker compose ps                       # status do container
```
