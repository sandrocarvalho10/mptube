# Deploy do mptube-server na VPS

Pré-requisitos na VPS: Docker + Docker Compose, e nginx já rodando (com certbot,
se você quiser HTTPS).

## 0. VPS pequena (ex: Magalu Cloud, 1GB RAM / 10GB disco)

A imagem é publicada só para **x86_64 (amd64)** — escolha uma VM x86. Antes de
tudo, na VPS:

```bash
# Swap de 2GB: sem isso um merge de vídeo no ffmpeg pode acabar com a RAM e o
# kernel mata processos (inclusive o sshd).
sudo fallocate -l 2G /swapfile && sudo chmod 600 /swapfile
sudo mkswap /swapfile && sudo swapon /swapfile
echo '/swapfile none swap sw 0 0' | sudo tee -a /etc/fstab
echo 'vm.swappiness=10' | sudo tee /etc/sysctl.d/99-swap.conf && sudo sysctl --system

# Docker + plugin compose (script oficial)
curl -fsSL https://get.docker.com | sudo sh
sudo usermod -aG docker $USER   # saia e entre de novo na sessão SSH

# Limita o journald para não comer o disco
sudo mkdir -p /etc/systemd/journald.conf.d
printf '[Journal]\nSystemMaxUse=200M\n' | sudo tee /etc/systemd/journald.conf.d/size.conf
sudo systemctl restart systemd-journald

sudo apt-get install -y nginx certbot python3-certbot-nginx
```

Orçamento aproximado de disco: SO ~2,5GB, imagem do mptube ~700MB, swap 2GB,
logs ~0,3GB. Sobram ~4GB para downloads temporários, por isso o
`.env.example` já vem com `MIN_FREE_DISK_MB=2000`, `MAX_CONCURRENT_DOWNLOADS=2`
e `RETENTION_MINUTES=30`. O `docker-compose.yml` limita o container a 700MB de
RAM e roda os logs em no máximo 30MB.

Libere as portas 80/443 (e 22) no security group da VM no painel da Magalu.

> **YouTube a partir de IP de datacenter:** é comum o YouTube responder "Sign in
> to confirm you're not a bot" para IPs de cloud. Os outros sites costumam
> funcionar normalmente. Se acontecer, não há correção do lado do mptube: a
> saída é usar cookies de uma conta (o servidor ainda não expõe isso) ou um
> proxy residencial.

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
docker image prune -f   # apaga a imagem antiga (~700MB cada) — essencial em disco de 10GB
```

## 4. Atualizar só o yt-dlp (quando o YouTube muda algo e passa a falhar)

O CI rebuilda a imagem toda segunda-feira com o `yt-dlp` mais recente (e
também a cada push na `main`). Para forçar na hora, rode o workflow "Build web
(Docker)" manualmente (aba Actions → Run workflow). Depois, na VPS:
`docker compose pull && docker compose up -d && docker image prune -f`.

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
