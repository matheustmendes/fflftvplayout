//! The planner of commercial breaks.
//!
//! It takes the programs of a weekday, the break windows and the campaigns
//! that run on the date, and builds the day: each break goes to the boundary
//! between two programs that is closest to its nominal clock time, and is
//! filled with spots.
//!
//! Everything here is a pure function of its input. The rotation is seeded
//! from the date and the break, so planning the same day twice gives the same
//! result, and no database, clock or file is touched.

use std::collections::{HashMap, HashSet};

use serde::Serialize;

use crate::db::models::BreakPosition;

const SECONDS_PER_DAY: f64 = 86_400.0;
/// A gap shorter than this is not worth an item of its own.
const MIN_FILLER_SECONDS: f64 = 1.0;
/// How far a day may run over its length before the plan is blocked. The
/// playout trims the last clip of the day by itself within this margin.
pub const DAY_OVERRUN_TOLERANCE_SECONDS: f64 = 5.0;
const EPSILON: f64 = 1e-6;

#[derive(Debug, Clone, PartialEq)]
pub struct PlanProgram {
    pub source: String,
    pub title: String,
    pub seek: f64,
    pub out: f64,
    pub duration: f64,
}

impl PlanProgram {
    fn airtime(&self) -> f64 {
        self.out - self.seek
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct PlanWindow {
    pub id: i32,
    /// Clock time, `HH:MM:SS`.
    pub nominal_time: String,
    pub duration_sec: i32,
    pub tolerance_sec: i32,
}

#[derive(Debug, Clone, PartialEq)]
pub struct PlanSpot {
    pub id: i32,
    pub title: String,
    pub file_path: String,
    pub duration: f64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct PlanCampaign {
    pub id: i32,
    pub name: String,
    pub advertiser_id: i32,
    pub advertiser_name: String,
    pub weight: i32,
    pub max_per_day: Option<i32>,
    pub position: BreakPosition,
    pub spots: Vec<PlanSpot>,
}

/// What fills the time no spot covers.
#[derive(Debug, Clone, PartialEq)]
pub struct PlanFiller {
    pub source: String,
    pub duration: f64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct PlanInput {
    /// `YYYY-MM-DD`; part of the seed of the rotation.
    pub date: String,
    pub day_start_sec: f64,
    pub day_length_sec: f64,
    pub programs: Vec<PlanProgram>,
    pub windows: Vec<PlanWindow>,
    pub campaigns: Vec<PlanCampaign>,
    pub filler: Option<PlanFiller>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ItemKind {
    Program,
    Spot,
    BreakFiller,
    DayFiller,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct PlannedItem {
    pub kind: ItemKind,
    /// Seconds since the start of the day.
    pub start_sec: f64,
    /// Clock time the item starts, `HH:MM:SS`.
    pub start_time: String,
    pub source: String,
    pub title: String,
    pub seek: f64,
    pub out: f64,
    pub duration: f64,
    pub break_window_id: Option<i32>,
    pub position_in_break: Option<usize>,
    pub spot_id: Option<i32>,
    pub campaign_id: Option<i32>,
    pub advertiser_id: Option<i32>,
}

impl PlannedItem {
    pub fn airtime(&self) -> f64 {
        self.out - self.seek
    }
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct PlannedBreak {
    pub window_id: i32,
    pub nominal_time: String,
    /// Clock time the break really starts.
    pub start_time: String,
    /// Seconds between the nominal time and the real start.
    pub distance_sec: f64,
    pub duration_sec: i32,
    pub spots_sec: f64,
    pub gap_sec: f64,
    pub spots: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Severity {
    Warning,
    Error,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum WarningCode {
    NoPrograms,
    InvalidBreakTime,
    NoFreeBoundary,
    BreakOffGrid,
    BreakEmpty,
    BreakUnderfilled,
    DayFilled,
    DaySlightlyLong,
    DayTooLong,
    SpotFileMissing,
    ProgramFileMissing,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct PlanWarning {
    pub code: WarningCode,
    pub severity: Severity,
    pub message: String,
    pub window_id: Option<i32>,
}

impl PlanWarning {
    pub fn new(
        code: WarningCode,
        severity: Severity,
        message: String,
        window_id: Option<i32>,
    ) -> Self {
        Self {
            code,
            severity,
            message,
            window_id,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct DayPlan {
    pub items: Vec<PlannedItem>,
    pub breaks: Vec<PlannedBreak>,
    pub warnings: Vec<PlanWarning>,
    pub programs_sec: f64,
    pub breaks_sec: f64,
    pub total_sec: f64,
    pub day_length_sec: f64,
}

impl DayPlan {
    pub fn has_errors(&self) -> bool {
        self.warnings
            .iter()
            .any(|warning| warning.severity == Severity::Error)
    }
}

/// A small deterministic generator (SplitMix64). It is written out here so the
/// rotation of a given day never changes with a library version.
struct Rng(u64);

impl Rng {
    fn new(seed: u64) -> Self {
        Self(seed)
    }

    fn next_u64(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);

        let mut value = self.0;

        value = (value ^ (value >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        value = (value ^ (value >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);

        value ^ (value >> 31)
    }

    fn next_unit(&mut self) -> f64 {
        (self.next_u64() >> 11) as f64 / (1_u64 << 53) as f64
    }

    /// Pick an index with a probability proportional to its weight.
    fn weighted(&mut self, weights: &[f64]) -> usize {
        let total: f64 = weights.iter().sum();
        let mut target = self.next_unit() * total;

        for (index, weight) in weights.iter().enumerate() {
            if target < *weight {
                return index;
            }

            target -= weight;
        }

        weights.len() - 1
    }
}

fn fnv1a(text: &str) -> u64 {
    text.bytes().fold(0xcbf2_9ce4_8422_2325, |hash, byte| {
        (hash ^ u64::from(byte)).wrapping_mul(0x0000_0100_0000_01b3)
    })
}

fn parse_clock(text: &str) -> Option<f64> {
    let mut parts = text.trim().split(':');
    let hours: u32 = parts.next()?.parse().ok()?;
    let minutes: u32 = parts.next()?.parse().ok()?;
    let seconds: u32 = parts.next()?.parse().ok()?;

    if parts.next().is_some() || hours > 23 || minutes > 59 || seconds > 59 {
        return None;
    }

    Some(f64::from(hours * 3600 + minutes * 60 + seconds))
}

fn clock(seconds: f64) -> String {
    let total = seconds.rem_euclid(SECONDS_PER_DAY).round() as u64 % 86_400;

    format!(
        "{:02}:{:02}:{:02}",
        total / 3600,
        total % 3600 / 60,
        total % 60
    )
}

fn duration_text(seconds: f64) -> String {
    let total = seconds.round() as u64;

    if total >= 60 {
        format!("{} min {} s", total / 60, total % 60)
    } else {
        format!("{total} s")
    }
}

/// A break window placed on the timeline.
struct Placed<'a> {
    window: &'a PlanWindow,
}

/// Usage counted over the day, so the limits and the rotation hold across
/// every break of it.
#[derive(Default)]
struct Usage {
    campaigns: HashMap<i32, i32>,
    spots: HashMap<i32, i32>,
}

impl Usage {
    fn campaign_uses(&self, campaign: &PlanCampaign) -> i32 {
        self.campaigns.get(&campaign.id).copied().unwrap_or(0)
    }

    fn spot_uses(&self, spot: &PlanSpot) -> i32 {
        self.spots.get(&spot.id).copied().unwrap_or(0)
    }

    fn record(&mut self, campaign: &PlanCampaign, spot: &PlanSpot) {
        *self.campaigns.entry(campaign.id).or_insert(0) += 1;
        *self.spots.entry(spot.id).or_insert(0) += 1;
    }
}

/// The spot of a campaign to use next: the one played least so far that fits.
fn best_spot<'a>(campaign: &'a PlanCampaign, usage: &Usage, room: f64) -> Option<&'a PlanSpot> {
    campaign
        .spots
        .iter()
        .filter(|spot| spot.duration > 0.0 && spot.duration <= room + EPSILON)
        .min_by_key(|spot| (usage.spot_uses(spot), spot.id))
}

/// Pick one of `candidates` at random, weighted by the campaign weights.
fn pick<'a>(
    candidates: &[(&'a PlanCampaign, &'a PlanSpot)],
    rng: &mut Rng,
) -> (&'a PlanCampaign, &'a PlanSpot) {
    let weights: Vec<f64> = candidates
        .iter()
        .map(|(campaign, _)| f64::from(campaign.weight.max(1)))
        .collect();

    candidates[rng.weighted(&weights)]
}

/// Choose the spots of one break, in air order.
fn fill_break<'a>(
    duration_sec: f64,
    campaigns: &'a [PlanCampaign],
    usage: &mut Usage,
    rng: &mut Rng,
) -> Vec<(&'a PlanCampaign, &'a PlanSpot)> {
    let mut room = duration_sec;
    let mut chosen: Vec<(&PlanCampaign, &PlanSpot)> = Vec::new();
    let mut in_break: HashSet<i32> = HashSet::new();

    // A campaign appears once per break and never beyond its daily limit.
    let candidates = |position: Option<BreakPosition>,
                      room: f64,
                      usage: &Usage,
                      in_break: &HashSet<i32>|
     -> Vec<(&'a PlanCampaign, &'a PlanSpot)> {
        campaigns
            .iter()
            .filter(|campaign| position.is_none_or(|wanted| campaign.position == wanted))
            .filter(|campaign| !in_break.contains(&campaign.id))
            .filter(|campaign| {
                campaign
                    .max_per_day
                    .is_none_or(|limit| usage.campaign_uses(campaign) < limit)
            })
            .filter_map(|campaign| best_spot(campaign, usage, room).map(|spot| (campaign, spot)))
            .collect()
    };

    let first = candidates(Some(BreakPosition::First), room, usage, &in_break);

    if !first.is_empty() {
        let (campaign, spot) = pick(&first, rng);

        room -= spot.duration;
        in_break.insert(campaign.id);
        usage.record(campaign, spot);
        chosen.push((campaign, spot));
    }

    // The campaign that wants the last place is reserved before the rotation,
    // so the middle never takes its room.
    let last_candidates = candidates(Some(BreakPosition::Last), room, usage, &in_break);
    let last = if last_candidates.is_empty() {
        None
    } else {
        let (campaign, spot) = pick(&last_candidates, rng);

        room -= spot.duration;
        in_break.insert(campaign.id);
        usage.record(campaign, spot);

        Some((campaign, spot))
    };

    loop {
        let mut available = candidates(None, room, usage, &in_break);

        if available.is_empty() {
            break;
        }

        // Do not put two spots of one advertiser in a row when there is a
        // choice.
        if let Some((previous, _)) = chosen.last() {
            let other_advertisers: Vec<_> = available
                .iter()
                .copied()
                .filter(|(campaign, _)| campaign.advertiser_id != previous.advertiser_id)
                .collect();

            if !other_advertisers.is_empty() {
                available = other_advertisers;
            }
        }

        let (campaign, spot) = pick(&available, rng);

        room -= spot.duration;
        in_break.insert(campaign.id);
        usage.record(campaign, spot);
        chosen.push((campaign, spot));
    }

    chosen.extend(last);

    chosen
}

/// Build the plan of a day.
pub fn plan_day(input: &PlanInput) -> DayPlan {
    let mut warnings = Vec::new();

    if input.programs.is_empty() {
        warnings.push(PlanWarning::new(
            WarningCode::NoPrograms,
            Severity::Error,
            "o dia da semana não tem programas na grade semanal".to_string(),
            None,
        ));

        return DayPlan {
            items: Vec::new(),
            breaks: Vec::new(),
            warnings,
            programs_sec: 0.0,
            breaks_sec: 0.0,
            total_sec: 0.0,
            day_length_sec: input.day_length_sec,
        };
    }

    let windows = sorted_windows(input, &mut warnings);
    let boundaries = place_windows(input, &windows, &mut warnings);
    let mut usage = Usage::default();
    let mut items: Vec<PlannedItem> = Vec::new();
    let mut breaks: Vec<PlannedBreak> = Vec::new();
    let mut cursor = 0.0;

    for boundary in 0..=input.programs.len() {
        if let Some(placed) = boundary_window(&boundaries, boundary) {
            let (break_items, planned) = build_break(input, placed, cursor, &mut usage);

            cursor += break_items.iter().map(PlannedItem::airtime).sum::<f64>();

            warn_about_break(&planned, placed.window, &mut warnings);

            items.extend(break_items);
            breaks.push(planned);
        }

        if let Some(program) = input.programs.get(boundary) {
            items.push(PlannedItem {
                kind: ItemKind::Program,
                start_sec: cursor,
                start_time: clock(input.day_start_sec + cursor),
                source: program.source.clone(),
                title: program.title.clone(),
                seek: program.seek,
                out: program.out,
                duration: program.duration,
                break_window_id: None,
                position_in_break: None,
                spot_id: None,
                campaign_id: None,
                advertiser_id: None,
            });

            cursor += program.airtime();
        }
    }

    close_the_day(input, &mut items, &mut cursor, &mut warnings);
    check_break_distances(input, &mut breaks, &mut warnings);

    let programs_sec = input.programs.iter().map(PlanProgram::airtime).sum();
    let breaks_sec = items
        .iter()
        .filter(|item| matches!(item.kind, ItemKind::Spot | ItemKind::BreakFiller))
        .map(PlannedItem::airtime)
        .sum();

    DayPlan {
        items,
        breaks,
        warnings,
        programs_sec,
        breaks_sec,
        total_sec: cursor,
        day_length_sec: input.day_length_sec,
    }
}

struct Window<'a> {
    window: &'a PlanWindow,
    /// Seconds from the start of the day to the nominal time.
    offset: f64,
}

/// The usable windows in clock order, counted from the start of the day.
fn sorted_windows<'a>(input: &'a PlanInput, warnings: &mut Vec<PlanWarning>) -> Vec<Window<'a>> {
    let mut windows: Vec<Window> = Vec::new();

    for window in &input.windows {
        let Some(time) = parse_clock(&window.nominal_time) else {
            warnings.push(PlanWarning::new(
                WarningCode::InvalidBreakTime,
                Severity::Warning,
                format!(
                    "o horário de intervalo {:?} não é válido e foi ignorado",
                    window.nominal_time
                ),
                Some(window.id),
            ));

            continue;
        };

        windows.push(Window {
            window,
            offset: (time - input.day_start_sec).rem_euclid(SECONDS_PER_DAY),
        });
    }

    windows.sort_by(|a, b| {
        a.offset
            .total_cmp(&b.offset)
            .then(a.window.id.cmp(&b.window.id))
    });

    windows
}

/// Give every window the boundary between programs that is closest to its
/// nominal time, counting the breaks already placed before it.
fn place_windows<'a>(
    input: &PlanInput,
    windows: &[Window<'a>],
    warnings: &mut Vec<PlanWarning>,
) -> Vec<Option<Placed<'a>>> {
    let count = input.programs.len();
    let mut prefix = vec![0.0; count + 1];

    for (index, program) in input.programs.iter().enumerate() {
        prefix[index + 1] = prefix[index] + program.airtime();
    }

    let mut slots: Vec<Option<Placed>> = (0..=count).map(|_| None).collect();

    for entry in windows {
        let mut shift = 0.0;
        let mut best: Option<(usize, f64)> = None;

        for boundary in 0..=count {
            if slots[boundary].is_none() {
                let distance = (prefix[boundary] + shift - entry.offset).abs();

                // Strictly closer wins, so a tie keeps the earlier boundary.
                if best.is_none_or(|(_, closest)| distance < closest - EPSILON) {
                    best = Some((boundary, distance));
                }
            }

            if let Some(placed) = &slots[boundary] {
                shift += f64::from(placed.window.duration_sec);
            }
        }

        match best {
            Some((boundary, _)) => {
                slots[boundary] = Some(Placed {
                    window: entry.window,
                });
            }
            None => warnings.push(PlanWarning::new(
                WarningCode::NoFreeBoundary,
                Severity::Warning,
                format!(
                    "o intervalo das {} não tem lugar livre entre os programas",
                    entry.window.nominal_time
                ),
                Some(entry.window.id),
            )),
        }
    }

    slots
}

fn boundary_window<'a, 'b>(
    boundaries: &'b [Option<Placed<'a>>],
    boundary: usize,
) -> Option<&'b Placed<'a>> {
    boundaries.get(boundary).and_then(Option::as_ref)
}

/// The items of one break that starts at `start` seconds into the day.
fn build_break(
    input: &PlanInput,
    placed: &Placed<'_>,
    start: f64,
    usage: &mut Usage,
) -> (Vec<PlannedItem>, PlannedBreak) {
    let window = placed.window;
    let seed = fnv1a(&format!(
        "{}|{}|{}",
        input.date, window.id, window.nominal_time
    ));
    let mut rng = Rng::new(seed);
    let duration = f64::from(window.duration_sec);
    let chosen = fill_break(duration, &input.campaigns, usage, &mut rng);
    let mut items = Vec::new();
    let mut cursor = start;

    for (position, (campaign, spot)) in chosen.iter().enumerate() {
        items.push(PlannedItem {
            kind: ItemKind::Spot,
            start_sec: cursor,
            start_time: clock(input.day_start_sec + cursor),
            source: spot.file_path.clone(),
            title: format!("{} - {}", campaign.advertiser_name, spot.title),
            seek: 0.0,
            out: spot.duration,
            duration: spot.duration,
            break_window_id: Some(window.id),
            position_in_break: Some(position),
            spot_id: Some(spot.id),
            campaign_id: Some(campaign.id),
            advertiser_id: Some(campaign.advertiser_id),
        });

        cursor += spot.duration;
    }

    let spots_sec = cursor - start;
    let gap = duration - spots_sec;

    if gap >= MIN_FILLER_SECONDS {
        items.push(filler_item(
            input,
            ItemKind::BreakFiller,
            cursor,
            gap,
            Some((window.id, chosen.len())),
        ));
    }

    let planned = PlannedBreak {
        window_id: window.id,
        nominal_time: window.nominal_time.clone(),
        start_time: clock(input.day_start_sec + start),
        distance_sec: 0.0,
        duration_sec: window.duration_sec,
        spots_sec,
        gap_sec: gap.max(0.0),
        spots: chosen.len(),
    };

    (items, planned)
}

fn filler_item(
    input: &PlanInput,
    kind: ItemKind,
    start: f64,
    length: f64,
    in_break: Option<(i32, usize)>,
) -> PlannedItem {
    let (source, duration) = match &input.filler {
        Some(filler) => (filler.source.clone(), filler.duration),
        // Without a filler file the engine plays black and silence.
        None => (String::new(), length),
    };

    PlannedItem {
        kind,
        start_sec: start,
        start_time: clock(input.day_start_sec + start),
        source,
        title: match kind {
            ItemKind::BreakFiller => "Break filler".to_string(),
            _ => "Day filler".to_string(),
        },
        seek: 0.0,
        out: length,
        duration,
        break_window_id: in_break.map(|(id, _)| id),
        position_in_break: in_break.map(|(_, position)| position),
        spot_id: None,
        campaign_id: None,
        advertiser_id: None,
    }
}

fn warn_about_break(planned: &PlannedBreak, window: &PlanWindow, warnings: &mut Vec<PlanWarning>) {
    if planned.spots == 0 {
        warnings.push(PlanWarning::new(
            WarningCode::BreakEmpty,
            Severity::Warning,
            format!(
                "o intervalo das {} não tem comerciais para exibir; ele é preenchido com o filler",
                window.nominal_time
            ),
            Some(window.id),
        ));
    } else if planned.gap_sec >= MIN_FILLER_SECONDS {
        warnings.push(PlanWarning::new(
            WarningCode::BreakUnderfilled,
            Severity::Warning,
            format!(
                "o intervalo das {} está {} abaixo da duração e é completado com o filler",
                window.nominal_time,
                duration_text(planned.gap_sec)
            ),
            Some(window.id),
        ));
    }
}

/// Complete the day with filler when it is short, and report when it is long.
fn close_the_day(
    input: &PlanInput,
    items: &mut Vec<PlannedItem>,
    cursor: &mut f64,
    warnings: &mut Vec<PlanWarning>,
) {
    let missing = input.day_length_sec - *cursor;

    if missing >= MIN_FILLER_SECONDS {
        items.push(filler_item(
            input,
            ItemKind::DayFiller,
            *cursor,
            missing,
            None,
        ));
        *cursor += missing;

        warnings.push(PlanWarning::new(
            WarningCode::DayFilled,
            Severity::Warning,
            format!(
                "os programas e intervalos ficam {} abaixo da duração do dia; o final é completado com o filler",
                duration_text(missing)
            ),
            None,
        ));
    } else if -missing > DAY_OVERRUN_TOLERANCE_SECONDS {
        warnings.push(PlanWarning::new(
            WarningCode::DayTooLong,
            Severity::Error,
            format!(
                "os programas e intervalos passam {} da duração do dia; retire ou encurte algo",
                duration_text(-missing)
            ),
            None,
        ));
    } else if -missing >= MIN_FILLER_SECONDS {
        warnings.push(PlanWarning::new(
            WarningCode::DaySlightlyLong,
            Severity::Warning,
            format!(
                "o dia passa {} da duração; o último item é cortado",
                duration_text(-missing)
            ),
            None,
        ));
    }
}

/// Report the breaks that are far from their nominal time, once the real start
/// of every break is known.
fn check_break_distances(
    input: &PlanInput,
    breaks: &mut [PlannedBreak],
    warnings: &mut Vec<PlanWarning>,
) {
    for planned in breaks.iter_mut() {
        let Some(window) = input.windows.iter().find(|w| w.id == planned.window_id) else {
            continue;
        };
        let (Some(nominal), Some(start)) = (
            parse_clock(&planned.nominal_time),
            parse_clock(&planned.start_time),
        ) else {
            continue;
        };
        let difference = (start - nominal).abs();
        let distance = difference.min(SECONDS_PER_DAY - difference);

        planned.distance_sec = distance;

        if distance > f64::from(window.tolerance_sec) + EPSILON {
            warnings.push(PlanWarning::new(
                WarningCode::BreakOffGrid,
                Severity::Warning,
                format!(
                    "o intervalo das {} começa às {}, a {} do horário (tolerância {})",
                    planned.nominal_time,
                    planned.start_time,
                    duration_text(distance),
                    duration_text(f64::from(window.tolerance_sec))
                ),
                Some(window.id),
            ));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn program(name: &str, minutes: f64) -> PlanProgram {
        PlanProgram {
            source: format!("{name}.mp4"),
            title: name.to_string(),
            seek: 0.0,
            out: minutes * 60.0,
            duration: minutes * 60.0,
        }
    }

    fn window(id: i32, time: &str, duration: i32) -> PlanWindow {
        PlanWindow {
            id,
            nominal_time: time.to_string(),
            duration_sec: duration,
            tolerance_sec: 60,
        }
    }

    fn spot(id: i32, seconds: f64) -> PlanSpot {
        PlanSpot {
            id,
            title: format!("spot {id}"),
            file_path: format!("commercials/spot-{id}.mp4"),
            duration: seconds,
        }
    }

    fn campaign(id: i32, advertiser_id: i32, spots: Vec<PlanSpot>) -> PlanCampaign {
        PlanCampaign {
            id,
            name: format!("campaign {id}"),
            advertiser_id,
            advertiser_name: format!("advertiser {advertiser_id}"),
            weight: 1,
            max_per_day: None,
            position: BreakPosition::Any,
            spots,
        }
    }

    /// A day that starts at midnight, with the given length.
    fn input(programs: Vec<PlanProgram>, windows: Vec<PlanWindow>, length: f64) -> PlanInput {
        PlanInput {
            date: "2026-10-15".to_string(),
            day_start_sec: 0.0,
            day_length_sec: length,
            programs,
            windows,
            campaigns: Vec::new(),
            filler: None,
        }
    }

    fn plan(input: &PlanInput) -> DayPlan {
        plan_day(input)
    }

    fn kinds(plan: &DayPlan) -> Vec<ItemKind> {
        plan.items.iter().map(|item| item.kind).collect()
    }

    fn codes(plan: &DayPlan) -> Vec<WarningCode> {
        plan.warnings.iter().map(|warning| warning.code).collect()
    }

    fn spot_ids(plan: &DayPlan) -> Vec<i32> {
        plan.items.iter().filter_map(|item| item.spot_id).collect()
    }

    // Three 20-minute programs: boundaries at 0:00, 0:20, 0:40 and 1:00.
    fn three_programs() -> Vec<PlanProgram> {
        vec![program("a", 20.0), program("b", 20.0), program("c", 20.0)]
    }

    #[test]
    fn a_break_goes_to_the_boundary_closest_to_its_time() {
        let mut data = input(three_programs(), vec![window(1, "00:19:00", 60)], 3660.0);
        data.campaigns = vec![campaign(1, 1, vec![spot(1, 60.0)])];
        let result = plan(&data);

        assert_eq!(
            kinds(&result),
            [
                ItemKind::Program,
                ItemKind::Spot,
                ItemKind::Program,
                ItemKind::Program
            ]
        );
        assert_eq!(result.breaks[0].start_time, "00:20:00");
        assert_eq!(result.breaks[0].distance_sec, 60.0);
        assert!(result.warnings.is_empty(), "{:?}", result.warnings);
    }

    #[test]
    fn a_tie_keeps_the_earlier_boundary() {
        // 00:30:00 is exactly between the boundaries at 0:20 and 0:40.
        let data = input(three_programs(), vec![window(1, "00:30:00", 60)], 3660.0);
        let result = plan(&data);

        assert_eq!(result.breaks[0].start_time, "00:20:00");
    }

    #[test]
    fn a_break_far_from_every_boundary_is_reported() {
        let data = input(three_programs(), vec![window(1, "00:29:00", 60)], 3660.0);
        let result = plan(&data);

        assert_eq!(result.breaks[0].start_time, "00:20:00");
        assert_eq!(result.breaks[0].distance_sec, 540.0);
        assert!(codes(&result).contains(&WarningCode::BreakOffGrid));

        let message = &result
            .warnings
            .iter()
            .find(|w| w.code == WarningCode::BreakOffGrid)
            .unwrap()
            .message;

        assert!(
            message.contains("00:29:00") && message.contains("9 min 0 s"),
            "{message}"
        );
    }

    #[test]
    fn a_break_within_its_tolerance_is_not_reported() {
        let mut data = input(three_programs(), vec![window(1, "00:20:50", 60)], 3660.0);
        data.windows[0].tolerance_sec = 60;
        let result = plan(&data);

        assert!(!codes(&result).contains(&WarningCode::BreakOffGrid));
    }

    #[test]
    fn earlier_breaks_push_the_later_boundaries() {
        // Break 1 (3 min) goes to 0:20. That moves the boundary between b and
        // c from 0:40 to 0:43, so a break at 0:42 lands there at 0:43.
        let data = input(
            three_programs(),
            vec![window(1, "00:20:00", 180), window(2, "00:42:00", 60)],
            3600.0 + 240.0,
        );
        let result = plan(&data);

        assert_eq!(result.breaks[0].start_time, "00:20:00");
        assert_eq!(result.breaks[1].start_time, "00:43:00");
        assert_eq!(result.breaks[1].distance_sec, 60.0);
    }

    #[test]
    fn two_breaks_never_share_a_boundary() {
        let data = input(
            three_programs(),
            vec![window(1, "00:20:00", 60), window(2, "00:21:00", 60)],
            3720.0,
        );
        let result = plan(&data);
        let starts: Vec<_> = result.breaks.iter().map(|b| b.start_time.clone()).collect();

        assert_eq!(starts.len(), 2);
        assert_ne!(starts[0], starts[1]);
        assert!(
            result.items.windows(2).all(|pair| {
                !(pair[0].break_window_id.is_some()
                    && pair[1].break_window_id.is_some()
                    && pair[0].break_window_id != pair[1].break_window_id)
            }),
            "two breaks are never back to back"
        );
    }

    #[test]
    fn more_breaks_than_boundaries_leave_the_extra_ones_out() {
        let windows = (1..=6).map(|id| window(id, "00:10:00", 30)).collect();
        let data = input(three_programs(), windows, 3600.0 + 4.0 * 30.0);
        let result = plan(&data);

        assert_eq!(
            result.breaks.len(),
            4,
            "three programs have four boundaries"
        );
        assert_eq!(
            codes(&result)
                .iter()
                .filter(|code| **code == WarningCode::NoFreeBoundary)
                .count(),
            2
        );
    }

    #[test]
    fn a_break_can_open_or_close_the_day() {
        let data = input(
            three_programs(),
            vec![window(1, "00:00:00", 60), window(2, "01:01:00", 60)],
            3720.0,
        );
        let result = plan(&data);

        assert_eq!(result.items.first().unwrap().break_window_id, Some(1));
        assert_eq!(result.items.last().unwrap().break_window_id, Some(2));
        assert_eq!(result.total_sec, 3720.0);
    }

    #[test]
    fn times_before_the_day_start_belong_to_the_end_of_the_day() {
        // The day starts at 06:00; a break at 01:00 is 19 hours in.
        let programs = vec![program("morning", 600.0), program("night", 600.0)];
        let mut data = input(programs, vec![window(1, "01:00:00", 60)], 72_060.0);
        data.day_start_sec = 6.0 * 3600.0;
        let result = plan(&data);

        // The boundaries are at 06:00, 16:00 and 02:00 (the next morning); the
        // closest to 01:00 is the last one, an hour away.
        assert_eq!(result.items[0].start_time, "06:00:00");
        assert_eq!(result.breaks[0].start_time, "02:00:00");
        assert_eq!(result.breaks[0].distance_sec, 3600.0);
        assert!(codes(&result).contains(&WarningCode::BreakOffGrid));

        data.windows = vec![window(1, "16:00:00", 60)];

        let exact = plan(&data);

        assert_eq!(exact.breaks[0].start_time, "16:00:00");
        assert_eq!(exact.breaks[0].distance_sec, 0.0);
        assert!(!codes(&exact).contains(&WarningCode::BreakOffGrid));
    }

    #[test]
    fn invalid_break_times_are_skipped_with_a_warning() {
        let data = input(
            three_programs(),
            vec![window(1, "25:00:00", 60), window(2, "00:20", 60)],
            3600.0,
        );
        let result = plan(&data);

        assert!(result.breaks.is_empty());
        assert_eq!(
            codes(&result)
                .iter()
                .filter(|code| **code == WarningCode::InvalidBreakTime)
                .count(),
            2
        );
    }

    #[test]
    fn a_day_without_programs_is_an_error() {
        let data = input(Vec::new(), vec![window(1, "20:00:00", 60)], 86_400.0);
        let result = plan(&data);

        assert!(result.has_errors());
        assert_eq!(codes(&result), [WarningCode::NoPrograms]);
        assert!(result.items.is_empty());
    }

    #[test]
    fn a_break_is_filled_with_spots_up_to_its_length() {
        let mut data = input(three_programs(), vec![window(1, "00:20:00", 60)], 3660.0);
        data.campaigns = vec![
            campaign(1, 1, vec![spot(1, 30.0)]),
            campaign(2, 2, vec![spot(2, 30.0)]),
            campaign(3, 3, vec![spot(3, 30.0)]),
        ];
        let result = plan(&data);
        let planned = &result.breaks[0];

        assert_eq!(planned.spots, 2);
        assert_eq!(planned.spots_sec, 60.0);
        assert_eq!(planned.gap_sec, 0.0);
        assert!(!kinds(&result).contains(&ItemKind::BreakFiller));
        assert_eq!(result.total_sec, 3660.0);
        assert!(result.warnings.is_empty(), "{:?}", result.warnings);
    }

    #[test]
    fn what_the_spots_do_not_cover_is_completed_with_filler() {
        let mut data = input(three_programs(), vec![window(1, "00:20:00", 60)], 3660.0);
        data.campaigns = vec![campaign(1, 1, vec![spot(1, 45.0)])];
        data.filler = Some(PlanFiller {
            source: "filler.mp4".to_string(),
            duration: 600.0,
        });
        let result = plan(&data);
        let filler = result
            .items
            .iter()
            .find(|item| item.kind == ItemKind::BreakFiller)
            .unwrap();

        assert_eq!(filler.source, "filler.mp4");
        assert_eq!(filler.out, 15.0, "the filler is trimmed to the gap");
        assert_eq!(filler.break_window_id, Some(1));
        assert_eq!(result.breaks[0].gap_sec, 15.0);
        assert!(codes(&result).contains(&WarningCode::BreakUnderfilled));
        assert_eq!(result.total_sec, 3660.0);
    }

    #[test]
    fn without_a_filler_file_the_gap_plays_black_and_silence() {
        let data = input(three_programs(), vec![window(1, "00:20:00", 60)], 3660.0);
        let result = plan(&data);
        let filler = result
            .items
            .iter()
            .find(|item| item.kind == ItemKind::BreakFiller)
            .unwrap();

        assert_eq!(filler.source, "");
        assert_eq!(filler.out, 60.0);
        assert_eq!(filler.duration, 60.0);
        assert!(codes(&result).contains(&WarningCode::BreakEmpty));
    }

    #[test]
    fn a_gap_under_one_second_gets_no_filler() {
        let mut data = input(three_programs(), vec![window(1, "00:20:00", 60)], 3659.5);
        data.campaigns = vec![campaign(1, 1, vec![spot(1, 59.5)])];
        let result = plan(&data);

        assert!(!kinds(&result).contains(&ItemKind::BreakFiller));
        assert!(!codes(&result).contains(&WarningCode::BreakUnderfilled));
    }

    #[test]
    fn a_spot_longer_than_the_break_is_never_used() {
        let mut data = input(three_programs(), vec![window(1, "00:20:00", 30)], 3630.0);
        data.campaigns = vec![campaign(1, 1, vec![spot(1, 31.0)])];
        let result = plan(&data);

        assert!(spot_ids(&result).is_empty());
        assert!(codes(&result).contains(&WarningCode::BreakEmpty));
    }

    #[test]
    fn a_campaign_appears_once_per_break() {
        let mut data = input(three_programs(), vec![window(1, "00:20:00", 120)], 3720.0);
        data.campaigns = vec![campaign(1, 1, vec![spot(1, 10.0), spot(2, 10.0)])];
        let result = plan(&data);

        assert_eq!(spot_ids(&result).len(), 1);
    }

    #[test]
    fn the_daily_limit_holds_across_all_breaks() {
        let mut data = input(
            three_programs(),
            vec![
                window(1, "00:20:00", 30),
                window(2, "00:40:00", 30),
                window(3, "01:00:00", 30),
            ],
            3690.0,
        );
        let mut limited = campaign(1, 1, vec![spot(1, 30.0)]);

        limited.max_per_day = Some(2);
        data.campaigns = vec![limited];
        let result = plan(&data);

        assert_eq!(spot_ids(&result), [1, 1]);
        assert_eq!(result.breaks[2].spots, 0);
    }

    #[test]
    fn the_first_and_last_places_follow_the_campaign_preference() {
        let mut data = input(three_programs(), vec![window(1, "00:20:00", 90)], 3690.0);
        let mut opener = campaign(1, 1, vec![spot(1, 30.0)]);
        let mut closer = campaign(2, 2, vec![spot(2, 30.0)]);

        opener.position = BreakPosition::First;
        closer.position = BreakPosition::Last;
        data.campaigns = vec![campaign(3, 3, vec![spot(3, 30.0)]), closer, opener];

        for day in 1..=20 {
            data.date = format!("2026-10-{day:02}");

            let ids = spot_ids(&plan(&data));

            assert_eq!(ids.first(), Some(&1), "{}", data.date);
            assert_eq!(ids.last(), Some(&2), "{}", data.date);
            assert_eq!(ids.len(), 3);
        }
    }

    #[test]
    fn the_last_place_keeps_its_room_even_when_the_rotation_is_greedy() {
        // 60 s break: the last-place spot (30 s) is reserved, so only one
        // other 30 s spot fits, never two.
        let mut data = input(three_programs(), vec![window(1, "00:20:00", 60)], 3660.0);
        let mut closer = campaign(1, 1, vec![spot(1, 30.0)]);

        closer.position = BreakPosition::Last;
        data.campaigns = vec![
            campaign(2, 2, vec![spot(2, 30.0)]),
            campaign(3, 3, vec![spot(3, 30.0)]),
            closer,
        ];
        let ids = spot_ids(&plan(&data));

        assert_eq!(ids.len(), 2);
        assert_eq!(ids.last(), Some(&1));
    }

    #[test]
    fn one_advertiser_is_not_repeated_in_a_row_when_there_is_a_choice() {
        let mut data = input(three_programs(), vec![window(1, "00:20:00", 120)], 3720.0);
        data.campaigns = vec![
            campaign(1, 1, vec![spot(1, 30.0)]),
            campaign(2, 1, vec![spot(2, 30.0)]),
            campaign(3, 2, vec![spot(3, 30.0)]),
            campaign(4, 2, vec![spot(4, 30.0)]),
        ];

        for day in 1..=30 {
            data.date = format!("2026-10-{day:02}");

            let advertisers: Vec<i32> = plan(&data)
                .items
                .iter()
                .filter_map(|item| item.advertiser_id)
                .collect();

            assert_eq!(advertisers.len(), 4);
            assert!(
                advertisers.windows(2).all(|pair| pair[0] != pair[1]),
                "{}: {advertisers:?}",
                data.date
            );
        }
    }

    #[test]
    fn spots_of_a_campaign_rotate_through_the_day() {
        let mut data = input(
            three_programs(),
            vec![
                window(1, "00:20:00", 30),
                window(2, "00:40:00", 30),
                window(3, "01:00:00", 30),
            ],
            3690.0,
        );
        data.campaigns = vec![campaign(
            1,
            1,
            vec![spot(1, 30.0), spot(2, 30.0), spot(3, 30.0)],
        )];
        let mut ids = spot_ids(&plan(&data));

        ids.sort_unstable();

        assert_eq!(ids, [1, 2, 3], "every spot is used once before any repeats");
    }

    #[test]
    fn the_same_day_always_gives_the_same_plan() {
        let mut data = input(
            three_programs(),
            vec![window(1, "00:20:00", 90), window(2, "00:40:00", 90)],
            3780.0,
        );
        data.campaigns = (1..=6)
            .map(|id| campaign(id, id, vec![spot(id, 20.0 + f64::from(id))]))
            .collect();

        assert_eq!(plan(&data), plan(&data));
    }

    #[test]
    fn different_days_rotate_differently() {
        let mut data = input(three_programs(), vec![window(1, "00:20:00", 60)], 3660.0);
        data.campaigns = (1..=8)
            .map(|id| campaign(id, id, vec![spot(id, 30.0)]))
            .collect();

        let mut seen = HashSet::new();

        for day in 1..=28 {
            data.date = format!("2026-10-{day:02}");

            seen.insert(spot_ids(&plan(&data)));
        }

        assert!(
            seen.len() > 5,
            "only {} different breaks in 28 days",
            seen.len()
        );
    }

    #[test]
    fn heavier_campaigns_are_chosen_more_often() {
        let mut data = input(three_programs(), vec![window(1, "00:20:00", 30)], 3630.0);
        let mut heavy = campaign(1, 1, vec![spot(1, 30.0)]);

        heavy.weight = 9;
        data.campaigns = vec![heavy, campaign(2, 2, vec![spot(2, 30.0)])];

        let mut heavy_days = 0;

        for day in 0..300 {
            data.date = format!("2027-{:02}-{:02}", day / 28 % 12 + 1, day % 28 + 1);

            if spot_ids(&plan(&data)) == [1] {
                heavy_days += 1;
            }
        }

        assert!(
            (240..=295).contains(&heavy_days),
            "weight 9 against 1 should win about 90% of 300 days, won {heavy_days}"
        );
    }

    #[test]
    fn a_short_day_is_completed_with_filler_at_the_end() {
        let data = input(three_programs(), Vec::new(), 3700.0);
        let result = plan(&data);
        let last = result.items.last().unwrap();

        assert_eq!(last.kind, ItemKind::DayFiller);
        assert_eq!(last.out, 100.0);
        assert_eq!(last.start_sec, 3600.0);
        assert_eq!(result.total_sec, 3700.0);
        assert!(codes(&result).contains(&WarningCode::DayFilled));
        assert!(!result.has_errors());
    }

    #[test]
    fn a_day_that_runs_over_is_blocked_beyond_the_tolerance() {
        let data = input(three_programs(), Vec::new(), 3590.0);
        let result = plan(&data);

        assert!(result.has_errors());
        assert!(codes(&result).contains(&WarningCode::DayTooLong));
        assert_eq!(result.total_sec, 3600.0);
    }

    #[test]
    fn a_small_overrun_is_only_a_warning() {
        let data = input(three_programs(), Vec::new(), 3597.0);
        let result = plan(&data);

        assert!(!result.has_errors());
        assert!(codes(&result).contains(&WarningCode::DaySlightlyLong));
    }

    #[test]
    fn a_day_that_fits_has_no_warnings() {
        let data = input(three_programs(), Vec::new(), 3600.0);
        let result = plan(&data);

        assert!(result.warnings.is_empty());
        assert_eq!(result.programs_sec, 3600.0);
        assert_eq!(result.breaks_sec, 0.0);
    }

    #[test]
    fn items_follow_each_other_without_gaps_and_report_clock_times() {
        let mut data = input(
            three_programs(),
            vec![window(1, "00:20:00", 90), window(2, "00:40:00", 60)],
            3750.0,
        );
        data.day_start_sec = 5.0 * 3600.0 + 59.0 * 60.0 + 25.0;
        data.campaigns = vec![
            campaign(1, 1, vec![spot(1, 30.0)]),
            campaign(2, 2, vec![spot(2, 30.0)]),
        ];
        let result = plan(&data);
        let mut expected = 0.0;

        for item in &result.items {
            assert!(
                (item.start_sec - expected).abs() < 1e-6,
                "{:?} starts at {} but {} was expected",
                item.kind,
                item.start_sec,
                expected
            );

            expected += item.airtime();
        }

        assert_eq!(result.items[0].start_time, "05:59:25");
        assert_eq!(result.total_sec, expected);
    }

    #[test]
    fn spots_carry_their_references_and_a_readable_title() {
        let mut data = input(three_programs(), vec![window(1, "00:20:00", 30)], 3630.0);
        data.campaigns = vec![campaign(7, 4, vec![spot(9, 30.0)])];
        let result = plan(&data);
        let item = result
            .items
            .iter()
            .find(|i| i.kind == ItemKind::Spot)
            .unwrap();

        assert_eq!(item.spot_id, Some(9));
        assert_eq!(item.campaign_id, Some(7));
        assert_eq!(item.advertiser_id, Some(4));
        assert_eq!(item.break_window_id, Some(1));
        assert_eq!(item.position_in_break, Some(0));
        assert_eq!(item.title, "advertiser 4 - spot 9");
        assert_eq!(item.source, "commercials/spot-9.mp4");
    }

    #[test]
    fn programs_keep_their_own_seek_and_out() {
        let mut data = input(vec![program("a", 20.0)], Vec::new(), 1190.0);
        data.programs[0].seek = 10.0;
        data.programs[0].out = 1200.0;
        data.programs[0].duration = 1500.0;
        let result = plan(&data);

        assert_eq!(result.items[0].seek, 10.0);
        assert_eq!(result.items[0].out, 1200.0);
        assert_eq!(result.items[0].duration, 1500.0);
        assert_eq!(result.programs_sec, 1190.0);
    }

    #[test]
    fn clock_text_wraps_at_midnight() {
        assert_eq!(clock(0.0), "00:00:00");
        assert_eq!(clock(86_399.4), "23:59:59");
        assert_eq!(clock(86_400.0 + 61.0), "00:01:01");
        assert_eq!(parse_clock("20:00:00"), Some(72_000.0));
        assert_eq!(parse_clock("24:00:00"), None);
        assert_eq!(parse_clock("20:00"), None);
        assert_eq!(parse_clock("20:00:00:00"), None);
    }
}
