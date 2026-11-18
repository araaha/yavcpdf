//! The list panels: the pickers of the bookmarks and the annotations, and the keys, wheel and clicks they share with `:info`.
use egui::{CursorIcon, Pos2, Response};

use crate::app::{App, Mode};
use crate::chrome::{ListLayout, ListView};
use crate::field::Field;
use crate::form::{clamp_selected, keep_visible, scrolled, wheel_step};
use crate::keymap::{Keymap, Stroke};
use crate::store;
use crate::view::Anchor;

/// The state of a picker, shared by the bookmarks and the annotations.
#[derive(Clone, Debug, Default)]
pub struct PickerState {
    /// The highlighted row, as an index into the list the picker shows.
    pub selected: usize,
    /// The first row shown, as a place in the rows the filter lets through.
    pub first: usize,
    /// The part of a row the wheel has moved the list by and not yet scrolled.
    pub wheel_rest: f32,
    pub filter: Field,
    pub filtering: bool,
}

impl PickerState {
    /// Whether the filter row is shown: while typing in it, and while it holds something.
    pub fn filter_shown(&self) -> bool {
        self.filtering || !self.filter.is_empty()
    }

    /// What the picker shows, given the rows the filter lets through.
    pub fn view(&self, rows: &[usize]) -> ListView<'_> {
        let at = rows.iter().position(|r| *r >= self.selected).unwrap_or(0);
        ListView { header: None, filter: self.filter_shown().then_some(&self.filter), len: rows.len(), at, first: Some(self.first) }
    }
}

/// What a key does to the open picker, beyond changing its state.
#[derive(Debug, PartialEq)]
pub enum Outcome {
    None,
    /// Go to the row at this index of the list.
    Jump(usize),
    /// Delete the row at this index.
    Delete(usize),
    Close,
}

/// Moves the highlight to the next row left at or after it, or the last one.
pub fn settle(rows: &[usize], state: &mut PickerState) {
    if let Some(row) = rows.iter().find(|r| **r >= state.selected).or(rows.last()) {
        state.selected = *row;
    }
}

/// Applies a key to `state`, given the rows (as indices) that a filter lets through. A deletion is left to the caller, which then settles.
pub fn key_step(rows_for: &dyn Fn(&str) -> Vec<usize>, state: &mut PickerState, stroke: &Stroke, keymap: &Keymap) -> Outcome {
    let rows = rows_for(&state.filter.text);
    let at = rows.iter().position(|r| *r >= state.selected).unwrap_or(0);
    let last = rows.len().saturating_sub(1);
    let here = rows.get(at).copied();
    let mut moved = None;
    if keymap.cancels(stroke) {
        if !state.filtering {
            return Outcome::Close;
        }
        state.filtering = false;
        state.filter = Field::default();
    } else if stroke.is("cr") {
        return here.map_or(Outcome::None, Outcome::Jump);
    } else if stroke.is_ctrl("n") || stroke.is("down") {
        moved = Some((at + 1).min(last));
    } else if stroke.is_ctrl("p") || stroke.is("up") {
        moved = Some(at.saturating_sub(1));
    } else if stroke.is_ctrl("d") {
        moved = Some((at + 10).min(last));
    } else if stroke.is_ctrl("u") {
        moved = Some(at.saturating_sub(10));
    } else if state.filtering {
        // Delete and a typed `d` edit the field here.
        if let Some(edit) = keymap.edit(stroke) {
            state.filter.apply(edit);
        } else if let Some(c) = stroke.text() {
            state.filter.insert(c.encode_utf8(&mut [0; 4]));
        }
    } else if stroke.is("del") {
        return here.map_or(Outcome::None, Outcome::Delete);
    } else {
        match stroke.text() {
            Some('j') => moved = Some((at + 1).min(last)),
            Some('k') => moved = Some(at.saturating_sub(1)),
            Some('g') => moved = Some(0),
            Some('G') => moved = Some(last),
            Some('/') => state.filtering = true,
            Some('d') => return here.map_or(Outcome::None, Outcome::Delete),
            Some('l') => return here.map_or(Outcome::None, Outcome::Jump),
            Some('q') => return Outcome::Close,
            _ => {}
        }
    }
    // The rows may have changed (a filter edit): the highlight moves to the next one left.
    let rows = rows_for(&state.filter.text);
    if let Some(row) = moved.and_then(|m| rows.get(m)) {
        state.selected = *row;
    }
    settle(&rows, state);
    Outcome::None
}

