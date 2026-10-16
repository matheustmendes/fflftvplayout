//! A picture of a channel at one moment: what the alerts compare from one look
//! to the next and what the commands tell.

use std::{path::Path, sync::atomic::Ordering};

use chrono::{DateTime, NaiveDate, Utc};
use chrono_tz::Tz;
use sqlx::SqlitePool;

use crate::{
    db::handles,
    player::{
        controller::ChannelManager,
        utils::{Media, time_in_seconds},
    },
    utils::{
        auto_plan::{active_playlist_date, days_without_playlist},
        config::ProcessMode,
        health::{FillerStatus, RelayStatus, check_relay, filler_status},
        system::SystemStat,
    },
};

/// How many days ahead a missing playlist is worth telling.
const PLAYLIST_DAYS_AHEAD: i64 = 1;

#[derive(Debug, Clone)]
pub struct Playing {
    pub title: String,
    pub ad: bool,
    /// Seconds since it started.
    pub elapsed: f64,
    pub duration: f64,
}

#[derive(Debug, Clone)]
pub struct Upcoming {
    pub title: String,
    pub ad: bool,
    pub duration: f64,
    /// Seconds from now until it starts.
    pub starts_in: f64,
}

#[derive(Debug, Clone, Copy)]
pub struct Disk {
    pub free: u64,
    pub total: u64,
}

impl Disk {
    pub fn free_percent(&self) -> f64 {
        if self.total == 0 {
            return 100.0;
        }

        self.free as f64 / self.total as f64 * 100.0
    }
}

#[derive(Debug, Clone)]
pub struct Snapshot {
    pub channel_id: i32,
    pub name: String,
    pub timezone: Tz,
    pub now: DateTime<Utc>,
    /// The operator wants the channel on the air.
    pub active: bool,
    /// The channel is running.
    pub alive: bool,
    pub playlist_mode: bool,
    pub current: Option<Playing>,
    pub upcoming: Vec<Upcoming>,
    /// A live source is on the air.
    pub live: bool,
    pub filler: FillerStatus,
    pub relay: RelayStatus,
    /// The date of the playlist on the air.
    pub air_date: NaiveDate,
    /// Days without a playlist, from the one on the air to the next.
    pub missing_days: Vec<NaiveDate>,
    pub disk: Option<Disk>,
}

impl Snapshot {
    pub fn local(&self, at: DateTime<Utc>) -> DateTime<Tz> {
        at.with_timezone(&self.timezone)
    }
}

/// What to show of a clip: its title, or the name of its file.
pub fn clip_title(media: &Media) -> String {
    media
        .title
        .as_deref()
        .map(str::trim)
        .filter(|title| !title.is_empty())
        .map(str::to_string)
        .unwrap_or_else(|| {
            Path::new(&media.source).file_name().map_or_else(
                || media.source.clone(),
                |name| name.to_string_lossy().to_string(),
            )
        })
}

/// Look at the channel now. `upcoming` is how many clips to list after the one
/// on the air.
pub async fn collect(
    manager: &ChannelManager,
    pool: &SqlitePool,
    system: &SystemStat,
    upcoming: usize,
) -> Snapshot {
    let now = Utc::now();
    let config = manager.config.read().await.clone();
    let channel = manager.channel.lock().await.clone();
    let timezone = channel.timezone.unwrap_or(Tz::UTC);
    let current_media = manager.current_media.lock().await.clone();
    let (live, _) = manager.playback_control.lock().await.live_status();
    let (current, upcoming) = match current_media {
        Some(media) if media.begin.is_some() => {
            let begin = media.begin.unwrap_or_default();
            let airtime = (media.out - media.seek).max(0.0);
            let elapsed = (time_in_seconds(&channel.timezone) - (begin - channel.time_shift))
                .clamp(0.0, airtime.max(media.duration));
            let mut starts_in = (airtime - elapsed).max(0.0);
            let mut coming = Vec::new();
            // Only what is listed is read: the playlist is long and cloning it every
            // few seconds would be a waste.
            let list = manager.current_list.lock().await;

            for next in list.iter().skip(media.index.map_or(usize::MAX, |i| i + 1)) {
                if coming.len() >= upcoming {
                    break;
                }

                coming.push(Upcoming {
                    title: clip_title(next),
                    ad: next.ad,
                    duration: (next.out - next.seek).max(0.0),
                    starts_in,
                });
                starts_in += (next.out - next.seek).max(0.0);
            }

            (
                Some(Playing {
                    title: clip_title(&media),
                    ad: media.ad,
                    elapsed,
                    duration: airtime,
                }),
                coming,
            )
        }
        _ => (None, Vec::new()),
    };
    let relay = match handles::select_relay(pool, manager.id).await {
        Ok(relay_config) => check_relay(&relay_config).await,
        Err(_) => RelayStatus {
            configured: false,
            reachable: false,
            destinations: Vec::new(),
            others: 0,
        },
    };
    let air_date = active_playlist_date(now, timezone, config.playlist.start_sec.unwrap_or(0.0));
    let storage = system.stat(&config).await.storage;

    Snapshot {
        channel_id: manager.id,
        name: channel.name,
        timezone,
        now,
        active: channel.active,
        alive: manager.is_alive.load(Ordering::SeqCst),
        playlist_mode: config.processing.mode == ProcessMode::Playlist,
        current,
        upcoming,
        live,
        filler: filler_status(&config),
        relay,
        air_date,
        missing_days: days_without_playlist(&config, now, PLAYLIST_DAYS_AHEAD).await,
        disk: (storage.total > 0).then_some(Disk {
            free: storage.free,
            total: storage.total,
        }),
    }
}
