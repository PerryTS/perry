//! The miss entry of the generated static-key store, and the SHAPE RULES its
//! inline hit rests on. The hit compares one ShapeId and stores; every
//! assertion below is a way a receiver could stop being a plain writable own
//! data slot, and each one must show up as a ShapeId the published word can no
//! longer match — or as a refusal to publish at all.
use super::*;
use std::sync::atomic::{AtomicU64, Ordering};

fn fnv1a(bytes: &[u8]) -> u64 {
    bytes.iter().fold(0xcbf2_9ce4_8422_2325u64, |hash, byte| {
        (hash ^ u64::from(*byte)).wrapping_mul(0x0000_0100_0000_01b3)
    })
}

fn interned(name: &[u8]) -> *const crate::StringHeader {
    let key = crate::string::js_string_from_bytes(name.as_ptr(), name.len() as u32);
    crate::string::js_string_intern(key, fnv1a(name))
}

fn boxed_string(key: *const crate::StringHeader) -> f64 {
    f64::from_bits(crate::value::js_nanbox_string(key as i64).to_bits())
}

/// A JSON-parsed object: class-less, birth-marked ordinary, stamped with a
/// real ShapeId, and sharing it with every other parse of the same text.
fn parsed(src: &[u8]) -> f64 {
    let text = crate::string::js_string_from_bytes(src.as_ptr(), src.len() as u32);
    let value = unsafe { crate::json::js_json_parse(text) };
    assert!(value.is_pointer(), "JSON.parse must yield an object");
    f64::from_bits(value.bits())
}

fn object_of(value: f64) -> *mut crate::ObjectHeader {
    (value.to_bits() & POINTER_MASK) as *mut crate::ObjectHeader
}

fn stamp(value: f64) -> u32 {
    unsafe { crate::object::shapes::object_shape_stamp(object_of(value)) }
}

fn header(value: f64) -> *mut crate::gc::GcHeader {
    unsafe {
        (object_of(value) as *mut u8).sub(crate::gc::GC_HEADER_SIZE) as *mut crate::gc::GcHeader
    }
}

/// One store through the miss entry with a fresh site; returns the word.
fn store_fresh(target: f64, key: *const crate::StringHeader, value: f64) -> (f64, u64) {
    let packed_site = PackedSetSite::empty();
    let packed = &packed_site.set;
    let mut cache: PackedSetWays = packed_set_cache_empty();
    let mut cache_slot: PackedSetWaysSlot = &mut cache;
    let stored = js_put_value_set_packed_miss(target, key, value, 0, &mut cache_slot, packed);
    (stored, packed.load(Ordering::Relaxed))
}

const SRC: &[u8] = br#"{"a":1,"n":2,"b":3}"#;

#[test]
fn packed_set_empty_matches_codegen() {
    // perry-codegen `expr/put_value_store_ic.rs::PACKED_SET_EMPTY`.
    assert_eq!(PACKED_SET_EMPTY, 0xFFFF_FFFF);
    // It must be unmatchable by any receiver's `+4` word: above the ShapeId
    // range, and `u32::MAX` is reserved as a never-allocated class id.
    assert!(PACKED_SET_EMPTY as u32 >= crate::object::shapes::SHAPE_ID_END);
    assert_eq!(PACKED_SET_EMPTY as u32, u32::MAX);
    assert!(!crate::object::shapes::is_site_matchable_shape_id(
        PACKED_SET_EMPTY as u32
    ));
}

#[test]
fn an_ordinary_receiver_publishes_its_shape_and_inline_slot() {
    let target = parsed(SRC);
    let key = interned(b"n");
    let (stored, word) = store_fresh(target, key, 7.0);
    assert_eq!(stored, 7.0, "the miss performs the store");
    assert_eq!(
        word as u32,
        stamp(target),
        "low half: the receiver's ShapeId"
    );
    assert_eq!(word >> 32, 1, "high half: `n` is the second own slot");
    // The discriminating half: the SAME receiver with the ordinary mark
    // cleared is a class-less receiver the hit path does not admit, so the
    // entry must not publish it either.
    unsafe {
        (*header(target))._reserved &= !crate::gc::OBJ_FLAG_PLAIN_ORDINARY;
    }
    let (_, word) = store_fresh(target, key, 8.0);
    assert_eq!(
        word, PACKED_SET_EMPTY,
        "an unmarked class-less receiver must not publish"
    );
}

