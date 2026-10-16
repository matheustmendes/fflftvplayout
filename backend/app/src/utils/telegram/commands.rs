//! The commands the group can send to the bot. All of them only read.
//!
//! Each command says who may use it ([`Access`]). Commands that change what is
//! on the air will be a third level that also asks who is sending, and are not
//! here yet: see `.dev/telegram.md`.

use chrono::{Datelike, Duration, NaiveDate, Weekday};
use sqlx::SqlitePool;

use super::{
    messages::{self, esc, truncate},
    monitor::{count_airings, counts_text, day_airings},
    snapshot::{Snapshot, collect},
};
use crate::{
    db::models::AirResult,
    player::controller::ChannelManager,
    utils::{auto_plan::days_without_playlist, health::ConnectionState, system::SystemStat},
};

const DEFAULT_LISTED: usize = 5;
const MAX_LISTED: usize = 15;
const COVERAGE_DAYS: i64 = 7;

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum Access {
    /// Any chat, even one no channel talks to. Only for what is needed to set
    /// the bot up.
    Anyone,
    /// Only the chat of a channel, and only when the channel allows commands.
    Chat,
}

pub struct CommandSpec {
    pub name: &'static str,
    pub help: &'static str,
    pub access: Access,
}

pub const COMMANDS: &[CommandSpec] = &[
    CommandSpec {
        name: "status",
        help: "como está o canal agora",
        access: Access::Chat,
    },
    CommandSpec {
        name: "proximos",
        help: "os próximos itens da programação (ex.: /proximos 8)",
        access: Access::Chat,
    },
    CommandSpec {
        name: "destinos",
        help: "quais destinos estão conectados",
        access: Access::Chat,
    },
    CommandSpec {
        name: "hoje",
        help: "como estão os comerciais de hoje",
        access: Access::Chat,
    },
    CommandSpec {
        name: "amanha",
        help: "a playlist de amanhã está pronta?",
        access: Access::Chat,
    },
    CommandSpec {
        name: "cobertura",
        help: "playlists dos próximos 7 dias",
        access: Access::Chat,
    },
    CommandSpec {
        name: "id",
        help: "o número deste chat, para configurar o bot",
        access: Access::Anyone,
    },
    CommandSpec {
        name: "ajuda",
        help: "esta lista",
        access: Access::Anyone,
    },
];

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum Command {
    Help,
    Id,
    Status,
    Next(usize),
    Destinations,
    Today,
    Tomorrow,
    Coverage,
}

impl Command {
    pub fn access(self) -> Access {
        match self {
            Self::Help | Self::Id => Access::Anyone,
            _ => Access::Chat,
        }
    }
}

/// Without accents and in lower case, so `/amanhã` and `/AMANHA` are one.
fn plain(word: &str) -> String {
    word.to_lowercase()
        .chars()
        .map(|c| match c {
            'á' | 'à' | 'â' | 'ã' => 'a',
            'é' | 'ê' => 'e',
            'í' => 'i',
            'ó' | 'ô' | 'õ' => 'o',
            'ú' | 'ü' => 'u',
            'ç' => 'c',
            other => other,
        })
        .collect()
}

/// The command in a message, or `None` when it is not one for this bot.
pub fn parse(text: &str, bot_name: &str) -> Option<Command> {
    let mut words = text.split_whitespace();
    let first = words.next()?.strip_prefix('/')?;
    let (name, addressed_to) = match first.split_once('@') {
        Some((name, bot)) => (name, Some(bot)),
        None => (first, None),
    };

    // In a group, a command for another bot is not ours.
    if addressed_to.is_some_and(|bot| !bot.eq_ignore_ascii_case(bot_name)) {
        return None;
    }

    Some(match plain(name).as_str() {
        "start" | "ajuda" | "help" => Command::Help,
        "id" => Command::Id,
        "status" => Command::Status,
        "proximos" => Command::Next(
            words
                .next()
                .and_then(|word| word.parse::<usize>().ok())
                .unwrap_or(DEFAULT_LISTED)
                .clamp(1, MAX_LISTED),
        ),
        "destinos" => Command::Destinations,
        "hoje" => Command::Today,
        "amanha" => Command::Tomorrow,
        "cobertura" => Command::Coverage,
        _ => return None,
    })
}

