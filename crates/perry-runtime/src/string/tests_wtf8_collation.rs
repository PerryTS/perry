//! #10692 — `String.prototype.normalize` / `localeCompare` over a WTF-8
//! payload, exercised through the FFI entry points codegen actually calls.
//!
//! The bug these pin was a **safety-contract** violation, not a wrong answer:
//! both entry points built a `&str` from the payload with an unconditional
//! `str::from_utf8_unchecked`, and a lone surrogate makes those bytes invalid
//! UTF-8. The output happened to be right because `unicode-normalization`
//! tolerates the out-of-range `char` it then decodes. These tests make the
//! answers a property of the implementation instead of an accident, and
//! `the_bytes_the_old_borrow_accepted_are_not_valid_utf8` states the violated
//! precondition mechanically.
//!
//! Every expectation is the output of Node v26.5.1 (the `.node-version`
//! oracle) under `node --experimental-strip-types`; the same table is in
//! `test-files/test_gap_10692_normalize_lone_surrogate.ts`.

use super::wtf8::Wtf8Str;
use super::*;

/// Encode UTF-16 code units as WTF-8, the way a Perry payload built from a
/// JavaScript string literal with a lone surrogate holds them.
fn wtf8(units: &[u16]) -> Vec<u8> {
    let mut out = Vec::new();
    for &unit in units {
        let code_point = unit as u32;
        if code_point < 0x80 {
            out.push(code_point as u8);
        } else if code_point < 0x800 {
            out.push(0xC0 | (code_point >> 6) as u8);
            out.push(0x80 | (code_point & 0x3F) as u8);
        } else {
            out.push(0xE0 | (code_point >> 12) as u8);
            out.push(0x80 | ((code_point >> 6) & 0x3F) as u8);
            out.push(0x80 | (code_point & 0x3F) as u8);
        }
    }
    out
}

/// Build a heap string the way a real payload is built: derive the
/// lone-surrogate flag from the bytes.
///
/// Deliberately NOT `js_string_from_wtf8_bytes`, which sets
/// `STRING_FLAG_HAS_LONE_SURROGATES` unconditionally without scanning — a
/// well-formed control built with it would be mis-flagged, and the positive
/// control below would fail for a reason that has nothing to do with the code
/// under test.
fn heap_string(units: &[u16]) -> *mut StringHeader {
    js_string_from_builder_bytes(&wtf8(units))
}

/// Read a payload back as UTF-16 code units, which is what `charCodeAt` sees
/// and therefore the form the Node oracle table is written in.
fn units_of(s: *const StringHeader) -> Vec<u16> {
    let bytes = unsafe { slice::from_raw_parts(string_data(s), (*s).byte_len as usize) }.to_vec();
    let mut out = Vec::new();
    let mut offset = 0usize;
    while offset < bytes.len() {
        let (advance, count, code_point) = wtf8_step(&bytes, offset);
        if count == 2 {
            let v = code_point - 0x1_0000;
            out.push(0xD800 + (v >> 10) as u16);
            out.push(0xDC00 + (v & 0x3FF) as u16);
        } else {
            out.push(code_point as u16);
        }
        offset = (offset + advance).min(bytes.len());
    }
    out
}

fn form_arg(form: &str) -> f64 {
    let ptr = js_string_from_bytes(form.as_ptr(), form.len() as u32);
    crate::value::js_nanbox_string(ptr as i64)
}

/// A real `{ numeric: true }` options object. Passing `undefined` here would
/// route straight back to `js_string_locale_compare`, so the numeric
/// comparator — the second function this change converted — would never run
/// and the assertion below would be vacuous.
fn numeric_options() -> f64 {
    let obj = crate::object::js_object_alloc(0, 1);
    let key = js_string_from_bytes(b"numeric".as_ptr(), 7);
    crate::object::js_object_set_field_by_name(obj, key, f64::from_bits(crate::value::TAG_TRUE));
    crate::value::js_nanbox_pointer(obj as i64)
}

