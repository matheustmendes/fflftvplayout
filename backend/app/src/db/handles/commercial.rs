use sqlx::sqlite::SqlitePool;

use crate::{
    db::models::{Advertiser, BreakWindow, Campaign, Spot, SpotStatus},
    utils::errors::ProcessError,
};

// Every query is scoped by `channel_id`, so a caller can never read or change
// the rows of another channel by guessing an id.

pub async fn select_advertisers(
    pool: &SqlitePool,
    channel_id: i32,
    include_archived: bool,
) -> Result<Vec<Advertiser>, ProcessError> {
    const QUERY: &str = "SELECT * FROM advertiser
        WHERE channel_id = $1 AND (active = 1 OR $2)
        ORDER BY name COLLATE NOCASE";

    let result = sqlx::query_as(QUERY)
        .bind(channel_id)
        .bind(include_archived)
        .fetch_all(pool)
        .await?;

    Ok(result)
}

pub async fn select_advertiser(
    pool: &SqlitePool,
    channel_id: i32,
    id: i32,
) -> Result<Option<Advertiser>, ProcessError> {
    const QUERY: &str = "SELECT * FROM advertiser WHERE channel_id = $1 AND id = $2";

    let result = sqlx::query_as(QUERY)
        .bind(channel_id)
        .bind(id)
        .fetch_optional(pool)
        .await?;

    Ok(result)
}

pub async fn insert_advertiser(
    pool: &SqlitePool,
    channel_id: i32,
    advertiser: &Advertiser,
) -> Result<i32, ProcessError> {
    const QUERY: &str =
        "INSERT INTO advertiser (channel_id, name, contact_name, email, notes, active)
        VALUES ($1, $2, $3, $4, $5, $6) RETURNING id";

    let id = sqlx::query_scalar(QUERY)
        .bind(channel_id)
        .bind(advertiser.name.trim())
        .bind(advertiser.contact_name.trim())
        .bind(advertiser.email.trim())
        .bind(&advertiser.notes)
        .bind(advertiser.active)
        .fetch_one(pool)
        .await?;

    Ok(id)
}

pub async fn update_advertiser(
    pool: &SqlitePool,
    channel_id: i32,
    id: i32,
    advertiser: &Advertiser,
) -> Result<bool, ProcessError> {
    const QUERY: &str = "UPDATE advertiser
        SET name = $1, contact_name = $2, email = $3, notes = $4, active = $5
        WHERE channel_id = $6 AND id = $7";

    let result = sqlx::query(QUERY)
        .bind(advertiser.name.trim())
        .bind(advertiser.contact_name.trim())
        .bind(advertiser.email.trim())
        .bind(&advertiser.notes)
        .bind(advertiser.active)
        .bind(channel_id)
        .bind(id)
        .execute(pool)
        .await?;

    Ok(result.rows_affected() == 1)
}

/// Archive an advertiser together with its campaigns and their spots.
pub async fn archive_advertiser(
    pool: &SqlitePool,
    channel_id: i32,
    id: i32,
) -> Result<bool, ProcessError> {
    let mut transaction = pool.begin().await?;
    let result = sqlx::query("UPDATE advertiser SET active = 0 WHERE channel_id = $1 AND id = $2")
        .bind(channel_id)
        .bind(id)
        .execute(&mut *transaction)
        .await?;

    if result.rows_affected() != 1 {
        return Ok(false);
    }

    sqlx::query(
        "UPDATE spot SET active = 0 WHERE channel_id = $1 AND campaign_id IN (
            SELECT id FROM campaign WHERE channel_id = $1 AND advertiser_id = $2)",
    )
    .bind(channel_id)
    .bind(id)
    .execute(&mut *transaction)
    .await?;
    sqlx::query("UPDATE campaign SET active = 0 WHERE channel_id = $1 AND advertiser_id = $2")
        .bind(channel_id)
        .bind(id)
        .execute(&mut *transaction)
        .await?;
    transaction.commit().await?;

    Ok(true)
}

