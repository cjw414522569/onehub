//! Cross-platform glyph atlas with cache eviction and DPI bucketing (T076).
//!
//! [`GlyphAtlas`] is a bounded cache of rasterized glyphs (text + font +
//! color -> texture placement) with LRU eviction so cache memory stays under
//! an explicit budget. [`AtlasSet`] keeps a separate atlas per DPI bucket, so
//! zooming / DPI hot-switching never renders a wrong texture (each bucket has
//! its own atlas), and [`GlyphAtlas::clear`] handles device loss. DPI
//! hot-switch and device-loss behavior is covered by unit tests; real GPU
//! texture upload is the renderer's job.

use std::collections::{HashMap, VecDeque};

/// Identifies a glyph to rasterize.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct GlyphKey {
    /// The glyph text (a grapheme cluster).
    pub text: String,
    /// Font family.
    pub family: String,
    /// Point size.
    pub size_pt: u32,
    /// Packed RGBA color.
    pub color: u32,
}

impl GlyphKey {
    /// A new glyph key.
    pub fn new(
        text: impl Into<String>,
        family: impl Into<String>,
        size_pt: u32,
        color: u32,
    ) -> Self {
        Self {
            text: text.into(),
            family: family.into(),
            size_pt,
            color,
        }
    }
}

/// A glyph's placement in an atlas texture.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AtlasEntry {
    /// Atlas page index.
    pub page: u32,
    /// X offset within the page.
    pub x: u16,
    /// Y offset within the page.
    pub y: u16,
    /// Glyph width in texels.
    pub width: u16,
    /// Glyph height in texels.
    pub height: u16,
    /// Estimated memory cost in bytes (RGBA).
    pub bytes: usize,
}

/// Atlas cache limits (explicit memory bound).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AtlasLimits {
    /// Maximum number of cached glyphs.
    pub max_entries: usize,
    /// Maximum total cache bytes.
    pub max_bytes: usize,
}

impl Default for AtlasLimits {
    fn default() -> Self {
        Self {
            max_entries: 4096,
            max_bytes: 64 * 1024 * 1024,
        }
    }
}

/// A bounded glyph atlas with LRU eviction.
#[derive(Debug, Clone)]
pub struct GlyphAtlas {
    limits: AtlasLimits,
    entries: HashMap<GlyphKey, AtlasEntry>,
    /// LRU order; the oldest entry is at the front.
    lru: VecDeque<GlyphKey>,
    next_page: u32,
    next_slot: u32,
    bytes: usize,
}

impl GlyphAtlas {
    /// An empty atlas with the given limits.
    pub fn new(limits: AtlasLimits) -> Self {
        Self {
            limits,
            entries: HashMap::new(),
            lru: VecDeque::new(),
            next_page: 0,
            next_slot: 0,
            bytes: 0,
        }
    }

    /// Looks up a glyph and promotes it to most-recently-used (true LRU).
    pub fn get(&mut self, key: &GlyphKey) -> Option<AtlasEntry> {
        let entry = self.entries.get(key).copied()?;
        self.promote(key);
        Some(entry)
    }

    /// Whether a glyph is cached.
    pub fn contains(&self, key: &GlyphKey) -> bool {
        self.entries.contains_key(key)
    }

    /// Inserts (or returns) a glyph placement, evicting least-recently-used
    /// entries until the cache is back under its explicit limits. The entry
    /// is promoted to most-recently-used.
    pub fn insert(&mut self, key: GlyphKey, width: u16, height: u16) -> AtlasEntry {
        if let Some(existing) = self.entries.get(&key) {
            let existing = *existing;
            self.promote(&key);
            return existing;
        }
        let bytes = (width as usize) * (height as usize) * 4;
        let entry = AtlasEntry {
            page: self.next_page,
            x: (self.next_slot & 0xffff) as u16,
            y: 0,
            width,
            height,
            bytes,
        };
        self.next_slot = self.next_slot.wrapping_add(1);
        if self.next_slot.is_multiple_of(4096) {
            self.next_page += 1;
        }
        self.entries.insert(key.clone(), entry);
        self.lru.push_back(key.clone());
        self.bytes += entry.bytes;
        self.evict_until_within_limits();
        *self.entries.get(&key).expect("just inserted")
    }

    /// Evicts least-recently-used entries while over the limits.
    fn evict_until_within_limits(&mut self) {
        while (self.entries.len() > self.limits.max_entries || self.bytes > self.limits.max_bytes)
            && !self.lru.is_empty()
        {
            let oldest = self.lru.pop_front().expect("lru non-empty while evicting");
            if let Some(entry) = self.entries.remove(&oldest) {
                self.bytes = self.bytes.saturating_sub(entry.bytes);
            }
        }
    }

    /// Promotes a key to most-recently-used.
    fn promote(&mut self, key: &GlyphKey) {
        if let Some(position) = self.lru.iter().position(|k| k == key) {
            self.lru.remove(position);
        }
        self.lru.push_back(key.clone());
    }

    /// Clears the atlas (device loss): all textures are invalidated.
    pub fn clear(&mut self) {
        self.entries.clear();
        self.lru.clear();
        self.bytes = 0;
        self.next_page = 0;
        self.next_slot = 0;
    }

    /// Number of cached glyphs.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Whether the atlas is empty.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Total cached bytes (memory bound evidence).
    pub fn bytes(&self) -> usize {
        self.bytes
    }

