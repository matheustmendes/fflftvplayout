use axum::{
    Json,
    extract::{Path, State},
};
use protect_axum::authorities::AuthDetails;
use serde::{Deserialize, Serialize};

use super::commercials::ensure_manager;
use crate::{
    api::{
        routes::{AuthUser, ensure_any_authority},
        state::AppState,
    },
    db::{
        handles,
        models::{Role, TelegramSettings},
    },
    utils::{
        errors::ServiceError,
        telegram::{
            client::{TelegramClient, TelegramError},
            messages::esc,
        },
    },
};

/// What the settings screen shows. The token is never sent back.
#[derive(Debug, Serialize)]
pub struct TelegramView {
    /// A token of the bot was saved.
    pub token_set: bool,
    pub settings: TelegramSettings,
}

#[derive(Debug, Deserialize)]
pub struct TokenRequest {
    /// The token the BotFather gave; empty removes it.
    #[serde(default)]
    pub token: String,
}

#[derive(Debug, Serialize)]
pub struct TelegramTest {
    pub bot: String,
}

async fn view(state: &AppState, channel: i32) -> Result<TelegramView, ServiceError> {
    Ok(TelegramView {
        token_set: !handles::select_telegram_token(&state.pool)
            .await?
            .is_empty(),
        settings: handles::select_telegram_settings(&state.pool, channel).await?,
    })
}

/// The token of a bot is `<number>:<letters, digits, - and _>`.
fn valid_token(token: &str) -> bool {
    let Some((id, secret)) = token.split_once(':') else {
        return false;
    };

    !id.is_empty()
        && id.chars().all(|c| c.is_ascii_digit())
        && secret.len() >= 20
        && token.len() <= 120
        && secret
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
}

/// What went wrong with Telegram, in words for the person at the screen.
fn telegram_message(error: &TelegramError) -> String {
    match error {
        TelegramError::Network(_) => {
            "Não consegui falar com o Telegram. Confira a internet do servidor.".to_string()
        }
        TelegramError::Api {
            code: 401 | 404, ..
        } => "O Telegram não aceitou o token. Copie de novo a mensagem do BotFather.".to_string(),
        TelegramError::Api { code: 403, .. } => {
            "O bot não pode escrever nesse chat. Coloque o bot no grupo e mande /start nele."
                .to_string()
        }
        TelegramError::Api {
            code: 400,
            description,
        } if description.contains("chat not found") => {
            "O Telegram não achou esse chat. Confira o número, que no grupo começa com -."
                .to_string()
        }
        TelegramError::Api { description, .. } => format!("O Telegram recusou: {description}"),
    }
}

/// #### Telegram bot
///
/// **What the screen shows: the settings of the channel and whether the bot has a token**
///
/// ```BASH
/// curl -X GET http://127.0.0.1:8787/api/telegram/1 -H 'Authorization: Bearer <TOKEN>'
/// ```
pub async fn get_telegram(
    State(state): State<AppState>,
    Path(channel): Path<i32>,
    user: AuthUser,
    details: AuthDetails<Role>,
) -> Result<Json<TelegramView>, ServiceError> {
    ensure_manager(&user, &details, channel)?;

    Ok(Json(view(&state, channel).await?))
}

/// **Save what the channel wants from the bot**
///
/// ```BASH
/// curl -X PUT http://127.0.0.1:8787/api/telegram/1 -H 'Content-Type: application/json' \
/// -d '{ "enabled": true, "chat_id": "-1001234567890", "summary_time": "08:00" }' \
/// -H 'Authorization: Bearer <TOKEN>'
/// ```
pub async fn update_telegram(
    State(state): State<AppState>,
    Path(channel): Path<i32>,
    user: AuthUser,
    details: AuthDetails<Role>,
    Json(settings): Json<TelegramSettings>,
) -> Result<Json<TelegramView>, ServiceError> {
    ensure_manager(&user, &details, channel)?;
    settings.validate().map_err(ServiceError::BadRequest)?;
    handles::replace_telegram_settings(&state.pool, channel, &settings).await?;

    Ok(Json(view(&state, channel).await?))
}

