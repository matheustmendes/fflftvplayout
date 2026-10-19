# Prompt para o Claude Code na VPS: importar os vídeos do YouTube da LFTV

Cole o texto abaixo (a partir de "Você é...") numa sessão do Claude Code aberta **na
própria VPS** (por SSH, dentro de `tmux`), com o repositório já clonado na branch
`claude/practical-davinci-v5r3ah`. Ele também pode ser lido do repositório:
`.dev/vps-youtube-brief.md`.

---

Você é o Claude Code e vai trazer para esta VPS os vídeos e playlists do canal da LFTV no
YouTube (https://www.youtube.com/@LFTVOficial/playlists), usando o script
`deploy/youtube-import.sh` do repositório. O dono não é programador: explique em português
simples, sem jargão, e pergunte antes de qualquer ação destrutiva.

## Antes de tudo
1. Leia `deploy/youtube-import.sh` e a seção 7 de `deploy/README.md`.
2. Confira o ambiente: espaço livre (`df -h /var/lib/ffplayout`), se o canal está no ar
   (`deploy/install.sh status`), e se há `node` ou `deno` (`node --version`). Sem nenhum
   dos dois, instale o Node (`sudo apt install nodejs`) e avise.
3. O script **nunca baixou nada de verdade**: só foi testada a listagem das playlists e a
   recusa de vídeo de outro canal. O YouTube bloqueou o IP da nuvem onde ele foi escrito,
   então o download, o formato 1080p, as pastas e o arquivo de "já baixados" ainda não
   foram vistos funcionando. Se falhar, descubra a causa real e conserte o script.

## Passo a passo (rode um por vez e mostre o resultado)
1. `sudo deploy/youtube-import.sh update` (instala o yt-dlp oficial).
2. `sudo deploy/youtube-import.sh init` e confira `/etc/ffplayout/youtube-import.conf`. Os
   padrões servem: fonte = playlists do canal, 1080p, pasta `/var/lib/ffplayout/storage/youtube`.
3. `sudo deploy/youtube-import.sh test`: todas as playlists devem aparecer como `ok`. Se
   alguma sair `RECUSADA`, **não baixe**: mostre ao usuário.
4. `sudo deploy/youtube-import.sh size`: compare a estimativa com o espaço livre. **Se não
   couber (deixe pelo menos 20% livre), pare e pergunte** ao usuário o que fazer (baixar só
   algumas playlists, `MAX_HEIGHT=720` ou `DATE_AFTER`). Não decida sozinho.
5. Teste com um vídeo antes de tudo: crie um `.conf` temporário
   (`FFP_YT_CONF=/tmp/yt-teste.conf`) com **uma** playlist pequena (por exemplo "LFTV pocket") e
   `LOCAL_DIR=/tmp/yt-teste`, e rode `FFP_YT_CONF=/tmp/yt-teste.conf sudo -E deploy/youtube-import.sh run`.
   Confira com `ffprobe`: MP4, vídeo H.264 de 1080p (ou o máximo que o vídeo tem) e áudio AAC;
   o nome do arquivo com acentos; a subpasta com o nome da playlist; e o `.baixados.txt`.
   Rode de novo e confirme que **não** baixa o que já tem.
6. Se aparecer "Sign in to confirm you're not a bot": o YouTube bloqueou o IP da VPS. Peça ao
   usuário um arquivo de cookies de uma conta logada (extensão "Get cookies.txt LOCALLY"),
   que ele mesmo deve colocar em `/etc/ffplayout/youtube-cookies.txt` (permissão 600) e você
   liga `COOKIES_FILE` no `.conf`. **Nunca peça nem imprima o conteúdo dos cookies.**
7. Com o teste bom, apague a pasta de teste e rode de verdade, **com limite de velocidade se
   o canal estiver no ar**: defina `LIMIT_MBIT=80` no `.conf`, depois
   `sudo deploy/youtube-import.sh start`. Acompanhe com `logs` e `status`.
8. Ao terminar: `sudo deploy/youtube-import.sh verify` (deve dizer que não falta nada) e confira
   o espaço livre. Os vídeos com falha ficam listados no log; rode `start` de novo para
   tentar só o que falta.
9. Pergunte se o usuário quer `install-timer` (busca vídeos novos todo dia às 5h).

## Regras
- Só o canal da LFTV Oficial (`UC9jfIeNHlejCauwxqZKOz-w`) é aceito: nunca mude
  `ALLOWED_CHANNEL_ID` nem tire o `--match-filter` do script.
- Não apague nada que já esteja em `/var/lib/ffplayout/storage` fora da pasta `youtube/`.
- Não abra portas nem mexa no firewall. Não pare o `ffplayout` nem o `mediamtx`.
- Mantenha o canal no ar: se a CPU ou a banda do sinal piorar durante o download, reduza
  `LIMIT_MBIT` e avise. O script já roda com prioridade baixa.
- Correções do script vão numa branch `fix/vps-youtube` (não use `claude/*`). Faça commit;
  só faça push se houver credencial do GitHub, senão mostre o `git diff`.
- Nada de segredo (cookies, senhas) no Git.

## Relatório final (em português simples)
1. O que foi baixado (quantidade de vídeos, horas, espaço usado e espaço que sobrou).
2. O que falhou e por quê (bloqueio, vídeo privado, erro de formato).
3. O que mudou no repositório (arquivos e motivo).
4. O que o usuário ainda precisa fazer (cookies, timer, cadastrar programas na grade).
