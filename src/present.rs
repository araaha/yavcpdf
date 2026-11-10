//! Presentation mode: one page fit to the screen, nothing else showing, and an optional timer.
use std::time::{Duration, Instant};

use egui::{Context, MouseWheelUnit, PointerButton, Sense, Ui, Vec2};

use crate::app::{App, Fit, HotbarState, Mode, Tool};
use crate::keymap::{Action, Stroke};
use crate::view::{Anchor, View};

/// Pages kept rendered ahead of the one showing, and behind it, so a page turn is instant.
pub const PREFETCH_AHEAD: usize = 2;
pub const PREFETCH_BEHIND: usize = 1;
/// How long the wheel has to be still for a trackpad's next flick to turn the page again.
const GESTURE_GAP: f64 = 0.3;
/// The shortest time between two turns by the notches of a wheel.
const NOTCH_GAP: f64 = 0.05;
/// How long after leaving the view is held on the page it goes back to, for the window to settle.
const RETURN_SETTLE: Duration = Duration::from_millis(500);
/// The longest it is held, however long the window keeps changing.
const RETURN_LIMIT: Duration = Duration::from_secs(1);

/// What presenting changes and leaving puts back.
#[derive(Clone, Copy, Debug)]
pub struct Saved {
    fullscreen: bool,
    statusbar: bool,
    hotbar: HotbarState,
    tool: Option<Tool>,
    portal_window: bool,
    fit: Option<Fit>,
    zoom: f32,
    render_zoom: f32,
    /// Where the view was, and the page in the middle of it.
    anchor: Anchor,
    page: usize,
}

/// A presentation under way.
#[derive(Clone, Debug)]
pub struct Presentation {
    saved: Saved,
    pub page: usize,
    started: Instant,
    timer: bool,
    keys: Keys,
    wheel: Wheel,
}

/// The place and zoom to keep for the document while presenting: the zoom it had before, and the top
/// of the page showing, or the place it had if that is the page it started on.
pub fn kept_place(saved: Anchor, saved_page: usize, zoom: f32, page: usize) -> (Anchor, f32) {
    if page == saved_page { (saved, zoom) } else { (Anchor { page, y: 0.0, x: saved.x }, zoom) }
}

/// A page turn, or what else a key asks for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Step {
    Next,
    Prev,
    First,
    Last,
    /// A page by its number, from 1.
    Goto(u32),
    Timer,
    Reset,
}

/// The digits and the first `g` typed, waiting for what follows.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Keys {
    count: Option<u32>,
    g: bool,
}

impl Keys {
    /// Whether something typed is waiting for what follows.
    pub fn pending(&self) -> bool {
        self.count.is_some() || self.g
    }

    /// What a key asks for, once it is typed in full.
    pub fn feed(&mut self, stroke: &Stroke) -> Option<Step> {
        let (count, g) = (self.count.take(), std::mem::take(&mut self.g));
        if let Some(d) = stroke.text().and_then(|c| c.to_digit(10)).filter(|d| *d != 0 || count.is_some()) {
            self.count = Some(count.unwrap_or(0).saturating_mul(10).saturating_add(d));
            return None;
        }
        let text = stroke.text();
        let named = |key: &str| stroke.is(key);
        let step = if g {
            return (text == Some('g')).then_some(count.map_or(Step::First, Step::Goto));
        } else if text == Some('g') {
            self.count = count;
            self.g = true;
            return None;
        } else if named("cr") {
            return count.map(Step::Goto);
        } else if text == Some('G') {
            count.map_or(Step::Last, Step::Goto)
        } else if stroke.key == "space" && stroke.shift {
            Step::Prev
        } else if matches!(text, Some('J' | ' ' | 'j' | 'l')) || ["pagedown", "down", "right"].into_iter().any(named) {
            Step::Next
        } else if matches!(text, Some('K' | 'k' | 'h'))
            || ["pageup", "bs", "up", "left"].into_iter().any(named)
            || stroke.key == "space" && stroke.shift
        {
            Step::Prev
        } else if named("home") {
            Step::First
        } else if named("end") {
            Step::Last
        } else if text == Some('t') {
            Step::Timer
        } else if text == Some('r') {
            Step::Reset
        } else {
            return None;
        };
        Some(step)
    }
}