pub async fn select_campaigns(
    pool: &SqlitePool,
    channel_id: i32,
    include_archived: bool,
) -> Result<Vec<Campaign>, ProcessError> {
    const QUERY: &str = "SELECT * FROM campaign
        WHERE channel_id = $1 AND (active = 1 OR $2)
        ORDER BY start_date IS NULL, start_date, name COLLATE NOCASE";

    let result = sqlx::query_as(QUERY)
        .bind(channel_id)
        .bind(include_archived)
        .fetch_all(pool)
        .await?;

    Ok(result)
}

pub async fn select_campaign(
    pool: &SqlitePool,
    channel_id: i32,
    id: i32,
) -> Result<Option<Campaign>, ProcessError> {
    const QUERY: &str = "SELECT * FROM campaign WHERE channel_id = $1 AND id = $2";

    let result = sqlx::query_as(QUERY)
        .bind(channel_id)
        .bind(id)
        .fetch_optional(pool)
        .await?;

    Ok(result)
}

/// Insert a campaign. Returns `None` when the advertiser does not exist in
/// this channel or is archived.
pub async fn insert_campaign(
    pool: &SqlitePool,
    channel_id: i32,
    campaign: &Campaign,
) -> Result<Option<i32>, ProcessError> {
    const QUERY: &str = "INSERT INTO campaign (
            channel_id, advertiser_id, name, kind, start_date, end_date, active,
            weight, max_per_day, position
        )
        SELECT $1, advertiser.id, $3, $4, $5, $6, $7, $8, $9, $10
        FROM advertiser
        WHERE advertiser.id = $2 AND advertiser.channel_id = $1 AND advertiser.active = 1
        RETURNING id";

    let id = sqlx::query_scalar(QUERY)
        .bind(channel_id)
        .bind(campaign.advertiser_id)
        .bind(campaign.name.trim())
        .bind(campaign.kind)
        .bind(&campaign.start_date)
        .bind(&campaign.end_date)
        .bind(campaign.active)
        .bind(campaign.weight)
        .bind(campaign.max_per_day)
        .bind(campaign.position)
        .fetch_optional(pool)
        .await?;

    Ok(id)
}

/// Update a campaign. Returns `false` when the campaign, or the advertiser it
/// should move to, does not exist in this channel.
pub async fn update_campaign(
    pool: &SqlitePool,
    channel_id: i32,
    id: i32,
    campaign: &Campaign,
) -> Result<bool, ProcessError> {
    const QUERY: &str = "UPDATE campaign
        SET advertiser_id = $1, name = $2, kind = $3, start_date = $4, end_date = $5,
            active = $6, weight = $7, max_per_day = $8, position = $9
        WHERE channel_id = $10 AND id = $11
            AND EXISTS (SELECT 1 FROM advertiser WHERE id = $1 AND channel_id = $10)";

    let result = sqlx::query(QUERY)
        .bind(campaign.advertiser_id)
        .bind(campaign.name.trim())
        .bind(campaign.kind)
        .bind(&campaign.start_date)
        .bind(&campaign.end_date)
        .bind(campaign.active)
        .bind(campaign.weight)
        .bind(campaign.max_per_day)
        .bind(campaign.position)
        .bind(channel_id)
        .bind(id)
        .execute(pool)
        .await?;

    Ok(result.rows_affected() == 1)
}

/// Archive a campaign together with its spots.
pub async fn archive_campaign(
    pool: &SqlitePool,
    channel_id: i32,
    id: i32,
) -> Result<bool, ProcessError> {
    let mut transaction = pool.begin().await?;
    let result = sqlx::query("UPDATE campaign SET active = 0 WHERE channel_id = $1 AND id = $2")
        .bind(channel_id)
        .bind(id)
        .execute(&mut *transaction)
        .await?;

    if result.rows_affected() != 1 {
        return Ok(false);
    }

    sqlx::query("UPDATE spot SET active = 0 WHERE channel_id = $1 AND campaign_id = $2")
        .bind(channel_id)
        .bind(id)
        .execute(&mut *transaction)
        .await?;
    transaction.commit().await?;

    Ok(true)
}

