//! A bound Buffer-method closure must not capture a pointer into the key
//! string, or into any other storage the caller owns.
//!
//! `js_class_method_bind` stores the method-name POINTER in the closure and
//! `dispatch_bound_method` re-reads it at CALL time. Its contract says so:
//! "Method-name pointer is expected to be stable for the closure's lifetime;
//! codegen emits it from the per-module `.str.N.bytes` rodata global." Two
//! runtime callers on the Buffer path did not honour it:
//!
//! * `get_field_by_name_tail` derived `key_ptr` as `key + size_of::<StringHeader>()`
//!   — the INTERIOR of a movable GC heap string — and passed it straight
//!   through `buffer_own_prop_or_method`. Every `typeof buf.readUInt8` /
//!   `const f = buf.readUInt8` read produced a closure pointing into the
//!   nursery. Once that string moved under a copying minor (or was reclaimed,
//!   the string being unreachable after the read), the closure named freed or
//!   relocated bytes and the call dispatched on garbage.
//! * `polymorphic_index`'s computed-key arm (`buf[k]`) bound
//!   `name.as_bytes().as_ptr()` where `name` is a local `String` — freed on
//!   return, so the closure dangled before any collection was involved.
//!
//! The failure is invisible on a host whose allocator happens to leave the old
//! bytes intact, which is why it surfaced as a conformance-smoke SIGSEGV on
//! Linux while the same tests passed locally. So the assertions here are
//! STRUCTURAL — the captured pointer must not alias the key string at all —
//! rather than "does it happen to still read correctly after a collection",
//! which is exactly the question a lucky allocator answers wrong.

use super::support::*;

/// The name bytes a bound closure keeps, as raw parts.
unsafe fn captured_name(bound: crate::value::JSValue) -> (*const u8, usize) {
    let closure = crate::value::js_nanbox_get_pointer(f64::from_bits(bound.bits()))
        as *const crate::ClosureHeader;
    assert!(!closure.is_null(), "the read must produce a bound closure");
    let ptr = crate::closure::js_closure_get_capture_ptr(closure, 1) as *const u8;
    let len = crate::closure::js_closure_get_capture_ptr(closure, 2) as usize;
    (ptr, len)
}

/// ★ The regression. Reading a Buffer method as a VALUE must not hand the
/// closure the key string's interior.
#[test]
fn a_bound_buffer_method_never_captures_the_key_strings_interior() {
    let _guard = GcTestIsolationGuard::new();

    unsafe {
        let buf = crate::buffer::buffer_alloc(8);
        let key = crate::string::js_string_from_bytes(b"readUInt8".as_ptr(), 9);
        let key_interior = (key as *const u8).add(std::mem::size_of::<crate::StringHeader>());

        let bound =
            crate::object::js_object_get_field_by_name(buf as *const crate::ObjectHeader, key);
        let (name_ptr, name_len) = captured_name(bound);

        let static_name = crate::object::buffer_method_name_static("readUInt8")
            .expect("readUInt8 is a Buffer method");
        assert_eq!(
            name_ptr,
            static_name.as_ptr(),
            "the closure must capture the 'static literal"
        );
        assert_ne!(
            name_ptr, key_interior,
            "the closure captured the KEY STRING's interior — that allocation \
             is movable and unreachable after this read, so the name it \
             dispatches on is freed or relocated bytes"
        );
        assert_eq!(name_len, 9, "the captured name must still be `readUInt8`");
        assert_eq!(
            std::slice::from_raw_parts(name_ptr, name_len),
            b"readUInt8",
            "and it must spell the method"
        );
    }
}

/// The same contract for the computed-key arm (`buf[k]`), whose name came from
/// a local `String` — dangling on return with no collection required.
#[test]
fn a_computed_key_buffer_method_never_captures_a_temporary() {
    let _guard = GcTestIsolationGuard::new();

    unsafe {
        let buf = crate::buffer::buffer_alloc(8);
        let key = crate::string::js_string_from_bytes(b"readUInt8".as_ptr(), 9);
        let key_interior = (key as *const u8).add(std::mem::size_of::<crate::StringHeader>());
        let key_value = f64::from_bits(crate::value::js_nanbox_string(key as i64).to_bits());

        let obj_handle = crate::value::js_nanbox_pointer(buf as i64).to_bits() as i64;
        let bound = crate::value::JSValue::from_bits(
            crate::object::js_object_get_index_polymorphic(obj_handle, key_value).to_bits(),
        );
        let (name_ptr, name_len) = captured_name(bound);

        // Pointer IDENTITY with the static literal, not merely "not the key
        // string". The broken version of this path captured a local `String`'s
        // bytes, which are neither the key's interior nor the literal — so an
        // inequality against the key would pass with the bug fully present, and
        // comparing the BYTES only fails on a host where the freed memory has
        // already been reused. Identity is the assertion that cannot be lucky.
        let static_name = crate::object::buffer_method_name_static("readUInt8")
            .expect("readUInt8 is a Buffer method");
        assert_eq!(
            name_ptr,
            static_name.as_ptr(),
            "the computed-key arm must capture the 'static literal — anything \
             else is storage the caller owns and the closure outlives"
        );
        assert_ne!(
            name_ptr, key_interior,
            "and in particular not the key string's interior"
        );
        assert_eq!(
            std::slice::from_raw_parts(name_ptr, name_len),
            b"readUInt8",
            "the captured name must spell the method"
        );
    }
}

/// LIVENESS for the two above: they only mean something if the captured
/// pointer is genuinely stable, so pin the property the fix relies on — the
/// `'static` lookup returns the LITERAL out of its own list, never a borrow of
/// the caller's bytes. A future edit that "simplifies" it to `Some(name)`
/// compiles and passes every behavioural test on a lucky allocator; it fails
/// here.
#[test]
fn the_static_method_name_lookup_does_not_borrow_its_argument() {
    let owned = String::from("readUInt8");
    let found =
        crate::object::buffer_method_name_static(&owned).expect("readUInt8 is a Buffer method");

    assert_ne!(
        found.as_ptr(),
        owned.as_ptr(),
        "the lookup returned a borrow of its argument — the whole point is a \
         pointer that outlives the caller's storage"
    );
    assert_eq!(found, "readUInt8");
    assert_eq!(
        found.as_ptr(),
        crate::object::buffer_method_name_static("readUInt8")
            .unwrap()
            .as_ptr(),
        "every caller must get the SAME static literal, whatever storage its \
         own copy of the name lives in"
    );
    assert!(
        crate::object::buffer_method_name_static("notAMethod").is_none(),
        "and a non-method must not resolve"
    );
}
