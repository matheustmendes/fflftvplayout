# Módulo de comerciais: andamento

Registro do que já foi implementado e verificado. Desenho em
`.dev/commercials_design.md`; decisões do cliente em `.dev/roadmap.md`.

## M1: fundação (concluída)

Implementado:

- `migrations/00006_commercials.sql`: `advertiser`, `campaign`, `spot`,
  `break_window`, com restrições no banco (datas coerentes, pesos e limites
  positivos, horários `HH:MM:SS`, nomes de anunciante únicos entre os ativos,
  um arquivo ativo por spot e canal).
- `db/models.rs`: modelos e validações (`Advertiser`, `Campaign`, `Spot`,
  `BreakWindow`, enums `CampaignKind`, `BreakPosition`, `SpotStatus`).
- `db/handles/commercial.rs`: todas as queries, sempre filtradas por canal.
  Arquivar (nunca apagar) anunciante, campanha e spot, em cascata.
- `utils/commercial.rs`: registro de um spot a partir de um arquivo do
  storage, com verificação por probe (precisa de vídeo, áudio e duração).
  Spot inválido é guardado como `failed` com o motivo.
- `api/routes/commercials.rs` e rotas em `api/path.rs`:

| Método e rota (`/api/commercials/{canal}/...`) | Quem |
|---|---|
| `GET/POST advertisers`, `PUT/DELETE advertisers/{id}` | leitura: operador e admins; escrita: admins |
| `GET/POST campaigns`, `PUT/DELETE campaigns/{id}` | idem |
| `GET/POST spots`, `DELETE spots/{id}` | leitura e cadastro: operador e admins; arquivar: admins |
| `GET/POST break-windows`, `PUT/DELETE break-windows/{id}` | leitura: todos; escrita: admins |

Verificação (ambiente de nuvem, FFmpeg 7.1.1):

- `cargo test -p ffplayout --lib -- commercial`: 24 testes (validações,
  queries, registro de spot com arquivos reais de `tests_assets/`).
- `cargo test -p ffplayout --test api_routes -- commercials`: 8 testes HTTP
  (autenticação, papéis, isolamento entre canais, ciclo de vida, conflitos).

Limitações conhecidas da M1 (previstas no plano):

- O spot só aceita um arquivo **já presente** no storage e tocável pelo
  engine. MXF e conversão automática são a M2; o upload ainda rejeita `.mxf`.
- Mensagens de erro do spot (`error`) estão em inglês; a tradução para a tela
  do operador fica para a M6, mapeando as mensagens conhecidas.
- Não há campo de anunciante no `spot` nem rodízio: isso é da M3.

## M2: upload e conversão (concluída, sem a amostra real de MXF)

Implementado:

- `migrations/00007_commercial_settings.sql`: tabela `commercial_settings`
  (por canal) e, em `spot`, as colunas `source_path` (o arquivo enviado) e
  `progress` (0 a 100).
- Upload: `application/mxf` entrou na lista de tipos aceitos
  (`file/upload.rs`). Antes, um `.mxf` era recusado.
- `utils/spot_convert.rs`: conversão com o `ffmpeg` (precisa estar instalado,
  com `libx264`). Saída: mp4 H.264/AAC no tamanho e na taxa de quadros da
  saída do canal, vídeo ajustado sem distorção (inclui fontes anamórficas) e
  quadros entrelaçados desentrelaçados; áudio estéreo a 48 kHz. Volume em
  duas passagens (mede e depois aplica ganho fixo, o que evita "bombeamento"
  em spots curtos). Escreve em arquivo temporário e só então move para o
  destino. Roda uma conversão por vez e com prioridade baixa (`nice`).
  Recusa áudio mudo e arquivos sem vídeo ou sem áudio.
- `utils/commercial.rs`: cadastro devolve o spot em `processing`; a conversão
  roda em segundo plano e grava o resultado. Nova tentativa para spot com
  falha.
- Varredura na inicialização (`main.rs`): conversões interrompidas por
  reinício viram `failed` com mensagem, em vez de ficar "processando" para
  sempre.

**Mapeamento de áudio (configurável, `commercial_settings.audio_mapping`):**

