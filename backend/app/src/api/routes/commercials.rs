use std::path::PathBuf;

use axum::{
    Json,
    extract::{Path, Query, State},
    http::{
        HeaderValue, StatusCode,
        header::{CONTENT_DISPOSITION, CONTENT_TYPE},
    },
    response::{IntoResponse, Response},
};
use chrono::{Datelike, NaiveDate, Utc};
use protect_axum::authorities::AuthDetails;
use serde::{Deserialize, Serialize};

use crate::{
    api::{
        routes::{AuthUser, ensure_any_authority},
        state::AppState,
    },
    db::{
        handles::{self, AiringFilter},
        models::{
            Advertiser, AirKind, AirResult, Airing, AutoPlanDay, AutoPlanSettings, AutoPlanState,
            BreakWindow, Campaign, CommercialSettings, ReportSettings, Role, Spot,
            WeekTemplateItem,
        },
    },
    file::norm_abs_path,
    player::controller::ChannelManager,
    utils::{
        auto_plan::{DayOutcome, active_playlist_date, ensure_upcoming_days, existing_playlist},
        commercial::{SpotRequest, register_spot, retry_spot, run_conversion, unique_conflict},
        day_plan::{
            PlanPreview, WeekItemInput, apply_plan, build_template_items, plan_for_date,
            preview_date,
        },
        errors::ServiceError,
        report::{Report, ReportRequest, build_report, to_csv},
        report_pdf::render_pdf,
        spot_convert::VideoTarget,
    },
};

#[derive(Debug, Deserialize)]
pub struct ArchivedQuery {
    #[serde(default)]
    pub include_archived: bool,
}

#[derive(Debug, Deserialize)]
pub struct SpotQuery {
    #[serde(default)]
    pub campaign_id: Option<i32>,
    #[serde(default)]
    pub include_archived: bool,
}

#[derive(Debug, Deserialize)]
pub struct AirlogQuery {
    /// First playlist date, `YYYY-MM-DD`.
    #[serde(default)]
    pub date_from: Option<String>,
    /// Last playlist date, `YYYY-MM-DD`.
    #[serde(default)]
    pub date_to: Option<String>,
    #[serde(default)]
    pub campaign_id: Option<i32>,
    #[serde(default)]
    pub advertiser_id: Option<i32>,
    #[serde(default)]
    pub result: Option<AirResult>,
    #[serde(default)]
    pub kind: Option<AirKind>,
    #[serde(default)]
    pub limit: Option<i64>,
}

const AIRLOG_DEFAULT_LIMIT: i64 = 1_000;
const AIRLOG_MAX_LIMIT: i64 = 10_000;

#[derive(Debug, Deserialize)]
pub struct WeekdayQuery {
    #[serde(default)]
    pub weekday: Option<i32>,
}

/// Everyone who can use the channel may look at the commercial data.
pub(super) fn ensure_reader(
    user: &AuthUser,
    details: &AuthDetails<Role>,
    channel: i32,
) -> Result<(), ServiceError> {
    ensure_any_authority(
        details,
        &[&Role::GlobalAdmin, &Role::ChannelAdmin, &Role::User],
    )?;

    user.ensure_channel_or_admin(channel)
}

/// Only administrators change advertisers, campaigns and break windows.
pub(super) fn ensure_manager(
    user: &AuthUser,
    details: &AuthDetails<Role>,
    channel: i32,
) -> Result<(), ServiceError> {
    ensure_any_authority(details, &[&Role::GlobalAdmin, &Role::ChannelAdmin])?;

    user.ensure_channel_or_admin(channel)
}

pub(super) fn not_found(what: &str, id: i32, channel: i32) -> ServiceError {
    ServiceError::NotFound(format!("{what} {id} not found for channel {channel}"))
}

/// #### Commercials
///
/// **List advertisers**
///
/// ```BASH
/// curl -X GET http://127.0.0.1:8787/api/commercials/1/advertisers?include_archived=false \
/// -H 'Authorization: Bearer <TOKEN>'
/// ```
pub async fn get_advertisers(
    State(state): State<AppState>,
    Path(channel): Path<i32>,
    Query(query): Query<ArchivedQuery>,
    user: AuthUser,
    details: AuthDetails<Role>,
) -> Result<Json<Vec<Advertiser>>, ServiceError> {
    ensure_reader(&user, &details, channel)?;

    Ok(Json(
        handles::select_advertisers(&state.pool, channel, query.include_archived).await?,
    ))
}

/// **Add advertiser**
///
/// ```BASH
/// curl -X POST http://127.0.0.1:8787/api/commercials/1/advertisers -H 'Content-Type: application/json' \
/// -d '{ "name": "Padaria Central", "email": "contato@padaria.example" }' \
/// -H 'Authorization: Bearer <TOKEN>'
/// ```
pub async fn add_advertiser(
    State(state): State<AppState>,
    Path(channel): Path<i32>,
    user: AuthUser,
    details: AuthDetails<Role>,
    Json(data): Json<Advertiser>,
) -> Result<(StatusCode, Json<Advertiser>), ServiceError> {
    ensure_manager(&user, &details, channel)?;
    data.validate().map_err(ServiceError::BadRequest)?;

    let id = handles::insert_advertiser(&state.pool, channel, &data)
        .await
        .map_err(|error| unique_conflict(error, "an advertiser with this name already exists"))?;
    let advertiser = handles::select_advertiser(&state.pool, channel, id)
        .await?
        .ok_or(ServiceError::InternalServerError)?;

    Ok((StatusCode::CREATED, Json(advertiser)))
}