/// The first row shown after a key, so that the highlight is in view.
pub fn keep_highlight(state: &PickerState, rows: &[usize], fits: usize) -> usize {
    let at = rows.iter().position(|r| *r == state.selected).unwrap_or(0);
    keep_visible(state.first, at, fits, rows.len())
}

/// The wheel's step on a list of `len` rows, from the first row `layout` shows and the unspent `rest`.
pub fn scroll_wheel(layout: &ListLayout, rest: f32, pixels_y: f32, len: usize) -> (usize, f32) {
    wheel_step(layout.first, rest, pixels_y, layout.row_height, layout.fits, len)
}

/// The wheel over a picker scrolls its list, and when it moved, the highlight is moved into view.
pub fn wheel(state: &mut PickerState, layout: &ListLayout, rows: &[usize], pixels_y: f32) {
    let before = layout.first;
    (state.first, state.wheel_rest) = scroll_wheel(layout, state.wheel_rest, pixels_y, rows.len());
    if state.first != before {
        let at = rows.iter().position(|r| *r >= state.selected).unwrap_or(0);
        let at = clamp_selected(at, state.first, layout.fits, rows.len());
        state.selected = rows.get(at).copied().unwrap_or(state.selected);
    }
}

/// What a key does to the `:info` panel.
#[derive(Debug, PartialEq)]
pub enum InfoStep {
    Close,
    /// The first row shown afterwards.
    First(usize),
}

/// Applies a key to the `:info` panel, which shows `first..first + fits` of `len` rows.
pub fn info_step(first: usize, fits: usize, len: usize, stroke: &Stroke, keymap: &Keymap) -> InfoStep {
    if keymap.cancels(stroke) || stroke.is_ctrl("c") || stroke.text() == Some('q') {
        InfoStep::Close
    } else if stroke.is_ctrl("n") || stroke.is("down") || stroke.text() == Some('j') {
        InfoStep::First(scrolled(first, 1, fits, len))
    } else if stroke.is_ctrl("p") || stroke.is("up") || stroke.text() == Some('k') {
        InfoStep::First(scrolled(first, -1, fits, len))
    } else {
        InfoStep::First(first)
    }
}

/// Where a click on a panel landed.
#[derive(Debug, PartialEq)]
pub enum Click {
    Outside,
    /// On this row, as a place in the rows.
    Row(usize),
    /// In the panel but on no row.
    Inside,
}

/// Where a click at `at` landed on a panel of `len` rows.
pub fn click(layout: &ListLayout, len: usize, at: Pos2) -> Click {
    if !layout.rect.contains(at) {
        return Click::Outside;
    }
    let hit = (layout.first..len).take(layout.fits).find(|&i| App::list_row_rect(layout, i).contains(at));
    hit.map_or(Click::Inside, Click::Row)
}

/// The two pickers, which differ in the list they show and what they do with a row.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Picker {
    Bookmarks,
    Annotations,
}

impl Picker {
    fn state(self, mode: &Mode) -> Option<&PickerState> {
        match (self, mode) {
            (Picker::Bookmarks, Mode::Bookmarks(state)) | (Picker::Annotations, Mode::Annotations(state)) => Some(state),
            _ => None,
        }
    }

