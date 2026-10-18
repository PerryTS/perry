use super::*;

#[test]
fn static_property_lookups_do_not_mint_builtin_class_values() {
    for cid in [0, 0x7FFF_FF00, 0xFFFF_0024, u32::MAX] {
        assert!(class_static_get(cid, "from").is_none());
        assert!(class_static_own_accessor(cid, "from").is_none());
        assert!(crate::object::class_registry::class_static_prototype(cid).is_null());
        assert!(!crate::object::class_registry::class_static_prototype_is_nulled(cid));
        assert!(class_value_cached(cid).is_none());
    }
}

fn register(cid: u32) {
    let mut guard = crate::object::REGISTERED_CLASS_IDS.write().unwrap();
    guard
        .get_or_insert_with(crate::fast_hash::new_ptr_hash_set)
        .insert(cid);
}

/// #11414: a class value is ONE function object per class — never an
/// INT32 word a number can equal — pinned and old, so its address is its
/// identity across collections.
#[test]
fn class_value_is_one_pinned_function_object_per_class() {
    let cid = 0x6A01;
    register(cid);
    let a = class_value(cid);
    let b = class_value(cid);
    assert_eq!(a.to_bits(), b.to_bits(), "one function object per class");
    let v = crate::value::JSValue::from_bits(a.to_bits());
    assert!(
        !v.is_int32() && !v.is_number(),
        "a class value is not a number"
    );
    assert!(v.is_pointer());
    let ptr = (a.to_bits() & crate::value::POINTER_MASK) as usize;
    assert!(
        crate::closure::is_closure_ptr(ptr),
        "a GC_TYPE_CLOSURE cell"
    );
    assert_eq!(class_value_id(a), Some(cid));
    assert_eq!(class_closure_id(ptr), Some(cid));
    // The number equal to the class id is not the class.
    assert_ne!(
        f64::from_bits(crate::value::INT32_TAG | cid as u64).to_bits(),
        a.to_bits()
    );
    let header = unsafe { crate::value::addr_class::try_read_gc_header(ptr) }.expect("header");
    assert_ne!(header.gc_flags & crate::gc::GC_FLAG_PINNED, 0, "pinned");
    assert_ne!(header.gc_flags & crate::gc::GC_FLAG_TENURED, 0, "born old");
    crate::gc::js_gc_collect();
    assert_eq!(class_value(cid).to_bits(), a.to_bits(), "never moves");
    assert_eq!(class_value_id(a), Some(cid), "survives a full collection");
    let other = class_value(0x6A02);
    assert_ne!(other.to_bits(), a.to_bits());
    assert_eq!(class_value_id(other), Some(0x6A02));
}

/// The class-value table is sized by how many ids of a band exist, never
/// by an id's magnitude: one class object for the highest id of every
/// band (fs.Stats' reserved `0xFFFF_0070` among them, which once grew the
/// directory to 16M pages — 128 MiB zero-filled — on a single
/// `stats.mtime` read) leaves every directory at most 256 pages.
#[test]
fn class_value_directories_are_sized_by_band_offset_not_by_id() {
    let ids = [
        0x7FFF_FF00,
        0x7FFF_FFFF,
        SYNTHETIC_CLASS_ID_BASE,
        0xFFFF_0070, // fs.Stats (`fs::stats::STATS_REGULAR_CLASS_ID`)
        0xFFFF_0071, // bigint fs.Stats
        crate::native_class_ids::CRYPTO_HASH,
        u32::MAX,
    ];
    // Not registered: a registered id is also an INT32 class-ref word
    // (#11414), and `u32::MAX` would turn every `int32(-1)` into a class.
    for cid in ids {
        let v = class_value(cid);
        assert_eq!(class_value_id(v), Some(cid), "{cid:#x} round-trips");
        assert_eq!(class_value(cid).to_bits(), v.to_bits(), "one object per id");
    }
    for (band, (_, len)) in class_value_dirs().into_iter().enumerate() {
        assert!(
            len <= CLASS_VALUE_PAGE_LEN,
            "band {band}: directory of {len} pages (sized by a raw class id)"
        );
    }
    // Distinct ids in distinct bands never share a slot.
    let objs: Vec<u64> = ids.iter().map(|&c| class_value(c).to_bits()).collect();
    let mut unique = objs.clone();
    unique.sort_unstable();
    unique.dedup();
    assert_eq!(unique.len(), objs.len(), "every band id has its own object");
    // Non-class words have no slot and are never minted.
    assert_eq!(class_value_band(0), None);
    assert_eq!(class_value_band(0x8000_0000), None);
    assert!(class_value_cached(0x8000_0000).is_none());
}

