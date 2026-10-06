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
    let mut next = 3;
    let fresh = (PIC_ID_TOKEN_BIT | u64::from(receiver_shape(CID, 90_003))) as i64;
    assert_eq!(
        unsafe { publication_way(&entries, &mut next, fresh, CID) },
        7
    );
    assert_eq!(next, 3, "expiry does not advance or arm the cursor");
    // A matching live way still wins over an expired one.
    assert_eq!(
        unsafe { publication_way(&entries, &mut next, entries[0].token, CID) },
        0
    );
    entries[7] = entries[0];
    entries[11] = EMPTY;
    assert_eq!(
        unsafe { publication_way(&entries, &mut next, fresh, CID) },
        11
    );
    entries[11] = entries[0];
    assert_eq!(
        unsafe { publication_way(&entries, &mut next, fresh, CID) },
        3
    );
    assert_eq!(next, 4);
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
    assert!(s.entries.iter().all(|e| !e.multi_absent()));
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
    for e in &s.entries {
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