    fn state_mut(self, mode: &mut Mode) -> Option<&mut PickerState> {
        match (self, mode) {
            (Picker::Bookmarks, Mode::Bookmarks(state)) | (Picker::Annotations, Mode::Annotations(state)) => Some(state),
            _ => None,
        }
    }

    fn mode(self, state: PickerState) -> Mode {
        match self {
            Picker::Bookmarks => Mode::Bookmarks(state),
            Picker::Annotations => Mode::Annotations(state),
        }
    }

    /// The rows the filter lets through, as indices.
    fn rows(self, app: &App, filter: &str) -> Vec<usize> {
        match self {
            Picker::Bookmarks => store::bookmark_rows(&app.side.bookmarks, filter),
            Picker::Annotations => crate::annotations::rows(&crate::annotations::items(&app.side.annotations), filter),
        }
    }

    fn len(self, app: &App) -> usize {
        match self {
            Picker::Bookmarks => app.side.bookmarks.len(),
            Picker::Annotations => app.side.annotations.len(),
        }
    }

    /// The row to highlight on opening: the last at or before `page`.
    fn initial(self, app: &App, page: usize) -> usize {
        match self {
            Picker::Bookmarks => app.side.bookmarks.iter().rposition(|b| b.anchor.page <= page).unwrap_or(0),
            Picker::Annotations => crate::annotations::items(&app.side.annotations).iter().rposition(|i| i.page <= page).unwrap_or(0),
        }
    }

    fn nothing(self) -> &'static str {
        match self {
            Picker::Bookmarks => "no bookmarks",
            Picker::Annotations => "no annotations",
        }
    }

    fn go(self, app: &mut App, index: usize) {
        app.mode = Mode::Normal;
        match self {
            Picker::Bookmarks => {
                if let Some(b) = app.side.bookmarks.get(index) {
                    app.jump_to(b.anchor);
                }
            }
            Picker::Annotations => {
                if let Some(item) = crate::annotations::items(&app.side.annotations).get(index) {
                    app.jump_to_point(Anchor { page: item.page, y: item.bounds[1], x: app.view.x });
                }
            }
        }
    }

    /// Deletes the row at `index`, saying so. Whether anything was deleted.
    fn delete(self, app: &mut App, index: usize) -> bool {
        match self {
            Picker::Bookmarks => {
                let Some(gone) = store::remove_bookmark(&mut app.side.bookmarks, index) else { return false };
                app.mark_dirty();
                app.say(format!("bookmark {} deleted", gone.name));
            }
            Picker::Annotations => {
                let list = crate::annotations::items(&app.side.annotations);
                let Some(item) = list.get(index) else { return false };
                app.remove_annotation(item.id);
                app.say(format!("{} deleted", item.kind));
            }
        }
        true
    }
}

impl App {
    /// Opens the picker, with the highlight on the last row at or before the view.
    pub fn open_picker(&mut self, picker: Picker) {
        let len = picker.len(self);
        if len == 0 {
            self.say(picker.nothing());
            return;
        }
        let page = self.view.current_page(&self.doc.layout);
        let selected = picker.initial(self, page);
        let mut state = PickerState { selected, ..Default::default() };
        state.first = keep_visible(0, selected, self.picker_layout(&state, len).fits, len);
        self.mode = picker.mode(state);
    }

    /// The layout of a picker showing `len` rows for `state`, as drawn.
    pub fn picker_layout(&self, state: &PickerState, len: usize) -> ListLayout {
        self.list_layout(false, state.filter_shown(), len, 0, Some(state.first))
    }

    pub fn picker_key(&mut self, picker: Picker, stroke: &Stroke) {
        let Some(state) = picker.state(&self.mode) else { return };
        let mut state = state.clone();
        let rows = match key_step(&|filter| picker.rows(self, filter), &mut state, stroke, &self.keymap) {
            Outcome::Close => {
                self.mode = Mode::Normal;
                return;
            }
            Outcome::Jump(row) => {
                picker.go(self, row);
                return;
            }
            Outcome::Delete(row) => {
                if picker.delete(self, row) && picker.len(self) == 0 {
                    self.mode = Mode::Normal;
                    return;
                }
                let rows = picker.rows(self, &state.filter.text);
                settle(&rows, &mut state);
                rows
            }
            Outcome::None => picker.rows(self, &state.filter.text),
        };
        state.first = keep_highlight(&state, &rows, self.picker_layout(&state, rows.len()).fits);
        self.mode = picker.mode(state);
    }