/// What a chat that no channel talks to is told.
pub fn unknown_chat_message(chat_id: i64) -> String {
    format!(
        "Este chat ainda não está ligado a um canal.\n\
         O número dele é <code>{chat_id}</code>.\n\
         Cole esse número em <b>Ajustes → Telegram</b> no painel e ligue o bot."
    )
}

pub fn help_message() -> String {
    let mut text = "🤖 <b>Comandos</b>\n".to_string();

    for command in COMMANDS {
        text.push_str(&format!("/{} — {}\n", command.name, command.help));
    }

    text
}

fn mark(ok: bool) -> &'static str {
    if ok { "✅" } else { "❌" }
}

fn weekday_name(day: Weekday) -> &'static str {
    match day {
        Weekday::Mon => "seg",
        Weekday::Tue => "ter",
        Weekday::Wed => "qua",
        Weekday::Thu => "qui",
        Weekday::Fri => "sex",
        Weekday::Sat => "sáb",
        Weekday::Sun => "dom",
    }
}

fn header(snapshot: &Snapshot) -> String {
    format!(
        "<b>{}</b> · {}",
        esc(&snapshot.name),
        messages::clock_text(&snapshot.local(snapshot.now))
    )
}

fn status_message(snapshot: &Snapshot) -> String {
    let mut text = format!("{}\n", header(snapshot));

    text.push_str(if snapshot.alive {
        "🟢 No ar\n"
    } else if snapshot.active {
        "🔴 Fora do ar — o sistema tenta reiniciar\n"
    } else {
        "⚪ Parado (desligado pelo operador)\n"
    });

    if snapshot.live {
        text.push_str("📡 Ao vivo no ar\n");
    }

    if let Some(current) = &snapshot.current {
        let kind = if current.ad { " (comercial)" } else { "" };

        text.push_str(&format!(
            "▶️ Agora: {}{kind} — {} de {}\n",
            esc(&truncate(&current.title, 60)),
            messages::duration_text(current.elapsed),
            messages::duration_text(current.duration)
        ));
    }

    if let Some(next) = snapshot.upcoming.first() {
        let at = snapshot.local(snapshot.now + Duration::seconds(next.starts_in as i64));

        text.push_str(&format!(
            "⏭ Em seguida: {} às {}\n",
            esc(&truncate(&next.title, 60)),
            messages::short_clock(&at)
        ));
    }

    if snapshot.relay.configured {
        if snapshot.relay.reachable {
            let connected = snapshot
                .relay
                .destinations
                .iter()
                .filter(|destination| destination.state == ConnectionState::Connected)
                .count();

            text.push_str(&format!(
                "📺 Destinos: {connected} de {} conectados\n",
                snapshot.relay.destinations.len()
            ));
        } else {
            text.push_str("📺 Destinos: relay sem resposta\n");
        }
    }

    text.push_str(&format!(
        "Arquivo de reserva: {}\n",
        mark(snapshot.filler.ok)
    ));

    if snapshot.playlist_mode {
        let tomorrow = snapshot.air_date + Duration::days(1);

        text.push_str(&format!(
            "Playlists: hoje {} · amanhã {}\n",
            mark(!snapshot.missing_days.contains(&snapshot.air_date)),
            mark(!snapshot.missing_days.contains(&tomorrow))
        ));
    }

    if let Some(disk) = snapshot.disk {
        text.push_str(&format!("Disco livre: {:.0}%\n", disk.free_percent()));
    }

    text
}