/// **Update advertiser**
///
/// ```BASH
/// curl -X PUT http://127.0.0.1:8787/api/commercials/1/advertisers/3 -H 'Content-Type: application/json' \
/// -d '{ "name": "Padaria Central", "email": "novo@padaria.example" }' \
/// -H 'Authorization: Bearer <TOKEN>'
/// ```
pub async fn edit_advertiser(
    State(state): State<AppState>,
    Path((channel, id)): Path<(i32, i32)>,
    user: AuthUser,
    details: AuthDetails<Role>,
    Json(data): Json<Advertiser>,
) -> Result<Json<Advertiser>, ServiceError> {
    ensure_manager(&user, &details, channel)?;
    data.validate().map_err(ServiceError::BadRequest)?;

    if !handles::update_advertiser(&state.pool, channel, id, &data)
        .await
        .map_err(|error| unique_conflict(error, "an advertiser with this name already exists"))?
    {
        return Err(not_found("advertiser", id, channel));
    }

    let advertiser = handles::select_advertiser(&state.pool, channel, id)
        .await?
        .ok_or_else(|| not_found("advertiser", id, channel))?;

    Ok(Json(advertiser))
}

/// **Archive advertiser** (also archives its campaigns and spots)
///
/// ```BASH
/// curl -X DELETE http://127.0.0.1:8787/api/commercials/1/advertisers/3 \
/// -H 'Authorization: Bearer <TOKEN>'
/// ```
pub async fn remove_advertiser(
    State(state): State<AppState>,
    Path((channel, id)): Path<(i32, i32)>,
    user: AuthUser,
    details: AuthDetails<Role>,
) -> Result<StatusCode, ServiceError> {
    ensure_manager(&user, &details, channel)?;

    if !handles::archive_advertiser(&state.pool, channel, id).await? {
        return Err(not_found("advertiser", id, channel));
    }

    Ok(StatusCode::NO_CONTENT)
}

/// **List campaigns**
///
/// ```BASH
/// curl -X GET http://127.0.0.1:8787/api/commercials/1/campaigns \
/// -H 'Authorization: Bearer <TOKEN>'
/// ```
pub async fn get_campaigns(
    State(state): State<AppState>,
    Path(channel): Path<i32>,
    Query(query): Query<ArchivedQuery>,
    user: AuthUser,
    details: AuthDetails<Role>,
) -> Result<Json<Vec<Campaign>>, ServiceError> {
    ensure_reader(&user, &details, channel)?;

    Ok(Json(
        handles::select_campaigns(&state.pool, channel, query.include_archived).await?,
    ))
}

/// **Add campaign**
///
/// ```BASH
/// curl -X POST http://127.0.0.1:8787/api/commercials/1/campaigns -H 'Content-Type: application/json' \
/// -d '{ "advertiser_id": 3, "name": "Natal", "start_date": "2026-12-01", "end_date": "2026-12-24",
///       "max_per_day": 4, "position": "first" }' \
/// -H 'Authorization: Bearer <TOKEN>'
/// ```
pub async fn add_campaign(
    State(state): State<AppState>,
    Path(channel): Path<i32>,
    user: AuthUser,
    details: AuthDetails<Role>,
    Json(data): Json<Campaign>,
) -> Result<(StatusCode, Json<Campaign>), ServiceError> {
    ensure_manager(&user, &details, channel)?;
    data.validate().map_err(ServiceError::BadRequest)?;

    let id = handles::insert_campaign(&state.pool, channel, &data)
        .await?
        .ok_or_else(|| not_found("advertiser", data.advertiser_id, channel))?;
    let campaign = handles::select_campaign(&state.pool, channel, id)
        .await?
        .ok_or(ServiceError::InternalServerError)?;

    Ok((StatusCode::CREATED, Json(campaign)))
}

/// **Update campaign**
pub async fn edit_campaign(
    State(state): State<AppState>,
    Path((channel, id)): Path<(i32, i32)>,
    user: AuthUser,
    details: AuthDetails<Role>,
    Json(data): Json<Campaign>,
) -> Result<Json<Campaign>, ServiceError> {
    ensure_manager(&user, &details, channel)?;
    data.validate().map_err(ServiceError::BadRequest)?;

    if !handles::update_campaign(&state.pool, channel, id, &data).await? {
        return Err(not_found("campaign", id, channel));
    }

    let campaign = handles::select_campaign(&state.pool, channel, id)
        .await?
        .ok_or_else(|| not_found("campaign", id, channel))?;

    Ok(Json(campaign))
}

/// **Archive campaign** (also archives its spots)
pub async fn remove_campaign(
    State(state): State<AppState>,
    Path((channel, id)): Path<(i32, i32)>,
    user: AuthUser,
    details: AuthDetails<Role>,
) -> Result<StatusCode, ServiceError> {
    ensure_manager(&user, &details, channel)?;

    if !handles::archive_campaign(&state.pool, channel, id).await? {
        return Err(not_found("campaign", id, channel));
    }

    Ok(StatusCode::NO_CONTENT)
}

