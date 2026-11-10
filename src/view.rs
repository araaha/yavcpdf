//! Where pages sit in the continuous column, and how a view looks at them.
use egui::{Pos2, Rect, Vec2, pos2, vec2};
use serde::{Deserialize, Serialize};

/// A rotation in degrees, clockwise, as a multiple of 90 in `0..360`: anything else is taken down
/// to the multiple of 90 below it, after wrapping.
pub fn normalize_rotation(degrees: i64) -> u16 {
    (degrees.rem_euclid(360) / 90 * 90) as u16
}

/// Where `p`, a point of an unrotated page of `size`, lands on the page turned clockwise by
/// `rotation`, measured from the turned page's top-left corner.
pub fn rotate_point(rotation: u16, size: [f32; 2], p: [f32; 2]) -> [f32; 2] {
    let [w, h] = size;
    match rotation {
        90 => [h - p[1], p[0]],
        180 => [w - p[0], h - p[1]],
        270 => [p[1], w - p[0]],
        _ => p,
    }
}

/// The inverse of `rotate_point`.
pub fn unrotate_point(rotation: u16, size: [f32; 2], d: [f32; 2]) -> [f32; 2] {
    let [w, h] = size;
    match rotation {
        90 => [d[1], h - d[0]],
        180 => [w - d[0], h - d[1]],
        270 => [w - d[1], d[0]],
        _ => d,
    }
}

/// The texture corners that the screen corners of a rotated tile show, in the order top-left,
/// top-right, bottom-right, bottom-left. The texture is of the unrotated page.
pub fn tile_uv(rotation: u16) -> [Pos2; 4] {
    let (a, b, c, d) = (pos2(0.0, 0.0), pos2(1.0, 0.0), pos2(1.0, 1.0), pos2(0.0, 1.0));
    match rotation {
        90 => [d, a, b, c],
        180 => [c, d, a, b],
        270 => [b, c, d, a],
        _ => [a, b, c, d],
    }
}

/// The pixel area of an unrotated page of `pixels` that shows through `clip` on screen, as
/// `[x0, y0, x1, y1]`, when the turned page's top-left is at `origin` and a pixel is `unit` points.
pub fn visible_pixels(rotation: u16, pixels: [f32; 2], origin: Pos2, unit: f32, clip: Rect) -> [f32; 4] {
    let at = |p: Pos2| unrotate_point(rotation, pixels, [(p.x - origin.x) / unit, (p.y - origin.y) / unit]);
    let (a, b) = (at(clip.min), at(clip.max));
    [a[0].min(b[0]), a[1].min(b[1]), a[0].max(b[0]), a[1].max(b[1])]
}

/// The pages stacked top to bottom, in points, as they are shown: turned by `rotation`.
#[derive(Clone, Debug, Default)]
pub struct Layout {
    /// The size of each page as shown: width and height swap at 90 and 270 degrees.
    pub sizes: Vec<[f32; 2]>,
    /// The size of each page itself, whatever the rotation: the space of rasters and annotations.
    pub page_sizes: Vec<[f32; 2]>,
    pub tops: Vec<f32>,
    pub total: f32,
    pub max_width: f32,
    /// Degrees clockwise, a multiple of 90 in `0..360`.
    pub rotation: u16,
    gap: f32,
}

impl Layout {
    pub fn new(sizes: Vec<[f32; 2]>, gap: f32) -> Self {
        let mut layout = Self { page_sizes: sizes, gap, ..Self::default() };
        layout.arrange();
        layout
    }

    /// Turns every page to `degrees` clockwise, as given to `normalize_rotation`.
    pub fn set_rotation(&mut self, degrees: i64) {
        self.rotation = normalize_rotation(degrees);
        self.arrange();
    }

    fn arrange(&mut self) {
        let turned = !self.rotation.is_multiple_of(180);
        self.sizes = self.page_sizes.iter().map(|&[w, h]| if turned { [h, w] } else { [w, h] }).collect();
        self.tops = Vec::with_capacity(self.sizes.len());
        let mut y = 0.0;
        for size in &self.sizes {
            self.tops.push(y);
            y += size[1] + self.gap;
        }
        self.total = (y - self.gap).max(0.0);
        self.max_width = self.sizes.iter().map(|s| s[0]).fold(0.0, f32::max);
    }

    /// A page rectangle `[x0, y0, x1, y1]` in the space of the page as shown.
    pub fn shown_rect(&self, page: usize, r: [f32; 4]) -> [f32; 4] {
        let size = self.page_sizes[page];
        let (a, b) = (rotate_point(self.rotation, size, [r[0], r[1]]), rotate_point(self.rotation, size, [r[2], r[3]]));
        [a[0].min(b[0]), a[1].min(b[1]), a[0].max(b[0]), a[1].max(b[1])]
    }

    /// A point `p` of an unrotated page, in the space of the page as shown.
    #[cfg(test)]
    pub fn shown_point(&self, page: usize, p: [f32; 2]) -> [f32; 2] {
        rotate_point(self.rotation, self.page_sizes[page], p)
    }