#[test]
fn per_object_facts_the_hit_retests_are_refused_at_publication() {
    let key = interned(b"n");
    for (what, flag) in [
        (
            "the typed-array prototype flag",
            crate::gc::OBJ_FLAG_TYPED_ARRAY_PROTO,
        ),
        (
            "the Array-subclass numeric proof",
            crate::gc::OBJ_FLAG_PACKED_NUMERIC_PROOF,
        ),
    ] {
        let target = parsed(SRC);
        unsafe {
            (*header(target))._reserved |= flag;
        }
        let packed = AtomicU64::new(PACKED_SET_EMPTY);
        unsafe { prime_packed_set(target, key, std::ptr::null_mut(), &packed) };
        assert_eq!(
            packed.load(Ordering::Relaxed),
            PACKED_SET_EMPTY,
            "a receiver carrying {what} must not publish"
        );
    }
    let target = parsed(SRC);
    unsafe {
        (*object_of(target)).class_id = crate::object::NATIVE_MODULE_CLASS_ID;
    }
    let packed = AtomicU64::new(PACKED_SET_EMPTY);
    unsafe { prime_packed_set(target, key, std::ptr::null_mut(), &packed) };
    assert_eq!(
        packed.load(Ordering::Relaxed),
        PACKED_SET_EMPTY,
        "a native-module receiver must not publish"
    );
}

/// Integrity: freezing/sealing/preventExtensions re-stamps the receiver with
/// a private ShapeId, so a word published from a sibling cannot match it, and
/// the entry refuses to publish the restricted receiver itself.
#[test]
fn integrity_operations_leave_the_published_shape() {
    let key = interned(b"n");
    let primer = parsed(SRC);
    let (_, word) = store_fresh(primer, key, 1.0);
    assert_eq!(word as u32, stamp(primer));
    for (what, op) in [
        (
            "freeze",
            crate::object::js_object_freeze as extern "C" fn(f64) -> f64,
        ),
        ("seal", crate::object::js_object_seal),
        (
            "preventExtensions",
            crate::object::js_object_prevent_extensions,
        ),
    ] {
        let sibling = parsed(SRC);
        assert_eq!(
            stamp(sibling),
            word as u32,
            "premise: same shape as the primer"
        );
        op(sibling);
        assert_ne!(
            stamp(sibling),
            word as u32,
            "{what} must move the receiver off the published ShapeId"
        );
        let (_, restricted_word) = store_fresh(sibling, key, 2.0);
        assert_eq!(
            restricted_word, PACKED_SET_EMPTY,
            "a receiver after {what} must not publish"
        );
    }
}

