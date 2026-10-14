# Roadmap do fork (emissora de TV local)

Contexto do cliente (informado pelo usuário):

- Grade que se repete, com variação de ordem por dia da semana.
- Saída única que alimenta vários destinos de distribuição: SRT, RTMP e m3u8.
  Os destinos **puxam** o sinal (SRT em modo listener).
- Raras entradas ao vivo, mas de longa duração.
- Interface à prova de leigo.
- Tolerância fora do ar: 1 min 30 s.
- Conteúdo em mp4; comerciais costumam chegar em **MXF**.
- Módulo de **comerciais**: configurar spots e inseri-los na grade em horários
  pré-definidos.
- Hospedagem futura em VPS.

Legenda: **[verificado]** = confirmado lendo o código; **[hipótese]** = precisa
de teste com o playout rodando.

## 1. Achados que condicionam o desenho

1. **Só há uma saída ativa por canal** [verificado].
   `migrations/00003_split_config.sql`: `config_output_one_active`.
2. **A saída SRT não aceita modo listener** [verificado].
   `backend/engine/src/utils/protocol.rs`: `mode` só aceita caller (0) e
   rendezvous (2), e há teste que garante a rejeição de `listener`. Além disso,
   o listener do FFmpeg/libsrt atende um cliente por vez, então não serve a
   vários destinos de qualquer forma.
3. **MXF não está na lista padrão de extensões** [verificado]. O padrão é
   `mp4;mkv;webm` (`config_storage.extensions`). O frontend já reconhece `mxf`
   como vídeo (`frontend/src/composables/helper.ts`).
4. **MXF costuma trazer vários áudios mono** [hipótese]. O engine escolhe um
   único stream de áudio (`streams.best(Audio)`,
   `backend/engine/src/playout.rs`). Um MXF com 8 trilhas mono pode sair só
   com um canal. Testar com amostras reais.
5. **Não existe lógica de comerciais** [verificado]. Só há o booleano `ad` em
   cada item, usado para o fade do logo e para marcar o item na grade
   (`player/input/playlist.rs:299`, `player/output/playout.rs:253`).
6. **O supervisor tenta reiniciar com espera de até 180 s**
   (`player/controller.rs:654`, `MAX_DELAY`). Isso passa da tolerância de
   90 s do cliente.
7. **Grade sem horário fixo, geração só manual, atraso acumulado no modo
   Stream**: ver `.dev/scheduler.md`.

## 2. Arquitetura proposta para a distribuição

```
 ffplayout ──(1 saída, RTMP ou SRT caller, codificada uma vez)──► relay local
                                                                   │
                                  ┌────────────────────────────────┼───────────────┐
                                  ▼                                ▼               ▼
                          SRT listener (N clientes)        RTMP (puxar/empurrar)   HLS (m3u8)
```

- O relay (MediaMTX, SRS ou similar) recebe um sinal e o serve em vários
  protocolos e para vários clientes, sem recodificar. Resolve os achados 1 e 2
  sem alterar o engine.
- Quando o ffplayout reinicia, os destinos continuam conectados ao relay.
- A validação do relay escolhido precisa ser feita no ambiente real; ainda não
  foi testada.
- Alternativa futura: múltiplas saídas dentro do engine (codifica uma vez, vários
  muxers). Só vale se o relay se mostrar insuficiente.

## 3. Módulo de comerciais (proposta)

Princípio: **compilar na playlist**. A grade-modelo e as regras de intervalo
geram o JSON final do dia, usando o formato e a sincronização que já existem.
Vantagens: o operador vê no editor exatamente o que vai ao ar, o controle de
horário e o EPG continuam valendo, e o engine não muda.

### Dados (migration nova, `00006_*`; não editar a 00005)

- `commercial` (spot): nome, anunciante, arquivo, duração, ativo, vigência
  (início/fim), peso/prioridade.
- `commercial_break` (regra de intervalo): dia(s) da semana, horário de início
  ou "a cada X minutos entre A e B", duração-alvo, grupo de spots permitidos.
- Grade-modelo semanal: itens por dia da semana (ver item 4.2).

### Lógica (módulo de domínio em `backend/app/src/utils/`, queries em `db/handles/`)

1. Posicionar cada intervalo no horário pedido. Para isso o item de programa
   precisa poder ser **dividido** em dois (`in`/`out` já existem): parte A até
   o ponto de corte, intervalo, parte B a partir dele.
2. Preferir **pontos de corte definidos por programa** (marcadores). Cortar em
   horário cravado pode cair no meio de uma cena.
3. Escolher os spots por rodízio ponderado, respeitando a vigência e evitando
   o mesmo anunciante em sequência.
4. Fechar a duração do intervalo exatamente (preencher com chamada/vinheta ou
   cortar o último spot).
5. Marcar os itens com `ad = true` (aproveita o fade do logo já existente).
6. Registrar o que foi ao ar (as-run) para comprovação ao anunciante.

### Ingestão de comerciais (MXF)

- Opção A: tocar o MXF direto (exige habilitar a extensão e validar áudio).
- Opção B (recomendada): ao enviar, converter para um mp4 padronizado (H.264 +
  AAC estéreo, resolução e fps do canal) com loudness normalizado, e só então
  disponibilizar o spot. Evita surpresas na hora do ar e protege o horário.

## 4. Plano por fases

### Fase 0: ambiente e medições (local)
- Subir o ffplayout no Linux (ver `.dev/local-setup-prompt.md`).
- Testar com amostras reais: MXF dos comerciais, mp4 do programa.
- Medir a deriva de horário no modo Stream (hipótese 7).

