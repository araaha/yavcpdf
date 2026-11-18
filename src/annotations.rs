//! The list of your annotations, a picker like the bookmarks, and their highlights and notes as Markdown.
use egui::{CursorIcon, Pos2, Response};

use crate::app::{App, Mode};
use crate::bookmarks::{BookmarkState, Outcome, picker_step, settle};
use crate::form::{clamp_selected, keep_visible, wheel_step};
use crate::keymap::Stroke;
use crate::reflow::{Row, reflow};
use crate::store::Annotation;
use crate::view::Anchor;

/// How far from a highlight a note may sit, in points, and still go under it in the Markdown.
const NEAR: f32 = 12.0;

/// One annotation as a row of the picker, with what the Markdown needs.
#[derive(Clone, Debug, PartialEq)]
pub struct Item {
    pub id: u64,
    pub page: usize,
    pub kind: &'static str,
    pub color: Option<[u8; 3]>,
    /// The text it carries: the highlighted text with its lines joined, a note's text, typed text. Empty for the others.
    pub text: String,
    /// Where it is on the page, in points.
    pub bounds: [f32; 4],
    /// The boxes it covers: one per line for a highlight, else its bounds.
    rects: Vec<[f32; 4]>,
}

impl Item {
    /// The row as shown: the kind and the text on one line.
    pub fn line(&self) -> String {
        let text = self.text.split_whitespace().collect::<Vec<_>>().join(" ");
        if text.is_empty() { self.kind.to_owned() } else { format!("{}  {text}", self.kind) }
    }
}

/// A highlight's text with its lines joined and its hyphenation undone, as copying does. The lines of the text are those of the
/// rectangles when they agree; else they are taken to be evenly stacked.
pub fn highlight_text(rects: &[[f32; 4]], text: &str) -> String {
    let lines: Vec<&str> = text.split('\n').collect();
    let rows: Vec<Row> = lines
        .iter()
        .enumerate()
        .map(|(i, line)| {
            let [left, top, right, bottom] =
                if lines.len() == rects.len() { rects[i] } else { [0.0, i as f32 * 10.0, 100.0, i as f32 * 10.0 + 10.0] };
            Row { text: (*line).to_owned(), left, right, top, bottom, new_page: i == 0, rotated: false }
        })
        .collect();
    reflow(&rows).trim().to_owned()
}

fn item(annotation: &Annotation) -> Item {
    let (kind, color, text) = match annotation {
        Annotation::Highlight { rects, color, text, .. } => ("highlight", Some(*color), highlight_text(rects, text)),
        Annotation::Stroke { color, .. } => ("ink", Some(*color), String::new()),
        Annotation::Note { color, text, .. } => ("note", Some(*color), text.trim().to_owned()),
        Annotation::Text { text, .. } => ("text", None, text.trim().to_owned()),
        Annotation::Mark { .. } => ("mark", None, String::new()),
        Annotation::Signature { .. } => ("signature", None, String::new()),
    };
    let bounds = annotation.bounds();
    let rects = match annotation {
        Annotation::Highlight { rects, .. } => rects.clone(),
        _ => vec![bounds],
    };
    Item { id: annotation.id(), page: annotation.page(), kind, color, text, bounds, rects }
}

/// The annotations as rows, in the order of the document: by page, then top to bottom, then left to right.
pub fn items(annotations: &[Annotation]) -> Vec<Item> {
    let mut items: Vec<Item> = annotations.iter().map(item).collect();
    items.sort_by(|a, b| a.page.cmp(&b.page).then(a.bounds[1].total_cmp(&b.bounds[1])).then(a.bounds[0].total_cmp(&b.bounds[0])));
    items
}

/// The rows (as indices) whose text contains `filter`, ignoring case.
pub fn rows(items: &[Item], filter: &str) -> Vec<usize> {
    let filter = filter.to_lowercase();
    items.iter().enumerate().filter(|(_, i)| i.text.to_lowercase().contains(&filter)).map(|(i, _)| i).collect()
}

fn near(note: &Item, highlight: &Item) -> bool {
    let n = note.bounds;
    highlight.rects.iter().any(|r| n[0] <= r[2] + NEAR && n[2] >= r[0] - NEAR && n[1] <= r[3] + NEAR && n[3] >= r[1] - NEAR)
}

