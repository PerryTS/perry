use super::*;

pub(super) fn pinned_shape(generation: u64) -> u32 {
    crate::object::shapes::shape_descriptor_ensure_with_generation(
        std::ptr::null(),
        0,
        0,
        generation,
        crate::object::shapes::ShapeObjectKind::Ordinary,
        PROTO_ID_DEFAULT,
        crate::object::shapes::ReceiverFacts::NONE,
    )
    .expect("pinned hop shape")
}

pub(super) fn receiver_shape(class_id: u32, generation: u64) -> u32 {
    crate::object::shapes::shape_descriptor_ensure_with_generation(
        std::ptr::null(),
        0,
        0,
        generation,
        crate::object::shapes::ShapeObjectKind::Ordinary,
        PROTO_ID_CLASS | u64::from(class_id),
        crate::object::shapes::ReceiverFacts::NONE,
    )
    .expect("receiver shape")
}

#[test]
fn empty_site_has_no_facts_or_roots() {
    let record = empty_site();
    let mut cache = [0i64; crate::object::PIC_CACHE_WORDS];
    cache[HOLDER_STATE] = STATE_CLASS_SITE | STATE_REGISTERED;
    cache[SITE_WORD] = (SITE_TAG | record as usize as u64) as i64;
    assert_eq!(unsafe { (*record).next }, 0);
    assert!(unsafe { &*record }.class_entries().all(|e| e.token == 0));
    let recv = shaped(pinned_shape(39_000));
    let token = (PIC_ID_TOKEN_BIT | u64::from(recv.parent_class_id)) as i64;
    assert_eq!(unsafe { leaf_answer(&cache, &*recv, token) }, None);
    let mut seen = Vec::new();
    let mut mark = |v: f64| seen.push(v.to_bits());
    scan_roots(
        &mut cache,
        &mut crate::gc::RuntimeRootVisitor::for_copy(&mut mark),
    );
    assert!(seen.is_empty());
    for e in unsafe { &*record }.class_entries() {
        assert!(e.hops.is_null());
        unsafe { e.drop_block() };
    }
    unsafe { drop(Box::from_raw(record)) };
}

#[test]
fn primary_ways_stay_single_until_churn() {
    if !crate::object::method_site::run_with_fresh_worker_gate(
        "primary_ways_stay_single_until_churn",
    ) {
        return;
    }
    const CID: u32 = 0x0C3C_89A1;
    let terminal = shaped(pinned_shape(40_000));
    let mut receivers: Vec<_> = (0..WAYS + 2)
        .map(|i| {
            let mut r = shaped(receiver_shape(CID, 40_001 + i as u64));
            r.class_id = CID;
            r
        })
        .collect();
    let w = Walk {
        holder: &*terminal as *const ObjectHeader as usize,
        holder_shape: terminal.parent_class_id,
        slot: None,
        hops: NO_HOPS,
        depth: 1,
        getter: 0,
    };
    let mut cache = [0i64; crate::object::PIC_CACHE_WORDS];
    cache[HOLDER_STATE] = STATE_REGISTERED;
    for r in &mut receivers[..WAYS] {
        unsafe { publish(&mut cache, &**r, &w) };
    }
    let s = unsafe { site(&cache).unwrap() };
    assert_eq!(s.class_entries().filter(|e| e.token != 0).count(), WAYS);
    assert!(s.class_entries().all(|e| !e.multi_absent()));
    assert_eq!((s.next >> CURSOR_BITS) & SHARED_MASK, 0);
    unsafe { publish(&mut cache, &*receivers[WAYS], &w) };
    let s = unsafe { site(&cache).unwrap() };
    assert_eq!(s.class_entries().filter(|e| e.token != 0).count(), WAYS);
    assert!(s.class_entries().all(|e| !e.multi_absent()));
    assert_ne!(s.next & ABSENT_CHURN, 0);
    unsafe { publish(&mut cache, &*receivers[WAYS + 1], &w) };
    // The one displaced shape is learned on its next confirmed miss.
    unsafe { publish(&mut cache, &*receivers[0], &w) };
    let s = unsafe { site(&cache).unwrap() };
    assert_eq!(s.class_entries().filter(|e| e.token != 0).count(), 1);
    assert_eq!(s.class_entry(0).slot & !MULTI_ABSENT, (WAYS + 2) as u32);
    assert_eq!((s.next >> CURSOR_BITS) & SHARED_MASK, 1);
    for r in &receivers {
        let token = (u64::from(r.parent_class_id) | PIC_ID_TOKEN_BIT) as i64;
        assert_eq!(
            unsafe { leaf_answer(&cache, &**r, token) },
            Some(crate::value::TAG_UNDEFINED)
        );
    }
    let record = (cache[SITE_WORD] as u64 & crate::value::POINTER_MASK) as usize as *mut Site;
    let data = Walk { slot: Some(0), ..w };
    // Point the cursor at the shared way: reclaimed empties must take
    // priority, even when ordinary rotation would evict the proof.
    unsafe { (*record).next &= !CURSOR_MASK };
    let mut fillers = Vec::new();
    for i in 1..WAYS {
        let cid = CID + i as u32;
        let mut r = shaped(receiver_shape(cid, 41_000 + i as u64));
        r.class_id = cid;
        unsafe { publish(&mut cache, &*r, &data) };
        fillers.push(r);
    }
    // Fill reclaimed ways with unrelated data proofs, then replace a
    // data way. Advancing the cursor must retain the shared-way bits.
    unsafe { (*record).next = ((*record).next & !CURSOR_MASK) | 1 };
    let cid = CID + WAYS as u32;
    let mut extra = shaped(receiver_shape(cid, 42_000));
    extra.class_id = cid;
    unsafe { publish(&mut cache, &*extra, &data) };
    assert_eq!((unsafe { (*record).next } >> CURSOR_BITS) & SHARED_MASK, 1);
    assert_ne!(unsafe { (*record).next } & ABSENT_CHURN, 0);
    for r in &receivers {
        let token = (u64::from(r.parent_class_id) | PIC_ID_TOKEN_BIT) as i64;
        assert_eq!(
            unsafe { leaf_answer(&cache, &**r, token) },
            Some(crate::value::TAG_UNDEFINED)
        );
    }
    unsafe { (*record).primary_class.drop_block() };
    unsafe { drop(Box::from_raw(record)) };
}

