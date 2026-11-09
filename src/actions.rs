//! What bound keys and commands do.
use std::fmt::Write;
use std::path::Path;
use std::sync::mpsc::TryRecvError;
use std::time::Instant;

use egui::Pos2;
use pdfcraft_render::LinkTarget;

use crate::app::{
    App, Bar, BarKind, Edit, Entry, EntryKind, FillItem, Fit, Landing, MarkOp, Mode, PendingPortal, Selection, SignPad, TocState, Tool,
};
use crate::config::expand_home;
use crate::doc::{dest_point, dest_y};
use crate::field::Field;
use crate::form::Report;
use crate::hint::{self, HintState, Purpose, Target, TargetKind};
use crate::keymap::Action;
use crate::search::Search;
use crate::smart::{Line, Reference, find_definitions, lines_of, references_in, touches};
use crate::store::{self, Annotation, MarkKind, NOTE_SIDE, Portal};
use crate::view::Anchor;

impl App {
    pub fn run(&mut self, ctx: &egui::Context, action: Action, count: Option<u32>) {
        if self.overview_action(action, count) {
            return;
        }
        let handled =
            self.run_motion(action, count) || self.run_zoom(action, count) || self.run_navigation(action, count) || self.run_modes(ctx, action);
        if !handled {
            self.run_marking(action);
            self.run_portal(action);
            self.run_toggle(ctx, action);
        }
        self.view.clamp(&self.doc.layout);
    }

    fn run_motion(&mut self, action: Action, count: Option<u32>) -> bool {
        let n = count.unwrap_or(1) as f32;
        let zoom = self.view.zoom;
        let step = self.config.view.scroll_step / zoom * n;
        let height = self.view.rect.height() / zoom;
        let span = self.doc.layout.max_width - self.view.rect.width() / zoom;
        match action {
            Action::ScrollDown => self.view.y += step,
            Action::ScrollUp => self.view.y -= step,
            Action::ScrollLeft => self.view.x -= step,
            Action::ScrollRight => self.view.x += step,
            Action::HalfDown => self.view.y += height / 2.0 * n,
            Action::HalfUp => self.view.y -= height / 2.0 * n,
            Action::ScreenDown => self.view.y += height * n,
            Action::ScreenUp => self.view.y -= height * n,
            Action::EdgeLeft => self.view.x = -span.max(0.0) / 2.0,
            Action::EdgeRight => self.view.x = span.max(0.0) / 2.0,
            Action::Center => self.view.x = 0.0,
            _ => return false,
        }
        true
    }

    fn run_zoom(&mut self, action: Action, count: Option<u32>) -> bool {
        let times = count.unwrap_or(1).min(64) as i32;
        match action {
            Action::ZoomIn => self.zoom_by(self.config.view.zoom_step.powi(times)),
            Action::ZoomOut => self.zoom_by(self.config.view.zoom_step.powi(-times)),
            Action::FitWidth => self.set_fit(Fit::Width),
            Action::FitHeight => self.set_fit(Fit::Height),
            _ => return false,
        }
        true
    }

    pub fn zoom_by(&mut self, factor: f32) {
        self.zoom_at(factor, None);
    }

    /// Zooms holding still what is at `at`, or what zooming was holding still.
    pub fn zoom_at(&mut self, factor: f32, at: Option<Pos2>) {
        let zoom = (self.view.zoom * factor).clamp(self.config.view.min_zoom, self.config.view.max_zoom);
        self.view.zoom_around(zoom, at, &self.doc.layout);
        self.zoom_changed = Instant::now();
        self.fit = None;
        self.mark_dirty();
    }

    fn set_fit(&mut self, fit: Fit) {
        self.fit = Some(fit);
        self.apply_fit();
        self.view.x = 0.0;
        self.mark_dirty();
    }

    /// Remembers where the view is before a long jump.
    pub fn record_jump(&mut self) {
        let here = self.view.anchor(&self.doc.layout);
        self.jumps.truncate(self.jump_index);
        if self.jumps.last() != Some(&here) {
            self.jumps.push(here);
        }
        self.jump_index = self.jumps.len();
    }

    /// Goes to a place, as a jump that `jump_back` can undo.
    pub fn jump_to(&mut self, anchor: Anchor) {
        self.record_jump();
        self.view.go_to(&self.doc.layout, anchor);
        self.mark_dirty();
    }

    /// Goes to a point, with it at `view.jump_position` of the window height.
    pub fn jump_to_point(&mut self, anchor: Anchor) {
        let span = self.view.rect.height() / self.view.zoom;
        let y = view_top(anchor.y, self.config.view.jump_position, span);
        self.jump_to(Anchor { y, ..anchor });
    }

    pub fn jump_to_page(&mut self, page: usize) {
        let last = self.doc.layout.len() - 1;
        self.jump_to(Anchor { page: page.min(last), y: 0.0, x: self.view.x });
    }

    fn jump_history(&mut self, back: bool, count: u32) {
        for _ in 0..count {
            let here = self.view.anchor(&self.doc.layout);
            if back {
                if self.jump_index == 0 {
                    break;
                }
                // Leaving the newest place: remember it so jump_forward can return.
                if self.jump_index == self.jumps.len() {
                    self.jumps.push(here);
                }
                self.jump_index -= 1;
            } else {
                if self.jump_index + 1 >= self.jumps.len() {
                    break;
                }
                self.jump_index += 1;
            }
            self.view.go_to(&self.doc.layout, self.jumps[self.jump_index]);
        }
        self.mark_dirty();
    }

    fn run_navigation(&mut self, action: Action, count: Option<u32>) -> bool {
        let n = count.unwrap_or(1) as usize;
        let layout = &self.doc.layout;
        let page = self.view.anchor(layout).page;
        match action {
            Action::NextPage => {
                let target = (page + n).min(layout.len() - 1);
                self.view.go_to(&self.doc.layout, Anchor { page: target, y: 0.0, x: self.view.x });
            }
            Action::PrevPage => self.view.go_to(&self.doc.layout, Anchor { page: page.saturating_sub(n), y: 0.0, x: self.view.x }),
            Action::GotoFirst => self.jump_to_page(count.map_or(0, |c| c.saturating_sub(1) as usize)),
            Action::GotoLast => match count {
                Some(c) => self.jump_to_page(c.saturating_sub(1) as usize),
                None => {
                    let end = Anchor { page: layout.len() - 1, y: f32::MAX / 4.0, x: self.view.x };
                    self.jump_to(end);
                }
            },
            Action::JumpBack => self.jump_history(true, count.unwrap_or(1)),
            Action::JumpForward => self.jump_history(false, count.unwrap_or(1)),
            Action::SearchNext => self.search_step(false),
            Action::SearchPrev => self.search_step(true),
            _ => return false,
        }
        self.mark_dirty();
        true
    }

