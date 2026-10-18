# Desenho técnico: módulo de comerciais, campanhas e relatórios

Complementa `.dev/roadmap.md` (decisões do cliente) e `.dev/scheduler.md`
(como o agendador funciona). Este documento é uma **proposta**: nada aqui está
implementado. Legenda: **[verificado]** = confirmado lendo o código;
**[hipótese]** = precisa de teste.

## 1. Escopo

Dentro:

- Cadastro de anunciantes, campanhas e spots; upload pelo operador com
  conversão automática (MXF e outros → mp4 padronizado).
- Janelas de intervalo em horário fixo, por dia da semana.
- Compilador que gera a playlist do dia (grade + intervalos preenchidos).
- Registro do que realmente foi ao ar (as-run) e relatórios em CSV e PDF para
  os anunciantes.
- Grade-modelo semanal que alimenta o compilador.

Fora (decisões do cliente):

- Intervalos durante o ao vivo (roadmap 6.2).
- Mudanças no engine de vídeo. Todo o módulo vive em `backend/app/` e no
  frontend; o engine continua recebendo uma playlist comum.

## 2. Fluxo ponta a ponta

```
Operador envia arquivo ──► upload (chunked, existente) ──► conversão ──► spot "pronto"
                                                                           │
Anunciante ► Campanha (início, fim, limite/dia, posição) ► spots ──────────┤
                                                                           ▼
Grade-modelo semanal ─┐                                         Compilador (dia D)
Janelas de intervalo ─┴──────────────────────────────────────►  playlist JSON + plano
                                                                           │
                                                                           ▼
                                   engine toca (sem mudanças) ──► as-run (banco)
                                                                           │
                                                                           ▼
                                                        Relatórios CSV / PDF
```

## 3. Dados (migration `00006_commercials.sql`)

> **Estado (M1 implementada):** a migration `00006` cria `advertiser`,
> `campaign`, `spot` e `break_window`. As tabelas `week_template_item` e
> `airlog` ficam para migrations próprias (`00007`, `00008`), junto das etapas
> que as usam (M3 e M4), para o esquema ser decidido com o código em mãos.
> `spot` ganhou o campo `active` (arquivamento) e `channel_id` próprio, que
> facilita o isolamento por canal; a consistência canal/campanha é garantida
> nas queries de inserção.


A última migration existente é a `00005` [verificado]; não editar as
anteriores. Todas as tabelas levam `channel_id` (o projeto suporta vários
canais).

| Tabela | Campos principais |
|---|---|
| `advertiser` | id, channel_id, name, contact_name, email, notes, active |
| `campaign` | id, channel_id, advertiser_id, name, **kind** (`campaign` \| `standalone`), start_date, end_date (nulos em `standalone`), active, weight, max_per_day (nulo = sem limite), position (`any` \| `first` \| `last`) |
| `spot` | id, campaign_id, title, file_path (relativo ao storage), original_filename, duration_sec, status (`processing` \| `ready` \| `failed`), error, created_by, created_at |
| `break_window` | id, channel_id, weekday (1 a 7), nominal_time (`HH:MM:SS`), duration_sec, tolerance_sec, enabled |
| `week_template_item` | id, channel_id, weekday, position, kind (`program` \| `filler`), source, in_sec, out_sec, duration_sec, title |
| `airlog` | ver 3.1 |

- Spot avulso = campanha `standalone` com um spot. Todo spot pertence a
  exatamente uma campanha: simplifica o rodízio e os relatórios.
- Exclusão: spot ou campanha já veiculados são **arquivados** (`active = 0`),
  não apagados, para o histórico não se perder.

### 3.1 `airlog` (previsto e veiculado na mesma tabela)

Uma linha por item do intervalo (e, em etapa posterior, por programa).

| Campo | Significado |
|---|---|
| channel_id, air_date | canal e data da programação |
| kind | `spot` \| `program` \| `filler` \| `live` |
| break_window_id, position_in_break | de qual janela e em que posição |
| spot_id, campaign_id, advertiser_id | referências |
| advertiser_name, campaign_name, spot_title, file_path | **cópia** do momento da compilação, para o relatório não mudar se alguém renomear depois |
| planned_start, planned_duration_sec | gravados na compilação |
| actual_start, actual_end | gravados no ar |
| result | `pending` \| `played` \| `skipped` \| `fallback` \| `not_aired` \| `live_missed` |
| reason | texto curto (ex.: "ao vivo no horário") |

Regra de contagem: só `played` com duração real ≥ 95% da prevista conta como
**veiculado**. `skipped` (operador pulou) e `fallback` (arquivo falhou e o
engine tocou filler) **não** contam. Fusos: horários guardados em UTC e
exibidos no fuso do canal.

