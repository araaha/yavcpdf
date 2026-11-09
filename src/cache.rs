//! Rendered pages and tiles as textures, dropped least recently used first.
use std::collections::{BTreeMap, HashMap, HashSet};

use egui::TextureHandle;
use pdfcraft_render::Tile;

/// The `x` and `y` of a whole page's key: no tile starts there.
const WHOLE: u32 = u32::MAX;

/// A whole page or a tile of one, at one render scale.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct TileKey {
    pub page: usize,
    pub scale: u32,
    pub x: u32,
    pub y: u32,
}

impl TileKey {
    pub fn new(page: usize, scale: f32, tile: Option<Tile>) -> Self {
        let (x, y) = tile.map_or((WHOLE, WHOLE), |t| (t.x, t.y));
        Self { page, scale: scale.to_bits(), x, y }
    }

    fn is_whole(&self) -> bool {
        self.x == WHOLE
    }

    /// Where it starts on the page, in its own pixels.
    pub fn offset(&self) -> (u32, u32) {
        if self.is_whole() { (0, 0) } else { (self.x, self.y) }
    }

    /// How far its scale is from `scale`, as a ratio either way.
    fn distance(&self, scale: f32) -> f32 {
        (f32::from_bits(self.scale) / scale).ln().abs()
    }
}

pub struct CachedTile {
    pub texture: TextureHandle,
    pub width: u32,
    pub height: u32,
    frame: u64,
}

#[derive(Default)]
pub struct TileCache {
    pages: BTreeMap<usize, HashMap<TileKey, CachedTile>>,
    /// Tiles that couldn't be rendered, so they aren't asked for again.
    failed: HashSet<TileKey>,
    /// What was showing when the pages changed under it (a form was filled in), until the new
    /// pages are in. Drawn as stand-ins, and never found as current.
    retired: BTreeMap<usize, Vec<(TileKey, CachedTile)>>,
    frame: u64,
    bytes: usize,
}

fn cost(width: u32, height: u32) -> usize {
    width as usize * height as usize * 4
}

impl TileCache {
    pub fn clear(&mut self) {
        *self = Self::default();
    }

    /// Keeps what is here as stand-ins for the pages that will replace it.
    pub fn retire(&mut self) {
        for (page, tiles) in std::mem::take(&mut self.pages) {
            let old = self.retired.entry(page).or_default();
            for (key, tile) in tiles {
                // Retired twice before the new pages came in: the newer one stands in.
                if let Some(at) = old.iter().position(|(k, _)| *k == key) {
                    let (_, gone) = old.remove(at);
                    self.bytes -= cost(gone.width, gone.height);
                }
                old.push((key, tile));
            }
        }
        self.failed.clear();
    }

    /// Lets go of the stand-ins for a page whose new raster is all here.
    pub fn settle(&mut self, page: usize) {
        for (_, tile) in self.retired.remove(&page).unwrap_or_default() {
            self.bytes -= cost(tile.width, tile.height);
        }
    }

    pub fn next_frame(&mut self) {
        self.frame += 1;
    }

    pub fn insert(&mut self, key: TileKey, texture: TextureHandle, width: u32, height: u32) {
        self.bytes += cost(width, height);
        let old = self.pages.entry(key.page).or_default().insert(key, CachedTile { texture, width, height, frame: self.frame });
        if let Some(old) = old {
            self.bytes -= cost(old.width, old.height);
        }
    }

    pub fn mark_failed(&mut self, key: TileKey) {
        self.failed.insert(key);
    }

    /// Whether the tile needs no rendering: it is here, or it can't be rendered.
    pub fn settled(&self, key: &TileKey) -> bool {
        self.pages.get(&key.page).is_some_and(|tiles| tiles.contains_key(key)) || self.failed.contains(key)
    }

    /// Whether anything of the page is here, at any scale.
    pub fn has_page(&self, page: usize) -> bool {
        self.pages.contains_key(&page)
    }

