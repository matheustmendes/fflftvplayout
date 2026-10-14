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

## Próximo: grade semanal e compilador de intervalos (M3)
