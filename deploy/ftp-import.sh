#!/usr/bin/env bash
# Traz os vídeos e arquivos de um servidor FTP direto para a VPS (o dado não passa
# pelo seu computador). Usa o lftp: retoma de onde parou, baixa vários arquivos ao
# mesmo tempo, aceita FTP, FTPS e SFTP, e não apaga nada por padrão.
#
#   sudo deploy/ftp-import.sh init        cria o arquivo de configuração (modelo)
#   sudo deploy/ftp-import.sh test        confere o acesso e mostra o que há no FTP
#   sudo deploy/ftp-import.sh size        tamanho do que há no FTP x espaço livre aqui
#   sudo deploy/ftp-import.sh dry-run     mostra o que seria baixado, sem baixar
#   sudo deploy/ftp-import.sh start       baixa em segundo plano (sobrevive ao SSH)
#   sudo deploy/ftp-import.sh logs        acompanha o andamento
#   sudo deploy/ftp-import.sh status      estado do download e do disco
#   sudo deploy/ftp-import.sh verify      confere se tudo o que há no FTP já está aqui
#   sudo deploy/ftp-import.sh install-timer   repete todo dia de madrugada (conteúdo novo)
#   sudo deploy/ftp-import.sh run         baixa agora, nesta tela (o "start" usa este)
#
# Tudo é configurado em /etc/ffplayout/ftp-import.conf (a senha fica lá, só o
# administrador lê). Os arquivos baixados pertencem ao usuário do ffplayout.

set -euo pipefail

SCRIPT="$(readlink -f "${BASH_SOURCE[0]}")"
CONF="${FFP_FTP_CONF:-/etc/ffplayout/ftp-import.conf}"
LOG="${FFP_FTP_LOG:-/var/log/ffplayout/ftp-import.log}"
SVC_USER="${FFP_USER:-ffpu}"
SYSTEMD_DIR="${FFP_SYSTEMD_DIR:-/etc/systemd/system}"
UNIT="ffplayout-ftp-import"
LOCK="${FFP_FTP_LOCK:-/run/ffplayout-ftp-import.lock}"

log() { printf '\n\033[1;34m==>\033[0m %s\n' "$*"; }
note() { printf '    %s\n' "$*"; }
die() {
    printf '\033[1;31m[erro]\033[0m %s\n' "$*" >&2
    exit 1
}

usage() { sed -n '2,20p' "$SCRIPT" | sed 's/^# \{0,1\}//'; }

need_root() {
    [[ $EUID -eq 0 ]] || die "rode como administrador: sudo $0 $*"
}

ensure_lftp() {
    command -v lftp >/dev/null && return 0
    [[ $EUID -eq 0 ]] || die "o lftp não está instalado: sudo apt install lftp"
    log "Instalando o lftp"
    DEBIAN_FRONTEND=noninteractive apt-get install -y --no-install-recommends lftp
}

# --- Configuração ------------------------------------------------------------

write_template() {
    mkdir -p "$(dirname "$CONF")"

    if [[ -e "$CONF" ]]; then
        die "$CONF já existe; edite-o (nano $CONF)"
    fi

    (
        umask 077
        cat >"$CONF" <<'EOF'
# Configuração da importação do FTP (ver deploy/ftp-import.sh).
# Este arquivo guarda a senha: só o administrador deve lê-lo.

# --- O servidor ---
FTP_HOST=ftp.exemplo.com.br
FTP_PORT=21
# ftp   = FTP comum, mas tenta TLS se o servidor oferecer
# ftps  = FTP com TLS obrigatório
# sftp  = SFTP (SSH), costuma ser a porta 22
FTP_PROTOCOL=ftp
FTP_USER=usuario
FTP_PASS='senha'
# Alternativa: FTP_PASS_FILE=/root/senha-ftp (um arquivo só com a senha)
# Em servidor FTPS com certificado vencido ou autoassinado, troque para no.
FTP_VERIFY_CERT=yes

# --- De onde para onde ---
# Pasta no FTP que será copiada (tudo o que está dentro dela).
FTP_REMOTE_DIR=/
# Pasta de destino na VPS (a mesma que o ffplayout usa como storage).
LOCAL_DIR=/var/lib/ffplayout/storage

# --- Como baixar ---
# missing = só baixa o que ainda não existe aqui (nunca sobrescreve) [recomendado]
# newer   = também baixa de novo o que mudou no FTP
FTP_MODE=missing
# Quantos arquivos ao mesmo tempo, e em quantos pedaços cada arquivo grande.
PARALLEL_FILES=3
SEGMENTS_PER_FILE=1
# Limite de velocidade em megabits por segundo (0 = sem limite). Use para não
# disputar banda com o sinal no ar enquanto o canal está transmitindo.
LIMIT_MBIT=0
# Não baixar arquivos que casam com estes padrões (separe com espaço).
EXCLUDE_GLOBS="*.tmp Thumbs.db .DS_Store *.part"
# Apagar aqui o que foi apagado no FTP? Deixe 0, a menos que tenha certeza.
DELETE_REMOVED=0
EOF
    )

    note "modelo criado em $CONF: edite com os dados do FTP (nano $CONF)"
}

