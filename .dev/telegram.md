# Bot do Telegram

Avisa a equipe, num grupo do Telegram, quando algo dá errado e responde a perguntas
sobre o canal. **Só consulta e avisa**: nenhum comando mexe no que está no ar.
Os textos são só em português.

Roteiro de aceite (precisa de um bot de verdade): `.dev/acceptance/F8.md`.

## Como funciona

- **Um bot por instalação.** O token fica na tabela `telegram_bot` (uma linha). Só o
  administrador geral grava; a API nunca o devolve (`token_set: true/false`) e ele
  não aparece em log nem em mensagem de erro (os erros do `reqwest` perdem a URL, que
  carrega o token).
- **Cada canal escolhe o grupo e os avisos** (`telegram_settings`: `chat_id`, um
  interruptor por tipo de aviso, resumo diário e comandos). Os padrões são tudo ligado,
  mas o bot do canal começa **desligado**.
- **Sem porta aberta.** O bot pergunta ao Telegram por novidades (long polling,
  `getUpdates`). A VPS só precisa sair para `api.telegram.org` em HTTPS.
- Tela: Ajustes → Telegram (administrador do canal; o token, só o administrador geral).

## Código (`backend/app/src/utils/telegram/`)

| Arquivo | Papel |
|---|---|
| `client.rs` | Chamadas da API do Telegram (`getMe`, `sendMessage`, `getUpdates`), corte de mensagens longas, espera do `429`. |
| `snapshot.rs` | Uma "foto" do canal (no ar, agora/próximos, destinos, filler, dias sem playlist, disco). Usada pelos avisos e pelos comandos. |
| `monitor.rs` | Decide o que avisar: `problems()` lista o que está errado; `Tracker` só diz o que mudou. Também o resumo diário e os comerciais que falharam. |
| `commands.rs` | Os comandos e o nível de acesso de cada um. |
| `bot.rs` | Escuta o Telegram, confere o chat, responde. Sobe o monitor. |
| `messages.rs` | Os textos em português (HTML do Telegram, tudo escapado). |
| `events.rs` | Canal de eventos do playout. Hoje só `OutputFailed`, emitido pelo supervisor. |

## Avisos

Cada problema é avisado **uma vez**, lembrado de tempos em tempos enquanto durar e,
quando acaba, avisado de novo ("voltou ao normal", com quanto tempo durou). O
`Tracker` aplica estas regras (`policy()` em `monitor.rs`):

| Problema | Só avisa depois de | Considera resolvido depois de | Lembra a cada |
|---|---|---|---|
| Saída fora do ar | 15 s (o evento do supervisor avisa na hora) | 30 s sem falha | 15 min |
| Canal parado pelo operador | 30 s | 15 s | 2 h |
| Relay sem resposta | 60 s | 30 s | 1 h |
| Destino desconectado | 30 s | 30 s | 1 h |
| Filler ausente | na hora | na hora | 6 h |
| Dia sem playlist (hoje ou amanhã) | na hora | na hora | 6 h |
| Pouco disco (< 10 % livre) | na hora | 10 min | 12 h |

Outros avisos:
- **Comercial que não foi ao ar** (`fallback` ou `failed` no airlog): lido a cada 60 s,
  agrupado numa mensagem. Na primeira leitura depois de subir o servidor o bot só
  "aprende" o que já existe, para não repetir coisa velha.
- **Resumo diário** no horário do canal: comerciais do dia anterior, destinos agora,
  playlists de hoje e amanhã, disco. Um reinício depois do horário não repete o
  resumo (só envia se o servidor subiu até 5 min depois da hora).
- Os 45 s depois de subir o servidor não geram aviso, para uma partida não parecer queda.

## Comandos (só leitura)

`/status`, `/proximos [n]`, `/destinos`, `/hoje`, `/amanha`, `/cobertura`, `/ajuda`
e `/id`. Aceitam sem acento e com `@nome_do_bot` (num grupo, um comando endereçado a
outro bot é ignorado).

Quem pode o quê (`Access` em `commands.rs`):
- **Qualquer chat**: `/id` e `/ajuda`. O `/id` é o jeito de descobrir o número do grupo
  na hora de configurar.
- **Só o chat de um canal** com o bot ligado e os comandos habilitados: o resto.
- Um chat que nenhum canal usa recebe uma resposta só (a cada hora), dizendo o número
  dele. Assim o bot não vira um jeito de fazê-lo falar.
- Mensagens antigas, de quando o servidor estava parado, são descartadas na partida.

## Como acrescentar comandos que mexem no ar (futuro)

Não existe nada disso ainda de propósito. O plano:

1. Um terceiro nível `Access::Action` em `commands.rs`. Quem está no grupo não basta:
   precisa saber **quem** mandou.
2. Ligar o `from.id` do Telegram a um usuário do sistema (tabela nova, preenchida
   na tela de usuários) e exigir o papel certo (administrador do canal para ações).
   Hoje `Message` em `client.rs` não lê o remetente; é preciso acrescentar `from`.
3. Confirmação com botões ("Reiniciar a saída? Sim / Não"), o que pede tratar
   `callback_query` em `getUpdates` (`allowed_updates`).
4. Registrar quem fez o quê e quando (ver o log de controle do player).
5. Ações candidatas, da mais segura para a mais perigosa: `/silenciar 2h` (só mexe nos
   avisos), `/gerar_amanha` (monta a playlist), `/pular`, `/voltar_grade`,
   `/reiniciar_saida`.
6. Cada ação chama as mesmas funções que a API já usa, sem lógica nova.

## Testes

- `client.rs`: contra um Telegram falso (`fake.rs`), incluindo o token que nunca vaza.
- `monitor.rs`: as regras do `Tracker` e dos problemas são funções puras; os testes com
  banco simulam saída que cai, comerciais e resumo.
- `bot.rs`: comando → resposta, grupo desconhecido, comandos desligados, partida do bot.
- `tests/api/telegram.rs`: permissões, token nunca devolvido, validações.

## Limites conhecidos

- O token é guardado em texto no SQLite, como a senha do SMTP e do ntfy.
- Um grupo por canal. Dois canais no mesmo grupo não são suportados (o comando
  responde pelo primeiro).
- Se outro programa (ou outra instalação) escutar o mesmo bot, o Telegram recusa com
  `409`: o bot espera 1 min e tenta de novo, e registra o motivo no log uma vez.
- Sem internet na VPS, o bot não avisa nada (e os avisos desse período se perdem).
  Os avisos por e-mail e ntfy continuam existindo em paralelo.
