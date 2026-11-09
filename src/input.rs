//! Keyboard and pointer input.
use egui::{CursorIcon, Event, MouseWheelUnit, PointerButton, Pos2, Response, Sense};

use crate::app::{App, Bar, BarKind, Landing, Mode, Selection, TocState, Tool};
use crate::field::{Edit, Field};
use crate::keymap::Stroke;
use crate::search::Search;
use crate::view::Anchor;

/// The characters a label or a mark can be typed with.
fn typed_char(stroke: &Stroke) -> Option<char> {
    stroke.text().filter(|c| *c != ' ')
}

/// The stroke behind a clipboard event: the window system's layer turns control-c and control-x
/// into these, and no key event comes with them.
fn clipboard_stroke(event: &Event) -> Option<Stroke> {
    let key = match event {
        Event::Copy => "c",
        Event::Cut => "x",
        _ => return None,
    };
    Some(Stroke { ctrl: true, ..Stroke::plain(key) })
}

impl App {
    pub fn keys(&mut self, ctx: &egui::Context, events: &[Event]) {
        // A chord, and shift-space, also arrive as text: only the key event counts.
        let mut skip_text = false;
        for event in events {
            match event {
                Event::Key { key, pressed: true, modifiers, .. } => {
                    let stroke = Stroke::from_key(*key, *modifiers);
                    skip_text = modifiers.alt || stroke.as_ref().is_some_and(|s| s.key == "space");
                    if let Some(stroke) = stroke {
                        self.on_stroke(ctx, stroke);
                    }
                }
                Event::Text(text) if !skip_text => {
                    for c in text.chars() {
                        self.on_stroke(ctx, Stroke::from_char(c));
                    }
                }
                Event::Paste(text) => self.paste(text),
                other => {
                    if let Some(stroke) = clipboard_stroke(other) {
                        self.on_stroke(ctx, stroke);
                    }
                }
            }
        }
    }

    fn on_stroke(&mut self, ctx: &egui::Context, stroke: Stroke) {
        match &self.mode {
            Mode::Normal => self.normal_key(ctx, stroke),
            Mode::Bar(_) => self.bar_key(ctx, &stroke),
            Mode::Hint(_) => self.hint_key(ctx, &stroke),
            Mode::Toc(_) => self.toc_key(&stroke),
            Mode::Sign(_) => self.sign_key(&stroke),
            Mode::AwaitMark(op) => {
                let op = *op;
                self.mode = Mode::Normal;
                if let Some(c) = typed_char(&stroke) {
                    self.mark_key(op, c);
                }
            }
        }
    }

    fn normal_key(&mut self, ctx: &egui::Context, stroke: Stroke) {
        if self.keymap.cancels(&stroke) {
            self.escape();
            return;
        }
        // With a tool in hand, digits pick its colour.
        if self.tool.is_some() {
            let colors = self.config.hotbar.colors.len() as u32;
            if let Some(digit) = stroke.text().and_then(|c| c.to_digit(10)).filter(|d| (1..=colors).contains(d)) {
                self.color = digit as usize - 1;
                return;
            }
        }
        if let Some((action, count)) = self.pending.feed(&self.keymap, stroke) {
            self.run(ctx, action, count);
        }
    }

    /// Cancel takes back one thing at a time, most recent first.
    pub fn escape(&mut self) {
        if !self.pending.display().is_empty() {
            self.pending.clear();
        } else if self.overview.take().is_some() {
            // That is all it takes back.
        } else if self.smart.take().is_some() {
            self.say("jump cancelled");
        } else if std::mem::take(&mut self.toc_waiting) {
            // The contents are still read, for `{chapter}`, but don't open.
        } else if self.pending_portal.take().is_some() {
            self.say("portal cancelled");
        } else if self.stroke.is_some() || self.selection.is_some() {
            self.stroke = None;
            self.selection = None;
        } else if self.tool.is_some() {
            self.set_tool(None);
        } else {
            self.search = None;
        }
    }

