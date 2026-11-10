//! A table of contents for a document with no outline, read from its printed contents pages when
//! their entries are links. A word processor's export often has those and no bookmarks.
use std::collections::BTreeMap;

use pdfcraft_render::PageText;

use crate::doc::TocEntry;

/// The fewest entries that make a table of contents, and the fewest on any one of its pages.
const MIN_ENTRIES: usize = 5;
const MIN_ON_PAGE: usize = 3;
/// Left edges this close, in points, are the same indent.
const ALIGNED: f32 = 2.0;
/// A contents page has few indents, where links in running text start anywhere.
const MAX_INDENTS: usize = 4;
/// The share of a page's links that must fit, so that one stray link doesn't spoil a page.
const MOSTLY: f32 = 0.8;

/// A link to a place in the document.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Row {
    pub page: usize,
    /// In page points, y down.
    pub rect: [f32; 4],
    pub target: (usize, f32),
    /// The x of the destination on its page, when it names one.
    pub target_x: Option<f32>,
    /// The bounds of a `FitR` destination.
    pub target_rect: Option<[f32; 4]>,
    /// Whether the destination names a point on the page, not just the page.
    pub point: bool,
}

/// The left edges in use, most used first.
fn indents(rows: &[Row]) -> Vec<(f32, usize)> {
    let mut found: Vec<(f32, usize)> = Vec::new();
    for row in rows {
        match found.iter_mut().find(|(left, _)| (*left - row.rect[0]).abs() <= ALIGNED) {
            Some((_, count)) => *count += 1,
            None => found.push((row.rect[0], 1)),
        }
    }
    found.sort_by_key(|f| std::cmp::Reverse(f.1));
    found
}

fn mostly(part: usize, whole: usize) -> bool {
    part as f32 >= whole as f32 * MOSTLY
}

/// The links of a page that form a list of entries, top to bottom, or nothing if they don't.
/// Citations fail this: they start anywhere, share lines, and don't move through the document.
fn listing(mut rows: Vec<Row>) -> Vec<Row> {
    rows.sort_by(|a, b| a.rect[1].total_cmp(&b.rect[1]).then(a.rect[0].total_cmp(&b.rect[0])));
    let lefts: Vec<f32> = indents(&rows).into_iter().take(MAX_INDENTS).map(|(left, _)| left).collect();
    let total = rows.len();
    rows.retain(|r| lefts.iter().any(|left| (left - r.rect[0]).abs() <= ALIGNED));
    let one_per_line = rows.windows(2).all(|w| w[1].rect[1] - w[0].rect[1] > ALIGNED);
    let forward = rows.windows(2).filter(|w| w[0].target <= w[1].target).count();
    let ahead = rows.iter().all(|r| r.target.0 >= r.page);
    let fits = rows.len() >= MIN_ON_PAGE && mostly(rows.len(), total) && one_per_line && ahead && mostly(forward, rows.len().saturating_sub(1));
    if fits { rows } else { Vec::new() }
}

/// The links of the document's contents pages: the first run of pages that list entries.
pub fn contents_rows(links: Vec<Row>) -> Vec<Row> {
    let mut pages: BTreeMap<usize, Vec<Row>> = BTreeMap::new();
    for link in links {
        pages.entry(link.page).or_default().push(link);
    }
    let mut run: Vec<Row> = Vec::new();
    let mut last = None;
    for (page, rows) in pages {
        let rows = listing(rows);
        if rows.is_empty() || last.is_some_and(|l: usize| l + 1 != page) {
            if run.len() >= MIN_ENTRIES {
                break;
            }
            run.clear();
            last = None;
        }
        if !rows.is_empty() {
            run.extend(rows);
            last = Some(page);
        }
    }
    if run.len() >= MIN_ENTRIES { run } else { Vec::new() }
}

/// The text under a link.
fn text_in(text: &PageText, rect: [f32; 4]) -> String {
    let mut out = String::new();
    let mut line = None;
    for (index, glyph) in text.glyphs.iter().enumerate() {
        let (x, y) = ((glyph.rect[0] + glyph.rect[2]) / 2.0, (glyph.rect[1] + glyph.rect[3]) / 2.0);
        if x < rect[0] || x > rect[2] || y < rect[1] || y > rect[3] {
            continue;
        }
        let this = text.line_of.get(index).copied();
        let gap = text.space_before.get(index).copied().unwrap_or(false) || (line.is_some() && line != Some(this));
        if gap && !out.is_empty() {
            out.push(' ');
        }
        out.push_str(&glyph.text);
        line = Some(this);
    }
    out
}

