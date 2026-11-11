//! What is drawn around the document: status bar, search bar, hotbar and table of contents.
use std::sync::Arc;

use egui::{Align, Align2, Color32, FontData, FontDefinitions, FontFamily, FontId, Painter, Pos2, Rect, Sense, Vec2, pos2, vec2};

use crate::app::{App, Bar, BarKind, ChoiceState, EntryKind, FillItem, Mode, PasswordState, SignPad, TocState, Tool};
use crate::bookmarks::BookmarkState;
use crate::config::{Config, Side};
use crate::field::Field;
use crate::hint::Purpose;
use crate::keymap::Action;
use crate::menu::Menu;
use crate::recent::RecentState;
use crate::view::{Layout, View, unrotate_point};

const PADDING: f32 = 8.0;

/// A row of a list panel: how far its text is indented, the text, and the mark at the right.
struct ListRow {
    indent: f32,
    text: String,
    mark: String,
    /// A tick drawn in the mark's place.
    ticked: bool,
}

/// What a list panel shows: the optional header and filter rows, `len` rows with the highlight
/// on `at`, and the first row shown when the caller fixes it (else `at` is kept in the middle).
#[derive(Clone, Copy)]
pub struct ListView<'a> {
    pub header: Option<&'a str>,
    pub filter: Option<&'a Field>,
    pub len: usize,
    pub at: usize,
    pub first: Option<usize>,
}

/// Where a list panel puts things.
pub struct ListLayout {
    pub rect: Rect,
    /// Where the first row's box starts, below the header and filter rows.
    pub top: f32,
    pub row_height: f32,
    /// The first row shown, and how many fit.
    pub first: usize,
    pub fits: usize,
}

pub fn bar_height(config: &Config) -> f32 {
    (config.font_size * 1.7).round()
}

fn font(config: &Config) -> FontId {
    FontId::proportional(config.font_size)
}

/// How far above the baseline the middle of the capitals is, as the font draws them.
fn cap_middle(painter: &Painter, font: FontId) -> f32 {
    let fallback = font.size * 0.35;
    let galley = painter.layout_no_wrap("H".to_owned(), font, Color32::WHITE);
    let glyph = galley.rows.first().and_then(|row| row.glyphs.first());
    glyph.map_or(fallback, |g| -(g.uv_rect.offset.y + g.uv_rect.size.y / 2.0))
}

/// Draws a line of interface text with the middle of its capitals at the height of `at`. Centring
/// its line box instead would depend on the font's metrics, and would move the text whenever a
/// fallback font makes that box taller. Only the horizontal part of `align` counts.
fn text_line(painter: &Painter, config: &Config, at: Pos2, align: Align2, text: impl ToString, color: Color32) -> Rect {
    let galley = painter.layout_no_wrap(text.to_string(), font(config), color);
    let baseline = galley.rows.first().map_or(0.0, |row| row.pos.y + row.glyphs.first().map_or(0.0, |g| g.pos.y));
    let width = galley.size().x;
    let x = match align.x() {
        Align::Min => at.x,
        Align::Center => at.x - width / 2.0,
        Align::Max => at.x - width,
    };
    let rect = Rect::from_min_size(pos2(x, at.y + cap_middle(painter, font(config)) - baseline), galley.size());
    painter.galley(rect.min, galley, color);
    rect
}

