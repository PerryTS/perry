//! Unit tests for the string runtime.
//!
//! Moved verbatim from the pre-split monolithic `string.rs`.

use super::intern::{with_intern_table, INTERN_TABLE_MASK};
use super::*;

fn malloc_object_count_for_test() -> usize {
    crate::gc::MALLOC_STATE.with(|s| s.borrow().objects.len())
}

unsafe fn gc_header_for_string(s: *const StringHeader) -> *const crate::gc::GcHeader {
    unsafe { (s as *const u8).sub(crate::gc::GC_HEADER_SIZE) as *const crate::gc::GcHeader }
}

fn fnv1a_for_test(bytes: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf29ce484222325;
    for &b in bytes {
        h ^= b as u64;
        h = h.wrapping_mul(0x100000001b3);
    }
    h
}

#[test]
fn test_string_create() {
    let data = b"hello";
    let s = js_string_from_bytes(data.as_ptr(), data.len() as u32);
    assert_eq!(js_string_length(s), 5);
}

#[test]
fn owned_string_bytes_copies_inline_and_spilled_payloads() {
    let short: &[u8] = b"short payload";
    let long = vec![b'x'; OwnedStringBytes::INLINE_CAPACITY + 1];
    for bytes in [short, long.as_slice()] {
        let header = js_string_from_bytes(bytes.as_ptr(), bytes.len() as u32);
        let owned = unsafe { OwnedStringBytes::copy_from_header(header) };
        assert_eq!(owned.as_bytes(), bytes);
    }
}

#[test]
fn rooted_string_bytes_rereads_the_handle_slot() {
    let first = js_string_from_bytes(b"before".as_ptr(), 6);
    let second = js_string_from_bytes(b"after".as_ptr(), 5);
    let scope = crate::gc::RuntimeHandleScope::new();
    let handle = scope.root_string_ptr(first);

    assert!(unsafe { handle.with_string_bytes(|bytes| bytes == b"before") });
    handle.set_raw_const_ptr(second);
    assert!(unsafe { handle.with_string_bytes(|bytes| bytes == b"after") });
}

#[test]
fn test_string_concat() {
    let a = js_string_from_bytes(b"hello".as_ptr(), 5);
    let b = js_string_from_bytes(b" world".as_ptr(), 6);
    let c = js_string_concat(a, b);
    assert_eq!(js_string_length(c), 11);
    assert_eq!(string_as_str(c), "hello world");
}

#[test]
fn short_boxed_strings_use_sso_without_malloc_tracking() {
    let before = malloc_object_count_for_test();
    let value = js_string_new_sso(b"abc".as_ptr(), 3);
    let after = malloc_object_count_for_test();
    let js_value = crate::value::JSValue::from_bits(value.to_bits());

    assert!(js_value.is_short_string());
    assert_eq!(after, before);
}

#[test]
fn dispatch_id_resolver_accepts_raw_heap_and_sso_string_forms() {
    fn bytes_from(id: i64) -> Vec<u8> {
        let mut scratch = [0u8; crate::value::SHORT_STRING_MAX_LEN];
        let resolved = perry_string_ref_from_dispatch_id(id, &mut scratch).unwrap();
        unsafe { std::slice::from_raw_parts(resolved.ptr, resolved.len).to_vec() }
    }

    let raw = js_string_from_bytes(b"score".as_ptr(), 5);
    assert_eq!(bytes_from(raw as i64), b"score");

    let boxed_heap = crate::value::JSValue::string_ptr(raw).bits() as i64;
    assert_eq!(bytes_from(boxed_heap), b"score");

    let boxed_sso = crate::value::JSValue::try_short_string(b"id")
        .unwrap()
        .bits() as i64;
    assert_eq!(bytes_from(boxed_sso), b"id");
}

#[test]
fn dispatch_id_resolver_accepts_static_rodata_descriptor_form() {
    let bytes = b"publish";
    let descriptor = StaticDispatchString {
        byte_len: bytes.len() as u32,
        flags: 0,
        hash: 0xe2bf_e841_1c47_2768,
        bytes: bytes.as_ptr(),
    };
    let id = STATIC_DISPATCH_TAG
        | ((&descriptor as *const StaticDispatchString as u64) & crate::value::POINTER_MASK);
    let mut scratch = [0u8; crate::value::SHORT_STRING_MAX_LEN];
    let resolved = perry_string_ref_from_dispatch_id(id as i64, &mut scratch).unwrap();
    assert!(resolved.heap.is_null());
    assert_eq!(
        unsafe { std::slice::from_raw_parts(resolved.ptr, resolved.len) },
        bytes
    );
}

#[test]
fn static_dispatch_key_materialization_is_cached_per_thread() {
    let bytes = b"publish";
    let descriptor = StaticDispatchString {
        byte_len: bytes.len() as u32,
        flags: 0,
        hash: 0xe2bf_e841_1c47_2768,
        bytes: bytes.as_ptr(),
    };
    let id = STATIC_DISPATCH_TAG
        | ((&descriptor as *const StaticDispatchString as u64) & crate::value::POINTER_MASK);
    let mut scratch = [0u8; crate::value::SHORT_STRING_MAX_LEN];
    let key = perry_string_ref_from_dispatch_id(id as i64, &mut scratch).unwrap();
    let first = materialize_dispatch_key(key);
    let second = materialize_dispatch_key(key);
    assert!(!first.is_null());
    assert_eq!(first, second);
}

