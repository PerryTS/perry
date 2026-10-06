use super::*;

extern "C" fn getter_two(_this: f64) -> f64 {
    2.0
}
extern "C" fn getter_eight(_this: f64) -> f64 {
    8.0
}

/// The entry an accessor prime publishes for `acc`.
fn accessor_entry(acc: &HolderAccessor) -> Walk {
    let mut hops = NO_HOPS;
    hops[0].0 = acc.pair;
    Walk {
        holder: acc.holder,
        holder_shape: acc.shape,
        slot: Some(acc.slot),
        hops,
        depth: 1,
        getter: acc.getter,
    }
}

#[test]
fn inherited_accessor_rechecks_intermediate_shapes() {
    if !crate::object::method_site::run_with_fresh_worker_gate(
        "inherited_accessor_rechecks_intermediate_shapes",
    ) {
        return;
    }
    let _lock = crate::gc::global_side_table_test_lock();
    let _no_move = crate::gc::GcSuppressScope::new();
    unsafe {
        let holder = crate::object::js_object_alloc(0, 0);
        let middle = crate::object::js_object_alloc(0x0C3C_79A6, 0);
        let receiver = crate::object::js_object_alloc(0, 0);
        let value = |p| crate::value::js_nanbox_pointer(p as i64);
        crate::object::js_object_set_prototype_of(
            value(holder),
            f64::from_bits(crate::value::TAG_NULL),
        );
        crate::object::js_object_set_prototype_of(value(middle), value(holder));
        crate::object::js_object_set_prototype_of(value(receiver), value(middle));
        assert!(
            shape_proto_id(object_shape_stamp(middle))
                .is_some_and(|pid| (PROTO_ID_MIXED..PROTO_ID_UNIQUE).contains(&pid)),
            "the intermediate class prototype must exercise a MIXED link"
        );
        let install = |obj: *mut ObjectHeader, raw_get| {
            crate::object::set_builtin_accessor_pair(
                obj as usize,
                "path".to_string(),
                crate::object::accessor_pair::Accessor {
                    raw_get,
                    ..Default::default()
                },
                crate::object::PropertyAttrs::new(true, false, true),
            );
        };
        install(holder, getter_two as *const () as usize);
        let key = crate::string::js_string_from_bytes(b"path".as_ptr(), 4);
        let mut slot = std::ptr::null_mut();
        assert_eq!(
            prime_read_holder(receiver, key, &mut slot).map(|v| v.as_number()),
            Some(2.0)
        );
        assert!(!slot.is_null(), "the inherited accessor must prime a site");
        assert_ne!((*slot)[HOLDER_KIND] as u64 & HOLDER_ACCESSOR_DEEP, 0);
        assert_eq!(
            try_cached_accessor(receiver, &mut slot).map(|v| v.as_number()),
            Some(2.0)
        );
        install(middle, getter_eight as *const () as usize);
        assert!(
            try_cached_accessor(receiver, &mut slot).is_none(),
            "a nearer accessor invalidates the deep entry"
        );
        assert_eq!(
            prime_read_holder(receiver, key, &mut slot).map(|v| v.as_number()),
            Some(8.0)
        );
        assert_eq!(
            try_cached_accessor(receiver, &mut slot).map(|v| v.as_number()),
            Some(8.0)
        );
    }
}