    fn search_step(&mut self, reverse: bool) {
        let Some(search) = self.search.as_mut() else {
            self.say("no search yet");
            return;
        };
        let target = search.advance(reverse).map(|m| (m.page, m.rects.first().copied()));
        match target {
            Some((page, Some(rect))) => {
                self.record_jump();
                self.view.reveal(&self.doc.layout, page, rect);
            }
            _ => {
                let status = self.search.as_ref().map(Search::status).unwrap_or_default();
                self.say(status);
            }
        }
    }

    fn open_bar(&mut self, kind: BarKind) {
        let previous = if matches!(kind, BarKind::Search { .. }) { self.search.clone() } else { None };
        let origin = self.view.anchor(&self.doc.layout);
        self.mode = Mode::Bar(Bar { kind, field: Field::default(), origin, previous, menu: None });
        self.refresh_menu();
    }

    fn run_modes(&mut self, ctx: &egui::Context, action: Action) -> bool {
        match action {
            Action::SearchForward => self.open_bar(BarKind::Search { forward: true }),
            Action::SearchBackward => self.open_bar(BarKind::Search { forward: false }),
            Action::Command => self.open_bar(BarKind::Command),
            Action::SetMark => self.mode = Mode::AwaitMark(MarkOp::Set),
            Action::GotoMark => self.mode = Mode::AwaitMark(MarkOp::Goto),
            Action::Cancel => self.escape(),
            // With something to place, hints choose where.
            Action::HintSelect if matches!(self.tool, Some(Tool::Note | Tool::Fill)) => self.mode = Mode::Hint(HintState::new(Purpose::Place)),
            Action::HintSelect => self.mode = Mode::Hint(HintState::new(Purpose::Select)),
            Action::SmartJump => self.mode = Mode::Hint(HintState::new(Purpose::Smart)),
            Action::Overview => self.mode = Mode::Hint(HintState::new(Purpose::Overview)),
            Action::HintLink => self.mode = Mode::Hint(HintState::new(Purpose::Link)),
            Action::HintForm => self.mode = Mode::Hint(HintState::new(Purpose::Form)),
            Action::DeleteAnnotation => self.mode = Mode::Hint(HintState::new(Purpose::DeleteAnnotation)),
            Action::ToggleToc => self.open_toc(),
            Action::Open => self.choose_file(ctx),
            Action::Quit => ctx.send_viewport_cmd_to(egui::ViewportId::ROOT, egui::ViewportCommand::Close),
            _ => return false,
        }
        true
    }

    fn open_toc(&mut self) {
        if !self.doc.contents.is_empty() {
            self.toc_waiting = true;
            self.say("reading the contents pages");
            return;
        }
        if self.doc.toc.is_empty() {
            self.say("this document has no table of contents");
            return;
        }
        let page = self.view.current_page(&self.doc.layout);
        let selected = self.doc.toc.iter().rposition(|e| e.page <= page).unwrap_or(0);
        self.mode = Mode::Toc(TocState { selected, ..Default::default() });
    }

    /// Reads the printed contents once their text has arrived.
    pub fn pump_contents(&mut self) {
        if self.doc.contents_ready() {
            self.doc.read_contents();
            if std::mem::take(&mut self.toc_waiting) {
                self.open_toc();
            }
        }
    }

    /// Runs a mark key.
    pub fn mark_key(&mut self, op: MarkOp, key: char) {
        match op {
            MarkOp::Set => {
                let here = self.view.anchor(&self.doc.layout);
                self.side.marks.insert(key, here);
                self.mark_dirty();
                self.say(format!("mark {key} set"));
            }
            MarkOp::Goto if key == '\'' || key == '`' => self.jump_history(true, 1),
            MarkOp::Goto => match self.side.marks.get(&key).copied() {
                Some(anchor) => self.jump_to(anchor),
                None => self.say(format!("no mark {key}")),
            },
        }
    }

    // Selections, highlights and drawings.

    /// The selected glyph range of each page it touches.
    pub fn selection_parts(&self) -> Vec<(usize, std::ops::Range<usize>)> {
        let Some(selection) = self.selection else { return Vec::new() };
        let mut parts = Vec::new();
        for page in selection.start.0..=selection.end.0 {
            let Some(text) = self.doc.text.get(page).and_then(|t| t.as_ref()) else { continue };
            let first = if page == selection.start.0 { selection.start.1 } else { 0 };
            let last = if page == selection.end.0 { selection.end.1 + 1 } else { text.glyphs.len() };
            parts.push((page, first..last.min(text.glyphs.len())));
        }
        parts
    }

    pub fn selection_text(&self) -> String {
        let parts = self.selection_parts();
        let text_of =
            |(page, range): &(usize, std::ops::Range<usize>)| self.doc.text[*page].as_ref().map(|t| t.text_of(range.clone())).unwrap_or_default();
        parts.iter().map(text_of).collect::<Vec<_>>().join("\n")
    }

    fn yank(&mut self, ctx: &egui::Context) {
        let text = self.selection_text();
        if text.is_empty() {
            self.say("nothing selected");
            return;
        }
        let chars = text.chars().count();
        ctx.copy_text(text);
        self.selection = None;
        self.say(format!("copied {chars} characters"));
    }

    fn current_color(&self) -> [u8; 3] {
        self.config.hotbar.colors[self.color.min(self.config.hotbar.colors.len() - 1)].rgb()
    }

    /// Turns the selection into highlights.
    pub fn highlight_selection(&mut self) {
        let color = self.current_color();
        let mut added = Vec::new();
        for (page, range) in self.selection_parts() {
            let Some(text) = self.doc.text[page].clone() else { continue };
            let rects = text.line_rects(range.clone());
            if !rects.is_empty() {
                added.push(Annotation::Highlight { id: self.side.take_id(), page, rects, color, text: text.text_of(range) });
            }
        }
        self.selection = None;
        self.add_annotations(added);
    }

    pub fn commit_stroke(&mut self) {
        let Some((page, points)) = self.stroke.take() else { return };
        let stroke = Annotation::Stroke { id: self.side.take_id(), page, points, color: self.current_color(), width: self.stroke_width };
        self.add_annotations(vec![stroke]);
    }

