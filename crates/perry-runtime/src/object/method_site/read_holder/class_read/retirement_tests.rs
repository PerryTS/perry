//! Miss-path retirement rules, independently sabotaged by the lane checks.
use super::tests::{pinned_shape, receiver_shape, shaped};
use super::*;

fn owned_receiver_shape(class_id: u32, generation: u64) -> (u32, usize) {
    let keys = crate::array::js_array_alloc(0);
    let shape = crate::object::shapes::shape_descriptor_ensure_with_generation(
        keys,
        0,
        0,
        generation,
        crate::object::shapes::ShapeObjectKind::Ordinary,
        PROTO_ID_CLASS | u64::from(class_id),
        crate::object::shapes::ReceiverFacts::NONE,
    )
    .unwrap();
    (shape, keys as usize)
}

fn retire(shape: u32, keys: usize) {
    assert!(crate::object::shapes::shape_record_by_id(shape).is_some());
    crate::object::shapes::test_drop_shape_descriptors(keys);
    assert!(crate::object::shapes::shape_record_by_id(shape).is_none());
}

#[test]
fn retired_way_precedes_cursor_without_churn() {
    let _lock = crate::gc::global_side_table_test_lock();
    let _no_move = crate::gc::GcSuppressScope::new();
    const CID: u32 = 0x0C3C_9010;
    let live = receiver_shape(CID, 90_001);
    let (expired, keys) = owned_receiver_shape(CID, 90_002);
    let mut entries = [Entry {
        token: (PIC_ID_TOKEN_BIT | u64::from(live)) as i64,
        class_id: CID,
        ..EMPTY
    }; WAYS];
    entries[7].token = (PIC_ID_TOKEN_BIT | u64::from(expired)) as i64;
    retire(expired, keys);
    let mut site = Site {
        primary_class: entries[0],
        entries: entries[1..].to_vec(),
        next: 3,
        holders: Vec::new(),
        holder_next: 0,
        accessor_hops: [(0, 0); HOLDER_MAX_DEPTH - 1],
        accessor_depth: 0,
    };
    let fresh = (PIC_ID_TOKEN_BIT | u64::from(receiver_shape(CID, 90_003))) as i64;
    assert_eq!(unsafe { publication_way(&mut site, fresh, CID) }, 7);
    assert_eq!(site.next, 3, "expiry does not advance or arm the cursor");
    // A matching live way still wins over an expired one.
    assert_eq!(
        unsafe { publication_way(&mut site, entries[0].token, CID) },
        0
    );
    site.entries[6] = entries[0];
    site.entries[10] = EMPTY;
    assert_eq!(unsafe { publication_way(&mut site, fresh, CID) }, 11);
    site.entries[10] = entries[0];
    assert_eq!(unsafe { publication_way(&mut site, fresh, CID) }, 3);
    assert_eq!(site.next, 4);
}

#[test]
fn retired_absent_replacement_does_not_arm_sharing() {
    if !crate::object::method_site::run_with_fresh_worker_gate(
        "retired_absent_replacement_does_not_arm_sharing",
    ) {
        return;
    }
    let _lock = crate::gc::global_side_table_test_lock();
    let _no_move = crate::gc::GcSuppressScope::new();
    const CID: u32 = 0x0C3C_9011;
    let holder = shaped(pinned_shape(91_000));
    let w = Walk {
        holder: &*holder as *const ObjectHeader as usize,
        holder_shape: holder.parent_class_id,
        slot: None,
        hops: NO_HOPS,
        depth: 1,
        getter: 0,
    };
    let mut cache = [0i64; crate::object::PIC_CACHE_WORDS];
    cache[HOLDER_STATE] = STATE_REGISTERED;
    let shapes: Vec<_> = (0..WAYS + 2)
        .map(|i| owned_receiver_shape(CID, 91_001 + i as u64))
        .collect();
    for &(shape, _) in &shapes[..WAYS] {
        let mut r = shaped(shape);
        r.class_id = CID;
        unsafe { publish(&mut cache, &*r, &w) };
    }
    retire(shapes[0].0, shapes[0].1);
    let mut r = shaped(shapes[WAYS].0);
    r.class_id = CID;
    unsafe { publish(&mut cache, &*r, &w) };
    let record = (cache[SITE_WORD] as u64 & crate::value::POINTER_MASK) as usize as *mut Site;
    let s = unsafe { &mut *record };
    assert_eq!(s.next & ABSENT_CHURN, 0, "expiry is not live-shape churn");
    assert!(s.class_entries().all(|e| !e.multi_absent()));
    // Genuine replacement of a live absent way still arms sharing.
    r.parent_class_id = shapes[WAYS + 1].0;
    unsafe { publish(&mut cache, &*r, &w) };
    assert_ne!(s.next & ABSENT_CHURN, 0);
    // Expiry must preserve the existing evidence of live churn.
    retire(shapes[1].0, shapes[1].1);
    let data = Walk { slot: Some(0), ..w };
    r.parent_class_id = owned_receiver_shape(CID, 92_000).0;
    unsafe { publish(&mut cache, &*r, &data) };
    assert_ne!(s.next & ABSENT_CHURN, 0);
    for e in s.class_entries() {
        unsafe { e.drop_block() };
    }
    unsafe { drop(Box::from_raw(record)) };
}

