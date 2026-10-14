//! Conversion of uploaded commercials (MXF and others) into the format the
//! channel plays: H.264/AAC in the channel size and frame rate, stereo audio
//! mapped as configured and normalized to the configured loudness.
//!
//! The command lines are built by pure functions so the audio mapping and the
//! filter graphs can be tested without running FFmpeg.

use std::{
    path::{Path, PathBuf},
    process::Stdio,
    sync::LazyLock,
    time::{Duration, Instant},
};

use serde::Deserialize;
use tokio::{
    fs,
    io::{AsyncBufReadExt, AsyncReadExt, BufReader},
    process::Command,
    sync::Semaphore,
    time::timeout,
};

use crate::{
    db::models::{AudioMapping, CommercialSettings},
    player::utils::{MediaProbe, probe_media},
    utils::config::ENGINE_AUDIO_SAMPLE_RATE,
};

const FFMPEG: &str = "ffmpeg";
const CONVERSION_TIMEOUT: Duration = Duration::from_secs(30 * 60);
const PROGRESS_INTERVAL: Duration = Duration::from_secs(2);
const LOUDNESS_RANGE: f64 = 11.0;
/// An integrated loudness this low means there is no audible program.
const SILENT_LOUDNESS_LUFS: f64 = -60.0;
const DURATION_TOLERANCE_SECONDS: f64 = 1.0;
/// Share of the progress bar used by the loudness measurement pass.
const MEASUREMENT_SHARE: f64 = 0.4;
const PROGRESS_DONE: f64 = 0.97;

/// Only one conversion runs at a time, so the playout keeps its CPU.
static CONVERSION_SLOT: LazyLock<Semaphore> = LazyLock::new(|| Semaphore::new(1));

/// Called with the progress in percent while a conversion runs.
pub type ProgressCallback<'a> = &'a (dyn Fn(i32) + Send + Sync);

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct VideoTarget {
    pub width: u32,
    pub height: u32,
    pub fps: f64,
}

/// What the conversion needs to know about the uploaded file.
#[derive(Debug, Clone, PartialEq)]
pub struct SourceInfo {
    /// Channel count of every audio track, in file order.
    pub audio_channels: Vec<Option<i64>>,
    pub duration: f64,
}

impl SourceInfo {
    pub fn from_probe(probe: &MediaProbe) -> Result<Self, String> {
        if probe.video.is_empty() {
            return Err("the file has no video track".to_string());
        }

        if probe.audio.is_empty() {
            return Err("the file has no audio track".to_string());
        }

        let duration = probe
            .format
            .duration
            .filter(|duration| duration.is_finite() && *duration > 0.0)
            .ok_or_else(|| "the duration of the file is unknown".to_string())?;

        Ok(Self {
            audio_channels: probe.audio.iter().map(|audio| audio.channels).collect(),
            duration,
        })
    }
}

/// Which audio tracks become the stereo program.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AudioPlan {
    /// One track, up- or down-mixed to stereo.
    Single(usize),
    /// Two tracks used as left and right.
    Pair(usize, usize),
    /// Every track mixed into both channels.
    MixAll(usize),
}

pub fn plan_audio(mapping: AudioMapping, channels: &[Option<i64>]) -> Result<AudioPlan, String> {
    if channels.is_empty() {
        return Err("the file has no audio track".to_string());
    }

    let all_mono = channels.iter().all(|count| *count == Some(1));

    match mapping {
        AudioMapping::Auto if channels.len() >= 2 && all_mono => Ok(AudioPlan::Pair(0, 1)),
        AudioMapping::Auto | AudioMapping::FirstStream => Ok(AudioPlan::Single(0)),
        AudioMapping::FirstTwoMono if channels.len() >= 2 => Ok(AudioPlan::Pair(0, 1)),
        AudioMapping::FirstTwoMono => {
            Err("the file has only one audio track, two are needed".to_string())
        }
        AudioMapping::MixAll if channels.len() == 1 => Ok(AudioPlan::Single(0)),
        AudioMapping::MixAll => Ok(AudioPlan::MixAll(channels.len())),
    }
}

/// The filter graph that turns the chosen tracks into one stereo stream,
/// labelled `[pre]`.
fn audio_graph(plan: AudioPlan) -> String {
    match plan {
        AudioPlan::Single(index) => {
            format!("[0:a:{index}]aformat=channel_layouts=stereo[pre]")
        }
        AudioPlan::Pair(left, right) => format!(
            "[0:a:{left}]aformat=channel_layouts=mono[l];\
             [0:a:{right}]aformat=channel_layouts=mono[r];\
             [l][r]amerge=inputs=2,pan=stereo|c0=c0|c1=c1[pre]"
        ),
        AudioPlan::MixAll(count) => {
            let monos: String = (0..count)
                .map(|index| format!("[0:a:{index}]aformat=channel_layouts=mono[m{index}];"))
                .collect();
            let inputs: String = (0..count).map(|index| format!("[m{index}]")).collect();

            format!("{monos}{inputs}amix=inputs={count}:normalize=0,pan=stereo|c0=c0|c1=c0[pre]")
        }
    }
}

