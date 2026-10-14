//! The campaign report as a PDF page set.
//!
//! The layout is computed first, page by page, so every page can say
//! "page X of Y". Text uses the built-in Helvetica fonts, which cover the
//! accents of Portuguese; characters outside Latin-1 become `?`.

use std::{fmt::Display, io::Cursor};

use log::*;
use printpdf::{
    BuiltinFont, Color, ImageTransform, IndirectFontRef, Line, Mm, PdfDocument,
    PdfDocumentReference, PdfLayerReference, Point, Rgb,
    image_crate::{self, DynamicImage, ImageFormat, RgbImage},
};

use crate::utils::{
    errors::ServiceError,
    report::{DayTotals, Report, ReportLine},
};

const PAGE_WIDTH: f32 = 210.0;
const PAGE_HEIGHT: f32 = 297.0;
const MARGIN: f32 = 15.0;
const BOTTOM: f32 = 22.0;
const CONTENT_WIDTH: f32 = PAGE_WIDTH - 2.0 * MARGIN;
const ROW_HEIGHT: f32 = 5.0;
const LOGO_HEIGHT: f32 = 18.0;
const LOGO_MAX_WIDTH: f32 = 50.0;
const LOGO_DPI: f32 = 300.0;
const LOGO_MAX_PIXELS: u32 = 4096;
/// Helvetica averages about half an em per character.
const AVERAGE_CHAR_EM: f32 = 0.5;
const POINT_IN_MM: f32 = 0.3528;

#[derive(Debug, Clone)]
enum Item {
    Text {
        x: f32,
        y: f32,
        size: f32,
        bold: bool,
        text: String,
    },
    Rule {
        y: f32,
    },
}

#[derive(Debug, Default)]
struct Page {
    items: Vec<Item>,
}

struct Column {
    title: &'static str,
    width: f32,
    /// Numbers line up better when they start at the same offset.
    size: f32,
}

fn latin1(text: &str) -> String {
    text.chars()
        .map(|character| match character {
            '\t' | '\n' | '\r' => ' ',
            character
                if (character as u32) < 0x20 || (0x7F..0xA0).contains(&(character as u32)) =>
            {
                '?'
            }
            character if (character as u32) <= 0xFF => character,
            _ => '?',
        })
        .collect()
}

/// Cut a text so it fits `width` millimetres at `size` points.
fn fit(text: &str, width: f32, size: f32) -> String {
    let text = latin1(text);
    let capacity = (width / (size * AVERAGE_CHAR_EM * POINT_IN_MM)).floor() as usize;

    if text.chars().count() <= capacity {
        return text;
    }

    let kept: String = text.chars().take(capacity.saturating_sub(3)).collect();

    format!("{}...", kept.trim_end())
}

/// `2026-12-10` as `10/12/2026`.
fn brazilian_date(date: &str) -> String {
    let parts: Vec<&str> = date.split('-').collect();

    match parts.as_slice() {
        [year, month, day] => format!("{day}/{month}/{year}"),
        _ => date.to_string(),
    }
}

fn brazilian_date_time(text: &str) -> String {
    match text.split_once(' ') {
        Some((date, time)) => format!("{} {time}", brazilian_date(date)),
        None => brazilian_date(text),
    }
}

struct Layout {
    pages: Vec<Page>,
    y: f32,
}

impl Layout {
    fn new() -> Self {
        Self {
            pages: vec![Page::default()],
            y: PAGE_HEIGHT - MARGIN,
        }
    }

    fn page(&mut self) -> &mut Page {
        self.pages.last_mut().expect("a layout always has a page")
    }

    fn text(&mut self, x: f32, size: f32, bold: bool, text: &str) {
        let y = self.y;

        self.page().items.push(Item::Text {
            x,
            y,
            size,
            bold,
            text: latin1(text),
        });
    }

    fn rule(&mut self, offset: f32) {
        let y = self.y + offset;

        self.page().items.push(Item::Rule { y });
    }

    fn down(&mut self, millimetres: f32) {
        self.y -= millimetres;
    }

    /// Start a new page when fewer than `needed` millimetres are left.
    fn ensure(&mut self, needed: f32) -> bool {
        if self.y - needed >= BOTTOM {
            return false;
        }

        self.pages.push(Page::default());
        self.y = PAGE_HEIGHT - MARGIN;

        true
    }

