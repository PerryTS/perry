use super::{gc_pointer_and_type_from_value, test_side_registry_tower_entries};

#[test]
fn arena_receiver_skips_side_registry_probe_tower() {
    // Arm the expensive late probes. The ordinary object below is still fully
    // described by its arena range and GcHeader, so none of these registries
    // should be consulted to classify it.
    let _map = crate::map::js_map_alloc(4);
    let _set = crate::set::js_set_alloc(4);
    let _symbol = unsafe { crate::symbol::alloc_symbol(std::ptr::null_mut(), false) };
    let object = crate::object::js_object_alloc(0, 0);
    let value = crate::value::js_nanbox_pointer(object as i64);

    let before = test_side_registry_tower_entries();
    let (actual, gc_type) = unsafe {
        gc_pointer_and_type_from_value(value)
            .expect("an ordinary arena object must resolve to its GC allocation")
    };

    assert_eq!(actual, object.cast());
    assert_eq!(gc_type, crate::gc::GC_TYPE_OBJECT);
    assert_eq!(
        test_side_registry_tower_entries(),
        before,
        "arena metadata should bypass every address-keyed side-registry probe"
    );
}

#[test]
fn arena_native_kinds_keep_their_existing_classification() {
    let buffer = crate::buffer::buffer_alloc(8);
    let typed_array =
        crate::typedarray::js_typed_array_new(crate::typedarray::KIND_UINT8 as i32, 8.0);
    let map = crate::map::js_map_alloc(4);
    let set = crate::set::js_set_alloc(4);
    let symbol = unsafe { crate::symbol::alloc_symbol(std::ptr::null_mut(), false) };

    let classify = |ptr: usize| unsafe {
        gc_pointer_and_type_from_value(crate::value::js_nanbox_pointer(ptr as i64))
    };
    assert_eq!(
        classify(buffer as usize).map(|(_, kind)| kind),
        Some(crate::gc::GC_TYPE_BUFFER)
    );
    assert_eq!(
        classify(typed_array as usize).map(|(_, kind)| kind),
        Some(crate::gc::GC_TYPE_TYPED_ARRAY)
    );
    assert!(classify(map as usize).is_none());
    assert!(classify(set as usize).is_none());
    assert!(classify(symbol as usize).is_none());

    // RegExp shares GC_TYPE_OBJECT with ordinary objects. Its resident magic,
    // not the address registry, is the discriminator on the arena fast path.
    let regexp = crate::arena::arena_alloc_gc(
        std::mem::size_of::<crate::regex::RegExpHeader>(),
        8,
        crate::gc::GC_TYPE_OBJECT,
    ) as *mut crate::regex::RegExpHeader;
    unsafe {
        std::ptr::write_bytes(
            regexp.cast::<u8>(),
            0,
            std::mem::size_of::<crate::regex::RegExpHeader>(),
        );
        (*regexp).magic = crate::regex::REGEXP_MAGIC;
    }
    assert!(classify(regexp as usize).is_none());
}

#[test]
fn headerless_values_still_use_the_side_registry_fallback() {
    // Persistent Symbols (`Symbol.for` / well-known symbols) are Box-leaked,
    // unlike fresh GC-managed Symbols. Build a fresh representative instead
    // of using the process cache: the test harness resets SYMBOL_POINTERS
    // between tests but deliberately does not reset that process-wide cache.
    let symbol = Box::into_raw(Box::new(crate::symbol::SymbolHeader {
        magic: crate::symbol::SYMBOL_MAGIC,
        registered: 0,
        description: std::ptr::null_mut(),
        id: u64::MAX,
    }));
    crate::symbol::register_symbol_pointer(symbol as usize);
    assert!(!crate::arena::pointer_in_nursery(symbol as usize));
    assert!(!crate::arena::pointer_in_old_gen(symbol as usize));

    let before = test_side_registry_tower_entries();
    let actual =
        unsafe { gc_pointer_and_type_from_value(crate::value::js_nanbox_pointer(symbol as i64)) };
    assert!(actual.is_none());
    assert_eq!(test_side_registry_tower_entries(), before + 1);

    let shared = crate::shared_sab::alloc_shared_sab(8);
    assert!(!crate::arena::pointer_in_nursery(shared as usize));
    assert!(!crate::arena::pointer_in_old_gen(shared as usize));

    let before = test_side_registry_tower_entries();
    let actual =
        unsafe { gc_pointer_and_type_from_value(crate::value::js_nanbox_pointer(shared as i64)) };
    assert_eq!(
        actual.map(|(_, kind)| kind),
        Some(crate::gc::GC_TYPE_BUFFER)
    );
    assert_eq!(test_side_registry_tower_entries(), before + 1);
}
