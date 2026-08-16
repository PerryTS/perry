//! A bound TIMER-handle / TextDecoder / TextEncoder method closure must not
//! capture a pointer into the key string.
//!
//! `js_class_method_bind` stores the method-name POINTER in the closure
//! (capture 1) and `dispatch_bound_method` re-reads it at CALL time. Its
//! contract says so: "Method-name pointer is expected to be stable for the
//! closure's lifetime; codegen emits it from the per-module `.str.N.bytes`
//! rodata global."
//!
//! #7747 fixed two Buffer callers that broke it (see
//! `buffer_bound_method_name.rs`). Its commit message states the failure mode
//! exactly, and it applies verbatim here:
//!
//! > `get_field_by_name_tail` passed `key + size_of::<StringHeader>()` — the
//! > interior of a movable GC heap string that is unreachable once the read
//! > returns — so a copying minor could relocate or reclaim the bytes the
//! > closure names. […] Whether the stale bytes still spell the method is an
//! > allocator property, not a program property, which is why this passed
//! > locally and took a SIGSEGV on conformance-smoke shards 7 and 8.
//!
//! #8133 is the same defect at four more sites in the same neighbourhood, plus
//! two the issue did not name:
//!
//! * `get_field_by_name_tail.rs` — the timer-handle arm, twice (a NaN-boxed
//!   small-handle receiver and an already-stripped handle-band one).
//! * `text.rs`'s `text_handle_property` — `TextDecoder.prototype.decode` and
//!   `TextEncoder.prototype.encode`/`encodeInto` read as VALUES. That
//!   function's own docstring says value reads are the reason it exists
//!   (`K.decode.bind(K)`, "the shape a minified SDK's cached decodeText helper
//!   takes"), so this is the intended hot path, not an edge.
//! * `ic_miss.rs` — the inline-cache MISS mirror, whose own comment says "the
//!   IC fast path funnels small handles here, bypassing the identical block in
//!   `js_object_get_field_by_name`, so it must be mirrored". A separate live
//!   entry point, not a redundant copy.
//! * `get_field_by_name.rs` — a third copy of the same timer block. Fixed
//!   defensively; nothing guarantees it stays unreachable across refactors.
//!
//! ## Why these assert IDENTITY and not bytes
//!
//! Quoting #7747's own testing note, which the issue repeats: the inequality
//! against the key string could pass with the bug present, and comparing the
//! BYTES only fails on a host where the freed memory has already been reused —
//! which is the lucky-allocator problem these tests exist to avoid. **Identity
//! with the literal cannot be lucky.** So every test below asserts
//! `captured_ptr == <the 'static literal>.as_ptr()`.
//!
//! A test that merely called the bound method after a collection would pass
//! with the bug fully present on any host whose allocator left the bytes
//! intact, which is precisely the test not to write.

use super::support::*;

/// The name bytes a bound closure keeps, as raw parts. Same helper shape as
/// `buffer_bound_method_name.rs`.
unsafe fn captured_name(bound: crate::value::JSValue) -> (*const u8, usize) {
    let closure = crate::value::js_nanbox_get_pointer(f64::from_bits(bound.bits()))
        as *const crate::ClosureHeader;
    assert!(!closure.is_null(), "the read must produce a bound closure");
    let ptr = crate::closure::js_closure_get_capture_ptr(closure, 1) as *const u8;
    let len = crate::closure::js_closure_get_capture_ptr(closure, 2) as usize;
    (ptr, len)
}

/// An interned heap key plus the interior pointer the buggy callers derived
/// from it (`key + size_of::<StringHeader>()`).
unsafe fn heap_key(name: &str) -> (*mut crate::string::StringHeader, *const u8) {
    let key = crate::string::js_string_from_bytes(name.as_ptr(), name.len() as u32);
    let interior = (key as *const u8).add(std::mem::size_of::<crate::StringHeader>());
    (key, interior)
}