    fn add_annotations(&mut self, added: Vec<Annotation>) {
        if added.is_empty() {
            return;
        }
        self.side.annotations.extend(added.iter().cloned());
        self.undo.push(Edit::Added(added));
        self.redo.clear();
        self.mark_dirty();
    }

    /// Removes an annotation, as a change `undo` can take back.
    pub fn remove_annotation(&mut self, id: u64) {
        let Some(index) = self.side.annotations.iter().position(|a| a.id() == id) else { return };
        let removed = self.side.annotations.remove(index);
        self.undo.push(Edit::Removed(vec![removed]));
        self.redo.clear();
        self.mark_dirty();
    }

    /// Applies a change, returning its opposite.
    fn apply_edit(&mut self, edit: Edit) -> Edit {
        match edit {
            Edit::Added(items) => {
                self.side.annotations.retain(|a| !items.iter().any(|i| i.id() == a.id()));
                Edit::Removed(items)
            }
            Edit::Removed(items) => {
                self.side.annotations.extend(items.iter().cloned());
                Edit::Added(items)
            }
            Edit::Replaced { before, after } => {
                if let Some(slot) = self.side.annotations.iter_mut().find(|a| a.id() == after.id()) {
                    *slot = before.clone();
                }
                Edit::Replaced { before: after, after: before }
            }
            Edit::Form(changes) => self.apply_form_edit(changes),
        }
    }

    fn undo_redo(&mut self, undo: bool) {
        // The pad is modal: undo takes off a stroke and redo does nothing, whatever asked.
        if let Mode::Sign(pad) = &mut self.mode {
            if undo {
                pad.undo_stroke();
            }
            return;
        }
        let source = if undo { &mut self.undo } else { &mut self.redo };
        let Some(edit) = source.pop() else {
            self.say(if undo { "nothing to undo" } else { "nothing to redo" });
            return;
        };
        let opposite = self.apply_edit(edit);
        if undo {
            self.redo.push(opposite)
        } else {
            self.undo.push(opposite)
        }
        self.mark_dirty();
    }

    pub fn set_tool(&mut self, tool: Option<Tool>) {
        if tool.is_some() {
            self.hotbar.tool_picked();
        } else {
            self.hotbar.tool_dropped();
        }
        self.tool = tool;
        self.stroke = None;
    }

    fn pick_tool(&mut self, tool: Tool) {
        // A selection made first is highlighted at once.
        if tool == Tool::Highlight && self.selection.is_some() {
            self.highlight_selection();
            return;
        }
        self.set_tool(if self.tool == Some(tool) { None } else { Some(tool) });
    }

    /// Takes up the fill & sign tool with `item`.
    pub fn pick_fill(&mut self, item: FillItem) {
        self.set_tool(Some(Tool::Fill));
        self.fill = item;
        if item == FillItem::Sign && self.signature.is_none() {
            self.mode = Mode::Sign(SignPad::default());
        }
    }

    /// Makes the tool's line, or what fill & sign writes, thinner or thicker.
    fn change_width(&mut self, thicker: bool) {
        if self.tool == Some(Tool::Fill) {
            self.fill_size = (self.fill_size + if thicker { 1.0 } else { -1.0 }).clamp(4.0, 72.0);
            self.say(format!("size {}", self.fill_size));
        } else {
            self.stroke_width = (self.stroke_width + if thicker { 0.5 } else { -0.5 }).clamp(0.5, 20.0);
        }
    }

    fn run_marking(&mut self, action: Action) {
        match action {
            Action::ToolHighlight => self.pick_tool(Tool::Highlight),
            Action::ToolDraw => self.pick_tool(Tool::Draw),
            Action::ToolErase => self.pick_tool(Tool::Erase),
            Action::ToolNote => self.pick_tool(Tool::Note),
            Action::ToolFill => self.pick_tool(Tool::Fill),
            Action::FillText => self.pick_fill(FillItem::Text),
            Action::FillDate => self.pick_fill(FillItem::Date),
            Action::FillCheck => self.pick_fill(FillItem::Check),
            Action::FillCross => self.pick_fill(FillItem::Cross),
            Action::FillDot => self.pick_fill(FillItem::Dot),
            Action::FillLine => self.pick_fill(FillItem::Line),
            Action::FillSign => self.pick_fill(FillItem::Sign),
            Action::ToolWidthDown => self.change_width(false),
            Action::ToolWidthUp => self.change_width(true),
            Action::Undo => self.undo_redo(true),
            Action::Redo => self.undo_redo(false),
            _ => {}
        }
    }

    // Notes and fill & sign.

    fn open_entry(&mut self, entry: Entry, text: String) {
        let origin = self.view.anchor(&self.doc.layout);
        self.mode = Mode::Bar(Bar { kind: BarKind::Entry(entry), field: Field::new(text), origin, previous: None, menu: None });
    }

    /// Opens a note for editing.
    pub fn edit_note(&mut self, id: u64) {
        let note = self.side.annotations.iter().find(|a| a.id() == id);
        if let Some(Annotation::Note { page, at, text, .. }) = note {
            let entry = Entry { kind: EntryKind::Note, page: *page, at: *at, editing: Some(id) };
            let text = text.clone();
            self.open_entry(entry, text);
        }
    }

    fn date_text(&self) -> String {
        let mut text = String::new();
        // `format` fails on a bad pattern, and `to_string` would panic on that.
        if write!(text, "{}", chrono::Local::now().format(&self.config.hotbar.date_format)).is_err() {
            text = chrono::Local::now().format("%Y-%m-%d").to_string();
        }
        text
    }

    fn add_mark(&mut self, page: usize, at: [f32; 2], mark: MarkKind) {
        let id = self.side.take_id();
        self.add_annotations(vec![Annotation::Mark { id, page, at, size: self.fill_size, mark }]);
    }

    fn add_text(&mut self, page: usize, at: [f32; 2], text: String) {
        let id = self.side.take_id();
        self.add_annotations(vec![Annotation::Text { id, page, at, size: self.fill_size, text }]);
    }

