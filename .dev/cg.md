# Gráficos na tela (CG) agendados

Texto ou imagem colocado em um ponto exato da imagem, que entra e sai sozinho
nos horários marcados. Pedido do cliente: "texto e imagem, no local exato que o
cliente queira, com agendamento".

## Como funciona

```
cg + cg_schedule (SQLite)
        │  a cada 5 s
        ▼
utils/cg.rs  run_cg_scheduler  ── a cada 1 s decide o que está no ar ──►  ChannelManager.graphics
                                                                                    │
                                          engine: compositor/layers.rs  ◄───────────┘
                                          (desenha as camadas sobre cada quadro)
```

- **Engine** (`backend/engine`): `GraphicLayersState` guarda uma *pilha* de
  camadas (`GraphicLayer` = id + texto ou imagem). Ao contrário do texto do
  operador (`TextOverlayState`, um só), aceita várias camadas ao mesmo tempo.
  `RuntimeGraphicLayers` recarrega só as camadas que mudaram e as desenha em
  `encode_composited_frame` (vídeo) e em `FallbackOverlays` (quadros pretos),
  depois do logo e dos textos já existentes. O texto do operador e o logo
  continuam como estavam.
- **Posição exata**: `x_pct`/`y_pct` são o canto superior esquerdo em % da
  imagem; `size_pct` é a altura das letras (texto) ou a largura (imagem), em %.
  Em % o gráfico não muda de lugar se a resolução da saída mudar. O servidor
  converte para pixels com o tamanho da saída do canal (`build_layer`).
- **Quando**: cada gráfico tem várias regras (`cg_schedule`):
  - `clock`: hora do dia (canal), dias da semana (bitmask, segunda = bit 0),
    período opcional de datas, duração, repetição a cada N s até uma hora.
    Janela que cruza a meia-noite funciona.
  - `program`: N segundos depois que um programa começa, filtrando por parte do
    título (vazio = qualquer). Comerciais não contam como programa, então a
    contagem segue durante o intervalo comercial.
  - "Mostrar agora" manual (`POST /api/cg/{canal}/{id}/show`), de 1 a 3600 s.
- **Decisão pura e testada**: `clock_active`, `program_active`,
  `scheduled_now` não dependem de relógio global.

## Gráfico de campanha (anúncio)

Um gráfico pode ter `campaign_id` (um QR code de patrocinador, um selo). Daí:

- **Datas:** só vai ao ar com a campanha ativa e dentro das datas dela. O
  "Mostrar agora" do operador ignora isso (a decisão é dele).
- **Registro de exibição:** cada aparição vira uma linha em `airlog` com
  `kind = 'cg'` (nome do gráfico em `spot_title`, anunciante e campanha copiados
  como num spot, início, fim e duração reais). Só é registrado com o canal no ar;
  reinício deixa a linha como `interrupted`, como nos spots.
- **Relatório:** o PDF ganha a seção "Gráficos na tela", o CSV um bloco "Gráficos"
  e o resumo da tela mostra quantas aparições e quanto tempo. Os totais de
  spots não mudam.
- **Retenção:** a mesma tabela e a mesma regra (60 dias, e histórico da campanha
  até 60 dias depois do fim dela).
- Não há planejador de intervalos nem limite por dia para CG: isso é coisa de
  spot dentro da playlist.

## API (`/api/cg/{canal}`)

| Método | Rota | Quem |
|---|---|---|
| GET | `/` lista gráficos + quais estão no ar | operador e admin |
| POST | `/` cria (com os horários) | admin |
| PUT | `/{id}` troca gráfico e horários | admin |
| DELETE | `/{id}` apaga | admin |
| POST / DELETE | `/{id}/show` mostra agora / tira | operador e admin |

Ao salvar, o servidor valida (posição 0–100 %, cores `#RRGGBB`, imagem existente
dentro do armazenamento do canal, intervalos coerentes) e responde com o motivo.

## Interface

Menu **Gráficos (CG)** (`views/CgView.vue`): lista com frase em português para
cada horário, selo "No ar agora", "Mostrar agora por N s" e liga/desliga.
Editor em 4 passos (`components/cg/CgEditor.vue`): o quê, aparência, onde (tela
de teste 16:9 em que se **arrasta** o gráfico + 9 posições rápidas + campos
numéricos) e quando. A imagem é escolhida entre os arquivos do canal.

## Limites conhecidos

- A tela de teste usa CSS: o tamanho da letra é muito parecido, mas a fonte e a
  quebra de linha reais são as do servidor. **Conferir no ar** (roteiro F5).
- O texto não rola (sem rodapé deslizante) e não há saída suave (só entrada).
- "Depois que um programa começa" conta a partir do momento em que o agendador
  percebe a troca (até 1 s de diferença); se o ffplayout reiniciar no meio do
  programa, a contagem recomeça dali.
- Vários gráficos que ocupam o mesmo lugar se sobrepõem na ordem do id.
