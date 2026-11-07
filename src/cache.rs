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
        let Some(tiles) = self.pages.get_mut(&page) else { return Vec::new() };
        let others = || tiles.keys().filter(|k| k.scale != wanted);
        let nearest = |a: &&TileKey, b: &&TileKey| a.distance(scale).total_cmp(&b.distance(scale));
        let whole = others().filter(|k| k.is_whole()).min_by(nearest).copied();
        let tiled = others().filter(|k| !k.is_whole()).min_by(nearest).map(|k| k.scale);
        let mut keys: Vec<TileKey> = whole.into_iter().collect();
        keys.extend(tiles.keys().filter(|k| !k.is_whole() && Some(k.scale) == tiled));
        let mut out = Vec::with_capacity(keys.len());
        for key in keys {
            if let Some(tile) = tiles.get_mut(&key) {
                tile.frame = frame;
                out.push((key, tile.texture.id(), tile.width, tile.height));
            }
        }
        out
    }

    /// Drops the least recently used tiles above `budget` bytes, never one used this frame.
    pub fn evict(&mut self, budget: usize) {
        if self.bytes <= budget {
            return;
        }
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
}
