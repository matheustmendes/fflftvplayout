//! The airing log: what the plan promised and what really went on the air.
//!
//! The playout never waits for the log. A clip is tracked by a spawned task
//! that owns every database write, so a slow or locked database delays the
//! log and never the air.

use std::time::{Duration, Instant};

use chrono::{DateTime, Duration as ChronoDuration, NaiveDate, SecondsFormat, TimeZone, Utc};
use chrono_tz::Tz;
use ff_engine::ClipResult;
use log::*;
use sqlx::sqlite::SqlitePool;
use tokio::{
    sync::oneshot,
    time::{MissedTickBehavior, interval},
};
use tokio_util::sync::CancellationToken;

use crate::{
    db::{
        handles,
        models::{AirResult, PlannedAiring},
    },
    player::controller::ChannelManager,
    utils::{
        break_planner::{DayPlan, ItemKind},
        config::PlayoutConfig,
        errors::ProcessError,
    },
};

/// How long the log is kept. A campaign keeps its whole history until this
/// time has passed since the campaign ended.
pub const RETENTION_DAYS: i64 = 60;

/// A spot that ran for at least this share of its length counts as played.
const PLAYED_SHARE: f64 = 0.9;

/// A planned spot is only declared missed this long after its start, so a spot
/// that is just starting is never settled by mistake.
const OVERDUE_GRACE_SECONDS: i64 = 120;

const MAINTENANCE_INTERVAL: Duration = Duration::from_secs(300);
const LIVE_POLL_INTERVAL: Duration = Duration::from_secs(1);

pub fn format_timestamp(time: DateTime<Utc>) -> String {
    time.to_rfc3339_opts(SecondsFormat::Millis, true)
}

/// The moment a clip planned at `begin_sec` (seconds since midnight of the
/// playlist date, in the channel timezone) starts.
pub fn planned_start_time(date: NaiveDate, timezone: Tz, begin_sec: f64) -> Option<DateTime<Utc>> {
    let midnight = date.and_hms_opt(0, 0, 0)?;
    let local = midnight + ChronoDuration::milliseconds((begin_sec * 1000.0).round() as i64);

    timezone
        .from_local_datetime(&local)
        .earliest()
        .map(|time| time.with_timezone(&Utc))
}

