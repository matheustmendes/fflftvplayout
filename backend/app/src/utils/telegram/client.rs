//! The part of the Telegram Bot API the bot needs: who am I, send a message and
//! wait for news.

use std::{fmt, sync::LazyLock, time::Duration};

use reqwest::Client;
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use serde_json::json;
use tokio::time::sleep;

pub const API_BASE: &str = "https://api.telegram.org";
const REQUEST_TIMEOUT: Duration = Duration::from_secs(15);
/// How long Telegram holds a request for news open before it answers empty.
const POLL_SECONDS: u64 = 25;
const POLL_TIMEOUT: Duration = Duration::from_secs(POLL_SECONDS + 10);
/// Telegram refuses more than 4096 characters in a message.
const MAX_MESSAGE_CHARS: usize = 4000;
const MAX_RETRY_WAIT: Duration = Duration::from_secs(30);

static HTTP: LazyLock<Client> = LazyLock::new(|| {
    let _ = rustls::crypto::ring::default_provider().install_default();

    Client::builder()
        .build()
        .expect("telegram HTTP client configuration must be valid")
});

#[derive(Debug, Clone)]
pub enum TelegramError {
    /// The request did not get to Telegram, or its answer could not be read.
    Network(String),
    /// Telegram answered that it did not do it.
    Api { code: i64, description: String },
}

impl fmt::Display for TelegramError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Network(error) => write!(f, "cannot reach Telegram: {error}"),
            Self::Api { code, description } => {
                write!(f, "Telegram refused ({code}): {description}")
            }
        }
    }
}

impl std::error::Error for TelegramError {}

impl TelegramError {
    /// The token is wrong or was revoked.
    pub fn is_unauthorized(&self) -> bool {
        matches!(
            self,
            Self::Api {
                code: 401 | 404,
                ..
            }
        )
    }

    /// Another program is already reading the news of this bot.
    pub fn is_conflict(&self) -> bool {
        matches!(self, Self::Api { code: 409, .. })
    }
}

#[derive(Debug, Deserialize)]
struct Envelope<T> {
    ok: bool,
    result: Option<T>,
    description: Option<String>,
    error_code: Option<i64>,
    parameters: Option<ResponseParameters>,
}

