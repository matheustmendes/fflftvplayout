use std::{error::Error, fmt, str::FromStr};

use chrono::{NaiveDate, NaiveTime};
use chrono_tz::Tz;
use regex::Regex;
use serde::{Deserialize, Serialize};
use sqlx::{FromRow, Pool, Row, Sqlite, sqlite::SqliteRow};

use crate::{
    db::handles,
    utils::config::{OutputMode, PlayoutConfig},
};

#[derive(Clone, Default, Debug, Deserialize, Serialize, sqlx::FromRow)]
pub struct GlobalSettings {
    pub id: i32,
    pub secret: Option<String>,
    pub logs: String,
    pub playlists: String,
    pub public: String,
    pub storage: String,
    pub shared: bool,
    pub smtp_server: String,
    pub smtp_user: String,
    pub smtp_password: String,
    pub smtp_starttls: bool,
    pub smtp_port: u16,
    pub notification_server: String,
    pub notification_token: String,
    pub setup_completed: bool,
}

impl GlobalSettings {
    pub async fn new(conn: &Pool<Sqlite>) -> Self {
        let global_settings = handles::select_global(conn);

        match global_settings.await {
            Ok(g) => g,
            Err(_) => Self {
                id: 0,
                secret: None,
                logs: String::new(),
                playlists: String::new(),
                public: String::new(),
                storage: String::new(),
                shared: false,
                smtp_server: String::new(),
                smtp_user: String::new(),
                smtp_password: String::new(),
                smtp_starttls: false,
                smtp_port: 465,
                notification_server: String::new(),
                notification_token: String::new(),
                setup_completed: true,
            },
        }
    }
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub struct Channel {
    #[serde(default = "default_id", skip_deserializing)]
    pub id: i32,
    pub name: String,
    pub preview_url: String,
    pub extra_extensions: String,
    pub active: bool,
    pub public: String,
    pub playlists: String,
    pub storage: String,
    pub last_date: Option<String>,
    pub time_shift: f64,
    #[serde(default)]
    pub timezone: Option<Tz>,
}

impl FromRow<'_, SqliteRow> for Channel {
    fn from_row(row: &SqliteRow) -> sqlx::Result<Self> {
        let mut timezone = None;

        if let Some(tz) = row
            .try_get::<String, _>("timezone")
            .ok()
            .and_then(|t: String| Tz::from_str(&t).ok())
        {
            timezone = Some(tz);
        } else if let Some(tz) = iana_time_zone::get_timezone()
            .ok()
            .and_then(|t: String| Tz::from_str(&t).ok())
        {
            timezone = Some(tz);
        }

        Ok(Self {
            id: row.try_get("id").unwrap_or_default(),
            name: row.try_get("name").unwrap_or_default(),
            preview_url: row.try_get("preview_url").unwrap_or_default(),
            extra_extensions: row.try_get("extra_extensions").unwrap_or_default(),
            active: row.try_get("active").unwrap_or_default(),
            public: row.try_get("public").unwrap_or_default(),
            playlists: row.try_get("playlists").unwrap_or_default(),
            storage: row.try_get("storage").unwrap_or_default(),
            last_date: row.try_get("last_date").unwrap_or_default(),
            time_shift: row.try_get("time_shift").unwrap_or_default(),
            timezone,
        })
    }
}

fn default_id() -> i32 {
    1
}