#[test]
fn non_absent_and_same_receiver_refreshes_do_not_arm_sharing() {
    if !crate::object::method_site::run_with_fresh_worker_gate(
        "non_absent_and_same_receiver_refreshes_do_not_arm_sharing",
    ) {
        return;
    }
    const CID: u32 = 0x0C3C_89A2;
    let holder = shaped(pinned_shape(43_000));
    let absent = deep_walk(&[], &holder);
    let data = Walk {
        slot: Some(0),
        ..absent
    };
    let mut receivers: Vec<_> = (0..WAYS * 3)
        .map(|i| {
            let mut r = shaped(receiver_shape(CID, 43_001 + i as u64));
            r.class_id = CID;
            r
        })
        .collect();
    let mut cache = [0i64; crate::object::PIC_CACHE_WORDS];
    cache[HOLDER_STATE] = STATE_REGISTERED;
    for r in &receivers {
        unsafe { publish(&mut cache, &**r, &data) };
    }
    let record = (cache[SITE_WORD] as u64 & crate::value::POINTER_MASK) as usize as *mut Site;
    assert_eq!(unsafe { (*record).next } & ABSENT_CHURN, 0);
    assert!(unsafe { &*record }
        .class_entries()
        .all(|e| !e.multi_absent()));
    for e in unsafe { &*record }.class_entries() {
        unsafe { e.drop_block() };
    }
    unsafe { drop(Box::from_raw(record)) };
    cache = [0i64; crate::object::PIC_CACHE_WORDS];
    cache[HOLDER_STATE] = STATE_REGISTERED;
    for _ in 0..WAYS * 3 {
        unsafe { publish(&mut cache, &*receivers[0], &absent) };
    }
    let record = (cache[SITE_WORD] as u64 & crate::value::POINTER_MASK) as usize as *mut Site;
    assert_eq!(unsafe { (*record).next } & ABSENT_CHURN, 0);
    assert!(unsafe { &*record }
        .class_entries()
        .all(|e| !e.multi_absent()));
    for e in unsafe { &*record }.class_entries() {
        unsafe { e.drop_block() };
    }
    unsafe { drop(Box::from_raw(record)) };
    receivers.clear();
}

