//! Drawing pages and what lies on them, and the portal window.
use egui::epaint::{TextShape, Vertex};
use egui::{Align2, Color32, FontId, Mesh, Painter, Pos2, Rect, Shape, Stroke, TextureId, Vec2, ViewportBuilder, ViewportId, pos2, vec2};
use pdfcraft_render::{RenderRequest, Tile, device_pixels, effective_scale};

use crate::app::{App, Mode, Tool, ZOOM_SETTLE};
use crate::cache::TileKey;
use crate::config::Color;
use crate::hint::HintState;
use crate::present::{PREFETCH_AHEAD, PREFETCH_BEHIND};
use crate::store::{Annotation, MarkKind, NOTE_SIDE, Portal, mark_rect};
use crate::view::{View, rotate_point, tile_side, tile_uv, tiles_crossing, visible_pixels};

const UV: Rect = Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0));
/// Pages up to this long in device pixels are rendered whole: a tile costs an interpretation of
/// the whole page, so one raster of a page is far cheaper than the same pixels in tiles.
const WHOLE_PAGE_MAX: u32 = 4096;
/// The long side of the low-resolution raster kept under the tiles of a larger page.
const BACKDROP_SIDE: f32 = 2048.0;
/// How wide a note's text may get on screen.
const NOTE_WIDTH: f32 = 280.0;

/// Text in the document scales with it, and a font is rasterised anew at each size: whole pixels,
/// and coarser steps when large, bound how many sizes zooming goes through.
fn font_pixels(size: f32) -> f32 {
    if size < 48.0 { size.round().max(4.0) } else { ((size / 8.0).round() * 8.0).min(512.0) }
}

/// `area` moved the least that puts it inside `bounds` (as far as it fits).
fn clamp_into(area: Rect, bounds: Rect) -> Rect {
    let dx = (bounds.right() - area.right()).min(0.0).max(bounds.left() - area.left());
    let dy = (bounds.bottom() - area.bottom()).min(0.0).max(bounds.top() - area.top());
    area.translate(vec2(dx, dy))
}

fn translucent(color: [u8; 3], alpha: u8) -> Color32 {
    Color32::from_rgba_unmultiplied(color[0], color[1], color[2], alpha)
}

/// Moves a label right, out of the one before it on the same line: a label is often wider than
/// the short word it stands for.
fn clear_of(previous: Option<Rect>, label: Rect) -> Rect {
    match previous {
        Some(p) if p.right() >= label.left() && p.bottom() > label.center().y && p.top() < label.center().y => {
            label.translate(vec2(p.right() + 1.0 - label.left(), 0.0))
        }
        _ => label,
    }
}

/// Where the pixel box `[x, y, width, height]` of an unrotated raster of `pixels` lands on screen,
/// when the page is shown turned by `rotation` with its top-left at `origin`.
fn pixel_rect(rotation: u16, pixels: [f32; 2], origin: Pos2, unit: f32, [x, y, width, height]: [f32; 4]) -> Rect {
    let at = |p: [f32; 2]| {
        let d = rotate_point(rotation, pixels, p);
        origin + vec2(d[0], d[1]) * unit
    };
    Rect::from_two_pos(at([x, y]), at([x + width, y + height]))
}

/// Draws the texture of an unrotated tile in `rect`, turned by `rotation`.
fn paint_tile(painter: &Painter, texture: TextureId, rect: Rect, rotation: u16) {
    if rotation == 0 {
        painter.image(texture, rect, UV, Color32::WHITE);
        return;
    }
    let mut mesh = Mesh::with_texture(texture);
    let corners = [rect.left_top(), rect.right_top(), rect.right_bottom(), rect.left_bottom()];
    for (pos, uv) in corners.into_iter().zip(tile_uv(rotation)) {
        mesh.vertices.push(Vertex { pos, uv, color: Color32::WHITE });
    }
    mesh.indices.extend([0, 1, 2, 0, 2, 3]);
    painter.add(Shape::mesh(mesh));
}

