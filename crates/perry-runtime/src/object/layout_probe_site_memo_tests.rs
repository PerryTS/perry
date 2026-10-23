use super::*;

fn atom(name: &[u8]) -> *const crate::StringHeader {
    crate::string::js_string_pool_atom(
        name.as_ptr(),
        name.len() as u32,
        unsafe { crate::object::key_bytes_hash(name.as_ptr(), name.len()) },
        0,
    ) as *const _
}
unsafe fn plain(name: &[u8], value: f64) -> *mut ObjectHeader {
    let obj = crate::object::js_object_alloc(0, 4);
    crate::object::js_object_set_field_by_name(obj, atom(name), value);
    obj
}
unsafe fn inherited(proto: *mut ObjectHeader) -> *mut ObjectHeader {
    let value = crate::object::js_object_create(crate::value::js_nanbox_pointer(proto as i64));
    (value.to_bits() & POINTER_MASK) as *mut ObjectHeader
}
unsafe fn data(answer: Option<Answer>) -> u64 {
    match answer {
        Some(Answer::Data(bits)) => bits,
        _ => panic!("expected memo data hit"),
    }
}
unsafe fn raw_getter(answer: Option<Answer>) -> usize {
    match answer {
        Some(Answer::Getter(pair)) => {
            crate::object::accessor_pair::pair_of_value(pair)
                .unwrap()
                .raw_get
        }
        _ => panic!("expected memo accessor hit"),
    }
}

#[test]
fn two_level_hits_load_live_data_and_fail_on_shadowing() {
    let _lock = crate::gc::global_side_table_test_lock();
    let _no_move = crate::gc::GcSuppressScope::new();
    unsafe {
        let dir = crate::object::shapes::ordinary_dir_addr();
        let key = atom(b"value");
        let hash = crate::string::property_atom_hash(key);
        let top = plain(b"value", 7.0);
        let mid = inherited(top);
        let recv = inherited(mid);
        let mut memo = ReadSiteMemo::EMPTY;
        assert_eq!(
            data(prime(dir, recv, STRING_TAG | key as u64, hash, &mut memo)),
            7.0f64.to_bits()
        );
        assert_eq!(
            data(lookup(dir, recv, &memo)),
            7.0f64.to_bits(),
            "the warmed path must actually hit"
        );
        crate::object::js_object_set_field_by_name(top, key, 8.0);
        assert_eq!(
            data(lookup(dir, recv, &memo)),
            8.0f64.to_bits(),
            "values are not cached"
        );
        crate::object::js_object_set_field_by_name(mid, key, 99.0);
        assert!(
            lookup(dir, recv, &memo).is_none(),
            "an intermediate own key invalidates its shape proof"
        );
    }
}

#[test]
fn accessor_hits_reload_the_holder_pair() {
    let _lock = crate::gc::global_side_table_test_lock();
    let _no_move = crate::gc::GcSuppressScope::new();
    unsafe extern "C" fn first(_: f64) -> f64 {
        1.0
    }
    unsafe extern "C" fn second(_: f64) -> f64 {
        2.0
    }
    unsafe {
        let dir = crate::object::shapes::ordinary_dir_addr();
        let proto = crate::object::js_object_alloc(0, 4);
        let pair = crate::object::accessor_pair::Accessor {
            raw_get: first as *const () as usize,
            ..Default::default()
        };
        crate::object::descriptor_state::set_builtin_accessor_pair_bytes(
            proto as usize,
            b"value",
            pair,
            crate::object::PropertyAttrs::new(true, true, true),
        );
        let recv = inherited(proto);
        let key = atom(b"value");
        let mut memo = ReadSiteMemo::EMPTY;
        let found = prime(
            dir,
            recv,
            STRING_TAG | key as u64,
            crate::string::property_atom_hash(key),
            &mut memo,
        );
        assert_eq!(raw_getter(found), first as *const () as usize);
        assert_eq!(
            raw_getter(lookup(dir, recv, &memo)),
            first as *const () as usize
        );
        // Mutate the live pair through the holder's slot without touching
        // the memo. A memo of a callable would incorrectly retain first.
        let record = record_of(dir, (*proto).parent_class_id);
        let (pos, _) = own_entry(
            record,
            STRING_TAG | key as u64,
            crate::string::property_atom_hash(key),
        )
        .unwrap();
        let keys = (*record).keys as usize as *const crate::array::ArrayHeader;
        let slot = crate::object::key_attrs::key_storage_slot(keys, pos);
        let bits = *((proto as *const u8)
            .add(std::mem::size_of::<ObjectHeader>() + slot as usize * 8)
            as *const u64);
        let array = (bits & POINTER_MASK) as *mut crate::array::ArrayHeader;
        *(crate::array::array_elements_ptr(array) as *mut u64)
            .add(crate::object::accessor_pair::PAIR_RAW_GET) = second as *const () as u64;
        assert_eq!(
            raw_getter(lookup(dir, recv, &memo)),
            second as *const () as usize
        );
    }
}