/// The file of a fontconfig font.
fn font_file(name: &str) -> Option<Vec<u8>> {
    // Without fontconfig (usual on Windows and macOS) the spawn fails at once and silently.
    let output = std::process::Command::new("fc-match")
        .args(["-f", "%{file}", name])
        .stdin(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .output()
        .ok()?;
    let path = String::from_utf8(output.stdout).ok()?;
    std::fs::read(path.trim()).ok()
}

/// The bytes of the configured font, if there is one and it can be found.
pub fn font_bytes(config: &Config) -> Option<Vec<u8>> {
    if config.font.is_empty() { None } else { font_file(&config.font) }
}

/// Uses the configured font for all interface text.
pub fn apply_font(ctx: &egui::Context, config: &Config) {
    apply_font_bytes(ctx, font_bytes(config));
}

/// Uses the font in `bytes`, or the default one without it, for all interface text.
pub fn apply_font_bytes(ctx: &egui::Context, bytes: Option<Vec<u8>>) {
    let mut fonts = FontDefinitions::default();
    if let Some(bytes) = bytes {
        fonts.font_data.insert("ui".to_owned(), Arc::new(FontData::from_owned(bytes)));
        for family in [FontFamily::Proportional, FontFamily::Monospace] {
            fonts.families.entry(family).or_default().insert(0, "ui".to_owned());
        }
    }
    ctx.set_fonts(fonts);
}

/// Fills in `{fields}`. Sections are separated by ` | `, and one whose fields are all empty goes.
pub fn expand(format: &str, field: impl Fn(&str) -> String) -> String {
    let mut sections = Vec::new();
    for section in format.split(" | ") {
        let mut out = String::new();
        let (mut fields, mut filled) = (0, 0);
        let mut rest = section;
        while let Some(open) = rest.find('{') {
            let Some(close) = rest[open..].find('}') else { break };
            out.push_str(&rest[..open]);
            let value = field(&rest[open + 1..open + close]);
            fields += 1;
            filled += usize::from(!value.is_empty());
            out.push_str(&value);
            rest = &rest[open + close + 1..];
        }
        out.push_str(rest);
        if fields == 0 || filled > 0 {
            sections.push(out);
        }
    }
    sections.join(" | ")
}

/// One button of the hotbar.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Item {
    Tool(Tool, &'static str),
    Fill(FillItem),
    Color(usize),
    Thinner,
    Thicker,
    Undo,
    Redo,
}

impl App {
    fn status_field(&self, name: &str) -> String {
        let layout = &self.doc.layout;
        let page = self.view.current_page(layout);
        match name {
            "page" => (page + 1).to_string(),
            "pages" => layout.len().to_string(),
            "label" => self.doc.info.pages[page].label.clone(),
            "percent" => format!("{:.0}%", (self.view.y / layout.total.max(1.0) * 100.0).clamp(0.0, 100.0)),
            "zoom" => format!("{:.0}%", self.view.zoom * 100.0),
            "chapter" => self.chapter(page),
            "title" => self.doc.info.title.clone().unwrap_or_default(),
            "file" => self.doc.file_name(),
            "mode" => self.mode_name(),
            "pending" => self.pending_text(),
            "search" => self
                .search
                .as_ref()
                .map(|s| format!("/{} {}", s.query, s.status((layout.rotation != 0).then_some(&crate::search::shown_top_left(layout) as _))))
                .unwrap_or_default(),
            "selection" => self.selection_status(),
            "message" => self.message().to_owned(),
            _ => String::new(),
        }
    }

    fn chapter(&self, page: usize) -> String {
        let y = centre_y(&self.doc.layout, &self.view, page);
        self.doc.chapter_at(page, y).map(|e| e.title.clone()).unwrap_or_default()
    }

    fn mode_name(&self) -> String {
        if let Mode::Hint(state) = &self.mode {
            return match (state.purpose, state.start) {
                (Purpose::Select, None) => "HINT: first word".to_owned(),
                (Purpose::Select, Some(_)) => "HINT: last word, or enter for this one".to_owned(),
                (Purpose::Link, _) => "HINT: link".to_owned(),
                (Purpose::DeleteAnnotation, _) => "HINT: delete".to_owned(),
                (Purpose::Smart, _) => "HINT: jump to".to_owned(),
                (Purpose::Overview, _) => "HINT: overview".to_owned(),
                (Purpose::Place, _) => "HINT: place".to_owned(),
                (Purpose::Form, _) => "HINT: form field".to_owned(),
            };
        }
        if let Some(overview) = &self.overview {
            return format!("OVERVIEW: {} ({}/{})", overview.title, overview.index + 1, overview.places.len());
        }
        match self.tool {
            Some(Tool::Highlight) => "HIGHLIGHT".to_owned(),
            Some(Tool::Draw) => "DRAW".to_owned(),
            Some(Tool::Erase) => "ERASE".to_owned(),
            Some(Tool::Note) => "NOTE".to_owned(),
            Some(Tool::Fill) => format!("FILL: {}", self.fill.label()),
            None => String::new(),
        }
    }

    fn pending_text(&self) -> String {
        if let Mode::AwaitMark(_) = self.mode {
            return "mark?".to_owned();
        }
        let keys = self.pending.display();
        if keys.is_empty() && self.pending_portal.is_some() { "portal: go to the target, press p".to_owned() } else { keys }
    }

    fn selection_status(&self) -> String {
        if self.selection.is_none() {
            return String::new();
        }
        format!("{} characters, y copies", self.selection_text().chars().count())
    }

    pub fn draw_chrome(&mut self, ui: &egui::Ui, status: Rect) {
        let painter = ui.painter();
        if self.statusbar_visible {
            self.draw_status(painter, status);
        }
        self.draw_hotbar(ui);
        match &self.mode {
            Mode::Bar(bar) => {
                self.draw_bar(painter, bar);
                if let Some(menu) = bar.menu.as_deref().filter(|m| m.is_shown()) {
                    self.draw_menu(painter, menu);
                }
            }
            Mode::Toc(state) => self.draw_toc(painter, state),
            Mode::Bookmarks(state) => self.draw_bookmarks(painter, state),
            Mode::Recent(state) => self.draw_recent(painter, state),
            Mode::Choice(state) => self.draw_choice(painter, state),
            Mode::Sign(pad) => self.draw_pad(painter, pad),
            Mode::Password(state) => self.draw_password(painter, state),
            _ => {}
        }
    }

    /// The elapsed time of a presentation, small in the corner.
    pub fn draw_timer(&self, painter: &Painter, text: &str) {
        let theme = self.config.active_theme();
        let area = self.view.rect;
        let galley = painter.layout_no_wrap(text.to_owned(), font(&self.config), theme.ui_text.egui());
        let rect =
            Rect::from_min_size(area.right_bottom() - galley.size() - vec2(PADDING, PADDING) * 2.0, galley.size() + vec2(PADDING, PADDING) * 2.0);
        painter.rect_filled(rect, 0.0, theme.ui_background.egui());
        text_line(painter, &self.config, rect.center(), Align2::CENTER_CENTER, text, theme.ui_text.egui());
    }

    fn draw_status(&self, painter: &Painter, rect: Rect) {
        let theme = self.config.active_theme();
        painter.rect_filled(rect, 0.0, theme.ui_background.egui());
        let bar = &self.config.statusbar;
        let (left, right) = (expand(&bar.left, |f| self.status_field(f)), expand(&bar.right, |f| self.status_field(f)));
        let color = theme.ui_text.egui();
        let right_rect = text_line(painter, &self.config, rect.right_center() - vec2(PADDING, 0.0), Align2::RIGHT_CENTER, right, color);
        let room = Rect::from_min_max(rect.min, pos2(right_rect.left() - PADDING, rect.max.y));
        text_line(&painter.with_clip_rect(room), &self.config, rect.left_center() + vec2(PADDING, 0.0), Align2::LEFT_CENTER, left, color);
    }

    /// The bar across the top: search and commands.
    fn draw_bar(&self, painter: &Painter, bar: &Bar) {
        let theme = self.config.active_theme();
        let top = self.view.rect.min;
        let rect = Rect::from_min_size(top, vec2(self.view.rect.width(), bar_height(&self.config)));
        painter.rect_filled(rect, 0.0, theme.ui_background.egui());
        let prompt = match bar.kind {
            BarKind::Search { forward: true } => "/",
            BarKind::Search { forward: false } => "?",
            BarKind::Command => ":",
            BarKind::Entry(entry) if entry.kind == EntryKind::Note => "note: ",
            BarKind::Entry(_) => "text: ",
            BarKind::Form => self.form.edit.as_ref().map_or("field: ", |edit| edit.prompt.as_str()),
        };
        self.draw_field(painter, rect, prompt, &bar.field, theme.ui_text.egui());
        if let Some(search) = self.search.as_ref().filter(|_| matches!(bar.kind, BarKind::Search { .. })) {
            let layout = &self.doc.layout;
            let shown = crate::search::shown_top_left(layout);
            let status = search.status((layout.rotation != 0).then_some(&shown as _));
            text_line(painter, &self.config, rect.right_center() - vec2(PADDING, 0.0), Align2::RIGHT_CENTER, status, theme.ui_accent.egui());
        }
    }

    /// How many rows of the menu fit under the bar.
    pub fn menu_rows(&self) -> usize {
        let room = self.view.rect.height() - bar_height(&self.config) * 2.0 - PADDING;
        (room / bar_height(&self.config)).max(0.0) as usize
    }

    /// Where the menu is: under the bar, at its left, a good share of the view wide.
    pub fn menu_rect(&self, menu: &Menu) -> Rect {
        let area = self.view.rect;
        let row_height = bar_height(&self.config);
        let (list, footer) = menu.layout(self.menu_rows());
        let rows = list + usize::from(footer);
        Rect::from_min_size(pos2(area.left(), area.top() + row_height), vec2(area.width(), rows as f32 * row_height + PADDING))
    }

    /// The menu's rows, each as its index in the list and the rectangle it covers.
    pub fn menu_row_rects(&self, menu: &Menu) -> Vec<(usize, Rect)> {
        let rect = self.menu_rect(menu);
        let row_height = bar_height(&self.config);
        let (list, _) = menu.layout(self.menu_rows());
        (0..list)
            .map(|row| {
                let top = rect.top() + PADDING / 2.0 + row as f32 * row_height;
                (menu.scroll + row, Rect::from_min_size(pos2(rect.left() + PADDING / 2.0, top), vec2(rect.width() - PADDING, row_height)))
            })
            .collect()
    }

    /// The menu of commands and actions under the `:` bar.
    fn draw_menu(&self, painter: &Painter, menu: &Menu) {
        let theme = self.config.active_theme();
        if let Some(count) = menu.count_label(self.menu_rows()) {
            let bar = Rect::from_min_size(self.view.rect.min, vec2(self.view.rect.width(), bar_height(&self.config)));
            let dim = theme.ui_text.egui().gamma_multiply(0.6);
            text_line(painter, &self.config, bar.right_center() - vec2(PADDING, 0.0), Align2::RIGHT_CENTER, &count, dim);
        }
        let rect = self.menu_rect(menu);
        painter.rect_filled(rect, 0.0, theme.ui_background.egui());
        painter.rect_stroke(rect, 0.0, egui::Stroke::new(self.config.view.border, theme.ui_accent.egui()), egui::StrokeKind::Inside);
        let clipped = painter.with_clip_rect(rect.shrink(PADDING / 2.0));
        let (text_color, accent) = (theme.ui_text.egui(), theme.ui_accent.egui());
        for (index, line) in self.menu_row_rects(menu) {
            let Some(entry) = menu.entries.get(index) else { continue };
            let selected = menu.highlighted() == Some(index);
            if selected {
                clipped.rect_filled(line, 0.0, accent);
            }
            let color = if selected { theme.ui_background.egui() } else { text_color };
            let dim = color.gamma_multiply(0.6);
            let keys = text_line(&clipped, &self.config, line.right_center() - vec2(PADDING, 0.0), Align2::RIGHT_CENTER, &entry.keys, dim);
            let room = Rect::from_min_max(line.min, pos2(keys.left() - PADDING, line.max.y)).intersect(clipped.clip_rect());
            let mut job = egui::text::LayoutJob::default();
            let format = |color: Color32| egui::TextFormat { font_id: font(&self.config), color, ..Default::default() };
            for (i, c) in entry.label.chars().enumerate() {
                let marked = entry.matched.contains(&i);
                job.append(c.encode_utf8(&mut [0; 4]), 0.0, format(if marked && !selected { accent } else { color }));
            }
            if !entry.hint.is_empty() {
                job.append(&format!(" {}", entry.hint), 0.0, format(dim));
            }
            let galley = painter.layout_job(job);
            let baseline = galley.rows.first().map_or(0.0, |row| row.pos.y + row.glyphs.first().map_or(0.0, |g| g.pos.y));
            let at = pos2(line.left() + PADDING, line.center().y + cap_middle(painter, font(&self.config)) - baseline);
            clipped.with_clip_rect(room).galley(at, galley, color);
        }
        if menu.incomplete {
            let bottom = Rect::from_min_size(
                pos2(rect.left(), rect.bottom() - PADDING / 2.0 - bar_height(&self.config)),
                vec2(rect.width(), bar_height(&self.config)),
            );
            text_line(
                &clipped,
                &self.config,
                bottom.left_center() + vec2(PADDING * 1.5, 0.0),
                Align2::LEFT_CENTER,
                "(listing incomplete)",
                text_color.gamma_multiply(0.6),
            );
        }
    }

    /// A prompt and a text field on one row. A line too long for the row slides left to keep the
    /// cursor in sight.
    fn draw_field(&self, painter: &Painter, rect: Rect, prompt: &str, field: &Field, color: Color32) {
        let width = |text: String| painter.layout_no_wrap(text, font(&self.config), color).size().x;
        let room = rect.shrink2(vec2(PADDING, 0.0));
        let to_cursor = width(format!("{prompt}{}", field.before_cursor()));
        let start = room.left() - (to_cursor - room.width() + 2.0).max(0.0);
        let clipped = painter.with_clip_rect(rect);
        text_line(&clipped, &self.config, pos2(start, rect.center().y), Align2::LEFT_CENTER, format!("{prompt}{}", field.text), color);
        let cursor = Rect::from_min_size(pos2(start + to_cursor, rect.top() + 3.0), vec2(1.5, rect.height() - 6.0));
        clipped.rect_filled(cursor, 0.0, self.config.active_theme().ui_accent.egui());
    }

    /// Where a signature is drawn.
    pub fn pad_rect(&self) -> Rect {
        let area = self.view.rect;
        Rect::from_center_size(area.center(), vec2((area.width() * 0.8).min(560.0), (area.height() * 0.6).min(200.0)))
    }

    fn draw_pad(&self, painter: &Painter, pad: &SignPad) {
        let theme = self.config.active_theme();
        let rect = self.pad_rect();
        let pen = egui::Stroke::new(2.0, theme.ui_text.egui());
        painter.rect_filled(rect.expand(PADDING), 0.0, theme.ui_background.egui());
        painter.rect_stroke(rect, 0.0, egui::Stroke::new(self.config.view.border, theme.ui_accent.egui()), egui::StrokeKind::Outside);
        let baseline = rect.bottom() - rect.height() * 0.25;
        painter
            .line_segment([pos2(rect.left() + 16.0, baseline), pos2(rect.right() - 16.0, baseline)], egui::Stroke::new(1.0, theme.ui_accent.egui()));
        for stroke in &pad.strokes {
            let on_pad = |p: &[f32; 2]| rect.min + vec2(p[0], p[1]) * rect.width();
            if let [point] = stroke.as_slice() {
                painter.circle_filled(on_pad(point), 1.5, theme.ui_text.egui());
            } else {
                painter.add(egui::Shape::line(stroke.iter().map(on_pad).collect(), pen));
            }
        }
    }

    fn draw_toc(&self, painter: &Painter, state: &TocState) {
        let rows = self.toc_rows(state);
        let at = rows.iter().position(|r| *r >= state.selected).unwrap_or(0);
        let filter = (state.filtering || !state.filter.is_empty()).then_some(&state.filter);
        self.draw_list(painter, &ListView { header: None, filter, len: rows.len(), at, first: None }, |i| {
            let entry = &self.doc.toc[rows[i]];
            ListRow { indent: entry.depth as f32 * 14.0, text: entry.title.clone(), mark: (entry.page + 1).to_string(), ticked: false }
        });
    }

    /// The bookmarks: a panel like the contents, each row a name and its page.
    fn draw_bookmarks(&self, painter: &Painter, state: &BookmarkState) {
        let rows = self.bookmark_rows(state);
        let at = rows.iter().position(|r| *r >= state.selected).unwrap_or(0);
        let filter = (state.filtering || !state.filter.is_empty()).then_some(&state.filter);
        self.draw_list(painter, &ListView { header: None, filter, len: rows.len(), at, first: Some(state.first) }, |i| {
            let bookmark = &self.side.bookmarks[rows[i]];
            ListRow { indent: 0.0, text: bookmark.name.clone(), mark: (bookmark.anchor.page + 1).to_string(), ticked: false }
        });
    }

    /// The recent files: a panel like the contents, each row a hint label, the name and the folder.
    fn draw_recent(&self, painter: &Painter, state: &RecentState) {
        let theme = self.config.active_theme();
        let layout = self.list_layout(true, false, state.files.len(), state.selected, None);
        let rect = layout.rect;
        painter.rect_filled(rect, 0.0, theme.ui_background.egui());
        painter.rect_stroke(rect, 0.0, egui::Stroke::new(self.config.view.border, theme.ui_accent.egui()), egui::StrokeKind::Inside);
        let clipped = painter.with_clip_rect(rect.shrink(PADDING / 2.0));
        let header = pos2(rect.left() + PADDING, rect.top() + PADDING + layout.row_height / 2.0);
        text_line(&clipped, &self.config, header, Align2::LEFT_CENTER, "Recent files", theme.ui_accent.egui());
        let home = crate::config::home();
        for i in (layout.first..state.files.len()).take(layout.fits) {
            let line = Self::list_row_rect(&layout, i);
            let selected = i == state.selected;
            if selected {
                clipped.rect_filled(line, 0.0, theme.ui_accent.egui());
            }
            let mut color = if selected { theme.ui_background.egui() } else { theme.ui_text.egui() };
            let mut label_color = if selected { color } else { theme.ui_accent.egui() };
            // What is typed so far dims the rows it no longer leads to.
            if !state.labels[i].starts_with(&state.typed) {
                (color, label_color) = (color.gamma_multiply(0.35), label_color.gamma_multiply(0.35));
            }
            let path = &state.files[i];
            let name = path.file_name().map_or_else(|| path.display().to_string(), |n| n.to_string_lossy().into_owned());
            let folder = path.parent().map_or_else(String::new, |p| crate::recent::tilde(p, &home));
            let at = |x: f32| pos2(x, line.center().y);
            let label = text_line(&clipped, &self.config, at(line.left() + PADDING), Align2::LEFT_CENTER, &state.labels[i], label_color);
            let name = text_line(&clipped, &self.config, at(label.right() + PADDING * 2.0), Align2::LEFT_CENTER, name, color);
            text_line(&clipped, &self.config, at(name.right() + PADDING), Align2::LEFT_CENTER, folder, color.gamma_multiply(0.6));
        }
    }

    /// The password menu, a panel like the contents with a header, the field and a line for what went wrong.
    fn draw_password(&self, painter: &Painter, state: &PasswordState) {
        let theme = self.config.active_theme();
        let area = self.view.rect;
        let row_height = bar_height(&self.config);
        let size = vec2((area.width() * 0.7).min(760.0), row_height * 3.0 + PADDING * 2.0);
        let rect = Rect::from_center_size(area.center(), size);
        painter.rect_filled(rect, 0.0, theme.ui_background.egui());
        painter.rect_stroke(rect, 0.0, egui::Stroke::new(self.config.view.border, theme.ui_accent.egui()), egui::StrokeKind::Inside);
        let clipped = painter.with_clip_rect(rect.shrink(PADDING / 2.0));
        let top = rect.top() + PADDING;
        let header = pos2(rect.left() + PADDING, top + row_height / 2.0);
        text_line(&clipped, &self.config, header, Align2::LEFT_CENTER, state.header(), theme.ui_accent.egui());
        let line = Rect::from_min_size(pos2(rect.left(), top + row_height), vec2(rect.width(), row_height));
        self.draw_field(painter, line, "> ", &state.field.masked(), theme.ui_text.egui());
        if let Some(notice) = &state.notice {
            let at = pos2(rect.left() + PADDING, top + row_height * 2.5);
            text_line(&clipped, &self.config, at, Align2::LEFT_CENTER, notice, theme.ui_accent.egui());
        }
    }

    fn draw_choice(&self, painter: &Painter, state: &ChoiceState) {
        let rows = state.rows();
        let filter = (state.filtering || !state.filter.is_empty()).then_some(&state.filter);
        let view = ListView { header: Some(&state.title), filter, len: rows.len(), at: state.selected, first: Some(state.first) };
        self.draw_list(painter, &view, |i| ListRow { indent: 0.0, text: state.label(rows[i]), mark: String::new(), ticked: state.ticked(rows[i]) });
    }

    /// Where a list panel (the contents, the choices) and its rows go: a centred box with the
    /// optional header and filter rows above the list, scrolled to keep row `at` in the middle,
    /// or to start at row `first` when that is given.
    pub fn list_layout(&self, header: bool, filter: bool, len: usize, at: usize, first: Option<usize>) -> ListLayout {
        let area = self.view.rect;
        let size = vec2((area.width() * 0.7).min(760.0), area.height() * 0.8);
        let rect = Rect::from_center_size(area.center(), size);
        let row_height = bar_height(&self.config);
        let top = rect.top() + PADDING + row_height * (usize::from(header) + usize::from(filter)) as f32;
        let fits = (((rect.bottom() - top - PADDING) / row_height) as usize).max(1);
        let first = first.unwrap_or_else(|| at.saturating_sub(fits / 2)).min(len.saturating_sub(fits));
        ListLayout { rect, top, row_height, first, fits }
    }

    /// The box of a row of a list panel, as `list_layout` placed it.
    pub fn list_row_rect(layout: &ListLayout, i: usize) -> Rect {
        let y = layout.top + (i - layout.first) as f32 * layout.row_height;
        Rect::from_min_size(pos2(layout.rect.left() + PADDING / 2.0, y), vec2(layout.rect.width() - PADDING, layout.row_height))
    }

    /// A panel of rows with a mark at the right of each, as the table of contents is drawn.
    fn draw_list(&self, painter: &Painter, view: &ListView, row: impl Fn(usize) -> ListRow) {
        let ListView { header, filter, len, at, first } = *view;
        let theme = self.config.active_theme();
        let layout = self.list_layout(header.is_some(), filter.is_some(), len, at, first);
        let rect = layout.rect;
        painter.rect_filled(rect, 0.0, theme.ui_background.egui());
        painter.rect_stroke(rect, 0.0, egui::Stroke::new(self.config.view.border, theme.ui_accent.egui()), egui::StrokeKind::Inside);
        let row_height = layout.row_height;
        let mut y = rect.top() + PADDING;
        let text_color = theme.ui_text.egui();
        if let Some(header) = header {
            let at = pos2(rect.left() + PADDING, y + row_height / 2.0);
            let room = Rect::from_min_max(rect.min, pos2(rect.right() - PADDING, rect.bottom())).intersect(rect.shrink(PADDING / 2.0));
            text_line(&painter.with_clip_rect(room), &self.config, at, Align2::LEFT_CENTER, header, theme.ui_accent.egui());
            y += row_height;
        }
        if let Some(filter) = filter {
            let line = Rect::from_min_size(pos2(rect.left(), y), vec2(rect.width(), row_height));
            self.draw_field(painter, line, "/", filter, theme.ui_accent.egui());
        }
        let clipped = painter.with_clip_rect(rect.shrink(PADDING / 2.0));
        for i in (layout.first..len).take(layout.fits) {
            let row = row(i);
            let line = Self::list_row_rect(&layout, i);
            let selected = i == at;
            if selected {
                clipped.rect_filled(line, 0.0, theme.ui_accent.egui());
            }
            let color = if selected { theme.ui_background.egui() } else { text_color };
            let indent = PADDING + row.indent;
            let mark = if row.ticked {
                // Drawn, not typed: the fonts bundled with egui have no check mark.
                let size = row_height * 0.4;
                let tick = Rect::from_min_size(pos2(line.right() - PADDING - size, line.center().y - size / 2.0), vec2(size, size));
                let at = |x: f32, y: f32| tick.min + vec2(x, y) * size;
                let points = vec![at(0.0, 0.55), at(0.4, 1.0), at(1.0, 0.0)];
                clipped.add(egui::Shape::line(points, egui::Stroke::new((row_height / 12.0).max(1.5), color)));
                tick
            } else {
                text_line(&clipped, &self.config, line.right_center() - vec2(PADDING, 0.0), Align2::RIGHT_CENTER, row.mark, color)
            };
            // A long title stops short of its mark.
            let room = Rect::from_min_max(line.min, pos2(mark.left() - PADDING, line.max.y)).intersect(clipped.clip_rect());
            text_line(&clipped.with_clip_rect(room), &self.config, pos2(line.left() + indent, line.center().y), Align2::LEFT_CENTER, row.text, color);
        }
    }

    // The hotbar.

    fn hotbar_items(&self) -> Vec<Item> {
        let mut items = vec![
            Item::Tool(Tool::Highlight, "highlight"),
            Item::Tool(Tool::Draw, "draw"),
            Item::Tool(Tool::Erase, "erase"),
            Item::Tool(Tool::Note, "note"),
            Item::Tool(Tool::Fill, "fill & sign"),
        ];
        // What fill & sign writes has no colour, so its choices take the colours' place.
        if self.tool == Some(Tool::Fill) {
            items.extend(FillItem::ALL.map(Item::Fill));
        } else {
            items.extend((0..self.config.hotbar.colors.len()).map(Item::Color));
        }
        items.extend([Item::Thinner, Item::Thicker, Item::Undo, Item::Redo]);
        items
    }

    fn item_label(&self, item: Item) -> String {
        match item {
            Item::Tool(_, label) => label.to_owned(),
            Item::Fill(fill) => fill.label().to_owned(),
            Item::Color(i) => (i + 1).to_string(),
            Item::Thinner => "-".to_owned(),
            Item::Thicker => "+".to_owned(),
            Item::Undo => "undo".to_owned(),
            Item::Redo => "redo".to_owned(),
        }
    }

    fn item_active(&self, item: Item) -> bool {
        match item {
            Item::Tool(tool, _) => self.tool == Some(tool),
            Item::Fill(fill) => self.tool == Some(Tool::Fill) && self.fill == fill,
            Item::Color(i) => self.color == i,
            _ => false,
        }
    }

    fn item_clicked(&mut self, ctx: &egui::Context, item: Item) {
        match item {
            Item::Tool(Tool::Highlight, _) => self.run(ctx, Action::ToolHighlight, None),
            Item::Tool(Tool::Draw, _) => self.run(ctx, Action::ToolDraw, None),
            Item::Tool(Tool::Erase, _) => self.run(ctx, Action::ToolErase, None),
            Item::Tool(Tool::Note, _) => self.run(ctx, Action::ToolNote, None),
            Item::Tool(Tool::Fill, _) => self.run(ctx, Action::ToolFill, None),
            Item::Fill(fill) => self.pick_fill(fill),
            Item::Color(i) => self.color = i,
            Item::Thinner => self.run(ctx, Action::ToolWidthDown, None),
            Item::Thicker => self.run(ctx, Action::ToolWidthUp, None),
            Item::Undo => self.run(ctx, Action::Undo, None),
            Item::Redo => self.run(ctx, Action::Redo, None),
        }
    }

    /// Where the hotbar goes for its size.
    fn hotbar_rect(&self, size: Vec2) -> Rect {
        let area = self.view.rect;
        let margin = 12.0;
        let min = match self.config.hotbar.position {
            Side::Bottom => pos2(area.center().x - size.x / 2.0, area.bottom() - size.y - margin),
            Side::Top => pos2(area.center().x - size.x / 2.0, area.top() + bar_height(&self.config) + margin),
            Side::Left => pos2(area.left() + margin, area.center().y - size.y / 2.0),
            Side::Right => pos2(area.right() - size.x - margin, area.center().y - size.y / 2.0),
        };
        Rect::from_min_size(min, size)
    }

    /// Shown as `HotbarState` says: asked for, or a tool in hand and not hidden.
    /// Not while the command menu is shown: this registers widgets after the view, so it would
    /// take the clicks meant for the menu that covers it.
    fn draw_hotbar(&mut self, ui: &egui::Ui) {
        if !self.hotbar.visible(self.tool.is_some()) || self.open_menu().is_some() || matches!(self.mode, Mode::Password(_)) {
            return;
        }
        let ctx = ui.ctx().clone();
        let painter = ui.painter();
        let items = self.hotbar_items();
        let vertical = matches!(self.config.hotbar.position, Side::Left | Side::Right);
        let height = bar_height(&self.config);
        let widths: Vec<f32> =
            items.iter().map(|i| painter.layout_no_wrap(self.item_label(*i), font(&self.config), Color32::WHITE).size().x + 2.0 * PADDING).collect();
        let widest = widths.iter().copied().fold(height, f32::max);
        let inner = if vertical { vec2(widest, height * items.len() as f32) } else { vec2(widths.iter().sum(), height) };
        let rect = self.hotbar_rect(inner + Vec2::splat(PADDING));
        let _ = ui.interact(rect, ui.scope_id().with("hotbar"), Sense::click_and_drag());
        let theme = self.config.active_theme().clone();
        painter.rect_filled(rect, 0.0, theme.ui_background.egui());
        let border = egui::Stroke::new(self.config.view.border, theme.ui_accent.egui());
        painter.rect_stroke(rect, 0.0, border, egui::StrokeKind::Inside);
        let mut at = rect.min + Vec2::splat(PADDING / 2.0);
        let mut clicked = None;
        for (index, (item, width)) in items.iter().zip(widths).enumerate() {
            let size = if vertical { vec2(widest, height) } else { vec2(width, height) };
            let cell = Rect::from_min_size(at, size);
            at += if vertical { vec2(0.0, height) } else { vec2(width, 0.0) };
            let response = ui.interact(cell, ui.scope_id().with(("hotbar", index)), Sense::click());
            if response.clicked() {
                clicked = Some(*item);
            }
            self.paint_item(painter, *item, cell, response.hovered());
        }
        if let Some(item) = clicked {
            self.item_clicked(&ctx, item);
        }
    }

    fn paint_item(&self, painter: &Painter, item: Item, cell: Rect, hovered: bool) {
        let theme = self.config.active_theme();
        let active = self.item_active(item);
        let inset = cell.shrink(2.0);
        let mut text = theme.ui_text.egui();
        if let Item::Color(i) = item {
            let swatch = self.config.hotbar.colors[i].egui();
            painter.rect_filled(inset, 0.0, swatch);
            text = theme.ui_background.egui();
            if active {
                painter.rect_stroke(inset, 0.0, egui::Stroke::new(2.0, theme.ui_text.egui()), egui::StrokeKind::Outside);
            }
        } else if active {
            painter.rect_filled(inset, 0.0, theme.ui_accent.egui());
            text = theme.ui_background.egui();
        } else if hovered {
            painter.rect_filled(inset, 0.0, theme.ui_accent.egui().gamma_multiply(0.3));
        }
        text_line(painter, &self.config, cell.center(), Align2::CENTER_CENTER, self.item_label(item), text);
    }
}

/// The y, in the page's own points, of the middle of the view on `page`.
fn centre_y(layout: &Layout, view: &View, page: usize) -> f32 {
    let shown = [view.x - layout.lefts[page], view.y + view.rect.height() / view.zoom / 2.0 - layout.tops[page]];
    unrotate_point(layout.rotation, layout.page_sizes[page], shown)[1]
}

#[cfg(test)]
mod tests {
    use super::{centre_y, expand};
    use crate::view::{Layout, View};

    fn field(name: &str) -> String {
        match name {
            "page" => "3".to_owned(),
            "pages" => "9".to_owned(),
            _ => String::new(),
        }
    }

    #[test]
    fn fields_are_filled_in() {
        assert_eq!(expand("Page {page} / {pages}", field), "Page 3 / 9");
    }

    #[test]
    fn a_section_with_only_empty_fields_goes() {
        assert_eq!(expand("Page {page} | {chapter} | {message}", field), "Page 3");
        assert_eq!(expand("{chapter} | {message}", field), "");
    }

    #[test]
    fn literal_sections_stay_and_a_partly_empty_section_stays() {
        assert_eq!(expand("text | {page}{chapter}", field), "text | 3");
    }

    #[test]
    fn an_unclosed_brace_is_left_alone() {
        assert_eq!(expand("a {page", field), "a {page");
    }

    #[test]
    fn the_middle_of_the_view_is_found_in_page_points_under_rotation() {
        let mut layout = Layout::new(vec![[600.0, 800.0]], 8.0);
        let rect = egui::Rect::from_min_size(egui::pos2(0.0, 0.0), egui::vec2(100.0, 200.0));
        // Scrolled so the view's middle is 300 down and 50 right of the middle, as shown.
        let view = View { x: 50.0, y: 200.0, zoom: 1.0, rect, zoom_hold: None };
        assert_eq!(centre_y(&layout, &view, 0), 300.0);
        // Turned 180: the shown point [350, 300] is the page point [250, 500].
        layout.set_rotation(180);
        assert_eq!(centre_y(&layout, &view, 0), 500.0);
        // Turned 90 (800 wide, 600 high): the shown point [450, 300] is the page point [300, 800 - 450].
        layout.set_rotation(90);
        assert_eq!(centre_y(&layout, &view, 0), 350.0);
    }
}

#[cfg(test)]
mod centre_tests {
    use super::centre_y;
    use crate::view::{Layout, View};

    fn view(x: f32, y: f32, zoom: f32, h: f32) -> View {
        let rect = egui::Rect::from_min_size(egui::pos2(0.0, 0.0), egui::vec2(100.0, h));
        View { x, y, zoom, rect, zoom_hold: None }
    }

    #[test]
    fn the_centre_on_a_later_page_and_a_zoom_at_every_rotation() {
        // Two 600x800 pages, gap 8: the second starts at 808 unrotated, at 608 turned 90 or 270.
        let mut layout = Layout::new(vec![[600.0, 800.0], [600.0, 800.0]], 8.0);
        // Zoom 2, a 400 high window: the middle is 100 view points below the top.
        let v = view(20.0, 1000.0, 2.0, 400.0);
        // Rotation 0: shown [320, 1100 - 808 = 292], the page point is the same.
        assert_eq!(centre_y(&layout, &v, 1), 292.0);
        // Rotation 180: shown [320, 292] -> y = 800 - 292.
        layout.set_rotation(180);
        assert_eq!(centre_y(&layout, &v, 1), 508.0);
        // Rotation 90: pages are 800 wide, 600 high, the second starts at 608. Shown [420, 492]:
        // page point [492, 800 - 420].
        layout.set_rotation(90);
        assert_eq!(centre_y(&layout, &v, 1), 380.0);
        // Rotation 270: shown [420, 492]: page point [600 - 492, 420].
        layout.set_rotation(270);
        assert_eq!(centre_y(&layout, &v, 1), 420.0);
    }

    #[test]
    fn a_zero_zoom_does_not_panic() {
        let layout = Layout::new(vec![[600.0, 800.0]], 8.0);
        let _ = centre_y(&layout, &view(0.0, 0.0, 0.0, 100.0), 0);
    }
}
