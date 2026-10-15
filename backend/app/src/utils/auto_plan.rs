//! Keep the upcoming days planned without anybody pressing a button.
//!
//! For each day from the one on the air to `horizon_days` ahead, the planner
//! writes the playlist when it is missing, and rewrites the one it wrote
//! itself when the plan changed (a spot uploaded in the afternoon still enters
//! tomorrow). It never touches the day on the air, nor a playlist somebody
//! else wrote or edited: an operator's work always wins.

use std::{collections::HashSet, sync::Arc, time::Duration};

use chrono::{DateTime, Duration as ChronoDuration, NaiveDate, Utc};
use chrono_tz::Tz;
use log::*;
use sqlx::sqlite::SqlitePool;
use tokio::{
    sync::RwLock,
    time::{MissedTickBehavior, interval, sleep},
};
use tokio_util::sync::CancellationToken;

use crate::{
    db::{handles, models::AutoPlanState},
    player::{controller::ChannelController, utils::JsonPlaylist},
    utils::{
        config::{PlayoutConfig, ProcessMode},
        day_plan::{apply_plan, plan_for_date, plan_to_playlist},
        errors::ServiceError,
        logging::Target,
        playlist::read_playlist,
    },
};

/// How often the planner looks at the upcoming days.
const CHECK_INTERVAL: Duration = Duration::from_secs(600);

/// The channels finish starting before the first look.
const FIRST_CHECK_DELAY: Duration = Duration::from_secs(30);

/// The day on the air and this many days after it must have a playlist.
const MISSING_PLAYLIST_DAYS_AHEAD: i64 = 1;

/// Records of the planner are kept this many days.
const HISTORY_DAYS: i64 = 30;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DayOutcome {
    pub date: NaiveDate,
    pub state: AutoPlanState,
    pub message: String,
}

/// The date of the playlist on the air at `now`: a day that starts at 06:00
/// is still the previous date at 03:00.
pub fn active_playlist_date(now: DateTime<Utc>, timezone: Tz, day_start_sec: f64) -> NaiveDate {
    let local = now.with_timezone(&timezone).naive_local();

    (local - ChronoDuration::milliseconds((day_start_sec * 1000.0) as i64)).date()
}

/// A fingerprint of what a playlist plays, which ignores formatting and
/// everything a person could change without changing the content.
pub fn fingerprint(playlist: &JsonPlaylist) -> String {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;

    for item in &playlist.program {
        let text = format!(
            "{}|{:.3}|{:.3}|{}",
            item.source, item.seek, item.out, item.ad
        );

        for byte in text.bytes().chain(*b"\n") {
            hash = (hash ^ u64::from(byte)).wrapping_mul(0x0000_0100_0000_01b3);
        }
    }

    format!("{hash:016x}")
}

pub async fn existing_playlist(
    config: &PlayoutConfig,
    date: NaiveDate,
) -> Result<Option<JsonPlaylist>, ServiceError> {
    match read_playlist(config, date.format("%Y-%m-%d").to_string()).await {
        Ok(playlist) => Ok(Some(playlist)),
        Err(ServiceError::NoContent()) => Ok(None),
        Err(error) => Err(error),
    }
}

