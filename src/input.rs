//! Keyboard and pointer input.
use egui::{CursorIcon, Event, MouseWheelUnit, PointerButton, Pos2, Response, Sense};

use crate::app::{App, Bar, BarKind, ChoiceState, Landing, Mode, Selection, TocState, Tool};
use crate::chrome::ListLayout;
use crate::field::{Edit, Field};
use crate::form::{Move, clamp_selected, keep_visible, wheel_step};
use crate::keymap::{Action, Stroke};
use crate::menu::{Menu, Outcome};
use crate::search::Search;
use crate::view::Anchor;

/// How long after a click another one is its double click, and how far from it: egui's own.
const DOUBLE_CLICK_DELAY: f64 = 0.3;
const DOUBLE_CLICK_DISTANCE: f32 = 6.0;

/// The characters a label or a mark can be typed with.
fn typed_char(stroke: &Stroke) -> Option<char> {
    stroke.text().filter(|c| *c != ' ')
}

/// How the menu's highlight moves for a key: Up and Ctrl-p back, Down and Ctrl-n forward.
fn menu_move(stroke: &Stroke) -> Option<isize> {
    if stroke.is("down") || stroke.is_ctrl("n") {
        Some(1)
    } else if stroke.is("up") || stroke.is_ctrl("p") {
        Some(-1)
    } else {
        None
    }
}

/// Half a page in the menu: Ctrl-d and PageDown (`true`) go down, Ctrl-u and PageUp up.
fn menu_page(stroke: &Stroke) -> Option<bool> {
    if stroke.is_ctrl("d") || stroke.is("pagedown") {
        Some(true)
    } else if stroke.is_ctrl("u") || stroke.is("pageup") {
        Some(false)
    } else {
        None
    }
}

