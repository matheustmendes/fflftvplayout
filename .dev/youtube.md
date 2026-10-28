# YouTube no playout

Dois assuntos: o que já existe (importar vídeos para o servidor) e o que o cliente
pediu a mais (colocar **lives e vídeos do YouTube na íntegra** no ar, sempre do canal
dele).

## 1. Importar vídeos (feito)

Tela **YouTube** no painel (administradores). O servidor roda o `yt-dlp` por conta
própria e grava em `storage/youtube/<playlist>/`. O vídeo entra na grade como qualquer
arquivo.

- Só endereços do YouTube. O dono do endereço precisa estar em **Canais permitidos**
  (só o administrador geral edita essa lista). O `yt-dlp` confere cada vídeo de novo
  (`--match-filters channel_id = ...`), então uma playlist com vídeo de terceiros não
  traz o vídeo.
- Um download por vez, em segundo plano, com prioridade baixa (`nice`). Retoma e não
  repete (`storage/youtube/.baixados.txt`, a mesma lista que o `deploy/youtube-import.sh`
  usa).
- Código: `backend/app/src/utils/youtube.rs`, `api/routes/youtube.rs`,
  `db/handles/youtube.rs`, migração `00018_youtube.sql`;
  frontend `views/YoutubeView.vue`. Roteiro de aceite: `.dev/acceptance/F10.md`.
- Variáveis: `FFPLAYOUT_YTDLP` (caminho do programa) e `FFPLAYOUT_YOUTUBE_COOKIES`
  (padrão `/etc/ffplayout/youtube-cookies.txt`, o usuário do serviço precisa ler).
- **Risco conhecido:** o YouTube recusa IP de datacenter ("confirme que você não é um
  robô"). Isso aconteceu no ambiente de desenvolvimento. O painel mostra a mensagem em
  português; a saída é o arquivo de cookies. Só a VPS de verdade dirá se acontece lá.

## 2. O pedido do cliente: lives e vídeos na íntegra, do canal dele

### O que o playout já sabe fazer (verificado no código)
- Item de playlist com fonte `rtmp://`, `srt://`, `udp://`, `rtsp://`, `tcp://`, `rist://`
  é tratado como **fonte ao vivo** com uma janela de tempo (`out`); sem `out`, vale o
  padrão. `normalize_live_source_timing` (`player/utils/mod.rs`).
- Item `http(s)://` é aceito como fonte remota, mas **não** é tratado como live (pode ser
  VOD ou manifesto), e o endereço do YouTube não é um arquivo nem um HLS direto: é uma
  página.
- Há entradas ao vivo por escuta (`config_live_input`, RTMP/SRT) que assumem o ar por
  prioridade.

### Vídeo na íntegra (VOD)
| Opção | Como | Prós | Contras |
|---|---|---|---|
| **A. Baixar antes (recomendada)** | A tela de importar, já pronta. O vídeo vira arquivo e entra na grade. | Confiável 24/7, nenhuma dependência do YouTube na hora de ir ao ar, qualidade controlada (H.264 1080p), nada de bloqueio durante a transmissão. | Ocupa disco; precisa baixar antes. |
| B. Resolver na hora | `yt-dlp -g` devolve o endereço direto do vídeo na hora de tocar. | Sem espaço em disco. | O endereço expira (cerca de 6 h) e é preso ao IP; se o YouTube bloquear ou cair no meio, **o canal fica fora do ar**. Não serve para 24/7. |

Ajuste que falta para A ficar "na íntegra" no uso diário: ao agendar um vídeo importado,
usar a duração real (já vem do arquivo). Nada novo no motor.

### Live
Uma live não tem arquivo nem duração: é um fluxo. O encaixe natural no que existe:

1. **Retransmissor local.** Um processo no servidor puxa a live do YouTube
   (`yt-dlp` resolve o HLS; `ffmpeg -c copy` empurra) e publica numa porta local,
   por exemplo `srt://127.0.0.1:9100?mode=listener` ou `rtmp://127.0.0.1/yt/1`.
2. **Item de playlist com essa fonte** e uma janela de horário (`out`): o motor já
   trata como fonte ao vivo. Fora da janela, volta ao programa seguinte; se a live cair,
   entra o reserva (filler), como hoje.
3. **Painel:** em "YouTube", uma seção "Lives do canal" lista as lives agendadas e em
   andamento (`yt-dlp --flat-playlist` na aba `/streams` do canal já devolve
   `live_status` e horário), com um botão "Colocar no ar às ... por ... min" que cria o
   item na grade. O dono é conferido como na importação.

Tamanho estimado: **médio** (3 a 5 dias): gerenciador do retransmissor (iniciar um
pouco antes, reconectar, encerrar), itens de playlist com "fonte YouTube" resolvida pelo
servidor, a seção de lives e testes com um fluxo simulado. O motor (`backend/engine`)
não muda.

### Riscos e o que não dá para prometer
- **Termos do YouTube.** Baixar ou retransmitir fora do player deles não é uma função
  oferecida pelo YouTube, mesmo sendo conteúdo do próprio cliente. O risco é o YouTube
  bloquear o IP ou a conta usada nos cookies, não um problema de direitos autorais. O
  cliente precisa saber disso e assumir. Quando houver o arquivo original (o que ele
  gravou), **usar o original é sempre melhor** que baixar do YouTube (qualidade maior,
  sem bloqueio).
- **Bloqueio de IP.** Já visto. Com cookies costuma resolver, mas pode voltar. Live
  depende disso o tempo todo; um bloqueio no meio derruba a live no canal.
- **Atraso.** A live do YouTube chega com 10 a 30 s de atraso sobre o real, mais o do
  canal. Serve para "reprisar a live", não para ao vivo de verdade.
- **Qualidade.** Quem assiste vê no máximo o que o YouTube entrega (no máximo 1080p, às
  vezes 720p em live); o canal transmite 1080p30 a 3000 kbps.
- **Anúncios do YouTube** não vêm no fluxo, mas o vídeo pode ter música com
  reivindicação de direitos que o cliente já tem em acordo no YouTube e **não** tem para a TV.
  Vale ele conferir.
- **Fixar o dono.** O filtro de canal garante que só vídeo da lista entra; não garante
  que o conteúdo é do cliente se a lista tiver canal errado. Só o administrador geral edita.

### Recomendação
1. Vídeos: usar a importação (A). Já está pronta e é a opção segura.
2. Lives: fazer o retransmissor **com salvaguarda**: gravar a live em disco enquanto vai
   ao ar (para reprisar depois sem depender do YouTube) e sempre com o reserva pronto.
3. Começar com **uma** live de teste, no horário de menos audiência, antes de prometer ao
   cliente.

### Perguntas para o cliente
1. As lives são **agendadas com antecedência** (dá para colocar na grade) ou acontecem de
   surpresa e ele quer entrar no ar na hora? Surpresa é bem mais difícil.
2. Pode ter 10 a 30 s de atraso e a qualidade do YouTube?
3. Quando a live acaba antes ou depois do horário, o que ele prefere: voltar ao programa
   seguinte na hora, ou esperar o fim da live?
4. Ele tem os arquivos originais dos vídeos? (Se sim, importar do FTP é melhor que do
   YouTube.)
5. Ele aceita o risco do item "Termos do YouTube" acima?
