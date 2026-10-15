//! Plan a date: load what the planner needs, run it, and write the result as
//! the playlist the playout already reads.

use std::path::Path;

use chrono::{Datelike, NaiveDate, NaiveTime, Utc};
use log::*;
use serde::{Deserialize, Serialize};
use sqlx::sqlite::SqlitePool;

use crate::{
    db::{
        handles,
        models::{EligibleSpot, WeekTemplateItem},
    },
    file::norm_abs_path,
    player::utils::{JsonPlaylist, Media, probe_media},
    utils::{
        airlog::record_plan,
        break_planner::{
            DayPlan, ItemKind, PlanCampaign, PlanFiller, PlanInput, PlanProgram, PlanSpot,
            PlanWarning, PlanWindow, Severity, WarningCode, plan_day,
        },
        config::PlayoutConfig,
        errors::ServiceError,
        playlist::{read_playlist, write_playlist},
    },
};

const MAX_PROGRAMS_PER_DAY: usize = 500;

/// One program as the operator sends it. Only the file is required: the
/// duration is read from the file.
#[derive(Debug, Clone, Deserialize)]
pub struct WeekItemInput {
    pub source: String,
    #[serde(default)]
    pub title: Option<String>,
    #[serde(default)]
    pub in_sec: Option<f64>,
    #[serde(default)]
    pub out_sec: Option<f64>,
    /// Clock time (`HH:MM`) the program must start at; empty or missing follows
    /// the program before it.
    #[serde(default)]
    pub fixed_start: Option<String>,
}

/// What planning a date would do, without changing anything.
#[derive(Debug, Clone, Serialize)]
pub struct PlanPreview {
    pub date: String,
    /// ISO weekday: 1 = Monday ... 7 = Sunday.
    pub weekday: i32,
    /// A playlist for the date exists and applying would replace it.
    pub playlist_exists: bool,
    pub plan: DayPlan,
}

/// Turn the files an operator picked into programs of the weekly template,
/// reading each file to learn how long it is.
pub async fn build_template_items(
    storage_root: &Path,
    inputs: &[WeekItemInput],
) -> Result<Vec<WeekTemplateItem>, ServiceError> {
    if inputs.len() > MAX_PROGRAMS_PER_DAY {
        return Err(ServiceError::BadRequest(format!(
            "um dia pode ter no máximo {MAX_PROGRAMS_PER_DAY} programas"
        )));
    }

    let mut items = Vec::with_capacity(inputs.len());

    for input in inputs {
        let (file, _, relative_path) = norm_abs_path(storage_root, &input.source)?;

        if !file.is_file() {
            return Err(ServiceError::BadRequest(format!(
                "o arquivo {relative_path:?} não existe no armazenamento do canal"
            )));
        }

        let media = Media::new(0, &file.to_string_lossy(), true).await;

        if media.duration <= 0.0 {
            return Err(ServiceError::BadRequest(format!(
                "não foi possível ler o arquivo {relative_path:?} como mídia"
            )));
        }

        let title = input
            .title
            .as_deref()
            .map(str::trim)
            .filter(|title| !title.is_empty())
            .map(str::to_string)
            .or_else(|| {
                file.file_stem()
                    .map(|stem| stem.to_string_lossy().to_string())
            })
            .unwrap_or_default();
        let item = WeekTemplateItem {
            id: 0,
            channel_id: 0,
            weekday: 0,
            position: 0,
            source: relative_path,
            title,
            in_sec: input.in_sec.unwrap_or(0.0),
            out_sec: input.out_sec.unwrap_or(media.duration),
            duration_sec: media.duration,
            fixed_start: normalized_fixed_start(input.fixed_start.as_deref())?,
        };

        item.validate().map_err(ServiceError::BadRequest)?;
        items.push(item);
    }

    Ok(items)
}

fn program_from_item(item: WeekTemplateItem) -> PlanProgram {
    let title = if item.title.is_empty() {
        item.source.clone()
    } else {
        item.title
    };

    PlanProgram {
        source: item.source,
        title,
        seek: item.in_sec,
        out: item.out_sec,
        duration: item.duration_sec,
        fixed_start: item.fixed_start,
    }
}

