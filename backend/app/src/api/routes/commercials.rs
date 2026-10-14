use std::path::PathBuf;

use axum::{
    Json,
    extract::{Path, Query, State},
    http::StatusCode,
};
use protect_axum::authorities::AuthDetails;
use serde::Deserialize;

use crate::{
    api::{
        routes::{AuthUser, ensure_any_authority},
        state::AppState,
    },
    db::{
        handles,
        models::{Advertiser, BreakWindow, Campaign, Role, Spot},
    },
    utils::{
        commercial::{SpotRequest, register_spot, unique_conflict},
        errors::ServiceError,
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

/// **Register spot**
///
/// Registers a file of the channel storage as a spot and checks that it can be
/// played. The response tells whether the spot is `ready` or `failed`.
///
/// ```BASH
/// curl -X POST http://127.0.0.1:8787/api/commercials/1/spots -H 'Content-Type: application/json' \
/// -d '{ "campaign_id": 5, "title": "Natal 30s", "file_path": "comerciais/natal.mp4" }' \
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

    let manager = {
        let guard = state.controller.read().await;
        guard.get(channel)
    }
    .ok_or_else(|| ServiceError::BadRequest(format!("Channel {channel} not found!")))?;
    let storage_root: PathBuf = manager.storage.root.read().await.clone();
    let spot = register_spot(&state.pool, &storage_root, channel, user.id, &data).await?;

    Ok((StatusCode::CREATED, Json(spot)))
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