    fn table_header(&mut self, columns: &[Column]) {
        let mut x = MARGIN;

        for column in columns {
            self.text(x, column.size, true, column.title);
            x += column.width;
        }

        self.rule(-1.5);
        self.down(ROW_HEIGHT);
    }

    fn table_row(&mut self, columns: &[Column], cells: &[String], bold: bool) {
        let mut x = MARGIN;

        for (column, cell) in columns.iter().zip(cells) {
            let text = fit(cell, column.width - 1.5, column.size);

            self.text(x, column.size, bold, &text);
            x += column.width;
        }

        self.down(ROW_HEIGHT);
    }

    /// Rows with a repeated header whenever the table crosses a page.
    fn table(&mut self, columns: &[Column], rows: &[Vec<String>], bold_last: bool) {
        self.ensure(ROW_HEIGHT * 3.0);
        self.table_header(columns);

        for (index, row) in rows.iter().enumerate() {
            if self.ensure(ROW_HEIGHT) {
                self.table_header(columns);
            }

            if bold_last && index + 1 == rows.len() {
                self.rule(ROW_HEIGHT - 1.5);
            }

            self.table_row(columns, row, bold_last && index + 1 == rows.len());
        }
    }
}

fn summary_row(day: &DayTotals, label: String) -> Vec<String> {
    vec![
        label,
        day.planned.to_string(),
        day.aired.to_string(),
        day.partial.to_string(),
        day.missed.to_string(),
        day.pending.to_string(),
    ]
}

fn detail_row(report: &Report, line: &ReportLine) -> Vec<String> {
    let what = if report.header.advertiser.is_none() {
        format!("{} / {} / {}", line.advertiser, line.campaign, line.spot)
    } else {
        format!("{} / {}", line.campaign, line.spot)
    };
    let duration = line
        .duration_sec
        .map(|seconds| format!("{seconds:.1}").replace('.', ","))
        .unwrap_or_default();

    vec![
        brazilian_date(&line.date),
        line.planned_time.clone(),
        line.aired_time.clone(),
        duration,
        what,
        line.status.clone(),
        line.note.clone(),
    ]
}

/// Logo size in millimetres once it is scaled to the header.
fn logo_size(width_px: u32, height_px: u32) -> (f32, f32) {
    let natural_width = width_px as f32 / LOGO_DPI * 25.4;
    let natural_height = height_px as f32 / LOGO_DPI * 25.4;
    let scale = (LOGO_HEIGHT / natural_height).min(LOGO_MAX_WIDTH / natural_width);

    (natural_width * scale, natural_height * scale)
}

/// A PNG or JPEG as an opaque RGB image; transparency is flattened onto white.
fn decode_logo(bytes: &[u8]) -> Option<RgbImage> {
    let format = image_crate::guess_format(bytes).ok()?;

    if !matches!(format, ImageFormat::Png | ImageFormat::Jpeg) {
        return None;
    }

    let (width, height) = image_crate::io::Reader::with_format(Cursor::new(bytes), format)
        .into_dimensions()
        .ok()?;

    if width == 0 || height == 0 || width > LOGO_MAX_PIXELS || height > LOGO_MAX_PIXELS {
        return None;
    }

    let decoded = image_crate::load_from_memory_with_format(bytes, format).ok()?;
    let mut rgb = RgbImage::new(decoded.width(), decoded.height());

    for (x, y, pixel) in decoded.to_rgba8().enumerate_pixels() {
        let alpha = u32::from(pixel[3]);
        let flatten =
            |channel: u8| ((u32::from(channel) * alpha + 255 * (255 - alpha)) / 255) as u8;

        rgb.put_pixel(
            x,
            y,
            image_crate::Rgb([flatten(pixel[0]), flatten(pixel[1]), flatten(pixel[2])]),
        );
    }

    Some(rgb)
}