/// Rule 1 for stores: a non-writable data descriptor on the key re-stamps the
/// receiver, and the entry refuses to publish that key on it. A descriptor on
/// ANOTHER key leaves this key plain data, and publishing it is allowed.
#[test]
fn a_descriptor_on_the_key_leaves_the_published_shape() {
    let key = interned(b"n");
    let primer = parsed(SRC);
    let (_, word) = store_fresh(primer, key, 1.0);

    let receiver = parsed(SRC);
    assert_eq!(stamp(receiver), word as u32);
    let desc = parsed(br#"{"value":5,"writable":false}"#);
    crate::object::js_object_define_property(receiver, boxed_string(key), desc);
    assert_ne!(
        stamp(receiver),
        word as u32,
        "defineProperty(writable:false) must re-stamp the receiver"
    );
    let (stored, refused) = store_fresh(receiver, key, 9.0);
    assert_eq!(
        stored, 9.0,
        "a sloppy rejected store still yields its value"
    );
    assert_eq!(
        refused, PACKED_SET_EMPTY,
        "a non-writable key must not publish"
    );
    let n = crate::object::js_object_get_field_by_name(object_of(receiver), key);
    assert_eq!(
        f64::from_bits(n.bits()),
        5.0,
        "the non-writable slot kept its value"
    );
}

/// #10826: delete is a shape transition, so a deleted key cannot be hit.
#[test]
fn delete_leaves_the_published_shape() {
    let key = interned(b"n");
    let primer = parsed(SRC);
    let (_, word) = store_fresh(primer, key, 1.0);
    let receiver = parsed(SRC);
    assert_eq!(stamp(receiver), word as u32);
    crate::object::js_object_delete_field(object_of(receiver), key);
    assert_ne!(
        stamp(receiver),
        word as u32,
        "delete must re-stamp the receiver"
    );
}

/// A Proxy is a POINTER-tagged id in the handle band, which the emitted
/// small-handle test (`handle > 0xFFFFF`) refuses before any header load.
#[test]
fn proxies_live_below_the_small_handle_floor() {
    use crate::value::addr_class::{HANDLE_BAND_MAX, PROXY_ID_BAND_START};
    // The emitted receiver test refuses every payload below HANDLE_BAND_MAX
    // (`perry-codegen` spells it `2^48 - HANDLE_BAND_MAX` after the bias).
    assert!(PROXY_ID_BAND_START < HANDLE_BAND_MAX);
    let target = parsed(SRC);
    let handler = parsed(b"{}");
    let proxy = crate::proxy::js_proxy_new(target, handler);
    assert!((proxy.to_bits() & POINTER_MASK) < HANDLE_BAND_MAX as u64);
}

/// A forwarded cell keeps the new user address in its first eight payload
/// bytes; the `+4` word the hit compares is that address's high half, which
/// is below the ShapeId floor on every supported target.
#[test]
fn a_forwarded_cell_cannot_match_a_shape_id() {
    let target = parsed(SRC);
    let other = parsed(SRC);
    unsafe {
        let obj = object_of(target);
        let saved = *(obj as *const u64);
        let saved_flags = (*header(target)).gc_flags;
        crate::gc::set_forwarding_address(header(target), object_of(other) as *mut u8);
        let word_at_4 = *((obj as *const u8).add(4) as *const u32);
        assert!(
            word_at_4 < crate::object::shapes::SHAPE_ID_BASE,
            "a forwarded cell's +4 word {word_at_4:#x} must lie below the ShapeId floor"
        );
        *(obj as *mut u64) = saved;
        (*header(target)).gc_flags = saved_flags;
    }
}

/// The ways: a site that has seen two shapes keeps both, in the word's own
/// format, and a way hit served by the runtime never rewrites the word.
#[test]
fn a_second_shape_is_kept_in_the_ways_without_moving_the_word() {
    let key = interned(b"n");
    let first = parsed(SRC);
    let second = parsed(br#"{"n":2,"z":0}"#);
    assert_ne!(stamp(first), stamp(second));
    let packed_site = PackedSetSite::empty();
    let packed = &packed_site.set;
    let mut cache: PackedSetWays = packed_set_cache_empty();
    let mut cache_slot: PackedSetWaysSlot = &mut cache;
    js_put_value_set_packed_miss(first, key, 1.0, 0, &mut cache_slot, packed);
    js_put_value_set_packed_miss(second, key, 2.0, 0, &mut cache_slot, packed);
    assert_eq!(
        packed.load(Ordering::Relaxed) as u32,
        stamp(second),
        "MRU prime"
    );
    assert_eq!(
        cache[0],
        (1u64 << 32) | u64::from(stamp(first)),
        "way 0: first shape, slot 1"
    );
    assert_eq!(
        cache[1],
        u64::from(stamp(second)),
        "way 1: second shape, slot 0"
    );
    assert_eq!(cache[2], PACKED_SET_EMPTY, "no other way is written");
    // Ways 0..4 are the EMITTED code's to compare; with a null word (the
    // full-outline form) the runtime serves them itself, without a prime.
    let stored =
        js_put_value_set_packed_miss(first, key, 3.0, 0, &mut cache_slot, std::ptr::null());
    assert_eq!(stored, 3.0);
    assert_eq!(
        packed.load(Ordering::Relaxed) as u32,
        stamp(second),
        "a way hit must not rewrite the word"
    );
    let n = crate::object::js_object_get_field_by_name(object_of(first), key);
    assert_eq!(f64::from_bits(n.bits()), 3.0);
}

/// A fresh way cache must be born EMPTY, not zero: an unstamped class-less
/// receiver carries `parent_class_id == 0` at `+4`, which a zero way would
/// match in the emitted compare.
#[test]
fn a_fresh_way_cache_is_born_empty_not_zero() {
    let key = interned(b"n");
    let target = parsed(SRC);
    let packed_site = PackedSetSite::empty();
    let packed = &packed_site.set;
    let mut slot: PackedSetWaysSlot = std::ptr::null_mut();
    js_put_value_set_packed_miss(target, key, 1.0, 0, &mut slot, packed);
    assert!(!slot.is_null(), "the first prime allocates the way cache");
    let ways = unsafe { &*slot };
    assert_eq!(ways[0] as u32, stamp(target));
    for (i, w) in ways[..PACKED_SET_WAYS].iter().enumerate().skip(1) {
        assert_eq!(*w, PACKED_SET_EMPTY, "way {i} must be born EMPTY");
    }
    // The chain-entry word after the ways is a pointer word, not a way: it is
    // born 0 (no entry), and no emitted compare reads it.
    assert_eq!(
        ways[PACKED_SET_CHAIN_WORD], 0,
        "the chain word is born empty"
    );
}

#[test]
fn packed_set_inline_ways_matches_codegen() {
    // perry-codegen `expr/put_value_store_ic.rs::PACKED_SET_INLINE_WAYS`.
    assert_eq!(PACKED_SET_INLINE_WAYS, 4);
    assert!(PACKED_SET_INLINE_WAYS <= PACKED_SET_WAYS);
}

/// The per-object receiver test the emitted hit (and this entry's ways 4..8)
/// re-reads on every store, because the ShapeId does not carry it: a
/// class-less receiver shares ShapeIds with the exotic class-less objects
/// (`URL`, `Object.prototype`, the typed-array prototypes), and only a birth
/// site's ordinary mark admits it. Same ShapeId, same slot, same way — only
/// the per-object facts differ, so each refusal below is the test itself.
#[test]
fn a_matching_shape_is_not_enough_without_the_receiver_kind() {
    let key = interned(b"n");
    let marked = parsed(SRC);
    let (_, word) = store_fresh(marked, key, 1.0);
    assert_eq!(word as u32, stamp(marked));
    let ways: [AtomicU64; PACKED_SET_WAYS] =
        std::array::from_fn(|_| AtomicU64::new(PACKED_SET_EMPTY));
    ways[0].store(word, Ordering::Relaxed);

    let served = |target: f64, v: f64| unsafe { packed_ways_store(&ways, 0, target, v) };
    let other = parsed(SRC);
    assert_eq!(stamp(other), word as u32, "premise: a shape twin");
    assert_eq!(
        served(other, 5.0),
        Some(5.0),
        "a marked ordinary twin is served"
    );

    let unmarked = parsed(SRC);
    unsafe { (*header(unmarked))._reserved &= !crate::gc::OBJ_FLAG_PLAIN_ORDINARY };
    assert_eq!(stamp(unmarked), word as u32);
    assert_eq!(
        served(unmarked, 6.0),
        None,
        "an UNMARKED class-less twin must not be served"
    );

    let proto = parsed(SRC);
    unsafe { (*header(proto))._reserved |= crate::gc::OBJ_FLAG_TYPED_ARRAY_PROTO };
    assert_eq!(
        served(proto, 7.0),
        None,
        "a typed-array prototype twin must not be served"
    );

    let proof = parsed(SRC);
    unsafe { (*header(proof))._reserved |= crate::gc::OBJ_FLAG_PACKED_NUMERIC_PROOF };
    assert_eq!(
        served(proof, 8.0),
        None,
        "a receiver carrying the numeric proof must not be served"
    );

    let native = parsed(SRC);
    unsafe { (*object_of(native)).class_id = crate::object::NATIVE_MODULE_CLASS_ID };
    assert_eq!(
        served(native, 9.0),
        None,
        "a native-module twin must not be served"
    );
}

/// Real code's keys are often heap strings the intern table never saw (tsc
/// and Zod: most store sites). The slot is found by CONTENT, so such a key
/// publishes too — only the pointer-keyed read-plan memo is skipped for it.
#[test]
fn a_non_interned_key_publishes_by_content() {
    let target = parsed(SRC);
    let key = crate::string::js_string_from_bytes(b"n".as_ptr(), 1);
    let gc =
        unsafe { crate::value::addr_class::try_read_gc_header(key as usize) }.expect("a heap key");
    if gc.gc_flags & crate::gc::GC_FLAG_INTERNED != 0 {
        // A short literal may already be interned; nothing to prove then.
        return;
    }
    let (stored, word) = store_fresh(target, key, 4.0);
    assert_eq!(stored, 4.0);
    assert_eq!(
        word as u32,
        stamp(target),
        "a non-interned key must still publish"
    );
    assert_eq!(word >> 32, 1);
}

/// A static-key store that ADDS a key to a class instance misses into this
/// entry on every call (the emitted hit only serves existing own slots), so
/// this is where the inherited-access chain verdict must run. Since #11241 no
/// static-key store reaches the write-PIC miss entries at all: a verdict wired
/// only there fires zero times while every trap test stays green. Asserts the
/// verdict is primed on THIS site and then serves the later stores.
#[test]
fn a_key_adding_static_store_is_served_by_the_chain_verdict() {
    const CID: u32 = 0x0004_2217;
    let first = interned(b"chainPackedFirst");
    let added = interned(b"chainPackedAdded");
    let mut first_cache: PackedSetWays = packed_set_cache_empty();
    let mut first_slot: PackedSetWaysSlot = &mut first_cache;
    let first_packed = AtomicU64::new(PACKED_SET_EMPTY);
    let mut cache: PackedSetWays = packed_set_cache_empty();
    let mut cache_slot: PackedSetWaysSlot = &mut cache;
    let packed = AtomicU64::new(PACKED_SET_EMPTY);
    // A class with a prototype object, as a compiled class has: the verdict
    // walks (and marks) the chain it proves clear, so a class with no
    // prototype to walk is refused.
    let proto = crate::object::js_object_alloc(0, 4);
    crate::object::class_prototype_object_root_store(CID, proto);
    let before = crate::object::chain_store::chain_store_hits_this_thread();
    const STORES: u64 = 6;
    for i in 0..STORES {
        let obj = crate::object::js_object_alloc(CID, 0);
        let target = f64::from_bits(crate::value::js_nanbox_pointer(obj as i64).to_bits());
        // One key first, so the receiver the chain-verdict site sees carries a
        // real ShapeId, as a constructor's receiver does.
        js_put_value_set_packed_miss(target, first, 1.0, 0, &mut first_slot, &first_packed);
        js_put_value_set_packed_miss(target, added, i as f64, 0, &mut cache_slot, &packed);
        let read = crate::object::js_object_get_field_by_name_f64(obj, added);
        assert_eq!(read, i as f64, "the added key holds the stored value");
    }
    assert_ne!(
        cache[PACKED_SET_CHAIN_WORD],
        0,
        "the site primed a chain verdict in its own cache: {}",
        crate::object::chain_store::chain_store_counters_for_test()
    );
    let served = crate::object::chain_store::chain_store_hits_this_thread() - before;
    assert!(
        served >= STORES - 1,
        "every store after the prime is served by the chain verdict: {served} of {STORES}"
    );
}

/// S6 on the store side: a dictionary-mode receiver must never publish a store
/// site word. Its ShapeId survives appends and in-place deletes, so no
/// `(ShapeId, slot)` is a fact of that id. The control is the same receiver
/// before the latch, which does publish. Two independent facts refuse it: the
/// dictionary shape publishes no key list (so the prime finds no slot), and the
/// prime admits only a matchable ShapeId. This pins the outcome; it goes red
/// only if BOTH are removed, which is why the band check is not the only
/// guard here.
#[test]
fn a_dictionary_receiver_never_publishes_a_store_site_word() {
    let _lock = crate::gc::global_side_table_test_lock();
    let _no_gc = crate::gc::GcSuppressScope::new();
    let target = parsed(SRC);
    let key = interned(b"n");
    let (_, word) = store_fresh(target, key, 7.0);
    assert_eq!(
        word as u32,
        stamp(target),
        "control: an ordinary receiver publishes"
    );
    assert!(unsafe { crate::object::dictionary::latch_object_to_dictionary(object_of(target)) });
    let id = stamp(target);
    assert!(
        !crate::object::shapes::is_site_matchable_shape_id(id),
        "premise: the dictionary id is outside the matchable band: {id:#x}"
    );
    let (stored, word) = store_fresh(target, key, 8.0);
    assert_eq!(stored, 8.0, "the miss still performs the store");
    assert_eq!(
        word, PACKED_SET_EMPTY,
        "a dictionary receiver must not publish"
    );
}
