#!/usr/bin/env bash
# Baixa os vídeos e playlists do canal da LFTV no YouTube direto para a VPS (nada
# passa pelo seu computador). Usa o yt-dlp: retoma de onde parou, lembra o que já
# baixou (rodar de novo traz só o novo) e baixa em 1080p, MP4.
#
# Só baixa vídeos do canal permitido (ALLOWED_CHANNEL_ID, o da LFTV Oficial). Qualquer
# vídeo de outro canal é recusado, mesmo que esteja numa playlist configurada.
#
#   sudo deploy/youtube-import.sh init           cria o arquivo de configuração
#   sudo deploy/youtube-import.sh test           confere o acesso e lista as playlists
#   sudo deploy/youtube-import.sh size           estimativa de tamanho x espaço livre
#   sudo deploy/youtube-import.sh dry-run        mostra o que seria baixado, sem baixar
#   sudo deploy/youtube-import.sh start          baixa em segundo plano (sobrevive ao SSH)
#   sudo deploy/youtube-import.sh logs           acompanha o andamento
#   sudo deploy/youtube-import.sh status         estado do download e do disco
#   sudo deploy/youtube-import.sh verify         lista o que do canal ainda não está aqui
#   sudo deploy/youtube-import.sh update         instala/atualiza o yt-dlp (o YouTube muda sempre)
#   sudo deploy/youtube-import.sh install-timer  repete todo dia de madrugada (vídeos novos)
#   sudo deploy/youtube-import.sh run            baixa agora, nesta tela (o "start" usa este)
#
# Configuração em /etc/ffplayout/youtube-import.conf. Os arquivos baixados pertencem
# ao usuário do ffplayout.

set -euo pipefail

SCRIPT="$(readlink -f "${BASH_SOURCE[0]}")"
CONF="${FFP_YT_CONF:-/etc/ffplayout/youtube-import.conf}"
LOG="${FFP_YT_LOG:-/var/log/ffplayout/youtube-import.log}"
SVC_USER="${FFP_USER:-ffpu}"
SYSTEMD_DIR="${FFP_SYSTEMD_DIR:-/etc/systemd/system}"
UNIT="ffplayout-youtube-import"
LOCK="${FFP_YT_LOCK:-/run/ffplayout-youtube-import.lock}"
YTDLP_BIN="${FFP_YTDLP_BIN:-/usr/local/bin/yt-dlp}"
YTDLP_URL="https://github.com/yt-dlp/yt-dlp/releases/latest/download/yt-dlp_linux"

# O canal da LFTV Oficial (@LFTVOficial). Só vídeos dele são baixados.
LFTV_CHANNEL_ID="UC9jfIeNHlejCauwxqZKOz-w"

log() { printf '\n\033[1;34m==>\033[0m %s\n' "$*"; }
note() { printf '    %s\n' "$*"; }
die() {
    printf '\033[1;31m[erro]\033[0m %s\n' "$*" >&2
    exit 1
}

usage() { sed -n '2,24p' "$SCRIPT" | sed 's/^# \{0,1\}//'; }

need_root() {
    [[ $EUID -eq 0 ]] || die "rode como administrador: sudo $0 $*"
}

# --- yt-dlp ------------------------------------------------------------------

install_ytdlp() {
    log "Instalando o yt-dlp (programa oficial, do GitHub)"
    curl -fsSL "$YTDLP_URL" -o "$YTDLP_BIN.new"
    chmod 755 "$YTDLP_BIN.new"
    mv "$YTDLP_BIN.new" "$YTDLP_BIN"
    note "versão: $("$YTDLP_BIN" --version)"
}

ensure_ytdlp() {
    command -v yt-dlp >/dev/null && return 0
    [[ $EUID -eq 0 ]] || die "o yt-dlp não está instalado: sudo $0 update"
    install_ytdlp
}

# O YouTube exige um interpretador de JavaScript para liberar todos os formatos.
js_runtime() {
    if command -v deno >/dev/null; then
        echo "deno"
    elif command -v node >/dev/null; then
        echo "node"
    fi
}