pub async fn select_spots(
    pool: &SqlitePool,
    channel_id: i32,
    campaign_id: Option<i32>,
    include_archived: bool,
) -> Result<Vec<Spot>, ProcessError> {
    const QUERY: &str = "SELECT * FROM spot
        WHERE channel_id = $1 AND ($2 IS NULL OR campaign_id = $2) AND (active = 1 OR $3)
        ORDER BY created_at DESC, id DESC";

    let result = sqlx::query_as(QUERY)
        .bind(channel_id)
        .bind(campaign_id)
        .bind(include_archived)
        .fetch_all(pool)
        .await?;

    Ok(result)
}

pub async fn select_spot(
    pool: &SqlitePool,
    channel_id: i32,
    id: i32,
) -> Result<Option<Spot>, ProcessError> {
    const QUERY: &str = "SELECT * FROM spot WHERE channel_id = $1 AND id = $2";

    let result = sqlx::query_as(QUERY)
        .bind(channel_id)
        .bind(id)
        .fetch_optional(pool)
        .await?;

    Ok(result)
}

/// Insert a spot in the `processing` state. Returns `None` when the campaign
/// does not exist in this channel or is archived.
pub async fn insert_spot(
    pool: &SqlitePool,
    channel_id: i32,
    campaign_id: i32,
    title: &str,
    file_path: &str,
    original_filename: &str,
    created_by: Option<i32>,
) -> Result<Option<i32>, ProcessError> {
    const QUERY: &str = "INSERT INTO spot (
            channel_id, campaign_id, title, file_path, original_filename, created_by
        )
        SELECT $1, campaign.id, $3, $4, $5, $6
        FROM campaign
        WHERE campaign.id = $2 AND campaign.channel_id = $1 AND campaign.active = 1
        RETURNING id";

    let id = sqlx::query_scalar(QUERY)
        .bind(channel_id)
        .bind(campaign_id)
        .bind(title.trim())
        .bind(file_path)
        .bind(original_filename)
        .bind(created_by)
        .fetch_optional(pool)
        .await?;

    Ok(id)
}

pub async fn update_spot_status(
    pool: &SqlitePool,
    channel_id: i32,
    id: i32,
    status: SpotStatus,
    duration_sec: Option<f64>,
    error: &str,
) -> Result<bool, ProcessError> {
    const QUERY: &str = "UPDATE spot SET status = $1, duration_sec = $2, error = $3
        WHERE channel_id = $4 AND id = $5";

    let result = sqlx::query(QUERY)
        .bind(status)
        .bind(duration_sec)
        .bind(error)
        .bind(channel_id)
        .bind(id)
        .execute(pool)
        .await?;

    Ok(result.rows_affected() == 1)
}

pub async fn archive_spot(
    pool: &SqlitePool,
    channel_id: i32,
    id: i32,
) -> Result<bool, ProcessError> {
    let result = sqlx::query("UPDATE spot SET active = 0 WHERE channel_id = $1 AND id = $2")
        .bind(channel_id)
        .bind(id)
        .execute(pool)
        .await?;

    Ok(result.rows_affected() == 1)
}

pub async fn select_break_windows(
    pool: &SqlitePool,
    channel_id: i32,
    weekday: Option<i32>,
) -> Result<Vec<BreakWindow>, ProcessError> {
    const QUERY: &str = "SELECT * FROM break_window
        WHERE channel_id = $1 AND ($2 IS NULL OR weekday = $2)
        ORDER BY weekday, nominal_time";

    let result = sqlx::query_as(QUERY)
        .bind(channel_id)
        .bind(weekday)
        .fetch_all(pool)
        .await?;

    Ok(result)
}

