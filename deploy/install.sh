#!/usr/bin/env bash
# Instalação e atualização do ffplayout numa VPS (Debian ou Ubuntu), com build.
#
#   sudo deploy/install.sh install --panel-domain painel.exemplo.com.br \
#        --signal-domain sinal.exemplo.com.br --email voce@exemplo.com.br
#   sudo deploy/install.sh update --pull
#   deploy/install.sh status
#
# Rode de dentro do clone do repositório. O que ele faz, na ordem:
#   dependências e FFmpeg 7 -> Node 24 e Rust -> build -> usuário e pastas ->
#   configuração inicial (sem perguntas) -> relay MediaMTX -> serviço do ffplayout
#   -> nginx com HTTPS -> backup diário -> firewall (se o ufw estiver ativo).
#
# É seguro rodar de novo: o que já existe (banco, senhas, certificados) não é
# refeito. Detalhes e o que ainda precisa de teste real: deploy/README.md.

set -euo pipefail

# --- Onde as coisas ficam (podem ser trocadas por variável de ambiente) --------

REPO_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
DEPLOY_DIR="$REPO_DIR/deploy"

SVC_USER="${FFP_USER:-ffpu}"
DATA_DIR="${FFP_DATA_DIR:-/var/lib/ffplayout}"
LOG_DIR="${FFP_LOG_DIR:-/var/log/ffplayout}"
SHARE_DIR="${FFP_SHARE_DIR:-/usr/share/ffplayout}"
BIN="${FFP_BIN:-/usr/local/bin/ffplayout}"
DB_FILE="$DATA_DIR/ffplayout.db"
BACKUP_DIR="${FFP_BACKUP_DIR:-/var/backups/ffplayout}"
BACKUP_BIN_DIR="${FFP_BACKUP_BIN_DIR:-/opt/ffplayout}"
SYSTEMD_DIR="${FFP_SYSTEMD_DIR:-/etc/systemd/system}"
MEDIAMTX_DIR="${FFP_MEDIAMTX_DIR:-/opt/mediamtx}"
MEDIAMTX_ETC="${FFP_MEDIAMTX_ETC:-/etc/mediamtx}"
MEDIAMTX_VERSION="${MEDIAMTX_VERSION:-v1.18.0}"
NGINX_DIR="${FFP_NGINX_DIR:-/etc/nginx}"
ACME_ROOT="${FFP_ACME_ROOT:-/var/www/certbot}"
CRED_FILE="${FFP_CRED_FILE:-/root/ffplayout-credenciais.txt}"
NODE_DIR="/opt/node"
NODE_MAJOR=24
PANEL_PORT=8787
CHANNEL_PATH="canal1"

# --- Opções ------------------------------------------------------------------

PANEL_DOMAIN=""
SIGNAL_DOMAIN=""
EMAIL=""
DESTINATIONS=3
ADMIN_USER="admin"
ADMIN_MAIL=""
WITH_RELAY=1
WITH_BACKUP=1
DO_PULL=0
DO_RESTART=1
SKIP_BUILD=0

BUILD_USER=""
BUILD_HOME=""

# --- Mensagens ---------------------------------------------------------------

log() { printf '\n\033[1;34m==>\033[0m %s\n' "$*"; }
note() { printf '    %s\n' "$*"; }
warn() { printf '\033[1;33m[aviso]\033[0m %s\n' "$*" >&2; }
die() {
    printf '\033[1;31m[erro]\033[0m %s\n' "$*" >&2
    exit 1
}

