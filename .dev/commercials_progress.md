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

## M4, registro de veiculação (as-run): implementada na nuvem

- Migration `00009_airlog.sql`: tabela `airlog` (previsto e veiculado na mesma
  linha), com nomes de anunciante, campanha e spot copiados para a linha, de
  modo que renomear depois não altera um relatório já gerado.
- Ciclo de uma linha de spot: `pending` (ao aplicar o plano) → `airing` (ao
  começar) → `played`, `partial`, `skipped`, `interrupted`, `fallback`,
  `failed`. Sem veiculação: `not_aired` ou `live_missed` (o horário foi
  tomado por um ao vivo). Spot inserido à mão na playlist vira linha sem
  campos de plano.
- Entradas ao vivo são registradas como janelas (`kind = live`), o que explica
  um spot que não foi ao ar.
- `utils/airlog.rs`: o `play_loop` só chama `start_tracking`, que dispara uma
  tarefa; todas as escritas no banco ficam nela. Banco lento, travado ou
  fechado atrasa o registro, nunca o ar (teste cobre isso).
- Casamento do plano com o que tocou: mesmo arquivo e `planned_begin_sec` mais
  próximo.
- Manutenção a cada 5 min: conciliação (spot vencido há mais de 2 min vira
  `not_aired` ou `live_missed`) e expurgo de 60 dias, preservando o histórico
  de campanhas encerradas há menos de 60 dias. Na partida, linhas abertas
  viram `interrupted`.
- API: `GET /api/commercials/{canal}/airlog` (operador lê), com filtros
  `date_from`, `date_to`, `campaign_id`, `advertiser_id`, `result`, `kind`,
  `limit`.
- Aplicar um plano (`POST .../plan/{data}`) regrava as promessas ainda não
  vencidas da data, sem duplicar.

Limitações e decisões:

- O mapeamento usa a data da playlist em execução; uma playlist trocada na hora
  por um operador gera linhas sem plano.
- A duração "tocada" é de relógio de parede; um spot que roda ≥ 90 % do tempo
  conta como `played`, abaixo disso `partial`.
- Relatórios CSV e PDF são a M5; a tela é a M6.

Verificação (nuvem): fmt, clippy `--deny warnings`, testes de handles (15),
domínio (classificação, rastreador, não bloqueio do ar, ao vivo, conciliação,
expurgo, partida), HTTP, e o teste real do engine (ignorado por padrão) agora
confirma que o spot aparece como `played` com horários reais.

## M5, relatórios (CSV e PDF): implementada na nuvem

- Migration `00010_report_settings.sql`: por canal, nome da emissora, dados
  (um item por linha), logotipo (PNG ou JPEG no storage, até 2 MB) e nota de
  rodapé. Nada fixo no código; sem nome configurado, usa o nome do canal.
- `utils/report.rs`: monta o relatório a partir do `airlog` (cópias de nomes,
  então renomear depois não altera relatório). Totais por dia: previstas,
  veiculadas, parciais, não veiculadas, pendentes. Horários no fuso do canal.
  Textos em pt-BR para o anunciante (o motivo interno em inglês não aparece).
  `live_missed` vira "Transmissão ao vivo no horário".
- CSV: UTF-8 com BOM, separador `;`, vírgula decimal (abre direto no Excel
  pt-BR); campos que começam com `= + - @` são neutralizados contra injeção de
  fórmula.
- PDF (`utils/report_pdf.rs`, crate `printpdf` 0.7, fonte Helvetica embutida
  que cobre acentos): logotipo e dados da emissora, filtros, resumo por dia,
  tabela de veiculações com cabeçalho repetido a cada página, notas, rodapé e
  "Página X de Y". Logotipo ilegível é omitido (o relatório sai sem ele).
- API (operador lê e baixa; só administradores alteram a configuração):
  `GET/PUT /api/commercials/{canal}/report-settings`,
  `GET .../reports/summary`, `.../reports/export.csv`, `.../reports/export.pdf`
  com `date_from`, `date_to` (obrigatórios, até 366 dias), `campaign_id`,
  `advertiser_id`.
- O relatório avisa quando o período passa da retenção de 60 dias e quando há
  spots ainda pendentes.

