//! Charter step 5, T1: a class's birth shape carries the representation
//! codegen declared for it. One test per guarantee P4's shape-only guard
//! rests on:
//! (a) the birth id is `F64` for exactly the declared lanes, the allocator
//!     birth-fills them, and the reverse typed-layout cross-check refuses an
//!     intact raw-f64 slot the birth id leaves `Any`;
//! (b) generalizing an instance moves the INSTANCE, never the class's id:
//!     the id keeps its identity and the next birth still carries it (an
//!     importer's all-`Any` stub never adopts an `F64` id: the rep is part of
//!     the static id's content, `static_shape_ids`);
//! (c) a non-Number or non-finite value bound for an `F64` birth lane goes
//!     through the checked funnel (generalize / canonicalize), never raw;
//! (d) the runtime takes the rep from the mint's argument and nowhere else:
//!     the same keys and class minted with two reps are two identities;
//! (e) one birth shape per class and literal: every allocation path, inline
//!     or outlined, stamps the one (keys, proto, rep) id and fills its `F64`
//!     lanes.

use super::field_rep::{slot_rep, REP_ANY, REP_F64};
use super::shapes::{
    js_object_shape_id_for_class_keys, object_shape_stamp, shape_descriptor_by_id,
};
use super::ObjectHeader;

const CID: u32 = 0x5117;
const F64_A_B: u64 = REP_F64 | (REP_F64 << 2);

fn keys(packed: &[u8], count: u32) -> u64 {
    crate::object::js_build_class_keys_array(CID, count, packed.as_ptr(), packed.len() as u32, 0)
        as usize as u64
}

fn rep_of(id: u32) -> u64 {
    shape_descriptor_by_id(id).expect("live shape").rep
}

unsafe fn slot_bits(obj: *mut ObjectHeader, index: usize) -> u64 {
    let fields = (obj as *mut u8).add(std::mem::size_of::<ObjectHeader>()) as *const u64;
    *fields.add(index)
}

unsafe fn birth(keys: u64, count: u32, id: u32) -> *mut ObjectHeader {
    crate::object::js_object_alloc_class_inline_keys_stamped(
        CID,
        0,
        count,
        keys as usize as *mut crate::array::ArrayHeader,
        id,
        super::field_rep::identity(rep_of(id)),
    )
}

/// (a) + (d): the birth id carries exactly the rep it was minted with, a rep
/// is identity (two reps, two ids), and the stamped allocator fills the
/// `F64` lanes with +0.0 while every `Any` lane keeps `undefined`.
#[test]
fn a_class_birth_id_carries_its_minted_rep_and_births_fill_its_f64_lanes() {
    let k = keys(b"a\0b\0c\0", 3);
    let typed = js_object_shape_id_for_class_keys(k, 3, CID, F64_A_B);
    let untyped = js_object_shape_id_for_class_keys(k, 3, CID, REP_ANY);
    assert_ne!(typed, untyped, "the rep is shape identity");
    assert_eq!(rep_of(typed), F64_A_B);
    assert_eq!(rep_of(untyped), REP_ANY);
    assert_eq!(js_object_shape_id_for_class_keys(k, 3, CID, F64_A_B), typed);
    unsafe {
        let obj = birth(k, 3, typed);
        assert_eq!(object_shape_stamp(obj), typed);
        assert_eq!(slot_bits(obj, 0), 0.0f64.to_bits());
        assert_eq!(slot_bits(obj, 1), 0.0f64.to_bits());
        assert_eq!(slot_bits(obj, 2), crate::value::TAG_UNDEFINED);
        let plain = birth(k, 3, untyped);
        assert_eq!(slot_bits(plain, 0), crate::value::TAG_UNDEFINED);
    }
}