/// Assert the closure names the `'static` literal, and specifically NOT the
/// caller's heap-string interior.
///
/// `expected` MUST be obtained by calling the lookup under test, never written
/// as a literal here: two occurrences of `b"ref"` in different modules are two
/// `&'static [u8]`s that the linker is free to leave at different addresses,
/// and it does. Asserting against a locally written literal fails on a correct
/// implementation — measured, before this comment existed.
unsafe fn assert_names_the_literal(
    bound: crate::value::JSValue,
    expected: &'static [u8],
    key_interior: *const u8,
    what: &str,
) {
    let (name_ptr, name_len) = captured_name(bound);
    // Pointer identity is only assertable where the linker merges identical
    // read-only strings. ELF (`SHF_MERGE|SHF_STRINGS`) and Mach-O
    // (`__TEXT,__cstring`) do; MSVC does not pool identical literals across
    // codegen units, so the copy the closure captures and the copy the lookup
    // returns can be two distinct `&'static [u8]` at different addresses.
    // Measured on the Windows runner: both failing pairs differed by the SAME
    // constant offset (0x161F90), i.e. two whole copies of the same read-only
    // data, not a heap pointer.
    //
    // Both are `'static`, which is the property this test exists for: the name
    // must not be the MOVABLE key string's interior. That invariant is asserted
    // unconditionally below, together with the length and the bytes, so Windows
    // keeps real coverage — it just cannot use address equality as the proxy.
    #[cfg(not(windows))]
    assert_eq!(
        name_ptr,
        expected.as_ptr(),
        "{what}: the closure must capture the 'static literal"
    );
    assert_ne!(
        name_ptr, key_interior,
        "{what}: the closure captured the KEY STRING's interior — that \
         allocation is movable and unreachable after this read, so the name it \
         dispatches on is freed or relocated bytes"
    );
    assert_eq!(name_len, expected.len(), "{what}: captured length");
    assert_eq!(
        std::slice::from_raw_parts(name_ptr, name_len),
        expected,
        "{what}: captured bytes"
    );
}

/// The `'static` literal the lookup under test answers for `key`. Never write
/// the literal locally — see [`assert_names_the_literal`].
fn timer_literal(key: &[u8]) -> &'static [u8] {
    crate::object::timer_handle_method_name_static(key).expect("a timer-handle method")
}

/// A live `Timeout` handle id. `is_known_timer_id` gates every arm under test,
/// so without a registered timer they all decline and the tests would be
/// vacuous — `assert!(is_known_timer_id(..))` below is what says they are not.
fn live_timer() -> i64 {
    let id = crate::timer::js_set_timeout_callback(0, 10_000.0);
    assert!(
        crate::timer::is_known_timer_id(id),
        "the arms under test are gated on `is_known_timer_id`; without a live \
         timer every assertion below would pass by never running"
    );
    id
}

/// ★ The regression, NaN-boxed small-handle receiver
/// (`get_field_by_name_tail.rs`, arm 1).
#[test]
fn a_bound_timer_method_never_captures_the_key_strings_interior() {
    let _guard = GcTestIsolationGuard::new();
    unsafe {
        let id = live_timer();
        let (key, key_interior) = heap_key("ref");
        let boxed = crate::value::js_nanbox_pointer(id).to_bits() as *const crate::ObjectHeader;

        let bound = crate::object::js_object_get_field_by_name(boxed, key);
        assert_names_the_literal(bound, timer_literal(b"ref"), key_interior, "timer.ref");
    }
}

/// ★ The regression, already-stripped handle-band receiver
/// (`get_field_by_name_tail.rs`, arm 2).
#[test]
fn a_bound_timer_method_from_a_raw_handle_never_captures_the_key() {
    let _guard = GcTestIsolationGuard::new();
    unsafe {
        let id = live_timer();
        let (key, key_interior) = heap_key("unref");

        let bound =
            crate::object::js_object_get_field_by_name(id as *const crate::ObjectHeader, key);
        assert_names_the_literal(
            bound,
            timer_literal(b"unref"),
            key_interior,
            "timer.unref (raw handle)",
        );
    }
}