/// Two holders with exactly one ShapeId but different compiled getters.
/// Replacing a declared class's registry pointer leaves the receiver's
/// bare CLASS ShapeId unchanged. The replacement must retire the old
/// holder's ShapeId, so the hit's holder compare refuses the stale entry
/// with no global word to consult.
#[test]
fn class_accessor_rechecks_same_shape_holder_link() {
    if !crate::object::method_site::run_with_fresh_worker_gate(
        "class_accessor_rechecks_same_shape_holder_link",
    ) {
        return;
    }
    let _lock = crate::gc::global_side_table_test_lock();
    const CID: u32 = 0x0C3C_79A3;
    let scope = crate::gc::RuntimeHandleScope::new();
    let p1 = scope.root_raw_mut_ptr(crate::object::js_object_alloc(0, 2));
    let p2 = scope.root_raw_mut_ptr(crate::object::js_object_alloc(0, 2));
    for (holder, raw_get) in [
        (&p1, getter_two as *const () as usize),
        (&p2, getter_eight as *const () as usize),
    ] {
        holder.with_mut_ptr::<ObjectHeader, _>(|ptr| {
            crate::object::set_builtin_accessor_pair(
                ptr as usize,
                "path".to_owned(),
                crate::object::accessor_pair::Accessor {
                    raw_get,
                    ..Default::default()
                },
                crate::object::PropertyAttrs::new(true, false, true),
            );
        });
    }
    p1.with_const_ptr::<ObjectHeader, _>(|first| {
        p2.with_const_ptr::<ObjectHeader, _>(|second| {
            assert_ne!(first, second);
            assert_eq!(unsafe { object_shape_stamp(first) }, unsafe {
                object_shape_stamp(second)
            });
        });
    });

    let packed = b"holder_class_key";
    let keys =
        crate::object::js_build_class_keys_array(CID, 1, packed.as_ptr(), packed.len() as u32, 0);
    let recv_shape =
        crate::object::shapes::js_object_shape_id_for_class_keys(keys as usize as u64, 1, CID, 0);
    let recv =
        crate::object::js_object_alloc_class_inline_keys_stamped(CID, 0, 1, keys, recv_shape, 0);
    let recv = scope.root_raw_mut_ptr(recv);
    recv.with_const_ptr::<ObjectHeader, _>(|receiver| {
        assert_eq!(
            unsafe { shape_proto_id(object_shape_stamp(receiver)) },
            Some(PROTO_ID_CLASS | u64::from(CID))
        );

        p1.with_const_ptr::<ObjectHeader, _>(|ptr| {
            crate::object::test_seed_class_decl_prototype_object_root(CID, ptr as usize);
        });
        let first = unsafe { accessor_walk(receiver, b"path") }.expect("first accessor");
        let cache: &'static mut PicCache =
            Box::leak(Box::new([0; crate::codegen_abi::PIC_CACHE_WORDS]));
        let mut slot: PicCacheSlot = cache;
        unsafe { publish(cache, receiver, &accessor_entry(&first), true) };
        assert_eq!(
            cache[HOLDER_HOP_SHAPES] as usize,
            getter_two as *const () as usize
        );
        assert_eq!(cache[HOLDER_HOPS] as usize, first.pair);
        assert_eq!(
            unsafe { try_cached_accessor(receiver, &mut slot) }.map(|v| v.as_number()),
            Some(2.0)
        );

        let old_relinks = read_accessor_same_shape_relinks();
        p2.with_const_ptr::<ObjectHeader, _>(|ptr| {
            crate::object::test_seed_class_decl_prototype_object_root(CID, ptr as usize);
        });
        assert_eq!(unsafe { object_shape_stamp(receiver) }, recv_shape);
        // The relink is seen through the old holder's ShapeId alone.
        p1.with_const_ptr::<ObjectHeader, _>(|old| {
            assert_ne!(unsafe { object_shape_stamp(old) }, first.shape);
            assert_ne!(unsafe { object_shape_stamp(old) }, 0);
        });
        assert!(
            unsafe { try_cached_accessor(receiver, &mut slot) }.is_none(),
            "stale getter was served after registry replacement"
        );
        let second = unsafe { accessor_walk(receiver, b"path") }.expect("second accessor");
        unsafe { publish(cache, receiver, &accessor_entry(&second), true) };
        assert!(read_accessor_same_shape_relinks() > old_relinks);
        assert_eq!(
            unsafe { try_cached_accessor(receiver, &mut slot) }.map(|v| v.as_number()),
            Some(8.0)
        );
        // A getter replacement may retain a compiled setter. Neither a
        // new prime nor an existing hit may mistake it for setter-only.
        // Keep the deliberate same-shape slot replacement noncollecting.
        let _no_gc = crate::gc::GcSuppressScope::new();
        extern "C" fn closure_getter(
            _closure: *const crate::closure::ClosureHeader,
            _this: crate::closure::JsThis,
        ) -> f64 {
            9.0
        }
        let closure = crate::closure::js_closure_alloc(crate::fn_info!(closure_getter, 0), 0);
        let pair = unsafe {
            crate::object::accessor_pair::pair_new(crate::object::accessor_pair::Accessor {
                get: crate::value::js_nanbox_pointer(closure as i64).to_bits(),
                raw_set: getter_eight as *const () as usize,
                ..Default::default()
            })
        };
        p2.with_mut_ptr::<ObjectHeader, _>(|holder| unsafe {
            crate::object::slot_store::store_object_field_slot(
                holder,
                second.slot as usize,
                crate::value::js_nanbox_pointer(pair as i64).to_bits(),
            );
            assert_eq!(object_shape_stamp(holder), second.shape);
        });
        // The replaced lane misses the primed pair, with no ShapeId
        // transition to see it by.
        assert!(unsafe { try_cached_accessor(receiver, &mut slot) }.is_none());
        // A function-object getter re-primes with the closure-ABI word
        // and is called with the receiver as `this`.
        let third = unsafe { accessor_walk(receiver, b"path") }.expect("closure accessor");
        assert_eq!(
            third.getter,
            crate::object::accessor_pair::holder_closure_getter_entry()
        );
        assert_eq!(third.pair, pair as usize);
        unsafe { publish(cache, receiver, &accessor_entry(&third), true) };
        assert_eq!(
            unsafe { try_cached_accessor(receiver, &mut slot) }.map(|v| v.as_number()),
            Some(9.0)
        );
    });
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

/// A class instance has a valid, stamped ShapeId, but its prototype is
/// resolved through the class vtable. The holder walk must refuse it even
/// when the shape and the object's current prototype id agree.
#[test]
fn class_prototype_identity_is_refused_by_read_holder() {
    let _lock = crate::gc::global_side_table_test_lock();
    const CID: u32 = 0x0C3C_79A2;
    let packed = b"holder_class_key";
    let keys =
        crate::object::js_build_class_keys_array(CID, 1, packed.as_ptr(), packed.len() as u32, 0);
    let shape_id =
        crate::object::shapes::js_object_shape_id_for_class_keys(keys as usize as u64, 1, CID, 0);
    let obj =
        crate::object::js_object_alloc_class_inline_keys_stamped(CID, 0, 1, keys, shape_id, 0);
    let claimed = shape_proto_id(shape_id).expect("class shape must be stamped");
    assert_eq!(claimed, crate::object::shapes::class_proto_id(CID));
    assert_eq!(
        unsafe { crate::object::shapes::object_proto_id(obj) },
        claimed
    );
    assert!(claimed >= crate::object::shapes::PROTO_ID_CLASS);
    assert_eq!(unsafe { admitted_proto_id(obj) }, None);
}