    // The bar.

    fn paste(&mut self, text: &str) {
        let Mode::Bar(bar) = &mut self.mode else { return };
        bar.field.insert(text.lines().next().unwrap_or(""));
        bar.completion = None;
        let kind = bar.kind;
        self.bar_changed(kind);
    }

    /// Restarts a search as its text changes, from where the bar opened.
    fn bar_changed(&mut self, kind: BarKind) {
        let BarKind::Search { forward } = kind else { return };
        let Mode::Bar(bar) = &self.mode else { return };
        let (text, origin) = (bar.field.text.clone(), bar.origin);
        self.view.go_to(&self.doc.layout, origin);
        self.search = if text.is_empty() {
            None
        } else {
            let pages = self.doc.layout.len();
            Some(Search::new(text, forward, self.config.view.smartcase, pages, (origin.page, origin.y)))
        };
    }

    fn bar_cancel(&mut self, bar: Bar) {
        if matches!(bar.kind, BarKind::Search { .. }) {
            self.view.go_to(&self.doc.layout, bar.origin);
            self.search = bar.previous;
        }
        self.mode = Mode::Normal;
    }

    fn bar_submit(&mut self, ctx: &egui::Context, bar: Bar) {
        self.mode = Mode::Normal;
        match bar.kind {
            BarKind::Command => self.command(ctx, &bar.field.text),
            BarKind::Entry(entry) => self.entry_done(entry, &bar.field.text),
            BarKind::Search { forward } => {
                // An empty search repeats the last one, as in vim.
                if bar.field.is_empty()
                    && let Some(mut previous) = bar.previous
                {
                    previous.forward = forward;
                    self.search = Some(previous);
                    self.run(ctx, crate::keymap::Action::SearchNext, None);
                    return;
                }
                let here = self.view.anchor(&self.doc.layout);
                self.view.go_to(&self.doc.layout, bar.origin);
                self.record_jump();
                self.view.go_to(&self.doc.layout, here);
                self.mark_dirty();
            }
        }
    }

    fn bar_key(&mut self, ctx: &egui::Context, stroke: &Stroke) {
        let Mode::Bar(bar) = &mut self.mode else { return };
        let kind = bar.kind;
        let tab = stroke.key == "tab" && !stroke.ctrl && !stroke.alt;
        if !tab {
            bar.completion = None;
        }
        if self.keymap.cancels(stroke) {
            let bar = bar.clone();
            self.bar_cancel(bar);
        } else if stroke.is("cr") {
            let bar = bar.clone();
            self.bar_submit(ctx, bar);
        } else if let Some(edit) = self.keymap.edit(stroke) {
            if edit == Edit::Backspace && bar.field.is_empty() {
                let bar = bar.clone();
                self.bar_cancel(bar);
            } else if bar.field.apply(edit) {
                self.bar_changed(kind);
            }
        } else if tab && kind == BarKind::Command {
            let themes: Vec<String> = self.config.themes.keys().cloned().collect();
            if let Some(message) = crate::complete::tab_field(&mut bar.field, &mut bar.completion, stroke.shift, &themes) {
                self.say(message);
            }
        } else if let Some(c) = stroke.text() {
            bar.field.insert(c.encode_utf8(&mut [0; 4]));
            self.bar_changed(kind);
        }
    }

    // The signature pad.

    fn sign_key(&mut self, stroke: &Stroke) {
        let Mode::Sign(pad) = &mut self.mode else { return };
        if self.keymap.cancels(stroke) {
            self.mode = Mode::Normal;
        } else if stroke.is("cr") {
            let pad = pad.clone();
            self.sign_done(&pad);
        } else if stroke.is("bs") {
            pad.strokes.clear();
        }
    }

    // Hints.