/// Which wheel events turn a page: one per notch of a wheel, and one per flick of a trackpad,
/// which sends events for as long as it coasts.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Wheel {
    last_event: Option<f64>,
    last_turn: Option<f64>,
}

impl Wheel {
    /// The turn an event at `now` (seconds) scrolling by `dy` makes: down is forwards.
    pub fn feed(&mut self, now: f64, dy: f32, notch: bool) -> Option<Step> {
        if dy == 0.0 {
            return None;
        }
        let quiet = self.last_event.is_none_or(|t| now - t >= GESTURE_GAP);
        self.last_event = Some(now);
        let turns = if notch { self.last_turn.is_none_or(|t| now - t >= NOTCH_GAP) } else { quiet };
        if !turns {
            return None;
        }
        self.last_turn = Some(now);
        Some(if dy < 0.0 { Step::Next } else { Step::Prev })
    }
}

/// The page a step goes to, in a document of `len` pages.
pub fn target_page(current: usize, len: usize, step: Step) -> usize {
    let last = len.saturating_sub(1);
    match step {
        Step::Next => (current + 1).min(last),
        Step::Prev => current.saturating_sub(1),
        Step::First => 0,
        Step::Last => last,
        Step::Goto(n) => (n.saturating_sub(1) as usize).min(last),
        Step::Timer | Step::Reset => current.min(last),
    }
}

/// The zoom that fits the whole of a page, of `page` points, to `area`, and the document's y at the top
/// of a view that centres it when the page's top is at `top`. Centred across is `x = 0`.
pub fn fit_whole(area: Vec2, page: [f32; 2], top: f32) -> (f32, f32) {
    let area = area.max(Vec2::splat(1.0));
    let zoom = (area.x / page[0].max(1.0)).min(area.y / page[1].max(1.0));
    (zoom, top + page[1] / 2.0 - area.y / zoom / 2.0)
}

/// Elapsed seconds as `mm:ss`, or `h:mm:ss` from an hour on.
pub fn format_elapsed(seconds: u64) -> String {
    let (h, m, s) = (seconds / 3600, seconds / 60 % 60, seconds % 60);
    if h == 0 { format!("{m:02}:{s:02}") } else { format!("{h}:{m:02}:{s:02}") }
}

impl Presentation {
    /// What is kept for the document in the meantime: see [`kept_place`].
    pub fn kept_place(&self) -> (Anchor, f32) {
        kept_place(self.saved.anchor, self.saved.page, self.saved.zoom, self.page)
    }
}

impl App {
    /// Leaves the presentation, if there is one.
    pub fn end_presentation(&mut self, ctx: &Context) {
        if let Some(presentation) = self.presentation.take() {
            self.leave_presentation(ctx, presentation.saved, presentation.page);
        }
    }

    /// Starts presenting the page in the middle of the view, or leaves.
    pub fn toggle_presentation(&mut self, ctx: &Context) {
        if let Some(presentation) = self.presentation.take() {
            return self.leave_presentation(ctx, presentation.saved, presentation.page);
        }
        if self.blank {
            self.say("no document open");
            return;
        }
        let page = self.view.current_page(&self.doc.layout);
        let saved = Saved {
            fullscreen: self.fullscreen,
            statusbar: self.statusbar_visible,
            hotbar: self.hotbar,
            tool: self.tool,
            portal_window: self.portal_window,
            fit: self.fit,
            zoom: self.view.zoom,
            render_zoom: self.render_zoom,
            anchor: self.view.anchor(&self.doc.layout),
            page,
        };
        self.set_tool(None);
        self.portal_window = false;
        self.pending.clear();
        self.smart = None;
        self.toc_waiting = false;
        self.mode = Mode::Normal;
        self.selection = None;
        self.overview = None;
        self.hover = None;
        self.return_to = None;
        self.statusbar_visible = false;
        self.fullscreen = true;
        ctx.send_viewport_cmd_to(egui::ViewportId::ROOT, egui::ViewportCommand::Fullscreen(true));
        self.presentation = Some(Presentation {
            saved,
            page,
            started: Instant::now(),
            timer: self.config.presentation.timer,
            keys: Keys::default(),
            wheel: Wheel::default(),
        });
    }