/// How a page is rendered and placed for a view.
struct Placement {
    page: usize,
    scale: f32,
    /// The raster's size, unrotated.
    width: u32,
    height: u32,
    /// The top-left of the page as shown.
    origin: Pos2,
    /// Screen points per rendered pixel.
    unit: f32,
    /// Degrees clockwise.
    rotation: u16,
}

impl Placement {
    fn pixels(&self) -> [f32; 2] {
        [self.width as f32, self.height as f32]
    }

    /// The page as shown, turned.
    fn rect(&self) -> Rect {
        let [w, h] = self.pixels();
        let turned = !self.rotation.is_multiple_of(180);
        Rect::from_min_size(self.origin, if turned { vec2(h, w) } else { vec2(w, h) } * self.unit)
    }

    fn tile_rect(&self, x: u32, y: u32, width: u32, height: u32) -> Rect {
        pixel_rect(self.rotation, self.pixels(), self.origin, self.unit, [x as f32, y as f32, width as f32, height as f32])
    }

    fn request(&self, unit: Option<Tile>) -> RenderRequest {
        RenderRequest { page: self.page, scale: self.scale, tile: unit, ..Default::default() }
    }

    fn whole(&self) -> bool {
        self.width.max(self.height) <= WHOLE_PAGE_MAX
    }

    /// What has to be rendered to show the pixel area `visible`: the whole page, or its tiles.
    fn units(&self, visible: [f32; 4]) -> Vec<Option<Tile>> {
        if self.whole() {
            return vec![None];
        }
        let side = tile_side(self.scale, self.width.max(self.height));
        tiles_crossing(self.width, self.height, visible, side).into_iter().map(Some).collect()
    }

    fn key(&self, unit: Option<Tile>) -> TileKey {
        TileKey::new(self.page, self.scale, unit)
    }
}

impl App {
    fn place(&self, view: &View, page: usize, ppp: f32, render_zoom: f32) -> Placement {
        let [w, h] = self.doc.layout.page_sizes[page];
        let scale = effective_scale(w, h, render_zoom * ppp);
        let at = view.transform(&self.doc.layout, page).origin;
        // On whole pixels, so a tile at its own scale is copied to the screen unchanged.
        let origin = pos2((at.x * ppp).round() / ppp, (at.y * ppp).round() / ppp);
        Placement {
            page,
            scale,
            width: device_pixels(w, scale),
            height: device_pixels(h, scale),
            origin,
            unit: view.zoom / scale,
            rotation: self.doc.layout.rotation,
        }
    }

    pub fn draw_page(&mut self, painter: &Painter, view: &View, page: usize, ppp: f32, render_zoom: f32) {
        // A page with nothing to stretch renders for the zoom as it is now, not as it was.
        let render_zoom = if self.cache.has_page(page) { render_zoom } else { view.zoom };
        let place = self.place(view, page, ppp, render_zoom);
        let area = place.rect();
        painter.rect_filled(area, 0.0, self.config.active_theme().page.egui());
        let clip = area.intersect(view.rect);
        if !clip.is_positive() {
            return;
        }
        let units = place.units(visible_pixels(place.rotation, place.pixels(), place.origin, place.unit, clip));
        if !place.whole() {
            self.keep_backdrop(page);
        }
        if units.iter().any(|u| !self.cache.settled(&place.key(*u))) {
            for (key, texture, width, height) in self.cache.stale(page, place.scale) {
                let unit = view.zoom / f32::from_bits(key.scale);
                let (x, y) = key.offset();
                let [w, h] = self.doc.layout.page_sizes[page];
                let pixels = [device_pixels(w, f32::from_bits(key.scale)) as f32, device_pixels(h, f32::from_bits(key.scale)) as f32];
                let rect = pixel_rect(place.rotation, pixels, place.origin, unit, [x as f32, y as f32, width as f32, height as f32]);
                paint_tile(painter, texture, rect, place.rotation);
            }
        } else {
            self.cache.settle(page);
        }
        for unit in units {
            let key = place.key(unit);
            if let Some(cached) = self.cache.touch(&key) {
                let (x, y) = key.offset();
                paint_tile(painter, cached.texture.id(), place.tile_rect(x, y, cached.width, cached.height), place.rotation);
            } else if !self.cache.settled(&key) {
                self.wants.push(place.request(unit));
            }
        }
    }