/// Decide and do what is needed for one day.
async fn ensure_day(
    pool: &SqlitePool,
    config: &PlayoutConfig,
    channel_id: i32,
    channel_name: &str,
    date: NaiveDate,
    on_air: bool,
    written_fingerprint: Option<&str>,
) -> Result<(AutoPlanState, String, String), ServiceError> {
    let plan = plan_for_date(pool, config, channel_id, date).await?;
    let wanted = fingerprint(&plan_to_playlist(&plan, channel_name, date));
    let existing = existing_playlist(config, date).await?;

    let Some(existing) = existing else {
        // A plan with an error is never applied, which reports the reason.
        let message =
            apply_plan(pool, config, channel_id, channel_name, &plan, date, false).await?;

        return Ok((AutoPlanState::Created, message, wanted));
    };

    let current = fingerprint(&existing);
    let is_ours = written_fingerprint == Some(current.as_str());
    // A playlist left alone keeps the fingerprint of what the planner wrote
    // (none when it wrote nothing), so it is never mistaken for its own later.
    let kept = written_fingerprint.unwrap_or_default().to_string();

    if on_air {
        let message = "o dia no ar nunca é alterado".to_string();

        return Ok((AutoPlanState::Existing, message, kept));
    }

    if !is_ours {
        return Ok(match written_fingerprint {
            Some(_) => (
                AutoPlanState::Edited,
                "a playlist foi alterada à mão e fica como está".to_string(),
                kept,
            ),
            None => (
                AutoPlanState::Existing,
                "o dia já tinha uma playlist e fica como está".to_string(),
                kept,
            ),
        });
    }

    if current == wanted {
        return Ok((AutoPlanState::Unchanged, String::new(), current));
    }

    let message = apply_plan(pool, config, channel_id, channel_name, &plan, date, true).await?;

    Ok((AutoPlanState::Updated, message, wanted))
}

/// Make sure the upcoming days of a channel are planned. Returns what was done
/// for each day, from the one on the air onwards.
pub async fn ensure_upcoming_days(
    pool: &SqlitePool,
    config: &PlayoutConfig,
    channel_id: i32,
    channel_name: &str,
    now: DateTime<Utc>,
) -> Result<Vec<DayOutcome>, ServiceError> {
    let settings = handles::select_auto_plan_settings(pool, channel_id).await?;

    if !settings.enabled {
        return Ok(Vec::new());
    }

    let timezone = config.channel.timezone.unwrap_or(Tz::UTC);
    let first = active_playlist_date(now, timezone, config.playlist.start_sec.unwrap_or(0.0));
    let last = first + ChronoDuration::days(i64::from(settings.horizon_days));
    let known = handles::select_auto_plan_days(
        pool,
        channel_id,
        &first.format("%Y-%m-%d").to_string(),
        &last.format("%Y-%m-%d").to_string(),
    )
    .await?;
    let mut outcomes = Vec::new();

    for offset in 0..=i64::from(settings.horizon_days) {
        let date = first + ChronoDuration::days(offset);
        let date_text = date.format("%Y-%m-%d").to_string();
        let previous = known.iter().find(|day| day.plan_date == date_text);
        let written = previous
            .map(|day| day.fingerprint.as_str())
            .filter(|value| !value.is_empty());
        let (state, message, fingerprint) = match ensure_day(
            pool,
            config,
            channel_id,
            channel_name,
            date,
            offset == 0,
            written,
        )
        .await
        {
            Ok(done) => done,
            // A day that cannot be planned says why; the others go on.
            Err(error) => (
                AutoPlanState::Failed,
                error.to_string(),
                // Keep what was written before: it still identifies the playlist.
                written.unwrap_or_default().to_string(),
            ),
        };
        // Only the first sight of a state or a reason is worth a log line.
        let changed = previous.is_none_or(|day| day.state != state || day.message != message);

        if changed {
            match state {
                AutoPlanState::Created | AutoPlanState::Updated => {
                    info!(channel = channel_id; "Dia {date_text} planejado automaticamente: {message}");
                }
                // The day on the air and tomorrow reach the notifications (mail);
                // a day further ahead only the log.
                AutoPlanState::Failed if offset <= 1 => {
                    error!(target: Target::All.as_str(), channel = channel_id; "Não foi possível planejar {date_text} automaticamente: {message}");
                }
                AutoPlanState::Failed => {
                    warn!(channel = channel_id; "Não foi possível planejar {date_text} automaticamente: {message}");
                }
                _ => {}
            }
        }

        handles::upsert_auto_plan_day(pool, channel_id, &date_text, state, &message, &fingerprint)
            .await?;
        outcomes.push(DayOutcome {
            date,
            state,
            message,
        });
    }

    Ok(outcomes)
}