/// Whether a half-page key goes to the menu before the edit bindings: only in the `:` bar, while
/// the menu is shown, where Ctrl-u and Ctrl-d win over `edit_erase_to_start`.
fn menu_page_wins(kind: BarKind, menu_shown: bool, stroke: &Stroke) -> bool {
    kind == BarKind::Command && menu_shown && menu_page(stroke).is_some()
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

/// What a key does to the choice panel as a whole.
enum ChoiceKey {
    Stay,
    Close,
    Choose,
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
            Mode::Choice(_) => self.choice_key(&stroke),
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
        let kind = bar.kind;
        self.bar_changed(kind);
        self.refresh_menu();
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
            BarKind::Form => self.form_entry_done(&bar.field.text),
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
        let rows = self.menu_rows();
        let Mode::Bar(bar) = &mut self.mode else { return };
        let kind = bar.kind;
        let tab = stroke.key == "tab" && !stroke.ctrl && !stroke.alt;
        if self.keymap.cancels(stroke) {
            let bar = bar.clone();
            self.bar_cancel(bar);
        } else if stroke.is("cr") {
            self.bar_enter(ctx);
            return;
        } else if menu_page_wins(kind, bar.menu.as_ref().is_some_and(|m| m.is_shown()), stroke)
            && let Some(menu) = bar.menu.as_mut()
            && let Some(down) = menu_page(stroke)
        {
            // Fixed keys, like Up and Down; they win over the edit bindings here.
            if let Some(text) = menu.move_page(down, rows) {
                bar.field.replace_before_cursor(&text);
            }
        } else if let Some(edit) = self.keymap.edit(stroke) {
            if edit == Edit::Backspace && bar.field.is_empty() {
                let bar = bar.clone();
                self.bar_cancel(bar);
            } else if bar.field.apply(edit) {
                self.bar_changed(kind);
            }
        } else if let Some(menu) = bar.menu.as_mut().filter(|_| kind == BarKind::Command)
            && let Some(delta) = menu_move(stroke)
        {
            // These keys are fixed, not under `[keys]`.
            // The field shows the choice, as with Tab.
            if let Some(text) = menu.move_by(delta, rows) {
                bar.field.replace_before_cursor(&text);
            }
        } else if tab && kind == BarKind::Command {
            // The field shows the choice; the menu keeps the entries of what was typed.
            if let Some(text) = bar.menu.as_mut().and_then(|menu| menu.tab(stroke.shift, rows)) {
                bar.field.replace_before_cursor(&text);
            }
        } else if let Some(c) = stroke.text() {
            bar.field.insert(c.encode_utf8(&mut [0; 4]));
            self.bar_changed(kind);
        }
        self.refresh_menu();
    }

    /// Enter in a bar. In the `:` bar it does what the menu's highlighted entry says.
    pub fn bar_enter(&mut self, ctx: &egui::Context) {
        let Mode::Bar(bar) = &mut self.mode else { return };
        let outcome = match (&bar.menu, bar.kind) {
            (Some(menu), BarKind::Command) => menu.enter(),
            _ => Outcome::Typed,
        };
        match outcome {
            Outcome::Typed => {
                let bar = bar.clone();
                self.bar_submit(ctx, bar);
            }
            Outcome::Fill(text) => {
                bar.field.replace_before_cursor(&text);
                // The text may be what Tab left in the field, which the menu would keep.
                self.make_menu(true);
            }
            Outcome::Run(action) => {
                self.mode = Mode::Normal;
                self.run(ctx, action, None);
            }
            Outcome::Command(line) => {
                self.mode = Mode::Normal;
                self.command(ctx, &line);
            }
        }
    }

    /// Makes the `:` bar's menu again when its text has changed since.
    pub fn refresh_menu(&mut self) {
        self.make_menu(false);
    }

    /// The menu is made from the text before the cursor, which is what Tab completes. The directory
    /// the last one read is passed on, so typing in a folder does not read it again.
    fn make_menu(&mut self, always: bool) {
        let Mode::Bar(bar) = &mut self.mode else { return };
        if bar.kind != BarKind::Command {
            return;
        }
        let (before, tail) = (bar.field.before_cursor(), !bar.field.at_end());
        if !always && let Some(menu) = bar.menu.as_mut().filter(|m| m.is_for(before)) {
            menu.set_tail(tail);
            return;
        }
        let cache = bar.menu.as_mut().map(|m| m.take_cache()).unwrap_or_default();
        let themes: Vec<String> = self.config.themes.keys().cloned().collect();
        let mut menu = Menu::with_cache(before, &self.config.keys, &themes, &crate::config::home(), cache);
        menu.set_tail(tail);
        bar.menu = Some(Box::new(menu));
    }

    // The signature pad.

    fn sign_key(&mut self, stroke: &Stroke) {
        let Mode::Sign(pad) = &mut self.mode else { return };
        if self.keymap.cancels(stroke) {
            self.mode = Mode::Normal;
        } else if stroke.is("cr") {
            let pad = pad.clone();
            self.sign_done(&pad);
        } else if self.keymap.is(stroke, Action::Undo) {
            pad.undo_stroke();
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

    // The choice panel of a combo or list box.

    /// How a stroke moves the highlight, in the panel and the contents alike.
    fn choice_move(stroke: &Stroke) -> Option<Move> {
        if stroke.is_ctrl("n") || stroke.is("down") {
            Some(Move::Next)
        } else if stroke.is_ctrl("p") || stroke.is("up") {
            Some(Move::Previous)
        } else if stroke.is_ctrl("d") {
            Some(Move::PageDown)
        } else if stroke.is_ctrl("u") {
            Some(Move::PageUp)
        } else {
            None
        }
    }

    /// What the plain keys do while the panel is not being filtered.
    fn choice_plain(state: &mut ChoiceState, stroke: &Stroke) -> ChoiceKey {
        match stroke.text() {
            Some('j') => state.go(Move::Next),
            Some('k') => state.go(Move::Previous),
            Some('g') => state.go(Move::First),
            Some('G') => state.go(Move::Last),
            Some('/') => state.filtering = true,
            Some(' ') => {
                if let Some(row) = state.highlighted() {
                    state.toggle(row);
                }
            }
            Some('l') if !state.nothing_to_choose() => return ChoiceKey::Choose,
            Some('q') => return ChoiceKey::Close,
            None if stroke.is("tab") => return ChoiceKey::Close,
            _ => {}
        }
        ChoiceKey::Stay
    }

    /// What a stroke does to the filter while it is being typed.
    fn choice_filter(&self, state: &mut ChoiceState, stroke: &Stroke) {
        if let Some(edit) = self.keymap.edit(stroke) {
            state.refilter(|s| {
                s.filter.apply(edit);
            });
        } else if let Some(c) = stroke.text() {
            state.refilter(|s| s.filter.insert(c.encode_utf8(&mut [0; 4])));
        }
    }

    fn choice_key(&mut self, stroke: &Stroke) {
        let Mode::Choice(state) = &self.mode else { return };
        let mut state = state.clone();
        let mut key = ChoiceKey::Stay;
        if self.keymap.cancels(stroke) {
            if state.filtering {
                state.refilter(|s| {
                    s.filtering = false;
                    s.filter = Field::default();
                });
            } else {
                key = ChoiceKey::Close;
            }
        } else if stroke.is("cr") {
            if !state.nothing_to_choose() {
                key = ChoiceKey::Choose;
            }
        } else if let Some(by) = Self::choice_move(stroke) {
            state.go(by);
        } else if state.filtering {
            self.choice_filter(&mut state, stroke);
        } else {
            key = Self::choice_plain(&mut state, stroke);
        }
        match key {
            ChoiceKey::Stay => {
                self.keep_choice_visible(&mut state);
                self.mode = Mode::Choice(state);
            }
            ChoiceKey::Close => self.mode = Mode::Normal,
            ChoiceKey::Choose => {
                let value = state.commit(state.highlighted());
                self.mode = Mode::Choice(state);
                self.choice_done(value);
            }
        }
    }

    /// The panel's layout for `state`, as drawn.
    fn choice_layout(&self, state: &ChoiceState) -> ListLayout {
        self.list_layout(true, state.filtering || !state.filter.is_empty(), state.rows().len(), state.selected, Some(state.first))
    }

    /// Scrolls the panel, if need be, to show the highlight.
    fn keep_choice_visible(&self, state: &mut ChoiceState) {
        let fits = self.choice_layout(state).fits;
        state.first = keep_visible(state.first, state.selected, fits, state.rows().len());
    }

    /// The wheel over the open panel scrolls its list; elsewhere it does nothing.
    fn wheel_choice(&mut self, pixels_y: f32, at: Option<Pos2>) {
        let Mode::Choice(state) = &self.mode else { return };
        let layout = self.choice_layout(state);
        if !at.is_some_and(|p| layout.rect.contains(p)) {
            return;
        }
        let len = state.rows().len();
        if let Mode::Choice(state) = &mut self.mode {
            let before = layout.first;
            (state.first, state.wheel_rest) = wheel_step(before, state.wheel_rest, pixels_y, layout.row_height, layout.fits, len);
            if state.first != before {
                state.selected = clamp_selected(state.selected, state.first, layout.fits, len);
            }
        }
    }

    /// A click on a row chooses it, or toggles it in a multi-select list; one outside the panel
    /// cancels. The page underneath gets no clicks while the panel is open.
    fn pointer_choice(&mut self, ctx: &egui::Context, response: &Response) {
        ctx.set_cursor_icon(CursorIcon::Default);
        let Some(at) = response.interact_pointer_pos().filter(|_| response.clicked()) else { return };
        let Mode::Choice(state) = &self.mode else { return };
        let mut state = state.clone();
        let len = state.rows().len();
        let layout = self.choice_layout(&state);
        let closed = Some((ctx.input(|i| i.time), at));
        let guarded = self
            .choice_opened
            .is_some_and(|(time, from)| ctx.input(|i| i.time) - time <= DOUBLE_CLICK_DELAY && at.distance(from) <= DOUBLE_CLICK_DISTANCE);
        if guarded {
            return;
        }
        self.choice_opened = None;
        if !layout.rect.contains(at) {
            self.mode = Mode::Normal;
            self.choice_closed = closed;
            return;
        }
        let hit = (layout.first..len).take(layout.fits).find(|&i| Self::list_row_rect(&layout, i).contains(at));
        let Some(i) = hit else { return };
        let value = state.click(i);
        let multi = state.multi;
        self.mode = Mode::Choice(state);
        if !multi {
            self.choice_done(value);
            self.choice_closed = closed;
        }
    }

    /// Whether this click is the second of a double click whose first closed the panel: it
    /// must not land on the form widget that was under the panel.
    fn swallowed_after_choice(&mut self, ctx: &egui::Context, response: &Response) -> bool {
        let Some((time, at)) = self.choice_closed else { return false };
        let Some(now) = response.interact_pointer_pos().filter(|_| response.clicked()) else {
            if ctx.input(|i| i.time) - time > DOUBLE_CLICK_DELAY {
                self.choice_closed = None;
            }
            return false;
        };
        self.choice_closed = None;
        ctx.input(|i| i.time) - time <= DOUBLE_CLICK_DELAY && now.distance(at) <= DOUBLE_CLICK_DISTANCE
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
            if matches!(self.mode, Mode::Choice(_)) {
                self.wheel_choice(pixels.y, at);
            } else if at.is_some_and(|p| self.over_menu(p)) {
                self.scroll_menu(-pixels.y);
            } else if modifiers.ctrl {
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

    /// The menu under the `:` bar, if it is open.
    pub fn open_menu(&self) -> Option<&Menu> {
        match &self.mode {
            Mode::Bar(bar) => bar.menu.as_deref().filter(|m| bar.kind == BarKind::Command && m.is_shown()),
            _ => None,
        }
    }

    fn over_menu(&self, at: Pos2) -> bool {
        self.open_menu().is_some_and(|menu| self.menu_rect(menu).contains(at))
    }

    fn scroll_menu(&mut self, pixels: f32) {
        let (rows, row_height) = (self.menu_rows(), crate::chrome::bar_height(&self.config));
        if let Mode::Bar(bar) = &mut self.mode
            && let Some(menu) = bar.menu.as_mut()
        {
            menu.scroll_pixels(pixels, row_height, rows);
        }
    }

    /// A click on a menu entry does what Enter does on it. Returns whether the pointer belongs to
    /// the menu, which then takes it from the document: when a press started in the menu, or the
    /// pointer is over it with no button down. A drag that started on the page goes on over the
    /// menu and finishes as it began.
    fn pointer_menu(&mut self, ctx: &egui::Context, response: &Response) -> bool {
        let gesture = response.id.with("menu_press");
        let (pressed, down, released) = ctx.input(|i| (i.pointer.any_pressed(), i.pointer.any_down(), i.pointer.any_released()));
        if pressed {
            let inside = ctx.input(|i| i.pointer.press_origin().or_else(|| i.pointer.interact_pos())).is_some_and(|p| self.over_menu(p));
            ctx.data_mut(|d| d.insert_temp(gesture, inside));
        } else if !down && !released {
            ctx.data_mut(|d| d.remove_temp::<bool>(gesture));
        }
        let started_in = ctx.data(|d| d.get_temp::<bool>(gesture));
        let at = response.hover_pos().or_else(|| response.interact_pointer_pos());
        let claimed = match started_in {
            Some(inside) => inside,
            None => at.is_some_and(|p| self.over_menu(p)),
        };
        if !claimed {
            return false;
        }
        ctx.set_cursor_icon(CursorIcon::Default);
        let Some(at) = at.filter(|_| response.clicked()) else { return true };
        let rows = self.menu_rows();
        let hit = self.open_menu().and_then(|menu| self.menu_row_rects(menu).into_iter().find(|(_, r)| r.contains(at)));
        if let Some((index, _)) = hit {
            let mut filled = false;
            if let Mode::Bar(bar) = &mut self.mode
                && let Some(menu) = bar.menu.as_mut()
                && let Some(text) = menu.select(index, rows)
            {
                // With text after the cursor a click only fills, as Tab does; Enter runs the text.
                if !bar.field.at_end() {
                    bar.field.replace_before_cursor(&text);
                    filled = true;
                }
            }
            if filled {
                self.refresh_menu();
            } else {
                self.bar_enter(ctx);
            }
        }
        true
    }

    pub fn pointer(&mut self, ui: &egui::Ui) {
        let was_choice = matches!(self.mode, Mode::Choice(_));
        self.pointer_view(ui);
        if !was_choice && matches!(self.mode, Mode::Choice(_)) {
            // Opened by a click: its double click must not pick a row.
            self.choice_opened = ui.input(|i| i.pointer.interact_pos()).map(|at| (ui.input(|i| i.time), at));
        }
    }

    fn pointer_view(&mut self, ui: &egui::Ui) {
        let response = ui.interact(self.view.rect, ui.scope_id().with("view"), Sense::click_and_drag());
        self.hover = response.hover_pos();
        if matches!(self.mode, Mode::Sign(_)) {
            self.pointer_pad(ui.ctx(), &response);
            ui.ctx().set_cursor_icon(CursorIcon::Crosshair);
            return;
        }
        if matches!(self.mode, Mode::Choice(_)) {
            self.pointer_choice(ui.ctx(), &response);
            return;
        }
        if self.swallowed_after_choice(ui.ctx(), &response) {
            return;
        }
        if self.pointer_menu(ui.ctx(), &response) {
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
            Some((page, at)) if self.link_at(page, at).is_some() || self.widget_at(page, at).is_some() => CursorIcon::PointingHand,
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
            let field = under.filter(|_| matches!(self.mode, Mode::Normal)).and_then(|(page, at)| self.widget_at(page, at));
            if let Some((field, widget)) = field {
                self.form_activate(field, widget);
            } else if let Some(link) = under.and_then(|(page, at)| self.link_at(page, at)) {
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
            pad.wait_for_release = false;
            return;
        }
        if pad.wait_for_release {
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
    use super::{clipboard_stroke, menu_page, menu_page_wins};
    use crate::app::BarKind;
    use crate::keymap::parse_sequence;
    use egui::Event;

    #[test]
    fn control_c_reaches_the_bindings_though_it_comes_as_a_copy_event() {
        assert_eq!(clipboard_stroke(&Event::Copy).as_ref(), parse_sequence("<C-c>").unwrap().first());
        assert_eq!(clipboard_stroke(&Event::Cut).as_ref(), parse_sequence("<C-x>").unwrap().first());
        assert_eq!(clipboard_stroke(&Event::Text("c".to_owned())), None);
    }

    #[test]
    fn half_page_keys_map_and_beat_the_edit_keys_only_in_the_command_bar() {
        let key = |n: &str| parse_sequence(n).unwrap().first().cloned().unwrap();
        assert_eq!(menu_page(&key("<C-d>")), Some(true));
        assert_eq!(menu_page(&key("<pagedown>")), Some(true));
        assert_eq!(menu_page(&key("<C-u>")), Some(false));
        assert_eq!(menu_page(&key("<pageup>")), Some(false));
        assert_eq!(menu_page(&key("<C-n>")), None);
        let ctrl_u = key("<C-u>");
        assert!(menu_page_wins(BarKind::Command, true, &ctrl_u));
        assert!(!menu_page_wins(BarKind::Command, false, &ctrl_u));
        assert!(!menu_page_wins(BarKind::Search { forward: true }, true, &ctrl_u), "search keeps erasing");
    }

    #[test]
    fn the_choice_panel_keys_and_moves() {
        use super::{App, ChoiceKey};
        use crate::app::ChoiceState;
        use crate::form::ChoiceRow;
        let key = |n: &str| parse_sequence(n).unwrap().first().cloned().unwrap();
        let mk = |n: usize| ChoiceState {
            name: "x".into(),
            title: "x".into(),
            options: (0..n).map(|i| (format!("e{i}"), format!("O{i}"))).collect(),
            multi: true,
            editable: false,
            marked: vec![],
            initial: vec![],
            strangers: vec![],
            selected: 0,
            first: 0,
            wheel_rest: 0.0,
            filter: crate::field::Field::default(),
            filtering: false,
        };
        let mut s = mk(30);
        assert!(matches!(App::choice_plain(&mut s, &key("G")), ChoiceKey::Stay));
        assert_eq!(s.selected, 29);
        App::choice_plain(&mut s, &key("g"));
        assert_eq!(s.selected, 0);
        App::choice_plain(&mut s, &key("j"));
        App::choice_plain(&mut s, &key("<space>"));
        assert_eq!(s.marked, vec!["e1".to_owned()]);
        App::choice_plain(&mut s, &key("<space>"));
        assert!(s.marked.is_empty());
        assert!(!s.filtering);
        App::choice_plain(&mut s, &key("/"));
        assert!(s.filtering);
        assert!(matches!(App::choice_plain(&mut s, &key("l")), ChoiceKey::Choose));
        s.filter = crate::field::Field::new("zzz".to_owned());
        assert!(matches!(App::choice_plain(&mut s, &key("l")), ChoiceKey::Stay), "nothing matches: stays open");
        s.filter = crate::field::Field::default();
        assert!(matches!(App::choice_plain(&mut s, &key("q")), ChoiceKey::Close));
        assert!(matches!(App::choice_plain(&mut s, &key("<tab>")), ChoiceKey::Close));
        assert!(matches!(App::choice_plain(&mut s, &key("z")), ChoiceKey::Stay));
        // space with nothing highlighted must not panic
        let mut e = mk(0);
        App::choice_plain(&mut e, &key("<space>"));
        assert_eq!(e.highlighted(), None::<ChoiceRow>);
        use crate::form::Move;
        assert_eq!(App::choice_move(&key("<C-n>")), Some(Move::Next));
        assert_eq!(App::choice_move(&key("<down>")), Some(Move::Next));
        assert_eq!(App::choice_move(&key("<C-p>")), Some(Move::Previous));
        assert_eq!(App::choice_move(&key("<up>")), Some(Move::Previous));
        assert_eq!(App::choice_move(&key("<C-d>")), Some(Move::PageDown));
        assert_eq!(App::choice_move(&key("<C-u>")), Some(Move::PageUp));
        assert_eq!(App::choice_move(&key("j")), None);
    }
}