    /// Keeps a low-resolution raster of a tiled page, so zooming and panning never show it empty.
    fn keep_backdrop(&mut self, page: usize) {
        let [w, h] = self.doc.layout.page_sizes[page];
        let scale = effective_scale(w, h, BACKDROP_SIDE / w.max(h).max(1.0));
        let key = TileKey::new(page, scale, None);
        if self.cache.touch(&key).is_none() && !self.cache.settled(&key) {
            self.wants.push(RenderRequest { page, scale, preview: true, ..Default::default() });
        }
    }

    /// Asks for the pages around the view, within the columns showing.
    fn prefetch(&mut self, view: &View, ppp: f32) {
        let shown = view.visible_pages(&self.doc.layout);
        let (first, last) = (*shown.start(), *shown.end());
        let reach = self.config.view.prefetch_pages;
        let after = (last + 1..self.doc.layout.len()).take(reach);
        let before = (0..first).rev().take(reach);
        // Half the budget at most: what is kept ahead must not push out what it was kept for, or
        // be evicted itself and asked for again, for ever. Being touched spares it this frame.
        let mut room = self.config.view.texture_budget_mb.saturating_mul(1024 * 1024) / 2;
        for page in after.chain(before) {
            if !self.prefetch_page(view, page, ppp, self.render_zoom, &mut room) {
                return;
            }
        }
    }

    /// Asks for the tiles of one page that `view` would show, as long as `room` (bytes) lasts.
    /// Returns whether it did: false once the room has run out.
    fn prefetch_page(&mut self, view: &View, page: usize, ppp: f32, render_zoom: f32, room: &mut usize) -> bool {
        let place = self.place(view, page, ppp, render_zoom);
        // The columns of the view, all the way down the page.
        let shown = place.rect();
        // A page wholly to one side of the view, as the other page of a pair can be, has nothing in it.
        if shown.right() < view.rect.left() || shown.left() > view.rect.right() {
            return true;
        }
        let columns = Rect::from_min_max(pos2(view.rect.left(), shown.top()), pos2(view.rect.right(), shown.bottom()));
        for unit in place.units(visible_pixels(place.rotation, place.pixels(), place.origin, place.unit, columns)) {
            let (width, height) = unit.map_or((place.width, place.height), |t| (t.w, t.h));
            let Some(left_over) = room.checked_sub(width as usize * height as usize * 4) else { return false };
            *room = left_over;
            let key = place.key(unit);
            if self.cache.touch(&key).is_none() && !self.cache.settled(&key) {
                self.wants.push(place.request(unit));
            }
        }
        true
    }

    /// Draws the page presented, then asks for the ones around it, each fit to the screen as it
    /// will be shown, so turning to it finds it rendered.
    pub fn draw_presentation(&mut self, ui: &egui::Ui, page: usize) {
        let painter = ui.painter().with_clip_rect(self.view.rect);
        let ppp = ui.ctx().pixels_per_point();
        let view = self.view;
        self.draw_page(&painter, &view, page, ppp, view.zoom);
        for annotation in self.side.annotations.iter().filter(|a| a.page() == page) {
            self.draw_annotation(&painter, &view, annotation);
        }
        let len = self.doc.layout.len();
        let ahead = (page + 1..len).take(PREFETCH_AHEAD);
        let behind = (0..page).rev().take(PREFETCH_BEHIND);
        let mut room = self.config.view.texture_budget_mb.saturating_mul(1024 * 1024) / 2;
        for near in ahead.chain(behind) {
            let near_view = self.presented_view(near);
            if !self.prefetch_page(&near_view, near, ppp, near_view.zoom, &mut room) {
                return;
            }
        }
    }

