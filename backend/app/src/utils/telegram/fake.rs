//! A stand-in for Telegram for the tests: it records what it is asked and
//! answers from a script.

use std::sync::{Arc, Mutex};

use axum::{
    Json, Router,
    extract::{Path, State},
    routing::post,
};
use serde_json::{Value, json};
use sqlx::SqlitePool;
use tokio::{net::TcpListener, sync::RwLock};

use crate::{
    db::{handles, models::TelegramSettings},
    player::controller::ChannelController,
    test_support::channel_manager,
};

/// What the fake was asked: `bot<token>/<method>` and the body.
pub(crate) type Calls = Arc<Mutex<Vec<(String, Value)>>>;

/// Answers one call from `answers`, in order, then `ok` for the rest.
pub(crate) async fn fake_telegram(answers: Vec<Value>) -> (String, Calls) {
    let calls: Calls = Arc::default();
    let answers = Arc::new(Mutex::new(answers));
    let app = Router::new()
        .route(
            "/{bot}/{method}",
            post(
                |State((calls, answers)): State<(Calls, Arc<Mutex<Vec<Value>>>)>,
                 Path((bot, method)): Path<(String, String)>,
                 Json(body): Json<Value>| async move {
                    calls
                        .lock()
                        .unwrap()
                        .push((format!("{bot}/{method}"), body));

                    let mut answers = answers.lock().unwrap();
                    let answer = if answers.is_empty() {
                        json!({ "ok": true, "result": { "username": "emissora_bot" } })
                    } else {
                        answers.remove(0)
                    };

                    Json(answer)
                },
            ),
        )
        .with_state((calls.clone(), answers));
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();

    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });

    (format!("http://{address}"), calls)
}

pub(crate) const TOKEN: &str = "123456789:AAH-abcdefghijklmnopqrstuvwxyz_0123";
pub(crate) const GROUP: i64 = -4242;

/// A channel whose bot is on for [`GROUP`], and a Telegram that answers from a
/// script.
pub(crate) struct World {
    pub pool: SqlitePool,
    pub controller: Arc<RwLock<ChannelController>>,
    pub base: String,
    pub calls: Calls,
}

pub(crate) async fn world(script: Vec<Value>) -> World {
    let (manager, pool) = channel_manager().await;
    let controller = Arc::new(RwLock::new(ChannelController::new()));
    let settings = TelegramSettings {
        channel_id: 1,
        enabled: true,
        chat_id: GROUP.to_string(),
        ..TelegramSettings::default()
    };

    controller.write().await.add(manager);
    handles::update_telegram_token(&pool, TOKEN).await.unwrap();
    handles::replace_telegram_settings(&pool, 1, &settings)
        .await
        .unwrap();

    let (base, calls) = fake_telegram(script).await;

    World {
        pool,
        controller,
        base,
        calls,
    }
}

/// The texts the bot sent, with the chat.
pub(crate) fn sent(calls: &Calls) -> Vec<(i64, String)> {
    calls
        .lock()
        .unwrap()
        .iter()
        .filter(|(method, _)| method.ends_with("/sendMessage"))
        .map(|(_, body)| {
            (
                body["chat_id"].as_i64().unwrap(),
                body["text"].as_str().unwrap().to_string(),
            )
        })
        .collect()
}
