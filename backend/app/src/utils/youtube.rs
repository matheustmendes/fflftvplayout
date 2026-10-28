//! Import of videos from the YouTube channels of the client, with `yt-dlp`.
//!
//! The server downloads the videos by itself, so nothing goes through the
//! computer of the operator. Two rules hold everywhere:
//!
//! - only addresses of YouTube are accepted, never a path or another site;
//! - only videos of a channel on the list of the playout channel are kept. The
//!   address is checked before the download, and `yt-dlp` checks every video
//!   again (`--match-filter`), so a playlist with a video of somebody else
//!   cannot bring it in.

use std::{
    collections::VecDeque,
    path::{Path, PathBuf},
    process::Stdio,
    sync::{
        LazyLock, Mutex, PoisonError,
        atomic::{AtomicU64, Ordering},
    },
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use serde::{Deserialize, Serialize};
use serde_json::Value;
use tokio::{
    io::{AsyncBufReadExt, AsyncRead, BufReader},
    process::Command,
    sync::mpsc,
    time::timeout,
};
use tokio_util::sync::CancellationToken;

use crate::utils::errors::ServiceError;

const PROGRAM_ENV: &str = "FFPLAYOUT_YTDLP";
const COOKIES_ENV: &str = "FFPLAYOUT_YOUTUBE_COOKIES";
const DEFAULT_COOKIES: &str = "/etc/ffplayout/youtube-cookies.txt";
const PROBE_TIMEOUT: Duration = Duration::from_secs(180);
/// Where `deploy/youtube-import.sh` keeps its list of downloaded videos too,
/// so a video brought by one of them is not downloaded again by the other.
pub const ARCHIVE_PATH: &str = "youtube/.baixados.txt";
pub const DEFAULT_FOLDER: &str = "youtube";
const HEIGHTS: [u32; 6] = [360, 480, 720, 1080, 1440, 2160];
const KEPT_JOBS: usize = 20;
const KEPT_ERRORS: usize = 10;

/// Hosts a video address may have.
const YOUTUBE_HOSTS: [&str; 5] = [
    "youtube.com",
    "www.youtube.com",
    "m.youtube.com",
    "music.youtube.com",
    "youtu.be",
];

/// How to run `yt-dlp`.
#[derive(Debug, Clone)]
pub struct YoutubeTool {
    pub program: String,
    pub cookies: Option<PathBuf>,
}

impl YoutubeTool {
    /// `FFPLAYOUT_YTDLP` names the program (default `yt-dlp`), and a cookies
    /// file at `FFPLAYOUT_YOUTUBE_COOKIES` (or the default place) is used when
    /// it exists: YouTube sometimes refuses the address of a server until it
    /// sees a logged-in browser.
    pub fn from_env() -> Self {
        let program = std::env::var(PROGRAM_ENV).unwrap_or_else(|_| "yt-dlp".to_string());
        let cookies = std::env::var(COOKIES_ENV)
            .map_or_else(|_| PathBuf::from(DEFAULT_COOKIES), PathBuf::from);

        Self {
            program,
            cookies: cookies.is_file().then_some(cookies),
        }
    }

    fn command(&self) -> Command {
        // Below the playout in priority: a download must not make it stutter.
        #[cfg(unix)]
        let mut command = {
            let mut command = Command::new("nice");

            command.args(["-n", "10", &self.program]);

            command
        };
        #[cfg(not(unix))]
        let mut command = Command::new(&self.program);

        command.arg("--no-warnings");

        if let Some(runtime) = js_runtime() {
            command.args(["--js-runtimes", runtime]);
        }

        if let Some(cookies) = &self.cookies {
            command.arg("--cookies").arg(cookies);
        }

        command
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true);

        command
    }

    /// The version of the program, or `None` when it is not installed.
    pub async fn version(&self) -> Option<String> {
        let mut command = Command::new(&self.program);

        command
            .arg("--version")
            .stdin(Stdio::null())
            .stderr(Stdio::null())
            .kill_on_drop(true);

        let output = timeout(Duration::from_secs(10), command.output())
            .await
            .ok()?
            .ok()?;

        output
            .status
            .success()
            .then(|| String::from_utf8_lossy(&output.stdout).trim().to_string())
    }
}

/// YouTube asks for a JavaScript interpreter to hand out every format.
fn js_runtime() -> Option<&'static str> {
    ["deno", "node"].into_iter().find(|name| {
        std::env::var_os("PATH").is_some_and(|paths| {
            std::env::split_paths(&paths).any(|directory| directory.join(name).is_file())
        })
    })
}