    /// The y as shown of a destination on `page` given by the page point `x`, `y` where known. A
    /// destination with no `y` names no point and is at the top, 0, however the pages are turned.
    /// With a `y`, upright and upside down only `y` matters, and turned a quarter only `x` does,
    /// the top being 0 where `x` is missing.
    pub fn shown_dest_y(&self, page: usize, x: Option<f32>, y: Option<f32>) -> f32 {
        let [w, h] = self.page_sizes[page];
        let Some(y) = y else { return 0.0 };
        match self.rotation {
            0 => y,
            180 => h - y,
            90 => x.unwrap_or(0.0),
            _ => x.map_or(0.0, |x| w - x),
        }
    }

    /// `shown_dest_y` for a destination that may be a rectangle `[x0, y0, x1, y1]` of the page, whose
    /// corner topmost as shown is taken.
    pub fn shown_spot_y(&self, page: usize, x: Option<f32>, y: Option<f32>, rect: Option<[f32; 4]>) -> f32 {
        let Some([x0, y0, x1, y1]) = rect else { return self.shown_dest_y(page, x, y) };
        let (cx, cy) = match self.rotation {
            0 | 90 => (x0, y0),
            180 => (x0, y1),
            _ => (x1, y0),
        };
        self.shown_dest_y(page, Some(cx), Some(cy))
    }

