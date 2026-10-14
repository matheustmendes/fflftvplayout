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
6. ~~O supervisor tenta reiniciar com espera de até 180 s~~ **Corrigido na
   Fase 1**: o teto é de 10 s (`MAX_RESTART_DELAY`, `player/controller.rs`).
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
- ~~Quando o ffplayout reinicia, os destinos continuam conectados ao relay.~~
  **Medido na nuvem (MediaMTX): quando a fonte cai, o relay desconecta os
  destinos RTMP e SRT**, e eles precisam reconectar. O que o destino vê é o
  tempo de reinício do ffplayout (agora ≤ 10 s) mais o tempo de reconexão dele.
- O relay foi testado na nuvem (publicar e puxar por RTMP, SRT e HLS, com
  senha, 12 destinos ao mesmo tempo); falta a validação no ambiente real. Ver
  `deploy/README.md`.
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
- 2.2 Horário fixo para programas (item "entra às HH:MM"). **Em espera
  (decisão do usuário).** Quando for feito, é **opcional por programa**: o
  operador decide se um programa entra em hora marcada; a grade se readequa
  quando for possível (ajustando o enchimento e os intervalos) e, quando não for,
  **sinaliza os conflitos** que isso causa (por exemplo, o programa anterior
  passa do horário) em vez de esconder ou quebrar a grade.
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
| Horário dos intervalos | **Fixos no relógio** | Exige horário fixo de programa (fase 2.2). Regra de alinhamento aprovada (ver 6.1). |
| Spots e campanhas | As duas modalidades. Campanha tem início, fim e anunciante | Modelo de dados com campanha e spot avulso (seção 3). |
| Onde cortar | **Entre programas** | Não é preciso dividir clipes em `in`/`out`. Simplifica a compilação da playlist. |
| Ao vivo | Raro e de longa duração. **Sem intervalos durante o ao vivo** (decisão: risco de quebrar > benefício) | Fora de escopo. O ao vivo segue como hoje; o operador agenda a duração e a grade retoma depois (ver 6.2). |
| Envio de comerciais | O **operador** envia pela interface | Upload na UI + conversão automática (MXF → mp4 padronizado) com progresso visível. |
| Campanhas | Limite de frequência por dia e preferência de posição (primeiro/último) no intervalo | Campos `max_per_day` e `position` na campanha. |
| Relatórios | Logs e relatórios de campanhas e anúncios, **enviados aos anunciantes em PDF e CSV** | Tabela de veiculação real (as-run), relatório por campanha, anunciante e período, com exportação em CSV e PDF. |
| Destinos | Estimativa de **12 destinos puxando ao mesmo tempo** | Dimensionamento da VPS (seção 8). |
| Qualidade do canal | **1080p, 30 fps, 3000 kbps** de vídeo | Spots convertidos para 1080p30; banda calculada na seção 8. |
| Volume de intervalos | Varia conforme a demanda; não dá para fixar | O sistema não pode assumir quantidade fixa; tudo é configurável. |
| Retenção de registros | **60 dias** | Ver alerta na seção 9. |
| Relatório | PDF com logotipo e dados da emissora; o operador **baixa e envia** | Não há envio de e-mail pelo sistema. |
| Tolerância fora do ar | 1 min 30 s | Teto de reinício do supervisor e alerta (fase 1.2). |
| Interface | À prova de leigo | Fase 3. |

## 6. Pontos de desenho a resolver

### 6.1 Intervalo em horário fixo versus fim do programa (aprovado)
Se o intervalo é às 20:00 e o programa anterior termina às 19:57, ou às
20:04, vale a regra:

- O horário do intervalo é **nominal**. O compilador procura a fronteira entre
  programas mais próxima dele e insere o intervalo ali.