/// The days with no playlist among the one on the air and the next
/// `days_ahead`, for a channel that plays playlists.
pub async fn days_without_playlist(
    config: &PlayoutConfig,
    now: DateTime<Utc>,
    days_ahead: i64,
) -> Vec<NaiveDate> {
    if config.processing.mode != ProcessMode::Playlist {
        return Vec::new();
    }

    let timezone = config.channel.timezone.unwrap_or(Tz::UTC);
    let first = active_playlist_date(now, timezone, config.playlist.start_sec.unwrap_or(0.0));
    let mut missing = Vec::new();

    for offset in 0..=days_ahead {
        let date = first + ChronoDuration::days(offset);

        if matches!(existing_playlist(config, date).await, Ok(None)) {
            missing.push(date);
        }
    }

    missing
}

/// Raise an alert for each day that has no playlist, once per day and channel.
async fn alert_missing_playlists(
    config: &PlayoutConfig,
    channel_id: i32,
    now: DateTime<Utc>,
    alerted: &mut HashSet<(i32, NaiveDate)>,
) {
    for date in days_without_playlist(config, now, MISSING_PLAYLIST_DAYS_AHEAD).await {
        if alerted.insert((channel_id, date)) {
            error!(target: Target::All.as_str(), channel = channel_id; "Não há playlist para {date}: o canal vai tocar o filler");
        }
    }
}

