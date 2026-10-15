//! Scheduled on-screen graphics (CG).
//!
//! A background task decides every second which graphics are on the air and
//! hands the full set to the engine, which draws them above the picture. The
//! decision depends only on the clock, the program on the air and the manual
//! "show now" requests, so it is a pure function that is easy to test.

use std::{
    collections::{HashMap, HashSet},
    path::{Component, Path},
    sync::Arc,
    time::{Duration, Instant},
};

use chrono::{
    DateTime, Datelike, Duration as ChronoDuration, NaiveDate, NaiveDateTime, Timelike, Utc,
};
use chrono_tz::Tz;
use ff_engine::{
    GraphicContent, GraphicLayer, LogoConfig, RgbaColor, TextBackgroundConfig, TextConfig,
    TextPosition, TextScroll, TextWeight,
};
use log::*;
use sqlx::sqlite::SqlitePool;
use tokio::{
    sync::RwLock,
    time::{MissedTickBehavior, interval},
};
use tokio_util::sync::CancellationToken;

use crate::{
    db::{
        handles,
        models::{Cg, CgKind, CgMode, CgSchedule, CgWithSchedules},
    },
    player::controller::{ChannelController, ChannelManager},
    utils::{
        config::PlayoutConfig,
        errors::ServiceError,
        text::{opacity_to_alpha, parse_color},
    },
};

const TICK: Duration = Duration::from_secs(1);

/// How often the graphics are read again, so an edit reaches the air quickly
/// without a database read on every tick.
const RELOAD_EVERY: Duration = Duration::from_secs(5);

/// Longest "show now" request.
pub const MAX_MANUAL_SECONDS: i64 = 3600;

const SECONDS_PER_DAY: i64 = 24 * 60 * 60;

fn day_matches(schedule: &CgSchedule, date: NaiveDate) -> bool {
    let weekday = date.weekday().num_days_from_monday();

    if schedule.weekdays & (1 << weekday) == 0 {
        return false;
    }

    let after_start = schedule
        .date_from
        .as_deref()
        .and_then(|from| NaiveDate::parse_from_str(from, "%Y-%m-%d").ok())
        .is_none_or(|from| date >= from);
    let before_end = schedule
        .date_to
        .as_deref()
        .and_then(|to| NaiveDate::parse_from_str(to, "%Y-%m-%d").ok())
        .is_none_or(|to| date <= to);

    after_start && before_end
}

fn seconds_of_day(time: chrono::NaiveTime) -> i64 {
    i64::from(time.num_seconds_from_midnight())
}

/// Whether a clock rule that started on `date` is on the air `seconds` after
/// that day began. `seconds` may pass 24 h for a window that crosses midnight.
fn clock_active_on(schedule: &CgSchedule, date: NaiveDate, seconds: i64) -> bool {
    if !day_matches(schedule, date) {
        return false;
    }

    let Ok(start) = schedule.start() else {
        return false;
    };
    let start = seconds_of_day(start);

    if seconds < start {
        return false;
    }

    let elapsed = seconds - start;
    let duration = i64::from(schedule.duration_seconds);

    let Some(repeat) = schedule.repeat_every_seconds.map(i64::from) else {
        return elapsed < duration;
    };

    let until = schedule
        .until_time
        .as_deref()
        .and_then(|time| {
            chrono::NaiveTime::parse_from_str(time, "%H:%M:%S")
                .or_else(|_| chrono::NaiveTime::parse_from_str(time, "%H:%M"))
                .ok()
        })
        .map_or(SECONDS_PER_DAY, seconds_of_day);
    let cycle = elapsed / repeat;

    // The last repetition is the last one that begins before the end time.
    start + cycle * repeat <= until && elapsed % repeat < duration
}

/// A rule that fires at a time of the day, in the channel clock.
pub fn clock_active(schedule: &CgSchedule, local: NaiveDateTime) -> bool {
    let seconds = seconds_of_day(local.time());

    clock_active_on(schedule, local.date(), seconds)
        || local.date().pred_opt().is_some_and(|yesterday| {
            clock_active_on(schedule, yesterday, seconds + SECONDS_PER_DAY)
        })
}