- Se a fronteira estiver a mais de uma tolerância configurável (ex.: 60 s), o
  sistema **avisa ao montar a grade** ("o intervalo das 20:00 cai 4 min depois
  do fim do programa X") e propõe completar com filler ou ajustar o programa.
- Como a grade repete, esse aviso aparece uma vez no planejamento, não no ar.

### 6.2 Intervalos durante o ao vivo (fora de escopo)
Decisão: **não** inserir intervalos durante o ao vivo. Motivo [verificado]: com
um ao vivo no ar a playlist fica suspensa e os comandos de navegação são
ignorados (`NavigationBlocked::Live`, `engine/src/playback_control.rs`).
Interromper o ao vivo, tocar o intervalo e voltar ao sinal exigiria uma
capacidade nova no engine, com risco alto para o caminho crítico do ar.

Comportamento adotado:
- O ao vivo entra na grade como item com duração definida pelo operador, ou
  por entrada RTMP/SRT já existente, como hoje.
- Ao terminar, a playlist retoma no ponto que o relógio indica (comportamento
  atual). Os intervalos cujo horário caiu dentro do ao vivo **não são
  recuperados**.
- O relatório de campanhas deve mostrar esses intervalos como "não veiculados
  por ao vivo", para não parecerem falha. Registrar o motivo na própria linha
  de veiculação prevista.
- Reavaliar apenas se o cliente exigir, e como projeto separado no engine.

### 6.3 Registro de veiculação (as-run)
- Gravar no banco cada item ao ar com início e fim **reais** e o resultado
  (`Played`, `Skipped`, `Fallback`), no ponto onde o app recebe o resultado do
  engine (`player/output/playout.rs`, `play_loop`). Spot pulado ou que caiu em
  fallback **não** conta como veiculado.
- Relatórios: por campanha, anunciante, spot e período; veiculados versus
  previstos; exportação CSV (e PDF numa etapa posterior).

## 7. Desenho técnico

O desenho do módulo de comerciais (dados, compilador, as-run, conversão de
MXF, API, relatórios, interface e etapas) está em
`.dev/commercials_design.md`.

Achado adicional [verificado]: o upload valida o tipo pela extensão contra uma
lista fixa (`backend/app/src/file/upload.rs`) que não inclui MXF (nem mkv ou
MPEG/TS), então um `.mxf` é rejeitado hoje.

## 8. Dimensionamento da VPS (estimativa)

O relay não recodifica: a banda de saída é **destinos × bitrate**. Com 12
destinos, 1080p30 a 3000 kbps de vídeo e cerca de 128 kbps de áudio (valor
assumido), cada destino consome ~3,1 Mbps.

| Item | Valor |
|---|---|
| Saída total (12 destinos) | ~37,5 Mbps |
| Dados por hora | ~16,9 GB |
| Dados por dia | ~405 GB |
| Dados por mês (30 dias) | ~12,2 TB (cerca de 13 TB com a sobrecarga do protocolo) |

Para comparação, cenários de maior qualidade (por destino): 5 Mbps somam
60 Mbps e 19,4 TB/mês; 8 Mbps somam 96 Mbps e 31,1 TB/mês.

Recomendações:

- **Banda:** porta de 1 Gbps, ou no mínimo o dobro da saída total (folga para
  picos e novos destinos). Se a quantidade de destinos crescer, a banda cresce
  linearmente.
- **Franquia de tráfego:** é o ponto que mais surpreende em VPS. Muitos planos
  limitam o tráfego mensal a valores abaixo de 12 TB. Escolher plano com
  tráfego **ilimitado** ou franquia de pelo menos ~15 TB, e conferir a regra
  aplicada depois da franquia.
- **CPU e RAM:** o README do projeto recomenda no mínimo 4 threads dedicadas e
  3 GB de RAM para 720p; para 1080p reservar mais. Planejar 4 a 6 vCPUs
  **dedicadas** (não compartilhadas) e 8 GB de RAM, já contando relay e
  conversão de spots. Validar medindo o uso real na fase 0.
- **Disco:** depende do tamanho da biblioteca de mídia; segmentos HLS e logs
  são pequenos.
- **Contingência:** segundo servidor, a decidir com o cliente.

## 9. Decisões fechadas e pendências

Fechadas: retenção de 60 dias com a exceção "campanha até o fim + 60 dias";
volume seguindo a norma brasileira, com alvo configurável; logotipo e dados da
emissora configuráveis por canal.

Pendência: conferir no texto da norma (ABNT NBR 15602) o valor padrão do alvo
de volume e as tolerâncias.

## 10. Estado do ambiente de nuvem

O container desta sessão (Ubuntu 24.04) tem FFmpeg 6.1 **sem** os pacotes de
desenvolvimento, e o projeto exige FFmpeg >= 7.0 com headers. Instalar pacotes
e adicionar o PPA `ppa:ubuntuhandbook1/ffmpeg7` foi bloqueado pelas permissões
da sessão. Sem isso o backend não compila na nuvem, então `cargo clippy` e
`cargo test` (exigidos pelo `backend/AGENTS.md`) não podem ser executados aqui.
Solução: instalar as dependências por meio do **script de setup do ambiente**
(configuração do ambiente de nuvem), que roda no início de cada sessão.
