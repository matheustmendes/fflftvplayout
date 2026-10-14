# Fase 3: interface à prova de leigo

Meta do usuário: o frontend inteiro do ffplayout, melhorado em visual e em
interação, **100% amigável para quem não entende de TV nem de informática**.

## Diagnóstico da interface original (capturas de tela, desktop e celular)

- Tela inicial é uma tabela de estatísticas do servidor (CPU, swap, rede) e uma
  fileira de botões cinza. Não diz se a emissora está no ar.
- Botões primários cinza (`btn-primary` era cinza #e0e0e0), sem hierarquia entre
  a ação principal e as secundárias; ícones sem texto no Player.
- Vocabulário técnico: "Stop Threshold", "ffmpeg Level", "Ignore Lines", "Eixo X",
  "Armazenamento", "Legenda" (que é texto na tela), "Registro" (que é o log).
- Parâmetros avançados misturados com os essenciais (ConfigPlayout tem 1500
  linhas de campos).
- Ações perigosas (parar, reiniciar, apagar) sem confirmação clara.
- Sem estados vazios que digam o que fazer; sem ajuda no lugar do campo.
- Estado do canal só aparece dentro da página Player.

## Princípios

1. **Dizer primeiro o que importa:** "Está no ar?" visível em toda tela.
2. **Uma ação principal por tela**, colorida; as outras discretas.
3. **Português do dia a dia**, sem sigla. Termo técnico só dentro de "Avançado".
4. **Avançado escondido por padrão** (um botão "Mostrar opções avançadas").
5. **Confirmar o que não tem volta** (parar, reiniciar, apagar, substituir) com
   texto que explica a consequência.
6. **Estado vazio ensina** ("Ainda não há X. Clique em Y.").
7. **Ajuda no lugar** (ícone "?" ao lado do campo).
8. **Funciona em celular** (operador no telefone) e no escuro (tema escuro).
9. **Operador (`user`) vê só o que usa.**

## Entregas

| # | Entrega | Situação |
|---|---|---|
| 3.1 | Fundação visual: paleta com contraste, botões com hierarquia, componentes (`ui/`) | feito |
| 3.2 | Navegação nova + estado do canal (NO AR / PARADO / REINICIANDO / AO VIVO) sempre visível | feito |
| 3.3 | Painel inicial: estado, agora e próximo, cobertura de 14 dias, avisos, primeiros passos | feito |
| 3.4 | Player renomeado "No ar": botões com texto e confirmação | feito |
| 3.5 | Ajustes: simples por padrão, avançado escondido | |
| 3.6 | Arquivos, Texto na tela, Histórico, Login e Primeiro acesso | |
| 3.7 | Editor visual da grade (arrastar e soltar) com validação em linguagem simples | feito |
| 3.8 | Operador: menu e ações reduzidas | feito |

Idiomas: pt-BR (principal) e en-US. Alemão e russo herdados do projeto original
caem para o inglês nos textos novos.

## Guia de design (para quem for mexer nas telas)

**Tema** (`frontend/src/assets/css/main.css`): `btn-primary` é azul com texto
branco (ação principal da tela, no máximo uma por área); `btn` neutro é a ação
secundária; `btn-error` só para ação destrutiva; laranja (`accent`) é só destaque
(aba ativa, barra de progresso de marca). Cores de estado: `success`, `warning`,
`error`, `info` com texto de contraste já definido (não force `text-black`).

**Componentes** (`frontend/src/components/ui/`):

| Componente | Para quê |
|---|---|
| `PageHeader` | título da tela (`h1`), subtítulo em uma frase e botões à direita (slot `actions`) |
| `HelpTip` | ícone "?" ao lado de um campo, com a explicação em linguagem simples |
| `EmptyState` | quando não há nada ainda: ícone, título, frase e o botão do que fazer |
| `StatusPill` | estado em uma palavra com cor (`ok`, `warn`, `bad`, `live`, `idle`) |
| `ConfirmModal` | confirmação com botão que diz o que vai acontecer ("Sim, parar o canal"); `danger` para o que não tem volta |

**Estado do canal para qualquer tela:** `useLive()` (`stores/live.ts`): `state`
(`on_air`, `live`, `restarting`, `stopped`, `offline`), `media`, `progress`,
`secondsLeft`. Já existe uma única conexão de status; não abra outra.

**Textos:** português do dia a dia em `frontend/src/locales/ux/pt-BR.ts` e a
versão em inglês em `en-US.ts` (chaves novas por área, ex.: `settings2.*`). Os
arquivos `pt-BR.ts` e `en-US.ts` principais só importam e espalham (`...ux`).

**Avançado escondido:** campos técnicos ficam dentro de um bloco "Mostrar opções
avançadas" (`ui.advancedShow`), fechado por padrão, com `ui.advancedHelp`.

**Verificação:** `npx vue-tsc --build`, `npx eslint frontend/src --config
frontend/eslint.config.ts`, `npx prettier --check`, `npx vite build` e conferir as
telas em 1280 px e 390 px, nos temas claro e escuro.