/// After observed churn, 96 receiver shapes coalesce into ONE way. Every guard
/// remains necessary and only its chain prefix is enumerated as roots.
#[test]
fn multi_absent_shares_chain_and_checks_every_shape() {
    if !crate::object::method_site::run_with_fresh_worker_gate(
        "multi_absent_shares_chain_and_checks_every_shape",
    ) {
        return;
    }
    let _lock = crate::gc::global_side_table_test_lock();
    const CID: u32 = 0x0C3C_7A10;
    let base = crate::object::shapes::SHAPE_ID_BASE;
    let chain: Vec<_> = (0..8).map(|i| shaped(pinned_shape(10_000 + i))).collect();
    let holder = shaped(base + 200);
    let w = deep_walk(&chain, &holder);
    let mut receivers: Vec<_> = (1..=96)
        .map(|i| {
            let mut r = shaped(receiver_shape(CID, i));
            r.class_id = CID;
            r
        })
        .collect();
    let mut cache = [0i64; crate::object::PIC_CACHE_WORDS];
    cache[HOLDER_STATE] = STATE_REGISTERED;
    let before = stats().0;
    for r in receivers.iter().chain(&receivers) {
        unsafe { publish(&mut cache, &**r, &w) };
    }
    let record = (cache[SITE_WORD] as u64 & crate::value::POINTER_MASK) as usize as *mut Site;
    let s = unsafe { &mut *record };
    assert_eq!(s.class_entries().filter(|e| e.token != 0).count(), 1);
    assert_eq!(stats().0 - before, (WAYS + 1) as u64);
    let e = s.class_entry_mut(0);
    assert!(e.multi_absent(), "the multi-shape proof must be exercised");
    assert_eq!(
        (unsafe { site(&cache).unwrap().next } >> CURSOR_BITS) & SHARED_MASK,
        1
    );
    assert_ne!(unsafe { site(&cache).unwrap().next } & ABSENT_CHURN, 0);
    assert_eq!(e.slot & !MULTI_ABSENT, 96);
    assert_eq!(unsafe { e.hops() }.len(), 8);
    assert_eq!(std::mem::size_of::<Entry>(), 48);
    // Fake hops are not registered shapes. Exercise the same pinned
    // shape comparisons the real admitted walk proves.
    for r in &receivers {
        let token = (PIC_ID_TOKEN_BIT | u64::from(r.parent_class_id)) as i64;
        assert_eq!(
            unsafe { answer(e, &**r) },
            Some(crate::value::TAG_UNDEFINED)
        );
        assert_eq!(
            unsafe { leaf_answer(&cache, &**r, token) },
            Some(crate::value::TAG_UNDEFINED),
        );
    }
    let r = &mut *receivers[47];
    let original = r.parent_class_id;
    r.parent_class_id = receiver_shape(CID, 1000);
    let token = (PIC_ID_TOKEN_BIT | u64::from(r.parent_class_id)) as i64;
    assert_eq!(
        unsafe { answer(e, r) },
        None,
        "an own add/getter/relink restamps"
    );
    assert_eq!(unsafe { leaf_answer(&cache, r, token) }, None);
    r.parent_class_id = original;
    let token = (PIC_ID_TOKEN_BIT | u64::from(original)) as i64;
    for (i, hop) in chain.iter().enumerate() {
        let ptr = &**hop as *const ObjectHeader as *mut ObjectHeader;
        let original = unsafe { (*ptr).parent_class_id };
        unsafe { (*ptr).parent_class_id = base + 500 };
        assert_eq!(unsafe { answer(e, r) }, None, "hop {i} changed");
        assert_eq!(
            unsafe { leaf_answer(&cache, r, token) },
            None,
            "leaf hop {i}"
        );
        unsafe { (*ptr).parent_class_id = original };
    }
    let ptr = &*holder as *const ObjectHeader as *mut ObjectHeader;
    unsafe { (*ptr).parent_class_id = base + 501 };
    assert_eq!(unsafe { answer(e, r) }, None, "terminal changed");
    assert_eq!(unsafe { leaf_answer(&cache, r, token) }, None);
    unsafe { (*ptr).parent_class_id = base + 200 };
    let mut seen = Vec::new();
    let mut mark = |v: f64| seen.push(v.to_bits() & crate::value::POINTER_MASK);
    scan_roots(
        &mut cache,
        &mut crate::gc::RuntimeRootVisitor::for_copy(&mut mark),
    );
    assert_eq!(
        seen.len(),
        9,
        "receiver ids must not be visited as pointers"
    );
    for hop in &chain {
        assert!(seen.contains(&((&**hop as *const ObjectHeader) as u64)));
    }
    assert!(seen.contains(&((&*holder as *const ObjectHeader) as u64)));
    // A new chain for a member replaces the owned group allocation.
    let short = deep_walk(&chain[..1], &holder);
    unsafe { publish(&mut cache, r, &short) };
    assert_eq!(s.class_entry(0).depth, 2);
    assert!(!s.class_entry(0).multi_absent());
    assert_eq!((s.next >> CURSOR_BITS) & SHARED_MASK, 0);
    unsafe { s.class_entry(0).drop_block() };
    unsafe { drop(Box::from_raw(record)) };
}

