//! The alerts. A look at the channel every few seconds is turned into a list of
//! problems; the tracker tells only what changed: a problem that lasts is said
//! once, reminded now and then and, when it is over, said to be over.

use std::{
    collections::{HashMap, HashSet},
    sync::Arc,
    time::Duration as StdDuration,
};

use chrono::{DateTime, Duration, NaiveDate, Utc};
use log::warn;
use sqlx::SqlitePool;
use tokio::{
    select,
    sync::{RwLock, broadcast::error::RecvError},
    time::{MissedTickBehavior, interval, sleep},
};
use tokio_util::sync::CancellationToken;

use super::{
    client::TelegramClient,
    events::{self, Event},
    messages::{self, esc, truncate},
    snapshot::{Snapshot, collect},
};
use crate::{
    db::{
        handles::{self, AiringFilter},
        models::{AirKind, AirResult, Airing, RelayProtocol, TelegramSettings},
    },
    player::controller::ChannelController,
    utils::{health::ConnectionState, system::SystemStat},
};

/// How often the channels are looked at.
const TICK: StdDuration = StdDuration::from_secs(15);
/// The first look waits for the channels to start, so a start is not a failure.
const FIRST_DELAY: StdDuration = StdDuration::from_secs(45);
/// The airing log is read once every this many looks.
const COMMERCIAL_EVERY_TICKS: u64 = 4;
/// Below this share of free space the disk is a problem.
const DISK_LOW_PERCENT: f64 = 10.0;
const MAX_LISTED_SPOTS: usize = 10;

/// What can be wrong with a channel.
#[derive(Debug, Clone, Eq, Hash, PartialEq)]
pub enum Key {
    /// The channel should be on the air and is not.
    Output,
    /// The operator turned the channel off.
    Stopped,
    /// The relay does not answer.
    Relay,
    Destination(i32),
    Filler,
    Playlist(NaiveDate),
    Disk,
}

impl Key {
    /// The setting that turns this alert on.
    fn enabled(&self, settings: &TelegramSettings) -> bool {
        match self {
            Self::Output | Self::Stopped => settings.alert_output,
            Self::Relay | Self::Destination(_) => settings.alert_destinations,
            Self::Filler => settings.alert_filler,
            Self::Playlist(_) => settings.alert_playlist,
            Self::Disk => settings.alert_disk,
        }
    }
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum NoticeKind {
    Raised,
    Reminder,
    Recovered,
}

/// Something to tell the group.
#[derive(Debug, Clone, PartialEq)]
pub struct Notice {
    pub kind: NoticeKind,
    pub key: Key,
    /// What is wrong, or what got right when `kind` is `Recovered`.
    pub body: String,
    /// How long the problem has lasted.
    pub lasted: Duration,
    /// How many times it failed in the meantime, for what restarts by itself.
    pub failures: u32,
}

/// What is wrong now, in the words of the bot.
#[derive(Debug, Clone, PartialEq)]
pub struct Problem {
    pub key: Key,
    pub body: String,
    /// What to say when it is over.
    pub resolved: String,
}

struct Policy {
    /// How long it must last before it is told, so a blink is not.
    confirm: Duration,
    /// How long it must be gone before it is told to be over.
    recover: Duration,
    /// How often to remind while it lasts; `None` for never.
    remind: Option<Duration>,
}

fn policy(key: &Key) -> Policy {
    let seconds = Duration::seconds;
    let hours = Duration::hours;

    match key {
        Key::Output => Policy {
            confirm: seconds(15),
            recover: seconds(30),
            remind: Some(Duration::minutes(15)),
        },
        Key::Stopped => Policy {
            confirm: seconds(30),
            recover: seconds(15),
            remind: Some(hours(2)),
        },
        Key::Relay => Policy {
            confirm: seconds(60),
            recover: seconds(30),
            remind: Some(hours(1)),
        },
        Key::Destination(_) => Policy {
            confirm: seconds(30),
            recover: seconds(30),
            remind: Some(hours(1)),
        },
        Key::Filler | Key::Playlist(_) => Policy {
            confirm: seconds(0),
            recover: seconds(0),
            remind: Some(hours(6)),
        },
        Key::Disk => Policy {
            confirm: seconds(0),
            recover: Duration::minutes(10),
            remind: Some(hours(12)),
        },
    }
}

#[derive(Default)]
struct State {
    bad_since: Option<DateTime<Utc>>,
    ok_since: Option<DateTime<Utc>>,
    /// When it was told.
    alerted: Option<DateTime<Utc>>,
    last_sent: Option<DateTime<Utc>>,
    failures: u32,
    body: String,
    resolved: String,
}

/// Remembers which problems were told, so each is told once.
#[derive(Default)]
pub struct Tracker {
    states: HashMap<Key, State>,
}

impl Tracker {
    /// The problem is there at `now`; nothing is said until it has lasted long
    /// enough. `immediate` is for what was seen as it happened: it skips the wait
    /// and counts as one more failure.
    fn present(&mut self, problem: Problem, now: DateTime<Utc>, immediate: bool) -> Option<Notice> {
        let rule = policy(&problem.key);
        let state = self.states.entry(problem.key.clone()).or_default();

        state.ok_since = None;
        state.body = problem.body;
        state.resolved = problem.resolved;

        if immediate {
            state.failures += 1;
        }

        let bad_since =
            *state
                .bad_since
                .get_or_insert(if immediate { now - rule.confirm } else { now });

        let Some(alerted) = state.alerted else {
            if now - bad_since < rule.confirm {
                return None;
            }

            state.alerted = Some(now);
            state.last_sent = Some(now);

            return Some(Notice {
                kind: NoticeKind::Raised,
                key: problem.key,
                body: state.body.clone(),
                lasted: Duration::zero(),
                failures: state.failures,
            });
        };

        let every = rule.remind?;

        if now - state.last_sent.unwrap_or(alerted) < every {
            return None;
        }

        state.last_sent = Some(now);

        Some(Notice {
            kind: NoticeKind::Reminder,
            key: problem.key,
            body: state.body.clone(),
            lasted: now - alerted,
            failures: state.failures,
        })
    }