/// (b) + (c): a non-Number stored into an `F64` birth lane goes through the
/// checked funnel: the instance moves off the birth id to a shape whose lane
/// is `Any`; the birth id keeps its `F64` identity and the next birth still
/// gets it (the class's id global never needs to move). NaN / Infinity keep
/// the lane and are stored canonical.
#[test]
fn generalizing_an_instance_moves_the_instance_not_the_class_birth_id() {
    let k = keys(b"u\0v\0", 2);
    let id = js_object_shape_id_for_class_keys(k, 2, CID, F64_A_B);
    unsafe {
        let obj = birth(k, 2, id);
        let key_v = crate::string::js_string_from_bytes(b"v".as_ptr(), 1);
        crate::object::js_object_set_field_by_name(obj, key_v, f64::INFINITY);
        assert_eq!(
            object_shape_stamp(obj),
            id,
            "Infinity is a Number: lane kept"
        );
        assert_eq!(slot_bits(obj, 1), f64::INFINITY.to_bits());
        let key_u = crate::string::js_string_from_bytes(b"u".as_ptr(), 1);
        let s = crate::string::js_string_from_bytes(b"not a number".as_ptr(), 12);
        let boxed = f64::from_bits(crate::value::js_nanbox_string(s as i64).to_bits());
        crate::object::js_object_set_field_by_name(obj, key_u, boxed);
        let moved = object_shape_stamp(obj);
        assert_ne!(moved, id, "the instance left the birth id");
        assert_eq!(slot_rep(rep_of(moved), 0), REP_ANY);
        assert_eq!(slot_rep(rep_of(moved), 1), REP_F64);
        assert_eq!(
            super::field_rep::identity(rep_of(id)),
            F64_A_B,
            "the birth id's identity never changes"
        );
        let next = birth(k, 2, id);
        assert_eq!(
            object_shape_stamp(next),
            id,
            "the next birth still gets the F64 id"
        );
        assert_eq!(slot_bits(next, 0), 0.0f64.to_bits());
    }
}

/// A loop region's bare store runs no field-representation check, so its
/// word must not admit a store of a value not proven a canonical double
/// (`boxed_mask`) into a non-`Any` lane; a proven double, or an `Any` lane,
/// packs as before.
#[test]
fn a_region_word_refuses_a_boxed_store_into_an_f64_lane() {
    use super::shapes::{js_region_loop_pack, REGION_GUARD_WORD_EMPTY};
    let k = keys(b"ra\0rb\0", 2);
    let typed = js_object_shape_id_for_class_keys(k, 2, CID, REP_F64);
    let untyped = js_object_shape_id_for_class_keys(k, 2, CID, REP_ANY);
    let (ra, rb) = unsafe {
        let (slots, len) = crate::object::keys_array_dense_slots_resolved(
            k as usize as *const crate::array::ArrayHeader,
        );
        assert!(len >= 2);
        ((*slots).to_bits(), (*slots.add(1)).to_bits())
    };
    let pack = |id: u32, key: u64, stored: u32, boxed: u32| {
        js_region_loop_pack(id, 1, key, 0, 0, 0, 0, stored, boxed)
    };
    assert_eq!(
        pack(typed, ra, 1, 1),
        REGION_GUARD_WORD_EMPTY,
        "a boxed store into the F64 lane of `ra` is refused"
    );
    assert_ne!(
        pack(typed, ra, 1, 0),
        REGION_GUARD_WORD_EMPTY,
        "a proven double packs"
    );
    assert_ne!(
        pack(typed, rb, 1, 1),
        REGION_GUARD_WORD_EMPTY,
        "`rb` is an Any lane"
    );
    assert_ne!(
        pack(untyped, ra, 1, 1),
        REGION_GUARD_WORD_EMPTY,
        "an all-Any shape packs"
    );
}