/// A static walk that reaches a BUILTIN parent (`class E extends Error`)
/// stops there: a builtin id has no class function object, so no reader
/// may mint one for it (minting `0xFFFF_0001` grew the class-value
/// directory to 16M pages, which every collection then scanned).
#[test]
fn a_builtin_parent_never_gets_a_class_function_object() {
    let cid = 0x6D71;
    register(cid);
    crate::object::js_register_class_parent(cid, crate::error::CLASS_ID_ERROR);
    let recv = class_value(cid);
    let pages_before = class_value_dirs().map(|(_, len)| len);
    let applied = unsafe {
        crate::object::class_registry::class_static_accessor_setter_apply(cid, "zz", recv, 1.0)
    };
    assert!(!applied, "no static accessor named zz on the chain");
    assert!(!crate::object::class_registry::static_accessor_in_chain(
        cid, "zz"
    ));
    // A static symbol read up the chain (latch armed, as once any class
    // has a static symbol member) reaches Error too and must not mint.
    crate::symbol::CLASS_STATIC_SYMBOLS_LATCH.arm();
    let sym = unsafe { crate::symbol::js_symbol_new_empty() };
    assert!(crate::symbol::class_static_symbol_lookup_in_chain(cid, sym).is_none());
    assert!(
        crate::symbol::class_static_symbol_keys_for_class(crate::error::CLASS_ID_ERROR).is_empty()
    );
    // Object.getPrototypeOf(C), the static `super` parent value and a
    // static `super[k] = v` all step to the parent id; none may mint.
    let _ = crate::object::js_object_get_prototype_of(recv);
    let _ = crate::object::class_registry::parent_static::template_dynamic_parent_value(cid);
    let _ = crate::proxy::js_super_put_value_set(
        crate::error::CLASS_ID_ERROR,
        crate::value::js_nanbox_string(
            crate::string::js_string_from_bytes(b"zz".as_ptr(), 2) as i64
        ),
        1.0,
        recv,
        0,
    );
    assert!(
        class_value_cached(crate::error::CLASS_ID_ERROR).is_none(),
        "the builtin Error id must not get a class function object"
    );
    let pages_after = class_value_dirs().map(|(_, len)| len);
    assert_eq!(
        pages_after, pages_before,
        "the walks grew the class-value directory"
    );
}

/// A class function object is traced like any other pinned object: a full
/// collection reaches its own-property bag (the statics) through the
/// pinned-object root scan, with no class-value scanner.
#[test]
fn a_full_collection_keeps_the_class_statics_bag() {
    let cid = 0x6B02;
    register(cid);
    // A unit-test thread may not have run `gc_init`'s scanner list.
    crate::gc::gc_register_mutable_root_scanner(crate::gc::scan_pinned_object_roots_mut);
    let text = "static-payload-11609";
    let s = crate::string::js_string_from_bytes(text.as_ptr(), text.len() as u32);
    class_static_set(
        cid,
        "k11609",
        f64::from_bits(crate::value::JSValue::string_ptr(s).bits()),
    );
    crate::gc::js_gc_collect();
    crate::gc::js_gc_collect();
    let got = class_static_get(cid, "k11609").expect("the static survives a full collection");
    let got = crate::value::JSValue::from_bits(got.to_bits());
    let hdr = got.as_string_ptr();
    assert!(!hdr.is_null());
    let bytes = unsafe { crate::string::OwnedStringBytes::copy_from_header(hdr) };
    assert_eq!(bytes.as_bytes(), text.as_bytes());
    let keys: Vec<String> = class_static_entries(cid)
        .into_iter()
        .map(|(k, _)| k)
        .collect();
    assert!(keys.iter().any(|k| k == "k11609"), "own keys: {keys:?}");
}