/// Look at every channel now and then until `shutdown`.
pub async fn run_auto_plan(
    pool: SqlitePool,
    controller: Arc<RwLock<ChannelController>>,
    shutdown: CancellationToken,
) {
    tokio::select! {
        () = shutdown.cancelled() => return,
        () = sleep(FIRST_CHECK_DELAY) => {}
    }

    let mut ticker = interval(CHECK_INTERVAL);
    let mut alerted = HashSet::new();

    ticker.set_missed_tick_behavior(MissedTickBehavior::Delay);

    loop {
        tokio::select! {
            () = shutdown.cancelled() => break,
            _ = ticker.tick() => {}
        }

        let managers = controller.read().await.managers.clone();
        let now = Utc::now();

        for manager in managers {
            let config = manager.config.read().await.clone();
            let name = manager.channel.lock().await.name.clone();

            if let Err(error) = ensure_upcoming_days(&pool, &config, manager.id, &name, now).await {
                error!(channel = manager.id; "Cannot check the upcoming days: {error}");
            }

            // This also covers a channel that turned the automatic planning off.
            alert_missing_playlists(&config, manager.id, now, &mut alerted).await;
        }

        // Alerts of past days are not needed again.
        let today = now.date_naive() - ChronoDuration::days(2);

        alerted.retain(|(_, date)| *date >= today);

        let cutoff = (now - ChronoDuration::days(HISTORY_DAYS))
            .format("%Y-%m-%d")
            .to_string();

        if let Err(error) = handles::delete_auto_plan_days_before(&pool, &cutoff).await {
            error!("Cannot clean the automatic planning history: {error}");
        }
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;
    use crate::{
        db::models::{
            Advertiser, BreakPosition, BreakWindow, Campaign, CampaignKind, WeekTemplateItem,
        },
        player::utils::Media,
        test_support::channel_manager,
        utils::playlist::write_playlist,
    };

    struct Fixture {
        pool: SqlitePool,
        config: PlayoutConfig,
        storage: PathBuf,
    }

    // 2026-10-15 is a Thursday. The channel runs in UTC and its day starts at
    // midnight, so the day on the air is the date of the clock.
    fn now() -> DateTime<Utc> {
        DateTime::parse_from_rfc3339("2026-10-15T10:00:00+00:00")
            .unwrap()
            .with_timezone(&Utc)
    }

    fn date(text: &str) -> NaiveDate {
        NaiveDate::parse_from_str(text, "%Y-%m-%d").unwrap()
    }

    fn template_item(name: &str) -> WeekTemplateItem {
        WeekTemplateItem {
            id: 0,
            channel_id: 0,
            weekday: 0,
            position: 0,
            source: format!("{name}.mp4"),
            title: name.to_string(),
            in_sec: 0.0,
            out_sec: 1200.0,
            duration_sec: 1200.0,
            fixed_start: None,
        }
    }

    /// Every weekday has three programs of 20 minutes and a one-minute break
    /// after the first.
    async fn fixture() -> Fixture {
        let (manager, pool) = channel_manager().await;
        let mut config = manager.config.read().await.clone();
        let storage = config.channel.storage.clone();

        config.channel.timezone = Some(Tz::UTC);
        config.playlist.start_sec = Some(0.0);
        config.playlist.length_sec = Some(3660.0);

        for name in ["a", "b", "c"] {
            std::fs::write(storage.join(format!("{name}.mp4")), b"media").unwrap();
        }

        for weekday in 1..=7 {
            handles::replace_week_template_day(
                &pool,
                1,
                weekday,
                &[template_item("a"), template_item("b"), template_item("c")],
            )
            .await
            .unwrap();
            handles::insert_break_window(
                &pool,
                1,
                &BreakWindow {
                    id: 0,
                    channel_id: 1,
                    weekday,
                    nominal_time: "00:20:00".to_string(),
                    duration_sec: 60,
                    tolerance_sec: 60,
                    enabled: true,
                },
            )
            .await
            .unwrap();
        }

        Fixture {
            pool,
            config,
            storage,
        }
    }

    async fn set_horizon(fixture: &Fixture, enabled: bool, days: i32) {
        handles::upsert_auto_plan_settings(
            &fixture.pool,
            1,
            &crate::db::models::AutoPlanSettings {
                channel_id: 1,
                enabled,
                horizon_days: days,
            },
        )
        .await
        .unwrap();
    }

    async fn run(fixture: &Fixture) -> Vec<DayOutcome> {
        ensure_upcoming_days(&fixture.pool, &fixture.config, 1, "Channel", now())
            .await
            .unwrap()
    }

    fn states(outcomes: &[DayOutcome]) -> Vec<AutoPlanState> {
        outcomes.iter().map(|outcome| outcome.state).collect()
    }

    async fn playlist(fixture: &Fixture, day: &str) -> JsonPlaylist {
        read_playlist(&fixture.config, day.to_string())
            .await
            .unwrap()
    }

    async fn add_spot(fixture: &Fixture) {
        let advertiser_id = handles::insert_advertiser(
            &fixture.pool,
            1,
            &Advertiser {
                id: 0,
                channel_id: 1,
                name: "Padaria".to_string(),
                contact_name: String::new(),
                email: String::new(),
                notes: String::new(),
                active: true,
            },
        )
        .await
        .unwrap();
        let campaign_id = handles::insert_campaign(
            &fixture.pool,
            1,
            &Campaign {
                id: 0,
                channel_id: 1,
                advertiser_id,
                name: "Natal".to_string(),
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
        let spot_id = handles::insert_spot(
            &fixture.pool,
            1,
            campaign_id,
            "Natal",
            "upload/natal.mxf",
            "natal.mxf",
            None,
        )
        .await
        .unwrap()
        .unwrap();
        let converted = format!("commercials/spot-{spot_id}.mp4");

        std::fs::create_dir_all(fixture.storage.join("commercials")).unwrap();
        std::fs::write(fixture.storage.join(&converted), b"media").unwrap();
        handles::complete_spot(&fixture.pool, 1, spot_id, &converted, 30.0)
            .await
            .unwrap();
    }

    fn has_spot(playlist: &JsonPlaylist) -> bool {
        playlist
            .program
            .iter()
            .any(|item| item.source.starts_with("commercials/"))
    }

    #[test]
    fn the_day_on_the_air_follows_the_start_of_the_day() {
        let sao_paulo: Tz = "America/Sao_Paulo".parse().unwrap();
        let six = 6.0 * 3600.0;

        // 08:00 UTC is 05:00 in Sao Paulo: the day of the 15th has not started.
        let before = DateTime::parse_from_rfc3339("2026-10-15T08:00:00+00:00").unwrap();
        let after = DateTime::parse_from_rfc3339("2026-10-15T10:00:00+00:00").unwrap();

        assert_eq!(
            active_playlist_date(before.with_timezone(&Utc), sao_paulo, six),
            date("2026-10-14")
        );
        assert_eq!(
            active_playlist_date(after.with_timezone(&Utc), sao_paulo, six),
            date("2026-10-15")
        );
        // 05:00 local is before a day start of 05:59:25: still the day before.
        assert_eq!(
            active_playlist_date(
                before.with_timezone(&Utc),
                sao_paulo,
                5.0 * 3600.0 + 59.0 * 60.0 + 25.0
            ),
            date("2026-10-14")
        );
    }

    #[test]
    fn the_fingerprint_follows_the_content_and_ignores_the_rest() {
        let media = |source: &str, ad: bool| Media {
            source: source.to_string(),
            seek: 0.0,
            out: 30.0,
            duration: 30.0,
            ad,
            ..Media::default()
        };
        let list = |items: Vec<Media>| JsonPlaylist {
            channel: "x".to_string(),
            date: "2026-10-15".to_string(),
            start_sec: None,
            length: None,
            path: None,
            modified: Some("whenever".to_string()),
            program: items,
        };
        let base = fingerprint(&list(vec![media("a.mp4", false), media("b.mp4", true)]));
        let mut retitled = media("a.mp4", false);

        retitled.title = Some("another title".to_string());

        assert_eq!(
            base,
            fingerprint(&list(vec![retitled, media("b.mp4", true)]))
        );
        assert_ne!(
            base,
            fingerprint(&list(vec![media("b.mp4", true), media("a.mp4", false)]))
        );
        assert_ne!(
            base,
            fingerprint(&list(vec![media("a.mp4", false), media("b.mp4", false)]))
        );
    }

    #[tokio::test]
    async fn the_upcoming_days_are_created_and_left_alone_when_nothing_changes() {
        let fixture = fixture().await;

        set_horizon(&fixture, true, 2).await;

        let first = run(&fixture).await;

        assert_eq!(
            first.iter().map(|outcome| outcome.date).collect::<Vec<_>>(),
            [date("2026-10-15"), date("2026-10-16"), date("2026-10-17")]
        );
        assert_eq!(states(&first), [AutoPlanState::Created; 3]);

        for day in ["2026-10-15", "2026-10-16", "2026-10-17"] {
            let programs = playlist(&fixture, day).await.program;
            let played = |name: &str| {
                programs
                    .iter()
                    .filter(|item| item.source.ends_with(name))
                    .count()
            };

            assert_eq!(
                (played("a.mp4"), played("b.mp4"), played("c.mp4")),
                (1, 1, 1),
                "{day}"
            );
        }

        let second = run(&fixture).await;

        assert_eq!(
            states(&second),
            [
                AutoPlanState::Existing,
                AutoPlanState::Unchanged,
                AutoPlanState::Unchanged
            ],
            "the day on the air is never touched"
        );
    }

    #[tokio::test]
    async fn a_playlist_somebody_else_wrote_is_never_replaced() {
        let fixture = fixture().await;

        set_horizon(&fixture, true, 1).await;

        let manual = JsonPlaylist {
            channel: "Channel".to_string(),
            date: "2026-10-16".to_string(),
            start_sec: None,
            length: None,
            path: None,
            modified: None,
            program: vec![Media {
                source: "a.mp4".to_string(),
                out: 600.0,
                duration: 600.0,
                ..Media::default()
            }],
        };

        write_playlist(&fixture.config, manual).await.unwrap();

        let outcomes = run(&fixture).await;

        assert_eq!(
            states(&outcomes),
            [AutoPlanState::Created, AutoPlanState::Existing]
        );
        assert_eq!(playlist(&fixture, "2026-10-16").await.program.len(), 1);

        add_spot(&fixture).await;

        let later = run(&fixture).await;

        assert_eq!(
            later[1].state,
            AutoPlanState::Existing,
            "still the operator's"
        );
        assert_eq!(playlist(&fixture, "2026-10-16").await.program.len(), 1);
    }

    #[tokio::test]
    async fn a_new_spot_reaches_the_days_the_planner_wrote_but_not_the_day_on_the_air() {
        let fixture = fixture().await;

        set_horizon(&fixture, true, 2).await;
        run(&fixture).await;

        assert!(!has_spot(&playlist(&fixture, "2026-10-16").await));

        add_spot(&fixture).await;

        let outcomes = run(&fixture).await;

        assert_eq!(
            states(&outcomes),
            [
                AutoPlanState::Existing,
                AutoPlanState::Updated,
                AutoPlanState::Updated
            ]
        );
        assert!(has_spot(&playlist(&fixture, "2026-10-16").await));
        assert!(has_spot(&playlist(&fixture, "2026-10-17").await));
        assert!(
            !has_spot(&playlist(&fixture, "2026-10-15").await),
            "the day on the air stays"
        );
        assert_eq!(states(&run(&fixture).await)[1], AutoPlanState::Unchanged);
    }

    #[tokio::test]
    async fn a_day_the_planner_wrote_and_somebody_edited_is_kept_as_edited() {
        let fixture = fixture().await;

        set_horizon(&fixture, true, 1).await;
        run(&fixture).await;

        let mut edited = playlist(&fixture, "2026-10-16").await;

        edited.program.truncate(1);
        write_playlist(&fixture.config, edited).await.unwrap();
        add_spot(&fixture).await;

        let outcomes = run(&fixture).await;

        assert_eq!(outcomes[1].state, AutoPlanState::Edited);
        assert_eq!(playlist(&fixture, "2026-10-16").await.program.len(), 1);
        assert_eq!(
            run(&fixture).await[1].state,
            AutoPlanState::Edited,
            "and stays so"
        );
    }

    #[tokio::test]
    async fn a_day_that_cannot_be_planned_says_why_and_the_others_go_on() {
        let fixture = fixture().await;

        // Friday has no programs.
        handles::replace_week_template_day(&fixture.pool, 1, 5, &[])
            .await
            .unwrap();
        set_horizon(&fixture, true, 2).await;

        let outcomes = run(&fixture).await;

        assert_eq!(
            states(&outcomes),
            [
                AutoPlanState::Created,
                AutoPlanState::Failed,
                AutoPlanState::Created
            ]
        );
        assert!(!outcomes[1].message.is_empty());

        let stored = handles::select_auto_plan_days(&fixture.pool, 1, "2026-10-16", "2026-10-16")
            .await
            .unwrap();

        assert_eq!(stored[0].state, AutoPlanState::Failed);
    }

    #[tokio::test]
    async fn days_without_a_playlist_are_found_for_the_day_on_the_air_and_tomorrow() {
        let fixture = fixture().await;

        assert_eq!(
            days_without_playlist(&fixture.config, now(), 1).await,
            [date("2026-10-15"), date("2026-10-16")]
        );

        set_horizon(&fixture, true, 1).await;
        run(&fixture).await;

        assert!(
            days_without_playlist(&fixture.config, now(), 1)
                .await
                .is_empty()
        );

        // Folder mode needs no playlist.
        let mut folder = fixture.config.clone();

        folder.processing.mode = ProcessMode::Folder;
        std::fs::remove_dir_all(&folder.channel.playlists).unwrap();

        assert!(days_without_playlist(&folder, now(), 1).await.is_empty());
    }

    #[tokio::test]
    async fn nothing_is_planned_when_the_channel_turns_it_off() {
        let fixture = fixture().await;

        set_horizon(&fixture, false, 3).await;

        assert!(run(&fixture).await.is_empty());
        assert!(
            read_playlist(&fixture.config, "2026-10-15".to_string())
                .await
                .is_err()
        );
    }
}