usage() {
    cat <<'EOF'
Uso: deploy/install.sh <comando> [opções]

Comandos:
  install   instala tudo (e compila o ffplayout)
  update    compila de novo e troca o binário (use depois de baixar uma versão nova)
  status    mostra o estado dos serviços, portas, disco e último backup

Opções do install:
  --panel-domain NOME    domínio do painel (HTTPS). Precisa apontar para esta VPS.
  --signal-domain NOME   domínio do m3u8 (HTTPS). Precisa apontar para esta VPS.
  --email EMAIL          e-mail do certificado Let's Encrypt (obrigatório com os domínios)
  --destinations N       quantos destinos terão usuário e senha no relay (padrão 3)
  --admin-user NOME      usuário do administrador geral (padrão admin)
  --admin-mail EMAIL     e-mail do administrador geral
  --no-relay             não instala o relay (MediaMTX)
  --no-backup            não instala o backup diário
  --skip-build           não compila; usa o binário já compilado em target/release

Opções do update:
  --pull                 faz "git pull --ff-only" antes de compilar
  --no-restart           troca o binário mas não reinicia (reinicie num momento calmo:
                         systemctl restart ffplayout)
  --skip-build           só instala o binário já compilado

Sem --panel-domain e --signal-domain o nginx não é configurado: o painel fica só em
127.0.0.1:8787 (acesse por túnel SSH: ssh -L 8787:127.0.0.1:8787 usuario@vps).

Variáveis úteis: MEDIAMTX_VERSION (padrão v1.18.0).
EOF
}

# --- Utilidades --------------------------------------------------------------

need_root() {
    [[ $EUID -eq 0 ]] || die "rode como administrador: sudo deploy/install.sh $*"
}

random_secret() {
    # 24 letras e números: cabe em URL (RTMP/SRT) sem escape.
    local secret
    secret="$(openssl rand -base64 48 | tr -dc 'A-Za-z0-9' | head -c 24)"
    [[ ${#secret} -eq 24 ]] || die "não consegui gerar uma senha aleatória"
    printf '%s' "$secret"
}

arch_name() {
    case "$(uname -m)" in
        x86_64 | amd64) echo "amd64" ;;
        aarch64 | arm64) echo "arm64" ;;
        *) die "arquitetura $(uname -m) não suportada (só x86_64 e aarch64)" ;;
    esac
}

detect_os() {
    [[ -r /etc/os-release ]] || die "não consegui identificar o sistema (/etc/os-release)"
    # shellcheck disable=SC1091
    . /etc/os-release
    OS_ID="${ID:-}"
    OS_VERSION="${VERSION_ID:-0}"

    case "$OS_ID" in
        ubuntu | debian) ;;
        *) die "sistema $OS_ID não suportado: use Debian ou Ubuntu" ;;
    esac

    command -v systemctl >/dev/null || die "este script precisa do systemd"
    note "sistema: $OS_ID $OS_VERSION, $(uname -m)"
}

# Roda um comando como o usuário que compila, com o HOME e o PATH dele.
as_builder() {
    runuser -u "$BUILD_USER" -- env \
        HOME="$BUILD_HOME" \
        PATH="$BUILD_HOME/.cargo/bin:$NODE_DIR/bin:/usr/local/bin:/usr/bin:/bin" \
        bash -c "$1"
}

pick_build_user() {
    # Compilar como administrador seria arriscado: scripts de build de terceiros
    # rodariam com todos os poderes.
    if [[ -n "${SUDO_USER:-}" && "$SUDO_USER" != "root" ]]; then
        BUILD_USER="$SUDO_USER"
    else
        BUILD_USER="ffbuild"

        if ! id "$BUILD_USER" >/dev/null 2>&1; then
            useradd --system --create-home --home-dir /var/lib/ffbuild \
                --shell /usr/sbin/nologin "$BUILD_USER"
        fi
    fi

    BUILD_HOME="$(getent passwd "$BUILD_USER" | cut -d: -f6)"
    [[ -d "$BUILD_HOME" ]] || die "o usuário $BUILD_USER não tem pasta pessoal"

    # O clone precisa ser dele para compilar.
    if [[ "$(stat -c %U "$REPO_DIR")" != "$BUILD_USER" ]]; then
        note "passando o clone ($REPO_DIR) para o usuário $BUILD_USER"
        chown -R "$BUILD_USER": "$REPO_DIR"
    fi
}

# --- Dependências ------------------------------------------------------------

apt_install() {
    DEBIAN_FRONTEND=noninteractive apt-get install -y --no-install-recommends "$@"
}

ffmpeg_ok() {
    pkg-config --exists libavformat 2>/dev/null &&
        [[ "$(pkg-config --modversion libavformat | cut -d. -f1)" -ge 61 ]] &&
        command -v ffmpeg >/dev/null
}