/// A rule that fires after a program started. Spots never start a program.
pub fn program_active(
    schedule: &CgSchedule,
    title: &str,
    started: NaiveDateTime,
    now: NaiveDateTime,
) -> bool {
    if !day_matches(schedule, started.date()) {
        return false;
    }

    let wanted = schedule.program_match.trim().to_lowercase();

    if !wanted.is_empty() && !title.to_lowercase().contains(&wanted) {
        return false;
    }

    let elapsed = (now - started).num_seconds();
    let offset = i64::from(schedule.offset_seconds);

    elapsed >= offset && elapsed < offset + i64::from(schedule.duration_seconds)
}

/// The program on the air, as far as the graphics are concerned.
#[derive(Debug, Clone)]
pub struct ProgramOnAir {
    pub title: String,
    pub started: NaiveDateTime,
}

/// Whether any rule of a graphic puts it on the air now.
pub fn scheduled_now(
    item: &CgWithSchedules,
    now: NaiveDateTime,
    program: Option<&ProgramOnAir>,
) -> bool {
    item.cg.enabled
        && item
            .schedules
            .iter()
            .filter(|rule| rule.enabled)
            .any(|rule| match rule.mode {
                CgMode::Clock => clock_active(rule, now),
                CgMode::Program => program.is_some_and(|program| {
                    program_active(rule, &program.title, program.started, now)
                }),
            })
}

fn relative_image_path(path: &str) -> Result<&Path, String> {
    let path = Path::new(path);
    let inside = path
        .components()
        .all(|component| matches!(component, Component::Normal(_) | Component::CurDir));

    if path.as_os_str().is_empty() || !inside {
        return Err("the image must be a file of the channel storage".to_string());
    }

    Ok(path)
}

/// The engine layer for a graphic, sized for the channel output.
pub fn build_layer(
    cg: &Cg,
    width: u32,
    height: u32,
    storage_root: &Path,
) -> Result<GraphicLayer, String> {
    let left = (cg.x_pct / 100.0 * f64::from(width)).round() as i32;
    let top = (cg.y_pct / 100.0 * f64::from(height)).round() as i32;

    let content = match cg.kind {
        CgKind::Text => {
            let font_size = (cg.size_pct / 100.0 * f64::from(height)).max(8.0) as f32;
            let background = cg.box_enabled.then(|| {
                let mut color = parse_color(&cg.box_color, RgbaColor::opaque(0, 0, 0));

                color.a = opacity_to_alpha(cg.box_opacity);

                TextBackgroundConfig {
                    color,
                    padding: (font_size * 0.3).round() as u32,
                }
            });

            GraphicContent::Text(TextConfig {
                text: Some(cg.text.clone()),
                font_family: (!cg.font_family.trim().is_empty()).then(|| cg.font_family.clone()),
                font_weight: match cg.font_weight.as_str() {
                    "semibold" => TextWeight::Semibold,
                    "normal" => TextWeight::Normal,
                    _ => TextWeight::Bold,
                },
                font_size,
                text_color: parse_color(&cg.text_color, RgbaColor::opaque(255, 255, 255)),
                opacity: cg.opacity,
                position_x: TextPosition::Pixels(left),
                position_y: TextPosition::Pixels(top),
                background,
                scroll: TextScroll::None,
                fade_in_seconds: cg.fade_in_seconds,
                ..TextConfig::default()
            })
        }
        CgKind::Image => {
            let file = storage_root.join(relative_image_path(&cg.image_path)?);

            if !file.is_file() {
                return Err(format!("image {} not found", cg.image_path));
            }

            let image_width = (cg.size_pct / 100.0 * f64::from(width)).round().max(2.0) as u32;

            GraphicContent::Image(LogoConfig {
                path: file.to_string_lossy().to_string(),
                scale: Some(format!("{image_width}:-1")),
                opacity: cg.opacity,
                position: format!("{left}:{top}"),
            })
        }
    };

    Ok(GraphicLayer {
        id: i64::from(cg.id),
        content,
    })
}

