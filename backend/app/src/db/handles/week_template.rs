use sqlx::sqlite::SqlitePool;

use crate::{db::models::WeekTemplateItem, utils::errors::ProcessError};

/// The programs of the weekly template in air order, for one weekday or for
/// the whole week.
pub async fn select_week_template(
    pool: &SqlitePool,
    channel_id: i32,
    weekday: Option<i32>,
) -> Result<Vec<WeekTemplateItem>, ProcessError> {
    const QUERY: &str = "SELECT * FROM week_template_item
        WHERE channel_id = $1 AND ($2 IS NULL OR weekday = $2)
        ORDER BY weekday, position";

    let result = sqlx::query_as(QUERY)
        .bind(channel_id)
        .bind(weekday)
        .fetch_all(pool)
        .await?;

    Ok(result)
}

/// Replace the programs of one weekday. The positions follow the order of
/// `items`; either the whole day is saved or nothing changes.
pub async fn replace_week_template_day(
    pool: &SqlitePool,
    channel_id: i32,
    weekday: i32,
    items: &[WeekTemplateItem],
) -> Result<(), ProcessError> {
    const INSERT: &str = "INSERT INTO week_template_item (
            channel_id, weekday, position, source, title, in_sec, out_sec, duration_sec
        )
        VALUES ($1, $2, $3, $4, $5, $6, $7, $8)";

    let mut transaction = pool.begin().await?;

    sqlx::query("DELETE FROM week_template_item WHERE channel_id = $1 AND weekday = $2")
        .bind(channel_id)
        .bind(weekday)
        .execute(&mut *transaction)
        .await?;

    for (position, item) in items.iter().enumerate() {
        sqlx::query(INSERT)
            .bind(channel_id)
            .bind(weekday)
            .bind(position as i32)
            .bind(&item.source)
            .bind(item.title.trim())
            .bind(item.in_sec)
            .bind(item.out_sec)
            .bind(item.duration_sec)
            .execute(&mut *transaction)
            .await?;
    }

    transaction.commit().await?;

    Ok(())
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
        sqlx::query("INSERT INTO channels (id, name, preview_url) VALUES (2, 'Other', '')")
            .execute(&pool)
            .await
            .unwrap();

        pool
    }

    fn program(source: &str, out_sec: f64) -> WeekTemplateItem {
        WeekTemplateItem {
            id: 0,
            channel_id: 0,
            weekday: 0,
            position: 0,
            source: source.to_string(),
            title: format!("  {source}  "),
            in_sec: 0.0,
            out_sec,
            duration_sec: out_sec,
        }
    }

    fn sources(items: &[WeekTemplateItem]) -> Vec<&str> {
        items.iter().map(|item| item.source.as_str()).collect()
    }

    #[tokio::test]
    async fn a_day_keeps_the_order_it_was_saved_in() {
        let pool = pool().await;

        replace_week_template_day(
            &pool,
            1,
            3,
            &[
                program("c.mp4", 30.0),
                program("a.mp4", 10.0),
                program("b.mp4", 20.0),
            ],
        )
        .await
        .unwrap();

        let day = select_week_template(&pool, 1, Some(3)).await.unwrap();

        assert_eq!(sources(&day), ["c.mp4", "a.mp4", "b.mp4"]);
        assert_eq!(
            day.iter().map(|item| item.position).collect::<Vec<_>>(),
            [0, 1, 2]
        );
        assert!(
            day.iter()
                .all(|item| item.weekday == 3 && item.channel_id == 1)
        );
        assert_eq!(day[0].title, "c.mp4", "titles are trimmed");
    }

    #[tokio::test]
    async fn saving_a_day_replaces_it_and_leaves_other_days_alone() {
        let pool = pool().await;

        replace_week_template_day(&pool, 1, 1, &[program("mon.mp4", 10.0)])
            .await
            .unwrap();
        replace_week_template_day(&pool, 1, 2, &[program("tue.mp4", 10.0)])
            .await
            .unwrap();
        replace_week_template_day(
            &pool,
            1,
            1,
            &[program("new.mp4", 5.0), program("new2.mp4", 6.0)],
        )
        .await
        .unwrap();

        assert_eq!(
            sources(&select_week_template(&pool, 1, Some(1)).await.unwrap()),
            ["new.mp4", "new2.mp4"]
        );
        assert_eq!(
            sources(&select_week_template(&pool, 1, Some(2)).await.unwrap()),
            ["tue.mp4"]
        );
        assert_eq!(select_week_template(&pool, 1, None).await.unwrap().len(), 3);

        replace_week_template_day(&pool, 1, 1, &[]).await.unwrap();

        assert!(
            select_week_template(&pool, 1, Some(1))
                .await
                .unwrap()
                .is_empty()
        );
    }

    #[tokio::test]
    async fn the_template_is_scoped_to_the_channel() {
        let pool = pool().await;

        replace_week_template_day(&pool, 1, 1, &[program("a.mp4", 10.0)])
            .await
            .unwrap();
        replace_week_template_day(&pool, 2, 1, &[program("b.mp4", 10.0)])
            .await
            .unwrap();

        assert_eq!(
            sources(&select_week_template(&pool, 1, None).await.unwrap()),
            ["a.mp4"]
        );
        assert_eq!(
            sources(&select_week_template(&pool, 2, None).await.unwrap()),
            ["b.mp4"]
        );
    }

    #[tokio::test]
    async fn a_failed_save_keeps_the_previous_day() {
        let pool = pool().await;

        replace_week_template_day(&pool, 1, 1, &[program("keep.mp4", 10.0)])
            .await
            .unwrap();

        let broken = WeekTemplateItem {
            out_sec: -5.0,
            ..program("bad.mp4", 10.0)
        };

        assert!(
            replace_week_template_day(&pool, 1, 1, &[program("other.mp4", 5.0), broken])
                .await
                .is_err()
        );
        assert_eq!(
            sources(&select_week_template(&pool, 1, Some(1)).await.unwrap()),
            ["keep.mp4"],
            "the transaction rolls back"
        );
    }
}