/// The prototype link (capture 2) and the statics bag of an OLD, pinned
/// class hold young objects written after its birth: the slot barrier
/// remembers them, so a moving minor keeps them alive and rewrites both
/// edges. Dropping the barrier in `class_decl_prototype_link_store` makes
/// the link read back a stale address.
#[test]
fn young_values_in_an_old_class_survive_moving_minors() {
    let _copying_nursery = crate::gc::CopyingNurseryTestGuard::new(0);
    let _triggers = crate::gc::GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    let _force_evacuation = crate::gc::knob_overrides::ForcedEvacuationTestGuard::on();
    crate::gc::register_runtime_handle_root_scanner_for_tests();
    let cid = 0x6B04;
    register(cid);
    let _ = class_value_ptr(cid);
    // The class is old and pinned from birth; everything below is young.
    let proto = crate::object::js_object_alloc(0x6B05, 1);
    crate::object::js_object_set_field(
        proto,
        0,
        crate::value::JSValue::from_bits(crate::value::INT32_TAG | 4242),
    );
    let before = proto as usize;
    class_decl_prototype_link_store(cid, proto);
    let text = "young-static-6b04";
    let s = crate::string::js_string_from_bytes(text.as_ptr(), text.len() as u32);
    class_static_set(
        cid,
        "k6b04",
        f64::from_bits(crate::value::JSValue::string_ptr(s).bits()),
    );
    for _ in 0..3 {
        let _ = crate::gc::gc_collect_minor();
    }
    let linked = class_decl_prototype_link(cid);
    assert!(!linked.is_null());
    assert_ne!(
        linked as usize, before,
        "forced evacuation must have moved the young prototype"
    );
    let field = crate::object::js_object_get_field(linked, 0);
    assert_eq!(field.bits(), crate::value::INT32_TAG | 4242);
    let got = class_static_get(cid, "k6b04").expect("the static survives minors");
    let hdr = crate::value::JSValue::from_bits(got.to_bits()).as_string_ptr();
    assert!(!hdr.is_null());
    let bytes = unsafe { crate::string::OwnedStringBytes::copy_from_header(hdr) };
    assert_eq!(bytes.as_bytes(), text.as_bytes());
}

