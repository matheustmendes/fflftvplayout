# Plano de responsividade para celular

Objetivo: o operador consegue acompanhar o canal e fazer o essencial pelo celular, com uma
mão, sem zoom e sem errar o toque. O que é edição pesada (grade, campanhas, relatórios)
precisa ser **usável**, não necessariamente tão rápido quanto no computador.

## 1. Como medir (critérios de pronto)

Vale para janelas de 360 a 430 px de largura (e conferir 768 px, tablet):

1. Nenhuma tela estoura para o lado (sem barra de rolagem horizontal na página).
2. Alvos de toque com pelo menos 40 px de altura e largura (meta: 44 px) e 8 px de espaço
   entre os vizinhos.
3. Texto de leitura com pelo menos 14 px; nada com menos de 12 px.
4. A ação principal de cada tela fica ao alcance do polegar (parte de baixo) e nunca
   escondida atrás da barra de navegação.
5. Nenhum controle muda de lugar enquanto o dedo está nele (o defeito que corrigimos no
   controle de tamanho do texto: ver `RangeField.vue`).
6. Campos usam o teclado certo (`inputmode`, `type=time`, `type=number`) e não são
   cobertos pelo teclado.

Ferramenta: `scripts/dev/mobile-audit.mjs` abre cada tela numa janela de celular, mede os
itens 1 a 3 e salva uma foto. Rode antes e depois de cada fase e compare com a tabela abaixo.

## 2. Situação hoje (390 x 844, medido em 18/10/2026)

Nenhuma das 20 telas estoura para o lado (a base é boa). O problema é toque e leitura:

| Tela | Alvos < 36 px | Texto < 12 px |
|---|---|---|
| No ar | **28** | **32** |
| Publicidade > Grade | **27** | 0 |
| Publicidade > Intervalos | 15 | 2 |
| Publicidade > Preferências | 13 | 0 |
| Arquivos | 10 | 0 |
| Texto na tela | 10 | 1 |
| Publicidade > Relatórios | 9 | 0 |
| Publicidade > Comerciais | 8 | 0 |
| Publicidade > Campanhas | 8 | 2 |
| Painel, Hoje, Ajustes (cada) | 4 a 7 | 0 |

Defeitos vistos nas fotos:

- **Abas da Publicidade cortadas.** "Hoje | Comerciais | Campanhas | Intervalos | Gra…": Grade,
  Relatórios e Preferências ficam fora da tela, sem sinal de que dá para rolar
  (`views/CommercialView.vue`).
- **Grade (`CommercialGrid.vue`)**: o nome do programa vira uma caixa minúscula ("Jorn",
  "Nov", "Film"); despertador, setas e "x" são pequenos e juntos; a alça de arrastar some, e o
  arrastar usa o recurso do navegador, que costuma não funcionar com o dedo (**hipótese:
  confirmar num celular de verdade**).
- **No ar (`PlayerControl.vue`)**: botões de duas linhas cortados ("Reiniciar o…"), a
  maior parte do texto abaixo de 12 px e os controles de playlist apertados.
- **Barra de baixo** (`HeaderMenu.vue`) sem margem para a área segura do iPhone
  (`env(safe-area-inset-bottom)`), e a barra "Salvar" (`SaveBar.vue`) flutua em cima dela.
- **Tabelas** (Hoje, Relatórios, Comerciais) no formato do computador.

## 3. Fases

Tamanhos: P (algumas horas), M (um a dois dias), G (mais de dois dias).

### F0. Base (P) — fazer primeiro, melhora tudo
- Regra global para tela de toque: `@media (pointer: coarse)` aumenta `btn-xs`/`btn-sm`,
  `input-xs`, `select-xs` e checkboxes para 40 px de altura mínima.
- Margem da área segura embaixo (barra de navegação e `SaveBar`) e espaço no fim da página
  para a barra não cobrir o último item.
- Texto mínimo de 12 px em tudo (revisar `text-[10px]`, `text-xs` usados como corpo).
- Padrão de componente para o que precisa de dedo: `RangeField` (já feito) para
  controles deslizantes; criar um `TouchRow` para linhas de lista com ações.
- Aceite: os números da tabela caem pela metade nas telas "Painel", "Hoje" e "Ajustes".