    /// Signs at the place, or asks for a signature first.
    fn add_signature(&mut self, page: usize, at: [f32; 2]) {
        let Some(signature) = self.signature.as_ref() else {
            self.mode = Mode::Sign(SignPad { place: Some((page, at)), ..Default::default() });
            return;
        };
        // A signature runs about twelve times as wide as the text beside it is tall.
        let strokes = store::place_signature(signature, at, self.fill_size * 12.5);
        let (id, width) = (self.side.take_id(), self.fill_size / 10.0);
        self.add_annotations(vec![Annotation::Signature { id, page, strokes, width }]);
    }

    /// Text starts level with the place it was put at, rather than below it.
    fn text_top(&self, at: [f32; 2]) -> [f32; 2] {
        [at[0], at[1] - self.fill_size * 0.6]
    }

    fn note_at(&self, page: usize, at: [f32; 2]) -> Option<u64> {
        let is_note = |a: &&Annotation| matches!(a, Annotation::Note { .. }) && a.page() == page && a.hit(at, 0.0);
        self.side.annotations.iter().rfind(is_note).map(Annotation::id)
    }

    /// What the tool in hand does at a place on a page.
    pub fn place_at(&mut self, page: usize, at: [f32; 2]) {
        match (self.tool, self.fill) {
            (Some(Tool::Note), _) => match self.note_at(page, at) {
                Some(id) => self.edit_note(id),
                None => {
                    let corner = [at[0] - NOTE_SIDE / 2.0, at[1] - NOTE_SIDE / 2.0];
                    self.open_entry(Entry { kind: EntryKind::Note, page, at: corner, editing: None }, String::new());
                }
            },
            (Some(Tool::Fill), FillItem::Text) => {
                let top = self.text_top(at);
                self.open_entry(Entry { kind: EntryKind::Text, page, at: top, editing: None }, String::new());
            }
            (Some(Tool::Fill), FillItem::Date) => self.add_text(page, self.text_top(at), self.date_text()),
            (Some(Tool::Fill), FillItem::Check) => self.add_mark(page, at, MarkKind::Check),
            (Some(Tool::Fill), FillItem::Cross) => self.add_mark(page, at, MarkKind::Cross),
            (Some(Tool::Fill), FillItem::Dot) => self.add_mark(page, at, MarkKind::Dot),
            (Some(Tool::Fill), FillItem::Line) => self.add_mark(page, at, MarkKind::Line),
            (Some(Tool::Fill), FillItem::Sign) => self.add_signature(page, at),
            _ => {}
        }
    }

    /// Makes what was typed in the bar.
    pub fn entry_done(&mut self, entry: Entry, text: &str) {
        let text = text.trim().to_owned();
        if text.is_empty() {
            return;
        }
        let Entry { kind, page, at, editing } = entry;
        match (kind, editing) {
            (EntryKind::Note, Some(id)) => self.replace_note(id, text),
            (EntryKind::Note, None) => {
                let (id, color) = (self.side.take_id(), self.current_color());
                self.add_annotations(vec![Annotation::Note { id, page, at, color, text }]);
            }
            (EntryKind::Text, _) => self.add_text(page, at, text),
        }
    }

    fn replace_note(&mut self, id: u64, new: String) {
        let Some(slot) = self.side.annotations.iter_mut().find(|a| a.id() == id) else { return };
        let before = slot.clone();
        if let Annotation::Note { text, .. } = slot {
            *text = new;
        }
        let after = slot.clone();
        if before != after {
            self.undo.push(Edit::Replaced { before, after });
            self.redo.clear();
            self.mark_dirty();
        }
    }

    /// Keeps a drawn signature, and signs where it was asked for.
    pub fn sign_done(&mut self, pad: &SignPad) {
        self.mode = Mode::Normal;
        let Some(signature) = store::normalize_signature(&pad.strokes) else {
            self.say("nothing drawn");
            return;
        };
        if let Err(e) = store::save_signature(&store::data_dir(), &signature) {
            self.say(format!("couldn't keep the signature: {e}"));
        }
        self.signature = Some(signature);
        if let Some((page, at)) = pad.place {
            self.add_signature(page, at);
        }
    }

    fn command_export(&mut self, argument: &str, force: bool) {
        let path = match argument {
            "" => crate::export::default_path(&self.doc.path),
            given => expand_home(given),
        };
        if let Some(refusal) = crate::export::refusal(&self.doc.path, &path, force) {
            self.say(refusal);
            return;
        }
        let (bytes, pages) = (self.doc.bytes.clone(), &self.doc.info.pages);
        let result = crate::export::export(bytes, self.doc.password(), pages, &self.side.annotations, &self.side.form, &path);
        self.say(match result {
            Ok(report) => exported_message(self.side.annotations.len(), report, &path),
            Err(e) => format!("export failed: {e}"),
        });
    }

    // Smart jump.

    /// Follows a reference, once the text of every page is in.
    pub fn smart_follow(&mut self, reference: Reference, landing: Landing) {
        self.say(format!("looking for {}", reference.describe()));
        self.smart = Some((reference, landing));
    }

    /// Looks up the waiting reference if the text has arrived.
    pub fn pump_smart(&mut self) {
        if self.smart.is_none() || self.doc.text.iter().any(Option::is_none) {
            return;
        }
        let Some((reference, landing)) = self.smart.take() else { return };
        let pages: Vec<Vec<Line>> = self.doc.text.iter().map(|t| t.as_ref().map(|t| lines_of(t)).unwrap_or_default()).collect();
        let places = find_definitions(&reference, &pages);
        let Some((page, y)) = places.first().copied() else {
            self.say(format!("nothing found for {}", reference.describe()));
            return;
        };
        self.say(reference.describe());
        match landing {
            // A little above it, with a jump back to here.
            Landing::Jump => self.jump_to_point(Anchor { page, y: y - 12.0, x: self.view.x }),
            Landing::Overview => self.open_overview(reference.describe(), places),
        }
    }

    /// Where a link points, if that is a place in the document.
    fn link_place(&self, index: usize) -> Option<(usize, f32)> {
        let link = self.doc.info.links.get(index)?;
        let LinkTarget::Page(page, dest) = &link.target else { return None };
        Some((*page, dest_y(self.doc.info.pages.get(*page)?, *dest)))
    }

    /// Shows where a link points in the overview.
    fn overview_link(&mut self, index: usize) {
        match self.link_place(index) {
            Some(place) => self.open_overview("link".to_owned(), vec![place]),
            None => self.say("this link doesn't lead to a place in the document"),
        }
    }