load_conf() {
    [[ -r "$CONF" ]] || die "não achei $CONF; crie com: sudo $0 init"

    # shellcheck disable=SC1090
    . "$CONF"

    FTP_PORT="${FTP_PORT:-21}"
    FTP_PROTOCOL="${FTP_PROTOCOL:-ftp}"
    FTP_VERIFY_CERT="${FTP_VERIFY_CERT:-yes}"
    FTP_REMOTE_DIR="${FTP_REMOTE_DIR:-/}"
    LOCAL_DIR="${LOCAL_DIR:-/var/lib/ffplayout/storage}"
    FTP_MODE="${FTP_MODE:-missing}"
    PARALLEL_FILES="${PARALLEL_FILES:-3}"
    SEGMENTS_PER_FILE="${SEGMENTS_PER_FILE:-1}"
    LIMIT_MBIT="${LIMIT_MBIT:-0}"
    EXCLUDE_GLOBS="${EXCLUDE_GLOBS:-}"
    DELETE_REMOVED="${DELETE_REMOVED:-0}"

    [[ -n "${FTP_HOST:-}" && "$FTP_HOST" != "ftp.exemplo.com.br" ]] || die "preencha FTP_HOST em $CONF"
    [[ -n "${FTP_USER:-}" ]] || die "preencha FTP_USER em $CONF"

    if [[ -z "${FTP_PASS:-}" && -n "${FTP_PASS_FILE:-}" ]]; then
        [[ -r "$FTP_PASS_FILE" ]] || die "não consigo ler FTP_PASS_FILE ($FTP_PASS_FILE)"
        FTP_PASS="$(head -n 1 "$FTP_PASS_FILE")"
    fi

    [[ -n "${FTP_PASS:-}" ]] || die "preencha FTP_PASS (ou FTP_PASS_FILE) em $CONF"
    [[ "$FTP_PROTOCOL" =~ ^(ftp|ftps|sftp)$ ]] || die "FTP_PROTOCOL deve ser ftp, ftps ou sftp"
    [[ "$FTP_MODE" =~ ^(missing|newer)$ ]] || die "FTP_MODE deve ser missing ou newer"
    [[ "$PARALLEL_FILES" =~ ^[0-9]+$ && "$SEGMENTS_PER_FILE" =~ ^[0-9]+$ && "$LIMIT_MBIT" =~ ^[0-9]+$ ]] ||
        die "PARALLEL_FILES, SEGMENTS_PER_FILE e LIMIT_MBIT devem ser números inteiros"
    [[ "$FTP_PORT" =~ ^[0-9]+$ ]] || die "FTP_PORT deve ser um número"
}

# Entre aspas para o lftp: barra e aspas viram texto comum.
lftp_quote() {
    local text="$1"

    text="${text//\\/\\\\}"
    text="${text//\"/\\\"}"
    printf '"%s"' "$text"
}

