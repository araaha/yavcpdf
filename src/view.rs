//! Where pages sit in the continuous column, and how a view looks at them.
use egui::{Pos2, Rect, Vec2, pos2, vec2};
use serde::{Deserialize, Serialize};

/// The pages stacked top to bottom, in points.
#[derive(Clone, Debug, Default)]
pub struct Layout {
    pub sizes: Vec<[f32; 2]>,
    pub tops: Vec<f32>,
    pub total: f32,
    pub max_width: f32,
}

impl Layout {
    pub fn new(sizes: Vec<[f32; 2]>, gap: f32) -> Self {
        let mut tops = Vec::with_capacity(sizes.len());
        let mut y = 0.0;
        for size in &sizes {
            tops.push(y);
            y += size[1] + gap;
        }
        let total = (y - gap).max(0.0);
        let max_width = sizes.iter().map(|s| s[0]).fold(0.0, f32::max);
        Self { sizes, tops, total, max_width }
    }

    /// The last page starting at or above `y`.
    pub fn page_at(&self, y: f32) -> usize {
        self.tops.partition_point(|top| *top <= y).saturating_sub(1)
    }

    pub fn len(&self) -> usize {
        self.sizes.len()
    }
}

/// A place in the document that doesn't depend on the zoom.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Anchor {
    pub page: usize,
    /// Points below the top of the page.
    pub y: f32,
    /// Horizontal offset of the view's centre from the column's centre, in points.
    pub x: f32,
}

/// Maps a page's points to the screen.
#[derive(Clone, Copy, Debug)]
pub struct PageTransform {
    /// The page's top-left corner on screen.
    pub origin: Pos2,
    /// Screen points per page point.
    pub scale: f32,
}

impl PageTransform {
    pub fn point(&self, p: [f32; 2]) -> Pos2 {
        self.origin + vec2(p[0], p[1]) * self.scale
    }

    pub fn rect(&self, r: [f32; 4]) -> Rect {
        Rect::from_two_pos(self.point([r[0], r[1]]), self.point([r[2], r[3]]))
    }

    pub fn to_page(self, p: Pos2) -> [f32; 2] {
        let d = (p - self.origin) / self.scale;
        [d.x, d.y]
    }
}

/// What a run of zooming holds still: a point of the document at a place in the view. Kept apart
/// from the view, which `clamp` moves whenever the document stops filling the window: zooming far
/// out and back would otherwise come back to wherever that left the view.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ZoomHold {
    /// The point: `x` from the column's centre and `y` from the top of the document, in points.
    point: [f32; 2],
    /// Where it stays, from the middle of the view.
    offset: Vec2,
    /// `x`, `y` and `zoom` as the last zoom left them. A view that has moved since starts anew.
    left: [f32; 3],
}

/// A window onto the column.
#[derive(Clone, Copy, Debug)]
pub struct View {
    /// Horizontal offset of the centre, in points from the column's centre.
    pub x: f32,
    /// The document's y at the top of the view, in points.
    pub y: f32,
    /// Screen points per page point.
    pub zoom: f32,
    /// The view's area on screen.
    pub rect: Rect,
    /// What zooming is holding still, if the view is where the last zoom left it.
    pub zoom_hold: Option<ZoomHold>,
}

impl View {
    pub fn new() -> Self {
        Self { x: 0.0, y: 0.0, zoom: 1.0, rect: Rect::from_min_size(Pos2::ZERO, vec2(1.0, 1.0)), zoom_hold: None }
    }

    /// The view's size in points of the page.
    fn span(&self) -> [f32; 2] {
        [self.rect.width() / self.zoom, self.rect.height() / self.zoom]
    }

    /// Keeps the document in view: a document shorter than the window is centred.
    pub fn clamp(&mut self, layout: &Layout) {
        let [w, h] = self.span();
        let overflow = (layout.max_width - w).max(0.0) / 2.0;
        self.x = self.x.clamp(-overflow, overflow);
        self.y = if layout.total <= h { (layout.total - h) / 2.0 } else { self.y.clamp(0.0, layout.total - h) };
    }

    /// The pages showing, first to last.
    pub fn visible_pages(&self, layout: &Layout) -> std::ops::RangeInclusive<usize> {
        let top = layout.page_at(self.y.max(0.0));
        let bottom = layout.page_at((self.y + self.span()[1]).max(0.0));
        top..=bottom
    }

    /// The page at the middle of the view: the one the status bar calls current.
    pub fn current_page(&self, layout: &Layout) -> usize {
        layout.page_at((self.y + self.span()[1] / 2.0).max(0.0))
    }