### F1. Operação ao vivo: Painel e No ar (M) — o que mais se usa no celular
- Painel: cartões em coluna, semáforo dos destinos em lista (um por linha, luz grande),
  botão "Ligar/Parar o canal" ocupando a largura.
- No ar: vídeo no topo, "agora/depois" legível, **ações em barra fixa embaixo** (Ligar,
  Parar, Pular, Reiniciar) com rótulo curto e confirmação como já existe. Playlist do dia
  em cartões; reordenar por botões "subir/descer" (arrastar fica opcional).
- Aceite: 0 alvos < 40 px e 0 textos < 12 px em `/` e `/player`; ligar e parar o canal
  com uma mão.

### F2. Navegação (P)
- Abas da Publicidade e dos Ajustes: rolagem horizontal com a aba ativa centralizada,
  sombra nas pontas como pista **ou** menu de seleção ("Publicidade: Grade ▾") no celular.
- Trocar de canal no celular (`HeaderMenu.vue`): conferir como está hoje e, se apertado,
  usar uma folha que sobe de baixo.
- Aceite: todas as abas alcançáveis sem adivinhar, e a aba atual sempre visível.

### F3. Telas de edição (G, por tela)
Ordem sugerida pelo uso: Grade, Texto na tela, Gráficos (CG), Campanhas, Intervalos, Comerciais.
- **Grade:** linha em duas camadas: título (largura toda) e abaixo horário, duração e ações
  com 40 px; "mover" por botões; hora marcada em seletor de hora grande.
- **Texto na tela:** pré-visualização fixa no topo enquanto se ajusta; botões de tamanho
  grandes; o campo de texto não fica sob o teclado.
- **CG:** a "tela de teste" vem primeiro e fica fixa no topo ao mexer nos controles; a
  grade de posições rápidas com alvos de 44 px.
- **Campanhas/Intervalos/Comerciais:** tabelas viram cartões com as informações principais
  e um menu "…" para as ações.
- Aceite por tela: 0 alvos < 40 px; a tarefa principal (por exemplo, "marcar o Jornal às
  20:00") feita no celular sem zoom.

### F4. Relatórios e tabelas largas (M)
- Hoje e Relatórios: cartões por linha no celular; filtros em uma folha recolhível;
  exportar CSV/PDF por botão grande (o download já funciona no celular).
- Onde a tabela precisar continuar tabela: rolagem horizontal **dentro** da tabela, com a
  primeira coluna fixa.

### F5. Arquivos (M)
- Navegador de arquivos com linhas de toque, pressionar e segurar para o menu, enviar
  arquivo da galeria/câmera, barra de progresso de envio visível (envios longos no celular:
  avisar para não bloquear a tela).

### F6. Instalar como aplicativo (P, opcional)
- `manifest.webmanifest` com o nome e os ícones LFTV e `display: standalone`, para "Adicionar à
  tela inicial". Sem modo offline (o canal é ao vivo, e dado velho engana).
- Os avisos de problema já chegam pelo Telegram, então **notificação push do navegador não
  é necessária**.

### F7. Qualidade contínua (P)
- Rodar `scripts/dev/mobile-audit.mjs` ao fim de cada fase e guardar a tabela aqui.
- Conferir em celular de verdade: Chrome no Android e Safari no iPhone (a barra de
  endereços muda a altura da tela; evitar `100vh`, usar `dvh`), nos dois sentidos.
- Roteiro manual de 10 minutos: ligar/parar o canal, pular um item, trocar a ordem do dia,
  marcar a hora de um programa, mostrar um gráfico agora, ler o resumo de comerciais.

## 4. Decisões que dependem do usuário
1. **O que a equipe faz no celular de verdade?** Se for só acompanhar e ligar/parar, as
   fases F0, F1 e F2 resolvem 80%, e F3 a F5 podem esperar.
2. **Quem usa**: operador (papel limitado) ou também o administrador? Muda quais telas
   entram na F3.
3. **iPhone, Android ou os dois?** Define onde testar primeiro.
4. **Instalar como aplicativo** (F6) interessa?

## 5. Ordem recomendada
F0 → F1 → F2 (estimativa: poucos dias de trabalho) → conferir no celular da equipe → decidir F3
a F5 com base no que eles realmente tentarem fazer.