    pub fn draw_document(&mut self, ui: &egui::Ui) {
        let painter = ui.painter().with_clip_rect(self.view.rect);
        let ppp = ui.ctx().pixels_per_point();
        let view = self.view;
        for page in view.visible_pages(&self.doc.layout) {
            self.draw_page(&painter, &view, page, ppp, self.render_zoom);
        }
        // While zooming, every worker goes to what is showing.
        if self.zoom_changed.elapsed() >= ZOOM_SETTLE {
            self.prefetch(&view, ppp);
        }
        self.draw_marks(&painter, &view);
        self.draw_portal_markers(&painter, &view);
        if let Mode::Hint(state) = &self.mode {
            self.draw_hints(&painter, &view, state);
        }
        self.draw_overview(ui);
    }

    fn fill_rects(&self, painter: &Painter, view: &View, page: usize, rects: &[[f32; 4]], color: Color) {
        let t = view.transform(&self.doc.layout, page);
        for r in rects {
            painter.rect_filled(t.rect(*r), 0.0, color.egui());
        }
    }

    /// Search results, highlights, drawings and the selection.
    pub fn draw_marks(&self, painter: &Painter, view: &View) {
        let shown = view.visible_pages(&self.doc.layout);
        let theme = self.config.active_theme();
        if self.links_shown {
            for page in shown.clone().filter(|p| self.cache.shows_page(*p)) {
                if let Some(rects) = self.doc.internal_links.get(page) {
                    self.fill_rects(painter, view, page, rects, theme.link);
                }
            }
        }
        if self.fields_shown {
            for page in shown.clone().filter(|p| self.cache.shows_page(*p)) {
                if let Some(rects) = self.doc.form.widget_rects.get(page) {
                    self.fill_rects(painter, view, page, rects, theme.field);
                }
            }
        }
        if let Some(search) = &self.search {
            let from = search.matches.partition_point(|m| m.page < *shown.start());
            let matches = search.matches[from..].iter().enumerate().take_while(|(_, m)| m.page <= *shown.end());
            for (i, m) in matches {
                let color = if search.current == Some(from + i) { theme.search_current } else { theme.search };
                self.fill_rects(painter, view, m.page, &m.rects, color);
            }
        }
        for annotation in self.side.annotations.iter().filter(|a| shown.contains(&a.page())) {
            self.draw_annotation(painter, view, annotation);
        }
        self.draw_note_texts(painter, view);
        self.draw_pdf_note_texts(painter, view);
        if let Some((page, points)) = &self.stroke {
            let color = self.config.hotbar.colors[self.color.min(self.config.hotbar.colors.len() - 1)].rgb();
            self.draw_stroke(painter, view, *page, points, color, self.stroke_width);
        }
        for (page, range) in self.selection_parts() {
            if let Some(text) = self.doc.text[page].as_ref().filter(|_| shown.contains(&page)) {
                self.fill_rects(painter, view, page, &text.line_rects(range), theme.selection);
            }
        }
    }

    fn draw_annotation(&self, painter: &Painter, view: &View, annotation: &Annotation) {
        match annotation {
            Annotation::Highlight { page, rects, color, .. } => {
                let t = view.transform(&self.doc.layout, *page);
                for r in rects {
                    painter.rect_filled(t.rect(*r), 0.0, translucent(*color, 110));
                }
            }
            Annotation::Stroke { page, points, color, width, .. } => self.draw_stroke(painter, view, *page, points, *color, *width),
            Annotation::Note { page, at, color, .. } => self.draw_note(painter, view, *page, *at, *color),
            Annotation::Text { page, at, size, text, .. } => {
                let font = FontId::proportional(font_pixels(size * view.zoom));
                // Turned with the page, as the export writes it.
                let point = view.transform(&self.doc.layout, *page).point(*at);
                let galley = painter.layout_no_wrap(text.clone(), font, self.ink());
                let angle = f32::from(self.doc.layout.rotation).to_radians();
                painter.add(TextShape::new(point, galley, self.ink()).with_angle(angle));
            }
            Annotation::Mark { page, at, size, mark, .. } => self.draw_mark(painter, view, *page, *at, *size, *mark),
            Annotation::Signature { page, strokes, width, .. } => {
                let [r, g, b, _] = self.ink().to_array();
                for stroke in strokes {
                    self.draw_stroke(painter, view, *page, stroke, [r, g, b], *width);
                }
            }
        }
    }