#[test]
fn multi_absent_bound_and_prototype_identity_are_enforced() {
    if !crate::object::method_site::run_with_fresh_worker_gate(
        "multi_absent_bound_and_prototype_identity_are_enforced",
    ) {
        return;
    }
    let _lock = crate::gc::global_side_table_test_lock();
    const CID: u32 = 0x0C3C_7A11;
    let base = crate::object::shapes::SHAPE_ID_BASE;
    let holder = shaped(base + 200);
    let w = deep_walk(&[], &holder);
    let mut cache = [0i64; crate::object::PIC_CACHE_WORDS];
    cache[HOLDER_STATE] = STATE_REGISTERED;
    let receivers: Vec<_> = (1..=ABSENT_RECEIVERS + 5)
        .map(|i| {
            let mut r = shaped(receiver_shape(CID, u64::from(i)));
            r.class_id = CID;
            r
        })
        .collect();
    for r in receivers.iter().chain(&receivers) {
        unsafe { publish(&mut cache, &**r, &w) };
    }
    let record = (cache[SITE_WORD] as u64 & crate::value::POINTER_MASK) as usize as *mut Site;
    let s = unsafe { &mut *record };
    assert_eq!(s.class_entries().filter(|e| e.token != 0).count(), 2);
    assert_eq!(s.class_entry(0).slot & !MULTI_ABSENT, ABSENT_RECEIVERS);
    // The class id and terminal alone do not prove a common direct link.
    let shape = crate::object::shapes::shape_descriptor_ensure_with_generation(
        std::ptr::null(),
        0,
        0,
        2000,
        crate::object::shapes::ShapeObjectKind::Ordinary,
        PROTO_ID_MIXED | u64::from(CID),
        crate::object::shapes::ReceiverFacts::NONE,
    )
    .unwrap();
    let mut other = shaped(shape);
    other.class_id = CID;
    unsafe { publish(&mut cache, &*other, &w) };
    assert_eq!(s.class_entries().filter(|e| e.token != 0).count(), 3);
    for r in &receivers {
        let token = (PIC_ID_TOKEN_BIT | u64::from(r.parent_class_id)) as i64;
        assert!(s
            .class_entries()
            .any(|e| unsafe { e.matches_receiver(token, CID) }));
    }
    for e in s.class_entries() {
        unsafe { e.drop_block() };
    }
    unsafe { drop(Box::from_raw(record)) };
}