| Valor | Comportamento |
|---|---|
| `auto` (padrão) | uma trilha: usa ela; várias trilhas **todas mono** (típico de MXF): as duas primeiras como esquerda e direita; senão a primeira |
| `first_stream` | só a primeira trilha, em estéreo |
| `first_two_mono` | as duas primeiras trilhas como esquerda e direita (erro se houver só uma) |
| `mix_all` | todas as trilhas misturadas nos dois canais |

Volume: `loudness_enabled` (padrão ligado), `loudness_target_lufs` (padrão
-24,0) e `loudness_true_peak_dbtp` (padrão -2,0). O teto de pico verdadeiro
de -2 dBTP é um valor de partida a conferir com a norma.

Rotas novas (`/api/commercials/{canal}/...`):

| Rota | Quem |
|---|---|
| `POST spots` agora responde `202` com o spot em `processing` | operador e admins |
| `POST spots/{id}/retry` | operador e admins |
| `GET settings` | operador e admins |
| `PUT settings` | admins |

Verificação (nuvem, FFmpeg 7.1.1):

- Conversão real de um MXF gerado (MPEG-2 + duas trilhas mono PCM): formato
  de saída, volume dentro de 1,5 LU do alvo, áudio mudo recusado, arquivo
  corrompido recusado, nenhum arquivo parcial deixado para trás.
- O mapeamento foi verificado **pelo conteúdo do áudio**: com `auto` e
  `first_two_mono` os canais esquerdo e direito da saída diferem; com
  `first_stream` e `mix_all` os dois canais são iguais.
- Fluxo HTTP completo (MXF enviado, `202`, acompanhamento até `ready`, arquivo
  convertido no disco, nova tentativa, permissões e isolamento entre canais).

**Limitação conhecida e pendência:** os MXF usados nos testes são gerados por
mim. Os arquivos reais da emissora (codecs como XDCAM, IMX ou DNxHD, layouts
de áudio, timecode) podem se comportar de outra forma. Quando houver uma
amostra real, rodar a conversão com ela e, se necessário, ajustar o
mapeamento padrão. Enquanto isso, o operador pode trocar o mapeamento nas
configurações e usar a nova tentativa.

Observações:

- Os arquivos convertidos ficam em `commercials/spot-<id>.mp4` dentro do
  storage do canal; o enviado é mantido.
- Para o arquivo `.mxf` aparecer na navegação de mídia do canal, a extensão
  precisa constar em `storage.extensions` (padrão `mp4;mkv;webm`). O cadastro
  do spot pela API aceita o caminho de qualquer extensão.
- A mensagem de erro do spot segue em inglês; a tradução é da M6.

### Roteiro de aceite e teste de fumaça da M2

- `.dev/acceptance/M2.md`: passo a passo para rodar na máquina local com um
  MXF real.
- `scripts/dev/commercials_smoke.py`: teste de ponta a ponta pela API (só
  biblioteca padrão do Python). Envia o arquivo pela API de upload, registra o
  spot, acompanha a conversão e, com `--storage`, analisa a saída (formato,
  volume, esquerdo × direito). Remove o que criou e restaura as
  configurações.
- `.dev/local-session-brief.md`: briefing para colar no Claude Code local.

O script foi **testado aqui contra um servidor real** (binário compilado,
setup pela API, MXF de duas trilhas mono): passou 17 verificações com o
mapeamento `auto` (esquerdo e direito diferentes) e com `mix_all` (canais
iguais, configuração restaurada), e falhou do jeito esperado, com mensagem
clara e código de saída 1, com áudio mudo.

Três achados do teste que corrigiram o roteiro e o script:

