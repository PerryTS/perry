use super::*;
use crate::gc::{RuntimeHandle, RuntimeHandleScope};
use crate::value::{js_nanbox_pointer, js_nanbox_string};

fn word() -> *mut u64 {
    Box::leak(Box::new(0u64))
}
fn literal<'s>(scope: &'s RuntimeHandleScope, site: *mut u64, pattern: &str) -> RuntimeHandle<'s> {
    let source = scope.root_string_ptr(super::super::js_string_from_str(pattern));
    let flags = scope.root_string_ptr(super::super::js_string_from_str("g"));
    scope.root_raw_mut_ptr(source.with_const_ptr(|source| {
        flags.with_const_ptr(|flags| js_regexp_literal(source, flags, site as i64))
    }))
}
fn data(re: &RuntimeHandle<'_>) -> usize {
    re.with_const_ptr::<RegExpHeader, _>(|re| super::super::regexp_data_ptr(re) as usize)
}

#[test]
fn literal_fresh() {
    let _lock = crate::gc::global_side_table_test_lock();
    let scope = RuntimeHandleScope::new();
    let site = word();
    let a = literal(&scope, site, "fresh");
    let b = literal(&scope, site, "fresh");
    assert_ne!(
        a.with_const_ptr::<RegExpHeader, _>(|p| p),
        b.with_const_ptr::<RegExpHeader, _>(|p| p)
    );
    assert_eq!(data(&a), data(&b));
    assert_eq!(
        unsafe { *site & crate::value::POINTER_MASK },
        data(&a) as u64,
        "the site holds data, never the object"
    );
    let _ = literal(&scope, site, "fresh");
    assert_eq!(data(&a) as u64, unsafe {
        *site & crate::value::POINTER_MASK
    });
}

#[test]
fn literal_state() {
    let _lock = crate::gc::global_side_table_test_lock();
    let scope = RuntimeHandleScope::new();
    let site = word();
    let a = literal(&scope, site, "a");
    let b = literal(&scope, site, "a");
    let input = scope.root_string_ptr(super::super::js_string_from_str("aa"));
    assert_ne!(
        a.with_const_ptr(|p| input.with_const_ptr(|s| super::super::js_regexp_test(p, s))),
        0
    );
    assert_eq!(
        a.with_const_ptr(|p| super::super::regex_last_index_offset(p)),
        1
    );
    assert_eq!(
        b.with_const_ptr(|p| super::super::regex_last_index_offset(p)),
        0
    );
    let c = literal(&scope, site, "a");
    assert_eq!(
        c.with_const_ptr(|p| super::super::regex_last_index_offset(p)),
        0
    );
    assert_eq!(data(&a), data(&c));
}

#[test]
fn literal_compile_replaces_only_one_instance_matcher() {
    let _lock = crate::gc::global_side_table_test_lock();
    let scope = RuntimeHandleScope::new();
    let site = word();
    let a = literal(&scope, site, "before");
    let b = literal(&scope, site, "before");
    let original = data(&a);
    let source = scope.root_string_ptr(super::super::js_string_from_str("after"));
    let flags = scope.root_string_ptr(super::super::js_string_from_str("i"));
    a.with_mut_ptr(|a| {
        source.with_const_ptr::<StringHeader, _>(|s| {
            flags.with_const_ptr::<StringHeader, _>(|f| {
                super::super::js_regexp_compile_value(
                    a,
                    js_nanbox_string(s as i64),
                    js_nanbox_string(f as i64),
                )
            })
        })
    });
    assert_ne!(data(&a), original);
    assert_eq!(data(&b), original);
    assert_eq!(
        unsafe { *site & crate::value::POINTER_MASK },
        original as u64
    );
    let c = literal(&scope, site, "before");
    assert_eq!(data(&c), original);
    for re in [&b, &c] {
        let source = re.with_const_ptr(|p| super::super::js_regexp_get_source(p));
        assert_eq!(super::super::string_as_str(source), "before");
    }
    assert!(super::super::regexp_data_of(
        a.with_const_ptr::<RegExpHeader, _>(|p| js_nanbox_pointer(p as i64))
    )
    .is_some());
}

#[test]
fn literal_worker_never_reads_or_publishes_the_primary_site_word() {
    let _lock = crate::gc::global_side_table_test_lock();
    // A deliberately invalid foreign heap address proves the worker guard
    // runs before even reading/dereferencing the site's cached data.
    let site = Box::leak(Box::new(u64::MAX)) as *mut u64 as usize;
    std::thread::spawn(move || {
        let agent = crate::agent::enter_worker_agent();
        let scope = RuntimeHandleScope::new();
        let a = literal(&scope, site as *mut u64, "worker");
        let b = literal(&scope, site as *mut u64, "worker");
        assert_eq!(
            data(&a),
            data(&b),
            "worker fallback uses its thread compile cache"
        );
        assert_ne!(
            a.with_const_ptr::<RegExpHeader, _>(|p| p),
            b.with_const_ptr::<RegExpHeader, _>(|p| p)
        );
        assert_eq!(unsafe { *(site as *mut u64) }, u64::MAX);
        crate::agent::retire_agent(agent);
    })
    .join()
    .unwrap();
}

#[test]
fn two_literal_sites_with_equal_length_patterns_keep_their_own_data() {
    let _lock = crate::gc::global_side_table_test_lock();
    let scope = RuntimeHandleScope::new();
    let site_a = word();
    let site_b = word();
    for _ in 0..2 {
        let a = literal(&scope, site_a, "a.c");
        let b = literal(&scope, site_b, "x.z");
        assert_ne!(data(&a), data(&b));
        for (re, expected) in [(&a, "a.c"), (&b, "x.z")] {
            let source = re.with_const_ptr(|p| super::super::js_regexp_get_source(p));
            assert_eq!(super::super::string_as_str(source), expected);
        }
    }
}
