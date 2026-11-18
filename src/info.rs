//! The document's properties, shown by `:info`.
use std::path::Path;
use std::sync::Arc;

use pdfcraft_cos::{Document, Object};

use crate::app::{App, Mode};
use crate::form;

/// Everything `:info` shows, as plain data.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Facts {
    pub path: String,
    pub size: u64,
    pub pages: usize,
    /// The current page, 1-based, and its size in points.
    pub page: usize,
    pub page_size: [f32; 2],
    pub version: String,
    pub title: Option<String>,
    pub author: Option<String>,
    pub subject: Option<String>,
    pub keywords: Option<String>,
    pub creator: Option<String>,
    pub producer: Option<String>,
    /// The raw PDF dates.
    pub created: Option<String>,
    pub modified: Option<String>,
    pub encrypted: bool,
    pub form: bool,
    pub outline: bool,
    pub signatures: usize,
    pub tagged: bool,
}

#[derive(Clone, Debug)]
pub struct InfoState {
    pub rows: Vec<(String, String)>,
    pub first: usize,
    /// The part of a row the wheel has moved the panel by and not yet scrolled.
    pub wheel_rest: f32,
}

/// `bytes` as a size people read: `1.5 MiB (1572864 bytes)`.
pub fn human_size(bytes: u64) -> String {
    const UNITS: [&str; 4] = ["KiB", "MiB", "GiB", "TiB"];
    if bytes < 1024 {
        return format!("{bytes} bytes");
    }
    let mut value = bytes as f64;
    let mut unit = "";
    for next in UNITS {
        if value < 1024.0 {
            break;
        }
        value /= 1024.0;
        unit = next;
    }
    format!("{value:.1} {unit} ({bytes} bytes)")
}

/// A PDF date such as `D:20240131120000+01'00'` as `2024-01-31 12:00:00 +01:00`. What is left out
/// of the date is left out of the result; text that is no date comes back as it is.
pub fn pdf_date(raw: &str) -> String {
    parse_date(raw.trim()).unwrap_or_else(|| raw.trim().to_owned())
}

fn parse_date(text: &str) -> Option<String> {
    let rest = text.strip_prefix("D:").unwrap_or(text);
    let digits = rest.bytes().take_while(u8::is_ascii_digit).count();
    let (number, zone) = rest.split_at(digits);
    if ![4, 6, 8, 10, 12, 14].contains(&number.len()) {
        return None;
    }
    let part = |from: usize, len: usize, default: &str| number.get(from..from + len).unwrap_or(default).to_owned();
    let (year, month, day) = (part(0, 4, ""), part(4, 2, "01"), part(6, 2, "01"));
    let in_range = |s: &str, max: u32| s.parse::<u32>().is_ok_and(|n| n <= max);
    let (hour, minute, second) = (part(8, 2, "00"), part(10, 2, "00"), part(12, 2, "00"));
    let valid = in_range(&month, 12) && month != "00" && in_range(&day, 31) && day != "00";
    if !valid || !in_range(&hour, 23) || !in_range(&minute, 59) || !in_range(&second, 60) {
        return None;
    }
    let mut out = format!("{year}-{month}-{day}");
    if number.len() > 8 {
        out.push_str(&format!(" {hour}:{minute}:{second}"));
    }
    if let Some(zone) = date_zone(zone) {
        out.push(' ');
        out.push_str(&zone);
    }
    Some(out)
}

/// The time zone of a PDF date: `Z`, or `+01'00'`, `-0500`, `+01`.
fn date_zone(zone: &str) -> Option<String> {
    let mut chars = zone.chars();
    let sign = chars.next()?;
    if sign == 'Z' {
        return Some("UTC".to_owned());
    }
    if sign != '+' && sign != '-' {
        return None;
    }
    let digits: String = chars.filter(char::is_ascii_digit).collect();
    let (hours, minutes) = match digits.len() {
        2 => (digits.as_str(), "00"),
        4 => (&digits[..2], &digits[2..]),
        _ => return None,
    };
    Some(format!("{sign}{hours}:{minutes}"))
}

/// The paper a page of `size` points is, in either orientation, when it is within a few points of one.
pub fn paper_name(size: [f32; 2]) -> Option<&'static str> {
    const PAPERS: [(&str, f32, f32); 8] = [
        ("A3", 841.89, 1190.55),
        ("A4", 595.28, 841.89),
        ("A5", 419.53, 595.28),
        ("A6", 297.64, 419.53),
        ("B5", 498.90, 708.66),
        ("Letter", 612.0, 792.0),
        ("Legal", 612.0, 1008.0),
        ("Tabloid", 792.0, 1224.0),
    ];
    let (short, long) = (size[0].min(size[1]), size[0].max(size[1]));
    PAPERS.iter().find(|(_, w, h)| (short - w).abs() <= 3.0 && (long - h).abs() <= 3.0).map(|(name, ..)| *name)
}