    pub fn transform(&self, layout: &Layout, page: usize) -> PageTransform {
        let width = layout.sizes[page][0];
        let left = self.rect.center().x + (-width / 2.0 - self.x) * self.zoom;
        let top = self.rect.top() + (layout.tops[page] - self.y) * self.zoom;
        PageTransform { origin: pos2(left, top), scale: self.zoom }
    }

    /// The page under a screen position, with the position in its points.
    pub fn page_under(&self, layout: &Layout, p: Pos2) -> Option<(usize, [f32; 2])> {
        self.visible_pages(layout).find_map(|page| {
            let at = self.transform(layout, page).to_page(p);
            let [w, h] = layout.sizes[page];
            ((0.0..=w).contains(&at[0]) && (0.0..=h).contains(&at[1])).then_some((page, at))
        })
    }

    /// Changes the zoom around the middle of the view.
    pub fn set_zoom(&mut self, zoom: f32, layout: &Layout) {
        self.zoom_around(zoom, None, layout);
    }

    /// Changes the zoom holding still what is at `at`, or what the last zoom held if the view
    /// hasn't moved since, or else the middle of the view.
    pub fn zoom_around(&mut self, zoom: f32, at: Option<Pos2>, layout: &Layout) {
        let asked = at.map_or(Vec2::ZERO, |p| p - self.rect.center());
        let here = [self.x, self.y, self.zoom];
        let held = self.zoom_hold.filter(|h| h.left == here && (at.is_none() || h.offset == asked));
        let (point, offset) = held.map_or_else(|| (self.point_at(asked), asked), |h| (h.point, h.offset));
        self.zoom = zoom;
        self.x = point[0] - offset.x / zoom;
        self.y = point[1] - (self.rect.height() / 2.0 + offset.y) / zoom;
        self.clamp(layout);
        self.zoom_hold = Some(ZoomHold { point, offset, left: [self.x, self.y, self.zoom] });
    }

    /// The document point `offset` from the middle of the view.
    fn point_at(&self, offset: Vec2) -> [f32; 2] {
        [self.x + offset.x / self.zoom, self.y + (self.rect.height() / 2.0 + offset.y) / self.zoom]
    }

    pub fn anchor(&self, layout: &Layout) -> Anchor {
        let page = layout.page_at(self.y.max(0.0));
        Anchor { page, y: self.y - layout.tops[page], x: self.x }
    }

    pub fn go_to(&mut self, layout: &Layout, anchor: Anchor) {
        let Some(top) = layout.tops.get(anchor.page.min(layout.len().saturating_sub(1))) else { return };
        self.y = top + anchor.y;
        self.x = anchor.x;
        self.clamp(layout);
    }

    /// Scrolls so `r` (page points) shows, centring it when it is out of view.
    pub fn reveal(&mut self, layout: &Layout, page: usize, r: [f32; 4]) {
        let [w, h] = self.span();
        let (top, bottom) = (layout.tops[page] + r[1], layout.tops[page] + r[3]);
        if top < self.y || bottom > self.y + h {
            self.y = (top + bottom) / 2.0 - h / 2.0;
        }
        let page_left = -layout.sizes[page][0] / 2.0;
        let (left, right) = (page_left + r[0], page_left + r[2]);
        if left < self.x - w / 2.0 || right > self.x + w / 2.0 {
            self.x = (left + right) / 2.0;
        }
        self.clamp(layout);
    }
}

/// Smallest side of a render tile in device pixels.
pub const TILE: u32 = 1024;
/// Largest side of a render tile: what the render backend accepts.
const MAX_TILE: u32 = 4096;

/// The side of the tiles of a page `longest` device pixels long at `scale`. Every tile costs an
/// interpretation of the whole page, so tiles grow with the zoom and the page.
pub fn tile_side(scale: f32, longest: u32) -> u32 {
    let by_zoom = if scale.is_finite() && scale > 0.0 { (TILE as f32 * (scale / 6.0)).clamp(TILE as f32, MAX_TILE as f32) as u32 } else { TILE };
    by_zoom.max(longest.div_ceil(4).min(MAX_TILE))
}

