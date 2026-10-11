//! The key-add memo: its ABI with the emitted hit, and the rules it rests on.
use super::*;
use std::sync::atomic::Ordering;

#[test]
fn packed_set_site_layout_matches_codegen() {
    // perry-codegen `expr/put_value_store_ic.rs`: PACKED_SET_SITE_WORDS,
    // ADD_SHAPES_WORD, ADD_GUARD_WORD, ADD_SLOT_BITS.
    assert_eq!(
        std::mem::size_of::<PackedSetSite>(),
        8 * PACKED_SET_SITE_WORDS
    );
    assert_eq!(PACKED_SET_SITE_WORDS, 5);
    assert_eq!(
        std::mem::size_of::<crate::object::shape_chain::ShapeChain>(),
        2 * std::mem::size_of::<usize>()
    );
    assert_eq!(
        std::mem::size_of::<crate::object::shape_chain::ShapeHop>(),
        crate::codegen_abi::SHAPE_CHAIN_HOP_BYTES
    );
    assert_eq!(std::mem::offset_of!(AddChain, links), 0);
    assert_eq!(
        std::mem::offset_of!(AddChain, rep),
        2 * std::mem::size_of::<usize>()
    );
    #[cfg(target_pointer_width = "64")]
    assert_eq!(
        std::mem::offset_of!(AddChain, rep),
        crate::codegen_abi::ADD_CHAIN_REP_OFFSET
    );
    let rep_word_offset = (3 * std::mem::size_of::<usize>() + 7) & !7;
    assert_eq!(std::mem::offset_of!(AddChain, rep_word), rep_word_offset);
    assert_eq!(std::mem::offset_of!(AddChain, body), rep_word_offset + 8);
    assert_eq!(std::mem::offset_of!(PackedSetSite, add_ways), 24);
    // perry_abi::PACKED_SET_CONSTFN_INFO_WORD: the site's ConstFn body,
    // compared by the emitted ConstFn store check.
    assert_eq!(
        std::mem::offset_of!(PackedSetSite, constfn_info),
        8 * crate::codegen_abi::PACKED_SET_CONSTFN_INFO_WORD
    );
    assert_eq!(crate::codegen_abi::PACKED_SET_CONSTFN_INFO_WORD, 4);
    // The guard's flag bits sit above the slot index: F64 = bit 15,
    // ConstFn = bit 14 (perry-codegen ADD_F64_SLOT / ADD_CONSTFN_SLOT).
    assert_eq!((ADD_F64_SLOT, ADD_CONSTFN_SLOT), (1 << 15, 1 << 14));
    assert_eq!(ADD_SLOT_MASK, (1 << 14) - 1);
    // The existing-key word: F64 = bit 63, ConstFn = bit 62, the index below.
    assert_eq!(
        (
            super::super::packed_set::PACKED_SET_F64_SLOT,
            super::super::packed_set::PACKED_SET_CONSTFN_SLOT
        ),
        (1 << 63, 1 << 62)
    );
    // The closure facts the emitted ConstFn check reads.
    assert_eq!(
        (
            crate::codegen_abi::CLOSURE_CAPTURES_THIS_FLAG,
            crate::codegen_abi::CLOSURE_NO_THIS_REBIND_FLAG
        ),
        (
            crate::closure::CAPTURES_THIS_FLAG,
            crate::closure::NO_THIS_REBIND_FLAG
        )
    );
    assert_eq!(
        std::mem::offset_of!(crate::closure::ClosureHeader, info),
        crate::codegen_abi::CLOSURE_INFO_OFFSET
    );
    assert_eq!(std::mem::offset_of!(PackedSetSite, set), 0);
    assert_eq!(
        std::mem::offset_of!(PackedSetSite, add_shapes),
        8 * ADD_SHAPES_WORD
    );
    assert_eq!(
        std::mem::offset_of!(PackedSetSite, add_guard),
        8 * ADD_GUARD_WORD
    );
    assert_eq!((ADD_SHAPES_WORD, ADD_GUARD_WORD, ADD_SLOT_BITS), (1, 2, 16));
    // ADD_WAYS_WORD, ADD_WAY_WORDS, ADD_WAYS_LOG2, ADD_WAY_HASH: the emitted
    // hit reads way `add_way_home(sid)` at `block + 8 * ADD_WAY_WORDS * i`,
    // the primary pair through the same pointer arithmetic from
    // `ADD_SHAPES_WORD`.
    assert_eq!(
        std::mem::offset_of!(PackedSetSite, add_ways),
        8 * ADD_WAYS_WORD
    );
    assert_eq!(std::mem::size_of::<AddWay>(), 8 * ADD_WAY_WORDS);
    assert_eq!(std::mem::offset_of!(AddWay, shapes), 0);
    assert_eq!(
        std::mem::offset_of!(AddWay, guard),
        std::mem::offset_of!(PackedSetSite, add_guard)
            - std::mem::offset_of!(PackedSetSite, add_shapes)
    );
    assert_eq!(
        (ADD_WAYS_WORD, ADD_WAY_WORDS, ADD_WAYS_LOG2, ADD_WAY_HASH),
        (3, 2, 6, 0x9E37_79B1)
    );
    // ADD_WAY_PROBES: the home and the next way.
    assert_eq!(ADD_WAY_PROBES, 2);
    assert_eq!(ADD_WAYS, 1 << ADD_WAYS_LOG2);
    // The home is a way of the block for every ShapeId.
    for sid in [0u32, 1, 0x8000_0000, 0x8000_0001, u32::MAX] {
        assert!(add_way_home(sid) < ADD_WAYS);
    }
    assert_eq!(
        add_way_home(0x8000_0001),
        (0x8000_0001u32.wrapping_mul(0x9E37_79B1) >> 26) as usize
    );
    // An empty site's pre half is unmatchable.
    let empty = PackedSetSite::empty();
    assert!(empty.add_shapes.load(Ordering::Relaxed) as u32 >= crate::object::shapes::SHAPE_ID_END);
}