fn default_true() -> bool {
    true
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub struct User {
    #[serde(skip_deserializing)]
    pub id: i32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mail: Option<String>,
    pub username: String,
    #[serde(skip_serializing, default = "String::new")]
    pub password: String,
    pub role_id: Option<i32>,
    pub channel_ids: Option<Vec<i32>>,
    #[serde(default = "default_true")]
    pub two_factor: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub token: Option<String>,
}

impl FromRow<'_, SqliteRow> for User {
    fn from_row(row: &SqliteRow) -> sqlx::Result<Self> {
        Ok(Self {
            id: row.try_get("id").unwrap_or_default(),
            mail: row.try_get("mail").ok(),
            username: row.try_get("username").unwrap_or_default(),
            password: row.try_get("password").unwrap_or_default(),
            role_id: row.try_get("role_id").ok(),
            channel_ids: Some(
                row.try_get::<String, &str>("channel_ids")
                    .unwrap_or_default()
                    .split(',')
                    .map(|i| i.parse::<i32>().unwrap_or_default())
                    .collect(),
            ),
            two_factor: row.try_get("two_factor").unwrap_or(true),
            token: None,
        })
    }
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct UserMeta {
    pub id: i32,
    pub channels: Vec<i32>,
}

impl UserMeta {
    pub fn new(id: i32, channels: Vec<i32>) -> Self {
        Self { id, channels }
    }
}

#[derive(Clone, Debug, Default, Eq, Hash, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Role {
    GlobalAdmin,
    ChannelAdmin,
    User,
    #[default]
    Guest,
}

impl FromStr for Role {
    type Err = String;

    fn from_str(input: &str) -> Result<Self, Self::Err> {
        match input {
            "global_admin" => Ok(Self::GlobalAdmin),
            "channel_admin" => Ok(Self::ChannelAdmin),
            "user" => Ok(Self::User),
            _ => Ok(Self::Guest),
        }
    }
}

impl fmt::Display for Role {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match *self {
            Self::GlobalAdmin => write!(f, "global_admin"),
            Self::ChannelAdmin => write!(f, "channel_admin"),
            Self::User => write!(f, "user"),
            Self::Guest => write!(f, "guest"),
        }
    }
}

impl<'r> sqlx::decode::Decode<'r, ::sqlx::Sqlite> for Role
where
    &'r str: sqlx::decode::Decode<'r, sqlx::Sqlite>,
{
    fn decode(
        value: sqlx::sqlite::SqliteValueRef<'r>,
    ) -> Result<Self, Box<dyn Error + 'static + Send + Sync>> {
        let value = <&str as sqlx::decode::Decode<sqlx::Sqlite>>::decode(value)?;

        Ok(value.parse()?)
    }
}

impl FromRow<'_, SqliteRow> for Role {
    fn from_row(row: &SqliteRow) -> sqlx::Result<Self> {
        match row.get("name") {
            "global_admin" => Ok(Self::GlobalAdmin),
            "channel_admin" => Ok(Self::ChannelAdmin),
            "user" => Ok(Self::User),
            _ => Ok(Self::Guest),
        }
    }
}

#[derive(Debug, Deserialize, Serialize, Clone, sqlx::FromRow)]
#[serde(default)]
pub struct TextPreset {
    #[sqlx(default)]
    #[serde(default)]
    pub id: i32,
    pub channel_id: i32,
    #[sqlx(default)]
    #[serde(default)]
    pub persistent: bool,
    pub name: String,
    pub text: String,
    pub use_filename: bool,
    pub font_family: String,
    pub font_weight: String,
    pub filename_regex: String,
    pub position_x: String,
    pub position_y: String,
    pub font_size: f32,
    pub line_spacing: f32,
    pub text_color: String,
    pub text_opacity: f64,
    pub background_enabled: bool,
    pub background_color: String,
    pub background_opacity: f64,
    pub background_padding: u32,
    pub opacity: f64,
    pub scroll_direction: String,
    pub scroll_speed: u32,
    pub scroll_repeat: i32,
    pub fade_in_seconds: f64,
    pub fade_out_seconds: f64,
}

impl Default for TextPreset {
    fn default() -> Self {
        Self {
            id: 0,
            channel_id: 1,
            persistent: false,
            name: String::new(),
            text: String::new(),
            use_filename: false,
            font_family: "DejaVu Sans".to_string(),
            font_weight: "normal".to_string(),
            filename_regex: r"^.+[/\\](.*)(.mp4|.mkv|.webm)$".to_string(),
            position_x: "center".to_string(),
            position_y: "end:72".to_string(),
            font_size: 24.0,
            line_spacing: 4.0,
            text_color: "#ffffff".to_string(),
            text_opacity: 1.0,
            background_enabled: false,
            background_color: "#000000".to_string(),
            background_opacity: 0.8,
            background_padding: 4,
            opacity: 1.0,
            scroll_direction: "none".to_string(),
            scroll_speed: 100,
            scroll_repeat: -1,
            fade_in_seconds: 0.0,
            fade_out_seconds: 0.0,
        }
    }
}