/// Loudness measured by the first pass, fed to the second pass.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Loudness {
    pub input_i: f64,
    pub input_tp: f64,
    pub input_lra: f64,
    pub input_thresh: f64,
    pub target_offset: f64,
}

#[derive(Debug, Deserialize)]
struct LoudnormReport {
    input_i: String,
    input_tp: String,
    input_lra: String,
    input_thresh: String,
    target_offset: String,
}

/// Read the JSON block that `loudnorm=print_format=json` writes to stderr.
pub fn parse_loudness_report(stderr: &str) -> Result<Loudness, String> {
    let start = stderr
        .rfind('{')
        .ok_or_else(|| "the loudness could not be measured".to_string())?;
    let end = stderr
        .rfind('}')
        .filter(|end| *end > start)
        .ok_or_else(|| "the loudness could not be measured".to_string())?;
    let report: LoudnormReport = serde_json::from_str(&stderr[start..=end])
        .map_err(|_| "the loudness could not be measured".to_string())?;
    let number = |text: &str| text.trim().parse::<f64>().unwrap_or(f64::NEG_INFINITY);
    let measured = Loudness {
        input_i: number(&report.input_i),
        input_tp: number(&report.input_tp),
        input_lra: number(&report.input_lra),
        input_thresh: number(&report.input_thresh),
        target_offset: number(&report.target_offset),
    };

    if !measured.input_i.is_finite() || measured.input_i <= SILENT_LOUDNESS_LUFS {
        return Err("the audio is silent".to_string());
    }

    if !measured.input_tp.is_finite()
        || !measured.input_lra.is_finite()
        || !measured.input_thresh.is_finite()
        || !measured.target_offset.is_finite()
    {
        return Err("the loudness could not be measured".to_string());
    }

    Ok(measured)
}

/// The `loudnorm` filter. With a measurement it runs in linear mode (a fixed
/// gain, no pumping on short spots); without one it only reports the loudness.
fn loudnorm_filter(settings: &CommercialSettings, measured: Option<&Loudness>) -> String {
    let base = format!(
        "loudnorm=I={:.1}:TP={:.1}:LRA={LOUDNESS_RANGE:.0}",
        settings.loudness_target_lufs, settings.loudness_true_peak_dbtp
    );

    match measured {
        Some(loudness) => format!(
            "{base}:measured_I={:.2}:measured_TP={:.2}:measured_LRA={:.2}:measured_thresh={:.2}:offset={:.2}:linear=true",
            loudness.input_i,
            loudness.input_tp,
            loudness.input_lra,
            loudness.input_thresh,
            loudness.target_offset
        ),
        None => format!("{base}:print_format=json"),
    }
}

/// The video filter chain: fit the picture into the channel size without
/// distortion (anamorphic sources included), deinterlace interlaced frames,
/// and convert to the channel frame rate.
fn video_filter(target: VideoTarget) -> String {
    let VideoTarget { width, height, fps } = target;

    format!(
        "yadif=deint=interlaced,\
         scale=w=trunc(iw*sar/2)*2:h=trunc(ih/2)*2,setsar=1,\
         scale={width}:{height}:force_original_aspect_ratio=decrease,\
         pad={width}:{height}:(ow-iw)/2:(oh-ih)/2,setsar=1,\
         fps={fps},format=yuv420p"
    )
}

fn base_args() -> Vec<String> {
    ["-nostdin", "-hide_banner", "-nostats", "-y"]
        .into_iter()
        .map(str::to_string)
        .collect()
}

/// First pass: decode the audio only and print its loudness.
pub fn measurement_args(
    source: &Path,
    settings: &CommercialSettings,
    plan: AudioPlan,
) -> Vec<String> {
    let graph = format!(
        "{};[pre]{}[a]",
        audio_graph(plan),
        loudnorm_filter(settings, None)
    );
    let mut args = base_args();

    args.extend(["-progress".to_string(), "pipe:1".to_string()]);
    args.extend(["-i".to_string(), source.to_string_lossy().to_string()]);
    args.extend(["-vn".to_string(), "-filter_complex".to_string(), graph]);
    args.extend(["-map".to_string(), "[a]".to_string()]);
    args.extend(["-f".to_string(), "null".to_string(), "-".to_string()]);

    args
}