install_packages() {
    log "Dependências do sistema"
    apt-get update
    apt_install ca-certificates curl git xz-utils openssl python3 pkg-config \
        build-essential clang libclang-dev software-properties-common
}

install_ffmpeg() {
    log "FFmpeg 7 ou mais novo (com os arquivos de desenvolvimento)"

    local packages=(ffmpeg libavcodec-dev libavdevice-dev libavfilter-dev
        libavformat-dev libavutil-dev libswresample-dev libswscale-dev)

    apt_install "${packages[@]}" || true

    if ! ffmpeg_ok && [[ "$OS_ID" == "ubuntu" ]]; then
        note "o FFmpeg desta versão do Ubuntu é antigo; usando o PPA ppa:ubuntuhandbook1/ffmpeg7"
        add-apt-repository -y ppa:ubuntuhandbook1/ffmpeg7
        apt-get update
        apt_install "${packages[@]}"
    fi

    ffmpeg_ok || die "não consegui um FFmpeg 7 ou mais novo (libavformat 61+). No Debian use a versão 13 ou mais nova; no Ubuntu, a 22.04 ou mais nova."
    note "$(ffmpeg -version | head -n 1)"
}

install_node() {
    log "Node $NODE_MAJOR"

    if command -v node >/dev/null && [[ "$(node -p 'process.versions.node.split(".")[0]')" -ge $NODE_MAJOR ]]; then
        note "já existe: $(node -v)"
        return
    fi

    local arch base sums file
    arch="$(arch_name)"
    [[ "$arch" == "amd64" ]] && arch="x64"
    base="https://nodejs.org/dist/latest-v$NODE_MAJOR.x"
    sums="$(curl -fsSL "$base/SHASUMS256.txt")"
    file="$(awk -v pat="linux-$arch.tar.xz" '$2 ~ pat {print $2; exit}' <<<"$sums")"
    [[ -n "$file" ]] || die "não achei o Node $NODE_MAJOR para $arch em $base"

    local tmp
    tmp="$(mktemp -d)"
    curl -fsSL -o "$tmp/$file" "$base/$file"
    (cd "$tmp" && grep " $file\$" <<<"$sums" | sha256sum -c -) || die "o download do Node não confere com o checksum"

    rm -rf "$NODE_DIR"
    mkdir -p "$NODE_DIR"
    tar -xJf "$tmp/$file" -C "$NODE_DIR" --strip-components=1
    rm -rf "$tmp"
    ln -sf "$NODE_DIR/bin/node" /usr/local/bin/node
    ln -sf "$NODE_DIR/bin/npm" /usr/local/bin/npm
    ln -sf "$NODE_DIR/bin/npx" /usr/local/bin/npx
    note "instalado: $(node -v)"
}

install_rust() {
    log "Rust (para o usuário $BUILD_USER)"

    if [[ -x "$BUILD_HOME/.cargo/bin/cargo" ]]; then
        note "já existe: $(as_builder 'cargo --version')"
        return
    fi

    as_builder "curl --proto '=https' --tlsv1.2 -fsSL https://sh.rustup.rs | sh -s -- -y --profile minimal --no-modify-path"
    note "instalado: $(as_builder 'cargo --version')"
}

# --- Build -------------------------------------------------------------------

build_ffplayout() {
    if [[ $SKIP_BUILD -eq 1 ]]; then
        [[ -x "$REPO_DIR/target/release/ffplayout" ]] || die "--skip-build, mas não há $REPO_DIR/target/release/ffplayout"
        log "Build pulado: usando target/release/ffplayout"
        return
    fi

    log "Compilando o painel (frontend)"
    as_builder "cd '$REPO_DIR' && npm ci && npm run build-only"

    log "Compilando o ffplayout (a primeira vez leva vários minutos)"
    # Sem o player de desktop: numa VPS não há tela nem placa de som.
    as_builder "cd '$REPO_DIR' && cargo build --release --package ffplayout --no-default-features --features embed_frontend"
}