/// A quote block: every line of `text` after a `>`.
fn quote(text: &str) -> String {
    text.lines().map(|l| if l.is_empty() { ">".to_owned() } else { format!("> {l}") }).collect::<Vec<_>>().join("\n")
}

/// The highlights and notes as Markdown under `title`, with a heading for each page by its label (`labels[page]`). A note on or next
/// to a highlight comes under it. `None` when there is nothing to write.
pub fn markdown(title: &str, items: &[Item], labels: &[String]) -> Option<String> {
    let used = |i: &&Item| matches!(i.kind, "highlight" | "note") && !i.text.is_empty();
    let items: Vec<&Item> = items.iter().filter(used).collect();
    if items.is_empty() {
        return None;
    }
    // Each note goes under the first highlight it is near, if any.
    let owner = |n: usize| items.iter().position(|h| h.kind == "highlight" && h.page == items[n].page && near(items[n], h));
    let owners: Vec<Option<usize>> = (0..items.len()).map(|n| if items[n].kind == "note" { owner(n) } else { None }).collect();
    let title = title.split_whitespace().collect::<Vec<_>>().join(" ");
    let mut blocks = vec![format!("# {title}")];
    let mut page = None;
    for (n, item) in items.iter().enumerate() {
        if owners[n].is_some() {
            continue;
        }
        if page != Some(item.page) {
            page = Some(item.page);
            blocks.push(format!("## Page {}", labels.get(item.page).cloned().unwrap_or_else(|| (item.page + 1).to_string())));
        }
        if item.kind == "highlight" {
            blocks.push(quote(&item.text));
            blocks.extend((0..items.len()).filter(|m| owners[*m] == Some(n)).map(|m| items[m].text.clone()));
        } else {
            blocks.push(item.text.clone());
        }
    }
    Some(blocks.join("\n\n") + "\n")
}

impl App {
    /// Opens the picker, with the highlight on the last annotation at or before the view.
    pub fn open_annotations(&mut self) {
        let items = items(&self.side.annotations);
        if items.is_empty() {
            self.say("no annotations");
            return;
        }
        let page = self.view.current_page(&self.doc.layout);
        let selected = items.iter().rposition(|i| i.page <= page).unwrap_or(0);
        let mut state = BookmarkState { selected, ..Default::default() };
        state.first = keep_visible(0, selected, self.annotation_layout(&state).fits, items.len());
        self.mode = Mode::Annotations(state);
    }

    fn annotation_rows(&self, state: &BookmarkState) -> Vec<usize> {
        rows(&items(&self.side.annotations), &state.filter.text)
    }

    pub fn annotation_layout(&self, state: &BookmarkState) -> crate::chrome::ListLayout {
        let filter = state.filtering || !state.filter.is_empty();
        self.list_layout(false, filter, self.annotation_rows(state).len(), 0, Some(state.first))
    }

    fn annotation_go(&mut self, index: usize) {
        self.mode = Mode::Normal;
        if let Some(item) = items(&self.side.annotations).get(index) {
            self.jump_to_point(Anchor { page: item.page, y: item.bounds[1], x: self.view.x });
        }
    }

    pub fn annotation_key(&mut self, stroke: &Stroke) {
        let Mode::Annotations(state) = &self.mode else { return };
        let mut state = state.clone();
        let list = items(&self.side.annotations);
        match picker_step(&|filter| rows(&list, filter), &mut state, stroke, &self.keymap) {
            Outcome::Close => {
                self.mode = Mode::Normal;
                return;
            }
            Outcome::Jump(row) => {
                self.annotation_go(row);
                return;
            }
            Outcome::Delete(row) => {
                if let Some(item) = list.get(row) {
                    self.remove_annotation(item.id);
                    self.say(format!("{} deleted", item.kind));
                    if self.side.annotations.is_empty() {
                        self.mode = Mode::Normal;
                        return;
                    }
                }
                settle(&self.annotation_rows(&state), &mut state);
            }
            Outcome::None => {}
        }
        let rows = self.annotation_rows(&state);
        let at = rows.iter().position(|r| *r == state.selected).unwrap_or(0);
        state.first = keep_visible(state.first, at, self.annotation_layout(&state).fits, rows.len());
        self.mode = Mode::Annotations(state);
    }