#[derive(Debug, Deserialize)]
struct ResponseParameters {
    retry_after: Option<u64>,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
pub struct BotInfo {
    pub username: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Update {
    pub update_id: i64,
    #[serde(default)]
    pub message: Option<Message>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Message {
    pub chat: Chat,
    #[serde(default)]
    pub text: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Chat {
    pub id: i64,
}

/// A connection to the bot of one token. The token never appears in a message
/// or in the log.
#[derive(Clone)]
pub struct TelegramClient {
    token: String,
    base: String,
}

impl fmt::Debug for TelegramClient {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("TelegramClient").finish_non_exhaustive()
    }
}

impl TelegramClient {
    pub fn new(token: &str) -> Self {
        Self::with_base(token, API_BASE)
    }

    pub fn with_base(token: &str, base: &str) -> Self {
        Self {
            token: token.trim().to_string(),
            base: base.trim_end_matches('/').to_string(),
        }
    }

    async fn call<T: DeserializeOwned>(
        &self,
        method: &str,
        body: &impl Serialize,
        timeout: Duration,
    ) -> Result<T, TelegramError> {
        let url = format!("{}/bot{}/{method}", self.base, self.token);
        let response = HTTP
            .post(url)
            .timeout(timeout)
            .json(body)
            .send()
            .await
            .map_err(|error| TelegramError::Network(error.without_url().to_string()))?;
        let envelope: Envelope<T> = response
            .json()
            .await
            .map_err(|error| TelegramError::Network(error.without_url().to_string()))?;

        match envelope {
            Envelope {
                ok: true,
                result: Some(result),
                ..
            } => Ok(result),
            Envelope {
                description,
                error_code,
                parameters,
                ..
            } => {
                let description = description.unwrap_or_default();

                // Telegram says how long to wait when it is asked too much.
                if error_code == Some(429)
                    && let Some(seconds) = parameters.and_then(|value| value.retry_after)
                {
                    return Err(TelegramError::Api {
                        code: 429,
                        description: format!("{description} (retry after {seconds} s)"),
                    });
                }

                Err(TelegramError::Api {
                    code: error_code.unwrap_or_default(),
                    description,
                })
            }
        }
    }

    pub async fn get_me(&self) -> Result<BotInfo, TelegramError> {
        self.call("getMe", &json!({}), REQUEST_TIMEOUT).await
    }

    /// Send a text in HTML. A long text goes in several messages.
    pub async fn send_message(&self, chat_id: i64, text: &str) -> Result<(), TelegramError> {
        for part in split_message(text) {
            let body = json!({
                "chat_id": chat_id,
                "text": part,
                "parse_mode": "HTML",
                "disable_web_page_preview": true,
            });
            let sent: Result<serde_json::Value, _> =
                self.call("sendMessage", &body, REQUEST_TIMEOUT).await;

            match sent {
                Ok(_) => {}
                Err(TelegramError::Api {
                    code: 429,
                    description,
                }) => {
                    // Once more after the wait that Telegram asked for.
                    sleep(retry_wait(&description)).await;
                    self.call::<serde_json::Value>("sendMessage", &body, REQUEST_TIMEOUT)
                        .await?;
                }
                Err(error) => return Err(error),
            }
        }

        Ok(())
    }

    /// The news since `offset`, waiting up to [`POLL_SECONDS`] for them. With
    /// `wait` false it answers at once.
    pub async fn get_updates(
        &self,
        offset: Option<i64>,
        wait: bool,
    ) -> Result<Vec<Update>, TelegramError> {
        let body = json!({
            "offset": offset,
            "timeout": if wait { POLL_SECONDS } else { 0 },
            "allowed_updates": ["message"],
        });

        self.call("getUpdates", &body, POLL_TIMEOUT).await
    }
}

fn retry_wait(description: &str) -> Duration {
    let seconds = description
        .rsplit("retry after ")
        .next()
        .and_then(|rest| rest.split(' ').next())
        .and_then(|value| value.parse::<u64>().ok())
        .unwrap_or(1);

    Duration::from_secs(seconds).min(MAX_RETRY_WAIT)
}

/// Cut a text into messages Telegram accepts, at the end of a line when it can.
pub fn split_message(text: &str) -> Vec<String> {
    let mut parts = Vec::new();
    let mut current = String::new();

    for line in text.split_inclusive('\n') {
        if !current.is_empty() && current.chars().count() + line.chars().count() > MAX_MESSAGE_CHARS
        {
            parts.push(std::mem::take(&mut current));
        }

        // A single line longer than the limit is cut where it must.
        if line.chars().count() > MAX_MESSAGE_CHARS {
            let chars: Vec<char> = line.chars().collect();

            for piece in chars.chunks(MAX_MESSAGE_CHARS) {
                parts.push(piece.iter().collect());
            }

            continue;
        }

        current.push_str(line);
    }

    if !current.trim().is_empty() {
        parts.push(current);
    }

    if parts.is_empty() {
        parts.push(text.to_string());
    }

    parts
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;
    use crate::utils::telegram::fake::fake_telegram;

    #[tokio::test]
    async fn a_message_goes_to_the_chat_as_html() {
        let (base, calls) = fake_telegram(Vec::new()).await;
        let client = TelegramClient::with_base("123:abc", &base);

        client.send_message(-1001, "<b>oi</b>").await.unwrap();

        let calls = calls.lock().unwrap();

        assert_eq!(calls[0].0, "bot123:abc/sendMessage");
        assert_eq!(calls[0].1["chat_id"], -1001);
        assert_eq!(calls[0].1["text"], "<b>oi</b>");
        assert_eq!(calls[0].1["parse_mode"], "HTML");
    }

    #[tokio::test]
    async fn the_bot_says_its_name() {
        let (base, _) = fake_telegram(Vec::new()).await;
        let info = TelegramClient::with_base("1:a", &base)
            .get_me()
            .await
            .unwrap();

        assert_eq!(info.username, "emissora_bot");
    }

    #[tokio::test]
    async fn a_refusal_carries_the_reason_and_never_the_token() {
        let (base, _) = fake_telegram(vec![json!({
            "ok": false, "error_code": 401, "description": "Unauthorized"
        })])
        .await;
        let error = TelegramClient::with_base("999:secret", &base)
            .get_me()
            .await
            .unwrap_err();

        assert!(error.is_unauthorized());
        assert!(!error.to_string().contains("secret"));
        assert!(!format!("{:?}", TelegramClient::new("999:secret")).contains("secret"));
    }

    #[tokio::test]
    async fn a_network_failure_does_not_show_the_token() {
        let error = TelegramClient::with_base("999:secret", "http://127.0.0.1:1")
            .get_me()
            .await
            .unwrap_err();

        assert!(matches!(error, TelegramError::Network(_)));
        assert!(!error.to_string().contains("secret"), "{error}");
    }

    #[tokio::test]
    async fn too_many_requests_wait_and_try_once_more() {
        let (base, calls) = fake_telegram(vec![
            json!({ "ok": false, "error_code": 429, "description": "Too Many Requests",
                    "parameters": { "retry_after": 0 } }),
            json!({ "ok": true, "result": { "message_id": 1 } }),
        ])
        .await;

        TelegramClient::with_base("1:a", &base)
            .send_message(5, "oi")
            .await
            .unwrap();

        assert_eq!(calls.lock().unwrap().len(), 2);
    }

    #[tokio::test]
    async fn updates_carry_the_chat_and_the_text() {
        let (base, calls) = fake_telegram(vec![json!({ "ok": true, "result": [
            { "update_id": 7, "message": { "chat": { "id": -42 }, "text": "/status" } },
            { "update_id": 8 }
        ] })])
        .await;
        let updates = TelegramClient::with_base("1:a", &base)
            .get_updates(Some(7), true)
            .await
            .unwrap();

        assert_eq!(updates.len(), 2);
        assert_eq!(updates[0].message.as_ref().unwrap().chat.id, -42);
        assert_eq!(
            updates[0].message.as_ref().unwrap().text.as_deref(),
            Some("/status")
        );
        assert!(updates[1].message.is_none());

        let calls = calls.lock().unwrap();

        assert_eq!(calls[0].1["offset"], 7);
        assert_eq!(calls[0].1["timeout"], POLL_SECONDS);
    }

    #[test]
    fn long_texts_are_cut_at_the_end_of_a_line() {
        let line = format!("{}\n", "a".repeat(999));
        let text = line.repeat(10);
        let parts = split_message(&text);

        assert!(parts.len() > 1);
        assert!(
            parts
                .iter()
                .all(|part| part.chars().count() <= MAX_MESSAGE_CHARS)
        );
        assert_eq!(parts.concat(), text);
    }

    #[test]
    fn a_line_longer_than_the_limit_is_cut_anyway() {
        let parts = split_message(&"b".repeat(MAX_MESSAGE_CHARS * 2 + 5));

        assert_eq!(parts.len(), 3);
        assert!(
            parts
                .iter()
                .all(|part| part.chars().count() <= MAX_MESSAGE_CHARS)
        );
    }

    #[test]
    fn the_wait_is_read_from_the_description_and_capped() {
        assert_eq!(retry_wait("x (retry after 3 s)"), Duration::from_secs(3));
        assert_eq!(retry_wait("x (retry after 900 s)"), MAX_RETRY_WAIT);
        assert_eq!(retry_wait("nothing"), Duration::from_secs(1));
    }
}