/// Write once, then survive several minors with NO further writes. With
/// the promotion age pinned at 4 the young prototype stays young across
/// minors 1..=3, so the class (old, pinned) must stay in the minor scan
/// set the whole time: an entry is re-armed by every scan that still finds
/// a young referent, not consumed after one minor. Returns false (without
/// reading the possibly stale link) the first time the entry is missing.
fn write_once_then_minors(cid: u32) -> bool {
    let _copying_nursery = crate::gc::CopyingNurseryTestGuard::new(0);
    let _triggers = crate::gc::GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    let _force_evacuation = crate::gc::knob_overrides::ForcedEvacuationTestGuard::on();
    let _age = crate::gc::pin_tenuring_survivals_for_test(4);
    crate::gc::register_runtime_handle_root_scanner_for_tests();
    register(cid);
    let closure = class_value_ptr(cid);
    let proto = crate::object::js_object_alloc(cid + 1, 1);
    crate::object::js_object_set_field(
        proto,
        0,
        crate::value::JSValue::from_bits(crate::value::INT32_TAG | 777),
    );
    class_decl_prototype_link_store(cid, proto);
    let slot = unsafe { prototype_link_slot(closure) } as usize;
    assert!(
        crate::gc::old_slot_page_is_remembered_for_test(slot),
        "the write must have remembered the class"
    );
    let mut moved_from = proto as usize;
    for minor in 1..=3 {
        let _ = crate::gc::gc_collect_minor();
        // No write since the first store. The referent must still be young
        // (age < 4), so the entry must still be present.
        if !crate::gc::old_slot_page_is_remembered_for_test(slot) {
            return false;
        }
        let linked = class_decl_prototype_link(cid);
        assert!(!linked.is_null());
        assert!(
            matches!(
                crate::arena::classify_heap_generation(linked as usize),
                crate::arena::HeapGeneration::Nursery
            ),
            "minor {minor}: the referent must still be young"
        );
        assert_ne!(
            linked as usize, moved_from,
            "minor {minor}: it must have moved"
        );
        moved_from = linked as usize;
        let field = crate::object::js_object_get_field(linked, 0);
        assert_eq!(field.bits(), crate::value::INT32_TAG | 777, "minor {minor}");
    }
    // The fourth survival promotes it; the value is intact afterwards too.
    let _ = crate::gc::gc_collect_minor();
    let linked = class_decl_prototype_link(cid);
    let field = crate::object::js_object_get_field(linked, 0);
    assert_eq!(
        field.bits(),
        crate::value::INT32_TAG | 777,
        "after promotion"
    );
    true
}

#[test]
fn a_class_stays_remembered_while_its_referent_is_young() {
    assert!(write_once_then_minors(0x6B10));
}

/// The kind is a shape fact: class function objects carry their own
/// ShapeId, distinct from FunctionDictionary (shapes are canonical per
/// facts — without its marker fact the class shape WOULD be the dictionary
/// id and every dictionary function would route as a class), and it is
/// sticky across own-property installs.
#[test]
fn class_function_objects_have_their_own_sticky_shape() {
    let cid = 0x6C01;
    register(cid);
    let class_shape = crate::closure::shape::function_class_shape();
    assert_ne!(
        class_shape,
        crate::closure::shape::function_dictionary_shape()
    );
    let ptr = class_value_ptr(cid);
    assert_eq!(unsafe { (*ptr).shape_id }, class_shape);
    class_static_set(cid, "s", 1.0);
    crate::closure::shape::note_function_own_state_changed(ptr as usize);
    assert_eq!(unsafe { (*ptr).shape_id }, class_shape, "sticky");
    extern "C" fn body(_: *const ClosureHeader, _this: crate::closure::JsThis) -> f64 {
        0.0
    }
    let f = crate::closure::js_closure_alloc(crate::fn_info!(body, 0), 0);
    crate::closure::shape::note_function_own_state_changed(f as usize);
    assert_ne!(
        unsafe { (*f).shape_id },
        class_shape,
        "a dictionary function is not a class"
    );
}