fn compose(report: &Report, logo_size_mm: Option<(f32, f32)>) -> Vec<Page> {
    let header = &report.header;
    let mut layout = Layout::new();
    let text_x = MARGIN + logo_size_mm.map_or(0.0, |(width, _)| width + 5.0);

    layout.down(5.0);
    layout.text(text_x, 16.0, true, &header.station_name);

    for line in header.station_details.iter().take(4) {
        layout.down(4.5);
        layout.text(
            text_x,
            8.0,
            false,
            &fit(line, CONTENT_WIDTH - (text_x - MARGIN), 8.0),
        );
    }

    layout.y = layout.y.min(PAGE_HEIGHT - MARGIN - LOGO_HEIGHT) - 6.0;
    layout.rule(3.0);
    layout.down(4.0);
    layout.text(MARGIN, 14.0, true, "Relatório de veiculação");
    layout.down(8.0);

    let all = "Todos";
    let period = format!(
        "{} a {}",
        brazilian_date(&header.date_from),
        brazilian_date(&header.date_to)
    );

    for (label, value) in [
        ("Anunciante", header.advertiser.as_deref().unwrap_or(all)),
        ("Campanha", header.campaign.as_deref().unwrap_or(all)),
        ("Período", period.as_str()),
        (
            "Emitido em",
            &format!(
                "{} (horário de {})",
                brazilian_date_time(&header.issued_at),
                header.timezone
            ),
        ),
    ] {
        layout.text(MARGIN, 9.0, true, label);
        layout.text(
            MARGIN + 28.0,
            9.0,
            false,
            &fit(value, CONTENT_WIDTH - 28.0, 9.0),
        );
        layout.down(5.0);
    }

    layout.down(4.0);
    layout.text(MARGIN, 11.0, true, "Resumo por dia");
    layout.down(6.0);

    let summary_columns = [
        Column {
            title: "Data",
            width: 40.0,
            size: 9.0,
        },
        Column {
            title: "Previstas",
            width: 28.0,
            size: 9.0,
        },
        Column {
            title: "Veiculadas",
            width: 28.0,
            size: 9.0,
        },
        Column {
            title: "Parciais",
            width: 28.0,
            size: 9.0,
        },
        Column {
            title: "Não veiculadas",
            width: 32.0,
            size: 9.0,
        },
        Column {
            title: "Pendentes",
            width: 24.0,
            size: 9.0,
        },
    ];
    let mut summary: Vec<Vec<String>> = report
        .days
        .iter()
        .map(|day| summary_row(day, brazilian_date(&day.date)))
        .collect();

    summary.push(summary_row(&report.totals, "Total".to_string()));
    layout.table(&summary_columns, &summary, true);

    layout.down(5.0);
    layout.ensure(ROW_HEIGHT * 5.0);
    layout.text(MARGIN, 11.0, true, "Veiculações");
    layout.down(6.0);

    if report.lines.is_empty() {
        layout.text(
            MARGIN,
            9.0,
            false,
            "Nenhuma veiculação registrada no período.",
        );
        layout.down(ROW_HEIGHT);
    } else {
        let detail_columns = [
            Column {
                title: "Data",
                width: 20.0,
                size: 8.0,
            },
            Column {
                title: "Previsto",
                width: 17.0,
                size: 8.0,
            },
            Column {
                title: "Veiculado",
                width: 18.0,
                size: 8.0,
            },
            Column {
                title: "Dur. (s)",
                width: 14.0,
                size: 8.0,
            },
            Column {
                title: "Anunciante / Campanha / Spot",
                width: 56.0,
                size: 8.0,
            },
            Column {
                title: "Situação",
                width: 27.0,
                size: 8.0,
            },
            Column {
                title: "Observação",
                width: 28.0,
                size: 8.0,
            },
        ];
        let rows: Vec<Vec<String>> = report
            .lines
            .iter()
            .map(|line| detail_row(report, line))
            .collect();

        layout.table(&detail_columns, &rows, false);
    }

    layout.down(4.0);

    let closing: Vec<String> = report
        .notes
        .iter()
        .cloned()
        .chain([format!(
            "Os horários seguem o fuso da emissora ({}).",
            header.timezone
        )])
        .chain((!header.footer_note.trim().is_empty()).then(|| header.footer_note.clone()))
        .collect();

    for note in closing {
        // Long notes wrap at the width of the page.
        for part in wrap(&note, CONTENT_WIDTH, 8.0) {
            layout.ensure(ROW_HEIGHT);
            layout.text(MARGIN, 8.0, false, &part);
            layout.down(4.0);
        }
    }

    layout.pages
}

/// Break a text into lines that fit `width` millimetres.
fn wrap(text: &str, width: f32, size: f32) -> Vec<String> {
    let capacity = ((width / (size * AVERAGE_CHAR_EM * POINT_IN_MM)).floor() as usize).max(10);
    let mut lines = Vec::new();
    let mut current = String::new();

    for word in latin1(text).split_whitespace() {
        let word: String = word.chars().take(capacity).collect();

        if !current.is_empty() && current.chars().count() + 1 + word.chars().count() > capacity {
            lines.push(std::mem::take(&mut current));
        }

        if !current.is_empty() {
            current.push(' ');
        }

        current.push_str(&word);
    }

    if !current.is_empty() {
        lines.push(current);
    }

    lines
}

