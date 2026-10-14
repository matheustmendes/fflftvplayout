use std::path::{Path, PathBuf};

use log::*;
use serde::Deserialize;
use sqlx::sqlite::SqlitePool;
use tokio::fs;

use crate::{
    db::{
        handles,
        models::{CommercialSettings, Spot, SpotStatus, validate_name},
    },
    file::norm_abs_path,
    utils::{
        errors::{ProcessError, ServiceError},
        spot_convert::{VideoTarget, convert_spot},
    },
};

/// Folder of the channel storage that holds the converted spots.
const COMMERCIALS_DIR: &str = "commercials";

#[derive(Debug, Clone, Deserialize)]
pub struct SpotRequest {
    pub campaign_id: i32,
    pub title: String,
    /// Path of an uploaded media file, relative to the channel storage.
    pub file_path: String,
}

/// Everything a conversion needs, captured when the spot is registered so the
/// job can run later without touching the request.
#[derive(Debug, Clone)]
pub struct ConversionJob {
    pool: SqlitePool,
    channel_id: i32,
    spot_id: i32,
    storage_root: PathBuf,
    source: PathBuf,
    settings: CommercialSettings,
    target: VideoTarget,
}

/// Register an uploaded file as a spot in the `processing` state. The caller
/// starts the returned job (see [`run_conversion`]); until it finishes, the
/// spot shows its progress.
pub async fn register_spot(
    pool: &SqlitePool,
    storage_root: &Path,
    channel_id: i32,
    created_by: i32,
    request: &SpotRequest,
    target: VideoTarget,
) -> Result<(Spot, ConversionJob), ServiceError> {
    validate_name("spot title", &request.title).map_err(ServiceError::BadRequest)?;
    ensure_target(target)?;

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
    let spot = handles::select_spot(pool, channel_id, spot_id)
        .await?
        .ok_or(ServiceError::InternalServerError)?;
    let job = ConversionJob {
        pool: pool.clone(),
        channel_id,
        spot_id,
        storage_root: storage_root.to_path_buf(),
        source: file,
        settings: handles::select_commercial_settings(pool, channel_id).await?,
        target,
    };

    Ok((spot, job))
}

/// Run the conversion of a failed spot again, for example after the audio
/// mapping was changed in the settings.
pub async fn retry_spot(
    pool: &SqlitePool,
    storage_root: &Path,
    channel_id: i32,
    spot_id: i32,
    target: VideoTarget,
) -> Result<(Spot, ConversionJob), ServiceError> {
    ensure_target(target)?;

    let existing = handles::select_spot(pool, channel_id, spot_id)
        .await?
        .filter(|spot| spot.active)
        .ok_or_else(|| {
            ServiceError::NotFound(format!("spot {spot_id} not found for channel {channel_id}"))
        })?;

    if !handles::restart_spot_conversion(pool, channel_id, spot_id).await? {
        return Err(ServiceError::Conflict(
            "only a spot whose conversion failed can be retried".to_string(),
        ));
    }

    let (source, _, _) = norm_abs_path(storage_root, &existing.source_path)?;
    let spot = handles::select_spot(pool, channel_id, spot_id)
        .await?
        .ok_or(ServiceError::InternalServerError)?;
    let job = ConversionJob {
        pool: pool.clone(),
        channel_id,
        spot_id,
        storage_root: storage_root.to_path_buf(),
        source,
        settings: handles::select_commercial_settings(pool, channel_id).await?,
        target,
    };

    Ok((spot, job))
}

fn ensure_target(target: VideoTarget) -> Result<(), ServiceError> {
    if target.width == 0 || target.height == 0 || !target.fps.is_finite() || target.fps <= 0.0 {
        return Err(ServiceError::Conflict(
            "the output size and frame rate of the channel are not configured".to_string(),
        ));
    }

    Ok(())
}

