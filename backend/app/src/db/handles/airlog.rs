use sqlx::sqlite::SqlitePool;

use crate::{
    db::models::{AirKind, AirResult, Airing, PlannedAiring, SpotIdentity},
    utils::errors::ProcessError,
};

// Timestamps are UTC text (`YYYY-MM-DDTHH:MM:SS.mmmZ`), so comparing them as
// text compares them as time. The caller formats them, which keeps this layer
// free of any clock.

/// Filters of the airing log; `None` leaves a field unrestricted.
#[derive(Debug, Clone, Default)]
pub struct AiringFilter {
    pub date_from: Option<String>,
    pub date_to: Option<String>,
    pub campaign_id: Option<i32>,
    pub advertiser_id: Option<i32>,
    pub result: Option<AirResult>,
    pub kind: Option<AirKind>,
    pub limit: i64,
}

/// Replace what the plan of a date promises. Rows that are still pending and
/// start after `now` are dropped and the new ones are inserted. What already
/// aired, and what was due before `now`, is history and stays untouched.
pub async fn replace_planned_airings(
    pool: &SqlitePool,
    channel_id: i32,
    air_date: &str,
    now: &str,
    rows: &[PlannedAiring],
) -> Result<(), ProcessError> {
    const INSERT: &str = "INSERT INTO airlog (
            channel_id, air_date, kind, spot_id, campaign_id, advertiser_id,
            advertiser_name, campaign_name, spot_title, file_path, break_window_id,
            position_in_break, planned_begin_sec, planned_start, planned_duration_sec, result
        )
        VALUES ($1, $2, 'spot', $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, 'pending')";

    let mut transaction = pool.begin().await?;

    sqlx::query(
        "DELETE FROM airlog
        WHERE channel_id = $1 AND air_date = $2 AND kind = 'spot' AND result = 'pending'
            AND (planned_start IS NULL OR planned_start > $3)",
    )
    .bind(channel_id)
    .bind(air_date)
    .bind(now)
    .execute(&mut *transaction)
    .await?;

    for row in rows {
        sqlx::query(INSERT)
            .bind(channel_id)
            .bind(air_date)
            .bind(row.spot_id)
            .bind(row.campaign_id)
            .bind(row.advertiser_id)
            .bind(&row.advertiser_name)
            .bind(&row.campaign_name)
            .bind(&row.spot_title)
            .bind(&row.file_path)
            .bind(row.break_window_id)
            .bind(row.position_in_break)
            .bind(row.planned_begin_sec)
            .bind(&row.planned_start)
            .bind(row.planned_duration_sec)
            .execute(&mut *transaction)
            .await?;
    }

    transaction.commit().await?;

    Ok(())
}

/// Who owns a spot file, preferring an active spot over an archived one.
pub async fn select_spot_identity(
    pool: &SqlitePool,
    channel_id: i32,
    file_path: &str,
) -> Result<Option<SpotIdentity>, ProcessError> {
    const QUERY: &str = "SELECT
            spot.id AS spot_id, campaign.id AS campaign_id, advertiser.id AS advertiser_id,
            advertiser.name AS advertiser_name, campaign.name AS campaign_name,
            spot.title AS spot_title
        FROM spot
        JOIN campaign ON campaign.id = spot.campaign_id
        JOIN advertiser ON advertiser.id = campaign.advertiser_id
        WHERE spot.channel_id = $1 AND spot.file_path = $2
        ORDER BY spot.active DESC, spot.id DESC
        LIMIT 1";

    let result = sqlx::query_as(QUERY)
        .bind(channel_id)
        .bind(file_path)
        .fetch_optional(pool)
        .await?;

    Ok(result)
}

