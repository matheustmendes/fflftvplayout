# Implantação em VPS: relay, HTTPS e backup

Passo a passo para colocar o canal no ar em uma VPS Linux (Debian ou Ubuntu).
Cobre a Fase 1 do `.dev/roadmap.md`: relay das saídas, reinício rápido, HTTPS
e backup.

> **O que já foi testado e o que não foi** está no fim deste arquivo. Leia antes
> de confiar no que não foi testado.

## Instalação rápida (script)

`deploy/install.sh` faz os passos deste arquivo sozinho, inclusive o build. Numa VPS
nova (Debian 13 ou Ubuntu 22.04/24.04), com os domínios já apontados para ela:

```sh
git clone https://github.com/matheustmendes/fflftvplayout.git
cd fflftvplayout
sudo deploy/install.sh install \
    --panel-domain painel.exemplo.com.br \
    --signal-domain sinal.exemplo.com.br \
    --email voce@exemplo.com.br \
    --destinations 12
```

O que ele faz, na ordem: instala as dependências e o FFmpeg 7 (no Ubuntu usa o PPA
`ppa:ubuntuhandbook1/ffmpeg7`), o Node 24 (do site oficial, com checksum) e o Rust
(`rustup`, para o usuário que compila); compila o painel e o ffplayout **sem o player
de desktop**; cria o usuário `ffpu` e as pastas em `/var/lib/ffplayout`; faz a
configuração inicial **sem perguntas** (administrador com senha gerada); instala o relay
MediaMTX (versão fixa, com checksum) com um usuário e uma senha por destino; liga o
serviço; configura o nginx e pede o certificado Let's Encrypt; instala o backup diário;
e libera as portas se o `ufw` estiver ativo.

As senhas e os links dos destinos são **gerados** e ficam em
`/root/ffplayout-credenciais.txt` (só o administrador lê). Nada é impresso na tela.

- Sem `--panel-domain`/`--signal-domain`, o nginx não é configurado e o painel fica só em
  `127.0.0.1:8787` (acesse por túnel SSH).
- Pode rodar de novo: banco, senhas e certificados existentes não são refeitos.
- **Atualizar:** `sudo deploy/install.sh update --pull` baixa a versão nova, compila, faz
  backup do banco e troca o binário. Reiniciar corta a saída por poucos segundos; com
  `--no-restart` o binário é trocado e você reinicia num momento calmo
  (`sudo systemctl restart ffplayout`).
- **Ver o estado:** `deploy/install.sh status`.
- `deploy/install.sh --help` lista todas as opções (`--no-relay`, `--no-backup`,
  `--skip-build`, `--admin-user`...).

A compilação leva alguns minutos na primeira vez (o `release` usa LTO) e pede uma VPS
com folga de memória e disco; como estimativa, não medida, conte com 4 GB de RAM (ou
um swap) e uns 8 GB livres. Se a VPS for pequena, compile em outra máquina e use
`--skip-build` com o binário copiado para `target/release/ffplayout`. Depois do script, siga os "Próximos passos" que ele imprime
(saída Stream para o relay, destinos no Painel, Telegram).

Os passos manuais abaixo continuam valendo para quem prefere instalar à mão ou já tem
parte pronta.

## Visão geral

```
ffplayout ──RTMP 127.0.0.1:1935──► relay (MediaMTX) ──┬─ RTMP  :1935  (destinos puxam)
 (1 codificação, 3000 kbps)                           ├─ SRT   :8890  (destinos puxam)
                                                      └─ HLS   :8888 ► nginx HTTPS ► m3u8
```

O ffplayout codifica **uma vez** e envia um sinal para o relay, na mesma
máquina. Os destinos puxam do relay: com 12 destinos a banda de saída é de
12 × 3 Mbps, e o ffplayout não codifica nada a mais.