#[test]
fn static_dispatch_key_materialization_preserves_wtf8_flag() {
    let bytes = b"\xED\xA0\x80";
    let descriptor = StaticDispatchString {
        byte_len: bytes.len() as u32,
        flags: STATIC_DISPATCH_FLAG_WTF8,
        hash: fnv1a_for_test(bytes),
        bytes: bytes.as_ptr(),
    };
    let id = STATIC_DISPATCH_TAG
        | ((&descriptor as *const StaticDispatchString as u64) & crate::value::POINTER_MASK);
    let mut scratch = [0u8; crate::value::SHORT_STRING_MAX_LEN];
    let key = perry_string_ref_from_dispatch_id(id as i64, &mut scratch).unwrap();
    let heap = materialize_dispatch_key(key);
    assert!(!heap.is_null());
    assert_ne!(
        unsafe { (*heap).flags } & STRING_FLAG_HAS_LONE_SURROGATES,
        0
    );
}

#[test]
fn small_and_medium_heap_strings_use_nursery_gc_pages() {
    let data = vec![b'x'; 1024];
    let before = malloc_object_count_for_test();
    let s = js_string_from_bytes(data.as_ptr(), data.len() as u32);
    let after = malloc_object_count_for_test();

    assert_eq!(after, before);
    assert_eq!(unsafe { (*s).byte_len }, data.len() as u32);
    assert_eq!(unsafe { (*s).flags }, 0);
    assert!(crate::arena::pointer_in_nursery(s as usize));
    assert!(!crate::arena::pointer_in_old_gen(s as usize));

    unsafe {
        let header = gc_header_for_string(s);
        assert_eq!((*header).obj_type, crate::gc::GC_TYPE_STRING);
        assert_ne!((*header).gc_flags & crate::gc::GC_FLAG_ARENA, 0);
        assert_eq!((*header).gc_flags & crate::gc::GC_FLAG_TENURED, 0);
    }
}

#[test]
fn large_heap_strings_use_old_gc_pages_without_malloc_tracking() {
    let len = crate::gc::LARGE_OBJECT_THRESHOLD_BYTES + 1;
    let data = vec![b'L'; len];
    let before = malloc_object_count_for_test();
    let s = js_string_from_bytes(data.as_ptr(), data.len() as u32);
    let after = malloc_object_count_for_test();

    assert_eq!(after, before);
    assert_eq!(unsafe { (*s).byte_len }, len as u32);
    assert_eq!(unsafe { (*s).flags }, 0);
    assert!(crate::arena::pointer_in_old_gen(s as usize));
    assert!(!crate::arena::pointer_in_nursery(s as usize));
    assert_eq!(string_as_str(s), std::str::from_utf8(&data).unwrap());

    unsafe {
        let header = gc_header_for_string(s);
        assert_eq!((*header).obj_type, crate::gc::GC_TYPE_STRING);
        assert_ne!((*header).gc_flags & crate::gc::GC_FLAG_ARENA, 0);
        assert_ne!((*header).gc_flags & crate::gc::GC_FLAG_TENURED, 0);
    }
}

#[test]
fn interned_strings_remain_scannable_and_content_equal() {
    let key = b"gc-managed-intern-key";
    let hash = fnv1a_for_test(key);
    let slot = (hash as usize) & INTERN_TABLE_MASK;
    let old_entry = with_intern_table(|t| unsafe { (*t)[slot] });

    let first = js_string_from_bytes(key.as_ptr(), key.len() as u32);
    let canonical = js_string_intern(first, hash);
    let second = js_string_from_bytes(key.as_ptr(), key.len() as u32);
    let reinterned = js_string_intern(second, hash);

    assert_eq!(canonical, first);
    assert_eq!(reinterned, canonical);
    assert_eq!(js_string_equals(canonical, second), 1);

    let mut scanned = false;
    scan_intern_table_roots(&mut |value| {
        let bits = value.to_bits();
        if (bits & !crate::value::POINTER_MASK) == crate::value::STRING_TAG
            && (bits & crate::value::POINTER_MASK) as usize == canonical as usize
        {
            scanned = true;
        }
    });
    assert!(scanned);

    unsafe {
        let header = gc_header_for_string(canonical);
        assert_ne!((*header).gc_flags & crate::gc::GC_FLAG_INTERNED, 0);
    }
    with_intern_table(|t| unsafe {
        (*t)[slot] = old_entry;
    });
}

#[test]
fn test_string_slice() {
    let s = js_string_from_bytes(b"hello world".as_ptr(), 11);
    let slice = js_string_slice(s, 0, 5);
    assert_eq!(string_as_str(slice), "hello");

    let slice2 = js_string_slice(s, 6, 11);
    assert_eq!(string_as_str(slice2), "world");
}

#[test]
fn test_string_index_of() {
    let s = js_string_from_bytes(b"hello world".as_ptr(), 11);
    let needle = js_string_from_bytes(b"world".as_ptr(), 5);
    assert_eq!(js_string_index_of(s, needle), 6);

    let not_found = js_string_from_bytes(b"xyz".as_ptr(), 3);
    assert_eq!(js_string_index_of(s, not_found), -1);
}

#[test]
fn test_string_last_index_of_from() {
    let s = js_string_from_bytes(b"abcabc".as_ptr(), 6);
    let c = js_string_from_bytes(b"c".as_ptr(), 1);
    // has_pos == 0 → search to the end (same as plain lastIndexOf).
    assert_eq!(js_string_last_index_of_from(s, c, 0.0, 0), 5);
    // Explicit position bounds the match start.
    assert_eq!(js_string_last_index_of_from(s, c, 3.0, 1), 2);
    assert_eq!(js_string_last_index_of_from(s, c, 0.0, 1), -1); // no 'c' at/before 0
    assert_eq!(js_string_last_index_of_from(s, c, 100.0, 1), 5); // clamp to end
    assert_eq!(js_string_last_index_of_from(s, c, -5.0, 1), -1); // negative → 0
                                                                 // Not found.
    let z = js_string_from_bytes(b"z".as_ptr(), 1);
    assert_eq!(js_string_last_index_of_from(s, z, 100.0, 1), -1);
    // Empty needle → min(position, length).
    let empty = js_string_from_bytes(b"".as_ptr(), 0);
    assert_eq!(js_string_last_index_of_from(s, empty, 2.0, 1), 2);
    assert_eq!(js_string_last_index_of_from(s, empty, 100.0, 1), 6);
}

