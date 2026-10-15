# Agendador e controle de horário

Análise do código de agendamento da playlist e da sincronização com o
relógio. Referências no formato `arquivo:linha` (estado de outubro de 2026,
base `d1eaf84`).

## 1. Modelo mental

- Não existe evento com hora marcada. Uma playlist JSON por dia é uma lista
  ordenada de clipes com `in`, `out` e `duration`. O horário de cada clipe é
  **derivado**: começa em `day_start` e soma `out - in` dos anteriores
  (`set_defaults`, `backend/app/src/player/utils/json_serializer.rs:76`).
- Esse horário fica em `Media.begin`, em segundos desde a meia-noite do fuso
  do canal. Pode passar de 86400 quando o dia de playlist atravessa a
  meia-noite (por exemplo, com `day_start` às 06:00).
- O playout é **pull-based**: o app pede um clipe ao iterador, entrega ao
  engine e espera ele terminar. O engine cadencia a saída em tempo real.
  A cada clipe o app compara o `begin` previsto com o relógio e decide se
  está sincronizado.
- O "agora" sempre vem de `time_now()` (`backend/app/src/utils/time_machine.rs:53`),
  que respeita o fuso do canal e o argumento oculto `--fake-time`, útil para
  testar viradas de dia e mudanças de horário de verão.

## 2. Configuração relevante

| Campo | Padrão | Onde | Efeito |
|---|---|---|---|
| `playlist.day_start` | `05:59:25` | `config_playlist` | Início do dia de playlist. Vira `start_sec`. |
| `playlist.length` | `24:00:00` | `config_playlist` | Duração do dia. Vira `length_sec`. Sem `:`, vale 86400 e a checagem de sincronização é **desligada**. |
| `playlist.infinit` | `false` | `config_playlist` | Repete a mesma playlist sem trocar de dia. |
| `general.stop_threshold` | `11.0` s | `config_general` | Atraso/adiantamento tolerado antes de ressincronizar. `0` desliga. |
| `channel.timezone` | fuso do SO | `channels` | Fuso usado em todos os cálculos de horário. |
| `storage.filler` | — | `config_storage` | Arquivo ou pasta usada para tapar buracos. |
| `channels.time_shift` / `last_date` | — | tabela `channels` | Deslocamento manual persistido (next/back) e data da playlist em uso. |

A conversão está em `backend/app/src/utils/config.rs:1308`. Constantes
fixas: `DUMMY_LEN = 60 s` (`utils/config.rs:23`), imagem padrão de 10 s e
fonte ao vivo padrão de 60 s (`player/utils/mod.rs:44-48`).

## 3. Cadeia de execução

```
ChannelManager::start            player/controller.rs:279
  └─ supervisor_loop             player/controller.rs:648  (reinicia com backoff até 180 s)
      └─ run_channel             player/controller.rs:769  (carrega lista de fillers)
          └─ player()            player/output/playout.rs:37 (abre o engine)
              └─ play_loop       player/output/playout.rs:187
                  ├─ source_generator → CurrentProgram | FolderSource   player/input/mod.rs:28
                  ├─ node = iterator.next()
                  ├─ hls_playout_rate(...)                             só no modo HLS
                  └─ AsyncPlayout::play_with_timing_logo_fade_rate_and_audio(...)
                      (bloqueia até o clipe terminar, ser pulado ou cair no fallback)
```

O estado compartilhado fica no `ChannelManager` (`player/controller.rs:87`):
`current_list`, `current_index`, `list_init`, `current_date`, `channel`
(com `time_shift` e `last_date`) e `playback_control`.

## 4. O iterador da playlist (`CurrentProgram::next`)

Arquivo: `backend/app/src/player/input/playlist.rs`.

1. Lê `list_init` **antes** de chamar `check_for_playlist` (`:777`).
2. `check_for_playlist` (`:195`) decide se é hora da playlist do dia
   seguinte. Isso acontece quando o próximo início passa de `length_sec`,
   quando `total_delta` está a menos de 2 s de 0 ou de `length_sec`, ou
   quando a data mudou. Se for o caso, carrega o novo JSON, zera
   `time_shift` e o índice. Caso contrário chama `load_or_update_playlist`,
   que recarrega o arquivo se o mtime mudou.
3. Se `list_init` era verdadeiro, chama `handle_playlist_init` (`:740`).
   Ele busca no relógio qual clipe deveria estar tocando agora e começa
   nele, com seek.
4. Caso contrário pega `current_list[current_index]` e passa por
   `timed_source` (`:496`), a checagem de sincronização.
5. Se a lista acabou antes do fim do dia (`total_delta > 1 s`), `fill_end`
   (`:404`) gera um filler do tamanho que falta.