    fn hint_key(&mut self, ctx: &egui::Context, stroke: &Stroke) {
        let Mode::Hint(state) = &mut self.mode else { return };
        if self.keymap.cancels(stroke) {
            self.mode = Mode::Normal;
        } else if stroke.is("bs") {
            state.typed.pop();
        } else if stroke.is("cr") {
            // Enter after the first word selects just that word.
            if let Some(start) = state.start {
                self.finish_selection(start);
            }
        } else if let Some(c) = typed_char(stroke) {
            state.typed.push(c);
            if !state.matches() {
                state.typed.pop();
            } else if let Some(target) = state.chosen().cloned() {
                self.hint_chosen(ctx, &target);
            }
        }
    }

    // The table of contents.

    /// The entries the filter lets through, as indices.
    pub fn toc_rows(&self, state: &TocState) -> Vec<usize> {
        let filter = state.filter.text.to_lowercase();
        let shown = |(_, e): &(usize, &crate::doc::TocEntry)| e.title.to_lowercase().contains(&filter);
        self.doc.toc.iter().enumerate().filter(shown).map(|(i, _)| i).collect()
    }

    fn toc_go(&mut self, entry: usize) {
        self.mode = Mode::Normal;
        if let Some(e) = self.doc.toc.get(entry) {
            let anchor = Anchor { page: e.page, y: e.y, x: self.view.x };
            if e.point { self.jump_to_point(anchor) } else { self.jump_to(anchor) }
        }
    }

    fn toc_key(&mut self, stroke: &Stroke) {
        let Mode::Toc(state) = &self.mode else { return };
        let mut state = state.clone();
        let rows = self.toc_rows(&state);
        let at = rows.iter().position(|r| *r >= state.selected).unwrap_or(0);
        let last = rows.len().saturating_sub(1);
        let mut moved = None;
        if self.keymap.cancels(stroke) {
            if state.filtering {
                state.filtering = false;
                state.filter = Field::default();
            } else {
                self.mode = Mode::Normal;
                return;
            }
        } else if stroke.is("cr") {
            if let Some(row) = rows.get(at) {
                self.toc_go(*row);
            }
            return;
        } else if stroke.is_ctrl("n") || stroke.is("down") {
            moved = Some((at + 1).min(last));
        } else if stroke.is_ctrl("p") || stroke.is("up") {
            moved = Some(at.saturating_sub(1));
        } else if stroke.is_ctrl("d") {
            moved = Some((at + 10).min(last));
        } else if stroke.is_ctrl("u") {
            moved = Some(at.saturating_sub(10));
        } else if state.filtering {
            if let Some(edit) = self.keymap.edit(stroke) {
                state.filter.apply(edit);
            } else if let Some(c) = stroke.text() {
                state.filter.insert(c.encode_utf8(&mut [0; 4]));
            }
        } else {
            match stroke.text() {
                Some('j') => moved = Some((at + 1).min(last)),
                Some('k') => moved = Some(at.saturating_sub(1)),
                Some('g') => moved = Some(0),
                Some('G') => moved = Some(last),
                Some('/') => state.filtering = true,
                Some('l') => {
                    if let Some(row) = rows.get(at) {
                        self.toc_go(*row);
                    }
                    return;
                }
                Some('q') => {
                    self.mode = Mode::Normal;
                    return;
                }
                None if stroke.is("tab") => {
                    self.mode = Mode::Normal;
                    return;
                }
                _ => {}
            }
        }
        if let Some(row) = moved.and_then(|m| rows.get(m)) {
            state.selected = *row;
        }
        self.mode = Mode::Toc(state);
    }

    // The pointer.

