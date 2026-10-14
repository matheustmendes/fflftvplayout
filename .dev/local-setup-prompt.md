# Prompt para o Claude Code local: preparar o ambiente

Copie o bloco abaixo e cole no Claude Code rodando na sua máquina Linux,
dentro de uma pasta onde você quer o projeto.

---

```text
Quero preparar este ambiente Linux para desenvolver e rodar o ffplayout
(playout 24/7 em Rust + FFmpeg). O repositório é
https://github.com/matheustmendes/fflftvplayout e a branch com as notas do
projeto é `claude/practical-davinci-v5r3ah`.

Regras para esta tarefa:
- Peça confirmação antes de qualquer comando com sudo, de instalar pacotes
  ou de adicionar repositórios/PPAs. Mostre o comando antes de rodar.
- Não use sudo para compilar. Não altere configurações do sistema além do
  necessário (pacotes, rustup, nvm/node).
- Não faça push, não abra PR e não mexa em branches que eu não pedi.
- Se algo falhar, mostre o erro e proponha a correção; não contorne com
  workarounds silenciosos.

Passos:

1. Diagnóstico (somente leitura). Reporte: distro e versão (`/etc/os-release`),
   arquitetura, CPU/RAM/disco livre, e o que já está instalado: git, rustc e
   cargo, node e npm, ffmpeg, pkg-config, clang, libclang, e as versões de
   libavformat/libavcodec/libavutil via `pkg-config --modversion`.
   O projeto exige FFmpeg >= 7.0 com headers de desenvolvimento
   (libavformat >= 61), Rust stable e Node 24.

2. Clonar e preparar a branch.
   - `git clone https://github.com/matheustmendes/fflftvplayout.git`
   - `git fetch origin claude/practical-davinci-v5r3ah`
   - Criar a branch local `feat/setup-local` a partir dela. Não trabalhar
     direto em `claude/*`: essa branch é do Claude na nuvem.
   - Ler `CLAUDE.md`, `backend/AGENTS.md` e `.dev/scheduler.md`.

3. Instalar o que faltar, só depois de eu confirmar:
   - Pacotes (Debian/Ubuntu): ffmpeg pkg-config clang libclang-dev
     libsqlite3-dev libasound2-dev libavcodec-dev libavdevice-dev
     libavfilter-dev libavformat-dev libavutil-dev libswresample-dev
     libswscale-dev. Opcional: libsrt-openssl-dev (logs nativos do SRT).
     Em outras distros, use os pacotes equivalentes.
   - Se o FFmpeg disponível for < 7.0, me apresente as opções (PPA
     ppa:ubuntuhandbook1/ffmpeg7, outra distro/versão, ou compilar em
     /usr/local com PKG_CONFIG_PATH) e espere eu escolher.
   - Rust stable via rustup (com rustfmt e clippy) e Node 24 (nvm ou
     NodeSource), se ainda não estiverem corretos.

4. Build.
   - `npm ci`
   - `cargo build --release --package ffplayout`
   Informe o tempo gasto. O build com LTO é pesado; se a máquina tiver pouca
   RAM, use `CARGO_BUILD_JOBS=2`.

5. Verificação.
   - `cargo fmt --all -- --check`
   - `cargo clippy --all-features --all-targets -- --deny warnings`
   - `cargo test`
   Reporte o que passou e o que falhou, com a saída dos erros. Testes que
   dependam de mídia ou rede podem falhar; diga quais e por quê.

6. Primeira execução (teste de fumaça).
   - Criar diretórios de trabalho dentro da pasta do projeto, por exemplo
     `./run/{storage,playlists,public,logs}`.
   - Rodar `./target/release/ffplayout -l 127.0.0.1:8787` em background e
     confirmar que a porta responde (`curl -sI http://127.0.0.1:8787`).
   - Me dizer para abrir http://127.0.0.1:8787 e completar o setup inicial
     (cria as configurações globais e o primeiro admin). Use as pastas de
     `./run` quando o setup perguntar. Não crie o admin por mim com senha
     que eu não tenha escolhido.
   - Se houver mídia de teste em `tests_assets/`, indique como usá-la.

7. Relatório final em português, curto:
   - versões instaladas (OS, Rust, Node, FFmpeg),
   - resultado do build, do clippy e dos testes,
   - como iniciar e parar o playout,
   - o que ficou pendente ou precisa de decisão minha.

Ao terminar, faça commit apenas de arquivos novos que façam sentido versionar
(por exemplo um `scripts/dev-setup.sh` se você criar um) na branch
`feat/setup-local`, sem push. Me mostre o diff antes.
```

---

## Notas

- O prompt é neutro quanto à distro: o passo 1 detecta o sistema e o passo 3
  só instala depois da sua confirmação.
- Para a VPS (futuro): o caminho recomendado é gerar o `.deb` com
  `scripts/build.sh` (Docker) e instalar com `apt install`; o serviço systemd
  e o modelo de nginx estão em `assets/`. Isso fica para uma etapa seguinte.
- Combinado entre os dois Claudes: a nuvem trabalha em `claude/*`, o local em
  `feat/*`. Integração por Git; detalhes no `CLAUDE.md`.