/// A spot is starting on the air. Claim the pending row it was planned in (the
/// one of the same file closest to `begin_sec`), or record it as an airing
/// nobody planned. Returns `None` when the file is not a known spot.
pub async fn begin_airing(
    pool: &SqlitePool,
    channel_id: i32,
    air_date: &str,
    file_path: &str,
    begin_sec: Option<f64>,
    started: &str,
) -> Result<Option<i32>, ProcessError> {
    const PLANNED: &str = "SELECT id FROM airlog
        WHERE channel_id = $1 AND air_date = $2 AND kind = 'spot' AND file_path = $3
            AND result = 'pending' AND planned_begin_sec IS NOT NULL
        ORDER BY ABS(planned_begin_sec - $4), planned_begin_sec
        LIMIT 1";
    const UNPLANNED: &str = "INSERT INTO airlog (
            channel_id, air_date, kind, spot_id, campaign_id, advertiser_id,
            advertiser_name, campaign_name, spot_title, file_path, actual_start, result
        )
        VALUES ($1, $2, 'spot', $3, $4, $5, $6, $7, $8, $9, $10, 'airing')
        RETURNING id";

    let Some(identity) = select_spot_identity(pool, channel_id, file_path).await? else {
        return Ok(None);
    };

    let mut transaction = pool.begin().await?;
    let planned: Option<i32> = sqlx::query_scalar(PLANNED)
        .bind(channel_id)
        .bind(air_date)
        .bind(file_path)
        .bind(begin_sec.unwrap_or(0.0))
        .fetch_optional(&mut *transaction)
        .await?;
    let id = match planned {
        Some(id) => {
            sqlx::query("UPDATE airlog SET result = 'airing', actual_start = $1 WHERE id = $2")
                .bind(started)
                .bind(id)
                .execute(&mut *transaction)
                .await?;

            id
        }
        None => {
            sqlx::query_scalar(UNPLANNED)
                .bind(channel_id)
                .bind(air_date)
                .bind(identity.spot_id)
                .bind(identity.campaign_id)
                .bind(identity.advertiser_id)
                .bind(&identity.advertiser_name)
                .bind(&identity.campaign_name)
                .bind(&identity.spot_title)
                .bind(file_path)
                .bind(started)
                .fetch_one(&mut *transaction)
                .await?
        }
    };

    transaction.commit().await?;

    Ok(Some(id))
}

/// A spot that was on the air has ended. Only a row still `airing` changes.
pub async fn finish_airing(
    pool: &SqlitePool,
    id: i32,
    ended: &str,
    duration_sec: f64,
    result: AirResult,
    reason: &str,
) -> Result<bool, ProcessError> {
    const QUERY: &str = "UPDATE airlog
        SET actual_end = $1, actual_duration_sec = $2, result = $3, reason = $4
        WHERE id = $5 AND result = 'airing'";

    let done = sqlx::query(QUERY)
        .bind(ended)
        .bind(duration_sec)
        .bind(result)
        .bind(reason)
        .bind(id)
        .execute(pool)
        .await?;

    Ok(done.rows_affected() == 1)
}

pub async fn begin_live(
    pool: &SqlitePool,
    channel_id: i32,
    air_date: &str,
    started: &str,
) -> Result<i32, ProcessError> {
    const QUERY: &str = "INSERT INTO airlog (channel_id, air_date, kind, actual_start, result)
        VALUES ($1, $2, 'live', $3, 'airing') RETURNING id";

    let id = sqlx::query_scalar(QUERY)
        .bind(channel_id)
        .bind(air_date)
        .bind(started)
        .fetch_one(pool)
        .await?;

    Ok(id)
}

pub async fn finish_live(
    pool: &SqlitePool,
    id: i32,
    ended: &str,
    duration_sec: f64,
) -> Result<(), ProcessError> {
    sqlx::query(
        "UPDATE airlog SET actual_end = $1, actual_duration_sec = $2, result = 'played'
        WHERE id = $3 AND kind = 'live' AND result = 'airing'",
    )
    .bind(ended)
    .bind(duration_sec)
    .bind(id)
    .execute(pool)
    .await?;

    Ok(())
}

/// A graphic (CG) appears on the screen. The names of the advertiser and the
/// campaign are copied into the row like for a spot.
pub async fn begin_cg_airing(
    pool: &SqlitePool,
    channel_id: i32,
    air_date: &str,
    campaign_id: Option<i32>,
    graphic_name: &str,
    started: &str,
) -> Result<i32, ProcessError> {
    const QUERY: &str = "INSERT INTO airlog (
            channel_id, air_date, kind, campaign_id, advertiser_id, advertiser_name,
            campaign_name, spot_title, actual_start, result
        )
        SELECT $1, $2, 'cg', campaign.id, campaign.advertiser_id,
            COALESCE(advertiser.name, ''), COALESCE(campaign.name, ''), $4, $5, 'airing'
        FROM (SELECT $3 AS campaign_id)
        LEFT JOIN campaign ON campaign.id = campaign_id AND campaign.channel_id = $1
        LEFT JOIN advertiser ON advertiser.id = campaign.advertiser_id
        RETURNING id";

    let id = sqlx::query_scalar(QUERY)
        .bind(channel_id)
        .bind(air_date)
        .bind(campaign_id)
        .bind(graphic_name)
        .bind(started)
        .fetch_one(pool)
        .await?;

    Ok(id)
}

/// A restart cannot finish what was on the air. Close those rows, so the log
/// never claims something is still airing.
pub async fn close_open_airings(pool: &SqlitePool, ended: &str) -> Result<u64, ProcessError> {
    let spots = sqlx::query(
        "UPDATE airlog
        SET result = 'interrupted', reason = 'the playout stopped', actual_end = $1
        WHERE kind IN ('spot', 'cg') AND result = 'airing'",
    )
    .bind(ended)
    .execute(pool)
    .await?;
    let live = sqlx::query(
        "UPDATE airlog SET result = 'played', actual_end = $1
        WHERE kind = 'live' AND result = 'airing'",
    )
    .bind(ended)
    .execute(pool)
    .await?;

    Ok(spots.rows_affected() + live.rows_affected())
}