/// `20:00` and `20:00:00` are the same time; an empty text means none.
fn normalized_fixed_start(text: Option<&str>) -> Result<Option<String>, ServiceError> {
    let Some(text) = text.map(str::trim).filter(|text| !text.is_empty()) else {
        return Ok(None);
    };
    let time = NaiveTime::parse_from_str(text, "%H:%M:%S")
        .or_else(|_| NaiveTime::parse_from_str(text, "%H:%M"))
        .map_err(|_| {
            ServiceError::BadRequest(format!(
                "o horário marcado {text:?} não é válido; use o formato HH:MM"
            ))
        })?;

    Ok(Some(time.format("%H:%M:%S").to_string()))
}

/// Group the eligible spots by campaign. A spot whose file is gone from the
/// storage is left out and reported, so the plan never airs a missing file.
fn campaigns_from_spots(
    spots: Vec<EligibleSpot>,
    storage_root: &Path,
) -> (Vec<PlanCampaign>, Vec<PlanWarning>) {
    let mut campaigns: Vec<PlanCampaign> = Vec::new();
    let mut warnings = Vec::new();

    for spot in spots {
        if !storage_root.join(&spot.file_path).is_file() {
            warnings.push(PlanWarning::new(
                WarningCode::SpotFileMissing,
                Severity::Warning,
                format!(
                    "o arquivo do comercial {:?} ({}) não está no armazenamento; o comercial não será usado",
                    spot.spot_title, spot.file_path
                ),
                None,
            ));

            continue;
        }

        let plan_spot = PlanSpot {
            id: spot.spot_id,
            title: spot.spot_title,
            file_path: spot.file_path,
            duration: spot.duration_sec,
        };

        match campaigns
            .iter_mut()
            .find(|campaign| campaign.id == spot.campaign_id)
        {
            Some(campaign) => campaign.spots.push(plan_spot),
            None => campaigns.push(PlanCampaign {
                id: spot.campaign_id,
                name: spot.campaign_name,
                advertiser_id: spot.advertiser_id,
                advertiser_name: spot.advertiser_name,
                weight: spot.weight,
                max_per_day: spot.max_per_day,
                position: spot.position,
                spots: vec![plan_spot],
            }),
        }
    }

    (campaigns, warnings)
}

/// A program whose file is gone stays in the day, since the playout fills its
/// time with the channel filler, but the operator has to know.
fn missing_program_warnings(items: &[WeekTemplateItem], storage_root: &Path) -> Vec<PlanWarning> {
    items
        .iter()
        .filter(|item| !storage_root.join(&item.source).is_file())
        .map(|item| {
            PlanWarning::new(
                WarningCode::ProgramFileMissing,
                Severity::Warning,
                format!(
                    "o arquivo do programa {:?} ({}) não está no armazenamento; o horário dele é preenchido com o filler",
                    item.title, item.source
                ),
                None,
            )
        })
        .collect()
}

/// The channel filler file, when one is configured, to complete the breaks.
async fn break_filler(config: &PlayoutConfig) -> Option<PlanFiller> {
    let path = &config.storage.filler_path;

    if !path.is_file() {
        return None;
    }

    let duration = probe_media(path)
        .await
        .ok()?
        .format
        .duration
        .filter(|duration| duration.is_finite() && *duration > 0.0)?;

    Some(PlanFiller {
        source: path.to_string_lossy().to_string(),
        duration,
    })
}