/// P7: a learned region publishes a Number-read (R) word for an exact F64
/// identity lane as it is. On any other non-SPECIAL inline lane (an `Any`
/// lane, or a deprecated F64 one) it publishes the word with
/// `REGION_LOOP_WORD_VALUE_TEST`, so the emitted guard tests the slot's value
/// on the object before F runs. An R key a bare store may write a non-Number
/// into is refused: no guard-time test could cover that store.
#[test]
fn a_region_prime_value_tests_a_requested_number_read_on_a_non_identity_lane() {
    use super::shapes::{
        js_region_loop_prime, REGION_GUARD_WORD_EMPTY, REGION_LOOP_WORD_VALUE_TEST,
    };
    use core::sync::atomic::{AtomicU64, Ordering};

    let k = keys(b"p7a\0p7b\0", 2);
    let typed = js_object_shape_id_for_class_keys(k, 2, CID, REP_F64);
    let untyped = js_object_shape_id_for_class_keys(k, 2, CID, REP_ANY);
    let (a, b) = unsafe {
        let (slots, len) = crate::object::keys_array_dense_slots_resolved(
            k as usize as *const crate::array::ArrayHeader,
        );
        assert!(len >= 2);
        ((*slots).to_bits(), (*slots.add(1)).to_bits())
    };
    let site = AtomicU64::new(REGION_GUARD_WORD_EMPTY);
    let prime = |id, key, stored, boxed, r_mask| unsafe {
        js_region_loop_prime(&site, id, 1, key, 0, 0, 0, 0, 0, stored, boxed, r_mask)
    };
    let tested = |word: u64| word & REGION_LOOP_WORD_VALUE_TEST != 0;

    // An Any lane: published, and the guard must test the value.
    let word = prime(untyped, a, 0, 0, 1);
    assert_ne!(word, REGION_GUARD_WORD_EMPTY);
    assert_eq!(site.load(Ordering::Relaxed), word);
    assert_eq!(word as u32, untyped);
    assert!(tested(word), "R on an Any lane must ask for a value test");
    // The same key read without R asks for nothing.
    assert!(!tested(prime(untyped, a, 0, 0, 0)));
    // The typed shape's second key is an Any lane; its first is identity F64.
    assert!(tested(prime(typed, b, 0, 0, 1)));
    let word = prime(typed, a, 0, 0, 1);
    assert_ne!(word, REGION_GUARD_WORD_EMPTY);
    assert!(!tested(word), "an identity F64 lane needs no value test");

    // A bare store that may write a non-Number into an R key: refused. The
    // same store without R is admitted, so the refusal is the R + boxed pair.
    assert_ne!(prime(untyped, a, 1, 1, 0), REGION_GUARD_WORD_EMPTY);
    site.store(REGION_GUARD_WORD_EMPTY, Ordering::Relaxed);
    assert_eq!(prime(untyped, a, 1, 1, 1), REGION_GUARD_WORD_EMPTY);
    assert_eq!(site.load(Ordering::Relaxed), REGION_GUARD_WORD_EMPTY);

    // A deprecated lane is no longer an identity fact for a new learned
    // region: its R is served by the value test.
    assert!(super::shapes::shape_record_by_id(typed)
        .expect("typed shape record")
        .deprecate_rep_slot(0));
    let word = prime(typed, a, 0, 0, 1);
    assert_ne!(word, REGION_GUARD_WORD_EMPTY);
    assert!(tested(word), "a deprecated lane must ask for a value test");
}

/// Design step 4 x T1: the rep is part of a static id's content, so a class
/// birth with an `F64` lane adopts its static id like an all-`Any` one, and
/// the same keys with the other rep are another content under another id.
#[test]
fn a_class_birth_with_an_f64_lane_adopts_its_static_id() {
    use super::static_shapes::js_object_shape_id_for_class_keys_static;
    let k = keys(b"sa\0sb\0", 2);
    let f64_id = crate::object::shapes::SHAPE_ID_BASE + 0x3a61;
    let any_id = crate::object::shapes::SHAPE_ID_BASE + 0x3a62;
    let typed = js_object_shape_id_for_class_keys_static(k, 2, 2, CID, f64_id, REP_F64);
    assert_eq!(typed, f64_id, "the F64 birth adopts its static id");
    assert_eq!(rep_of(typed), REP_F64);
    let untyped = js_object_shape_id_for_class_keys_static(k, 2, 2, CID, any_id, REP_ANY);
    assert_eq!(untyped, any_id, "the all-Any content is another id");
    assert_eq!(
        js_object_shape_id_for_class_keys_static(k, 2, 2, CID, f64_id, REP_F64),
        typed,
        "a second registration resolves to the same id"
    );
}