impl TextPreset {
    pub fn validate(&self) -> Result<(), String> {
        if self.name.trim().is_empty() {
            return Err("text preset name must not be empty".to_string());
        }
        if !self.font_size.is_finite() || self.font_size <= 0.0 {
            return Err("text font size must be a positive number".to_string());
        }
        if self.font_family.trim().is_empty() {
            return Err("text font family must not be empty".to_string());
        }
        if !matches!(self.font_weight.as_str(), "normal" | "semibold" | "bold") {
            return Err("invalid text font weight".to_string());
        }
        if !self.line_spacing.is_finite() || self.line_spacing < 0.0 {
            return Err("text line spacing must not be negative".to_string());
        }
        for (name, value) in [
            ("text opacity", self.text_opacity),
            ("background opacity", self.background_opacity),
            ("opacity", self.opacity),
        ] {
            if !value.is_finite() || !(0.0..=1.0).contains(&value) {
                return Err(format!("{name} must be between 0 and 1"));
            }
        }
        if !self.fade_in_seconds.is_finite() || self.fade_in_seconds < 0.0 {
            return Err("fade-in duration must not be negative".to_string());
        }
        if !self.fade_out_seconds.is_finite() || self.fade_out_seconds < 0.0 {
            return Err("fade-out duration must not be negative".to_string());
        }
        if !matches!(
            self.scroll_direction.as_str(),
            "none" | "left_to_right" | "right_to_left"
        ) {
            return Err("invalid text scroll direction".to_string());
        }
        if self.scroll_direction != "none" && self.scroll_speed == 0 {
            return Err("text scroll speed must be greater than zero".to_string());
        }
        if self.scroll_repeat < -1 {
            return Err("text scroll repeat must be -1 or greater".to_string());
        }
        if self.use_filename {
            Regex::new(&self.filename_regex)
                .map_err(|error| format!("invalid filename regex: {error}"))?;
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Deserialize, Serialize, sqlx::FromRow)]
/// Flattened read model assembled from the thematic `config_*` tables.
///
/// Keeping this projection separate from persistence lets the runtime build a
/// `PlayoutConfig` in one query without coupling writes to one wide table.
pub struct Configuration {
    pub id: i32,
    pub channel_id: i32,
    pub general_stop_threshold: f64,

    pub mail_subject: String,
    pub mail_recipient: String,
    pub mail_level: String,
    pub mail_interval: i64,

    pub notification_topic: String,
    pub notification_level: String,
    pub notification_tags: String,

    pub logging_ffmpeg_level: String,
    pub logging_ingest_level: String,
    pub logging_detect_silence: bool,
    #[serde(default)]
    pub logging_ignore: String,

    pub processing_mode: String,
    pub processing_add_logo: bool,
    pub processing_logo: String,
    pub processing_logo_scale: String,
    pub processing_logo_opacity: f64,
    pub processing_logo_position: String,
    pub processing_volume: f64,
    pub processing_loudness_scope: String,
    pub processing_compressor_ratio: f64,
    pub processing_compressor_attack_ms: f64,
    pub processing_compressor_hold_ms: f64,
    pub processing_compressor_release_ms: f64,
    pub processing_compressor_strong_release_ms: f64,
    pub processing_compressor_knee_db: f64,
    pub processing_pause_hold_ms: f64,
    pub processing_pause_return_delay_ms: f64,
    pub processing_loudness_output_max_correction_db: f64,
    pub processing_loudness_output_gain_up_db_per_second: f64,
    pub processing_loudness_output_gain_down_db_per_second: f64,