/// A class constructor's `length` and `name` are own data properties of
/// its function object (in its own-property object, intrinsic
/// attributes); a static method of that name owns the key instead.
#[test]
fn name_and_length_are_own_data_of_the_function_object() {
    let cid = 0x6D01;
    register(cid);
    unsafe { crate::object::js_register_class_name(cid, b"Zed".as_ptr(), 3) };
    crate::object::js_register_class_length(cid, 2);
    let ptr = class_value_ptr(cid) as usize;
    assert_eq!(
        unsafe { crate::closure::props::bag_get(ptr, b"length") },
        Some(2.0),
        "own length"
    );
    let name = unsafe { crate::closure::props::bag_get(ptr, b"name") }.expect("own name");
    let mut scratch = [0u8; crate::value::SHORT_STRING_MAX_LEN];
    // SAFETY: a live string value just read from the object.
    let bytes = unsafe {
        crate::string::js_string_key_bytes(
            crate::value::JSValue::from_bits(name.to_bits()),
            &mut scratch,
        )
    }
    .expect("a string");
    assert_eq!(bytes, b"Zed");
    for key in ["length", "name"] {
        assert_eq!(
            crate::object::class_registry::class_static_defined_attrs(cid, key),
            Some(INTRINSIC_ATTRS),
            "{key}: non-writable, non-enumerable, configurable"
        );
    }
    extern "C" fn static_name() -> f64 {
        0.0
    }
    unsafe {
        crate::object::class_registry::js_register_class_static_method(
            cid as i64,
            b"name".as_ptr(),
            4,
            static_name as *const () as usize as i64,
            0,
            0,
        )
    };
    extern "C" fn static_name_entry(
        _closure: *const crate::closure::ClosureHeader,
        _this: crate::closure::JsThis,
    ) -> f64 {
        0.0
    }
    let entry_info = crate::fn_info!(static_name_entry, 0);
    unsafe {
        crate::object::class_registry::parent_static::js_register_class_static_method_entry(
            cid as i64,
            b"name".as_ptr(),
            4,
            entry_info as i64,
        )
    };
    // node: `class Zed { static name() {} }` -> Zed.name is the method, an
    // own data property { writable, !enumerable, configurable }.
    let method = unsafe { crate::closure::props::bag_get(ptr, b"name") }
        .expect("a static method named `name` is the class's own `name`");
    assert_eq!(
        unsafe { static_method_code_of(method) },
        Some(entry_info as usize),
        "its value is the method's function object"
    );
    assert_eq!(
        crate::object::class_registry::class_static_defined_attrs(cid, "name"),
        Some((true, false, true)),
        "method attributes, not the intrinsic's"
    );
}

/// A ClassBody static accessor is an accessor property of the class
/// function object: ClassBody attributes, one closure per half across
/// reads, attributes changed in place, and a delete removes it.
#[test]
fn static_accessors_are_accessor_properties_of_the_function_object() {
    let cid = 0x6E01;
    register(cid);
    extern "C" fn getter() -> f64 {
        41.0
    }
    unsafe {
        crate::object::js_register_class_name(cid, b"Acc".as_ptr(), 3);
        crate::object::class_registry::js_register_class_static_getter(
            cid as i64,
            b"g".as_ptr(),
            1,
            getter as *const () as usize as i64,
        );
    }
    let (acc, enumerable, configurable) =
        class_static_own_accessor(cid, "g").expect("an own accessor property");
    assert_ne!(acc.get, 0, "a reflected getter closure");
    assert_eq!(acc.set, 0);
    assert_eq!((enumerable, configurable), CLASS_ACCESSOR_DEFAULT_ATTRS);
    let again = class_static_own_accessor(cid, "g").unwrap().0;
    assert_eq!(again.get, acc.get, "one closure per half");
    let ptr = class_value_ptr(cid) as usize;
    assert_eq!(
        unsafe { crate::closure::props::bag_get(ptr, b"g") },
        None,
        "an accessor key has no data value"
    );
    let got = unsafe { class_static_accessor_call_get(acc, class_value(cid)) };
    assert_eq!(got, 41.0);
    class_static_set_accessor_attrs(cid, "g", true, false);
    assert_eq!(
        class_static_own_accessor(cid, "g").map(|(_, e, c)| (e, c)),
        Some((true, false))
    );
    class_static_set_accessor_attrs(cid, "g", false, true);
    assert!(class_static_remove(cid, "g"), "delete removes the property");
    assert!(class_static_own_accessor(cid, "g").is_none());
}

/// Only the function object's own code pointer names a class.
#[test]
fn ordinary_closures_and_numbers_are_not_class_values() {
    extern "C" fn body(_: *const ClosureHeader, _this: crate::closure::JsThis) -> f64 {
        0.0
    }
    let c = crate::closure::js_closure_alloc(crate::fn_info!(body, 0), 0);
    assert_eq!(class_closure_id(c as usize), None);
    assert_eq!(class_value_id(42.0), None);
    assert_eq!(
        class_value_id(f64::from_bits(crate::value::TAG_UNDEFINED)),
        None
    );
}