fn draw_page(
    layer: &PdfLayerReference,
    page: &Page,
    regular: &IndirectFontRef,
    bold: &IndirectFontRef,
) {
    layer.set_outline_thickness(0.3);
    layer.set_outline_color(Color::Rgb(Rgb::new(0.6, 0.6, 0.6, None)));
    layer.set_fill_color(Color::Rgb(Rgb::new(0.0, 0.0, 0.0, None)));

    for item in &page.items {
        match item {
            Item::Text {
                x,
                y,
                size,
                bold: is_bold,
                text,
            } => {
                layer.use_text(
                    text.as_str(),
                    *size,
                    Mm(*x),
                    Mm(*y),
                    if *is_bold { bold } else { regular },
                );
            }
            Item::Rule { y } => layer.add_line(Line {
                points: vec![
                    (Point::new(Mm(MARGIN), Mm(*y)), false),
                    (Point::new(Mm(PAGE_WIDTH - MARGIN), Mm(*y)), false),
                ],
                is_closed: false,
            }),
        }
    }
}

fn pdf_error(error: impl Display) -> ServiceError {
    error!("Cannot build the PDF report: {error}");

    ServiceError::InternalServerError
}

/// Render the report. A logo that cannot be read is left out: the report is
/// more useful without it than not at all.
pub fn render_pdf(report: &Report, logo: Option<&[u8]>) -> Result<Vec<u8>, ServiceError> {
    let logo = logo.and_then(decode_logo);
    let logo_size_mm = logo
        .as_ref()
        .map(|image| logo_size(image.width(), image.height()));
    let pages = compose(report, logo_size_mm);
    let title = format!("Relatório de veiculação {}", report.header.station_name);
    let (document, first_page, first_layer): (PdfDocumentReference, _, _) =
        PdfDocument::new(latin1(&title), Mm(PAGE_WIDTH), Mm(PAGE_HEIGHT), "Conteúdo");
    let regular = document
        .add_builtin_font(BuiltinFont::Helvetica)
        .map_err(pdf_error)?;
    let bold = document
        .add_builtin_font(BuiltinFont::HelveticaBold)
        .map_err(pdf_error)?;
    let total = pages.len();

    for (index, page) in pages.iter().enumerate() {
        let layer = if index == 0 {
            document.get_page(first_page).get_layer(first_layer)
        } else {
            let (page_ref, layer_ref) =
                document.add_page(Mm(PAGE_WIDTH), Mm(PAGE_HEIGHT), "Conteúdo");

            document.get_page(page_ref).get_layer(layer_ref)
        };

        draw_page(&layer, page, &regular, &bold);
        layer.use_text(
            format!("Página {} de {total}", index + 1),
            8.0,
            Mm(PAGE_WIDTH - MARGIN - 22.0),
            Mm(MARGIN - 4.0),
            &regular,
        );

        if index == 0
            && let (Some(image), Some((_, height_mm))) = (&logo, logo_size_mm)
        {
            let natural_height_mm = image.height() as f32 / LOGO_DPI * 25.4;

            printpdf::Image::from_dynamic_image(&DynamicImage::ImageRgb8(image.clone()))
                .add_to_layer(
                    layer.clone(),
                    ImageTransform {
                        translate_x: Some(Mm(MARGIN)),
                        translate_y: Some(Mm(PAGE_HEIGHT - MARGIN - height_mm)),
                        scale_x: Some(height_mm / natural_height_mm),
                        scale_y: Some(height_mm / natural_height_mm),
                        dpi: Some(LOGO_DPI),
                        ..ImageTransform::default()
                    },
                );
        }
    }

    document.save_to_bytes().map_err(pdf_error)
}

#[cfg(test)]
mod tests {
    use std::{
        io::Write,
        process::{Command, Stdio},
    };

    use super::*;
    use crate::{
        db::models::AirResult,
        utils::report::{Report, ReportHeader},
    };

    fn line(index: usize) -> ReportLine {
        ReportLine {
            date: "2026-12-10".to_string(),
            planned_time: "09:00:00".to_string(),
            aired_time: "09:00:02".to_string(),
            duration_sec: Some(29.9),
            advertiser: "Padaria São João".to_string(),
            campaign: "Natal".to_string(),
            spot: format!("Spot número {index}"),
            result: AirResult::Played,
            status: "Veiculado".to_string(),
            note: String::new(),
        }
    }