pub async fn insert_break_window(
    pool: &SqlitePool,
    channel_id: i32,
    window: &BreakWindow,
) -> Result<i32, ProcessError> {
    const QUERY: &str = "INSERT INTO break_window (
            channel_id, weekday, nominal_time, duration_sec, tolerance_sec, enabled
        )
        VALUES ($1, $2, $3, $4, $5, $6) RETURNING id";

    let id = sqlx::query_scalar(QUERY)
        .bind(channel_id)
        .bind(window.weekday)
        .bind(&window.nominal_time)
        .bind(window.duration_sec)
        .bind(window.tolerance_sec)
        .bind(window.enabled)
        .fetch_one(pool)
        .await?;

    Ok(id)
}

pub async fn update_break_window(
    pool: &SqlitePool,
    channel_id: i32,
    id: i32,
    window: &BreakWindow,
) -> Result<bool, ProcessError> {
    const QUERY: &str = "UPDATE break_window
        SET weekday = $1, nominal_time = $2, duration_sec = $3, tolerance_sec = $4, enabled = $5
        WHERE channel_id = $6 AND id = $7";

    let result = sqlx::query(QUERY)
        .bind(window.weekday)
        .bind(&window.nominal_time)
        .bind(window.duration_sec)
        .bind(window.tolerance_sec)
        .bind(window.enabled)
        .bind(channel_id)
        .bind(id)
        .execute(pool)
        .await?;

    Ok(result.rows_affected() == 1)
}

pub async fn delete_break_window(
    pool: &SqlitePool,
    channel_id: i32,
    id: i32,
) -> Result<bool, ProcessError> {
    let result = sqlx::query("DELETE FROM break_window WHERE channel_id = $1 AND id = $2")
        .bind(channel_id)
        .bind(id)
        .execute(pool)
        .await?;

    Ok(result.rows_affected() == 1)
}

#[cfg(test)]
mod tests {
    use sqlx::sqlite::SqlitePoolOptions;

    use super::*;
    use crate::db::{
        handles::db_migrate,
        models::{BreakPosition, CampaignKind},
    };

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

    fn advertiser(name: &str) -> Advertiser {
        Advertiser {
            id: 0,
            channel_id: 0,
            name: name.to_string(),
            contact_name: String::new(),
            email: String::new(),
            notes: String::new(),
            active: true,
        }
    }

    fn campaign(advertiser_id: i32, name: &str) -> Campaign {
        Campaign {
            id: 0,
            channel_id: 0,
            advertiser_id,
            name: name.to_string(),
            kind: CampaignKind::Campaign,
            start_date: Some("2026-10-01".to_string()),
            end_date: Some("2026-10-31".to_string()),
            active: true,
            weight: 2,
            max_per_day: Some(3),
            position: BreakPosition::First,
        }
    }

    fn window(weekday: i32, time: &str) -> BreakWindow {
        BreakWindow {
            id: 0,
            channel_id: 0,
            weekday,
            nominal_time: time.to_string(),
            duration_sec: 180,
            tolerance_sec: 60,
            enabled: true,
        }
    }

    #[tokio::test]
    async fn advertisers_are_scoped_to_their_channel() {
        let pool = pool().await;
        let id = insert_advertiser(&pool, 1, &advertiser("Padaria"))
            .await
            .unwrap();

        assert!(select_advertiser(&pool, 1, id).await.unwrap().is_some());
        assert!(select_advertiser(&pool, 2, id).await.unwrap().is_none());
        assert!(
            select_advertisers(&pool, 2, false)
                .await
                .unwrap()
                .is_empty()
        );
        assert!(
            !update_advertiser(&pool, 2, id, &advertiser("Hacked"))
                .await
                .unwrap()
        );
        assert!(!archive_advertiser(&pool, 2, id).await.unwrap());
        assert_eq!(
            select_advertiser(&pool, 1, id).await.unwrap().unwrap().name,
            "Padaria"
        );
    }

