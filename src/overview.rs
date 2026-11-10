//! The overview: what a reference points at, shown over the page without leaving it.
use egui::{Align2, FontId, Pos2, Rect, Sense, Stroke, StrokeKind, pos2, vec2};

use crate::app::App;
use crate::config::OverviewConfig;
use crate::keymap::Action;
use crate::view::{Anchor, Layout, View};

/// Points shown above the place, so that what comes just before it is in sight.
const LEAD: f32 = 12.0;
/// However far out the document is zoomed, the overview stays this share of its width, or more.
const SMALLEST: f32 = 0.5;

/// The overview, showing one of the places a reference may point at.
#[derive(Clone, Debug, PartialEq)]
pub struct Overview {
    pub title: String,
    /// Page and y of each candidate, the likeliest first.
    pub places: Vec<(usize, f32)>,
    pub index: usize,
    /// The document's y at the top of the overview.
    top: f32,
}

fn top_of(layout: &Layout, (page, y): (usize, f32)) -> f32 {
    layout.tops.get(page).copied().unwrap_or(0.0) + y - LEAD
}

impl Overview {
    /// Moves on to another candidate, around the list.
    fn step(&mut self, by: i32, layout: &Layout) {
        let count = self.places.len().max(1) as i32;
        self.index = (self.index as i32 + by).rem_euclid(count) as usize;
        if let Some(place) = self.places.get(self.index) {
            self.top = top_of(layout, *place);
        }
    }
}

/// Where the overview goes in `view`: never outside it, wherever the config puts it.
pub fn area(view: Rect, config: &OverviewConfig) -> Rect {
    let size = view.size() * vec2(config.width.clamp(0.1, 1.0), config.height.clamp(0.1, 1.0));
    let middle = view.center() + vec2(config.x.clamp(-1.0, 1.0) * view.width(), config.y.clamp(-1.0, 1.0) * view.height()) / 2.0;
    let left = (middle.x - size.x / 2.0).clamp(view.left(), view.right() - size.x);
    let top = (middle.y - size.y / 2.0).clamp(view.top(), view.bottom() - size.y);
    Rect::from_min_size(pos2(left, top), size)
}

impl App {
    /// Opens the overview on the first of `places`.
    /// The places are pages with the y they have as shown.
    pub fn open_overview(&mut self, title: String, places: Vec<(usize, f32)>) {
        let Some(first) = places.first().copied() else { return };
        self.overview = Some(Overview { title, places, index: 0, top: top_of(&self.doc.layout, first) });
    }

    fn overview_area(&self) -> Rect {
        area(self.view.rect, &self.config.overview)
    }

    pub fn over_overview(&self, p: Pos2) -> bool {
        self.overview.is_some() && self.overview_area().contains(p)
    }

    /// How the overview looks at the document: at the zoom of the view as far as that fits, which
    /// also lets it show what is rendered already.
    fn overview_view(&self, overview: &Overview) -> Option<View> {
        let layout = &self.doc.layout;
        let (page, _) = *overview.places.get(overview.index)?;
        let rect = self.overview_area();
        let fit = rect.width() / layout.sizes.get(page)?[0].max(1.0);
        let zoom = self.view.zoom.clamp(fit * SMALLEST, fit);
        let mut view = View { x: 0.0, y: overview.top, zoom, rect, zoom_hold: None };
        view.clamp(layout);
        Some(view)
    }

    /// Scrolls the overview by `pixels` on screen.
    pub fn scroll_overview(&mut self, pixels: f32) {
        let Some(view) = self.overview.as_ref().and_then(|o| self.overview_view(o)) else { return };
        let mut moved = View { y: view.y + pixels / view.zoom, ..view };
        moved.clamp(&self.doc.layout);
        if let Some(overview) = &mut self.overview {
            overview.top = moved.y;
        }
    }

    /// Goes to what the overview shows, as a jump that `jump_back` can undo.
    fn goto_overview(&mut self) {
        let Some(view) = self.overview.as_ref().and_then(|o| self.overview_view(o)) else { return };
        self.overview = None;
        let layout = &self.doc.layout;
        let page = layout.page_at(view.y.max(0.0));
        let y = view.y - layout.tops.get(page).copied().unwrap_or(0.0);
        self.jump_to(Anchor { page, y, x: self.view.x });
    }

