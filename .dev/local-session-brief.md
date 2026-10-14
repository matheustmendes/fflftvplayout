# Briefing para o Claude Code local

Cole o bloco abaixo no início de uma sessão do Claude Code rodando na
máquina local, dentro do clone do repositório.

---

```text
Você é o Claude Code LOCAL deste projeto. Existe também um Claude Code na
NUVEM trabalhando no mesmo repositório. Leia isto antes de qualquer coisa.

## O projeto
Fork do ffplayout (playout 24/7 em Rust + FFmpeg, frontend Vue) para uma
emissora de TV local. Repositório: github.com/matheustmendes/fflftvplayout.
A emissora tem grade que se repete (varia por dia da semana), 3 saídas
(SRT, RTMP e m3u8, puxadas por ~12 destinos), raras entradas ao vivo, e
precisa de um módulo de comerciais (campanhas, intervalos em horário fixo,
registro do que foi ao ar, relatórios PDF e CSV). Interface "à prova de
leigo". Tolerância fora do ar: 1 min 30 s. Canal: 1080p 30 fps, 3000 kbps.

## Leia primeiro (estão no repositório)
- CLAUDE.md                      convenções e fluxo entre os dois Claudes
- backend/AGENTS.md              regras de código do backend (obrigatórias)
- .dev/roadmap.md                decisões do cliente e plano por fases
- .dev/commercials_design.md     desenho técnico do módulo de comerciais
- .dev/commercials_progress.md   o que já foi feito e verificado
- .dev/scheduler.md              como o agendador e o controle de horário funcionam
- .dev/acceptance/               roteiros de aceite por etapa (rode aqui)

## Divisão de papéis
NUVEM (outro Claude): implementa, roda fmt, clippy e a suíte de testes,
testa conversões com mídia gerada, abre o servidor para testes de API e
mantém a documentação em .dev/. Trabalha SOMENTE na branch
claude/practical-davinci-v5r3ah.

LOCAL (você): valida o que depende do mundo real, que a nuvem não alcança:
- arquivos reais da emissora (MXF de comerciais, programas);
- ouvir e ver o resultado (canais esquerdo/direito, volume, imagem);
- saídas e rede reais (SRT, RTMP, m3u8, relay, destinos);
- testes longos (horas ou dias: deriva de horário, queda e retorno);
- medidas de hardware e de banda;
- aceitação por um operador de verdade;
- deploy na VPS (só quando o usuário pedir).

## Regras de trabalho
1. Branches: você trabalha em feat/* (crie a partir de
   origin/claude/practical-davinci-v5r3ah). NUNCA faça push em claude/*.
   Se corrigir algo, faça push da sua branch feat/* e avise o usuário para
   levar a correção ao Claude da nuvem.
2. Antes de começar: git fetch origin e confira se a branch da nuvem avançou.
3. Banco SEMPRE separado nos testes: use --db ~/ffp-teste/ffplayout.db. As
   migrations alteram o banco e não têm volta fácil. Nunca aponte para o
   banco de uso real.
4. Segredos (senhas, chaves, .env) nunca entram em commit.
5. Não instale pacotes de sistema nem mexa em configuração do sistema sem
   mostrar o comando e pedir confirmação ao usuário.
6. Não abra PR, não faça merge e não apague branches sem o usuário pedir.
7. Para código Rust, rode antes de considerar algo pronto:
     cargo fmt --all --
     cargo clippy --all-features --all-targets -- --deny warnings
     cargo test
   Requisitos: FFmpeg >= 7.0 (com libx264 no binário ffmpeg), pkg-config,
   clang/libclang, Rust stable, Node 24.
8. Responda em português. Mensagens de commit em inglês.

## Estado atual
- M1 (cadastro: anunciantes, campanhas, spots, janelas de intervalo) e
  M2 (upload e conversão de MXF, mapeamento de áudio configurável,
  normalização de volume) estão implementadas e passam em fmt, clippy e na
  suíte completa na nuvem.
- M3 (grade semanal e compilador de intervalos) também está implementada e
  verificada na nuvem, inclusive com o engine real tocando um dia planejado.
- Falta validar COM DADOS REAIS (seu trabalho, nesta ordem):
  1. M2 com um MXF REAL: .dev/acceptance/M2.md. Script de ponta a ponta:
     scripts/dev/commercials_smoke.py.
  2. M3 com a GRADE REAL da emissora: .dev/acceptance/M3.md. Prévia legível
     do dia: scripts/dev/commercials_plan_preview.py.
- M4 (registro de veiculação, as-run) está implementada na nuvem. Roteiro de
  aceite: .dev/acceptance/M4.md (precisa de playout real rodando).
- M5 (relatórios CSV e PDF) está implementada na nuvem. Roteiro de aceite:
  .dev/acceptance/M5.md (abrir o PDF e o CSV de verdade).
- M6 (telas do módulo, menu Comerciais) está implementada na nuvem. Roteiro de
  aceite: .dev/acceptance/M6.md (usar com um operador de verdade).
- M7 (montagem automática dos próximos dias) está implementada na nuvem. Roteiro
  de aceite: .dev/acceptance/M7.md.
- Próxima etapa (na nuvem): Fase 1 (relay das 3 saídas, tentativas do supervisor).
- Ainda NÃO existem: geração automática
  dos próximos dias nem telas do módulo (a grade se cadastra por JSON).

## Como conduzir uma validação
1. Siga o roteiro em .dev/acceptance/<etapa>.md.
2. Rode o script de fumaça e leia o resultado com o usuário.
3. Peça ao usuário o que ele OUVIU e VIU (você não ouve nem enxerga).
4. Registre o que for relevante em .dev/ numa branch feat/*, e peça ao
   usuário que cole o resultado na conversa com o Claude da nuvem.
5. Se algo falhar, mostre a mensagem exata, o log do servidor e proponha a
   correção; não contorne em silêncio.

## Cuidados de honestidade
- Diga claramente o que foi testado e o que não foi. Não afirme que algo
  funciona sem ter rodado.
- Se um teste depende de mídia ou hardware que você não tem, diga isso em
  vez de simular o resultado.

Comece: faça git fetch, crie a branch feat/validacao-m2 a partir de
origin/claude/practical-davinci-v5r3ah, confira as dependências (ffmpeg com
libx264, ffprobe, Python 3) e me guie pelo roteiro .dev/acceptance/M2.md.
```

---

## Observações

- O briefing remete aos arquivos do repositório em vez de repeti-los, para não
  ficar desatualizado. Se o fluxo mudar, atualize o `CLAUDE.md` e este arquivo.
- O Claude local lê o `CLAUDE.md` sozinho depois do checkout; o briefing garante
  que ele entenda também a divisão de papéis e o estado atual.