# yt-dlp como o usuário do ffplayout (os arquivos nascem com o dono certo).
ytdlp() {
    local runtime
    local args=(--no-warnings)

    runtime="$(js_runtime)"
    [[ -n "$runtime" ]] && args+=(--js-runtimes "$runtime")
    [[ -n "${COOKIES_FILE:-}" ]] && args+=(--cookies "$COOKIES_FILE")

    if [[ $EUID -eq 0 && "$SVC_USER" != "root" ]]; then
        runuser -u "$SVC_USER" -- yt-dlp "${args[@]}" "$@"
    else
        yt-dlp "${args[@]}" "$@"
    fi
}

# --- Configuração ------------------------------------------------------------

write_template() {
    mkdir -p "$(dirname "$CONF")"

    if [[ -e "$CONF" ]]; then
        die "$CONF já existe; edite-o (nano $CONF)"
    fi

    cat >"$CONF" <<CONFIG
# Configuração da importação do YouTube (ver deploy/youtube-import.sh).

# --- O que baixar ---
# Um endereço por linha. A página "playlists" do canal baixa TODAS as playlists
# dele; também pode ser uma playlist ou um vídeo só.
SOURCES=(
    "https://www.youtube.com/@LFTVOficial/playlists"
)
# Só vídeos deste canal são baixados (o da LFTV Oficial). Não mude, a menos que
# os vídeos sejam seus.
ALLOWED_CHANNEL_ID=$LFTV_CHANNEL_ID

# --- Para onde ---
# Dentro da pasta de mídia do ffplayout; cada playlist vira uma subpasta.
LOCAL_DIR=/var/lib/ffplayout/storage/youtube

# --- Como baixar ---
# Altura máxima do vídeo (1080 = Full HD).
MAX_HEIGHT=1080
# Limite de velocidade em megabits por segundo (0 = sem limite). Use um limite
# enquanto o canal estiver no ar, para não disputar banda com o sinal.
LIMIT_MBIT=0
# Quantos pedaços do mesmo vídeo ao mesmo tempo.
CONCURRENT_FRAGMENTS=4
# Pausa entre um vídeo e outro, em segundos (ajuda a não ser bloqueado).
SLEEP_SECONDS=3
# Só baixa vídeos publicados depois desta data (AAAAMMDD). Vazio = todos.
DATE_AFTER=
# Se o YouTube bloquear o IP da VPS ("confirme que você não é um robô"), exporte
# os cookies de uma conta logada (extensão "Get cookies.txt LOCALLY") e aponte aqui:
# COOKIES_FILE=/etc/ffplayout/youtube-cookies.txt
CONFIG
    chmod 600 "$CONF"

    note "modelo criado em $CONF"
}

load_conf() {
    [[ -r "$CONF" ]] || die "não achei $CONF; crie com: sudo $0 init"

    # shellcheck disable=SC1090
    . "$CONF"

    LOCAL_DIR="${LOCAL_DIR:-/var/lib/ffplayout/storage/youtube}"
    MAX_HEIGHT="${MAX_HEIGHT:-1080}"
    LIMIT_MBIT="${LIMIT_MBIT:-0}"
    CONCURRENT_FRAGMENTS="${CONCURRENT_FRAGMENTS:-4}"
    SLEEP_SECONDS="${SLEEP_SECONDS:-3}"
    DATE_AFTER="${DATE_AFTER:-}"
    ALLOWED_CHANNEL_ID="${ALLOWED_CHANNEL_ID:-$LFTV_CHANNEL_ID}"

    [[ "${#SOURCES[@]}" -gt 0 ]] || die "SOURCES está vazio em $CONF"
    [[ "$MAX_HEIGHT" =~ ^[0-9]+$ && "$LIMIT_MBIT" =~ ^[0-9]+$ && "$CONCURRENT_FRAGMENTS" =~ ^[0-9]+$ &&
        "$SLEEP_SECONDS" =~ ^[0-9]+$ ]] ||
        die "MAX_HEIGHT, LIMIT_MBIT, CONCURRENT_FRAGMENTS e SLEEP_SECONDS devem ser números inteiros"
    [[ -z "$DATE_AFTER" || "$DATE_AFTER" =~ ^[0-9]{8}$ ]] || die "DATE_AFTER deve ser AAAAMMDD"
    [[ "$ALLOWED_CHANNEL_ID" =~ ^UC[A-Za-z0-9_-]{22}$ ]] || die "ALLOWED_CHANNEL_ID inválido em $CONF"
}