/// **List spots**
///
/// ```BASH
/// curl -X GET 'http://127.0.0.1:8787/api/commercials/1/spots?campaign_id=5' \
/// -H 'Authorization: Bearer <TOKEN>'
/// ```
pub async fn get_spots(
    State(state): State<AppState>,
    Path(channel): Path<i32>,
    Query(query): Query<SpotQuery>,
    user: AuthUser,
    details: AuthDetails<Role>,
) -> Result<Json<Vec<Spot>>, ServiceError> {
    ensure_reader(&user, &details, channel)?;

    Ok(Json(
        handles::select_spots(
            &state.pool,
            channel,
            query.campaign_id,
            query.include_archived,
        )
        .await?,
    ))
}

/// **Airing log**
///
/// What was planned and what really went on the air, oldest first. Live
/// broadcasts appear as rows of kind `live`.
///
/// ```BASH
/// curl -X GET 'http://127.0.0.1:8787/api/commercials/1/airlog?date_from=2026-10-01&campaign_id=5' \
/// -H 'Authorization: Bearer <TOKEN>'
/// ```
pub async fn get_airlog(
    State(state): State<AppState>,
    Path(channel): Path<i32>,
    Query(query): Query<AirlogQuery>,
    user: AuthUser,
    details: AuthDetails<Role>,
) -> Result<Json<Vec<Airing>>, ServiceError> {
    ensure_reader(&user, &details, channel)?;

    for date in [&query.date_from, &query.date_to].into_iter().flatten() {
        parse_date(date)?;
    }

    let filter = AiringFilter {
        date_from: query.date_from,
        date_to: query.date_to,
        campaign_id: query.campaign_id,
        advertiser_id: query.advertiser_id,
        result: query.result,
        kind: query.kind,
        limit: query
            .limit
            .unwrap_or(AIRLOG_DEFAULT_LIMIT)
            .clamp(1, AIRLOG_MAX_LIMIT),
    };

    Ok(Json(
        handles::select_airings(&state.pool, channel, &filter).await?,
    ))
}

/// The running manager of a channel.
pub(super) async fn channel_manager(
    state: &AppState,
    channel: i32,
) -> Result<ChannelManager, ServiceError> {
    let guard = state.controller.read().await;

    guard
        .get(channel)
        .ok_or_else(|| ServiceError::BadRequest(format!("Channel {channel} not found!")))
}

/// The channel storage, and the format spots must be converted to.
async fn spot_context(
    state: &AppState,
    channel: i32,
) -> Result<(PathBuf, VideoTarget), ServiceError> {
    let manager = channel_manager(state, channel).await?;
    let storage_root: PathBuf = manager.storage.root.read().await.clone();
    let config = manager.config.read().await;
    let target = VideoTarget {
        width: config.output.width,
        height: config.output.height,
        fps: config.output.fps,
    };

    Ok((storage_root, target))
}

/// **Register spot**
///
/// Registers an uploaded file of the channel storage as a spot. The file is
/// converted in the background to the format of the channel (size, frame rate,
/// stereo audio, loudness), so the response is `202 Accepted` with the spot in
/// the `processing` state. Poll the spot list for `status` and `progress`.
///
/// ```BASH
/// curl -X POST http://127.0.0.1:8787/api/commercials/1/spots -H 'Content-Type: application/json' \
/// -d '{ "campaign_id": 5, "title": "Natal 30s", "file_path": "upload/natal.mxf" }' \
/// -H 'Authorization: Bearer <TOKEN>'
/// ```
pub async fn add_spot(
    State(state): State<AppState>,
    Path(channel): Path<i32>,
    user: AuthUser,
    details: AuthDetails<Role>,
    Json(data): Json<SpotRequest>,
) -> Result<(StatusCode, Json<Spot>), ServiceError> {
    ensure_reader(&user, &details, channel)?;

    let (storage_root, target) = spot_context(&state, channel).await?;
    let (spot, job) =
        register_spot(&state.pool, &storage_root, channel, user.id, &data, target).await?;

    tokio::spawn(run_conversion(job));

    Ok((StatusCode::ACCEPTED, Json(spot)))
}

/// **Retry the conversion of a failed spot**
///
/// Useful after changing the audio mapping in the settings.
///
/// ```BASH
/// curl -X POST http://127.0.0.1:8787/api/commercials/1/spots/7/retry \
/// -H 'Authorization: Bearer <TOKEN>'
/// ```
pub async fn retry_spot_conversion(
    State(state): State<AppState>,
    Path((channel, id)): Path<(i32, i32)>,
    user: AuthUser,
    details: AuthDetails<Role>,
) -> Result<(StatusCode, Json<Spot>), ServiceError> {
    ensure_reader(&user, &details, channel)?;

    let (storage_root, target) = spot_context(&state, channel).await?;
    let (spot, job) = retry_spot(&state.pool, &storage_root, channel, id, target).await?;

    tokio::spawn(run_conversion(job));

    Ok((StatusCode::ACCEPTED, Json(spot)))
}