fn upcoming_message(snapshot: &Snapshot, count: usize) -> String {
    let mut text = format!("{}\n", header(snapshot));

    if snapshot.upcoming.is_empty() {
        text.push_str("Não há itens depois do atual na playlist carregada.\n");

        return text;
    }

    text.push_str("<b>Próximos itens</b>\n");

    for item in snapshot.upcoming.iter().take(count) {
        let at = snapshot.local(snapshot.now + Duration::seconds(item.starts_in as i64));
        let kind = if item.ad { " 🏷" } else { "" };

        text.push_str(&format!(
            "{} · {}{kind} ({})\n",
            messages::clock_text(&at),
            esc(&truncate(&item.title, 50)),
            messages::duration_text(item.duration)
        ));
    }

    text
}

fn destinations_message(snapshot: &Snapshot) -> String {
    let mut text = format!("{}\n", header(snapshot));

    if !snapshot.relay.configured {
        text.push_str("O monitoramento de destinos não está configurado para este canal.\n");

        return text;
    }

    if !snapshot.relay.reachable {
        text.push_str("🔴 O relay não respondeu, então não sei o estado dos destinos.\n");

        return text;
    }

    if snapshot.relay.destinations.is_empty() {
        text.push_str("Nenhum destino cadastrado para acompanhar.\n");
    }

    for destination in &snapshot.relay.destinations {
        let light = match destination.state {
            ConnectionState::Connected => "🟢",
            ConnectionState::Disconnected => "🔴",
            ConnectionState::Unknown => "⚪",
        };

        text.push_str(&format!("{light} {}\n", esc(&destination.name)));
    }

    if snapshot.relay.others > 0 {
        text.push_str(&format!(
            "+ {} leitor(es) que não são destinos da lista\n",
            snapshot.relay.others
        ));
    }

    text
}

async fn today_message(pool: &SqlitePool, snapshot: &Snapshot) -> String {
    let rows = day_airings(pool, snapshot.channel_id, snapshot.air_date).await;
    let mut text = format!(
        "{}\n<b>Comerciais de hoje</b>\n{}\n",
        header(snapshot),
        counts_text(&count_airings(&rows))
    );

    let next = rows
        .iter()
        .filter(|row| row.result == AirResult::Pending)
        .filter_map(|row| {
            let start = chrono::DateTime::parse_from_rfc3339(row.planned_start.as_deref()?).ok()?;

            (start.with_timezone(&chrono::Utc) >= snapshot.now).then_some((start, row))
        })
        .min_by_key(|(start, _)| *start);

    if let Some((start, row)) = next {
        text.push_str(&format!(
            "Próximo: {} · {} — {}\n",
            messages::short_clock(&start.with_timezone(&snapshot.timezone)),
            esc(&row.advertiser_name),
            esc(&row.spot_title)
        ));
    }

    text
}

fn tomorrow_message(snapshot: &Snapshot) -> String {
    let tomorrow = snapshot.air_date + Duration::days(1);
    let date = messages::date_text(tomorrow);

    if !snapshot.playlist_mode {
        return format!(
            "{}\nEste canal não usa playlist por dia.\n",
            header(snapshot)
        );
    }

    if snapshot.missing_days.contains(&tomorrow) {
        format!(
            "{}\n❌ Ainda não há playlist para amanhã ({date}).\n\
             O planejamento automático tenta montar de tempos em tempos; se não montar, \
             veja a aba Hoje em Publicidade.\n",
            header(snapshot)
        )
    } else {
        format!(
            "{}\n✅ A playlist de amanhã ({date}) está montada.\n",
            header(snapshot)
        )
    }
}

async fn coverage_message(manager: &ChannelManager, snapshot: &Snapshot) -> String {
    let config = manager.config.read().await.clone();
    let missing: Vec<NaiveDate> =
        days_without_playlist(&config, snapshot.now, COVERAGE_DAYS - 1).await;
    let mut text = format!("{}\n<b>Playlists dos próximos dias</b>\n", header(snapshot));

    for offset in 0..COVERAGE_DAYS {
        let date = snapshot.air_date + Duration::days(offset);

        text.push_str(&format!(
            "{} {} {}\n",
            mark(!missing.contains(&date)),
            weekday_name(date.weekday()),
            messages::date_text(date)
        ));
    }

    text
}

