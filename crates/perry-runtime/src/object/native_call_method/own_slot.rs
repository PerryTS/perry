//! Own-method slot lookup shared by the fallback dispatcher and vtable guard.

use crate::array::ArrayHeader;
use crate::object::{keys_lookup, shapes};

/// Resolve a method name without an O(own keys) scan on every call (#10502).
///
/// Unlike mutation-heavy property lookups, method dispatch builds the shared
/// key index on first use for any receiver wider than
/// [`DIRECT_SCAN_MAX_KEYS`]; a narrower one is compared directly. The index
/// validates candidate contents and proves absence when complete. It stores
/// slot numbers, not callees: overwriting a method still loads the current
/// receiver's value. Existing shape mutation and GC maintenance
/// invalidate/rekey the index.
///
/// Only Rust storage is allocated; this lookup cannot collect or invoke JS.
/// `keys` must be a validated live keys array read from the receiver's shape
/// on this straight-line path with no allocation since (the
/// `keys_array_dense_slots_resolved` contract), and `key_count` its logical
/// bound.
#[inline(always)]
pub(super) unsafe fn find_method_slot(
    keys: *const ArrayHeader,
    key_count: u32,
    name: &[u8],
) -> Option<u32> {
    if key_count <= DIRECT_SCAN_MAX_KEYS {
        return scan_method_slot(keys, key_count, name);
    }
    find_method_slot_indexed(keys, key_count, name)
}

/// [`find_method_slot`] for a receiver wider than [`DIRECT_SCAN_MAX_KEYS`]:
/// the shared index, out of line so the direct compare stays in its callers.
#[inline(never)]
unsafe fn find_method_slot_indexed(
    keys: *const ArrayHeader,
    key_count: u32,
    name: &[u8],
) -> Option<u32> {
    let hash = keys_lookup::key_bytes_hash(name.as_ptr(), name.len());
    match shapes::shape_slot_lookup_verdict(keys, name, hash, key_count, true) {
        shapes::KeysIndexVerdict::Found(slot) => Some(slot),
        shapes::KeysIndexVerdict::Absent => None,
        // A declined/incomplete index cannot prove absence. Preserve the
        // content-based dense lookup as a correctness backstop.
        shapes::KeysIndexVerdict::Unindexed => {
            keys_lookup::keys_find_slot_by_bytes(keys, key_count, name)
        }
    }
}

/// Own key lists up to this length are compared directly. A byte compare per
/// key is cheaper than hashing the name and probing the shared index for a
/// receiver of a few fields (a class instance with one field paid ~270
/// instructions per call through the index against ~25 for the compare), and
/// the index still answers for every wider receiver.
const DIRECT_SCAN_MAX_KEYS: u32 = 8;