# O roteiro do lftp. Vai pela entrada padrão, então a senha nunca aparece na lista
# de processos (ps) nem em arquivo.
lftp_prelude() {
    echo "set cmd:fail-exit yes"
    echo "set net:timeout 30"
    echo "set net:max-retries 30"
    echo "set net:reconnect-interval-base 10"
    echo "set net:reconnect-interval-max 60"
    echo "set ftp:passive-mode yes"
    # Baixa para "nome.part" e só troca para o nome final quando termina: o ffplayout
    # observa a pasta e não deve ver arquivo pela metade.
    echo "set xfer:use-temp-file yes"
    echo "set xfer:temp-file-name *.part"
    echo "set xfer:clobber no"

    case "$FTP_PROTOCOL" in
        ftp) echo "set ftp:ssl-allow yes" ;;
        ftps)
            echo "set ftp:ssl-allow yes"
            echo "set ftp:ssl-force yes"
            echo "set ftp:ssl-protect-data yes"
            ;;
    esac

    [[ "$FTP_VERIFY_CERT" == "yes" ]] || echo "set ssl:verify-certificate no"

    if [[ "$LIMIT_MBIT" -gt 0 ]]; then
        # lftp conta em bytes por segundo: megabit x 125000.
        echo "set net:limit-total-rate $((LIMIT_MBIT * 125000)):0"
    fi

    local scheme="$FTP_PROTOCOL"
    [[ "$scheme" == "ftps" ]] && scheme="ftp"

    echo "open -u $(lftp_quote "$FTP_USER"),$(lftp_quote "$FTP_PASS") -p $FTP_PORT $scheme://$FTP_HOST"
}

mirror_command() {
    # --no-perms: não copia as permissões do servidor (um servidor que mostra 000
    # deixaria os arquivos daqui ilegíveis). Os arquivos ficam com o dono e o umask
    # do usuário do ffplayout.
    local dry="$1" cmd="mirror --verbose --continue --no-perms --parallel=$PARALLEL_FILES"
    local glob

    [[ "$SEGMENTS_PER_FILE" -gt 1 ]] && cmd+=" --use-pget-n=$SEGMENTS_PER_FILE"
    [[ "$FTP_MODE" == "missing" ]] && cmd+=" --only-missing" || cmd+=" --only-newer"
    [[ "$DELETE_REMOVED" == "1" ]] && cmd+=" --delete"
    [[ "$dry" == "1" ]] && cmd+=" --dry-run"

    for glob in $EXCLUDE_GLOBS; do
        cmd+=" --exclude-glob $(lftp_quote "$glob")"
    done

    cmd+=" $(lftp_quote "$FTP_REMOTE_DIR") $(lftp_quote "$LOCAL_DIR")"
    echo "$cmd"
}

# O lftp mostra as URLs com usuário e senha (no --dry-run e em alguns erros): a
# senha nunca deve ir para a tela nem para o log.
mask_secrets() {
    sed -E 's#(://[^:/@ ]*):[^@ ]*@#\1:***@#g'
}

# Roda o roteiro do lftp como o usuário do ffplayout (os arquivos nascem com o dono
# certo). Sem administrador (testes), roda como o usuário atual.
run_lftp() {
    if [[ $EUID -eq 0 && "$SVC_USER" != "root" ]]; then
        runuser -u "$SVC_USER" -- lftp -f /dev/stdin 2>&1 | mask_secrets
    else
        lftp -f /dev/stdin 2>&1 | mask_secrets
    fi
}

lftp_script() {
    lftp_prelude
    cat
}

prepare_local_dir() {
    mkdir -p "$LOCAL_DIR"

    if [[ $EUID -eq 0 && "$SVC_USER" != "root" ]]; then
        chown "$SVC_USER": "$LOCAL_DIR"
    fi

    [[ -w "$LOCAL_DIR" || $EUID -eq 0 ]] || die "sem permissão para escrever em $LOCAL_DIR"
}

# --- Comandos ----------------------------------------------------------------

cmd_test() {
    ensure_lftp
    load_conf
    log "Conectando em $FTP_PROTOCOL://$FTP_HOST:$FTP_PORT como $FTP_USER"

    lftp_script <<EOF | run_lftp
echo "Conectado. Conteúdo de $FTP_REMOTE_DIR:"
cls -l $(lftp_quote "$FTP_REMOTE_DIR")
EOF
}