#[test]
fn packed_add_refuse_bits_match_codegen() {
    // perry-codegen ADD_REFUSE_RESERVED / ADD_REFUSE_GC_FLAGS, read as one
    // little-endian u32 at the GcHeader: obj_type | gc_flags << 8 | _reserved << 16.
    // Charter step 3: the numeric proof is a shape kind, not a refused bit.
    let reserved = crate::gc::OBJ_FLAG_STABLE_TOMBSTONES;
    assert_eq!(reserved, 0x0400);
    assert_eq!(crate::gc::GC_FLAG_TENURED, 0x20);
    assert_eq!(std::mem::offset_of!(crate::gc::GcHeader, obj_type), 0);
    assert_eq!(std::mem::offset_of!(crate::gc::GcHeader, gc_flags), 1);
    assert_eq!(std::mem::offset_of!(crate::gc::GcHeader, _reserved), 2);
}

fn fnv1a(bytes: &[u8]) -> u64 {
    bytes.iter().fold(0xcbf2_9ce4_8422_2325u64, |hash, byte| {
        (hash ^ u64::from(*byte)).wrapping_mul(0x0000_0100_0000_01b3)
    })
}

fn interned(name: &[u8]) -> *const crate::StringHeader {
    let key = crate::string::js_string_from_bytes(name.as_ptr(), name.len() as u32);
    crate::string::js_string_intern(key, fnv1a(name))
}

/// A JSON-parsed object: class-less, birth-marked ordinary, and sharing its
/// ShapeId with every other parse of the same key list.
fn parsed(src: &[u8]) -> f64 {
    let text = crate::string::js_string_from_bytes(src.as_ptr(), src.len() as u32);
    let value = unsafe { crate::json::js_json_parse(text) };
    assert!(value.is_pointer(), "JSON.parse must yield an object");
    f64::from_bits(value.bits())
}

fn stamp(value: f64) -> u32 {
    let obj = (value.to_bits() & POINTER_MASK) as *const crate::ObjectHeader;
    unsafe { crate::object::shapes::object_shape_stamp(obj) }
}

/// A site that lives as long as the process, as an emitted one does: the
/// runtime registers primed sites and walks them after every full trace.
fn leaked_site() -> &'static PackedSetSite {
    Box::leak(Box::new(PackedSetSite::empty()))
}

