//! The bot that listens: it reads what the group sends, answers the commands and
//! starts the monitor that sends the alerts.

use std::{
    collections::HashMap,
    sync::Arc,
    time::{Duration, Instant},
};

use log::{info, warn};
use sqlx::SqlitePool;
use tokio::{select, sync::RwLock, time::sleep};
use tokio_util::sync::CancellationToken;

use super::{
    client::{API_BASE, TelegramClient, Update},
    commands::{self, Access, Command},
    monitor::run_alerts,
};
use crate::{
    db::{handles, models::TelegramSettings},
    player::controller::ChannelController,
    utils::system::SystemStat,
};

/// Wait when there is no token to listen with.
const IDLE_WAIT: Duration = Duration::from_secs(30);
/// Wait after Telegram refused the token or another program is listening.
const REFUSED_WAIT: Duration = Duration::from_secs(60);
/// Wait after the network failed.
const NETWORK_WAIT: Duration = Duration::from_secs(10);
/// A chat that no channel talks to is answered once in this time, so the bot is
/// not a way to make it talk.
const STRANGER_WAIT: Duration = Duration::from_secs(3600);

struct Session {
    token: String,
    client: TelegramClient,
    name: String,
    offset: i64,
}

struct Listener {
    /// Where Telegram is; a stand-in in the tests.
    base: String,
    pool: SqlitePool,
    controller: Arc<RwLock<ChannelController>>,
    system: SystemStat,
    session: Option<Session>,
    strangers: HashMap<i64, Instant>,
    last_error: Option<String>,
}

impl Listener {
    /// Tell a problem once, not at every try.
    fn report(&mut self, text: String) {
        if self.last_error.as_deref() != Some(text.as_str()) {
            warn!(target: "telegram", "{text}");
            self.last_error = Some(text);
        }
    }

    /// Connect with the token and skip what was said while nobody listened.
    async fn connect(&mut self, token: &str) -> bool {
        let client = TelegramClient::with_base(token, &self.base);
        let name = match client.get_me().await {
            Ok(info) => info.username,
            Err(error) => {
                self.report(format!("Cannot start the Telegram bot: {error}"));

                return false;
            }
        };
        let offset = match client.get_updates(Some(-1), false).await {
            Ok(updates) => updates.last().map_or(0, |update| update.update_id + 1),
            Err(error) => {
                self.report(format!("Cannot start the Telegram bot: {error}"));

                return false;
            }
        };

        info!(target: "telegram", "Telegram bot @{name} is listening");

        self.last_error = None;
        self.session = Some(Session {
            token: token.to_string(),
            client,
            name,
            offset,
        });

        true
    }

    /// One round of listening. Returns how long to wait before the next one.
    async fn step(&mut self) -> Duration {
        let token = handles::select_telegram_token(&self.pool)
            .await
            .unwrap_or_default();

        if token.is_empty() {
            self.session = None;

            return IDLE_WAIT;
        }

        if self
            .session
            .as_ref()
            .is_none_or(|session| session.token != token)
            && !self.connect(&token).await
        {
            return REFUSED_WAIT;
        }

        let Some(session) = self.session.as_ref() else {
            return IDLE_WAIT;
        };
        let client = session.client.clone();
        let name = session.name.clone();
        let offset = session.offset;

        match client.get_updates(Some(offset), true).await {
            Ok(updates) => {
                let next = updates
                    .iter()
                    .map(|update| update.update_id + 1)
                    .max()
                    .unwrap_or(offset);

                if let Some(session) = self.session.as_mut() {
                    session.offset = next;
                }

                self.last_error = None;

                for update in updates {
                    self.handle(&client, &name, update).await;
                }

                Duration::ZERO
            }
            Err(error) if error.is_unauthorized() || error.is_conflict() => {
                self.report(format!("The Telegram bot cannot listen: {error}"));
                self.session = None;

                REFUSED_WAIT
            }
            Err(error) => {
                self.report(format!("The Telegram bot lost the connection: {error}"));

                NETWORK_WAIT
            }
        }
    }

    async fn reply(&self, client: &TelegramClient, chat: i64, text: &str) {
        if text.is_empty() {
            return;
        }

        if let Err(error) = client.send_message(chat, text).await {
            warn!(target: "telegram", "Cannot answer in the chat {chat}: {error}");
        }
    }

    async fn handle(&mut self, client: &TelegramClient, bot_name: &str, update: Update) {
        let Some(message) = update.message else {
            return;
        };
        let Some(command) = message
            .text
            .as_deref()
            .and_then(|text| commands::parse(text, bot_name))
        else {
            return;
        };
        let chat = message.chat.id;
        let settings = handles::select_enabled_telegram_settings(&self.pool)
            .await
            .unwrap_or_default()
            .into_iter()
            .find(|settings| settings.chat() == Some(chat));

        if command == Command::Id {
            self.reply(
                client,
                chat,
                &format!("Número deste chat: <code>{chat}</code>"),
            )
            .await;

            return;
        }

        let Some(settings) = settings else {
            self.answer_stranger(client, chat).await;

            return;
        };

        self.answer(client, chat, command, &settings).await;
    }