### Fase 1: operação confiável (sem alterar o núcleo de vídeo)
- 1.1 Relay local e as 3 saídas (SRT listener, RTMP, HLS). Documentar e
  empacotar (compose/systemd).
- 1.2 Reduzir o teto de reinício do supervisor para dentro da tolerância
  (ex.: 5 a 10 s) e alertar quando a saída cair.
- 1.3 Playlist sempre existindo: geração automática dos próximos N dias a
  partir da grade-modelo semanal, e alerta de dia sem playlist.
- 1.4 Backup (SQLite, playlists, storage) e HTTPS/autenticação no m3u8.

### Fase 2: grade e comerciais
- 2.1 Grade-modelo por dia da semana.
- 2.2 Horário fixo para programas (item "entra às HH:MM").
- 2.3 Módulo de comerciais (seção 3), com ingestão MXF.
- 2.4 Registro do que foi ao ar (as-run) e relatório por anunciante.

### Fase 3: interface para leigos
- Tela inicial com semáforo por saída (no ar / reconectando / caiu) e próximo
  programa/intervalo.
- Editor visual de semana (arrastar e soltar), com validação em linguagem
  simples ("esta grade passa 3 min de 24 h").
- Perfil "operador" com poucas ações (trocar ordem do dia, subir comercial,
  entrar ao vivo, voltar à grade), usando os papéis de usuário existentes.
- Textos em pt-BR (já existe `frontend/src/locales/pt-BR.ts`).
- Cobertura dos próximos 14 dias: dias sem playlist em vermelho.

### Fase 4: robustez e VPS
- Deploy na VPS (Docker Compose ou `.deb`), monitoramento e alertas por
  e-mail/HTTP, contingência opcional.

## 5. Decisões do cliente (respondidas pelo usuário)

| Tema | Decisão | Consequência |
|---|---|---|
| RTMP | A emissora entrega o link RTMP aos destinos, que **puxam** o sinal | O ffplayout só envia; quem serve RTMP, SRT e HLS é o relay. Sem relay não há como atender os destinos. |
| Horário dos intervalos | **Fixos no relógio** | Exige horário fixo de programa (fase 2.2) e regra para quando o programa não fecha no horário (ver 6.1). |
| Spots e campanhas | As duas modalidades. Campanha tem início, fim e anunciante | Modelo de dados com campanha e spot avulso (seção 3). |
| Onde cortar | **Entre programas** | Não é preciso dividir clipes em `in`/`out`. Simplifica a compilação da playlist. |
| Ao vivo | Raro e de longa duração. Intervalos **continuam entrando** durante o ao vivo | Capacidade nova no engine (ver 6.2). |
| Envio de comerciais | O **operador** envia pela interface | Upload na UI + conversão automática (MXF → mp4 padronizado) com progresso visível. |
| Relatórios | Logs e relatórios de campanhas e anúncios | Tabela de veiculação real (as-run), relatório por campanha, anunciante e período, com exportação. |
| Tolerância fora do ar | 1 min 30 s | Teto de reinício do supervisor e alerta (fase 1.2). |
| Interface | À prova de leigo | Fase 3. |

## 6. Pontos de desenho a resolver

### 6.1 Intervalo em horário fixo versus fim do programa
Se o intervalo é às 20:00 e o programa anterior termina às 19:57, ou às
20:04, é preciso uma regra. Proposta:

- O horário do intervalo é **nominal**. O compilador procura a fronteira entre
  programas mais próxima dele e insere o intervalo ali.
- Se a fronteira estiver a mais de uma tolerância configurável (ex.: 60 s), o
  sistema **avisa ao montar a grade** ("o intervalo das 20:00 cai 4 min depois
  do fim do programa X") e propõe completar com filler ou ajustar o programa.
- Como a grade repete, esse aviso aparece uma vez no planejamento, não no ar.

### 6.2 Intervalos durante o ao vivo [verificado: hoje não é possível]
Com um ao vivo no ar, a playlist fica suspensa e os comandos de navegação são
ignorados (`NavigationBlocked::Live`, `engine/src/playback_control.rs`).
Inserir spots no meio de um ao vivo exige uma capacidade nova no engine:
interromper a saída ao vivo, tocar o intervalo e voltar ao sinal, que continua
chegando durante esse tempo.

Proposta em duas etapas:
1. **Botão do operador** "Intervalo agora" durante o ao vivo (e "Voltar ao
   vivo"), com o intervalo do dia escolhido automaticamente.
2. **Automático** nos horários fixos, depois que a etapa 1 estiver estável.

### 6.3 Registro de veiculação (as-run)
- Gravar no banco cada item ao ar com início e fim **reais** e o resultado
  (`Played`, `Skipped`, `Fallback`), no ponto onde o app recebe o resultado do
  engine (`player/output/playout.rs`, `play_loop`). Spot pulado ou que caiu em
  fallback **não** conta como veiculado.
- Relatórios: por campanha, anunciante, spot e período; veiculados versus
  previstos; exportação CSV (e PDF numa etapa posterior).

## 7. Questões em aberto

1. Regra do item 6.1: a proposta (fronteira mais próxima + aviso) atende?
2. Ao vivo (6.2): começar pelo botão do operador e automatizar depois?
3. Campanha: precisa de limite de frequência (ex.: no máximo N exibições por
   dia) e de preferência de posição no intervalo (primeiro/último)?
4. Quem pode ver os relatórios e eles são enviados aos anunciantes pela
   emissora (PDF/CSV) ou só consultados internamente?
5. Qual a banda de saída disponível na VPS e quantos destinos simultâneos?