/// Second pass: write the converted file.
pub fn conversion_args(
    source: &Path,
    output: &Path,
    settings: &CommercialSettings,
    plan: AudioPlan,
    target: VideoTarget,
    measured: Option<&Loudness>,
) -> Vec<String> {
    let audio_tail = if settings.loudness_enabled {
        format!(
            "[pre]{},aresample={ENGINE_AUDIO_SAMPLE_RATE}[a]",
            loudnorm_filter(settings, measured)
        )
    } else {
        format!("[pre]aresample={ENGINE_AUDIO_SAMPLE_RATE}[a]")
    };
    let graph = format!(
        "[0:v:0]{}[v];{};{audio_tail}",
        video_filter(target),
        audio_graph(plan)
    );
    let mut args = base_args();

    args.extend(["-progress".to_string(), "pipe:1".to_string()]);
    args.extend(["-i".to_string(), source.to_string_lossy().to_string()]);
    args.extend(["-filter_complex".to_string(), graph]);

    for option in [
        "-map",
        "[v]",
        "-map",
        "[a]",
        "-map_metadata",
        "-1",
        "-c:v",
        "libx264",
        "-preset",
        "medium",
        "-crf",
        "18",
        "-pix_fmt",
        "yuv420p",
        "-c:a",
        "aac",
        "-b:a",
        "192k",
        "-ar",
    ] {
        args.push(option.to_string());
    }

    args.push(ENGINE_AUDIO_SAMPLE_RATE.to_string());
    args.extend(["-ac".to_string(), "2".to_string()]);
    args.extend(["-movflags".to_string(), "+faststart".to_string()]);
    args.extend(["-f".to_string(), "mp4".to_string()]);
    args.push(output.to_string_lossy().to_string());

    args
}

/// The seconds of media already processed, from an `-progress` line.
pub fn parse_progress_line(line: &str) -> Option<f64> {
    let microseconds: f64 = line.strip_prefix("out_time_us=")?.trim().parse().ok()?;

    (microseconds >= 0.0).then_some(microseconds / 1_000_000.0)
}

fn ffmpeg_command(args: &[String]) -> Command {
    // Run below the playout in priority so a conversion cannot make it stutter.
    #[cfg(unix)]
    let mut command = {
        let mut command = Command::new("nice");

        command.args(["-n", "10", FFMPEG]);

        command
    };
    #[cfg(not(unix))]
    let mut command = Command::new(FFMPEG);

    command
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);

    command
}

/// The last line FFmpeg printed, which names the reason of a failure.
fn last_error_line(stderr: &str) -> String {
    let line = stderr
        .lines()
        .rev()
        .map(str::trim)
        .find(|line| !line.is_empty())
        .unwrap_or("no details");

    line.chars().take(200).collect()
}

/// Run FFmpeg and return what it wrote to stderr. `progress` receives values
/// between `base` and `base + span` (fractions of the whole job) for a stream
/// of `total_seconds`.
async fn run_ffmpeg(
    args: &[String],
    total_seconds: f64,
    base: f64,
    span: f64,
    progress: ProgressCallback<'_>,
) -> Result<String, String> {
    let mut child = ffmpeg_command(args).spawn().map_err(|error| {
        log::error!("cannot start ffmpeg: {error}");

        "ffmpeg could not be started".to_string()
    })?;
    let stdout = child.stdout.take().ok_or("ffmpeg has no output")?;
    let mut stderr = child.stderr.take().ok_or("ffmpeg has no error output")?;
    let stderr_task = tokio::spawn(async move {
        let mut text = String::new();
        let _ = stderr.read_to_string(&mut text).await;

        text
    });
    let mut lines = BufReader::new(stdout).lines();
    let mut last_report = Instant::now();

    while let Ok(Some(line)) = lines.next_line().await {
        let Some(seconds) = parse_progress_line(&line) else {
            continue;
        };

        if last_report.elapsed() < PROGRESS_INTERVAL {
            continue;
        }

        last_report = Instant::now();

        let fraction = (seconds / total_seconds).clamp(0.0, 1.0);

        progress(((base + span * fraction) * 100.0).round() as i32);
    }

    let status = child
        .wait()
        .await
        .map_err(|_| "ffmpeg did not finish".to_string())?;
    let stderr_text = stderr_task.await.unwrap_or_default();

    if status.success() {
        return Ok(stderr_text);
    }

    log::debug!("ffmpeg failed ({status}): {stderr_text}");

    if status.code() == Some(127) && stderr_text.contains("No such file") {
        return Err("ffmpeg is not installed".to_string());
    }

    Err(format!(
        "the conversion failed: {}",
        last_error_line(&stderr_text)
    ))
}

fn partial_path(output: &Path) -> PathBuf {
    let mut name = output.file_name().unwrap_or_default().to_os_string();

    name.push(".part");

    output.with_file_name(name)
}