**Retenção:** 60 dias (decisão do cliente), com uma exceção proposta: os
registros de uma campanha são mantidos até o **fim da campanha + 60 dias**,
para que o relatório final de uma campanha longa não perca o começo. Uma
tarefa periódica apaga o que passou do prazo, e o relatório avisa quando o
período pedido ultrapassa o que ainda está guardado.

## 4. Compilador de intervalos

Módulo de domínio em `backend/app/src/utils/` (por exemplo
`break_planner.rs`), sem acesso a SQL (o AGENTS.md exige queries só em
`db/handles/`). É uma função **pura** e determinística:

```
entrada: data, fuso, programas do dia (da grade-modelo, com duração),
         janelas de intervalo do dia da semana, campanhas elegíveis,
         contagem de exibições já planejadas no dia
saída:   playlist (Vec<Media>), linhas de plano (airlog pending), avisos
```

Passos:

1. Calcular o início de cada programa a partir de `day_start` e das durações
   (como `set_defaults` faz hoje, `json_serializer.rs:76`).
2. Para cada janela, achar a **fronteira entre programas mais próxima** do
   horário nominal (regra aprovada, roadmap 6.1).
3. Se a distância passar de `tolerance_sec`: ainda posicionar, mas gerar
   **aviso** ("intervalo das 20:00 cai 4 min depois do fim do programa X").
4. Escolher os spots:
   - campanhas ativas na data, dentro de `[start_date, end_date]`, com spots
     `ready`;
   - descartar as que já atingiram `max_per_day`;
   - respeitar `position` (`first`/`last`) antes do rodízio;
   - rodízio ponderado por `weight`, **semente fixa** (data + janela) para que
     recompilar dê o mesmo resultado;
   - não repetir o mesmo anunciante em sequência quando houver alternativa.
5. Fechar o intervalo: a duração da janela é uma **meta**, não um teto. Um
   comercial sempre vai ao ar inteiro. Acrescentar spots enquanto couberem; se
   nenhum couber, aceitar um que ultrapasse a meta em no máximo o tempo que
   faltava (o bloco fica o mais perto possível dela). Campanha com posição
   `first`/`last` sempre entra. Se o bloco passar da meta, o programa seguinte
   começa depois; se faltar pouco, completar com filler/vinheta
   (`storage.filler`). Programa com hora marcada reduz a meta do intervalo
   anterior.
6. Inserir os itens com `ad = true` (isso já aciona o fade do logo, 
   `player/output/playout.rs:253` [verificado]).
7. Validar o total do dia: se a soma diferir do tamanho do dia, mostrar o
   desvio em linguagem simples ("a grade tem 24h02min; passa 2 min"). Os
   intervalos acrescentam tempo, então o dia só fecha se a grade-modelo já os
   considerar. A geração fica **bloqueada** acima de uma tolerância
   configurável, e completa com filler abaixo dela.

A playlist sai no formato JSON atual e é gravada pelo caminho existente
(`utils/playlist.rs`). **Não é preciso alterar `Media`**: na hora de gravar o
as-run, o app identifica o spot pelo caminho do arquivo (único), então o
formato da playlist não muda.

Cenários de teste unitário (relógio e fuso explícitos, como o AGENTS.md pede):
fronteira mais próxima, empate, aviso de tolerância, limite por dia,
`first`/`last`, anunciante não repetido, janela preenchida exatamente, spot
maior que a janela, campanha no primeiro e no último dia, nenhuma campanha
elegível, dia com mudança de horário de verão.

### 4.1 Quando compilar

- Sob demanda (botão "Gerar dia"), com **pré-visualização** e avisos antes de
  gravar.
- Automático: tarefa em segundo plano que gera os próximos N dias (padrão 7)
  e alerta se algum falhar. Reaproveita o que já existe em
  `utils/generator.rs` / `utils/playlist.rs` (`generate_playlist`).
- Recompilar um dia só substitui linhas `pending` **futuras**. O que já foi ao
  ar nunca é reescrito.
- Se o operador editar a playlist depois, o plano do dia fica desatualizado:
  a tela deve avisar e oferecer "recompilar".

## 5. Registro de veiculação (as-run)

Ponto de gravação: `play_loop` em `player/output/playout.rs`, que recebe o
`ClipResult` de cada clipe [verificado: `Played`, `Skipped`, `LiveEnded`,
`Fallback { reason }`, `Stopped`].

- Antes de tocar um item que é spot (identificado pelo caminho): guardar
  `actual_start`.