/// A page size as `595 x 842 pt (210 x 297 mm, A4)`.
pub fn page_size_text(size: [f32; 2]) -> String {
    let mm = |points: f32| (points * 25.4 / 72.0).round();
    let mut text = format!("{} \u{d7} {} pt ({} \u{d7} {} mm", size[0].round(), size[1].round(), mm(size[0]), mm(size[1]));
    if let Some(name) = paper_name(size) {
        text.push_str(", ");
        text.push_str(name);
    }
    text.push(')');
    text
}

fn yes_no(on: bool) -> String {
    if on { "yes" } else { "no" }.to_owned()
}

/// The rows of the panel: a name and a value each. A property with no value is left out.
pub fn rows(facts: &Facts) -> Vec<(String, String)> {
    let text =
        |name: &str, value: &Option<String>| value.as_deref().map(str::trim).filter(|v| !v.is_empty()).map(|v| (name.to_owned(), v.to_owned()));
    let date = |name: &str, value: &Option<String>| {
        let value = value.as_deref().map(pdf_date).filter(|v| !v.is_empty());
        value.map(|v| (name.to_owned(), v))
    };
    let known = |name: &str, value: String| (!value.is_empty()).then(|| (name.to_owned(), value));
    let size = if facts.page_size[0] > 0.0 && facts.page_size[1] > 0.0 { page_size_text(facts.page_size) } else { String::new() };
    [
        known("file", facts.path.clone()),
        (facts.size > 0).then(|| ("file size".to_owned(), human_size(facts.size))),
        Some(("pages".to_owned(), facts.pages.to_string())),
        known(&format!("page {} size", facts.page), size),
        known("PDF version", facts.version.clone()),
        text("title", &facts.title),
        text("author", &facts.author),
        text("subject", &facts.subject),
        text("keywords", &facts.keywords),
        text("creator", &facts.creator),
        text("producer", &facts.producer),
        date("created", &facts.created),
        date("modified", &facts.modified),
        Some(("encrypted".to_owned(), yes_no(facts.encrypted))),
        Some(("has a form".to_owned(), yes_no(facts.form))),
        Some(("has an outline".to_owned(), yes_no(facts.outline))),
        (facts.signatures > 0).then(|| ("digital signatures".to_owned(), facts.signatures.to_string())),
        Some(("tagged".to_owned(), yes_no(facts.tagged))),
    ]
    .into_iter()
    .flatten()
    .collect()
}

/// The creation and modification dates, and the count of signatures.
pub type FileFacts = ((Option<String>, Option<String>), usize);

/// The `/CreationDate` and `/ModDate` of the Info dictionary, as the PDF has them. Read only.
fn info_dates(bytes: Arc<Vec<u8>>, password: Option<&str>) -> (Option<String>, Option<String>) {
    let read = || {
        let doc = Document::open_with_password(bytes, password).ok()?;
        let info = doc.dict(doc.trailer().get(b"Info")?)?;
        let date = |key: &[u8]| match &*doc.resolve(info.get(key)?) {
            Object::String(s) => Some(s.to_text()),
            _ => None,
        };
        Some((date(b"CreationDate"), date(b"ModDate")))
    };
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(read)).ok().flatten().unwrap_or_default()
}