    /// Whether the page has pixels to draw: a current tile or a retired stand-in.
    pub fn shows_page(&self, page: usize) -> bool {
        self.has_page(page) || self.retired.get(&page).is_some_and(|tiles| !tiles.is_empty())
    }

    /// Looks up a tile for drawing, marking it as used.
    pub fn touch(&mut self, key: &TileKey) -> Option<&CachedTile> {
        let frame = self.frame;
        let tile = self.pages.get_mut(&key.page)?.get_mut(key)?;
        tile.frame = frame;
        Some(tile)
    }

    /// What can stand in for the page at `scale` until the sharp raster arrives, in drawing order:
    /// the whole raster nearest that scale, then the tiles of the nearest tiled scale. Only those
    /// are kept alive and drawn, so a page zoomed through many scales costs no more than one.
    pub fn stale(&mut self, page: usize, scale: f32) -> Vec<(TileKey, egui::TextureId, u32, u32)> {
        let frame = self.frame;
        let wanted = scale.to_bits();
        let mut out = self.retired_for(page, scale);
        for (key, tile) in self.retired.get_mut(&page).into_iter().flatten() {
            if out.iter().any(|(k, ..)| k == key) {
                tile.frame = frame;
            }
        }
        let Some(tiles) = self.pages.get_mut(&page) else { return out };
        let others = || tiles.keys().filter(|k| k.scale != wanted);
        let nearest = |a: &&TileKey, b: &&TileKey| a.distance(scale).total_cmp(&b.distance(scale));
        let whole = others().filter(|k| k.is_whole()).min_by(nearest).copied();
        let tiled = others().filter(|k| !k.is_whole()).min_by(nearest).map(|k| k.scale);
        let mut keys: Vec<TileKey> = whole.into_iter().collect();
        keys.extend(tiles.keys().filter(|k| !k.is_whole() && Some(k.scale) == tiled));
        for key in keys {
            if let Some(tile) = tiles.get_mut(&key) {
                tile.frame = frame;
                out.push((key, tile.texture.id(), tile.width, tile.height));
            }
        }
        out
    }

    /// The retired raster nearest the scale, and the retired tiles of the nearest tiled scale.
    fn retired_for(&self, page: usize, scale: f32) -> Vec<(TileKey, egui::TextureId, u32, u32)> {
        let Some(old) = self.retired.get(&page) else { return Vec::new() };
        let nearest = |a: &&TileKey, b: &&TileKey| a.distance(scale).total_cmp(&b.distance(scale));
        let whole = old.iter().map(|(k, _)| k).filter(|k| k.is_whole()).min_by(nearest).copied();
        let tiled = old.iter().map(|(k, _)| k).filter(|k| !k.is_whole()).min_by(nearest).map(|k| k.scale);
        let wanted = |(k, _): &&(TileKey, CachedTile)| (k.is_whole() && Some(*k) == whole) || (!k.is_whole() && Some(k.scale) == tiled);
        old.iter().filter(wanted).map(|(k, t)| (*k, t.texture.id(), t.width, t.height)).collect()
    }

    /// Drops the least recently used tiles above `budget` bytes, never one used this frame.
    pub fn evict(&mut self, budget: usize) {
        if self.bytes <= budget {
            return;
        }
        // Stand-ins go first: they are only waiting for what replaces them. One drawn this frame
        // stays, as a tile does.
        let frame = self.frame;
        let mut freed = 0;
        for tiles in self.retired.values_mut() {
            tiles.retain(|(_, t)| {
                t.frame == frame || {
                    freed += cost(t.width, t.height);
                    false
                }
            });
        }
        self.retired.retain(|_, tiles| !tiles.is_empty());
        self.bytes -= freed;
        let all = self.pages.values().flat_map(HashMap::iter);
        let mut oldest: Vec<(u64, TileKey)> = all.filter(|(_, t)| t.frame != self.frame).map(|(k, t)| (t.frame, *k)).collect();
        oldest.sort_unstable_by_key(|(frame, _)| *frame);
        for (_, key) in oldest {
            if self.bytes <= budget {
                break;
            }
            if let Some(tile) = self.pages.get_mut(&key.page).and_then(|tiles| tiles.remove(&key)) {
                self.bytes -= cost(tile.width, tile.height);
            }
        }
        self.pages.retain(|_, tiles| !tiles.is_empty());
    }