install_binary() {
    log "Instalando o binário em $BIN"
    install -m 0755 "$REPO_DIR/target/release/ffplayout" "$BIN.new"
    mv -f "$BIN.new" "$BIN"
    note "$("$BIN" --version 2>/dev/null || echo ffplayout)"
}

# --- Usuário, pastas e configuração inicial -----------------------------------

create_service_user() {
    log "Usuário do serviço e pastas"

    if ! id "$SVC_USER" >/dev/null 2>&1; then
        useradd --system --home-dir "$DATA_DIR" --create-home \
            --shell /usr/sbin/nologin "$SVC_USER"
    fi

    mkdir -p "$DATA_DIR"/{playlists,storage,public,recordings} "$LOG_DIR" "$SHARE_DIR"
    cp -f "$REPO_DIR/assets/dummy.vtt" "$REPO_DIR/assets/logo.png" "$SHARE_DIR/"
    chown -R "$SVC_USER": "$DATA_DIR" "$LOG_DIR"
    chmod 750 "$DATA_DIR"
    note "dados em $DATA_DIR, logs em $LOG_DIR"
}

save_credential() {
    # Anexa uma linha ao arquivo de credenciais (só o administrador lê). O umask
    # vale só dentro dos parênteses, para não afetar o resto da instalação.
    (umask 077 && printf '%s\n' "$*" >>"$CRED_FILE")
}

initial_setup() {
    log "Configuração inicial (administrador e pastas)"

    if [[ -f "$DB_FILE" ]]; then
        note "o banco já existe ($DB_FILE): configuração inicial mantida"
        return
    fi

    local password mail
    password="$(random_secret)"
    mail="${ADMIN_MAIL:-${EMAIL:-admin@localhost}}"

    # O e-mail do sistema (SMTP) fica com valores de exemplo: ajuste depois em
    # Ajustes > Sistema. A senha do administrador aparece na lista de processos
    # só durante este comando, numa VPS que ainda não tem outros usuários.
    runuser -u "$SVC_USER" -- "$BIN" \
        --db "$DB_FILE" --init \
        --username "$ADMIN_USER" --mail "$mail" --password "$password" --two-factor false \
        --storage "$DATA_DIR/storage" --playlists "$DATA_DIR/playlists" \
        --logs "$LOG_DIR" --public "$DATA_DIR/public" \
        --smtp-server "mail.example.org" --smtp-user "ffplayout@example.org" \
        --smtp-password "" --smtp-starttls false --smtp-port 465 >/dev/null

    [[ -f "$DB_FILE" ]] || die "a configuração inicial não criou o banco em $DB_FILE"

    save_credential "# ffplayout: credenciais geradas em $(date '+%Y-%m-%d %H:%M')"
    save_credential "painel_usuario=$ADMIN_USER"
    save_credential "painel_senha=$password"
    note "administrador criado: $ADMIN_USER (a senha está em $CRED_FILE)"
}

# --- Relay (MediaMTX) --------------------------------------------------------

# O mediamtx.yml do repositório, com um usuário e uma senha para cada destino.
render_mediamtx_config() {
    local count="$1" passwords_file="$2" block
    block="$(mktemp)"

    {
        echo "  # Os destinos só leem, com usuário e senha. Um usuário para cada destino:"
        echo "  # é assim que o Painel mostra qual deles caiu."
        local n
        for ((n = 1; n <= count; n++)); do
            printf '  - user: destino%d\n    pass: %s\n    ips: []\n    permissions:\n      - action: read\n        path: %s\n' \
                "$n" "$(sed -n "${n}p" "$passwords_file")" "$CHANNEL_PATH"
        done
    } >"$block"

    awk -v block="$block" '
        /^  # Os destinos só leem/ {
            skip = 1
            while ((getline line < block) > 0) print line
            next
        }
        skip && /^paths:/ { skip = 0; print "" }
        !skip { print }
    ' "$DEPLOY_DIR/relay/mediamtx.yml"

    rm -f "$block"
}