    /// What is written on the page: dark on the paper of a page shown as it is, or the colour the
    /// theme gives text.
    fn ink(&self) -> Color32 {
        let theme = self.config.active_theme();
        if theme.recolor { theme.page_text.egui() } else { Color32::BLACK }
    }

    fn draw_note(&self, painter: &Painter, view: &View, page: usize, at: [f32; 2], color: [u8; 3]) {
        let t = view.transform(&self.doc.layout, page);
        let icon = t.rect([at[0], at[1], at[0] + NOTE_SIDE, at[1] + NOTE_SIDE]);
        painter.rect_filled(icon, 0.0, Color32::from_rgb(color[0], color[1], color[2]));
        let line = Stroke::new(1.5 * view.zoom, Color32::from_black_alpha(150));
        for row in [0.3, 0.5, 0.7] {
            let y = icon.top() + icon.height() * row;
            painter.line_segment([pos2(icon.left() + icon.width() * 0.2, y), pos2(icon.right() - icon.width() * 0.2, y)], line);
        }
    }

    /// A note's text shows while the pointer is on it, and all of them with the note tool in hand.
    fn draw_note_texts(&self, painter: &Painter, view: &View) {
        let shown = view.visible_pages(&self.doc.layout);
        for annotation in self.side.annotations.iter().filter(|a| shown.contains(&a.page())) {
            let Annotation::Note { page, at, text, .. } = annotation else { continue };
            let icon = view.transform(&self.doc.layout, *page).rect([at[0], at[1], at[0] + NOTE_SIDE, at[1] + NOTE_SIDE]);
            if self.tool == Some(Tool::Note) || self.hover.is_some_and(|p| icon.contains(p)) {
                self.draw_note_text(painter, view, icon, text);
            }
        }
    }

    /// The PDF's own notes show like the sticky notes: under the pointer (the topmost one only,
    /// and not while a tool is drawing), or once picked.
    fn draw_pdf_note_texts(&self, painter: &Painter, view: &View) {
        let shown = view.visible_pages(&self.doc.layout);
        let tooling = matches!(self.tool, Some(Tool::Draw | Tool::Erase | Tool::Note | Tool::Fill));
        let hovered = self
            .hover
            .filter(|_| !tooling)
            .and_then(|p| view.page_under(&self.doc.layout, p))
            .and_then(|(page, at)| crate::doc::note_at(&self.doc.notes, page, at));
        let picked = self.shown_note.filter(|i| Some(*i) != hovered);
        for index in [picked, hovered].into_iter().flatten() {
            let Some(note) = self.doc.notes.get(index).filter(|n| shown.contains(&n.page)) else { continue };
            let icon = view.transform(&self.doc.layout, note.page).rect(note.rect);
            self.draw_note_text(painter, view, icon, &note.text());
        }
    }

    fn draw_note_text(&self, painter: &Painter, view: &View, icon: Rect, text: &str) {
        let theme = self.config.active_theme();
        let galley = painter.layout(text.to_owned(), FontId::proportional(self.config.font_size), theme.ui_text.egui(), NOTE_WIDTH);
        let size = galley.size() + Vec2::splat(12.0);
        let beside = Rect::from_min_size(icon.right_top() + vec2(6.0, 0.0), size);
        let area = if beside.right() > view.rect.right() { beside.translate(vec2(-(size.x + icon.width() + 12.0), 0.0)) } else { beside };
        let area = clamp_into(area, view.rect);
        painter.rect_filled(area, 0.0, theme.ui_background.egui());
        painter.rect_stroke(area, 0.0, Stroke::new(self.config.view.border, theme.ui_accent.egui()), egui::StrokeKind::Inside);
        painter.galley(area.min + Vec2::splat(6.0), galley, theme.ui_text.egui());
    }