cmd_size() {
    ensure_lftp
    load_conf
    log "Medindo o que há no FTP (pode demorar com muitos arquivos)"

    lftp_script <<EOF | run_lftp
du -s -h $(lftp_quote "$FTP_REMOTE_DIR")
EOF

    mkdir -p "$LOCAL_DIR"
    note "Espaço livre aqui ($LOCAL_DIR): $(df -h --output=avail "$LOCAL_DIR" | tail -n 1 | tr -d ' ')"
    note "Já baixado: $(du -sh "$LOCAL_DIR" 2>/dev/null | cut -f1)"
}

cmd_dry_run() {
    ensure_lftp
    load_conf
    log "O que seria baixado (modo $FTP_MODE), sem baixar nada"

    lftp_script <<<"$(mirror_command 1)" | run_lftp
}

cmd_run() {
    ensure_lftp
    load_conf
    prepare_local_dir
    mkdir -p "$(dirname "$LOG")"

    # Uma importação por vez (o "start" e o timer usam o mesmo cadeado).
    exec 9>"$LOCK"
    flock -n 9 || die "já há uma importação em andamento (veja: sudo $0 status)"

    log "Importando $FTP_HOST:$FTP_REMOTE_DIR -> $LOCAL_DIR (modo $FTP_MODE)"
    echo "[$(date '+%F %T')] início" >>"$LOG"

    if lftp_script <<<"$(mirror_command 0)" | run_lftp 2>&1 | tee -a "$LOG"; then
        echo "[$(date '+%F %T')] fim: ok" >>"$LOG"
        log "Importação concluída"
    else
        echo "[$(date '+%F %T')] fim: erro" >>"$LOG"
        die "a importação parou com erro (rode de novo: ele continua de onde parou). Veja $LOG"
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
        --description="Importação do FTP para o ffplayout" "$SCRIPT" run >/dev/null

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
    # Só a pasta de destino interessa aqui; uma configuração incompleta não impede de olhar.
    if [[ -r "$CONF" ]]; then
        # shellcheck disable=SC1090
        LOCAL_DIR="$(. "$CONF" 2>/dev/null && echo "${LOCAL_DIR:-}")"
    fi

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
    log "Disco"
    df -h "${LOCAL_DIR:-/var/lib/ffplayout/storage}" | sed 's/^/    /'
    note "Já baixado: $(du -sh "${LOCAL_DIR:-/var/lib/ffplayout/storage}" 2>/dev/null | cut -f1)"
    log "Arquivos incompletos (.part)"
    find "${LOCAL_DIR:-/var/lib/ffplayout/storage}" -name '*.part' 2>/dev/null | head -n 10 | sed 's/^/    /' || true
}

cmd_verify() {
    ensure_lftp
    load_conf
    log "Conferindo o que ainda falta baixar"

    local output
    output="$(FTP_MODE=missing lftp_script <<<"$(FTP_MODE=missing mirror_command 1)" | run_lftp 2>&1)" || true

    if grep -qE '^(get|mirror|Transferring|Making|Removing)' <<<"$output" || grep -q ' -> ' <<<"$output"; then
        note "ainda falta baixar:"
        grep -E '^(get|mirror|Transferring|Making|Removing)|->' <<<"$output" | head -n 20 | sed 's/^/    /'
        return 1
    fi

    note "tudo o que há no FTP já está em $LOCAL_DIR"
}

cmd_install_timer() {
    need_root install-timer
    load_conf

    cat >"$SYSTEMD_DIR/$UNIT.service" <<EOF
[Unit]
Description=Importação do FTP para o ffplayout (conteúdo novo)

[Service]
Type=oneshot
Nice=10
IOSchedulingClass=idle
ExecStart=$SCRIPT run
EOF
    cat >"$SYSTEMD_DIR/$UNIT.timer" <<EOF
[Unit]
Description=Importação diária do FTP para o ffplayout

[Timer]
# Madrugada, quando o canal tem menos o que fazer.
OnCalendar=*-*-* 04:30:00
RandomizedDelaySec=10min
Persistent=true

[Install]
WantedBy=timers.target
EOF
    systemctl daemon-reload
    systemctl enable --now "$UNIT.timer"
    log "Importação diária ligada (todo dia às 4h30)"
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
        install-timer) cmd_install_timer ;;
        help | -h | --help) usage ;;
        *) usage; die "comando desconhecido: $command" ;;
    esac
}

if [[ "${BASH_SOURCE[0]}" == "$0" ]]; then
    main "$@"
fi