download_mediamtx() {
    local arch base file tmp
    arch="$(arch_name)"
    file="mediamtx_${MEDIAMTX_VERSION}_linux_${arch}.tar.gz"
    base="https://github.com/bluenviron/mediamtx/releases/download/$MEDIAMTX_VERSION"
    tmp="$(mktemp -d)"

    curl -fsSL -o "$tmp/$file" "$base/$file"
    curl -fsSL -o "$tmp/checksums.sha256" "$base/checksums.sha256"
    (cd "$tmp" && grep " \*\?$file\$" checksums.sha256 | sha256sum -c -) || die "o download do MediaMTX não confere com o checksum"

    mkdir -p "$MEDIAMTX_DIR"
    tar -xzf "$tmp/$file" -C "$tmp" mediamtx
    install -m 0755 "$tmp/mediamtx" "$MEDIAMTX_DIR/mediamtx.new"
    mv -f "$MEDIAMTX_DIR/mediamtx.new" "$MEDIAMTX_DIR/mediamtx"
    rm -rf "$tmp"
}

public_host() {
    if [[ -n "$SIGNAL_DOMAIN" ]]; then
        echo "$SIGNAL_DOMAIN"
    else
        hostname -I 2>/dev/null | awk '{print $1}'
    fi
}

install_relay() {
    log "Relay MediaMTX $MEDIAMTX_VERSION"

    if ! id mediamtx >/dev/null 2>&1; then
        useradd --system --no-create-home --shell /usr/sbin/nologin mediamtx
    fi

    download_mediamtx
    mkdir -p "$MEDIAMTX_ETC"

    if [[ -f "$MEDIAMTX_ETC/mediamtx.yml" ]]; then
        note "mantendo a configuração e as senhas de $MEDIAMTX_ETC/mediamtx.yml"
    else
        local passwords host n
        passwords="$(mktemp)"

        for ((n = 1; n <= DESTINATIONS; n++)); do
            random_secret >>"$passwords"
            echo >>"$passwords"
        done

        render_mediamtx_config "$DESTINATIONS" "$passwords" >"$MEDIAMTX_ETC/mediamtx.yml"
        chown root:mediamtx "$MEDIAMTX_ETC/mediamtx.yml"
        chmod 640 "$MEDIAMTX_ETC/mediamtx.yml"

        host="$(public_host)"
        save_credential ""
        save_credential "# Links para os destinos (um usuário e uma senha para cada)"

        for ((n = 1; n <= DESTINATIONS; n++)); do
            local pass
            pass="$(sed -n "${n}p" "$passwords")"
            save_credential "destino$n RTMP: rtmp://$host:1935/$CHANNEL_PATH?user=destino$n&pass=$pass"
            save_credential "destino$n SRT:  srt://$host:8890?streamid=read:$CHANNEL_PATH:destino$n:$pass"
        done

        rm -f "$passwords"
        note "$DESTINATIONS destinos criados; os links estão em $CRED_FILE"
    fi

    install -m 0644 "$DEPLOY_DIR/systemd/mediamtx.service" "$SYSTEMD_DIR/mediamtx.service"
}

# --- Serviço do ffplayout -----------------------------------------------------

install_service() {
    log "Serviço do ffplayout"
    install -m 0644 "$DEPLOY_DIR/systemd/ffplayout.service" "$SYSTEMD_DIR/ffplayout.service"

    # O unit do repositório usa o usuário ffpu; se o usuário for outro, ajusta.
    if [[ "$SVC_USER" != "ffpu" ]]; then
        sed -i "s/^User=ffpu\$/User=$SVC_USER/" "$SYSTEMD_DIR/ffplayout.service"
    fi

    if [[ $WITH_RELAY -eq 0 ]]; then
        sed -i '/mediamtx.service/d' "$SYSTEMD_DIR/ffplayout.service"
    fi

    systemctl daemon-reload
}

# --- nginx e HTTPS -----------------------------------------------------------

render_nginx_config() {
    local panel="$1" signal="$2"

    sed -e "s/painel\.exemplo\.com\.br/$panel/g" -e "s/sinal\.exemplo\.com\.br/$signal/g" \
        "$DEPLOY_DIR/nginx/ffplayout.conf"
}

