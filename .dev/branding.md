# Marca: LFTV

O sistema é apresentado à emissora como **LFTV**, um playout feito sob medida. O código
vem do projeto de código aberto ffplayout (licença GPL-3.0, ver `LICENSE`).

## O que mostra "LFTV"
- Painel: título da aba, tela de login, rodapé de estatísticas e textos de ajuda (pt-BR,
  en-US, de-DE e ru-RU).
- Mensagens que aparecem para o operador: erros de opções gerenciadas, título dos avisos
  por ntfy ("LFTV channel N …"), assunto e texto do e-mail com o código de verificação,
  título da janela do player de desktop, `--help` e as linhas de partida no log.
- Nome do arquivo do log baixado pelo painel (`lftv_<canal>_<data>.log`).
- Logos: `frontend/src/assets/images/lftv.png` (login) e `lftv-small.png` (cabeçalho). **São
  os logos antigos com outro nome**: troque os dois arquivos pelos da emissora, com o mesmo
  nome e as mesmas proporções. O ícone da aba é `frontend/public/favicon.ico`.

## O que continua "ffplayout" de propósito
Renomear estes quebraria a VPS que já está no ar e o `deploy/install.sh update`:
- o binário (`ffplayout`), o serviço do systemd (`ffplayout.service`), os timers e o
  backup, o usuário `ffpu`, as pastas (`/var/lib/ffplayout`, `/var/log/ffplayout`,
  `/usr/share/ffplayout`) e o banco (`ffplayout.db`);
- os nomes dos arquivos de log no servidor (`ffplayout_<canal>.log`), as variáveis de
  ambiente (`FFPLAYOUT_*`) e o nome do pacote (crate) no Rust;
- os comandos que as instruções pedem para digitar (`systemctl restart ffplayout`).

Se um dia a emissora quiser o nome técnico também, é uma migração à parte: renomear o
serviço e as pastas na VPS, mover os dados e ajustar `deploy/` junto.

## Licença
O ffplayout é GPL-3.0. Manter o arquivo `LICENSE` e os avisos de direito autoral dos
arquivos de origem. Usar o sistema internamente com outro nome é permitido; se um dia o
programa for entregue a terceiros, a GPL manda entregar também o código-fonte.
