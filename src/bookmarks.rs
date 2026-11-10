//! The named bookmarks: the `:bookmark` command and the picker, a panel like the contents.
use egui::{CursorIcon, Pos2, Response};

use crate::app::{App, Mode};
use crate::chrome::ListLayout;
use crate::field::Field;
use crate::form::{clamp_selected, keep_visible, wheel_step};
use crate::keymap::{Keymap, Stroke};
use crate::store::{self, Bookmark};

#[derive(Clone, Debug, Default)]
pub struct BookmarkState {
    /// The highlighted bookmark, as an index into the sidecar's list.
    pub selected: usize,
    /// The first row shown, as a place in the rows the filter lets through.
    pub first: usize,
    /// The part of a row the wheel has moved the list by and not yet scrolled.
    pub wheel_rest: f32,
    pub filter: Field,
    pub filtering: bool,
}

impl App {
    /// Adds a bookmark at the current place, or moves the one of that name.
    pub fn command_bookmark(&mut self, name: &str) {
        let name = store::normalise_bookmark_name(name);
        let name = name.as_str();
        if self.blank {
            self.say("no document open");
        } else if name.is_empty() {
            self.say("usage: bookmark {name}");
        } else {
            let here = self.view.anchor(&self.doc.layout);
            let moved = store::add_bookmark(&mut self.side.bookmarks, name, here);
            self.mark_dirty();
            self.say(if moved { format!("bookmark {name} moved") } else { format!("bookmark {name} added") });
        }
    }

    /// Opens the picker, with the highlight on the last bookmark at or before the view.
    pub fn open_bookmarks(&mut self) {
        if self.side.bookmarks.is_empty() {
            self.say("no bookmarks");
            return;
        }
        let page = self.view.current_page(&self.doc.layout);
        let selected = self.side.bookmarks.iter().rposition(|b| b.anchor.page <= page).unwrap_or(0);
        let mut state = BookmarkState { selected, ..Default::default() };
        state.first = keep_visible(0, selected, self.bookmark_layout(&state).fits, self.side.bookmarks.len());
        self.mode = Mode::Bookmarks(state);
    }

    /// The bookmarks the filter lets through, as indices.
    pub fn bookmark_rows(&self, state: &BookmarkState) -> Vec<usize> {
        store::bookmark_rows(&self.side.bookmarks, &state.filter.text)
    }

    /// The panel's layout for `state`, as drawn.
    pub fn bookmark_layout(&self, state: &BookmarkState) -> ListLayout {
        let filter = state.filtering || !state.filter.is_empty();
        self.list_layout(false, filter, self.bookmark_rows(state).len(), 0, Some(state.first))
    }

    fn bookmark_go(&mut self, index: usize) {
        self.mode = Mode::Normal;
        if let Some(b) = self.side.bookmarks.get(index) {
            self.jump_to(b.anchor);
        }
    }

    pub fn bookmark_key(&mut self, stroke: &Stroke) {
        let Mode::Bookmarks(state) = &self.mode else { return };
        let mut state = state.clone();
        match bookmark_step(&self.side.bookmarks, &mut state, stroke, &self.keymap) {
            Outcome::Close => {
                self.mode = Mode::Normal;
                return;
            }
            Outcome::Jump(row) => {
                self.bookmark_go(row);
                return;
            }
            Outcome::Delete(row) => {
                if let Some(gone) = store::remove_bookmark(&mut self.side.bookmarks, row) {
                    self.mark_dirty();
                    self.say(format!("bookmark {} deleted", gone.name));
                    if self.side.bookmarks.is_empty() {
                        self.mode = Mode::Normal;
                        return;
                    }
                }
                settle(&self.bookmark_rows(&state), &mut state);
            }
            Outcome::None => {}
        }
        let rows = self.bookmark_rows(&state);
        let at = rows.iter().position(|r| *r == state.selected).unwrap_or(0);
        state.first = keep_visible(state.first, at, self.bookmark_layout(&state).fits, rows.len());
        self.mode = Mode::Bookmarks(state);
    }