/// Convert `source` into `output`. The result is written to a temporary file
/// first, so the playout never sees a half-written spot. Returns the duration
/// of the converted file.
pub async fn convert_spot(
    source: &Path,
    output: &Path,
    settings: &CommercialSettings,
    target: VideoTarget,
    progress: ProgressCallback<'_>,
) -> Result<f64, String> {
    let _slot = CONVERSION_SLOT
        .acquire()
        .await
        .map_err(|_| "the conversion queue is closed".to_string())?;
    let partial = partial_path(output);
    let result = timeout(
        CONVERSION_TIMEOUT,
        convert_to_partial(source, &partial, settings, target, progress),
    )
    .await
    .unwrap_or_else(|_| Err("the conversion took too long".to_string()));

    match result {
        Ok(duration) => {
            fs::rename(&partial, output).await.map_err(|error| {
                log::error!("cannot move the converted spot into place: {error}");

                "the converted file could not be saved".to_string()
            })?;

            Ok(duration)
        }
        Err(reason) => {
            let _ = fs::remove_file(&partial).await;

            Err(reason)
        }
    }
}

async fn convert_to_partial(
    source: &Path,
    partial: &Path,
    settings: &CommercialSettings,
    target: VideoTarget,
    progress: ProgressCallback<'_>,
) -> Result<f64, String> {
    let probe = probe_media(source)
        .await
        .map_err(|_| "the file could not be read as a video".to_string())?;
    let info = SourceInfo::from_probe(&probe)?;
    let plan = plan_audio(settings.audio_mapping, &info.audio_channels)?;
    let (measured, conversion_base, conversion_span) = if settings.loudness_enabled {
        let report = run_ffmpeg(
            &measurement_args(source, settings, plan),
            info.duration,
            0.0,
            MEASUREMENT_SHARE,
            progress,
        )
        .await?;

        (
            Some(parse_loudness_report(&report)?),
            MEASUREMENT_SHARE,
            PROGRESS_DONE - MEASUREMENT_SHARE,
        )
    } else {
        (None, 0.0, PROGRESS_DONE)
    };

    run_ffmpeg(
        &conversion_args(source, partial, settings, plan, target, measured.as_ref()),
        info.duration,
        conversion_base,
        conversion_span,
        progress,
    )
    .await?;

    verify_converted(partial, info.duration).await
}

/// The converted file must play as expected before it replaces the upload.
async fn verify_converted(file: &Path, source_duration: f64) -> Result<f64, String> {
    let probe = probe_media(file)
        .await
        .map_err(|_| "the converted file could not be read".to_string())?;
    let info = SourceInfo::from_probe(&probe)
        .map_err(|reason| format!("the converted file is not valid: {reason}"))?;

    if (info.duration - source_duration).abs() > DURATION_TOLERANCE_SECONDS {
        return Err("the converted file has a different duration".to_string());
    }

    Ok(info.duration)
}

#[cfg(test)]
mod tests {
    use super::*;

    const TARGET: VideoTarget = VideoTarget {
        width: 1920,
        height: 1080,
        fps: 30.0,
    };

    fn mono(count: usize) -> Vec<Option<i64>> {
        vec![Some(1); count]
    }

    #[test]
    fn auto_uses_the_first_two_mono_tracks_as_left_and_right() {
        assert_eq!(
            plan_audio(AudioMapping::Auto, &mono(8)),
            Ok(AudioPlan::Pair(0, 1))
        );
        assert_eq!(
            plan_audio(AudioMapping::Auto, &mono(2)),
            Ok(AudioPlan::Pair(0, 1))
        );
    }

    #[test]
    fn auto_keeps_a_single_or_stereo_first_track() {
        assert_eq!(
            plan_audio(AudioMapping::Auto, &mono(1)),
            Ok(AudioPlan::Single(0))
        );
        assert_eq!(
            plan_audio(AudioMapping::Auto, &[Some(2)]),
            Ok(AudioPlan::Single(0))
        );
        assert_eq!(
            plan_audio(AudioMapping::Auto, &[Some(2), Some(2)]),
            Ok(AudioPlan::Single(0))
        );
        assert_eq!(
            plan_audio(AudioMapping::Auto, &[Some(1), Some(2)]),
            Ok(AudioPlan::Single(0)),
            "mixed layouts do not look like split stereo"
        );
        assert_eq!(
            plan_audio(AudioMapping::Auto, &[None, None]),
            Ok(AudioPlan::Single(0)),
            "unknown channel counts are not guessed"
        );
    }

