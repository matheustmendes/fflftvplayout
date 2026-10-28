use axum::{
    Json,
    extract::{Path, State},
    http::StatusCode,
};
use protect_axum::authorities::AuthDetails;
use serde::{Deserialize, Serialize};

use super::commercials::{channel_manager, ensure_manager};
use crate::{
    api::{
        routes::{AuthUser, ensure_any_authority},
        state::AppState,
    },
    db::{
        handles,
        models::{Role, YoutubeSource},
    },
    utils::{
        errors::ServiceError,
        youtube::{
            self, ImportBody, ImportJob, ImportRequest, Probe, SourceKind, YoutubeTool, cancel_job,
            clean_folder, default_folder, jobs_of, valid_height, validate_url,
        },
    },
};

/// What the YouTube screen shows.
#[derive(Debug, Serialize)]
pub struct YoutubeView {
    /// Version of `yt-dlp`, or `None` when it is not installed on the server.
    pub version: Option<String>,
    /// The server has a cookies file for YouTube.
    pub cookies: bool,
    pub sources: Vec<YoutubeSource>,
    pub jobs: Vec<ImportJob>,
}

#[derive(Debug, Deserialize)]
pub struct UrlBody {
    pub url: String,
}

/// What an address is, and whether the channel may take it.
#[derive(Debug, Serialize)]
pub struct YoutubePreview {
    #[serde(flatten)]
    pub probe: Probe,
    /// The owner of the address is on the list of the channel.
    pub allowed: bool,
    /// Folder the videos go to when the person does not choose one.
    pub default_folder: String,
}

async fn allowed_ids(state: &AppState, channel: i32) -> Result<Vec<String>, ServiceError> {
    Ok(handles::select_youtube_sources(&state.pool, channel)
        .await?
        .into_iter()
        .map(|source| source.youtube_channel_id)
        .collect())
}

async fn preview_of(
    state: &AppState,
    channel: i32,
    tool: &YoutubeTool,
    url: &str,
) -> Result<YoutubePreview, ServiceError> {
    let probe = youtube::probe(tool, url).await?;
    let allowed =
        !probe.owner_id.is_empty() && allowed_ids(state, channel).await?.contains(&probe.owner_id);
    let default_folder = default_folder(&probe);

    Ok(YoutubePreview {
        probe,
        allowed,
        default_folder,
    })
}

/// #### YouTube
///
/// **What the screen shows: the tool, the channels allowed and the downloads**
///
/// ```BASH
/// curl -X GET http://127.0.0.1:8787/api/youtube/1 -H 'Authorization: Bearer <TOKEN>'
/// ```
pub async fn get_youtube(
    State(state): State<AppState>,
    Path(channel): Path<i32>,
    user: AuthUser,
    details: AuthDetails<Role>,
) -> Result<Json<YoutubeView>, ServiceError> {
    ensure_manager(&user, &details, channel)?;

    let tool = YoutubeTool::from_env();

    Ok(Json(YoutubeView {
        version: tool.version().await,
        cookies: tool.cookies.is_some(),
        sources: handles::select_youtube_sources(&state.pool, channel).await?,
        jobs: jobs_of(channel),
    }))
}

/// **Only the downloads** (the screen asks for them while one runs)
///
/// ```BASH
/// curl -X GET http://127.0.0.1:8787/api/youtube/1/jobs -H 'Authorization: Bearer <TOKEN>'
/// ```
pub async fn get_youtube_jobs(
    Path(channel): Path<i32>,
    user: AuthUser,
    details: AuthDetails<Role>,
) -> Result<Json<Vec<ImportJob>>, ServiceError> {
    ensure_manager(&user, &details, channel)?;

    Ok(Json(jobs_of(channel)))
}

/// **Allow a YouTube channel** (only the global administrator). Any address of
/// the channel works: a video, a playlist or the channel page.
///
/// ```BASH
/// curl -X POST http://127.0.0.1:8787/api/youtube/1/sources -H 'Content-Type: application/json' \
/// -d '{ "url": "https://www.youtube.com/@LFTVOficial/playlists" }' \
/// -H 'Authorization: Bearer <TOKEN>'
/// ```
pub async fn add_youtube_source(
    State(state): State<AppState>,
    Path(channel): Path<i32>,
    user: AuthUser,
    details: AuthDetails<Role>,
    Json(body): Json<UrlBody>,
) -> Result<(StatusCode, Json<Vec<YoutubeSource>>), ServiceError> {
    ensure_any_authority(&details, &[&Role::GlobalAdmin])?;
    user.ensure_channel_or_admin(channel)?;

    let url = validate_url(&body.url).map_err(ServiceError::BadRequest)?;
    let probe = youtube::probe(&YoutubeTool::from_env(), &url).await?;

    if probe.owner_id.is_empty() {
        return Err(ServiceError::BadRequest(
            "Não descobri de qual canal é esse endereço. Cole o endereço de um vídeo ou da página do canal."
                .to_string(),
        ));
    }

    handles::upsert_youtube_source(&state.pool, channel, &probe.owner_id, &probe.owner_name)
        .await?;

    Ok((
        StatusCode::CREATED,
        Json(handles::select_youtube_sources(&state.pool, channel).await?),
    ))
}

