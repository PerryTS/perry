//! Diagnostic-only `PERRY_GC_CENSUS` rows for RegExp-owned Rust tables.
//!
//! Nothing in this module is called from construction, matching, collection,
//! or cache maintenance. The census enters it only after a request has armed a
//! synchronous full collection. Engine crates do not expose the size of the
//! heap graph behind their public `Regex` values, so program bytes are an
//! explicitly labelled opaque lower-bound: the `Arc` allocation, public value,
//! and source/capture buffers that can be observed without unsafe layout
//! assumptions.

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

    let rows = vec![
        pointer_row(),
        standard_cache_row(),
        fancy_cache_row(),
        repeat_cache_row(),
        validation_cache_row(),
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
    test_reset_walks();
}
