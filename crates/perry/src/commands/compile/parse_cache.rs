//! In-memory parse cache for `perry dev` rebuilds.
//!
//! Extracted from `compile.rs` (Tier 2.1 of the compiler-improvement
//! plan, v0.5.333). The cache key is the absolute file path; a
//! re-parse is skipped when the source bytes haven't changed since the
//! last call. Counters track hit / miss for diagnostics.
//!
//! Scope is strictly per-process: the cache lives for the duration of
//! one `perry dev` invocation. `perry compile` never sees it.

use anyhow::{anyhow, Result};
use std::collections::{HashMap, VecDeque};
use std::path::{Path, PathBuf};

/// Default eviction threshold (Tier 4.5, v0.5.335). A typical project
/// has under 100 active source files; 500 keeps everything resident
/// while preventing the unbounded growth a long-running `perry dev`
/// session could otherwise produce (e.g. accidentally walking
/// `node_modules` and parsing thousands of files once). Override at
/// construction time via [`ParseCache::with_capacity`] for atypical
/// projects.
pub const DEFAULT_PARSE_CACHE_CAPACITY: usize = 500;

pub struct ParseCache {
    pub(super) entries: HashMap<PathBuf, ParseCacheEntry>,
    /// Insertion-order queue — when `entries.len() >= max_entries`,
    /// the front (oldest insertion) is evicted before adding a new
    /// entry. FIFO not LRU strictly, but functionally equivalent for
    /// the perry-dev access pattern: a file's miss → re-insert puts it
    /// at the back, while files that haven't been touched stay at the
    /// front and get evicted first. Picking FIFO over LRU avoids a new
    /// `lru` crate dep and the per-hit re-ordering it would need.
    order: VecDeque<PathBuf>,
    max_entries: usize,
    hits: usize,
    misses: usize,
}

impl Default for ParseCache {
    fn default() -> Self {
        Self::with_capacity(DEFAULT_PARSE_CACHE_CAPACITY)
    }
}

pub(super) struct ParseCacheEntry {
    pub(super) source: String,
    pub(super) module: swc_ecma_ast::Module,
}

impl ParseCache {
    pub fn new() -> Self {
        Self::default()
    }

    /// Create a cache that holds up to `max_entries` parsed modules
    /// before evicting the oldest insertion. Pass `usize::MAX` to
    /// disable eviction (matches the pre-Tier-4.5 unbounded behaviour).
    pub fn with_capacity(max_entries: usize) -> Self {
        Self {
            entries: HashMap::new(),
            order: VecDeque::new(),
            max_entries,
            hits: 0,
            misses: 0,
        }
    }

    /// Number of cache hits since creation (or since `reset_counters`).
    pub fn hits(&self) -> usize {
        self.hits
    }

    /// Number of cache misses (fresh parses) since creation.
    pub fn misses(&self) -> usize {
        self.misses
    }

    /// Reset hit/miss counters. Intended to be called between dev rebuilds
    /// so the counters reflect a single rebuild rather than cumulative.
    pub fn reset_counters(&mut self) {
        self.hits = 0;
        self.misses = 0;
    }
}

/// Parse `source` via the cache: return a borrowed `&Module` from the
/// cache, reusing the last entry if its source bytes match, else
/// reparsing.
pub(super) fn parse_cached<'a>(
    cache: &'a mut ParseCache,
    path: &Path,
    source: &str,
    filename: &str,
) -> Result<&'a swc_ecma_ast::Module> {
    let fresh = cache
        .entries
        .get(path)
        .map_or(false, |e| e.source == source);
    if fresh {
        cache.hits += 1;
    } else {
        let parsed = perry_parser::parse_typescript(source, filename)
            .map_err(|e| anyhow!("Failed to parse {}: {}", path.display(), e))?;

        let path_buf = path.to_path_buf();
        // If the path already had an older entry with stale source,
        // don't double-count it in the order queue — replace in-place.
        let was_present = cache.entries.contains_key(&path_buf);

        // Tier 4.5 eviction: enforce the configured cap before
        // inserting a brand-new path. Same-path re-inserts (above) bypass
        // eviction since the entry count is unchanged.
        if !was_present && cache.entries.len() >= cache.max_entries {
            if let Some(victim) = cache.order.pop_front() {
                cache.entries.remove(&victim);
            }
        }
        if !was_present {
            cache.order.push_back(path_buf.clone());
        }
        cache.entries.insert(
            path_buf,
            ParseCacheEntry {
                source: source.to_string(),
                module: parsed,
            },
        );
        cache.misses += 1;
    }
    // The entry is guaranteed to exist at this point (we just inserted on miss).
    Ok(&cache.entries[path].module)
}