/// A YouTube address, cleaned. Anything else is refused.
pub fn validate_url(input: &str) -> Result<String, String> {
    let url = input.trim();
    let rest = url
        .strip_prefix("https://")
        .or_else(|| url.strip_prefix("http://"))
        .ok_or("Cole o endereço completo do YouTube, começando com https://")?;
    let host = rest
        .split(['/', '?', '#'])
        .next()
        .unwrap_or_default()
        .to_ascii_lowercase();

    if !YOUTUBE_HOSTS.contains(&host.as_str()) {
        return Err("Só aceito endereços do YouTube (youtube.com ou youtu.be).".to_string());
    }

    if url.chars().any(|c| c.is_control() || c.is_whitespace()) || url.len() > 500 {
        return Err("O endereço tem caracteres que não pode ter.".to_string());
    }

    Ok(url.to_string())
}

/// Where the videos go when the person does not choose: a playlist gets a
/// folder of its own, a collection makes one folder per playlist inside the
/// default one, and a single video goes straight to the default folder.
pub fn default_folder(probe: &Probe) -> String {
    match probe.kind {
        SourceKind::Playlist if !probe.title.trim().is_empty() => {
            clean_folder(&format!("{DEFAULT_FOLDER}/{}", probe.title))
                .unwrap_or_else(|_| DEFAULT_FOLDER.to_string())
        }
        _ => DEFAULT_FOLDER.to_string(),
    }
}