/// The precondition #10692 was about, stated so it cannot be argued with: the
/// exact bytes the pre-fix `string_as_str(s)` handed to
/// `str::from_utf8_unchecked` are rejected by the checked decoder, and the
/// guarded borrow now refuses to produce a `&str` from them at all.
#[test]
fn the_bytes_the_old_borrow_accepted_are_not_valid_utf8() {
    for units in [&[0xD800u16][..], &[0xDC00][..], &[0x65, 0xD800, 0x301][..]] {
        let bytes = wtf8(units);
        assert!(
            std::str::from_utf8(&bytes).is_err(),
            "{units:x?} must be WTF-8, not UTF-8 — otherwise this proves nothing"
        );
        let s = heap_string(units);
        assert_ne!(
            unsafe { (*s).flags } & STRING_FLAG_HAS_LONE_SURROGATES,
            0,
            "{units:x?} must be flagged, or the guard cannot fire"
        );
        // SAFETY: `s` is live and the borrow does not outlive this statement.
        assert!(
            unsafe { Wtf8Str::from_header(s) }.as_str().is_none(),
            "{units:x?} must not be borrowable as `&str`"
        );
    }
    // The control: a well-formed payload still borrows, so the guard is a
    // guard and not a blanket refusal.
    let ok = heap_string(&[0x61, 0xE9]);
    assert_eq!(unsafe { Wtf8Str::from_header(ok) }.as_str(), Some("aé"));
}

/// The oracle table. Node v26.5.1, all four forms, twelve inputs.
#[cfg(feature = "string-normalize")]
#[test]
fn normalize_matches_node_on_lone_surrogate_payloads() {
    // (input, NFC, NFD, NFKC, NFKD)
    let table: &[(&[u16], &[u16], &[u16], &[u16], &[u16])] = &[
        (&[0xD800], &[0xD800], &[0xD800], &[0xD800], &[0xD800]),
        (&[0xDC00], &[0xDC00], &[0xDC00], &[0xDC00], &[0xDC00]),
        (
            &[0x65, 0x301, 0xD800],
            &[0xE9, 0xD800],
            &[0x65, 0x301, 0xD800],
            &[0xE9, 0xD800],
            &[0x65, 0x301, 0xD800],
        ),
        (
            &[0xE9, 0xD800],
            &[0xE9, 0xD800],
            &[0x65, 0x301, 0xD800],
            &[0xE9, 0xD800],
            &[0x65, 0x301, 0xD800],
        ),
        (
            &[0xD800, 0xE9],
            &[0xD800, 0xE9],
            &[0xD800, 0x65, 0x301],
            &[0xD800, 0xE9],
            &[0xD800, 0x65, 0x301],
        ),
        // The subtle one: the surrogate is a starter (ccc = 0), so the
        // combining acute must NOT compose onto the `e` across it.
        (
            &[0x65, 0xD800, 0x301],
            &[0x65, 0xD800, 0x301],
            &[0x65, 0xD800, 0x301],
            &[0x65, 0xD800, 0x301],
            &[0x65, 0xD800, 0x301],
        ),
        (
            &[0xFB01, 0xD800],
            &[0xFB01, 0xD800],
            &[0xFB01, 0xD800],
            &[0x66, 0x69, 0xD800],
            &[0x66, 0x69, 0xD800],
        ),
        (
            &[0xDC00, 0xE9],
            &[0xDC00, 0xE9],
            &[0xDC00, 0x65, 0x301],
            &[0xDC00, 0xE9],
            &[0xDC00, 0x65, 0x301],
        ),
        (
            &[0x61, 0xD800, 0x62],
            &[0x61, 0xD800, 0x62],
            &[0x61, 0xD800, 0x62],
            &[0x61, 0xD800, 0x62],
            &[0x61, 0xD800, 0x62],
        ),
        (
            &[0xD800, 0x301],
            &[0xD800, 0x301],
            &[0xD800, 0x301],
            &[0xD800, 0x301],
            &[0xD800, 0x301],
        ),
        // U+212B ANGSTROM SIGN: a singleton decomposition, so NFC rewrites it
        // even though its run holds a single scalar.
        (
            &[0x212B, 0xD800],
            &[0xC5, 0xD800],
            &[0x41, 0x30A, 0xD800],
            &[0xC5, 0xD800],
            &[0x41, 0x30A, 0xD800],
        ),
        (
            &[0xD800, 0xD800],
            &[0xD800, 0xD800],
            &[0xD800, 0xD800],
            &[0xD800, 0xD800],
            &[0xD800, 0xD800],
        ),
    ];
    for (input, nfc, nfd, nfkc, nfkd) in table {
        for (name, expected) in [("NFC", nfc), ("NFD", nfd), ("NFKC", nfkc), ("NFKD", nfkd)] {
            let out = js_string_normalize(heap_string(input), form_arg(name));
            assert_eq!(
                units_of(out),
                expected.to_vec(),
                "{input:x?}.normalize({name:?}) (Node v26.5.1)"
            );
            // The surrogate survives, so the result is still not well-formed.
            assert_ne!(
                unsafe { (*out).flags } & STRING_FLAG_HAS_LONE_SURROGATES,
                0,
                "{input:x?}.normalize({name:?}) lost the lone-surrogate flag"
            );
        }
        // The omitted argument defaults to NFC (§22.1.3.13).
        let default = js_string_normalize(
            heap_string(input),
            f64::from_bits(crate::value::TAG_UNDEFINED),
        );
        assert_eq!(units_of(default), nfc.to_vec(), "{input:x?}.normalize()");
    }
}