    /// What keys do while the overview is open: those that scroll scroll it, and any other closes
    /// it and goes on to do what it does. Returns whether that was all of it.
    pub fn overview_action(&mut self, action: Action, count: Option<u32>) -> bool {
        if self.overview.is_none() {
            return false;
        }
        let times = count.unwrap_or(1) as f32;
        let (step, height) = (self.config.view.scroll_step, self.overview_area().height());
        match action {
            Action::ScrollDown => self.scroll_overview(step * times),
            Action::ScrollUp => self.scroll_overview(-step * times),
            Action::HalfDown => self.scroll_overview(height / 2.0 * times),
            Action::HalfUp => self.scroll_overview(-height / 2.0 * times),
            Action::ScreenDown => self.scroll_overview(height * times),
            Action::ScreenUp => self.scroll_overview(-height * times),
            Action::GotoOverview => self.goto_overview(),
            Action::OverviewNext | Action::OverviewPrev => {
                let by = if action == Action::OverviewNext { 1 } else { -1 };
                let layout = &self.doc.layout;
                if let Some(overview) = &mut self.overview {
                    overview.step(by, layout);
                }
            }
            Action::Cancel => self.overview = None,
            _ => {
                self.overview = None;
                return false;
            }
        }
        true
    }

    /// Draws the overview over the page.
    pub fn draw_overview(&mut self, ui: &egui::Ui) {
        let Some(overview) = self.overview.clone() else { return };
        let Some(view) = self.overview_view(&overview) else { return };
        let response = ui.interact(view.rect, ui.scope_id().with("overview"), Sense::click());
        let painter = ui.painter().with_clip_rect(view.rect);
        let theme = self.config.active_theme().clone();
        painter.rect_filled(view.rect, 0.0, theme.background.egui());
        let ppp = ui.ctx().pixels_per_point();
        let queued = self.wants.len();
        for page in view.visible_pages(&self.doc.layout) {
            self.draw_page(&painter, &view, page, ppp, view.zoom);
        }
        // It is what is being looked at, so its pages go first.
        let added = self.wants.len() - queued;
        self.wants.rotate_right(added);
        self.draw_marks(&painter, &view);
        // The place it was opened on, in the margin.
        if let Some((page, y)) = overview.places.get(overview.index).copied() {
            // `y` is down the page as shown, as an anchor's is.
            let t = view.transform(&self.doc.layout, page);
            let at = t.origin + vec2(0.0, y) * t.scale;
            let mark = Rect::from_min_size(pos2(view.rect.left(), at.y), vec2(4.0, 12.0 * view.zoom));
            painter.rect_filled(mark, 0.0, theme.ui_accent.egui());
        }
        painter.rect_stroke(view.rect, 0.0, Stroke::new(self.config.view.border, theme.ui_accent.egui()), StrokeKind::Inside);
        let page = view.current_page(&self.doc.layout) + 1;
        painter.text(
            view.rect.right_bottom() - vec2(8.0, 6.0),
            Align2::RIGHT_BOTTOM,
            page.to_string(),
            FontId::monospace(11.0),
            theme.ui_accent.egui(),
        );
        if response.clicked() {
            self.goto_overview();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn config(width: f32, height: f32, x: f32, y: f32) -> OverviewConfig {
        OverviewConfig { width, height, x, y }
    }

    fn view() -> Rect {
        Rect::from_min_size(pos2(100.0, 50.0), vec2(1000.0, 800.0))
    }

    #[test]
    fn it_is_centred_on_the_view_by_default() {
        let rect = area(view(), &config(0.8, 0.5, 0.0, 0.0));
        assert_eq!(rect, Rect::from_center_size(view().center(), vec2(800.0, 400.0)));
    }

    #[test]
    fn its_offset_moves_it_and_it_stays_in_the_view() {
        let up = area(view(), &config(0.8, 0.5, 0.0, -0.5));
        assert_eq!(up.center().y, view().center().y - 200.0);
        let far = area(view(), &config(0.5, 0.5, 9.0, 9.0));
        assert_eq!(far.right_bottom(), view().right_bottom());
    }

    #[test]
    fn a_nonsense_size_is_kept_to_something_usable() {
        let rect = area(view(), &config(0.0, 40.0, 0.0, 0.0));
        assert_eq!(rect.size(), vec2(100.0, 800.0));
    }

    #[test]
    fn candidates_go_around_and_each_shows_its_place() {
        let layout = Layout::new(vec![[600.0, 800.0]; 3], 10.0);
        let places = vec![(0, 100.0), (2, 50.0)];
        let mut overview = Overview { title: String::new(), places, index: 0, top: 0.0 };
        overview.step(1, &layout);
        assert_eq!((overview.index, overview.top), (1, 1620.0 + 50.0 - LEAD));
        overview.step(1, &layout);
        assert_eq!((overview.index, overview.top), (0, 100.0 - LEAD));
        overview.step(-1, &layout);
        assert_eq!(overview.index, 1);
    }

    #[test]
    fn stepping_through_nothing_does_nothing() {
        let layout = Layout::new(vec![[600.0, 800.0]], 10.0);
        let mut overview = Overview { title: String::new(), places: Vec::new(), index: 0, top: 3.0 };
        overview.step(1, &layout);
        assert_eq!((overview.index, overview.top), (0, 3.0));
    }
}
