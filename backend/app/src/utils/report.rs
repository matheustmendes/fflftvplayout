//! The campaign report an operator downloads and sends to an advertiser:
//! what the plan promised against what really aired, from the airing log.
//!
//! The text of the report is Brazilian Portuguese: the readers are the
//! advertisers of the station. Names come from copies stored in the log, so a
//! report does not change when somebody renames an advertiser later.

use std::collections::BTreeMap;

use chrono::{DateTime, Duration, NaiveDate, Utc};
use chrono_tz::Tz;
use serde::Serialize;
use sqlx::sqlite::SqlitePool;

use crate::{
    db::{
        handles::{self, AiringFilter},
        models::{AirKind, AirResult, Airing},
    },
    utils::{airlog::RETENTION_DAYS, errors::ServiceError},
};

/// A report covers at most this many days, which keeps it readable and the
/// query small.
pub const MAX_REPORT_DAYS: i64 = 366;

const MAX_REPORT_ROWS: i64 = 100_000;

#[derive(Debug, Clone)]
pub struct ReportRequest {
    pub channel_id: i32,
    pub campaign_id: Option<i32>,
    pub advertiser_id: Option<i32>,
    pub date_from: NaiveDate,
    pub date_to: NaiveDate,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ReportHeader {
    pub station_name: String,
    pub station_details: Vec<String>,
    pub logo_path: String,
    pub footer_note: String,
    pub advertiser: Option<String>,
    pub campaign: Option<String>,
    pub date_from: String,
    pub date_to: String,
    /// When the report was issued, in the timezone of the station.
    pub issued_at: String,
    pub timezone: String,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ReportLine {
    pub date: String,
    /// When the spot was planned to start, `HH:MM:SS` in the station timezone.
    pub planned_time: String,
    /// When it really started; empty when it did not air.
    pub aired_time: String,
    pub duration_sec: Option<f64>,
    pub advertiser: String,
    pub campaign: String,
    pub spot: String,
    pub result: AirResult,
    pub status: String,
    pub note: String,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct DayTotals {
    pub date: String,
    /// Spots the plan promised for the day.
    pub planned: u32,
    /// Spots that aired in full, including those an operator added by hand.
    pub aired: u32,
    pub partial: u32,
    /// Skipped, interrupted, replaced, failed, never started or lost to a
    /// live broadcast.
    pub missed: u32,
    /// Not yet due, or on the air right now.
    pub pending: u32,
}

impl DayTotals {
    fn add(&mut self, line: &ReportLine, was_planned: bool) {
        if was_planned {
            self.planned += 1;
        }

        match line.result {
            AirResult::Played => self.aired += 1,
            AirResult::Partial => self.partial += 1,
            AirResult::Pending | AirResult::Airing => self.pending += 1,
            AirResult::Skipped
            | AirResult::Interrupted
            | AirResult::Fallback
            | AirResult::Failed
            | AirResult::NotAired
            | AirResult::LiveMissed => self.missed += 1,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Report {
    pub header: ReportHeader,
    pub lines: Vec<ReportLine>,
    pub days: Vec<DayTotals>,
    pub totals: DayTotals,
    /// Remarks the reader should see, such as a period the log may not cover.
    pub notes: Vec<String>,
}

pub fn result_label(result: AirResult) -> &'static str {
    match result {
        AirResult::Played => "Veiculado",
        AirResult::Partial => "Veiculado parcialmente",
        AirResult::Skipped => "Não veiculado",
        AirResult::Interrupted => "Interrompido",
        AirResult::Fallback => "Não veiculado",
        AirResult::Failed => "Não veiculado",
        AirResult::NotAired => "Não veiculado",
        AirResult::LiveMissed => "Não veiculado",
        AirResult::Pending => "Agendado",
        AirResult::Airing => "No ar",
    }
}

fn result_note(result: AirResult, aired_sec: Option<f64>, planned_sec: Option<f64>) -> String {
    match result {
        AirResult::Played | AirResult::Pending | AirResult::Airing => String::new(),
        AirResult::Partial => match (aired_sec, planned_sec) {
            (Some(aired), Some(planned)) => {
                format!("Exibido por {aired:.1} s de {planned:.1} s")
            }
            _ => "Exibido parcialmente".to_string(),
        },
        AirResult::Skipped => "Pulado pela emissora".to_string(),
        AirResult::Interrupted => "Exibição interrompida".to_string(),
        AirResult::Fallback => "Substituído por conteúdo reserva".to_string(),
        AirResult::Failed => "Falha técnica na exibição".to_string(),
        AirResult::NotAired => "O spot não chegou a ser exibido".to_string(),
        AirResult::LiveMissed => "Transmissão ao vivo no horário".to_string(),
    }
}

/// `HH:MM:SS` of a UTC timestamp in the station timezone; empty when the
/// text is missing or not a timestamp.
fn local_clock(timestamp: Option<&str>, timezone: Tz) -> String {
    timestamp
        .and_then(|text| DateTime::parse_from_rfc3339(text).ok())
        .map(|time| time.with_timezone(&timezone).format("%H:%M:%S").to_string())
        .unwrap_or_default()
}

/// Turn log rows into report lines and per-day totals.
pub fn summarize(rows: &[Airing], timezone: Tz) -> (Vec<ReportLine>, Vec<DayTotals>, DayTotals) {
    let mut lines = Vec::with_capacity(rows.len());
    let mut days: BTreeMap<String, DayTotals> = BTreeMap::new();
    let mut totals = DayTotals {
        date: "total".to_string(),
        ..DayTotals::default()
    };

    for row in rows.iter().filter(|row| row.kind == AirKind::Spot) {
        let line = ReportLine {
            date: row.air_date.clone(),
            planned_time: local_clock(row.planned_start.as_deref(), timezone),
            aired_time: local_clock(row.actual_start.as_deref(), timezone),
            duration_sec: row.actual_duration_sec.or(row.planned_duration_sec),
            advertiser: row.advertiser_name.clone(),
            campaign: row.campaign_name.clone(),
            spot: row.spot_title.clone(),
            result: row.result,
            status: result_label(row.result).to_string(),
            note: result_note(
                row.result,
                row.actual_duration_sec,
                row.planned_duration_sec,
            ),
        };
        let was_planned = row.planned_start.is_some();

        days.entry(row.air_date.clone())
            .or_insert_with(|| DayTotals {
                date: row.air_date.clone(),
                ..DayTotals::default()
            })
            .add(&line, was_planned);
        totals.add(&line, was_planned);
        lines.push(line);
    }

    (lines, days.into_values().collect(), totals)
}

fn split_details(details: &str) -> Vec<String> {
    details
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .map(str::to_string)
        .collect()
}

fn not_found(what: &str, id: i32, channel_id: i32) -> ServiceError {
    ServiceError::NotFound(format!("{what} {id} not found for channel {channel_id}"))
}

/// Build the report of a period. `now` only decides what is still pending and
/// whether the period reaches past the retention time.
pub async fn build_report(
    pool: &SqlitePool,
    request: &ReportRequest,
    channel_name: &str,
    timezone: Tz,
    now: DateTime<Utc>,
) -> Result<Report, ServiceError> {
    if request.date_from > request.date_to {
        return Err(ServiceError::BadRequest(
            "the start date must not be after the end date".to_string(),
        ));
    }

    if (request.date_to - request.date_from).num_days() >= MAX_REPORT_DAYS {
        return Err(ServiceError::BadRequest(format!(
            "a report covers at most {MAX_REPORT_DAYS} days"
        )));
    }

    let campaign = match request.campaign_id {
        Some(id) => Some(
            handles::select_campaign(pool, request.channel_id, id)
                .await?
                .ok_or_else(|| not_found("campaign", id, request.channel_id))?,
        ),
        None => None,
    };
    let advertiser_id = campaign
        .as_ref()
        .map(|campaign| campaign.advertiser_id)
        .or(request.advertiser_id);
    let advertiser = match advertiser_id {
        Some(id) => Some(
            handles::select_advertiser(pool, request.channel_id, id)
                .await?
                .ok_or_else(|| not_found("advertiser", id, request.channel_id))?,
        ),
        None => None,
    };

    if let (Some(campaign), Some(requested)) = (&campaign, request.advertiser_id)
        && campaign.advertiser_id != requested
    {
        return Err(ServiceError::BadRequest(
            "the campaign does not belong to the advertiser".to_string(),
        ));
    }

    let settings = handles::select_report_settings(pool, request.channel_id).await?;
    let rows = handles::select_airings(
        pool,
        request.channel_id,
        &AiringFilter {
            date_from: Some(request.date_from.format("%Y-%m-%d").to_string()),
            date_to: Some(request.date_to.format("%Y-%m-%d").to_string()),
            campaign_id: request.campaign_id,
            advertiser_id,
            kind: Some(AirKind::Spot),
            limit: MAX_REPORT_ROWS,
            ..AiringFilter::default()
        },
    )
    .await?;
    let (lines, days, totals) = summarize(&rows, timezone);
    let today = now.with_timezone(&timezone).date_naive();
    let mut notes = Vec::new();

    if request.date_from < today - Duration::days(RETENTION_DAYS) {
        notes.push(format!(
            "O registro de veiculação é mantido por {RETENTION_DAYS} dias; \
            dias mais antigos podem não constar neste relatório."
        ));
    }

    if totals.pending > 0 {
        notes.push(format!(
            "{} spot(s) ainda não foram exibidos ou estão no ar neste momento.",
            totals.pending
        ));
    }

    let station_name = if settings.station_name.trim().is_empty() {
        channel_name.to_string()
    } else {
        settings.station_name.clone()
    };

    Ok(Report {
        header: ReportHeader {
            station_name,
            station_details: split_details(&settings.station_details),
            logo_path: settings.logo_path,
            footer_note: settings.footer_note,
            advertiser: advertiser.map(|advertiser| advertiser.name),
            campaign: campaign.map(|campaign| campaign.name),
            date_from: request.date_from.format("%Y-%m-%d").to_string(),
            date_to: request.date_to.format("%Y-%m-%d").to_string(),
            issued_at: now
                .with_timezone(&timezone)
                .format("%Y-%m-%d %H:%M")
                .to_string(),
            timezone: timezone.name().to_string(),
        },
        lines,
        days,
        totals,
        notes,
    })
}

/// One CSV field. A spreadsheet runs text that starts with `=`, `+`, `-` or
/// `@` as a formula, and the names come from users, so those are defused.
fn csv_field(value: &str) -> String {
    let defused = match value.chars().next() {
        Some('=' | '+' | '-' | '@' | '\t' | '\r') => format!("'{value}"),
        _ => value.to_string(),
    };

    if defused.contains([';', '"', '\n', '\r']) {
        format!("\"{}\"", defused.replace('"', "\"\""))
    } else {
        defused
    }
}

/// The report as CSV for a spreadsheet in Brazilian Portuguese: UTF-8 with a
/// byte order mark, `;` as separator and a decimal comma.
pub fn to_csv(report: &Report) -> Vec<u8> {
    let mut text = String::from("\u{feff}");
    let columns = [
        "Data",
        "Horário previsto",
        "Horário veiculado",
        "Duração (s)",
        "Anunciante",
        "Campanha",
        "Spot",
        "Situação",
        "Observação",
    ];

    text.push_str(&columns.join(";"));
    text.push_str("\r\n");

    for line in &report.lines {
        let duration = line
            .duration_sec
            .map(|seconds| format!("{seconds:.1}").replace('.', ","))
            .unwrap_or_default();
        let fields = [
            csv_field(&line.date),
            csv_field(&line.planned_time),
            csv_field(&line.aired_time),
            duration,
            csv_field(&line.advertiser),
            csv_field(&line.campaign),
            csv_field(&line.spot),
            csv_field(&line.status),
            csv_field(&line.note),
        ];

        text.push_str(&fields.join(";"));
        text.push_str("\r\n");
    }

    text.into_bytes()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(id: i32, date: &str, result: AirResult, planned: Option<&str>) -> Airing {
        Airing {
            id,
            channel_id: 1,
            air_date: date.to_string(),
            kind: AirKind::Spot,
            spot_id: Some(1),
            campaign_id: Some(1),
            advertiser_id: Some(1),
            advertiser_name: "Padaria".to_string(),
            campaign_name: "Natal".to_string(),
            spot_title: "Spot de Natal".to_string(),
            file_path: "commercials/spot-1.mp4".to_string(),
            break_window_id: None,
            position_in_break: None,
            planned_begin_sec: planned.map(|_| 100.0),
            planned_start: planned.map(str::to_string),
            planned_duration_sec: Some(30.0),
            actual_start: (result == AirResult::Played)
                .then(|| "2026-12-10T12:00:02.000Z".to_string()),
            actual_end: None,
            actual_duration_sec: (result == AirResult::Played).then_some(29.9),
            result,
            reason: "internal english reason".to_string(),
            created_at: String::new(),
        }
    }

    #[test]
    fn totals_count_each_result_in_its_column() {
        let rows = [
            row(
                1,
                "2026-12-10",
                AirResult::Played,
                Some("2026-12-10T12:00:00.000Z"),
            ),
            row(
                2,
                "2026-12-10",
                AirResult::LiveMissed,
                Some("2026-12-10T15:00:00.000Z"),
            ),
            row(
                3,
                "2026-12-10",
                AirResult::Partial,
                Some("2026-12-10T18:00:00.000Z"),
            ),
            row(4, "2026-12-10", AirResult::Played, None),
            row(
                5,
                "2026-12-11",
                AirResult::Pending,
                Some("2026-12-11T12:00:00.000Z"),
            ),
        ];
        let (lines, days, totals) = summarize(&rows, Tz::UTC);

        assert_eq!(lines.len(), 5);
        assert_eq!(days.len(), 2);
        assert_eq!(
            days[0],
            DayTotals {
                date: "2026-12-10".to_string(),
                planned: 3,
                aired: 2,
                partial: 1,
                missed: 1,
                pending: 0,
            },
            "a spot added by hand is aired but was never planned"
        );
        assert_eq!((totals.planned, totals.aired, totals.pending), (4, 2, 1));
    }

    #[test]
    fn times_follow_the_station_timezone_and_notes_are_in_portuguese() {
        let sao_paulo: Tz = "America/Sao_Paulo".parse().unwrap();
        let rows = [
            row(
                1,
                "2026-12-10",
                AirResult::Played,
                Some("2026-12-10T12:00:00.000Z"),
            ),
            row(
                2,
                "2026-12-10",
                AirResult::LiveMissed,
                Some("2026-12-10T15:00:00.000Z"),
            ),
        ];
        let (lines, _, _) = summarize(&rows, sao_paulo);

        assert_eq!(lines[0].planned_time, "09:00:00");
        assert_eq!(lines[0].aired_time, "09:00:02");
        assert_eq!(lines[0].duration_sec, Some(29.9));
        assert_eq!(lines[1].aired_time, "");
        assert_eq!(lines[1].status, "Não veiculado");
        assert_eq!(lines[1].note, "Transmissão ao vivo no horário");
        assert!(!lines[1].note.contains("english"));
    }

    #[test]
    fn live_windows_are_not_report_lines() {
        let mut live = row(1, "2026-12-10", AirResult::Played, None);

        live.kind = AirKind::Live;

        let (lines, days, totals) = summarize(&[live], Tz::UTC);

        assert!(lines.is_empty() && days.is_empty());
        assert_eq!(totals.aired, 0);
    }

    #[test]
    fn csv_fields_are_quoted_and_formulas_defused() {
        assert_eq!(csv_field("simples"), "simples");
        assert_eq!(csv_field("a;b"), "\"a;b\"");
        assert_eq!(csv_field("diz \"oi\""), "\"diz \"\"oi\"\"\"");
        assert_eq!(csv_field("=HYPERLINK(\"x\")"), "\"'=HYPERLINK(\"\"x\"\")\"");
        assert_eq!(csv_field("-5"), "'-5");
        assert_eq!(csv_field("@cmd"), "'@cmd");
        assert_eq!(csv_field("linha\nnova"), "\"linha\nnova\"");
    }

    #[test]
    fn csv_has_a_byte_order_mark_semicolons_and_decimal_commas() {
        let rows = [row(
            1,
            "2026-12-10",
            AirResult::Played,
            Some("2026-12-10T12:00:00.000Z"),
        )];
        let (lines, days, totals) = summarize(&rows, Tz::UTC);
        let report = Report {
            header: ReportHeader {
                station_name: "TV".to_string(),
                station_details: vec![],
                logo_path: String::new(),
                footer_note: String::new(),
                advertiser: None,
                campaign: None,
                date_from: "2026-12-10".to_string(),
                date_to: "2026-12-10".to_string(),
                issued_at: String::new(),
                timezone: "UTC".to_string(),
            },
            lines,
            days,
            totals,
            notes: vec![],
        };
        let bytes = to_csv(&report);
        let text = String::from_utf8(bytes.clone()).unwrap();

        assert_eq!(&bytes[..3], [0xEF, 0xBB, 0xBF]);

        let rows: Vec<&str> = text.trim_start_matches('\u{feff}').split("\r\n").collect();

        assert!(rows[0].starts_with("Data;Horário previsto;"));
        assert_eq!(
            rows[1],
            "2026-12-10;12:00:00;12:00:02;29,9;Padaria;Natal;Spot de Natal;Veiculado;"
        );
    }
}