#[test]
fn multi_absent_reproves_shared_class_link_after_generation_change() {
    if !crate::object::method_site::run_with_fresh_worker_gate(
        "multi_absent_reproves_shared_class_link_after_generation_change",
    ) {
        return;
    }
    let _lock = crate::gc::global_side_table_test_lock();
    // Real GC headers are required by stated_link; keep this fixture
    // in a no-move scope while its raw receiver vector is constructed.
    let _no_move = crate::gc::GcSuppressScope::new();
    const CID: u32 = 0x0C3C_7A12;
    const PROTO_CID: u32 = 0x0C3C_7A13;
    let keys = crate::object::js_build_class_keys_array(PROTO_CID, 1, b"marker".as_ptr(), 6, 0);
    let shape = crate::object::shapes::js_object_shape_id_for_class_keys(
        keys as usize as u64,
        1,
        PROTO_CID,
        0,
    );
    let a =
        crate::object::js_object_alloc_class_inline_keys_stamped(PROTO_CID, 0, 1, keys, shape, 0);
    // Seed the bare link to preserve the deliberate equal-shape premise.
    crate::object::test_seed_class_decl_prototype_object_root(CID, a as usize);
    let b =
        crate::object::js_object_alloc_class_inline_keys_stamped(PROTO_CID, 0, 1, keys, shape, 0);
    let a = crate::object::class_decl_prototype_object(CID);
    assert_ne!(a, b);
    assert_eq!(unsafe { object_shape_stamp(a) }, unsafe {
        object_shape_stamp(b)
    });
    let receivers: Vec<_> = (1..=24)
        .map(|i| {
            let name = (0..i)
                .map(|n| format!("own{n}"))
                .collect::<Vec<_>>()
                .join("\0");
            let keys = crate::object::js_build_class_keys_array(
                CID,
                i,
                name.as_ptr(),
                name.len() as u32,
                0,
            );
            let shape = crate::object::shapes::js_object_shape_id_for_class_keys(
                keys as usize as u64,
                i,
                CID,
                0,
            );
            crate::object::js_object_alloc_class_inline_keys_stamped(CID, 0, i, keys, shape, 0)
        })
        .collect();
    let w = Walk {
        holder: a as usize,
        holder_shape: unsafe { object_shape_stamp(a) },
        slot: None,
        hops: NO_HOPS,
        depth: 1,
        getter: 0,
    };
    let mut cache = [0i64; crate::object::PIC_CACHE_WORDS];
    cache[HOLDER_STATE] = STATE_REGISTERED;
    for r in receivers.iter().chain(&receivers) {
        assert_eq!(unsafe { class_link(*r) }, Some(a as *const ObjectHeader));
        unsafe { publish(&mut cache, *r, &w) };
    }
    let record = (cache[SITE_WORD] as u64 & crate::value::POINTER_MASK) as usize as *mut Site;
    let e = unsafe { &(*record).primary_class };
    assert!(e.multi_absent());
    assert_eq!(e.slot & !MULTI_ABSENT, 24);
    crate::object::class_registry::class_lookup_surface_gen_bump();
    for r in &receivers {
        let token = (PIC_ID_TOKEN_BIT | u64::from(unsafe { object_shape_stamp(*r) })) as i64;
        assert_eq!(unsafe { leaf_answer(&cache, *r, token) }, None);
    }
    assert_eq!(
        unsafe { try_shared_hit(record, receivers[5]) },
        Some(crate::value::TAG_UNDEFINED)
    );
    // A same-identity generation re-proof covers the whole set.
    for r in &receivers {
        let token = (PIC_ID_TOKEN_BIT | u64::from(unsafe { object_shape_stamp(*r) })) as i64;
        assert_eq!(
            unsafe { leaf_answer(&cache, *r, token) },
            Some(crate::value::TAG_UNDEFINED)
        );
    }
    crate::object::test_seed_class_decl_prototype_object_root(CID, b as usize);
    for r in &receivers {
        let token = (PIC_ID_TOKEN_BIT | u64::from(unsafe { object_shape_stamp(*r) })) as i64;
        assert_eq!(unsafe { leaf_answer(&cache, *r, token) }, None);
        assert_eq!(unsafe { try_shared_hit(record, *r) }, None);
    }
    unsafe { (*record).primary_class.drop_block() };
    unsafe { drop(Box::from_raw(record)) };
}