/// Planned rows that started before `now` and are still pending.
pub async fn select_overdue_planned(
    pool: &SqlitePool,
    now: &str,
) -> Result<Vec<Airing>, ProcessError> {
    const QUERY: &str = "SELECT * FROM airlog
        WHERE kind = 'spot' AND result = 'pending' AND planned_start IS NOT NULL
            AND planned_start <= $1
        ORDER BY planned_start, id";

    let result = sqlx::query_as(QUERY).bind(now).fetch_all(pool).await?;

    Ok(result)
}

/// The live broadcasts of a channel that overlap `[from, to]`. A broadcast
/// that has not ended has no `actual_end`.
pub async fn select_live_windows(
    pool: &SqlitePool,
    channel_id: i32,
    from: &str,
    to: &str,
) -> Result<Vec<Airing>, ProcessError> {
    const QUERY: &str = "SELECT * FROM airlog
        WHERE channel_id = $1 AND kind = 'live' AND actual_start IS NOT NULL
            AND actual_start < $3 AND (actual_end IS NULL OR actual_end > $2)
        ORDER BY actual_start";

    let result = sqlx::query_as(QUERY)
        .bind(channel_id)
        .bind(from)
        .bind(to)
        .fetch_all(pool)
        .await?;

    Ok(result)
}

/// Settle a planned row that is overdue. Only a row still `pending` changes.
pub async fn settle_pending(
    pool: &SqlitePool,
    id: i32,
    result: AirResult,
    reason: &str,
) -> Result<bool, ProcessError> {
    let done = sqlx::query(
        "UPDATE airlog SET result = $1, reason = $2 WHERE id = $3 AND result = 'pending'",
    )
    .bind(result)
    .bind(reason)
    .bind(id)
    .execute(pool)
    .await?;

    Ok(done.rows_affected() == 1)
}

pub async fn select_airings(
    pool: &SqlitePool,
    channel_id: i32,
    filter: &AiringFilter,
) -> Result<Vec<Airing>, ProcessError> {
    const QUERY: &str = "SELECT * FROM airlog
        WHERE channel_id = $1
            AND ($2 IS NULL OR air_date >= $2)
            AND ($3 IS NULL OR air_date <= $3)
            AND ($4 IS NULL OR campaign_id = $4)
            AND ($5 IS NULL OR advertiser_id = $5)
            AND ($6 IS NULL OR result = $6)
            AND ($7 IS NULL OR kind = $7)
        ORDER BY COALESCE(actual_start, planned_start, created_at), id
        LIMIT $8";

    let result = sqlx::query_as(QUERY)
        .bind(channel_id)
        .bind(&filter.date_from)
        .bind(&filter.date_to)
        .bind(filter.campaign_id)
        .bind(filter.advertiser_id)
        .bind(filter.result)
        .bind(filter.kind)
        .bind(filter.limit)
        .fetch_all(pool)
        .await?;

    Ok(result)
}

/// Delete rows older than `cutoff` (a UTC timestamp), except those of a
/// campaign that ended on or after `cutoff_date`: a campaign keeps its whole
/// history until the retention time has passed since its end.
pub async fn purge_airlog(
    pool: &SqlitePool,
    cutoff: &str,
    cutoff_date: &str,
) -> Result<u64, ProcessError> {
    const QUERY: &str = "DELETE FROM airlog
        WHERE COALESCE(actual_start, planned_start, created_at) < $1
            AND (
                campaign_id IS NULL
                OR NOT EXISTS (
                    SELECT 1 FROM campaign
                    WHERE campaign.id = airlog.campaign_id
                        AND campaign.end_date IS NOT NULL AND campaign.end_date >= $2
                )
            )";

    let done = sqlx::query(QUERY)
        .bind(cutoff)
        .bind(cutoff_date)
        .execute(pool)
        .await?;

    Ok(done.rows_affected())
}

#[cfg(test)]
mod tests {
    use sqlx::sqlite::SqlitePoolOptions;

    use super::*;
    use crate::db::{
        handles::{complete_spot, db_migrate, insert_advertiser, insert_campaign, insert_spot},
        models::{Advertiser, BreakPosition, Campaign, CampaignKind},
    };

    const DATE: &str = "2026-12-10";
    const NOON: &str = "2026-12-10T12:00:00.000Z";

    async fn pool() -> SqlitePool {
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .unwrap();
        db_migrate(&pool).await.unwrap();
        sqlx::query("INSERT INTO channels (id, name, preview_url) VALUES (2, 'Other', '')")
            .execute(&pool)
            .await
            .unwrap();

        pool
    }