/// A folder of the storage, as a clean relative path: no way out of the
/// storage, no characters a file system dislikes.
pub fn clean_folder(input: &str) -> Result<String, String> {
    let mut parts = Vec::new();

    for part in input.replace('\\', "/").split('/') {
        let part = part.trim();

        match part {
            "" | "." => continue,
            ".." => return Err("A pasta não pode ter \"..\".".to_string()),
            _ => {}
        }

        let cleaned: String = part
            .chars()
            .map(|c| {
                if c.is_control() || ":*?\"<>|".contains(c) {
                    '_'
                } else {
                    c
                }
            })
            .take(100)
            .collect();

        parts.push(cleaned.trim_end_matches('.').to_string());
    }

    if parts.is_empty() {
        return Ok(DEFAULT_FOLDER.to_string());
    }

    Ok(parts.join("/"))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SourceKind {
    Video,
    Playlist,
    /// A page that lists playlists, such as `/@channel/playlists`.
    Collection,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct VideoEntry {
    pub id: String,
    pub title: String,
    pub duration: Option<f64>,
}

/// What an address is, and whose.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Probe {
    pub kind: SourceKind,
    pub title: String,
    pub owner_id: String,
    pub owner_name: String,
    /// The videos of a video or playlist; empty for a collection.
    pub videos: Vec<VideoEntry>,
    /// The playlists of a collection.
    pub playlists: Vec<PlaylistEntry>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct PlaylistEntry {
    pub url: String,
    pub title: String,
}

fn text(value: &Value, key: &str) -> String {
    value
        .get(key)
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string()
}

/// Reads the JSON `yt-dlp -J --flat-playlist` prints.
pub fn parse_probe(json: &str) -> Result<Probe, String> {
    let root: Value = serde_json::from_str(json)
        .map_err(|error| format!("Não entendi a resposta do yt-dlp: {error}"))?;
    let owner_id = text(&root, "channel_id");
    let owner_name = [text(&root, "channel"), text(&root, "uploader")]
        .into_iter()
        .find(|name| !name.is_empty())
        .unwrap_or_default();
    let title = [text(&root, "title"), text(&root, "id")]
        .into_iter()
        .find(|name| !name.is_empty())
        .unwrap_or_default();

    if root.get("_type").and_then(Value::as_str) != Some("playlist") {
        return Ok(Probe {
            kind: SourceKind::Video,
            videos: vec![VideoEntry {
                id: text(&root, "id"),
                title: title.clone(),
                duration: root.get("duration").and_then(Value::as_f64),
            }],
            title,
            owner_id,
            owner_name,
            playlists: Vec::new(),
        });
    }

    let entries = root
        .get("entries")
        .and_then(Value::as_array)
        .map_or(&[][..], Vec::as_slice);
    let is_playlist_link = |entry: &Value| text(entry, "url").contains("playlist?list=");

    if !entries.is_empty() && entries.iter().all(is_playlist_link) {
        return Ok(Probe {
            kind: SourceKind::Collection,
            title,
            owner_id,
            owner_name,
            videos: Vec::new(),
            playlists: entries
                .iter()
                .map(|entry| PlaylistEntry {
                    url: text(entry, "url"),
                    title: text(entry, "title"),
                })
                .collect(),
        });
    }

    Ok(Probe {
        kind: SourceKind::Playlist,
        title,
        owner_id,
        owner_name,
        videos: entries
            .iter()
            .filter(|entry| !text(entry, "id").is_empty())
            .map(|entry| VideoEntry {
                id: text(entry, "id"),
                title: text(entry, "title"),
                duration: entry.get("duration").and_then(Value::as_f64),
            })
            .collect(),
        playlists: Vec::new(),
    })
}

/// What `yt-dlp` said, in words for the person at the screen.
pub fn friendly_error(stderr: &str) -> String {
    let lowered = stderr.to_lowercase();

    if lowered.contains("not a bot") || lowered.contains("sign in to confirm") {
        return "O YouTube recusou o endereço do servidor (\"confirme que você não é um robô\"). \
                Peça ao administrador para colocar o arquivo de cookies do YouTube no servidor."
            .to_string();
    }

    if lowered.contains("private video") || lowered.contains("members-only") {
        return "O vídeo é privado ou exclusivo para membros; o servidor não consegue baixá-lo."
            .to_string();
    }

    if lowered.contains("video unavailable") || lowered.contains("not available") {
        return "O vídeo não está disponível no YouTube.".to_string();
    }

    if lowered.contains("unable to download") || lowered.contains("urlopen error") {
        return "Não consegui falar com o YouTube. Confira a internet do servidor.".to_string();
    }

    let line = stderr
        .lines()
        .rev()
        .map(str::trim)
        .find(|line| !line.is_empty())
        .unwrap_or("sem detalhes");

    format!(
        "O yt-dlp falhou: {}",
        line.chars().take(200).collect::<String>()
    )
}

fn not_installed() -> ServiceError {
    ServiceError::ServiceUnavailable(
        "O yt-dlp não está instalado no servidor. Rode: sudo deploy/youtube-import.sh update"
            .to_string(),
    )
}

/// Asks YouTube what an address is, without downloading anything.
pub async fn probe(tool: &YoutubeTool, url: &str) -> Result<Probe, ServiceError> {
    let mut command = tool.command();

    command.args(["-J", "--flat-playlist", "--", url]);

    let child = command.spawn().map_err(|_| not_installed())?;
    let output = timeout(PROBE_TIMEOUT, child.wait_with_output())
        .await
        .map_err(|_| {
            ServiceError::ServiceUnavailable("O YouTube demorou demais para responder.".to_string())
        })?
        .map_err(|_| ServiceError::InternalServerError)?;

    if !output.status.success() {
        return Err(ServiceError::BadRequest(friendly_error(
            &String::from_utf8_lossy(&output.stderr),
        )));
    }

    parse_probe(&String::from_utf8_lossy(&output.stdout)).map_err(ServiceError::BadRequest)
}

/// The best H.264/AAC file up to `height`, in MP4: what the playout reads
/// without effort. Falls back to what exists when YouTube has no H.264 there.
fn format_selector(height: u32) -> String {
    let h = height;

    format!(
        "bv*[height<={h}][vcodec^=avc1]+ba[acodec^=mp4a]/bv*[height<={h}][ext=mp4]+ba[ext=m4a]\
         /b[height<={h}][ext=mp4]/bv*[height<={h}]+ba/b[height<={h}]"
    )
}

pub fn valid_height(height: u32) -> bool {
    HEIGHTS.contains(&height)
}

/// The arguments of one download. `output_dir` is where the files go and
/// `allowed` the channels whose videos are kept.
pub fn download_args(
    output_dir: &Path,
    archive: &Path,
    allowed: &[String],
    height: u32,
    subfolder_by_playlist: bool,
    url: &str,
) -> Vec<String> {
    let template = if subfolder_by_playlist {
        "%(playlist_title,Avulsos)s/%(title).150B [%(id)s].%(ext)s"
    } else {
        "%(title).150B [%(id)s].%(ext)s"
    };
    let mut args: Vec<String> = [
        "--newline",
        "--no-simulate",
        "--no-quiet",
        "--ignore-errors",
        "--windows-filenames",
        "--no-mtime",
        "--merge-output-format",
        "mp4",
        "--retries",
        "10",
        "--fragment-retries",
        "10",
        "--concurrent-fragments",
        "4",
        "--sleep-interval",
        "3",
    ]
    .iter()
    .map(ToString::to_string)
    .collect();

    args.extend([
        "--format".to_string(),
        format_selector(height),
        "--download-archive".to_string(),
        archive.to_string_lossy().to_string(),
        "--output".to_string(),
        output_dir.join(template).to_string_lossy().to_string(),
        "--progress-template".to_string(),
        "download:PROGRESS|%(progress.downloaded_bytes)s|%(progress.total_bytes)s|%(progress.total_bytes_estimate)s"
            .to_string(),
        "--print".to_string(),
        "before_dl:ITEM|%(playlist_index|1)s|%(n_entries|1)s|%(id)s|%(title)s".to_string(),
        "--print".to_string(),
        "after_move:DONE|%(id)s|%(filepath)s".to_string(),
    ]);

    // Repeated filters are alternatives: the video passes if it is of any of
    // the allowed channels.
    for channel in allowed {
        args.extend([
            "--match-filters".to_string(),
            format!("channel_id = {channel}"),
        ]);
    }

    args.extend(["--".to_string(), url.to_string()]);

    args
}

/// A line the download printed that the job cares about.
#[derive(Debug, Clone, PartialEq)]
pub enum Event {
    Item {
        index: u32,
        total: u32,
        title: String,
    },
    Progress {
        percent: f64,
    },
    Done {
        path: String,
    },
    AlreadyHave,
    /// The video is of a channel that is not on the list.
    Rejected,
    Failed {
        message: String,
    },
}

pub fn parse_line(line: &str) -> Option<Event> {
    let line = line.trim();

    if let Some(rest) = line.strip_prefix("ITEM|") {
        let mut parts = rest.splitn(4, '|');
        let index = parts.next()?.parse().ok()?;
        let total = parts.next()?.parse().ok()?;
        let _id = parts.next()?;
        let title = parts.next().unwrap_or_default().to_string();

        return Some(Event::Item {
            index,
            total,
            title,
        });
    }

    if let Some(rest) = line.strip_prefix("PROGRESS|") {
        let mut parts = rest.split('|');
        let downloaded: f64 = parts.next()?.parse().ok()?;
        let total = parts
            .filter_map(|part| part.parse::<f64>().ok())
            .find(|size| *size > 0.0)?;

        return Some(Event::Progress {
            percent: (downloaded / total * 100.0).clamp(0.0, 100.0),
        });
    }

    if let Some(rest) = line.strip_prefix("DONE|") {
        let (_, path) = rest.split_once('|')?;

        return Some(Event::Done {
            path: path.to_string(),
        });
    }

    if line.contains("has already been recorded in the archive") {
        return Some(Event::AlreadyHave);
    }

    if line.contains("does not pass filter") {
        return Some(Event::Rejected);
    }

    line.strip_prefix("ERROR:").map(|message| Event::Failed {
        message: friendly_error(message),
    })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum JobStatus {
    Running,
    Done,
    Failed,
    Cancelled,
}

#[derive(Debug, Clone, Serialize)]
pub struct ImportJob {
    pub id: String,
    pub channel_id: i32,
    pub url: String,
    pub folder: String,
    pub title: String,
    pub status: JobStatus,
    /// Playlist being downloaded (1-based) and how many there are.
    pub playlist_index: u32,
    pub playlist_total: u32,
    /// Video being downloaded within the playlist, and how many it has.
    pub item_index: u32,
    pub item_total: u32,
    pub current: String,
    pub percent: f64,
    pub downloaded: u32,
    pub already_had: u32,
    /// Videos left out because they are of a channel that is not allowed.
    pub rejected: u32,
    pub failed: u32,
    pub errors: Vec<String>,
    pub started_at: i64,
    pub finished_at: Option<i64>,
}

struct JobEntry {
    job: ImportJob,
    cancel: CancellationToken,
}

static JOBS: LazyLock<Mutex<VecDeque<JobEntry>>> = LazyLock::new(|| Mutex::new(VecDeque::new()));
static NEXT_JOB: AtomicU64 = AtomicU64::new(1);

fn now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |elapsed| elapsed.as_secs() as i64)
}

fn with_jobs<T>(action: impl FnOnce(&mut VecDeque<JobEntry>) -> T) -> T {
    action(&mut JOBS.lock().unwrap_or_else(PoisonError::into_inner))
}

/// The jobs of a channel, the newest first.
pub fn jobs_of(channel_id: i32) -> Vec<ImportJob> {
    with_jobs(|jobs| {
        jobs.iter()
            .filter(|entry| entry.job.channel_id == channel_id)
            .map(|entry| entry.job.clone())
            .collect()
    })
}

/// Stops a running job of the channel. Returns whether there was one.
pub fn cancel_job(channel_id: i32, id: &str) -> bool {
    with_jobs(|jobs| {
        jobs.iter()
            .find(|entry| {
                entry.job.id == id
                    && entry.job.channel_id == channel_id
                    && entry.job.status == JobStatus::Running
            })
            .map(|entry| entry.cancel.cancel())
            .is_some()
    })
}

fn update_job(id: &str, change: impl FnOnce(&mut ImportJob)) {
    with_jobs(|jobs| {
        if let Some(entry) = jobs.iter_mut().find(|entry| entry.job.id == id) {
            change(&mut entry.job);
        }
    });
}

/// What the job needs to run, all checked by the caller.
pub struct ImportRequest {
    pub channel_id: i32,
    pub url: String,
    pub title: String,
    pub probe: Probe,
    pub storage_root: PathBuf,
    /// Relative to the storage, already cleaned.
    pub folder: String,
    pub allowed: Vec<String>,
    pub height: u32,
}

/// Registers the job and runs it in the background. One download at a time:
/// two would fight for the network the broadcast uses.
pub fn start_import(tool: YoutubeTool, request: ImportRequest) -> Result<ImportJob, ServiceError> {
    let id = NEXT_JOB.fetch_add(1, Ordering::Relaxed).to_string();
    let cancel = CancellationToken::new();
    let playlist_total = match request.probe.kind {
        SourceKind::Collection => request.probe.playlists.len() as u32,
        _ => 1,
    };
    let job = ImportJob {
        id,
        channel_id: request.channel_id,
        url: request.url.clone(),
        folder: request.folder.clone(),
        title: request.title.clone(),
        status: JobStatus::Running,
        playlist_index: 0,
        playlist_total,
        item_index: 0,
        item_total: 0,
        current: String::new(),
        percent: 0.0,
        downloaded: 0,
        already_had: 0,
        rejected: 0,
        failed: 0,
        errors: Vec::new(),
        started_at: now(),
        finished_at: None,
    };

    let started = with_jobs(|jobs| {
        if jobs
            .iter()
            .any(|entry| entry.job.status == JobStatus::Running)
        {
            return false;
        }

        jobs.push_front(JobEntry {
            job: job.clone(),
            cancel: cancel.clone(),
        });
        jobs.truncate(KEPT_JOBS);

        true
    });

    if !started {
        return Err(ServiceError::Conflict(
            "Já há uma importação em andamento. Espere ela terminar.".to_string(),
        ));
    }

    tokio::spawn(run_import(tool, request, job.id.clone(), cancel));

    Ok(job)
}

/// The downloads of a job: one per playlist of a collection, else one.
fn targets(request: &ImportRequest) -> Vec<(String, bool)> {
    match request.probe.kind {
        SourceKind::Collection => request
            .probe
            .playlists
            .iter()
            .map(|playlist| (playlist.url.clone(), true))
            .collect(),
        _ => vec![(request.url.clone(), false)],
    }
}

async fn run_import(
    tool: YoutubeTool,
    request: ImportRequest,
    id: String,
    cancel: CancellationToken,
) {
    let output_dir = request.storage_root.join(&request.folder);
    let archive = request.storage_root.join(ARCHIVE_PATH);
    let mut result = Ok(());

    for (position, (url, by_playlist)) in targets(&request).into_iter().enumerate() {
        update_job(&id, |job| {
            job.playlist_index = position as u32 + 1;
            job.item_index = 0;
            job.item_total = 0;
        });

        let args = download_args(
            &output_dir,
            &archive,
            &request.allowed,
            request.height,
            by_playlist,
            &url,
        );

        result = download(&tool, &args, &id, &cancel, &output_dir, &archive).await;

        if result.is_err() {
            break;
        }
    }

    let cancelled = cancel.is_cancelled();

    update_job(&id, |job| {
        job.finished_at = Some(now());
        job.percent = 0.0;
        job.current.clear();

        if cancelled {
            job.status = JobStatus::Cancelled;
        } else if let Err(message) = result {
            job.status = JobStatus::Failed;
            job.errors.push(message);
        } else if job.downloaded == 0
            && job.already_had == 0
            && (job.failed > 0 || job.rejected > 0)
        {
            job.status = JobStatus::Failed;
        } else {
            job.status = JobStatus::Done;
        }
    });
}

/// Pipes a stream line by line into a channel.
fn forward_lines(stream: impl AsyncRead + Unpin + Send + 'static, sender: mpsc::Sender<String>) {
    tokio::spawn(async move {
        let mut lines = BufReader::new(stream).lines();

        while let Ok(Some(line)) = lines.next_line().await {
            if sender.send(line).await.is_err() {
                break;
            }
        }
    });
}

async fn download(
    tool: &YoutubeTool,
    args: &[String],
    id: &str,
    cancel: &CancellationToken,
    output_dir: &Path,
    archive: &Path,
) -> Result<(), String> {
    for directory in [Some(output_dir), archive.parent()].into_iter().flatten() {
        tokio::fs::create_dir_all(directory)
            .await
            .map_err(|error| {
                format!(
                    "Não consegui criar a pasta {}: {error}",
                    directory.display()
                )
            })?;
    }

    let mut command = tool.command();

    command.args(args);

    let mut child = command
        .spawn()
        .map_err(|_| "O yt-dlp não está instalado no servidor.".to_string())?;
    let (sender, mut lines) = mpsc::channel::<String>(256);

    if let Some(stdout) = child.stdout.take() {
        forward_lines(stdout, sender.clone());
    }

    if let Some(stderr) = child.stderr.take() {
        forward_lines(stderr, sender.clone());
    }

    drop(sender);

    loop {
        tokio::select! {
            () = cancel.cancelled() => {
                let _ = child.kill().await;

                return Ok(());
            }
            line = lines.recv() => {
                let Some(line) = line else { break };

                if let Some(event) = parse_line(&line) {
                    apply_event(id, event);
                }
            }
        }
    }

    // Success or not, what matters is in the counters: `--ignore-errors` makes
    // the exit code 1 when a single video failed.
    let _ = child.wait().await;

    Ok(())
}

fn apply_event(id: &str, event: Event) {
    update_job(id, |job| match event {
        Event::Item {
            index,
            total,
            title,
        } => {
            job.item_index = index;
            job.item_total = total;
            job.current = title;
            job.percent = 0.0;
        }
        Event::Progress { percent } => job.percent = percent,
        Event::Done { .. } => {
            job.downloaded += 1;
            job.percent = 100.0;
        }
        Event::AlreadyHave => job.already_had += 1,
        Event::Rejected => job.rejected += 1,
        Event::Failed { message } => {
            job.failed += 1;

            if job.errors.len() < KEPT_ERRORS && !job.errors.contains(&message) {
                job.errors.push(message);
            }
        }
    });
}

/// The request body to start an import.
#[derive(Debug, Deserialize)]
pub struct ImportBody {
    pub url: String,
    #[serde(default)]
    pub folder: String,
    #[serde(default = "default_height")]
    pub height: u32,
}

fn default_height() -> u32 {
    1080
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_addresses_of_youtube_are_accepted() {
        assert!(validate_url("https://www.youtube.com/@LFTVOficial/playlists").is_ok());
        assert!(validate_url(" https://youtu.be/abc123 ").is_ok());
        assert!(validate_url("https://m.youtube.com/watch?v=abc").is_ok());

        for bad in [
            "http://example.com/video.mp4",
            "https://youtube.com.evil.example/watch?v=1",
            "https://evil.example/youtube.com",
            "file:///etc/passwd",
            "/etc/passwd",
            "youtube.com/watch?v=1",
            "https://www.youtube.com/watch?v=1 --exec id",
            "https://www.youtube.com/watch?v=1\n--exec id",
        ] {
            assert!(validate_url(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn a_folder_stays_inside_the_storage() {
        assert_eq!(clean_folder("").unwrap(), "youtube");
        assert_eq!(clean_folder("  ").unwrap(), "youtube");
        assert_eq!(clean_folder("youtube/Jornal").unwrap(), "youtube/Jornal");
        assert_eq!(clean_folder("/youtube//Jornal/").unwrap(), "youtube/Jornal");
        assert_eq!(clean_folder("a:b/c*d").unwrap(), "a_b/c_d");
        assert_eq!(clean_folder("a\\b").unwrap(), "a/b");
        assert!(clean_folder("../fora").is_err());
        assert!(clean_folder("youtube/../../fora").is_err());
    }

    #[test]
    fn a_playlist_is_read_with_its_owner() {
        let json = r#"{"_type":"playlist","id":"PL1","title":"LFTV pocket","channel_id":"UC123","channel":"LFTV Oficial",
            "entries":[{"id":"a1","title":"Um","duration":260.0},{"id":"b2","title":"Dois","duration":null}]}"#;
        let probe = parse_probe(json).unwrap();

        assert_eq!(probe.kind, SourceKind::Playlist);
        assert_eq!(probe.owner_id, "UC123");
        assert_eq!(probe.owner_name, "LFTV Oficial");
        assert_eq!(probe.videos.len(), 2);
        assert_eq!(probe.videos[0].duration, Some(260.0));
        assert_eq!(probe.videos[1].duration, None);
    }

    #[test]
    fn a_page_of_playlists_is_a_collection() {
        let json = r#"{"_type":"playlist","title":"LFTV Oficial - Playlists","channel_id":"UC123",
            "entries":[{"url":"https://www.youtube.com/playlist?list=PL1","title":"Carnaval"},
                       {"url":"https://www.youtube.com/playlist?list=PL2","title":"Natal"}]}"#;
        let probe = parse_probe(json).unwrap();

        assert_eq!(probe.kind, SourceKind::Collection);
        assert_eq!(probe.playlists.len(), 2);
        assert_eq!(probe.playlists[1].title, "Natal");
        assert!(probe.videos.is_empty());
    }

    #[test]
    fn one_video_is_read_with_its_owner() {
        let json = r#"{"id":"abc","title":"Jornal","duration":600,"channel_id":"UC123","uploader":"LFTV"}"#;
        let probe = parse_probe(json).unwrap();

        assert_eq!(probe.kind, SourceKind::Video);
        assert_eq!(probe.owner_id, "UC123");
        assert_eq!(probe.owner_name, "LFTV");
        assert_eq!(probe.videos[0].id, "abc");
        assert!(parse_probe("not json").is_err());
    }

    #[test]
    fn the_download_only_keeps_videos_of_the_allowed_channels() {
        let args = download_args(
            Path::new("/storage/youtube"),
            Path::new("/storage/youtube/.baixados.txt"),
            &["UC111".to_string(), "UC222".to_string()],
            1080,
            true,
            "https://www.youtube.com/playlist?list=PL1",
        );
        let filters: Vec<&String> = args
            .iter()
            .zip(args.iter().skip(1))
            .filter(|(flag, _)| flag.as_str() == "--match-filters")
            .map(|(_, value)| value)
            .collect();

        assert_eq!(filters, ["channel_id = UC111", "channel_id = UC222"]);
        assert_eq!(args[args.len() - 2], "--");
        assert_eq!(
            args.last().unwrap(),
            "https://www.youtube.com/playlist?list=PL1"
        );
        assert!(
            args.iter()
                .any(|arg| arg.contains("/storage/youtube/%(playlist_title,Avulsos)s/"))
        );
        assert!(args.iter().any(|arg| arg.contains("height<=1080")));
    }

    #[test]
    fn the_lines_of_the_download_become_events() {
        assert_eq!(
            parse_line("ITEM|3|40|abc|Jornal | Noite"),
            Some(Event::Item {
                index: 3,
                total: 40,
                title: "Jornal | Noite".to_string()
            })
        );
        assert_eq!(
            parse_line("PROGRESS|2500|10000|NA"),
            Some(Event::Progress { percent: 25.0 })
        );
        assert_eq!(
            parse_line("PROGRESS|2500|NA|5000"),
            Some(Event::Progress { percent: 50.0 }),
            "the estimate is used when the size is not known"
        );
        assert_eq!(parse_line("PROGRESS|10|NA|NA"), None);
        assert_eq!(
            parse_line("DONE|abc|/storage/youtube/Jornal [abc].mp4"),
            Some(Event::Done {
                path: "/storage/youtube/Jornal [abc].mp4".to_string()
            })
        );
        assert_eq!(
            parse_line("[download] x: has already been recorded in the archive"),
            Some(Event::AlreadyHave)
        );
        assert!(matches!(
            parse_line("ERROR: [youtube] abc: Sign in to confirm you’re not a bot"),
            Some(Event::Failed { message }) if message.contains("robô")
        ));
        assert_eq!(
            parse_line("[download] x does not pass filter (channel_id = UC1), skipping .."),
            Some(Event::Rejected)
        );
        assert_eq!(parse_line("[info] x: Downloading 1 format(s)"), None);
    }

    #[test]
    fn the_height_is_one_of_the_offered() {
        assert!(valid_height(1080));
        assert!(valid_height(720));
        assert!(!valid_height(1000));
    }

    /// A program that prints what `yt-dlp` prints, then exits.
    #[cfg(unix)]
    fn fake_tool(directory: &Path, body: &str) -> YoutubeTool {
        use std::os::unix::fs::PermissionsExt;

        let program = directory.join("fake-yt-dlp");

        std::fs::write(&program, format!("#!/bin/sh\n{body}\n")).unwrap();
        std::fs::set_permissions(&program, std::fs::Permissions::from_mode(0o755)).unwrap();

        YoutubeTool {
            program: program.to_string_lossy().to_string(),
            cookies: None,
        }
    }

    fn request(channel_id: i32, directory: &Path) -> ImportRequest {
        ImportRequest {
            channel_id,
            url: "https://www.youtube.com/watch?v=a1".to_string(),
            title: "Jornal".to_string(),
            probe: Probe {
                kind: SourceKind::Video,
                title: "Jornal".to_string(),
                owner_id: "UC111".to_string(),
                owner_name: "LFTV".to_string(),
                videos: Vec::new(),
                playlists: Vec::new(),
            },
            storage_root: directory.to_path_buf(),
            folder: "youtube".to_string(),
            allowed: vec!["UC111".to_string()],
            height: 1080,
        }
    }

    async fn wait_for_the_end(channel_id: i32) -> ImportJob {
        for _ in 0..200 {
            let job = jobs_of(channel_id).remove(0);

            if job.status != JobStatus::Running {
                return job;
            }

            tokio::time::sleep(Duration::from_millis(50)).await;
        }

        panic!("the job did not end");
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn a_job_counts_what_the_download_prints_and_runs_one_at_a_time() {
        let directory = std::env::temp_dir().join(format!("ffplayout-youtube-{}", now()));

        std::fs::create_dir_all(&directory).unwrap();

        // Counts what was brought, what it already had and what failed.
        let tool = fake_tool(
            &directory,
            "echo 'ITEM|1|3|a1|Um'\n\
             echo 'PROGRESS|50|100|NA'\n\
             echo 'DONE|a1|/storage/youtube/Um [a1].mp4'\n\
             echo '[download] Dois: has already been recorded in the archive'\n\
             echo 'ITEM|3|3|c3|Tres'\n\
             echo \"ERROR: [youtube] c3: Private video. Sign in\" >&2\n\
             exit 1",
        );
        let job = start_import(tool, request(4242, &directory)).unwrap();

        assert_eq!(job.status, JobStatus::Running);

        let finished = wait_for_the_end(4242).await;

        assert_eq!(finished.status, JobStatus::Done);
        assert_eq!(finished.downloaded, 1);
        assert_eq!(finished.already_had, 1);
        assert_eq!(finished.failed, 1);
        assert_eq!(finished.item_total, 3);
        assert!(
            finished.errors[0].contains("privado"),
            "{:?}",
            finished.errors
        );
        assert!(directory.join("youtube").is_dir());

        // A second job is refused while one runs, and a running one can be stopped.
        let slow = fake_tool(&directory, "sleep 30");
        let running = start_import(slow.clone(), request(4243, &directory)).unwrap();

        assert!(matches!(
            start_import(slow, request(4243, &directory)),
            Err(ServiceError::Conflict(_))
        ));
        assert!(
            !cancel_job(9999, &running.id),
            "another channel cannot stop it"
        );
        assert!(cancel_job(4243, &running.id));
        assert_eq!(wait_for_the_end(4243).await.status, JobStatus::Cancelled);

        let _ = std::fs::remove_dir_all(&directory);
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn a_missing_program_is_reported_in_plain_words() {
        let tool = YoutubeTool {
            program: "/nonexistent/yt-dlp".to_string(),
            cookies: None,
        };

        assert_eq!(tool.version().await, None);
        assert!(matches!(
            probe(&tool, "https://youtu.be/abc").await,
            Err(ServiceError::ServiceUnavailable(_) | ServiceError::BadRequest(_))
        ));
    }
}