    /// Seen by looking, from time to time.
    pub fn observe(&mut self, problem: Problem, now: DateTime<Utc>) -> Option<Notice> {
        self.present(problem, now, false)
    }

    /// Seen as it happened.
    pub fn happened(&mut self, problem: Problem, now: DateTime<Utc>) -> Option<Notice> {
        self.present(problem, now, true)
    }

    /// The problem is not there at `now`.
    pub fn absent(&mut self, key: &Key, now: DateTime<Utc>) -> Option<Notice> {
        let state = self.states.get_mut(key)?;

        state.bad_since = None;

        let Some(alerted) = state.alerted else {
            // Never told, so there is nothing to say it is over.
            self.states.remove(key);

            return None;
        };
        let ok_since = *state.ok_since.get_or_insert(now);

        if now - ok_since < policy(key).recover {
            return None;
        }

        let state = self.states.remove(key)?;

        Some(Notice {
            kind: NoticeKind::Recovered,
            key: key.clone(),
            body: state.resolved,
            lasted: now - alerted,
            failures: state.failures,
        })
    }

    /// Stop following the problems the group chose not to hear about.
    fn retain(&mut self, keep: impl Fn(&Key) -> bool) {
        self.states.retain(|key, _| keep(key));
    }

    fn tracked(&self) -> Vec<Key> {
        self.states.keys().cloned().collect()
    }
}

fn day_label(date: NaiveDate, air_date: NaiveDate) -> String {
    let text = messages::date_text(date);

    if date == air_date {
        format!("hoje ({text})")
    } else if date == air_date + Duration::days(1) {
        format!("amanhã ({text})")
    } else {
        text
    }
}

/// What is wrong with the channel in this picture.
pub fn problems(snapshot: &Snapshot, settings: &TelegramSettings) -> Vec<Problem> {
    let mut found = Vec::new();

    if !snapshot.alive {
        found.push(if snapshot.active {
            Problem {
                key: Key::Output,
                body: "A transmissão caiu. O sistema está tentando reiniciar sozinho.".to_string(),
                resolved: "O canal voltou ao ar.".to_string(),
            }
        } else {
            Problem {
                key: Key::Stopped,
                body: "O canal está desligado e não está transmitindo.".to_string(),
                resolved: "O canal voltou a transmitir.".to_string(),
            }
        });
    }

    if snapshot.relay.configured {
        if snapshot.relay.reachable {
            for destination in &snapshot.relay.destinations {
                if destination.state != ConnectionState::Disconnected {
                    continue;
                }

                let name = esc(&destination.name);
                let protocol = match destination.protocol {
                    RelayProtocol::Rtmp => "RTMP",
                    RelayProtocol::Srt => "SRT",
                };

                found.push(Problem {
                    key: Key::Destination(destination.id),
                    body: format!("<b>{name}</b> ({protocol}) não está puxando o sinal."),
                    resolved: format!("<b>{name}</b> voltou a puxar o sinal."),
                });
            }
        } else {
            found.push(Problem {
                key: Key::Relay,
                body:
                    "Não consegui consultar o relay, então não sei se os destinos estão conectados."
                        .to_string(),
                resolved: "O relay voltou a responder.".to_string(),
            });
        }
    }

    if !snapshot.filler.ok {
        found.push(Problem {
            key: Key::Filler,
            body: format!(
                "O arquivo de reserva (<code>{}</code>) não foi encontrado. Se um clipe falhar, a tela fica preta.",
                esc(&snapshot.filler.path)
            ),
            resolved: "O arquivo de reserva está no lugar.".to_string(),
        });
    }

    if snapshot.playlist_mode {
        for date in &snapshot.missing_days {
            let label = day_label(*date, snapshot.air_date);

            found.push(Problem {
                key: Key::Playlist(*date),
                body: format!("Não há playlist para <b>{label}</b>. O canal vai tocar o filler."),
                resolved: format!("A playlist de {label} foi criada."),
            });
        }
    }

    if let Some(disk) = snapshot.disk
        && disk.free_percent() < DISK_LOW_PERCENT
    {
        let free_gb = disk.free as f64 / 1e9;

        found.push(Problem {
            key: Key::Disk,
            body: format!(
                "Restam <b>{:.0}%</b> livres no disco do canal ({free_gb:.1} GB).",
                disk.free_percent()
            ),
            resolved: "O disco voltou a ter espaço livre.".to_string(),
        });
    }

    found.retain(|problem| problem.key.enabled(settings));

    found
}

/// Compare this picture with what was told before and say what changed.
pub fn evaluate(
    tracker: &mut Tracker,
    snapshot: &Snapshot,
    settings: &TelegramSettings,
) -> Vec<Notice> {
    let now = snapshot.now;
    let found = problems(snapshot, settings);
    let present: HashSet<Key> = found.iter().map(|problem| problem.key.clone()).collect();
    let mut notices: Vec<Notice> = Vec::new();

    tracker.retain(|key| key.enabled(settings));

    for problem in found {
        notices.extend(tracker.observe(problem, now));
    }

    for key in tracker.tracked() {
        if !present.contains(&key) {
            notices.extend(tracker.absent(&key, now));
        }
    }

    notices
}

/// The spots of the day that did not go to the air and were not told yet.
fn new_failures<'a>(rows: &'a [Airing], told: &mut HashSet<i32>) -> Vec<&'a Airing> {
    rows.iter()
        .filter(|row| row.kind == AirKind::Spot)
        .filter(|row| matches!(row.result, AirResult::Fallback | AirResult::Failed))
        .filter(|row| told.insert(row.id))
        .collect()
}