    pub processing_compressor_threshold_dbfs: f64,
    pub processing_pause_threshold_dbfs: f64,
    pub processing_loudness_enable: bool,
    pub processing_loudness_target_lufs: f64,
    pub processing_loudness_dead_band_lu: f64,
    pub processing_loudness_max_gain_db: f64,
    pub processing_loudness_max_attenuation_db: f64,
    pub processing_loudness_gain_up_db_per_second: f64,
    pub processing_loudness_gain_down_db_per_second: f64,
    pub processing_loudness_silence_gate_lufs: f64,
    pub processing_loudness_true_peak_ceiling_dbtp: f64,
    #[serde(default)]
    pub processing_vtt_enable: bool,
    #[serde(default)]
    pub processing_vtt_dummy: Option<String>,
    #[serde(default = "default_vtt_name")]
    pub processing_vtt_name: String,
    #[serde(default = "default_vtt_language")]
    pub processing_vtt_language: String,
    #[serde(default)]
    pub processing_vtt_default: bool,

    pub playlist_day_start: String,
    pub playlist_length: String,
    pub playlist_infinit: bool,

    pub storage_filler: String,
    pub storage_extensions: String,
    pub storage_shuffle: bool,

    pub text_preset_id: Option<i32>,

    pub task_enable: bool,
    pub task_path: String,

