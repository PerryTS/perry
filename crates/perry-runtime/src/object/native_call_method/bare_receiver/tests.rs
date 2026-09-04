//! #9675 regression coverage for [`canonicalize_bare_gc_receiver`].
//!
//! Two claims, and the second is the one that keeps a fix like this honest:
//!
//! * **the reclassification happens** — a bare `GC_TYPE_STRING` receiver comes
//!   back a STRING, a bare `GC_TYPE_BIGINT` receiver a BIGINT, an ordinary
//!   managed object a POINTER, an already-forwarded receiver at its CURRENT
//!   address, and a bare string receiver's dynamic `.slice` returns the sliced
//!   string instead of throwing;
//! * **nothing else is reclassified** — genuine subnormal doubles whose bits sit
//!   squarely in the heap-address range, unrelated allocations carrying
//!   synthetic GC headers, headerless registry handles, a fresh `Symbol()`
//!   (which is itself a `GC_TYPE_STRING` allocation), and every NaN-boxed value
//!   are returned bit-for-bit unchanged — and a word nothing vouched for is
//!   routed to NUMBER dispatch, so no magnitude-gated probe can dereference it.
//!
//! Without the second half this file would pass with the gate widened to "any
//! address-shaped word", which is exactly the mistake the magnitude-only tail
//! recovery makes.

use super::*;
use crate::gc::{GcHeader, GC_FLAG_FORWARDED, GC_HEADER_SIZE};
use crate::value::{JSValue, BIGINT_TAG, POINTER_MASK, POINTER_TAG, STRING_TAG, TAG_MASK};

/// Encode `addr` the way a managed pointer looks when it was never NaN-boxed:
/// the raw address, top 16 bits zero. This is the shape
/// `gc_pointer_and_type_from_value` already accepts and the shape
/// `js_native_call_method` classified as a "number".
fn bare(addr: usize) -> f64 {
    f64::from_bits(addr as u64)
}

/// Every test below is vacuous unless the receiver really is bare-encodable on
/// this host, and unless a bare word really does carry the zero tag that both
/// `try_mark_value` and `try_rewrite_nanboxed_value` reject (see
/// `gc/tests/root_words.rs`, #6910 — the same hole, in the shadow/global
/// registries).
fn assert_bare_premise(addr: usize, what: &str) {
    assert!(addr != 0, "test premise: {what} allocated");
    assert_eq!(
        (addr as u64) >> 48,
        0,
        "test premise: {what} must be representable as a bare receiver on this \
         host (top 16 bits zero)"
    );
    assert_eq!(
        bare(addr).to_bits() & TAG_MASK,
        0,
        "test premise: a bare receiver's tag is zero, which is why a `Nanbox` \
         root slot holding one is neither marked nor rewritten"
    );
}

fn tag_of(value: f64) -> u64 {
    value.to_bits() & TAG_MASK
}

fn payload_of(value: f64) -> usize {
    (value.to_bits() & POINTER_MASK) as usize
}

unsafe fn rust_string(header: *const crate::StringHeader) -> String {
    let bytes = std::slice::from_raw_parts(
        crate::string::string_data(header),
        (*header).byte_len as usize,
    );
    String::from_utf8_lossy(bytes).into_owned()
}

unsafe fn header_of(addr: usize) -> *mut GcHeader {
    (addr - GC_HEADER_SIZE) as *mut GcHeader
}

// ---------------------------------------------------------------------------
// The reclassification happens.
// ---------------------------------------------------------------------------

/// The #9675 shape. `string_methods::dispatch_string` gates on
/// `is_any_string()`, which accepts only `STRING_TAG`/`SHORT_STRING_TAG` — so
/// the tail recovery's unconditional `JSValue::pointer` rebox produced a
/// receiver that is not a string, `.slice` never reached the string arm, and
/// the call fell through to the `<m> is not a function` catch-all.
#[test]
fn bare_gc_string_receiver_is_canonicalized_to_string_tag() {
    let header = crate::string::js_string_from_str("abcdef");
    let addr = header as usize;
    assert_bare_premise(addr, "a GC string");

    let canonical = unsafe { canonicalize_bare_gc_receiver(bare(addr)) };
    assert_eq!(
        tag_of(canonical),
        STRING_TAG,
        "a bare GC_TYPE_STRING receiver must be reboxed with STRING_TAG"
    );
    assert_eq!(
        payload_of(canonical),
        addr,
        "payload must be the same object"
    );
    assert!(
        JSValue::from_bits(canonical.to_bits()).is_any_string(),
        "the canonical receiver must satisfy the exact predicate \
         `string_methods::dispatch_string` gates on"
    );

    // What the tail recovery produced instead, stated so the reason this test
    // exists cannot be lost: same object, wrong kind.
    let as_the_tail_reboxed_it = f64::from_bits(JSValue::pointer(addr as *const u8).bits());
    assert!(
        !JSValue::from_bits(as_the_tail_reboxed_it.to_bits()).is_any_string(),
        "premise of the bug: an unconditional POINTER_TAG rebox of a GC string \
         is not a string, so `.slice` cannot dispatch"
    );
}

