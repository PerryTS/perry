//! A bound TIMER-handle / TextDecoder / TextEncoder / primitive-receiver method
//! closure must not capture a pointer into the key string.
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
//!   `TextEncoder.prototype.encode`/`encodeInto` read as VALUES
//!   (`K.decode.bind(K)`, "the shape a minified SDK's cached decodeText helper
//!   takes"), so this was the intended hot path, not an edge. #340/#341 made
//!   the text family ordinary objects and deleted that reifier: those reads now
//!   resolve on the family prototype and bind nothing, so the two text tests
//!   below assert the stronger property instead (see
//!   `assert_names_the_installed_method`).
//! * `ic_miss.rs` — the inline-cache MISS mirror, whose own comment says "the
//!   IC fast path funnels small handles here, bypassing the identical block in
//!   `js_object_get_field_by_name`, so it must be mirrored". A separate live
//!   entry point, not a redundant copy.
//! * `get_field_by_name.rs` — a third copy of the same timer block. Fixed
//!   defensively; nothing guarantees it stays unreachable across refactors.
//! * `get_field_by_name.rs`'s primitive-number receiver — `(5).toString` and
//!   the inherited Object prototype methods used the computed key's movable
//!   interior directly (#8178).
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
/// #340/#341: for a family whose instances are ORDINARY objects the read
/// resolves the method on the family prototype and returns that prototype's
/// own closure. `js_class_method_bind` never runs, so there is no per-read
/// captured key pointer left to get wrong — the hazard is unwritable for this
/// family rather than merely fixed, and `captured_name` reads capture slots a
/// prototype closure does not have.
///
/// What still has to hold, and still fails if the lookup regresses: the read
/// yields the REAL installed method (a placeholder or a miss fails here), and
/// the name it carries is the install-time string rather than the caller's
/// movable key-string interior — the same property `assert_names_the_literal`
/// asserts for the bound form.
unsafe fn assert_names_the_installed_method(
    value: crate::value::JSValue,
    expected: &str,
    key_interior: *const u8,
    what: &str,
) {
    let closure = crate::value::js_nanbox_get_pointer(f64::from_bits(value.bits()))
        as *const crate::ClosureHeader;
    assert!(
        !closure.is_null(),
        "{what}: the read must produce a callable"
    );
    let name = crate::closure::closure_get_dynamic_prop(closure as usize, "name");
    let name_value = crate::value::JSValue::from_bits(name.to_bits());
    assert!(
        name_value.is_string(),
        "{what}: the read must yield the installed named method, got {:#018x}",
        name.to_bits()
    );
    let name_ptr = (name.to_bits() & crate::value::POINTER_MASK) as *const crate::StringHeader;
    assert!(
        !name_ptr.is_null(),
        "{what}: the method name must be a string"
    );
    let interior = (name_ptr as *const u8).add(std::mem::size_of::<crate::StringHeader>());
    let got = std::str::from_utf8(std::slice::from_raw_parts(
        interior,
        (*name_ptr).byte_len as usize,
    ))
    .unwrap_or("<non-utf8>");
    assert_eq!(got, expected, "{what}: wrong method name");
    assert_ne!(
        interior, key_interior,
        "{what}: the value names the KEY STRING's interior — that allocation \
         is movable and unreachable after this read"
    );
}

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