6. Se a lista acabou e já é outro dia, começa pelo primeiro clipe da nova
   lista.

### Partida no meio da grade (init/reload/reset)

- `get_current_clip` (`:321`): pega o horário atual (soma 86400 se estiver
  antes de `day_start`), soma `time_shift` e escolhe o primeiro clipe cujo
  fim (`begin + out - in`) é maior que agora.
- `init_clip` (`:363`): calcula `elapsed = agora - (begin - time_shift)` e
  aplica `apply_init_seek` (`:840`). Arquivos recebem seek. Imagens e fontes
  ao vivo encurtam o `out`.
- `handle_list_init` (`:457`): corta o clipe para não passar do fim do dia.

### Sincronização (`timed_source`)

- `get_delta` (`player/utils/mod.rs:587`) devolve:
  - `delta = begin - agora`: positivo significa playout adiantado (o clipe
    estava previsto para depois); negativo significa atrasado.
  - `total_delta`: tempo que falta até o fim do dia de playlist.
  - Trata a virada da meia-noite com tolerância de `stop_threshold + 2`.
- Só verifica se `playlist.length` contém `:`. Com `time_shift` ativo no
  mesmo dia, usa `delta - time_shift`.
- Se `|delta| > stop_threshold`:
  - se `|delta|` está perto de 3600 s, assume mudança de horário de verão e
    grava `time_shift` (em vez de pular);
  - senão registra "Clip begin out of sync", zera `time_shift`, marca
    `list_init` e **pula** o clipe. Na próxima chamada o iterador busca o
    clipe certo pelo relógio e entra nele com seek. Isso é um corte visível
    na saída.
- Os dois primeiros clipes (`index < 2`), o modo `infinit` e o
  `length` sem `:` sempre tocam sem checagem.
- O último clipe do dia passa por `handle_list_end` (`:470`), que corta ou
  estica o `out` para fechar exatamente no fim do dia (tolerância de 0,5 s).
  Se não der, registra "Playlist is not long enough".

### Clipes ausentes e fillers

`gen_source` (`:573`):

- Pula clipes com menos de 1 s.
- Se a mídia não existe ou o probe falha, usa um filler da pasta (em
  rodízio), ou o arquivo de filler, ou gera "dummy" (preto + silêncio) de no
  máximo 60 s. Também marca `list_init`, então o próximo clipe é
  ressincronizado pelo relógio.

## 5. Edição da playlist com o canal no ar

- A cada `next()`, `load_or_update_playlist` compara o mtime do JSON (ou o
  `Last-Modified` de uma playlist remota).
- Se mudou, recarrega e marca `list_init`. O efeito só aparece **na fronteira
  de clipe**. Como `list_init` foi lido antes do reload, o próximo clipe ainda
  sai pelo índice da lista nova, e o seguinte é ressincronizado pelo relógio.
- Consequência: num clipe longo (filme de 2 h), uma edição na grade só tem
  efeito quando ele termina.

## 6. Comandos de controle (next/back/reset)

`control_state` (`backend/app/src/utils/control.rs:114`):

- **Next**: `time_shift = delta` do próximo clipe (`begin - agora`), ou
  seja, a grade inteira passa a correr adiantada. Em seguida pede
  navegação ao engine, que interrompe o clipe atual (`take_navigation` →
  `PlaybackSkipped`).
- **Back**: índice volta 2 posições e `time_shift` fica negativo.
- **Reset**: `time_shift = 0` e `list_init`. Volta à grade original.
- `time_shift` é gravado no banco junto com `last_date`. Sobrevive a
  reinício e é zerado na troca de dia (`set_status`, `:269`).
- Durante live ingest, os comandos são ignorados (`NavigationBlocked::Live`).

## 7. Live ingest (RTMP/SRT)

- Os listeners rodam em paralelo com a playlist. Quando um sinal entra, ele
  assume a saída (`LiveOverrideOutput`) e o decoder da playlist fica
  suspenso.
- Quando o sinal termina, o engine devolve `ClipResult::LiveEnded` e o app
  marca `list_init` e recria o iterador (`player/output/playout.rs:270`).
  A playlist volta **no ponto que o relógio indica**, não onde parou: o tempo
  do ao vivo "consome" a grade.

## 8. Cadência no engine

- `PlayoutClock` (`backend/engine/src/utils/clock.rs`) segura cada pacote
  até `anchor_wall + (pts - anchor_pts) / rate`. É chamado em
  `write_packet` (`backend/engine/src/output/encoded.rs:861`), para saídas
  Stream e HLS. A gravação não é cadenciada.