/// The user-visible statement: a dynamic `.slice` on a bare managed string
/// receiver returns the sliced string.
#[test]
fn bare_gc_string_receiver_dispatches_slice_as_a_string() {
    let header = crate::string::js_string_from_str("abcdef");
    let addr = header as usize;
    assert_bare_premise(addr, "a GC string");

    let args = [1.0f64];
    let result = unsafe {
        crate::object::js_native_call_method(
            bare(addr),
            b"slice".as_ptr() as *const i8,
            5,
            args.as_ptr(),
            args.len(),
        )
    };
    assert!(
        JSValue::from_bits(result.to_bits()).is_any_string(),
        "`.slice(1)` on a bare managed string receiver must return a string"
    );
    let out = crate::value::js_get_string_pointer_unified(result) as *const crate::StringHeader;
    assert!(!out.is_null(), "the returned string must be readable");
    assert_eq!(unsafe { rust_string(out) }, "bcdef");
}

#[test]
fn bare_gc_bigint_receiver_is_canonicalized_to_bigint_tag() {
    let mut limbs = [0u64; crate::bigint::BIGINT_LIMBS];
    limbs[0] = 42;
    let addr = crate::bigint::bigint_alloc_with_limbs(limbs) as usize;
    assert_bare_premise(addr, "a GC BigInt");

    let canonical = unsafe { canonicalize_bare_gc_receiver(bare(addr)) };
    assert_eq!(
        tag_of(canonical),
        BIGINT_TAG,
        "a bare GC_TYPE_BIGINT receiver must be reboxed with BIGINT_TAG"
    );
    assert_eq!(payload_of(canonical), addr);
    assert!(JSValue::from_bits(canonical.to_bits()).is_bigint());
}

#[test]
fn bare_gc_object_receiver_is_canonicalized_to_pointer_tag() {
    let addr = crate::object::js_object_alloc(0, 4) as usize;
    assert_bare_premise(addr, "a plain managed object");

    let canonical = unsafe { canonicalize_bare_gc_receiver(bare(addr)) };
    assert_eq!(
        tag_of(canonical),
        POINTER_TAG,
        "an ordinary managed receiver keeps POINTER_TAG — the tag it always had"
    );
    assert_eq!(payload_of(canonical), addr);
}

/// The tail recovery did no forwarding walk, so a receiver a collection had
/// already moved was reboxed at its from-space address and dispatched there.
#[test]
fn bare_receiver_canonicalization_follows_forwarding() {
    let from = crate::string::js_string_from_str("abcdef") as usize;
    let to = crate::string::js_string_from_str("uvwxyz") as usize;
    assert_bare_premise(from, "the from-space GC string");
    assert_bare_premise(to, "the to-space GC string");
    assert_ne!(from, to, "test premise: two distinct allocations");

    unsafe {
        let header = header_of(from);
        crate::gc::set_forwarding_address(header, to as *mut u8);
        (*header).gc_flags |= GC_FLAG_FORWARDED;
    }

    let canonical = unsafe { canonicalize_bare_gc_receiver(bare(from)) };
    assert_eq!(
        payload_of(canonical),
        to,
        "a forwarded bare receiver must canonicalize to its CURRENT address, \
         not the from-space one the tail recovery used"
    );
    assert_eq!(tag_of(canonical), STRING_TAG);

    unsafe {
        (*header_of(from)).gc_flags &= !GC_FLAG_FORWARDED;
    }
}