    /// A spot of campaign `name` whose converted file is `path`.
    async fn spot(pool: &SqlitePool, channel: i32, name: &str, path: &str) -> SpotIdentity {
        let advertiser_id = insert_advertiser(
            pool,
            channel,
            &Advertiser {
                id: 0,
                channel_id: channel,
                name: format!("{name} advertiser"),
                contact_name: String::new(),
                email: String::new(),
                notes: String::new(),
                active: true,
            },
        )
        .await
        .unwrap();
        let campaign_id = insert_campaign(
            pool,
            channel,
            &Campaign {
                id: 0,
                channel_id: channel,
                advertiser_id,
                name: format!("{name} campaign"),
                kind: CampaignKind::Standalone,
                start_date: None,
                end_date: None,
                active: true,
                weight: 1,
                max_per_day: None,
                position: BreakPosition::Any,
            },
        )
        .await
        .unwrap()
        .unwrap();
        let spot_id = insert_spot(
            pool,
            channel,
            campaign_id,
            &format!("{name} spot"),
            &format!("upload/{name}.mxf"),
            "x.mxf",
            None,
        )
        .await
        .unwrap()
        .unwrap();

        complete_spot(pool, channel, spot_id, path, 30.0)
            .await
            .unwrap();

        select_spot_identity(pool, channel, path)
            .await
            .unwrap()
            .unwrap()
    }

    fn planned(identity: &SpotIdentity, path: &str, begin: f64, start: &str) -> PlannedAiring {
        PlannedAiring {
            spot_id: identity.spot_id,
            campaign_id: identity.campaign_id,
            advertiser_id: identity.advertiser_id,
            advertiser_name: identity.advertiser_name.clone(),
            campaign_name: identity.campaign_name.clone(),
            spot_title: identity.spot_title.clone(),
            file_path: path.to_string(),
            break_window_id: None,
            position_in_break: Some(0),
            planned_begin_sec: begin,
            planned_start: Some(start.to_string()),
            planned_duration_sec: 30.0,
        }
    }

    async fn all(pool: &SqlitePool) -> Vec<Airing> {
        select_airings(
            pool,
            1,
            &AiringFilter {
                limit: 1000,
                ..AiringFilter::default()
            },
        )
        .await
        .unwrap()
    }

    #[tokio::test]
    async fn the_identity_of_a_spot_file_is_found_by_its_converted_path() {
        let pool = pool().await;
        let identity = spot(&pool, 1, "padaria", "commercials/spot-1.mp4").await;

        assert_eq!(identity.advertiser_name, "padaria advertiser");
        assert_eq!(identity.campaign_name, "padaria campaign");
        assert_eq!(identity.spot_title, "padaria spot");
        assert!(
            select_spot_identity(&pool, 1, "programs/show.mp4")
                .await
                .unwrap()
                .is_none()
        );
        assert!(
            select_spot_identity(&pool, 2, "commercials/spot-1.mp4")
                .await
                .unwrap()
                .is_none(),
            "another channel does not see it"
        );
    }

    #[tokio::test]
    async fn applying_a_plan_records_pending_rows() {
        let pool = pool().await;
        let identity = spot(&pool, 1, "padaria", "commercials/spot-1.mp4").await;

        replace_planned_airings(
            &pool,
            1,
            DATE,
            "2026-12-09T00:00:00.000Z",
            &[
                planned(
                    &identity,
                    "commercials/spot-1.mp4",
                    21_600.0,
                    "2026-12-10T09:00:00.000Z",
                ),
                planned(
                    &identity,
                    "commercials/spot-1.mp4",
                    43_200.0,
                    "2026-12-10T15:00:00.000Z",
                ),
            ],
        )
        .await
        .unwrap();

        let rows = all(&pool).await;

        assert_eq!(rows.len(), 2);
        assert!(rows.iter().all(|row| row.result == AirResult::Pending));
        assert!(rows.iter().all(|row| row.kind == AirKind::Spot));
        assert_eq!(rows[0].planned_begin_sec, Some(21_600.0));
        assert_eq!(rows[0].advertiser_name, "padaria advertiser");
        assert_eq!(rows[0].planned_duration_sec, Some(30.0));
        assert!(rows[0].actual_start.is_none());
    }