    /// The wheel over the open picker scrolls its list; elsewhere it does nothing.
    pub fn picker_wheel(&mut self, picker: Picker, pixels_y: f32, at: Option<Pos2>) {
        let Some(state) = picker.state(&self.mode) else { return };
        let rows = picker.rows(self, &state.filter.text);
        let layout = self.picker_layout(state, rows.len());
        if !at.is_some_and(|p| layout.rect.contains(p)) {
            return;
        }
        if let Some(state) = picker.state_mut(&mut self.mode) {
            wheel(state, &layout, &rows, pixels_y);
        }
    }

    /// A click on a row jumps to it; one outside the panel closes it. The page gets no clicks meanwhile.
    pub fn picker_pointer(&mut self, picker: Picker, ctx: &egui::Context, response: &Response) {
        ctx.set_cursor_icon(CursorIcon::Default);
        let Some(at) = response.interact_pointer_pos().filter(|_| response.clicked()) else { return };
        let Some(state) = picker.state(&self.mode) else { return };
        let rows = picker.rows(self, &state.filter.text);
        let layout = self.picker_layout(state, rows.len());
        match click(&layout, rows.len(), at) {
            Click::Outside => {
                self.mode = Mode::Normal;
                self.choice_closed = Some((ctx.input(|i| i.time), at));
            }
            Click::Row(i) => {
                picker.go(self, rows[i]);
                // Jumped by a click: its double click must not land on the page.
                self.choice_closed = Some((ctx.input(|i| i.time), at));
            }
            Click::Inside => {}
        }
    }

    pub fn info_key(&mut self, stroke: &Stroke) {
        let Mode::Info(state) = &self.mode else { return };
        let len = state.rows.len();
        let fits = self.info_layout(len, state.first).fits;
        match info_step(state.first, fits, len, stroke, &self.keymap) {
            InfoStep::Close => self.mode = Mode::Normal,
            InfoStep::First(first) => {
                if let Mode::Info(state) = &mut self.mode {
                    state.first = first;
                }
            }
        }
    }

    pub fn info_wheel(&mut self, pixels_y: f32) {
        let Mode::Info(state) = &self.mode else { return };
        let layout = self.info_layout(state.rows.len(), state.first);
        if let Mode::Info(state) = &mut self.mode {
            (state.first, state.wheel_rest) = scroll_wheel(&layout, state.wheel_rest, pixels_y, state.rows.len());
        }
    }