#[test]
fn test_string_split() {
    use crate::array::{js_array_get_f64, js_array_length};

    let s = js_string_from_bytes(b"a,b,c".as_ptr(), 5);
    let delim = js_string_from_bytes(b",".as_ptr(), 1);
    let arr = js_string_split(s, delim);

    assert_eq!(js_array_length(arr), 3);
    // `split` produces a pointer-only result array. Its layout is recorded
    // once for the initialized prefix rather than through one side-table
    // update per string element.
    assert_eq!(
        crate::gc::test_layout_pointer_slot_count(arr as usize, 3),
        Some(3)
    );

    // Get the string pointers from the array and verify their contents
    // Note: split() stores NaN-boxed string pointers with STRING_TAG
    const POINTER_MASK: u64 = 0x0000_FFFF_FFFF_FFFF;

    unsafe {
        // Extract pointer from NaN-boxed value by masking off STRING_TAG
        let ptr0 = (js_array_get_f64(arr, 0).to_bits() & POINTER_MASK) as *const StringHeader;
        let ptr1 = (js_array_get_f64(arr, 1).to_bits() & POINTER_MASK) as *const StringHeader;
        let ptr2 = (js_array_get_f64(arr, 2).to_bits() & POINTER_MASK) as *const StringHeader;

        assert_eq!(string_as_str(ptr0), "a");
        assert_eq!(string_as_str(ptr1), "b");
        assert_eq!(string_as_str(ptr2), "c");
        assert_eq!((*ptr0).flags, 0);
        assert_eq!((*ptr1).flags, 0);
        assert_eq!((*ptr2).flags, 0);
    }

    // A later non-pointer write must conservatively drop the specialized
    // pointer-only layout instead of retaining stale pointer assumptions.
    crate::array::js_array_set_f64(arr, 1, 42.0);
    assert_eq!(
        crate::gc::test_layout_pointer_slot_count(arr as usize, 3),
        None
    );
}

#[test]
fn test_string_split_part_value() {
    let s = js_string_from_bytes(b"a,b,c".as_ptr(), 5);
    let delim = js_string_from_bytes(b",".as_ptr(), 1);
    let value = super::split::js_string_split_part_value(s, delim, 1);
    let ptr = (value.to_bits() & crate::value::POINTER_MASK) as *const StringHeader;
    assert_eq!(string_as_str(ptr), "b");
    assert_eq!(
        super::split::js_string_split_part_value(s, delim, 3).to_bits(),
        crate::value::TAG_UNDEFINED
    );
}

#[test]
fn test_string_split_part_utf16_length() {
    let ascii = js_string_from_bytes(b"a,bc,d".as_ptr(), 6);
    let comma = js_string_from_bytes(b",".as_ptr(), 1);
    assert_eq!(
        super::split::js_string_split_part_utf16_length(ascii, comma, 1),
        2.0
    );
    assert_eq!(
        super::split::js_string_split_part_utf16_length(ascii, comma, 3),
        0.0
    );
    let trailing = js_string_from_bytes(b"a,".as_ptr(), 2);
    assert_eq!(
        super::split::js_string_split_part_utf16_length(trailing, comma, 1),
        0.0
    );

    let unicode = js_string_from_str("a,😀,d");
    assert_eq!(
        super::split::js_string_split_part_utf16_length(unicode, comma, 1),
        2.0
    );

    let multi = js_string_from_bytes(b"a--bc".as_ptr(), 5);
    let double_dash = js_string_from_bytes(b"--".as_ptr(), 2);
    assert_eq!(
        super::split::js_string_split_part_utf16_length(multi, double_dash, 1),
        2.0
    );
}

#[test]
fn test_scalar_split_parts_derive_malformed_metadata_from_part_bytes() {
    let source_bytes = [0x80u8, b'|', 0xF0];
    let source = js_string_from_bytes(source_bytes.as_ptr(), source_bytes.len() as u32);
    let delimiter = js_string_from_bytes(b"|".as_ptr(), 1);

    assert_eq!(
        super::split::js_string_split_part_utf16_length(source, delimiter, 0),
        0.0
    );
    assert_eq!(
        super::split::js_string_split_part_utf16_length(source, delimiter, 1),
        2.0
    );

    let value = super::split::js_string_split_part_value(source, delimiter, 1);
    let part = crate::value::js_nanbox_get_pointer(value) as *const StringHeader;
    let bytes = unsafe { slice::from_raw_parts(string_data(part), (*part).byte_len as usize) };
    assert_eq!(bytes, &[0xF0]);
    assert_eq!(unsafe { (*part).utf16_len }, 2);
}

#[test]
fn test_scalar_split_part_value_preserves_lone_surrogate_flag() {
    let source_bytes = [0xEDu8, 0xA0, 0x80, b'|', b'A'];
    let source = js_string_from_wtf8_bytes(source_bytes.as_ptr(), source_bytes.len() as u32);
    let delimiter = js_string_from_bytes(b"|".as_ptr(), 1);

    let value = super::split::js_string_split_part_value(source, delimiter, 0);
    let part = crate::value::js_nanbox_get_pointer(value) as *const StringHeader;
    assert_eq!(
        unsafe { (*part).flags & STRING_FLAG_HAS_LONE_SURROGATES },
        STRING_FLAG_HAS_LONE_SURROGATES
    );
}

