/// Initialize the WeakMap/WeakSet internal entry slot on an existing user
/// class instance, then consume the optional iterable through the ordinary
/// builtin algorithm. `kind`: 0 = WeakMap, 1 = WeakSet.
#[no_mangle]
pub extern "C" fn js_weak_collection_subclass_init(this: f64, kind: i32, iterable: f64) -> f64 {
    let scope = crate::gc::RuntimeHandleScope::new();
    let this = scope.root_nanbox_f64(this);
    let iterable = scope.root_nanbox_f64(iterable);
    let raw = crate::value::js_nanbox_get_pointer(this.get_nanbox_f64()) as usize;
    let is_object = unsafe {
        crate::value::addr_class::try_read_gc_header(raw)
            .is_some_and(|header| header.obj_type == crate::gc::GC_TYPE_OBJECT)
    };
    if !is_object {
        return this.get_nanbox_f64();
    }
    storage::initialize(
        this.get_nanbox_f64(),
        if kind == 0 {
            CLASS_ID_WEAKMAP
        } else {
            CLASS_ID_WEAKSET
        },
    );
    if kind == 0 {
        js_weakmap_init_iterable(this.get_nanbox_f64(), iterable.get_nanbox_f64())
    } else {
        js_weakset_init_iterable(this.get_nanbox_f64(), iterable.get_nanbox_f64())
    }
}
