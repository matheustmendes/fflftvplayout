# Prompt para o Claude Code na VPS

Cole o texto abaixo (a partir de "Você é...") numa sessão do Claude Code aberta **na
própria VPS** (por SSH, dentro de `tmux`) ou na máquina local com acesso SSH à VPS.
Ele também pode ser lido do repositório: `.dev/vps-session-brief.md`.

---

Você é o Claude Code e vai instalar, configurar e validar o ffplayout nesta VPS (Ubuntu
ou Debian, recém-criada). O projeto é um fork do ffplayout (playout 24/7 em Rust + FFmpeg
com painel Vue) para uma emissora de TV: um canal, 1080p30 a 3000 kbps, cerca de 12
destinos puxando o sinal (a maioria por m3u8, poucos por RTMP/SRT). O dono não é
programador: explique em português simples, sem jargão, e faça as perguntas que faltarem
antes de agir.

## Antes de tudo, leia
1. Clone a branch `claude/practical-davinci-v5r3ah` de
   `https://github.com/matheustmendes/fflftvplayout.git` (a `main` ainda não tem o
   script) e leia: `CLAUDE.md`, `deploy/README.md`, `deploy/install.sh`,
   `deploy/ftp-import.sh`, `.dev/telegram.md`, `.dev/acceptance/F8.md`.
2. Confira o ambiente: `lsb_release -a`, CPU, RAM, disco livre, IP público, e se os
   domínios do painel e do sinal já apontam para este IP (`dig +short`).

## Pergunte ao usuário (todas de uma vez, e espere)
- Domínio do painel e domínio do sinal (m3u8), e e-mail para o certificado.
- Quantos destinos terão usuário e senha no relay (padrão 12).
- Se o snapshot da VPS já foi tirado (peça para tirar antes de instalar).
- Dados do FTP (servidor, protocolo ftp/ftps/sftp, usuário, senha, pasta), tamanho total
  e se chega conteúdo novo depois. A senha **nunca** deve aparecer na tela, no log nem
  no repositório: peça para o usuário editá-la ele mesmo em `/etc/ffplayout/ftp-import.conf`.
- O token do bot do Telegram e o número do grupo, se for configurar agora (idem: o token
  é segredo; entre pelo painel ou peça para o usuário colar direto no campo).

## Instalar
1. Rode dentro do `tmux`, com log: 
   `sudo deploy/install.sh install --panel-domain … --signal-domain … --email … --destinations N 2>&1 | tee ~/install.log`
2. O `install.sh` **nunca rodou numa VPS de verdade** (só partes isoladas foram testadas
   na nuvem). Se ele falhar, leia o erro, descubra a causa real e conserte o script. Rode
   de novo: ele é idempotente. Não contorne com comandos manuais sem registrar o que fez.
3. Correções do script vão numa branch `fix/vps-deploy` (não use `claude/*`, que é da
   sessão da nuvem). Faça commit; só faça push se houver credencial do GitHub, senão mostre
   o `git diff` ao usuário para ele levar de volta.
4. O build usa Rust estável novo: se o `clippy`/build reclamar de uma regra nova, corrija
   o código no mínimo necessário e avise.

## Validar (marque cada item como OK/FALHOU com a evidência)
- `deploy/install.sh status`: `ffplayout`, `mediamtx`, `nginx` e o timer do backup ativos.
- Portas em escuta: 80, 443, 1935/TCP, 8890/UDP; a 8787 só em 127.0.0.1.
- `https://PAINEL/` abre com certificado válido e o login funciona com a senha de
  `/root/ffplayout-credenciais.txt`. A renovação do certificado: `certbot renew --dry-run`.
- Painel > Ajustes > Transmissão: saída **Stream** para `rtmp://127.0.0.1:1935/canal1`,
  1080p 30 fps, 3000 kbps. Painel > Destinos: API `http://127.0.0.1:9997`, canal `canal1`,
  um destino por usuário (`destino1`…).
- Crie uma programação de teste com mídia de amostra (gere com `ffmpeg -f lavfi` se não
  houver) e ponha o canal no ar. Depois confira, com `ffprobe`, que o sinal chega pelos
  três caminhos usando as credenciais do arquivo: m3u8 (`https://SINAL/canal1/index.m3u8`
  com o usuário e a senha do nginx), RTMP e SRT.
- Derrube o ffmpeg do canal (`kill`) e veja: o supervisor reinicia em até 10 s, o semáforo
  do Painel mostra o destino, e (se o Telegram estiver ligado) chega o aviso.
- `systemctl restart ffplayout`: quanto tempo o sinal fica fora (meça) e se volta sozinho.
- CPU: rode `top` por alguns minutos com o canal no ar e anote o **steal** (`st`); acima
  de uns 5% constante é problema para o canal. Anote também o uso de banda.
- Backup: `systemctl start ffplayout-backup.service` e confira o arquivo em
  `/var/backups/ffplayout`.

## FTP (só depois de o canal estar estável)
`sudo deploy/ftp-import.sh init`, o usuário preenche a senha, depois: `test`, `size`
(compare com o disco livre e avise se não cabe), `dry-run`, `start`, `logs`, `verify`.
Nunca ligue `DELETE_REMOVED`. Use `LIMIT_MBIT` (por exemplo 80) enquanto o canal estiver no
ar. Os comerciais precisam ser **registrados** na tela de Comerciais (ou
`POST /api/commercials/1/spots`) depois de chegarem ao storage.

## Telegram (se o usuário passar o token)
Salve o token pelo painel (Ajustes > Telegram) com o administrador geral, mande `/id` no
grupo, cole o número, ligue o bot e use "Enviar mensagem de teste". Rode o roteiro
`.dev/acceptance/F8.md` no que for possível.

## Regras
- Nunca imprima senhas, tokens ou o conteúdo de `/root/ffplayout-credenciais.txt` na
  conversa; diga só onde está. Nada de segredo no Git.
- Não abra portas além de 22, 80, 443, 1935/TCP e 8890/UDP; não desligue o firewall.
- Pergunte antes de qualquer ação destrutiva (apagar dados, reinstalar, mexer em disco).
- Não faça `git push --force` nem mexa na `main`.
- Se algo deixar o canal fora do ar, avise na hora.

## Relatório final (em português simples)
1. O que está funcionando (com a evidência).
2. O que falhou ou não deu para testar, e por quê.
3. O que ainda depende do usuário (DNS, token, FTP, destinos reais).
4. O que você mudou no repositório (arquivos e motivo).
5. Riscos que sobraram: CPU compartilhada (steal), tráfego mensal "ilimitado*" da
   Contabo (cerca de 12 TB/mês), destinos que só puxam RTMP.