/// **Archive spot**
pub async fn remove_spot(
    State(state): State<AppState>,
    Path((channel, id)): Path<(i32, i32)>,
    user: AuthUser,
    details: AuthDetails<Role>,
) -> Result<StatusCode, ServiceError> {
    ensure_manager(&user, &details, channel)?;

    if !handles::archive_spot(&state.pool, channel, id).await? {
        return Err(not_found("spot", id, channel));
    }

    Ok(StatusCode::NO_CONTENT)
}

/// **List break windows**
///
/// ```BASH
/// curl -X GET 'http://127.0.0.1:8787/api/commercials/1/break-windows?weekday=3' \
/// -H 'Authorization: Bearer <TOKEN>'
/// ```
pub async fn get_break_windows(
    State(state): State<AppState>,
    Path(channel): Path<i32>,
    Query(query): Query<WeekdayQuery>,
    user: AuthUser,
    details: AuthDetails<Role>,
) -> Result<Json<Vec<BreakWindow>>, ServiceError> {
    ensure_reader(&user, &details, channel)?;

    Ok(Json(
        handles::select_break_windows(&state.pool, channel, query.weekday).await?,
    ))
}

/// **Add break window**
///
/// `weekday` is 1 (Monday) to 7 (Sunday); `nominal_time` is `HH:MM:SS`.
///
/// ```BASH
/// curl -X POST http://127.0.0.1:8787/api/commercials/1/break-windows -H 'Content-Type: application/json' \
/// -d '{ "weekday": 3, "nominal_time": "20:00:00", "duration_sec": 180 }' \
/// -H 'Authorization: Bearer <TOKEN>'
/// ```
pub async fn add_break_window(
    State(state): State<AppState>,
    Path(channel): Path<i32>,
    user: AuthUser,
    details: AuthDetails<Role>,
    Json(mut data): Json<BreakWindow>,
) -> Result<(StatusCode, Json<BreakWindow>), ServiceError> {
    ensure_manager(&user, &details, channel)?;
    data.validate().map_err(ServiceError::BadRequest)?;

    let id = handles::insert_break_window(&state.pool, channel, &data)
        .await
        .map_err(|error| unique_conflict(error, "there is already a break at this time"))?;
    let window = handles::select_break_windows(&state.pool, channel, Some(data.weekday))
        .await?
        .into_iter()
        .find(|window| window.id == id)
        .ok_or(ServiceError::InternalServerError)?;

    Ok((StatusCode::CREATED, Json(window)))
}

/// **Update break window**
pub async fn edit_break_window(
    State(state): State<AppState>,
    Path((channel, id)): Path<(i32, i32)>,
    user: AuthUser,
    details: AuthDetails<Role>,
    Json(mut data): Json<BreakWindow>,
) -> Result<Json<BreakWindow>, ServiceError> {
    ensure_manager(&user, &details, channel)?;
    data.validate().map_err(ServiceError::BadRequest)?;

    if !handles::update_break_window(&state.pool, channel, id, &data)
        .await
        .map_err(|error| unique_conflict(error, "there is already a break at this time"))?
    {
        return Err(not_found("break window", id, channel));
    }

    let window = handles::select_break_windows(&state.pool, channel, Some(data.weekday))
        .await?
        .into_iter()
        .find(|window| window.id == id)
        .ok_or_else(|| not_found("break window", id, channel))?;

    Ok(Json(window))
}

/// **Delete break window**
pub async fn remove_break_window(
    State(state): State<AppState>,
    Path((channel, id)): Path<(i32, i32)>,
    user: AuthUser,
    details: AuthDetails<Role>,
) -> Result<StatusCode, ServiceError> {
    ensure_manager(&user, &details, channel)?;

    if !handles::delete_break_window(&state.pool, channel, id).await? {
        return Err(not_found("break window", id, channel));
    }

    Ok(StatusCode::NO_CONTENT)
}

/// **Conversion settings**
///
/// How uploaded spots are converted: which audio tracks become the stereo
/// program (`auto`, `first_stream`, `first_two_mono`, `mix_all`) and the
/// loudness target.
///
/// ```BASH
/// curl -X GET http://127.0.0.1:8787/api/commercials/1/settings \
/// -H 'Authorization: Bearer <TOKEN>'
/// ```
pub async fn get_commercial_settings(
    State(state): State<AppState>,
    Path(channel): Path<i32>,
    user: AuthUser,
    details: AuthDetails<Role>,
) -> Result<Json<CommercialSettings>, ServiceError> {
    ensure_reader(&user, &details, channel)?;

    Ok(Json(
        handles::select_commercial_settings(&state.pool, channel).await?,
    ))
}

/// **Update conversion settings**
///
/// ```BASH
/// curl -X PUT http://127.0.0.1:8787/api/commercials/1/settings -H 'Content-Type: application/json' \
/// -d '{ "audio_mapping": "first_two_mono", "loudness_enabled": true,
///       "loudness_target_lufs": -24.0, "loudness_true_peak_dbtp": -2.0 }' \
/// -H 'Authorization: Bearer <TOKEN>'
/// ```
pub async fn update_commercial_settings(
    State(state): State<AppState>,
    Path(channel): Path<i32>,
    user: AuthUser,
    details: AuthDetails<Role>,
    Json(mut data): Json<CommercialSettings>,
) -> Result<Json<CommercialSettings>, ServiceError> {
    ensure_manager(&user, &details, channel)?;
    data.validate().map_err(ServiceError::BadRequest)?;
    data.channel_id = channel;

    handles::upsert_commercial_settings(&state.pool, channel, &data).await?;

    Ok(Json(data))
}