fn failures_message(rows: &[&Airing], snapshot: &Snapshot) -> String {
    let mut text = if rows.len() == 1 {
        "⚠️ <b>Um comercial não foi ao ar</b>\n".to_string()
    } else {
        format!("⚠️ <b>{} comerciais não foram ao ar</b>\n", rows.len())
    };

    for row in rows.iter().take(MAX_LISTED_SPOTS) {
        let when = row
            .planned_start
            .as_deref()
            .and_then(|value| DateTime::parse_from_rfc3339(value).ok())
            .map(|value| messages::short_clock(&value.with_timezone(&snapshot.timezone)))
            .unwrap_or_default();
        let reason = if row.reason.is_empty() {
            String::new()
        } else {
            format!(" — <i>{}</i>", esc(&truncate(&row.reason, 120)))
        };

        text.push_str(&format!(
            "• {when} {} · {}{reason}\n",
            esc(&row.advertiser_name),
            esc(&row.spot_title)
        ));
    }

    if rows.len() > MAX_LISTED_SPOTS {
        text.push_str(&format!("e mais {}\n", rows.len() - MAX_LISTED_SPOTS));
    }

    text.push_str("O filler foi ao ar no lugar.\n");
    text.push_str(&format!(
        "<i>{} · {}</i>",
        esc(&snapshot.name),
        messages::clock_text(&snapshot.local(snapshot.now))
    ));

    text
}

/// How the spots of a day went.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct Counts {
    pub planned: usize,
    pub played: usize,
    /// Still to come.
    pub pending: usize,
    pub airing: usize,
    /// Did not go to the air because of a live broadcast.
    pub live: usize,
    /// Cut short, failed, replaced by the filler, never started or interrupted.
    pub problems: usize,
    /// Skipped by the operator.
    pub skipped: usize,
}

pub fn count_airings(rows: &[Airing]) -> Counts {
    let mut counts = Counts::default();

    for row in rows.iter().filter(|row| row.kind == AirKind::Spot) {
        counts.planned += 1;

        match row.result {
            AirResult::Played => counts.played += 1,
            AirResult::Pending => counts.pending += 1,
            AirResult::Airing => counts.airing += 1,
            AirResult::LiveMissed => counts.live += 1,
            AirResult::Skipped => counts.skipped += 1,
            AirResult::Partial
            | AirResult::Interrupted
            | AirResult::Fallback
            | AirResult::Failed
            | AirResult::NotAired => counts.problems += 1,
        }
    }

    counts
}

/// The line about the commercials of a day, for the summary and the commands.
pub fn counts_text(counts: &Counts) -> String {
    if counts.planned == 0 {
        return "nenhum comercial previsto".to_string();
    }

    let mut text = format!("{} de {} veiculados", counts.played, counts.planned);

    for (number, what) in [
        (counts.pending + counts.airing, "ainda por vir"),
        (counts.problems, "com problema"),
        (counts.skipped, "pulados"),
        (counts.live, "perdidos por causa do ao vivo"),
    ] {
        if number > 0 {
            text.push_str(&format!(" · {number} {what}"));
        }
    }

    text
}

pub(super) async fn day_airings(pool: &SqlitePool, channel: i32, date: NaiveDate) -> Vec<Airing> {
    let day = date.format("%Y-%m-%d").to_string();
    let filter = AiringFilter {
        date_from: Some(day.clone()),
        date_to: Some(day),
        campaign_id: None,
        advertiser_id: None,
        result: None,
        kind: Some(AirKind::Spot),
        limit: 5000,
    };

    handles::select_airings(pool, channel, &filter)
        .await
        .unwrap_or_default()
}

/// The morning message: how the last day went and how the next ones look.
pub async fn summary_message(pool: &SqlitePool, snapshot: &Snapshot) -> String {
    let day = snapshot.air_date - Duration::days(1);
    let counts = count_airings(&day_airings(pool, snapshot.channel_id, day).await);
    let connected = snapshot
        .relay
        .destinations
        .iter()
        .filter(|destination| destination.state == ConnectionState::Connected)
        .count();
    let mut text = format!(
        "📋 <b>Resumo de {}</b>\n{}\n\n",
        messages::date_text(day),
        esc(&snapshot.name)
    );

    text.push_str(&format!("Comerciais: {}\n", counts_text(&counts)));

    if snapshot.relay.configured && snapshot.relay.reachable {
        text.push_str(&format!(
            "Destinos agora: {connected} de {} conectados\n",
            snapshot.relay.destinations.len()
        ));
    }

    if snapshot.playlist_mode {
        let playlists = [snapshot.air_date, snapshot.air_date + Duration::days(1)]
            .iter()
            .map(|date| {
                let mark = if snapshot.missing_days.contains(date) {
                    "❌"
                } else {
                    "✅"
                };

                format!("{} {mark}", day_label(*date, snapshot.air_date))
            })
            .collect::<Vec<_>>()
            .join(" · ");

        text.push_str(&format!("Playlists: {playlists}\n"));
    }

    if let Some(disk) = snapshot.disk {
        text.push_str(&format!("Disco livre: {:.0}%\n", disk.free_percent()));
    }

    text
}

/// What the monitor remembers of a channel from one look to the next.
#[derive(Default)]
struct ChannelState {
    tracker: Tracker,
    /// The spots already told as failed; `None` until the first look, which
    /// only learns what is already there.
    told_failures: Option<HashSet<i32>>,
    last_summary: Option<NaiveDate>,
}

struct Monitor {
    pool: SqlitePool,
    controller: Arc<RwLock<ChannelController>>,
    system: SystemStat,
    channels: HashMap<i32, ChannelState>,
    ticks: u64,
}

impl Monitor {
    async fn send(&self, client: &TelegramClient, settings: &TelegramSettings, text: &str) {
        let Some(chat) = settings.chat() else {
            return;
        };

        if let Err(error) = client.send_message(chat, text).await {
            warn!(target: "telegram", "Cannot send the message of channel {}: {error}", settings.channel_id);
        }
    }