impl App {
    pub fn open_info(&mut self) {
        let doc = &self.doc;
        let page = self.view.current_page(&doc.layout);
        let info = &doc.info;
        // Both read the whole file again, so they are kept for as long as this document is the one open.
        let cached = self.info_extra.as_ref().filter(|(bytes, _)| Arc::ptr_eq(bytes, &doc.bytes));
        let ((created, modified), signatures) = match cached {
            Some((_, extra)) => extra.clone(),
            None => {
                let extra = (info_dates(doc.bytes.clone(), doc.password()), form::signature_count(doc.bytes.clone(), doc.password()));
                self.info_extra = Some((doc.bytes.clone(), extra.clone()));
                extra
            }
        };
        let facts = Facts {
            path: Path::new(&doc.path).display().to_string(),
            size: doc.bytes.len() as u64,
            pages: info.pages.len(),
            page: page + 1,
            page_size: info.pages.get(page).map_or([0.0, 0.0], |p| [p.width, p.height]),
            version: info.pdf_version.clone(),
            title: info.title.clone(),
            author: info.author.clone(),
            subject: info.subject.clone(),
            keywords: info.keywords.clone(),
            creator: info.creator.clone(),
            producer: info.producer.clone(),
            created,
            modified,
            encrypted: info.encrypted,
            form: !info.fields.is_empty(),
            outline: !info.outline.is_empty(),
            signatures,
            tagged: info.tagged,
        };
        self.mode = Mode::Info(InfoState { rows: rows(&facts), first: 0, wheel_rest: 0.0 });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dates_are_read_with_what_the_pdf_gives() {
        assert_eq!(pdf_date("D:20240131120000+01'00'"), "2024-01-31 12:00:00 +01:00");
        assert_eq!(pdf_date("D:20240131120000Z"), "2024-01-31 12:00:00 UTC");
        assert_eq!(pdf_date("D:20240131120000-0500"), "2024-01-31 12:00:00 -05:00");
        assert_eq!(pdf_date("D:20240131"), "2024-01-31");
        assert_eq!(pdf_date("D:202401"), "2024-01-01");
        assert_eq!(pdf_date("D:2024"), "2024-01-01");
        assert_eq!(pdf_date("20240131120000"), "2024-01-31 12:00:00");
        assert_eq!(pdf_date("D:20240131120000+01"), "2024-01-31 12:00:00 +01:00");
    }

    #[test]
    fn text_that_is_no_date_is_kept() {
        assert_eq!(pdf_date("yesterday"), "yesterday");
        assert_eq!(pdf_date("D:20241345"), "D:20241345");
        assert_eq!(pdf_date("D:2024013"), "D:2024013");
        assert_eq!(pdf_date("  "), "");
    }

    #[test]
    fn sizes_are_human() {
        assert_eq!(human_size(0), "0 bytes");
        assert_eq!(human_size(1023), "1023 bytes");
        assert_eq!(human_size(1024), "1.0 KiB (1024 bytes)");
        assert_eq!(human_size(1_572_864), "1.5 MiB (1572864 bytes)");
        assert_eq!(human_size(3 * 1024 * 1024 * 1024), "3.0 GiB (3221225472 bytes)");
    }

    #[test]
    fn papers_match_in_either_orientation() {
        assert_eq!(paper_name([595.0, 842.0]), Some("A4"));
        assert_eq!(paper_name([841.89, 595.28]), Some("A4"));
        assert_eq!(paper_name([612.0, 792.0]), Some("Letter"));
        assert_eq!(paper_name([612.0, 1008.0]), Some("Legal"));
        assert_eq!(paper_name([500.0, 700.0]), None);
        assert_eq!(page_size_text([595.28, 841.89]), "595 \u{d7} 842 pt (210 \u{d7} 297 mm, A4)");
        assert_eq!(page_size_text([100.0, 200.0]), "100 \u{d7} 200 pt (35 \u{d7} 71 mm)");
    }

    #[test]
    fn rows_leave_out_what_is_empty() {
        let facts = Facts {
            path: "/a/b.pdf".into(),
            size: 2048,
            pages: 3,
            page: 2,
            page_size: [612.0, 792.0],
            version: "1.7".into(),
            title: Some("  Title ".into()),
            author: Some(String::new()),
            created: Some("D:20240131120000Z".into()),
            ..Facts::default()
        };
        let rows = rows(&facts);
        let names: Vec<&str> = rows.iter().map(|(n, _)| n.as_str()).collect();
        assert_eq!(
            names,
            ["file", "file size", "pages", "page 2 size", "PDF version", "title", "created", "encrypted", "has a form", "has an outline", "tagged"]
        );
        assert_eq!(rows[5].1, "Title");
        assert_eq!(rows[6].1, "2024-01-31 12:00:00 UTC");
        assert!(rows.iter().all(|(n, _)| n != "digital signatures"));
        let signed = Facts { signatures: 2, ..facts };
        assert!(super::rows(&signed).contains(&("digital signatures".to_owned(), "2".to_owned())));
    }

    #[test]
    fn rows_without_a_page_size_or_file_size_skip_them() {
        let names: Vec<String> = rows(&Facts::default()).into_iter().map(|(n, _)| n).collect();
        assert!(!names.iter().any(|n| n.starts_with("page ") || n == "file size" || n == "file"));
    }
}
