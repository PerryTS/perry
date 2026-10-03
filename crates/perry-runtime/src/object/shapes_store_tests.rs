use super::*;

/// Every kind survives the record field, and the two store-fact kinds
/// (charter step 3) occupy codes 5 and 6 — distinct values, so distinct
/// ShapeIds for otherwise identical facts.
#[test]
fn kind_codes_round_trip() {
    for kind in [
        ShapeObjectKind::Ordinary,
        ShapeObjectKind::Class,
        ShapeObjectKind::Dictionary,
        ShapeObjectKind::Function,
        ShapeObjectKind::FunctionDictionary,
        ShapeObjectKind::OrdinaryUnmarked,
        ShapeObjectKind::OrdinaryNumericProof,
    ] {
        assert!(kind.code() as u32 <= RECORD_KIND_MAX_CODE);
        let r = ShapeRecord::new(0x1000, 1, 1, 0, kind, 0);
        assert_eq!(r.object_kind(), kind);
        for other in [ShapeObjectKind::Ordinary, ShapeObjectKind::OrdinaryUnmarked] {
            if other != kind {
                assert!(!r.facts_match(0x1000, 1, 1, 0, other, 0));
                assert_ne!(
                    facts_key(0x1000, 1, 1, 0, kind, 0),
                    facts_key(0x1000, 1, 1, 0, other, 0)
                );
            }
        }
    }
}

/// `locate` agrees with the band constants at every boundary, and an id
/// outside the ShapeId range lands in the always-empty band 3.
#[test]
fn locate_names_the_band_and_index_at_every_boundary() {
    use super::super::SHAPE_ID_END;
    let cases = [
        (SHAPE_ID_BASE, 0, 0),
        (DICTIONARY_SHAPE_ID_BASE - 1, 0, (DICT_REL - 1) as usize),
        (DICTIONARY_SHAPE_ID_BASE, 1, 0),
        (
            EXOTIC_SHAPE_ID_BASE - 1,
            1,
            (EXOTIC_REL - DICT_REL - 1) as usize,
        ),
        (EXOTIC_SHAPE_ID_BASE, 2, 0),
        (SHAPE_ID_END - 1, 2, (END_REL - EXOTIC_REL - 1) as usize),
    ];
    for (id, band, index) in cases {
        assert_eq!(locate(id), (band, index), "{id:#x}");
    }
    for id in [0, 1, SHAPE_ID_BASE - 1, SHAPE_ID_END, 0xF000_0000, u32::MAX] {
        assert_eq!(locate(id).0, 3, "{id:#x}");
    }
}