    /// What a middle or right click does: follows a link, or else the reference written there.
    pub fn follow_at(&mut self, ctx: &egui::Context, p: Pos2, landing: Landing) {
        let Some((page, at)) = self.view.page_under(&self.doc.layout, p) else { return };
        if let Some(link) = self.link_at(page, at) {
            match landing {
                Landing::Jump => self.follow_link(ctx, link),
                Landing::Overview => self.overview_link(link),
            }
            return;
        }
        let Some(text) = self.doc.text.get(page).and_then(|t| t.clone()) else { return };
        let glyph = text.nearest(at[0], at[1]).filter(|g| text.glyphs.get(*g).is_some_and(|g| touches(g.rect, at)));
        let Some(glyph) = glyph else {
            self.say("no reference here");
            return;
        };
        let lines = lines_of(&text);
        let found = lines.iter().find_map(|line| {
            let at = line.glyphs.iter().position(|g| *g == glyph)?;
            references_in(&line.text).into_iter().find(|f| f.range.contains(&at))
        });
        match found {
            Some(found) => self.smart_follow(found.reference, landing),
            None => self.say("no reference here"),
        }
    }

    // Portals.

    /// The portal whose source is nearest the middle of the view.
    pub fn closest_portal(&self) -> Option<&Portal> {
        let layout = &self.doc.layout;
        let centre = self.view.y + self.view.rect.height() / self.view.zoom / 2.0;
        let distance = |p: &&Portal| (layout.tops.get(p.source.page).copied().unwrap_or(0.0) + p.source.y - centre).abs();
        self.side.portals.iter().min_by(|a, b| distance(a).total_cmp(&distance(b)))
    }

    /// The middle of the view: where a portal starts, and what it points at.
    fn portal_anchor(&self) -> Anchor {
        let layout = &self.doc.layout;
        let middle = self.view.y + self.view.rect.height() / self.view.zoom / 2.0;
        let page = layout.page_at(middle.max(0.0));
        Anchor { page, y: middle - layout.tops[page], x: self.view.x }
    }

    /// The first press starts a portal here; the second ends it at the place reached, and goes
    /// back to the source with the target showing beside it.
    fn portal_key(&mut self) {
        let Some(pending) = self.pending_portal.take() else {
            let source = self.portal_anchor();
            self.pending_portal = Some(PendingPortal { source, replacing: None });
            return;
        };
        let target = self.view.anchor(&self.doc.layout);
        match pending.replacing.and_then(|id| self.side.portals.iter_mut().find(|p| p.id == id)) {
            Some(portal) => portal.target = target,
            None => {
                let id = self.side.take_id();
                self.side.portals.push(Portal { id, source: pending.source, target });
            }
        }
        self.view.go_to(&self.doc.layout, top_of_view(self, pending.source));
        self.portal_window = true;
        self.mark_dirty();
        self.say("portal made");
    }

    fn run_portal(&mut self, action: Action) {
        match action {
            Action::Portal => self.portal_key(),
            Action::GotoPortal => match self.closest_portal().map(|p| p.target) {
                Some(target) => self.jump_to(target),
                None => self.say("no portals"),
            },
            Action::DeletePortal => match self.closest_portal().map(|p| p.id) {
                Some(id) => {
                    self.side.portals.retain(|p| p.id != id);
                    self.mark_dirty();
                    self.say("portal deleted");
                }
                None => self.say("no portals"),
            },
            Action::EditPortal => match self.closest_portal().cloned() {
                Some(portal) => {
                    let source = self.portal_anchor();
                    self.pending_portal = Some(PendingPortal { source, replacing: Some(portal.id) });
                    self.view.go_to(&self.doc.layout, portal.target);
                }
                None => self.say("no portals"),
            },
            Action::TogglePortalWindow => {
                self.portal_window = !self.portal_window;
                if self.portal_window && self.side.portals.is_empty() {
                    self.say("no portals yet: press p, go to the target, press p");
                }
            }
            _ => {}
        }
    }

    fn run_toggle(&mut self, ctx: &egui::Context, action: Action) {
        match action {
            Action::ToggleStatusbar => self.statusbar_visible = !self.statusbar_visible,
            Action::ToggleHotbar => self.hotbar.toggle(self.tool.is_some()),
            Action::ToggleTheme => self.next_theme(),
            Action::ToggleLightDark => self.toggle_light_dark(),
            Action::ToggleLinks => self.links_shown = !self.links_shown,
            Action::ToggleFullscreen => {
                self.fullscreen = !self.fullscreen;
                ctx.send_viewport_cmd_to(egui::ViewportId::ROOT, egui::ViewportCommand::Fullscreen(self.fullscreen));
            }
            Action::Reload => {
                let path = self.doc.path.clone();
                match self.open(ctx, &path) {
                    Ok(()) => self.say("reloaded"),
                    Err(e) => self.say(format!("reload failed: {e}")),
                }
            }
            Action::Yank => self.yank(ctx),
            _ => {}
        }
    }

    fn set_theme(&mut self, name: &str) {
        self.config.theme = name.to_owned();
        self.doc.apply_theme(self.config.active_theme());
        self.reset_rendering();
    }

    fn toggle_light_dark(&mut self) {
        let next = if self.config.theme == "light" { "dark" } else { "light" };
        if self.config.themes.contains_key(next) {
            self.set_theme(next);
        } else {
            self.say(format!("there is no theme called {next}"));
        }
    }

    fn next_theme(&mut self) {
        let names: Vec<String> = self.config.themes.keys().cloned().collect();
        let at = names.iter().position(|n| *n == self.config.theme).unwrap_or(0);
        let next = names[(at + 1) % names.len()].clone();
        self.set_theme(&next);
        self.say(format!("theme {next}"));
    }

    // Hints.

    /// What hints are shown on, in page points: each page showing and its visible part.
    fn visible_areas(&self) -> Vec<(usize, [f32; 4])> {
        let layout = &self.doc.layout;
        let rect = self.view.rect;
        self.view
            .visible_pages(layout)
            .map(|page| {
                let t = self.view.transform(layout, page);
                let (a, b) = (t.to_page(rect.min), t.to_page(rect.max));
                (page, [a[0], a[1], b[0], b[1]])
            })
            .collect()
    }

