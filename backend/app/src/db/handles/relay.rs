use sqlx::sqlite::SqlitePool;

use crate::{
    db::models::{MonitorDestination, RelayConfig},
    utils::errors::ProcessError,
};

pub async fn select_relay(pool: &SqlitePool, channel_id: i32) -> Result<RelayConfig, ProcessError> {
    const SETTINGS: &str = "SELECT api_url, path FROM relay_settings WHERE channel_id = $1";
    const DESTINATIONS: &str =
        "SELECT * FROM monitor_destination WHERE channel_id = $1 ORDER BY name COLLATE NOCASE, id";

    let settings: Option<(String, String)> = sqlx::query_as(SETTINGS)
        .bind(channel_id)
        .fetch_optional(pool)
        .await?;
    let destinations: Vec<MonitorDestination> = sqlx::query_as(DESTINATIONS)
        .bind(channel_id)
        .fetch_all(pool)
        .await?;
    let (api_url, path) = settings.unwrap_or_default();

    Ok(RelayConfig {
        api_url,
        path,
        destinations,
    })
}

/// Replaces the relay address and the destinations in one step.
pub async fn replace_relay(
    pool: &SqlitePool,
    channel_id: i32,
    config: &RelayConfig,
) -> Result<(), ProcessError> {
    let mut transaction = pool.begin().await?;

    sqlx::query(
        "INSERT INTO relay_settings (channel_id, api_url, path) VALUES ($1, $2, $3)
        ON CONFLICT (channel_id) DO UPDATE SET api_url = excluded.api_url, path = excluded.path",
    )
    .bind(channel_id)
    .bind(config.api_url.trim().trim_end_matches('/'))
    .bind(config.path.trim())
    .execute(&mut *transaction)
    .await?;
    sqlx::query("DELETE FROM monitor_destination WHERE channel_id = $1")
        .bind(channel_id)
        .execute(&mut *transaction)
        .await?;

    for destination in &config.destinations {
        sqlx::query(
            "INSERT INTO monitor_destination (
                channel_id, name, protocol, match_user, match_address, enabled
            )
            VALUES ($1, $2, $3, $4, $5, $6)",
        )
        .bind(channel_id)
        .bind(destination.name.trim())
        .bind(destination.protocol)
        .bind(destination.match_user.trim())
        .bind(destination.match_address.trim())
        .bind(destination.enabled)
        .execute(&mut *transaction)
        .await?;
    }

    transaction.commit().await?;

    Ok(())
}
