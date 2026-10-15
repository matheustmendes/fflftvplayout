//! A planned day played by the real playout engine.
//!
//! Like the other playout tests this runs in real time, so it is ignored by
//! default. Run it with:
//!
//! ```sh
//! cargo test -p ffplayout --test playout_commercials -- --ignored --nocapture
//! ```

use std::{
    path::{Path, PathBuf},
    sync::atomic::Ordering,
    time::{Duration, Instant},
};

use chrono::NaiveDate;
use chrono_tz::Tz;
use serial_test::serial;
use sqlx::sqlite::SqlitePoolOptions;

use ffplayout::{
    db::{
        handles,
        models::{
            Advertiser, AirResult, BreakPosition, BreakWindow, Campaign, CampaignKind,
            WeekTemplateItem,
        },
    },
    player::{controller::ChannelManager, output::player},
    utils::{
        config::{PlayoutConfig, ProcessMode::Playlist},
        day_plan::{apply_plan, plan_for_date},
        system::SystemStat,
        time_machine::set_mock_time,
    },
};

struct Workspace(PathBuf);

impl Drop for Workspace {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn media(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests_assets/storage")
        .join(name)
}

fn copy(from: &str, to: &Path) {
    std::fs::create_dir_all(to.parent().unwrap()).unwrap();
    std::fs::copy(media(from), to).unwrap();
}

fn program(source: &str, seconds: f64) -> WeekTemplateItem {
    WeekTemplateItem {
        id: 0,
        channel_id: 0,
        weekday: 0,
        position: 0,
        source: source.to_string(),
        title: source.to_string(),
        in_sec: 0.0,
        out_sec: seconds,
        duration_sec: seconds,
        fixed_start: None,
    }
}

#[tokio::test]
#[serial]
#[ignore]
async fn the_engine_plays_a_planned_day_with_the_spot_where_the_break_is() {
    let root = std::env::temp_dir().join(format!("ffplayout-commercials-{}", uuid::Uuid::new_v4()));
    let workspace = Workspace(root.clone());
    let storage = root.join("storage");

    for dir in ["playlists", "public", "log"] {
        std::fs::create_dir_all(root.join(dir)).unwrap();
    }

    // Programs of 10 s and 15 s, and a converted spot of 10 s.
    copy(
        "media_mix/short_video.mp4",
        &storage.join("programs/first.mp4"),
    );
    copy(
        "media_filler/filler_0.mp4",
        &storage.join("programs/second.mp4"),
    );
    copy(
        "media_mix/short_audio.mp4",
        &storage.join("commercials/spot-1.mp4"),
    );

    let pool = SqlitePoolOptions::new()
        .connect("sqlite::memory:")
        .await
        .unwrap();
    handles::db_migrate(&pool).await.unwrap();

    sqlx::query(
        r#"
        UPDATE config_global SET public = $1, logs = $2, playlists = $3, storage = $4;
        UPDATE channels SET public = $1, playlists = $3, storage = $4;
        UPDATE config_output SET width = 1024, height = 576;
        "#,
    )
    .bind(root.join("public").to_string_lossy())
    .bind(root.join("log").to_string_lossy())
    .bind(root.join("playlists").to_string_lossy())
    .bind(storage.to_string_lossy())
    .execute(&pool)
    .await
    .unwrap();

    handles::replace_week_template_day(
        &pool,
        1,
        4,
        &[
            program("programs/first.mp4", 10.0),
            program("programs/second.mp4", 15.0),
        ],
    )
    .await
    .unwrap();
    handles::insert_break_window(
        &pool,
        1,
        &BreakWindow {
            id: 0,
            channel_id: 1,
            weekday: 4,
            nominal_time: "00:00:00".to_string(),
            duration_sec: 10,
            tolerance_sec: 60,
            enabled: true,
        },
    )
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
    let spot_id = handles::insert_spot(
        &pool,
        1,
        campaign_id,
        "Natal",
        "upload/natal.mxf",
        "natal.mxf",
        None,
    )
    .await
    .unwrap()
    .unwrap();

    assert_eq!(spot_id, 1, "the copied file is commercials/spot-1.mp4");

    handles::complete_spot(&pool, 1, spot_id, "commercials/spot-1.mp4", 10.0)
        .await
        .unwrap();

    let mut config = PlayoutConfig::new(&pool, 1, None).await.unwrap();
    let channel = handles::select_channel(&pool, &1).await.unwrap();

    config.general.skip_validation = true;
    config.mail.recipient = String::new();
    config.processing.mode = Playlist;
    config.ingest.listeners.clear();
    config.channel.timezone = Some(Tz::UTC);
    config.playlist.day_start = "00:00:00".into();
    config.playlist.start_sec = Some(0.0);
    config.playlist.length = "24:00:00".into();
    config.playlist.length_sec = Some(86_400.0);

    // Plan Thursday 2026-12-10 and write it as the playlist of the day.
    let date = NaiveDate::from_ymd_opt(2026, 12, 10).unwrap();
    let plan = plan_for_date(&pool, &config, 1, date).await.unwrap();

    assert!(!plan.has_errors(), "{:?}", plan.warnings);
    assert_eq!(plan.items[0].source, "commercials/spot-1.mp4");

    apply_plan(&pool, &config, 1, "Channel", &plan, date, false)
        .await
        .unwrap();

    let log_pool = pool.clone();
    let manager = ChannelManager::new(
        pool,
        channel,
        config.clone(),
        tokio_util::sync::CancellationToken::new(),
        SystemStat::new(),
    )
    .await
    .expect("test storage should initialize");

    manager.update_config(config).await;
    manager.is_alive.store(true, Ordering::SeqCst);
    manager.list_init.store(true, Ordering::SeqCst);

    // The clock starts at midnight of the planned day and runs in real time.
    set_mock_time(&Some("2026-12-10T00:00:00+00:00".to_string())).unwrap();

    // Watch what the engine is playing, then stop it after the second program.
    let watcher = manager.clone();
    let observed = tokio::spawn(async move {
        let started = Instant::now();
        let mut seen: Vec<String> = Vec::new();

        while started.elapsed() < Duration::from_secs(33) {
            if let Some(media) = watcher.current_media.lock().await.as_ref()
                && seen.last() != Some(&media.source)
            {
                seen.push(media.source.clone());
            }

            tokio::time::sleep(Duration::from_millis(200)).await;
        }

        watcher.channel.lock().await.active = false;
        watcher.stop_all(false).await;

        seen
    });

    if let Err(error) = player(manager.clone()).await {
        eprintln!("{error:?}");
    }

    let seen = observed.await.unwrap();
    let names: Vec<&str> = seen
        .iter()
        .map(|source| source.rsplit('/').next().unwrap_or(source))
        .collect();

    println!("played in order: {names:?}");

    assert!(
        names.len() >= 3,
        "the engine should have reached the second program, saw {names:?}"
    );
    assert_eq!(
        names[..3],
        ["spot-1.mp4", "first.mp4", "second.mp4"],
        "the spot plays first, then the programs in order"
    );

    // The as-run log: the planned row was claimed by the real airing.
    tokio::time::sleep(Duration::from_millis(500)).await;

    let rows = handles::select_airings(
        &log_pool,
        1,
        &handles::AiringFilter {
            limit: 100,
            ..handles::AiringFilter::default()
        },
    )
    .await
    .unwrap();

    assert_eq!(rows.len(), 1, "one spot aired, once: {rows:?}");
    assert_eq!(rows[0].result, AirResult::Played, "{:?}", rows[0]);
    assert!(rows[0].actual_start.is_some() && rows[0].actual_end.is_some());
    assert!(
        rows[0].planned_start.is_some(),
        "the row came from the plan"
    );
    drop(workspace);
}
