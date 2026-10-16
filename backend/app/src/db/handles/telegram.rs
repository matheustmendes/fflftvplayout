use sqlx::sqlite::SqlitePool;

use crate::{db::models::TelegramSettings, utils::errors::ProcessError};

/// The token of the bot of the installation; empty when there is none.
pub async fn select_telegram_token(pool: &SqlitePool) -> Result<String, ProcessError> {
    let token: Option<String> = sqlx::query_scalar("SELECT token FROM telegram_bot WHERE id = 1")
        .fetch_optional(pool)
        .await?;

    Ok(token.unwrap_or_default())
}

pub async fn update_telegram_token(pool: &SqlitePool, token: &str) -> Result<(), ProcessError> {
    sqlx::query(
        "INSERT INTO telegram_bot (id, token) VALUES (1, $1)
        ON CONFLICT (id) DO UPDATE SET token = excluded.token",
    )
    .bind(token)
    .execute(pool)
    .await?;

    Ok(())
}

/// The settings of a channel; a channel that never saved any has the defaults.
pub async fn select_telegram_settings(
    pool: &SqlitePool,
    channel_id: i32,
) -> Result<TelegramSettings, ProcessError> {
    let settings: Option<TelegramSettings> =
        sqlx::query_as("SELECT * FROM telegram_settings WHERE channel_id = $1")
            .bind(channel_id)
            .fetch_optional(pool)
            .await?;

    Ok(settings.unwrap_or(TelegramSettings {
        channel_id,
        ..TelegramSettings::default()
    }))
}

/// The settings of every channel that has the bot turned on.
pub async fn select_enabled_telegram_settings(
    pool: &SqlitePool,
) -> Result<Vec<TelegramSettings>, ProcessError> {
    let settings = sqlx::query_as(
        "SELECT * FROM telegram_settings WHERE enabled = 1 AND chat_id <> '' ORDER BY channel_id",
    )
    .fetch_all(pool)
    .await?;

    Ok(settings)
}

pub async fn replace_telegram_settings(
    pool: &SqlitePool,
    channel_id: i32,
    settings: &TelegramSettings,
) -> Result<(), ProcessError> {
    const QUERY: &str = "INSERT INTO telegram_settings (
            channel_id, enabled, chat_id, alert_output, alert_destinations, alert_filler,
            alert_playlist, alert_commercials, alert_disk, daily_summary, summary_time,
            commands_enabled
        )
        VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12)
        ON CONFLICT (channel_id) DO UPDATE SET
            enabled = excluded.enabled, chat_id = excluded.chat_id,
            alert_output = excluded.alert_output,
            alert_destinations = excluded.alert_destinations,
            alert_filler = excluded.alert_filler, alert_playlist = excluded.alert_playlist,
            alert_commercials = excluded.alert_commercials, alert_disk = excluded.alert_disk,
            daily_summary = excluded.daily_summary, summary_time = excluded.summary_time,
            commands_enabled = excluded.commands_enabled";

    sqlx::query(QUERY)
        .bind(channel_id)
        .bind(settings.enabled)
        .bind(settings.chat_id.trim())
        .bind(settings.alert_output)
        .bind(settings.alert_destinations)
        .bind(settings.alert_filler)
        .bind(settings.alert_playlist)
        .bind(settings.alert_commercials)
        .bind(settings.alert_disk)
        .bind(settings.daily_summary)
        .bind(&settings.summary_time)
        .bind(settings.commands_enabled)
        .execute(pool)
        .await?;

    Ok(())
}