/// ★ The regression, inline-cache MISS path (`ic_miss.rs`). A separate live
/// entry point: its own comment says the IC fast path funnels small handles
/// here, bypassing the block in `js_object_get_field_by_name`.
#[test]
fn a_bound_timer_method_from_the_ic_miss_path_never_captures_the_key() {
    let _guard = GcTestIsolationGuard::new();
    unsafe {
        let id = live_timer();
        let (key, key_interior) = heap_key("hasRef");
        let mut cache = crate::object::PicCache::default();

        let bits = crate::object::js_object_get_field_ic_miss(
            id as *const crate::ObjectHeader,
            key,
            &mut cache,
        );
        assert_names_the_literal(
            crate::value::JSValue::from_bits(bits.to_bits()),
            timer_literal(b"hasRef"),
            key_interior,
            "timer.hasRef (IC miss)",
        );
    }
}

/// ★ The regression, `TextDecoder.prototype.decode` read as a VALUE — the
/// `K.decode.bind(K)` shape `text_handle_property`'s docstring exists for.
#[test]
fn a_bound_text_decoder_decode_never_captures_the_key_strings_interior() {
    let _guard = GcTestIsolationGuard::new();
    unsafe {
        let undefined = f64::from_bits(crate::value::TAG_UNDEFINED);
        let id = crate::text::js_text_decoder_new(undefined, undefined, undefined);
        assert!(
            crate::text::is_known_text_decoder_id(id),
            "the decoder arm is gated on `is_known_text_decoder_id`"
        );
        let (key, key_interior) = heap_key("decode");

        let bound =
            crate::object::js_object_get_field_by_name(id as *const crate::ObjectHeader, key);
        let expected = crate::text::text_decoder_method_name_static(b"decode")
            .expect("decode is a TextDecoder method");
        assert_names_the_literal(bound, expected, key_interior, "TextDecoder.decode");
    }
}

/// ★ The regression, `TextEncoder.prototype.encode` / `encodeInto`.
#[test]
fn a_bound_text_encoder_method_never_captures_the_key_strings_interior() {
    let _guard = GcTestIsolationGuard::new();
    unsafe {
        let id = crate::text::js_text_encoder_new();
        for name in ["encode", "encodeInto"] {
            let (key, key_interior) = heap_key(name);
            let bound =
                crate::object::js_object_get_field_by_name(id as *const crate::ObjectHeader, key);
            let expected = crate::text::text_encoder_method_name_static(name.as_bytes())
                .expect("a TextEncoder method");
            assert_names_the_literal(bound, expected, key_interior, name);
        }
    }
}

/// The lookups must not simply echo their argument — a `|k| Some(k)` that
/// type-checked would pass every identity assertion above while still handing
/// back the caller's storage.
#[test]
fn the_static_name_lookups_do_not_borrow_their_argument() {
    let _guard = GcTestIsolationGuard::new();

    let owned = String::from("refresh");
    let found = crate::object::timer_handle_method_name_static(owned.as_bytes())
        .expect("refresh is a timer-handle method");
    assert_ne!(
        found.as_ptr(),
        owned.as_bytes().as_ptr(),
        "the lookup must answer the LITERAL, not a borrow of its argument"
    );

    // Same literal for every caller, whatever storage the caller used.
    let second = String::from("refresh");
    assert_eq!(
        crate::object::timer_handle_method_name_static(second.as_bytes())
            .unwrap()
            .as_ptr(),
        found.as_ptr(),
        "every call must answer the same 'static address"
    );

    assert!(
        crate::object::timer_handle_method_name_static(b"notATimerMethod").is_none(),
        "a non-method key must not resolve"
    );
    // The list must not have shrunk while being rewritten: #8133 replaced the
    // `is_timer_handle_method_key` predicate with this lookup, and a dropped
    // name would silently stop binding that method rather than fail loudly.
    for name in [
        &b"ref"[..],
        b"unref",
        b"hasRef",
        b"refresh",
        b"close",
        b"__perry_dispose__",
        b"@@__perry_wk_dispose",
        b"@@__perry_wk_toPrimitive",
    ] {
        assert_eq!(
            crate::object::timer_handle_method_name_static(name),
            Some(name),
            "every pre-#8133 timer-handle method must still resolve"
        );
    }
}