    async fn answer_stranger(&mut self, client: &TelegramClient, chat: i64) {
        let now = Instant::now();

        if self
            .strangers
            .get(&chat)
            .is_some_and(|last| now.duration_since(*last) < STRANGER_WAIT)
        {
            return;
        }

        self.strangers.insert(chat, now);
        self.strangers
            .retain(|_, last| now.duration_since(*last) < STRANGER_WAIT);
        self.reply(client, chat, &commands::unknown_chat_message(chat))
            .await;
    }

    async fn answer(
        &self,
        client: &TelegramClient,
        chat: i64,
        command: Command,
        settings: &TelegramSettings,
    ) {
        if command == Command::Help {
            self.reply(client, chat, &commands::help_message()).await;

            return;
        }

        if command.access() == Access::Chat && !settings.commands_enabled {
            self.reply(
                client,
                chat,
                "Os comandos estão desligados para este canal. Ligue em Ajustes → Telegram.",
            )
            .await;

            return;
        }

        let Some(manager) = self.controller.read().await.get(settings.channel_id) else {
            self.reply(client, chat, "O canal não está carregado no momento.")
                .await;

            return;
        };
        let text = commands::execute(command, &manager, &self.pool, &self.system).await;

        self.reply(client, chat, &text).await;
    }
}