    fn link_targets(&self, areas: &[(usize, [f32; 4])]) -> Vec<Target> {
        let info = &self.doc.info;
        let mut out = Vec::new();
        for (index, link) in info.links.iter().enumerate() {
            let Some((_, area)) = areas.iter().find(|(page, _)| *page == link.page) else { continue };
            let Some(page) = info.pages.get(link.page) else { continue };
            let (a, b) = (page.user_to_view(link.rect[0], link.rect[1]), page.user_to_view(link.rect[2], link.rect[3]));
            let rect = [a[0].min(b[0]), a[1].min(b[1]), a[0].max(b[0]), a[1].max(b[1])];
            if hint::intersects(rect, *area) {
                out.push(Target { label: String::new(), page: link.page, rect, kind: TargetKind::Link(index) });
            }
        }
        out
    }

    fn annotation_targets(&self, areas: &[(usize, [f32; 4])]) -> Vec<Target> {
        let shown = |a: &&Annotation| areas.iter().any(|(page, area)| *page == a.page() && hint::intersects(a.bounds(), *area));
        let target = |a: &Annotation| Target { label: String::new(), page: a.page(), rect: a.bounds(), kind: TargetKind::Annotation(a.id()) };
        self.side.annotations.iter().filter(shown).map(target).collect()
    }

    fn word_targets(&self, areas: &[(usize, [f32; 4])]) -> Vec<Target> {
        let text_of = |&(page, area): &(usize, [f32; 4])| self.doc.text[page].as_ref().map(|t| hint::word_targets(t, page, area));
        areas.iter().filter_map(text_of).flatten().collect()
    }

    /// Where a tool can put things: by the words, and on the notes it can edit.
    fn place_targets(&self, areas: &[(usize, [f32; 4])]) -> Vec<Target> {
        let mut targets = self.word_targets(areas);
        if self.tool == Some(Tool::Note) {
            let notes = self.annotation_targets(areas).into_iter();
            let is_note = |t: &Target| matches!(t.kind, TargetKind::Annotation(id) if self.side.annotations.iter().any(|a| a.id() == id && matches!(a, Annotation::Note { .. })));
            targets.extend(notes.filter(is_note));
        }
        targets
    }

    /// The references written on the pages showing, and the links to places in the document. A
    /// reference that is a link already goes by the link, which needs no guessing.
    fn smart_targets(&self, areas: &[(usize, [f32; 4])]) -> (Vec<Target>, Vec<Reference>) {
        let is_internal =
            |t: &Target| matches!(t.kind, TargetKind::Link(i) if matches!(self.doc.info.links.get(i).map(|l| &l.target), Some(LinkTarget::Page(..))));
        let mut targets: Vec<Target> = self.link_targets(areas).into_iter().filter(is_internal).collect();
        let links = targets.clone();
        let mut references = Vec::new();
        for (page, area) in areas {
            let Some(text) = self.doc.text[*page].as_ref() else { continue };
            for line in lines_of(text) {
                for found in references_in(&line.text) {
                    let glyphs = found.range.clone().filter_map(|i| line.glyphs.get(i)).filter_map(|g| text.glyphs.get(*g));
                    let Some(rect) = glyphs.map(|g| g.rect).reduce(hint::union) else { continue };
                    if hint::intersects(rect, *area) && !links.iter().any(|l| l.page == *page && hint::intersects(l.rect, rect)) {
                        targets.push(Target { label: String::new(), page: *page, rect, kind: TargetKind::Reference(references.len()) });
                        references.push(found.reference);
                    }
                }
            }
        }
        (targets, references)
    }

    /// Labels the targets once what they need has arrived.
    pub fn prepare_hints(&mut self) {
        let Mode::Hint(state) = &self.mode else { return };
        if state.ready {
            return;
        }
        let purpose = state.purpose;
        let areas = self.visible_areas();
        let needs_text = matches!(purpose, Purpose::Select | Purpose::Place | Purpose::Smart | Purpose::Overview);
        // The form is read in the background: the labels wait for it.
        if (purpose == Purpose::Form && !self.doc.form.ready) || needs_text && areas.iter().any(|(page, _)| self.doc.text[*page].is_none()) {
            return;
        }
        let targets = match purpose {
            Purpose::Select => self.word_targets(&areas),
            Purpose::Place => self.place_targets(&areas),
            Purpose::Link => self.link_targets(&areas),
            Purpose::Form => self.form_targets(&areas),
            Purpose::DeleteAnnotation => self.annotation_targets(&areas),
            Purpose::Smart | Purpose::Overview => {
                let (targets, references) = self.smart_targets(&areas);
                self.hint_references = references;
                targets
            }
        };
        if targets.is_empty() {
            self.mode = Mode::Normal;
            self.say(match purpose {
                Purpose::Select | Purpose::Place => "no text here",
                Purpose::Link => "no links here",
                Purpose::Form => "no form fields here",
                Purpose::DeleteAnnotation => "no annotations here",
                Purpose::Smart | Purpose::Overview => "no references here",
            });
            return;
        }
        let alphabet: Vec<char> = self.config.hints.alphabet.chars().collect();
        if let Mode::Hint(state) = &mut self.mode {
            state.assign(targets, &alphabet);
        }
    }

    /// Ends a selection at the word given, or at the first word again.
    pub fn finish_selection(&mut self, end: (usize, usize, usize)) {
        let Mode::Hint(state) = &self.mode else { return };
        let Some(start) = state.start else { return };
        let (first, last) = if (end.0, end.1) < (start.0, start.1) { (end, start) } else { (start, end) };
        self.selection = Some(Selection::new((first.0, first.1), (last.0, last.2)));
        self.mode = Mode::Normal;
        if self.tool == Some(Tool::Highlight) {
            self.highlight_selection();
        }
    }

    /// Acts on a chosen label.
    pub fn hint_chosen(&mut self, ctx: &egui::Context, target: &Target) {
        let purpose = if let Mode::Hint(state) = &self.mode { state.purpose } else { Purpose::Select };
        let placing = purpose == Purpose::Place;
        let landing = if purpose == Purpose::Overview { Landing::Overview } else { Landing::Jump };
        match target.kind {
            TargetKind::Link(index) if landing == Landing::Overview => {
                self.mode = Mode::Normal;
                self.overview_link(index);
            }
            TargetKind::Word(..) if placing => {
                self.mode = Mode::Normal;
                // Just after the word: in a form, that is the blank it labels.
                let [_, top, right, bottom] = target.rect;
                self.place_at(target.page, [right + 4.0, (top + bottom) / 2.0]);
            }
            TargetKind::Annotation(id) if placing => {
                self.mode = Mode::Normal;
                self.edit_note(id);
            }
            TargetKind::Reference(index) => {
                self.mode = Mode::Normal;
                if let Some(reference) = self.hint_references.get(index).cloned() {
                    self.smart_follow(reference, landing);
                }
            }
            TargetKind::Word(first, last) => {
                let picked = (target.page, first, last);
                let start = if let Mode::Hint(state) = &mut self.mode {
                    state.typed.clear();
                    state.start
                } else {
                    None
                };
                if start.is_some() {
                    self.finish_selection(picked);
                } else if let Mode::Hint(state) = &mut self.mode {
                    state.start = Some(picked);
                }
            }
            TargetKind::Link(index) => {
                self.mode = Mode::Normal;
                self.follow_link(ctx, index);
            }
            TargetKind::Annotation(id) => {
                self.mode = Mode::Normal;
                self.remove_annotation(id);
            }
            TargetKind::Widget(field, widget) => {
                self.mode = Mode::Normal;
                self.form_activate(field, widget);
            }
        }
    }