/// The agent directory ([`AGENT_SHAPE_DIR`]) is the agent slab's, entry
/// for entry, in every band, and per agent; an id that names no record —
/// never minted, retired, past every page, in no band, or read before the
/// agent has any runtime state — reads the shared empty record: not
/// present, every lane `Any`, position bound 0. Runs on threads of its
/// own, each its own agent, so the records it installs are seen by
/// nothing else.
#[test]
fn the_agent_directory_is_the_agent_slab_and_an_absent_id_reads_empty() {
    use super::super::{
        alloc_dictionary_shape_id, alloc_exotic_shape_id, alloc_shape_id, shape_record_by_id,
        shape_rep_by_id, SHAPE_ID_END,
    };
    fn absent(id: u32) {
        // SAFETY: `agent_record` never returns null.
        let r = unsafe { &*ShapeSlab::agent_record(id) };
        assert!(!r.present(), "{id:#x} reads a present record");
        assert_eq!(r.rep, 0, "{id:#x}: absent rep");
        assert_eq!(r.position_bound(), 0, "{id:#x}: absent position bound");
        assert!(shape_record_by_id(id).is_none(), "{id:#x}");
        assert_eq!(shape_rep_by_id(id), 0, "{id:#x}");
    }
    // Ids no counter reaches in a test process, in and around every band.
    const NEVER: [u32; 10] = [
        0,
        1,
        0x7FFF_FF00,
        SHAPE_ID_BASE - 1,
        DICTIONARY_SHAPE_ID_BASE - 1,
        EXOTIC_SHAPE_ID_BASE - 1,
        SHAPE_ID_END - 1,
        SHAPE_ID_END,
        0xF000_0000,
        u32::MAX,
    ];
    std::thread::spawn(|| {
        // No runtime state yet: the const directory answers.
        for id in NEVER {
            absent(id);
        }
        let table = &crate::state::state().shapes;
        let ids = [
            alloc_shape_id(super::super::PROTO_ID_DEFAULT).unwrap(),
            alloc_dictionary_shape_id(super::super::PROTO_ID_DEFAULT).unwrap(),
            alloc_exotic_shape_id(super::super::PROTO_ID_DEFAULT).unwrap(),
        ];
        let reps = [0b01u64, 0b01 << 2, 0b10 << 4];
        for (&id, &rep) in ids.iter().zip(&reps) {
            // Keyless: nothing here ever dereferences a keys word.
            let record = ShapeRecord::new(0, 0, 3, 0, ShapeObjectKind::Ordinary, 0).with_rep(rep);
            // SAFETY: no slab reference is held.
            unsafe { table.slab_mut().insert(id, record) };
        }
        let check = |ids: &[u32], reps: &[u64]| {
            for (&id, &rep) in ids.iter().zip(reps) {
                assert_eq!(
                    table.slab().record_ptr(id),
                    Some(ShapeSlab::agent_record(id))
                );
                assert_eq!(shape_rep_by_id(id), rep, "{id:#x}");
                assert_eq!(
                    shape_record_by_id(id).map(|r| r.live_inline_slot_count()),
                    Some(3)
                );
                // The id's chunk neighbour is allocated and absent.
                absent(id ^ 1);
            }
        };
        check(&ids, &reps);
        for id in NEVER {
            absent(id);
        }
        // Growing a band's page vector moves its buffer: the directory
        // follows, and the records already there are still found.
        let far = DICTIONARY_SHAPE_ID_BASE - 2;
        let record = ShapeRecord::new(0, 0, 3, 0, ShapeObjectKind::Ordinary, 0).with_rep(0b01);
        // SAFETY: no slab reference is held.
        unsafe { table.slab_mut().insert(far, record) };
        check(&ids, &reps);
        check(&[far], &[0b01]);
        // Another agent sees none of this agent's records, before and
        // after it builds its own state.
        std::thread::spawn(move || {
            for id in ids {
                absent(id);
            }
            let _ = crate::state::state();
            for id in ids.into_iter().chain([far]) {
                absent(id);
            }
        })
        .join()
        .unwrap();
        // Retired, and its chunk and page released: absent again, and the
        // directory shrinks with the slab.
        for id in ids.into_iter().chain([far]) {
            // SAFETY: no slab reference is held.
            assert!(unsafe { table.slab_mut().remove(id) }.is_some());
            absent(id);
        }
        // SAFETY: no slab reference is held.
        unsafe { table.slab_mut().release_empty_chunks() };
        for id in ids.into_iter().chain([far]).chain(NEVER) {
            absent(id);
        }
    })
    .join()
    .unwrap();
}