#[test]
fn a_different_receiver_with_the_same_shape_reads_its_own_slot() {
    let _lock = crate::gc::global_side_table_test_lock();
    let _no_move = crate::gc::GcSuppressScope::new();
    unsafe {
        let dir = crate::object::shapes::ordinary_dir_addr();
        let one = plain(b"value", 1.0);
        let two = plain(b"value", 2.0);
        assert_eq!((*one).parent_class_id, (*two).parent_class_id);
        let key = atom(b"value");
        let mut memo = ReadSiteMemo::EMPTY;
        prime(
            dir,
            one,
            STRING_TAG | key as u64,
            crate::string::property_atom_hash(key),
            &mut memo,
        )
        .unwrap();
        assert_eq!(data(lookup(dir, two, &memo)), 2.0f64.to_bits());
        assert_eq!(
            std::mem::size_of::<Answer>(),
            16,
            "data hits must not use an Accessor-sized return ABI"
        );
        assert_eq!(std::mem::size_of::<ReadSiteMemo>(), 40);
        assert_eq!(std::mem::size_of::<KeyedSiteMemo>(), 192);
    }
}

#[test]
fn rust_runtime_sites_share_the_memo_and_preserve_getter_this() {
    let _lock = crate::gc::global_side_table_test_lock();
    let _no_move = crate::gc::GcSuppressScope::new();
    unsafe extern "C-unwind" fn receiver(this: f64) -> f64 {
        this
    }
    unsafe {
        let proto = crate::object::js_object_alloc(0, 4);
        let pair = crate::object::accessor_pair::Accessor {
            raw_get: receiver as *const () as usize,
            ..Default::default()
        };
        crate::object::descriptor_state::set_builtin_accessor_pair_bytes(
            proto as usize,
            b"value",
            pair,
            crate::object::PropertyAttrs::new(true, true, true),
        );
        let recv = inherited(proto);
        let key = crate::runtime_state_key!(b"value");
        let expected = crate::value::js_nanbox_pointer(recv as i64).to_bits();
        assert_eq!(key.read_object(recv).to_bits(), expected);
        let memo = key.memo.unwrap()();
        assert_eq!(
            raw_getter(lookup(
                crate::object::shapes::ordinary_dir_addr(),
                recv,
                &*memo
            )),
            receiver as *const () as usize
        );
        assert_eq!(
            key.read_object(recv).to_bits(),
            expected,
            "a warmed Rust site calls with the original receiver"
        );
    }
}

#[test]
fn leaf_answers_never_touch_the_site_memo() {
    let _lock = crate::gc::global_side_table_test_lock();
    let _no_move = crate::gc::GcSuppressScope::new();
    unsafe {
        let own = plain(b"value", 1.0);
        let recv = inherited(plain(b"value", 2.0));
        let key = crate::runtime_state_key!(b"value");
        assert_eq!(key.read_object(own), 1.0);
        assert_eq!(key.read_object(recv), 2.0);
        let memo = key.memo.unwrap()();
        assert_eq!(
            (*memo).pairs,
            [0; PAIRS],
            "own and inherited data are stateless probe answers"
        );
        // The control: a declined read (an inherited getter) does derive.
        unsafe extern "C-unwind" fn seven(_: f64) -> f64 {
            7.0
        }
        let proto = crate::object::js_object_alloc(0, 4);
        crate::object::descriptor_state::set_builtin_accessor_pair_bytes(
            proto as usize,
            b"value",
            crate::object::accessor_pair::Accessor {
                raw_get: seven as *const () as usize,
                ..Default::default()
            },
            crate::object::PropertyAttrs::new(true, true, true),
        );
        let getter_recv = inherited(proto);
        assert_eq!(key.read_object(getter_recv), 7.0);
        assert_ne!(
            (*memo).pairs[0],
            0,
            "the collecting entrance derives the memo"
        );
    }
}

