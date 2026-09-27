//! #11503: a RegExp's user properties (`re.tag = v`) are its ONLY
//! address-keyed state. `REGEX_SOURCE_TABLE` and the per-type death walks that
//! enumerated it are gone, so the shared dead-owner fan-out
//! (`prune_dead_exotic_expando_owners`) is now the one thing that drops a dead
//! RegExp's expando entry on the non-copying cycle kinds. The copied-minor
//! counterpart is `nursery_regexp_that_dies_young_is_finalized_by_the_copied_minor`.
//!
//! A stale entry is not only a leak: `expando_clear_on_alloc` covers a RegExp
//! or Date recycled at the address, but any other exotic kind born there reads
//! the dead RegExp's properties as its own.

use super::*;

/// A production-constructed RegExp, unrooted once the caller's scope ends.
fn construct_regexp(pattern: &str) -> usize {
    let scope = RuntimeHandleScope::new();
    let source = scope.root_string_ptr(crate::string::js_string_from_bytes(
        pattern.as_ptr(),
        pattern.len() as u32,
    ));
    let flags = scope.root_string_ptr(crate::string::js_string_from_bytes(b"g".as_ptr(), 1));
    let re = source.with_const_ptr(|source| {
        flags.with_const_ptr(|flags| crate::regex::js_regexp_new(source, flags))
    });
    assert!(
        crate::regex::is_registered_regex(re as usize),
        "test premise: the header identifies as a RegExp"
    );
    re as usize
}

#[test]
fn test_dead_regexp_expando_pruned_on_full_gc() {
    let _guard = GcTestIsolationGuard::with_realm_bootstrapped();
    let addr = construct_regexp("dies-before-the-full-trace");
    crate::object::exotic_expando::test_seed_exotic_expando_entry(
        addr,
        "tag",
        crate::value::JSValue::int32(7).bits(),
    );
    assert!(crate::object::exotic_expando::test_exotic_expando_entry_exists(addr));
    // The construction cache holds the compiled program, not the header, but
    // evict it anyway so nothing the fixture made is reachable.
    crate::regex::perex_cache::clear_for_tests();

    // No roots: the RegExp is dead at the full trace.
    full_gc_with_no_block_persistence();

    assert!(
        !crate::object::exotic_expando::test_exotic_expando_entry_exists(addr),
        "a dead RegExp's EXOTIC_EXPANDO entry must be pruned by the full \
         collection's dead-owner fan-out"
    );
}

#[test]
fn test_live_regexp_expando_survives_full_gc() {
    let _guard = CopyingNurseryTestGuard::new(1);
    let addr = construct_regexp("stays-live-across-the-full-trace");
    crate::object::exotic_expando::test_seed_exotic_expando_entry(
        addr,
        "tag",
        crate::value::JSValue::int32(42).bits(),
    );
    js_shadow_slot_set(0, ptr_bits(addr));

    full_gc();

    // Full mark-sweep is non-moving: the rooted RegExp keeps its address.
    assert_eq!((js_shadow_slot_get(0) & POINTER_MASK) as usize, addr);
    assert!(crate::regex::is_registered_regex(addr));
    assert_eq!(
        crate::object::exotic_expando::value_lookup(
            crate::object::exotic_expando::ExoticKind::RegExp,
            addr,
            "tag",
        ),
        Some(crate::value::JSValue::int32(42).bits()),
        "a live RegExp's expando must survive a full GC"
    );
    js_shadow_slot_set(0, 0);
}