    #[test]
    fn explicit_mappings_override_the_automatic_choice() {
        assert_eq!(
            plan_audio(AudioMapping::FirstStream, &mono(8)),
            Ok(AudioPlan::Single(0))
        );
        assert_eq!(
            plan_audio(AudioMapping::FirstTwoMono, &[Some(2), Some(2)]),
            Ok(AudioPlan::Pair(0, 1))
        );
        assert_eq!(
            plan_audio(AudioMapping::MixAll, &mono(4)),
            Ok(AudioPlan::MixAll(4))
        );
        assert_eq!(
            plan_audio(AudioMapping::MixAll, &mono(1)),
            Ok(AudioPlan::Single(0))
        );
    }

    #[test]
    fn mappings_report_what_is_missing() {
        assert!(plan_audio(AudioMapping::Auto, &[]).is_err());
        assert!(plan_audio(AudioMapping::FirstTwoMono, &mono(1)).is_err());
    }

    #[test]
    fn audio_graphs_match_the_plan() {
        assert_eq!(
            audio_graph(AudioPlan::Single(2)),
            "[0:a:2]aformat=channel_layouts=stereo[pre]"
        );

        let pair = audio_graph(AudioPlan::Pair(0, 1));

        assert!(pair.contains("[0:a:0]aformat=channel_layouts=mono[l]"));
        assert!(pair.contains("[0:a:1]aformat=channel_layouts=mono[r]"));
        assert!(pair.ends_with("[l][r]amerge=inputs=2,pan=stereo|c0=c0|c1=c1[pre]"));

        let mix = audio_graph(AudioPlan::MixAll(3));

        assert!(mix.contains("[m0][m1][m2]amix=inputs=3:normalize=0"));
        assert!(mix.ends_with("pan=stereo|c0=c0|c1=c0[pre]"));
    }

    #[test]
    fn video_filter_fits_the_channel_size_and_frame_rate() {
        let filter = video_filter(TARGET);

        assert!(filter.starts_with("yadif=deint=interlaced,"));
        assert!(filter.contains("scale=1920:1080:force_original_aspect_ratio=decrease"));
        assert!(filter.contains("pad=1920:1080:(ow-iw)/2:(oh-ih)/2"));
        assert!(filter.contains("fps=30"));
        assert!(filter.ends_with("format=yuv420p"));
        assert!(
            filter.contains("iw*sar"),
            "anamorphic sources are squared first"
        );
    }

    #[test]
    fn video_filter_keeps_fractional_frame_rates() {
        let filter = video_filter(VideoTarget {
            fps: 29.97,
            ..TARGET
        });

        assert!(filter.contains("fps=29.97"));
    }

    #[test]
    fn measurement_pass_prints_json_and_writes_nothing() {
        let args = measurement_args(
            Path::new("/media/in.mxf"),
            &CommercialSettings::default(),
            AudioPlan::Pair(0, 1),
        );
        let joined = args.join(" ");

        assert!(joined.contains("-vn"));
        assert!(joined.contains("loudnorm=I=-24.0:TP=-2.0:LRA=11:print_format=json"));
        assert!(joined.ends_with("-f null -"));
        assert!(args.contains(&"/media/in.mxf".to_string()));
    }

    #[test]
    fn conversion_pass_uses_the_measurement_in_linear_mode() {
        let measured = Loudness {
            input_i: -18.5,
            input_tp: -1.0,
            input_lra: 4.0,
            input_thresh: -29.0,
            target_offset: 0.5,
        };
        let args = conversion_args(
            Path::new("/media/in.mxf"),
            Path::new("/media/out.mp4.part"),
            &CommercialSettings::default(),
            AudioPlan::Single(0),
            TARGET,
            Some(&measured),
        );
        let joined = args.join(" ");

        assert!(joined.contains("measured_I=-18.50"));
        assert!(joined.contains("measured_thresh=-29.00"));
        assert!(joined.contains("offset=0.50:linear=true"));
        assert!(joined.contains("aresample=48000[a]"));
        assert!(joined.contains("-c:v libx264"));
        assert!(joined.contains("-c:a aac"));
        assert!(joined.contains("-map_metadata -1"));
        assert!(joined.contains("-f mp4 /media/out.mp4.part"));
        assert!(!joined.contains("print_format"));
    }

    #[test]
    fn conversion_without_loudness_skips_the_filter() {
        let settings = CommercialSettings {
            loudness_enabled: false,
            ..CommercialSettings::default()
        };
        let joined = conversion_args(
            Path::new("/media/in.mxf"),
            Path::new("/media/out.mp4.part"),
            &settings,
            AudioPlan::Single(0),
            TARGET,
            None,
        )
        .join(" ");

        assert!(!joined.contains("loudnorm"));
        assert!(joined.contains("[pre]aresample=48000[a]"));
    }

