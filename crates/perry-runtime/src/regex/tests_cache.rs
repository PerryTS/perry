use super::*;
use std::collections::HashSet;

pub(super) fn note_cache_eviction() {
    crate::hot_diag::test_note_regex_cache_eviction();
}

fn make_string(text: &str) -> *mut StringHeader {
    js_string_from_str(text)
}

fn cache_test_key(slot: usize) -> i64 {
    (0x6000_0000usize + (slot << 3)) as i64
}

/// Sabotage: remove the literal pin in `site_cache::evict_one_dynamic`, or put
/// back the whole-map clear in `evict_regex_cache_if_full`.
///
/// The first sabotage lets a content-capacity eviction discard the target's
/// sole `Arc<Programs>` owner. The second removes 512 answers instead of one.
/// After an explicit young collection finalizes both target headers, either
/// regression makes the next evaluation run the lazy builder again.
#[test]
fn literal_site_program_is_not_rebuilt_after_cache_overflow_and_young_collection() {
    let _lock = crate::gc::global_side_table_test_lock();
    site_key::test_reset();
    site_cache::test_reset();
    REGEX_CACHE.with(|cache| cache.borrow_mut().clear());
    FANCY_CACHE.with(|cache| cache.borrow_mut().clear());
    REPEAT_MATCHER_CACHE.with(|cache| cache.borrow_mut().clear());
    VALIDATED_PATTERNS.with(|cache| cache.borrow_mut().clear());
    lazy::test_reset_program_builds();

    let target = "literal-site-overflow-target-[0-9]+";
    let target_key = cache_test_key(0);
    let first = js_regexp_new_site(make_string(target), make_string(""), target_key);
    assert_eq!(
        js_regexp_test(first, make_string("literal-site-overflow-target-42")),
        1
    );
    assert_eq!(
        lazy::test_program_builds(),
        1,
        "the target builds exactly once"
    );

    // A second construction installs the content cache's program bundle into
    // the site's weak lane. Both headers must then die so that only the cache
    // policy, not a surviving receiver, can keep that weak reference live.
    let second = js_regexp_new_site(make_string(target), make_string(""), target_key);
    assert!(!unsafe { (*second).programs_ptr.is_null() });
    let first_addr = first as usize;
    let second_addr = second as usize;
    assert!(
        !site_cache::test_try_evict_one_dynamic(),
        "the only content entry is site-referenced, so no dynamic victim exists"
    );

    // Overflow the bounded content table with dynamic patterns. The target is
    // the only recorded literal among them, so every capacity eviction must
    // choose one of the dynamic entries and leave its program bundle intact.
    for i in 0..site_cache::MAX_ENTRIES {
        let _ = js_regexp_new(
            make_string(&format!("content-overflow-{i}[a-z]")),
            make_string(""),
        );
    }
    assert_eq!(site_cache::test_len(), site_cache::MAX_ENTRIES);
    assert_eq!(
        site_cache::test_has_programs(target, ""),
        Some(true),
        "a recorded literal's content entry must survive capacity eviction"
    );

    // 513 compiled literals (the target plus this 512-pattern flood) cross the
    // former 512-entry wholesale-clear boundary. Snapshot at capacity so the
    // assertion proves the overflow path preserved 511 old answers.
    for i in 0..(REGEX_CACHE_MAX_ENTRIES - 1) {
        let pattern = format!("overflow-literal-{i}$");
        let re = js_regexp_new_site(
            make_string(&pattern),
            make_string(""),
            cache_test_key(100 + i),
        );
        assert_eq!(
            js_regexp_test(re, make_string(&format!("overflow-literal-{i}"))),
            1
        );
    }
    let before: HashSet<_> = REGEX_CACHE.with(|cache| cache.borrow().keys().cloned().collect());
    assert_eq!(before.len(), REGEX_CACHE_MAX_ENTRIES);

    let last_i = REGEX_CACHE_MAX_ENTRIES - 1;
    let last_pattern = format!("overflow-literal-{last_i}$");
    let last = js_regexp_new_site(
        make_string(&last_pattern),
        make_string(""),
        cache_test_key(100 + last_i),
    );
    assert_eq!(
        js_regexp_test(last, make_string(&format!("overflow-literal-{last_i}"))),
        1
    );
    let survivors = REGEX_CACHE.with(|cache| {
        cache
            .borrow()
            .keys()
            .filter(|key| before.contains(*key))
            .count()
    });
    assert_eq!(survivors, REGEX_CACHE_MAX_ENTRIES - 1);
    assert!(
        crate::hot_diag::test_regex_builds_and_evictions().1 > 0,
        "the capacity eviction path must have executed"
    );

    let builds_before_gc = lazy::test_program_builds();
    let _ = crate::gc::gc_collect_minor();
    assert!(
        !test_regex_pointer_entry_exists(first_addr)
            && !test_regex_pointer_entry_exists(second_addr),
        "the explicit young collection must finalize both unrooted target headers"
    );

    let rebuilt = js_regexp_new_site(make_string(target), make_string(""), target_key);
    assert!(
        !unsafe { (*rebuilt).programs_ptr.is_null() },
        "the still-recorded site must be born built after cache overflow and young GC"
    );
    assert_eq!(
        js_regexp_test(rebuilt, make_string("literal-site-overflow-target-7")),
        1
    );
    assert_eq!(
        lazy::test_program_builds(),
        builds_before_gc,
        "the target site's compiled program must not be rebuilt"
    );
}
