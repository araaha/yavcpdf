//! Finding text across the document. Pages are scanned as their text arrives.
use pdfcraft_render::PageText;

#[derive(Clone, Debug, PartialEq)]
pub struct Match {
    pub page: usize,
    pub start: usize,
    pub end: usize,
    pub rects: Vec<[f32; 4]>,
}

impl Match {
    fn top(&self) -> f32 {
        self.rects.first().map_or(0.0, |r| r[1])
    }
}

#[derive(Clone, Debug)]
pub struct Search {
    pub query: String,
    pub forward: bool,
    case_sensitive: bool,
    pub matches: Vec<Match>,
    pub current: Option<usize>,
    scanned: Vec<bool>,
    /// A jump waiting for its first match: from this page and y.
    pending_jump: Option<(usize, f32)>,
}

impl Search {
    pub fn new(query: String, forward: bool, smartcase: bool, pages: usize, from: (usize, f32)) -> Self {
        let case_sensitive = smartcase && query.chars().any(char::is_uppercase);
        Self { query, forward, case_sensitive, matches: Vec::new(), current: None, scanned: vec![false; pages], pending_jump: Some(from) }
    }

    /// The same search over a reloaded document, staying where the view is.
    pub fn restarted(&self, pages: usize) -> Self {
        Self { matches: Vec::new(), current: None, scanned: vec![false; pages], pending_jump: None, ..self.clone() }
    }

    pub fn is_scanned(&self, page: usize) -> bool {
        self.scanned.get(page).copied().unwrap_or(true)
    }

    pub fn is_complete(&self) -> bool {
        self.scanned.iter().all(|s| *s)
    }

    /// Adds a page's matches, keeping them in document order.
    pub fn scan(&mut self, page: usize, text: &PageText) {
        let Some(done) = self.scanned.get_mut(page) else { return };
        if std::mem::replace(done, true) {
            return;
        }
        let found: Vec<Match> = text
            .find_opts(&self.query, self.case_sensitive, false)
            .into_iter()
            .map(|r| Match { page, start: r.start, end: r.end, rects: text.line_rects(r) })
            .collect();
        let at = self.matches.partition_point(|m| m.page < page);
        if let Some(current) = self.current.as_mut().filter(|c| **c >= at) {
            *current += found.len();
        }
        self.matches.splice(at..at, found);
    }

    /// Settles a waiting jump if it can be: the first match after the start in the search's
    /// direction, wrapping around once every page is known. Returns the match to show.
    pub fn resolve_jump(&mut self) -> Option<&Match> {
        let (page, y) = self.pending_jump?;
        let after = |m: &Match| (m.page, m.top()) >= (page, y);
        let index = if self.forward {
            let scanned_up_to_it = |m: &Match| (page..m.page).all(|p| self.is_scanned(p));
            self.matches.iter().position(after).filter(|&i| scanned_up_to_it(&self.matches[i]))
        } else {
            let before = |m: &Match| (m.page, m.top()) < (page, y);
            let scanned_back_to_it = |m: &Match| (m.page + 1..=page).all(|p| self.is_scanned(p));
            self.matches.iter().rposition(before).filter(|&i| scanned_back_to_it(&self.matches[i]))
        };
        let index = match index {
            Some(i) => i,
            None if self.is_complete() && !self.matches.is_empty() => {
                if self.forward {
                    0
                } else {
                    self.matches.len() - 1
                }
            }
            None => {
                if self.is_complete() {
                    self.pending_jump = None;
                }
                return None;
            }
        };
        self.pending_jump = None;
        self.current = Some(index);
        self.matches.get(index)
    }

    /// Moves to the next match in the direction, or against it with `reverse` (`N`).
    pub fn advance(&mut self, reverse: bool) -> Option<&Match> {
        let forward = self.forward != reverse;
        let len = self.matches.len();
        if len == 0 {
            return None;
        }
        let next = match self.current {
            Some(i) if forward => (i + 1) % len,
            Some(i) => (i + len - 1) % len,
            None if forward => 0,
            None => len - 1,
        };
        self.current = Some(next);
        self.pending_jump = None;
        self.matches.get(next)
    }