fn miss(
    site: &'static PackedSetSite,
    target: f64,
    key: *const crate::StringHeader,
    value: f64,
) -> f64 {
    let mut cache_slot: super::super::packed_set::PackedSetWaysSlot = std::ptr::null_mut();
    js_put_value_set_packed_miss(target, key, value, 0, &mut cache_slot, &site.set)
}

/// A key-add through the miss entry publishes `pre -> post` at the append
/// slot, and the memo then serves another receiver of `pre` without the
/// full `[[Set]]`, landing it on exactly the successor shape.
#[test]
fn a_key_add_publishes_a_memo_that_serves_the_next_receiver() {
    let key = interned(b"added_key");
    let first = parsed(b"{\"a\":1}");
    let second = parsed(b"{\"a\":2}");
    let pre = stamp(first);
    assert_eq!(pre, stamp(second), "one key list, one ShapeId");
    let site = leaked_site();
    miss(site, first, key, 5.0);
    let post = stamp(first);
    assert_ne!(post, pre);
    let shapes = site.add_shapes.load(Ordering::Relaxed);
    assert_eq!((shapes as u32, (shapes >> 32) as u32), (pre, post));
    assert_eq!(site.add_guard.load(Ordering::Relaxed) & ADD_SLOT_MASK, 1);
    assert_eq!(
        unsafe { js_packed_add_chain_valid(site.add_guard.load(Ordering::Relaxed)) },
        1
    );
    let served = unsafe { packed_add_try(site, second, 6.0) };
    assert_eq!(
        served,
        Some(6.0),
        "the memo must serve a receiver of its pre-shape"
    );
    assert_eq!(
        stamp(second),
        post,
        "the served add lands on the memo's successor"
    );
}

/// A class store records the clear-chain proof before publishing the append.
/// The publication must consume that proof and still serve a fresh receiver.
#[test]
fn a_class_key_add_uses_the_store_sites_clear_chain_verdict() {
    const CID: u32 = 0x0004_2231;
    let proto = crate::object::js_object_alloc(0, 2);
    crate::object::class_prototype_object_root_store(CID, proto);
    let scope = crate::gc::RuntimeHandleScope::new();
    let first = scope.root_raw_mut_ptr(crate::object::js_object_alloc(CID, 4));
    let second = scope.root_raw_mut_ptr(crate::object::js_object_alloc(CID, 4));
    let boxed = |p| crate::value::js_nanbox_pointer(p as i64);
    let key = interned(b"classProofAdd");
    let site = leaked_site();
    let mut slot: super::super::packed_set::PackedSetWaysSlot = std::ptr::null_mut();
    let pre = stamp(boxed(first.get_raw_mut_ptr::<crate::ObjectHeader>()));
    js_put_value_set_packed_miss(
        boxed(first.get_raw_mut_ptr::<crate::ObjectHeader>()),
        key,
        5.0,
        1,
        &mut slot,
        &site.set,
    );
    assert!(
        unsafe {
            crate::object::chain_store::chain_store_proven(
                crate::object::chain_store::ChainSite::Packed(&mut slot),
                first.get_raw_const_ptr::<crate::ObjectHeader>(),
                key,
            )
        },
        "the reused proof must be live"
    );
    assert_eq!(site.add_shapes.load(Ordering::Relaxed) as u32, pre);
    assert_eq!(
        unsafe {
            packed_add_try(
                site,
                boxed(second.get_raw_mut_ptr::<crate::ObjectHeader>()),
                6.0,
            )
        },
        Some(6.0)
    );
}

/// Any move of either verdict word refuses the memo: an inherited setter or
/// non-writable property installed since the prime moves one of them.
#[test]
fn unrelated_generation_keeps_the_memo_but_a_changed_holder_refuses_it() {
    let key = interned(b"added_gen");
    let first = parsed(b"{\"g\":1}");
    let second = parsed(b"{\"g\":2}");
    let site = leaked_site();
    miss(site, first, key, 1.0);
    assert_ne!(site.add_shapes.load(Ordering::Relaxed), PACKED_SET_EMPTY);
    crate::object::proto_validity::bump_proto_validity();
    assert_eq!(unsafe { packed_add_try(site, second, 2.0) }, Some(2.0));
    let third = parsed(b"{\"g\":3}");
    let proto = crate::array::object_prototype_addr_if_resolved() as *mut crate::ObjectHeader;
    assert!(!proto.is_null());
    let changed_key = interned(b"__missread2_chain_mutation");
    unsafe { crate::object::js_object_set_field_by_name(proto, changed_key, 1.0) };
    let pre = stamp(third);
    assert_eq!(unsafe { packed_add_try(site, third, 3.0) }, None);
    assert_eq!(stamp(third), pre, "a refused memo changes nothing");
    crate::object::js_object_delete_field(proto, changed_key);
}