#[test]
fn slab_records_are_addressed_by_id_and_keep_their_address() {
    let mut slab = ShapeSlab::new();
    let id_a = SHAPE_ID_BASE + 5;
    let id_b = SHAPE_ID_BASE + 5 + (CHUNK_LEN * PAGE_LEN) as u32 * 3;
    assert_eq!(slab.get(id_a), None);
    assert_eq!(
        slab.insert(
            id_a,
            ShapeRecord::new(0x1000, 1, 1, 0, ShapeObjectKind::Ordinary, 0)
        )
        .map(|r| r.keys),
        None
    );
    let a_ptr = slab.record_ptr(id_a).expect("present");
    // A later insert into another chunk must not move the first record.
    slab.insert(
        id_b,
        ShapeRecord::new(0x2000, 2, 2, 7, ShapeObjectKind::Class, 1),
    );
    assert_eq!(slab.record_ptr(id_a), Some(a_ptr));
    assert_eq!(slab.len(), 2);
    assert_eq!(slab.chunk_count(), 2);
    let b = slab.get(id_b).unwrap();
    assert_eq!(b.object_kind(), ShapeObjectKind::Class);
    assert_eq!(b.semantic_generation, 7);
    assert_eq!(b.hole_count, 1);
    assert!(b.facts_match(0x2000, 2, 2, 7, ShapeObjectKind::Class, 1));
    assert!(!b.facts_match(0x2000, 2, 2, 7, ShapeObjectKind::Ordinary, 1));
    // Ids outside the range and never-minted ids resolve to nothing.
    assert_eq!(slab.get(0), None);
    assert_eq!(slab.get(SHAPE_ID_BASE + 6), None);
    assert_eq!(slab.get(super::super::SHAPE_ID_END - 1), None);
    assert_eq!(slab.ids(), vec![id_a, id_b]);
    // Removal clears the record and, once a chunk is empty, the chunk.
    assert_eq!(slab.remove(id_a).map(|r| r.keys), Some(0x1000));
    assert_eq!(slab.remove(id_a), None);
    assert_eq!(slab.len(), 1);
    slab.release_empty_chunks();
    assert_eq!(slab.chunk_count(), 1);
    assert_eq!(slab.get(id_b).map(|r| r.keys), Some(0x2000));
    assert_eq!(slab.remove(id_b).map(|r| r.keys), Some(0x2000));
    slab.release_empty_chunks();
    assert_eq!(slab.chunk_count(), 0);
    assert_eq!(slab.estimated_bytes(), 0);
}

#[test]
fn lifted_descriptor_mirrors_the_record_and_names_its_address() {
    let mut slab = ShapeSlab::new();
    let id = SHAPE_ID_BASE + 42;
    let mut record = ShapeRecord::new(0x3000, 4, 6, 9, ShapeObjectKind::Ordinary, 2);
    record.set(RECORD_FLAG_OLD_CARRIER, true);
    record.set(RECORD_FLAG_CACHE_CARRIER, true);
    record.set(RECORD_FLAG_FACTS_INDEXED, false);
    slab.insert(id, record);
    let ptr = slab.record_ptr(id).unwrap();
    let lifted = slab.lift(id).unwrap();
    assert_eq!(lifted.record, ptr as usize);
    assert_eq!(lifted.keys, 0x3000);
    assert_eq!(lifted.logical_key_count, 4);
    assert_eq!(lifted.live_inline_slot_count, 6);
    assert_eq!(lifted.semantic_generation, 9);
    assert_eq!(lifted.hole_count, 2);
    assert!(lifted.old_carrier);
    assert!(lifted.cache_carrier);
    assert!(!slab.get(id).unwrap().has(RECORD_FLAG_FACTS_INDEXED));
    assert_eq!(lifted.keys_slot(), Some(ptr as *mut u64));
    // Writing through the slot is what an evacuating visitor does.
    unsafe { *lifted.keys_slot().unwrap() = 0x4000 };
    assert_eq!(slab.get(id).unwrap().keys, 0x4000);
}

/// The geometry that makes the object kind FREE, and the O(1) property of
/// the probe path, asserted together on purpose: the kind fits only
/// because it lives in bytes that were already padding, so a future field
/// that grows the record silently takes that away. Fail here rather than
/// discovering it as RSS. ConstFn's owned extension is the first explicit
/// growth since these earlier packed facts.
///
/// 32 -> 40 bytes is deliberate: [[Prototype]] is a shape fact
/// (`proto_id`), and a 64-bit prototype identity does not fit the padding.
/// 40 -> 48 is deliberate too: the per-slot field representation (charter
/// step 5, `rep`) is a shape fact with no free bits left to live in.
/// 48 -> 56 is deliberate: POSBOUND (`position_bound`, offset 40) is the
/// one-compare position fact the megamorphic read confirm needs; `rep`
/// moves to offset 48 behind it.
/// 56 -> 64 is the record-owned ConstFn extension pointer; no side table
/// or pointer to a heap closure participates in shape identity.
/// The [[Prototype]] itself is NOT in the record: it is one word per prototype
/// identity (`shapes_prototype`), so the record stays one cache line.
#[test]
fn the_record_geometry_is_free_and_facts_key_is_o1() {
    assert_eq!(std::mem::size_of::<ShapeRecord>(), 64, "record grew");
    assert_eq!(std::mem::align_of::<ShapeRecord>(), 8, "record realigned");

    // `facts_key` folds the keys ADDRESS; it must never dereference it.
    // A wild, unmapped address must be folded, not read. If the probe
    // path is ever changed to walk key strings (an O(N) content fold),
    // this reads garbage and the test dies -- which is the assertion.
    let wild: u64 = 0xDEAD_BEEF_DEAD_BEEF;
    let a = facts_key(wild, 3, 4, 7, ShapeObjectKind::Ordinary, 0);
    let b = facts_key(wild, 3, 4, 7, ShapeObjectKind::Ordinary, 0);
    assert_eq!(a, b, "facts_key must be a pure fold of its arguments");
}

