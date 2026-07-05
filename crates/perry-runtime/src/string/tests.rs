//! Unit tests for the string runtime.
//!
//! Moved verbatim from the pre-split monolithic `string.rs`.

use super::intern::{with_intern_table, InternEntry, INTERN_TABLE_MASK};
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