#[test]
fn an_own_hole_reads_live_presence_before_using_its_tail() {
    let _lock = crate::gc::global_side_table_test_lock();
    let _no_move = crate::gc::GcSuppressScope::new();
    unsafe {
        let dir = crate::object::shapes::ordinary_dir_addr();
        let key = atom(b"error");
        let word = STRING_TAG | key as u64;
        let hash = crate::string::property_atom_hash(key);
        let proto = plain(b"error", 7.0);
        let recv = crate::object::layout_desc::perry_alloc_layout(std::ptr::addr_of!(
            crate::object::runtime_layouts::SPAWN_SYNC_RESULT
        ));
        crate::object::prototype_chain::object_link_created_prototype(
            recv as usize,
            crate::value::js_nanbox_pointer(proto as i64).to_bits(),
        );
        let shape = (*recv).parent_class_id;
        let record = record_of(dir, shape);
        let (pos, _) = own_entry(record, word, hash).unwrap();
        let keys = (*record).keys as usize as *const crate::array::ArrayHeader;
        assert_ne!(
            crate::object::key_attrs::keys_entry(keys, pos)
                & crate::object::key_attrs::ENTRY_OPTIONAL,
            0,
            "the fixture must permit live presence without a layout change"
        );
        let slot = crate::object::key_attrs::key_storage_slot(keys, pos);
        assert!(slot < (*record).live_inline_slot_count);
        let field = (recv as *mut u8).add(std::mem::size_of::<ObjectHeader>() + slot as usize * 8)
            as *mut u64;
        assert_eq!(*field, TAG_HOLE);
        assert_eq!(
            perry_probe_get(
                dir,
                (recv as usize - perry_abi::RECEIVER_HANDLE_FLOOR) as i64,
                word,
                hash
            ),
            7.0,
            "the fixture must have ordinary inherited Get semantics"
        );
        let mut memo = ReadSiteMemo::EMPTY;
        assert!(
            ordinary_word(recv).is_some(),
            "the fresh fixture must be memo-admissible"
        );
        let primed = prime(dir, recv, word, hash, &mut memo);
        assert!(
            primed.is_some(),
            "the optional slot must retain its prototype tail"
        );
        assert_eq!(data(primed), 7.0f64.to_bits());
        assert_eq!(data(lookup(dir, recv, &memo)), 7.0f64.to_bits());
        // Model an emitted optional-slot store: the slot itself owns its
        // live presence. Both test words are pointer-free scalars.
        // GC_STORE_AUDIT(POINTER_FREE): scalar in a test's optional slot.
        *field = 11.0f64.to_bits();
        assert_eq!((*recv).parent_class_id, shape);
        assert_eq!(
            data(lookup(dir, recv, &memo)),
            11.0f64.to_bits(),
            "an own value shadows the remembered tail"
        );
        // GC_STORE_AUDIT(POINTER_FREE): hole in a test's optional slot.
        *field = TAG_HOLE;
        crate::object::js_object_set_field_by_name(proto, key, 8.0);
        assert_eq!(data(lookup(dir, recv, &memo)), 8.0f64.to_bits());
    }
}

#[test]
fn a_decline_is_a_shape_checked_answer_that_never_displaces_a_proof() {
    let _lock = crate::gc::global_side_table_test_lock();
    let _no_move = crate::gc::GcSuppressScope::new();
    unsafe {
        let dir = crate::object::shapes::ordinary_dir_addr();
        let key = atom(b"value");
        let word = STRING_TAG | key as u64;
        let hash = crate::string::property_atom_hash(key);
        let a = plain(b"value", 1.0);
        let b = plain(b"other", 2.0);
        crate::object::js_object_set_field_by_name(b, atom(b"more"), 3.0);
        assert_ne!((*a).parent_class_id, (*b).parent_class_id);
        let mut memo = ReadSiteMemo::EMPTY;
        decline(b, &mut memo);
        assert!(declined(b, &memo));
        assert!(!declined(a, &memo), "the decline names b's shape only");
        assert!(lookup(dir, b, &memo).is_none());
        assert!(
            collect(dir, b, word, hash, &mut memo).is_none(),
            "a declined shape is not derived again"
        );
        assert!(declined(b, &memo));
        // A receiver of another shape still derives, replacing the decline.
        assert_eq!(
            data(collect(dir, a, word, hash, &mut memo)),
            1.0f64.to_bits()
        );
        let proof = memo;
        decline(b, &mut memo);
        assert_eq!(memo.pairs, proof.pairs, "a decline never displaces a proof");
        assert_eq!(data(lookup(dir, a, &memo)), 1.0f64.to_bits());
    }
}