#[test]
fn collecting_shared_reads_check_receiver_and_chain() {
    if !crate::object::method_site::run_with_fresh_worker_gate(
        "collecting_shared_reads_check_receiver_and_chain",
    ) {
        return;
    }
    let _lock = crate::gc::global_side_table_test_lock();
    const CID: u32 = 0x0C3C_7A14;
    let mut chain = vec![shaped(pinned_shape(11_000))];
    let mut holder = shaped(crate::object::shapes::SHAPE_ID_BASE + 300);
    let w = deep_walk(&chain, &holder);
    let mut receivers: Vec<_> = (1..=24)
        .map(|i| {
            let mut r = shaped(receiver_shape(CID, i));
            r.class_id = CID;
            r
        })
        .collect();
    let mut cache = [0i64; crate::object::PIC_CACHE_WORDS];
    cache[HOLDER_STATE] = STATE_REGISTERED;
    for r in receivers.iter().chain(&receivers) {
        unsafe { publish(&mut cache, &**r, &w) };
    }
    let record = (cache[SITE_WORD] as u64 & crate::value::POINTER_MASK) as usize as *mut Site;
    let r = &mut *receivers[5];
    assert_eq!(
        unsafe { try_shared_hit(record, r) },
        Some(crate::value::TAG_UNDEFINED)
    );
    let shape = r.parent_class_id;
    r.parent_class_id = receiver_shape(CID, 1000);
    assert_eq!(
        unsafe { try_shared_hit(record, r) },
        None,
        "receiver restamped"
    );
    r.parent_class_id = shape;
    r.class_id = CID + 1;
    assert_eq!(
        unsafe { try_shared_hit(record, r) },
        None,
        "different class"
    );
    r.class_id = CID;
    let shape = chain[0].parent_class_id;
    chain[0].parent_class_id = crate::object::shapes::SHAPE_ID_BASE + 301;
    assert_eq!(unsafe { try_shared_hit(record, r) }, None, "hop restamped");
    chain[0].parent_class_id = shape;
    let shape = holder.parent_class_id;
    holder.parent_class_id = crate::object::shapes::SHAPE_ID_BASE + 302;
    assert_eq!(
        unsafe { try_shared_hit(record, r) },
        None,
        "terminal restamped"
    );
    holder.parent_class_id = shape;
    assert_eq!(
        unsafe { try_shared_hit(record, r) },
        Some(crate::value::TAG_UNDEFINED)
    );
    for e in unsafe { &*record }.class_entries() {
        unsafe { e.drop_block() };
    }
    unsafe { drop(Box::from_raw(record)) };
}

#[test]
fn unpinned_hop_is_not_published() {
    if !crate::object::method_site::run_with_fresh_worker_gate("unpinned_hop_is_not_published") {
        return;
    }
    let _lock = crate::gc::global_side_table_test_lock();
    const CID: u32 = 0x0C3C_7A14;
    let recv = shaped(receiver_shape(CID, 1));
    let chain = vec![shaped(receiver_shape(CID, 2))];
    let holder = shaped(pinned_shape(30_000));
    let w = deep_walk(&chain, &holder);
    let mut cache = [0i64; crate::object::PIC_CACHE_WORDS];
    cache[HOLDER_STATE] = STATE_REGISTERED;
    unsafe { publish(&mut cache, &*recv, &w) };
    assert!(
        unsafe { site(&cache) }.is_none(),
        "a bare CLASS hop cannot pin the next prototype by shape"
    );
}

/// Fake objects whose only meaningful word is their ShapeId.
pub(super) fn shaped(shape: u32) -> Box<ObjectHeader> {
    if crate::object::shapes::shape_is_retired(shape) {
        assert!(crate::object::shapes::test_install_external_shape_id(
            shape,
            std::ptr::null(),
            0,
            0,
        ));
    }
    Box::new(ObjectHeader {
        class_id: 0,
        parent_class_id: shape,
        meta: std::ptr::null_mut(),
    })
}

fn deep_walk(chain: &[Box<ObjectHeader>], holder: &ObjectHeader) -> Walk {
    let mut hops = NO_HOPS;
    for (i, h) in chain.iter().enumerate() {
        hops[i] = ((&**h as *const ObjectHeader) as usize, h.parent_class_id);
    }
    Walk {
        holder: (holder as *const ObjectHeader) as usize,
        holder_shape: holder.parent_class_id,
        slot: None,
        hops,
        depth: chain.len() + 1,
        getter: 0,
    }
}