pub async fn plan_for_date(
    pool: &SqlitePool,
    config: &PlayoutConfig,
    channel_id: i32,
    date: NaiveDate,
) -> Result<DayPlan, ServiceError> {
    let weekday = date.weekday().number_from_monday() as i32;
    let date_text = date.format("%Y-%m-%d").to_string();
    let template = handles::select_week_template(pool, channel_id, Some(weekday)).await?;
    let missing_programs = missing_program_warnings(&template, &config.channel.storage);
    let programs = template.into_iter().map(program_from_item).collect();
    let windows = handles::select_break_windows(pool, channel_id, Some(weekday))
        .await?
        .into_iter()
        .filter(|window| window.enabled)
        .map(|window| PlanWindow {
            id: window.id,
            nominal_time: window.nominal_time,
            duration_sec: window.duration_sec,
            tolerance_sec: window.tolerance_sec,
        })
        .collect();
    let spots = handles::select_eligible_spots(pool, channel_id, &date_text).await?;
    let (campaigns, missing_files) = campaigns_from_spots(spots, &config.channel.storage);
    let input = PlanInput {
        date: date_text,
        day_start_sec: config.playlist.start_sec.unwrap_or(0.0),
        day_length_sec: config.playlist.length_sec.unwrap_or(86_400.0),
        programs,
        windows,
        campaigns,
        filler: break_filler(config).await,
    };
    let mut plan = plan_day(&input);

    plan.warnings
        .splice(0..0, missing_programs.into_iter().chain(missing_files));

    Ok(plan)
}

pub async fn preview_date(
    pool: &SqlitePool,
    config: &PlayoutConfig,
    channel_id: i32,
    date: NaiveDate,
) -> Result<PlanPreview, ServiceError> {
    Ok(PlanPreview {
        date: date.format("%Y-%m-%d").to_string(),
        weekday: date.weekday().number_from_monday() as i32,
        playlist_exists: playlist_exists(config, date).await,
        plan: plan_for_date(pool, config, channel_id, date).await?,
    })
}

async fn playlist_exists(config: &PlayoutConfig, date: NaiveDate) -> bool {
    !matches!(
        read_playlist(config, date.format("%Y-%m-%d").to_string()).await,
        Err(ServiceError::NoContent())
    )
}

/// The plan as a playlist: spots and break filler are marked as advertising,
/// so the playout fades the logo around them.
pub fn plan_to_playlist(plan: &DayPlan, channel_name: &str, date: NaiveDate) -> JsonPlaylist {
    let program = plan
        .items
        .iter()
        .map(|item| Media {
            title: Some(item.title.clone()),
            seek: item.seek,
            out: item.out,
            duration: item.duration,
            ad: matches!(item.kind, ItemKind::Spot | ItemKind::BreakFiller),
            source: item.source.clone(),
            ..Media::default()
        })
        .collect();

    JsonPlaylist {
        channel: channel_name.to_string(),
        date: date.format("%Y-%m-%d").to_string(),
        start_sec: None,
        length: None,
        path: None,
        modified: None,
        program,
    }
}