    fn draw_mark(&self, painter: &Painter, view: &View, page: usize, at: [f32; 2], size: f32, mark: MarkKind) {
        let t = view.transform(&self.doc.layout, page);
        let [x0, y0, x1, y1] = mark_rect(mark, at, size);
        let (w, h) = (x1 - x0, y1 - y0);
        if mark == MarkKind::Dot {
            painter.circle_filled(t.point(at), w.min(h) * 0.3 * view.zoom, self.ink());
        }
        let pen = Stroke::new(size * 0.1 * view.zoom, self.ink());
        // The strokes come in a box with y going up, as in a PDF.
        for stroke in mark.fill_mark().strokes([0.0, 0.0, 1.0, 1.0]) {
            let points = stroke.iter().map(|p| t.point([x0 + p[0] as f32 * w, y1 - p[1] as f32 * h])).collect();
            painter.add(Shape::line(points, pen));
        }
    }

    fn draw_stroke(&self, painter: &Painter, view: &View, page: usize, points: &[[f32; 2]], color: [u8; 3], width: f32) {
        let t = view.transform(&self.doc.layout, page);
        let line_width = width * view.zoom;
        let paint = Color32::from_rgb(color[0], color[1], color[2]);
        if let [point] = points {
            painter.circle_filled(t.point(*point), line_width / 2.0, paint);
            return;
        }
        painter.add(Shape::line(points.iter().map(|p| t.point(*p)).collect(), Stroke::new(line_width, paint)));
    }

    /// A small dot in the page margin at each portal's source.
    fn draw_portal_markers(&self, painter: &Painter, view: &View) {
        let shown = view.visible_pages(&self.doc.layout);
        let accent = self.config.active_theme().ui_accent.egui();
        for portal in self.side.portals.iter().filter(|p| shown.contains(&p.source.page)) {
            let t = view.transform(&self.doc.layout, portal.source.page);
            // A portal's `y` is how far down the page as shown, as an anchor's is.
            painter.circle_filled(t.origin + vec2(6.0, portal.source.y) * t.scale, 3.0, accent);
        }
    }

    fn draw_hints(&self, painter: &Painter, view: &View, state: &HintState) {
        let theme = self.config.active_theme();
        let font = FontId::monospace(self.config.hints.font_size);
        if let Some((page, first, last)) = state.start
            && let Some(text) = self.doc.text.get(page).and_then(|t| t.as_ref())
        {
            self.fill_rects(painter, view, page, &text.line_rects(first..last + 1), theme.selection);
        }
        let mut previous = None;
        for target in &state.targets {
            let Some(rest) = target.label.strip_prefix(state.typed.as_str()) else { continue };
            // The label stays upright at the corner of the target that is top left on screen.
            let at = view.transform(&self.doc.layout, target.page).rect(target.rect).min;
            let galley = painter.layout_no_wrap(rest.to_owned(), font.clone(), theme.hint_text.egui());
            let box_rect = clear_of(previous, Rect::from_min_size(at, galley.size() + vec2(2.0, 0.0)));
            painter.rect_filled(box_rect, 0.0, theme.hint_background.egui());
            painter.galley(box_rect.min + vec2(1.0, 0.0), galley, theme.hint_text.egui());
            previous = Some(box_rect);
        }
    }

    // The portal window.

    /// Shows the target of the portal nearest the view in a window of its own.
    pub fn portal_viewport(&mut self, ctx: &egui::Context) {
        if !self.portal_window {
            return;
        }
        let Some(portal) = self.closest_portal().cloned() else { return };
        let builder = ViewportBuilder::default()
            .with_title("yavcpdf portal")
            .with_app_id("yavcpdf-portal")
            .with_inner_size([self.config.portal.width, self.config.portal.height])
            .with_decorations(self.config.view.decorations);
        ctx.show_viewport_immediate(ViewportId::from_hash_of("portal"), builder, |ui, _| self.portal_ui(ui, &portal));
    }