    #[tokio::test]
    async fn planning_again_replaces_the_future_and_keeps_the_history() {
        let pool = pool().await;
        let identity = spot(&pool, 1, "padaria", "commercials/spot-1.mp4").await;
        let path = "commercials/spot-1.mp4";

        replace_planned_airings(
            &pool,
            1,
            DATE,
            "2026-12-09T00:00:00.000Z",
            &[
                planned(&identity, path, 21_600.0, "2026-12-10T09:00:00.000Z"),
                planned(&identity, path, 43_200.0, "2026-12-10T15:00:00.000Z"),
                planned(&identity, path, 64_800.0, "2026-12-10T21:00:00.000Z"),
            ],
        )
        .await
        .unwrap();

        // The 09:00 spot aired; the day is replanned at 12:00.
        let aired = begin_airing(
            &pool,
            1,
            DATE,
            path,
            Some(21_600.0),
            "2026-12-10T09:00:00.100Z",
        )
        .await
        .unwrap()
        .unwrap();

        finish_airing(
            &pool,
            aired,
            "2026-12-10T09:00:30.100Z",
            30.0,
            AirResult::Played,
            "",
        )
        .await
        .unwrap();
        replace_planned_airings(
            &pool,
            1,
            DATE,
            NOON,
            &[planned(
                &identity,
                path,
                54_000.0,
                "2026-12-10T18:00:00.000Z",
            )],
        )
        .await
        .unwrap();

        let rows = all(&pool).await;
        let starts: Vec<_> = rows
            .iter()
            .map(|row| row.planned_start.clone().unwrap())
            .collect();

        assert_eq!(
            rows.len(),
            2,
            "the aired row stays, the two pending futures are replaced"
        );
        assert_eq!(starts[0], "2026-12-10T09:00:00.000Z");
        assert_eq!(starts[1], "2026-12-10T18:00:00.000Z");
        assert_eq!(rows[0].result, AirResult::Played);
        assert_eq!(rows[1].result, AirResult::Pending);
    }

    #[tokio::test]
    async fn a_pending_row_from_the_past_is_not_replaced() {
        let pool = pool().await;
        let identity = spot(&pool, 1, "padaria", "commercials/spot-1.mp4").await;
        let path = "commercials/spot-1.mp4";

        replace_planned_airings(
            &pool,
            1,
            DATE,
            "2026-12-09T00:00:00.000Z",
            &[planned(
                &identity,
                path,
                21_600.0,
                "2026-12-10T09:00:00.000Z",
            )],
        )
        .await
        .unwrap();
        replace_planned_airings(&pool, 1, DATE, NOON, &[])
            .await
            .unwrap();

        assert_eq!(all(&pool).await.len(), 1, "it was due before the new plan");
    }

    #[tokio::test]
    async fn replanning_a_date_does_not_touch_other_dates_or_channels() {
        let pool = pool().await;
        let identity = spot(&pool, 1, "padaria", "commercials/spot-1.mp4").await;
        let path = "commercials/spot-1.mp4";
        let early = "2026-12-01T00:00:00.000Z";

        replace_planned_airings(
            &pool,
            1,
            "2026-12-11",
            early,
            &[planned(&identity, path, 100.0, "2026-12-11T09:00:00.000Z")],
        )
        .await
        .unwrap();
        replace_planned_airings(
            &pool,
            2,
            DATE,
            early,
            &[planned(&identity, path, 100.0, "2026-12-10T09:00:00.000Z")],
        )
        .await
        .unwrap();
        replace_planned_airings(&pool, 1, DATE, early, &[])
            .await
            .unwrap();

        assert_eq!(
            all(&pool).await.len(),
            1,
            "the other date of channel 1 stays"
        );
        assert_eq!(
            select_airings(
                &pool,
                2,
                &AiringFilter {
                    limit: 10,
                    ..AiringFilter::default()
                }
            )
            .await
            .unwrap()
            .len(),
            1,
            "channel 2 is untouched"
        );
    }

    #[tokio::test]
    async fn a_starting_spot_claims_the_planned_row_closest_to_its_begin() {
        let pool = pool().await;
        let identity = spot(&pool, 1, "padaria", "commercials/spot-1.mp4").await;
        let path = "commercials/spot-1.mp4";

        replace_planned_airings(
            &pool,
            1,
            DATE,
            "2026-12-09T00:00:00.000Z",
            &[
                planned(&identity, path, 21_600.0, "2026-12-10T09:00:00.000Z"),
                planned(&identity, path, 43_200.0, "2026-12-10T15:00:00.000Z"),
                planned(&identity, path, 64_800.0, "2026-12-10T21:00:00.000Z"),
            ],
        )
        .await
        .unwrap();

        let id = begin_airing(
            &pool,
            1,
            DATE,
            path,
            Some(43_210.0),
            "2026-12-10T15:00:10.000Z",
        )
        .await
        .unwrap()
        .unwrap();
        let rows = all(&pool).await;
        let claimed = rows.iter().find(|row| row.id == id).unwrap();

        assert_eq!(claimed.planned_begin_sec, Some(43_200.0));
        assert_eq!(claimed.result, AirResult::Airing);
        assert_eq!(
            claimed.actual_start.as_deref(),
            Some("2026-12-10T15:00:10.000Z")
        );
        assert_eq!(rows.len(), 3, "no new row was created");
        assert_eq!(
            rows.iter()
                .filter(|row| row.result == AirResult::Pending)
                .count(),
            2
        );
    }