    /// The wheel over the open panel scrolls its list; elsewhere it does nothing.
    pub fn wheel_annotations(&mut self, pixels_y: f32, at: Option<Pos2>) {
        let Mode::Annotations(state) = &self.mode else { return };
        let layout = self.annotation_layout(state);
        if !at.is_some_and(|p| layout.rect.contains(p)) {
            return;
        }
        let rows = self.annotation_rows(state);
        if let Mode::Annotations(state) = &mut self.mode {
            let before = layout.first;
            (state.first, state.wheel_rest) = wheel_step(before, state.wheel_rest, pixels_y, layout.row_height, layout.fits, rows.len());
            if state.first != before {
                let at = rows.iter().position(|r| *r >= state.selected).unwrap_or(0);
                let at = clamp_selected(at, state.first, layout.fits, rows.len());
                state.selected = rows.get(at).copied().unwrap_or(state.selected);
            }
        }
    }

    /// A click on a row jumps to it; one outside the panel closes it. The page gets no clicks meanwhile.
    pub fn pointer_annotations(&mut self, ctx: &egui::Context, response: &Response) {
        ctx.set_cursor_icon(CursorIcon::Default);
        let Some(at) = response.interact_pointer_pos().filter(|_| response.clicked()) else { return };
        let Mode::Annotations(state) = &self.mode else { return };
        let rows = self.annotation_rows(state);
        let layout = self.annotation_layout(state);
        if !layout.rect.contains(at) {
            self.mode = Mode::Normal;
            self.choice_closed = Some((ctx.input(|i| i.time), at));
            return;
        }
        let hit = (layout.first..rows.len()).take(layout.fits).find(|&i| Self::list_row_rect(&layout, i).contains(at));
        if let Some(i) = hit {
            self.annotation_go(rows[i]);
            // Jumped by a click: its double click must not land on the page.
            self.choice_closed = Some((ctx.input(|i| i.time), at));
        }
    }