prepare_local_dir() {
    mkdir -p "$LOCAL_DIR"

    if [[ $EUID -eq 0 && "$SVC_USER" != "root" ]]; then
        chown "$SVC_USER": "$LOCAL_DIR"
    fi

    [[ -w "$LOCAL_DIR" || $EUID -eq 0 ]] || die "sem permissão para escrever em $LOCAL_DIR"
}

# --- Lista de playlists e verificação do canal --------------------------------

# Os endereços de cada fonte. Uma página "playlists" de canal vira uma linha por
# playlist; playlist ou vídeo avulso passam como estão.
expand_sources() {
    local source

    for source in "${SOURCES[@]}"; do
        if [[ "$source" == */playlists || "$source" == */playlists/ ]]; then
            ytdlp --flat-playlist --print "%(url)s" "$source"
        else
            echo "$source"
        fi
    done
}

# "título|canal" de uma playlist ou de um vídeo avulso. A lista rápida (flat) não
# traz o dono de um vídeo avulso, então ele é lido por inteiro.
probe() {
    local url="$1"

    if [[ "$url" == *"watch?v="* || "$url" == *youtu.be/* ]]; then
        ytdlp --skip-download --print "%(title)s|%(channel_id)s" "$url" 2>/dev/null | head -n 1 || true
    else
        ytdlp --flat-playlist --playlist-items 1 --print "%(playlist_title,title)s|%(channel_id)s" "$url" 2>/dev/null |
            head -n 1 || true
    fi
}

# As fontes aceitas, uma por linha. Recusa o que não for do canal permitido: a
# conferência definitiva de cada vídeo é feita de novo no download (--match-filter);
# esta é a verificação antecipada, para o erro aparecer antes de baixar.
verified_sources() {
    local url owner refused=0

    while IFS= read -r url; do
        [[ -n "$url" ]] || continue
        owner="$(probe "$url")"
        owner="${owner##*|}"

        if [[ "$owner" == "$ALLOWED_CHANNEL_ID" ]]; then
            echo "$url"
        else
            printf '    recusado (canal %s, não é o permitido): %s\n' "${owner:-desconhecido}" "$url" >&2
            refused=1
        fi
    done < <(expand_sources)

    return "$refused"
}

# --- Download -----------------------------------------------------------------

# H.264/AAC em MP4 até a altura pedida: o que o playout lê sem esforço. Cai para o
# melhor que houver se o YouTube não oferecer H.264 naquela altura.
format_selector() {
    local h="$MAX_HEIGHT"

    echo "bv*[height<=$h][vcodec^=avc1]+ba[acodec^=mp4a]/bv*[height<=$h][ext=mp4]+ba[ext=m4a]/b[height<=$h][ext=mp4]/bv*[height<=$h]+ba/b[height<=$h]"
}

download_args() {
    local args=(
        --format "$(format_selector)"
        --merge-output-format mp4
        --match-filter "channel_id = $ALLOWED_CHANNEL_ID"
        --download-archive "$LOCAL_DIR/.baixados.txt"
        --output "$LOCAL_DIR/%(playlist_title,Avulsos)s/%(title).150B [%(id)s].%(ext)s"
        --windows-filenames
        --no-mtime
        --ignore-errors
        --retries 10
        --fragment-retries 10
        --concurrent-fragments "$CONCURRENT_FRAGMENTS"
        --sleep-interval "$SLEEP_SECONDS"
        --newline
    )

    [[ "$LIMIT_MBIT" -gt 0 ]] && args+=(--limit-rate "$((LIMIT_MBIT * 125000))")
    [[ -n "$DATE_AFTER" ]] && args+=(--dateafter "$DATE_AFTER")

    printf '%s\n' "${args[@]}"
}

# --- Comandos ----------------------------------------------------------------

cmd_test() {
    ensure_ytdlp
    load_conf
    log "yt-dlp $(yt-dlp --version); interpretador de JavaScript: $(js_runtime || true)"

    if [[ -z "$(js_runtime)" ]]; then
        note "AVISO: sem deno nem node, o YouTube pode esconder formatos. Instale: sudo apt install nodejs"
    fi

    log "Playlists encontradas (só o canal $ALLOWED_CHANNEL_ID é aceito)"

    local url title owner total=0 refused=0

    while IFS= read -r url; do
        [[ -n "$url" ]] || continue
        IFS='|' read -r title owner < <(probe "$url") || true
        title="${title:-$url}"
        total=$((total + 1))

        if [[ "$owner" == "$ALLOWED_CHANNEL_ID" ]]; then
            note "ok       $title"
        else
            note "RECUSADA $title (canal ${owner:-desconhecido})"
            refused=$((refused + 1))
        fi
    done < <(expand_sources)

    note "$total fontes, $refused recusadas"
    [[ "$refused" -eq 0 ]]
}

cmd_size() {
    ensure_ytdlp
    load_conf
    log "Estimando o tamanho (soma das durações; pode demorar com muitos vídeos)"

    local totals url

    totals="$(
        while IFS= read -r url; do
            ytdlp --flat-playlist --print "%(duration)s" "$url" 2>/dev/null || true
        done < <(verified_sources 2>/dev/null) |
            awk '$1 ~ /^[0-9.]+$/ { total += $1; count++ } END { printf "%d %d", total, count }'
    )"

    local videos="${totals#* }" seconds="${totals% *}"

    # 1080p H.264 do YouTube gira em torno de 8 Mbps com o áudio: é só uma estimativa.
    local gigabytes=$((seconds * 8 / 8 / 1000))

    mkdir -p "$LOCAL_DIR"
    note "$videos vídeos, $((seconds / 3600)) horas de conteúdo"
    note "estimativa de espaço: cerca de ${gigabytes} GB (a conta usa ~8 Mbps; pode variar bastante)"
    note "Espaço livre aqui ($LOCAL_DIR): $(df -h --output=avail "$LOCAL_DIR" | tail -n 1 | tr -d ' ')"
    note "Já baixado: $(du -sh "$LOCAL_DIR" 2>/dev/null | cut -f1)"
}

cmd_dry_run() {
    ensure_ytdlp
    load_conf
    log "O que seria baixado, sem baixar nada"
    mkdir -p "$LOCAL_DIR"

    local args url

    mapfile -t args < <(download_args)

    while IFS= read -r url; do
        ytdlp "${args[@]}" --simulate --print "%(playlist_title,Avulsos)s / %(title)s" "$url" || true
    done < <(verified_sources)
}

cmd_run() {
    ensure_ytdlp
    load_conf
    prepare_local_dir
    mkdir -p "$(dirname "$LOG")"

    # Uma importação por vez (o "start" e o timer usam o mesmo cadeado).
    exec 9>"$LOCK"
    flock -n 9 || die "já há uma importação em andamento (veja: sudo $0 status)"

    log "Importando do YouTube -> $LOCAL_DIR (até ${MAX_HEIGHT}p)"
    echo "[$(date '+%F %T')] início" >>"$LOG"

    local urls args url failed=0

    urls="$(verified_sources)" || die "há fontes de outro canal na lista (rode: sudo $0 test)"
    mapfile -t args < <(download_args)

    while IFS= read -r url; do
        [[ -n "$url" ]] || continue
        ytdlp "${args[@]}" "$url" 2>&1 | tee -a "$LOG" || true

        # --ignore-errors segue para o próximo vídeo; o código de saída diz se algum falhou.
        [[ "${PIPESTATUS[0]}" -eq 0 ]] || failed=1
    done <<<"$urls"

    if [[ "$failed" -eq 0 ]]; then
        echo "[$(date '+%F %T')] fim: ok" >>"$LOG"
        log "Importação concluída"
    else
        echo "[$(date '+%F %T')] fim: com erros" >>"$LOG"
        die "algum vídeo falhou (rode de novo: ele só tenta o que falta). Veja $LOG"
    fi
}

cmd_start() {
    need_root start
    load_conf
    command -v systemd-run >/dev/null || die "preciso do systemd-run"

    if systemctl is-active --quiet "$UNIT"; then
        die "já há uma importação rodando (acompanhe com: sudo $0 logs)"
    fi

    # Segundo plano de verdade: continua se a sessão SSH cair. Prioridade baixa de
    # CPU e de disco, para não atrapalhar o sinal no ar.
    systemd-run --unit="$UNIT" --collect --property=Nice=10 \
        --property=IOSchedulingClass=idle \
        --description="Importação do YouTube para o ffplayout" "$SCRIPT" run >/dev/null

    log "Importação iniciada em segundo plano"
    note "Acompanhe: sudo $0 logs     Estado: sudo $0 status"
    note "Se parar por erro de rede, é só rodar o start de novo: ele continua."
}

cmd_logs() {
    if systemctl is-active --quiet "$UNIT" 2>/dev/null; then
        journalctl -u "$UNIT" -f --no-hostname -o cat
    else
        [[ -f "$LOG" ]] || die "ainda não há log em $LOG"
        tail -n 50 "$LOG"
    fi
}

cmd_status() {
    if [[ -r "$CONF" ]]; then
        # shellcheck disable=SC1090
        LOCAL_DIR="$(. "$CONF" 2>/dev/null && echo "${LOCAL_DIR:-}")"
    fi

    local dir="${LOCAL_DIR:-/var/lib/ffplayout/storage/youtube}"

    log "Importação"

    if systemctl is-active --quiet "$UNIT" 2>/dev/null; then
        note "em andamento"
    else
        note "parada"
    fi

    if systemctl cat "$UNIT.timer" >/dev/null 2>&1; then
        note "repetição diária: $(systemctl is-enabled "$UNIT.timer" 2>/dev/null || echo desligada)"
    fi

    [[ -f "$LOG" ]] && note "última linha do log: $(tail -n 1 "$LOG")"
    [[ -f "$dir/.baixados.txt" ]] && note "vídeos já baixados: $(wc -l <"$dir/.baixados.txt")"
    log "Disco"
    df -h "$dir" 2>/dev/null | sed 's/^/    /' || df -h / | sed 's/^/    /'
    note "Já baixado: $(du -sh "$dir" 2>/dev/null | cut -f1)"
    log "Arquivos incompletos (.part)"
    find "$dir" -name '*.part' 2>/dev/null | head -n 10 | sed 's/^/    /' || true
}

cmd_verify() {
    ensure_ytdlp
    load_conf
    log "Conferindo o que do canal ainda falta baixar"

    local archive="$LOCAL_DIR/.baixados.txt" missing=0 id title url

    while IFS= read -r url; do
        [[ -n "$url" ]] || continue

        while IFS='|' read -r id title; do
            if [[ ! -f "$archive" ]] || ! grep -q " $id\$" "$archive"; then
                note "falta: $title ($id)"
                missing=$((missing + 1))
            fi
        done < <(ytdlp --flat-playlist --print "%(id)s|%(title)s" "$url" 2>/dev/null)
    done < <(verified_sources)

    [[ "$missing" -eq 0 ]] || return 1
    note "tudo o que há nas playlists já está em $LOCAL_DIR"
}

cmd_update() {
    need_root update
    install_ytdlp
}

cmd_install_timer() {
    need_root install-timer
    load_conf

    cat >"$SYSTEMD_DIR/$UNIT.service" <<UNITFILE
[Unit]
Description=Importação do YouTube para o ffplayout (vídeos novos)

[Service]
Type=oneshot
Nice=10
IOSchedulingClass=idle
ExecStart=$SCRIPT run
UNITFILE
    cat >"$SYSTEMD_DIR/$UNIT.timer" <<TIMERFILE
[Unit]
Description=Importação diária do YouTube para o ffplayout

[Timer]
# Madrugada, quando o canal tem menos o que fazer.
OnCalendar=*-*-* 05:00:00
RandomizedDelaySec=10min
Persistent=true

[Install]
WantedBy=timers.target
TIMERFILE
    systemctl daemon-reload
    systemctl enable --now "$UNIT.timer"
    log "Importação diária ligada (todo dia às 5h)"
    note "Desligar: sudo systemctl disable --now $UNIT.timer"
}

main() {
    local command="${1:-help}"

    case "$command" in
        init) need_root init; write_template ;;
        test) cmd_test ;;
        size) cmd_size ;;
        dry-run) cmd_dry_run ;;
        run) cmd_run ;;
        start) cmd_start ;;
        logs) cmd_logs ;;
        status) cmd_status ;;
        verify) cmd_verify ;;
        update) cmd_update ;;
        install-timer) cmd_install_timer ;;
        help | -h | --help) usage ;;
        *) usage; die "comando desconhecido: $command" ;;
    esac
}

if [[ "${BASH_SOURCE[0]}" == "$0" ]]; then
    main "$@"
fi
