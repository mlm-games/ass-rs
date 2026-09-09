//! Caching system for expensive operations

#[cfg(feature = "shaping")]
use crate::pipeline::shaping::ShapedText;
#[cfg(feature = "vector")]
use lyon_path::Path;

#[cfg(all(not(feature = "nostd"), any(feature = "shaping", feature = "vector")))]
use std::collections::HashMap;
#[cfg(all(not(feature = "nostd"), feature = "shaping"))]
use std::sync::Arc;

#[cfg(all(feature = "nostd", any(feature = "shaping", feature = "vector")))]
use alloc::collections::BTreeMap as HashMap;
#[cfg(feature = "nostd")]
use alloc::string::String;
#[cfg(all(feature = "nostd", feature = "shaping"))]
use alloc::sync::Arc;

/// Cache key for shaped text
#[derive(Debug, Clone, Hash, Eq, PartialEq, Ord, PartialOrd)]
pub struct TextCacheKey {
    /// Text content to be shaped
    pub text: String,
    /// Font family name
    pub font_family: String,
    /// Font size (rounded to avoid float comparison issues)
    pub font_size: u32,
    /// Whether text should be bold
    pub bold: bool,
    /// Whether text should be italic
    pub italic: bool,
}

/// Cache key for drawing paths
#[derive(Debug, Clone, Hash, Eq, PartialEq, Ord, PartialOrd)]
pub struct DrawingCacheKey {
    /// Drawing commands as string
    pub commands: String,
}

/// Render cache for expensive operations
pub struct RenderCache {
    /// Cache for shaped text (only with `shaping`)
    #[cfg(feature = "shaping")]
    shaped_text_cache: HashMap<TextCacheKey, Arc<ShapedText>>,
    #[cfg(feature = "shaping")]
    max_shaped_entries: usize,

    /// Cache for drawing paths (only with `vector`)
    #[cfg(feature = "vector")]
    drawing_path_cache: HashMap<DrawingCacheKey, Option<Path>>,
    #[cfg(feature = "vector")]
    max_drawing_entries: usize,

    /// Cache statistics
    pub stats: CacheStats,
}

/// Cache statistics for monitoring
#[derive(Debug, Default, Clone)]
pub struct CacheStats {
    /// Number of text cache hits
    pub text_hits: usize,
    /// Number of text cache misses
    pub text_misses: usize,
    /// Number of drawing cache hits
    pub drawing_hits: usize,
    /// Number of drawing cache misses
    pub drawing_misses: usize,
    /// Number of cache evictions
    pub evictions: usize,
}

impl RenderCache {
    /// Create a new render cache
    pub fn new() -> Self {
        Self {
            #[cfg(feature = "shaping")]
            shaped_text_cache: HashMap::new(),
            #[cfg(feature = "shaping")]
            max_shaped_entries: 1000,
            #[cfg(feature = "vector")]
            drawing_path_cache: HashMap::new(),
            #[cfg(feature = "vector")]
            max_drawing_entries: 500,
            stats: CacheStats::default(),
        }
    }

    /// Create with custom limits
    pub fn with_limits(max_shaped: usize, max_drawing: usize) -> Self {
        Self {
            #[cfg(feature = "shaping")]
            shaped_text_cache: HashMap::new(),
            #[cfg(feature = "shaping")]
            max_shaped_entries: max_shaped,
            #[cfg(feature = "vector")]
            drawing_path_cache: HashMap::new(),
            #[cfg(feature = "vector")]
            max_drawing_entries: max_drawing,
            stats: CacheStats::default(),
        }
    }

    /// Get shaped text from cache
    #[cfg(feature = "shaping")]
    pub fn get_shaped_text(&mut self, key: &TextCacheKey) -> Option<Arc<ShapedText>> {
        if let Some(shaped) = self.shaped_text_cache.get(key) {
            self.stats.text_hits += 1;
            Some(Arc::clone(shaped))
        } else {
            self.stats.text_misses += 1;
            None
        }
    }

    /// Store shaped text in cache
    #[cfg(feature = "shaping")]
    pub fn store_shaped_text(&mut self, key: TextCacheKey, shaped: ShapedText) -> Arc<ShapedText> {
        // Evict if at capacity
        if self.shaped_text_cache.len() >= self.max_shaped_entries {
            // Simple LRU: remove first item (not ideal but simple)
            if let Some(first_key) = self.shaped_text_cache.keys().next().cloned() {
                self.shaped_text_cache.remove(&first_key);
                self.stats.evictions += 1;
            }
        }

        let arc_shaped = Arc::new(shaped);
        self.shaped_text_cache.insert(key, Arc::clone(&arc_shaped));
        arc_shaped
    }

    /// Get drawing path from cache
    #[cfg(feature = "vector")]
    pub fn get_drawing_path(&mut self, key: &DrawingCacheKey) -> Option<Option<Path>> {
        if let Some(path) = self.drawing_path_cache.get(key) {
            self.stats.drawing_hits += 1;
            Some(path.clone())
        } else {
            self.stats.drawing_misses += 1;
            None
        }
    }

    /// Store drawing path in cache
    #[cfg(feature = "vector")]
    pub fn store_drawing_path(&mut self, key: DrawingCacheKey, path: Option<Path>) {
        // Evict if at capacity
        if self.drawing_path_cache.len() >= self.max_drawing_entries {
            if let Some(first_key) = self.drawing_path_cache.keys().next().cloned() {
                self.drawing_path_cache.remove(&first_key);
                self.stats.evictions += 1;
            }
        }

        self.drawing_path_cache.insert(key, path);
    }

    /// Clear all caches
    pub fn clear(&mut self) {
        #[cfg(feature = "shaping")]
        self.shaped_text_cache.clear();
        #[cfg(feature = "vector")]
        self.drawing_path_cache.clear();
        self.stats = CacheStats::default();
    }

    /// Get cache statistics
    pub fn stats(&self) -> &CacheStats {
        &self.stats
    }

    /// Print cache statistics
    pub fn print_stats(&self) {
        #[cfg(all(not(feature = "nostd"), feature = "shaping"))]
        let text_ratio = if self.stats.text_hits + self.stats.text_misses > 0 {
            self.stats.text_hits as f64 / (self.stats.text_hits + self.stats.text_misses) as f64
        } else {
            0.0
        };

        #[cfg(all(not(feature = "nostd"), feature = "vector"))]
        let drawing_ratio = if self.stats.drawing_hits + self.stats.drawing_misses > 0 {
            self.stats.drawing_hits as f64
                / (self.stats.drawing_hits + self.stats.drawing_misses) as f64
        } else {
            0.0
        };

        #[cfg(not(feature = "nostd"))]
        eprintln!("=== Cache Statistics ===");
        #[cfg(all(not(feature = "nostd"), feature = "shaping"))]
        eprintln!(
            "Text Cache: {} entries, {:.1}% hit rate ({}/{} hits)",
            self.shaped_text_cache.len(),
            text_ratio * 100.0,
            self.stats.text_hits,
            self.stats.text_hits + self.stats.text_misses
        );
        #[cfg(all(not(feature = "nostd"), feature = "vector"))]
        eprintln!(
            "Drawing Cache: {} entries, {:.1}% hit rate ({}/{} hits)",
            self.drawing_path_cache.len(),
            drawing_ratio * 100.0,
            self.stats.drawing_hits,
            self.stats.drawing_hits + self.stats.drawing_misses
        );
        #[cfg(not(feature = "nostd"))]
        eprintln!("Total Evictions: {}", self.stats.evictions);
    }
}

impl Default for RenderCache {
    fn default() -> Self {
        Self::new()
    }
}