    pub output_id: i32,
}

impl Configuration {
    pub fn from(id: i32, channel_id: i32, config: PlayoutConfig) -> Self {
        Self {
            id,
            channel_id,
            general_stop_threshold: config.general.stop_threshold,
            mail_subject: config.mail.subject,
            mail_recipient: config.mail.recipient,
            mail_level: config.mail.mail_level.to_string(),
            mail_interval: config.mail.interval,
            notification_topic: config.notification.topic,
            notification_level: config.notification.level.to_string(),
            notification_tags: config.notification.tags,
            logging_ffmpeg_level: config.logging.ffmpeg_level,
            logging_ingest_level: config.logging.ingest_level,
            logging_detect_silence: config.logging.detect_silence,
            logging_ignore: config.logging.ignore_lines.join(";"),
            processing_mode: config.processing.mode.to_string(),
            processing_add_logo: config.processing.add_logo,
            processing_logo: config.processing.logo,
            processing_logo_scale: config.processing.logo_scale,
            processing_logo_opacity: config.processing.logo_opacity,
            processing_logo_position: config.processing.logo_position,
            processing_volume: config.audio.volume,
            processing_loudness_scope: config.audio.loudness_scope.clone(),
            processing_compressor_ratio: config.audio.compressor_ratio,
            processing_compressor_attack_ms: config.audio.compressor_attack_ms,
            processing_compressor_hold_ms: config.audio.compressor_hold_ms,
            processing_compressor_release_ms: config.audio.compressor_release_ms,
            processing_compressor_strong_release_ms: config.audio.compressor_strong_release_ms,
            processing_compressor_knee_db: config.audio.compressor_knee_db,
            processing_pause_hold_ms: config.audio.pause_hold_ms,
            processing_pause_return_delay_ms: config.audio.pause_return_delay_ms,
            processing_loudness_output_max_correction_db: config
                .audio
                .loudness_output_max_correction_db,
            processing_loudness_output_gain_up_db_per_second: config
                .audio
                .loudness_output_gain_up_db_per_second,
            processing_loudness_output_gain_down_db_per_second: config
                .audio
                .loudness_output_gain_down_db_per_second,

            processing_compressor_threshold_dbfs: config.audio.compressor_threshold_dbfs,
            processing_pause_threshold_dbfs: config.audio.pause_threshold_dbfs,
            processing_loudness_enable: config.audio.loudness_scope != "off",
            processing_loudness_target_lufs: config.audio.loudness_target_lufs,
            processing_loudness_dead_band_lu: config.audio.loudness_dead_band_lu,
            processing_loudness_max_gain_db: config.audio.loudness_max_gain_db,
            processing_loudness_max_attenuation_db: config.audio.loudness_max_attenuation_db,
            processing_loudness_gain_up_db_per_second: config.audio.loudness_gain_up_db_per_second,
            processing_loudness_gain_down_db_per_second: config
                .audio
                .loudness_gain_down_db_per_second,
            processing_loudness_silence_gate_lufs: config.audio.loudness_silence_gate_lufs,
            processing_loudness_true_peak_ceiling_dbtp: config
                .audio
                .loudness_true_peak_ceiling_dbtp,
            processing_vtt_enable: config.processing.vtt_enable,
            processing_vtt_dummy: config.processing.vtt_dummy,
            processing_vtt_name: config.processing.vtt_name,
            processing_vtt_language: config.processing.vtt_language,
            processing_vtt_default: config.processing.vtt_default,
            playlist_day_start: config.playlist.day_start,
            playlist_length: config.playlist.length,
            playlist_infinit: config.playlist.infinit,
            storage_filler: config.storage.filler,
            storage_extensions: config.storage.extensions.join(";"),
            storage_shuffle: config.storage.shuffle,
            text_preset_id: config.text.preset_id,
            task_enable: config.task.enable,
            task_path: config.task.path.to_string_lossy().to_string(),
            output_id: config.output.id,
        }
    }
}

#[derive(Clone, Default, Debug, Deserialize, Serialize, sqlx::FromRow)]
pub struct Recording {
    pub id: i32,
    pub channel_id: i32,
    pub enabled: bool,
    pub source: String,
    pub source_output_id: Option<i32>,
    pub hls_variant: String,
    pub path: String,
    pub segment_duration: i64,
    pub retention_days: i64,
    pub minimum_free_space_gb: i64,
    pub width: i64,
    pub height: i64,
    pub video_codec: String,
    pub video_options: String,
    pub audio_codec: String,
    pub audio_options: String,
    pub audio_bitrate: i64,
}

#[derive(Clone, Default, Debug, Deserialize, Serialize, sqlx::FromRow)]
pub struct Output {
    pub id: i32,
    pub channel_id: i32,
    pub name: String,
    pub hls_variants: String,
    pub stream_url: String,
    pub stream_type: Option<String>,
    pub stream_format: Option<String>,
    pub hls_playlist_name: Option<String>,
    pub hls_segment_duration: Option<i64>,
    pub hls_list_size: Option<i64>,
    #[sqlx(default)]
    #[serde(default)]
    pub desktop_fullscreen: bool,
    pub width: i64,
    pub height: i64,
    pub fps: f64,
    pub video_codec: Option<String>,
    pub video_options: String,
    pub protocol_options: String,
    pub muxer_options: String,
    pub metadata_options: String,
    pub audio_codec: Option<String>,
    pub audio_options: String,
    pub audio_bitrate: Option<i64>,
}

impl Output {
    pub fn new(channel_id: i32, mode: OutputMode) -> Self {
        let stream_url = if mode == OutputMode::Stream {
            "rtmp://127.0.0.1/live/stream".to_string()
        } else {
            Default::default()
        };
        let hls_playlist_name = (mode == OutputMode::HLS).then(|| "stream".to_string());
        let hls_segment_duration = (mode == OutputMode::HLS).then_some(6);
        let hls_list_size = (mode == OutputMode::HLS).then_some(600);
        let encoded = matches!(mode, OutputMode::HLS | OutputMode::Stream);

        Self {
            id: 0,
            channel_id,
            name: mode.to_string(),
            hls_variants: String::new(),
            stream_url,
            stream_type: (mode == OutputMode::Stream).then(|| "rtmp".to_string()),
            stream_format: None,
            hls_playlist_name,
            hls_segment_duration,
            hls_list_size,
            desktop_fullscreen: false,
            width: 1280,
            height: 720,
            fps: 25.0,
            video_codec: encoded.then(|| "libx264".to_string()),
            video_options: if encoded {
                "{\"preset\":\"faster\",\"rate_control\":\"crf\",\"quality\":\"23\",\"maxrate\":\"2400\"}".to_string()
            } else {
                Default::default()
            },
            protocol_options: "{}".to_string(),
            muxer_options: "{}".to_string(),
            metadata_options: "{}".to_string(),
            audio_codec: encoded.then(|| "aac".to_string()),
            audio_options: "{}".to_string(),
            audio_bitrate: encoded.then_some(128),
        }
    }
}

fn default_vtt_name() -> String {
    "Subtitles".to_string()
}

fn default_vtt_language() -> String {
    "und".to_string()
}

#[derive(Debug, sqlx::FromRow)]
pub struct LiveInputRecord {
    pub id: i32,
    pub priority: i32,
    pub enabled: bool,
    pub name: String,
    pub backend: String,
    pub identifier: String,
    pub options: String,
    pub demuxer_options: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct SetupSettings {
    pub logs: String,
    pub playlists: String,
    pub public: String,
    pub storage: String,
    pub shared: bool,
    pub smtp_server: String,
    pub smtp_user: String,
    pub smtp_password: String,
    pub smtp_starttls: bool,
    pub smtp_port: u16,
}

impl From<GlobalSettings> for SetupSettings {
    fn from(settings: GlobalSettings) -> Self {
        Self {
            logs: settings.logs,
            playlists: settings.playlists,
            public: settings.public,
            storage: settings.storage,
            shared: settings.shared,
            smtp_server: settings.smtp_server,
            smtp_user: settings.smtp_user,
            smtp_password: String::new(),
            smtp_starttls: settings.smtp_starttls,
            smtp_port: settings.smtp_port,
        }
    }
}

#[derive(Debug)]
pub struct InitialSetup {
    pub settings: SetupSettings,
    pub channel_public: String,
    pub channel_playlists: String,
    pub channel_storage: String,
    pub username: String,
    pub mail: String,
    pub password_hash: String,
    pub two_factor: bool,
}

#[derive(Debug)]
pub(crate) struct SetupRollback {
    pub global: GlobalSettings,
    pub channel: Channel,
    pub active_outputs: Vec<i32>,
    pub user_id: i32,
}

const DATE_FORMAT: &str = "%Y-%m-%d";
const TIME_FORMAT: &str = "%H:%M:%S";
const MAX_NAME_LENGTH: usize = 120;
const MAX_BREAK_SECONDS: i64 = 3600;

const fn active_by_default() -> bool {
    true
}

fn validate_name(label: &str, name: &str) -> Result<(), String> {
    if name.trim().is_empty() {
        return Err(format!("{label} must not be empty"));
    }

    if name.chars().count() > MAX_NAME_LENGTH {
        return Err(format!(
            "{label} must not be longer than {MAX_NAME_LENGTH} characters"
        ));
    }

    Ok(())
}

fn parse_date(label: &str, value: &str) -> Result<NaiveDate, String> {
    NaiveDate::parse_from_str(value, DATE_FORMAT)
        .map_err(|_| format!("{label} must be a date in the format YYYY-MM-DD"))
}

#[derive(Debug, Clone, Deserialize, Serialize, sqlx::FromRow)]
pub struct Advertiser {
    #[sqlx(default)]
    #[serde(default)]
    pub id: i32,
    #[sqlx(default)]
    #[serde(default)]
    pub channel_id: i32,
    pub name: String,
    #[serde(default)]
    pub contact_name: String,
    #[serde(default)]
    pub email: String,
    #[serde(default)]
    pub notes: String,
    #[serde(default = "active_by_default")]
    pub active: bool,
}

impl Advertiser {
    pub fn validate(&self) -> Result<(), String> {
        validate_name("advertiser name", &self.name)?;

        let email = self.email.trim();

        if !email.is_empty() && !email.contains('@') {
            return Err("advertiser email is not valid".to_string());
        }

        Ok(())
    }
}

#[derive(Debug, Clone, Copy, Default, Deserialize, Serialize, Eq, PartialEq, sqlx::Type)]
#[serde(rename_all = "snake_case")]
#[sqlx(type_name = "TEXT", rename_all = "snake_case")]
pub enum CampaignKind {
    #[default]
    Campaign,
    Standalone,
}

#[derive(Debug, Clone, Copy, Default, Deserialize, Serialize, Eq, PartialEq, sqlx::Type)]
#[serde(rename_all = "snake_case")]
#[sqlx(type_name = "TEXT", rename_all = "snake_case")]
pub enum BreakPosition {
    #[default]
    Any,
    First,
    Last,
}

#[derive(Debug, Clone, Deserialize, Serialize, sqlx::FromRow)]
pub struct Campaign {
    #[sqlx(default)]
    #[serde(default)]
    pub id: i32,
    #[sqlx(default)]
    #[serde(default)]
    pub channel_id: i32,
    pub advertiser_id: i32,
    pub name: String,
    #[serde(default)]
    pub kind: CampaignKind,
    #[serde(default)]
    pub start_date: Option<String>,
    #[serde(default)]
    pub end_date: Option<String>,
    #[serde(default = "active_by_default")]
    pub active: bool,
    #[serde(default = "default_campaign_weight")]
    pub weight: i32,
    #[serde(default)]
    pub max_per_day: Option<i32>,
    #[serde(default)]
    pub position: BreakPosition,
}

const fn default_campaign_weight() -> i32 {
    1
}

impl Campaign {
    pub fn validate(&self) -> Result<(), String> {
        validate_name("campaign name", &self.name)?;

        if self.weight < 1 {
            return Err("campaign weight must be at least 1".to_string());
        }

        if self.max_per_day.is_some_and(|limit| limit < 1) {
            return Err("daily limit must be at least 1".to_string());
        }

        let start = self
            .start_date
            .as_deref()
            .map(|date| parse_date("start date", date))
            .transpose()?;
        let end = self
            .end_date
            .as_deref()
            .map(|date| parse_date("end date", date))
            .transpose()?;

        if self.kind == CampaignKind::Campaign && (start.is_none() || end.is_none()) {
            return Err("a campaign needs a start date and an end date".to_string());
        }

        if let (Some(start), Some(end)) = (start, end)
            && start > end
        {
            return Err("the end date must not be before the start date".to_string());
        }

        Ok(())
    }
}

#[derive(Debug, Clone, Copy, Default, Deserialize, Serialize, Eq, PartialEq, sqlx::Type)]
#[serde(rename_all = "snake_case")]
#[sqlx(type_name = "TEXT", rename_all = "snake_case")]
pub enum SpotStatus {
    #[default]
    Processing,
    Ready,
    Failed,
}

#[derive(Debug, Clone, Deserialize, Serialize, sqlx::FromRow)]
pub struct Spot {
    pub id: i32,
    pub channel_id: i32,
    pub campaign_id: i32,
    pub title: String,
    pub file_path: String,
    pub original_filename: String,
    pub duration_sec: Option<f64>,
    pub status: SpotStatus,
    pub error: String,
    pub active: bool,
    pub created_by: Option<i32>,
    pub created_at: String,
}

#[derive(Debug, Clone, Deserialize, Serialize, sqlx::FromRow)]
pub struct BreakWindow {
    #[sqlx(default)]
    #[serde(default)]
    pub id: i32,
    #[sqlx(default)]
    #[serde(default)]
    pub channel_id: i32,
    pub weekday: i32,
    pub nominal_time: String,
    pub duration_sec: i32,
    #[serde(default = "default_break_tolerance")]
    pub tolerance_sec: i32,
    #[serde(default = "active_by_default")]
    pub enabled: bool,
}

const fn default_break_tolerance() -> i32 {
    60
}

impl BreakWindow {
    /// Validate the window and rewrite its time in the canonical `HH:MM:SS`
    /// form that the database constraint and the planner expect.
    pub fn validate(&mut self) -> Result<(), String> {
        if !(1..=7).contains(&self.weekday) {
            return Err("weekday must be between 1 (Monday) and 7 (Sunday)".to_string());
        }

        let time = NaiveTime::parse_from_str(self.nominal_time.trim(), TIME_FORMAT)
            .map_err(|_| "break time must be in the format HH:MM:SS".to_string())?;
        self.nominal_time = time.format(TIME_FORMAT).to_string();

        if !(1..=MAX_BREAK_SECONDS).contains(&i64::from(self.duration_sec)) {
            return Err(format!(
                "break duration must be between 1 and {MAX_BREAK_SECONDS} seconds"
            ));
        }

        if !(0..=MAX_BREAK_SECONDS).contains(&i64::from(self.tolerance_sec)) {
            return Err(format!(
                "break tolerance must be between 0 and {MAX_BREAK_SECONDS} seconds"
            ));
        }

        Ok(())
    }
}

#[cfg(test)]
mod commercial_tests {
    use super::*;