- Depois: `Played` → `played`; `Skipped` → `skipped`; `Fallback` → `fallback`
  com o motivo; `Stopped`/falha → não marcar veiculado.
- A escrita é feita em tarefa separada (`tokio::spawn`), com erro apenas
  registrado em log. **Nunca pode atrasar nem derrubar o ar.** Se o banco
  falhar, o item fica `pending` e a conciliação resolve depois.
- **Conciliação** (tarefa periódica e na virada do dia): linhas `pending` cujo
  horário passou viram `not_aired` ou `live_missed`. Para saber se houve ao
  vivo naquele horário é preciso registrar as janelas de ao vivo
  (início/fim). O engine expõe `PlaybackControl::live_status()`
  [verificado], mas a melhor forma de capturar o início e o fim do ao vivo
  ainda precisa ser investigada [hipótese].

## 6. Upload e conversão (MXF)

Achados [verificados]:

- O upload valida o tipo **só pela extensão**, via `mime_guess`
  (`file/upload.rs:106`), contra uma lista fixa (`ALLOWED_MIME_TYPES`). A lista
  **não** contém `application/mxf` nem `application/octet-stream`; portanto um
  `.mxf` é rejeitado hoje. `video/x-matroska` (mkv) e MPEG/TS também não
  estão na lista.
- O limite por arquivo é 4 GB (`MAX_UPLOAD_SIZE`) e o envio é em pedaços de
  10 MB, com retomada.
- O engine lê qualquer formato que o FFmpeg demultiplexe, mas escolhe **uma
  única trilha de áudio** (`streams.best(Audio)`, `engine/src/playout.rs`)
  [hipótese de problema: MXF com várias trilhas mono].

Proposta:

1. Permitir `.mxf` no upload da área de comerciais (ajuste da lista, com
   teste).
2. Ao terminar o upload, criar o spot com `status = processing` e enfileirar a
   **conversão**:
   - saída mp4, H.264, AAC estéreo, **1080p a 30 fps** (formato do canal);
   - áudio mono múltiplo: mapear as duas primeiras trilhas para estéreo, ou
     misturar todas; **decidir depois de inspecionar amostras reais**;
   - normalização de volume (loudness) seguindo a **norma brasileira**, com o
     alvo **configurável** por canal. Valor padrão proposto: −24 LKFS
     (ABNT NBR 15602, a conferir no texto da norma antes de fixar o padrão);
   - execução com o binário `ffmpeg` (já é requisito do projeto), com
     prioridade baixa para não competir com o ar.
3. Validar o resultado: duração lida pelo `probe_media`, presença de áudio,
   não silencioso (já existe `detect_audio_silence`), duração coerente com o
   original.
4. Atualizar `status` para `ready` ou `failed` com mensagem **legível**
   ("o arquivo não tem áudio"). O progresso aparece na tela por SSE (já
   existe o módulo `sse/`).
5. Só spots `ready` entram no compilador. O arquivo original é mantido até o
   operador confirmar.

## 7. API (REST)

Rotas em `backend/app/src/api/routes/commercials.rs`, com lógica em módulos
de domínio e SQL em `db/handles/commercial.rs` e `db/handles/airlog.rs`;
modelos em `db/models.rs`. Papéis existentes [verificado]: `GlobalAdmin`,
`ChannelAdmin`, `User`, `Guest`.

| Recurso | Operações | Quem |
|---|---|---|
| `/api/commercials/{channel}/advertisers` | CRUD | admins |
| `/campaigns` | CRUD, ativar/arquivar | admins |
| `/spots` | registrar após upload, listar, status, arquivar | **User (operador)** envia e vê; admins gerenciam |
| `/break-windows` | CRUD | admins |
| `/week-template` | ler/gravar por dia da semana | admins |
| `/plan/{date}` | `GET` pré-visualização e avisos; `POST` aplicar | admins |
| `/reports/campaigns` | filtros: período, anunciante, campanha | admins |
| `/reports/export.csv`, `/reports/export.pdf` | download | admins |

Usar os helpers existentes de autenticação, papéis e acesso a arquivos
(AGENTS.md); validar o caminho do spot contra o storage do canal.

## 8. Relatórios (CSV e PDF)

Enviados aos anunciantes pelo operador, que **baixa o arquivo e envia por
conta própria** (o sistema não envia e-mail). Conteúdo:

- Cabeçalho: logotipo e dados da emissora (configuráveis), anunciante,
  campanha, período, data de emissão.
- Resumo: previstas × veiculadas × não veiculadas, por dia.
- Tabela: data, hora real, spot, duração, resultado (e motivo quando não
  veiculado, inclusive "ao vivo no horário").
- Rodapé: observação de que os horários seguem o fuso da emissora.