    #[cfg(test)]
    fn len(&self) -> usize {
        self.pages.values().map(HashMap::len).sum()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(page: usize, scale: f32) -> TileKey {
        TileKey::new(page, scale, None)
    }

    fn tile(page: usize, scale: f32, x: u32) -> TileKey {
        TileKey::new(page, scale, Some(Tile { x, y: 0, w: 1, h: 1 }))
    }

    fn scales(stale: &[(TileKey, egui::TextureId, u32, u32)]) -> Vec<f32> {
        stale.iter().map(|s| f32::from_bits(s.0.scale)).collect()
    }

    fn texture(ctx: &egui::Context) -> TextureHandle {
        ctx.load_texture("t", egui::ColorImage::filled([1, 1], egui::Color32::WHITE), egui::TextureOptions::LINEAR)
    }

    #[test]
    fn eviction_drops_the_oldest_and_spares_this_frame() {
        let ctx = egui::Context::default();
        let mut cache = TileCache::default();
        cache.insert(key(0, 1.0), texture(&ctx), 10, 10);
        cache.next_frame();
        cache.insert(key(1, 1.0), texture(&ctx), 10, 10);
        cache.next_frame();
        cache.insert(key(2, 1.0), texture(&ctx), 10, 10);
        cache.touch(&key(0, 1.0));
        cache.evict(cost(10, 10) * 2);
        assert_eq!(cache.len(), 2);
        assert!(cache.settled(&key(0, 1.0)) && !cache.settled(&key(1, 1.0)) && cache.settled(&key(2, 1.0)));
        cache.evict(0);
        assert_eq!(cache.len(), 2, "both were used this frame");
    }

    #[test]
    fn replacing_a_tile_does_not_count_it_twice() {
        let ctx = egui::Context::default();
        let mut cache = TileCache::default();
        cache.insert(key(0, 1.0), texture(&ctx), 10, 10);
        cache.insert(key(0, 1.0), texture(&ctx), 10, 10);
        assert_eq!(cache.bytes, cost(10, 10));
    }

    #[test]
    fn one_whole_raster_stands_in_the_nearest_to_the_scale() {
        let ctx = egui::Context::default();
        let mut cache = TileCache::default();
        for scale in [4.0, 1.0, 2.5] {
            cache.insert(key(0, scale), texture(&ctx), 1, 1);
        }
        cache.insert(key(1, 2.0), texture(&ctx), 1, 1);
        assert_eq!(scales(&cache.stale(0, 2.0)), [2.5]);
        assert!(cache.stale(2, 2.0).is_empty());
    }

    #[test]
    fn the_exact_scale_never_stands_in_for_itself() {
        let ctx = egui::Context::default();
        let mut cache = TileCache::default();
        cache.insert(key(0, 2.0), texture(&ctx), 1, 1);
        assert!(cache.stale(0, 2.0).is_empty());
    }

    #[test]
    fn the_tiles_of_one_scale_go_over_the_whole_raster() {
        let ctx = egui::Context::default();
        let mut cache = TileCache::default();
        cache.insert(key(0, 1.0), texture(&ctx), 1, 1);
        cache.insert(tile(0, 3.0, 0), texture(&ctx), 1, 1);
        cache.insert(tile(0, 3.0, 1024), texture(&ctx), 1, 1);
        cache.insert(tile(0, 9.0, 0), texture(&ctx), 1, 1);
        assert_eq!(scales(&cache.stale(0, 4.0)), [1.0, 3.0, 3.0]);
    }

    #[test]
    fn stand_ins_left_undrawn_can_be_evicted() {
        let ctx = egui::Context::default();
        let mut cache = TileCache::default();
        cache.insert(key(0, 1.0), texture(&ctx), 10, 10);
        cache.insert(key(0, 4.0), texture(&ctx), 10, 10);
        cache.next_frame();
        cache.stale(0, 3.0);
        cache.evict(cost(10, 10));
        assert!(cache.settled(&key(0, 4.0)) && !cache.settled(&key(0, 1.0)));
    }

    #[test]
    fn a_whole_page_and_its_first_tile_are_different_keys() {
        assert_ne!(key(0, 1.0), tile(0, 1.0, 0));
        assert_eq!(key(0, 1.0).offset(), (0, 0));
        assert_eq!(tile(0, 1.0, 1024).offset(), (1024, 0));
    }

    #[test]
    fn a_failed_tile_is_not_asked_for_again() {
        let mut cache = TileCache::default();
        cache.mark_failed(key(0, 1.0));
        assert!(cache.settled(&key(0, 1.0)));
        assert!(cache.touch(&key(0, 1.0)).is_none());
    }

    #[test]
    fn a_page_shows_pixels_only_with_a_tile_or_a_stand_in() {
        let ctx = egui::Context::default();
        let mut cache = TileCache::default();
        assert!(!cache.shows_page(0));
        cache.insert(key(0, 1.0), texture(&ctx), 10, 10);
        assert!(cache.shows_page(0) && !cache.shows_page(1));
    }

    #[test]
    fn retired_tiles_stand_in_until_the_page_settles_and_are_never_current() {
        let ctx = egui::Context::default();
        let mut cache = TileCache::default();
        cache.insert(key(0, 2.0), texture(&ctx), 10, 10);
        cache.retire();
        assert!(!cache.settled(&key(0, 2.0)) && !cache.has_page(0) && cache.touch(&key(0, 2.0)).is_none());
        assert!(cache.shows_page(0), "a stand-in counts as pixels");
        assert_eq!(scales(&cache.stale(0, 2.0)), [2.0], "even at the very scale, as the page has changed");
        cache.insert(key(0, 2.0), texture(&ctx), 10, 10);
        cache.settle(0);
        assert!(cache.stale(0, 2.0).is_empty());
        cache.retire();
        cache.next_frame();
        cache.evict(0);
        assert!(cache.stale(0, 2.0).is_empty(), "eviction lets go of them first");
    }

    #[test]
    fn a_stand_in_drawn_this_frame_is_not_evicted() {
        let ctx = egui::Context::default();
        let mut cache = TileCache::default();
        cache.insert(key(0, 2.0), texture(&ctx), 10, 10);
        cache.insert(key(1, 2.0), texture(&ctx), 10, 10);
        cache.retire();
        cache.next_frame();
        assert_eq!(cache.stale(0, 2.0).len(), 1);
        cache.evict(0);
        assert_eq!(cache.stale(0, 2.0).len(), 1, "drawn this frame");
        assert!(!cache.retired.contains_key(&1), "the one left undrawn went");
        assert_eq!(cache.bytes, cost(10, 10));
    }

    #[test]
    fn retiring_twice_keeps_one_stand_in_per_key() {
        let ctx = egui::Context::default();
        let mut cache = TileCache::default();
        cache.insert(key(0, 2.0), texture(&ctx), 10, 10);
        cache.retire();
        cache.insert(key(0, 2.0), texture(&ctx), 10, 10);
        cache.retire();
        assert_eq!(cache.retired[&0].len(), 1);
        assert_eq!(cache.bytes, cost(10, 10));
        cache.settle(0);
        assert_eq!(cache.bytes, 0);
    }

    #[test]
    fn evict_zero_keeps_a_stand_in_drawn_this_frame_and_drops_one_that_was_not() {
        let ctx = egui::Context::default();
        let mut cache = TileCache::default();
        cache.insert(key(0, 2.0), texture(&ctx), 10, 10);
        cache.insert(key(1, 2.0), texture(&ctx), 20, 10);
        cache.insert(tile(2, 2.0, 0), texture(&ctx), 5, 5);
        cache.retire();
        cache.next_frame();
        assert_eq!(cache.stale(0, 2.0).len(), 1);
        assert_eq!(cache.stale(2, 2.0).len(), 1);
        cache.evict(0);
        assert!(cache.retired.contains_key(&0) && cache.retired.contains_key(&2));
        assert!(!cache.retired.contains_key(&1));
        assert_eq!(cache.bytes, cost(10, 10) + cost(5, 5));
        // Next frame nothing is drawn: all go.
        cache.next_frame();
        cache.evict(0);
        assert!(cache.retired.is_empty());
        assert_eq!(cache.bytes, 0);
    }

    #[test]
    fn retiring_twice_counts_bytes_right_with_different_sizes_and_keys() {
        let ctx = egui::Context::default();
        let mut cache = TileCache::default();
        cache.insert(key(0, 2.0), texture(&ctx), 10, 10);
        cache.insert(key(0, 3.0), texture(&ctx), 7, 7);
        cache.retire();
        cache.insert(key(0, 2.0), texture(&ctx), 20, 20);
        cache.retire();
        assert_eq!(cache.retired[&0].len(), 2);
        assert_eq!(cache.bytes, cost(20, 20) + cost(7, 7));
        // A third time with nothing current changes nothing.
        cache.retire();
        assert_eq!(cache.retired[&0].len(), 2);
        assert_eq!(cache.bytes, cost(20, 20) + cost(7, 7));
        // Current and retired of one key at once are both counted.
        cache.insert(key(0, 2.0), texture(&ctx), 4, 4);
        assert_eq!(cache.bytes, cost(20, 20) + cost(7, 7) + cost(4, 4));
        cache.settle(0);
        assert_eq!(cache.bytes, cost(4, 4));
        cache.evict(0);
        assert_eq!(cache.bytes, cost(4, 4), "one used this frame (frame 0) stays");
        cache.next_frame();
        cache.evict(0);
        assert_eq!(cache.bytes, 0);
    }

    #[test]
    fn shows_page_covers_every_state() {
        let ctx = egui::Context::default();
        let mut cache = TileCache::default();
        assert!(!cache.shows_page(0), "no tiles");
        cache.insert(tile(0, 1.0, 0), texture(&ctx), 10, 10);
        assert!(cache.shows_page(0), "a tiled current tile");
        cache.retire();
        assert!(cache.shows_page(0) && !cache.has_page(0), "a retired stand-in only");
        assert!(!cache.shows_page(1));
        cache.settle(0);
        assert!(!cache.shows_page(0), "settled with nothing current: retired list gone");
        // A retired entry left empty is not pixels (as evict's retain would leave none, force one).
        cache.retired.insert(5, Vec::new());
        assert!(!cache.shows_page(5), "an empty retired list");
        // Eviction removing the last current tile.
        cache.insert(key(2, 1.0), texture(&ctx), 10, 10);
        cache.next_frame();
        cache.evict(0);
        assert!(!cache.shows_page(2) && !cache.has_page(2));
        // Eviction removing the last retired stand-in.
        cache.insert(key(3, 1.0), texture(&ctx), 10, 10);
        cache.retire();
        cache.next_frame();
        assert!(cache.shows_page(3));
        cache.evict(0);
        assert!(!cache.shows_page(3));
        // A stand-in plus a new current tile, then settle: still shown by the current one.
        cache.insert(key(4, 1.0), texture(&ctx), 10, 10);
        cache.retire();
        cache.insert(key(4, 2.0), texture(&ctx), 10, 10);
        cache.settle(4);
        assert!(cache.shows_page(4));
    }
}
