use sqlx::sqlite::SqlitePool;

use crate::{
    db::models::{AutoPlanDay, AutoPlanSettings, AutoPlanState},
    utils::errors::ProcessError,
};

/// The settings of a channel, or the defaults when none were saved yet.
pub async fn select_auto_plan_settings(
    pool: &SqlitePool,
    channel_id: i32,
) -> Result<AutoPlanSettings, ProcessError> {
    const QUERY: &str = "SELECT * FROM auto_plan_settings WHERE channel_id = $1";

    let stored: Option<AutoPlanSettings> = sqlx::query_as(QUERY)
        .bind(channel_id)
        .fetch_optional(pool)
        .await?;

    Ok(stored.unwrap_or(AutoPlanSettings {
        channel_id,
        ..AutoPlanSettings::default()
    }))
}

pub async fn upsert_auto_plan_settings(
    pool: &SqlitePool,
    channel_id: i32,
    settings: &AutoPlanSettings,
) -> Result<(), ProcessError> {
    const QUERY: &str = "INSERT INTO auto_plan_settings (channel_id, enabled, horizon_days)
        VALUES ($1, $2, $3)
        ON CONFLICT (channel_id) DO UPDATE SET
            enabled = excluded.enabled,
            horizon_days = excluded.horizon_days";

    sqlx::query(QUERY)
        .bind(channel_id)
        .bind(settings.enabled)
        .bind(settings.horizon_days)
        .execute(pool)
        .await?;

    Ok(())
}

/// What the planner did with the days of a period (`YYYY-MM-DD`, inclusive).
pub async fn select_auto_plan_days(
    pool: &SqlitePool,
    channel_id: i32,
    from: &str,
    to: &str,
) -> Result<Vec<AutoPlanDay>, ProcessError> {
    const QUERY: &str = "SELECT * FROM auto_plan_day
        WHERE channel_id = $1 AND plan_date >= $2 AND plan_date <= $3
        ORDER BY plan_date";

    let result = sqlx::query_as(QUERY)
        .bind(channel_id)
        .bind(from)
        .bind(to)
        .fetch_all(pool)
        .await?;

    Ok(result)
}

pub async fn upsert_auto_plan_day(
    pool: &SqlitePool,
    channel_id: i32,
    plan_date: &str,
    state: AutoPlanState,
    message: &str,
    fingerprint: &str,
) -> Result<(), ProcessError> {
    const QUERY: &str =
        "INSERT INTO auto_plan_day (channel_id, plan_date, state, message, fingerprint)
        VALUES ($1, $2, $3, $4, $5)
        ON CONFLICT (channel_id, plan_date) DO UPDATE SET
            state = excluded.state,
            message = excluded.message,
            fingerprint = excluded.fingerprint,
            updated_at = CURRENT_TIMESTAMP";

    sqlx::query(QUERY)
        .bind(channel_id)
        .bind(plan_date)
        .bind(state)
        .bind(message)
        .bind(fingerprint)
        .execute(pool)
        .await?;

    Ok(())
}

/// Forget the days before `date` (`YYYY-MM-DD`).
pub async fn delete_auto_plan_days_before(
    pool: &SqlitePool,
    date: &str,
) -> Result<u64, ProcessError> {
    let done = sqlx::query("DELETE FROM auto_plan_day WHERE plan_date < $1")
        .bind(date)
        .execute(pool)
        .await?;

    Ok(done.rows_affected())
}

#[cfg(test)]
mod tests {
    use sqlx::sqlite::SqlitePoolOptions;

    use super::*;
    use crate::db::handles::db_migrate;

    async fn pool() -> SqlitePool {
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .unwrap();

        db_migrate(&pool).await.unwrap();

        pool
    }

    #[tokio::test]
    async fn settings_default_to_three_days_and_are_saved_per_channel() {
        let pool = pool().await;
        let defaults = select_auto_plan_settings(&pool, 1).await.unwrap();

        assert!(defaults.enabled);
        assert_eq!(defaults.horizon_days, 3);

        upsert_auto_plan_settings(
            &pool,
            1,
            &AutoPlanSettings {
                channel_id: 1,
                enabled: false,
                horizon_days: 7,
            },
        )
        .await
        .unwrap();

        let saved = select_auto_plan_settings(&pool, 1).await.unwrap();

        assert!(!saved.enabled);
        assert_eq!(saved.horizon_days, 7);
    }

    #[tokio::test]
    async fn days_are_kept_updated_and_pruned() {
        let pool = pool().await;

        for (date, state) in [
            ("2026-10-01", AutoPlanState::Created),
            ("2026-10-15", AutoPlanState::Created),
            ("2026-10-16", AutoPlanState::Failed),
        ] {
            upsert_auto_plan_day(&pool, 1, date, state, "", "fp")
                .await
                .unwrap();
        }

        upsert_auto_plan_day(
            &pool,
            1,
            "2026-10-15",
            AutoPlanState::Updated,
            "changed",
            "fp2",
        )
        .await
        .unwrap();

        let days = select_auto_plan_days(&pool, 1, "2026-10-10", "2026-10-31")
            .await
            .unwrap();

        assert_eq!(days.len(), 2);
        assert_eq!(days[0].state, AutoPlanState::Updated);
        assert_eq!(days[0].fingerprint, "fp2");
        assert_eq!(days[1].state, AutoPlanState::Failed);
        assert!(
            select_auto_plan_days(&pool, 2, "2026-10-01", "2026-10-31")
                .await
                .unwrap()
                .is_empty()
        );
        assert_eq!(
            delete_auto_plan_days_before(&pool, "2026-10-10")
                .await
                .unwrap(),
            1
        );
    }

    #[test]
    fn the_horizon_is_validated() {
        let mut settings = AutoPlanSettings::default();

        assert!(settings.validate().is_ok());

        for days in [0, 15, -1] {
            settings.horizon_days = days;

            assert!(settings.validate().is_err(), "{days}");
        }
    }
}