Limitações:

- Os textos do relatório são fixos em pt-BR (não passam pela tradução).
- Fonte Helvetica embutida: caracteres fora do Latin-1 aparecem como `?`.
- O sistema não envia e-mail: o operador baixa e envia (decisão do cliente).
- Tela de relatórios é da M6.

Verificação (nuvem): fmt, clippy `--deny warnings`, testes de relatório (totais,
fuso, CSV, PDF com texto extraído por `pdftotext`, paginação, logotipo, logotipo
quebrado) e HTTP (configuração, papéis, resumo, CSV, PDF, datas inválidas,
campanha inexistente, sem token). O PDF foi aberto e conferido visualmente.

## M6, interface (telas do módulo): implementada na nuvem

Menu "Comerciais" (rota `/commercial`), em pt-BR e en-US (de e ru caem para
o inglês), com abas:

- **Hoje:** dia a dia (anterior, próximo, hoje), avisos do plano em português
  e em destaque, intervalos do dia, o que foi ao ar (com situação), próximo
  comercial, linha do tempo e o botão "Montar este dia" (pede confirmação
  antes de substituir uma programação existente). Atualiza sozinha a cada 15 s
  quando o dia é hoje.
- **Comerciais (spots):** formulário de envio (MXF ou MP4) com barra de
  progresso, lista com "Convertendo" (barra), "Pronto" ou "Com erro" (motivo e
  botão "Tentar de novo"), remover com confirmação. O arquivo vai para a pasta
  `comerciais-entrada` do storage.
- **Campanhas:** cartões com situação (No ar, Ainda não começou, Termina em
  breve, Sem comerciais, Encerrada), assistente em 4 passos (anunciante,
  período, frequência e posição, confirmar) e gestão de anunciantes.
- **Intervalos:** por dia da semana, com soma do dia, ativar/desativar e
  "copiar para os outros dias".
- **Grade:** por dia da semana, escolher programas do storage, reordenar,
  renomear, total contra o tamanho do dia, aviso de alterações não salvas e
  "copiar para os outros dias".
- **Relatórios:** anunciante, campanha e período; resumo na tela, PDF e CSV.
- **Ajustes (administradores):** dados da emissora e logotipo do relatório;
  mapeamento de áudio e volume dos comerciais.

Papéis: o operador (`user`) vê tudo, envia comerciais e baixa relatórios; só
administradores alteram campanhas, intervalos, grade e ajustes (os botões nem
aparecem para o operador, e o servidor recusa de qualquer forma).

Verificação (nuvem): `vue-tsc`, ESLint e `vite build` limpos; todas as chaves de
texto existem em pt-BR e en-US. Com o backend real e dados de teste, um
Chromium headless percorreu todas as abas e ações (envio de comercial, criação
de campanha pelo assistente, seletor de programas e gravação da grade, resumo,
download de PDF e CSV, ajustes) sem erro de console nem resposta HTTP de erro.
As capturas foram conferidas visualmente.

Limitações:

- Não há testes automatizados de frontend no projeto; a verificação foi o
  roteiro de navegador acima (não está no repositório).
- Avisos raros do plano (arquivo sumido, dia grande demais) ainda mostram o
  detalhe em inglês vindo do servidor.
- O binário de debug não serve o frontend (só o de release); ver
  `.dev/acceptance/M6.md` para rodar com o build de release.
- Não foi testado em celular nem com um operador de verdade.

## M7, montagem automática dos próximos dias: implementada na nuvem

- Migration `00011_auto_plan.sql`: configurações por canal (ligado, dias à
  frente: 1 a 14, padrão 3) e o histórico do que foi feito em cada dia.
- `utils/auto_plan.rs`: uma tarefa em segundo plano olha todos os canais a cada
  10 minutos (a primeira vez 30 s depois de iniciar) e, do dia que está no ar
  até N dias à frente:
  - **dia sem programação:** monta e grava (usa o mesmo compilador da M3 e já
    registra as promessas do as-run da M4);
  - **dia que o próprio sistema montou:** regrava quando o plano muda (um
    comercial enviado à tarde entra na programação de amanhã);
  - **dia montado ou editado à mão, e o dia no ar:** nunca é tocado.
  O sistema reconhece o que escreveu por uma impressão digital do conteúdo
  (fontes, cortes e marca de comercial), então qualquer edição manual protege o
  dia. Um dia que não dá para montar (por exemplo, sem programas na grade)
  registra o motivo e os outros dias continuam.