| O quê | Onde | Porta | Quem acessa |
|---|---|---|---|
| Painel e API do ffplayout | nginx (HTTPS) → `127.0.0.1:8787` | 443 | operadores |
| m3u8 | nginx (HTTPS) → relay `127.0.0.1:8888` | 443 | destinos |
| RTMP | relay | 1935/TCP | destinos |
| SRT | relay | 8890/UDP | destinos |

Abra no firewall só 22, 80, 443, 1935/TCP e 8890/UDP.

## 1. Relay (MediaMTX)

1. Baixe o **binário de uma versão estável** em
   <https://github.com/bluenviron/mediamtx/releases> (não use a branch
   principal) e coloque em `/opt/mediamtx/mediamtx`.
2. Copie `deploy/relay/mediamtx.yml` para `/etc/mediamtx/mediamtx.yml` e
   **troque as senhas** `SENHA_DO_DESTINO_1`, `SENHA_DO_DESTINO_2`... por senhas fortes (um usuário por destino).
3. Crie o usuário e ative o serviço:

```sh
sudo useradd --system --no-create-home --shell /usr/sbin/nologin mediamtx
sudo cp deploy/systemd/mediamtx.service /etc/systemd/system/
sudo systemctl daemon-reload && sudo systemctl enable --now mediamtx
```

Se o MediaMTX recusar a configuração por causa de um parâmetro desconhecido,
a versão é diferente da testada: veja o aviso no log (`journalctl -u mediamtx`)
e ajuste o nome do parâmetro conforme a documentação daquela versão.

## 2. ffplayout

1. Instale o ffplayout (pacote `.deb` ou binário) e o FFmpeg 7 ou mais novo.
2. Copie `deploy/systemd/ffplayout.service` (ele escuta só em `127.0.0.1`). Ele espera o binário em `/usr/local/bin/ffplayout` e o banco em `/var/lib/ffplayout`; com o pacote `.deb` ajuste o `ExecStart` (veja o comentário no arquivo).
3. No painel, em **Configurar > Playout**, escolha a saída **Stream** e use:

```
rtmp://127.0.0.1:1935/canal1
```

com vídeo 1080p 30 fps a 3000 kbps. O relay aceita publicação só a partir de
`127.0.0.1`, sem senha (veja `authInternalUsers` no `mediamtx.yml`).

O supervisor do ffplayout reinicia o canal em **no máximo 10 s** depois de uma
falha (os intervalos crescem 1, 2, 4, 8 e 10 s). O `systemd` reinicia o
processo inteiro em 1 s se ele morrer. Falhas do canal e "dia sem programação" são
registradas como erro e seguem pelas notificações do ffplayout (e-mail; ver
`docs/notifications.md`).

### Semáforo dos destinos no Painel

O `mediamtx.yml` liga a API do relay em `127.0.0.1:9997` (só nesta máquina). No
ffplayout, como administrador, abra o Painel > **Destinos** > Configurar e
informe:

- o endereço da API: `http://127.0.0.1:9997`;
- o nome do canal no relay: `canal1`;
- cada destino com o **usuário** que ele usa (RTMP: `destino1`) ou o **endereço
  IP** de onde ele puxa (SRT, que não mostra usuário na API).

O Painel então mostra uma luz por destino: verde quando está conectado, vermelho
quando não está, cinza quando o relay não responde. Destinos de m3u8 (HLS) não
aparecem: o relay não separa quem está lendo o HLS.

### O que os destinos veem quando o ffplayout reinicia

**Quando o ffplayout para de enviar, o MediaMTX desconecta todos os destinos**
(RTMP e SRT), e eles precisam reconectar sozinhos. Isso foi medido (ver abaixo).
Na prática, o corte que o destino vê dura: tempo de reinício do ffplayout
(poucos segundos) + o tempo que o próprio destino leva para reconectar. A
tolerância da emissora é de 1 min 30 s; confirme com os destinos que o player
deles **reconecta sozinho**, e meça com o destino real.

## 3. HTTPS (nginx)