    /// Puts back what presenting changed. The view goes to the top of the page last shown, with one
    /// jump entry for the whole presentation, or to where it was if that page is the one it started on.
    fn leave_presentation(&mut self, ctx: &Context, saved: Saved, page: usize) {
        self.statusbar_visible = saved.statusbar;
        if self.fullscreen != saved.fullscreen {
            self.fullscreen = saved.fullscreen;
            ctx.send_viewport_cmd_to(egui::ViewportId::ROOT, egui::ViewportCommand::Fullscreen(saved.fullscreen));
        }
        self.set_tool(saved.tool);
        self.hotbar = saved.hotbar;
        self.portal_window = saved.portal_window;
        self.fit = saved.fit;
        self.view.zoom = saved.zoom;
        self.view.zoom_hold = None;
        self.render_zoom = saved.render_zoom;
        self.zoom_changed = Instant::now();
        let page = page.min(self.doc.layout.len() - 1);
        let landing = if page == saved.page { saved.anchor } else { Anchor { page, y: 0.0, x: saved.anchor.x } };
        if page != saved.page {
            self.view.go_to(&self.doc.layout, saved.anchor);
            self.jump_to(landing);
        } else {
            self.view.go_to(&self.doc.layout, landing);
        }
        self.return_to = Some((landing, Instant::now()));
        self.mark_dirty();
    }

    /// Holds the view where leaving put it while the window changes back, which would move it.
    pub fn settle_return(&mut self, ctx: &Context, resized: bool) {
        let Some((anchor, since)) = self.return_to else { return };
        self.view.go_to(&self.doc.layout, anchor);
        if since.elapsed() >= RETURN_LIMIT || !resized && since.elapsed() >= RETURN_SETTLE {
            self.return_to = None;
        } else {
            ctx.request_repaint_after(RETURN_SETTLE.saturating_sub(since.elapsed()).max(Duration::from_millis(16)));
        }
    }

    /// A key while presenting: only the page turns, the timer, and leaving do anything.
    pub fn presentation_key(&mut self, ctx: &Context, stroke: &Stroke) {
        let leaves = self.keymap.cancels(stroke) || self.keymap.is(stroke, Action::TogglePresentation) || stroke.text() == Some('q');
        let cancel = self.keymap.cancels(stroke);
        if cancel && self.presentation.as_ref().is_some_and(|p| p.keys.pending()) {
            if let Some(presentation) = self.presentation.as_mut() {
                presentation.keys = Keys::default();
            }
            return;
        }
        if leaves {
            return self.toggle_presentation(ctx);
        }
        let Some(presentation) = self.presentation.as_mut() else { return };
        if let Some(step) = presentation.keys.feed(stroke) {
            self.present_step(step);
        }
        ctx.request_repaint();
    }

    pub fn presentation_wheel(&mut self, ctx: &Context, unit: MouseWheelUnit, dy: f32) {
        let now = ctx.input(|i| i.time);
        let Some(presentation) = self.presentation.as_mut() else { return };
        if let Some(step) = presentation.wheel.feed(now, dy, unit != MouseWheelUnit::Point) {
            self.present_step(step);
        }
        ctx.request_repaint();
    }

    fn present_step(&mut self, step: Step) {
        let len = self.doc.layout.len();
        let Some(presentation) = self.presentation.as_mut() else { return };
        match step {
            Step::Timer => presentation.timer = !presentation.timer,
            Step::Reset => presentation.started = Instant::now(),
            step => presentation.page = target_page(presentation.page, len, step),
        }
    }

    /// The view that shows `page` whole, centred in the window.
    pub fn presented_view(&self, page: usize) -> View {
        let layout = &self.doc.layout;
        let rect = self.view.rect;
        let (zoom, y) = fit_whole(rect.size(), layout.sizes[page], layout.tops[page]);
        View { x: 0.0, y, zoom, rect, zoom_hold: None }
    }