/// The site owns both ShapeIds it may stamp: a full trace's carrier
/// recompute re-notes them, so an intermediate shape no live object carries
/// cannot be retired under the site.
#[test]
fn a_published_memo_owns_both_shapes_across_a_full_trace() {
    let key = interned(b"added_own");
    let first = parsed(b"{\"o\":1}");
    let site = leaked_site();
    let pre = stamp(first);
    miss(site, first, key, 1.0);
    let post = stamp(first);
    let carrier = |id: u32| {
        crate::object::shapes::shape_descriptor_by_id(id)
            .expect("descriptor exists")
            .cache_carrier
    };
    crate::object::shapes::clear_all_cache_carriers();
    assert!(
        !carrier(post),
        "the clear must reset the bit this test asserts"
    );
    note_packed_add_carriers();
    assert!(
        carrier(pre) && carrier(post),
        "a published memo must own its pre- and post-shape"
    );
}

/// A polymorphic site's displaced memos sit at their pre-shape's HOME way —
/// the one way the emitted hit compares — unless that way was already
/// taken, and the runtime serves every memo wherever it sits. Sabotage:
/// placement in arrival order (way 0, 1, ...) -> a home way stays empty
/// while its memo sits elsewhere.
#[test]
fn a_displaced_memo_sits_at_its_home_way() {
    let key = interned(b"added_home");
    let srcs: [&[u8]; 6] = [
        b"{\"h0\":1}",
        b"{\"h1\":1}",
        b"{\"h2\":1}",
        b"{\"h3\":1}",
        b"{\"h4\":1}",
        b"{\"h5\":1}",
    ];
    let site = leaked_site();
    let mut pres = Vec::new();
    for src in srcs {
        let first = parsed(src);
        pres.push(stamp(first));
        miss(site, first, key, 1.0);
    }
    let ways = unsafe { site_ways(site) }.expect("a polymorphic site has ways");
    let primary = site.add_shapes.load(Ordering::Relaxed) as u32;
    assert_eq!(primary, *pres.last().unwrap(), "the newest memo is primary");
    for &pre in &pres[..pres.len() - 1] {
        let at_home = ways[add_way_home(pre)].shapes.load(Ordering::Relaxed);
        assert!(
            at_home as u32 == pre || at_home != PACKED_SET_EMPTY,
            "memo {pre:#x}: its home way {} is empty, so it must sit there",
            add_way_home(pre)
        );
    }
    for (i, src) in srcs.iter().enumerate() {
        let next = parsed(src);
        assert_eq!(stamp(next), pres[i]);
        let served = if pres[i] == primary {
            Some(2.0)
        } else {
            unsafe { packed_add_try(site, next, 2.0) }
        };
        assert_eq!(served, Some(2.0), "memo {i} must be served");
    }
}

fn fresh_ways() -> Box<AddWays> {
    Box::new(std::array::from_fn(|_| AddWay {
        shapes: AtomicU64::new(PACKED_SET_EMPTY),
        guard: AtomicU64::new(0),
    }))
}

/// The first ShapeId at or after `from` whose home is `home`.
fn sid_with_home(home: usize, from: u32) -> u32 {
    (from..).find(|&sid| add_way_home(sid) == home).unwrap()
}

