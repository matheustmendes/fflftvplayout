use std::path::Path;

use serde::Deserialize;
use sqlx::sqlite::SqlitePool;

use crate::{
    db::{
        handles,
        models::{Spot, SpotStatus, validate_name},
    },
    file::norm_abs_path,
    player::utils::probe_media,
    utils::errors::{ProcessError, ServiceError},
};

#[derive(Debug, Clone, Deserialize)]
pub struct SpotRequest {
    pub campaign_id: i32,
    pub title: String,
    /// Path of an existing media file, relative to the channel storage.
    pub file_path: String,
}

/// Register a media file of the channel storage as a spot and check that it
/// can be played. The returned spot is `ready` with its duration, or `failed`
/// with the reason; a file that cannot be played is kept so the operator can
/// see why it was refused.
pub async fn register_spot(
    pool: &SqlitePool,
    storage_root: &Path,
    channel_id: i32,
    created_by: i32,
    request: &SpotRequest,
) -> Result<Spot, ServiceError> {
    validate_name("spot title", &request.title).map_err(ServiceError::BadRequest)?;

    let (file, _, relative_path) = norm_abs_path(storage_root, &request.file_path)?;

    if !file.is_file() {
        return Err(ServiceError::BadRequest(format!(
            "file {relative_path:?} does not exist in the channel storage"
        )));
    }

    let original_filename = file
        .file_name()
        .map(|name| name.to_string_lossy().to_string())
        .unwrap_or_default();
    let spot_id = handles::insert_spot(
        pool,
        channel_id,
        request.campaign_id,
        &request.title,
        &relative_path,
        &original_filename,
        Some(created_by),
    )
    .await
    .map_err(|error| unique_conflict(error, "this file is already registered as a spot"))?
    .ok_or_else(|| {
        ServiceError::NotFound(format!(
            "campaign {} not found for channel {channel_id}",
            request.campaign_id
        ))
    })?;

    match check_spot_file(&file).await {
        Ok(duration) => {
            handles::update_spot_status(
                pool,
                channel_id,
                spot_id,
                SpotStatus::Ready,
                Some(duration),
                "",
            )
            .await?;
        }
        Err(reason) => {
            handles::update_spot_status(
                pool,
                channel_id,
                spot_id,
                SpotStatus::Failed,
                None,
                &reason,
            )
            .await?;
        }
    }

    handles::select_spot(pool, channel_id, spot_id)
        .await?
        .ok_or(ServiceError::InternalServerError)
}

/// Turn a unique-constraint violation into a conflict the operator can act
/// on; any other database error keeps its generic mapping.
pub fn unique_conflict(error: ProcessError, message: &str) -> ServiceError {
    match error {
        ProcessError::DB(detail) if detail.contains("UNIQUE constraint failed") => {
            ServiceError::Conflict(message.to_string())
        }
        other => other.into(),
    }
}