    /// Draws one frame of the presentation: the page, and the timer if it is on.
    pub fn present_frame(&mut self, ui: &Ui) {
        let ctx = ui.ctx().clone();
        let Some(presentation) = self.presentation.as_ref() else { return };
        let page = presentation.page.min(self.doc.layout.len() - 1);
        self.view = self.presented_view(page);
        self.render_zoom = self.view.zoom;
        let response = ui.interact(self.view.rect, ui.scope_id().with("view"), Sense::click());
        self.hover = None;
        ctx.set_cursor_icon(egui::CursorIcon::None);
        if response.clicked_by(PointerButton::Primary) {
            self.present_step(Step::Next);
            ctx.request_repaint();
        }
        let Some(presentation) = self.presentation.as_ref() else { return };
        let page = presentation.page.min(self.doc.layout.len() - 1);
        let (timer, started) = (presentation.timer, presentation.started);
        self.draw_presentation(ui, page);
        if timer {
            let elapsed = started.elapsed();
            self.draw_timer(ui.painter(), &format_elapsed(elapsed.as_secs()));
            ctx.request_repaint_after(Duration::from_millis(1000 - u64::from(elapsed.subsec_millis()) + 1));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn feed(keys: &mut Keys, typed: &[&str]) -> Vec<Option<Step>> {
        typed
            .iter()
            .map(|k| keys.feed(&if k.chars().count() == 1 { Stroke::from_char(k.chars().next().unwrap()) } else { Stroke::plain(k) }))
            .collect()
    }

    #[test]
    fn the_whole_page_fits_and_is_centred() {
        // A wide window: the height decides, and the page is centred across by `x = 0`.
        let (zoom, y) = fit_whole(Vec2::new(1600.0, 800.0), [400.0, 400.0], 1000.0);
        assert_eq!(zoom, 2.0);
        assert_eq!(y, 1000.0);
        // A narrow window: the width decides, and the spare height is split above and below.
        let (zoom, y) = fit_whole(Vec2::new(400.0, 800.0), [400.0, 400.0], 0.0);
        assert_eq!(zoom, 1.0);
        assert_eq!(y, -200.0);
        // An empty page doesn't divide by zero.
        assert!(fit_whole(Vec2::new(100.0, 100.0), [0.0, 0.0], 0.0).0.is_finite());
    }

    #[test]
    fn the_place_kept_is_never_the_presentations() {
        let start = Anchor { page: 4, y: 120.0, x: 7.0 };
        // Still on the start page: where the view was, at the zoom it had.
        assert_eq!(kept_place(start, 4, 1.5, 4), (start, 1.5));
        // Elsewhere: the top of that page, at the same zoom.
        assert_eq!(kept_place(start, 4, 1.5, 9), (Anchor { page: 9, y: 0.0, x: 7.0 }, 1.5));
    }

    #[test]
    fn pending_keys_are_known() {
        let mut keys = Keys::default();
        assert!(!keys.pending());
        feed(&mut keys, &["1"]);
        assert!(keys.pending());
        keys = Keys::default();
        feed(&mut keys, &["g"]);
        assert!(keys.pending());
    }

    #[test]
    fn the_timer_reads_minutes_and_seconds_then_hours() {
        assert_eq!(format_elapsed(0), "00:00");
        assert_eq!(format_elapsed(59), "00:59");
        assert_eq!(format_elapsed(61), "01:01");
        assert_eq!(format_elapsed(3599), "59:59");
        assert_eq!(format_elapsed(3600), "1:00:00");
        assert_eq!(format_elapsed(3600 * 12 + 62), "12:01:02");
    }

    #[test]
    fn pages_turn_within_the_document() {
        assert_eq!(target_page(3, 10, Step::Next), 4);
        assert_eq!(target_page(9, 10, Step::Next), 9);
        assert_eq!(target_page(0, 10, Step::Prev), 0);
        assert_eq!(target_page(5, 10, Step::Prev), 4);
        assert_eq!(target_page(5, 10, Step::First), 0);
        assert_eq!(target_page(5, 10, Step::Last), 9);
        assert_eq!(target_page(5, 10, Step::Goto(3)), 2);
        assert_eq!(target_page(5, 10, Step::Goto(0)), 0);
        assert_eq!(target_page(5, 10, Step::Goto(99)), 9);
        assert_eq!(target_page(0, 1, Step::Next), 0);
        assert_eq!(target_page(7, 10, Step::Timer), 7);
    }

    #[test]
    fn keys_turn_pages_and_count() {
        let mut keys = Keys::default();
        assert_eq!(feed(&mut keys, &["j", "J", "space", "down", "right", "pagedown", "l"]), vec![Some(Step::Next); 7]);
        assert_eq!(feed(&mut keys, &["k", "K", "bs", "up", "left", "pageup", "h"]), vec![Some(Step::Prev); 7]);
        assert_eq!(feed(&mut keys, &["home", "end", "G"]), vec![Some(Step::First), Some(Step::Last), Some(Step::Last)]);
        assert_eq!(feed(&mut keys, &["g", "g"]), vec![None, Some(Step::First)]);
        assert_eq!(feed(&mut keys, &["1", "2", "G"]), vec![None, None, Some(Step::Goto(12))]);
        assert_eq!(feed(&mut keys, &["4", "cr"]), vec![None, Some(Step::Goto(4))]);
        assert_eq!(feed(&mut keys, &["cr"]), vec![None]);
        assert_eq!(feed(&mut keys, &["3", "g", "g"]), vec![None, None, Some(Step::Goto(3))]);
        assert_eq!(feed(&mut keys, &["t", "r"]), vec![Some(Step::Timer), Some(Step::Reset)]);
    }

    #[test]
    fn other_keys_do_nothing_and_drop_what_was_typed() {
        let mut keys = Keys::default();
        assert_eq!(feed(&mut keys, &["+", "z", "f", "/", ":", "0"]), vec![None; 6]);
        assert_eq!(feed(&mut keys, &["1", "x", "G"]), vec![None, None, Some(Step::Last)]);
        assert_eq!(feed(&mut keys, &["g", "x", "g"]), vec![None, None, None]);
        let mut shifted = Stroke::plain("space");
        shifted.shift = true;
        assert_eq!(Keys::default().feed(&shifted), Some(Step::Prev));
    }

    #[test]
    fn a_trackpad_flick_turns_one_page_and_a_wheel_one_per_notch() {
        let mut wheel = Wheel::default();
        assert_eq!(wheel.feed(0.0, -3.0, false), Some(Step::Next));
        // Coasting events follow close together.
        for i in 1..40 {
            assert_eq!(wheel.feed(f64::from(i) * 0.02, -1.0, false), None);
        }
        // After a pause, the next flick turns again, either way.
        assert_eq!(wheel.feed(2.0, 3.0, false), Some(Step::Prev));
        assert_eq!(wheel.feed(2.01, 0.0, false), None);
        let mut wheel = Wheel::default();
        assert_eq!(wheel.feed(0.0, -1.0, true), Some(Step::Next));
        assert_eq!(wheel.feed(0.01, -1.0, true), None);
        assert_eq!(wheel.feed(0.1, -1.0, true), Some(Step::Next));
        assert_eq!(wheel.feed(0.2, 1.0, true), Some(Step::Prev));
    }

    #[test]
    fn fit_is_finite_for_degenerate_pages_and_windows() {
        let sizes = [[0.0, 0.0], [0.0, 500.0], [500.0, 0.0], [-5.0, -5.0], [f32::MIN_POSITIVE, f32::MIN_POSITIVE], [1e-30, 1e30]];
        let areas = [Vec2::ZERO, Vec2::new(0.0, 800.0), Vec2::new(800.0, 0.0), Vec2::new(1920.0, 1080.0)];
        let mut bad = Vec::new();
        for page in sizes {
            for area in areas {
                let (zoom, y) = fit_whole(area, page, 100.0);
                if !(zoom.is_finite() && y.is_finite()) {
                    bad.push(format!("{page:?} in {area:?} gave {zoom} {y}"));
                }
            }
        }
        assert!(bad.is_empty(), "{bad:#?}");
    }

    #[test]
    fn fit_very_wide_and_very_tall_pages() {
        let (zoom, _) = fit_whole(Vec2::new(1000.0, 1000.0), [100_000.0, 100.0], 0.0);
        assert!((zoom - 0.01).abs() < 1e-6);
        let (zoom, y) = fit_whole(Vec2::new(1000.0, 1000.0), [100.0, 100_000.0], 0.0);
        assert!((zoom - 0.01).abs() < 1e-6);
        assert!((y - 0.0).abs() < 1e-2, "{y}");
        // The page always fits the window, whichever side decides.
        for page in [[612.0, 792.0], [792.0, 612.0], [10.0, 5000.0]] {
            let area = Vec2::new(1920.0, 1080.0);
            let (zoom, _) = fit_whole(area, page, 0.0);
            assert!(page[0] * zoom <= area.x + 1e-3 && page[1] * zoom <= area.y + 1e-3);
        }
    }

    #[test]
    fn fit_is_not_clamped_to_the_zoom_limits() {
        // Documents current behaviour: fit_whole knows nothing of min_zoom/max_zoom, so a tiny page
        // in a big window gets a zoom beyond any configured max_zoom.
        let config = crate::config::Config::load_str("").unwrap();
        let (zoom, _) = fit_whole(Vec2::new(3840.0, 2160.0), [1.0, 1.0], 0.0);
        assert!(zoom > config.view.max_zoom, "fit zoom {zoom} vs max {}", config.view.max_zoom);
    }

    #[test]
    fn timer_edges() {
        assert_eq!(format_elapsed(60), "01:00");
        assert_eq!(format_elapsed(3601), "1:00:01");
        assert_eq!(format_elapsed(86_400), "24:00:00");
        assert_eq!(format_elapsed(100 * 3600 + 59 * 60 + 59), "100:59:59");
        assert!(format_elapsed(u64::MAX).len() > 8);
        // The caller passes `Duration::as_secs`, which truncates: 59.999 s reads 00:59.
        assert_eq!(format_elapsed(Duration::from_millis(59_999).as_secs()), "00:59");
        assert_eq!(format_elapsed(Duration::from_millis(999).as_secs()), "00:00");
        assert_eq!(format_elapsed(Duration::from_millis(60_000).as_secs()), "01:00");
    }

    #[test]
    fn page_clamping_edges() {
        for step in [Step::Next, Step::Prev, Step::First, Step::Last, Step::Goto(0), Step::Goto(1), Step::Goto(9), Step::Timer, Step::Reset] {
            assert_eq!(target_page(0, 1, step), 0, "{step:?}");
        }
        assert_eq!(target_page(4, 5, Step::Next), 4);
        assert_eq!(target_page(0, 5, Step::Prev), 0);
        assert_eq!(target_page(2, 5, Step::Goto(0)), 0);
        assert_eq!(target_page(2, 5, Step::Goto(5)), 4);
        assert_eq!(target_page(2, 5, Step::Goto(6)), 4);
        assert_eq!(target_page(2, 5, Step::Goto(u32::MAX)), 4);
        // An empty document doesn't panic.
        assert_eq!(target_page(0, 0, Step::Next), 0);
        assert_eq!(target_page(3, 0, Step::Goto(7)), 0);
        // A current page past the end is pulled back by Timer/Reset, and by Next.
        assert_eq!(target_page(9, 5, Step::Timer), 4);
        assert_eq!(target_page(9, 5, Step::Next), 4);
        assert_eq!(target_page(usize::MAX - 1, usize::MAX, Step::Next), usize::MAX - 1);
    }

    #[test]
    fn digit_buffer_edges() {
        let mut keys = Keys::default();
        // Overflow saturates instead of panicking, and the page clamps.
        let long = vec!["9"; 40];
        let mut typed = long.clone();
        typed.push("G");
        let out = feed(&mut keys, &typed);
        assert_eq!(out.last().unwrap(), &Some(Step::Goto(u32::MAX)));
        assert_eq!(target_page(0, 10, Step::Goto(u32::MAX)), 9);
        // Leading zeros are ignored, a lone zero is nothing.
        assert_eq!(feed(&mut keys, &["0", "0", "G"]), vec![None, None, Some(Step::Last)]);
        assert_eq!(feed(&mut keys, &["0", "5", "G"]), vec![None, None, Some(Step::Goto(5))]);
        // 10 is typeable (0 after a digit counts).
        assert_eq!(feed(&mut keys, &["1", "0", "cr"]), vec![None, None, Some(Step::Goto(10))]);
        // Digits then Esc, or any non-command key: the buffer is dropped.
        assert_eq!(feed(&mut keys, &["1", "2", "esc", "G"]), vec![None, None, None, Some(Step::Last)]);
        assert_eq!(feed(&mut keys, &["1", "2", "z", "cr"]), vec![None, None, None, None]);
        assert_eq!(feed(&mut keys, &["1", "2", "t"]), vec![None, None, Some(Step::Timer)]);
        assert_eq!(feed(&mut keys, &["cr", "cr"]), vec![None, None]);
        // Digits then a page-turn key turn a single page and clear the count.
        assert_eq!(feed(&mut keys, &["7", "j", "G"]), vec![None, Some(Step::Next), Some(Step::Last)]);
        // g then digits: the digit cancels the g.
        assert_eq!(feed(&mut keys, &["g", "3", "g"]), vec![None, None, None]);
    }

    #[test]
    fn wheel_alternating_and_bursts() {
        let mut wheel = Wheel::default();
        // Alternating notches 100 ms apart turn every time, in the direction scrolled.
        for i in 0..10 {
            let want = if i % 2 == 0 { Step::Next } else { Step::Prev };
            let dy = if i % 2 == 0 { -1.0 } else { 1.0 };
            assert_eq!(wheel.feed(f64::from(i) * 0.1, dy, true), Some(want), "{i}");
        }
        // Many notches inside 50 ms: only the first turns.
        let mut wheel = Wheel::default();
        let turns = (0..50).filter(|i| wheel.feed(f64::from(*i) * 0.001, -1.0, true).is_some()).count();
        assert_eq!(turns, 1);
        // Notches exactly at the gap, and just under it.
        let mut wheel = Wheel::default();
        assert!(wheel.feed(1.0, -1.0, true).is_some());
        assert!(wheel.feed(1.049, -1.0, true).is_none());
        assert!(wheel.feed(1.1, -1.0, true).is_some());
    }

    #[test]
    fn wheel_trackpad_inertia_and_pauses() {
        // Events every 16 ms for 2 s: one turn.
        let mut wheel = Wheel::default();
        let turns = (0..125).filter(|i| wheel.feed(f64::from(*i) * 0.016, -2.0, false).is_some()).count();
        assert_eq!(turns, 1);
        // A pause just under 300 ms keeps it the same gesture; just over starts a new one.
        let mut wheel = Wheel::default();
        assert!(wheel.feed(0.0, -1.0, false).is_some());
        assert!(wheel.feed(0.299, -1.0, false).is_none());
        // The gap counts from the last event, not the last turn: 0.299 -> 0.598 is again < 0.3.
        assert!(wheel.feed(0.598, -1.0, false).is_none());
        assert!(wheel.feed(0.9, -1.0, false).is_some());
        // Zero-delta events neither turn nor extend the gesture.
        let mut wheel = Wheel::default();
        assert!(wheel.feed(0.0, -1.0, false).is_some());
        assert!(wheel.feed(0.2, 0.0, false).is_none());
        assert!(wheel.feed(0.31, -1.0, false).is_some());
    }

    #[test]
    fn wheel_survives_time_going_backwards() {
        let mut wheel = Wheel::default();
        assert!(wheel.feed(10.0, -1.0, true).is_some());
        // Time jumps back: no panic, and no turn until it has moved on by the gap.
        assert!(wheel.feed(5.0, -1.0, true).is_none());
        // The last turn is still remembered at 10.0, so the wheel stays dead until time passes it.
        assert!(wheel.feed(5.1, -1.0, true).is_none());
        assert!(wheel.feed(10.1, -1.0, true).is_some());
        let mut wheel = Wheel::default();
        assert!(wheel.feed(10.0, -1.0, false).is_some());
        assert!(wheel.feed(2.0, -1.0, false).is_none());
        // NaN time must not panic.
        let _ = wheel.feed(f64::NAN, -1.0, true);
    }
}