- `set_rate` faz o relógio se reancorar no próximo pacote. Ele é chamado em
  **todo** clipe (`Playout::play_timed_with_live`, `engine/src/lib.rs:748`).
- **HLS**: o app calcula uma correção de velocidade por clipe
  (`hls_rate_correction`, `player/output/playout.rs:352`). Com atraso de
  0,1 s a 2 s, a velocidade fica entre 0,98 e 1,02 para recuperar o horário
  ao longo do clipe. Atrasos maiores que a duração do clipe são ignorados.
- **Stream (RTMP etc.)**: rate fixo em 1.0. Não há correção suave, só a
  ressincronização brusca de `stop_threshold`.
- **Desktop**: usa o áudio como relógio mestre (`AudioMasterClock`,
  `engine/src/output/desktop/timing.rs`).

## 9. Modo pasta, gerador e EPG

- **Modo pasta** (`player/input/folder.rs`): sem grade. Toca em ordem
  alfabética ou embaralhado, em loop. `begin` = agora, sem checagem de
  sincronização.
- **Gerador** (`utils/generator.rs`): cria playlists por data a partir de
  pastas ou de um template. No template, cada bloco usa só `duration`.
  O campo `start` é **ignorado**: os blocos são concatenados em sequência.
  Com template, a mesma lista gerada é gravada em todas as datas do
  intervalo.
- **EPG / programa** (`utils/program.rs:35`): recalcula os horários lendo
  os JSON (`day_start` + soma das durações) e devolve itens com data/hora
  no fuso do canal. Não considera `time_shift` nem o atraso real do ar.

## 10. Observações e pontos de atenção

Itens encontrados na leitura. Os marcados como *hipótese* precisam ser
confirmados com o playout rodando.

1. **Sem horário fixo.** *(Resolvido na fase 2.2: ver `.dev/roadmap.md`.)* Um programa que precisa entrar às 20:00 em ponto só
   entra no horário se a soma das durações anteriores estiver exata. Não há
   tipo de item "hard start" nem "fill até". É a maior lacuna para uma grade
   de TV tradicional.
2. **Deriva no modo Stream** (*hipótese*). Como o `PlayoutClock` se reancora
   em cada clipe, o tempo gasto abrindo o próximo arquivo não é compensado. O
   atraso tende a crescer até passar de `stop_threshold` (11 s) e provocar um
   corte de ressincronização. No HLS isso é amortecido pela correção de
   velocidade. Para medir: logs em nível `debug` ("Delta: …").
3. **Ressincronização é um corte.** Ao passar do limite, o clipe é pulado e o
   seguinte entra com seek. Não existe recuperação gradual fora do HLS.
4. **Edição só na fronteira do clipe** (seção 5).
5. **`start` do template ignorado** e mesma lista repetida em todas as datas
   (seção 9).
6. **EPG não reflete `time_shift`** nem a deriva real.
7. **Constantes fixas** espalhadas: 86400 em `get_current_time` mesmo com
   `length` diferente de 24 h, 86397,9 em `get_date`, limiares de 1,5 s, 2 s
   e 0,5 s no início de `playlist.rs`.
8. **Horário de verão** é detectado por heurística (delta próximo de 3600 s);
   o próprio comentário do código diz que precisa ser validado em produção.
9. Vários `unwrap()` em `start_sec` e `begin` em caminhos de produção.

## 11. Onde mexer para cada tipo de mudança

| Objetivo | Arquivos |
|---|---|
| Itens com horário fixo / blocos de grade | `player/utils/json_serializer.rs` (`set_defaults`), `player/utils/mod.rs` (`Media`), `player/input/playlist.rs` (`timed_source`, `handle_list_end`), editor no frontend |
| Política de sincronização (limiar, correção suave) | `player/input/playlist.rs` (`timed_source`), `player/output/playout.rs` (`hls_rate_correction`), `engine/src/utils/clock.rs` |
| Correção de deriva no Stream | `engine/src/utils/clock.rs`, `engine/src/lib.rs:748`, `player/output/playout.rs` |
| Virada de dia, `day_start`, `length` | `player/input/playlist.rs` (`check_for_playlist`), `player/utils/mod.rs` (`get_date`, `get_delta`) |
| Comandos de operador | `utils/control.rs`, `engine/src/playback_control.rs`, rotas em `api/routes/control.rs` |
| Geração automática de grade | `utils/generator.rs` |
| EPG | `utils/program.rs` |

Para testar cenários de horário sem esperar: `ffplayout --fake-time
2026-10-14T23:59:30-03:00`. Nos testes unitários, use `with_mock_time`
(`utils/time_machine.rs:79`).