# Só os blocos da porta 80, para o Let's Encrypt validar o domínio antes de
# existir certificado.
render_nginx_bootstrap() {
    local panel="$1" signal="$2"

    cat <<EOF
server {
    listen 80;
    server_name $panel $signal;

    location /.well-known/acme-challenge/ {
        root $ACME_ROOT;
    }

    location / {
        return 404;
    }
}
EOF
}

issue_certificate() {
    local domain="$1"

    if [[ -f "/etc/letsencrypt/live/$domain/fullchain.pem" ]]; then
        note "certificado de $domain já existe"
        return 0
    fi

    certbot certonly --webroot -w "$ACME_ROOT" -d "$domain" --cert-name "$domain" \
        --non-interactive --agree-tos --no-eff-email -m "$EMAIL"
}

install_nginx() {
    log "nginx e HTTPS ($PANEL_DOMAIN e $SIGNAL_DOMAIN)"

    apt_install nginx certbot
    mkdir -p "$ACME_ROOT"

    local site="$NGINX_DIR/sites-available/ffplayout"
    local enabled="$NGINX_DIR/sites-enabled/ffplayout"

    # 1) só a porta 80, para provar ao Let's Encrypt que o domínio é desta máquina.
    render_nginx_bootstrap "$PANEL_DOMAIN" "$SIGNAL_DOMAIN" >"$site"
    ln -sf "$site" "$enabled"
    nginx -t
    systemctl enable --now nginx
    systemctl reload nginx

    if ! { issue_certificate "$PANEL_DOMAIN" && issue_certificate "$SIGNAL_DOMAIN"; }; then
        warn "não consegui o certificado. Confira se os dois domínios apontam para esta VPS"
        warn "(DNS tipo A) e se a porta 80 está aberta; depois rode o install de novo."
        warn "O painel continua em http://127.0.0.1:$PANEL_PORT (túnel SSH)."
        return 0
    fi

    # 2) agora a configuração completa, com HTTPS.
    render_nginx_config "$PANEL_DOMAIN" "$SIGNAL_DOMAIN" >"$site"

    # Senha do m3u8 (usuário "destino"), criada uma vez.
    if [[ ! -f "$NGINX_DIR/hls.htpasswd" ]]; then
        local pass
        pass="$(random_secret)"
        printf 'destino:%s\n' "$(openssl passwd -apr1 "$pass")" >"$NGINX_DIR/hls.htpasswd"
        chown root:www-data "$NGINX_DIR/hls.htpasswd"
        chmod 640 "$NGINX_DIR/hls.htpasswd"
        save_credential ""
        save_credential "m3u8: https://$SIGNAL_DOMAIN/$CHANNEL_PATH/index.m3u8 (usuário destino, senha $pass)"
    fi

    nginx -t
    systemctl reload nginx

    # Renovação automática: recarrega o nginx quando o certificado muda.
    mkdir -p /etc/letsencrypt/renewal-hooks/deploy
    printf '#!/bin/sh\nsystemctl reload nginx\n' >/etc/letsencrypt/renewal-hooks/deploy/reload-nginx.sh
    chmod 755 /etc/letsencrypt/renewal-hooks/deploy/reload-nginx.sh
    note "HTTPS pronto: https://$PANEL_DOMAIN"
}

# --- Backup ------------------------------------------------------------------

install_backup() {
    log "Backup diário"
    mkdir -p "$BACKUP_BIN_DIR" "$BACKUP_DIR"
    install -m 0755 "$DEPLOY_DIR/backup/ffplayout-backup.py" "$BACKUP_BIN_DIR/ffplayout-backup.py"
    chown "$SVC_USER": "$BACKUP_DIR"

    install -m 0644 "$DEPLOY_DIR/systemd/ffplayout-backup.service" "$SYSTEMD_DIR/ffplayout-backup.service"
    install -m 0644 "$DEPLOY_DIR/systemd/ffplayout-backup.timer" "$SYSTEMD_DIR/ffplayout-backup.timer"

    if [[ "$SVC_USER" != "ffpu" ]]; then
        sed -i "s/^User=ffpu\$/User=$SVC_USER/" "$SYSTEMD_DIR/ffplayout-backup.service"
    fi

    systemctl daemon-reload
    note "backups em $BACKUP_DIR (copie-os para fora da VPS)"
}