    async fn look(&mut self, client: &TelegramClient) {
        self.ticks += 1;

        let Ok(all) = handles::select_enabled_telegram_settings(&self.pool).await else {
            return;
        };
        let wanted: HashSet<i32> = all.iter().map(|settings| settings.channel_id).collect();

        self.channels.retain(|id, _| wanted.contains(id));

        for settings in all {
            let Some(manager) = self.controller.read().await.get(settings.channel_id) else {
                continue;
            };
            let snapshot = collect(&manager, &self.pool, &self.system, 0).await;
            let state = self.channels.entry(settings.channel_id).or_default();
            let notices = evaluate(&mut state.tracker, &snapshot, &settings);

            for notice in notices {
                let at = snapshot.local(snapshot.now);

                self.send(
                    client,
                    &settings,
                    &messages::alert(&notice, &snapshot.name, &at),
                )
                .await;
            }

            // The first look is one of them, so what already failed is learned at once.
            if settings.alert_commercials && (self.ticks - 1).is_multiple_of(COMMERCIAL_EVERY_TICKS)
            {
                self.look_at_commercials(client, &settings, &snapshot).await;
            }

            if settings.daily_summary {
                self.send_summary(client, &settings, &snapshot).await;
            }
        }
    }

    async fn look_at_commercials(
        &mut self,
        client: &TelegramClient,
        settings: &TelegramSettings,
        snapshot: &Snapshot,
    ) {
        let mut rows = day_airings(&self.pool, settings.channel_id, snapshot.air_date).await;

        rows.extend(
            day_airings(
                &self.pool,
                settings.channel_id,
                snapshot.air_date - Duration::days(1),
            )
            .await,
        );

        let state = self.channels.entry(settings.channel_id).or_default();
        let first_look = state.told_failures.is_none();
        let told = state.told_failures.get_or_insert_with(HashSet::new);
        let failed = new_failures(&rows, told);

        if first_look || failed.is_empty() {
            return;
        }

        let text = failures_message(&failed, snapshot);

        self.send(client, settings, &text).await;
    }

    async fn send_summary(
        &mut self,
        client: &TelegramClient,
        settings: &TelegramSettings,
        snapshot: &Snapshot,
    ) {
        let local = snapshot.local(snapshot.now);
        let today = local.date_naive();
        let due = local.time() >= settings.summary_clock();
        let state = self.channels.entry(settings.channel_id).or_default();

        if !due || state.last_summary == Some(today) {
            return;
        }

        let first_look = state.last_summary.is_none();

        state.last_summary = Some(today);

        // Started after the hour of the summary: it was already due today, and
        // a restart must not say it twice.
        if first_look && local.time() - settings.summary_clock() > Duration::minutes(5) {
            return;
        }

        let text = summary_message(&self.pool, snapshot).await;

        self.send(client, settings, &text).await;
    }

    /// The output of a channel failed: say it as it happens.
    async fn output_failed(&mut self, client: &TelegramClient, channel: i32, reason: &str) {
        let Ok(settings) = handles::select_telegram_settings(&self.pool, channel).await else {
            return;
        };

        if !settings.enabled || !settings.alert_output || settings.chat().is_none() {
            return;
        }

        let Some(manager) = self.controller.read().await.get(channel) else {
            return;
        };
        let name = manager.channel.lock().await.name.clone();
        let timezone = manager
            .channel
            .lock()
            .await
            .timezone
            .unwrap_or(chrono_tz::Tz::UTC);
        let now = Utc::now();
        let problem = Problem {
            key: Key::Output,
            body: format!(
                "A transmissão caiu e o sistema está tentando reiniciar sozinho.\nMotivo: <code>{}</code>",
                esc(&truncate(reason, 200))
            ),
            resolved: "O canal voltou ao ar.".to_string(),
        };
        let state = self.channels.entry(channel).or_default();

        if let Some(notice) = state.tracker.happened(problem, now) {
            let at = now.with_timezone(&timezone);

            self.send(client, &settings, &messages::alert(&notice, &name, &at))
                .await;
        }
    }
}