/// The canonical receiver's whole purpose is to be rootable: the tower parks it
/// in a `RuntimeHandleSlot::Nanbox`, and that slot kind is marked by
/// `try_mark_value` and rewritten by `try_rewrite_nanboxed_value`, both of
/// which accept exactly `POINTER_TAG`/`STRING_TAG`/`BIGINT_TAG` and reject
/// everything else.
#[test]
fn every_canonical_receiver_carries_a_tag_the_collector_traces() {
    let mut limbs = [0u64; crate::bigint::BIGINT_LIMBS];
    limbs[0] = 7;
    let candidates = [
        (
            "string",
            crate::string::js_string_from_str("abcdef") as usize,
        ),
        (
            "bigint",
            crate::bigint::bigint_alloc_with_limbs(limbs) as usize,
        ),
        ("object", crate::object::js_object_alloc(0, 4) as usize),
        ("array", crate::array::js_array_alloc(4) as usize),
    ];
    for (what, addr) in candidates {
        assert_bare_premise(addr, what);
        let canonical = unsafe { canonicalize_bare_gc_receiver(bare(addr)) };
        assert!(
            matches!(tag_of(canonical), POINTER_TAG | STRING_TAG | BIGINT_TAG),
            "{what}: a bare receiver must be reboxed under a tag a `Nanbox` root \
             slot is marked and rewritten through; a zero tag is dropped by both \
             passes (#6910's hole, re-opened in the transient-handle registry)"
        );
    }
}

// ---------------------------------------------------------------------------
// Nothing else is reclassified.
// ---------------------------------------------------------------------------

/// Address magnitude is not evidence of anything. These are genuine positive
/// subnormal doubles whose bit patterns sit squarely inside the platform heap
/// window the tail recovery's `is_valid_obj_ptr` accepts.
#[test]
fn pointer_magnitude_subnormal_numbers_are_not_reclassified() {
    for bits in [
        0x1000u64,
        0x0010_0000,
        0x0000_5555_5555_5555,
        0x0000_7fff_ffff_f000,
        0x0000_0001_0000_0008,
        1,
        0,
    ] {
        let value = f64::from_bits(bits);
        assert!(
            JSValue::from_bits(bits).is_number(),
            "test premise: {bits:#x} decodes as a number, so the tower would \
             report it as `(number)`"
        );
        let out = unsafe { canonicalize_bare_gc_receiver(value) };
        assert_eq!(
            out.to_bits(),
            bits,
            "{bits:#x} is owned by nothing and must be returned bit-for-bit \
             unchanged"
        );
    }
}

/// An unrelated allocation with a hand-built `GcHeader` in front of it passes
/// every magnitude test and every header-content test. Only allocator
/// ownership rejects it.
#[test]
fn unrelated_allocations_with_synthetic_headers_are_not_reclassified() {
    #[repr(C)]
    struct Synthetic {
        header: GcHeader,
        payload: [u64; 4],
    }

    let synthetic = Box::new(Synthetic {
        header: GcHeader {
            obj_type: crate::gc::GC_TYPE_STRING,
            gc_flags: 0,
            _reserved: 0,
            size: std::mem::size_of::<Synthetic>() as u32,
        },
        payload: [0; 4],
    });
    let user = &synthetic.payload as *const [u64; 4] as usize;
    assert_bare_premise(user, "a non-Perry allocation");

    let out = unsafe { canonicalize_bare_gc_receiver(bare(user)) };
    assert_eq!(
        out.to_bits(),
        user as u64,
        "a `Box` allocation is owned by neither the arena nor the GC malloc \
         registry, so it must not be reboxed as a managed receiver"
    );
}

/// Registry handles (timers, sockets, zlib streams, proxies, …) are small ids
/// with no header at all. Dereferencing one is the #4665/#4800 SIGSEGV.
#[test]
fn headerless_registry_handles_are_not_reclassified() {
    for id in [1u64, 0x1008, 0x1_0000, 0x4_0000, 0xF_0000, 0xF_FFF8] {
        let out = unsafe { canonicalize_bare_gc_receiver(f64::from_bits(id)) };
        assert_eq!(
            out.to_bits(),
            id,
            "handle id {id:#x} must be returned unchanged, without a header read"
        );
    }
}