#[test]
fn test_uppercase_split_length_and_index_of_without_intermediate_string() {
    let dash = js_string_from_bytes(b"-".as_ptr(), 1);
    let ascii = js_string_from_bytes(b"item-9".as_ptr(), 6);
    let nine = js_string_from_bytes(b"9".as_ptr(), 1);
    assert_eq!(
        super::split::js_string_to_upper_case_split_part_utf16_length(ascii, dash, 1),
        1.0
    );
    assert_eq!(
        super::slice_ops::js_string_to_upper_case_index_of(ascii, nine),
        5
    );

    let unicode = js_string_from_str("straße-😀");
    let ss = js_string_from_bytes(b"SS".as_ptr(), 2);
    assert_eq!(
        super::split::js_string_to_upper_case_split_part_utf16_length(unicode, dash, 1),
        2.0
    );
    assert_eq!(
        super::slice_ops::js_string_to_upper_case_index_of(unicode, ss),
        4
    );

    let malformed_bytes = [b'a', b'-', 0xF0];
    let malformed = js_string_from_bytes(malformed_bytes.as_ptr(), malformed_bytes.len() as u32);
    assert_eq!(
        super::split::js_string_to_upper_case_split_part_utf16_length(malformed, dash, 1),
        2.0
    );
}

#[test]
fn test_string_append_inplace() {
    // First append: creates new string with 2x capacity and refcount=1
    let a = js_string_from_bytes(b"hello".as_ptr(), 5);
    let b = js_string_from_bytes(b" world".as_ptr(), 6);
    let result = js_string_append(a, b);
    assert_eq!(string_as_str(result), "hello world");
    assert_eq!(unsafe { (*result).refcount }, 1); // uniquely owned
    assert!(unsafe { (*result).capacity } >= 22); // 2x capacity

    // Second append: should reuse same allocation (in-place)
    let c = js_string_from_bytes(b"!".as_ptr(), 1);
    let result2 = js_string_append(result, c);
    assert_eq!(result2, result); // Same pointer — in-place append!
    assert_eq!(string_as_str(result2), "hello world!");
    assert_eq!(unsafe { (*result2).refcount }, 1); // still uniquely owned
}

#[test]
fn test_string_append_shared_no_inplace() {
    // Create a string via append (refcount=1)
    let a = js_string_from_bytes(b"hello".as_ptr(), 5);
    let b = js_string_from_bytes(b" ".as_ptr(), 1);
    let result = js_string_append(a, b);
    assert_eq!(unsafe { (*result).refcount }, 1);

    // Mark as shared (simulates `let y = x` in codegen)
    js_string_addref(result);
    assert_eq!(unsafe { (*result).refcount }, 0); // shared

    // Append should NOT be in-place — must allocate fresh
    let c = js_string_from_bytes(b"world".as_ptr(), 5);
    let result2 = js_string_append(result, c);
    assert_ne!(result2, result); // Different pointer — allocated fresh
    assert_eq!(string_as_str(result2), "hello world");
    assert_eq!(string_as_str(result), "hello "); // Original unchanged
}

#[test]
fn test_string_append_empty_reuses_only_shared_source() {
    let empty = js_string_from_bytes(b"".as_ptr(), 0);
    let shared = js_string_from_bytes(b"suffix".as_ptr(), 6);
    assert_eq!(unsafe { (*shared).refcount }, 0);

    let reused = js_string_append(empty, shared);
    assert_eq!(reused, shared);
    assert_eq!(unsafe { (*reused).refcount }, 0);

    // A later append must allocate instead of changing the aliased source.
    let bang = js_string_from_bytes(b"!".as_ptr(), 1);
    let grown = js_string_append(reused, bang);
    assert_ne!(grown, shared);
    assert_eq!(string_as_str(shared), "suffix");
    assert_eq!(string_as_str(grown), "suffix!");

    // A unique source cannot be reused: doing so would silently create a
    // second owner while leaving its in-place mutation permission intact.
    let a = js_string_from_bytes(b"a".as_ptr(), 1);
    let b = js_string_from_bytes(b"b".as_ptr(), 1);
    let unique = js_string_append(a, b);
    assert_eq!(unsafe { (*unique).refcount }, 1);
    let empty2 = js_string_from_bytes(b"".as_ptr(), 0);
    let copied = js_string_append(empty2, unique);
    assert_ne!(copied, unique);
    assert_eq!(string_as_str(copied), "ab");
    assert_eq!(string_as_str(unique), "ab");

    // The tag-checked codegen entry point applies the same ownership rule.
    let empty3 = js_string_from_bytes(b"".as_ptr(), 0);
    assert_eq!(js_string_append_known_heap(empty3, shared), shared);
    let empty4 = js_string_from_bytes(b"".as_ptr(), 0);
    let copied_known = js_string_append_known_heap(empty4, unique);
    assert_ne!(copied_known, unique);
    assert_eq!(string_as_str(copied_known), "ab");
}

#[test]
fn test_string_append_self() {
    // Self-append (s += s) must always allocate fresh
    let a = js_string_from_bytes(b"ab".as_ptr(), 2);
    let result = js_string_append(a, a);
    assert_eq!(string_as_str(result), "abab");
}

#[test]
fn test_string_append_loop() {
    // Simulate the common loop pattern: result = result + "x" repeated
    let mut result = js_string_from_bytes(b"".as_ptr(), 0);
    let x = js_string_from_bytes(b"x".as_ptr(), 1);
    let mut inplace_count = 0u32;
    for _ in 0..1000 {
        let old_ptr = result;
        result = js_string_append(result, x);
        if result == old_ptr {
            inplace_count += 1;
        }
    }
    assert_eq!(js_string_length(result), 1000);
    // Most appends should be in-place (only ~10 re-allocations for 1000 appends)
    assert!(
        inplace_count > 980,
        "Expected >980 in-place appends, got {}",
        inplace_count
    );
}