/// A chain deeper than the site's own holder words: every hop, the ones
/// past the holder entry's words included, is compared on every use, the
/// root scan reaches the deepest hop, and a way overwritten by a shallow
/// chain trades its block for one sized to the new chain.
#[test]
fn deep_entry_compares_every_hop() {
    if !crate::object::method_site::run_with_fresh_worker_gate("deep_entry_compares_every_hop") {
        return;
    }
    let _lock = crate::gc::global_side_table_test_lock();
    let base = crate::object::shapes::SHAPE_ID_BASE;
    let recv = shaped(base + 1);
    let chain: Vec<Box<ObjectHeader>> = (0..CLASS_READ_MAX_DEPTH as u32 - 1)
        .map(|i| shaped(pinned_shape(20_000 + u64::from(i))))
        .collect();
    let holder = shaped(base + 200);
    let w = deep_walk(&chain, &holder);
    assert_eq!(w.depth, CLASS_READ_MAX_DEPTH);
    let mut cache = [0i64; crate::object::PIC_CACHE_WORDS];
    // Skip registration: the stack cache is not a process-lifetime PIC
    // allocation, and this test drives only the published words.
    cache[HOLDER_STATE] = STATE_REGISTERED;
    unsafe { publish(&mut cache, &*recv, &w) };
    let token = (PIC_ID_TOKEN_BIT | u64::from(base + 1)) as i64;
    let s = unsafe { site(&cache) }.expect("published site");
    let e = s.class_entries().find(|e| e.token == token).expect("entry");
    assert_eq!(
        unsafe { e.hops() }.len(),
        CLASS_READ_MAX_DEPTH - 1,
        "a deep chain's block holds every hop"
    );
    // Fake headers carry admitted shape descriptors; the comparison
    // checks below exercise every shape the real walk records.
    let mut e = *e;
    e.generation = crate::object::class_lookup_surface_generation();
    assert_eq!(
        unsafe { pinned_answer(&e) },
        Some(crate::value::TAG_UNDEFINED)
    );
    // Each hop's ShapeId is a fact the answer rests on, the deepest too.
    for i in [
        0,
        HOLDER_MAX_DEPTH - 2,
        HOLDER_MAX_DEPTH - 1,
        CLASS_READ_MAX_DEPTH - 2,
    ] {
        let hop = &*chain[i] as *const ObjectHeader as *mut ObjectHeader;
        let original = unsafe { (*hop).parent_class_id };
        unsafe { (*hop).parent_class_id = base + 300 };
        assert_eq!(unsafe { pinned_answer(&e) }, None, "hop {i} moved");
        unsafe { (*hop).parent_class_id = original };
    }
    // The root scan visits every hop, the deepest too.
    let deepest = (&*chain[CLASS_READ_MAX_DEPTH - 2] as *const ObjectHeader) as usize;
    let mut seen: Vec<u64> = Vec::new();
    {
        let mut mark = |v: f64| seen.push(v.to_bits() & crate::value::POINTER_MASK);
        let mut visitor = crate::gc::RuntimeRootVisitor::for_copy(&mut mark);
        scan_roots(&mut cache, &mut visitor);
    }
    assert!(
        seen.contains(&(deepest as u64)),
        "the root scan must visit the deepest hop"
    );
    // A shallow chain published over the same way gets a block its size.
    let shallow = deep_walk(&chain[..1], &holder);
    unsafe { publish(&mut cache, &*recv, &shallow) };
    let s = unsafe { site(&cache) }.expect("published site");
    let e = s.class_entries().find(|e| e.token == token).expect("entry");
    assert_eq!(e.depth, 2);
    assert_eq!(unsafe { e.hops() }, &[w.hops[0]][..]);
    let block = unsafe { hop_block(e.hops, e.depth) } as *mut [Hop];
    unsafe { drop(Box::from_raw(block)) };
    let record = (cache[SITE_WORD] as u64 & crate::value::POINTER_MASK) as usize as *mut Site;
    unsafe { drop(Box::from_raw(record)) };
}

#[test]
fn foreign_scratch_word_is_not_a_site() {
    let mut cache = [0i64; crate::object::PIC_CACHE_WORDS];
    cache[SITE_WORD] = 0xA11CE;
    assert!(unsafe { site(&cache) }.is_none());
    cache[HOLDER_STATE] |= STATE_CLASS_SITE;
    assert!(unsafe { site(&cache) }.is_none());
}