    /// A click outside the panel closes it. The page gets no clicks meanwhile.
    pub fn info_pointer(&mut self, ctx: &egui::Context, response: &Response) {
        ctx.set_cursor_icon(CursorIcon::Default);
        let Some(at) = response.interact_pointer_pos().filter(|_| response.clicked()) else { return };
        let Mode::Info(state) = &self.mode else { return };
        let layout = self.info_layout(state.rows.len(), state.first);
        if click(&layout, 0, at) == Click::Outside {
            self.mode = Mode::Normal;
            self.choice_closed = Some((ctx.input(|i| i.time), at));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Config;
    use egui::{Rect, pos2, vec2};

    fn keymap() -> Keymap {
        Keymap::new(&Config::load_str("").unwrap().keys, &Default::default()).0
    }

    fn key(name: &str) -> Stroke {
        crate::keymap::parse_sequence(name).unwrap().remove(0)
    }

    /// A layout whose rows are 10 high from y = 20, three of them fitting, the first being `first`.
    fn layout(first: usize) -> ListLayout {
        ListLayout { rect: Rect::from_min_size(pos2(0.0, 0.0), vec2(100.0, 60.0)), top: 20.0, row_height: 10.0, first, fits: 3 }
    }

    #[test]
    fn keep_highlight_scrolls_to_the_highlighted_row() {
        let rows: Vec<usize> = (0..10).collect();
        let state = PickerState { selected: 5, first: 0, ..Default::default() };
        assert_eq!(keep_highlight(&state, &rows, 3), 3);
        let state = PickerState { selected: 1, first: 4, ..Default::default() };
        assert_eq!(keep_highlight(&state, &rows, 3), 1);
        let state = PickerState { selected: 9, first: 0, ..Default::default() };
        assert_eq!(keep_highlight(&state, &[], 3), 0, "no rows: nothing to scroll");
    }

    #[test]
    fn the_wheel_scrolls_and_moves_the_highlight_into_view() {
        let rows: Vec<usize> = (0..10).collect();
        let mut state = PickerState::default();
        // Two rows down: the highlight on row 0 follows to the first row shown.
        wheel(&mut state, &layout(0), &rows, -20.0);
        assert_eq!((state.first, state.selected), (2, 2));
        // A highlight already in view stays.
        let mut state = PickerState { selected: 4, first: 2, ..Default::default() };
        wheel(&mut state, &layout(2), &rows, -10.0);
        assert_eq!((state.first, state.selected), (3, 4));
        // Up to the top.
        wheel(&mut state, &layout(3), &rows, 100.0);
        assert_eq!((state.first, state.selected), (0, 2));
    }

    #[test]
    fn the_wheel_that_does_not_scroll_leaves_the_highlight() {
        let rows: Vec<usize> = (0..10).collect();
        let mut state = PickerState { selected: 7, ..Default::default() };
        wheel(&mut state, &layout(0), &rows, 5.0);
        assert_eq!((state.first, state.selected), (0, 7));
        assert_eq!(state.wheel_rest, 0.0, "stopped at the top: no rest kept");
    }

    #[test]
    fn info_keys_close_or_scroll_by_a_row() {
        let k = keymap();
        for name in ["<esc>", "<c-c>", "q"] {
            assert_eq!(info_step(2, 3, 10, &key(name), &k), InfoStep::Close, "{name}");
        }
        for name in ["j", "<c-n>", "<down>"] {
            assert_eq!(info_step(2, 3, 10, &key(name), &k), InfoStep::First(3), "{name}");
        }
        for name in ["k", "<c-p>", "<up>"] {
            assert_eq!(info_step(2, 3, 10, &key(name), &k), InfoStep::First(1), "{name}");
        }
        assert_eq!(info_step(0, 3, 10, &key("k"), &k), InfoStep::First(0));
        assert_eq!(info_step(7, 3, 10, &key("j"), &k), InfoStep::First(7));
        assert_eq!(info_step(2, 3, 10, &key("x"), &k), InfoStep::First(2));
    }

    #[test]
    fn a_click_is_outside_on_a_row_or_inside() {
        let l = layout(2);
        assert_eq!(click(&l, 10, pos2(200.0, 30.0)), Click::Outside);
        // The first row shown (row 2) is at y = 20..30.
        assert_eq!(click(&l, 10, pos2(50.0, 25.0)), Click::Row(2));
        assert_eq!(click(&l, 10, pos2(50.0, 45.0)), Click::Row(4));
        // The header area above the rows.
        assert_eq!(click(&l, 10, pos2(50.0, 5.0)), Click::Inside);
        // The margin at the side of a row.
        assert_eq!(click(&l, 10, pos2(0.5, 25.0)), Click::Inside);
        // Below the last row there is.
        assert_eq!(click(&l, 4, pos2(50.0, 45.0)), Click::Inside);
        assert_eq!(click(&l, 0, pos2(50.0, 25.0)), Click::Inside);
    }

    #[test]
    fn the_view_shows_the_filter_only_when_it_is_in_use() {
        let rows = [3, 5, 8];
        let mut state = PickerState { selected: 4, first: 1, ..Default::default() };
        let view = state.view(&rows);
        assert!(view.filter.is_none());
        assert_eq!((view.len, view.at, view.first), (3, 1, Some(1)));
        state.filtering = true;
        assert!(state.view(&rows).filter.is_some());
    }

    fn texts(texts: &[(u64, f32, &str)]) -> Vec<crate::store::Annotation> {
        texts
            .iter()
            .map(|(id, y, text)| crate::store::Annotation::Text { id: *id, page: 0, at: [5.0, *y], size: 12.0, text: (*text).to_owned() })
            .collect()
    }

    /// Three notes, stored out of document order: sorted they are Ada (id 30), Bob (id 20), Cab (id 10).
    fn stored() -> Vec<crate::store::Annotation> {
        texts(&[(10, 90.0, "Cab"), (20, 60.0, "Bob"), (30, 30.0, "Ada")])
    }

    #[test]
    fn the_annotation_picker_filters_by_text_and_deletes_by_the_sorted_index() {
        let k = keymap();
        let items = crate::annotations::items(&stored());
        assert_eq!(items.iter().map(|i| i.id).collect::<Vec<_>>(), [30, 20, 10]);
        let rows_for = |filter: &str| crate::annotations::rows(&items, filter);
        let mut state = PickerState::default();
        assert_eq!(key_step(&rows_for, &mut state, &key("/"), &k), Outcome::None);
        assert!(state.filtering);
        assert_eq!(key_step(&rows_for, &mut state, &key("a"), &k), Outcome::None);
        // "a" lets Ada and Cab through, not Bob; the highlight stays on the first.
        assert_eq!(rows_for("a"), [0, 2]);
        assert_eq!(state.selected, 0);
        // Down moves to the second row shown, which is the third item.
        assert_eq!(key_step(&rows_for, &mut state, &key("<down>"), &k), Outcome::None);
        assert_eq!(state.selected, 2);
        // Enter and Delete outside the filter act on the item's index in the whole list.
        assert_eq!(key_step(&rows_for, &mut state, &key("<cr>"), &k), Outcome::Jump(2));
        state.filtering = false;
        assert_eq!(key_step(&rows_for, &mut state, &key("d"), &k), Outcome::Delete(2));
        assert_eq!(key_step(&rows_for, &mut state, &key("<del>"), &k), Outcome::Delete(2));
        assert_eq!(key_step(&rows_for, &mut state, &key("l"), &k), Outcome::Jump(2));
        // Escape clears the filter first, then closes.
        assert_eq!(key_step(&rows_for, &mut state, &key("<esc>"), &k), Outcome::Close);
        state.filtering = true;
        assert_eq!(key_step(&rows_for, &mut state, &key("<esc>"), &k), Outcome::None);
        assert!(!state.filtering && state.filter.is_empty());
    }

    #[test]
    fn deleting_an_annotation_then_settling_keeps_the_highlight_on_a_row_left() {
        let k = keymap();
        let mut list = stored();
        let mut state = PickerState { selected: 2, ..Default::default() };
        let rows_for = |list: &[crate::store::Annotation], filter: &str| crate::annotations::rows(&crate::annotations::items(list), filter);
        let Outcome::Delete(row) = key_step(&|f| rows_for(&list, f), &mut state, &key("d"), &k) else { panic!("not a delete") };
        let gone = crate::annotations::items(&list)[row].id;
        assert_eq!(gone, 10);
        list.retain(|a| a.id() != gone);
        settle(&rows_for(&list, ""), &mut state);
        assert_eq!(state.selected, 1);
    }

    #[test]
    fn the_wheel_maps_the_highlight_through_the_rows_the_filter_lets_through() {
        // Five rows shown out of ten; three fit.
        let rows = [1, 4, 6, 8, 9];
        let mut state = PickerState { selected: 1, ..Default::default() };
        wheel(&mut state, &layout(0), &rows, -20.0);
        // Two places down: the highlight (place 0) follows to place 2, which is item 6.
        assert_eq!((state.first, state.selected), (2, 6));
        // A highlight between the rows shown counts as the next one left, place 1 (item 4), and is clamped to place 2.
        let mut state = PickerState { selected: 3, ..Default::default() };
        wheel(&mut state, &layout(0), &rows, -20.0);
        assert_eq!((state.first, state.selected), (2, 6));
        // Back up to the top: the highlight on place 4 is pulled up to the last place in view, place 2 (item 6).
        let mut state = PickerState { selected: 9, first: 2, ..Default::default() };
        wheel(&mut state, &layout(2), &rows, 20.0);
        assert_eq!((state.first, state.selected), (0, 6));
    }

    #[test]
    fn a_click_on_a_shared_boundary_lands_on_the_upper_row() {
        let l = layout(2);
        // Rows 2, 3, 4 are at y = 20..30, 30..40, 40..50; the one past them (5) is not shown.
        assert_eq!(click(&l, 10, pos2(50.0, 20.0)), Click::Row(2));
        assert_eq!(click(&l, 10, pos2(50.0, 29.9)), Click::Row(2));
        assert_eq!(click(&l, 10, pos2(50.0, 30.0)), Click::Row(2), "the boundary is on both rows; the upper one is found first");
        assert_eq!(click(&l, 10, pos2(50.0, 30.1)), Click::Row(3));
        assert_eq!(click(&l, 10, pos2(50.0, 49.9)), Click::Row(4));
        assert_eq!(click(&l, 10, pos2(50.0, 50.1)), Click::Inside, "row 5 does not fit");
        assert_eq!(click(&l, 10, pos2(50.0, 60.1)), Click::Outside);
    }

    #[test]
    fn a_click_in_the_header_or_the_filter_row_is_inside_and_on_no_row() {
        let l = layout(0);
        assert_eq!(click(&l, 10, pos2(50.0, 0.0)), Click::Inside);
        assert_eq!(click(&l, 10, pos2(50.0, 12.0)), Click::Inside);
        assert_eq!(click(&l, 10, pos2(50.0, 19.9)), Click::Inside);
        assert_eq!(click(&l, 10, pos2(100.5, 25.0)), Click::Outside);
        assert_eq!(click(&l, 10, pos2(-0.5, 25.0)), Click::Outside);
    }

    #[test]
    fn info_keys_that_mean_nothing_leave_the_panel_as_it_was() {
        let k = keymap();
        for name in ["g", "G", "<cr>", "<c-d>"] {
            assert_eq!(info_step(4, 3, 10, &key(name), &k), InfoStep::First(4), "{name}");
        }
        // Closing keys close wherever the panel is scrolled to.
        for name in ["<c-c>", "q", "<esc>"] {
            assert_eq!(info_step(0, 3, 3, &key(name), &k), InfoStep::Close, "{name}");
        }
    }

    #[test]
    fn info_scrolling_stops_at_both_ends() {
        let k = keymap();
        assert_eq!(info_step(0, 3, 10, &key("<up>"), &k), InfoStep::First(0));
        assert_eq!(info_step(0, 3, 10, &key("<c-p>"), &k), InfoStep::First(0));
        assert_eq!(info_step(7, 3, 10, &key("<down>"), &k), InfoStep::First(7));
        assert_eq!(info_step(6, 3, 10, &key("<c-n>"), &k), InfoStep::First(7));
        // Everything fits: nowhere to scroll.
        assert_eq!(info_step(0, 5, 3, &key("j"), &k), InfoStep::First(0));
        assert_eq!(info_step(0, 5, 0, &key("j"), &k), InfoStep::First(0));
    }
}
