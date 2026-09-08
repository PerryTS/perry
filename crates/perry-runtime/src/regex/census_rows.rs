//! Diagnostic-only `PERRY_GC_CENSUS` rows for RegExp-owned Rust tables.
//!
//! Nothing in this module is called from construction, matching, collection,
//! or cache maintenance. The census enters it only after a request has armed a
//! synchronous full collection. Engine crates do not expose the size of the
//! heap graph behind their public `Regex` values, so program bytes are an
//! explicitly labelled opaque lower-bound: the `Arc` allocation, public value,
//! and source/capture buffers that can be observed without unsafe layout
//! assumptions.

use std::collections::HashSet;
use std::sync::Arc;

use super::*;

pub(crate) struct RegexCensusRow {
    pub(crate) table: &'static str,
    pub(crate) entries: usize,
    pub(crate) bytes: usize,
    fields: serde_json::Map<String, serde_json::Value>,
}

impl RegexCensusRow {
    fn new(table: &'static str, entries: usize, bytes: usize) -> Self {
        Self {
            table,
            entries,
            bytes,
            fields: serde_json::Map::new(),
        }
    }

    fn usize(mut self, name: &'static str, value: usize) -> Self {
        self.fields.insert(name.into(), serde_json::json!(value));
        self
    }

    fn u64(mut self, name: &'static str, value: u64) -> Self {
        self.fields.insert(name.into(), serde_json::json!(value));
        self
    }

    fn bool(mut self, name: &'static str, value: bool) -> Self {
        self.fields.insert(name.into(), serde_json::json!(value));
        self
    }

    fn text(mut self, name: &'static str, value: &'static str) -> Self {
        self.fields.insert(name.into(), serde_json::json!(value));
        self
    }

    pub(crate) fn json(&self) -> serde_json::Value {
        let mut value = serde_json::Map::new();
        value.insert("table".into(), serde_json::json!(self.table));
        value.insert("entries".into(), serde_json::json!(self.entries));
        value.insert("bytes".into(), serde_json::json!(self.bytes));
        value.extend(self.fields.clone());
        serde_json::Value::Object(value)
    }
}

pub(crate) struct RegexCensusSnapshot {
    pub(crate) rows: Vec<RegexCensusRow>,
    /// Built independently of JSON serialization. If a row is accidentally
    /// omitted from the emitted array, the reconciliation test sees the gap.
    pub(crate) attributed_bytes: usize,
}

#[cfg(test)]
static TEST_CENSUS_WALKS: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

#[cfg(test)]
pub(crate) fn test_reset_walks() {
    TEST_CENSUS_WALKS.store(0, std::sync::atomic::Ordering::Relaxed);
}

#[cfg(test)]
pub(crate) fn test_walks() -> usize {
    TEST_CENSUS_WALKS.load(std::sync::atomic::Ordering::Relaxed)
}

#[inline]
fn arc_allocation_bytes<T>() -> usize {
    // Two strong/weak counters precede the Arc payload in today's allocator
    // representation. This is an estimate, not a promise about Arc layout.
    2 * std::mem::size_of::<usize>() + std::mem::size_of::<T>()
}

fn standard_program_bytes(program: &regex::Regex) -> usize {
    arc_allocation_bytes::<regex::Regex>() + program.as_str().len()
}

fn fancy_program_bytes(program: &fancy_regex::Regex) -> usize {
    arc_allocation_bytes::<fancy_regex::Regex>() + program.as_str().len()
}

fn repeat_program_bytes(program: &repeat_matcher::RepeatMatcherRegex) -> usize {
    arc_allocation_bytes::<repeat_matcher::RepeatMatcherRegex>() + program.census_buffer_bytes()
}