/// Store what the plan of `date` promises, replacing the promises that have
/// not come due yet. Returns the number of planned spots.
pub async fn record_plan(
    pool: &SqlitePool,
    channel_id: i32,
    config: &PlayoutConfig,
    plan: &DayPlan,
    date: NaiveDate,
    now: DateTime<Utc>,
) -> Result<usize, ProcessError> {
    let timezone = config.channel.timezone.unwrap_or(Tz::UTC);
    let day_start_sec = config.playlist.start_sec.unwrap_or(0.0);
    let mut rows = Vec::new();

    for item in plan.items.iter().filter(|item| item.kind == ItemKind::Spot) {
        let Some(identity) = handles::select_spot_identity(pool, channel_id, &item.source).await?
        else {
            continue;
        };
        let begin_sec = day_start_sec + item.start_sec;

        rows.push(PlannedAiring {
            spot_id: identity.spot_id,
            campaign_id: identity.campaign_id,
            advertiser_id: identity.advertiser_id,
            advertiser_name: identity.advertiser_name,
            campaign_name: identity.campaign_name,
            spot_title: identity.spot_title,
            file_path: item.source.clone(),
            break_window_id: item.break_window_id,
            position_in_break: item.position_in_break.map(|position| position as i32),
            planned_begin_sec: begin_sec,
            planned_start: planned_start_time(date, timezone, begin_sec).map(format_timestamp),
            planned_duration_sec: item.airtime(),
        });
    }

    handles::replace_planned_airings(
        pool,
        channel_id,
        &date.format("%Y-%m-%d").to_string(),
        &format_timestamp(now),
        &rows,
    )
    .await?;

    Ok(rows.len())
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClipOutcome {
    pub result: AirResult,
    pub reason: String,
}

impl ClipOutcome {
    fn new(result: AirResult, reason: impl Into<String>) -> Self {
        Self {
            result,
            reason: reason.into(),
        }
    }
}

/// Turn the end of a clip into a log result. `alive` is false while the
/// channel is shutting down, which makes a skip an interruption.
pub fn classify_clip(
    result: &ClipResult,
    alive: bool,
    aired_sec: f64,
    expected_sec: f64,
) -> ClipOutcome {
    match result {
        ClipResult::Played if aired_sec >= expected_sec * PLAYED_SHARE => {
            ClipOutcome::new(AirResult::Played, "")
        }
        ClipResult::Played => ClipOutcome::new(
            AirResult::Partial,
            format!("ended after {aired_sec:.1} of {expected_sec:.1} seconds"),
        ),
        ClipResult::Skipped if alive => {
            ClipOutcome::new(AirResult::Skipped, "skipped by an operator")
        }
        ClipResult::Skipped | ClipResult::Stopped => {
            ClipOutcome::new(AirResult::Interrupted, "the playout was stopped")
        }
        ClipResult::LiveEnded => ClipOutcome::new(AirResult::Interrupted, "a live input took over"),
        ClipResult::Fallback { reason } => ClipOutcome::new(AirResult::Fallback, reason.clone()),
    }
}

struct ClipEnd {
    ended: DateTime<Utc>,
    aired_sec: f64,
    outcome: ClipOutcome,
}

/// A clip on the air. Dropping it without calling [`ClipTracker::finish`]
/// closes the row as interrupted.
pub struct ClipTracker {
    started: Instant,
    expected_sec: f64,
    sender: oneshot::Sender<ClipEnd>,
}

/// Start tracking a clip. Returns at once: the row is written by a task.
pub fn start_tracking(
    pool: SqlitePool,
    channel_id: i32,
    air_date: String,
    file_path: String,
    begin_sec: Option<f64>,
    expected_sec: f64,
) -> ClipTracker {
    let started_at = Utc::now();
    let (sender, receiver) = oneshot::channel();

    tokio::spawn(async move {
        record_clip(
            &pool, channel_id, &air_date, &file_path, begin_sec, started_at, receiver,
        )
        .await;
    });

    ClipTracker {
        started: Instant::now(),
        expected_sec,
        sender,
    }
}

impl ClipTracker {
    pub fn finish(self, result: &ClipResult, alive: bool) {
        let aired_sec = self.started.elapsed().as_secs_f64();
        let outcome = classify_clip(result, alive, aired_sec, self.expected_sec);

        self.send(aired_sec, outcome);
    }

    pub fn fail(self, error: &str) {
        let aired_sec = self.started.elapsed().as_secs_f64();

        self.send(aired_sec, ClipOutcome::new(AirResult::Failed, error));
    }

    fn send(self, aired_sec: f64, outcome: ClipOutcome) {
        // A closed receiver means the writer task is gone; nothing to do.
        let _ = self.sender.send(ClipEnd {
            ended: Utc::now(),
            aired_sec,
            outcome,
        });
    }
}

async fn record_clip(
    pool: &SqlitePool,
    channel_id: i32,
    air_date: &str,
    file_path: &str,
    begin_sec: Option<f64>,
    started_at: DateTime<Utc>,
    receiver: oneshot::Receiver<ClipEnd>,
) {
    let id = match handles::begin_airing(
        pool,
        channel_id,
        air_date,
        file_path,
        begin_sec,
        &format_timestamp(started_at),
    )
    .await
    {
        Ok(Some(id)) => id,
        // Not a spot: programs and filler are not logged.
        Ok(None) => return,
        Err(error) => {
            error!("Cannot log the start of {file_path}: {error}");

            return;
        }
    };

    let end = receiver.await.unwrap_or_else(|_| {
        let ended = Utc::now();

        ClipEnd {
            ended,
            aired_sec: (ended - started_at).num_milliseconds() as f64 / 1000.0,
            outcome: ClipOutcome::new(AirResult::Interrupted, "the playout stopped"),
        }
    });

    if let Err(error) = handles::finish_airing(
        pool,
        id,
        &format_timestamp(end.ended),
        end.aired_sec,
        end.outcome.result,
        &end.outcome.reason,
    )
    .await
    {
        error!("Cannot log the end of {file_path}: {error}");
    }
}

#[derive(Debug, Clone, Copy)]
pub struct OpenLiveWindow {
    id: i32,
    started: DateTime<Utc>,
}

/// Follow the live state of a channel: a window opens when a live input takes
/// the air and closes when it leaves.
pub async fn live_step(
    pool: &SqlitePool,
    channel_id: i32,
    air_date: &str,
    live: bool,
    open: &mut Option<OpenLiveWindow>,
    now: DateTime<Utc>,
) {
    match (live, *open) {
        (true, None) => {
            match handles::begin_live(pool, channel_id, air_date, &format_timestamp(now)).await {
                Ok(id) => *open = Some(OpenLiveWindow { id, started: now }),
                Err(error) => error!("Cannot log the start of the live broadcast: {error}"),
            }
        }
        (false, Some(window)) => {
            let seconds = (now - window.started).num_milliseconds() as f64 / 1000.0;

            if let Err(error) =
                handles::finish_live(pool, window.id, &format_timestamp(now), seconds).await
            {
                error!("Cannot log the end of the live broadcast: {error}");
            }

            *open = None;
        }
        _ => {}
    }
}

/// Log the live windows of a channel until `token` is cancelled; a window
/// that is still open then is closed.
pub async fn watch_live(manager: ChannelManager, token: CancellationToken) {
    let mut ticker = interval(LIVE_POLL_INTERVAL);
    let mut open = None;

    ticker.set_missed_tick_behavior(MissedTickBehavior::Delay);

    loop {
        tokio::select! {
            _ = token.cancelled() => break,
            _ = ticker.tick() => {}
        }

        let live = manager.playback_control.lock().await.live_status().0;
        let air_date = current_air_date(&manager).await;

        live_step(
            &manager.db_pool,
            manager.id,
            &air_date,
            live,
            &mut open,
            Utc::now(),
        )
        .await;
    }

    live_step(
        &manager.db_pool,
        manager.id,
        "",
        false,
        &mut open,
        Utc::now(),
    )
    .await;
}

/// The date of the playlist on the air, or today's when none is loaded.
pub async fn current_air_date(manager: &ChannelManager) -> String {
    let date = manager.current_date.lock().await.clone();

    if !date.is_empty() {
        return date;
    }

    let timezone = manager.channel.lock().await.timezone.unwrap_or(Tz::UTC);

    Utc::now()
        .with_timezone(&timezone)
        .format("%Y-%m-%d")
        .to_string()
}

/// Settle the promised spots that are long past their start: they either
/// never started or lost their time to a live broadcast.
pub async fn reconcile_overdue(
    pool: &SqlitePool,
    now: DateTime<Utc>,
) -> Result<usize, ProcessError> {
    let due = now - ChronoDuration::seconds(OVERDUE_GRACE_SECONDS);
    let rows = handles::select_overdue_planned(pool, &format_timestamp(due)).await?;
    let mut settled = 0;

    for row in rows {
        let Some(start_text) = row.planned_start.as_deref() else {
            continue;
        };
        let Ok(start) = DateTime::parse_from_rfc3339(start_text) else {
            continue;
        };
        let end = start
            + ChronoDuration::milliseconds(
                (row.planned_duration_sec.unwrap_or(0.0) * 1000.0) as i64,
            );
        let live = handles::select_live_windows(
            pool,
            row.channel_id,
            start_text,
            &format_timestamp(end.with_timezone(&Utc)),
        )
        .await?;
        let (result, reason) = if live.is_empty() {
            (AirResult::NotAired, "the spot never started")
        } else {
            (AirResult::LiveMissed, "a live broadcast was on the air")
        };

        if handles::settle_pending(pool, row.id, result, reason).await? {
            settled += 1;
        }
    }

    Ok(settled)
}

/// Delete what is older than the retention time.
pub async fn purge_old(pool: &SqlitePool, now: DateTime<Utc>) -> Result<u64, ProcessError> {
    let cutoff = now - ChronoDuration::days(RETENTION_DAYS);

    handles::purge_airlog(
        pool,
        &format_timestamp(cutoff),
        &cutoff.format("%Y-%m-%d").to_string(),
    )
    .await
}

/// Close the rows a stopped process left open. Run once at startup.
pub async fn close_open_rows(pool: &SqlitePool, now: DateTime<Utc>) {
    match handles::close_open_airings(pool, &format_timestamp(now)).await {
        Ok(0) => {}
        Ok(count) => warn!("{count} airing log row(s) were open when the playout stopped"),
        Err(error) => error!("Cannot close the open airing log rows: {error}"),
    }
}

/// Periodic upkeep of the log: settle missed spots and drop old rows.
pub async fn run_maintenance(pool: SqlitePool, shutdown: CancellationToken) {
    let mut ticker = interval(MAINTENANCE_INTERVAL);

    ticker.set_missed_tick_behavior(MissedTickBehavior::Delay);

    loop {
        tokio::select! {
            _ = shutdown.cancelled() => break,
            _ = ticker.tick() => {}
        }

        let now = Utc::now();

        if let Err(error) = reconcile_overdue(&pool, now).await {
            error!("Cannot settle the planned spots: {error}");
        }

        if let Err(error) = purge_old(&pool, now).await {
            error!("Cannot purge the airing log: {error}");
        }
    }
}

#[cfg(test)]
mod tests {
    use sqlx::sqlite::SqlitePoolOptions;

    use super::*;
    use crate::db::{
        handles::{
            AiringFilter, complete_spot, db_migrate, insert_advertiser, insert_campaign,
            insert_spot, select_airings,
        },
        models::{Advertiser, AirKind, BreakPosition, Campaign, CampaignKind},
    };

    const PATH: &str = "commercials/spot-1.mp4";

    fn utc(text: &str) -> DateTime<Utc> {
        DateTime::parse_from_rfc3339(text)
            .unwrap()
            .with_timezone(&Utc)
    }

    async fn pool_with_spot() -> SqlitePool {
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .unwrap();

        db_migrate(&pool).await.unwrap();

        let advertiser_id = insert_advertiser(
            &pool,
            1,
            &Advertiser {
                id: 0,
                channel_id: 1,
                name: "Padaria".into(),
                contact_name: String::new(),
                email: String::new(),
                notes: String::new(),
                active: true,
            },
        )
        .await
        .unwrap();
        let campaign_id = insert_campaign(
            &pool,
            1,
            &Campaign {
                id: 0,
                channel_id: 1,
                advertiser_id,
                name: "Natal".into(),
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
        let spot_id = insert_spot(&pool, 1, campaign_id, "Natal", "up/a.mxf", "a.mxf", None)
            .await
            .unwrap()
            .unwrap();

        complete_spot(&pool, 1, spot_id, PATH, 30.0).await.unwrap();

        pool
    }

    async fn rows(pool: &SqlitePool) -> Vec<crate::db::models::Airing> {
        select_airings(
            pool,
            1,
            &AiringFilter {
                limit: 100,
                ..AiringFilter::default()
            },
        )
        .await
        .unwrap()
    }

    /// Give the writer task time to finish; the tracker never waits for it.
    async fn settle() {
        tokio::time::sleep(Duration::from_millis(300)).await;
    }

    #[test]
    fn timestamps_have_a_fixed_utc_format() {
        assert_eq!(
            format_timestamp(utc("2026-12-10T09:00:00.5+00:00")),
            "2026-12-10T09:00:00.500Z"
        );
    }

    #[test]
    fn a_planned_start_follows_the_channel_timezone() {
        let date = NaiveDate::from_ymd_opt(2026, 12, 10).unwrap();
        let sao_paulo: Tz = "America/Sao_Paulo".parse().unwrap();

        // Sao Paulo is UTC-3 all year since 2019.
        assert_eq!(
            planned_start_time(date, sao_paulo, 6.0 * 3600.0),
            Some(utc("2026-12-10T09:00:00+00:00"))
        );
        // A day that starts in the evening runs past 24 hours.
        assert_eq!(
            planned_start_time(date, Tz::UTC, 90_000.0),
            Some(utc("2026-12-11T01:00:00+00:00"))
        );
    }

    #[test]
    fn clip_results_are_classified() {
        let fallback = ClipResult::Fallback {
            reason: "decoder failed".into(),
        };
        let cases = [
            (ClipResult::Played, true, 30.0, AirResult::Played),
            (ClipResult::Played, true, 27.0, AirResult::Played),
            (ClipResult::Played, true, 10.0, AirResult::Partial),
            (ClipResult::Skipped, true, 5.0, AirResult::Skipped),
            (ClipResult::Skipped, false, 5.0, AirResult::Interrupted),
            (ClipResult::LiveEnded, true, 5.0, AirResult::Interrupted),
            (ClipResult::Stopped, true, 5.0, AirResult::Interrupted),
            (fallback, true, 1.0, AirResult::Fallback),
        ];

        for (clip, alive, aired, expected) in cases {
            assert_eq!(
                classify_clip(&clip, alive, aired, 30.0).result,
                expected,
                "{clip:?} alive={alive} aired={aired}"
            );
        }
    }

    #[tokio::test]
    async fn a_tracked_spot_is_logged_from_start_to_end() {
        let pool = pool_with_spot().await;
        let tracker = start_tracking(
            pool.clone(),
            1,
            "2026-12-10".into(),
            PATH.into(),
            Some(100.0),
            0.2,
        );

        settle().await;
        assert_eq!(rows(&pool).await[0].result, AirResult::Airing);

        tokio::time::sleep(Duration::from_millis(250)).await;
        tracker.finish(&ClipResult::Played, true);
        settle().await;

        let row = &rows(&pool).await[0];

        assert_eq!(row.result, AirResult::Played);
        assert!(row.actual_end.is_some());
        assert!(row.actual_duration_sec.unwrap() >= 0.25);
        assert_eq!(row.advertiser_name, "Padaria");
    }

    #[tokio::test]
    async fn files_that_are_not_spots_are_not_logged() {
        let pool = pool_with_spot().await;
        let tracker = start_tracking(
            pool.clone(),
            1,
            "2026-12-10".into(),
            "programs/news.mp4".into(),
            None,
            60.0,
        );

        tracker.finish(&ClipResult::Played, true);
        settle().await;

        assert!(rows(&pool).await.is_empty());
    }

    #[tokio::test]
    async fn a_dropped_tracker_closes_the_row_as_interrupted() {
        let pool = pool_with_spot().await;
        let tracker = start_tracking(
            pool.clone(),
            1,
            "2026-12-10".into(),
            PATH.into(),
            None,
            30.0,
        );

        settle().await;
        drop(tracker);
        settle().await;

        assert_eq!(rows(&pool).await[0].result, AirResult::Interrupted);
    }

    #[tokio::test]
    async fn a_failed_clip_is_logged_with_its_reason() {
        let pool = pool_with_spot().await;
        let tracker = start_tracking(
            pool.clone(),
            1,
            "2026-12-10".into(),
            PATH.into(),
            None,
            30.0,
        );

        settle().await;
        tracker.fail("engine crashed");
        settle().await;

        let row = &rows(&pool).await[0];

        assert_eq!(row.result, AirResult::Failed);
        assert_eq!(row.reason, "engine crashed");
    }

    /// The air must not wait for the log: with the only connection held by
    /// somebody else, starting and finishing a clip still returns at once.
    #[tokio::test]
    async fn tracking_never_blocks_on_a_busy_database() {
        let pool = pool_with_spot().await;
        let held = pool.acquire().await.unwrap();
        let started = Instant::now();
        let tracker = start_tracking(
            pool.clone(),
            1,
            "2026-12-10".into(),
            PATH.into(),
            None,
            30.0,
        );

        tracker.finish(&ClipResult::Played, true);

        assert!(started.elapsed() < Duration::from_millis(100));

        drop(held);
        settle().await;

        assert_eq!(
            rows(&pool).await.len(),
            1,
            "the row is written once the database is free"
        );
    }

    #[tokio::test]
    async fn a_live_window_opens_and_closes_with_the_live_state() {
        let pool = pool_with_spot().await;
        let mut open = None;

        live_step(
            &pool,
            1,
            "2026-12-10",
            false,
            &mut open,
            utc("2026-12-10T10:00:00+00:00"),
        )
        .await;
        assert!(open.is_none() && rows(&pool).await.is_empty());

        live_step(
            &pool,
            1,
            "2026-12-10",
            true,
            &mut open,
            utc("2026-12-10T10:00:01+00:00"),
        )
        .await;
        live_step(
            &pool,
            1,
            "2026-12-10",
            true,
            &mut open,
            utc("2026-12-10T10:00:02+00:00"),
        )
        .await;
        assert_eq!(
            rows(&pool).await.len(),
            1,
            "one window while the live state holds"
        );

        live_step(
            &pool,
            1,
            "2026-12-10",
            false,
            &mut open,
            utc("2026-12-10T11:00:01+00:00"),
        )
        .await;

        let row = &rows(&pool).await[0];

        assert!(open.is_none());
        assert_eq!(row.kind, AirKind::Live);
        assert_eq!(row.result, AirResult::Played);
        assert_eq!(row.actual_duration_sec, Some(3600.0));
    }

    async fn plan_spot(pool: &SqlitePool, start: &str) {
        let identity = handles::select_spot_identity(pool, 1, PATH)
            .await
            .unwrap()
            .unwrap();

        handles::replace_planned_airings(
            pool,
            1,
            "2026-12-10",
            "2026-12-01T00:00:00.000Z",
            &[PlannedAiring {
                spot_id: identity.spot_id,
                campaign_id: identity.campaign_id,
                advertiser_id: identity.advertiser_id,
                advertiser_name: identity.advertiser_name,
                campaign_name: identity.campaign_name,
                spot_title: identity.spot_title,
                file_path: PATH.into(),
                break_window_id: None,
                position_in_break: Some(0),
                planned_begin_sec: 32_400.0,
                planned_start: Some(start.into()),
                planned_duration_sec: 30.0,
            }],
        )
        .await
        .unwrap();
    }

    #[tokio::test]
    async fn a_spot_that_never_started_is_settled_as_not_aired() {
        let pool = pool_with_spot().await;

        plan_spot(&pool, "2026-12-10T09:00:00.000Z").await;

        assert_eq!(
            reconcile_overdue(&pool, utc("2026-12-10T09:01:00+00:00"))
                .await
                .unwrap(),
            0,
            "inside the grace time nothing is settled"
        );
        assert_eq!(
            reconcile_overdue(&pool, utc("2026-12-10T09:05:00+00:00"))
                .await
                .unwrap(),
            1
        );

        let row = &rows(&pool).await[0];

        assert_eq!(row.result, AirResult::NotAired);
    }

    #[tokio::test]
    async fn a_spot_covered_by_a_live_broadcast_is_settled_as_live_missed() {
        let pool = pool_with_spot().await;
        let id = handles::begin_live(&pool, 1, "2026-12-10", "2026-12-10T08:30:00.000Z")
            .await
            .unwrap();

        handles::finish_live(&pool, id, "2026-12-10T10:00:00.000Z", 5400.0)
            .await
            .unwrap();
        plan_spot(&pool, "2026-12-10T09:00:00.000Z").await;

        reconcile_overdue(&pool, utc("2026-12-10T09:05:00+00:00"))
            .await
            .unwrap();

        let spot = rows(&pool)
            .await
            .into_iter()
            .find(|row| row.kind == AirKind::Spot)
            .unwrap();

        assert_eq!(spot.result, AirResult::LiveMissed);
    }

    #[tokio::test]
    async fn rows_older_than_the_retention_time_are_purged() {
        let pool = pool_with_spot().await;

        plan_spot(&pool, "2026-09-01T09:00:00.000Z").await;
        handles::settle_pending(&pool, rows(&pool).await[0].id, AirResult::NotAired, "")
            .await
            .unwrap();

        assert_eq!(
            purge_old(&pool, utc("2026-09-30T00:00:00+00:00"))
                .await
                .unwrap(),
            0
        );
        assert_eq!(
            purge_old(&pool, utc("2026-11-01T00:00:00+00:00"))
                .await
                .unwrap(),
            1
        );
    }

    #[tokio::test]
    async fn open_rows_are_closed_at_startup() {
        let pool = pool_with_spot().await;

        handles::begin_airing(
            &pool,
            1,
            "2026-12-10",
            PATH,
            None,
            "2026-12-10T09:00:00.000Z",
        )
        .await
        .unwrap();
        close_open_rows(&pool, utc("2026-12-10T09:00:30+00:00")).await;

        assert_eq!(rows(&pool).await[0].result, AirResult::Interrupted);
    }
}