    #[test]
    fn custom_loudness_target_reaches_the_filter() {
        let settings = CommercialSettings {
            loudness_target_lufs: -23.0,
            loudness_true_peak_dbtp: -1.0,
            ..CommercialSettings::default()
        };

        assert!(loudnorm_filter(&settings, None).starts_with("loudnorm=I=-23.0:TP=-1.0:LRA=11"));
    }

    #[test]
    fn loudness_report_is_read_from_the_end_of_the_log() {
        let log = "ffmpeg noise {not json}\n[Parsed_loudnorm_0 @ 0x1]\n{\n\
            \t\"input_i\" : \"-18.51\",\n\t\"input_tp\" : \"-0.50\",\n\
            \t\"input_lra\" : \"3.20\",\n\t\"input_thresh\" : \"-28.90\",\n\
            \t\"output_i\" : \"-24.10\",\n\t\"target_offset\" : \"0.10\"\n}\n";
        let measured = parse_loudness_report(log).unwrap();

        assert_eq!(measured.input_i, -18.51);
        assert_eq!(measured.input_thresh, -28.9);
        assert_eq!(measured.target_offset, 0.1);
    }

    #[test]
    fn silent_or_unreadable_loudness_is_an_error() {
        let silent = "{ \"input_i\" : \"-inf\", \"input_tp\" : \"-inf\", \"input_lra\" : \"0.00\", \
            \"input_thresh\" : \"-70.00\", \"target_offset\" : \"inf\" }";

        assert_eq!(
            parse_loudness_report(silent),
            Err("the audio is silent".to_string())
        );
        assert!(parse_loudness_report("no report here").is_err());
        assert!(parse_loudness_report("{ broken").is_err());
    }

    #[test]
    fn progress_lines_give_the_processed_seconds() {
        assert_eq!(parse_progress_line("out_time_us=1500000"), Some(1.5));
        assert_eq!(parse_progress_line("out_time_us=0"), Some(0.0));
        assert_eq!(parse_progress_line("out_time_us=N/A"), None);
        assert_eq!(parse_progress_line("progress=continue"), None);
    }

    #[test]
    fn partial_files_sit_next_to_the_output() {
        assert_eq!(
            partial_path(Path::new("/s/commercials/spot-1.mp4")),
            PathBuf::from("/s/commercials/spot-1.mp4.part")
        );
    }

    #[test]
    fn the_last_error_line_names_the_failure() {
        assert_eq!(
            last_error_line("a\nb\n  Invalid data found when processing input \n\n"),
            "Invalid data found when processing input"
        );
        assert_eq!(last_error_line(""), "no details");
    }

    // The tests below run the real FFmpeg on small generated files.

    struct Workspace(PathBuf);

    impl Workspace {
        fn new() -> Self {
            let dir = std::env::temp_dir().join(format!("ffplayout-spot-{}", uuid::Uuid::new_v4()));

            std::fs::create_dir_all(&dir).unwrap();

            Self(dir)
        }

        fn path(&self, name: &str) -> PathBuf {
            self.0.join(name)
        }

        /// Generate a media file with FFmpeg's test sources.
        fn generate(&self, name: &str, args: &[&str]) -> PathBuf {
            let file = self.path(name);
            let status = std::process::Command::new("ffmpeg")
                .args(["-v", "error", "-y"])
                .args(args)
                .arg(&file)
                .status()
                .unwrap();

            assert!(status.success(), "could not generate {name}");

            file
        }
    }

    impl Drop for Workspace {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    const SMALL: VideoTarget = VideoTarget {
        width: 640,
        height: 360,
        fps: 30.0,
    };

    fn ignore_progress(_: i32) {}

    /// A five-second MXF like the commercials: MPEG-2 video and one mono
    /// PCM track per tone.
    fn mono_track_mxf(workspace: &Workspace, tones: &[u32]) -> PathBuf {
        let mut args: Vec<String> = ["-f", "lavfi", "-i", "testsrc=d=5:s=320x180:r=25"]
            .into_iter()
            .map(str::to_string)
            .collect();

        for tone in tones {
            args.extend([
                "-f".to_string(),
                "lavfi".to_string(),
                "-i".to_string(),
                format!("sine=f={tone}:r=48000:d=5"),
            ]);
        }

        args.extend(["-map".to_string(), "0:v".to_string()]);

        for index in 1..=tones.len() {
            args.extend(["-map".to_string(), format!("{index}:a")]);
        }

        args.extend(
            ["-c:v", "mpeg2video", "-c:a", "pcm_s24le", "-f", "mxf"]
                .into_iter()
                .map(str::to_string),
        );

        let refs: Vec<&str> = args.iter().map(String::as_str).collect();

        workspace.generate("commercial.mxf", &refs)
    }