fn program_bundle_bytes(
    bundle_ptrs: impl IntoIterator<Item = usize>,
    standard_skip: &HashSet<usize>,
    fancy_skip: &HashSet<usize>,
    repeat_skip: &HashSet<usize>,
) -> usize {
    let mut standard_seen = standard_skip.clone();
    let mut fancy_seen = fancy_skip.clone();
    let mut repeat_seen = repeat_skip.clone();
    let mut bytes = 0usize;
    for ptr in bundle_ptrs {
        let programs = unsafe { &*(ptr as *const site_cache::Programs) };
        bytes += arc_allocation_bytes::<site_cache::Programs>();
        if standard_seen.insert(Arc::as_ptr(&programs.std) as usize) {
            bytes += standard_program_bytes(&programs.std);
        }
        if let Some(program) = &programs.fancy {
            if fancy_seen.insert(Arc::as_ptr(program) as usize) {
                bytes += fancy_program_bytes(program);
            }
        }
        if let Some(program) = &programs.repeat {
            if repeat_seen.insert(Arc::as_ptr(program) as usize) {
                bytes += repeat_program_bytes(program);
            }
        }
    }
    bytes
}

fn pointer_row() -> RegexCensusRow {
    REGEX_POINTERS.with(|table| {
        let table = table.borrow();
        let live_headers = table
            .iter()
            .filter(|&&addr| unsafe {
                crate::value::addr_class::try_read_gc_header(addr).is_some_and(|header| {
                    header.obj_type == crate::gc::GC_TYPE_REGEXP
                        && header.gc_flags & (crate::gc::GC_FLAG_MARKED | crate::gc::GC_FLAG_PINNED)
                            != 0
                })
            })
            .count();
        RegexCensusRow::new(
            "regex.pointers",
            table.len(),
            crate::gc::census::set_bytes(&*table),
        )
        .usize("live_headers", live_headers)
    })
}

fn standard_cache_row() -> RegexCensusRow {
    REGEX_CACHE.with(|cache| {
        let cache = cache.borrow();
        let programs = cache
            .values()
            .map(|program| (Arc::as_ptr(program) as usize, program))
            .collect::<std::collections::HashMap<_, _>>();
        let opaque = programs
            .values()
            .map(|program| standard_program_bytes(program))
            .sum::<usize>();
        let mut cleared = 0;
        let mut evictions = 0;
        if crate::hot_diag::regex_on() {
            crate::hot_diag::regex_counters(|diag| {
                cleared = diag.cache_clears;
                evictions = diag.cache_evictions;
            });
        }
        RegexCensusRow::new(
            "regex.program_cache",
            cache.len(),
            crate::gc::census::map_bytes(&*cache) + opaque,
        )
        .usize("compiled_programs", programs.len())
        .usize("opaque_program_bytes", opaque)
        .text("program_bytes_estimate", "opaque_inline_lower_bound")
        .bool("program_bytes_inside_side_table_bytes", true)
        .u64("cleared", cleared)
        .u64("evictions", evictions)
        .text("cache_event_scope", "all_regex_caches")
    })
}

fn fancy_cache_row() -> RegexCensusRow {
    FANCY_CACHE.with(|cache| {
        let cache = cache.borrow();
        let programs = cache
            .values()
            .map(|program| (Arc::as_ptr(program) as usize, program))
            .collect::<std::collections::HashMap<_, _>>();
        let opaque = programs
            .values()
            .map(|program| fancy_program_bytes(program))
            .sum::<usize>();
        RegexCensusRow::new(
            "regex.fancy_cache",
            cache.len(),
            crate::gc::census::map_bytes(&*cache) + opaque,
        )
        .usize("compiled_programs", programs.len())
        .usize("opaque_program_bytes", opaque)
        .text("program_bytes_estimate", "opaque_inline_lower_bound")
        .bool("program_bytes_inside_side_table_bytes", true)
    })
}

fn repeat_cache_row() -> RegexCensusRow {
    REPEAT_MATCHER_CACHE.with(|cache| {
        let cache = cache.borrow();
        let programs = cache
            .values()
            .map(|program| (Arc::as_ptr(program) as usize, program))
            .collect::<std::collections::HashMap<_, _>>();
        let opaque = programs
            .values()
            .map(|program| repeat_program_bytes(program))
            .sum::<usize>();
        RegexCensusRow::new(
            "regex.repeat_cache",
            cache.len(),
            crate::gc::census::map_bytes(&*cache) + opaque,
        )
        .usize("compiled_programs", programs.len())
        .usize("opaque_program_bytes", opaque)
        .text("program_bytes_estimate", "opaque_inline_lower_bound")
        .bool("program_bytes_inside_side_table_bytes", true)
    })
}