#[derive(Debug, Deserialize)]
pub struct WeekTemplateBody {
    #[serde(default)]
    pub items: Vec<WeekItemInput>,
}

#[derive(Debug, Deserialize)]
pub struct ApplyQuery {
    /// Replace a playlist that already exists for the date.
    #[serde(default)]
    pub overwrite: bool,
}

fn parse_date(date: &str) -> Result<NaiveDate, ServiceError> {
    NaiveDate::parse_from_str(date, "%Y-%m-%d").map_err(|_| {
        ServiceError::BadRequest("the date must be in the format YYYY-MM-DD".to_string())
    })
}

/// **Weekly template**
///
/// The programs of each weekday, in air order. `weekday` is 1 (Monday) to 7
/// (Sunday).
///
/// ```BASH
/// curl -X GET 'http://127.0.0.1:8787/api/commercials/1/week-template?weekday=4' \
/// -H 'Authorization: Bearer <TOKEN>'
/// ```
pub async fn get_week_template(
    State(state): State<AppState>,
    Path(channel): Path<i32>,
    Query(query): Query<WeekdayQuery>,
    user: AuthUser,
    details: AuthDetails<Role>,
) -> Result<Json<Vec<WeekTemplateItem>>, ServiceError> {
    ensure_reader(&user, &details, channel)?;

    Ok(Json(
        handles::select_week_template(&state.pool, channel, query.weekday).await?,
    ))
}

/// **Save the programs of one weekday**
///
/// Replaces the whole day. Only the file is needed for each program: its
/// length is read from the file.
///
/// ```BASH
/// curl -X PUT http://127.0.0.1:8787/api/commercials/1/week-template/4 \
/// -H 'Content-Type: application/json' \
/// -d '{ "items": [ { "source": "shows/news.mp4", "title": "News" },
///                  { "source": "shows/film.mp4", "in_sec": 5.0 } ] }' \
/// -H 'Authorization: Bearer <TOKEN>'
/// ```
pub async fn save_week_template_day(
    State(state): State<AppState>,
    Path((channel, weekday)): Path<(i32, i32)>,
    user: AuthUser,
    details: AuthDetails<Role>,
    Json(body): Json<WeekTemplateBody>,
) -> Result<Json<Vec<WeekTemplateItem>>, ServiceError> {
    ensure_manager(&user, &details, channel)?;

    if !(1..=7).contains(&weekday) {
        return Err(ServiceError::BadRequest(
            "weekday must be between 1 (Monday) and 7 (Sunday)".to_string(),
        ));
    }

    let (storage_root, _) = spot_context(&state, channel).await?;
    let items = build_template_items(&storage_root, &body.items).await?;

    handles::replace_week_template_day(&state.pool, channel, weekday, &items).await?;

    Ok(Json(
        handles::select_week_template(&state.pool, channel, Some(weekday)).await?,
    ))
}

/// **Preview the plan of a date**
///
/// Shows how the date would be built from the weekly template, the break
/// windows and the running campaigns, with the warnings to read before
/// applying it. Nothing is changed.
///
/// ```BASH
/// curl -X GET http://127.0.0.1:8787/api/commercials/1/plan/2026-10-15 \
/// -H 'Authorization: Bearer <TOKEN>'
/// ```
pub async fn preview_day_plan(
    State(state): State<AppState>,
    Path((channel, date)): Path<(i32, String)>,
    user: AuthUser,
    details: AuthDetails<Role>,
) -> Result<Json<PlanPreview>, ServiceError> {
    ensure_reader(&user, &details, channel)?;

    let date = parse_date(&date)?;
    let manager = channel_manager(&state, channel).await?;
    let config = manager.config.read().await.clone();

    Ok(Json(
        preview_date(&state.pool, &config, channel, date).await?,
    ))
}

/// **Apply the plan of a date**
///
/// Writes the planned day as the playlist of the date. A playlist that already
/// exists is only replaced with `overwrite=true`; a plan with an error is
/// never applied.
///
/// ```BASH
/// curl -X POST 'http://127.0.0.1:8787/api/commercials/1/plan/2026-10-15?overwrite=false' \
/// -H 'Authorization: Bearer <TOKEN>'
/// ```
pub async fn apply_day_plan(
    State(state): State<AppState>,
    Path((channel, date)): Path<(i32, String)>,
    Query(query): Query<ApplyQuery>,
    user: AuthUser,
    details: AuthDetails<Role>,
) -> Result<Json<serde_json::Value>, ServiceError> {
    ensure_manager(&user, &details, channel)?;

    let date = parse_date(&date)?;
    let manager = channel_manager(&state, channel).await?;
    let config = manager.config.read().await.clone();
    let channel_name = manager.channel.lock().await.name.clone();
    let plan = plan_for_date(&state.pool, &config, channel, date).await?;
    let message = apply_plan(
        &state.pool,
        &config,
        channel,
        &channel_name,
        &plan,
        date,
        query.overwrite,
    )
    .await?;

    Ok(Json(serde_json::json!({
        "message": message,
        "warnings": plan.warnings,
    })))
}