    fn campaign(kind: CampaignKind, start: Option<&str>, end: Option<&str>) -> Campaign {
        Campaign {
            id: 0,
            channel_id: 1,
            advertiser_id: 1,
            name: "Summer sale".to_string(),
            kind,
            start_date: start.map(str::to_string),
            end_date: end.map(str::to_string),
            active: true,
            weight: 1,
            max_per_day: None,
            position: BreakPosition::Any,
        }
    }

    fn window(weekday: i32, time: &str, duration: i32) -> BreakWindow {
        BreakWindow {
            id: 0,
            channel_id: 1,
            weekday,
            nominal_time: time.to_string(),
            duration_sec: duration,
            tolerance_sec: 60,
            enabled: true,
        }
    }

    #[test]
    fn advertiser_needs_a_name_and_a_plausible_email() {
        let mut advertiser = Advertiser {
            id: 0,
            channel_id: 1,
            name: "  ".to_string(),
            contact_name: String::new(),
            email: String::new(),
            notes: String::new(),
            active: true,
        };

        assert!(advertiser.validate().is_err());

        advertiser.name = "Padaria Central".to_string();
        assert!(advertiser.validate().is_ok());

        advertiser.email = "not-an-email".to_string();
        assert!(advertiser.validate().is_err());

        advertiser.email = "contato@padaria.example".to_string();
        assert!(advertiser.validate().is_ok());
    }