    #[tokio::test]
    async fn the_same_planned_row_is_not_claimed_twice() {
        let pool = pool().await;
        let identity = spot(&pool, 1, "padaria", "commercials/spot-1.mp4").await;
        let path = "commercials/spot-1.mp4";

        replace_planned_airings(
            &pool,
            1,
            DATE,
            "2026-12-09T00:00:00.000Z",
            &[planned(
                &identity,
                path,
                21_600.0,
                "2026-12-10T09:00:00.000Z",
            )],
        )
        .await
        .unwrap();

        let first = begin_airing(&pool, 1, DATE, path, Some(21_600.0), NOON)
            .await
            .unwrap()
            .unwrap();
        let second = begin_airing(&pool, 1, DATE, path, Some(21_600.0), NOON)
            .await
            .unwrap()
            .unwrap();

        assert_ne!(first, second);

        let rows = all(&pool).await;

        assert_eq!(rows.len(), 2);
        assert!(rows.iter().any(|row| row.planned_begin_sec.is_none()));
    }

    #[tokio::test]
    async fn a_spot_nobody_planned_is_recorded_as_an_unplanned_airing() {
        let pool = pool().await;
        let identity = spot(&pool, 1, "padaria", "commercials/spot-1.mp4").await;
        let id = begin_airing(&pool, 1, DATE, "commercials/spot-1.mp4", Some(100.0), NOON)
            .await
            .unwrap()
            .unwrap();
        let row = all(&pool).await.remove(0);

        assert_eq!(row.id, id);
        assert_eq!(row.spot_id, Some(identity.spot_id));
        assert_eq!(row.campaign_name, "padaria campaign");
        assert!(row.planned_start.is_none() && row.planned_begin_sec.is_none());
        assert_eq!(row.result, AirResult::Airing);
    }

    #[tokio::test]
    async fn a_file_that_is_not_a_spot_leaves_no_trace() {
        let pool = pool().await;

        assert!(
            begin_airing(&pool, 1, DATE, "programs/show.mp4", Some(100.0), NOON)
                .await
                .unwrap()
                .is_none()
        );
        assert!(all(&pool).await.is_empty());
    }

    #[tokio::test]
    async fn finishing_records_the_end_once() {
        let pool = pool().await;

        spot(&pool, 1, "padaria", "commercials/spot-1.mp4").await;

        let id = begin_airing(&pool, 1, DATE, "commercials/spot-1.mp4", None, NOON)
            .await
            .unwrap()
            .unwrap();

        assert!(
            finish_airing(
                &pool,
                id,
                "2026-12-10T12:00:30.000Z",
                30.0,
                AirResult::Played,
                ""
            )
            .await
            .unwrap()
        );
        assert!(
            !finish_airing(
                &pool,
                id,
                "2026-12-10T12:09:00.000Z",
                9.0,
                AirResult::Failed,
                "late"
            )
            .await
            .unwrap(),
            "a finished row does not change again"
        );

        let row = all(&pool).await.remove(0);

        assert_eq!(row.result, AirResult::Played);
        assert_eq!(row.actual_duration_sec, Some(30.0));
        assert_eq!(row.actual_end.as_deref(), Some("2026-12-10T12:00:30.000Z"));
    }

    #[tokio::test]
    async fn a_restart_closes_what_was_on_the_air() {
        let pool = pool().await;

        spot(&pool, 1, "padaria", "commercials/spot-1.mp4").await;

        begin_airing(&pool, 1, DATE, "commercials/spot-1.mp4", None, NOON)
            .await
            .unwrap();
        begin_live(&pool, 1, DATE, NOON).await.unwrap();

        assert_eq!(
            close_open_airings(&pool, "2026-12-10T12:05:00.000Z")
                .await
                .unwrap(),
            2
        );

        let rows = all(&pool).await;
        let spot_row = rows.iter().find(|row| row.kind == AirKind::Spot).unwrap();
        let live_row = rows.iter().find(|row| row.kind == AirKind::Live).unwrap();

        assert_eq!(spot_row.result, AirResult::Interrupted);
        assert_eq!(live_row.result, AirResult::Played);
        assert!(rows.iter().all(|row| row.actual_end.is_some()));
        assert_eq!(close_open_airings(&pool, NOON).await.unwrap(), 0);
    }