```sh
sudo apt install nginx certbot python3-certbot-nginx apache2-utils
sudo cp deploy/nginx/ffplayout.conf /etc/nginx/sites-available/ffplayout
# troque painel.exemplo.com.br e sinal.exemplo.com.br pelos seus domínios
sudo ln -s /etc/nginx/sites-available/ffplayout /etc/nginx/sites-enabled/
sudo htpasswd -c /etc/nginx/hls.htpasswd destino
sudo certbot --nginx -d painel.exemplo.com.br -d sinal.exemplo.com.br
sudo nginx -t && sudo systemctl reload nginx
```

O m3u8 fica em `https://sinal.exemplo.com.br/canal1/index.m3u8`, com usuário e
senha do nginx. O relay responde com um redirecionamento de verificação de
cookie na primeira chamada; players e o `ffprobe` seguem sozinhos.

## 4. Links para entregar aos destinos

| Formato | Link |
|---|---|
| RTMP | `rtmp://HOST:1935/canal1?user=destino1&pass=SENHA` |
| SRT | `srt://HOST:8890?streamid=read:canal1:destino1:SENHA` |
| m3u8 | `https://sinal.exemplo.com.br/canal1/index.m3u8` (usuário e senha do nginx) |

## 5. Backup

`deploy/backup/ffplayout-backup.py` copia o banco com a API de backup do
SQLite (seguro com o ffplayout rodando), as programações e as pastas
escolhidas do storage (`commercials` e `marca` por padrão: os programas ficam
de fora, pois são grandes e a emissora já os tem). Mantém os últimos N backups.

```sh
sudo mkdir -p /opt/ffplayout /var/backups/ffplayout
sudo cp deploy/backup/ffplayout-backup.py /opt/ffplayout/
sudo chown ffpu /var/backups/ffplayout
sudo cp deploy/systemd/ffplayout-backup.{service,timer} /etc/systemd/system/
# ajuste os caminhos no .service conforme a instalação
sudo systemctl daemon-reload && sudo systemctl enable --now ffplayout-backup.timer
```

Para **restaurar**: pare o ffplayout, extraia o `.tar.gz` mais recente e
copie `ffplayout.db`, `playlists/` e `storage/…` de volta para os caminhos de
uso, e inicie o ffplayout.

```sh
sudo systemctl stop ffplayout
tar xzf /var/backups/ffplayout/ffplayout-AAAAMMDD-HHMMSS.tar.gz -C /tmp/restaura
# copie ffplayout.db, playlists/ e storage/* para os caminhos de uso
sudo systemctl start ffplayout
```

**Copie os backups para fora da VPS** (outro servidor ou armazenamento de
objetos): um backup na mesma máquina não protege contra a perda da máquina.

## 6. Trazer o conteúdo de um servidor FTP

`deploy/ftp-import.sh` copia os vídeos e arquivos de um servidor FTP **direto para a
VPS** (nada passa pelo seu computador). Usa o `lftp`: retoma de onde parou se a conexão
cair, baixa vários arquivos ao mesmo tempo e aceita FTP, FTPS e SFTP.

```sh
sudo deploy/ftp-import.sh init       # cria /etc/ffplayout/ftp-import.conf (modelo)
sudo nano /etc/ffplayout/ftp-import.conf   # servidor, usuário, senha, pastas
sudo deploy/ftp-import.sh test       # confere o acesso e lista a pasta
sudo deploy/ftp-import.sh size       # tamanho no FTP x espaço livre aqui
sudo deploy/ftp-import.sh dry-run    # o que seria baixado, sem baixar
sudo deploy/ftp-import.sh start      # baixa em segundo plano (sobrevive ao SSH)
sudo deploy/ftp-import.sh logs       # acompanha
sudo deploy/ftp-import.sh verify     # confere se tudo já está aqui
sudo deploy/ftp-import.sh install-timer   # repete todo dia às 4h30 (conteúdo novo)
```

Cuidados que o script já toma:
- **Nunca apaga e nunca sobrescreve** por padrão (`FTP_MODE=missing`, `DELETE_REMOVED=0`).
- Baixa para `nome.part` e só troca para o nome final quando termina, para o ffplayout
  não ver arquivo pela metade.