#[test]
fn full_shared_set_rehashes_retired_receivers_before_refusal() {
    let _lock = crate::gc::global_side_table_test_lock();
    let _no_move = crate::gc::GcSuppressScope::new();
    // Force one long collision chain, including wraparound. Removing a
    // bucket without rehashing would hide the remaining receivers.
    let top = crate::object::shapes::test_unused_external_shape_id();
    let shapes: Vec<_> = (0..=ABSENT_RECEIVERS)
        .map(|i| {
            let keys = crate::array::js_array_alloc(0);
            let id = top - i * ABSENT_BUCKETS as u32;
            assert!(crate::object::shapes::test_install_external_shape_id(
                id, keys, 0, 0
            ));
            (id, keys as usize)
        })
        .collect();
    let mut entry = Entry {
        token: (PIC_ID_TOKEN_BIT | u64::from(shapes[0].0)) as i64,
        depth: 1,
        absent: true,
        ..EMPTY
    };
    for &(id, _) in &shapes[..ABSENT_RECEIVERS as usize] {
        assert!(unsafe { entry.add_receiver(id) });
    }
    assert_eq!(entry.slot & !MULTI_ABSENT, ABSENT_RECEIVERS);
    assert!(
        !unsafe { entry.add_receiver(shapes[ABSENT_RECEIVERS as usize].0) },
        "a full live set still refuses"
    );
    for &(id, keys) in shapes[..ABSENT_RECEIVERS as usize].iter().step_by(2) {
        retire(id, keys);
    }
    let extra = shapes[ABSENT_RECEIVERS as usize].0;
    assert!(
        unsafe { entry.add_receiver(extra) },
        "expired ids cannot exhaust the bound"
    );
    assert_eq!(entry.slot & !MULTI_ABSENT, ABSENT_RECEIVERS / 2 + 1);
    for (i, &(id, _)) in shapes[..ABSENT_RECEIVERS as usize].iter().enumerate() {
        assert_eq!(
            unsafe { *entry.receiver_bucket(id) != 0 },
            i % 2 != 0,
            "rehashing must preserve membership across collisions"
        );
    }
    assert!(unsafe { entry.add_receiver(extra) });
    assert_eq!(entry.slot & !MULTI_ABSENT, ABSENT_RECEIVERS / 2 + 1);
    unsafe { entry.drop_block() };
}

#[test]
fn retired_holder_and_intermediate_reclaim_only_the_expired_way() {
    let _lock = crate::gc::global_side_table_test_lock();
    let _no_move = crate::gc::GcSuppressScope::new();
    const CID: u32 = 0x0C3C_9012;
    for intermediate in [false, true] {
        let (expired, keys) = owned_receiver_shape(CID, 93_000 + u64::from(intermediate));
        let live = receiver_shape(CID, 93_100 + u64::from(intermediate));
        let mut hops = [(1, if intermediate { expired } else { live })];
        let proof = Entry {
            token: (PIC_ID_TOKEN_BIT | u64::from(live)) as i64,
            class_id: CID,
            depth: 2,
            holder: 1,
            holder_shape: if intermediate { live } else { expired },
            hops: hops.as_mut_ptr(),
            ..EMPTY
        };
        assert!(!unsafe { proof.retired() });
        let mut site = Site {
            primary_class: Entry {
                token: proof.token + 1,
                holder: 0,
                depth: 0,
                hops: std::ptr::null_mut(),
                ..proof
            },
            entries: vec![proof],
            next: 0,
            holders: Vec::new(),
            holder_next: 0,
            accessor_hops: [(0, 0); HOLDER_MAX_DEPTH - 1],
            accessor_depth: 0,
        };
        site.primary_class.token = (PIC_ID_TOKEN_BIT
            | u64::from(receiver_shape(CID, 93_200 + u64::from(intermediate))))
            as i64;
        retire(expired, keys);
        assert!(unsafe { site.entries[0].retired() });
        let fresh = (PIC_ID_TOKEN_BIT
            | u64::from(receiver_shape(CID, 93_300 + u64::from(intermediate))))
            as i64;
        assert_eq!(unsafe { publication_way(&mut site, fresh, CID) }, 1);
        assert_eq!(site.next, 0, "expiry must preserve the live way and cursor");
    }
}

#[test]
fn ordinary_memo_expires_receiver_holder_and_intermediate_shapes() {
    let _lock = crate::gc::global_side_table_test_lock();
    let _no_move = crate::gc::GcSuppressScope::new();
    const CID: u32 = 0x0C3C_9013;
    let live = receiver_shape(CID, 94_000);
    for position in 0..3 {
        let (expired, keys) = owned_receiver_shape(CID, 94_001 + position);
        let mut words = HolderEntry([0; HOLDER_STATE - HOLDER_RECV]);
        words[HOLDER_RECV] =
            (PIC_ID_TOKEN_BIT | u64::from(if position == 0 { expired } else { live })) as i64;
        words[HOLDER_SHAPE] = i64::from(if position == 1 { expired } else { live });
        words[HOLDER_KIND] = (HOLDER_STUB | (2 << HOLDER_DEPTH_SHIFT)) as i64;
        words[HOLDER_HOP_SHAPES] = i64::from(if position == 2 { expired } else { live });
        assert!(!unsafe { super::super::holder_entry_retired(&words) });
        retire(expired, keys);
        assert!(unsafe { super::super::holder_entry_retired(&words) });
        // Accessor pairs/code words must never be mistaken for hop ShapeIds.
        if position != 2 {
            words[HOLDER_KIND] = HOLDER_ACCESSOR as i64;
            assert!(unsafe { super::super::holder_entry_retired(&words) });
        }
    }
}