    #[test]
    fn campaign_requires_both_dates_in_order() {
        assert!(
            campaign(
                CampaignKind::Campaign,
                Some("2026-10-01"),
                Some("2026-10-31")
            )
            .validate()
            .is_ok()
        );
        assert!(
            campaign(
                CampaignKind::Campaign,
                Some("2026-10-01"),
                Some("2026-10-01")
            )
            .validate()
            .is_ok()
        );
        assert!(
            campaign(CampaignKind::Campaign, None, None)
                .validate()
                .is_err()
        );
        assert!(
            campaign(CampaignKind::Campaign, Some("2026-10-01"), None)
                .validate()
                .is_err()
        );
        assert!(
            campaign(
                CampaignKind::Campaign,
                Some("2026-11-01"),
                Some("2026-10-31")
            )
            .validate()
            .is_err()
        );
        assert!(
            campaign(
                CampaignKind::Campaign,
                Some("01/10/2026"),
                Some("2026-10-31")
            )
            .validate()
            .is_err()
        );
    }

    #[test]
    fn standalone_campaign_may_omit_its_dates() {
        assert!(
            campaign(CampaignKind::Standalone, None, None)
                .validate()
                .is_ok()
        );
        assert!(
            campaign(
                CampaignKind::Standalone,
                Some("2026-11-01"),
                Some("2026-10-31")
            )
            .validate()
            .is_err()
        );
    }

    #[test]
    fn campaign_limits_must_be_positive() {
        let mut item = campaign(CampaignKind::Standalone, None, None);

        item.weight = 0;
        assert!(item.validate().is_err());

        item.weight = 2;
        item.max_per_day = Some(0);
        assert!(item.validate().is_err());

        item.max_per_day = Some(3);
        assert!(item.validate().is_ok());
    }

    #[test]
    fn break_window_time_is_normalized() {
        let mut item = window(3, " 20:00:00 ", 180);

        assert!(item.validate().is_ok());
        assert_eq!(item.nominal_time, "20:00:00");
    }

    #[test]
    fn break_window_rejects_invalid_values() {
        assert!(window(0, "20:00:00", 180).validate().is_err());
        assert!(window(8, "20:00:00", 180).validate().is_err());
        assert!(window(1, "24:00:00", 180).validate().is_err());
        assert!(window(1, "20:00", 180).validate().is_err());
        assert!(window(1, "20:00:00", 0).validate().is_err());
        assert!(window(1, "20:00:00", 3601).validate().is_err());

        let mut item = window(1, "20:00:00", 180);
        item.tolerance_sec = -1;
        assert!(item.validate().is_err());
    }
}
