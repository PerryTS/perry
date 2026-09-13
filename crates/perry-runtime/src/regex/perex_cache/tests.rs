use super::*;
use crate::regex::RegExpHeader;

struct Reset;
impl Reset {
    fn new() -> Self {
        clear_for_tests();
        Self
    }
}
impl Drop for Reset {
    fn drop(&mut self) {
        clear_for_tests();
    }
}

fn regex<'s>(scope: &'s RuntimeHandleScope, source: &str, flags: &str) -> RuntimeHandle<'s> {
    let source = scope.root_string_ptr(crate::regex::js_string_from_str(source));
    let flags = scope.root_string_ptr(crate::regex::js_string_from_str(flags));
    scope.root_raw_mut_ptr(
        source.with_const_ptr(|p| flags.with_const_ptr(|f| crate::regex::js_regexp_new(p, f))),
    )
}

fn program(re: &RuntimeHandle<'_>) -> usize {
    re.with_const_ptr(|p| crate::regex::test_regexp_program_address(p))
}

#[test]
fn identical_content_and_canonical_flags_share_programs() {
    let _lock = crate::gc::global_side_table_test_lock();
    let _reset = Reset::new();
    let scope = RuntimeHandleScope::new();
    for pattern in ["abc", r"(?<=a)b", r"(a)\1", r"\p{Emoji}"] {
        let a = regex(&scope, pattern, "ig");
        let b = regex(&scope, pattern, "gi");
        let c = regex(&scope, pattern, "g");
        assert_eq!(program(&a), program(&b));
        assert_ne!(program(&a), program(&c));
        assert_ne!(
            a.with_const_ptr(|p: *const RegExpHeader| p),
            b.with_const_ptr(|p: *const RegExpHeader| p)
        );
        a.with_mut_ptr(|p| crate::regex::js_regexp_set_last_index(p, 17.0));
        assert_eq!(
            b.with_const_ptr(|p| crate::regex::js_regexp_get_last_index(p)),
            0.0
        );
    }
}

#[test]
fn identity_hit_does_not_compile_or_hash_the_pattern() {
    let _lock = crate::gc::global_side_table_test_lock();
    let _reset = Reset::new();
    let scope = RuntimeHandleScope::new();
    let re = regex(&scope, "identity", "g");
    let source = scope
        .root_string_ptr(re.with_const_ptr(|p: *const RegExpHeader| unsafe { (*p).pattern_ptr }));
    let flags = CanonicalFlags::parse(b"g").unwrap();
    let cached =
        get_or_compile(&scope, &source, flags, || panic!("identity hit recompiled")).unwrap();
    assert_eq!(cached.with_ptr(|p| p as usize), program(&re));
    // A corrupted content index is irrelevant to an identity hit.
    REGEX_CACHE.with(|cache| cache.borrow_mut().contents.clear());
    get_or_compile(&scope, &source, flags, || {
        panic!("identity hit used content index")
    })
    .unwrap();
}

#[test]
fn lru_overflow_keeps_hot_program_and_evicts_one_at_a_time() {
    let _lock = crate::gc::global_side_table_test_lock();
    let _reset = Reset::new();
    let scope = RuntimeHandleScope::new();
    let hot = regex(&scope, "hot", "g");
    let oldest = regex(&scope, "oldest", "g");
    for i in 0..CAPACITY * 2 {
        let local = RuntimeHandleScope::new();
        let fresh = regex(&local, &format!("cold{i}"), "g");
        assert_ne!(program(&fresh), program(&hot));
        let same = regex(&local, "hot", "g");
        assert_eq!(program(&same), program(&hot), "hot entry evicted at {i}");
        REGEX_CACHE.with(|cache| {
            let cache = cache.borrow();
            assert_eq!(cache.identities.len(), (i + 3).min(CAPACITY));
            assert!(cache.bytes <= BYTE_LIMIT);
        });
    }
    // An evicted program can still be owned by an existing RegExp.
    let rebuilt = regex(&scope, "oldest", "g");
    assert_ne!(program(&oldest), program(&rebuilt));
}

#[test]
fn hash_collisions_do_not_share_different_sources() {
    let _lock = crate::gc::global_side_table_test_lock();
    let _reset = Reset::new();
    let scope = RuntimeHandleScope::new();
    let wrong = regex(&scope, "collision-a", "");
    let key = (
        fingerprint(b"collision-b"),
        CanonicalFlags::parse(b"").unwrap(),
    );
    REGEX_CACHE.with(|cache| {
        let mut cache = cache.borrow_mut();
        let old_key = cache.entries[0].as_ref().unwrap().key;
        let bucket = cache.contents.remove(&old_key).unwrap();
        cache.contents.insert(key, bucket);
        cache.entries[0].as_mut().unwrap().key = key;
    });
    let right = regex(&scope, "collision-b", "");
    assert_ne!(program(&wrong), program(&right));
    let again = regex(&scope, "collision-b", "");
    assert_eq!(program(&right), program(&again));
}

#[test]
fn invalid_pattern_never_enters_the_cache() {
    let _lock = crate::gc::global_side_table_test_lock();
    let _reset = Reset::new();
    let scope = RuntimeHandleScope::new();
    let source = scope.root_string_ptr(crate::regex::js_string_from_str("("));
    let flags = scope.root_string_ptr(crate::regex::js_string_from_str("g"));
    assert!(source
        .with_const_ptr(|p| flags.with_const_ptr(|f| crate::regex::perex_construct::new(p, f)))
        .is_err());
    REGEX_CACHE.with(|cache| assert!(cache.borrow().entries.is_empty()));
}

#[test]
fn content_hit_seals_the_new_original_source_against_unique_append() {
    let _lock = crate::gc::global_side_table_test_lock();
    let _reset = Reset::new();
    let scope = RuntimeHandleScope::new();
    let first = regex(&scope, "shared-source", "");
    let source = scope.root_string_ptr(crate::regex::js_string_from_str("shared-source"));
    source.with_mut_ptr::<StringHeader, _>(|p| unsafe { (*p).refcount = 1 });
    let flags = scope.root_string_ptr(crate::regex::js_string_from_str(""));
    let second = scope.root_raw_mut_ptr(
        source.with_const_ptr(|p| flags.with_const_ptr(|f| crate::regex::js_regexp_new(p, f))),
    );
    assert_eq!(program(&first), program(&second));
    assert_eq!(
        source.with_const_ptr::<StringHeader, _>(|p| unsafe { (*p).refcount }),
        0
    );
}