- Não copia as permissões do servidor (um servidor que mostra `000` deixaria os arquivos
  ilegíveis) e deixa os arquivos com o dono do ffplayout.
- A senha fica só no arquivo de configuração (só o administrador lê), vai ao `lftp` pela
  entrada padrão e é mascarada em tudo o que aparece na tela e no log.
- `LIMIT_MBIT` limita a velocidade, para a importação não disputar banda com o sinal no
  ar. O `start` roda com prioridade baixa de CPU e de disco.

Sobre os comerciais: o script só traz os arquivos para o storage. Para um comercial valer
na programação ele precisa ser **registrado** (Publicidade > Comerciais, ou
`POST /api/commercials/{canal}/spots` apontando o arquivo do storage), o que dispara a
conversão para o formato do canal.

## Testado e não testado

Testado na nuvem (MediaMTX compilado da branch principal, FFmpeg 7.1.1):

- Publicar por RTMP e puxar por RTMP, SRT e HLS (h264 + aac, 1280x720 e
  1920x1080), **com e sem senha** (sem senha é recusado nos três).
- **12 destinos simultâneos** (6 RTMP + 6 SRT) por 20 s: sem queda; o relay usou
  cerca de 13 % de uma CPU e 62 MB (sinal de 3000 kbps).
- **Queda do publicador:** os destinos RTMP e SRT foram desconectados no mesmo
  instante e o relay aceitou a nova publicação poucos segundos depois.
- O backup: criação, retenção, restauração do banco, erros de entrada.
- O teto de reinício do supervisor e os alertas, com testes automatizados.

Testado na nuvem para o `install.sh`:

- `shellcheck` sem avisos e `bash -n`.
- A geração do `mediamtx.yml` com 12 destinos, carregada pelo MediaMTX v1.18.0
  (a versão fixada no script); o checksum e a extração do Node 24.
- A configuração inicial sem perguntas (`ffplayout --init ...` com todas as opções):
  cria o banco e o administrador, e o login funciona com a senha gerada.
- O `nginx -t` da configuração completa (com certificados de teste) e da parte só de
  porta 80. Isso achou um erro antigo: o `http2 on;` não existe no nginx 1.24 do
  Ubuntu 24.04; o modelo agora usa `listen 443 ssl http2;`.
- A compilação **sem** o player de desktop (`--no-default-features --features
  embed_frontend`), com `cargo check`.

Testado na nuvem para o `ftp-import.sh` (contra um servidor FTP local): acesso com senha
cheia de caracteres especiais, `dry-run`, download com retomada depois de interrupção
(os arquivos conferem byte a byte), exclusão de padrões, 3 pedaços por arquivo, modo
`newer`, `DELETE_REMOVED`, `verify` achando arquivo novo, e a senha ausente do log.
**Não testado:** FTPS e SFTP, o `start` e o `install-timer` (precisam de systemd) e um
servidor FTP real, que pode ter limites de conexões simultâneas (se reclamar, baixe
`PARALLEL_FILES` para 1 ou 2).

**Não testado** no `install.sh` (precisa de uma VPS de verdade): o `apt`, o PPA do
FFmpeg, o `rustup`, o build `release` completo, o `certbot`, o `systemd` e o `ufw`.
Rode primeiro numa VPS descartável.

**Não testado** (precisa da máquina e dos destinos reais):

- Uma versão **estável** do MediaMTX (a testada foi uma compilação de
  desenvolvimento); nomes de parâmetros podem diferir.
- nginx e certbot (não há nginx na nuvem); as unidades do systemd foram
  verificadas só quanto à sintaxe.
- A saída do **ffplayout** (modo Stream) para o relay, com a programação
  rodando 24 h, e o tempo real de queda e retorno visto por um destino.
- Destinos reais: se reconectam sozinhos, e com qual atraso.
- Banda, CPU e a estabilidade do SRT numa VPS de verdade.
