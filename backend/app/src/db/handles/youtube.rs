use sqlx::sqlite::SqlitePool;

use crate::{db::models::YoutubeSource, utils::errors::ProcessError};

pub async fn select_youtube_sources(
    pool: &SqlitePool,
    channel_id: i32,
) -> Result<Vec<YoutubeSource>, ProcessError> {
    let sources = sqlx::query_as(
        "SELECT id, channel_id, youtube_channel_id, name FROM youtube_source
        WHERE channel_id = $1 ORDER BY name COLLATE NOCASE, id",
    )
    .bind(channel_id)
    .fetch_all(pool)
    .await?;

    Ok(sources)
}

/// Adds the channel, or renames it when it is already on the list.
pub async fn upsert_youtube_source(
    pool: &SqlitePool,
    channel_id: i32,
    youtube_channel_id: &str,
    name: &str,
) -> Result<(), ProcessError> {
    sqlx::query(
        "INSERT INTO youtube_source (channel_id, youtube_channel_id, name) VALUES ($1, $2, $3)
        ON CONFLICT (channel_id, youtube_channel_id) DO UPDATE SET name = excluded.name",
    )
    .bind(channel_id)
    .bind(youtube_channel_id)
    .bind(name)
    .execute(pool)
    .await?;

    Ok(())
}

/// Returns whether a row was removed.
pub async fn delete_youtube_source(
    pool: &SqlitePool,
    channel_id: i32,
    id: i32,
) -> Result<bool, ProcessError> {
    let result = sqlx::query("DELETE FROM youtube_source WHERE channel_id = $1 AND id = $2")
        .bind(channel_id)
        .bind(id)
        .execute(pool)
        .await?;

    Ok(result.rows_affected() > 0)
}