- `ffplayout --init` não funciona sem terminal interativo ("The input device
  is not a TTY"); o roteiro usa o setup pela tela ou por `POST /api/setup`.
- O servidor recusa pastas em `/tmp`, `/root`, `/var`, `/usr` e similares.
- O login é limitado a 3 por minuto por endereço (resposta 429); o script
  agora espera e tenta de novo sozinho.

## M3: grade semanal e compilador de intervalos (concluída, falta validar com a grade real)

Implementado:

- `migrations/00008_week_template.sql`: `week_template_item` (programas de
  cada dia da semana, na ordem de exibição).
- `utils/break_planner.rs`: o compilador, como função **pura e
  determinística** (sem banco, relógio ou arquivos). Regras:
  - cada intervalo vai para a fronteira entre programas mais próxima do
    horário pedido, contando os intervalos já colocados antes (que empurram
    as fronteiras seguintes); empate fica com a fronteira mais cedo; dois
    intervalos nunca dividem a mesma fronteira; horários antes do início do
    dia contam como o fim do dia;
  - preenchimento por rodízio ponderado pelo peso, com sorteio **semeado por
    data e intervalo** (a mesma data dá sempre o mesmo resultado, e o gerador
    é próprio para não mudar com versão de biblioteca);
  - uma campanha aparece no máximo uma vez por intervalo e nunca passa do
    limite por dia (contado em todos os intervalos do dia); "primeiro" e
    "último" lugar respeitados (o "último" reserva o espaço antes do
    rodízio); mesmo anunciante não fica colado quando há alternativa;
  - o que os spots não cobrem vira enchimento (arquivo de filler do canal, ou
    preto e silêncio); sobra menor que 1 s é ignorada;
  - o dia é completado com enchimento no fim se faltar, e **bloqueado** se
    passar mais de 5 s (de 1 a 5 s é só aviso, pois o playout corta o último
    clipe).
- `utils/day_plan.rs`: monta a entrada a partir do banco e do storage
  (grade do dia da semana, janelas ativas, spots prontos das campanhas em
  vigor na data), confere se os arquivos existem (spots e programas), roda o
  compilador e grava a playlist no formato e caminho que o engine já lê. Uma
  playlist existente só é substituída com confirmação; um plano com erro
  nunca é gravado. Spots e enchimento de intervalo saem marcados como
  publicidade, o que faz o playout esconder o logo.
- Rotas (`/api/commercials/{canal}/...`):

| Rota | Quem |
|---|---|
| `GET week-template[?weekday=N]` | operador e admins |
| `PUT week-template/{weekday}` | admins. Só o arquivo é obrigatório; a duração é lida do arquivo |
| `GET plan/{AAAA-MM-DD}` | operador e admins: prévia, sem gravar |
| `POST plan/{AAAA-MM-DD}[?overwrite=true]` | admins: grava a playlist da data |

- `scripts/dev/commercials_plan_preview.py`: linha do tempo legível do dia,
  com horários, intervalos e avisos; `--apply` grava.
- `.dev/acceptance/M3.md`: roteiro de aceite com a grade real.

Verificação (nuvem, FFmpeg 7.1.1):

- `cargo fmt`, `cargo clippy --all-features --all-targets -- --deny warnings`
  e `cargo test --workspace` (363 do engine, 326 do app e 22 de rotas, sem
  falhas).
- Compilador: 33 testes (fronteiras, empates, deslocamento por intervalos
  anteriores, limite por dia, primeiro e último lugar, anunciante, rodízio,
  determinismo, variação entre datas, pesos, fechamento do dia, horários
  antes do início do dia).
- Orquestração: 15 testes com banco e arquivos reais.
- HTTP: 15 testes, incluindo o fluxo completo (MXF enviado, convertido, grade,
  janela, prévia, gravação, bloqueio de sobrescrita, papéis e canais).
- **Reprodução real:** `tests/playout_commercials.rs` (ignorado por padrão,
  roda em tempo real, 33 s). O engine de verdade tocou o dia planejado na
  ordem certa: spot, primeiro programa, segundo programa.
- Ferramenta de prévia testada contra um servidor real, com dois spots
  convertidos de MXF: o "primeiro lugar" abriu cada intervalo e o segundo
  intervalo foi para a fronteira mais próxima.

Limitações conhecidas e decisões a confirmar com o cliente:

- A grade é cadastrada por JSON; a tela é da M6.
- O enchimento usa o **arquivo** de filler configurado; se o filler for uma
  pasta, o intervalo usa preto e silêncio.
- A duração de cada programa é gravada quando entra na grade. Se o arquivo for
  trocado por outro de duração diferente, o plano continua usando a antiga
  (o engine ajusta e a validação da playlist avisa).
- Um programa cujo arquivo sumiu **permanece** no dia (o playout preenche o
  tempo com o filler) e aparece como aviso `program_file_missing`.
- O limite por dia conta o que foi **planejado** no dia, não o que foi ao ar
  (o registro de veiculação é a M4).
- A tolerância do dia (5 s) e a de cada intervalo (60 s por padrão, por janela)
  estão definidas assim; ver as perguntas em `.dev/acceptance/M3.md`.
- Ainda não há geração automática dos próximos dias (M7).

## Próximo: M4, registro de veiculação (as-run)