    /// The wheel over the open panel scrolls its list; elsewhere it does nothing.
    pub fn wheel_bookmarks(&mut self, pixels_y: f32, at: Option<Pos2>) {
        let Mode::Bookmarks(state) = &self.mode else { return };
        let layout = self.bookmark_layout(state);
        if !at.is_some_and(|p| layout.rect.contains(p)) {
            return;
        }
        let rows = self.bookmark_rows(state);
        if let Mode::Bookmarks(state) = &mut self.mode {
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
    pub fn pointer_bookmarks(&mut self, ctx: &egui::Context, response: &Response) {
        ctx.set_cursor_icon(CursorIcon::Default);
        let Some(at) = response.interact_pointer_pos().filter(|_| response.clicked()) else { return };
        let Mode::Bookmarks(state) = &self.mode else { return };
        let rows = self.bookmark_rows(state);
        let layout = self.bookmark_layout(state);
        if !layout.rect.contains(at) {
            self.mode = Mode::Normal;
            self.choice_closed = Some((ctx.input(|i| i.time), at));
            return;
        }
        let hit = (layout.first..rows.len()).take(layout.fits).find(|&i| Self::list_row_rect(&layout, i).contains(at));
        if let Some(i) = hit {
            self.bookmark_go(rows[i]);
            // Jumped by a click: its double click must not land on the page.
            self.choice_closed = Some((ctx.input(|i| i.time), at));
        }
    }
}

/// What a key does to the open picker, beyond changing its state.
#[derive(Debug, PartialEq)]
pub enum Outcome {
    None,
    /// Go to the bookmark at this index of the sidecar's list.
    Jump(usize),
    /// Delete the bookmark at this index.
    Delete(usize),
    Close,
}

/// Moves the highlight to the next row left at or after it, or the last one.
fn settle(rows: &[usize], state: &mut BookmarkState) {
    if let Some(row) = rows.iter().find(|r| **r >= state.selected).or(rows.last()) {
        state.selected = *row;
    }
}

/// Applies a key to `state`, given the bookmarks. A deletion is left to the caller, which then settles.
pub fn bookmark_step(list: &[Bookmark], state: &mut BookmarkState, stroke: &Stroke, keymap: &Keymap) -> Outcome {
    let rows = store::bookmark_rows(list, &state.filter.text);
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
    let rows = store::bookmark_rows(list, &state.filter.text);
    if let Some(row) = moved.and_then(|m| rows.get(m)) {
        state.selected = *row;
    }
    settle(&rows, state);
    Outcome::None
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Config;
    use crate::view::Anchor;

    fn keymap() -> Keymap {
        Keymap::new(&Config::load_str("").unwrap().keys).0
    }

    fn list(names: &[&str]) -> Vec<Bookmark> {
        names.iter().enumerate().map(|(i, n)| Bookmark { name: (*n).to_owned(), anchor: Anchor { page: i, y: 0.0, x: 0.0 } }).collect()
    }

    fn key(name: &str) -> Stroke {
        crate::keymap::parse_sequence(name).unwrap().remove(0)
    }

    fn press(list: &[Bookmark], state: &mut BookmarkState, name: &str) -> Outcome {
        bookmark_step(list, state, &key(name), &keymap())
    }

    /// Presses a key and carries out a deletion, as `App::bookmark_key` does.
    fn press_applying(list: &mut Vec<Bookmark>, state: &mut BookmarkState, name: &str) -> Outcome {
        let outcome = press(list, state, name);
        if let Outcome::Delete(row) = outcome {
            store::remove_bookmark(list, row);
            settle(&store::bookmark_rows(list, &state.filter.text), state);
        }
        outcome
    }

    fn filtering(text: &str) -> BookmarkState {
        let mut filter = Field::default();
        filter.insert(text);
        BookmarkState { filtering: true, filter, ..Default::default() }
    }

    #[test]
    fn delete_and_d_edit_the_filter_while_filtering() {
        let l = list(&["dad", "mum"]);
        let mut state = filtering("dd");
        assert_eq!(press(&l, &mut state, "d"), Outcome::None);
        assert_eq!(state.filter.text, "ddd");
        // The cursor is at the end, so Delete has nothing to remove there.
        assert_eq!(press(&l, &mut state, "<del>"), Outcome::None);
        assert_eq!(state.filter.text, "ddd");
        press(&l, &mut state, "<left>");
        assert_eq!(press(&l, &mut state, "<del>"), Outcome::None);
        assert_eq!(state.filter.text, "dd");
        assert_eq!(l.len(), 2);
    }

    #[test]
    fn delete_and_d_delete_when_not_filtering() {
        let l = list(&["a", "b", "c"]);
        let mut state = BookmarkState { selected: 1, ..Default::default() };
        assert_eq!(press(&l, &mut state, "d"), Outcome::Delete(1));
        assert_eq!(press(&l, &mut state, "<del>"), Outcome::Delete(1));
    }

    #[test]
    fn deleting_the_last_row_moves_the_highlight_up() {
        let mut l = list(&["a", "b", "c"]);
        let mut state = BookmarkState { selected: 2, ..Default::default() };
        assert_eq!(press_applying(&mut l, &mut state, "d"), Outcome::Delete(2));
        assert_eq!((l.len(), state.selected), (2, 1));
        press_applying(&mut l, &mut state, "<del>");
        press_applying(&mut l, &mut state, "d");
        assert_eq!((l.len(), state.selected), (0, 0));
    }

    #[test]
    fn enter_l_and_d_on_an_empty_filtered_list_do_nothing() {
        let l = list(&["a", "b"]);
        let mut state = filtering("zzz");
        assert_eq!(press(&l, &mut state, "<cr>"), Outcome::None);
        state.filtering = false;
        assert_eq!(press(&l, &mut state, "l"), Outcome::None);
        assert_eq!(press(&l, &mut state, "d"), Outcome::None);
        assert_eq!(press(&l, &mut state, "<del>"), Outcome::None);
        assert_eq!(l.len(), 2);
    }

    #[test]
    fn the_selection_stays_in_range_after_a_delete_and_a_filter_change() {
        let mut l = list(&["alpha", "beta", "gamma", "alpine"]);
        let mut state = BookmarkState { selected: 3, ..Default::default() };
        press_applying(&mut l, &mut state, "d");
        assert!(state.selected < l.len());
        state.selected = 2;
        press(&l, &mut state, "/");
        press(&l, &mut state, "b");
        assert_eq!(store::bookmark_rows(&l, &state.filter.text), [1]);
        assert_eq!(state.selected, 1);
        press(&l, &mut state, "z");
        assert_eq!(state.selected, 1, "no rows: the highlight stays put");
        press(&l, &mut state, "<bs>");
        press(&l, &mut state, "<bs>");
        assert!(state.selected < l.len());
    }

    #[test]
    fn escape_clears_the_filter_first_then_closes() {
        let l = list(&["a", "b"]);
        let mut state = filtering("b");
        assert_eq!(press(&l, &mut state, "<esc>"), Outcome::None);
        assert!(!state.filtering && state.filter.is_empty());
        assert_eq!(press(&l, &mut state, "<esc>"), Outcome::Close);
    }

    #[test]
    fn enter_and_l_jump_and_q_closes() {
        let l = list(&["a", "b"]);
        let mut state = BookmarkState { selected: 1, ..Default::default() };
        assert_eq!(press(&l, &mut state, "<cr>"), Outcome::Jump(1));
        assert_eq!(press(&l, &mut state, "l"), Outcome::Jump(1));
        assert_eq!(press(&l, &mut state, "q"), Outcome::Close);
    }
}
