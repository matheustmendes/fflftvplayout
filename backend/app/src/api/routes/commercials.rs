use std::path::PathBuf;

use axum::{
    Json,
    extract::{Path, Query, State},
    http::StatusCode,
};
use chrono::NaiveDate;
use protect_axum::authorities::AuthDetails;
use serde::Deserialize;

use crate::{
    api::{
        routes::{AuthUser, ensure_any_authority},
        state::AppState,
    },
    db::{
        handles::{self, AiringFilter},
        models::{
            Advertiser, AirKind, AirResult, Airing, BreakWindow, Campaign, CommercialSettings,
            Role, Spot, WeekTemplateItem,
        },
    },
    player::controller::ChannelManager,
    utils::{
        commercial::{SpotRequest, register_spot, retry_spot, run_conversion, unique_conflict},
        day_plan::{
            PlanPreview, WeekItemInput, apply_plan, build_template_items, plan_for_date,
            preview_date,
        },
        errors::ServiceError,
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
fn ensure_reader(
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
fn ensure_manager(
    user: &AuthUser,
    details: &AuthDetails<Role>,
    channel: i32,
) -> Result<(), ServiceError> {
    ensure_any_authority(details, &[&Role::GlobalAdmin, &Role::ChannelAdmin])?;

    user.ensure_channel_or_admin(channel)
}

fn not_found(what: &str, id: i32, channel: i32) -> ServiceError {
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
async fn channel_manager(state: &AppState, channel: i32) -> Result<ChannelManager, ServiceError> {
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