/// Start the bot: the alerts and the commands. Both stop at `shutdown`.
pub async fn run(
    pool: SqlitePool,
    controller: Arc<RwLock<ChannelController>>,
    system: SystemStat,
    shutdown: CancellationToken,
) {
    tokio::spawn(run_alerts(
        pool.clone(),
        controller.clone(),
        system.clone(),
        shutdown.clone(),
    ));

    let mut listener = Listener {
        base: API_BASE.to_string(),
        pool,
        controller,
        system,
        session: None,
        strangers: HashMap::new(),
        last_error: None,
    };

    loop {
        let wait = select! {
            () = shutdown.cancelled() => break,
            wait = listener.step() => wait,
        };

        if wait.is_zero() {
            continue;
        }

        select! {
            () = shutdown.cancelled() => break,
            () = sleep(wait) => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use serde_json::{Value, json};

    use super::*;
    use crate::utils::telegram::{
        client::{Chat, Message},
        fake::{Calls, GROUP, TOKEN, sent, world},
    };

    struct Fixture {
        listener: Listener,
        client: TelegramClient,
        calls: Calls,
    }

    async fn fixture(script: Vec<Value>) -> Fixture {
        let world = world(script).await;

        Fixture {
            client: TelegramClient::with_base(TOKEN, &world.base),
            listener: Listener {
                base: world.base,
                pool: world.pool,
                controller: world.controller,
                system: SystemStat::new(),
                session: None,
                strangers: HashMap::new(),
                last_error: None,
            },
            calls: world.calls,
        }
    }

    fn said(chat: i64, text: &str) -> Update {
        Update {
            update_id: 1,
            message: Some(Message {
                chat: Chat { id: chat },
                text: Some(text.to_string()),
            }),
        }
    }

    #[tokio::test]
    async fn the_group_of_the_channel_is_told_how_the_channel_is() {
        let mut fixture = fixture(Vec::new()).await;

        fixture
            .listener
            .handle(&fixture.client, "emissora_bot", said(GROUP, "/status"))
            .await;

        let sent = sent(&fixture.calls);

        assert_eq!(sent.len(), 1);
        assert_eq!(sent[0].0, GROUP);
        assert!(sent[0].1.contains("Parado"), "{}", sent[0].1);
        assert!(sent[0].1.contains("Arquivo de reserva"), "{}", sent[0].1);
    }

    #[tokio::test]
    async fn every_command_of_the_list_gets_an_answer() {
        let mut fixture = fixture(Vec::new()).await;

        for spec in commands::COMMANDS {
            fixture
                .listener
                .handle(
                    &fixture.client,
                    "emissora_bot",
                    said(GROUP, &format!("/{}", spec.name)),
                )
                .await;
        }

        let sent = sent(&fixture.calls);

        assert_eq!(sent.len(), commands::COMMANDS.len());
        assert!(
            sent.iter()
                .all(|(chat, text)| *chat == GROUP && !text.is_empty())
        );
    }

    #[tokio::test]
    async fn any_chat_can_ask_for_its_number_but_for_nothing_else() {
        let mut fixture = fixture(Vec::new()).await;

        fixture
            .listener
            .handle(&fixture.client, "emissora_bot", said(777, "/id"))
            .await;
        fixture
            .listener
            .handle(&fixture.client, "emissora_bot", said(777, "/status"))
            .await;
        fixture
            .listener
            .handle(&fixture.client, "emissora_bot", said(777, "/hoje"))
            .await;

        let sent = sent(&fixture.calls);

        assert_eq!(sent.len(), 2, "{sent:?}");
        assert_eq!(
            sent[0],
            (777, "Número deste chat: <code>777</code>".to_string())
        );
        assert!(
            sent[1].1.contains("ainda não está ligado a um canal"),
            "{}",
            sent[1].1
        );
        assert!(!sent[1].1.contains("Parado"));
    }

    #[tokio::test]
    async fn a_chat_that_is_off_is_not_served_even_if_it_was_set() {
        let mut fixture = fixture(Vec::new()).await;
        let off = TelegramSettings {
            channel_id: 1,
            enabled: false,
            chat_id: GROUP.to_string(),
            ..TelegramSettings::default()
        };

        handles::replace_telegram_settings(&fixture.listener.pool, 1, &off)
            .await
            .unwrap();
        fixture
            .listener
            .handle(&fixture.client, "emissora_bot", said(GROUP, "/status"))
            .await;

        let sent = sent(&fixture.calls);

        assert_eq!(sent.len(), 1);
        assert!(sent[0].1.contains("não está ligado"), "{}", sent[0].1);
    }

    #[tokio::test]
    async fn the_group_can_turn_the_commands_off() {
        let mut fixture = fixture(Vec::new()).await;
        let settings = TelegramSettings {
            channel_id: 1,
            enabled: true,
            chat_id: GROUP.to_string(),
            commands_enabled: false,
            ..TelegramSettings::default()
        };

        handles::replace_telegram_settings(&fixture.listener.pool, 1, &settings)
            .await
            .unwrap();
        fixture
            .listener
            .handle(&fixture.client, "emissora_bot", said(GROUP, "/status"))
            .await;
        fixture
            .listener
            .handle(&fixture.client, "emissora_bot", said(GROUP, "/ajuda"))
            .await;

        let sent = sent(&fixture.calls);

        assert!(sent[0].1.contains("desligados"), "{}", sent[0].1);
        assert!(sent[1].1.contains("/status"), "the help still works");
    }

    #[tokio::test]
    async fn a_stranger_is_answered_once_and_other_bots_commands_are_ignored() {
        let mut fixture = fixture(Vec::new()).await;

        for _ in 0..3 {
            fixture
                .listener
                .handle(&fixture.client, "emissora_bot", said(555, "/status"))
                .await;
        }

        fixture
            .listener
            .handle(
                &fixture.client,
                "emissora_bot",
                said(GROUP, "/status@outro_bot"),
            )
            .await;
        fixture
            .listener
            .handle(
                &fixture.client,
                "emissora_bot",
                said(GROUP, "bom dia, equipe"),
            )
            .await;

        assert_eq!(sent(&fixture.calls).len(), 1);
    }

    #[tokio::test]
    async fn the_bot_connects_skips_the_old_news_and_answers() {
        let mut fixture = fixture(vec![
            json!({ "ok": true, "result": { "username": "emissora_bot" } }),
            json!({ "ok": true, "result": [
                { "update_id": 40, "message": { "chat": { "id": GROUP }, "text": "/status" } }
            ] }),
            json!({ "ok": true, "result": [
                { "update_id": 41, "message": { "chat": { "id": GROUP }, "text": "/ajuda" } }
            ] }),
        ])
        .await;

        assert_eq!(fixture.listener.step().await, Duration::ZERO);

        let calls: Vec<(String, Value)> = fixture.calls.lock().unwrap().clone();
        let methods: Vec<&str> = calls
            .iter()
            .map(|(method, _)| method.rsplit('/').next().unwrap())
            .collect();

        assert_eq!(
            methods,
            ["getMe", "getUpdates", "getUpdates", "sendMessage"]
        );
        assert_eq!(calls[1].1["offset"], -1, "it asks for the last one only");
        assert_eq!(calls[2].1["offset"], 41, "and goes on after it");
        assert_eq!(
            sent(&fixture.calls).len(),
            1,
            "the old command is not answered"
        );
        assert!(sent(&fixture.calls)[0].1.contains("Comandos"));
        assert_eq!(fixture.listener.session.as_ref().unwrap().offset, 42);
    }

    #[tokio::test]
    async fn a_refused_token_waits_and_a_removed_token_stops_the_bot() {
        let mut fixture = fixture(vec![json!({
            "ok": false, "error_code": 401, "description": "Unauthorized"
        })])
        .await;

        assert_eq!(fixture.listener.step().await, REFUSED_WAIT);
        assert!(fixture.listener.session.is_none());

        handles::update_telegram_token(&fixture.listener.pool, "")
            .await
            .unwrap();

        assert_eq!(fixture.listener.step().await, IDLE_WAIT);
        assert_eq!(fixture.calls.lock().unwrap().len(), 1, "no token, no call");
    }
}