/// Every kind must reach the fold distinctly. The predecessor of this
/// test folded `object_kind == Class` as a BOOL, which gave Ordinary and
/// Dictionary the same contribution; `facts_match` re-checked the full
/// enum so it was never a wrong answer, but the two kinds differ in every
/// consumer and must not share a hash slot by construction.
#[test]
fn every_object_kind_reaches_the_facts_fold() {
    let w: u64 = 0x1234_5678;
    let o = facts_key(w, 3, 4, 7, ShapeObjectKind::Ordinary, 0);
    let c = facts_key(w, 3, 4, 7, ShapeObjectKind::Class, 0);
    let d = facts_key(w, 3, 4, 7, ShapeObjectKind::Dictionary, 0);
    assert_ne!(o, c, "Ordinary and Class collide");
    assert_ne!(
        o, d,
        "Ordinary and Dictionary collide -- the bool fold is back"
    );
    assert_ne!(c, d, "Class and Dictionary collide");
}

/// A record must report the kind it was built with. Storing the kind in a
/// single flag bit could represent only two, so a Dictionary record read
/// back as Ordinary -- and because `facts_match` compares the full enum,
/// that is a WRONG IDENTITY MATCH, not a hash collision.
#[test]
fn a_record_round_trips_all_three_kinds_beside_its_flags() {
    for kind in [
        ShapeObjectKind::Ordinary,
        ShapeObjectKind::Class,
        ShapeObjectKind::Dictionary,
    ] {
        let mut r = ShapeRecord::new(0x4000, 2, 2, 11, kind, 1);
        assert_eq!(r.object_kind(), kind, "kind did not round-trip");
        assert!(r.has(RECORD_FLAG_PRESENT));
        assert!(r.has(RECORD_FLAG_FACTS_INDEXED));
        // flags and kind share one word: moving a flag must not move the
        // kind, and vice versa.
        r.set(RECORD_FLAG_OLD_CARRIER, true);
        assert_eq!(r.object_kind(), kind, "setting a flag moved the kind");
        r.set(RECORD_FLAG_OLD_CARRIER, false);
        r.set(RECORD_FLAG_CACHE_CARRIER, true);
        assert_eq!(r.object_kind(), kind, "clearing a flag moved the kind");
        assert!(
            !r.has(RECORD_FLAG_OLD_CARRIER),
            "clear leaked into another flag"
        );
    }
}

#[test]
fn special_body_identity_uses_record_owned_extension() {
    let old = ShapeRecord::new(0x40, 1, 1, 0, ShapeObjectKind::Ordinary, 0);
    assert_eq!(old.special_constfn_mask(), 0);
    let a = [ConstFnSlotInfo {
        slot: 0,
        info: 0x1000,
    }];
    let b = [ConstFnSlotInfo {
        slot: 0,
        info: 0x2000,
    }];
    let rep = crate::object::field_rep::with_slot_rep(0, 0, crate::object::field_rep::REP_SPECIAL);
    let special = old.with_special_facts(rep, &a);
    assert_eq!(special.special_constfn_mask(), 1);
    assert_eq!(special.constfn_infos(), &a);
    assert_eq!(special.position_bound_raw(), old.position_bound_raw());
    assert_eq!(std::mem::size_of::<ShapeRecord>(), 64);
    assert!(special.facts_match_proto_with_special(
        0x40,
        1,
        1,
        0,
        ShapeObjectKind::Ordinary,
        0,
        0,
        0,
        rep,
        &a
    ));
    assert!(!special.facts_match_proto_with_special(
        0x40,
        1,
        1,
        0,
        ShapeObjectKind::Ordinary,
        0,
        0,
        0,
        rep,
        &b
    ));
    let hash_a =
        facts_key_proto_with_special(0x40, 1, 1, 0, ShapeObjectKind::Ordinary, 0, 0, 0, rep, &a);
    let hash_b =
        facts_key_proto_with_special(0x40, 1, 1, 0, ShapeObjectKind::Ordinary, 0, 0, 0, rep, &b);
    assert_ne!(hash_a, hash_b);
    // SAFETY: this synthetic record is the unique owner of its box.
    unsafe { special.release_extras() };
}