    pub fn follow_link(&mut self, _ctx: &egui::Context, index: usize) {
        let Some(link) = self.doc.info.links.get(index) else { return };
        match &link.target {
            LinkTarget::Page(page, dest) => {
                let (page, dest) = (*page, *dest);
                let Some(info) = self.doc.info.pages.get(page) else { return };
                let point = dest_point(info, dest);
                let anchor = Anchor { page, y: point.unwrap_or(0.0), x: self.view.x };
                if point.is_some() { self.jump_to_point(anchor) } else { self.jump_to(anchor) }
            }
            LinkTarget::Uri(uri) => {
                let uri = uri.clone();
                self.open_uri(&uri);
            }
            _ => self.say("this kind of link isn't supported"),
        }
    }

    pub fn open_uri(&mut self, uri: &str) {
        let lower = uri.to_lowercase();
        if !["http://", "https://", "mailto:"].iter().any(|s| lower.starts_with(s)) {
            self.say(format!("not opening {uri}"));
            return;
        }
        // The URI is one argument, never parsed by a shell.
        #[cfg(windows)]
        const OPENER: &str = "explorer.exe";
        #[cfg(target_os = "macos")]
        const OPENER: &str = "open";
        #[cfg(not(any(windows, target_os = "macos")))]
        const OPENER: &str = "xdg-open";
        match std::process::Command::new(OPENER).arg(uri).spawn() {
            Ok(mut child) => {
                std::thread::spawn(move || child.wait());
                self.say(format!("opened {uri}"));
            }
            Err(e) => self.say(format!("{OPENER}: {e}")),
        }
    }

    // The command line.

    /// Runs what was typed after `:`.
    pub fn command(&mut self, ctx: &egui::Context, line: &str) {
        let line = line.trim();
        let (name, argument) = line.split_once(char::is_whitespace).map_or((line, ""), |(n, a)| (n, a.trim()));
        if let Ok(page) = name.parse::<usize>() {
            self.jump_to_page(page.saturating_sub(1));
            return;
        }
        match name {
            "" => {}
            "q" | "quit" | "q!" | "wq" | "x" => ctx.send_viewport_cmd_to(egui::ViewportId::ROOT, egui::ViewportCommand::Close),
            "w" | "write" => {
                self.save();
                self.say("saved");
            }
            "open" if argument.is_empty() => self.choose_file(ctx),
            "e" | "edit" | "open" => self.command_open(ctx, argument),
            "noh" | "nohlsearch" => self.search = None,
            "theme" => self.command_theme(argument),
            "zoom" => self.command_zoom(argument),
            "reload" => self.run(ctx, Action::Reload, None),
            "toc" => self.open_toc(),
            "hotbar" => self.hotbar.toggle(self.tool.is_some()),
            "export" => self.command_export(argument, false),
            "export!" => self.command_export(argument, true),
            "clearform" => self.clear_form(),
            "sign" => self.mode = Mode::Sign(SignPad::default()),
            "marks" => {
                let marks: String = self.side.marks.keys().collect();
                self.say(if marks.is_empty() { "no marks".to_owned() } else { format!("marks: {marks}") });
            }
            // An action by its name, as under `[keys]`. The commands above come first.
            other => match Action::from_name(other).filter(|a| a.is_listed()) {
                Some(action) => self.command_action(ctx, action, argument),
                None => self.say(format!("unknown command: {other}")),
            },
        }
    }

    /// An action named in a command. The one argument it may have is a count, as when typed before
    /// the keys of the action.
    fn command_action(&mut self, ctx: &egui::Context, action: Action, argument: &str) {
        if argument.is_empty() {
            self.run(ctx, action, None);
        } else if let Some(count) = count_argument(argument) {
            self.run(ctx, action, Some(count));
        } else {
            self.say(format!("{} takes no argument (a count is allowed)", action.name()));
        }
    }

    fn command_open(&mut self, ctx: &egui::Context, argument: &str) {
        if argument.is_empty() {
            self.say("usage: :e <file>");
            return;
        }
        let path = expand_home(argument);
        let path = std::fs::canonicalize(&path).map_or(path, crate::strip_verbatim);
        if let Err(e) = self.open(ctx, &path) {
            self.say(e);
        }
    }

    /// Asks the desktop's file chooser for a document, through its portal when it has one. That
    /// can stay open for as long as it likes, so it has a thread of its own.
    fn choose_file(&mut self, ctx: &egui::Context) {
        if self.chooser.is_some() {
            self.say("the file chooser is already open");
            return;
        }
        let (send, receive) = std::sync::mpsc::channel();
        let folder = self.doc.path.parent().map(Path::to_owned);
        let repaint = ctx.clone();
        let choose = move || {
            let dialog = rfd::FileDialog::new().set_title("Open a PDF").add_filter("PDF", &["pdf", "PDF"]);
            let dialog = match folder {
                Some(folder) => dialog.set_directory(folder),
                None => dialog,
            };
            // The viewer may have closed by now, with nothing left to tell.
            let _ = send.send(dialog.pick_file());
            repaint.request_repaint();
        };
        match std::thread::Builder::new().name("file chooser".to_owned()).spawn(choose) {
            Ok(_) => self.chooser = Some(receive),
            Err(e) => self.say(format!("file chooser: {e}")),
        }
    }