/// An importing module can initialize before its defining module. Its
/// all-Any stub must take its own static id and leave every slot undefined;
/// the later definer's F64 birth keeps its distinct id and +0.0 birth fill.
#[test]
fn importer_first_birth_keeps_definer_rep_and_stub_slots_distinct() {
    use super::static_shapes::js_object_shape_id_for_class_keys_static;
    let k = keys(b"ifa\0ifb\0", 2);
    let any_id = crate::object::shapes::SHAPE_ID_BASE + 0x3a71;
    let f64_id = crate::object::shapes::SHAPE_ID_BASE + 0x3a72;

    let importer = js_object_shape_id_for_class_keys_static(k, 2, 2, CID, any_id, REP_ANY);
    assert_eq!(importer, any_id);
    let imported_birth = unsafe { birth(k, 2, importer) };
    assert_eq!(unsafe { object_shape_stamp(imported_birth) }, any_id);
    assert_eq!(
        unsafe { slot_bits(imported_birth, 0) },
        crate::value::TAG_UNDEFINED
    );

    let definer = js_object_shape_id_for_class_keys_static(k, 2, 2, CID, f64_id, REP_F64);
    assert_eq!(definer, f64_id);
    assert_ne!(definer, importer);
    let defined_birth = unsafe { birth(k, 2, definer) };
    assert_eq!(unsafe { object_shape_stamp(defined_birth) }, f64_id);
    assert_eq!(unsafe { slot_bits(defined_birth, 0) }, 0.0f64.to_bits());
    assert_eq!(
        unsafe { slot_bits(defined_birth, 1) },
        crate::value::TAG_UNDEFINED
    );
    assert_eq!(rep_of(importer), REP_ANY);
    assert_eq!(rep_of(definer), REP_F64);
}

/// (e) The shape is the truth: a literal (anonymous shape class) or a class
/// born with `F64` lanes has ONE birth shape, whichever allocator runs. The
/// compiled inline `new` stamps the module-init birth id (the stamped
/// allocator here, which stamps the same id); the outlined births — the
/// shape-cache allocator (`js_object_alloc_class_with_keys`) and the runtime
/// construct path from the class memo (`alloc_class_instance_with_keys`,
/// `new` of a class value, `Reflect.construct`) — carry the birth rep and so
/// land on that id too, with every `F64` lane a canonical double from birth
/// and after a Number store (an INT32 immediate included). For the literal,
/// the shape-cache mint beside the keys IS the birth shape (so a seed-less
/// static request of it misses, `static_shape_seeds`); for a named class it
/// is the default-prototype sibling, and the birth still carries the rep.
#[test]
fn every_birth_path_of_a_rep_literal_and_class_stamps_one_shape_and_fills_its_f64_lanes() {
    for (cid, literal) in [(0x511A_u32, true), (0x511B_u32, false)] {
        if literal {
            unsafe { crate::object::js_register_anon_shape_class_id(cid) };
        }
        let packed = b"pa\0pb\0pc\0";
        let k = crate::object::js_build_class_keys_array(
            cid,
            3,
            packed.as_ptr(),
            packed.len() as u32,
            F64_A_B,
        ) as usize as u64;
        // Module init's birth id (what the inline `new` bakes in).
        let id = js_object_shape_id_for_class_keys(k, 3, cid, F64_A_B);
        assert_eq!(rep_of(id), F64_A_B);
        let (_, cache_id) =
            super::shape_cache_get_with_id(super::alloc_plain::class_keys_cache_slot(cid, 3));
        assert_eq!(rep_of(cache_id), F64_A_B, "the cache mint carries the rep");
        assert_eq!(
            cache_id == id,
            literal,
            "a literal's cache mint is its birth shape; a class's is its sibling"
        );
        unsafe {
            let inline = crate::object::js_object_alloc_class_inline_keys_stamped(
                cid,
                0,
                3,
                k as usize as *mut crate::array::ArrayHeader,
                id,
                F64_A_B,
            );
            let cached = crate::object::js_object_alloc_class_with_keys(
                cid,
                0,
                3,
                packed.as_ptr(),
                packed.len() as u32,
            );
            let (memo_keys, memo_count) =
                crate::object::registered_class_keys_array(cid).expect("class memo");
            let constructed =
                crate::object::alloc::alloc_class_instance_with_keys(cid, 0, memo_count, memo_keys);
            for (path, obj) in [
                ("inline", inline),
                ("shape cache", cached),
                ("construct", constructed),
            ] {
                assert_eq!(
                    object_shape_stamp(obj),
                    id,
                    "{path} (literal={literal}): one birth shape"
                );
                assert_eq!(slot_bits(obj, 0), 0.0f64.to_bits(), "{path}: F64 lane");
                assert_eq!(slot_bits(obj, 1), 0.0f64.to_bits(), "{path}: F64 lane");
                // An INT32 immediate through the funnel: lane kept, canonical.
                crate::object::store_object_field_slot(obj, 1, crate::value::INT32_TAG | 7);
                assert_eq!(
                    object_shape_stamp(obj),
                    id,
                    "{path}: a Number keeps the lane"
                );
                assert_eq!(
                    slot_bits(obj, 1),
                    7.0f64.to_bits(),
                    "{path}: canonical double"
                );
            }
        }
    }
}