#[test]
fn string_append_chain_owns_one_result_and_reuses_capacity() {
    fn boxed(s: *mut StringHeader) -> f64 {
        f64::from_bits(crate::value::STRING_TAG | (s as u64 & crate::value::POINTER_MASK))
    }
    fn heap(text: &str) -> *mut StringHeader {
        js_string_from_bytes(text.as_ptr(), text.len() as u32)
    }

    let first_parts = [
        boxed(heap("")),
        boxed(heap("[")),
        boxed(heap("n")),
        boxed(heap("]")),
    ];
    let first = js_string_append_chain(first_parts.as_ptr(), first_parts.len() as i32);
    assert_eq!(string_as_str(first), "[n]");
    assert_eq!(unsafe { (*first).refcount }, 1);
    assert_eq!(unsafe { (*first).capacity }, 3);

    let second_parts = [
        boxed(first),
        boxed(heap("[")),
        boxed(heap("fib")),
        boxed(heap("]")),
    ];
    let second = js_string_append_chain(second_parts.as_ptr(), second_parts.len() as i32);
    assert_ne!(second, first);
    assert_eq!(string_as_str(second), "[n][fib]");
    assert_eq!(unsafe { (*second).refcount }, 1);
    assert!(unsafe { (*second).capacity } >= 32);

    let third_parts = [
        boxed(second),
        boxed(heap("[")),
        boxed(heap("x")),
        boxed(heap("]")),
    ];
    let third = js_string_append_chain(third_parts.as_ptr(), third_parts.len() as i32);
    assert_eq!(third, second);
    assert_eq!(string_as_str(third), "[n][fib][x]");
}

#[test]
fn string_append_chain_falls_back_for_overlap_and_dynamic_parts() {
    fn boxed(s: *mut StringHeader) -> f64 {
        f64::from_bits(crate::value::STRING_TAG | (s as u64 & crate::value::POINTER_MASK))
    }

    let value = js_string_from_bytes(b"ab".as_ptr(), 2);
    let overlap = [boxed(value), boxed(value)];
    let doubled = js_string_append_chain(overlap.as_ptr(), overlap.len() as i32);
    assert_ne!(doubled, value);
    assert_eq!(string_as_str(value), "ab");
    assert_eq!(string_as_str(doubled), "abab");

    let suffix = js_string_from_bytes(b"x".as_ptr(), 1);
    let dynamic = [42.0, boxed(suffix)];
    let joined = js_string_append_chain(dynamic.as_ptr(), dynamic.len() as i32);
    assert_eq!(string_as_str(joined), "42x");
}

// ── Repsel Phase 3a: js_string_compare_value ───────────────────────────────

#[test]
fn string_compare_value_heap_and_sso_mixes() {
    use super::compare::js_string_compare_value;
    let heap = |s: &str| {
        let p = js_string_from_bytes(s.as_ptr(), s.len() as u32);
        f64::from_bits(crate::value::JSValue::string_ptr(p).bits())
    };
    let sso = |s: &str| {
        f64::from_bits(
            crate::value::JSValue::try_short_string(s.as_bytes())
                .expect("<=5 bytes")
                .bits(),
        )
    };
    // heap × heap
    assert_eq!(js_string_compare_value(heap("abc"), heap("abd")), -1);
    assert_eq!(js_string_compare_value(heap("abc"), heap("abc")), 0);
    // SSO × SSO
    assert_eq!(js_string_compare_value(sso("ab"), sso("ac")), -1);
    assert_eq!(js_string_compare_value(sso("ab"), sso("ab")), 0);
    assert_eq!(js_string_compare_value(sso("b"), sso("a")), 1);
    // mixed representations, equal content
    assert_eq!(js_string_compare_value(sso("ok"), heap("ok")), 0);
    assert_eq!(js_string_compare_value(heap("ok"), sso("oz")), -1);
    // astral vs BMP: UTF-16 code-unit order, not code-point order
    assert_eq!(
        js_string_compare_value(heap("\u{1F600}"), heap("\u{FFFD}")),
        -1
    );
    // number operand coerces via its decimal string form (legacy unified
    // behavior this helper's arm replaces) — both orders and both string
    // representations, exercising the "allocating coercions complete before
    // any heap-payload view is taken" phase split (the number path calls
    // js_number_to_string, which allocates and may move the other operand's
    // heap string under evacuation).
    assert_eq!(js_string_compare_value(42.0, heap("42")), 0);
    assert_eq!(js_string_compare_value(heap("42"), 42.0), 0);
    assert_eq!(js_string_compare_value(42.0, heap("5")), -1);
    assert_eq!(js_string_compare_value(heap("5"), 42.0), 1);
    assert_eq!(js_string_compare_value(42.0, sso("42")), 0);
    assert_eq!(js_string_compare_value(sso("41"), 42.0), -1);
    assert_eq!(js_string_compare_value(1.5, 2.5), -1); // both numbers coerce
                                                       // non-string, non-number operands rank as invalid
    let undef = f64::from_bits(crate::value::JSValue::undefined().bits());
    assert_eq!(js_string_compare_value(undef, heap("x")), -1);
    assert_eq!(js_string_compare_value(heap("x"), undef), 1);
    assert_eq!(js_string_compare_value(undef, undef), 0);
}

// ── js_string_concat_box's non-string operand delegates ────────────────────