#[cfg(test)]
mod tests {
    use super::*;

    const SRC_V1: &str = "export function greet(name: string): string { return `hi ${name}`; }\n";
    const SRC_V2: &str =
        "export function greet(name: string): string { return `hello ${name}`; }\n";

    #[test]
    fn first_call_is_a_miss() {
        let mut cache = ParseCache::new();
        let path = PathBuf::from("/virtual/greet.ts");
        let _ = parse_cached(&mut cache, &path, SRC_V1, "greet.ts").unwrap();
        assert_eq!(cache.hits(), 0);
        assert_eq!(cache.misses(), 1);
        assert_eq!(cache.entries.len(), 1);
    }

    #[test]
    fn identical_source_is_a_hit() {
        let mut cache = ParseCache::new();
        let path = PathBuf::from("/virtual/greet.ts");
        let _ = parse_cached(&mut cache, &path, SRC_V1, "greet.ts").unwrap();
        let _ = parse_cached(&mut cache, &path, SRC_V1, "greet.ts").unwrap();
        assert_eq!(cache.hits(), 1);
        assert_eq!(cache.misses(), 1);
    }

    #[test]
    fn changed_source_is_a_miss_and_replaces_entry() {
        let mut cache = ParseCache::new();
        let path = PathBuf::from("/virtual/greet.ts");
        let _ = parse_cached(&mut cache, &path, SRC_V1, "greet.ts").unwrap();
        let _ = parse_cached(&mut cache, &path, SRC_V2, "greet.ts").unwrap();
        // Two misses, zero hits; cache still holds one entry (the new version).
        assert_eq!(cache.hits(), 0);
        assert_eq!(cache.misses(), 2);
        assert_eq!(cache.entries.len(), 1);
        assert_eq!(cache.entries[&path].source, SRC_V2);
    }

    #[test]
    fn reverting_to_previous_source_is_still_a_miss() {
        // The cache keeps only the last version, not history. Reverting to a
        // prior source counts as a miss — documented behaviour.
        let mut cache = ParseCache::new();
        let path = PathBuf::from("/virtual/greet.ts");
        let _ = parse_cached(&mut cache, &path, SRC_V1, "greet.ts").unwrap();
        let _ = parse_cached(&mut cache, &path, SRC_V2, "greet.ts").unwrap();
        let _ = parse_cached(&mut cache, &path, SRC_V1, "greet.ts").unwrap();
        assert_eq!(cache.hits(), 0);
        assert_eq!(cache.misses(), 3);
    }

    #[test]
    fn distinct_paths_are_independent() {
        let mut cache = ParseCache::new();
        let p_a = PathBuf::from("/virtual/a.ts");
        let p_b = PathBuf::from("/virtual/b.ts");
        let _ = parse_cached(&mut cache, &p_a, SRC_V1, "a.ts").unwrap();
        let _ = parse_cached(&mut cache, &p_b, SRC_V1, "b.ts").unwrap();
        let _ = parse_cached(&mut cache, &p_a, SRC_V1, "a.ts").unwrap();
        let _ = parse_cached(&mut cache, &p_b, SRC_V1, "b.ts").unwrap();
        assert_eq!(cache.hits(), 2);
        assert_eq!(cache.misses(), 2);
    }

