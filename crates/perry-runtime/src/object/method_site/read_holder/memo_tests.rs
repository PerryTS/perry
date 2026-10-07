//! Complete per-shape answers must remain independently usable and guarded.
use super::*;

fn shaped(shape: u32) -> Box<ObjectHeader> {
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

#[test]
fn alternating_terminals_and_deep_hops_hit_and_invalidate_independently() {
    if !super::super::run_with_fresh_worker_gate(
        "alternating_terminals_and_deep_hops_hit_and_invalidate_independently",
    ) {
        return;
    }
    let _lock = crate::gc::global_side_table_test_lock();
    let base = crate::object::shapes::SHAPE_ID_BASE;
    let receivers: Vec<_> = (0..10).map(|i| shaped(base + i)).collect();
    let mut holders: Vec<_> = (0..10).map(|i| shaped(base + 100 + i)).collect();
    let mut hops: Vec<_> = (0..10).map(|i| shaped(base + 200 + i)).collect();
    let mut cache = [0; crate::object::PIC_CACHE_WORDS];
    cache[HOLDER_STATE] = STATE_REGISTERED; // stack cache: do not register it
    for i in 0..9 {
        let mut chain = NO_HOPS;
        chain[0] = (
            (&*hops[i] as *const ObjectHeader) as usize,
            base + 200 + i as u32,
        );
        let walk = Walk {
            holder: (&*holders[i] as *const ObjectHeader) as usize,
            holder_shape: base + 100 + i as u32,
            slot: None,
            hops: chain,
            depth: 2,
            getter: 0,
        };
        unsafe { publish(&mut cache, &*receivers[i], &walk, false) };
    }
    let token = |i: usize| (PIC_ID_TOKEN_BIT | u64::from(base + i as u32)) as i64;
    // Nine complete answers, including distinct holders and deep chains.
    for _ in 0..10 {
        for i in 0..9 {
            assert_eq!(
                unsafe { entry_answer(&cache, token(i)) },
                Some(crate::value::TAG_UNDEFINED)
            );
            assert_eq!(
                unsafe {
                    primary_entry_answer(&cache, token(i)).or_else(|| {
                        let bits = class_entry_answer(&cache, &*receivers[i], token(i));
                        (bits != crate::value::TAG_HOLE).then_some(bits)
                    })
                },
                Some(crate::value::TAG_UNDEFINED),
                "native front must use each saved entry after primary admission fails"
            );
        }
    }
    hops[3].parent_class_id += 1000;
    assert_eq!(unsafe { entry_answer(&cache, token(3)) }, None);
    assert_eq!(
        unsafe { entry_answer(&cache, token(2)) },
        Some(crate::value::TAG_UNDEFINED)
    );
    holders[4].parent_class_id += 1000;
    assert_eq!(unsafe { entry_answer(&cache, token(4)) }, None);
    assert_eq!(
        unsafe { entry_answer(&cache, token(5)) },
        Some(crate::value::TAG_UNDEFINED)
    );
    assert_eq!(
        unsafe { entry_answer(&cache, token(9)) },
        None,
        "unseen receiver"
    );
    refuse();
    assert_eq!(
        cache[HOLDER_STATE] & STATE_LATCHED,
        0,
        "refusal is not a site latch"
    );
    assert_eq!(
        unsafe { entry_answer(&cache, token(2)) },
        Some(crate::value::TAG_UNDEFINED)
    );
    let mut seen = Vec::new();
    let mut mark = |v: f64| seen.push(v.to_bits() & crate::value::POINTER_MASK);
    class_read::scan_roots(
        &mut cache,
        &mut crate::gc::RuntimeRootVisitor::for_copy(&mut mark),
    );
    for i in 0..8 {
        assert!(seen.contains(&((&*holders[i] as *const ObjectHeader) as u64)));
        assert!(seen.contains(&((&*hops[i] as *const ObjectHeader) as u64)));
    }
    let tenth = Walk {
        holder: (&*holders[9] as *const ObjectHeader) as usize,
        holder_shape: base + 109,
        slot: None,
        hops: NO_HOPS,
        depth: 1,
        getter: 0,
    };
    unsafe { publish(&mut cache, &*receivers[9], &tenth, false) };
    assert_eq!(
        unsafe { entry_answer(&cache, token(0)) },
        None,
        "bounded eviction removes the oldest answer"
    );
    assert_eq!(
        unsafe { entry_answer(&cache, token(1)) },
        Some(crate::value::TAG_UNDEFINED)
    );
    assert_eq!(
        unsafe { entry_answer(&cache, token(9)) },
        Some(crate::value::TAG_UNDEFINED)
    );
    let first = Walk {
        holder: (&*holders[0] as *const ObjectHeader) as usize,
        holder_shape: base + 100,
        ..tenth
    };
    unsafe { publish(&mut cache, &*receivers[0], &first, false) };
    assert_eq!(
        unsafe { entry_answer(&cache, token(0)) },
        Some(crate::value::TAG_UNDEFINED),
        "an evicted receiver can prime again"
    );
    WORKER_AGENTS_EXIST.store(1, Ordering::SeqCst);
    assert_eq!(unsafe { entry_answer(&cache, token(2)) }, None);
}

extern "C" fn getter_eight(_this: f64) -> f64 {
    8.0
}

extern "C" fn receiver_number(this: f64) -> f64 {
    let addr = (this.to_bits() & crate::value::POINTER_MASK) as usize;
    f64::from_bits(unsafe { slot_bits(addr, 0) })
}

#[test]
fn alternating_getter_receivers_use_cached_pairs_and_original_this() {
    if !super::super::run_with_fresh_worker_gate(
        "alternating_getter_receivers_use_cached_pairs_and_original_this",
    ) {
        return;
    }
    let _lock = crate::gc::global_side_table_test_lock();
    let scope = crate::gc::RuntimeHandleScope::new();
    let key = scope.root_string_ptr(crate::string::js_string_from_bytes(b"answer".as_ptr(), 6));
    let first_key =
        scope.root_string_ptr(crate::string::js_string_from_bytes(b"first".as_ptr(), 5));
    let second_key =
        scope.root_string_ptr(crate::string::js_string_from_bytes(b"second".as_ptr(), 6));
    let holder = scope.root_raw_mut_ptr(crate::object::js_object_alloc(0, 2));
    holder.with_mut_ptr::<ObjectHeader, _>(|h| {
        crate::object::set_builtin_accessor_pair(
            h as usize,
            "answer".to_owned(),
            crate::object::accessor_pair::Accessor {
                raw_get: receiver_number as *const () as usize,
                ..Default::default()
            },
            crate::object::PropertyAttrs::new(true, false, true),
        );
    });
    let a = scope.root_raw_mut_ptr(crate::object::js_object_alloc(0, 2));
    let b = scope.root_raw_mut_ptr(crate::object::js_object_alloc(0, 2));
    let exotic = scope.root_raw_mut_ptr(crate::object::js_object_alloc(0, 2));
    exotic.with_mut_ptr::<ObjectHeader, _>(|r| unsafe {
        crate::object::proto_validity::mark_exotic_read_receiver(r as usize);
    });
    for (recv, own, n) in [(&a, &first_key, 12.0), (&b, &second_key, 18.0)] {
        recv.with_mut_ptr::<ObjectHeader, _>(|r| {
            own.with_const_ptr(|k| crate::object::js_object_set_field_by_name(r, k, n));
            holder.with_const_ptr::<ObjectHeader, _>(|h| {
                crate::object::js_object_set_prototype_of(
                    crate::value::js_nanbox_pointer(r as i64),
                    crate::value::js_nanbox_pointer(h as i64),
                );
            });
        });
    }
    let mut slot: PicCacheSlot = std::ptr::null_mut();
    for (recv, n) in [(&a, 12.0), (&b, 18.0)] {
        recv.with_const_ptr::<ObjectHeader, _>(|r| {
            key.with_const_ptr(|k| {
                assert_eq!(
                    unsafe { prime_read_holder(r, k, &mut slot) }.map(|v| v.as_number()),
                    Some(n)
                );
                if n == 12.0 {
                    let cache = unsafe {
                        crate::object::field_get_set::pic_slot_peek::<PicCache>(&mut slot)
                    };
                    assert!(
                        !unsafe { class_read::has_site(&*cache) },
                        "a monomorphic ordinary site must not allocate saved-answer storage"
                    );
                }
            })
        });
    }
    let primes = read_accessor_stats().0;
    exotic.with_const_ptr::<ObjectHeader, _>(|r| {
        key.with_const_ptr(|k| {
            assert!(unsafe { prime_read_holder(r, k, &mut slot) }.is_none());
        });
    });
    for _ in 0..20 {
        for (recv, n) in [(&a, 12.0), (&b, 18.0)] {
            recv.with_const_ptr::<ObjectHeader, _>(|r| {
                assert!(
                    !unsafe { test_accessor_entry(r, &mut slot) }.is_null(),
                    "the shared runtime guard backend must validate both saved getter answers"
                );
                assert_eq!(
                    unsafe {
                        try_cached_class_read(r, &mut slot)
                            .or_else(|| try_cached_accessor(r, &mut slot))
                    }
                    .map(|v| v.as_number()),
                    Some(n)
                );
            });
        }
    }
    assert_eq!(
        read_accessor_stats().0,
        primes,
        "every alternating read was a memo hit"
    );
    assert_eq!(read_accessor_stats().1, 40);
    // A same-shape lane replacement must reject both saved pairs.
    holder.with_mut_ptr::<ObjectHeader, _>(|h| {
        crate::object::set_builtin_accessor_pair(
            h as usize,
            "answer".to_owned(),
            crate::object::accessor_pair::Accessor {
                raw_get: getter_eight as *const () as usize,
                ..Default::default()
            },
            crate::object::PropertyAttrs::new(true, false, true),
        );
    });
    for recv in [&a, &b] {
        recv.with_const_ptr::<ObjectHeader, _>(|r| {
            assert!(unsafe { test_accessor_entry(r, &mut slot) }.is_null());
            assert!(unsafe {
                try_cached_class_read(r, &mut slot).or_else(|| try_cached_accessor(r, &mut slot))
            }
            .is_none())
        });
    }
}

#[test]
fn ten_receiver_shapes_share_one_confirmed_absent_terminal() {
    if !crate::object::method_site::run_with_fresh_worker_gate(
        "ten_receiver_shapes_share_one_confirmed_absent_terminal",
    ) {
        return;
    }
    let _lock = crate::gc::global_side_table_test_lock();
    let base = crate::object::shapes::SHAPE_ID_BASE;
    // Unlike an unregistered stamp, these facts are live until retired.
    for shape in (base..base + 12).chain([base + 100, base + 101]) {
        let _ = shaped(shape);
    }
    let holder = Box::new(ObjectHeader {
        class_id: 0,
        parent_class_id: base + 100,
        meta: std::ptr::null_mut(),
    });
    let other = Box::new(ObjectHeader {
        class_id: 0,
        parent_class_id: base + 101,
        meta: std::ptr::null_mut(),
    });
    let mut cache = [0; crate::object::PIC_CACHE_WORDS];
    // The stack cache is not a process-lifetime PIC allocation. Skip
    // registration; this test exercises only the published words.
    cache[HOLDER_STATE] = STATE_REGISTERED;
    let mut recv = ObjectHeader {
        class_id: 0,
        parent_class_id: base,
        meta: std::ptr::null_mut(),
    };
    let absent = Walk {
        holder: (&*holder as *const ObjectHeader) as usize,
        holder_shape: base + 100,
        slot: None,
        hops: NO_HOPS,
        depth: 1,
        getter: 0,
    };
    for i in 0..11 {
        recv.parent_class_id = base + i;
        unsafe { publish(&mut cache, &recv, &absent, false) };
    }
    assert_ne!(cache[HOLDER_KIND] as u64 & HOLDER_MULTI_ABSENT, 0);
    assert_eq!(
        unsafe { entry_answer(&cache, (PIC_ID_TOKEN_BIT | u64::from(base)) as i64) },
        None,
        "the oldest of eleven shapes must leave a ten-shape site"
    );
    for i in 1..11 {
        let token = (PIC_ID_TOKEN_BIT | u64::from(base + i)) as i64;
        assert_eq!(
            unsafe { entry_answer(&cache, token) },
            Some(crate::value::TAG_UNDEFINED)
        );
    }
    // A new shape after an own-key shadow has no entry, while a terminal
    // mutation invalidates every receiver shape in the shared entry.
    assert_eq!(
        unsafe { entry_answer(&cache, (PIC_ID_TOKEN_BIT | u64::from(base + 11)) as i64) },
        None
    );
    let mut moved = Box::new(ObjectHeader {
        class_id: 0,
        parent_class_id: base + 100,
        meta: std::ptr::null_mut(),
    });
    cache[HOLDER_OBJ] = (&mut *moved as *mut ObjectHeader) as i64;
    assert_eq!(
        unsafe { entry_answer(&cache, (PIC_ID_TOKEN_BIT | u64::from(base + 10)) as i64) },
        Some(crate::value::TAG_UNDEFINED)
    );
    moved.parent_class_id = base + 102;
    assert_eq!(
        unsafe { entry_answer(&cache, (PIC_ID_TOKEN_BIT | u64::from(base + 10)) as i64) },
        None
    );
    // A different terminal never inherits the old entry's receiver set.
    let distinct = Walk {
        holder: (&*other as *const ObjectHeader) as usize,
        holder_shape: base + 101,
        ..absent
    };
    recv.parent_class_id = base + 11;
    unsafe { publish(&mut cache, &recv, &distinct, false) };
    assert_eq!(cache[HOLDER_KIND], HOLDER_ABSENT_DEPTH1);
    assert_eq!(
        unsafe { entry_answer(&cache, (PIC_ID_TOKEN_BIT | u64::from(base + 10)) as i64) },
        None
    );
    assert_eq!(
        unsafe { entry_answer(&cache, (PIC_ID_TOKEN_BIT | u64::from(base + 11)) as i64) },
        Some(crate::value::TAG_UNDEFINED)
    );
    WORKER_AGENTS_EXIST.store(1, Ordering::SeqCst);
    assert_eq!(
        unsafe { entry_answer(&cache, (PIC_ID_TOKEN_BIT | u64::from(base + 11)) as i64) },
        None
    );
}