/// Checks that a graphic can be drawn, so a mistake shows up when it is saved
/// and not at the scheduled time.
pub fn check_cg(item: &CgWithSchedules, storage_root: &Path) -> Result<(), ServiceError> {
    item.validate().map_err(ServiceError::BadRequest)?;

    if item.cg.kind == CgKind::Image {
        build_layer(&item.cg, 1920, 1080, storage_root).map_err(ServiceError::BadRequest)?;
    }

    Ok(())
}

/// The graphics that are on the air at `now`, in a stable order.
pub fn active_layers(
    items: &[CgWithSchedules],
    now: NaiveDateTime,
    program: Option<&ProgramOnAir>,
    forced: &HashSet<i32>,
    config: &PlayoutConfig,
    warned: &mut HashSet<i32>,
) -> Vec<GraphicLayer> {
    let mut layers = Vec::new();

    for item in items {
        let wanted = forced.contains(&item.cg.id) || scheduled_now(item, now, program);

        if !wanted {
            continue;
        }

        match build_layer(
            &item.cg,
            config.output.width,
            config.output.height,
            &config.storage.path,
        ) {
            Ok(layer) => layers.push(layer),
            Err(error) => {
                if warned.insert(item.cg.id) {
                    warn!(channel = config.general.channel_id; "Graphic '{}' not shown: {error}", item.cg.name);
                }
            }
        }
    }

    layers.sort_by_key(|layer| layer.id);

    layers
}

#[derive(Default)]
struct ChannelCg {
    items: Vec<CgWithSchedules>,
    loaded: Option<Instant>,
    program: Option<ProgramOnAir>,
    program_key: Option<(String, u64)>,
    warned: HashSet<i32>,
}

impl ChannelCg {
    async fn reload_if_old(&mut self, pool: &SqlitePool, channel: i32) {
        if self
            .loaded
            .is_some_and(|loaded| loaded.elapsed() < RELOAD_EVERY)
        {
            return;
        }

        self.loaded = Some(Instant::now());

        match handles::select_cgs(pool, channel).await {
            Ok(items) => {
                let ids: HashSet<i32> = items.iter().map(|item| item.cg.id).collect();

                self.warned.retain(|id| ids.contains(id));
                self.items = items;
            }
            Err(error) => error!(channel = channel; "Cannot read the graphics: {error}"),
        }
    }

    /// Remembers when the program on the air started. Spots do not count, so a
    /// rule counted from the program start keeps counting through a break.
    async fn follow_program(&mut self, manager: &ChannelManager, now: NaiveDateTime) {
        let current = manager.current_media.lock().await.clone();
        let Some(media) = current.filter(|media| !media.ad && !media.source.is_empty()) else {
            return;
        };
        let key = (
            media.source.clone(),
            media.begin.unwrap_or_default().to_bits(),
        );

        if self.program_key.as_ref() == Some(&key) {
            return;
        }

        self.program_key = Some(key);
        self.program = Some(ProgramOnAir {
            title: media.title.clone().unwrap_or_else(|| {
                Path::new(&media.source)
                    .file_stem()
                    .map(|name| name.to_string_lossy().to_string())
                    .unwrap_or_default()
            }),
            started: now,
        });
    }
}

/// Graphics the operator asked to show right now, until the stored time.
pub fn forced_ids(manager: &ChannelManager, now: DateTime<Utc>) -> HashSet<i32> {
    let mut manual = manager
        .cg_manual
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);

    manual.retain(|_, until| *until > now);

    manual.keys().copied().collect()
}

pub fn show_now(manager: &ChannelManager, cg_id: i32, seconds: i64, now: DateTime<Utc>) {
    let until = now + ChronoDuration::seconds(seconds.clamp(1, MAX_MANUAL_SECONDS));

    manager
        .cg_manual
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .insert(cg_id, until);
}