    /// `3/41`, with `+` while pages are still being searched.
    pub fn status(&self) -> String {
        let more = if self.is_complete() { "" } else { "+" };
        if self.matches.is_empty() {
            return if self.is_complete() { format!("no match for \"{}\"", self.query) } else { "searching".to_owned() };
        }
        format!("{}/{}{more}", self.current.map_or(0, |c| c + 1), self.matches.len())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pdfcraft_render::TextGlyph;

    /// A page with one line per entry of `lines`, 20 points apart.
    fn page(lines: &[&str]) -> PageText {
        let mut text = PageText::default();
        for (n, line) in lines.iter().enumerate() {
            for (i, c) in line.chars().enumerate() {
                let (x, y) = (i as f32 * 5.0, n as f32 * 20.0);
                text.glyphs.push(TextGlyph {
                    text: c.to_string(),
                    rect: [x, y, x + 5.0, y + 10.0],
                    quad: [[x, y + 10.0], [x + 5.0, y + 10.0], [x + 5.0, y], [x, y]],
                    direction: [1.0, 0.0],
                });
                text.line_of.push(n as u32);
                text.space_before.push(false);
            }
        }
        text
    }

    fn search(query: &str, forward: bool, pages: usize, from: (usize, f32)) -> Search {
        Search::new(query.to_owned(), forward, true, pages, from)
    }

    fn at(m: Option<&Match>) -> Option<(usize, f32)> {
        m.map(|m| (m.page, m.top()))
    }

    #[test]
    fn pages_may_arrive_in_any_order() {
        let mut s = search("x", true, 3, (0, 0.0));
        s.scan(2, &page(&["x"]));
        s.scan(0, &page(&["x", "x"]));
        s.scan(1, &page(&["none"]));
        assert_eq!(s.matches.iter().map(|m| m.page).collect::<Vec<_>>(), [0, 0, 2]);
    }

    #[test]
    fn a_page_is_only_scanned_once() {
        let mut s = search("x", true, 1, (0, 0.0));
        s.scan(0, &page(&["x"]));
        s.scan(0, &page(&["x"]));
        assert_eq!(s.matches.len(), 1);
    }

    #[test]
    fn the_jump_goes_to_the_next_match_after_the_view() {
        let mut s = search("x", true, 2, (0, 15.0));
        s.scan(0, &page(&["x", "x"]));
        s.scan(1, &page(&["x"]));
        assert_eq!(at(s.resolve_jump()), Some((0, 20.0)));
        assert_eq!(s.status(), "2/3");
    }

    #[test]
    fn the_jump_waits_for_the_pages_between() {
        let mut s = search("x", true, 3, (0, 0.0));
        s.scan(2, &page(&["x"]));
        assert!(s.resolve_jump().is_none());
        s.scan(0, &page(&["none"]));
        s.scan(1, &page(&["x"]));
        assert_eq!(at(s.resolve_jump()), Some((1, 0.0)));
    }

    #[test]
    fn the_jump_wraps_around_once_everything_is_known() {
        let mut s = search("x", true, 2, (1, 500.0));
        s.scan(0, &page(&["x"]));
        assert!(s.resolve_jump().is_none());
        s.scan(1, &page(&["none"]));
        assert_eq!(at(s.resolve_jump()), Some((0, 0.0)));
    }

    #[test]
    fn a_backward_search_goes_up_from_the_view() {
        let mut s = search("x", false, 2, (1, 0.0));
        s.scan(1, &page(&["x"]));
        s.scan(0, &page(&["x", "x"]));
        assert_eq!(at(s.resolve_jump()), Some((0, 20.0)));
    }

    #[test]
    fn n_follows_the_direction_and_shift_n_opposes_it() {
        let mut s = search("x", true, 1, (0, 0.0));
        s.scan(0, &page(&["x", "x", "x"]));
        s.resolve_jump();
        assert_eq!(at(s.advance(false)), Some((0, 20.0)));
        assert_eq!(at(s.advance(true)), Some((0, 0.0)));
        assert_eq!(at(s.advance(true)), Some((0, 40.0)), "wraps");
        s.forward = false;
        assert_eq!(at(s.advance(false)), Some((0, 20.0)));
        assert_eq!(at(s.advance(true)), Some((0, 40.0)));
    }

    #[test]
    fn the_current_match_stays_put_when_earlier_pages_arrive() {
        let mut s = search("x", true, 2, (1, 0.0));
        s.scan(1, &page(&["x"]));
        assert_eq!(at(s.resolve_jump()), Some((1, 0.0)));
        s.scan(0, &page(&["x", "x"]));
        assert_eq!(s.current, Some(2));
        assert_eq!(s.matches[2].page, 1);
    }

    #[test]
    fn a_restarted_search_finds_again_without_moving_the_view() {
        let mut s = search("x", true, 1, (0, 0.0));
        s.scan(0, &page(&["x"]));
        s.resolve_jump();
        let mut again = s.restarted(2);
        assert!(again.matches.is_empty() && !again.is_complete());
        again.scan(0, &page(&["x"]));
        again.scan(1, &page(&["x"]));
        assert_eq!(again.matches.len(), 2);
        assert!(again.resolve_jump().is_none());
    }

    #[test]
    fn smartcase_needs_an_upper_case_letter() {
        let text = page(&["Word", "word"]);
        let mut s = search("word", true, 1, (0, 0.0));
        s.scan(0, &text);
        assert_eq!(s.matches.len(), 2);
        let mut s = search("Word", true, 1, (0, 0.0));
        s.scan(0, &text);
        assert_eq!(s.matches.len(), 1);
    }

    #[test]
    fn no_match_is_reported_and_n_does_nothing() {
        let mut s = search("zzz", true, 1, (0, 0.0));
        assert_eq!(s.status(), "searching");
        s.scan(0, &page(&["x"]));
        assert!(s.resolve_jump().is_none());
        assert!(s.advance(false).is_none());
        assert_eq!(s.status(), "no match for \"zzz\"");
    }
}