    /// The configured limits.
    pub fn limits(&self) -> AtlasLimits {
        self.limits
    }
}

/// Quantizes a DPI scale into a bucket (scale * 100, rounded).
pub fn dpi_bucket(scale: f32) -> u32 {
    (scale * 100.0).round() as u32
}

/// One atlas per DPI bucket, so zoom / DPI hot-switching never renders a
/// wrong texture.
#[derive(Debug, Clone, Default)]
pub struct AtlasSet {
    atlases: HashMap<u32, GlyphAtlas>,
}

impl AtlasSet {
    /// An empty atlas set.
    pub fn new() -> Self {
        Self::default()
    }

    /// The atlas for a scale, if it has been created.
    pub fn for_bucket(&self, scale: f32) -> Option<&GlyphAtlas> {
        self.atlases.get(&dpi_bucket(scale))
    }

    /// Gets or creates the atlas for a scale.
    pub fn get_or_create(&mut self, scale: f32, limits: AtlasLimits) -> &mut GlyphAtlas {
        self.atlases
            .entry(dpi_bucket(scale))
            .or_insert_with(|| GlyphAtlas::new(limits))
    }

    /// Clears every bucket atlas (device loss).
    pub fn clear_all(&mut self) {
        for atlas in self.atlases.values_mut() {
            atlas.clear();
        }
    }

    /// Number of buckets.
    pub fn buckets(&self) -> usize {
        self.atlases.len()
    }
}

#[cfg(test)]
mod tests {
    use super::{dpi_bucket, AtlasLimits, AtlasSet, GlyphAtlas, GlyphKey};

    fn key(text: &str) -> GlyphKey {
        GlyphKey::new(text, "Cascadia Mono", 12, 0xffff_ffff)
    }

    #[test]
    fn insert_get_round_trip_and_dedup() {
        let mut atlas = GlyphAtlas::new(AtlasLimits::default());
        let first = atlas.insert(key("a"), 8, 16);
        let second = atlas.insert(key("a"), 8, 16);
        assert_eq!(first, second, "duplicate insert returns the same placement");
        assert_eq!(atlas.len(), 1);
        assert!(atlas.contains(&key("a")));
        assert_eq!(atlas.get(&key("a")).unwrap().bytes, 8 * 16 * 4);
    }

    #[test]
    fn lru_eviction_bounds_memory() {
        let limits = AtlasLimits {
            max_entries: 3,
            max_bytes: 1_000_000,
        };
        let mut atlas = GlyphAtlas::new(limits);
        atlas.insert(key("a"), 8, 16);
        atlas.insert(key("b"), 8, 16);
        atlas.insert(key("c"), 8, 16);
        assert_eq!(atlas.len(), 3);
        // Touching "a" makes it most-recent; inserting "d" evicts "b".
        let _ = atlas.get(&key("a"));
        atlas.insert(key("d"), 8, 16);
        assert_eq!(atlas.len(), 3);
        assert!(!atlas.contains(&key("b")), "oldest (b) must be evicted");
        assert!(atlas.contains(&key("a")));
        assert!(atlas.contains(&key("d")));
        assert!(atlas.bytes() <= limits.max_bytes);
    }

    #[test]
    fn byte_budget_is_enforced() {
        let limits = AtlasLimits {
            max_entries: 100,
            max_bytes: 8 * 16 * 4 * 2, // room for ~2 glyphs
        };
        let mut atlas = GlyphAtlas::new(limits);
        for i in 0..10 {
            atlas.insert(key(&format!("g{i}")), 8, 16);
        }
        assert!(
            atlas.bytes() <= limits.max_bytes,
            "bytes {} > {}",
            atlas.bytes(),
            limits.max_bytes
        );
        assert!(atlas.len() <= 2);
    }

    #[test]
    fn dpi_bucketing_and_hot_switch() {
        assert_eq!(dpi_bucket(1.0), 100);
        assert_eq!(dpi_bucket(1.25), 125);
        assert_eq!(dpi_bucket(1.5), 150);
        assert_eq!(dpi_bucket(2.0), 200);

        let mut set = AtlasSet::new();
        {
            let atlas = set.get_or_create(1.0, AtlasLimits::default());
            atlas.insert(key("中"), 16, 16);
        }
        // A different bucket has no atlas: no wrong texture can be served.
        assert!(set.for_bucket(1.5).is_none());
        // Switch to 1.5 and insert; the 1.0 glyph is untouched.
        set.get_or_create(1.5, AtlasLimits::default())
            .insert(key("中"), 20, 20);
        assert!(set.for_bucket(1.0).unwrap().contains(&key("中")));
        assert!(set.for_bucket(1.5).unwrap().contains(&key("中")));
        assert_eq!(set.buckets(), 2);
    }

    #[test]
    fn device_loss_clears_atlases() {
        let mut atlas = GlyphAtlas::new(AtlasLimits::default());
        atlas.insert(key("a"), 8, 16);
        assert!(!atlas.is_empty());
        atlas.clear();
        assert!(atlas.is_empty());
        assert!(!atlas.contains(&key("a")));
        assert_eq!(atlas.bytes(), 0);

        let mut set = AtlasSet::new();
        set.get_or_create(1.0, AtlasLimits::default())
            .insert(key("a"), 8, 16);
        set.clear_all();
        assert!(set.for_bucket(1.0).unwrap().is_empty());
    }
}