CSV: gerado no servidor, UTF-8 com BOM e separador `;` para abrir direto no
Excel em pt-BR. PDF: decidir a biblioteca na implementação (avaliar crates de
geração de PDF); começar com CSV e uma página de relatório imprimível, e
acrescentar o PDF em seguida. O logotipo e os dados da emissora ficam em uma
configuração do canal.

## 9. Interface (à prova de leigo)

Menu "Comerciais" com abas simples, textos em pt-BR (já existe
`frontend/src/locales/pt-BR.ts` [verificado]):

1. **Hoje**: intervalos do dia, o que já foi ao ar e o próximo; avisos em
   destaque.
2. **Spots**: arrastar o arquivo, barra de progresso, estado
   "Convertendo / Pronto / Com erro (motivo)".
3. **Campanhas**: assistente em 4 passos (anunciante → período → spots →
   frequência e posição). Campanhas que vencem em breve ou sem spots ficam
   marcadas.
4. **Intervalos**: horários e durações por dia da semana, com a soma e o
   desvio visíveis.
5. **Grade**: ordem dos programas por dia da semana.
6. **Relatórios**: escolher anunciante e período; botões CSV e PDF.

Princípios: poucas ações por tela, confirmação antes de apagar, mensagens que
dizem **o que fazer**, cor e ícone além do texto para o estado, e nada de
termos técnicos (FFmpeg, probe, codec) na tela do operador. Novas telas em
`frontend/src/views/` e `components/`, rota em `frontend/src/router/index.ts`.

## 10. Plano de implementação

**Andamento:** M1, M2 e M3 concluídas (ver `.dev/commercials_progress.md`).


Cada etapa é entregável e testável por si; as quatro primeiras podem ser
validadas na nuvem com `cargo fmt`, `clippy` e `cargo test`.

| # | Etapa | Entrega | Testes |
|---|---|---|---|
| M1 | Fundação | Migration `00006`, modelos, handles, CRUD da API | testes de handles (SQLite em memória) e de rotas com autorização (`backend/app/tests/commercials.rs`) |
| M2 | Ingestão | Ajuste do upload para MXF, conversão, estados do spot | amostra MXF pequena em `tests_assets/`; falhas (sem áudio, corrompido) |
| M3 | Grade semanal e compilador | tabelas da grade, `break_planner`, pré-visualização | testes unitários puros (seção 4), fusos explícitos |
| M4 | As-run | gravação no `play_loop`, conciliação | simulação de `ClipResult`; falha do banco não afeta o ar |
| M5 | Relatórios | consultas, CSV, PDF | totais previstos × veiculados; casos de `live_missed` |
| M6 | Interface | telas da seção 9 | `npm run type-check`, `lint`; roteiro manual |
| M7 | Automação | geração dos próximos N dias, alertas, "Hoje" | simulação de dia sem playlist; tempo de recuperação |

Dependências com o resto do roadmap:

- Fase 1 (relay, teto de reinício do supervisor, backup) é independente e
  deve vir antes de levar ao cliente.
- Horário fixo de programa (roadmap 2.2) entra primeiro como **validação com
  aviso** (compilador compara o horário âncora com o calculado). Impor o
  horário no engine fica para depois, se necessário.

## 11. Riscos

| Risco | Mitigação |
|---|---|
| Gravação do as-run atrasar ou derrubar o ar | tarefa separada, falha só em log, conciliação posterior |
| Playlist editada depois da compilação | aviso de plano desatualizado, recompilar só o futuro |
| Spot apagado ou movido com campanha ativa | validar na compilação; no ar o engine já toca filler, e o registro marca `fallback` |
| Áudio de MXF (várias trilhas mono) | decidir o mapeamento com amostras reais antes da M2 |
| Dias com mudança de horário de verão | compilar com fuso explícito; teste dedicado |
| Relatório mudar depois de enviado | cópias de nomes no `airlog` |
| Banco SQLite crescendo | índices por canal e data; política de retenção |
| Vários canais | todas as tabelas e consultas com `channel_id` |

## 12. Decisões fechadas e pendências

Fechadas: 1080p30 a 3000 kbps; quantidade de intervalos depende da demanda
(tudo configurável, sem valor fixo); o operador baixa e envia o relatório;
retenção de 60 dias, com a exceção "campanha até o fim + 60 dias" (aprovada);
volume seguindo a norma brasileira, com alvo configurável; logotipo e dados da
emissora no PDF **configuráveis** (por canal), sem valor fixo no código.

Pendência (não bloqueia o desenho): conferir no texto da norma o valor padrão
do alvo de volume e as tolerâncias.