    fn portal_ui(&mut self, ui: &mut egui::Ui, portal: &Portal) {
        let ctx = ui.ctx().clone();
        if ctx.input(|i| i.viewport().close_requested()) {
            self.portal_window = false;
            return;
        }
        let area = ctx.content_rect();
        ui.painter().rect_filled(area, 0.0, self.config.active_theme().background.egui());
        let layout = &self.doc.layout;
        let page = portal.target.page.min(layout.len() - 1);
        let zoom = area.width() / layout.sizes[page][0];
        let mut view = View { x: 0.0, y: layout.tops[page] + portal.target.y, zoom, rect: area, zoom_hold: None };
        view.clamp(layout);
        view.x = layout.lefts[page] + layout.sizes[page][0] / 2.0;
        let painter = ui.painter().with_clip_rect(area);
        let ppp = ctx.pixels_per_point();
        for shown in view.visible_pages(&self.doc.layout) {
            self.draw_page(&painter, &view, shown, ppp, zoom);
        }
        self.draw_marks(&painter, &view);
        painter.text(
            area.right_bottom() - vec2(6.0, 4.0),
            Align2::RIGHT_BOTTOM,
            format!("{}", page + 1),
            FontId::monospace(11.0),
            self.config.active_theme().ui_accent.egui(),
        );
        // It often has no title bar and the same colours as the window behind it.
        painter.rect_stroke(area, 0.0, Stroke::new(self.config.view.border, self.config.active_theme().ui_accent.egui()), egui::StrokeKind::Inside);
        // Keys typed here work as in the main window; the wheel moves this portal's target.
        let events = ctx.input(|i| i.events.clone());
        self.keys(&ctx, &events);
        self.portal_wheel(&events, portal.id, zoom);
        if !events.is_empty() {
            ctx.request_repaint_of(ViewportId::ROOT);
        }
    }