pub fn hide_now(manager: &ChannelManager, cg_id: i32) {
    manager
        .cg_manual
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .remove(&cg_id);
}

pub async fn run_cg_scheduler(
    pool: SqlitePool,
    controller: Arc<RwLock<ChannelController>>,
    shutdown: CancellationToken,
) {
    let mut ticker = interval(TICK);
    let mut channels: HashMap<i32, ChannelCg> = HashMap::new();

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
            let local = now
                .with_timezone(&config.channel.timezone.unwrap_or(Tz::UTC))
                .naive_local();
            let state = channels.entry(manager.id).or_default();

            state.reload_if_old(&pool, manager.id).await;
            state.follow_program(&manager, local).await;

            let forced = forced_ids(&manager, now);
            let layers = active_layers(
                &state.items,
                local,
                state.program.as_ref(),
                &forced,
                &config,
                &mut state.warned,
            );

            manager.graphics.set(layers);
        }
    }
}

#[cfg(test)]
mod tests {
    use chrono::NaiveDate;

    use super::*;

    fn at(date: &str, time: &str) -> NaiveDateTime {
        NaiveDateTime::parse_from_str(&format!("{date} {time}"), "%Y-%m-%d %H:%M:%S").unwrap()
    }

    fn clock(start: &str, duration: i32) -> CgSchedule {
        CgSchedule {
            id: 0,
            cg_id: 0,
            mode: CgMode::Clock,
            weekdays: 127,
            date_from: None,
            date_to: None,
            start_time: start.to_string(),
            program_match: String::new(),
            offset_seconds: 0,
            duration_seconds: duration,
            repeat_every_seconds: None,
            until_time: None,
            enabled: true,
        }
    }

    fn graphic(kind: CgKind) -> Cg {
        Cg {
            id: 7,
            channel_id: 1,
            name: "g".to_string(),
            kind,
            text: "Hello".to_string(),
            image_path: String::new(),
            x_pct: 50.0,
            y_pct: 10.0,
            size_pct: 5.0,
            font_family: String::new(),
            font_weight: "bold".to_string(),
            text_color: "#ff0000".to_string(),
            box_enabled: true,
            box_color: "#000000".to_string(),
            box_opacity: 0.5,
            opacity: 1.0,
            fade_in_seconds: 0.0,
            enabled: true,
        }
    }

    #[test]
    fn a_single_window_opens_at_the_start_and_closes_after_the_duration() {
        let rule = clock("18:00:00", 15);

        assert!(!clock_active(&rule, at("2026-10-12", "17:59:59")));
        assert!(clock_active(&rule, at("2026-10-12", "18:00:00")));
        assert!(clock_active(&rule, at("2026-10-12", "18:00:14")));
        assert!(!clock_active(&rule, at("2026-10-12", "18:00:15")));
    }

    #[test]
    fn a_repeating_rule_shows_again_until_the_end_time() {
        let mut rule = clock("18:00", 10);

        rule.repeat_every_seconds = Some(1800);
        rule.until_time = Some("19:00".to_string());

        assert!(clock_active(&rule, at("2026-10-12", "18:30:05")));
        assert!(!clock_active(&rule, at("2026-10-12", "18:30:10")));
        assert!(clock_active(&rule, at("2026-10-12", "19:00:03")));
        assert!(!clock_active(&rule, at("2026-10-12", "19:30:03")));
    }

    #[test]
    fn weekdays_and_dates_limit_the_rule() {
        let mut rule = clock("18:00", 60);

        // 2026-10-12 is a Monday, 2026-10-17 a Saturday.
        rule.weekdays = 0b001_1111;

        assert!(clock_active(&rule, at("2026-10-12", "18:00:30")));
        assert!(!clock_active(&rule, at("2026-10-17", "18:00:30")));

        rule.weekdays = 127;
        rule.date_from = Some("2026-10-13".to_string());
        rule.date_to = Some("2026-10-14".to_string());

        assert!(!clock_active(&rule, at("2026-10-12", "18:00:30")));
        assert!(clock_active(&rule, at("2026-10-13", "18:00:30")));
        assert!(!clock_active(&rule, at("2026-10-15", "18:00:30")));
    }

