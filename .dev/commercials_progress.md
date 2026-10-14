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

## Próximo: M2 (ingestão e conversão de MXF)
