//! Lifecycle proof for the per-site concat cache (`string/concat_site.rs`).
//!
//! The table's contract ("a non-zero slot `k` is the live heap string
//! `prefix + String(k)`") rests on one call: the filling arm registering the
//! slot as a global root. Drop that call and
//! `test_filled_slot_is_rewritten_by_a_copied_minor` fails — the string moves
//! and the slot keeps its pre-move address.

use super::super::*;
use super::support::*;
use crate::string::{js_string_concat_site_value, js_string_from_bytes, CONCAT_SITE_SLOTS};
use crate::value::{POINTER_MASK, STRING_TAG};

fn fresh_table() -> &'static mut [u64; CONCAT_SITE_SLOTS] {
    // Leaked on purpose: a filled slot's address becomes a GC root, and the
    // guard's `reset_global_roots` runs at drop, after the test body.
    Box::leak(Box::new([0u64; CONCAT_SITE_SLOTS]))
}

fn is_heap_string(bits: u64) -> bool {
    bits & !POINTER_MASK == STRING_TAG
}

fn heap_string_bytes(bits: u64) -> Vec<u8> {
    assert!(is_heap_string(bits), "expected a heap string handle");
    let ptr = (bits & POINTER_MASK) as *const crate::string::StringHeader;
    unsafe {
        let len = (*ptr).byte_len as usize;
        std::slice::from_raw_parts(crate::string::string_data(ptr), len).to_vec()
    }
}

fn global_root_count() -> usize {
    GLOBAL_ROOTS.with(|roots| roots.borrow().len())
}

/// A cacheable integer fills its slot once with the returned handle and every
/// later call answers that identical handle; every value that selects no slot
/// (fractional, negative, past the table, NaN) is answered correctly and
/// leaves the table alone; `-0` is slot 0; an SSO result is cached by value
/// and, holding no pointer, is not registered as a root.
#[test]
fn test_site_slot_is_filled_once_and_answers_identically() {
    let _guard = CopyingNurseryTestGuard::new(0);
    let table = fresh_table();
    let prefix = js_string_from_bytes(b"field_".as_ptr(), 6);

    let roots_before = global_root_count();
    let first = js_string_concat_site_value(table.as_mut_ptr(), prefix, 7.0);
    assert_eq!(heap_string_bytes(first.to_bits()), b"field_7");
    assert_eq!(
        global_root_count(),
        roots_before + 1,
        "a heap handle's slot is registered as exactly one global root"
    );
    assert_eq!(
        table[7],
        first.to_bits(),
        "a cacheable heap result fills its slot"
    );

    let again = js_string_concat_site_value(table.as_mut_ptr(), prefix, 7.0);
    assert_eq!(
        again.to_bits(),
        first.to_bits(),
        "a filled slot answers the identical handle"
    );

    for (value, expect) in [
        (7.5, &b"field_7.5"[..]),
        (-1.0, &b"field_-1"[..]),
        (CONCAT_SITE_SLOTS as f64, &b"field_32"[..]),
        (f64::NAN, &b"field_NaN"[..]),
    ] {
        let out = js_string_concat_site_value(table.as_mut_ptr(), prefix, value);
        assert_eq!(heap_string_bytes(out.to_bits()), expect);
        assert_eq!(
            table.iter().filter(|&&s| s != 0).count(),
            1,
            "a value that selects no slot must not touch the table"
        );
        assert_eq!(table[7], first.to_bits());
    }

    let zero = js_string_concat_site_value(table.as_mut_ptr(), prefix, -0.0);
    assert_eq!(heap_string_bytes(zero.to_bits()), b"field_0");
    assert_eq!(
        table[0],
        zero.to_bits(),
        "-0 selects slot 0, as JS prints it"
    );

    let short_prefix = js_string_from_bytes(b"k".as_ptr(), 1);
    let sso_table = fresh_table();
    let roots_before = global_root_count();
    let sso = js_string_concat_site_value(sso_table.as_mut_ptr(), short_prefix, 4.0);
    assert!(
        !is_heap_string(sso.to_bits()),
        "test premise: \"k4\" is an SSO immediate"
    );
    assert_eq!(
        sso_table[4],
        sso.to_bits(),
        "an SSO immediate is cached by value"
    );
    assert_eq!(
        global_root_count(),
        roots_before,
        "an SSO slot holds no pointer and must not be registered as a root"
    );
    let sso_again = js_string_concat_site_value(sso_table.as_mut_ptr(), short_prefix, 4.0);
    assert_eq!(sso_again.to_bits(), sso.to_bits());
}

/// A filled slot is a strong root the collector rewrites: after a copied
/// minor moves the cached string, the slot holds the new address and still
/// reads the same bytes. Fails if the filling arm loses its
/// `js_gc_register_global_root` call.
///
/// A shadow-stack root holds the same string independently, so the string
/// is kept alive and moved whether or not the table registered its slot —
/// the premise cannot pass or fail on the behaviour under test, and a
/// missing registration fails on the assertion that names it (sabotage-run
/// while writing this: without the call the slot keeps the pre-move address
/// while the shadow root shows the new one).
#[test]
fn test_filled_slot_is_rewritten_by_a_copied_minor() {
    let _guard = CopyingNurseryTestGuard::new(1);
    let table = fresh_table();
    let prefix = js_string_from_bytes(b"field_".as_ptr(), 6);

    let handle = js_string_concat_site_value(table.as_mut_ptr(), prefix, 7.0);
    let old_addr = (handle.to_bits() & POINTER_MASK) as usize;
    assert_eq!(
        table[7],
        handle.to_bits(),
        "test premise: the slot is filled"
    );
    js_shadow_slot_set(0, handle.to_bits());

    let _ = gc_collect_minor();

    let moved_bits = js_shadow_slot_get(0);
    let new_addr = (moved_bits & POINTER_MASK) as usize;
    assert_ne!(
        new_addr, old_addr,
        "test premise: the cached string must actually move"
    );
    assert_eq!(
        table[7], moved_bits,
        "the slot must follow the moved string — is the filled slot still \
         registered as a global root?"
    );
    assert_eq!(heap_string_bytes(table[7]), b"field_7");
}