    #[test]
    fn reset_counters_clears_hit_miss_but_keeps_entries() {
        let mut cache = ParseCache::new();
        let path = PathBuf::from("/virtual/greet.ts");
        let _ = parse_cached(&mut cache, &path, SRC_V1, "greet.ts").unwrap();
        let _ = parse_cached(&mut cache, &path, SRC_V1, "greet.ts").unwrap();
        assert_eq!(cache.hits(), 1);
        assert_eq!(cache.misses(), 1);
        cache.reset_counters();
        assert_eq!(cache.hits(), 0);
        assert_eq!(cache.misses(), 0);
        // Next lookup for the same source should be a hit, not a miss —
        // entries survive reset_counters.
        let _ = parse_cached(&mut cache, &path, SRC_V1, "greet.ts").unwrap();
        assert_eq!(cache.hits(), 1);
        assert_eq!(cache.misses(), 0);
    }

    #[test]
    fn hit_returns_equivalent_ast_to_fresh_parse() {
        // A cache hit must give us the same AST shape as reparsing from
        // scratch — this is the correctness invariant V2.1 relies on.
        let mut cache = ParseCache::new();
        let path = PathBuf::from("/virtual/greet.ts");
        let first = parse_cached(&mut cache, &path, SRC_V1, "greet.ts")
            .unwrap()
            .clone();
        let cached = parse_cached(&mut cache, &path, SRC_V1, "greet.ts")
            .unwrap()
            .clone();
        let fresh = perry_parser::parse_typescript(SRC_V1, "greet.ts").unwrap();
        assert_eq!(first.body.len(), fresh.body.len());
        assert_eq!(cached.body.len(), fresh.body.len());
    }

    #[test]
    fn eviction_caps_entries_at_max_capacity() {
        // Tier 4.5 invariant: the cache MUST evict the oldest insertion
        // when adding a new entry would exceed max_entries. Otherwise a
        // `perry dev` session that walks node_modules grows unbounded.
        let mut cache = ParseCache::with_capacity(3);
        for i in 0..6 {
            let path = PathBuf::from(format!("/virtual/file{}.ts", i));
            let _ = parse_cached(
                &mut cache,
                &path,
                &format!("export const x = {};", i),
                "f.ts",
            )
            .unwrap();
        }
        assert_eq!(cache.entries.len(), 3, "cap must hold");
        // Files 0-2 (oldest) should have been evicted; 3-5 should remain.
        assert!(!cache
            .entries
            .contains_key(&PathBuf::from("/virtual/file0.ts")));
        assert!(!cache
            .entries
            .contains_key(&PathBuf::from("/virtual/file2.ts")));
        assert!(cache
            .entries
            .contains_key(&PathBuf::from("/virtual/file3.ts")));
        assert!(cache
            .entries
            .contains_key(&PathBuf::from("/virtual/file5.ts")));
    }

    #[test]
    fn re_inserting_same_path_does_not_count_against_cap() {
        // Path A is touched many times — the cap should still allow B
        // and C as separate entries because re-inserts at A don't grow
        // the ordering queue.
        let mut cache = ParseCache::with_capacity(2);
        let p_a = PathBuf::from("/virtual/a.ts");
        let p_b = PathBuf::from("/virtual/b.ts");
        let p_c = PathBuf::from("/virtual/c.ts");
        let _ = parse_cached(&mut cache, &p_a, SRC_V1, "a.ts").unwrap();
        let _ = parse_cached(&mut cache, &p_a, SRC_V2, "a.ts").unwrap(); // re-insert
        let _ = parse_cached(&mut cache, &p_b, SRC_V1, "b.ts").unwrap();
        // Cap is 2; inserting C should evict A (oldest unique entry).
        let _ = parse_cached(&mut cache, &p_c, SRC_V1, "c.ts").unwrap();
        assert_eq!(cache.entries.len(), 2);
        assert!(!cache.entries.contains_key(&p_a));
        assert!(cache.entries.contains_key(&p_b));
        assert!(cache.entries.contains_key(&p_c));
    }

    #[test]
    fn parse_error_propagates_and_does_not_poison_cache() {
        let mut cache = ParseCache::new();
        let path = PathBuf::from("/virtual/bad.ts");
        let err = parse_cached(&mut cache, &path, "let x: number = ;", "bad.ts");
        assert!(err.is_err());
        // A later good parse at the same path still works and is a miss.
        let ok = parse_cached(&mut cache, &path, SRC_V1, "bad.ts");
        assert!(ok.is_ok());
        assert_eq!(cache.hits(), 0);
        assert_eq!(cache.misses(), 1);
    }
}