    /// An anchor made while the pages were turned to `from`, as it is for `to`. The anchor is taken
    /// as the page point at the top of the view, at its centre across: that point stays put. An
    /// anchor at or above the top of its page (`y <= 0`) stays at the top, y 0, and keeps its x
    /// only if it is turned by a half turn or none: across a quarter turn the page's width becomes
    /// its height, so x is 0 (the column's centre). Such an anchor is not lossless; one below the top is.
    pub fn turn_anchor(&self, a: Anchor, from: u16, to: u16) -> Anchor {
        let Some(&size) = self.page_sizes.get(a.page) else { return a };
        if a.y <= 0.0 {
            let quarter = from.is_multiple_of(180) != to.is_multiple_of(180);
            return Anchor { page: a.page, y: 0.0, x: if quarter { 0.0 } else { a.x } };
        }
        let width = |rotation: u16| if rotation.is_multiple_of(180) { size[0] } else { size[1] };
        let p = unrotate_point(from, size, [width(from) / 2.0 + a.x, a.y]);
        let [dx, dy] = rotate_point(to, size, p);
        Anchor { page: a.page, y: dy, x: dx - width(to) / 2.0 }
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

/// Maps a page's points to the screen, through the rotation the page is shown at.
#[derive(Clone, Copy, Debug)]
pub struct PageTransform {
    /// The top-left corner on screen of the page as shown, turned.
    pub origin: Pos2,
    /// Screen points per page point.
    pub scale: f32,
    /// Degrees clockwise, a multiple of 90.
    pub rotation: u16,
    /// The page's own size, unrotated.
    pub size: [f32; 2],
}

impl PageTransform {
    pub fn point(&self, p: [f32; 2]) -> Pos2 {
        let d = rotate_point(self.rotation, self.size, p);
        self.origin + vec2(d[0], d[1]) * self.scale
    }

    pub fn rect(&self, r: [f32; 4]) -> Rect {
        Rect::from_two_pos(self.point([r[0], r[1]]), self.point([r[2], r[3]]))
    }

    pub fn to_page(self, p: Pos2) -> [f32; 2] {
        let d = (p - self.origin) / self.scale;
        unrotate_point(self.rotation, self.size, [d.x, d.y])
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
        PageTransform { origin: pos2(left, top), scale: self.zoom, rotation: layout.rotation, size: layout.page_sizes[page] }
    }

    /// The page under a screen position, with the position in its points.
    pub fn page_under(&self, layout: &Layout, p: Pos2) -> Option<(usize, [f32; 2])> {
        self.visible_pages(layout).find_map(|page| {
            let at = self.transform(layout, page).to_page(p);
            let [w, h] = layout.page_sizes[page];
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
        let r = layout.shown_rect(page, r);
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
    const ROTATIONS: [u16; 4] = [0, 90, 180, 270];

    fn transform(rotation: u16) -> PageTransform {
        PageTransform { origin: pos2(10.0, 20.0), scale: 2.0, rotation, size: [600.0, 800.0] }
    }

    #[test]
    fn page_points_round_trip_through_every_rotation() {
        for rotation in ROTATIONS {
            let t = transform(rotation);
            for p in [[0.0, 0.0], [600.0, 800.0], [123.0, 456.0], [599.0, 1.0]] {
                let back = t.to_page(t.point(p));
                assert!((back[0] - p[0]).abs() < 1e-3 && (back[1] - p[1]).abs() < 1e-3, "{rotation}: {p:?} -> {back:?}");
            }
        }
    }

    #[test]
    fn a_page_rect_maps_to_the_screen_rect_of_its_turned_corners() {
        let r = [100.0, 50.0, 300.0, 90.0];
        // Width 600, height 800; scale 2 from (10, 20). Turned by 90 the page is 800 wide, 600 tall.
        let expect = [
            (0, [210.0, 120.0, 610.0, 200.0]),
            (90, [10.0 + 2.0 * (800.0 - 90.0), 20.0 + 200.0, 10.0 + 2.0 * (800.0 - 50.0), 20.0 + 600.0]),
            (180, [10.0 + 2.0 * 300.0, 20.0 + 2.0 * 710.0, 10.0 + 2.0 * 500.0, 20.0 + 2.0 * 750.0]),
            (270, [10.0 + 2.0 * 50.0, 20.0 + 2.0 * 300.0, 10.0 + 2.0 * 90.0, 20.0 + 2.0 * 500.0]),
        ];
        for (rotation, [x0, y0, x1, y1]) in expect {
            let got = transform(rotation).rect(r);
            assert_eq!([got.min.x, got.min.y, got.max.x, got.max.y], [x0, y0, x1, y1], "{rotation}");
        }
    }

    #[test]
    fn the_whole_page_fills_the_turned_page_area() {
        for rotation in ROTATIONS {
            let t = transform(rotation);
            let got = t.rect([0.0, 0.0, 600.0, 800.0]);
            let (w, h) = if rotation % 180 == 0 { (1200.0, 1600.0) } else { (1600.0, 1200.0) };
            assert_eq!([got.min.x, got.min.y, got.width(), got.height()], [10.0, 20.0, w, h], "{rotation}");
        }
    }

    #[test]
    fn a_turned_layout_swaps_the_sizes_and_restacks_the_pages() {
        let mut l = layout();
        for (rotation, sizes, tops) in [
            (0, [[600.0, 800.0], [600.0, 800.0], [300.0, 400.0]], [0.0, 810.0, 1620.0]),
            (90, [[800.0, 600.0], [800.0, 600.0], [400.0, 300.0]], [0.0, 610.0, 1220.0]),
            (180, [[600.0, 800.0], [600.0, 800.0], [300.0, 400.0]], [0.0, 810.0, 1620.0]),
            (270, [[800.0, 600.0], [800.0, 600.0], [400.0, 300.0]], [0.0, 610.0, 1220.0]),
        ] {
            l.set_rotation(rotation);
            assert_eq!(l.sizes, sizes, "{rotation}");
            assert_eq!(l.tops, tops, "{rotation}");
            assert_eq!(l.page_sizes, [[600.0, 800.0], [600.0, 800.0], [300.0, 400.0]]);
        }
        l.set_rotation(90);
        assert_eq!((l.total, l.max_width), (1520.0, 800.0));
    }

    #[test]
    fn rotations_are_normalised() {
        assert_eq!([normalize_rotation(0), normalize_rotation(90), normalize_rotation(450), normalize_rotation(-90)], [0, 90, 90, 270]);
        assert_eq!([normalize_rotation(360), normalize_rotation(100), normalize_rotation(-1), normalize_rotation(-360)], [0, 90, 270, 0]);
    }

    #[test]
    fn the_page_is_under_the_pointer_in_its_own_space_when_turned() {
        let mut l = layout();
        l.set_rotation(90);
        let v = view(800.0, 600.0);
        let t = v.transform(&l, 0);
        let p = t.point([590.0, 10.0]);
        let (page, at) = v.page_under(&l, p).unwrap();
        assert_eq!(page, 0);
        assert!((at[0] - 590.0).abs() < 1e-3 && (at[1] - 10.0).abs() < 1e-3);
        // Beyond the turned page's height of 600 is the gap, though the page is 800 tall unturned.
        assert!(v.page_under(&l, pos2(400.0, 605.0)).is_none());
    }

    #[test]
    fn the_tile_texture_corners_follow_the_rotation() {
        let screen = [[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 1.0]];
        for rotation in ROTATIONS {
            // Each screen corner must show the texture corner that the rotation takes there.
            for (uv, want) in tile_uv(rotation).into_iter().zip(screen) {
                let at = rotate_point(rotation, [1.0, 1.0], [uv.x, uv.y]);
                assert_eq!(at, want, "{rotation}");
            }
        }
        assert_eq!(tile_uv(0), [pos2(0.0, 0.0), pos2(1.0, 0.0), pos2(1.0, 1.0), pos2(0.0, 1.0)]);
        assert_eq!(tile_uv(90)[0], pos2(0.0, 1.0));
    }

    #[test]
    fn the_needed_pixels_are_found_in_unturned_space() {
        // A 1000 x 2000 pixel raster; turned by 90 it shows 2000 wide and 1000 tall at (100, 100).
        let origin = pos2(100.0, 100.0);
        let clip = Rect::from_min_max(pos2(100.0, 100.0), pos2(600.0, 300.0));
        assert_eq!(visible_pixels(0, [1000.0, 2000.0], origin, 1.0, clip), [0.0, 0.0, 500.0, 200.0]);
        // The left of the turned page is the bottom of the raster, and its top is the left.
        assert_eq!(visible_pixels(90, [1000.0, 2000.0], origin, 1.0, clip), [0.0, 1500.0, 200.0, 2000.0]);
        assert_eq!(visible_pixels(180, [1000.0, 2000.0], origin, 1.0, clip), [500.0, 1800.0, 1000.0, 2000.0]);
        assert_eq!(visible_pixels(270, [1000.0, 2000.0], origin, 1.0, clip), [800.0, 0.0, 1000.0, 500.0]);
    }

    #[test]
    fn reveal_looks_at_the_turned_page() {
        let mut l = layout();
        l.set_rotation(90);
        let mut v = view(600.0, 400.0);
        // Page 1 is 600 tall turned, from 610. Its x of 590 is at y 10 turned.
        v.reveal(&l, 1, [590.0, 700.0, 595.0, 710.0]);
        assert_eq!(v.y, 610.0 + 592.5 - 200.0);
    }

    const SIZES: [[f32; 2]; 5] = [[600.0, 800.0], [800.0, 600.0], [1.0, 1000.0], [333.5, 71.25], [500.0, 500.0]];

    fn close(a: [f32; 2], b: [f32; 2]) -> bool {
        (a[0] - b[0]).abs() < 1e-2 && (a[1] - b[1]).abs() < 1e-2
    }

    #[test]
    fn prop_to_page_inverts_point_everywhere() {
        for rotation in ROTATIONS {
            for size in SIZES {
                for (origin, scale) in [(pos2(0.0, 0.0), 1.0), (pos2(-37.5, 12.25), 0.37), (pos2(1000.0, -500.0), 3.0)] {
                    let t = PageTransform { origin, scale, rotation, size };
                    let mut points =
                        vec![[0.0, 0.0], [size[0], 0.0], [0.0, size[1]], size, [size[0] / 2.0, 0.0], [0.0, size[1] / 2.0], [size[0], size[1] / 2.0]];
                    for i in 0..=20 {
                        for j in 0..=20 {
                            points.push([size[0] * i as f32 / 20.0, size[1] * j as f32 / 20.0]);
                        }
                    }
                    for p in points {
                        let back = t.to_page(t.point(p));
                        assert!(close(back, p), "{rotation} {size:?} {scale}: {p:?} -> {back:?}");
                        let again = unrotate_point(rotation, size, rotate_point(rotation, size, p));
                        assert!(close(again, p));
                    }
                }
            }
        }
    }

    #[test]
    fn prop_corners_go_to_the_expected_corners_clockwise() {
        // Page corners in order: top-left, top-right, bottom-right, bottom-left. Turned clockwise
        // by 90, the top-left goes to the top-right, so the list rotates by one corner per quarter.
        for size in SIZES {
            let [w, h] = size;
            let corners = [[0.0, 0.0], [w, 0.0], [w, h], [0.0, h]];
            for (rotation, shift) in [(0u16, 0usize), (90, 1), (180, 2), (270, 3)] {
                let (tw, th) = if rotation % 180 == 0 { (w, h) } else { (h, w) };
                let shown = [[0.0, 0.0], [tw, 0.0], [tw, th], [0.0, th]];
                for (i, c) in corners.iter().enumerate() {
                    let got = rotate_point(rotation, size, *c);
                    assert!(close(got, shown[(i + shift) % 4]), "{rotation} {size:?} corner {i}: {got:?}");
                }
            }
        }
    }

    #[test]
    fn prop_tile_uv_agrees_with_rotate_point() {
        // Screen corners of a tile at pixel box (x, y, tw, th) of a raster; the uv at each screen
        // corner is the tile's page corner that rotate_point puts there.
        let raster = [1000.0_f32, 1700.0];
        for rotation in ROTATIONS {
            for (x, y, tw, th) in [(0.0, 0.0, 1000.0, 1700.0), (0.0, 0.0, 100.0, 50.0), (900.0, 1650.0, 100.0, 50.0), (24.0, 512.0, 300.0, 77.0)] {
                let uv = tile_uv(rotation);
                let page_corner = |u: Pos2| [x + u.x * tw, y + u.y * th];
                let shown: Vec<[f32; 2]> = uv.iter().map(|u| rotate_point(rotation, raster, page_corner(*u))).collect();
                let (x0, x1) = (shown.iter().map(|p| p[0]).fold(f32::MAX, f32::min), shown.iter().map(|p| p[0]).fold(f32::MIN, f32::max));
                let (y0, y1) = (shown.iter().map(|p| p[1]).fold(f32::MAX, f32::min), shown.iter().map(|p| p[1]).fold(f32::MIN, f32::max));
                let want = [[x0, y0], [x1, y0], [x1, y1], [x0, y1]];
                for (got, want) in shown.iter().zip(want) {
                    assert!(close(*got, want), "{rotation} ({x},{y}): {shown:?} vs {want:?}");
                }
                // And the tile's top-left page point is where uv (0,0) is shown.
                let corner = rotate_point(rotation, raster, [x, y]);
                let at00 = uv.iter().position(|u| *u == pos2(0.0, 0.0)).unwrap();
                assert!(close(shown[at00], corner), "{rotation}");
            }
        }
    }

    #[test]
    fn prop_visible_pixels_with_tiles_matches_brute_force() {
        let (pw, ph) = (1300u32, 2100u32);
        let side = 512u32;
        let origin = pos2(100.0, 50.0);
        let unit = 0.5;
        for rotation in ROTATIONS {
            let pixels = [pw as f32, ph as f32];
            let turned = if rotation % 180 == 0 { [pixels[0], pixels[1]] } else { [pixels[1], pixels[0]] };
            let shown = Rect::from_min_size(origin, vec2(turned[0], turned[1]) * unit);
            // Clips inside the page, with a positive area, and some that touch tile edges
            // from inside only.
            let mut clips = vec![];
            for i in 0..12 {
                for j in 0..12 {
                    let a = pos2(shown.left() + shown.width() * i as f32 / 12.0, shown.top() + shown.height() * j as f32 / 12.0);
                    for (dw, dh) in [(1.0, 1.0), (37.3, 80.1), (300.0, 20.0), (shown.width(), shown.height())] {
                        clips.push(Rect::from_min_max(a, pos2((a.x + dw).min(shown.right()), (a.y + dh).min(shown.bottom()))));
                    }
                }
            }
            for clip in clips {
                if !(clip.width() > 0.0 && clip.height() > 0.0) {
                    continue;
                }
                let got = tiles_crossing(pw, ph, visible_pixels(rotation, pixels, origin, unit, clip), side);
                // Brute force: a tile is needed when its screen rect overlaps the clip with area.
                let mut want = vec![];
                for ty in 0..ph.div_ceil(side) {
                    for tx in 0..pw.div_ceil(side) {
                        let (x, y) = (tx * side, ty * side);
                        let (w, h) = (side.min(pw - x), side.min(ph - y));
                        let a = rotate_point(rotation, pixels, [x as f32, y as f32]);
                        let b = rotate_point(rotation, pixels, [(x + w) as f32, (y + h) as f32]);
                        let r = Rect::from_two_pos(origin + vec2(a[0], a[1]) * unit, origin + vec2(b[0], b[1]) * unit);
                        let inter = r.intersect(clip);
                        if inter.width() > 1e-3 && inter.height() > 1e-3 {
                            want.push((x, y, w, h));
                        }
                    }
                }
                let mut got: Vec<_> = got.iter().map(|t| (t.x, t.y, t.w, t.h)).collect();
                got.sort_unstable();
                want.sort_unstable();
                // Every tile that truly intersects must be present; extras may only touch an edge.
                for t in &want {
                    assert!(got.contains(t), "{rotation} {clip:?}: missing {t:?}");
                }
                for t in got.iter().filter(|t| !want.contains(t)) {
                    let a = rotate_point(rotation, pixels, [t.0 as f32, t.1 as f32]);
                    let b = rotate_point(rotation, pixels, [(t.0 + t.2) as f32, (t.1 + t.3) as f32]);
                    let r = Rect::from_two_pos(origin + vec2(a[0], a[1]) * unit, origin + vec2(b[0], b[1]) * unit);
                    let gap_x = (r.left() - clip.right()).max(clip.left() - r.right());
                    let gap_y = (r.top() - clip.bottom()).max(clip.top() - r.bottom());
                    assert!(gap_x.max(gap_y) < 1e-3, "{rotation} {clip:?}: extra tile {t:?} is not even touching");
                }
            }
        }
    }

    #[test]
    fn visible_pixels_edge_touching_clip_reports_the_neighbour() {
        // A clip that ends exactly on a tile boundary: record how many tiles are asked for.
        // Rotation 0: clip right edge at pixel 512 exactly.
        let origin = pos2(0.0, 0.0);
        let clip = Rect::from_min_max(pos2(0.0, 0.0), pos2(512.0, 100.0));
        let n = tiles_crossing(1300, 2100, visible_pixels(0, [1300.0, 2100.0], origin, 1.0, clip), 512).len();
        assert!(n <= 2, "{n}");
    }

    #[test]
    fn set_rotation_round_trips_and_normalises() {
        let mut l = layout();
        let (tops, total, mw) = (l.tops.clone(), l.total, l.max_width);
        // set_rotation is absolute: 90 then 270 is 270, and 90 + 270 is the original.
        l.set_rotation(90);
        l.set_rotation(270);
        assert_eq!(l.rotation, 270);
        l.set_rotation(90 + 270);
        assert_eq!((l.tops.clone(), l.total, l.max_width, l.rotation), (tops.clone(), total, mw, 0));
        l.set_rotation(0);
        assert_eq!((l.tops.clone(), l.total), (tops, total));
        for (input, want) in [
            (-90, 270u16),
            (-450, 270),
            (450, 90),
            (i64::MAX, normalize_rotation(i64::MAX)),
            (i64::MIN, normalize_rotation(i64::MIN)),
            (1_000_000_000_000, normalize_rotation(1_000_000_000_000)),
        ] {
            l.set_rotation(input);
            assert_eq!(l.rotation, want, "{input}");
            assert!(l.rotation < 360 && l.rotation.is_multiple_of(90));
        }
        let mut l = Layout::new(vec![], 8.0);
        l.set_rotation(90);
        assert_eq!((l.total, l.max_width), (0.0, 0.0));
    }

    #[test]
    fn normalize_rotation_edge_values() {
        assert_eq!(normalize_rotation(-90), 270);
        assert_eq!(normalize_rotation(450), 90);
        assert_eq!(normalize_rotation(45), 0);
        assert_eq!(normalize_rotation(-45), 270);
        assert_eq!(normalize_rotation(89), 0);
        assert_eq!(normalize_rotation(359), 270);
        for v in [i64::MIN, i64::MAX, 1_000_000_000_007, -1_000_000_000_007, 70000] {
            let r = normalize_rotation(v);
            assert!(r < 360 && r.is_multiple_of(90), "{v} -> {r}");
        }
    }

    #[test]
    fn shown_rect_agrees_with_the_transform_rect() {
        let mut l = layout();
        for rotation in ROTATIONS {
            l.set_rotation(rotation.into());
            let size = l.page_sizes[2];
            let t = PageTransform { origin: pos2(0.0, 0.0), scale: 1.0, rotation, size };
            for r in [[10.0, 20.0, 100.0, 300.0], [0.0, 0.0, 300.0, 400.0], [299.0, 399.0, 300.0, 400.0]] {
                let s = l.shown_rect(2, r);
                let q = t.rect(r);
                assert!(close([s[0], s[1]], [q.min.x, q.min.y]) && close([s[2], s[3]], [q.max.x, q.max.y]), "{rotation} {r:?}");
            }
        }
    }

    #[test]
    fn shown_points_and_turned_anchors_are_lossless_on_a_non_square_page() {
        let mut l = Layout::new(vec![[600.0, 800.0]], 8.0);
        for rotation in ROTATIONS {
            l.set_rotation(rotation.into());
            let p = [123.0, 456.0];
            let d = l.shown_point(0, p);
            assert_eq!(unrotate_point(rotation, [600.0, 800.0], d), p, "{rotation}");
        }
        for from in ROTATIONS {
            for to in ROTATIONS {
                for a in [Anchor { page: 0, y: 456.0, x: 0.0 }, Anchor { page: 0, y: 10.0, x: -40.0 }] {
                    let there = l.turn_anchor(a, from, to);
                    let back = l.turn_anchor(there, to, from);
                    assert!((back.y - a.y).abs() < 1e-3 && (back.x - a.x).abs() < 1e-3, "{from}->{to}: {a:?} {back:?}");
                }
            }
        }
    }

    #[test]
    fn a_quarter_turn_of_an_anchor_by_hand() {
        let l = Layout::new(vec![[600.0, 800.0]], 8.0);
        // Unturned, y 100 and x 50 right of the centre is the page point [350, 100]. Turned 90 degrees
        // clockwise (the page is then 800 wide) that point is at [800 - 100, 350] = [700, 350], which is
        // 700 - 400 = 300 right of the centre, 350 down.
        let got = l.turn_anchor(Anchor { page: 0, y: 100.0, x: 50.0 }, 0, 90);
        assert_eq!((got.page, got.y, got.x), (0, 350.0, 300.0));
        // A half turn keeps the width: [350, 100] -> [250, 700], x -50.
        let got = l.turn_anchor(Anchor { page: 0, y: 100.0, x: 50.0 }, 0, 180);
        assert_eq!((got.y, got.x), (700.0, -50.0));
        // A page that isn't there is left alone.
        let gone = Anchor { page: 9, y: 1.0, x: 2.0 };
        assert_eq!(l.turn_anchor(gone, 0, 90), gone);
    }

    #[test]
    fn a_page_point_is_shown_where_the_rotation_puts_it() {
        let mut l = Layout::new(vec![[600.0, 800.0]], 8.0);
        assert_eq!(l.shown_point(0, [100.0, 200.0]), [100.0, 200.0]);
        l.set_rotation(90);
        assert_eq!(l.shown_point(0, [100.0, 200.0]), [600.0, 100.0]);
        l.set_rotation(180);
        assert_eq!(l.shown_point(0, [100.0, 200.0]), [500.0, 600.0]);
    }
}

#[cfg(test)]
mod turn_fuzz_tests {
    use super::*;

    /// A small deterministic xorshift generator, as the crate has no property-testing dependency.
    struct Rng(u64);

    impl Rng {
        fn next(&mut self) -> u64 {
            self.0 ^= self.0 << 13;
            self.0 ^= self.0 >> 7;
            self.0 ^= self.0 << 17;
            self.0
        }

        /// A float in `lo..hi`, on a coarse grid so that the arithmetic is exact in f32.
        fn float(&mut self, lo: f32, hi: f32) -> f32 {
            lo + (hi - lo) * (self.next() % 4096) as f32 / 4096.0
        }
    }

    const ROTATIONS: [u16; 4] = [0, 90, 180, 270];

    fn close(a: f32, b: f32) -> bool {
        (a - b).abs() <= 1e-2 + 1e-5 * a.abs().max(b.abs())
    }

    fn same(a: Anchor, b: Anchor) -> bool {
        a.page == b.page && close(a.y, b.y) && close(a.x, b.x)
    }

    fn cases(count: usize) -> Vec<(Layout, Anchor)> {
        let mut rng = Rng(0x9E37_79B9_7F4A_7C15);
        (0..count)
            .map(|_| {
                let pages = 1 + (rng.next() % 4) as usize;
                let sizes = (0..pages).map(|_| [rng.float(10.0, 2000.0), rng.float(10.0, 2000.0)]).collect();
                let layout = Layout::new(sizes, 8.0);
                let page = (rng.next() % pages as u64) as usize;
                let [w, h] = layout.page_sizes[page];
                // Any in-page point, whatever the rotation: `x` within half the narrower side.
                let a = Anchor { page, y: rng.float(0.5, w.min(h)), x: rng.float(-w.min(h) / 2.0, w.min(h) / 2.0) };
                (layout, a)
            })
            .collect()
    }

    #[test]
    fn turn_anchor_round_trips_for_every_pair() {
        for (layout, a) in cases(2000) {
            for from in ROTATIONS {
                for to in ROTATIONS {
                    let there = layout.turn_anchor(a, from, to);
                    let back = layout.turn_anchor(there, to, from);
                    assert!(same(a, back), "{:?} {a:?} {from}->{to}: {back:?}", layout.page_sizes[a.page]);
                }
            }
        }
    }

    #[test]
    fn turn_anchor_to_the_same_rotation_is_the_identity() {
        for (layout, a) in cases(500) {
            for r in ROTATIONS {
                assert!(same(a, layout.turn_anchor(a, r, r)), "{:?} {a:?} {r}", layout.page_sizes[a.page]);
            }
        }
    }

    #[test]
    fn four_quarter_turns_return_the_original() {
        for (layout, a) in cases(2000) {
            for start in ROTATIONS {
                let mut at = a;
                let mut r = start;
                for _ in 0..4 {
                    let next = (r + 90) % 360;
                    at = layout.turn_anchor(at, r, next);
                    r = next;
                }
                assert!(same(a, at), "{:?} {a:?} from {start}: {at:?}", layout.page_sizes[a.page]);
            }
        }
    }

    #[test]
    fn an_anchor_at_or_above_the_page_top_stays_at_the_top() {
        let layout = Layout::new(vec![[600.0, 800.0]], 8.0);
        for y in [0.0, -5.0, -0.5] {
            for from in ROTATIONS {
                for to in ROTATIONS {
                    let there = layout.turn_anchor(Anchor { page: 0, y, x: 40.0 }, from, to);
                    assert_eq!(there.y, 0.0, "{y} {from}->{to}");
                    // The x survives a half turn or none, and is the column's centre across a quarter turn.
                    let quarter = from % 180 != to % 180;
                    assert_eq!(there.x, if quarter { 0.0 } else { 40.0 }, "{y} {from}->{to}");
                }
            }
        }
    }

    #[test]
    fn turn_anchor_keeps_the_page_point() {
        for (mut layout, a) in cases(2000) {
            for from in ROTATIONS {
                for to in ROTATIONS {
                    let size = layout.page_sizes[a.page];
                    let width = |r: u16| if r.is_multiple_of(180) { size[0] } else { size[1] };
                    layout.set_rotation(i64::from(from));
                    let point = unrotate_point(from, size, [width(from) / 2.0 + a.x, a.y]);
                    let turned = layout.turn_anchor(a, from, to);
                    layout.set_rotation(i64::from(to));
                    let shown = layout.shown_point(a.page, point);
                    assert!(close(shown[0], width(to) / 2.0 + turned.x) && close(shown[1], turned.y), "{size:?} {a:?} {from}->{to}");
                    // And the page point is the same one the turned anchor unrotates to.
                    let back = unrotate_point(to, size, shown);
                    assert!(close(back[0], point[0]) && close(back[1], point[1]));
                }
            }
        }
    }

    #[test]
    fn shown_point_at_rotation_zero_is_the_identity() {
        let layout = Layout::new(vec![[300.0, 500.0], [612.0, 792.0]], 8.0);
        let mut rng = Rng(7);
        for _ in 0..500 {
            let p = [rng.float(-100.0, 900.0), rng.float(-100.0, 900.0)];
            for page in 0..2 {
                assert_eq!(layout.shown_point(page, p), p);
            }
        }
    }

    #[test]
    fn shown_point_stays_inside_the_shown_page() {
        let mut rng = Rng(11);
        for r in ROTATIONS {
            let mut layout = Layout::new(vec![[300.0, 500.0]], 8.0);
            layout.set_rotation(i64::from(r));
            let [w, h] = layout.sizes[0];
            for _ in 0..200 {
                let p = layout.shown_point(0, [rng.float(0.0, 300.0), rng.float(0.0, 500.0)]);
                assert!((0.0..=w).contains(&p[0]) && (0.0..=h).contains(&p[1]), "{r} {p:?}");
            }
        }
    }

    #[test]
    fn turn_anchor_on_a_page_past_the_end_is_unchanged() {
        let layout = Layout::new(vec![[300.0, 500.0]], 8.0);
        for page in [1, 7, usize::MAX] {
            let a = Anchor { page, y: 12.0, x: -3.0 };
            assert_eq!(layout.turn_anchor(a, 0, 90), a);
        }
        assert_eq!(Layout::default().turn_anchor(Anchor { page: 0, y: 1.0, x: 2.0 }, 90, 270), Anchor { page: 0, y: 1.0, x: 2.0 });
    }

    #[test]
    fn turn_anchor_hand_computed_on_a_non_square_page() {
        // 600 wide, 800 high. The page point at the top of the view, centred: [300, 100].
        let layout = Layout::new(vec![[600.0, 800.0]], 8.0);
        let a = Anchor { page: 0, y: 100.0, x: 0.0 };
        // At 90 the point [300, 100] is shown at [800 - 100, 300] on a page 800 wide: x = 700 - 400.
        assert_eq!(layout.turn_anchor(a, 0, 90), Anchor { page: 0, y: 300.0, x: 300.0 });
        // At 180 it is at [300, 700].
        assert_eq!(layout.turn_anchor(a, 0, 180), Anchor { page: 0, y: 700.0, x: 0.0 });
        // At 270 it is at [100, 300] on a page 800 wide: x = 100 - 400.
        assert_eq!(layout.turn_anchor(a, 0, 270), Anchor { page: 0, y: 300.0, x: -300.0 });
    }
}

#[cfg(test)]
mod rotation_round_two_tests {
    use super::*;

    const ROTATIONS: [i64; 4] = [0, 90, 180, 270];
    const W: f32 = 300.0;
    const H: f32 = 500.0;

    fn layout(rotation: i64) -> Layout {
        let mut layout = Layout::new(vec![[W, H]], 8.0);
        layout.set_rotation(rotation);
        layout
    }

    fn grid(max: f32) -> Vec<f32> {
        (0..=8).map(|i| max * i as f32 / 8.0).collect()
    }

    #[test]
    fn shown_dest_y_is_within_the_displayed_height_for_every_presence_combination() {
        for r in ROTATIONS {
            let l = layout(r);
            let shown_h = l.sizes[0][1];
            let xs: Vec<Option<f32>> = std::iter::once(None).chain(grid(W).into_iter().map(Some)).collect();
            let ys: Vec<Option<f32>> = std::iter::once(None).chain(grid(H).into_iter().map(Some)).collect();
            for &x in &xs {
                for &y in &ys {
                    let v = l.shown_dest_y(0, x, y);
                    assert!((0.0..=shown_h).contains(&v), "r {r} x {x:?} y {y:?} -> {v} not in 0..={shown_h}");
                    if y.is_none() {
                        assert_eq!(v, 0.0, "no y is the top at {r}");
                    }
                    if x.is_none() && r % 180 != 0 {
                        assert_eq!(v, 0.0, "no x is the top at {r}");
                    }
                }
            }
        }
    }

    #[test]
    fn shown_dest_y_has_the_documented_value_at_each_rotation() {
        let (x, y) = (Some(70.0), Some(120.0));
        assert_eq!(layout(0).shown_dest_y(0, x, y), 120.0);
        assert_eq!(layout(180).shown_dest_y(0, x, y), H - 120.0);
        assert_eq!(layout(90).shown_dest_y(0, x, y), 70.0);
        assert_eq!(layout(270).shown_dest_y(0, x, y), W - 70.0);
        assert_eq!(layout(270).shown_dest_y(0, None, y), 0.0);
        assert_eq!(layout(90).shown_dest_y(0, None, y), 0.0);
    }

    #[test]
    fn at_rotation_zero_shown_dest_y_is_the_old_behaviour() {
        let l = layout(0);
        for x in [None, Some(0.0), Some(123.0), Some(-5.0)] {
            for y in [None, Some(0.0), Some(1.5), Some(499.0), Some(-3.0), Some(900.0)] {
                assert_eq!(l.shown_dest_y(0, x, y), y.unwrap_or(0.0), "{x:?} {y:?}");
                assert_eq!(l.shown_spot_y(0, x, y, None), y.unwrap_or(0.0));
            }
        }
    }

    #[test]
    fn shown_spot_y_agrees_with_shown_dest_y_without_a_rect_and_is_within_the_page_with_one() {
        let rects = [[10.0, 20.0, 200.0, 400.0], [0.0, 0.0, W, H], [50.0, 60.0, 50.0, 60.0], [299.0, 499.0, 300.0, 500.0]];
        for r in ROTATIONS {
            let l = layout(r);
            let shown_h = l.sizes[0][1];
            for x in [None, Some(30.0)] {
                for y in [None, Some(40.0)] {
                    assert_eq!(l.shown_spot_y(0, x, y, None), l.shown_dest_y(0, x, y));
                    for rect in rects {
                        let v = l.shown_spot_y(0, x, y, Some(rect));
                        assert!((0.0..=shown_h).contains(&v), "r {r} {rect:?} -> {v}");
                        // The topmost corner: no corner is shown higher.
                        let [x0, y0, x1, y1] = rect;
                        let lowest_corner_y =
                            [[x0, y0], [x1, y0], [x0, y1], [x1, y1]].iter().map(|c| l.shown_point(0, *c)[1]).fold(f32::INFINITY, f32::min);
                        assert!((v - lowest_corner_y).abs() < 1e-3, "r {r} {rect:?}: {v} vs topmost {lowest_corner_y}");
                    }
                }
            }
        }
    }

    #[test]
    fn turn_anchor_at_or_above_the_top_stays_at_the_top_for_every_pair() {
        let l = layout(0);
        for from in [0u16, 90, 180, 270] {
            for to in [0u16, 90, 180, 270] {
                let quarter = from % 180 != to % 180;
                for y in [0.0, -0.0, -1.0, -250.0] {
                    for x in [-120.0, 0.0, 33.0] {
                        let got = l.turn_anchor(Anchor { page: 0, y, x }, from, to);
                        assert_eq!(got.y, 0.0, "{from}->{to} y {y}");
                        assert_eq!(got.x, if quarter { 0.0 } else { x }, "{from}->{to} y {y} x {x}");
                    }
                }
            }
        }
    }

    #[test]
    fn turn_anchor_slightly_below_the_top_is_lossless_and_stays_near_the_top_of_its_page() {
        let l = layout(0);
        for from in [0u16, 90, 180, 270] {
            for to in [0u16, 90, 180, 270] {
                for y in [0.001, 0.01, 0.5] {
                    let a = Anchor { page: 0, y, x: 12.0 };
                    let there = l.turn_anchor(a, from, to);
                    let back = l.turn_anchor(there, to, from);
                    assert!((back.y - y).abs() < 1e-2 && (back.x - 12.0).abs() < 1e-2, "{from}->{to} y {y}: {back:?}");
                    assert!(there.y.is_finite() && there.x.is_finite());
                }
            }
        }
    }
}