fn validation_cache_row() -> RegexCensusRow {
    VALIDATED_PATTERNS.with(|cache| {
        let cache = cache.borrow();
        let text_bytes = cache
            .keys()
            .map(|(pattern, flags)| pattern.capacity() + flags.capacity())
            .sum::<usize>();
        RegexCensusRow::new(
            "regex.validated_patterns",
            cache.len(),
            crate::gc::census::map_bytes(&*cache) + text_bytes,
        )
        .usize("text_bytes", text_bytes)
    })
}

fn matcher_kind_row() -> RegexCensusRow {
    let mut counts = [0usize; 4];
    REGEX_POINTERS.with(|table| {
        for &addr in table.borrow().iter() {
            let re = addr as *const RegExpHeader;
            if !is_valid_regex_ptr(re) {
                continue;
            }
            let index = unsafe {
                match (*re).matcher_kind {
                    MatcherKind::Unbuilt => 0,
                    MatcherKind::Standard => 1,
                    MatcherKind::Fancy => 2,
                    MatcherKind::Repeat => 3,
                }
            };
            counts[index] += 1;
        }
    });
    RegexCensusRow::new("regex.matcher_kinds", counts.iter().sum(), 0)
        .usize("unbuilt", counts[0])
        .usize("standard", counts[1])
        .usize("fancy", counts[2])
        .usize("repeat", counts[3])
        .bool("bytes_inside_side_table_bytes", false)
        .text("storage", "RegExpHeader.matcher_kind")
}

fn content_row(
    standard_cached: &HashSet<usize>,
    fancy_cached: &HashSet<usize>,
    repeat_cached: &HashSet<usize>,
) -> RegexCensusRow {
    let (entries, table_bytes, bundle_ptrs) = site_cache::census_parts();
    let opaque = program_bundle_bytes(
        bundle_ptrs.iter().copied(),
        standard_cached,
        fancy_cached,
        repeat_cached,
    );
    RegexCensusRow::new("regex.content_cache", entries, table_bytes + opaque)
        .usize("pinned_programs", bundle_ptrs.len())
        .usize("opaque_program_bytes", opaque)
        .text("program_bytes_estimate", "opaque_inline_lower_bound")
        .bool("program_bytes_inside_side_table_bytes", true)
}

fn site_table_row(
    content_bundles: &HashSet<usize>,
    standard_cached: &HashSet<usize>,
    fancy_cached: &HashSet<usize>,
    repeat_cached: &HashSet<usize>,
) -> RegexCensusRow {
    let (sites, table_bytes, header_ptrs, bundle_ptrs) = site_test::census_parts();
    let rooted_headers = header_ptrs.len();
    let exclusive = bundle_ptrs
        .iter()
        .copied()
        .filter(|ptr| !content_bundles.contains(ptr))
        .collect::<Vec<_>>();
    let attributed_program_bytes = program_bundle_bytes(
        exclusive.iter().copied(),
        standard_cached,
        fancy_cached,
        repeat_cached,
    );
    let pinned_program_bytes = program_bundle_bytes(
        bundle_ptrs.iter().copied(),
        &HashSet::new(),
        &HashSet::new(),
        &HashSet::new(),
    );
    RegexCensusRow::new(
        "regex.site_table",
        sites,
        table_bytes + attributed_program_bytes,
    )
    .usize("sites", sites)
    .usize("rooted_headers", rooted_headers)
    .usize(
        "rooted_header_bytes",
        rooted_headers * std::mem::size_of::<RegExpHeader>(),
    )
    .bool("rooted_header_bytes_inside_side_table_bytes", false)
    .usize("pinned_programs", bundle_ptrs.len())
    .usize("exclusively_attributed_programs", exclusive.len())
    .usize("pinned_program_bytes", pinned_program_bytes)
    .usize("attributed_program_bytes", attributed_program_bytes)
    .text("program_bytes_estimate", "opaque_inline_lower_bound")
    .bool("pinned_program_bytes_inside_side_table_bytes", false)
    .bool("attributed_program_bytes_inside_side_table_bytes", true)
}