/// The grid tiles of side `side` of a `width` × `height` pixel page that cross the pixel area
/// `visible` (`[x0, y0, x1, y1]`).
pub fn tiles_crossing(width: u32, height: u32, visible: [f32; 4], side: u32) -> Vec<pdfcraft_render::Tile> {
    let index = |v: f32, limit: u32| ((v.max(0.0) as u32) / side).min(limit.saturating_sub(1) / side);
    let (x0, y0) = (index(visible[0], width), index(visible[1], height));
    let (x1, y1) = (index(visible[2], width), index(visible[3], height));
    let mut out = Vec::new();
    for ty in y0..=y1 {
        for tx in x0..=x1 {
            let (x, y) = (tx * side, ty * side);
            out.push(pdfcraft_render::Tile { x, y, w: side.min(width - x), h: side.min(height - y) });
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn layout() -> Layout {
        Layout::new(vec![[600.0, 800.0], [600.0, 800.0], [300.0, 400.0]], 10.0)
    }

    fn view(w: f32, h: f32) -> View {
        View { rect: Rect::from_min_size(Pos2::ZERO, vec2(w, h)), ..View::new() }
    }

    #[test]
    fn pages_stack_with_gaps() {
        let l = layout();
        assert_eq!(l.tops, [0.0, 810.0, 1620.0]);
        assert_eq!(l.total, 2020.0);
        assert_eq!(l.page_at(0.0), 0);
        assert_eq!(l.page_at(809.0), 0);
        assert_eq!(l.page_at(810.0), 1);
        assert_eq!(l.page_at(99999.0), 2);
    }

    #[test]
    fn the_view_stops_at_both_ends() {
        let l = layout();
        let mut v = view(600.0, 500.0);
        v.y = -50.0;
        v.clamp(&l);
        assert_eq!(v.y, 0.0);
        v.y = 99999.0;
        v.clamp(&l);
        assert_eq!(v.y, l.total - 500.0);
    }

    #[test]
    fn a_short_document_is_centred_and_a_narrow_one_cannot_pan() {
        let l = Layout::new(vec![[100.0, 100.0]], 10.0);
        let mut v = view(500.0, 500.0);
        v.x = 40.0;
        v.clamp(&l);
        assert_eq!(v.x, 0.0);
        assert_eq!(v.y, -200.0);
        assert_eq!(v.transform(&l, 0).origin, pos2(200.0, 200.0));
    }

    #[test]
    fn zoom_keeps_the_middle_of_the_view() {
        let l = layout();
        let mut v = view(600.0, 400.0);
        v.y = 500.0;
        let centre = v.y + 200.0;
        v.set_zoom(2.0, &l);
        assert_eq!(v.y + 100.0, centre);
    }

    /// Zooms out in steps until the document is small, and back by the same steps.
    fn out_and_back(v: &mut View, l: &Layout, at: Option<Pos2>) {
        let start = v.zoom;
        let steps = 40;
        for i in 1..=steps {
            v.zoom_around((start * 1.2_f32.powi(-i)).max(0.02), at, l);
            v.clamp(l);
        }
        for i in (0..steps).rev() {
            v.zoom_around((start * 1.2_f32.powi(-i)).max(0.02), at, l);
            v.clamp(l);
        }
    }

    #[test]
    fn zooming_far_out_and_back_returns_to_the_same_place() {
        let l = Layout::new(vec![[600.0, 800.0]; 15], 6.0);
        for y in [0.0, 50.0, 3000.0, l.total - 900.0, l.total - 400.0] {
            let mut v = view(1000.0, 400.0);
            v.y = y;
            v.clamp(&l);
            let (x, y) = (v.x, v.y);
            out_and_back(&mut v, &l, None);
            assert!((v.y - y).abs() < 0.01 && (v.x - x).abs() < 0.01, "from {y}: at {} {}", v.x, v.y);
        }
    }

    #[test]
    fn a_panned_view_comes_back_sideways_too() {
        let l = Layout::new(vec![[600.0, 800.0]; 3], 6.0);
        let mut v = view(300.0, 400.0);
        v.zoom = 2.0;
        v.x = 120.0;
        v.y = 500.0;
        v.clamp(&l);
        let (x, y) = (v.x, v.y);
        out_and_back(&mut v, &l, None);
        assert!((v.x - x).abs() < 0.01 && (v.y - y).abs() < 0.01, "at {} {}", v.x, v.y);
    }

    #[test]
    fn zooming_at_the_pointer_keeps_what_is_under_it() {
        let l = Layout::new(vec![[600.0, 800.0]; 15], 6.0);
        let mut v = view(1000.0, 800.0);
        v.y = 3000.0;
        let at = pos2(700.0, 200.0);
        let before = v.point_at(at - v.rect.center());
        v.zoom_around(3.0, Some(at), &l);
        let after = v.point_at(at - v.rect.center());
        assert!((before[1] - after[1]).abs() < 0.01, "{before:?} {after:?}");
        out_and_back(&mut v, &l, Some(at));
        let back = v.point_at(at - v.rect.center());
        assert!((before[1] - back[1]).abs() < 0.01, "{before:?} {back:?}");
    }

    #[test]
    fn moving_the_view_starts_a_new_zoom() {
        let l = Layout::new(vec![[600.0, 800.0]; 15], 6.0);
        let mut v = view(600.0, 400.0);
        v.y = 3000.0;
        v.set_zoom(0.5, &l);
        v.y += 1000.0;
        let middle = v.point_at(Vec2::ZERO);
        v.set_zoom(2.0, &l);
        assert!((v.point_at(Vec2::ZERO)[1] - middle[1]).abs() < 0.01);
    }

    #[test]
    fn a_new_pointer_position_is_held_at_once_and_keys_keep_what_was_held() {
        let l = Layout::new(vec![[600.0, 800.0]; 15], 6.0);
        let mut v = view(1000.0, 800.0);
        v.y = 3000.0;
        v.zoom_around(2.0, Some(pos2(100.0, 100.0)), &l);
        let other = pos2(800.0, 700.0);
        let before = v.point_at(other - v.rect.center());
        v.zoom_around(3.0, Some(other), &l);
        let after = v.point_at(other - v.rect.center());
        assert!((before[1] - after[1]).abs() < 0.01);
        let held = v.zoom_hold;
        v.set_zoom(4.0, &l);
        assert_eq!(v.zoom_hold.map(|h| h.point), held.map(|h| h.point));
    }

    #[test]
    fn tiles_grow_with_the_zoom_and_the_page_within_what_can_be_rendered() {
        assert_eq!(tile_side(2.0, 3000), TILE);
        assert_eq!(tile_side(12.0, 3000), 2048);
        assert_eq!(tile_side(100.0, 3000), MAX_TILE);
        assert_eq!(tile_side(2.0, 12000), 3000);
        assert_eq!(tile_side(f32::NAN, 100), TILE);
    }

    #[test]
    fn screen_and_page_positions_round_trip() {
        let l = layout();
        let mut v = view(800.0, 600.0);
        v.zoom = 2.0;
        v.y = 810.0;
        let t = v.transform(&l, 1);
        let screen = t.point([100.0, 50.0]);
        let (page, at) = v.page_under(&l, screen).unwrap();
        assert_eq!(page, 1);
        assert!((at[0] - 100.0).abs() < 1e-3 && (at[1] - 50.0).abs() < 1e-3);
    }

    #[test]
    fn the_gap_between_pages_is_no_page() {
        let l = layout();
        let mut v = view(600.0, 400.0);
        v.y = 805.0;
        assert!(v.page_under(&l, pos2(300.0, 0.0)).is_none());
    }

    #[test]
    fn reveal_moves_only_when_it_must() {
        let l = layout();
        let mut v = view(600.0, 400.0);
        v.reveal(&l, 0, [10.0, 100.0, 50.0, 110.0]);
        assert_eq!(v.y, 0.0);
        v.reveal(&l, 1, [10.0, 100.0, 50.0, 120.0]);
        assert_eq!(v.y, 810.0 + 110.0 - 200.0);
    }

    #[test]
    fn anchors_survive_a_zoom() {
        let l = layout();
        let mut v = view(600.0, 400.0);
        v.y = 900.0;
        let a = v.anchor(&l);
        assert_eq!((a.page, a.y), (1, 90.0));
        v.zoom = 3.0;
        v.y = 0.0;
        v.go_to(&l, a);
        assert_eq!(v.y, 900.0);
    }

    #[test]
    fn tiles_cover_the_visible_area_and_stop_at_the_page_edge() {
        let tiles = tiles_crossing(1500, 2100, [0.0, 0.0, 1500.0, 1100.0], TILE);
        assert_eq!(tiles.len(), 4);
        assert_eq!((tiles[1].x, tiles[1].w), (1024, 476));
        let last = tiles_crossing(1500, 2100, [1400.0, 2090.0, 9000.0, 9000.0], TILE);
        assert_eq!(last.len(), 1);
        assert_eq!((last[0].x, last[0].y, last[0].w, last[0].h), (1024, 2048, 476, 52));
    }

    #[test]
    fn an_exact_multiple_of_the_tile_has_no_empty_tile() {
        let tiles = tiles_crossing(2048, 1024, [0.0, 0.0, 2048.0, 1024.0], TILE);
        assert_eq!(tiles.len(), 2);
        assert!(tiles.iter().all(|t| t.w > 0 && t.h > 0));
    }
}