/// A live `Timeout` handle. #340/#341: this is an ORDINARY OBJECT now, not a
/// registry id, so the assertion that keeps the tests below from going vacuous
/// changed with it — it used to be `is_known_timer_id(id)` (the gate on the
/// arms under test); it is now "this really is a timer handle, and the timer it
/// names is live". Both halves matter: the first says the read has a branded
/// receiver to resolve, the second that the family's own registry agrees.
fn live_timer() -> i64 {
    let handle = crate::timer::js_set_timeout_callback(0, 10_000.0);
    let value = crate::value::js_nanbox_pointer(handle);
    let (id, is_immediate) = crate::timer::timer_handle_parts(value)
        .expect("setTimeout must return a branded Timeout handle");
    assert!(!is_immediate, "setTimeout is a Timeout, not an Immediate");
    assert!(
        crate::timer::is_known_timer_id(id),
        "the handle must name a LIVE timer, or the methods under test would be \
         resolving against a dead id"
    );
    assert!(
        !crate::value::addr_class::is_handle_band(handle as usize),
        "gate B: the producer must not hand back a small band id ({handle:#x})"
    );
    handle
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
        assert_names_the_installed_method(bound, "ref", key_interior, "timer.ref");
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
        assert_names_the_installed_method(bound, "unref", key_interior, "timer.unref (raw handle)");
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
        let mut cache_slot: crate::object::PicCacheSlot = &mut cache;
        let bits = crate::object::js_object_get_field_ic_miss(
            id as *const crate::ObjectHeader,
            key,
            &mut cache_slot,
        );
        assert_names_the_installed_method(
            crate::value::JSValue::from_bits(bits.to_bits()),
            "hasRef",
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
        let (key, key_interior) = heap_key("decode");

        // #340/#341: a decoder is an ordinary object now, so this read resolves
        // `decode` on `TextDecoder.prototype` and returns the prototype's own
        // closure. No `js_class_method_bind` runs, so there is no captured key
        // pointer for #8133 to be about — the hazard is now unwritable for this
        // family rather than merely fixed. Assert the stronger property: the
        // value is a callable that does NOT alias the heap key's interior.
        let bound =
            crate::object::js_object_get_field_by_name(id as *const crate::ObjectHeader, key);
        assert_names_the_installed_method(bound, "decode", key_interior, "TextDecoder.decode");
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
            // See the decoder sibling: ordinary prototype lookup, no bind.
            let bound =
                crate::object::js_object_get_field_by_name(id as *const crate::ObjectHeader, key);
            assert_names_the_installed_method(bound, name, key_interior, name);
        }
    }
}

/// ★ #8178's runtime regression: the helper shared by both primitive-number
/// receiver guards must capture the static spelling, not the computed heap
/// key's interior.
#[test]
fn a_bound_primitive_method_never_captures_the_key_strings_interior() {
    let _guard = GcTestIsolationGuard::new();
    unsafe {
        let (key, key_interior) = heap_key("toString");
        let key_bytes = std::slice::from_raw_parts(key_interior, (*key).byte_len as usize);
        let bound = crate::object::bind_primitive_proto_method_static(5.0, key_bytes)
            .expect("toString is a primitive prototype method");
        let expected = crate::object::primitive_proto_method_name_static(b"toString")
            .expect("toString is a primitive prototype method");
        assert_names_the_literal(bound, expected, key_interior, "(5).toString");
    }
}

/// The lookups must not simply echo their argument — a `|k| Some(k)` that
/// type-checked would pass every identity assertion above while still handing
/// back the caller's storage.
#[test]
fn the_static_name_lookups_do_not_borrow_their_argument() {
    let _guard = GcTestIsolationGuard::new();

    // #340/#341: the timer half of this test is gone with the lookup it pinned.
    // A timer handle is an ordinary object whose prototype owns `ref` / `unref`
    // / `hasRef` / `refresh` / `close` / `[Symbol.dispose]` /
    // `[Symbol.toPrimitive]` as real installed methods, so there is no
    // per-read name to borrow and nothing left to get wrong — the timer tests
    // above now assert the installed method instead. The primitive half below
    // is untouched and still pins its own lookup.

    let owned = String::from("propertyIsEnumerable");
    let found = crate::object::primitive_proto_method_name_static(owned.as_bytes())
        .expect("propertyIsEnumerable is a primitive prototype method");
    assert_ne!(
        found.as_ptr(),
        owned.as_ptr(),
        "the primitive lookup must answer a literal, not borrow its argument"
    );
    for name in [
        &b"toString"[..],
        b"valueOf",
        b"hasOwnProperty",
        b"isPrototypeOf",
        b"propertyIsEnumerable",
        b"toLocaleString",
    ] {
        assert_eq!(
            crate::object::primitive_proto_method_name_static(name),
            Some(name),
            "every primitive prototype method must still resolve"
        );
    }
    assert!(crate::object::primitive_proto_method_name_static(b"notAPrototypeMethod").is_none());
}