/// Convert the file of a registered spot and record the outcome. This never
/// fails: a problem is stored on the spot so the operator can read it.
pub async fn run_conversion(job: ConversionJob) {
    let ConversionJob {
        pool,
        channel_id,
        spot_id,
        storage_root,
        source,
        settings,
        target,
    } = job;

    if let Err(reason) = convert_and_record(
        &pool,
        channel_id,
        spot_id,
        &storage_root,
        &source,
        &settings,
        target,
    )
    .await
    {
        warn!(channel = channel_id; "Spot {spot_id} could not be converted: {reason}");

        if let Err(error) = handles::update_spot_status(
            &pool,
            channel_id,
            spot_id,
            SpotStatus::Failed,
            None,
            &reason,
        )
        .await
        {
            error!(channel = channel_id; "Cannot record the failure of spot {spot_id}: {error}");
        }
    }
}

async fn convert_and_record(
    pool: &SqlitePool,
    channel_id: i32,
    spot_id: i32,
    storage_root: &Path,
    source: &Path,
    settings: &CommercialSettings,
    target: VideoTarget,
) -> Result<(), String> {
    if !source.is_file() {
        return Err("the uploaded file is no longer in the storage".to_string());
    }

    let relative_output = format!("{COMMERCIALS_DIR}/spot-{spot_id}.mp4");
    let output = storage_root.join(&relative_output);

    fs::create_dir_all(storage_root.join(COMMERCIALS_DIR))
        .await
        .map_err(|error| {
            error!(channel = channel_id; "Cannot create the commercials folder: {error}");

            "the folder for converted spots could not be created".to_string()
        })?;

    let progress_pool = pool.clone();
    let report = move |percent: i32| {
        let pool = progress_pool.clone();

        tokio::spawn(async move {
            let _ = handles::update_spot_progress(&pool, channel_id, spot_id, percent).await;
        });
    };
    let duration = convert_spot(source, &output, settings, target, &report).await?;

    handles::complete_spot(pool, channel_id, spot_id, &relative_output, duration)
        .await
        .map_err(|error| {
            error!(channel = channel_id; "Cannot record spot {spot_id}: {error}");

            "the converted spot could not be saved".to_string()
        })?;

    info!(channel = channel_id; "Spot {spot_id} is ready ({duration:.1} s)");

    Ok(())
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

#[cfg(test)]
mod tests {
    use sqlx::sqlite::SqlitePoolOptions;

    use super::*;
    use crate::db::{
        handles::db_migrate,
        models::{Advertiser, AudioMapping, BreakPosition, Campaign, CampaignKind},
    };

    const OPERATOR_ID: i32 = 1;

    const TARGET: VideoTarget = VideoTarget {
        width: 640,
        height: 360,
        fps: 30.0,
    };

    /// A temporary channel storage with copies of the test media.
    struct Storage(PathBuf);

    impl Storage {
        fn new() -> Self {
            let root =
                std::env::temp_dir().join(format!("ffplayout-spots-{}", uuid::Uuid::new_v4()));
            let assets =
                PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests_assets/storage");

            std::fs::create_dir_all(root.join("upload")).unwrap();

            for (source, name) in [
                ("media_mix/ad.mp4", "ad.mp4"),
                ("media_mix/no_audio.mp4", "no_audio.mp4"),
                ("media_mix/audio.mp3", "audio.mp3"),
                ("media_mix/subtitle.vtt", "subtitle.vtt"),
            ] {
                std::fs::copy(assets.join(source), root.join("upload").join(name)).unwrap();
            }

            Self(root)
        }
    }

    impl Drop for Storage {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
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

    async fn register(
        pool: &SqlitePool,
        storage: &Storage,
        campaign_id: i32,
        file_path: &str,
    ) -> Result<(Spot, ConversionJob), ServiceError> {
        register_spot(
            pool,
            &storage.0,
            1,
            OPERATOR_ID,
            &request(campaign_id, file_path),
            TARGET,
        )
        .await
    }

    #[tokio::test]
    async fn registering_starts_in_processing_and_keeps_the_upload_as_source() {
        let (pool, campaign_id) = pool_with_campaign().await;
        let storage = Storage::new();
        let (spot, _job) = register(&pool, &storage, campaign_id, "upload/ad.mp4")
            .await
            .unwrap();

        assert_eq!(spot.status, SpotStatus::Processing);
        assert_eq!(spot.source_path, "upload/ad.mp4");
        assert_eq!(spot.original_filename, "ad.mp4");
        assert_eq!(spot.progress, 0);
        assert_eq!(spot.created_by, Some(OPERATOR_ID));
        assert!(spot.duration_sec.is_none());
    }

    #[tokio::test]
    async fn the_conversion_makes_the_spot_ready_with_a_converted_copy() {
        let (pool, campaign_id) = pool_with_campaign().await;
        let storage = Storage::new();
        let (spot, job) = register(&pool, &storage, campaign_id, "upload/ad.mp4")
            .await
            .unwrap();

        run_conversion(job).await;

        let ready = handles::select_spot(&pool, 1, spot.id)
            .await
            .unwrap()
            .unwrap();

        assert_eq!(ready.status, SpotStatus::Ready, "{}", ready.error);
        assert_eq!(ready.file_path, format!("commercials/spot-{}.mp4", spot.id));
        assert_eq!(ready.source_path, "upload/ad.mp4");
        assert_eq!(ready.progress, 100);
        assert!(ready.duration_sec.is_some_and(|duration| duration > 0.0));
        assert!(storage.0.join(&ready.file_path).is_file());
        assert!(
            storage.0.join("upload/ad.mp4").is_file(),
            "the upload is kept"
        );
    }

    #[tokio::test]
    async fn a_file_that_cannot_be_converted_is_kept_as_failed_with_the_reason() {
        let (pool, campaign_id) = pool_with_campaign().await;
        let storage = Storage::new();
        let mut reasons = Vec::new();

        for file in ["no_audio.mp4", "audio.mp3", "subtitle.vtt"] {
            let (spot, job) = register(&pool, &storage, campaign_id, &format!("upload/{file}"))
                .await
                .unwrap();

            run_conversion(job).await;

            let failed = handles::select_spot(&pool, 1, spot.id)
                .await
                .unwrap()
                .unwrap();

            assert_eq!(failed.status, SpotStatus::Failed, "{file}");
            assert!(failed.duration_sec.is_none());
            assert!(
                !storage
                    .0
                    .join(format!("commercials/spot-{}.mp4", spot.id))
                    .exists()
            );

            reasons.push(failed.error);
        }

        assert_eq!(reasons[0], "the file has no audio track");
        assert_eq!(reasons[1], "the file has no video track");
        assert_eq!(
            reasons[2], "the file has no video track",
            "a subtitle file is readable media without video"
        );
    }

    #[tokio::test]
    async fn a_failed_spot_can_be_retried_after_changing_the_settings() {
        let (pool, campaign_id) = pool_with_campaign().await;
        let storage = Storage::new();

        handles::upsert_commercial_settings(
            &pool,
            1,
            &CommercialSettings {
                audio_mapping: AudioMapping::FirstTwoMono,
                ..CommercialSettings::default()
            },
        )
        .await
        .unwrap();

        let (spot, job) = register(&pool, &storage, campaign_id, "upload/ad.mp4")
            .await
            .unwrap();

        run_conversion(job).await;

        let failed = handles::select_spot(&pool, 1, spot.id)
            .await
            .unwrap()
            .unwrap();

        assert_eq!(failed.status, SpotStatus::Failed);
        assert_eq!(
            failed.error,
            "the file has only one audio track, two are needed"
        );

        handles::upsert_commercial_settings(&pool, 1, &CommercialSettings::default())
            .await
            .unwrap();

        let (restarted, job) = retry_spot(&pool, &storage.0, 1, spot.id, TARGET)
            .await
            .unwrap();

        assert_eq!(restarted.status, SpotStatus::Processing);
        assert!(restarted.error.is_empty());

        run_conversion(job).await;

        let ready = handles::select_spot(&pool, 1, spot.id)
            .await
            .unwrap()
            .unwrap();

        assert_eq!(ready.status, SpotStatus::Ready, "{}", ready.error);
    }

    #[tokio::test]
    async fn only_failed_spots_of_the_channel_can_be_retried() {
        let (pool, campaign_id) = pool_with_campaign().await;
        let storage = Storage::new();
        let (spot, _job) = register(&pool, &storage, campaign_id, "upload/ad.mp4")
            .await
            .unwrap();

        assert!(matches!(
            retry_spot(&pool, &storage.0, 1, spot.id, TARGET).await,
            Err(ServiceError::Conflict(_))
        ));
        assert!(matches!(
            retry_spot(&pool, &storage.0, 1, 9999, TARGET).await,
            Err(ServiceError::NotFound(_))
        ));
        assert!(matches!(
            retry_spot(&pool, &storage.0, 2, spot.id, TARGET).await,
            Err(ServiceError::NotFound(_))
        ));
    }

    #[tokio::test]
    async fn a_removed_upload_fails_the_conversion_with_a_clear_reason() {
        let (pool, campaign_id) = pool_with_campaign().await;
        let storage = Storage::new();
        let (spot, job) = register(&pool, &storage, campaign_id, "upload/ad.mp4")
            .await
            .unwrap();

        std::fs::remove_file(storage.0.join("upload/ad.mp4")).unwrap();
        run_conversion(job).await;

        let failed = handles::select_spot(&pool, 1, spot.id)
            .await
            .unwrap()
            .unwrap();

        assert_eq!(
            failed.error,
            "the uploaded file is no longer in the storage"
        );
    }

    #[tokio::test]
    async fn missing_files_and_path_escapes_are_rejected_without_a_row() {
        let (pool, campaign_id) = pool_with_campaign().await;
        let storage = Storage::new();

        for path in ["upload/missing.mp4", "../Cargo.toml", "upload"] {
            assert!(
                register(&pool, &storage, campaign_id, path).await.is_err(),
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
    async fn registering_the_same_upload_twice_is_a_conflict() {
        let (pool, campaign_id) = pool_with_campaign().await;
        let storage = Storage::new();

        register(&pool, &storage, campaign_id, "upload/ad.mp4")
            .await
            .unwrap();

        assert!(matches!(
            register(&pool, &storage, campaign_id, "upload/ad.mp4").await,
            Err(ServiceError::Conflict(_))
        ));
    }

    #[tokio::test]
    async fn an_unknown_campaign_and_a_bad_title_are_rejected() {
        let (pool, campaign_id) = pool_with_campaign().await;
        let storage = Storage::new();

        assert!(matches!(
            register(&pool, &storage, 9999, "upload/ad.mp4").await,
            Err(ServiceError::NotFound(_))
        ));

        let mut bad = request(campaign_id, "upload/ad.mp4");
        bad.title = "   ".to_string();

        assert!(matches!(
            register_spot(&pool, &storage.0, 1, OPERATOR_ID, &bad, TARGET).await,
            Err(ServiceError::BadRequest(_))
        ));
    }

    #[tokio::test]
    async fn a_channel_without_an_output_format_cannot_register_spots() {
        let (pool, campaign_id) = pool_with_campaign().await;
        let storage = Storage::new();
        let unset = VideoTarget { width: 0, ..TARGET };

        assert!(matches!(
            register_spot(
                &pool,
                &storage.0,
                1,
                OPERATOR_ID,
                &request(campaign_id, "upload/ad.mp4"),
                unset
            )
            .await,
            Err(ServiceError::Conflict(_))
        ));
    }
}