/// **Save the token of the bot** (only the global administrator)
///
/// ```BASH
/// curl -X PUT http://127.0.0.1:8787/api/telegram/1/token -H 'Content-Type: application/json' \
/// -d '{ "token": "123456:ABC..." }' -H 'Authorization: Bearer <TOKEN>'
/// ```
pub async fn update_telegram_token(
    State(state): State<AppState>,
    Path(channel): Path<i32>,
    details: AuthDetails<Role>,
    Json(request): Json<TokenRequest>,
) -> Result<Json<TelegramView>, ServiceError> {
    ensure_any_authority(&details, &[&Role::GlobalAdmin])?;

    let token = request.token.trim();

    if !token.is_empty() && !valid_token(token) {
        return Err(ServiceError::BadRequest(
            "Esse não parece um token de bot. Ele é parecido com 123456789:AAH-xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx.".to_string(),
        ));
    }

    handles::update_telegram_token(&state.pool, token).await?;

    Ok(Json(view(&state, channel).await?))
}

/// **Send a test message to the chat of the channel**
///
/// ```BASH
/// curl -X POST http://127.0.0.1:8787/api/telegram/1/test -H 'Authorization: Bearer <TOKEN>'
/// ```
pub async fn test_telegram(
    State(state): State<AppState>,
    Path(channel): Path<i32>,
    user: AuthUser,
    details: AuthDetails<Role>,
) -> Result<Json<TelegramTest>, ServiceError> {
    ensure_manager(&user, &details, channel)?;

    let token = handles::select_telegram_token(&state.pool).await?;
    let settings = handles::select_telegram_settings(&state.pool, channel).await?;

    if token.is_empty() {
        return Err(ServiceError::BadRequest(
            "Falta o token do bot. Peça ao administrador geral para salvá-lo.".to_string(),
        ));
    }

    let Some(chat) = settings.chat() else {
        return Err(ServiceError::BadRequest(
            "Falta o número do chat. Mande /id no grupo para o bot dizer qual é.".to_string(),
        ));
    };
    let name = state
        .controller
        .read()
        .await
        .get(channel)
        .ok_or_else(|| ServiceError::BadRequest(format!("Channel {channel} not found!")))?
        .channel
        .lock()
        .await
        .name
        .clone();
    let client = TelegramClient::new(&token);
    let bot = client
        .get_me()
        .await
        .map_err(|error| ServiceError::BadRequest(telegram_message(&error)))?;

    client
        .send_message(
            chat,
            &format!(
                "✅ <b>Teste do bot</b>\nO bot do canal <b>{}</b> está funcionando neste chat.\nDigite /ajuda para ver os comandos.",
                esc(&name)
            ),
        )
        .await
        .map_err(|error| ServiceError::BadRequest(telegram_message(&error)))?;

    Ok(Json(TelegramTest { bot: bot.username }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_token_looks_like_the_botfather_gives() {
        assert!(valid_token(
            "123456789:AAH-abcdefghijklmnopqrstuvwxyz_01234"
        ));
        assert!(!valid_token(""));
        assert!(!valid_token("123456789"));
        assert!(!valid_token("abc:AAH-abcdefghijklmnopqrstuvwxyz_01234"));
        assert!(!valid_token("123456789:short"));
        assert!(!valid_token(
            "123456789:AAH abcdefghijklmnopqrstuvwxyz_01234"
        ));
        assert!(!valid_token(&format!("1:{}", "a".repeat(200))));
    }

    #[test]
    fn the_person_is_told_what_to_do() {
        let refused = TelegramError::Api {
            code: 403,
            description: "Forbidden".to_string(),
        };

        assert!(telegram_message(&refused).contains("/start"));
        assert!(telegram_message(&TelegramError::Network("x".to_string())).contains("internet"));
    }
}