    #[tokio::test]
    async fn active_advertiser_names_are_unique_per_channel_ignoring_case() {
        let pool = pool().await;
        insert_advertiser(&pool, 1, &advertiser("Padaria"))
            .await
            .unwrap();

        assert!(
            insert_advertiser(&pool, 1, &advertiser("PADARIA"))
                .await
                .is_err()
        );
        assert!(
            insert_advertiser(&pool, 2, &advertiser("Padaria"))
                .await
                .is_ok()
        );
    }

    #[tokio::test]
    async fn archiving_an_advertiser_archives_campaigns_and_spots() {
        let pool = pool().await;
        let advertiser_id = insert_advertiser(&pool, 1, &advertiser("Padaria"))
            .await
            .unwrap();
        let campaign_id = insert_campaign(&pool, 1, &campaign(advertiser_id, "Natal"))
            .await
            .unwrap()
            .unwrap();
        let spot_id = insert_spot(
            &pool,
            1,
            campaign_id,
            "Spot",
            "ads/spot.mp4",
            "spot.mxf",
            None,
        )
        .await
        .unwrap()
        .unwrap();

        assert!(archive_advertiser(&pool, 1, advertiser_id).await.unwrap());

        assert!(select_campaigns(&pool, 1, false).await.unwrap().is_empty());
        assert_eq!(select_campaigns(&pool, 1, true).await.unwrap().len(), 1);
        assert!(
            select_spots(&pool, 1, None, false)
                .await
                .unwrap()
                .is_empty()
        );
        assert!(
            !select_spot(&pool, 1, spot_id)
                .await
                .unwrap()
                .unwrap()
                .active
        );
    }

    #[tokio::test]
    async fn campaign_requires_an_active_advertiser_of_the_same_channel() {
        let pool = pool().await;
        let advertiser_id = insert_advertiser(&pool, 1, &advertiser("Padaria"))
            .await
            .unwrap();

        assert!(
            insert_campaign(&pool, 2, &campaign(advertiser_id, "Natal"))
                .await
                .unwrap()
                .is_none()
        );
        assert!(
            insert_campaign(&pool, 1, &campaign(9999, "Natal"))
                .await
                .unwrap()
                .is_none()
        );

        archive_advertiser(&pool, 1, advertiser_id).await.unwrap();

        assert!(
            insert_campaign(&pool, 1, &campaign(advertiser_id, "Natal"))
                .await
                .unwrap()
                .is_none()
        );
    }

    #[tokio::test]
    async fn campaign_keeps_its_scheduling_fields() {
        let pool = pool().await;
        let advertiser_id = insert_advertiser(&pool, 1, &advertiser("Padaria"))
            .await
            .unwrap();
        let id = insert_campaign(&pool, 1, &campaign(advertiser_id, "Natal"))
            .await
            .unwrap()
            .unwrap();
        let stored = select_campaign(&pool, 1, id).await.unwrap().unwrap();

        assert_eq!(stored.kind, CampaignKind::Campaign);
        assert_eq!(stored.position, BreakPosition::First);
        assert_eq!(stored.weight, 2);
        assert_eq!(stored.max_per_day, Some(3));
        assert_eq!(stored.start_date.as_deref(), Some("2026-10-01"));
    }

    #[tokio::test]
    async fn database_rejects_a_campaign_without_dates() {
        let pool = pool().await;
        let advertiser_id = insert_advertiser(&pool, 1, &advertiser("Padaria"))
            .await
            .unwrap();
        let mut invalid = campaign(advertiser_id, "No dates");
        invalid.start_date = None;
        invalid.end_date = None;

        assert!(insert_campaign(&pool, 1, &invalid).await.is_err());

        invalid.kind = CampaignKind::Standalone;

        assert!(insert_campaign(&pool, 1, &invalid).await.unwrap().is_some());
    }