    pub fn wheel(&mut self, ctx: &egui::Context, events: &[Event]) {
        for event in events {
            let Event::MouseWheel { unit, delta, modifiers, .. } = event else { continue };
            let pixels = match unit {
                MouseWheelUnit::Point => *delta,
                MouseWheelUnit::Line => *delta * self.config.view.scroll_step,
                MouseWheelUnit::Page => *delta * self.view.rect.height(),
            };
            let at = ctx.input(|i| i.pointer.hover_pos());
            if modifiers.ctrl {
                self.zoom_at((pixels.y / 200.0).exp(), at);
            } else if at.is_some_and(|p| self.over_overview(p)) {
                self.scroll_overview(-pixels.y);
            } else {
                let (dx, dy) = if modifiers.shift { (pixels.y, pixels.x) } else { (pixels.x, pixels.y) };
                self.view.x -= dx / self.view.zoom;
                self.view.y -= dy / self.view.zoom;
                self.view.clamp(&self.doc.layout);
                self.mark_dirty();
            }
            ctx.request_repaint();
        }
    }

    pub fn pointer(&mut self, ui: &egui::Ui) {
        let response = ui.interact(self.view.rect, ui.scope_id().with("view"), Sense::click_and_drag());
        self.hover = response.hover_pos();
        if matches!(self.mode, Mode::Sign(_)) {
            self.pointer_pad(ui.ctx(), &response);
            ui.ctx().set_cursor_icon(CursorIcon::Crosshair);
            return;
        }
        // A click beside the overview only closes it.
        if response.clicked() && self.overview.take().is_some() {
            return;
        }
        let landing = match (response.clicked_by(PointerButton::Middle), response.secondary_clicked()) {
            (true, _) => Some(Landing::Jump),
            (_, true) => Some(Landing::Overview),
            _ => None,
        };
        if let (Some(landing), Some(p)) = (landing, response.interact_pointer_pos()) {
            self.follow_at(ui.ctx(), p, landing);
        }
        match self.tool {
            Some(Tool::Draw) => self.pointer_draw(ui.ctx(), &response),
            Some(Tool::Erase) => self.pointer_erase(&response),
            Some(Tool::Note | Tool::Fill) => self.pointer_place(&response),
            _ => self.pointer_select(ui.ctx(), &response),
        }
        let icon = match self.tool {
            Some(Tool::Draw | Tool::Erase | Tool::Note | Tool::Fill) => CursorIcon::Crosshair,
            Some(Tool::Highlight) => CursorIcon::Text,
            None => self.hover_icon(&response),
        };
        ui.ctx().set_cursor_icon(icon);
    }

    pub fn link_at(&self, page: usize, at: [f32; 2]) -> Option<usize> {
        let info = self.doc.info.pages.get(page)?;
        self.doc.info.links.iter().position(|link| {
            let (a, b) = (info.user_to_view(link.rect[0], link.rect[1]), info.user_to_view(link.rect[2], link.rect[3]));
            let inside = (a[0].min(b[0])..=a[0].max(b[0])).contains(&at[0]) && (a[1].min(b[1])..=a[1].max(b[1])).contains(&at[1]);
            link.page == page && inside
        })
    }

    fn hover_icon(&self, response: &Response) -> CursorIcon {
        let over = response.hover_pos().and_then(|p| self.view.page_under(&self.doc.layout, p));
        match over {
            Some((page, at)) if self.link_at(page, at).is_some() => CursorIcon::PointingHand,
            _ => CursorIcon::Default,
        }
    }

    /// The glyph nearest a screen position.
    fn glyph_at(&self, p: egui::Pos2) -> Option<(usize, usize)> {
        let (page, at) = self.view.page_under(&self.doc.layout, p)?;
        let text = self.doc.text.get(page)?.as_ref()?;
        Some((page, text.nearest(at[0], at[1])?))
    }