/// A completed CF shape can serve an unrelated numeric lane, but a raw
/// Number store cannot preserve the method body's invariant. In particular,
/// boxed_mask=0 is not permission to bypass the checked SPECIAL store funnel.
#[test]
fn a_region_constfn_shape_keeps_numeric_admission_and_refuses_special_stores() {
    use super::field_rep::REP_SPECIAL;
    use super::shapes::{js_region_loop_pack, js_region_loop_prime, REGION_GUARD_WORD_EMPTY};
    use super::static_shapes::{
        js_object_final_shape_id_for_class_keys_static_constfn, ConstFnStaticEntry,
    };
    use core::sync::atomic::{AtomicU64, Ordering};

    extern "C" fn body(
        _closure: *const crate::closure::ClosureHeader,
        _this: crate::closure::JsThis,
    ) -> f64 {
        7.0
    }
    let _lock = crate::gc::global_side_table_test_lock();
    let info = crate::fn_info!(body, 0; with_flags(crate::codegen_abi::FN_PERMANENT_IMAGE));
    let entries = [ConstFnStaticEntry { slot: 0, info }];
    let k = keys(b"p7cf_method\0p7cf_x\0", 2);
    let rep = REP_SPECIAL | (REP_F64 << 2);
    let completed = js_object_final_shape_id_for_class_keys_static_constfn(
        k,
        2,
        2,
        CID,
        0,
        rep,
        entries.as_ptr(),
        1,
    );
    let descriptor = shape_descriptor_by_id(completed).expect("completed CF record");
    assert_eq!(descriptor.special_constfn_mask, 1);
    assert_eq!(descriptor.rep, rep);
    assert_eq!(descriptor.constfn_infos()[0].info, info as usize as u64);
    let (method, x) = unsafe {
        let (slots, len) = crate::object::keys_array_dense_slots_resolved(
            k as usize as *const crate::array::ArrayHeader,
        );
        assert!(len >= 2);
        ((*slots).to_bits(), (*slots.add(1)).to_bits())
    };
    let pack = |stored_mask| js_region_loop_pack(completed, 2, method, x, 0, 0, 0, stored_mask, 0);
    assert_ne!(
        pack(0),
        REGION_GUARD_WORD_EMPTY,
        "read-only CF keys remain admitted"
    );
    assert_ne!(
        pack(2),
        REGION_GUARD_WORD_EMPTY,
        "Number store to x remains admitted"
    );
    assert_eq!(
        pack(1),
        REGION_GUARD_WORD_EMPTY,
        "even a Number cannot bare-store the CF method"
    );
    let site = AtomicU64::new(REGION_GUARD_WORD_EMPTY);
    let prime = |stored_mask, r_mask| unsafe {
        js_region_loop_prime(
            &site,
            completed,
            2,
            method,
            x,
            0,
            0,
            0,
            0,
            stored_mask,
            0,
            r_mask,
        )
    };
    assert_ne!(
        prime(2, 2),
        REGION_GUARD_WORD_EMPTY,
        "numeric x read/store really primes R"
    );
    site.store(REGION_GUARD_WORD_EMPTY, Ordering::Relaxed);
    assert_eq!(
        prime(1, 0),
        REGION_GUARD_WORD_EMPTY,
        "CF store refused even without R or boxed bits"
    );
    assert_eq!(site.load(Ordering::Relaxed), REGION_GUARD_WORD_EMPTY);
}
