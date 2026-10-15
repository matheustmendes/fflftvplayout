use axum::{
    Json,
    extract::{Path, State},
};
use protect_axum::authorities::AuthDetails;
use serde::Serialize;

use super::commercials::{channel_manager, ensure_manager, ensure_reader};
use crate::{
    api::{routes::AuthUser, state::AppState},
    db::{
        handles,
        models::{RelayConfig, Role},
    },
    utils::{
        errors::ServiceError,
        health::{FillerStatus, RelayStatus, check_relay, filler_status},
    },
};

#[derive(Debug, Serialize)]
pub struct ChannelHealth {
    pub filler: FillerStatus,
    pub relay: RelayStatus,
}

/// **Health of the channel**
///
/// What the dashboard shows as lights: the filler file and the destinations
/// connected to the relay.
///
/// ```BASH
/// curl -X GET http://127.0.0.1:8787/api/health/1 -H 'Authorization: Bearer <TOKEN>'
/// ```
pub async fn get_health(
    State(state): State<AppState>,
    Path(channel): Path<i32>,
    user: AuthUser,
    details: AuthDetails<Role>,
) -> Result<Json<ChannelHealth>, ServiceError> {
    ensure_reader(&user, &details, channel)?;

    let manager = channel_manager(&state, channel).await?;
    let config = manager.config.read().await.clone();
    let relay = handles::select_relay(&state.pool, channel).await?;

    Ok(Json(ChannelHealth {
        filler: filler_status(&config),
        relay: check_relay(&relay).await,
    }))
}

/// **Relay and destinations to watch**
pub async fn get_relay(
    State(state): State<AppState>,
    Path(channel): Path<i32>,
    user: AuthUser,
    details: AuthDetails<Role>,
) -> Result<Json<RelayConfig>, ServiceError> {
    ensure_manager(&user, &details, channel)?;

    Ok(Json(handles::select_relay(&state.pool, channel).await?))
}

/// **Save the relay address and the destinations**
///
/// ```BASH
/// curl -X PUT http://127.0.0.1:8787/api/relay/1 -H 'Content-Type: application/json' \
/// -d '{ "api_url": "http://127.0.0.1:9997", "path": "canal1",
/// "destinations": [{ "name": "TV Praia", "protocol": "rtmp", "match_user": "praia" }] }' \
/// -H 'Authorization: Bearer <TOKEN>'
/// ```
pub async fn update_relay(
    State(state): State<AppState>,
    Path(channel): Path<i32>,
    user: AuthUser,
    details: AuthDetails<Role>,
    Json(config): Json<RelayConfig>,
) -> Result<Json<RelayConfig>, ServiceError> {
    ensure_manager(&user, &details, channel)?;
    config.validate().map_err(ServiceError::BadRequest)?;
    handles::replace_relay(&state.pool, channel, &config).await?;

    Ok(Json(handles::select_relay(&state.pool, channel).await?))
}