    /// Writes the highlights and notes to a Markdown file, by default beside the document.
    pub fn command_export_notes(&mut self, argument: &str, force: bool) {
        if self.blank {
            return self.say("no document open");
        }
        let path = match argument {
            "" => crate::export::notes_path(&self.doc.path),
            given => crate::config::expand_home(given),
        };
        if let Some(refusal) = crate::export::refusal(&self.doc.path, &path, force, "export-notes") {
            self.say(refusal);
            return;
        }
        // A highlight keeps its text, so no page's text has to be loaded yet.
        let labels: Vec<String> = self.doc.info.pages.iter().map(|p| p.label.clone()).collect();
        let name = self.doc.path.file_name().map_or_else(|| "document".to_owned(), |n| n.to_string_lossy().into_owned());
        let title = self.doc.info.title.as_deref().map(str::trim).filter(|t| !t.is_empty()).map_or(name, str::to_owned);
        let Some(text) = markdown(&title, &items(&self.side.annotations), &labels) else {
            return self.say("no highlights or notes to export");
        };
        self.say(match crate::export::write_atomic(&path, text.as_bytes(), force, None) {
            Ok(()) => format!("wrote the notes to {}", path.display()),
            Err(e) => format!("export-notes failed: {e}"),
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn highlight(id: u64, page: usize, top: f32, text: &str) -> Annotation {
        let rects = text.split('\n').enumerate().map(|(i, _)| [72.0, top + i as f32 * 14.0, 300.0, top + 12.0 + i as f32 * 14.0]).collect();
        Annotation::Highlight { id, page, rects, color: [250, 189, 47], text: text.to_owned() }
    }

    fn note(id: u64, page: usize, at: [f32; 2], text: &str) -> Annotation {
        Annotation::Note { id, page, at, color: [1, 2, 3], text: text.to_owned() }
    }

    fn labels() -> Vec<String> {
        ["i", "ii", "1"].map(String::from).to_vec()
    }

    #[test]
    fn rows_come_in_document_order_with_their_kind_and_text() {
        let list = vec![
            note(1, 1, [10.0, 10.0], "later"),
            Annotation::Stroke { id: 2, page: 0, points: vec![[1.0, 300.0]], color: [0; 3], width: 2.0 },
            highlight(3, 0, 100.0, "first"),
            Annotation::Text { id: 4, page: 0, at: [5.0, 50.0], size: 12.0, text: "Ada".to_owned() },
        ];
        let got: Vec<_> = items(&list).iter().map(|i| (i.id, i.kind, i.text.clone())).collect();
        assert_eq!(
            got,
            [(4, "text", "Ada".to_owned()), (3, "highlight", "first".to_owned()), (2, "ink", String::new()), (1, "note", "later".to_owned())]
        );
        assert_eq!(items(&list)[2].line(), "ink");
        assert_eq!(items(&list)[0].color, None);
    }

    #[test]
    fn a_highlight_has_its_lines_joined_and_its_hyphenation_undone() {
        let list = [highlight(1, 0, 100.0, "an exam\u{ad}\nple of text")];
        assert_eq!(items(&list)[0].text, "an example of text");
        let plain = [highlight(1, 0, 100.0, "one\ntwo")];
        assert_eq!(items(&plain)[0].line(), "highlight  one two");
        // Rectangles that do not agree with the lines still give running text.
        assert_eq!(highlight_text(&[[0.0, 0.0, 10.0, 10.0]], "one\ntwo"), "one two");
    }

    #[test]
    fn the_filter_matches_the_text_without_regard_to_case() {
        let list = [
            highlight(1, 0, 10.0, "Alpha"),
            note(2, 0, [0.0, 50.0], "beta"),
            Annotation::Mark { id: 3, page: 0, at: [0.0, 90.0], size: 9.0, mark: crate::store::MarkKind::Dot },
        ];
        let all = items(&list);
        assert_eq!(rows(&all, ""), [0, 1, 2]);
        assert_eq!(rows(&all, "ALP"), [0]);
        assert_eq!(rows(&all, "zz"), Vec::<usize>::new());
    }

    #[test]
    fn markdown_has_headings_quotes_and_notes_and_leaves_the_rest_out() {
        let list = vec![
            highlight(1, 0, 100.0, "quoted\nwords"),
            Annotation::Stroke { id: 2, page: 0, points: vec![[1.0, 1.0]], color: [0; 3], width: 2.0 },
            note(3, 0, [400.0, 500.0], "far note"),
            Annotation::Text { id: 4, page: 0, at: [5.0, 5.0], size: 12.0, text: "signed".to_owned() },
            highlight(5, 2, 20.0, "on the third"),
        ];
        let md = markdown("Title", &items(&list), &labels()).unwrap();
        assert_eq!(md, "# Title\n\n## Page i\n\n> quoted words\n\nfar note\n\n## Page 1\n\n> on the third\n");
    }

    #[test]
    fn a_note_on_or_next_to_a_highlight_goes_under_it() {
        // On it, and just right of its end: both under the highlight, though the first sorts before it.
        let list = vec![
            note(1, 0, [80.0, 90.0], "above"),
            highlight(2, 0, 100.0, "quote"),
            note(3, 0, [310.0, 100.0], "beside"),
            note(4, 0, [500.0, 100.0], "away"),
        ];
        let md = markdown("T", &items(&list), &labels()).unwrap();
        assert_eq!(md, "# T\n\n## Page i\n\n> quote\n\nabove\n\nbeside\n\naway\n");
        // A note on another page is not under a highlight there.
        let other = vec![highlight(1, 0, 100.0, "quote"), note(2, 1, [80.0, 100.0], "elsewhere")];
        assert!(markdown("T", &items(&other), &labels()).unwrap().contains("> quote\n\n## Page ii\n\nelsewhere"));
    }

    #[test]
    fn a_title_with_line_breaks_stays_one_heading() {
        let list = vec![highlight(1, 0, 100.0, "quote")];
        let md = markdown("  Two\nlines \r\n  and\tgaps ", &items(&list), &labels()).unwrap();
        assert!(md.starts_with("# Two lines and gaps\n\n"), "{md}");
    }

    #[test]
    fn nothing_to_write_without_highlights_or_notes() {
        assert_eq!(markdown("T", &[], &labels()), None);
        let list = [Annotation::Stroke { id: 1, page: 0, points: vec![[1.0, 1.0]], color: [0; 3], width: 2.0 }, note(2, 0, [0.0, 0.0], "  ")];
        assert_eq!(markdown("T", &items(&list), &labels()), None);
    }

    #[test]
    fn quotes_keep_paragraph_breaks_inside_the_block() {
        assert_eq!(quote("a\n\nb"), "> a\n>\n> b");
    }
}