/// Charter step 5, P1 is inert: an all-`Any` record's facts key is
/// EXACTLY the fold it had before the `rep` word existed (recomputed here
/// without it), so no existing shape changes bucket.
#[test]
fn an_all_any_rep_keeps_the_pre_rep_facts_key() {
    const FNV_OFFSET_BASIS: u64 = 0xcbf2_9ce4_8422_2325;
    const FNV_PRIME: u64 = 0x0000_0100_0000_01b3;
    let fold = |acc: u64, word: u64| (acc ^ word).wrapping_mul(FNV_PRIME);
    for kind in [ShapeObjectKind::Ordinary, ShapeObjectKind::Class] {
        let mut h = fold(FNV_OFFSET_BASIS, 0x1111_2222_3333_4444);
        for word in [7, 3, 9, 2, kind.code(), 0x77] {
            h = fold(h, word);
        }
        let pre_rep = h ^ (h >> 32);
        let key = facts_key_proto(0x1111_2222_3333_4444, 7, 3, 9, kind, 2, 0x77, 0, 0);
        assert_eq!(key, pre_rep, "an all-Any shape must keep its key");
    }
}

/// `rep` is compared on every bucket hit, not only hashed: a 64-bit fold
/// collision must never hand an F64 shape to an all-`Any` request.
#[test]
fn facts_match_compares_the_rep_identity() {
    use crate::object::field_rep::{with_slot_rep, REP_F64, REP_F64_DEPRECATED};
    let f64_at_0 = with_slot_rep(0, 0, REP_F64);
    let record = ShapeRecord::new(0x40, 1, 1, 0, ShapeObjectKind::Ordinary, 0).with_rep(f64_at_0);
    let facts =
        |rep| record.facts_match_proto(0x40, 1, 1, 0, ShapeObjectKind::Ordinary, 0, 0, 0, rep);
    assert!(facts(f64_at_0));
    assert!(!facts(0), "same facts, all-Any rep: not this shape");
    assert!(
        facts(with_slot_rep(0, 0, REP_F64_DEPRECATED)),
        "deprecated is not identity"
    );
}

/// Varying any ONE fact must change the key: a fold that dropped a field
/// would send two different shapes to one bucket for every value of it.
#[test]
fn facts_key_folds_every_field() {
    let base = facts_key(0x1111_2222_3333_4444, 7, 3, 9, ShapeObjectKind::Ordinary, 0);
    let variants = [
        (
            "keys",
            facts_key(0x5555_6666_7777_8888, 7, 3, 9, ShapeObjectKind::Ordinary, 0),
        ),
        (
            "logical",
            facts_key(0x1111_2222_3333_4444, 8, 3, 9, ShapeObjectKind::Ordinary, 0),
        ),
        (
            "live",
            facts_key(0x1111_2222_3333_4444, 7, 4, 9, ShapeObjectKind::Ordinary, 0),
        ),
        (
            "generation",
            facts_key(
                0x1111_2222_3333_4444,
                7,
                3,
                10,
                ShapeObjectKind::Ordinary,
                0,
            ),
        ),
        (
            "kind",
            facts_key(0x1111_2222_3333_4444, 7, 3, 9, ShapeObjectKind::Class, 0),
        ),
        (
            "holes",
            facts_key(0x1111_2222_3333_4444, 7, 3, 9, ShapeObjectKind::Ordinary, 1),
        ),
    ];
    for (field, key) in variants {
        assert_ne!(
            key, base,
            "changing `{field}` alone must change the facts key"
        );
    }
    let rep = facts_key_proto(
        0x1111_2222_3333_4444,
        7,
        3,
        9,
        ShapeObjectKind::Ordinary,
        0,
        0,
        0,
        crate::object::field_rep::REP_F64,
    );
    assert_ne!(rep, base, "changing `rep` alone must change the facts key");
    let record = ShapeRecord::new(0x1111_2222_3333_4444, 7, 3, 9, ShapeObjectKind::Ordinary, 0);
    assert_eq!(record.facts_key_with_keys(0x1111_2222_3333_4444), base);
    assert_eq!(
        record.facts_key_with_keys(0x5555_6666_7777_8888),
        variants[0].1
    );
}