/// `alloc_symbol` gc_mallocs a `SymbolHeader` as `GC_TYPE_STRING`, so the
/// header alone would call a fresh `Symbol()` a string. Tagging one STRING_TAG
/// would hand `js_get_string_pointer_unified` a `SymbolHeader` to read as text.
#[test]
fn a_fresh_symbol_receiver_keeps_pointer_tag() {
    let addr = unsafe { crate::value::js_nanbox_get_pointer(crate::symbol::js_symbol_new_empty()) }
        as usize;
    assert_bare_premise(addr, "a fresh Symbol()");
    assert!(
        unsafe { crate::symbol::may_be_symbol_header(addr as *const u8) },
        "test premise: a symbol carries SYMBOL_MAGIC in its first word; without \
         it this test cannot distinguish the symbol screen from its absence"
    );

    let canonical = unsafe { canonicalize_bare_gc_receiver(bare(addr)) };
    assert_eq!(
        tag_of(canonical),
        POINTER_TAG,
        "a fresh Symbol() is a GC_TYPE_STRING allocation but is boxed as a \
         POINTER; the SYMBOL_MAGIC screen is what keeps it out of the string arm"
    );
    assert_eq!(payload_of(canonical), addr);
}

/// Defect 4. A word no owner claims is not a pointer, so the tower must answer
/// it as a number instead of handing it to probes that classify by address
/// magnitude and then dereference `addr - GC_HEADER_SIZE`.
///
/// These are the two shapes that actually bit. `1e-310` is `0x1268_8b70_e62b`:
/// above the handle band, inside `is_valid_obj_ptr`'s window, and unmapped —
/// `(1e-310 as any).toString()` SIGSEGV'd in
/// `url::search_params::shape_is_url_search_params`. `5e-324` is `0x1`, which
/// aliases the handle band instead, and the handle dispatcher answered it with
/// `undefined`. Node prints `1e-310` and `5e-324`.
#[test]
fn address_shaped_subnormals_route_to_number_dispatch() {
    for (label, bits) in [
        ("1e-310 (heap-magnitude alias)", 1e-310f64.to_bits()),
        ("5e-324 (handle-band alias)", 5e-324f64.to_bits()),
        ("the largest bare-shaped word", 0x0000_FFFF_FFFF_FFFFu64),
        ("mid-range bare-shaped word", 0x0000_5555_5555_5555u64),
    ] {
        let value = f64::from_bits(bits);
        assert_eq!(
            (bits >> 48),
            0,
            "test premise: {label} is a bare-shaped word, or it cannot alias an \
             address at all"
        );
        assert!(
            is_unvouched_bare_word(unsafe { canonicalize_bare_gc_receiver(value) }),
            "{label}: no owner may claim a genuine subnormal double — if one \
             does, it is about to be dereferenced as an object"
        );
    }
}

/// The boundary, and why the routing arm can be this narrow. Only a word whose
/// bits are BELOW 2^48 is address-shaped, so only those reach a bare-pointer
/// arm. A subnormal near the top of its range — `2.2e-308` is
/// `bits >> 48 == 15` — is not bare-shaped, and
/// `as_pointer()` masking cannot expose it either: every probe that masks is
/// gated on `is_pointer()`, and its tag is not `POINTER_TAG`. It therefore stays
/// on the ordinary path, which is where it already behaved correctly.
///
/// This test exists because the first version of the routing test above asserted
/// `2.2e-308` WAS bare-shaped and went red on its own premise.
#[test]
fn subnormals_above_the_bare_shaped_range_stay_on_the_ordinary_path() {
    for (label, value) in [
        ("2.2e-308", 2.2e-308f64),
        ("f64::MIN_POSITIVE", f64::MIN_POSITIVE),
        ("1e-300", 1e-300f64),
    ] {
        let bits = value.to_bits();
        assert_ne!(
            bits >> 48,
            0,
            "test premise: {label} must NOT be bare-shaped, or this test is \
             asserting the wrong side of the boundary"
        );
        assert!(
            !JSValue::from_bits(bits).is_pointer(),
            "{label}: not POINTER_TAG, so no `as_pointer()`-masking probe can \
             reach it as an address"
        );
        let out = unsafe { canonicalize_bare_gc_receiver(value) };
        assert_eq!(out.to_bits(), bits, "{label} must be returned unchanged");
        assert!(
            !is_unvouched_bare_word(out),
            "{label} must not be routed to number dispatch — it was never \
             address-shaped"
        );
    }
}