/// A `string`-declared operand that holds something else at runtime must get
/// the full dynamic `+`, not silently vanish.
///
/// Perry does not validate declared types at runtime, so the codegen's
/// static string proof (`is_definitely_string_expr`) is a claim about an
/// ANNOTATION. This helper used to treat an operand it could not decode as
/// the empty string, which made `"ab" + 42` render as `"ab"` — a silent wrong
/// answer, and the reason the concat fast path had to be withheld from every
/// declaration-based proof. Delegating instead is what lets the proof be a
/// performance decision that cannot change a program's output.
#[test]
fn concat_box_delegates_a_non_string_operand_to_the_dynamic_add() {
    let heap = |s: &str| {
        let p = js_string_from_bytes(s.as_ptr(), s.len() as u32);
        f64::from_bits(crate::value::JSValue::string_ptr(p).bits())
    };
    let text = |v: f64| {
        let p = crate::value::js_jsvalue_to_string(v);
        let bytes = unsafe { std::slice::from_raw_parts(string_data(p), (*p).byte_len as usize) };
        String::from_utf8(bytes.to_vec()).expect("ascii")
    };

    // string + number → concatenation with the number's decimal form.
    assert_eq!(text(js_string_concat_box(heap("ab"), 42.0)), "ab42");
    // number + string → same, other order.
    assert_eq!(text(js_string_concat_box(42.0, heap("ab"))), "42ab");
    // Both operands lying: `+` is then plain numeric addition, and the result
    // is a NUMBER, not a string. This is the arm the old `unwrap_or` answered
    // with the empty string.
    assert_eq!(js_string_concat_box(40.0, 2.0), 42.0);
    // An int32-tagged operand must decode to its value, not to its boxed bits.
    let int42 = f64::from_bits(crate::value::JSValue::int32(42).bits());
    assert_eq!(text(js_string_concat_box(heap("n="), int42)), "n=42");
    // undefined / null keep their ToString forms.
    let undef = f64::from_bits(crate::value::JSValue::undefined().bits());
    assert_eq!(text(js_string_concat_box(heap("v:"), undef)), "v:undefined");

    // The all-strings path is untouched, including the SSO result encoding.
    let sso_result = js_string_concat_box(heap("a"), heap("b"));
    assert_eq!(text(sso_result), "ab");
    assert!(
        crate::value::JSValue::from_bits(sso_result.to_bits()).is_short_string(),
        "a 2-byte ASCII result must still be assembled inline as SSO"
    );
}

// ---------------------------------------------------------------- #7837

/// NaN-box a heap string, the way codegen's `nanbox_string_inline` does.
fn boxed_heap(s: &str) -> f64 {
    let h = js_string_from_bytes(s.as_ptr(), s.len() as u32);
    crate::value::js_nanbox_string(h as i64)
}

fn boxed_text(v: f64) -> String {
    let p = crate::value::js_jsvalue_to_string(v);
    string_as_str(p).to_string()
}

#[test]
fn string_add_value_picks_the_operator_from_the_bits() {
    // #7837 defect 1. `js_string_concat_value` takes an already-unboxed
    // `StringHeader*`, so it cannot tell a lie from a string; these two take
    // the NaN-box precisely so they can.
    unsafe {
        // A real string on the declared side concatenates, whatever the other
        // operand holds — including a heap string, an SSO string and a number.
        assert_eq!(
            boxed_text(js_string_add_value(boxed_heap("ab"), 5.0)),
            "ab5"
        );
        assert_eq!(
            boxed_text(js_value_add_string(5.0, boxed_heap("ab"))),
            "5ab"
        );
        let sso = js_string_new_sso(b"ab".as_ptr(), 2);
        assert_eq!(boxed_text(js_string_add_value(sso, 5.0)), "ab5");
        assert_eq!(boxed_text(js_value_add_string(5.0, sso)), "5ab");
        assert_eq!(
            boxed_text(js_string_add_value(boxed_heap("ab"), boxed_heap("cd"))),
            "abcd"
        );

        // A LIE on the declared side is a numeric add, not a concatenation:
        // `const s: string = (42 as any); s + 7` is 49 in Node, and was "427".
        assert_eq!(js_string_add_value(42.0, 7.0), 49.0);
        assert_eq!(js_value_add_string(7.0, 42.0), 49.0);
        // ...and still concatenates when the OTHER operand is a real string.
        assert_eq!(
            boxed_text(js_string_add_value(42.0, boxed_heap("x"))),
            "42x"
        );
        assert_eq!(
            boxed_text(js_value_add_string(boxed_heap("x"), 42.0)),
            "x42"
        );
    }
}

/// #7912: the unrooted `js_string_concat_chain` fast path.
///
/// The change it covers replaces ~2N transient-handle round trips per chain
/// with a proof: `string_storage_alloc_no_collect` returns `Some` only when
/// the nursery block that was already open could serve the request, and that
/// is the one arena path that precedes `gc_check_trigger`. These tests hold
/// both halves — that the answer is unchanged, and that the premise the
/// answer rests on is actually true at run time.
mod concat_chain_no_collect {
    use super::super::concat::CONCAT_CHAIN_NO_COLLECT_HITS;
    use super::*;

    fn hits() -> u64 {
        CONCAT_CHAIN_NO_COLLECT_HITS.with(|c| c.get())
    }

    fn heap(text: &str) -> f64 {
        let p = js_string_from_bytes(text.as_ptr(), text.len() as u32);
        f64::from_bits(crate::value::STRING_TAG | (p as u64 & 0x0000_FFFF_FFFF_FFFF))
    }

    fn chain(parts: &[f64]) -> *mut StringHeader {
        js_string_concat_chain(parts.as_ptr(), parts.len() as i32)
    }

    fn text(s: *mut StringHeader) -> String {
        string_as_str(s).to_string()
    }

    /// The exact shape a tree-walking interpreter's environment lookup emits:
    /// `seen = seen + "[" + names[i] + "]"`, four heap-string parts, run in a
    /// loop so the accumulator grows.
    #[test]
    fn four_heap_string_parts_take_the_unrooted_path_and_answer_correctly() {
        let before = hits();
        let mut acc = heap("");
        let mut expected = String::new();
        for name in ["n", "fib", "go", "cat"] {
            acc = {
                let joined = chain(&[acc, heap("["), heap(name), heap("]")]);
                expected = format!("{expected}[{name}]");
                assert_eq!(text(joined), expected);
                f64::from_bits(crate::value::STRING_TAG | (joined as u64 & 0x0000_FFFF_FFFF_FFFF))
            };
        }
        assert!(
            hits() >= before + 4,
            "every all-heap-string chain must take the unrooted path: {} -> {}",
            before,
            hits()
        );
    }