/// A memo the runtime serves from beyond the two ways the emitted hit
/// compares moves into the second of them, trading places with a memo that
/// was not at its own home; a way whose memo IS at its own home is kept.
/// Sabotage: `promote_way` moves nothing -> the hot memo stays out of reach
/// of the emitted hit (tsc: 8 sites, every hit 2-3 ways from home).
#[test]
fn a_far_memo_moves_into_the_inline_ways() {
    let base = crate::object::shapes::SHAPE_ID_BASE;
    let h = 10usize;
    let hot = sid_with_home(h, base);
    let at_home = sid_with_home(h, hot + 1);
    let stray = sid_with_home(40, base);
    let ways = fresh_ways();
    let word = |sid: u32| u64::from(sid) | (u64::from(sid + 1) << 32);
    ways[h].shapes.store(word(at_home), Ordering::Relaxed);
    ways[h].guard.store(1, Ordering::Relaxed);
    ways[h + 1].shapes.store(word(stray), Ordering::Relaxed);
    ways[h + 1].guard.store(2, Ordering::Relaxed);
    ways[h + 3].shapes.store(word(hot), Ordering::Relaxed);
    ways[h + 3].guard.store(3, Ordering::Relaxed);
    promote_way(&ways, h, 3);
    let at = |i: usize| {
        (
            ways[i].shapes.load(Ordering::Relaxed) as u32,
            ways[i].guard.load(Ordering::Relaxed),
        )
    };
    assert_eq!(at(h), (at_home, 1), "a memo at its own home is kept");
    assert_eq!(
        at(h + 1),
        (hot, 3),
        "the served memo moves in with its guard"
    );
    assert_eq!(at(h + 3), (stray, 2), "the displaced memo takes its place");
    // Both inline ways hold memos at their own homes: nothing moves.
    let next_home = sid_with_home(h + 1, base);
    ways[h + 1].shapes.store(word(next_home), Ordering::Relaxed);
    ways[h + 3].shapes.store(word(hot), Ordering::Relaxed);
    promote_way(&ways, h, 3);
    assert_eq!(at(h + 1).0, next_home);
    assert_eq!(at(h + 3).0, hot);
}

/// Charter step 5 (P2c): a memo whose successor has an `F64` lane at the
/// slot carries the flag the emitted hit refuses non-doubles with; a memo
/// learned from a non-Number does not.
#[test]
fn a_number_key_add_memo_carries_the_store_check_flag() {
    let key = interned(b"p2c_added_number");
    let first = parsed(b"{\"p2c_q\":1}");
    let site = leaked_site();
    miss(site, first, key, 5.5);
    assert!(
        !crate::object::field_rep_store::shape_slot_is_any(stamp(first), 1),
        "the successor has an F64 lane"
    );
    let guard = site.add_guard.load(Ordering::Relaxed);
    assert_ne!(guard & ADD_F64_SLOT, 0);
    assert_eq!(guard & ADD_SLOT_MASK, 1);
    assert_eq!(unsafe { js_packed_add_chain_valid(guard) }, 1);

    let other = interned(b"p2c_added_string");
    let second = parsed(b"{\"p2c_q\":1}");
    let text = crate::string::js_string_from_bytes(b"s".as_ptr(), 1);
    let boxed = f64::from_bits(crate::value::js_nanbox_string(text as i64).to_bits());
    let site2 = leaked_site();
    miss(site2, second, other, boxed);
    assert_eq!(site2.add_guard.load(Ordering::Relaxed) & ADD_F64_SLOT, 0);
}

#[test]
fn a_deprecated_successor_refuses_the_emitted_chain_guard() {
    let key = interned(b"add_deprecated_successor");
    let first = parsed(b"{\"representation\":1}");
    let second = parsed(b"{\"representation\":2}");
    let site = leaked_site();
    miss(site, first, key, 5.0);
    let guard = site.add_guard.load(Ordering::Relaxed);
    assert_eq!(unsafe { js_packed_add_chain_valid(guard) }, 1);
    let object = (first.to_bits() & POINTER_MASK) as *mut crate::ObjectHeader;
    crate::object::js_object_set_field_by_name(object, key, f64::from_bits(crate::value::TAG_TRUE));
    assert_eq!(unsafe { js_packed_add_chain_valid(guard) }, 0);
    assert_eq!(unsafe { packed_add_try(site, second, 6.0) }, None);
}

