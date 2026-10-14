# Implantação em VPS: relay, HTTPS e backup

Passo a passo para colocar o canal no ar em uma VPS Linux (Debian ou Ubuntu).
Cobre a Fase 1 do `.dev/roadmap.md`: relay das saídas, reinício rápido, HTTPS
e backup.

> **O que já foi testado e o que não foi** está no fim deste arquivo. Leia antes
> de confiar no que não foi testado.

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
   **troque `SENHA_DOS_DESTINOS`** por uma senha forte.
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
2. Copie `deploy/systemd/ffplayout.service` (ele escuta só em `127.0.0.1`).
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
| RTMP | `rtmp://HOST:1935/canal1?user=destino&pass=SENHA` |
| SRT | `srt://HOST:8890?streamid=read:canal1:destino:SENHA` |
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

**Não testado** (precisa da máquina e dos destinos reais):

- Uma versão **estável** do MediaMTX (a testada foi uma compilação de
  desenvolvimento); nomes de parâmetros podem diferir.
- nginx e certbot (não há nginx na nuvem); as unidades do systemd foram
  verificadas só quanto à sintaxe.
- A saída do **ffplayout** (modo Stream) para o relay, com a programação
  rodando 24 h, e o tempo real de queda e retorno visto por um destino.
- Destinos reais: se reconectam sozinhos, e com qual atraso.
- Banda, CPU e a estabilidade do SRT numa VPS de verdade.