/// Largest logo file accepted for the PDF report.
const MAX_LOGO_BYTES: u64 = 2 * 1024 * 1024;

/// **Report settings**
///
/// How the reports present the station: name, details, logo and a closing note.
///
/// ```BASH
/// curl -X GET http://127.0.0.1:8787/api/commercials/1/report-settings \
/// -H 'Authorization: Bearer <TOKEN>'
/// ```
pub async fn get_report_settings(
    State(state): State<AppState>,
    Path(channel): Path<i32>,
    user: AuthUser,
    details: AuthDetails<Role>,
) -> Result<Json<ReportSettings>, ServiceError> {
    ensure_reader(&user, &details, channel)?;

    Ok(Json(
        handles::select_report_settings(&state.pool, channel).await?,
    ))
}

/// **Update report settings**
///
/// `logo_path` is a PNG or JPEG inside the channel storage, up to 2 MB.
///
/// ```BASH
/// curl -X PUT http://127.0.0.1:8787/api/commercials/1/report-settings -H 'Content-Type: application/json' \
/// -d '{ "station_name": "TV Exemplo", "station_details": "Rua das Flores, 100\nCNPJ 00.000.000/0001-00",
///       "logo_path": "branding/logo.png", "footer_note": "Documento sem valor fiscal." }' \
/// -H 'Authorization: Bearer <TOKEN>'
/// ```
pub async fn update_report_settings(
    State(state): State<AppState>,
    Path(channel): Path<i32>,
    user: AuthUser,
    details: AuthDetails<Role>,
    Json(mut data): Json<ReportSettings>,
) -> Result<Json<ReportSettings>, ServiceError> {
    ensure_manager(&user, &details, channel)?;
    data.validate().map_err(ServiceError::BadRequest)?;
    data.channel_id = channel;

    if data.logo_path.trim().is_empty() {
        data.logo_path = String::new();
    } else {
        let manager = channel_manager(&state, channel).await?;
        let storage = manager.config.read().await.channel.storage.clone();
        let (file, _, relative) = norm_abs_path(&storage, &data.logo_path)?;
        let extension = file
            .extension()
            .map(|extension| extension.to_string_lossy().to_lowercase())
            .unwrap_or_default();

        if !matches!(extension.as_str(), "png" | "jpg" | "jpeg") {
            return Err(ServiceError::BadRequest(
                "the logo must be a PNG or JPEG file".to_string(),
            ));
        }

        let size = tokio::fs::metadata(&file)
            .await
            .map_err(|_| {
                ServiceError::BadRequest(format!(
                    "file {relative:?} does not exist in the channel storage"
                ))
            })?
            .len();

        if size > MAX_LOGO_BYTES {
            return Err(ServiceError::BadRequest(
                "the logo must not be larger than 2 MB".to_string(),
            ));
        }

        data.logo_path = relative;
    }

    handles::upsert_report_settings(&state.pool, channel, &data).await?;

    Ok(Json(data))
}

#[derive(Debug, Deserialize)]
pub struct ReportQuery {
    /// First playlist date, `YYYY-MM-DD`.
    pub date_from: String,
    /// Last playlist date, `YYYY-MM-DD`.
    pub date_to: String,
    #[serde(default)]
    pub campaign_id: Option<i32>,
    #[serde(default)]
    pub advertiser_id: Option<i32>,
}

async fn load_report(
    state: &AppState,
    channel: i32,
    query: &ReportQuery,
) -> Result<(Report, PathBuf), ServiceError> {
    let request = ReportRequest {
        channel_id: channel,
        campaign_id: query.campaign_id,
        advertiser_id: query.advertiser_id,
        date_from: parse_date(&query.date_from)?,
        date_to: parse_date(&query.date_to)?,
    };
    let manager = channel_manager(state, channel).await?;
    let storage = manager.config.read().await.channel.storage.clone();
    let (name, timezone) = {
        let channel = manager.channel.lock().await;

        (
            channel.name.clone(),
            channel.timezone.unwrap_or(chrono_tz::Tz::UTC),
        )
    };
    let report = build_report(&state.pool, &request, &name, timezone, Utc::now()).await?;

    Ok((report, storage))
}

/// A file name that is safe in a header: ASCII letters and digits only.
fn report_file_name(report: &Report, extension: &str) -> String {
    let subject = report
        .header
        .campaign
        .as_deref()
        .or(report.header.advertiser.as_deref())
        .unwrap_or("todos");
    let slug: String = subject
        .chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() {
                character.to_ascii_lowercase()
            } else {
                '-'
            }
        })
        .collect();
    let slug = slug
        .split('-')
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>()
        .join("-");

    format!(
        "veiculacao-{slug}-{}-{}.{extension}",
        report.header.date_from, report.header.date_to
    )
}

fn download(content_type: &'static str, file_name: &str, body: Vec<u8>) -> Response {
    let disposition = HeaderValue::from_str(&format!("attachment; filename=\"{file_name}\""))
        .unwrap_or_else(|_| HeaderValue::from_static("attachment"));

    (
        [
            (CONTENT_TYPE, HeaderValue::from_static(content_type)),
            (CONTENT_DISPOSITION, disposition),
        ],
        body,
    )
        .into_response()
}