/// The answer to a command of a chat that belongs to a channel.
pub async fn execute(
    command: Command,
    manager: &ChannelManager,
    pool: &SqlitePool,
    system: &SystemStat,
) -> String {
    let upcoming = match command {
        Command::Next(count) => count,
        _ => 1,
    };

    match command {
        Command::Help => help_message(),
        Command::Id => String::new(),
        Command::Status => status_message(&collect(manager, pool, system, upcoming).await),
        Command::Next(count) => {
            upcoming_message(&collect(manager, pool, system, upcoming).await, count)
        }
        Command::Destinations => {
            destinations_message(&collect(manager, pool, system, upcoming).await)
        }
        Command::Today => {
            today_message(pool, &collect(manager, pool, system, upcoming).await).await
        }
        Command::Tomorrow => tomorrow_message(&collect(manager, pool, system, upcoming).await),
        Command::Coverage => {
            coverage_message(manager, &collect(manager, pool, system, upcoming).await).await
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn commands_are_read_with_or_without_accents_and_case() {
        assert_eq!(parse("/status", "bot"), Some(Command::Status));
        assert_eq!(parse("/STATUS", "bot"), Some(Command::Status));
        assert_eq!(parse("/amanhã", "bot"), Some(Command::Tomorrow));
        assert_eq!(parse("/amanha", "bot"), Some(Command::Tomorrow));
        assert_eq!(parse("/próximos", "bot"), Some(Command::Next(5)));
        assert_eq!(parse("/start", "bot"), Some(Command::Help));
    }

    #[test]
    fn the_number_of_items_is_limited() {
        assert_eq!(parse("/proximos 8", "bot"), Some(Command::Next(8)));
        assert_eq!(
            parse("/proximos 500", "bot"),
            Some(Command::Next(MAX_LISTED))
        );
        assert_eq!(parse("/proximos 0", "bot"), Some(Command::Next(1)));
        assert_eq!(parse("/proximos abc", "bot"), Some(Command::Next(5)));
    }

    #[test]
    fn a_command_for_another_bot_is_not_ours() {
        assert_eq!(
            parse("/status@emissora_bot", "Emissora_Bot"),
            Some(Command::Status)
        );
        assert_eq!(parse("/status@outro_bot", "emissora_bot"), None);
    }

    #[test]
    fn what_is_not_a_command_is_ignored() {
        assert_eq!(parse("bom dia", "bot"), None);
        assert_eq!(parse("/desconhecido", "bot"), None);
        assert_eq!(parse("", "bot"), None);
        assert_eq!(parse("/", "bot"), None);
    }

    #[test]
    fn only_the_setup_commands_work_for_any_chat() {
        for command in [
            Command::Status,
            Command::Next(3),
            Command::Destinations,
            Command::Today,
            Command::Tomorrow,
            Command::Coverage,
        ] {
            assert_eq!(command.access(), Access::Chat, "{command:?}");
        }

        assert_eq!(Command::Id.access(), Access::Anyone);
        assert_eq!(Command::Help.access(), Access::Anyone);
    }

    #[test]
    fn every_command_of_the_list_can_be_parsed() {
        for spec in COMMANDS {
            let parsed = parse(&format!("/{}", spec.name), "bot")
                .unwrap_or_else(|| panic!("/{} is listed but not understood", spec.name));

            assert_eq!(parsed.access(), spec.access, "/{}", spec.name);
        }
    }

    #[test]
    fn the_help_lists_every_command() {
        let help = help_message();

        for spec in COMMANDS {
            assert!(help.contains(&format!("/{}", spec.name)), "{help}");
        }
    }

    #[test]
    fn the_unknown_chat_is_told_its_number() {
        assert!(unknown_chat_message(-100123).contains("<code>-100123</code>"));
    }
}
