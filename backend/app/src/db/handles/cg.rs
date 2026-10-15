use std::collections::HashMap;

use sqlx::sqlite::SqlitePool;

use crate::{
    db::models::{Cg, CgSchedule, CgWithSchedules},
    utils::errors::ProcessError,
};

const INSERT_CG: &str = "INSERT INTO cg (
        channel_id, name, kind, text, image_path, x_pct, y_pct, size_pct, font_family,
        font_weight, text_color, box_enabled, box_color, box_opacity, opacity,
        fade_in_seconds, enabled
    )
    VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, $15, $16, $17)";

const UPDATE_CG: &str = "UPDATE cg SET
        name = $3, kind = $4, text = $5, image_path = $6, x_pct = $7, y_pct = $8,
        size_pct = $9, font_family = $10, font_weight = $11, text_color = $12,
        box_enabled = $13, box_color = $14, box_opacity = $15, opacity = $16,
        fade_in_seconds = $17, enabled = $18
    WHERE id = $1 AND channel_id = $2";

const INSERT_SCHEDULE: &str = "INSERT INTO cg_schedule (
        cg_id, mode, weekdays, date_from, date_to, start_time, program_match,
        offset_seconds, duration_seconds, repeat_every_seconds, until_time, enabled
    )
    VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12)";

/// All graphics of a channel with their schedules.
pub async fn select_cgs(
    pool: &SqlitePool,
    channel_id: i32,
) -> Result<Vec<CgWithSchedules>, ProcessError> {
    const GRAPHICS: &str =
        "SELECT * FROM cg WHERE channel_id = $1 ORDER BY name COLLATE NOCASE, id";
    const SCHEDULES: &str = "SELECT cg_schedule.* FROM cg_schedule
        JOIN cg ON cg.id = cg_schedule.cg_id
        WHERE cg.channel_id = $1
        ORDER BY cg_schedule.id";

    let graphics: Vec<Cg> = sqlx::query_as(GRAPHICS)
        .bind(channel_id)
        .fetch_all(pool)
        .await?;
    let schedules: Vec<CgSchedule> = sqlx::query_as(SCHEDULES)
        .bind(channel_id)
        .fetch_all(pool)
        .await?;
    let mut by_graphic: HashMap<i32, Vec<CgSchedule>> = HashMap::new();

    for schedule in schedules {
        by_graphic.entry(schedule.cg_id).or_default().push(schedule);
    }

    let result = graphics
        .into_iter()
        .map(|cg| CgWithSchedules {
            schedules: by_graphic.remove(&cg.id).unwrap_or_default(),
            cg,
        })
        .collect();

    Ok(result)
}

pub async fn select_cg(
    pool: &SqlitePool,
    channel_id: i32,
    id: i32,
) -> Result<Option<CgWithSchedules>, ProcessError> {
    let found = select_cgs(pool, channel_id)
        .await?
        .into_iter()
        .find(|item| item.cg.id == id);

    Ok(found)
}

/// Saves a new graphic with its schedule in one step and returns its id.
pub async fn insert_cg(
    pool: &SqlitePool,
    channel_id: i32,
    item: &CgWithSchedules,
) -> Result<i32, ProcessError> {
    let mut transaction = pool.begin().await?;
    let cg = &item.cg;
    let id: i32 = sqlx::query(INSERT_CG)
        .bind(channel_id)
        .bind(cg.name.trim())
        .bind(cg.kind)
        .bind(&cg.text)
        .bind(&cg.image_path)
        .bind(cg.x_pct)
        .bind(cg.y_pct)
        .bind(cg.size_pct)
        .bind(&cg.font_family)
        .bind(&cg.font_weight)
        .bind(&cg.text_color)
        .bind(cg.box_enabled)
        .bind(&cg.box_color)
        .bind(cg.box_opacity)
        .bind(cg.opacity)
        .bind(cg.fade_in_seconds)
        .bind(cg.enabled)
        .execute(&mut *transaction)
        .await
        .map(|done| done.last_insert_rowid())
        .and_then(|rowid| i32::try_from(rowid).map_err(|e| sqlx::Error::Decode(Box::new(e))))?;

    insert_schedules(&mut transaction, id, &item.schedules).await?;
    transaction.commit().await?;

    Ok(id)
}

/// Replaces a graphic and its schedule; returns `false` when the graphic does
/// not exist on this channel.
pub async fn update_cg(
    pool: &SqlitePool,
    channel_id: i32,
    id: i32,
    item: &CgWithSchedules,
) -> Result<bool, ProcessError> {
    let mut transaction = pool.begin().await?;
    let cg = &item.cg;
    let updated = sqlx::query(UPDATE_CG)
        .bind(id)
        .bind(channel_id)
        .bind(cg.name.trim())
        .bind(cg.kind)
        .bind(&cg.text)
        .bind(&cg.image_path)
        .bind(cg.x_pct)
        .bind(cg.y_pct)
        .bind(cg.size_pct)
        .bind(&cg.font_family)
        .bind(&cg.font_weight)
        .bind(&cg.text_color)
        .bind(cg.box_enabled)
        .bind(&cg.box_color)
        .bind(cg.box_opacity)
        .bind(cg.opacity)
        .bind(cg.fade_in_seconds)
        .bind(cg.enabled)
        .execute(&mut *transaction)
        .await?
        .rows_affected();

    if updated == 0 {
        return Ok(false);
    }

    sqlx::query("DELETE FROM cg_schedule WHERE cg_id = $1")
        .bind(id)
        .execute(&mut *transaction)
        .await?;
    insert_schedules(&mut transaction, id, &item.schedules).await?;
    transaction.commit().await?;

    Ok(true)
}

pub async fn delete_cg(pool: &SqlitePool, channel_id: i32, id: i32) -> Result<bool, ProcessError> {
    let deleted = sqlx::query("DELETE FROM cg WHERE id = $1 AND channel_id = $2")
        .bind(id)
        .bind(channel_id)
        .execute(pool)
        .await?
        .rows_affected();

    Ok(deleted > 0)
}

async fn insert_schedules(
    transaction: &mut sqlx::SqliteConnection,
    cg_id: i32,
    schedules: &[CgSchedule],
) -> Result<(), ProcessError> {
    for schedule in schedules {
        sqlx::query(INSERT_SCHEDULE)
            .bind(cg_id)
            .bind(schedule.mode)
            .bind(schedule.weekdays)
            .bind(&schedule.date_from)
            .bind(&schedule.date_to)
            .bind(&schedule.start_time)
            .bind(schedule.program_match.trim())
            .bind(schedule.offset_seconds)
            .bind(schedule.duration_seconds)
            .bind(schedule.repeat_every_seconds)
            .bind(&schedule.until_time)
            .bind(schedule.enabled)
            .execute(&mut *transaction)
            .await?;
    }

    Ok(())
}
