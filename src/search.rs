//! Finding text across the document. Pages are scanned as their text arrives.
use pdfcraft_render::PageText;

use crate::view::Layout;

/// Where a match's top left is as shown, `[x, y]`, when the pages are turned. `None` is for pages
/// shown upright, where matches stay in the order they were found and are placed by page y.
pub type Place<'a> = Option<&'a dyn Fn(&Match) -> [f32; 2]>;

/// Where a match starts in the pages as `layout` shows them.
pub fn shown_top_left(layout: &Layout) -> impl Fn(&Match) -> [f32; 2] + '_ {
    move |m| {
        let r = m.rects.first().map_or([0.0; 4], |r| layout.shown_rect(m.page, *r));
        [r[0], r[1]]
    }
}

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
    /// A jump waiting for its first match: from this page and y as shown.
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

    /// The matches' indices in reading order as shown: as found for `None`, otherwise by page, then
    /// where they start as shown.
    fn order(&self, place: Place) -> Vec<usize> {
        let mut order: Vec<usize> = (0..self.matches.len()).collect();
        if let Some(at) = place {
            let key = |i: &usize| (self.matches[*i].page, at(&self.matches[*i]));
            order.sort_by(|a, b| {
                let (ka, kb) = (key(a), key(b));
                ka.0.cmp(&kb.0).then(ka.1[1].total_cmp(&kb.1[1])).then(ka.1[0].total_cmp(&kb.1[0]))
            });
        }
        order
    }

    /// Settles a waiting jump if it can be: the first match after the start in the search's
    /// direction, wrapping around once every page is known. Returns the match to show.
    /// The start's y is as shown, so `place` says where the matches are as shown.
    pub fn resolve_jump(&mut self, place: Place) -> Option<&Match> {
        let (page, y) = self.pending_jump?;
        let key = |i: usize| {
            let m = &self.matches[i];
            let [x, y] = place.map_or_else(|| [0.0, m.top()], |at| at(m));
            (m.page, y, x)
        };
        let cmp = |a: &usize, b: &usize| {
            if place.is_none() {
                return a.cmp(b);
            }
            let (ka, kb) = (key(*a), key(*b));
            ka.0.cmp(&kb.0).then(ka.1.total_cmp(&kb.1)).then(ka.2.total_cmp(&kb.2))
        };
        // Whether a match is at or after the start: by page, then y as shown.
        let after = |i: usize| {
            let k = key(i);
            k.0 > page || (k.0 == page && k.1.total_cmp(&y).is_ge())
        };
        let all = 0..self.matches.len();
        let index = if self.forward {
            let scanned_up_to_it = |i: &usize| (page..self.matches[*i].page).all(|p| self.is_scanned(p));
            all.clone().filter(|&i| after(i)).min_by(cmp).filter(scanned_up_to_it)
        } else {
            let scanned_back_to_it = |i: &usize| (self.matches[*i].page + 1..=page).all(|p| self.is_scanned(p));
            all.clone().filter(|&i| !after(i)).max_by(cmp).filter(scanned_back_to_it)
        };
        let index = match index {
            Some(i) => i,
            None if self.is_complete() && !self.matches.is_empty() => {
                let wrapped = if self.forward { all.min_by(cmp) } else { all.max_by(cmp) };
                wrapped.unwrap_or(0)
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

    /// Restarts a waiting jump from `from`, a page and y as shown.
    pub fn restart_jump(&mut self, from: (usize, f32)) {
        if self.pending_jump.is_some() {
            self.pending_jump = Some(from);
        }
    }

    /// Moves to the next match in the direction, or against it with `reverse` (`N`), in the order
    /// the matches are shown in.
    pub fn advance(&mut self, reverse: bool, place: Place) -> Option<&Match> {
        let forward = self.forward != reverse;
        let len = self.matches.len();
        if len == 0 {
            return None;
        }
        let order = self.order(place);
        let at = self.current.and_then(|c| order.iter().position(|&i| i == c));
        let next = match at {
            Some(i) if forward => order[(i + 1) % len],
            Some(i) => order[(i + len - 1) % len],
            None if forward => order[0],
            None => order[len - 1],
        };
        self.current = Some(next);
        self.pending_jump = None;
        self.matches.get(next)
    }

    /// `3/41`, with `+` while pages are still being searched. The place of the current match is
    /// its rank in the order shown when `place` is given.
    pub fn status(&self, place: Place) -> String {
        let more = if self.is_complete() { "" } else { "+" };
        if self.matches.is_empty() {
            return if self.is_complete() { format!("no match for \"{}\"", self.query) } else { "searching".to_owned() };
        }
        let rank = self.current.map_or(0, |c| match place {
            None => c + 1,
            Some(at) => {
                let key = |m: &Match| (m.page, at(m));
                let here = key(&self.matches[c]);
                let before = |m: &Match| {
                    let k = key(m);
                    k.0.cmp(&here.0).then(k.1[1].total_cmp(&here.1[1])).then(k.1[0].total_cmp(&here.1[0])).is_lt()
                };
                self.matches.iter().filter(|m| before(m)).count() + 1
            }
        });
        format!("{rank}/{}{more}", self.matches.len())
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
        assert_eq!(at(s.resolve_jump(None)), Some((0, 20.0)));
        assert_eq!(s.status(None), "2/3");
    }

    #[test]
    fn the_jump_waits_for_the_pages_between() {
        let mut s = search("x", true, 3, (0, 0.0));
        s.scan(2, &page(&["x"]));
        assert!(s.resolve_jump(None).is_none());
        s.scan(0, &page(&["none"]));
        s.scan(1, &page(&["x"]));
        assert_eq!(at(s.resolve_jump(None)), Some((1, 0.0)));
    }

    #[test]
    fn the_jump_wraps_around_once_everything_is_known() {
        let mut s = search("x", true, 2, (1, 500.0));
        s.scan(0, &page(&["x"]));
        assert!(s.resolve_jump(None).is_none());
        s.scan(1, &page(&["none"]));
        assert_eq!(at(s.resolve_jump(None)), Some((0, 0.0)));
    }

    #[test]
    fn a_backward_search_goes_up_from_the_view() {
        let mut s = search("x", false, 2, (1, 0.0));
        s.scan(1, &page(&["x"]));
        s.scan(0, &page(&["x", "x"]));
        assert_eq!(at(s.resolve_jump(None)), Some((0, 20.0)));
    }

    #[test]
    fn n_follows_the_direction_and_shift_n_opposes_it() {
        let mut s = search("x", true, 1, (0, 0.0));
        s.scan(0, &page(&["x", "x", "x"]));
        s.resolve_jump(None);
        assert_eq!(at(s.advance(false, None)), Some((0, 20.0)));
        assert_eq!(at(s.advance(true, None)), Some((0, 0.0)));
        assert_eq!(at(s.advance(true, None)), Some((0, 40.0)), "wraps");
        s.forward = false;
        assert_eq!(at(s.advance(false, None)), Some((0, 20.0)));
        assert_eq!(at(s.advance(true, None)), Some((0, 40.0)));
    }

    #[test]
    fn the_current_match_stays_put_when_earlier_pages_arrive() {
        let mut s = search("x", true, 2, (1, 0.0));
        s.scan(1, &page(&["x"]));
        assert_eq!(at(s.resolve_jump(None)), Some((1, 0.0)));
        s.scan(0, &page(&["x", "x"]));
        assert_eq!(s.current, Some(2));
        assert_eq!(s.matches[2].page, 1);
    }

    #[test]
    fn a_restarted_search_finds_again_without_moving_the_view() {
        let mut s = search("x", true, 1, (0, 0.0));
        s.scan(0, &page(&["x"]));
        s.resolve_jump(None);
        let mut again = s.restarted(2);
        assert!(again.matches.is_empty() && !again.is_complete());
        again.scan(0, &page(&["x"]));
        again.scan(1, &page(&["x"]));
        assert_eq!(again.matches.len(), 2);
        assert!(again.resolve_jump(None).is_none());
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
        assert_eq!(s.status(None), "searching");
        s.scan(0, &page(&["x"]));
        assert!(s.resolve_jump(None).is_none());
        assert!(s.advance(false, None).is_none());
        assert_eq!(s.status(None), "no match for \"zzz\"");
    }

    /// The page tops as shown, for pages 100 by 100 turned to `rotation`.
    fn turned(rotation: i64) -> Layout {
        let mut layout = Layout::new(vec![[100.0, 100.0]], 8.0);
        layout.set_rotation(rotation);
        layout
    }

    fn page_y(m: Option<&Match>) -> Option<f32> {
        m.map(Match::top)
    }

    #[test]
    fn a_jump_from_a_view_y_is_found_in_shown_space_at_180() {
        let layout = turned(180);
        let place = shown_top_left(&layout);
        // Lines at page y 0, 20, 40 are shown at y 90, 70, 50; the view is at 60.
        let mut s = search("x", true, 1, (0, 60.0));
        s.scan(0, &page(&["x", "x", "x"]));
        assert_eq!(page_y(s.resolve_jump(Some(&place))), Some(20.0));
        // Going on follows the screen: down to the line shown at 90, then around to the top one.
        assert_eq!(page_y(s.advance(false, Some(&place))), Some(0.0));
        assert_eq!(page_y(s.advance(false, Some(&place))), Some(40.0));
        // Backwards from the same place is the line shown above it.
        let mut s = search("x", false, 1, (0, 60.0));
        s.scan(0, &page(&["x", "x", "x"]));
        assert_eq!(page_y(s.resolve_jump(Some(&place))), Some(40.0));
    }

    #[test]
    fn a_jump_from_a_view_y_is_found_in_shown_space_at_90() {
        let layout = turned(90);
        let place = shown_top_left(&layout);
        // Lines start at page x 0, 5, 10, which are the shown ys; the view is at 7.
        let mut s = search("x", true, 1, (0, 7.0));
        s.scan(0, &page(&["x", " x", "  x"]));
        assert_eq!(page_y(s.resolve_jump(Some(&place))), Some(40.0));
        let mut s = search("x", false, 1, (0, 7.0));
        s.scan(0, &page(&["x", " x", "  x"]));
        assert_eq!(page_y(s.resolve_jump(Some(&place))), Some(20.0));
    }

    /// A small deterministic xorshift generator.
    struct Rng(u64);

    impl Rng {
        fn next(&mut self) -> u64 {
            self.0 ^= self.0 << 13;
            self.0 ^= self.0 >> 7;
            self.0 ^= self.0 << 17;
            self.0
        }

        fn below(&mut self, n: u64) -> u64 {
            self.next() % n
        }
    }

    const TURNS: [i64; 4] = [0, 90, 180, 270];

    /// Random text for a page: up to 6 lines of up to 12 columns, each cell an `x` or blank.
    fn random_lines(rng: &mut Rng) -> Vec<String> {
        (0..1 + rng.below(6)).map(|_| (0..1 + rng.below(12)).map(|_| if rng.below(3) == 0 { 'x' } else { ' ' }).collect()).collect()
    }

    fn rotated(pages: usize, rotation: i64) -> Layout {
        let mut layout = Layout::new((0..pages).map(|p| [200.0 + 10.0 * p as f32, 300.0]).collect(), 8.0);
        layout.set_rotation(rotation);
        layout
    }

    /// The matches in on-screen reading order, by brute force: page, shown y, shown x.
    fn reference(s: &Search, layout: &Layout) -> Vec<Match> {
        let place = shown_top_left(layout);
        let mut all = s.matches.clone();
        all.sort_by(|a, b| {
            let (pa, pb) = (place(a), place(b));
            a.page.cmp(&b.page).then(pa[1].total_cmp(&pb[1])).then(pa[0].total_cmp(&pb[0]))
        });
        all
    }

    fn texts(seed: u64, pages: usize) -> Vec<PageText> {
        let mut rng = Rng(seed);
        (0..pages).map(|_| page(&random_lines(&mut rng).iter().map(String::as_str).collect::<Vec<_>>())).collect()
    }

    fn scrambled(rng: &mut Rng, n: usize) -> Vec<usize> {
        let mut order: Vec<usize> = (0..n).collect();
        for i in (1..n).rev() {
            order.swap(i, rng.below(i as u64 + 1) as usize);
        }
        order
    }

    #[test]
    fn n_walks_the_screen_in_reading_order_and_wraps_at_every_rotation() {
        for seed in 1..40u64 {
            for rotation in TURNS {
                let pages = 1 + (seed % 4) as usize;
                let layout = rotated(pages, rotation);
                let place = shown_top_left(&layout);
                let texts = texts(seed * 7919, pages);
                let mut rng = Rng(seed * 104_729);
                for forward in [true, false] {
                    let mut s = search("x", forward, pages, (0, 0.0));
                    for p in scrambled(&mut rng, pages) {
                        s.scan(p, &texts[p]);
                    }
                    let want = reference(&s, &layout);
                    if want.is_empty() {
                        assert!(s.advance(false, Some(&place)).is_none());
                        continue;
                    }
                    // Forward from nothing starts at the first, backward at the last.
                    let first = if forward { 0 } else { want.len() - 1 };
                    assert_eq!(s.advance(false, Some(&place)), Some(&want[first]), "seed {seed} rot {rotation}");
                    for step in 1..=2 * want.len() {
                        let index = if forward { (first + step) % want.len() } else { (first + want.len() * 2 - step) % want.len() };
                        assert_eq!(s.advance(false, Some(&place)), Some(&want[index]), "seed {seed} rot {rotation} step {step}");
                    }
                    // N walks the other way, and back to where it was.
                    let here = s.current.map(|c| s.matches[c].clone()).unwrap();
                    let back = s.advance(true, Some(&place)).cloned().unwrap();
                    let again = s.advance(false, Some(&place)).cloned().unwrap();
                    assert_eq!(again, here);
                    let pos = want.iter().position(|m| *m == here).unwrap();
                    let opposite = if forward { (pos + want.len() - 1) % want.len() } else { (pos + 1) % want.len() };
                    assert_eq!(back, want[opposite]);
                }
            }
        }
    }

    #[test]
    fn resolve_jump_matches_a_brute_force_reference_at_every_rotation() {
        for seed in 1..30u64 {
            for rotation in TURNS {
                let pages = 1 + (seed % 3) as usize;
                let layout = rotated(pages, rotation);
                let place = shown_top_left(&layout);
                let texts = texts(seed * 31, pages);
                let mut rng = Rng(seed * 977);
                for forward in [true, false] {
                    for _ in 0..12 {
                        let (page_at, y) = (rng.below(pages as u64) as usize, rng.below(40) as f32 * 7.5 - 5.0);
                        let mut s = Search::new("x".to_owned(), forward, true, pages, (page_at, y));
                        for p in scrambled(&mut rng, pages) {
                            s.scan(p, &texts[p]);
                        }
                        let want = reference(&s, &layout);
                        let key = |m: &Match| (m.page, place(m)[1]);
                        let expected = if want.is_empty() {
                            None
                        } else if forward {
                            want.iter().find(|m| key(m) >= (page_at, y)).or(want.first())
                        } else {
                            want.iter().rev().find(|m| key(m) < (page_at, y)).or(want.last())
                        };
                        let got = s.resolve_jump(Some(&place)).cloned();
                        assert_eq!(got.as_ref(), expected, "seed {seed} rot {rotation} fwd {forward} from {page_at}/{y}");
                    }
                }
            }
        }
    }

    #[test]
    fn the_current_match_stays_the_same_when_pages_arrive_and_the_rotation_changes() {
        for seed in 1..40u64 {
            let pages = 4;
            let texts = texts(seed * 13, pages);
            let mut rng = Rng(seed * 6007);
            let mut s = search("x", true, pages, (0, 0.0));
            for p in scrambled(&mut rng, pages) {
                let before = s.current.map(|c| s.matches[c].clone());
                s.scan(p, &texts[p]);
                if let Some(before) = before {
                    assert_eq!(s.current.map(|c| &s.matches[c]), Some(&before), "seed {seed}: page {p} arrived");
                }
                for _ in 0..3 {
                    // Turn between steps: each step is relative to the layout at that step.
                    let layout = rotated(pages, TURNS[rng.below(4) as usize]);
                    let place = shown_top_left(&layout);
                    let reverse = rng.below(2) == 0;
                    let prev = s.current.map(|c| s.matches[c].clone());
                    let got = s.advance(reverse, Some(&place)).cloned();
                    if let Some(got) = got {
                        let want = reference(&s, &layout);
                        let at = want.iter().position(|m| *m == got).unwrap();
                        if let Some(prev) = prev {
                            let from = want.iter().position(|m| *m == prev).unwrap();
                            let n = want.len();
                            assert_eq!(at, if reverse { (from + n - 1) % n } else { (from + 1) % n }, "seed {seed}");
                        }
                    }
                }
            }
            assert!(s.is_complete());
        }
    }

    #[test]
    fn at_rotation_zero_a_place_agrees_with_none() {
        for seed in 1..30u64 {
            let pages = 3;
            let layout = rotated(pages, 0);
            let place = shown_top_left(&layout);
            let texts = texts(seed * 5, pages);
            for forward in [true, false] {
                let (mut a, mut b) = (search("x", forward, pages, (1, 25.0)), search("x", forward, pages, (1, 25.0)));
                for (p, text) in texts.iter().enumerate() {
                    a.scan(p, text);
                    b.scan(p, text);
                }
                assert_eq!(a.resolve_jump(None).cloned(), b.resolve_jump(Some(&place)).cloned(), "seed {seed}");
                for reverse in [false, true, false, false] {
                    assert_eq!(a.advance(reverse, None).cloned(), b.advance(reverse, Some(&place)).cloned(), "seed {seed}");
                }
            }
        }
    }

    #[test]
    fn the_status_counts_in_the_order_shown() {
        let layout = rotated(1, 180);
        let place = shown_top_left(&layout);
        let mut s = search("x", true, 1, (0, 0.0));
        s.scan(0, &page(&["x", "x", "x"]));
        for step in 1..=3 {
            s.advance(false, Some(&place));
            assert_eq!(s.status(Some(&place)), format!("{step}/3"));
        }
        assert_eq!(s.status(None), "1/3");
    }
}
