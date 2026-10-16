//! The texts the bot sends, in Portuguese. Telegram reads them as HTML, so
//! everything that comes from outside goes through [`esc`].

use chrono::{DateTime, Datelike, NaiveDate, Timelike};
use chrono_tz::Tz;

use super::monitor::{Key, Notice, NoticeKind};

pub fn esc(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

/// At most `max` characters, with an ellipsis when it was cut.
pub fn truncate(text: &str, max: usize) -> String {
    if text.chars().count() <= max {
        return text.to_string();
    }

    let mut cut: String = text.chars().take(max.saturating_sub(1)).collect();

    cut.push('…');

    cut
}

pub fn duration_text(seconds: f64) -> String {
    let total = seconds.max(0.0).round() as u64;

    if total >= 3600 {
        format!("{} h {:02} min", total / 3600, total % 3600 / 60)
    } else if total >= 60 {
        format!("{} min {:02} s", total / 60, total % 60)
    } else {
        format!("{total} s")
    }
}

pub fn clock_text(time: &DateTime<Tz>) -> String {
    format!(
        "{:02}:{:02}:{:02}",
        time.hour(),
        time.minute(),
        time.second()
    )
}

pub fn short_clock(time: &DateTime<Tz>) -> String {
    format!("{:02}:{:02}", time.hour(), time.minute())
}

pub fn date_text(date: NaiveDate) -> String {
    format!("{:02}/{:02}/{}", date.day(), date.month(), date.year())
}

/// The title of an alert when the problem starts and when it is over.
pub fn headline(key: &Key) -> (&'static str, &'static str) {
    match key {
        Key::Output => ("Saída fora do ar", "Saída normalizada"),
        Key::Stopped => ("Canal parado", "Canal ligado de novo"),
        Key::Relay => ("Relay sem resposta", "Relay respondendo"),
        Key::Destination(_) => ("Destino desconectado", "Destino reconectado"),
        Key::Filler => (
            "Arquivo de reserva ausente",
            "Arquivo de reserva encontrado",
        ),
        Key::Playlist(_) => ("Dia sem playlist", "Playlist criada"),
        Key::Disk => ("Pouco espaço em disco", "Espaço em disco normalizado"),
    }
}

/// A problem that takes the picture off the air or cuts a destination.
fn is_critical(key: &Key) -> bool {
    matches!(key, Key::Output | Key::Destination(_))
}

/// The message of an alert, ready to send.
pub fn alert(notice: &Notice, channel: &str, at: &DateTime<Tz>) -> String {
    let (problem, resolved) = headline(&notice.key);
    let footer = format!("<i>{} · {}</i>", esc(channel), clock_text(at));

    match notice.kind {
        NoticeKind::Raised => {
            let mark = if is_critical(&notice.key) {
                "🔴"
            } else {
                "⚠️"
            };

            format!("{mark} <b>{problem}</b>\n{}\n{footer}", notice.body)
        }
        NoticeKind::Reminder => format!(
            "⏰ <b>{problem}</b> (continua há {})\n{}\n{footer}",
            duration_text(notice.lasted.num_seconds() as f64),
            notice.body
        ),
        NoticeKind::Recovered => {
            let failures = if notice.failures > 1 {
                format!(" · {} falhas", notice.failures)
            } else {
                String::new()
            };

            format!(
                "✅ <b>{resolved}</b>\n{}\nFicou assim por {}{failures}\n{footer}",
                notice.body,
                duration_text(notice.lasted.num_seconds() as f64)
            )
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn html_from_outside_cannot_break_the_message() {
        assert_eq!(esc("a <b>&</b>"), "a &lt;b&gt;&amp;&lt;/b&gt;");
    }

    #[test]
    fn long_texts_are_cut_with_an_ellipsis() {
        assert_eq!(truncate("abcdef", 4), "abc…");
        assert_eq!(truncate("abc", 4), "abc");
        assert_eq!(truncate("ação", 4), "ação");
    }

    #[test]
    fn durations_read_naturally() {
        assert_eq!(duration_text(0.0), "0 s");
        assert_eq!(duration_text(42.4), "42 s");
        assert_eq!(duration_text(185.0), "3 min 05 s");
        assert_eq!(duration_text(4800.0), "1 h 20 min");
        assert_eq!(duration_text(-5.0), "0 s");
    }

    #[test]
    fn dates_are_written_the_brazilian_way() {
        assert_eq!(
            date_text(NaiveDate::from_ymd_opt(2026, 10, 5).unwrap()),
            "05/10/2026"
        );
    }
}
