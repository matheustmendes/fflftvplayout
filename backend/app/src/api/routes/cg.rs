use axum::{
    Json,
    extract::{Path, State},
    http::StatusCode,
};
use chrono::Utc;
use protect_axum::authorities::AuthDetails;
use serde::{Deserialize, Serialize};

use super::commercials::{channel_manager, ensure_manager, ensure_reader, not_found};
use crate::{
    api::{routes::AuthUser, state::AppState},
    db::{
        handles,
        models::{CgWithSchedules, Role},
    },
    utils::{
        cg::{MAX_MANUAL_SECONDS, check_cg, hide_now, show_now},
        errors::ServiceError,
    },
};

/// Every graphic of the channel and which of them are on the air now.
#[derive(Debug, Serialize)]
pub struct CgOverview {
    pub items: Vec<CgWithSchedules>,
    pub on_air: Vec<i32>,
}

#[derive(Debug, Deserialize)]
pub struct ShowRequest {
    /// How long the graphic stays on the air, in seconds.
    #[serde(default = "default_show_seconds")]
    pub seconds: i64,
}

const fn default_show_seconds() -> i64 {
    10
}

async fn storage_root(state: &AppState, channel: i32) -> Result<std::path::PathBuf, ServiceError> {
    let manager = channel_manager(state, channel).await?;

    Ok(manager.storage.root.read().await.clone())
}

/// Checks the graphic and that the campaign it advertises is one of the channel.
async fn check_item(
    state: &AppState,
    channel: i32,
    item: &CgWithSchedules,
) -> Result<(), ServiceError> {
    check_cg(item, &storage_root(state, channel).await?)?;

    if let Some(campaign) = item.cg.campaign_id
        && !handles::campaign_exists(&state.pool, channel, campaign).await?
    {
        return Err(ServiceError::BadRequest(format!(
            "campaign {campaign} not found for channel {channel}"
        )));
    }

    Ok(())
}

/// #### Graphics (CG)
///
/// **List the graphics**
///
/// ```BASH
/// curl -X GET http://127.0.0.1:8787/api/cg/1 -H 'Authorization: Bearer <TOKEN>'
/// ```
pub async fn get_cgs(
    State(state): State<AppState>,
    Path(channel): Path<i32>,
    user: AuthUser,
    details: AuthDetails<Role>,
) -> Result<Json<CgOverview>, ServiceError> {
    ensure_reader(&user, &details, channel)?;

    let manager = channel_manager(&state, channel).await?;
    let items = handles::select_cgs(&state.pool, channel).await?;
    let on_air = manager
        .graphics
        .layers()
        .into_iter()
        .filter_map(|layer| i32::try_from(layer.id).ok())
        .collect();

    Ok(Json(CgOverview { items, on_air }))
}

/// **Add a graphic**
///
/// ```BASH
/// curl -X POST http://127.0.0.1:8787/api/cg/1 -H 'Content-Type: application/json' \
/// -d '{ "name": "Hot line", "kind": "text", "text": "Call 123", "x_pct": 5, "y_pct": 85,
/// "size_pct": 5, "schedules": [{ "mode": "clock", "start_time": "18:00", "duration_seconds": 15 }] }' \
/// -H 'Authorization: Bearer <TOKEN>'
/// ```
pub async fn add_cg(
    State(state): State<AppState>,
    Path(channel): Path<i32>,
    user: AuthUser,
    details: AuthDetails<Role>,
    Json(item): Json<CgWithSchedules>,
) -> Result<(StatusCode, Json<CgWithSchedules>), ServiceError> {
    ensure_manager(&user, &details, channel)?;
    check_item(&state, channel, &item).await?;

    let id = handles::insert_cg(&state.pool, channel, &item).await?;
    let saved = handles::select_cg(&state.pool, channel, id)
        .await?
        .ok_or_else(|| not_found("Graphic", id, channel))?;

    Ok((StatusCode::CREATED, Json(saved)))
}

/// **Edit a graphic and its schedule**
pub async fn edit_cg(
    State(state): State<AppState>,
    Path((channel, id)): Path<(i32, i32)>,
    user: AuthUser,
    details: AuthDetails<Role>,
    Json(item): Json<CgWithSchedules>,
) -> Result<Json<CgWithSchedules>, ServiceError> {
    ensure_manager(&user, &details, channel)?;
    check_item(&state, channel, &item).await?;

    if !handles::update_cg(&state.pool, channel, id, &item).await? {
        return Err(not_found("Graphic", id, channel));
    }

    let saved = handles::select_cg(&state.pool, channel, id)
        .await?
        .ok_or_else(|| not_found("Graphic", id, channel))?;

    Ok(Json(saved))
}

/// **Delete a graphic**
pub async fn remove_cg(
    State(state): State<AppState>,
    Path((channel, id)): Path<(i32, i32)>,
    user: AuthUser,
    details: AuthDetails<Role>,
) -> Result<StatusCode, ServiceError> {
    ensure_manager(&user, &details, channel)?;

    let manager = channel_manager(&state, channel).await?;

    if !handles::delete_cg(&state.pool, channel, id).await? {
        return Err(not_found("Graphic", id, channel));
    }

    hide_now(&manager, id);

    Ok(StatusCode::NO_CONTENT)
}

/// **Show a graphic now**
///
/// The operator puts a graphic on the air by hand for a few seconds.
///
/// ```BASH
/// curl -X POST http://127.0.0.1:8787/api/cg/1/3/show -H 'Content-Type: application/json' \
/// -d '{ "seconds": 15 }' -H 'Authorization: Bearer <TOKEN>'
/// ```
pub async fn show_cg(
    State(state): State<AppState>,
    Path((channel, id)): Path<(i32, i32)>,
    user: AuthUser,
    details: AuthDetails<Role>,
    Json(request): Json<ShowRequest>,
) -> Result<StatusCode, ServiceError> {
    ensure_reader(&user, &details, channel)?;

    if !(1..=MAX_MANUAL_SECONDS).contains(&request.seconds) {
        return Err(ServiceError::BadRequest(format!(
            "the time on the air must be between 1 and {MAX_MANUAL_SECONDS} seconds"
        )));
    }

    let manager = channel_manager(&state, channel).await?;

    handles::select_cg(&state.pool, channel, id)
        .await?
        .ok_or_else(|| not_found("Graphic", id, channel))?;
    show_now(&manager, id, request.seconds, Utc::now());

    Ok(StatusCode::NO_CONTENT)
}

/// **Take a graphic off the air**
///
/// Ends a manual "show now". A graphic that its schedule puts on the air at this
/// moment stays until the schedule ends.
pub async fn hide_cg(
    State(state): State<AppState>,
    Path((channel, id)): Path<(i32, i32)>,
    user: AuthUser,
    details: AuthDetails<Role>,
) -> Result<StatusCode, ServiceError> {
    ensure_reader(&user, &details, channel)?;

    let manager = channel_manager(&state, channel).await?;

    hide_now(&manager, id);

    Ok(StatusCode::NO_CONTENT)
}