#[test]
fn bare_class_link_replacement_with_same_holder_shape_declines() {
    if !crate::object::method_site::run_with_fresh_worker_gate(
        "bare_class_link_replacement_with_same_holder_shape_declines",
    ) {
        return;
    }
    let _lock = crate::gc::global_side_table_test_lock();
    const CID: u32 = 0x0C3C_79A3;
    const PROTO_CID: u32 = 0x0C3C_79A4;
    let proto_keys =
        crate::object::js_build_class_keys_array(PROTO_CID, 1, b"marker".as_ptr(), 6, 0);
    let proto_shape = crate::object::shapes::js_object_shape_id_for_class_keys(
        proto_keys as usize as u64,
        1,
        PROTO_CID,
        0,
    );
    let a = crate::object::js_object_alloc_class_inline_keys_stamped(
        PROTO_CID,
        0,
        1,
        proto_keys,
        proto_shape,
        0,
    );
    // Seed the bare link to preserve the deliberate equal-shape premise.
    crate::object::test_seed_class_decl_prototype_object_root(CID, a as usize);
    let b = crate::object::js_object_alloc_class_inline_keys_stamped(
        PROTO_CID,
        0,
        1,
        proto_keys,
        proto_shape,
        0,
    );
    let a = crate::object::class_decl_prototype_object(CID);
    assert_ne!(a, b);
    assert_eq!(unsafe { object_shape_stamp(a) }, unsafe {
        object_shape_stamp(b)
    });
    let recv_keys = crate::object::js_build_class_keys_array(CID, 1, b"own".as_ptr(), 3, 0);
    let recv_shape = crate::object::shapes::js_object_shape_id_for_class_keys(
        recv_keys as usize as u64,
        1,
        CID,
        0,
    );
    let recv = crate::object::js_object_alloc_class_inline_keys_stamped(
        CID, 0, 1, recv_keys, recv_shape, 0,
    );
    assert_eq!(unsafe { class_link(recv) }, Some(a as *const ObjectHeader));
    let mut entry = Entry {
        token: (PIC_ID_TOKEN_BIT | u64::from(recv_shape)) as i64,
        class_id: CID,
        depth: 1,
        absent: true,
        pinned_hops: true,
        slot: 0,
        holder: a as usize,
        holder_shape: proto_shape,
        hops: std::ptr::null_mut(),
        generation: 0,
    };
    assert_eq!(
        unsafe { answer(&mut entry, recv) },
        Some(crate::value::TAG_UNDEFINED)
    );
    let mut data_entry = entry;
    data_entry.absent = false;
    unsafe {
        let slot = (a as *mut u8).add(std::mem::size_of::<ObjectHeader>()) as *mut u64;
        // GC_STORE_AUDIT(POINTER_FREE): the test stores Number bits, never a heap pointer.
        std::ptr::write(slot, 42.0f64.to_bits());
        assert_eq!(answer(&mut data_entry, recv), Some(42.0f64.to_bits()));
        // GC_STORE_AUDIT(POINTER_FREE): undefined is an immediate NaN-box tag.
        std::ptr::write(slot, crate::value::TAG_UNDEFINED);
        assert_eq!(answer(&mut data_entry, recv), None);
        // GC_STORE_AUDIT(POINTER_FREE): null is an immediate NaN-box tag.
        std::ptr::write(slot, crate::value::TAG_NULL);
        assert_eq!(answer(&mut data_entry, recv), None);
        // GC_STORE_AUDIT(POINTER_FREE): the test stores Number bits, never a heap pointer.
        std::ptr::write(slot, 43.0f64.to_bits());
        assert_eq!(answer(&mut data_entry, recv), Some(43.0f64.to_bits()));
    }
    crate::object::test_seed_class_decl_prototype_object_root(CID, b as usize);
    assert_eq!(unsafe { answer(&mut entry, recv) }, None);
    // The displaced holder's ShapeId was retired: an entry naming it can
    // never answer again, whatever the registry says later.
    assert_ne!(unsafe { object_shape_stamp(a) }, proto_shape);

    crate::object::test_seed_class_decl_prototype_object_root(CID, a as usize);
    assert_eq!(unsafe { answer(&mut entry, recv) }, None);
    let mut entry = Entry {
        holder_shape: unsafe { object_shape_stamp(a) },
        ..entry
    };
    assert_eq!(
        unsafe { answer(&mut entry, recv) },
        Some(crate::value::TAG_UNDEFINED)
    );
    let record = Box::into_raw(Box::new(Site {
        primary_class: entry,
        entries: vec![entry; WAYS - 1],
        holders: Vec::new(),
        holder_next: 0,
        next: 0,
        accessor_hops: [(0, 0); HOLDER_MAX_DEPTH - 1],
        accessor_depth: 0,
    }));
    let mut cache = [0i64; crate::object::PIC_CACHE_WORDS];
    cache[SITE_WORD] = (SITE_TAG | record as usize as u64) as i64;
    cache[HOLDER_STATE] = STATE_CLASS_SITE;
    let mut slot = &mut cache as *mut PicCache;
    assert_eq!(
        unsafe { try_hit(recv, &mut slot) }.map(|v| v.bits()),
        Some(crate::value::TAG_UNDEFINED)
    );
    WORKER_AGENTS_EXIST.store(1, Ordering::SeqCst);
    assert!(unsafe { try_hit(recv, &mut slot) }.is_none());
    unsafe { drop(Box::from_raw(record)) };
}