/// The direct form of [`find_method_slot`]: the RAW dense slots of `keys`,
/// last occurrence first so a duplicate resolves as the index does. A keys
/// array is dense and holds only strings, so a hole or non-string slot simply
/// fails the byte compare.
///
/// # Safety
/// As [`find_method_slot`]: `keys` is a validated live keys array.
#[inline(always)]
unsafe fn scan_method_slot(keys: *const ArrayHeader, key_count: u32, name: &[u8]) -> Option<u32> {
    let (slots, slot_len) = keys_lookup::keys_array_dense_slots_resolved(keys);
    let mut i = (key_count as usize).min(slot_len);
    while i > 0 {
        i -= 1;
        let key = crate::JSValue::from_bits((*slots.add(i)).to_bits());
        if crate::string::js_string_key_matches_bytes(key, name) {
            return Some(i as u32);
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::object::{js_object_alloc_class_with_keys, object_keys};
    use crate::{closure, gc::RuntimeHandleScope, value, JSValue};

    extern "C" fn method(
        _closure: *const closure::ClosureHeader,
        this: closure::JsThis,
        _arg: f64,
    ) -> f64 {
        this.as_f64()
    }

    fn object(keys: &[&str]) -> *mut crate::object::ObjectHeader {
        let packed = keys.join("\0");
        js_object_alloc_class_with_keys(
            0,
            0,
            keys.len() as u32,
            packed.as_ptr(),
            packed.len() as u32,
        )
    }

    unsafe fn lookup(obj: *const crate::object::ObjectHeader, name: &[u8]) -> Option<u32> {
        let keys = object_keys(obj);
        find_method_slot(keys.arr(), keys.count(), name)
    }

    #[test]
    fn method_lookup_10502_index_respects_logical_prefix_and_last_duplicate() {
        let _lock = crate::gc::global_side_table_test_lock();
        let obj = object(&["m", "other", "m", "long_method_name"]);
        unsafe {
            let keys = object_keys(obj);
            assert_eq!(lookup(obj, b"m"), Some(2));
            assert_eq!(lookup(obj, b"long_method_name"), Some(3));
            // A shorter canonical list can share the indexed backing. The
            // longest list's duplicate must not hide its earlier occurrence.
            assert_eq!(find_method_slot(keys.arr(), 1, b"m"), Some(0));
            assert_eq!(find_method_slot(keys.arr(), 1, b"long_method_name"), None);
            assert_eq!(lookup(obj, b"missing"), None);
        }
    }

    #[test]
    fn method_lookup_wide_receivers_take_the_index_narrow_ones_the_scan() {
        let _lock = crate::gc::global_side_table_test_lock();
        let names: Vec<String> = (0..12).map(|i| format!("w{i}")).collect();
        let mut wide: Vec<&str> = names.iter().map(String::as_str).collect();
        wide.push("w3");
        let wide = object(&wide);
        let narrow = object(&["n0", "n1", "n0"]);
        unsafe {
            // 13 keys: past DIRECT_SCAN_MAX_KEYS, the index answers.
            assert_eq!(lookup(wide, b"w3"), Some(12));
            assert_eq!(lookup(wide, b"w11"), Some(11));
            assert_eq!(lookup(wide, b"missing"), None);
            // 3 keys: the direct compare answers, last occurrence first.
            assert_eq!(lookup(narrow, b"n0"), Some(2));
            assert_eq!(lookup(narrow, b"n1"), Some(1));
            assert_eq!(lookup(narrow, b"missing"), None);
        }
    }

    #[test]
    fn method_lookup_10502_mutations_invalidate_slot_answers() {
        let _lock = crate::gc::global_side_table_test_lock();
        let scope = RuntimeHandleScope::new();
        let recv = scope.root_nanbox_f64(value::js_nanbox_pointer(object(&["m", "x", "y"]) as i64));
        let key = crate::string::js_string_from_bytes(b"m".as_ptr(), 1);
        let key = scope.root_nanbox_f64(f64::from_bits(JSValue::string_ptr(key).bits()));
        unsafe {
            let ptr = || {
                JSValue::from_bits(recv.get_nanbox_u64())
                    .as_pointer::<crate::object::ObjectHeader>()
                    as *mut crate::object::ObjectHeader
            };
            assert_eq!(lookup(ptr(), b"m"), Some(0));
            assert_eq!(
                crate::object::js_object_delete_field(
                    ptr(),
                    JSValue::from_bits(key.get_nanbox_u64()).as_string_ptr()
                ),
                1
            );
            assert_eq!(lookup(ptr(), b"m"), None);
            crate::object::js_object_set_field_by_name(
                ptr(),
                JSValue::from_bits(key.get_nanbox_u64()).as_string_ptr(),
                42.0,
            );
            assert!(lookup(ptr(), b"m").is_some());
            assert!(lookup(ptr(), b"missing").is_none());
        }
    }

    #[test]
    fn method_lookup_10502_dispatch_reloads_method_and_binds_receiver() {
        let _lock = crate::gc::global_side_table_test_lock();
        let scope = RuntimeHandleScope::new();
        // Both sides of `DIRECT_SCAN_MAX_KEYS`: the direct compare and the index.
        for width in [4, 16, 64] {
            let names: Vec<String> = (0..width).map(|i| format!("f{i:03}")).collect();
            let keys: Vec<&str> = names.iter().map(String::as_str).collect();
            let recv = scope.root_nanbox_f64(value::js_nanbox_pointer(object(&keys) as i64));
            let callable = scope.root_nanbox_f64(value::js_nanbox_pointer(
                closure::js_closure_alloc(crate::fn_info!(method, 1; with_declared(1)), 0) as i64,
            ));
            let ptr = || {
                JSValue::from_bits(recv.get_nanbox_u64())
                    .as_pointer::<crate::object::ObjectHeader>()
                    as *mut crate::object::ObjectHeader
            };
            let method_name = names[width - 1].as_bytes();
            unsafe {
                crate::object::js_object_set_field(
                    ptr(),
                    (width - 1) as u32,
                    JSValue::from_bits(callable.get_nanbox_u64()),
                );
                for _ in 0..2 {
                    let args = [1.0];
                    let result = crate::object::js_native_call_method(
                        recv.get_nanbox_f64(),
                        method_name.as_ptr().cast(),
                        method_name.len(),
                        args.as_ptr(),
                        1,
                    );
                    assert_eq!(result.to_bits(), recv.get_nanbox_u64());
                }
                let replacement =
                    scope.root_nanbox_f64(value::js_nanbox_pointer(closure::js_closure_alloc(
                        crate::fn_info!(replacement_method, 1; with_declared(1)),
                        0,
                    ) as i64));
                crate::object::js_object_set_field(
                    ptr(),
                    (width - 1) as u32,
                    JSValue::from_bits(replacement.get_nanbox_u64()),
                );
                let args = [17.0];
                let result = crate::object::js_native_call_method(
                    recv.get_nanbox_f64(),
                    method_name.as_ptr().cast(),
                    method_name.len(),
                    args.as_ptr(),
                    1,
                );
                assert_eq!(result, 17.0);
            }
        }
    }

    #[test]
    fn method_lookup_10502_index_survives_moving_keys() {
        let _guard = crate::gc::CopyingNurseryTestGuard::new(0);
        let _triggers = crate::gc::GcTriggerThresholdTestGuard::suppress_automatic_triggers();
        let _evacuation = crate::gc::knob_overrides::ForcedEvacuationTestGuard::on();
        crate::gc::register_runtime_handle_root_scanner_for_tests();
        crate::gc::gc_register_mutable_root_scanner(crate::object::scan_object_cache_roots_mut);
        crate::gc::gc_register_mutable_root_scanner(crate::object::scan_shape_cache_roots_mut);
        crate::gc::gc_register_mutable_root_scanner(crate::object::scan_transition_cache_roots_mut);
        crate::gc::gc_register_mutable_root_scanner(shapes::scan_shape_table_rekey_mut);
        let scope = RuntimeHandleScope::new();
        // Past `DIRECT_SCAN_MAX_KEYS`: a narrower list is compared directly.
        for width in [16, 64] {
            let names: Vec<String> = (0..width).map(|i| format!("f{i:03}")).collect();
            let keys: Vec<&str> = names.iter().map(String::as_str).collect();
            let recv = scope.root_raw_mut_ptr(object(&keys));
            let before_keys = recv.with_const_ptr(|obj| unsafe {
                assert_eq!(
                    lookup(obj, names[width - 1].as_bytes()),
                    Some((width - 1) as u32)
                );
                object_keys(obj).arr()
            });
            crate::gc::gc_collect_minor();
            recv.with_const_ptr(|obj| unsafe {
                assert_ne!(
                    object_keys(obj).arr(),
                    before_keys,
                    "the indexed keys must actually move"
                );
                let before = crate::string::test_key_byte_reads();
                assert_eq!(
                    lookup(obj, names[width - 1].as_bytes()),
                    Some((width - 1) as u32)
                );
                assert_eq!(lookup(obj, b"missing"), None);
                assert_eq!(
                    crate::string::test_key_byte_reads() - before,
                    1,
                    "the moved index must be rekeyed, not rebuilt or scanned"
                );
            });
        }
    }

    extern "C" fn replacement_method(
        _closure: *const closure::ClosureHeader,
        _this: closure::JsThis,
        arg: f64,
    ) -> f64 {
        arg
    }
}
