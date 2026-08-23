/// Initialize the WeakMap/WeakSet internal entry slot on an existing user
/// class instance, then consume the optional iterable through the ordinary
/// builtin algorithm. `kind`: 0 = WeakMap, 1 = WeakSet.
#[no_mangle]
pub extern "C" fn js_weak_collection_subclass_init(this: f64, kind: i32, iterable: f64) -> f64 {
    let scope = crate::gc::RuntimeHandleScope::new();
    let this = scope.root_nanbox_f64(this);
    let raw = crate::value::js_nanbox_get_pointer(this.get_nanbox_f64()) as usize;
    let is_object = unsafe {
        crate::value::addr_class::try_read_gc_header(raw)
            .is_some_and(|header| header.obj_type == crate::gc::GC_TYPE_OBJECT)
    };
    if !is_object {
        return this.get_nanbox_f64();
    }
    let entries = js_array_alloc(0);
    let entries = scope.root_raw_mut_ptr(entries);
    let key = crate::string::js_string_from_bytes(WEAK_ENTRIES_KEY.as_ptr(), 18);
    let object = crate::value::js_nanbox_get_pointer(this.get_nanbox_f64()) as *mut ObjectHeader;
    // #7341: the entries address is a SCOPED argument, never a copy held
    // across a collection point. `js_string_from_bytes` above is the last
    // allocating step before the call, and `object` is re-derived from the
    // rooted `this` after it; `with_mut_ptr` reads the array pointer inside
    // the argument expression and hands it straight to a self-rooting entry
    // point.
    entries.with_mut_ptr(|entries_ptr: *mut crate::array::ArrayHeader| {
        js_object_set_field_by_name(
            object,
            key,
            f64::from_bits(JSValue::array_ptr(entries_ptr).bits()),
        )
    });
    if kind == 0 {
        js_weakmap_init_iterable(this.get_nanbox_f64(), iterable)
    } else {
        js_weakset_init_iterable(this.get_nanbox_f64(), iterable)
    }
}
