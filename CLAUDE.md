# CLAUDE.md

Fork do [ffplayout](https://github.com/ffplayout/ffplayout): playout 24/7 em
Rust + FFmpeg com frontend Vue. Este arquivo vale para o Claude Code local e
para o Claude Code na nuvem.

## Fluxo entre Claude local e Claude na nuvem

- O único meio compartilhado é o Git (GitHub `matheustmendes/fflftvplayout`).
  Nada fica salvo fora de commits enviados por push.
- Cada ambiente trabalha na própria branch:
  - nuvem: `claude/*` (a branch designada pela sessão)
  - local: `feat/*`, `fix/*` ou outra branch definida pelo usuário
- Nunca editar a mesma branch nos dois ambientes ao mesmo tempo.
- Antes de começar: `git fetch` e partir da branch combinada.
- Integração na `main` via PR, aberto apenas quando o usuário pedir.
- Decisões, pendências e análises que precisam sobreviver entre sessões
  vão para `.dev/` (notas) ou para este arquivo (convenções).
- Segredos (`.env`, senhas, chaves) nunca entram no repositório.

### Divisão de trabalho

- Nuvem: mudanças de código validáveis por `cargo fmt`, `clippy` e testes,
  refatorações, revisão e documentação.
- Local: execução real do playout (FFmpeg 7+, mídia, RTMP/SRT, HLS), serviço
  systemd, nginx e deploy na VPS.

## Build e verificação

```sh
npm ci && npm run build-only                  # frontend
cargo build --release --package ffplayout     # binário em target/release/ffplayout
cargo fmt --all --
cargo clippy --all-features --all-targets -- --deny warnings
cargo test
```

Requer FFmpeg >= 7.0 (libavformat >= 61) com headers de desenvolvimento,
`pkg-config`, `clang`/`libclang-dev`, Rust stable e Node 24.

## Regras de código

As regras do backend estão em `backend/AGENTS.md` e são obrigatórias:

@backend/AGENTS.md

## Mapa rápido

- `backend/app/`: HTTP/API, banco (SQLite, queries só em `db/handles/`),
  agendamento da playlist (`player/`), configuração (`utils/config.rs`).
- `backend/engine/`: decodificação, composição, encoding e saídas (FFmpeg).
- `frontend/`: UI web (Vue + Vite).
- `migrations/`: SQL. Não editar migrations já publicadas; criar novas.
- `.dev/`: notas de desenvolvimento e backlog.
- Agendador e controle de horário: ver `.dev/scheduler.md`.