    fn portal_wheel(&mut self, events: &[egui::Event], id: u64, zoom: f32) {
        let step = self.config.view.scroll_step;
        let delta: f32 = events
            .iter()
            .filter_map(|e| match e {
                egui::Event::MouseWheel { unit: egui::MouseWheelUnit::Point, delta, .. } => Some(delta.y),
                egui::Event::MouseWheel { delta, .. } => Some(delta.y * step),
                _ => None,
            })
            .sum();
        if delta == 0.0 {
            return;
        }
        if let Some(portal) = self.side.portals.iter_mut().find(|p| p.id == id) {
            portal.target.y -= delta / zoom;
            self.mark_dirty();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{clamp_into, clear_of};
    use egui::{Rect, pos2, vec2};

    #[test]
    fn a_popup_is_kept_inside_the_view() {
        let view = Rect::from_min_size(pos2(0.0, 0.0), vec2(100.0, 100.0));
        let popup = |x, y| Rect::from_min_size(pos2(x, y), vec2(30.0, 20.0));
        assert_eq!(clamp_into(popup(10.0, 10.0), view), popup(10.0, 10.0));
        assert_eq!(clamp_into(popup(90.0, 95.0), view), popup(70.0, 80.0));
        assert_eq!(clamp_into(popup(-20.0, -5.0), view), popup(0.0, 0.0));
    }

    fn label(x: f32, y: f32) -> Rect {
        Rect::from_min_size(pos2(x, y), vec2(14.0, 10.0))
    }

    #[test]
    fn text_sizes_are_few_and_bounded() {
        assert_eq!(super::font_pixels(13.4), 13.0);
        assert_eq!(super::font_pixels(0.2), 4.0);
        assert_eq!(super::font_pixels(101.0), 104.0);
        assert_eq!(super::font_pixels(9000.0), 512.0);
    }

    #[test]
    fn a_label_moves_out_of_the_one_before_it() {
        let moved = clear_of(Some(label(100.0, 50.0)), label(105.0, 50.0));
        assert_eq!(moved.left(), 115.0);
        assert_eq!(moved.width(), 14.0);
    }

    #[test]
    fn labels_with_room_stay_where_they_are() {
        assert_eq!(clear_of(None, label(5.0, 5.0)), label(5.0, 5.0));
        assert_eq!(clear_of(Some(label(0.0, 50.0)), label(40.0, 50.0)), label(40.0, 50.0));
    }

    #[test]
    fn the_next_line_starts_again_at_its_word() {
        assert_eq!(clear_of(Some(label(300.0, 50.0)), label(20.0, 70.0)), label(20.0, 70.0));
    }
    #[test]
    fn a_tile_lands_where_the_turned_page_puts_it() {
        // A raster of 1000 x 2000 pixels with a tile of 100 x 50 at (10, 20), at 0.5 points a pixel.
        let size = [1000.0, 2000.0];
        let tile = [10.0, 20.0, 100.0, 50.0];
        let origin = pos2(5.0, 7.0);
        let at = |rotation| {
            let r = super::pixel_rect(rotation, size, origin, 0.5, tile);
            [r.min.x, r.min.y, r.max.x, r.max.y]
        };
        assert_eq!(at(0), [10.0, 17.0, 60.0, 42.0]);
        assert_eq!(at(90), [5.0 + 0.5 * (2000.0 - 70.0), 7.0 + 5.0, 5.0 + 0.5 * (2000.0 - 20.0), 7.0 + 55.0]);
        assert_eq!(at(180), [5.0 + 0.5 * 890.0, 7.0 + 0.5 * 1930.0, 5.0 + 0.5 * 990.0, 7.0 + 0.5 * 1980.0]);
        assert_eq!(at(270), [5.0 + 0.5 * 20.0, 7.0 + 0.5 * 890.0, 5.0 + 0.5 * 70.0, 7.0 + 0.5 * 990.0]);
    }

    #[test]
    fn prop_tiles_cover_the_turned_page_without_gaps_or_overlaps() {
        for rotation in [0u16, 90, 180, 270] {
            for (pw, ph) in [(1000u32, 1700u32), (333, 77), (1, 50), (512, 512), (1025, 513)] {
                for side in [64u32, 100, 512, 1024] {
                    let pixels = [pw as f32, ph as f32];
                    let origin = pos2(13.0, -7.0);
                    let unit = 1.0;
                    let (tw, th) = if rotation % 180 == 0 { (pw, ph) } else { (ph, pw) };
                    // Count coverage on the shown pixel grid.
                    let mut hits = vec![0u8; (tw * th) as usize];
                    for y in (0..ph).step_by(side as usize) {
                        for x in (0..pw).step_by(side as usize) {
                            let (w, h) = (side.min(pw - x), side.min(ph - y));
                            let r = super::pixel_rect(rotation, pixels, origin, unit, [x as f32, y as f32, w as f32, h as f32]);
                            assert!(r.min.x >= origin.x - 1e-3 && r.max.x <= origin.x + tw as f32 + 1e-3, "{rotation} out of bounds");
                            assert!(r.min.y >= origin.y - 1e-3 && r.max.y <= origin.y + th as f32 + 1e-3, "{rotation} out of bounds");
                            let (x0, y0) = ((r.min.x - origin.x).round() as u32, (r.min.y - origin.y).round() as u32);
                            let (x1, y1) = ((r.max.x - origin.x).round() as u32, (r.max.y - origin.y).round() as u32);
                            assert_eq!((x1 - x0, y1 - y0), if rotation % 180 == 0 { (w, h) } else { (h, w) });
                            for yy in y0..y1 {
                                for xx in x0..x1 {
                                    hits[(yy * tw + xx) as usize] += 1;
                                }
                            }
                        }
                    }
                    assert!(hits.iter().all(|&n| n == 1), "{rotation} {pw}x{ph} side {side}: gaps or overlaps");
                }
            }
        }
    }
}