#[test]
fn id_list_keeps_order_across_the_inline_to_spill_boundary() {
    let mut list = IdList::default();
    assert!(list.is_empty());
    list.push_back(2);
    list.push_back(3);
    list.push_front(1);
    list.push_back(2); // duplicate ignored
    assert_eq!(list.as_slice(), &[1, 2, 3]);
    assert!(matches!(list, IdList::Inline { .. }));
    list.push_back(4);
    assert!(matches!(list, IdList::Spill(_)));
    assert_eq!(list.as_slice(), &[1, 2, 3, 4]);
    list.push_front(0);
    assert_eq!(list.as_slice(), &[0, 1, 2, 3, 4]);
    // The ORDERED removal keeps this list's order, which is what
    // `by_facts` depends on.
    assert!(list.remove_ordered(2));
    assert!(!list.remove_ordered(2));
    assert_eq!(list.as_slice(), &[0, 1, 3, 4]);
    assert!(list.replace(3, 30));
    assert!(!list.replace(3, 300));
    assert_eq!(list.as_slice(), &[0, 1, 30, 4]);
    assert!(list.heap_bytes() >= 4 * 4);

    let mut inline = IdList::default();
    inline.push_back(7);
    inline.push_back(8);
    inline.push_back(9);
    assert!(inline.remove_ordered(8));
    assert_eq!(inline.as_slice(), &[7, 9]);
    assert!(inline.replace(9, 10));
    assert_eq!(inline.as_slice(), &[7, 10]);
    assert!(inline.remove_ordered(7));
    assert!(inline.remove_ordered(10));
    assert!(inline.is_empty());
    assert_eq!(inline.heap_bytes(), 0);
}

/// THE GUARD for this change, and it is an asymmetric one: the unordered
/// removal must move O(1) elements per call, and the ordered one is
/// allowed to move O(n) because that is what preserving the order costs.
///
/// Front removal is the measured shape of the defect — removals sit at
/// position ~0.31 of a list up to 514,030 long — so the test removes from
/// the front, which is the worst case for `Vec::remove` and the best case
/// for nothing.
///
/// **Sabotage: point `remove_unordered` at `remove_ordered`.** The bound
/// below is `4 * N`; the O(n) path moves `N * (N - 1) / 2` = 1,999,000
/// elements for N = 2,000, i.e. 250x the bound, and this fails. A bound
/// expressed as a MULTIPLE of N rather than an absolute is what makes the
/// assertion about the complexity class instead of about one N.
#[test]
fn unordered_removal_moves_o1_elements_and_scans_o1_entries() {
    const N: u32 = 2_000;

    let baseline = ID_LIST_OP_STATS.with(std::cell::Cell::get);
    let mut list = IdList::default();
    for id in 1..=N {
        // The interning sites' entry point: no membership probe.
        list.append_unchecked(id);
    }
    assert_eq!(list.len(), N as usize);
    assert!(matches!(list, IdList::Spill(_)));

    // Remove every id from the FRONT of the list, in insertion order.
    for id in 1..=N {
        assert!(list.remove_unordered(id), "id {id} was not present");
    }
    assert!(list.is_empty());

    let after = ID_LIST_OP_STATS.with(std::cell::Cell::get);
    let moved = after.elems_moved - baseline.elems_moved;
    let scanned = after.positions_scanned - baseline.positions_scanned;
    let removals = after.removals - baseline.removals;
    assert_eq!(removals, u64::from(N));

    // O(1) per removal, with room for the swap itself.
    assert!(
        moved <= 4 * u64::from(N),
        "unordered removal moved {moved} elements for {N} removals — that \
         is the O(n) tail shift this structure exists to remove \
         (the ordered path would move {})",
        u64::from(N) * (u64::from(N) - 1) / 2
    );
    // The index answers `position`, so no linear scan may be charged for
    // a list this long. Sabotage: raise SPILL_INDEX_MIN above N and this
    // fails with ~N*N/2 scanned entries.
    assert!(
        scanned <= 4 * u64::from(N),
        "unordered removal scanned {scanned} entries for {N} removals — \
         the spill index is not answering `position`"
    );
}