/// Look at the channels and listen to the playout until `shutdown`.
pub async fn run_alerts(
    pool: SqlitePool,
    controller: Arc<RwLock<ChannelController>>,
    system: SystemStat,
    shutdown: CancellationToken,
) {
    select! {
        () = shutdown.cancelled() => return,
        () = sleep(FIRST_DELAY) => {}
    }

    let mut monitor = Monitor {
        pool: pool.clone(),
        controller,
        system,
        channels: HashMap::new(),
        ticks: 0,
    };
    let mut events = events::subscribe();
    let mut ticker = interval(TICK);

    ticker.set_missed_tick_behavior(MissedTickBehavior::Delay);

    loop {
        select! {
            () = shutdown.cancelled() => break,
            _ = ticker.tick() => {
                let Ok(token) = handles::select_telegram_token(&pool).await else {
                    continue;
                };

                if !token.is_empty() {
                    monitor.look(&TelegramClient::new(&token)).await;
                }
            }
            event = events.recv() => match event {
                Ok(Event::OutputFailed { channel, reason }) => {
                    let Ok(token) = handles::select_telegram_token(&pool).await else {
                        continue;
                    };

                    if !token.is_empty() {
                        monitor.output_failed(&TelegramClient::new(&token), channel, &reason).await;
                    }
                }
                // Missed some while busy: the next look sees the state anyway.
                Err(RecvError::Lagged(_)) => {}
                Err(RecvError::Closed) => break,
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use chrono::TimeZone;
    use chrono_tz::Tz;

    use super::*;
    use crate::utils::{
        health::{DestinationStatus, FillerStatus, RelayStatus},
        telegram::snapshot::Disk,
    };

    fn at(seconds: i64) -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 10, 15, 12, 0, 0).unwrap() + Duration::seconds(seconds)
    }

    fn day(date: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(2026, 10, date).unwrap()
    }

    fn problem(key: Key) -> Problem {
        Problem {
            key,
            body: "quebrou".to_string(),
            resolved: "consertou".to_string(),
        }
    }

    /// A channel with nothing wrong.
    fn healthy() -> Snapshot {
        Snapshot {
            channel_id: 1,
            name: "TV Praia".to_string(),
            timezone: Tz::America__Sao_Paulo,
            now: at(0),
            active: true,
            alive: true,
            playlist_mode: true,
            current: None,
            upcoming: Vec::new(),
            live: false,
            filler: FillerStatus {
                path: "filler.mp4".to_string(),
                ok: true,
            },
            relay: RelayStatus {
                configured: true,
                reachable: true,
                destinations: vec![DestinationStatus {
                    id: 7,
                    name: "Praia <FM>".to_string(),
                    protocol: RelayProtocol::Rtmp,
                    state: ConnectionState::Connected,
                }],
                others: 0,
            },
            air_date: day(15),
            missing_days: Vec::new(),
            disk: Some(Disk {
                free: 50,
                total: 100,
            }),
        }
    }

    fn keys(notices: &[Notice]) -> Vec<(NoticeKind, Key)> {
        notices
            .iter()
            .map(|notice| (notice.kind, notice.key.clone()))
            .collect()
    }

    fn at_time(snapshot: &Snapshot, seconds: i64) -> Snapshot {
        Snapshot {
            now: at(seconds),
            ..snapshot.clone()
        }
    }

    #[test]
    fn a_problem_is_told_only_after_it_lasted() {
        let mut tracker = Tracker::default();

        assert!(tracker.observe(problem(Key::Output), at(0)).is_none());
        assert!(tracker.observe(problem(Key::Output), at(14)).is_none());

        let notice = tracker.observe(problem(Key::Output), at(15)).unwrap();

        assert_eq!(notice.kind, NoticeKind::Raised);
        assert_eq!(notice.body, "quebrou");
    }

    #[test]
    fn a_blink_is_never_told() {
        let mut tracker = Tracker::default();

        assert!(tracker.observe(problem(Key::Output), at(0)).is_none());
        assert!(tracker.absent(&Key::Output, at(5)).is_none());
        assert!(tracker.absent(&Key::Output, at(120)).is_none());
        assert!(tracker.observe(problem(Key::Output), at(130)).is_none());
    }

    #[test]
    fn a_problem_that_lasts_is_told_once_and_reminded_later() {
        let mut tracker = Tracker::default();

        assert!(tracker.observe(problem(Key::Filler), at(0)).is_some());

        for seconds in (15..6 * 3600).step_by(15) {
            assert!(
                tracker.observe(problem(Key::Filler), at(seconds)).is_none(),
                "told again after {seconds} s"
            );
        }

        let reminder = tracker.observe(problem(Key::Filler), at(6 * 3600)).unwrap();

        assert_eq!(reminder.kind, NoticeKind::Reminder);
        assert_eq!(reminder.lasted, Duration::hours(6));
        assert!(
            tracker
                .observe(problem(Key::Filler), at(6 * 3600 + 15))
                .is_none()
        );
    }

    #[test]
    fn the_end_of_a_problem_is_told_with_how_long_it_lasted() {
        let mut tracker = Tracker::default();

        tracker.observe(problem(Key::Destination(1)), at(0));
        tracker
            .observe(problem(Key::Destination(1)), at(30))
            .unwrap();

        assert!(tracker.absent(&Key::Destination(1), at(100)).is_none());
        assert!(tracker.absent(&Key::Destination(1), at(120)).is_none());

        let notice = tracker.absent(&Key::Destination(1), at(130)).unwrap();

        assert_eq!(notice.kind, NoticeKind::Recovered);
        assert_eq!(notice.body, "consertou");
        assert_eq!(notice.lasted, Duration::seconds(100));
        assert!(tracker.absent(&Key::Destination(1), at(200)).is_none());
    }

    #[test]
    fn a_problem_that_comes_back_while_recovering_is_the_same_problem() {
        let mut tracker = Tracker::default();

        tracker.observe(problem(Key::Destination(1)), at(0));
        tracker
            .observe(problem(Key::Destination(1)), at(30))
            .unwrap();
        tracker.absent(&Key::Destination(1), at(60));

        assert!(
            tracker
                .observe(problem(Key::Destination(1)), at(80))
                .is_none()
        );
        assert!(tracker.absent(&Key::Destination(1), at(100)).is_none());
        assert!(tracker.absent(&Key::Destination(1), at(129)).is_none());
        assert!(tracker.absent(&Key::Destination(1), at(130)).is_some());
    }

    #[test]
    fn what_was_seen_as_it_happened_is_told_at_once_and_counted() {
        let mut tracker = Tracker::default();
        let first = tracker.happened(problem(Key::Output), at(0)).unwrap();

        assert_eq!(first.kind, NoticeKind::Raised);
        assert_eq!(first.failures, 1);
        assert!(tracker.happened(problem(Key::Output), at(10)).is_none());
        assert!(tracker.happened(problem(Key::Output), at(20)).is_none());

        // Quiet again, but not for long enough.
        assert!(tracker.absent(&Key::Output, at(25)).is_none());
        assert!(tracker.absent(&Key::Output, at(50)).is_none());

        // Another failure starts the quiet time over.
        assert!(tracker.happened(problem(Key::Output), at(52)).is_none());
        assert!(tracker.absent(&Key::Output, at(55)).is_none());
        assert!(tracker.absent(&Key::Output, at(84)).is_none());

        let done = tracker.absent(&Key::Output, at(85)).unwrap();

        assert_eq!(done.kind, NoticeKind::Recovered);
        assert_eq!(done.failures, 4);
        assert_eq!(done.lasted, Duration::seconds(85));
    }

    #[test]
    fn a_healthy_channel_says_nothing() {
        let mut tracker = Tracker::default();
        let settings = TelegramSettings::default();

        for seconds in (0..600).step_by(15) {
            let notices = evaluate(&mut tracker, &at_time(&healthy(), seconds), &settings);

            assert!(notices.is_empty(), "{notices:?}");
        }
    }

    #[test]
    fn a_destination_that_drops_is_told_then_told_back() {
        let mut tracker = Tracker::default();
        let settings = TelegramSettings::default();
        let mut snapshot = healthy();

        snapshot.relay.destinations[0].state = ConnectionState::Disconnected;

        assert!(evaluate(&mut tracker, &at_time(&snapshot, 0), &settings).is_empty());

        let raised = evaluate(&mut tracker, &at_time(&snapshot, 30), &settings);

        assert_eq!(keys(&raised), [(NoticeKind::Raised, Key::Destination(7))]);
        assert!(
            raised[0].body.contains("Praia &lt;FM&gt;"),
            "{}",
            raised[0].body
        );

        snapshot.relay.destinations[0].state = ConnectionState::Connected;

        assert!(evaluate(&mut tracker, &at_time(&snapshot, 45), &settings).is_empty());

        let back = evaluate(&mut tracker, &at_time(&snapshot, 80), &settings);

        assert_eq!(keys(&back), [(NoticeKind::Recovered, Key::Destination(7))]);
    }

    #[test]
    fn an_unreachable_relay_is_one_problem_not_one_per_destination() {
        let settings = TelegramSettings::default();
        let mut snapshot = healthy();

        snapshot.relay.reachable = false;
        snapshot.relay.destinations[0].state = ConnectionState::Unknown;

        let found = problems(&snapshot, &settings);

        assert_eq!(found.len(), 1);
        assert_eq!(found[0].key, Key::Relay);
    }

    #[test]
    fn a_channel_without_a_relay_has_no_destination_problems() {
        let mut snapshot = healthy();

        snapshot.relay.configured = false;
        snapshot.relay.reachable = false;

        assert!(problems(&snapshot, &TelegramSettings::default()).is_empty());
    }

    #[test]
    fn a_stopped_channel_is_not_a_crash() {
        let mut snapshot = healthy();

        snapshot.alive = false;

        let crashed = problems(&snapshot, &TelegramSettings::default());

        assert_eq!(crashed[0].key, Key::Output);

        snapshot.active = false;

        let stopped = problems(&snapshot, &TelegramSettings::default());

        assert_eq!(stopped[0].key, Key::Stopped);
    }

    #[test]
    fn missing_playlists_say_today_or_tomorrow() {
        let mut snapshot = healthy();

        snapshot.missing_days = vec![day(15), day(16)];

        let found = problems(&snapshot, &TelegramSettings::default());

        assert_eq!(found.len(), 2);
        assert!(
            found[0].body.contains("hoje (15/10/2026)"),
            "{}",
            found[0].body
        );
        assert!(
            found[1].body.contains("amanhã (16/10/2026)"),
            "{}",
            found[1].body
        );
    }

    #[test]
    fn a_channel_that_does_not_play_playlists_has_no_playlist_problem() {
        let mut snapshot = healthy();

        snapshot.playlist_mode = false;
        snapshot.missing_days = vec![day(15)];

        assert!(problems(&snapshot, &TelegramSettings::default()).is_empty());
    }

    #[test]
    fn the_disk_is_a_problem_below_ten_percent() {
        let mut snapshot = healthy();

        snapshot.disk = Some(Disk {
            free: 10,
            total: 100,
        });
        assert!(problems(&snapshot, &TelegramSettings::default()).is_empty());

        snapshot.disk = Some(Disk {
            free: 9,
            total: 100,
        });

        let found = problems(&snapshot, &TelegramSettings::default());

        assert_eq!(found[0].key, Key::Disk);
        assert!(found[0].body.contains("9%"), "{}", found[0].body);
    }

    #[test]
    fn the_filler_is_a_problem_when_it_is_missing() {
        let mut snapshot = healthy();

        snapshot.filler.ok = false;

        assert_eq!(
            problems(&snapshot, &TelegramSettings::default())[0].key,
            Key::Filler
        );
    }

    #[test]
    fn alerts_the_group_turned_off_are_not_told_nor_followed() {
        let mut tracker = Tracker::default();
        let mut settings = TelegramSettings::default();
        let mut snapshot = healthy();

        snapshot.filler.ok = false;
        snapshot.disk = Some(Disk {
            free: 1,
            total: 100,
        });
        settings.alert_filler = false;

        let notices = evaluate(&mut tracker, &snapshot, &settings);

        assert_eq!(keys(&notices), [(NoticeKind::Raised, Key::Disk)]);

        // Turned off while it lasts: nothing more is said, not even the end.
        settings.alert_disk = false;
        snapshot.disk = Some(Disk {
            free: 50,
            total: 100,
        });

        assert!(evaluate(&mut tracker, &at_time(&snapshot, 3600), &settings).is_empty());
        assert!(tracker.tracked().is_empty());
    }

    #[test]
    fn a_day_that_passes_is_forgotten_with_its_problem() {
        let mut tracker = Tracker::default();
        let settings = TelegramSettings::default();
        let mut snapshot = healthy();

        snapshot.missing_days = vec![day(16)];
        evaluate(&mut tracker, &snapshot, &settings);

        snapshot.missing_days = Vec::new();

        let back = evaluate(&mut tracker, &at_time(&snapshot, 20), &settings);

        assert_eq!(
            keys(&back),
            [(NoticeKind::Recovered, Key::Playlist(day(16)))]
        );
    }

    fn airing(id: i32, result: AirResult) -> Airing {
        Airing {
            id,
            channel_id: 1,
            air_date: "2026-10-15".to_string(),
            kind: AirKind::Spot,
            spot_id: Some(id),
            campaign_id: None,
            advertiser_id: None,
            advertiser_name: "Padaria <Sol>".to_string(),
            campaign_name: String::new(),
            spot_title: format!("Spot {id}"),
            file_path: String::new(),
            break_window_id: None,
            position_in_break: None,
            planned_begin_sec: None,
            planned_start: Some("2026-10-15T15:30:00Z".to_string()),
            planned_duration_sec: None,
            actual_start: None,
            actual_end: None,
            actual_duration_sec: None,
            result,
            reason: "file <missing>".to_string(),
            created_at: String::new(),
        }
    }

    #[test]
    fn the_spots_of_a_day_are_counted_by_how_they_went() {
        let rows = vec![
            airing(1, AirResult::Played),
            airing(2, AirResult::Played),
            airing(3, AirResult::Pending),
            airing(4, AirResult::Fallback),
            airing(5, AirResult::NotAired),
            airing(6, AirResult::Skipped),
            airing(7, AirResult::LiveMissed),
            airing(8, AirResult::Airing),
        ];
        let counts = count_airings(&rows);

        assert_eq!(
            counts,
            Counts {
                planned: 8,
                played: 2,
                pending: 1,
                airing: 1,
                live: 1,
                problems: 2,
                skipped: 1,
            }
        );
        assert_eq!(
            counts_text(&counts),
            "2 de 8 veiculados · 2 ainda por vir · 2 com problema · 1 pulados · 1 perdidos por causa do ao vivo"
        );
        assert_eq!(counts_text(&Counts::default()), "nenhum comercial previsto");
    }

    #[test]
    fn a_failed_spot_is_told_once() {
        let mut told = HashSet::new();
        let rows = vec![
            airing(1, AirResult::Played),
            airing(2, AirResult::Fallback),
            airing(3, AirResult::Failed),
            airing(4, AirResult::Interrupted),
        ];

        assert_eq!(new_failures(&rows, &mut told).len(), 2);
        assert!(new_failures(&rows, &mut told).is_empty());

        let more = [rows, vec![airing(5, AirResult::Failed)]].concat();

        assert_eq!(new_failures(&more, &mut told).len(), 1);
    }

    #[test]
    fn the_failure_message_names_the_spot_and_cannot_break_the_html() {
        let rows = [airing(2, AirResult::Fallback)];
        let refs: Vec<&Airing> = rows.iter().collect();
        let text = failures_message(&refs, &healthy());

        assert!(text.contains("Um comercial não foi ao ar"), "{text}");
        assert!(
            text.contains("12:30 Padaria &lt;Sol&gt; · Spot 2"),
            "{text}"
        );
        assert!(text.contains("file &lt;missing&gt;"), "{text}");
        assert!(text.contains("TV Praia"), "{text}");
    }

    #[test]
    fn a_long_list_of_failures_is_cut() {
        let rows: Vec<Airing> = (1..=14).map(|id| airing(id, AirResult::Failed)).collect();
        let refs: Vec<&Airing> = rows.iter().collect();
        let text = failures_message(&refs, &healthy());

        assert!(text.contains("14 comerciais não foram ao ar"), "{text}");
        assert!(text.contains("e mais 4"), "{text}");
        assert_eq!(text.matches('•').count(), MAX_LISTED_SPOTS);
    }

    #[test]
    fn alerts_read_well() {
        let at_noon = at(0).with_timezone(&Tz::America__Sao_Paulo);
        let raised = Notice {
            kind: NoticeKind::Raised,
            key: Key::Output,
            body: "A transmissão caiu.".to_string(),
            lasted: Duration::zero(),
            failures: 1,
        };
        let text = messages::alert(&raised, "TV <Praia>", &at_noon);

        assert_eq!(
            text,
            "🔴 <b>Saída fora do ar</b>\nA transmissão caiu.\n<i>TV &lt;Praia&gt; · 09:00:00</i>"
        );

        let warning = Notice {
            key: Key::Filler,
            ..raised.clone()
        };

        assert!(messages::alert(&warning, "TV", &at_noon).starts_with("⚠️"));

        let reminder = Notice {
            kind: NoticeKind::Reminder,
            lasted: Duration::minutes(15),
            ..raised.clone()
        };

        assert!(
            messages::alert(&reminder, "TV", &at_noon)
                .contains("⏰ <b>Saída fora do ar</b> (continua há 15 min 00 s)")
        );

        let recovered = Notice {
            kind: NoticeKind::Recovered,
            body: "O canal voltou ao ar.".to_string(),
            lasted: Duration::seconds(185),
            failures: 3,
            ..raised
        };
        let text = messages::alert(&recovered, "TV", &at_noon);

        assert!(text.starts_with("✅ <b>Saída normalizada</b>"), "{text}");
        assert!(
            text.contains("Ficou assim por 3 min 05 s · 3 falhas"),
            "{text}"
        );
    }

    mod world_tests {
        use super::*;
        use crate::utils::telegram::{
            fake::{Calls, GROUP, TOKEN, sent, world},
            snapshot::collect,
        };

        struct Fixture {
            monitor: Monitor,
            client: TelegramClient,
            calls: Calls,
        }

        async fn fixture() -> Fixture {
            let world = world(Vec::new()).await;

            Fixture {
                client: TelegramClient::with_base(TOKEN, &world.base),
                monitor: Monitor {
                    pool: world.pool,
                    controller: world.controller,
                    system: SystemStat::new(),
                    channels: HashMap::new(),
                    ticks: 0,
                },
                calls: world.calls,
            }
        }

        async fn settings(fixture: &Fixture) -> TelegramSettings {
            handles::select_telegram_settings(&fixture.monitor.pool, 1)
                .await
                .unwrap()
        }

        async fn snapshot(fixture: &Fixture) -> Snapshot {
            let manager = fixture.monitor.controller.read().await.get(1).unwrap();

            collect(&manager, &fixture.monitor.pool, &fixture.monitor.system, 0).await
        }

        async fn insert_airing(fixture: &Fixture, id: i32, date: &str, result: &str) {
            sqlx::query(
                "INSERT INTO airlog (id, channel_id, air_date, kind, advertiser_name, spot_title,
                    planned_start, result, reason)
                VALUES ($1, 1, $2, 'spot', 'Padaria', $3, '2026-10-15T15:30:00Z', $4, 'file gone')",
            )
            .bind(id)
            .bind(date)
            .bind(format!("Spot {id}"))
            .bind(result)
            .execute(&fixture.monitor.pool)
            .await
            .unwrap();
        }

        #[tokio::test]
        async fn a_failure_of_the_output_is_told_at_once_with_its_reason() {
            let mut fixture = fixture().await;

            fixture
                .monitor
                .output_failed(&fixture.client, 1, "encoder <died>")
                .await;

            let sent = sent(&fixture.calls);

            assert_eq!(sent.len(), 1);
            assert_eq!(sent[0].0, GROUP);
            assert!(
                sent[0].1.starts_with("🔴 <b>Saída fora do ar</b>"),
                "{}",
                sent[0].1
            );
            assert!(sent[0].1.contains("encoder &lt;died&gt;"), "{}", sent[0].1);

            // The same incident is not told again while it lasts.
            fixture
                .monitor
                .output_failed(&fixture.client, 1, "encoder died again")
                .await;

            assert_eq!(sent_count(&fixture.calls), 1);
        }

        fn sent_count(calls: &Calls) -> usize {
            sent(calls).len()
        }

        #[tokio::test]
        async fn a_group_that_turned_the_output_alert_off_hears_nothing() {
            let mut fixture = fixture().await;
            let mut settings = settings(&fixture).await;

            settings.alert_output = false;
            handles::replace_telegram_settings(&fixture.monitor.pool, 1, &settings)
                .await
                .unwrap();
            fixture
                .monitor
                .output_failed(&fixture.client, 1, "boom")
                .await;

            settings.alert_output = true;
            settings.enabled = false;
            handles::replace_telegram_settings(&fixture.monitor.pool, 1, &settings)
                .await
                .unwrap();
            fixture
                .monitor
                .output_failed(&fixture.client, 1, "boom")
                .await;

            assert_eq!(sent_count(&fixture.calls), 0);
        }

        #[tokio::test]
        async fn a_look_tells_what_is_wrong_once() {
            let mut fixture = fixture().await;

            fixture.monitor.look(&fixture.client).await;
            fixture.monitor.look(&fixture.client).await;

            let texts: Vec<String> = sent(&fixture.calls)
                .into_iter()
                .map(|(_, text)| text)
                .collect();

            // The test channel has no filler file: that is told at once and only once.
            assert_eq!(
                texts
                    .iter()
                    .filter(|text| text.contains("Arquivo de reserva ausente"))
                    .count(),
                1,
                "{texts:?}"
            );
        }

        #[tokio::test]
        async fn the_first_look_learns_the_failed_spots_and_the_next_tells_the_new_ones() {
            let mut fixture = fixture().await;
            let settings = settings(&fixture).await;
            let snapshot = snapshot(&fixture).await;
            let date = snapshot.air_date.format("%Y-%m-%d").to_string();

            insert_airing(&fixture, 1, &date, "failed").await;
            fixture
                .monitor
                .look_at_commercials(&fixture.client, &settings, &snapshot)
                .await;

            assert_eq!(
                sent_count(&fixture.calls),
                0,
                "what was already there is old news"
            );

            insert_airing(&fixture, 2, &date, "played").await;
            insert_airing(&fixture, 3, &date, "fallback").await;
            fixture
                .monitor
                .look_at_commercials(&fixture.client, &settings, &snapshot)
                .await;

            let texts = sent(&fixture.calls);

            assert_eq!(texts.len(), 1);
            assert!(
                texts[0].1.contains("Um comercial não foi ao ar"),
                "{}",
                texts[0].1
            );
            assert!(texts[0].1.contains("Spot 3"), "{}", texts[0].1);
            assert!(!texts[0].1.contains("Spot 1"), "{}", texts[0].1);

            fixture
                .monitor
                .look_at_commercials(&fixture.client, &settings, &snapshot)
                .await;

            assert_eq!(sent_count(&fixture.calls), 1, "told once");
        }

        fn at_local(snapshot: &Snapshot, day: u32, hour: u32, minute: u32) -> Snapshot {
            let local = snapshot
                .timezone
                .with_ymd_and_hms(2026, 10, day, hour, minute, 0)
                .unwrap();

            Snapshot {
                now: local.with_timezone(&Utc),
                air_date: day_of(day),
                ..snapshot.clone()
            }
        }

        fn day_of(day: u32) -> NaiveDate {
            NaiveDate::from_ymd_opt(2026, 10, day).unwrap()
        }

        #[tokio::test]
        async fn the_summary_comes_once_a_day_after_its_hour() {
            let mut fixture = fixture().await;
            let settings = settings(&fixture).await;
            let base = snapshot(&fixture).await;

            insert_airing(&fixture, 1, "2026-10-14", "played").await;
            insert_airing(&fixture, 2, "2026-10-14", "played").await;
            insert_airing(&fixture, 3, "2026-10-14", "fallback").await;

            // Before the hour: nothing.
            fixture
                .monitor
                .send_summary(&fixture.client, &settings, &at_local(&base, 15, 7, 59))
                .await;

            assert_eq!(sent_count(&fixture.calls), 0);

            fixture
                .monitor
                .send_summary(&fixture.client, &settings, &at_local(&base, 15, 8, 2))
                .await;

            let texts = sent(&fixture.calls);

            assert_eq!(texts.len(), 1);
            assert!(
                texts[0].1.contains("Resumo de 14/10/2026"),
                "{}",
                texts[0].1
            );
            assert!(
                texts[0]
                    .1
                    .contains("Comerciais: 2 de 3 veiculados · 1 com problema"),
                "{}",
                texts[0].1
            );

            // The same day, later: not again.
            fixture
                .monitor
                .send_summary(&fixture.client, &settings, &at_local(&base, 15, 12, 0))
                .await;

            assert_eq!(sent_count(&fixture.calls), 1);

            // The next day: again.
            fixture
                .monitor
                .send_summary(&fixture.client, &settings, &at_local(&base, 16, 8, 1))
                .await;

            assert_eq!(sent_count(&fixture.calls), 2);
        }

        #[tokio::test]
        async fn a_restart_after_the_hour_does_not_say_the_summary_twice() {
            let mut fixture = fixture().await;
            let settings = settings(&fixture).await;
            let base = snapshot(&fixture).await;

            fixture
                .monitor
                .send_summary(&fixture.client, &settings, &at_local(&base, 15, 14, 0))
                .await;
            fixture
                .monitor
                .send_summary(&fixture.client, &settings, &at_local(&base, 15, 14, 1))
                .await;

            assert_eq!(sent_count(&fixture.calls), 0);

            fixture
                .monitor
                .send_summary(&fixture.client, &settings, &at_local(&base, 16, 8, 0))
                .await;

            assert_eq!(sent_count(&fixture.calls), 1);
        }
    }
}