/// The other direction of the same predicate: a real managed allocation must
/// NOT be routed to number dispatch, or every bare receiver starts throwing.
#[test]
fn vouched_bare_receivers_are_not_routed_to_number_dispatch() {
    let mut limbs = [0u64; crate::bigint::BIGINT_LIMBS];
    limbs[0] = 3;
    let candidates = [
        (
            "string",
            crate::string::js_string_from_str("abcdef") as usize,
        ),
        (
            "bigint",
            crate::bigint::bigint_alloc_with_limbs(limbs) as usize,
        ),
        ("object", crate::object::js_object_alloc(0, 4) as usize),
        ("array", crate::array::js_array_alloc(4) as usize),
    ];
    for (what, addr) in candidates {
        assert_bare_premise(addr, what);
        let canonical = unsafe { canonicalize_bare_gc_receiver(bare(addr)) };
        assert!(
            !is_unvouched_bare_word(canonical),
            "{what}: the allocator vouched for this address, so it must reach \
             the dispatch tower, not the number arm"
        );
    }
}

/// `+0.0` is not address-shaped and no probe can mistake it for a pointer, so it
/// deliberately stays on the ordinary path — this change is scoped to the words
/// that actually alias addresses.
#[test]
fn positive_zero_is_not_routed_to_number_dispatch() {
    assert!(!is_unvouched_bare_word(0.0f64));
}

/// A headerless registry allocation has no `GcHeader` for the allocator gate to
/// find, but its owning registry can answer for the address without touching
/// memory. Such a receiver must be reboxed as a POINTER, not sent to the number
/// arm — that is the case the deleted tail recovery legitimately served.
#[test]
fn headerless_registry_allocations_are_vouched_as_pointers() {
    // `Symbol.for` leaks a `Box` with no GcHeader; a unique key guarantees this
    // thread allocates and registers it (see `probe_dispatch_tests`).
    let key = crate::string::js_string_from_str("perry-9675-vouch");
    let key_f64 = f64::from_bits(crate::value::js_nanbox_string(key as i64).to_bits());
    let addr = unsafe { crate::value::js_nanbox_get_pointer(crate::symbol::js_symbol_for(key_f64)) }
        as usize;
    assert_bare_premise(addr, "a Symbol.for symbol");
    assert!(
        crate::symbol::is_registered_symbol(addr),
        "test premise: Symbol.for registered on this thread; otherwise no owner \
         can vouch and this test proves nothing"
    );
    assert!(
        unsafe { crate::value::addr_class::try_read_tracked_gc_header(addr) }.is_none(),
        "test premise: a Box-leaked symbol is NOT an allocator-tracked \
         allocation — that is what makes the registry arm load-bearing"
    );

    let canonical = unsafe { canonicalize_bare_gc_receiver(bare(addr)) };
    assert!(
        !is_unvouched_bare_word(canonical),
        "a registered symbol must be vouched for, not dispatched as a number"
    );
    assert_eq!(
        tag_of(canonical),
        POINTER_TAG,
        "a headerless registry allocation has no GcHeader to read a kind from \
         and is boxed as a POINTER"
    );
    assert_eq!(payload_of(canonical), addr);
}

/// The ordinary path pays one compare. Every NaN-boxed receiver — including
/// `INT32_TAG`, which shares its high half with nothing else, and a real
/// pointer, which is already canonical — comes back untouched.
#[test]
fn nanboxed_receivers_are_returned_unchanged() {
    let object = crate::object::js_object_alloc(0, 4) as usize;
    let string = crate::string::js_string_from_str("hello") as usize;
    let probes = [
        ("undefined", crate::value::TAG_UNDEFINED),
        ("null", crate::value::TAG_NULL),
        ("true", crate::value::TAG_TRUE),
        ("false", crate::value::TAG_FALSE),
        ("int32 0", crate::value::INT32_TAG),
        ("int32 42", crate::value::INT32_TAG | 42),
        ("double 3.5", 3.5f64.to_bits()),
        ("double -1.0", (-1.0f64).to_bits()),
        ("double 1e300", 1e300f64.to_bits()),
        ("nan", f64::NAN.to_bits()),
        ("pointer", POINTER_TAG | object as u64),
        ("string", STRING_TAG | string as u64),
    ];
    for (what, bits) in probes {
        let out = unsafe { canonicalize_bare_gc_receiver(f64::from_bits(bits)) };
        assert_eq!(
            out.to_bits(),
            bits,
            "{what} is already NaN-boxed and must be returned unchanged"
        );
    }
}