/// The index must agree with the vector after every operation, including
/// the swap that moves a third element nobody named. Checked exhaustively
/// against a plain `Vec` oracle, because an index that drifts is a wrong
/// ANSWER (a descriptor that cannot be found, or one found under the wrong
/// id), not a slow one.
///
/// Sabotage: drop the `self.pos.insert(self.ids[pos], pos as u32)` fixup
/// in `remove_unordered` — the element the swap relocated keeps a stale
/// index and the `contains` check below fails.
#[test]
fn the_spill_index_agrees_with_the_vector_after_every_operation() {
    let mut list = IdList::default();
    let mut oracle: Vec<u32> = Vec::new();
    for id in 1..=200u32 {
        list.append_unchecked(id);
        oracle.push(id);
    }
    // Remove a scattered third of them, front, middle and back.
    for &id in &[1u32, 2, 3, 100, 101, 199, 200, 50, 150, 7] {
        assert!(list.remove_unordered(id));
        oracle.retain(|&x| x != id);
    }
    // Same SET, whatever the order.
    let mut got = list.as_slice().to_vec();
    got.sort_unstable();
    let mut want = oracle.clone();
    want.sort_unstable();
    assert_eq!(got, want);
    // And every survivor is still findable through the index.
    for &id in &want {
        assert!(list.contains(id), "id {id} lost its index entry");
    }
    for &id in &[1u32, 2, 3, 100, 101, 199, 200, 50, 150, 7] {
        assert!(!list.contains(id), "removed id {id} is still findable");
    }
    // `replace` must keep the index coherent too.
    let survivor = want[0];
    assert!(list.replace(survivor, 9_999));
    assert!(!list.contains(survivor));
    assert!(list.contains(9_999));
}

/// A list that never reaches `SPILL_INDEX_MIN` must not allocate an index
/// — the map is the structure's memory cost and it is only worth paying
/// where the scan hurts. `by_facts` lists, measured at length 1 on cc,
/// live entirely in this regime.
#[test]
fn a_short_spilled_list_builds_no_index() {
    let mut list = IdList::default();
    for id in 1..=8u32 {
        list.append_unchecked(id);
    }
    assert!(matches!(list, IdList::Spill(_)));
    match &list {
        IdList::Spill(v) => assert!(
            !v.indexed(),
            "a list of 8 built an index; SPILL_INDEX_MIN is {SPILL_INDEX_MIN}"
        ),
        IdList::Inline { .. } => unreachable!(),
    }
    // Still correct without one.
    assert!(list.remove_unordered(4));
    assert!(!list.contains(4));
    assert!(list.contains(8));
}

/// A dictionary-band id lives in its own directory: inserting one must not
/// grow the ordinary directory to the band's offset (~24,577 page slots),
/// which moved the GC arena's pages and cost tsc +2.3% instructions.
#[test]
fn a_dictionary_band_id_does_not_grow_the_ordinary_directory() {
    let mut slab = ShapeSlab::new();
    let ordinary = super::super::SHAPE_ID_BASE + 3;
    let dict = super::super::DICTIONARY_SHAPE_ID_BASE + 5;
    slab.insert(ordinary, ShapeRecord::EMPTY);
    slab.insert(dict, ShapeRecord::EMPTY);
    assert_eq!(
        slab.pages.len(),
        1,
        "one ordinary page slot, not the band offset"
    );
    assert_eq!(slab.dict_pages.len(), 1);
    assert!(slab.record_ptr(ordinary).is_some() && slab.record_ptr(dict).is_some());
    assert_eq!(slab.ids(), vec![ordinary, dict]);
    assert!(slab.remove(dict).is_some() && slab.record_ptr(dict).is_none());
}