    #[tokio::test]
    async fn a_multi_track_mxf_becomes_a_stereo_mp4_in_the_channel_format() {
        let workspace = Workspace::new();
        let source = mono_track_mxf(&workspace, &[440, 880]);
        let output = workspace.path("spot.mp4");
        let duration = convert_spot(
            &source,
            &output,
            &CommercialSettings::default(),
            SMALL,
            &ignore_progress,
        )
        .await
        .unwrap();

        assert!((duration - 5.0).abs() < 0.5, "duration {duration}");
        assert!(output.is_file());
        assert!(!workspace.path("spot.mp4.part").exists());

        let probe = probe_media(&output).await.unwrap();

        assert_eq!(probe.video.len(), 1);
        assert_eq!(probe.video[0].width, Some(640));
        assert_eq!(probe.video[0].height, Some(360));
        assert!(probe.video[0].frame_rate.starts_with("30"));
        assert_eq!(probe.video[0].codec_name.as_deref(), Some("h264"));
        assert_eq!(probe.audio.len(), 1);
        assert_eq!(probe.audio[0].channels, Some(2));
        assert_eq!(probe.audio[0].codec_name.as_deref(), Some("aac"));
        assert_eq!(probe.audio[0].sample_rate, Some(48_000));
    }

    #[tokio::test]
    async fn the_picture_is_letterboxed_not_stretched() {
        let workspace = Workspace::new();
        let source = workspace.generate(
            "square.mp4",
            &[
                "-f",
                "lavfi",
                "-i",
                "testsrc=d=2:s=400x400:r=25",
                "-f",
                "lavfi",
                "-i",
                "sine=f=440:r=48000:d=2",
                "-c:v",
                "libx264",
                "-c:a",
                "aac",
            ],
        );
        let output = workspace.path("spot.mp4");

        convert_spot(
            &source,
            &output,
            &CommercialSettings::default(),
            SMALL,
            &ignore_progress,
        )
        .await
        .unwrap();

        let probe = probe_media(&output).await.unwrap();

        assert_eq!(
            (probe.video[0].width, probe.video[0].height),
            (Some(640), Some(360)),
            "the output always has the channel size"
        );
    }

    #[tokio::test]
    async fn the_loudness_of_the_result_follows_the_target() {
        let workspace = Workspace::new();
        let source = mono_track_mxf(&workspace, &[440]);
        let settings = CommercialSettings {
            loudness_target_lufs: -20.0,
            ..CommercialSettings::default()
        };
        let output = workspace.path("spot.mp4");

        convert_spot(&source, &output, &settings, SMALL, &ignore_progress)
            .await
            .unwrap();

        let report = run_ffmpeg(
            &measurement_args(
                &output,
                &settings,
                plan_audio(AudioMapping::Auto, &[Some(2)]).unwrap(),
            ),
            5.0,
            0.0,
            1.0,
            &ignore_progress,
        )
        .await
        .unwrap();
        let measured = parse_loudness_report(&report).unwrap();

        assert!(
            (measured.input_i - -20.0).abs() < 1.5,
            "integrated loudness {} LUFS",
            measured.input_i
        );
    }

    #[tokio::test]
    async fn loudness_can_be_switched_off() {
        let workspace = Workspace::new();
        let source = mono_track_mxf(&workspace, &[440]);
        let settings = CommercialSettings {
            loudness_enabled: false,
            ..CommercialSettings::default()
        };
        let output = workspace.path("spot.mp4");

        assert!(
            convert_spot(&source, &output, &settings, SMALL, &ignore_progress)
                .await
                .is_ok()
        );
        assert!(output.is_file());
    }

    #[tokio::test]
    async fn progress_is_reported_while_converting() {
        use std::sync::Mutex;

        let workspace = Workspace::new();
        let source = mono_track_mxf(&workspace, &[440, 880]);
        let output = workspace.path("spot.mp4");
        let seen = Mutex::new(Vec::new());
        let record = |value: i32| seen.lock().unwrap().push(value);

        convert_spot(
            &source,
            &output,
            &CommercialSettings::default(),
            SMALL,
            &record,
        )
        .await
        .unwrap();

        assert!(
            seen.lock()
                .unwrap()
                .iter()
                .all(|value| (0..=100).contains(value)),
            "progress stays within 0 to 100"
        );
    }

    #[tokio::test]
    async fn silent_audio_is_refused_and_leaves_nothing_behind() {
        let workspace = Workspace::new();
        let source = workspace.generate(
            "silent.mp4",
            &[
                "-f",
                "lavfi",
                "-i",
                "testsrc=d=3:s=320x180:r=25",
                "-f",
                "lavfi",
                "-i",
                "anullsrc=r=48000:cl=stereo",
                "-t",
                "3",
                "-c:v",
                "libx264",
                "-c:a",
                "aac",
            ],
        );
        let output = workspace.path("spot.mp4");
        let result = convert_spot(
            &source,
            &output,
            &CommercialSettings::default(),
            SMALL,
            &ignore_progress,
        )
        .await;

        assert_eq!(result, Err("the audio is silent".to_string()));
        assert!(!output.exists());
        assert!(!workspace.path("spot.mp4.part").exists());
    }