    fn report(lines: usize) -> Report {
        let totals = DayTotals {
            date: "total".to_string(),
            planned: lines as u32,
            aired: lines as u32,
            ..DayTotals::default()
        };

        Report {
            header: ReportHeader {
                station_name: "TV Exemplo".to_string(),
                station_details: vec![
                    "Rua das Flores, 100".to_string(),
                    "CNPJ 00.000.000/0001-00".to_string(),
                ],
                logo_path: String::new(),
                footer_note: "Documento sem valor fiscal.".to_string(),
                advertiser: Some("Padaria São João".to_string()),
                campaign: None,
                date_from: "2026-12-01".to_string(),
                date_to: "2026-12-31".to_string(),
                issued_at: "2026-12-31 18:30".to_string(),
                timezone: "America/Sao_Paulo".to_string(),
            },
            lines: (0..lines).map(line).collect(),
            days: vec![DayTotals {
                date: "2026-12-10".to_string(),
                ..totals.clone()
            }],
            totals,
            notes: vec!["1 spot(s) ainda não foram exibidos.".to_string()],
        }
    }

    /// The text of a PDF, or `None` when `pdftotext` is not installed.
    fn text_of(pdf: &[u8]) -> Option<String> {
        let mut child = Command::new("pdftotext")
            .args(["-layout", "-", "-"])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .spawn()
            .ok()?;

        child.stdin.take()?.write_all(pdf).ok()?;

        let output = child.wait_with_output().ok()?;

        Some(String::from_utf8_lossy(&output.stdout).to_string())
    }

    fn png(width: u32, height: u32) -> Vec<u8> {
        let image = RgbImage::from_pixel(width, height, image_crate::Rgb([200, 30, 30]));
        let mut bytes = Cursor::new(Vec::new());

        DynamicImage::ImageRgb8(image)
            .write_to(&mut bytes, ImageFormat::Png)
            .unwrap();

        bytes.into_inner()
    }

    #[test]
    fn the_pdf_carries_the_portuguese_text_of_the_report() {
        let bytes = render_pdf(&report(3), None).unwrap();

        assert!(bytes.starts_with(b"%PDF"));

        if let Some(text) = text_of(&bytes) {
            for expected in [
                "TV Exemplo",
                "Rua das Flores, 100",
                "Relatório de veiculação",
                "Padaria São João",
                "10/12/2026",
                "Não veiculadas",
                "Spot número 2",
                "Documento sem valor fiscal.",
                "Página 1 de 1",
                "horário de America/Sao_Paulo",
            ] {
                assert!(text.contains(expected), "missing {expected:?} in:\n{text}");
            }
        }
    }

    #[test]
    fn a_long_report_continues_on_more_pages_with_the_header_repeated() {
        let bytes = render_pdf(&report(120), None).unwrap();

        if let Some(text) = text_of(&bytes) {
            assert!(
                text.contains("Página 1 de 3") || text.contains("Página 1 de 4"),
                "{text}"
            );
            assert!(text.contains("Spot número 119"));
            assert!(
                text.matches("Observação").count() >= 2,
                "the table header repeats"
            );
        }
    }

    #[test]
    fn a_logo_is_embedded_and_a_broken_one_is_left_out() {
        let plain = render_pdf(&report(1), None).unwrap();
        let with_logo = render_pdf(&report(1), Some(&png(600, 200))).unwrap();
        let broken = render_pdf(&report(1), Some(b"not an image")).unwrap();

        assert!(with_logo.len() > plain.len(), "the image adds bytes");
        assert!(broken.starts_with(b"%PDF"));
        assert!(
            decode_logo(&png(5000, 10)).is_none(),
            "oversized images are refused"
        );
    }

    #[test]
    fn long_text_is_cut_and_unknown_characters_become_question_marks() {
        let cut = fit(&"a".repeat(200), 30.0, 8.0);

        assert!(cut.ends_with("...") && cut.chars().count() < 40);
        assert_eq!(latin1("ação ✓"), "ação ?");
        assert_eq!(brazilian_date("2026-12-10"), "10/12/2026");

        let wrapped = wrap(&"palavra ".repeat(40), 60.0, 8.0);

        assert!(wrapped.len() > 1 && wrapped.iter().all(|part| part.chars().count() <= 43));
    }
}