# --- Firewall e partida ------------------------------------------------------

configure_firewall() {
    local ports=(22/tcp 80/tcp 443/tcp)

    [[ $WITH_RELAY -eq 1 ]] && ports+=(1935/tcp 8890/udp)

    if command -v ufw >/dev/null && ufw status 2>/dev/null | grep -q "Status: active"; then
        log "Firewall (ufw)"
        local port
        for port in "${ports[@]}"; do
            ufw allow "$port" >/dev/null
        done
        note "liberado: ${ports[*]}"
    else
        log "Firewall"
        note "o ufw não está ativo, então nada foi mexido. Abra só: ${ports[*]}"
        note "(a porta $PANEL_PORT do painel NÃO deve ficar aberta: o nginx faz o HTTPS na frente)"
    fi
}

wait_for_panel() {
    for _ in $(seq 1 30); do
        if curl -fsS -o /dev/null "http://127.0.0.1:$PANEL_PORT/api/setup"; then
            return 0
        fi
        sleep 1
    done

    return 1
}

start_services() {
    log "Ligando os serviços"
    systemctl daemon-reload

    [[ $WITH_RELAY -eq 1 ]] && systemctl enable --now mediamtx
    systemctl enable ffplayout
    systemctl restart ffplayout
    [[ $WITH_BACKUP -eq 1 ]] && systemctl enable --now ffplayout-backup.timer

    if wait_for_panel; then
        note "o ffplayout respondeu em http://127.0.0.1:$PANEL_PORT"
    else
        warn "o ffplayout não respondeu em 30 s. Veja: journalctl -u ffplayout -n 50"
    fi
}

summary() {
    log "Pronto"

    if [[ -n "$PANEL_DOMAIN" && -d "/etc/letsencrypt/live/$PANEL_DOMAIN" ]]; then
        note "Painel:      https://$PANEL_DOMAIN"
    else
        note "Painel:      http://127.0.0.1:$PANEL_PORT  (túnel: ssh -L $PANEL_PORT:127.0.0.1:$PANEL_PORT usuario@vps)"
    fi

    note "Credenciais: $CRED_FILE  (sudo cat $CRED_FILE)"
    note ""
    note "Próximos passos:"
    note "  1. Entre no painel com o usuário e a senha do arquivo acima e troque a senha."
    note "  2. Ajustes > Transmissão: saída Stream, rtmp://127.0.0.1:1935/$CHANNEL_PATH, 1080p 30 fps, 3000 kbps."
    note "  3. Painel > Destinos > Configurar: API http://127.0.0.1:9997, canal $CHANNEL_PATH, e um destino por usuário."
    note "  4. Ajustes > Telegram, se for usar o bot."
    note "  5. Entregue os links dos destinos (no arquivo de credenciais)."
    note ""
    note "Depois, para atualizar: sudo deploy/install.sh update --pull"
}

# --- Comandos ----------------------------------------------------------------

cmd_install() {
    need_root install
    detect_os

    if [[ -n "$PANEL_DOMAIN$SIGNAL_DOMAIN" ]]; then
        [[ -n "$PANEL_DOMAIN" && -n "$SIGNAL_DOMAIN" ]] || die "informe os dois domínios: --panel-domain e --signal-domain"
        [[ "$PANEL_DOMAIN" =~ ^[A-Za-z0-9.-]+$ && "$SIGNAL_DOMAIN" =~ ^[A-Za-z0-9.-]+$ ]] || die "domínio inválido: use só letras, números, ponto e hífen"
        [[ -n "$EMAIL" ]] || die "informe --email (o Let's Encrypt avisa por ele quando o certificado estiver para vencer)"
    fi

    [[ "$DESTINATIONS" =~ ^[0-9]+$ && $DESTINATIONS -ge 1 && $DESTINATIONS -le 50 ]] || die "--destinations deve ser de 1 a 50"

    pick_build_user
    install_packages
    install_ffmpeg
    install_node
    install_rust
    build_ffplayout
    create_service_user
    install_binary
    initial_setup
    [[ $WITH_RELAY -eq 1 ]] && install_relay
    install_service
    [[ $WITH_BACKUP -eq 1 ]] && install_backup
    [[ -n "$PANEL_DOMAIN" ]] && install_nginx
    configure_firewall
    start_services
    summary
}