/// **Report summary**
///
/// What the plan promised against what aired in a period, as JSON. Filter by
/// `campaign_id` or `advertiser_id`; a report covers at most 366 days.
///
/// ```BASH
/// curl -X GET 'http://127.0.0.1:8787/api/commercials/1/reports/summary?date_from=2026-12-01&date_to=2026-12-31&campaign_id=5' \
/// -H 'Authorization: Bearer <TOKEN>'
/// ```
pub async fn get_report_summary(
    State(state): State<AppState>,
    Path(channel): Path<i32>,
    Query(query): Query<ReportQuery>,
    user: AuthUser,
    details: AuthDetails<Role>,
) -> Result<Json<Report>, ServiceError> {
    ensure_reader(&user, &details, channel)?;

    Ok(Json(load_report(&state, channel, &query).await?.0))
}

/// **Report as CSV**
///
/// UTF-8 with a byte order mark and `;` as separator, ready for a spreadsheet
/// in Brazilian Portuguese.
///
/// ```BASH
/// curl -X GET 'http://127.0.0.1:8787/api/commercials/1/reports/export.csv?date_from=2026-12-01&date_to=2026-12-31' \
/// -H 'Authorization: Bearer <TOKEN>' -o report.csv
/// ```
pub async fn export_report_csv(
    State(state): State<AppState>,
    Path(channel): Path<i32>,
    Query(query): Query<ReportQuery>,
    user: AuthUser,
    details: AuthDetails<Role>,
) -> Result<Response, ServiceError> {
    ensure_reader(&user, &details, channel)?;

    let (report, _) = load_report(&state, channel, &query).await?;

    Ok(download(
        "text/csv; charset=utf-8",
        &report_file_name(&report, "csv"),
        to_csv(&report),
    ))
}

/// **Report as PDF**
///
/// The report with the logo and details of the station, ready to send to the
/// advertiser.
///
/// ```BASH
/// curl -X GET 'http://127.0.0.1:8787/api/commercials/1/reports/export.pdf?date_from=2026-12-01&date_to=2026-12-31' \
/// -H 'Authorization: Bearer <TOKEN>' -o report.pdf
/// ```
pub async fn export_report_pdf(
    State(state): State<AppState>,
    Path(channel): Path<i32>,
    Query(query): Query<ReportQuery>,
    user: AuthUser,
    details: AuthDetails<Role>,
) -> Result<Response, ServiceError> {
    ensure_reader(&user, &details, channel)?;

    let (report, storage) = load_report(&state, channel, &query).await?;
    let logo = read_logo(&storage, &report.header.logo_path).await;
    let file_name = report_file_name(&report, "pdf");
    let bytes = tokio::task::spawn_blocking(move || render_pdf(&report, logo.as_deref()))
        .await
        .map_err(|_| ServiceError::InternalServerError)??;

    Ok(download("application/pdf", &file_name, bytes))
}

/// The logo bytes, or `None` when there is no usable logo: a report without
/// the logo is better than no report.
async fn read_logo(storage: &std::path::Path, logo_path: &str) -> Option<Vec<u8>> {
    if logo_path.is_empty() {
        return None;
    }

    let (file, _, _) = norm_abs_path(storage, logo_path).ok()?;
    let size = tokio::fs::metadata(&file).await.ok()?.len();

    if size > MAX_LOGO_BYTES {
        return None;
    }

    tokio::fs::read(&file).await.ok()
}

#[derive(Debug, Serialize)]
pub struct AutoPlanStatus {
    pub settings: AutoPlanSettings,
    /// The date of the day on the air.
    pub active_date: String,
    /// The day on the air and the days kept planned after it.
    pub days: Vec<AutoPlanDay>,
}

#[derive(Debug, Serialize)]
pub struct AutoPlanRun {
    pub date: String,
    pub state: AutoPlanState,
    pub message: String,
}

impl From<DayOutcome> for AutoPlanRun {
    fn from(outcome: DayOutcome) -> Self {
        Self {
            date: outcome.date.format("%Y-%m-%d").to_string(),
            state: outcome.state,
            message: outcome.message,
        }
    }
}

/// **Automatic planning**
///
/// Whether the upcoming days are planned automatically, how far ahead, and
/// what was done with each day.
///
/// ```BASH
/// curl -X GET http://127.0.0.1:8787/api/commercials/1/auto-plan \
/// -H 'Authorization: Bearer <TOKEN>'
/// ```
pub async fn get_auto_plan(
    State(state): State<AppState>,
    Path(channel): Path<i32>,
    user: AuthUser,
    details: AuthDetails<Role>,
) -> Result<Json<AutoPlanStatus>, ServiceError> {
    ensure_reader(&user, &details, channel)?;

    let manager = channel_manager(&state, channel).await?;
    let config = manager.config.read().await.clone();
    let settings = handles::select_auto_plan_settings(&state.pool, channel).await?;
    let first = active_playlist_date(
        Utc::now(),
        config.channel.timezone.unwrap_or(chrono_tz::Tz::UTC),
        config.playlist.start_sec.unwrap_or(0.0),
    );
    let last = first + chrono::Duration::days(i64::from(settings.horizon_days));
    let days = handles::select_auto_plan_days(
        &state.pool,
        channel,
        &first.format("%Y-%m-%d").to_string(),
        &last.format("%Y-%m-%d").to_string(),
    )
    .await?;

    Ok(Json(AutoPlanStatus {
        settings,
        active_date: first.format("%Y-%m-%d").to_string(),
        days,
    }))
}