    #[tokio::test]
    async fn live_windows_that_overlap_a_period_are_found() {
        let pool = pool().await;
        let ended = |hour: u32| format!("2026-12-10T{hour:02}:00:00.000Z");

        for (from, to) in [(8, 9), (10, 12), (14, 15)] {
            let id = begin_live(&pool, 1, DATE, &ended(from)).await.unwrap();

            finish_live(&pool, id, &ended(to), 3600.0).await.unwrap();
        }

        let open = begin_live(&pool, 1, DATE, &ended(18)).await.unwrap();
        let found = select_live_windows(&pool, 1, &ended(11), &ended(14))
            .await
            .unwrap();

        assert_eq!(found.len(), 1, "only the 10 to 12 window overlaps 11 to 14");
        assert_eq!(
            found[0].actual_start.as_deref(),
            Some("2026-12-10T10:00:00.000Z")
        );

        let still_on = select_live_windows(&pool, 1, &ended(19), &ended(20))
            .await
            .unwrap();

        assert_eq!(
            still_on.len(),
            1,
            "a broadcast that has not ended reaches any later time"
        );
        assert_eq!(still_on[0].id, open);
        assert!(
            select_live_windows(&pool, 2, &ended(0), &ended(23))
                .await
                .unwrap()
                .is_empty()
        );
    }

    #[tokio::test]
    async fn overdue_rows_are_those_started_and_still_pending() {
        let pool = pool().await;
        let identity = spot(&pool, 1, "padaria", "commercials/spot-1.mp4").await;
        let path = "commercials/spot-1.mp4";

        replace_planned_airings(
            &pool,
            1,
            DATE,
            "2026-12-09T00:00:00.000Z",
            &[
                planned(&identity, path, 21_600.0, "2026-12-10T09:00:00.000Z"),
                planned(&identity, path, 64_800.0, "2026-12-10T21:00:00.000Z"),
            ],
        )
        .await
        .unwrap();

        let overdue = select_overdue_planned(&pool, NOON).await.unwrap();

        assert_eq!(overdue.len(), 1);
        assert_eq!(overdue[0].planned_begin_sec, Some(21_600.0));
        assert!(
            settle_pending(&pool, overdue[0].id, AirResult::NotAired, "never started")
                .await
                .unwrap()
        );
        assert!(
            !settle_pending(&pool, overdue[0].id, AirResult::LiveMissed, "again")
                .await
                .unwrap(),
            "a settled row stays settled"
        );
        assert!(
            select_overdue_planned(&pool, NOON)
                .await
                .unwrap()
                .is_empty()
        );
    }

    #[tokio::test]
    async fn the_log_can_be_filtered() {
        let pool = pool().await;
        let first = spot(&pool, 1, "padaria", "commercials/spot-1.mp4").await;
        let second = spot(&pool, 1, "mercado", "commercials/spot-2.mp4").await;

        let row = |identity: &SpotIdentity, path: &str, date: &str| {
            planned(identity, path, 100.0, &format!("{date}T09:00:00.000Z"))
        };

        // One call per date: a later call replaces the pending rows of its date.
        for (date, rows) in [
            (
                "2026-12-10",
                vec![row(&first, "commercials/spot-1.mp4", "2026-12-10")],
            ),
            (
                "2026-12-11",
                vec![
                    row(&first, "commercials/spot-1.mp4", "2026-12-11"),
                    row(&second, "commercials/spot-2.mp4", "2026-12-11"),
                ],
            ),
        ] {
            replace_planned_airings(&pool, 1, date, "2026-12-01T00:00:00.000Z", &rows)
                .await
                .unwrap();
        }

        let with_default_limit = |filter: AiringFilter| AiringFilter {
            limit: if filter.limit == 0 { 100 } else { filter.limit },
            ..filter
        };
        let count = |filter: AiringFilter| {
            let pool = pool.clone();

            async move {
                select_airings(&pool, 1, &with_default_limit(filter))
                    .await
                    .unwrap()
                    .len()
            }
        };

        assert_eq!(count(AiringFilter::default()).await, 3);
        assert_eq!(
            count(AiringFilter {
                date_from: Some("2026-12-11".into()),
                ..AiringFilter::default()
            })
            .await,
            2
        );
        assert_eq!(
            count(AiringFilter {
                date_to: Some("2026-12-10".into()),
                ..AiringFilter::default()
            })
            .await,
            1
        );
        assert_eq!(
            count(AiringFilter {
                campaign_id: Some(second.campaign_id),
                ..AiringFilter::default()
            })
            .await,
            1
        );
        assert_eq!(
            count(AiringFilter {
                advertiser_id: Some(first.advertiser_id),
                ..AiringFilter::default()
            })
            .await,
            2
        );
        assert_eq!(
            count(AiringFilter {
                result: Some(AirResult::Played),
                ..AiringFilter::default()
            })
            .await,
            0
        );
        assert_eq!(
            count(AiringFilter {
                limit: 2,
                ..AiringFilter::default()
            })
            .await,
            2
        );
        assert_eq!(
            select_airings(
                &pool,
                2,
                &AiringFilter {
                    limit: 100,
                    ..AiringFilter::default()
                }
            )
            .await
            .unwrap()
            .len(),
            0,
            "another channel sees nothing"
        );
    }