cmd_update() {
    need_root update
    detect_os
    [[ -x "$BIN" ]] || die "o ffplayout não está instalado em $BIN: rode primeiro o install"

    pick_build_user

    if [[ $DO_PULL -eq 1 ]]; then
        log "Baixando a versão nova"
        as_builder "cd '$REPO_DIR' && git pull --ff-only"
    fi

    build_ffplayout

    # Backup antes de trocar o binário: uma versão nova pode mudar o banco.
    if systemctl cat ffplayout-backup.service >/dev/null 2>&1; then
        log "Backup antes de atualizar"
        systemctl start ffplayout-backup.service || warn "o backup falhou; confira antes de seguir"
    fi

    install_binary

    if [[ $DO_RESTART -eq 1 ]]; then
        log "Reiniciando o ffplayout (a saída fica fora por poucos segundos)"
        systemctl restart ffplayout

        if wait_for_panel; then
            note "no ar com a versão nova"
        else
            warn "o ffplayout não respondeu em 30 s. Veja: journalctl -u ffplayout -n 50"
        fi
    else
        note "binário trocado; a versão nova só vale depois de: sudo systemctl restart ffplayout"
    fi
}

cmd_status() {
    local unit
    log "Serviços"

    for unit in ffplayout mediamtx nginx ffplayout-backup.timer; do
        if systemctl cat "$unit" >/dev/null 2>&1; then
            printf '    %-26s %s\n' "$unit" "$(systemctl is-active "$unit" || true)"
        fi
    done

    log "Versão"
    note "$("$BIN" --version 2>/dev/null || echo 'ffplayout não instalado')"
    command -v ffmpeg >/dev/null && note "$(ffmpeg -version | head -n 1)"

    log "Portas em escuta"
    ss -ltnuH 2>/dev/null | awk '{print $5}' | grep -E ":(80|443|1935|8890|8787|8888|9997)$" | sort -u | sed 's/^/    /' || true

    log "Disco"
    df -h "$DATA_DIR" 2>/dev/null | tail -n 1 | sed 's/^/    /' || true

    log "Último backup"
    # shellcheck disable=SC2012
    ls -1t "$BACKUP_DIR"/ffplayout-*.tar.gz 2>/dev/null | head -n 1 | sed 's/^/    /' || true
}

parse_options() {
    while [[ $# -gt 0 ]]; do
        case "$1" in
            --panel-domain) PANEL_DOMAIN="${2:?falta o valor de $1}"; shift 2 ;;
            --signal-domain) SIGNAL_DOMAIN="${2:?falta o valor de $1}"; shift 2 ;;
            --email) EMAIL="${2:?falta o valor de $1}"; shift 2 ;;
            --destinations) DESTINATIONS="${2:?falta o valor de $1}"; shift 2 ;;
            --admin-user) ADMIN_USER="${2:?falta o valor de $1}"; shift 2 ;;
            --admin-mail) ADMIN_MAIL="${2:?falta o valor de $1}"; shift 2 ;;
            --no-relay) WITH_RELAY=0; shift ;;
            --no-backup) WITH_BACKUP=0; shift ;;
            --pull) DO_PULL=1; shift ;;
            --no-restart) DO_RESTART=0; shift ;;
            --skip-build) SKIP_BUILD=1; shift ;;
            -h | --help) usage; exit 0 ;;
            *) die "opção desconhecida: $1 (veja --help)" ;;
        esac
    done
}

main() {
    local command="${1:-help}"

    [[ $# -gt 0 ]] && shift

    case "$command" in
        install) parse_options "$@"; cmd_install ;;
        update) parse_options "$@"; cmd_update ;;
        status) parse_options "$@"; cmd_status ;;
        help | -h | --help) usage ;;
        *) usage; die "comando desconhecido: $command" ;;
    esac
}

# Permite carregar o arquivo só para usar as funções (testes).
if [[ "${BASH_SOURCE[0]}" == "$0" ]]; then
    main "$@"
fi