    fn pointer_select(&mut self, ctx: &egui::Context, response: &Response) {
        if response.drag_started() {
            let origin = ctx.input(|i| i.pointer.press_origin());
            self.drag_anchor = origin.and_then(|p| self.glyph_at(p));
        }
        if response.dragged()
            && let (Some(anchor), Some(p)) = (self.drag_anchor, response.interact_pointer_pos())
            && let Some(glyph) = self.glyph_at(p)
        {
            self.selection = Some(Selection::new(anchor, glyph));
        }
        if response.drag_stopped() {
            self.drag_anchor = None;
            if self.tool == Some(Tool::Highlight) {
                self.highlight_selection();
            }
        }
        if response.clicked() {
            self.selection = None;
            let under = response.interact_pointer_pos().and_then(|p| self.view.page_under(&self.doc.layout, p));
            if let Some(link) = under.and_then(|(page, at)| self.link_at(page, at)) {
                self.follow_link(ctx, link);
            }
        }
    }

    fn pointer_draw(&mut self, ctx: &egui::Context, response: &Response) {
        if !response.is_pointer_button_down_on() {
            self.commit_stroke();
            return;
        }
        for p in pointer_path(ctx, response) {
            match &mut self.stroke {
                Some((page, points)) => {
                    let at = self.view.transform(&self.doc.layout, *page).to_page(p);
                    if points.last() != Some(&at) {
                        points.push(at);
                    }
                }
                None => {
                    if let Some((page, at)) = self.view.page_under(&self.doc.layout, p) {
                        self.stroke = Some((page, vec![at]));
                    }
                }
            }
        }
    }

    fn pointer_place(&mut self, response: &Response) {
        if !response.clicked() {
            return;
        }
        let under = response.interact_pointer_pos().and_then(|p| self.view.page_under(&self.doc.layout, p));
        if let Some((page, at)) = under {
            self.place_at(page, at);
        }
    }

    fn pointer_pad(&mut self, ctx: &egui::Context, response: &Response) {
        let rect = self.pad_rect();
        let Mode::Sign(pad) = &mut self.mode else { return };
        if !response.is_pointer_button_down_on() {
            pad.drawing = false;
            return;
        }
        for p in pointer_path(ctx, response) {
            if !rect.contains(p) {
                pad.drawing = false;
                continue;
            }
            if !pad.drawing {
                pad.drawing = true;
                pad.strokes.push(Vec::new());
            }
            let at = [(p.x - rect.left()) / rect.width(), (p.y - rect.top()) / rect.width()];
            if let Some(stroke) = pad.strokes.last_mut().filter(|s| s.last() != Some(&at)) {
                stroke.push(at);
            }
        }
    }

    fn pointer_erase(&mut self, response: &Response) {
        if !response.is_pointer_button_down_on() {
            return;
        }
        let under = response.interact_pointer_pos().and_then(|p| self.view.page_under(&self.doc.layout, p));
        let Some((page, at)) = under else { return };
        let reach = 4.0 / self.view.zoom;
        let hit = self.side.annotations.iter().rfind(|a| a.page() == page && a.hit(at, reach)).map(crate::store::Annotation::id);
        if let Some(id) = hit {
            self.remove_annotation(id);
        }
    }
}

/// Every position since the last frame, so fast strokes keep their shape.
fn pointer_path(ctx: &egui::Context, response: &Response) -> Vec<Pos2> {
    let moved = |e: &Event| if let Event::PointerMoved(p) = e { Some(*p) } else { None };
    let mut path: Vec<Pos2> = ctx.input(|i| i.events.iter().filter_map(moved).collect());
    path.extend(response.interact_pointer_pos());
    path
}

#[cfg(test)]
mod tests {
    use super::clipboard_stroke;
    use crate::keymap::parse_sequence;
    use egui::Event;

    #[test]
    fn control_c_reaches_the_bindings_though_it_comes_as_a_copy_event() {
        assert_eq!(clipboard_stroke(&Event::Copy).as_ref(), parse_sequence("<C-c>").unwrap().first());
        assert_eq!(clipboard_stroke(&Event::Cut).as_ref(), parse_sequence("<C-x>").unwrap().first());
        assert_eq!(clipboard_stroke(&Event::Text("c".to_owned())), None);
    }
}