    #[tokio::test]
    async fn spot_belongs_to_a_campaign_of_its_channel_and_files_are_unique() {
        let pool = pool().await;
        let advertiser_id = insert_advertiser(&pool, 1, &advertiser("Padaria"))
            .await
            .unwrap();
        let campaign_id = insert_campaign(&pool, 1, &campaign(advertiser_id, "Natal"))
            .await
            .unwrap()
            .unwrap();

        assert!(
            insert_spot(&pool, 2, campaign_id, "Spot", "a.mp4", "a.mp4", None)
                .await
                .unwrap()
                .is_none()
        );

        let spot_id = insert_spot(&pool, 1, campaign_id, "Spot", "a.mp4", "a.mp4", None)
            .await
            .unwrap()
            .unwrap();

        assert!(
            insert_spot(&pool, 1, campaign_id, "Again", "a.mp4", "a.mp4", None)
                .await
                .is_err()
        );

        archive_spot(&pool, 1, spot_id).await.unwrap();

        assert!(
            insert_spot(&pool, 1, campaign_id, "Again", "a.mp4", "a.mp4", None)
                .await
                .unwrap()
                .is_some()
        );
    }

    #[tokio::test]
    async fn spot_status_follows_processing() {
        let pool = pool().await;
        let advertiser_id = insert_advertiser(&pool, 1, &advertiser("Padaria"))
            .await
            .unwrap();
        let campaign_id = insert_campaign(&pool, 1, &campaign(advertiser_id, "Natal"))
            .await
            .unwrap()
            .unwrap();
        let id = insert_spot(&pool, 1, campaign_id, "Spot", "a.mp4", "a.mp4", None)
            .await
            .unwrap()
            .unwrap();

        assert_eq!(
            select_spot(&pool, 1, id).await.unwrap().unwrap().status,
            SpotStatus::Processing
        );

        assert!(
            update_spot_status(&pool, 1, id, SpotStatus::Ready, Some(30.0), "")
                .await
                .unwrap()
        );

        let ready = select_spot(&pool, 1, id).await.unwrap().unwrap();

        assert_eq!(ready.status, SpotStatus::Ready);
        assert_eq!(ready.duration_sec, Some(30.0));
        assert!(
            update_spot_status(&pool, 1, id, SpotStatus::Ready, None, "")
                .await
                .is_err(),
            "a ready spot needs its duration"
        );
        assert!(
            !update_spot_status(&pool, 2, id, SpotStatus::Failed, None, "x")
                .await
                .unwrap()
        );
    }

    #[tokio::test]
    async fn break_windows_are_ordered_filtered_and_unique() {
        let pool = pool().await;
        insert_break_window(&pool, 1, &window(2, "20:30:00"))
            .await
            .unwrap();
        let id = insert_break_window(&pool, 1, &window(2, "12:00:00"))
            .await
            .unwrap();
        insert_break_window(&pool, 1, &window(3, "12:00:00"))
            .await
            .unwrap();

        let tuesday = select_break_windows(&pool, 1, Some(2)).await.unwrap();

        assert_eq!(
            tuesday
                .iter()
                .map(|w| w.nominal_time.as_str())
                .collect::<Vec<_>>(),
            ["12:00:00", "20:30:00"]
        );
        assert_eq!(select_break_windows(&pool, 1, None).await.unwrap().len(), 3);
        assert!(
            insert_break_window(&pool, 1, &window(2, "12:00:00"))
                .await
                .is_err()
        );
        assert!(!delete_break_window(&pool, 2, id).await.unwrap());
        assert!(delete_break_window(&pool, 1, id).await.unwrap());
    }

    #[tokio::test]
    async fn database_rejects_invalid_break_windows() {
        let pool = pool().await;

        assert!(
            insert_break_window(&pool, 1, &window(9, "12:00:00"))
                .await
                .is_err()
        );
        assert!(
            insert_break_window(&pool, 1, &window(1, "noon"))
                .await
                .is_err()
        );

        let mut zero = window(1, "12:00:00");
        zero.duration_sec = 0;

        assert!(insert_break_window(&pool, 1, &zero).await.is_err());
    }
}
