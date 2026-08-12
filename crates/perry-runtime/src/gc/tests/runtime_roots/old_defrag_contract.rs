//! #7876: old-page compaction is safe only when every non-heap address copy
//! participates in the registered mutable-root rewrite. These probes plant a
//! forwarding address directly, so they test the rewrite contract without
//! depending on heap pressure or page-selection policy.

use super::*;

fn forwarded_string() -> (usize, usize, ValidPointerSet) {
    let from = crate::string::js_string_from_bytes_longlived(b"id".as_ptr(), 2) as usize;
    let valid_ptrs = build_valid_pointer_set();
    let to = crate::string::js_string_from_bytes_longlived(b"id".as_ptr(), 2) as usize;
    unsafe {
        set_forwarding_address(header_from_user_ptr(from as *const u8), to as *mut u8);
    }
    (from, to, valid_ptrs)
}

#[test]
fn json_hot_key_ring_rewrites_with_its_owning_cache() {
    crate::json::test_clear_parse_roots();
    let (from, to, valid_ptrs) = forwarded_string();
    crate::json::test_seed_parse_roots(
        f64::from_bits(crate::value::TAG_UNDEFINED),
        from as *const _,
    );
    crate::json::test_seed_parse_key_ring(from as *const _);

    crate::json::scan_parse_roots_mut(&mut RuntimeRootVisitor::for_rewrite(&valid_ptrs));

    assert_eq!(crate::json::test_parse_roots_snapshot().1, to);
    assert_eq!(
        crate::json::test_parse_key_ring_snapshot(),
        vec![to],
        "the hot-key mirror must not retain the old address after its owning cache rewrites"
    );
    crate::json::test_clear_parse_roots();
}

#[test]
fn performance_entry_shape_identity_rewrites_as_metadata() {
    let from = crate::array::js_array_alloc_with_length_longlived(0) as usize;
    let valid_ptrs = build_valid_pointer_set();
    let to = crate::array::js_array_alloc_with_length_longlived(0) as usize;
    unsafe {
        set_forwarding_address(header_from_user_ptr(from as *const u8), to as *mut u8);
    }
    crate::perf_hooks::test_seed_perf_entry_keys_array(from);

    crate::perf_hooks::scan_perf_entries_roots_mut(&mut RuntimeRootVisitor::for_rewrite(
        &valid_ptrs,
    ));

    assert_eq!(
        crate::perf_hooks::test_perf_entry_keys_array(),
        to,
        "performance-entry identity must follow its structurally rooted keys array"
    );
}

#[test]
fn diagnostics_symbol_lookup_key_rekeys_after_a_move() {
    let from = unsafe { crate::value::js_nanbox_get_pointer(crate::symbol::js_symbol_new_empty()) }
        as usize;
    let valid_ptrs = build_valid_pointer_set();
    let to = unsafe { crate::value::js_nanbox_get_pointer(crate::symbol::js_symbol_new_empty()) }
        as usize;
    unsafe {
        set_forwarding_address(header_from_user_ptr(from as *const u8), to as *mut u8);
    }
    crate::node_submodules::diagnostics::test_seed_diag_symbol_key(
        POINTER_TAG | (from as u64 & POINTER_MASK),
    );

    crate::node_submodules::scan_node_submodule_singleton_roots_mut(
        &mut RuntimeRootVisitor::for_rewrite(&valid_ptrs),
    );

    assert_eq!(
        crate::node_submodules::diagnostics::test_diag_symbol_keys(),
        vec![POINTER_TAG | (to as u64 & POINTER_MASK)],
        "diagnostics_channel's address-keyed symbol lookup must be rekeyed"
    );
}