fn literal_site_row() -> RegexCensusRow {
    let (sites, bytes) = site_key::census_parts();
    RegexCensusRow::new("regex.literal_sites", sites, bytes).usize("sites", sites)
}

fn active_factory_row() -> RegexCensusRow {
    let (entries, bytes) = site_test::active_factory_census_parts();
    RegexCensusRow::new("regex.active_factory_sites", entries, bytes)
}

fn expando_row() -> RegexCensusRow {
    let (owners, properties, bytes) = crate::object::exotic_expando::regex_expando_census();
    RegexCensusRow::new("regex.expando_owners", owners, bytes)
        .usize("owners", owners)
        .usize("properties", properties)
}

/// Snapshot every RegExp-owned table only when the census requests it.
pub(crate) fn census_snapshot() -> RegexCensusSnapshot {
    #[cfg(test)]
    TEST_CENSUS_WALKS.fetch_add(1, std::sync::atomic::Ordering::Relaxed);

    let standard_cached = REGEX_CACHE.with(|cache| {
        cache
            .borrow()
            .values()
            .map(|program| Arc::as_ptr(program) as usize)
            .collect::<HashSet<_>>()
    });
    let fancy_cached = FANCY_CACHE.with(|cache| {
        cache
            .borrow()
            .values()
            .map(|program| Arc::as_ptr(program) as usize)
            .collect::<HashSet<_>>()
    });
    let repeat_cached = REPEAT_MATCHER_CACHE.with(|cache| {
        cache
            .borrow()
            .values()
            .map(|program| Arc::as_ptr(program) as usize)
            .collect::<HashSet<_>>()
    });
    let content_bundles = site_cache::census_program_ptrs();

    let rows = vec![
        pointer_row(),
        standard_cache_row(),
        fancy_cache_row(),
        repeat_cache_row(),
        validation_cache_row(),
        content_row(&standard_cached, &fancy_cached, &repeat_cached),
        literal_site_row(),
        site_table_row(
            &content_bundles,
            &standard_cached,
            &fancy_cached,
            &repeat_cached,
        ),
        active_factory_row(),
        expando_row(),
        matcher_kind_row(),
    ];

    // Deliberately independent from JSON row registration below: this second
    // diagnostic walk is what makes an omitted row fail reconciliation.
    let attributed_bytes = pointer_row().bytes
        + standard_cache_row().bytes
        + fancy_cache_row().bytes
        + repeat_cache_row().bytes
        + validation_cache_row().bytes
        + content_row(&standard_cached, &fancy_cached, &repeat_cached).bytes
        + literal_site_row().bytes
        + site_table_row(
            &content_bundles,
            &standard_cached,
            &fancy_cached,
            &repeat_cached,
        )
        .bytes
        + active_factory_row().bytes
        + expando_row().bytes
        + matcher_kind_row().bytes;

    RegexCensusSnapshot {
        rows,
        attributed_bytes,
    }
}

#[cfg(test)]
pub(crate) fn test_reset_tables() {
    REGEX_POINTERS.with(|table| table.borrow_mut().clear());
    REGEX_CACHE.with(|cache| cache.borrow_mut().clear());
    FANCY_CACHE.with(|cache| cache.borrow_mut().clear());
    REPEAT_MATCHER_CACHE.with(|cache| cache.borrow_mut().clear());
    VALIDATED_PATTERNS.with(|cache| cache.borrow_mut().clear());
    site_cache::test_reset();
    site_key::test_reset();
    site_test::test_reset();
    test_reset_walks();
}