/// **Update automatic planning**
///
/// ```BASH
/// curl -X PUT http://127.0.0.1:8787/api/commercials/1/auto-plan -H 'Content-Type: application/json' \
/// -d '{ "enabled": true, "horizon_days": 3 }' \
/// -H 'Authorization: Bearer <TOKEN>'
/// ```
pub async fn update_auto_plan(
    State(state): State<AppState>,
    Path(channel): Path<i32>,
    user: AuthUser,
    details: AuthDetails<Role>,
    Json(mut data): Json<AutoPlanSettings>,
) -> Result<Json<AutoPlanSettings>, ServiceError> {
    ensure_manager(&user, &details, channel)?;
    data.validate().map_err(ServiceError::BadRequest)?;
    data.channel_id = channel;

    handles::upsert_auto_plan_settings(&state.pool, channel, &data).await?;

    Ok(Json(data))
}

/// **Plan the upcoming days now**
///
/// Does at once what the background task does every few minutes.
///
/// ```BASH
/// curl -X POST http://127.0.0.1:8787/api/commercials/1/auto-plan/run \
/// -H 'Authorization: Bearer <TOKEN>'
/// ```
pub async fn run_auto_plan_now(
    State(state): State<AppState>,
    Path(channel): Path<i32>,
    user: AuthUser,
    details: AuthDetails<Role>,
) -> Result<Json<Vec<AutoPlanRun>>, ServiceError> {
    ensure_manager(&user, &details, channel)?;

    let manager = channel_manager(&state, channel).await?;
    let config = manager.config.read().await.clone();
    let name = manager.channel.lock().await.name.clone();
    let outcomes = ensure_upcoming_days(&state.pool, &config, channel, &name, Utc::now()).await?;

    Ok(Json(outcomes.into_iter().map(AutoPlanRun::from).collect()))
}

#[derive(Debug, Deserialize)]
pub struct CoverageQuery {
    #[serde(default)]
    pub days: Option<i64>,
}

#[derive(Debug, Serialize)]
pub struct CoverageDay {
    pub date: String,
    /// ISO weekday: 1 = Monday ... 7 = Sunday.
    pub weekday: i32,
    /// The day has a playlist the channel can play.
    pub playlist_exists: bool,
    /// The weekly schedule has programs for the weekday, so the day can be planned.
    pub has_schedule: bool,
    /// What the automatic planner did with the day, when it looked at it.
    pub auto_state: Option<AutoPlanState>,
    /// The day on the air.
    pub on_air: bool,
}

const COVERAGE_DEFAULT_DAYS: i64 = 14;
const COVERAGE_MAX_DAYS: i64 = 60;

/// **Coverage of the upcoming days**
///
/// For each day from the one on the air: does it have a playlist, and could it
/// be planned from the weekly schedule. Used to show the days without
/// programming in red.
///
/// ```BASH
/// curl -X GET 'http://127.0.0.1:8787/api/commercials/1/coverage?days=14' \
/// -H 'Authorization: Bearer <TOKEN>'
/// ```
pub async fn get_coverage(
    State(state): State<AppState>,
    Path(channel): Path<i32>,
    Query(query): Query<CoverageQuery>,
    user: AuthUser,
    details: AuthDetails<Role>,
) -> Result<Json<Vec<CoverageDay>>, ServiceError> {
    ensure_reader(&user, &details, channel)?;

    let days = query
        .days
        .unwrap_or(COVERAGE_DEFAULT_DAYS)
        .clamp(1, COVERAGE_MAX_DAYS);
    let manager = channel_manager(&state, channel).await?;
    let config = manager.config.read().await.clone();
    let first = active_playlist_date(
        Utc::now(),
        config.channel.timezone.unwrap_or(chrono_tz::Tz::UTC),
        config.playlist.start_sec.unwrap_or(0.0),
    );
    let last = first + chrono::Duration::days(days - 1);
    let template = handles::select_week_template(&state.pool, channel, None).await?;
    let known = handles::select_auto_plan_days(
        &state.pool,
        channel,
        &first.format("%Y-%m-%d").to_string(),
        &last.format("%Y-%m-%d").to_string(),
    )
    .await?;
    let mut result = Vec::new();

    for offset in 0..days {
        let date = first + chrono::Duration::days(offset);
        let date_text = date.format("%Y-%m-%d").to_string();
        let weekday = date.weekday().number_from_monday() as i32;

        result.push(CoverageDay {
            playlist_exists: matches!(existing_playlist(&config, date).await, Ok(Some(_))),
            has_schedule: template.iter().any(|item| item.weekday == weekday),
            auto_state: known
                .iter()
                .find(|day| day.plan_date == date_text)
                .map(|day| day.state),
            on_air: offset == 0,
            date: date_text,
            weekday,
        });
    }

    Ok(Json(result))
}