    /// The safety premise, asserted rather than assumed: a fast-path chain
    /// reaches ZERO allocation-point GC triggers. If it ever reached one, the
    /// raw part pointers read in the sizing loop could have been moved out
    /// from under the copy loop — which is precisely the bug the transient
    /// handles used to prevent.
    #[test]
    fn the_unrooted_path_reaches_no_collection_point() {
        // Warm the block so the very first allocation of the test is not the
        // one that installs a fresh one.
        let _ = chain(&[heap("warm"), heap("up")]);
        crate::arena::reset_gc_trigger_arena_probe();
        let triggers_before = crate::arena::gc_trigger_arena_calls();
        let hits_before = hits();

        let joined = chain(&[heap("a"), heap("bb"), heap("ccc")]);
        assert_eq!(text(joined), "abbccc");

        assert!(
            hits() > hits_before,
            "test premise: the chain must have taken the unrooted path"
        );
        assert_eq!(
            crate::arena::gc_trigger_arena_calls(),
            triggers_before,
            "the unrooted path must not reach an allocation-point collection"
        );
    }

    /// A part that is not a live heap string (SSO, a number, `undefined`)
    /// needs `js_jsvalue_to_string`, which allocates — so it must fall back
    /// to the rooted path, and still answer correctly.
    #[test]
    fn non_heap_string_parts_fall_back_to_the_rooted_path() {
        let sso = js_string_new_sso(b"ab".as_ptr(), 2);
        for (parts, want) in [
            (vec![heap("x"), 7.0, heap("y")], "x7y"),
            (vec![heap("x"), sso, heap("y")], "xaby"),
            (
                vec![heap("v="), f64::from_bits(crate::value::TAG_UNDEFINED)],
                "v=undefined",
            ),
            (vec![7.0, 8.0], "78"),
        ] {
            let before = hits();
            assert_eq!(text(chain(&parts)), want);
            assert_eq!(
                hits(),
                before,
                "a non-heap-string part must not take the unrooted path: {want}"
            );
        }
    }

    /// An EMPTY part contributes no bytes AND no flags — the rooted loop ORs
    /// `piece_flags` inside its `blen > 0` guard, and a fast path that
    /// diverged there would change WTF-8 behaviour silently rather than
    /// visibly.
    #[test]
    fn empty_parts_contribute_neither_bytes_nor_flags() {
        let empty = heap("");
        let joined = chain(&[empty, heap("a"), empty, heap("b"), empty]);
        assert_eq!(text(joined), "ab");
        unsafe {
            assert_eq!((*joined).byte_len, 2);
            assert_eq!((*joined).utf16_len, 2);
            assert_eq!((*joined).flags & STRING_FLAG_HAS_LONE_SURROGATES, 0);
        }
    }

    /// Multi-byte and surrogate handling survives the fast path: `utf16_len`
    /// is summed from the parts, and an adjacent high→low pair produced by
    /// the JOIN is still canonicalised to its astral form.
    #[test]
    fn utf16_length_and_surrogate_canonicalisation_survive_the_fast_path() {
        let joined = chain(&[heap("é"), heap("漢"), heap("ab")]);
        assert_eq!(text(joined), "é漢ab");
        unsafe {
            assert_eq!((*joined).utf16_len, 4);
            assert_eq!((*joined).byte_len, 2 + 3 + 2);
        }

        let high = js_string_from_char_code(0xD83D as f64);
        let low = js_string_from_char_code(0xDE00 as f64);
        let hi_box =
            f64::from_bits(crate::value::STRING_TAG | (high as u64 & 0x0000_FFFF_FFFF_FFFF));
        let lo_box =
            f64::from_bits(crate::value::STRING_TAG | (low as u64 & 0x0000_FFFF_FFFF_FFFF));
        let merged = chain(&[hi_box, lo_box]);
        assert_eq!(text(merged), "\u{1F600}");
        unsafe {
            assert_eq!((*merged).utf16_len, 2);
        }
    }

    /// The REAL fallback: not "a part was a number", but "the open nursery
    /// block could not serve the result". Driven on its own thread so filling
    /// the block cannot leak into the rest of the suite.
    ///
    /// This is the arm that used to be reachable only in production. It has to
    /// answer identically, because a refusal is not an event — nothing has
    /// collected at that point, so the rooted path re-reads its operands from
    /// the same `parts` array and gets the same pointers.
    #[test]
    fn a_full_block_falls_back_to_the_rooted_path_with_the_same_answer() {
        std::thread::spawn(|| {
            // Fill the open block through the same no-collect entry the concat
            // uses, so the very next chain is guaranteed to be refused.
            // Build the operands FIRST — `js_string_from_bytes` goes through
            // the COLLECTING entry and would install a fresh block, undoing
            // the fill.
            let parts = [heap("a"), heap("bb"), heap("ccc"), heap("dddd")];

            // Now fill through the no-collect entry, which by construction
            // installs nothing and moves nothing, so `parts` stays valid.
            // Coarse-to-fine, because refusing a 4 KB request only proves
            // there is less than 4 KB left — and a 4-part chain of 10 bytes
            // fits in 40.
            let mut filled = false;
            for chunk in [crate::gc::LARGE_OBJECT_THRESHOLD_BYTES / 4, 256, 8] {
                let bound = 8 * 1024 * 1024 / chunk;
                filled = false;
                for _ in 0..bound {
                    if crate::arena::arena_alloc_gc_no_collect(chunk, 8, crate::gc::GC_TYPE_STRING)
                        .is_null()
                    {
                        filled = true;
                        break;
                    }
                }
                assert!(filled, "test premise: {chunk} B fill never refused");
            }
            assert!(filled, "test premise: the block must actually be full");

            let before = hits();
            let joined = chain(&parts);
            assert_eq!(text(joined), "abbcccdddd");
            assert_eq!(
                hits(),
                before,
                "with the block full the chain must have taken the ROOTED path"
            );
        })
        .join()
        .expect("full-block fallback test panicked");
    }