/// A title without the dot leaders and page number a printed entry ends with.
fn clean_title(title: &str) -> String {
    let title = title.split_whitespace().collect::<Vec<_>>().join(" ");
    let number = title.trim_end_matches(|c: char| c.is_ascii_digit() || c == ' ');
    let bare = number.trim_end_matches(['.', '…', ' ']);
    if number.len() - bare.len() >= 3 { bare.to_owned() } else { title }
}

/// The table of contents those rows make, given each page's text. An entry that wraps is two links
/// to one place, and an indent is a level.
pub fn entries(rows: &[Row], text_of: impl Fn(usize) -> Option<std::sync::Arc<PageText>>) -> Vec<TocEntry> {
    let mut lefts: Vec<f32> = indents(rows).into_iter().map(|(left, _)| left).collect();
    lefts.sort_by(f32::total_cmp);
    let mut out: Vec<TocEntry> = Vec::new();
    let mut previous = None;
    for row in rows {
        let Some(text) = text_of(row.page) else { continue };
        let title = text_in(&text, row.rect);
        if title.trim().is_empty() {
            continue;
        }
        let wrapped = previous == Some(row.target);
        previous = Some(row.target);
        match out.last_mut().filter(|_| wrapped) {
            Some(entry) => {
                entry.title.push(' ');
                entry.title.push_str(&title);
            }
            None => {
                let depth = lefts.iter().position(|left| (left - row.rect[0]).abs() <= ALIGNED).unwrap_or(0);
                out.push(TocEntry { title, depth, page: row.target.0, x: row.target_x, y: row.target.1, rect: row.target_rect, point: row.point });
            }
        }
    }
    for entry in &mut out {
        entry.title = clean_title(&entry.title);
    }
    out
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use pdfcraft_render::TextGlyph;

    use super::*;

    fn row(page: usize, left: f32, line: usize, target: usize) -> Row {
        let top = 100.0 + line as f32 * 15.0;
        Row { page, rect: [left, top, left + 300.0, top + 12.0], target: (target, 0.0), target_x: None, target_rect: None, point: true }
    }

    /// Chapters at 72 and sections at 90, as a word processor's export has them.
    fn listing_page(page: usize, first_target: usize) -> Vec<Row> {
        (0..6).map(|i| row(page, if i % 3 == 0 { 72.0 } else { 90.0 }, i, first_target + i)).collect()
    }

    fn page_text(lines: &[(f32, &str)]) -> Arc<PageText> {
        let mut text = PageText { glyphs: Vec::new(), line_of: Vec::new(), space_before: Vec::new(), truncated: false };
        for (number, (left, line)) in lines.iter().enumerate() {
            let top = 100.0 + number as f32 * 15.0;
            let mut space = false;
            for (i, c) in line.chars().enumerate() {
                if c == ' ' {
                    space = true;
                    continue;
                }
                let x = left + i as f32 * 6.0;
                let rect = [x, top, x + 6.0, top + 12.0];
                let quad = [[x, top], [x + 6.0, top], [x + 6.0, top + 12.0], [x, top + 12.0]];
                text.glyphs.push(TextGlyph { text: c.to_string(), rect, quad, direction: [1.0, 0.0] });
                text.line_of.push(number as u32);
                text.space_before.push(space);
                space = false;
            }
        }
        Arc::new(text)
    }

    #[test]
    fn contents_pages_are_found_across_pages() {
        let mut links = listing_page(1, 10);
        links.extend(listing_page(2, 20));
        assert_eq!(contents_rows(links).len(), 12);
    }

    #[test]
    fn citations_in_running_text_are_not_contents() {
        let lefts = [72.0, 131.0, 244.0, 310.0, 402.0, 188.0, 96.0, 355.0];
        let links = lefts.iter().enumerate().map(|(i, left)| row(4, *left, i, 200)).collect();
        assert_eq!(contents_rows(links), vec![]);
    }

    #[test]
    fn links_back_to_earlier_pages_are_not_contents() {
        let links = (0..8).map(|i| row(50, 72.0, i, 3 + i)).collect();
        assert_eq!(contents_rows(links), vec![]);
    }

    #[test]
    fn links_sharing_lines_are_not_contents() {
        let links = (0..8).map(|i| row(2, 72.0, i / 2, 10 + i)).collect();
        assert_eq!(contents_rows(links), vec![]);
    }

    #[test]
    fn links_that_jump_about_are_not_contents() {
        let targets = [90, 12, 70, 5, 66, 9, 80, 3];
        let links = targets.iter().enumerate().map(|(i, target)| row(1, 72.0, i, *target)).collect();
        assert_eq!(contents_rows(links), vec![]);
    }

    #[test]
    fn too_few_links_are_not_contents() {
        let links = (0..4).map(|i| row(1, 72.0, i, 10 + i)).collect();
        assert_eq!(contents_rows(links), vec![]);
    }

    #[test]
    fn a_later_list_does_not_join_the_contents() {
        let mut links = listing_page(1, 10);
        links.extend(listing_page(90, 95));
        assert!(contents_rows(links).iter().all(|r| r.page == 1));
    }

    #[test]
    fn a_stray_link_does_not_spoil_a_page() {
        let mut links = listing_page(1, 10);
        links.push(Row { page: 1, rect: [289.0, 20.0, 337.0, 32.0], target: (10, 0.0), target_x: None, target_rect: None, point: true });
        assert!(contents_rows(links).len() >= 6);
    }

    #[test]
    fn entries_read_their_titles_and_levels() {
        let text = page_text(&[(72.0, "Preface"), (90.0, "1.0 | Summary"), (90.0, "1.1 | Heritability")]);
        let rows = [row(0, 72.0, 0, 5), row(0, 90.0, 1, 6), row(0, 90.0, 2, 7)];
        let found = entries(&rows, |_| Some(text.clone()));
        let shape: Vec<(&str, usize, usize)> = found.iter().map(|e| (e.title.as_str(), e.depth, e.page)).collect();
        assert_eq!(shape, [("Preface", 0, 5), ("1.0 | Summary", 1, 6), ("1.1 | Heritability", 1, 7)]);
    }

    #[test]
    fn an_entry_that_wraps_is_one_entry() {
        let text = page_text(&[(72.0, "A very long"), (72.0, "title indeed"), (72.0, "Next")]);
        let rows = [row(0, 72.0, 0, 5), row(0, 72.0, 1, 5), row(0, 72.0, 2, 9)];
        let titles: Vec<String> = entries(&rows, |_| Some(text.clone())).into_iter().map(|e| e.title).collect();
        assert_eq!(titles, ["A very long title indeed", "Next"]);
    }

    #[test]
    fn a_link_over_nothing_is_no_entry() {
        let text = page_text(&[(72.0, "Preface")]);
        let rows = [row(0, 72.0, 0, 5), row(0, 72.0, 7, 9)];
        assert_eq!(entries(&rows, |_| Some(text.clone())).len(), 1);
        assert_eq!(entries(&rows, |_| None), vec![]);
    }

    #[test]
    fn leaders_and_their_page_number_go() {
        assert_eq!(clean_title("3.1 Methods ........ 12"), "3.1 Methods");
        assert_eq!(clean_title("Methods  . . . . 12"), "Methods");
        assert_eq!(clean_title("Results…… 7"), "Results");
    }

    #[test]
    fn numbers_that_belong_to_the_title_stay() {
        assert_eq!(clean_title("Chapter 12"), "Chapter 12");
        assert_eq!(clean_title("5.10 | Rare variants"), "5.10 | Rare variants");
        assert_eq!(clean_title("Version 2.0"), "Version 2.0");
    }

    #[test]
    fn hostile_geometry_does_not_panic() {
        let bad = Row {
            page: usize::MAX,
            rect: [f32::NAN, f32::INFINITY, f32::NAN, f32::NEG_INFINITY],
            target: (usize::MAX, f32::NAN),
            target_x: None,
            target_rect: None,
            point: true,
        };
        assert_eq!(contents_rows(vec![bad; 9]), vec![]);
        assert_eq!(entries(&[bad], |_| Some(page_text(&[(72.0, "x")]))), vec![]);
    }
}