/// A spot needs a video track, an audio track and a known duration.
async fn check_spot_file(file: &Path) -> Result<f64, String> {
    let probe = probe_media(file)
        .await
        .map_err(|_| "the file could not be read as a video".to_string())?;

    if probe.video.is_empty() {
        return Err("the file has no video track".to_string());
    }

    if probe.audio.is_empty() {
        return Err("the file has no audio track".to_string());
    }

    probe
        .format
        .duration
        .filter(|duration| duration.is_finite() && *duration > 0.0)
        .ok_or_else(|| "the duration of the file is unknown".to_string())
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use sqlx::sqlite::SqlitePoolOptions;

    use super::*;
    use crate::db::{
        handles::db_migrate,
        models::{Advertiser, BreakPosition, Campaign, CampaignKind},
    };

    const OPERATOR_ID: i32 = 1;

    fn storage() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests_assets/storage")
    }

    async fn pool_with_campaign() -> (SqlitePool, i32) {
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .unwrap();
        db_migrate(&pool).await.unwrap();
        sqlx::query(
            "INSERT INTO auth_user (id, mail, username, password, role_id, two_factor)
            VALUES (1, 'operator@example.org', 'operator', 'unchanged', 3, 0)",
        )
        .execute(&pool)
        .await
        .unwrap();

        let advertiser_id = handles::insert_advertiser(
            &pool,
            1,
            &Advertiser {
                id: 0,
                channel_id: 1,
                name: "Padaria".to_string(),
                contact_name: String::new(),
                email: String::new(),
                notes: String::new(),
                active: true,
            },
        )
        .await
        .unwrap();
        let campaign_id = handles::insert_campaign(
            &pool,
            1,
            &Campaign {
                id: 0,
                channel_id: 1,
                advertiser_id,
                name: "Natal".to_string(),
                kind: CampaignKind::Standalone,
                start_date: None,
                end_date: None,
                active: true,
                weight: 1,
                max_per_day: None,
                position: BreakPosition::Any,
            },
        )
        .await
        .unwrap()
        .unwrap();

        (pool, campaign_id)
    }

    fn request(campaign_id: i32, file_path: &str) -> SpotRequest {
        SpotRequest {
            campaign_id,
            title: "Spot".to_string(),
            file_path: file_path.to_string(),
        }
    }

    #[tokio::test]
    async fn a_playable_file_becomes_a_ready_spot_with_its_duration() {
        let (pool, campaign_id) = pool_with_campaign().await;
        let spot = register_spot(
            &pool,
            &storage(),
            1,
            OPERATOR_ID,
            &request(campaign_id, "media_mix/ad.mp4"),
        )
        .await
        .unwrap();

        assert_eq!(spot.status, SpotStatus::Ready);
        assert!(spot.duration_sec.is_some_and(|duration| duration > 0.0));
        assert_eq!(spot.file_path, "media_mix/ad.mp4");
        assert_eq!(spot.original_filename, "ad.mp4");
        assert!(spot.error.is_empty());
    }

    #[tokio::test]
    async fn a_file_without_audio_is_kept_as_failed_with_the_reason() {
        let (pool, campaign_id) = pool_with_campaign().await;
        let spot = register_spot(
            &pool,
            &storage(),
            1,
            OPERATOR_ID,
            &request(campaign_id, "media_mix/no_audio.mp4"),
        )
        .await
        .unwrap();

        assert_eq!(spot.status, SpotStatus::Failed);
        assert_eq!(spot.error, "the file has no audio track");
        assert!(spot.duration_sec.is_none());
    }

    #[tokio::test]
    async fn an_audio_only_file_is_refused_for_lack_of_video() {
        let (pool, campaign_id) = pool_with_campaign().await;
        let spot = register_spot(
            &pool,
            &storage(),
            1,
            OPERATOR_ID,
            &request(campaign_id, "media_mix/audio.mp3"),
        )
        .await
        .unwrap();

        assert_eq!(spot.status, SpotStatus::Failed);
        assert_eq!(spot.error, "the file has no video track");
    }

    #[tokio::test]
    async fn a_file_that_is_not_media_is_refused() {
        let (pool, campaign_id) = pool_with_campaign().await;
        let spot = register_spot(
            &pool,
            &storage(),
            1,
            OPERATOR_ID,
            &request(campaign_id, "media_mix/subtitle.vtt"),
        )
        .await
        .unwrap();

        assert_eq!(spot.status, SpotStatus::Failed);
    }

    #[tokio::test]
    async fn missing_files_and_path_escapes_are_rejected_without_a_row() {
        let (pool, campaign_id) = pool_with_campaign().await;

        for path in ["media_mix/missing.mp4", "../Cargo.toml", "media_mix"] {
            assert!(
                register_spot(
                    &pool,
                    &storage(),
                    1,
                    OPERATOR_ID,
                    &request(campaign_id, path)
                )
                .await
                .is_err(),
                "{path} must be rejected"
            );
        }

        assert!(
            handles::select_spots(&pool, 1, None, true)
                .await
                .unwrap()
                .is_empty()
        );
    }

    #[tokio::test]
    async fn registering_the_same_file_twice_is_a_conflict() {
        let (pool, campaign_id) = pool_with_campaign().await;
        let first = request(campaign_id, "media_mix/ad.mp4");

        register_spot(&pool, &storage(), 1, OPERATOR_ID, &first)
            .await
            .unwrap();

        assert!(matches!(
            register_spot(&pool, &storage(), 1, OPERATOR_ID, &first).await,
            Err(ServiceError::Conflict(_))
        ));
    }

    #[tokio::test]
    async fn an_unknown_campaign_is_not_found() {
        let (pool, _) = pool_with_campaign().await;

        assert!(matches!(
            register_spot(
                &pool,
                &storage(),
                1,
                OPERATOR_ID,
                &request(9999, "media_mix/ad.mp4")
            )
            .await,
            Err(ServiceError::NotFound(_))
        ));
    }

    #[tokio::test]
    async fn an_empty_title_is_rejected() {
        let (pool, campaign_id) = pool_with_campaign().await;
        let mut bad = request(campaign_id, "media_mix/ad.mp4");
        bad.title = "   ".to_string();

        assert!(matches!(
            register_spot(&pool, &storage(), 1, OPERATOR_ID, &bad).await,
            Err(ServiceError::BadRequest(_))
        ));
    }
}