    /// The 4/8/32 scratch-size dispatch all route through the same fast path.
    #[test]
    fn every_scratch_size_class_takes_the_fast_path() {
        for n in [1usize, 2, 4, 5, 8, 9, 16, 32] {
            let parts: Vec<f64> = (0..n).map(|i| heap(&format!("{i}"))).collect();
            let want: String = (0..n).map(|i| i.to_string()).collect();
            let before = hits();
            assert_eq!(text(chain(&parts)), want, "n={n}");
            assert!(hits() > before, "n={n} must take the unrooted path");
        }
    }
}

/// The short-concat memo: `"prefix" + smallint` results are content-addressed,
/// so two evaluations producing the same bytes share one string object.
///
/// This asserts OBJECT IDENTITY, not equality. Equality would pass whether or
/// not the memo fires, which would make it a test that cannot fail — the whole
/// point is proving the allocation was skipped.
#[test]
fn concat_memo_returns_one_object_for_equal_results() {
    let _lock = crate::gc::global_side_table_test_lock();
    crate::string::concat::test_clear_concat_memo();

    let prefix = crate::string::js_string_from_bytes(b"field_".as_ptr(), 6);
    let first = crate::string::js_string_concat_value(prefix, 7.0);
    // A DIFFERENT prefix object with the same bytes must still reach the entry:
    // the memo is keyed on result content, not on operand identity.
    let other_prefix = crate::string::js_string_from_bytes(b"field_".as_ptr(), 6);
    assert_ne!(prefix as usize, other_prefix as usize);
    let second = crate::string::js_string_concat_value(other_prefix, 7.0);

    assert_eq!(
        first as usize, second as usize,
        "equal concat results must share one memoized string"
    );
    unsafe {
        assert_eq!((*first).byte_len, 7);
        // Shared, so the in-place `+=` append can never mutate it under a
        // second holder — the property that makes sharing sound at all.
        assert_eq!((*first).refcount, 0);
        let bytes = std::slice::from_raw_parts(crate::string::string_data(first), 7);
        assert_eq!(bytes, b"field_7");
    }
}

/// Distinct results must not alias, including across the memo's hash: a
/// collision has to degrade to a miss, never to a wrong string.
#[test]
fn concat_memo_never_aliases_distinct_results() {
    let _lock = crate::gc::global_side_table_test_lock();
    crate::string::concat::test_clear_concat_memo();

    let prefix = crate::string::js_string_from_bytes(b"field_".as_ptr(), 6);
    let mut seen: Vec<(usize, String)> = Vec::new();
    for n in 0..40u32 {
        let s = crate::string::js_string_concat_value(prefix, n as f64);
        let text = unsafe {
            let b =
                std::slice::from_raw_parts(crate::string::string_data(s), (*s).byte_len as usize);
            String::from_utf8_lossy(b).into_owned()
        };
        assert_eq!(text, format!("field_{n}"));
        for (other_ptr, other_text) in &seen {
            if *other_text != text {
                assert_ne!(
                    *other_ptr, s as usize,
                    "distinct results {other_text} and {text} must not share an object"
                );
            }
        }
        seen.push((s as usize, text));
    }
}

/// A non-ASCII prefix is excluded: its `utf16_len` differs from `byte_len`, so
/// a memoized result would carry the wrong `.length`.
#[test]
fn concat_memo_declines_non_ascii_prefixes() {
    let _lock = crate::gc::global_side_table_test_lock();
    crate::string::concat::test_clear_concat_memo();

    let prefix = crate::string::js_string_from_bytes("é_".as_bytes().as_ptr(), 3);
    let a = crate::string::js_string_concat_value(prefix, 1.0);
    let b = crate::string::js_string_concat_value(prefix, 1.0);
    assert_ne!(
        a as usize, b as usize,
        "a non-ASCII prefix must not be memoized"
    );
    unsafe {
        // "é_1" is 4 bytes but 3 UTF-16 units; the memo path would have
        // asserted byte_len == utf16_len.
        assert_eq!((*a).byte_len, 4);
        assert_eq!((*a).utf16_len, 3);
    }
}

/// #9391: the memo retires itself on a workload it never helps.
///
/// `bench_gc_pressure` builds 500k DISTINCT short strings, so every concat paid
/// a hash, a lookup miss and a rooted insert for a ~0% hit rate — measured at
/// 11 -> 17 ms when the memo landed. After `CONCAT_MEMO_TRIAL` attempts a hit
/// rate below one-in-eight turns it off, while a reusing workload keeps it.
///
/// The assertion is on the memo's OWN state rather than on a timing: a
/// throughput test would be flaky, and the decision is what this change makes.
#[test]
fn concat_memo_retires_on_an_all_distinct_workload() {
    let _lock = crate::gc::global_side_table_test_lock();
    crate::string::concat::test_clear_concat_memo();

    // All-distinct short results: the shape the memo cannot serve.
    for i in 0..(crate::string::concat::test_concat_memo_trial() + 64) {
        let s = crate::string::js_string_from_bytes(
            format!("item_{i}").as_ptr(),
            format!("item_{i}").len() as u32,
        );
        std::hint::black_box(s);
        crate::string::concat::test_note_concat_memo_miss();
    }
    assert!(
        crate::string::concat::test_concat_memo_retired(),
        "an all-distinct workload must retire the memo; otherwise every concat \
         keeps paying a hash + miss + rooted insert for no hits"
    );

    // A reusing workload must NOT retire it.
    crate::string::concat::test_clear_concat_memo();
    for _ in 0..(crate::string::concat::test_concat_memo_trial() + 64) {
        crate::string::concat::test_note_concat_memo_hit();
    }
    assert!(
        !crate::string::concat::test_concat_memo_retired(),
        "a workload that hits must keep the memo — this is what preserves the \
         bench_object_property win (36 -> 25 ms) the memo exists for"
    );
}