#[test]
fn empty_literal_birth_is_plain_before_its_first_shape() {
    let _lock = crate::gc::global_side_table_test_lock();
    let _no_move = crate::gc::GcSuppressScope::new();
    crate::object::builtin_prototype_value("Object");
    let key = interned(b"literal_birth_added");
    let first = crate::object::js_object_alloc_plain(0);
    let first = crate::value::js_nanbox_pointer(first as i64);
    let pre = stamp(first);
    assert_eq!(
        crate::object::shapes::shape_object_kind_by_id(pre),
        Some(crate::object::shapes::ShapeObjectKind::Ordinary)
    );
    let site = leaked_site();
    miss(site, first, key, 11.0);
    assert_eq!(site.add_shapes.load(Ordering::Relaxed) as u32, pre);
    let second = crate::object::js_object_alloc_plain(0);
    let second = crate::value::js_nanbox_pointer(second as i64);
    assert_eq!(stamp(second), pre);
    assert_eq!(unsafe { packed_add_try(site, second, 22.0) }, Some(22.0));
}

#[test]
fn a_worker_with_seeded_ids_cannot_consume_a_primary_add_proof() {
    let _lock = crate::gc::global_side_table_test_lock();
    let _no_move = crate::gc::GcSuppressScope::new();
    crate::object::builtin_prototype_value("Object");
    let first = parsed(b"{\"worker_source\":1}");
    let pre = stamp(first);
    let key = interned(b"worker_added");
    let site = leaked_site();
    miss(site, first, key, 11.0);
    let post = stamp(first);
    assert_ne!(pre, post);
    unsafe {
        crate::object::shapes::note_external_shape_carrier(
            crate::object::shapes::shape_descriptor_by_id(pre),
        );
        crate::object::shapes::note_external_shape_carrier(
            crate::object::shapes::shape_descriptor_by_id(post),
        );
    }
    let seed = crate::object::shapes::worker_shape_seed();
    let site_addr = site as *const PackedSetSite as usize;
    std::thread::spawn(move || {
        // Exercise runtime ownership independently of the emitted worker
        // gate, without changing that sticky process-wide test input.
        crate::agent::enter_agent_for_test(u64::MAX - 0x12257);
        crate::gc::ensure_gc_initialized();
        crate::object::shapes::install_worker_shape_seed(&seed);
        let shape = crate::object::shapes::shape_descriptor_by_id(pre).unwrap();
        assert!(crate::object::shapes::shape_descriptor_by_id(post).is_some());
        let object = crate::object::alloc_plain::alloc_plain_record_inline_keys_stamped(
            shape.live_inline_slot_count,
            shape.keys as usize as *mut crate::array::ArrayHeader,
            pre,
        );
        let object = crate::value::js_nanbox_pointer(object as i64);
        assert_eq!(stamp(object), pre);
        assert_eq!(
            unsafe { packed_add_try(site_addr as *const PackedSetSite, object, 22.0) },
            None,
            "matching seeded shapes do not authorize a foreign proof"
        );
        assert_eq!(stamp(object), pre);
    })
    .join()
    .unwrap();
}

#[test]
fn an_any_append_still_guards_a_typed_prefix() {
    let _lock = crate::gc::global_side_table_test_lock();
    let _no_move = crate::gc::GcSuppressScope::new();
    crate::object::builtin_prototype_value("Object");
    let prefix = interned(b"mixed_prefix_number");
    let key = interned(b"mixed_any_append");
    let first = crate::object::js_object_alloc_plain(0);
    crate::object::js_object_set_field_by_name(first, prefix, 1.5);
    let value = crate::value::js_nanbox_pointer(first as i64);
    let pre = stamp(value);
    assert!(crate::object::field_rep_store::shape_slot_is_f64(pre, 0));
    let site = leaked_site();
    miss(site, value, key, f64::from_bits(crate::value::TAG_NULL));
    let guard = site.add_guard.load(Ordering::Relaxed);
    assert_eq!(unsafe { js_packed_add_chain_valid(guard) }, 1);
    crate::object::js_object_set_field_by_name(
        first,
        prefix,
        f64::from_bits(crate::value::TAG_TRUE),
    );
    assert_eq!(
        unsafe { js_packed_add_chain_valid(guard) },
        0,
        "an Any append cannot stamp a successor whose typed prefix was deprecated"
    );
}