    #[tokio::test]
    async fn purging_keeps_recent_rows_and_the_history_of_recent_campaigns() {
        let pool = pool().await;
        let old = spot(&pool, 1, "old", "commercials/spot-1.mp4").await;
        let running = spot(&pool, 1, "running", "commercials/spot-2.mp4").await;

        // The second campaign ended recently: its history must stay.
        sqlx::query(
            "UPDATE campaign SET kind = 'campaign', start_date = '2026-01-01',
            end_date = '2026-11-20' WHERE id = $1",
        )
        .bind(running.campaign_id)
        .execute(&pool)
        .await
        .unwrap();

        // One call per date: a later call replaces the pending rows of its date.
        for (date, rows) in [
            (
                "2026-08-01",
                vec![
                    planned(
                        &old,
                        "commercials/spot-1.mp4",
                        100.0,
                        "2026-08-01T09:00:00.000Z",
                    ),
                    planned(
                        &running,
                        "commercials/spot-2.mp4",
                        200.0,
                        "2026-08-01T09:01:00.000Z",
                    ),
                ],
            ),
            (
                "2026-12-05",
                vec![planned(
                    &old,
                    "commercials/spot-1.mp4",
                    100.0,
                    "2026-12-05T09:00:00.000Z",
                )],
            ),
        ] {
            replace_planned_airings(&pool, 1, date, "2026-01-01T00:00:00.000Z", &rows)
                .await
                .unwrap();
        }

        let live = begin_live(&pool, 1, "2026-08-01", "2026-08-01T10:00:00.000Z")
            .await
            .unwrap();

        finish_live(&pool, live, "2026-08-01T11:00:00.000Z", 3600.0)
            .await
            .unwrap();

        // 60 days before 2026-12-10 is 2026-10-11.
        let removed = purge_airlog(&pool, "2026-10-11T00:00:00.000Z", "2026-10-11")
            .await
            .unwrap();
        let left = all(&pool).await;

        assert_eq!(removed, 2, "the old spot row and the old live window");
        assert_eq!(left.len(), 2);
        assert!(left.iter().any(|row| row.air_date == "2026-12-05"));
        assert!(
            left.iter()
                .any(|row| row.air_date == "2026-08-01"
                    && row.campaign_id == Some(running.campaign_id)),
            "the campaign that ended on 2026-11-20 keeps its August history"
        );
    }

    #[tokio::test]
    async fn a_graphic_of_a_campaign_is_logged_like_a_spot() {
        let pool = pool().await;
        let identity = spot(&pool, 1, "Natal", "commercials/natal.mp4").await;

        let id = begin_cg_airing(&pool, 1, DATE, Some(identity.campaign_id), "QR Natal", NOON)
            .await
            .unwrap();
        let open = all(&pool).await;

        assert_eq!(open.len(), 1);
        assert_eq!(open[0].kind, AirKind::Cg);
        assert_eq!(open[0].result, AirResult::Airing);
        assert_eq!(open[0].spot_title, "QR Natal");
        assert_eq!(open[0].campaign_id, Some(identity.campaign_id));
        assert_eq!(open[0].campaign_name, identity.campaign_name);
        assert_eq!(open[0].advertiser_name, identity.advertiser_name);

        assert!(
            finish_airing(
                &pool,
                id,
                "2026-12-10T12:00:15.000Z",
                15.0,
                AirResult::Played,
                ""
            )
            .await
            .unwrap()
        );

        let closed = all(&pool).await;

        assert_eq!(closed[0].result, AirResult::Played);
        assert_eq!(closed[0].actual_duration_sec, Some(15.0));
    }

    #[tokio::test]
    async fn a_graphic_without_a_campaign_is_logged_and_a_restart_interrupts_it() {
        let pool = pool().await;

        begin_cg_airing(&pool, 1, DATE, None, "Selo", NOON)
            .await
            .unwrap();
        // A campaign that is not of this channel leaves the names empty.
        let other = spot(&pool, 2, "Outra", "commercials/outra.mp4").await;

        begin_cg_airing(&pool, 1, DATE, Some(other.campaign_id), "Selo 2", NOON)
            .await
            .unwrap();

        let rows = all(&pool).await;

        assert!(rows.iter().all(|row| row.campaign_name.is_empty()));
        assert_eq!(close_open_airings(&pool, NOON).await.unwrap(), 2);
        assert!(
            all(&pool)
                .await
                .iter()
                .all(|row| row.result == AirResult::Interrupted)
        );
    }
}