/// Write the plan as the playlist of its date. An existing playlist is only
/// replaced when `overwrite` is set, since an operator may have edited it.
pub async fn apply_plan(
    pool: &SqlitePool,
    config: &PlayoutConfig,
    channel_id: i32,
    channel_name: &str,
    plan: &DayPlan,
    date: NaiveDate,
    overwrite: bool,
) -> Result<String, ServiceError> {
    if let Some(error) = plan
        .warnings
        .iter()
        .find(|warning| warning.severity == Severity::Error)
    {
        return Err(ServiceError::Conflict(format!(
            "o plano tem um problema e não foi aplicado: {}",
            error.message
        )));
    }

    if !overwrite && playlist_exists(config, date).await {
        return Err(ServiceError::Conflict(format!(
            "já existe uma playlist para {}; confirme para substituir",
            date.format("%Y-%m-%d")
        )));
    }

    let message = match write_playlist(config, plan_to_playlist(plan, channel_name, date)).await {
        // The same playlist is already in place.
        Err(ServiceError::Conflict(_)) => format!(
            "a playlist de {} já está atualizada",
            date.format("%Y-%m-%d")
        ),
        other => other?,
    };

    // The promised spots are recorded for the as-run log. A failure here must
    // not undo a playlist that is already written, so it is only reported.
    if let Err(error) = record_plan(pool, channel_id, config, plan, date, Utc::now()).await {
        error!("Cannot record the planned spots of {date}: {error}");
    }

    Ok(message)
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use sqlx::SqlitePool;

    use super::*;
    use crate::{
        db::models::{Advertiser, BreakPosition, BreakWindow, Campaign, CampaignKind},
        player::utils::json_serializer::set_defaults,
        test_support::channel_manager,
    };

    // 2026-10-15 is a Thursday (weekday 4) and 2026-10-16 a Friday.
    fn thursday() -> NaiveDate {
        NaiveDate::from_ymd_opt(2026, 10, 15).unwrap()
    }

    struct Fixture {
        pool: SqlitePool,
        config: PlayoutConfig,
        storage: PathBuf,
    }

    /// A channel whose day starts at midnight and lasts an hour and a minute:
    /// three 20-minute programs and one 60-second break.
    async fn fixture() -> Fixture {
        let (manager, pool) = channel_manager().await;
        let mut config = manager.config.read().await.clone();
        let storage = config.channel.storage.clone();

        config.playlist.start_sec = Some(0.0);
        config.playlist.length_sec = Some(3660.0);

        let program = |name: &str| WeekTemplateItem {
            id: 0,
            channel_id: 0,
            weekday: 0,
            position: 0,
            source: format!("{name}.mp4"),
            title: name.to_string(),
            in_sec: 0.0,
            out_sec: 1200.0,
            duration_sec: 1200.0,
            fixed_start: None,
        };

        handles::replace_week_template_day(
            &pool,
            1,
            4,
            &[program("a"), program("b"), program("c")],
        )
        .await
        .unwrap();

        for name in ["a", "b", "c"] {
            std::fs::write(storage.join(format!("{name}.mp4")), b"media").unwrap();
        }
        handles::insert_break_window(
            &pool,
            1,
            &BreakWindow {
                id: 0,
                channel_id: 1,
                weekday: 4,
                nominal_time: "00:20:00".to_string(),
                duration_sec: 60,
                tolerance_sec: 60,
                enabled: true,
            },
        )
        .await
        .unwrap();

        Fixture {
            pool,
            config,
            storage,
        }
    }

    /// A ready spot whose converted file exists in the storage.
    async fn add_spot(fixture: &Fixture, name: &str, seconds: f64, campaign: Campaign) -> i32 {
        let advertiser_id = handles::insert_advertiser(
            &fixture.pool,
            1,
            &Advertiser {
                id: 0,
                channel_id: 1,
                name: format!("advertiser of {name}"),
                contact_name: String::new(),
                email: String::new(),
                notes: String::new(),
                active: true,
            },
        )
        .await
        .unwrap();
        let campaign_id = handles::insert_campaign(
            &fixture.pool,
            1,
            &Campaign {
                advertiser_id,
                name: name.to_string(),
                ..campaign
            },
        )
        .await
        .unwrap()
        .unwrap();
        let spot_id = handles::insert_spot(
            &fixture.pool,
            1,
            campaign_id,
            name,
            &format!("upload/{name}.mxf"),
            &format!("{name}.mxf"),
            None,
        )
        .await
        .unwrap()
        .unwrap();
        let converted = format!("commercials/spot-{spot_id}.mp4");

        std::fs::create_dir_all(fixture.storage.join("commercials")).unwrap();
        std::fs::write(fixture.storage.join(&converted), b"media").unwrap();
        handles::complete_spot(&fixture.pool, 1, spot_id, &converted, seconds)
            .await
            .unwrap();

        spot_id
    }

    fn standalone() -> Campaign {
        Campaign {
            id: 0,
            channel_id: 1,
            advertiser_id: 0,
            name: String::new(),
            kind: CampaignKind::Standalone,
            start_date: None,
            end_date: None,
            active: true,
            weight: 1,
            max_per_day: None,
            position: BreakPosition::Any,
        }
    }

    fn kinds(plan: &DayPlan) -> Vec<ItemKind> {
        plan.items.iter().map(|item| item.kind).collect()
    }

    #[tokio::test]
    async fn the_day_is_built_from_the_template_windows_and_running_campaigns() {
        let fixture = fixture().await;

        add_spot(&fixture, "padaria", 30.0, standalone()).await;
        add_spot(&fixture, "mercado", 30.0, standalone()).await;

        let plan = plan_for_date(&fixture.pool, &fixture.config, 1, thursday())
            .await
            .unwrap();

        assert_eq!(
            kinds(&plan),
            [
                ItemKind::Program,
                ItemKind::Spot,
                ItemKind::Spot,
                ItemKind::Program,
                ItemKind::Program
            ]
        );
        assert!(plan.warnings.is_empty(), "{:?}", plan.warnings);
        assert_eq!(plan.total_sec, 3660.0);
        assert_eq!(plan.breaks[0].start_time, "00:20:00");
    }

    #[tokio::test]
    async fn a_weekday_without_a_template_cannot_be_planned() {
        let fixture = fixture().await;
        let friday = NaiveDate::from_ymd_opt(2026, 10, 16).unwrap();
        let plan = plan_for_date(&fixture.pool, &fixture.config, 1, friday)
            .await
            .unwrap();

        assert!(plan.has_errors());
        assert_eq!(plan.warnings[0].code, WarningCode::NoPrograms);
    }

    #[tokio::test]
    async fn disabled_windows_are_ignored() {
        let fixture = fixture().await;
        let mut window = handles::select_break_windows(&fixture.pool, 1, Some(4))
            .await
            .unwrap()
            .remove(0);

        window.enabled = false;
        handles::update_break_window(&fixture.pool, 1, window.id, &window)
            .await
            .unwrap();

        let plan = plan_for_date(&fixture.pool, &fixture.config, 1, thursday())
            .await
            .unwrap();

        assert!(plan.breaks.is_empty());
    }

    #[tokio::test]
    async fn only_campaigns_running_on_the_date_are_used() {
        let fixture = fixture().await;
        let november = Campaign {
            kind: CampaignKind::Campaign,
            start_date: Some("2026-11-01".to_string()),
            end_date: Some("2026-11-30".to_string()),
            ..standalone()
        };

        add_spot(&fixture, "future", 30.0, november).await;

        let plan = plan_for_date(&fixture.pool, &fixture.config, 1, thursday())
            .await
            .unwrap();

        assert!(!kinds(&plan).contains(&ItemKind::Spot));
    }

    #[tokio::test]
    async fn a_spot_whose_file_is_gone_is_left_out_and_reported() {
        let fixture = fixture().await;
        let spot_id = add_spot(&fixture, "gone", 30.0, standalone()).await;

        std::fs::remove_file(
            fixture
                .storage
                .join(format!("commercials/spot-{spot_id}.mp4")),
        )
        .unwrap();

        let plan = plan_for_date(&fixture.pool, &fixture.config, 1, thursday())
            .await
            .unwrap();

        assert!(!kinds(&plan).contains(&ItemKind::Spot));
        assert_eq!(plan.warnings[0].code, WarningCode::SpotFileMissing);
        assert!(plan.warnings[0].message.contains("gone"));
    }

    #[tokio::test]
    async fn a_program_whose_file_is_gone_stays_in_the_day_and_is_reported() {
        let fixture = fixture().await;

        std::fs::remove_file(fixture.storage.join("b.mp4")).unwrap();

        let plan = plan_for_date(&fixture.pool, &fixture.config, 1, thursday())
            .await
            .unwrap();
        let program_count = plan
            .items
            .iter()
            .filter(|item| item.kind == ItemKind::Program)
            .count();

        assert_eq!(program_count, 3, "the slot is kept so the day still fits");
        assert_eq!(plan.warnings[0].code, WarningCode::ProgramFileMissing);
        assert!(plan.warnings[0].message.contains("b.mp4"));
        assert!(
            !plan.has_errors(),
            "the playout fills the slot, so it is a warning"
        );
    }

    #[tokio::test]
    async fn the_channel_filler_completes_a_break() {
        let mut fixture = fixture().await;
        let asset = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../tests_assets/storage/media_filler/filler_0.mp4");
        let filler = fixture.storage.join("filler.mp4");

        std::fs::copy(asset, &filler).unwrap();

        fixture.config.storage.filler_path = filler.clone();

        let plan = plan_for_date(&fixture.pool, &fixture.config, 1, thursday())
            .await
            .unwrap();
        let item = plan
            .items
            .iter()
            .find(|item| item.kind == ItemKind::BreakFiller)
            .unwrap();

        assert_eq!(item.source, filler.to_string_lossy());
        assert_eq!(item.out, 60.0);
        assert!(item.duration > 0.0);
    }

    #[tokio::test]
    async fn the_playlist_marks_breaks_as_advertising_and_matches_the_day() {
        let fixture = fixture().await;

        add_spot(&fixture, "padaria", 30.0, standalone()).await;
        add_spot(&fixture, "mercado", 30.0, standalone()).await;

        let plan = plan_for_date(&fixture.pool, &fixture.config, 1, thursday())
            .await
            .unwrap();
        let mut playlist = plan_to_playlist(&plan, "Channel", thursday());

        assert_eq!(playlist.date, "2026-10-15");
        assert_eq!(
            playlist.program.iter().map(|m| m.ad).collect::<Vec<_>>(),
            [false, true, true, false, false]
        );
        assert_eq!(playlist.program[0].source, "a.mp4");
        assert!(playlist.program[1].source.starts_with("commercials/spot-"));
        assert!(
            playlist.program[1]
                .title
                .as_deref()
                .unwrap()
                .contains(" - ")
        );

        // The playout lays the clips out from the day start; the total must
        // be the day length.
        playlist.start_sec = Some(0.0);
        set_defaults(&fixture.config, &mut playlist);

        assert_eq!(playlist.length, Some(3660.0));
        assert_eq!(playlist.program[1].begin, Some(1200.0));
        assert_eq!(playlist.program[3].begin, Some(1260.0));
    }

    #[tokio::test]
    async fn applying_writes_the_playlist_the_playout_reads() {
        let fixture = fixture().await;

        add_spot(&fixture, "padaria", 30.0, standalone()).await;

        let plan = plan_for_date(&fixture.pool, &fixture.config, 1, thursday())
            .await
            .unwrap();

        apply_plan(
            &fixture.pool,
            &fixture.config,
            1,
            "Channel",
            &plan,
            thursday(),
            false,
        )
        .await
        .unwrap();

        let stored = read_playlist(&fixture.config, "2026-10-15".to_string())
            .await
            .unwrap();

        assert_eq!(stored.program.len(), plan.items.len());
        assert_eq!(stored.date, "2026-10-15");
        assert_eq!(stored.program[0].source, "a.mp4");
        assert!(
            fixture
                .config
                .channel
                .playlists
                .join("2026/10/2026-10-15.json")
                .is_file()
        );
    }

    #[tokio::test]
    async fn an_existing_playlist_is_replaced_only_when_confirmed() {
        let fixture = fixture().await;

        add_spot(&fixture, "padaria", 30.0, standalone()).await;

        let plan = plan_for_date(&fixture.pool, &fixture.config, 1, thursday())
            .await
            .unwrap();

        apply_plan(
            &fixture.pool,
            &fixture.config,
            1,
            "Channel",
            &plan,
            thursday(),
            false,
        )
        .await
        .unwrap();

        assert!(
            preview_date(&fixture.pool, &fixture.config, 1, thursday())
                .await
                .unwrap()
                .playlist_exists
        );
        assert!(matches!(
            apply_plan(
                &fixture.pool,
                &fixture.config,
                1,
                "Channel",
                &plan,
                thursday(),
                false
            )
            .await,
            Err(ServiceError::Conflict(_))
        ));

        let message = apply_plan(
            &fixture.pool,
            &fixture.config,
            1,
            "Channel",
            &plan,
            thursday(),
            true,
        )
        .await
        .unwrap();

        assert!(message.contains("atualizada"), "{message}");

        add_spot(&fixture, "mercado", 30.0, standalone()).await;

        let changed = plan_for_date(&fixture.pool, &fixture.config, 1, thursday())
            .await
            .unwrap();

        apply_plan(
            &fixture.pool,
            &fixture.config,
            1,
            "Channel",
            &changed,
            thursday(),
            true,
        )
        .await
        .unwrap();

        let stored = read_playlist(&fixture.config, "2026-10-15".to_string())
            .await
            .unwrap();

        assert_eq!(stored.program.len(), changed.items.len());
    }

    #[tokio::test]
    async fn a_plan_with_an_error_is_never_applied() {
        let fixture = fixture().await;
        let friday = NaiveDate::from_ymd_opt(2026, 10, 16).unwrap();
        let plan = plan_for_date(&fixture.pool, &fixture.config, 1, friday)
            .await
            .unwrap();

        assert!(matches!(
            apply_plan(
                &fixture.pool,
                &fixture.config,
                1,
                "Channel",
                &plan,
                friday,
                true
            )
            .await,
            Err(ServiceError::Conflict(_))
        ));
        assert!(
            !preview_date(&fixture.pool, &fixture.config, 1, friday)
                .await
                .unwrap()
                .playlist_exists
        );
    }

    fn assets() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests_assets/storage/media_mix")
    }

    fn input(source: &str) -> WeekItemInput {
        WeekItemInput {
            source: source.to_string(),
            title: None,
            in_sec: None,
            out_sec: None,
            fixed_start: None,
        }
    }

    #[tokio::test]
    async fn template_items_learn_their_length_from_the_file() {
        let fixture = fixture().await;

        std::fs::copy(assets().join("ad.mp4"), fixture.storage.join("show.mp4")).unwrap();

        let items = build_template_items(&fixture.storage, &[input("show.mp4")])
            .await
            .unwrap();

        assert_eq!(items[0].source, "show.mp4");
        assert_eq!(items[0].title, "show");
        assert!(items[0].duration_sec > 0.0);
        assert_eq!(items[0].out_sec, items[0].duration_sec);
        assert_eq!(items[0].in_sec, 0.0);
    }

    #[tokio::test]
    async fn template_items_accept_a_title_and_a_cut() {
        let fixture = fixture().await;

        std::fs::copy(assets().join("ad.mp4"), fixture.storage.join("show.mp4")).unwrap();

        let items = build_template_items(
            &fixture.storage,
            &[WeekItemInput {
                source: "show.mp4".to_string(),
                title: Some("  Evening news ".to_string()),
                in_sec: Some(1.0),
                out_sec: Some(3.0),
                fixed_start: Some(" 20:30 ".to_string()),
            }],
        )
        .await
        .unwrap();

        assert_eq!(items[0].title, "Evening news");
        assert_eq!(items[0].airtime(), 2.0);
    }

    #[tokio::test]
    async fn template_items_reject_missing_unreadable_and_escaping_files() {
        let fixture = fixture().await;

        std::fs::write(fixture.storage.join("notes.mp4"), "not media").unwrap();

        for source in ["missing.mp4", "notes.mp4"] {
            assert!(
                matches!(
                    build_template_items(&fixture.storage, &[input(source)]).await,
                    Err(ServiceError::BadRequest(_))
                ),
                "{source}"
            );
        }

        assert!(matches!(
            build_template_items(&fixture.storage, &[input("../outside.mp4")]).await,
            Err(ServiceError::Forbidden(_))
        ));
    }

    #[tokio::test]
    async fn template_items_reject_an_end_before_the_start() {
        let fixture = fixture().await;

        std::fs::copy(assets().join("ad.mp4"), fixture.storage.join("show.mp4")).unwrap();

        let result = build_template_items(
            &fixture.storage,
            &[WeekItemInput {
                source: "show.mp4".to_string(),
                title: None,
                in_sec: Some(5.0),
                out_sec: Some(2.0),
                fixed_start: None,
            }],
        )
        .await;

        assert!(matches!(result, Err(ServiceError::BadRequest(_))));
    }
}