- "Dia no ar" respeita o início do dia do canal (um dia que começa às 06:00 ainda
  é a data anterior às 03:00) e o fuso do canal.
- API: `GET/PUT /api/commercials/{canal}/auto-plan` (operador lê, administrador
  altera) e `POST .../auto-plan/run` ("montar agora", administrador).
- Telas: seção "Próximos dias" na aba Hoje (Montado, Atualizado, Editado à mão,
  Já existia, Não foi possível montar) e a configuração, com o botão "Montar
  agora", em Ajustes.

Limitações e decisões:

- Um dia montado pelo botão "Montar este dia" conta como "já existia" e não é
  atualizado sozinho; para voltar ao automático, apague a programação do dia.
- O motivo de um dia que falhou vem do servidor, em inglês.
- A tarefa não envia e-mail nem alerta: o aviso é a tela (Hoje) e o log. Um
  alerta ativo (e-mail) para "amanhã não está montada" é uma melhoria possível.
- A tolerância do dia no ar (não regravar) é total: se o operador quiser mudar
  o dia no ar, faz pelo painel.

Verificação (nuvem): fmt, clippy `--deny warnings` e suíte completa. Testes de
domínio: dia no ar com fuso e início do dia, impressão digital, criação dos dias,
nada muda quando nada mudou, playlist alheia preservada, comercial novo chega aos
dias do sistema mas não ao dia no ar, edição manual protegida, dia com falha
isolado, canal desligado. Testes de HTTP (papéis, validação, execução e relatório).
Conferido com servidor e navegador reais.

## Fase 1, operação confiável: feita na nuvem, falta validar no ambiente real

- **1.2 Reinício rápido:** o teto de espera do supervisor caiu de 180 s para
  **10 s** (`MAX_RESTART_DELAY`; esperas de 1, 2, 4, 8 e 10 s). Dez falhas
  seguidas somam menos de 90 s de espera. Testes cobrem a sequência, o teto e a
  volta ao início depois de uma execução longa. O `systemd` reinicia o processo
  em 1 s e nunca desiste (`StartLimitIntervalSec=0`).
- **1.3 Alertas:** um dia sem programação (o dia no ar e o seguinte, canais em
  modo playlist) gera erro nas notificações, uma vez por dia; falha ao montar o
  dia no ar ou o seguinte também. Vale mesmo com a montagem automática desligada.
- **1.1 Relay:** `deploy/relay/mediamtx.yml` (publica só de 127.0.0.1, destinos
  leem com senha por RTMP, SRT e HLS), unidades do systemd e guia em
  `deploy/README.md`.
- **1.4 HTTPS e backup:** `deploy/nginx/ffplayout.conf` (HTTPS no painel e no
  m3u8 com senha) e `deploy/backup/ffplayout-backup.py` (banco com a API de
  backup do SQLite, programações e pastas de marca, retenção e restauração).

**Achado importante (medido):** quando o ffplayout para de enviar, o MediaMTX
**desconecta todos os destinos**; eles precisam reconectar. A hipótese anterior
("os destinos continuam conectados") era falsa. O corte que o destino vê é o
reinício do ffplayout mais a reconexão dele, por isso o teto de 10 s e a
conversa com os destinos sobre reconexão automática.

Verificação (nuvem, MediaMTX compilado da branch principal): publicar e puxar
por RTMP, SRT e HLS, com e sem senha; 12 destinos simultâneos (6 RTMP e 6 SRT)
sem queda, relay a ~13 % de uma CPU e 62 MB; queda e volta do publicador;
backup (criação, retenção, restauração, erros). Suíte do projeto com os testes
novos.

Não testado: versão estável do MediaMTX, nginx e certbot, a saída real do
ffplayout (modo Stream) para o relay por horas, destinos reais e VPS real. Ver
`.dev/acceptance/F1.md`.

## Próximo: validar com a grade e os destinos reais; depois a Fase 4 (monitoramento e contingência)