    #[tokio::test]
    async fn files_without_audio_or_media_are_refused() {
        let workspace = Workspace::new();
        let video_only = workspace.generate(
            "video.mp4",
            &[
                "-f",
                "lavfi",
                "-i",
                "testsrc=d=2:s=320x180:r=25",
                "-c:v",
                "libx264",
            ],
        );
        let text = workspace.path("notes.mp4");

        std::fs::write(&text, "this is not a video").unwrap();

        for (file, expected) in [
            (video_only, "the file has no audio track"),
            (text, "the file could not be read as a video"),
        ] {
            let result = convert_spot(
                &file,
                &workspace.path("out.mp4"),
                &CommercialSettings::default(),
                SMALL,
                &ignore_progress,
            )
            .await;

            assert_eq!(result, Err(expected.to_string()));
        }
    }

    #[tokio::test]
    async fn asking_for_two_tracks_from_a_single_track_file_fails_clearly() {
        let workspace = Workspace::new();
        let source = mono_track_mxf(&workspace, &[440]);
        let settings = CommercialSettings {
            audio_mapping: AudioMapping::FirstTwoMono,
            ..CommercialSettings::default()
        };
        let result = convert_spot(
            &source,
            &workspace.path("out.mp4"),
            &settings,
            SMALL,
            &ignore_progress,
        )
        .await;

        assert_eq!(
            result,
            Err("the file has only one audio track, two are needed".to_string())
        );
    }

    /// The samples of one channel of the converted file.
    fn channel_samples(file: &Path, channel: usize) -> Vec<i16> {
        let output = std::process::Command::new("ffmpeg")
            .args(["-v", "error", "-i"])
            .arg(file)
            .args(["-af", &format!("pan=mono|c0=c{channel}")])
            .args(["-f", "s16le", "-ac", "1", "-"])
            .output()
            .unwrap();

        assert!(output.status.success());

        output
            .stdout
            .chunks_exact(2)
            .map(|bytes| i16::from_le_bytes([bytes[0], bytes[1]]))
            .collect()
    }

    /// How different the two channels are: 0 for identical channels, about
    /// 1.4 for two unrelated signals of the same level.
    fn channel_difference(file: &Path) -> f64 {
        let left = channel_samples(file, 0);
        let right = channel_samples(file, 1);
        let length = left.len().min(right.len());
        let energy = |samples: &[f64]| samples.iter().map(|value| value * value).sum::<f64>();
        let left: Vec<f64> = left[..length].iter().map(|v| f64::from(*v)).collect();
        let right: Vec<f64> = right[..length].iter().map(|v| f64::from(*v)).collect();
        let difference: Vec<f64> = left.iter().zip(&right).map(|(l, r)| l - r).collect();

        (energy(&difference) / energy(&left).max(1.0)).sqrt()
    }

    async fn convert_with_mapping(
        workspace: &Workspace,
        source: &Path,
        mapping: AudioMapping,
    ) -> PathBuf {
        let settings = CommercialSettings {
            audio_mapping: mapping,
            ..CommercialSettings::default()
        };
        let output = workspace.path(&format!("{mapping:?}.mp4"));

        convert_spot(source, &output, &settings, SMALL, &ignore_progress)
            .await
            .unwrap();

        output
    }

    #[tokio::test]
    async fn separate_tracks_stay_on_separate_channels() {
        let workspace = Workspace::new();
        let source = mono_track_mxf(&workspace, &[440, 880]);

        for mapping in [AudioMapping::Auto, AudioMapping::FirstTwoMono] {
            let output = convert_with_mapping(&workspace, &source, mapping).await;
            let difference = channel_difference(&output);

            assert!(
                difference > 0.5,
                "{mapping:?}: left and right should differ, difference {difference}"
            );
        }
    }

    #[tokio::test]
    async fn a_single_track_or_a_mix_feeds_both_channels_equally() {
        let workspace = Workspace::new();
        let source = mono_track_mxf(&workspace, &[440, 880]);

        for mapping in [AudioMapping::FirstStream, AudioMapping::MixAll] {
            let output = convert_with_mapping(&workspace, &source, mapping).await;
            let difference = channel_difference(&output);

            assert!(
                difference < 0.2,
                "{mapping:?}: both channels should carry the same signal, difference {difference}"
            );
        }
    }
}