/// `localeCompare` signs, also measured against Node v26.5.1. Perry's
/// collation is code-point order rather than ICU root collation (a documented
/// divergence, see `js_string_locale_compare`), but the two agree on every
/// case here — a lone surrogate sorts by its code point in both.
#[test]
fn locale_compare_matches_node_on_lone_surrogate_payloads() {
    let pairs: &[(&[u16], &[u16], f64)] = &[
        (&[0xD800], &[0xD800], 0.0),
        (&[0xD800], &[0xDC00], -1.0),
        (&[0xD800], &[0x61], 1.0),
        (&[0x61], &[0xD800], -1.0),
        (&[0xD800], &[], 1.0),
        (&[], &[0xD800], -1.0),
        (&[0xD800], &[0xE000], -1.0),
        (&[0x61, 0xD800], &[0x61, 0xD800], 0.0),
        (&[0x61, 0xD800], &[0x62, 0xD800], -1.0),
        (&[0xD800, 0x61], &[0xD800, 0x62], -1.0),
    ];
    let sign = |v: f64| {
        if v < 0.0 {
            -1.0
        } else if v > 0.0 {
            1.0
        } else {
            0.0
        }
    };
    for (a, b, expected) in pairs {
        let (sa, sb) = (heap_string(a), heap_string(b));
        assert_eq!(
            sign(js_string_locale_compare(sa, sb)),
            *expected,
            "{a:x?}.localeCompare({b:x?}) (Node v26.5.1)"
        );
        // `{ numeric: true }` routes through `locale_compare_numeric_raw`,
        // a second comparator that also had to stop taking a `&str`. Node
        // gives the same answers here (no digit runs are involved).
        let (sa, sb) = (heap_string(a), heap_string(b));
        assert_eq!(
            sign(js_string_locale_compare_opts(sa, sb, numeric_options())),
            *expected,
            "{a:x?}.localeCompare({b:x?}, _, {{numeric:true}}) (Node v26.5.1)"
        );
    }
}

/// Canonical equivalence is a mandatory part of the `localeCompare` contract,
/// and it has to keep holding when a lone surrogate is in the string. Node
/// answers 0 for both of these.
#[cfg(feature = "string-normalize")]
#[test]
fn canonical_equivalence_survives_a_lone_surrogate() {
    for (a, b) in [
        (&[0x65u16, 0x301, 0xD800][..], &[0xE9u16, 0xD800][..]),
        (&[0xD800, 0x65, 0x301][..], &[0xD800, 0xE9][..]),
    ] {
        assert_eq!(
            js_string_locale_compare(heap_string(a), heap_string(b)),
            0.0,
            "{a:x?} vs {b:x?} must be canonically equal"
        );
    }
}

/// The *form* argument is user-controlled too, so it can itself be WTF-8.
/// Node throws `RangeError` — a lone surrogate is not one of the four names.
#[test]
fn a_lone_surrogate_form_argument_throws_rangeerror() {
    let subject = heap_string(&[0x61]);
    let bad_form = heap_string(&[0xD800]);
    let form_value = crate::value::js_nanbox_string(bad_form as i64);
    let outcome = crate::exception::catch_js_throw(|| {
        js_string_normalize(subject, form_value);
    });
    assert!(
        outcome.is_err(),
        "a lone-surrogate normalization form must throw"
    );
}