/// **Stop allowing a YouTube channel** (only the global administrator)
///
/// ```BASH
/// curl -X DELETE http://127.0.0.1:8787/api/youtube/1/sources/3 -H 'Authorization: Bearer <TOKEN>'
/// ```
pub async fn remove_youtube_source(
    State(state): State<AppState>,
    Path((channel, id)): Path<(i32, i32)>,
    user: AuthUser,
    details: AuthDetails<Role>,
) -> Result<Json<Vec<YoutubeSource>>, ServiceError> {
    ensure_any_authority(&details, &[&Role::GlobalAdmin])?;
    user.ensure_channel_or_admin(channel)?;

    if !handles::delete_youtube_source(&state.pool, channel, id).await? {
        return Err(ServiceError::NotFound(format!(
            "YouTube channel {id} not found for channel {channel}"
        )));
    }

    Ok(Json(
        handles::select_youtube_sources(&state.pool, channel).await?,
    ))
}

/// **Look at an address before downloading**: what it is, whose it is, which
/// videos it has, and whether the channel may take it.
///
/// ```BASH
/// curl -X POST http://127.0.0.1:8787/api/youtube/1/preview -H 'Content-Type: application/json' \
/// -d '{ "url": "https://www.youtube.com/playlist?list=PL..." }' -H 'Authorization: Bearer <TOKEN>'
/// ```
pub async fn preview_youtube(
    State(state): State<AppState>,
    Path(channel): Path<i32>,
    user: AuthUser,
    details: AuthDetails<Role>,
    Json(body): Json<UrlBody>,
) -> Result<Json<YoutubePreview>, ServiceError> {
    ensure_manager(&user, &details, channel)?;

    let url = validate_url(&body.url).map_err(ServiceError::BadRequest)?;

    Ok(Json(
        preview_of(&state, channel, &YoutubeTool::from_env(), &url).await?,
    ))
}

/// **Download videos into the storage** of the channel, in the background. Only
/// videos of the allowed YouTube channels are kept.
///
/// ```BASH
/// curl -X POST http://127.0.0.1:8787/api/youtube/1/imports -H 'Content-Type: application/json' \
/// -d '{ "url": "https://www.youtube.com/playlist?list=PL...", "folder": "youtube/Jornal", "height": 1080 }' \
/// -H 'Authorization: Bearer <TOKEN>'
/// ```
pub async fn start_youtube_import(
    State(state): State<AppState>,
    Path(channel): Path<i32>,
    user: AuthUser,
    details: AuthDetails<Role>,
    Json(body): Json<ImportBody>,
) -> Result<(StatusCode, Json<ImportJob>), ServiceError> {
    ensure_manager(&user, &details, channel)?;

    let url = validate_url(&body.url).map_err(ServiceError::BadRequest)?;

    if !valid_height(body.height) {
        return Err(ServiceError::BadRequest(
            "Escolha uma qualidade da lista.".to_string(),
        ));
    }

    let chosen_folder = match body.folder.trim() {
        "" => None,
        folder => Some(clean_folder(folder).map_err(ServiceError::BadRequest)?),
    };
    let tool = YoutubeTool::from_env();
    let preview = preview_of(&state, channel, &tool, &url).await?;

    if !preview.allowed {
        return Err(ServiceError::Forbidden(format!(
            "Este endereço é do canal \"{}\", que não está na lista de canais permitidos.",
            preview.probe.owner_name
        )));
    }

    let folder = chosen_folder.unwrap_or(preview.default_folder);
    let storage_root = channel_manager(&state, channel)
        .await?
        .storage
        .root
        .read()
        .await
        .clone();
    let title = match preview.probe.kind {
        SourceKind::Video => preview
            .probe
            .videos
            .first()
            .map_or_else(|| preview.probe.title.clone(), |video| video.title.clone()),
        _ => preview.probe.title.clone(),
    };
    let job = youtube::start_import(
        tool,
        ImportRequest {
            channel_id: channel,
            url,
            title,
            probe: preview.probe,
            storage_root,
            folder,
            allowed: allowed_ids(&state, channel).await?,
            height: body.height,
        },
    )?;

    Ok((StatusCode::ACCEPTED, Json(job)))
}

/// **Stop a download**
///
/// ```BASH
/// curl -X DELETE http://127.0.0.1:8787/api/youtube/1/imports/3 -H 'Authorization: Bearer <TOKEN>'
/// ```
pub async fn cancel_youtube_import(
    Path((channel, id)): Path<(i32, String)>,
    user: AuthUser,
    details: AuthDetails<Role>,
) -> Result<StatusCode, ServiceError> {
    ensure_manager(&user, &details, channel)?;

    if !cancel_job(channel, &id) {
        return Err(ServiceError::NotFound(format!(
            "No running download {id} for channel {channel}"
        )));
    }

    Ok(StatusCode::NO_CONTENT)
}