    /// Opens what the file chooser picked, once it has. It doesn't say whether it was cancelled or
    /// couldn't open at all, so neither does this.
    pub fn pump_chooser(&mut self, ctx: &egui::Context) {
        let Some(chooser) = &self.chooser else { return };
        let answer = match chooser.try_recv() {
            Ok(answer) => answer,
            Err(TryRecvError::Empty) => return,
            Err(TryRecvError::Disconnected) => {
                self.chooser = None;
                self.say("the file chooser failed");
                return;
            }
        };
        self.chooser = None;
        match answer {
            Some(path) => {
                if let Err(e) = self.open(ctx, &path) {
                    self.say(e);
                }
            }
            None => self.say("no file chosen"),
        }
    }

    fn command_theme(&mut self, name: &str) {
        if self.config.themes.contains_key(name) {
            self.set_theme(name);
        } else {
            let names: Vec<&str> = self.config.themes.keys().map(String::as_str).collect();
            self.say(format!("themes: {}", names.join(", ")));
        }
    }

    fn command_zoom(&mut self, argument: &str) {
        match argument.trim_end_matches('%').parse::<f32>() {
            Ok(percent) if percent > 0.0 => self.zoom_by(percent / 100.0 / self.view.zoom),
            _ => self.say("usage: :zoom <percent>"),
        }
    }
}

/// What an export says for itself.
fn exported_message(annotations: usize, report: Report, path: &Path) -> String {
    let filled = if report.filled > 0 { format!(" and {} filled-in fields", report.filled) } else { String::new() };
    let skipped = if report.skipped > 0 { format!(" ({} fields couldn't be filled in)", report.skipped) } else { String::new() };
    let recalc = report.recalc.map_or_else(String::new, |why| format!(" (the calculated fields couldn't be recalculated: {why})"));
    format!("exported {annotations} annotations{filled} to {}{skipped}{recalc}", path.display())
}

/// A source anchor marks the middle of the view: the top of the view that puts it there.
fn top_of_view(app: &App, source: Anchor) -> Anchor {
    let span = app.view.rect.height() / app.view.zoom;
    Anchor { y: view_top(source.y, 50.0, span), ..source }
}

/// The top of a view `span` points tall that puts `y` at `percent` of its height: 0 is the bottom edge, 100 the top.
fn view_top(y: f32, percent: f32, span: f32) -> f32 {
    y - (1.0 - percent / 100.0) * span
}

/// The count in `:zoom_in 3`: a positive whole number, saturating when it is too big for one.
fn count_argument(argument: &str) -> Option<u32> {
    let digits = !argument.is_empty() && argument.bytes().all(|b| b.is_ascii_digit());
    let count = if digits { argument.parse().unwrap_or(u32::MAX) } else { 0 };
    (count > 0).then_some(count)
}

#[cfg(test)]
mod tests {
    use super::{count_argument, view_top};
    use crate::view::{Anchor, Layout, View};
    use egui::{Rect, pos2, vec2};

    #[test]
    fn a_jump_puts_its_target_at_the_asked_height() {
        assert_eq!(view_top(500.0, 100.0, 200.0), 500.0);
        assert_eq!(view_top(500.0, 50.0, 200.0), 400.0);
        assert_eq!(view_top(500.0, 0.0, 200.0), 300.0);
        assert_eq!(view_top(500.0, 25.0, 200.0), 350.0);
    }

    #[test]
    fn an_action_takes_a_positive_whole_number_as_its_count() {
        assert_eq!(count_argument("3"), Some(3));
        assert_eq!(count_argument("007"), Some(7));
        assert_eq!(count_argument("99999999999999999999"), Some(u32::MAX));
        for bad in ["", "0", "00", "-1", "+1", "1.5", "x", "3 4", "٣"] {
            assert_eq!(count_argument(bad), None, "{bad:?}");
        }
    }

    #[test]
    fn view_top_edges_fractions_and_huge_spans() {
        assert_eq!(view_top(500.0, 0.0, 200.0), 300.0);
        assert_eq!(view_top(500.0, 50.0, 200.0), 400.0);
        assert_eq!(view_top(500.0, 100.0, 200.0), 500.0);
        assert!((view_top(500.0, 33.3, 300.0) - (500.0 - 0.667 * 300.0)).abs() < 1e-3);
        assert!((view_top(10.0, 0.1, 1000.0) - (10.0 - 0.999 * 1000.0)).abs() < 1e-2);
        assert_eq!(view_top(500.0, 50.0, 0.0), 500.0);
        assert_eq!(view_top(0.0, 50.0, 1e30), -5e29);
        let huge = view_top(1.0, 0.0, f32::MAX);
        assert!(huge.is_finite() && huge < 0.0, "{huge}");
        assert_eq!(view_top(1.0, 100.0, f32::MAX), 1.0);
    }

    /// The jump path: `jump_to_point`'s anchor, then `View::go_to` with its clamping.
    fn landed(layout: &Layout, page: usize, y: f32, percent: f32, window: f32) -> f32 {
        let mut view = View::new();
        view.rect = Rect::from_min_size(pos2(0.0, 0.0), vec2(600.0, window));
        let span = view.rect.height() / view.zoom;
        view.go_to(layout, Anchor { page, y: view_top(y, percent, span), x: 0.0 });
        view.y
    }

    #[test]
    fn a_jump_through_go_to_centres_clamps_at_the_ends() {
        let layout = Layout::new(vec![[600.0, 800.0]; 5], 10.0);
        // 5 pages: tops 0, 810, 1620, 2430, 3240; total 4040.
        assert_eq!(layout.total, 4040.0);
        // Mid-document: page 2, y 100 -> centred in an 800 window: top = 1620+100-400.
        assert_eq!(landed(&layout, 2, 100.0, 50.0, 800.0), 1320.0);
        assert_eq!(landed(&layout, 2, 100.0, 100.0, 800.0), 1720.0);
        assert_eq!(landed(&layout, 2, 100.0, 0.0, 800.0), 920.0);
        // Near the start: would be negative, clamped to 0.
        assert_eq!(landed(&layout, 0, 100.0, 50.0, 800.0), 0.0);
        // Near the end: clamped to total - window.
        assert_eq!(landed(&layout, 4, 750.0, 50.0, 800.0), 3240.0);
        // A negative anchor on a later page reaches into the previous one rather than failing.
        assert_eq!(landed(&layout, 1, 50.0, 50.0, 800.0), 810.0 + 50.0 - 400.0);
        // A document shorter than the window is centred whatever the percent.
        let short = Layout::new(vec![[600.0, 300.0]], 10.0);
        assert_eq!(landed(&short, 0, 100.0, 50.0, 800.0), -250.0);
    }
}
