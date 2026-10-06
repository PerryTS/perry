use super::*;

#[repr(C)]
struct Holder {
    header: ObjectHeader,
    lane: u64,
}

#[test]
fn shared_runtime_guards_reject_changed_facts_and_workers() {
    if !crate::object::method_site::run_with_fresh_worker_gate(
        "shared_runtime_guards_reject_changed_facts_and_workers",
    ) {
        return;
    }
    let _lock = crate::gc::global_side_table_test_lock();
    let shape = crate::object::shapes::SHAPE_ID_BASE + 22;
    let pair = 0x120040;
    let mut holder = Holder {
        header: ObjectHeader {
            class_id: 0,
            parent_class_id: shape,
            meta: std::ptr::null_mut(),
        },
        lane: crate::value::POINTER_TAG | pair,
    };
    let token = (u64::from(shape) | PIC_ID_TOKEN_BIT) as i64;
    let mut words = HolderEntry([0; HOLDER_STATE - HOLDER_RECV]);
    words[HOLDER_RECV] = token;
    let holder_ptr = std::ptr::addr_of_mut!(holder);
    words[HOLDER_OBJ] = holder_ptr as i64;
    words[HOLDER_SHAPE] = i64::from(shape);
    words[HOLDER_KIND] = HOLDER_ACCESSOR as i64;
    words[HOLDER_HOPS] = pair as i64;
    words[HOLDER_HOP_SHAPES] = 8; // examined, never called in this test
    unsafe {
        assert_eq!(
            validated_accessor::<true>(&words, token),
            Some((8, pair as usize))
        );
        assert!(validated_accessor::<true>(&words, 0).is_none());
        assert!(validated_accessor::<true>(&words, token + 1).is_none());
        (*holder_ptr).header.parent_class_id += 1;
        assert!(validated_accessor::<true>(&words, token).is_none());
        (*holder_ptr).header.parent_class_id = shape;
        (*holder_ptr).lane ^= 1;
        assert!(validated_accessor::<true>(&words, token).is_none());
        (*holder_ptr).lane ^= 1;
        words[HOLDER_KIND] = 0;
        assert!(validated_accessor::<false>(&words, token).is_none());
        words[HOLDER_KIND] = HOLDER_ACCESSOR as i64;
        words[HOLDER_HOP_SHAPES] = 0;
        assert!(validated_accessor::<true>(&words, token).is_none());
        assert_eq!(
            validated_accessor::<false>(&words, token),
            Some((0, pair as usize))
        );
        // No holder load may occur after worker exclusion fails.
        words[HOLDER_OBJ] = 1;
        WORKER_AGENTS_EXIST.store(1, Ordering::SeqCst);
        let declined = validated_accessor::<false>(&words, token).is_none();
        WORKER_AGENTS_EXIST.store(0, Ordering::SeqCst);
        assert!(declined);
    }
}

#[test]
fn inline_getter_word_proves_the_lane_is_not_spill() {
    if !crate::object::method_site::run_with_fresh_worker_gate(
        "inline_getter_word_proves_the_lane_is_not_spill",
    ) {
        return;
    }
    let _lock = crate::gc::global_side_table_test_lock();
    let recv = ObjectHeader {
        class_id: 0,
        parent_class_id: crate::object::shapes::SHAPE_ID_BASE + 1,
        meta: std::ptr::null_mut(),
    };
    // Publication also records native probe code from the immutable pair.
    let _no_move = crate::gc::GcSuppressScope::new();
    let pair = unsafe { crate::object::accessor_pair::pair_new(Default::default()) };
    let mut hops = NO_HOPS;
    hops[0] = (pair as usize, 0);
    for (slot, getter) in [(HOLDER_SLOT_SPILL, 8), (0, 0), (0, 8)] {
        let w = Walk {
            holder: 0x120000,
            holder_shape: crate::object::shapes::SHAPE_ID_BASE + 2,
            slot: Some(slot),
            hops,
            depth: 1,
            getter,
        };
        let mut cache = [0; crate::object::PIC_CACHE_WORDS];
        cache[HOLDER_STATE] = STATE_REGISTERED; // stack cache never enters the root list
        unsafe { publish(&mut cache, &recv, &w, true) };
        let expected = if slot & HOLDER_SLOT_SPILL != 0 {
            0
        } else {
            getter
        };
        assert_eq!(cache[HOLDER_HOP_SHAPES] as usize, expected);
    }
}