    #[test]
    fn a_window_can_cross_midnight() {
        let rule = clock("23:59:30", 120);

        assert!(clock_active(&rule, at("2026-10-12", "23:59:45")));
        assert!(clock_active(&rule, at("2026-10-13", "00:00:50")));
        assert!(!clock_active(&rule, at("2026-10-13", "00:01:31")));
    }

    #[test]
    fn a_program_rule_counts_from_the_program_start() {
        let mut rule = clock("00:00", 20);

        rule.mode = CgMode::Program;
        rule.program_match = "JORNAL".to_string();
        rule.offset_seconds = 30;

        let started = at("2026-10-12", "19:00:00");

        assert!(!program_active(
            &rule,
            "Jornal da Noite",
            started,
            at("2026-10-12", "19:00:29")
        ));
        assert!(program_active(
            &rule,
            "Jornal da Noite",
            started,
            at("2026-10-12", "19:00:30")
        ));
        assert!(!program_active(
            &rule,
            "Jornal da Noite",
            started,
            at("2026-10-12", "19:00:50")
        ));
        assert!(!program_active(
            &rule,
            "Novela",
            started,
            at("2026-10-12", "19:00:40")
        ));

        rule.program_match = String::new();

        assert!(program_active(
            &rule,
            "Novela",
            started,
            at("2026-10-12", "19:00:40")
        ));
    }

    #[test]
    fn a_disabled_graphic_or_rule_is_never_scheduled() {
        let mut item = CgWithSchedules {
            cg: graphic(CgKind::Text),
            schedules: vec![clock("18:00", 60)],
        };
        let now = at("2026-10-12", "18:00:10");

        assert!(scheduled_now(&item, now, None));

        item.schedules[0].enabled = false;
        assert!(!scheduled_now(&item, now, None));

        item.schedules[0].enabled = true;
        item.cg.enabled = false;
        assert!(!scheduled_now(&item, now, None));
    }

    #[test]
    fn the_position_follows_the_output_size() {
        let cg = graphic(CgKind::Text);

        for (width, height) in [(1920_u32, 1080_u32), (1280, 720)] {
            let layer = build_layer(&cg, width, height, Path::new("/tmp")).unwrap();
            let GraphicContent::Text(text) = layer.content else {
                panic!("a text graphic must build a text layer");
            };

            assert_eq!(
                text.position_x,
                TextPosition::Pixels((f64::from(width) / 2.0) as i32)
            );
            assert_eq!(
                text.position_y,
                TextPosition::Pixels((f64::from(height) / 10.0).round() as i32)
            );
            assert!((f64::from(text.font_size) - f64::from(height) * 0.05).abs() < 1.0);
            assert!(text.background.is_some());
        }
    }

    #[test]
    fn an_image_must_be_a_file_inside_the_storage() {
        let mut cg = graphic(CgKind::Image);

        for path in ["../x.png", "/etc/passwd", ""] {
            cg.image_path = path.to_string();
            assert!(
                build_layer(&cg, 1920, 1080, Path::new("/tmp")).is_err(),
                "{path}"
            );
        }

        cg.image_path = "missing-file.png".to_string();
        assert!(
            build_layer(&cg, 1920, 1080, Path::new("/tmp"))
                .unwrap_err()
                .contains("not found")
        );
    }

    #[test]
    fn day_matching_uses_monday_as_bit_zero() {
        let mut rule = clock("00:00", 1);

        rule.weekdays = 1;

        assert!(day_matches(
            &rule,
            NaiveDate::from_ymd_opt(2026, 10, 12).unwrap()
        ));
        assert!(!day_matches(
            &rule,
            NaiveDate::from_ymd_opt(2026, 10, 18).unwrap()
        ));
    }
}
