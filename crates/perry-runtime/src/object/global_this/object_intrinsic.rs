use super::*;

/// A realm's `%Object%` constructor and `%Object.prototype%`.
type ObjectPair = (*mut crate::closure::ClosureHeader, *mut ObjectHeader);

crate::perry_thread_local! {
    /// Set while THIS thread builds its `%Object%` / `%Object.prototype%`.
    /// The build stores into the objects it creates, and the store path asks
    /// "is this receiver %Object.prototype%?", which lands back in
    /// [`ensure_object_intrinsics`]. That nested question has an honest answer
    /// without building: the intrinsic does not exist yet, so nothing is it.
    static OBJECT_INTRINSICS_BUILDING: std::sync::atomic::AtomicBool =
        const { std::sync::atomic::AtomicBool::new(false) };
}

/// This realm's `%Object%` constructor and `%Object.prototype%`, built on
/// first use, complete: the constructor with its statics (`keys`, `create`,
/// `getPrototypeOf`, ...), `name`/`length`, and a non-writable `prototype`;
/// the prototype with `constructor` and its methods (`toString`,
/// `hasOwnProperty`, `__proto__`, ...).
///
/// Nothing here reads `globalThis`. A declared class's prototype object has
/// `%Object.prototype%` as its `[[Prototype]]`, and so does every ordinary
/// object; reaching it through `globalThis.Object` made the first class
/// materialization build the whole realm global (several hundred builtins,
/// ~50M instructions) for one pointer. `populate_global_this_builtins`
/// ADOPTS these two objects as `globalThis.Object` / `Object.prototype`, so
/// there is exactly one of each per realm whichever side asks first.
///
/// Both are rooted in `scan_object_cache_roots_mut` and the prototype's
/// address is memoized in the `Object.prototype` row of the per-thread
/// prototype-address cache, like the other realm intrinsics.
///
/// Returns null pointers only while this thread is inside the build itself
/// (see [`OBJECT_INTRINSICS_BUILDING`]) or if an allocation failed.
pub(crate) fn ensure_object_intrinsics() -> ObjectPair {
    let loaded = || {
        (
            crate::object::OBJECT_INTRINSIC_PTR.load(Ordering::Acquire),
            crate::object::OBJECT_INTRINSIC_PROTO_PTR.load(Ordering::Acquire),
        )
    };
    let (ctor, proto) = loaded();
    if ctor != 0 && proto != 0 {
        return (ctor as *mut _, proto as *mut _);
    }
    if OBJECT_INTRINSICS_BUILDING.with(|b| b.swap(true, Ordering::AcqRel)) {
        return (std::ptr::null_mut(), std::ptr::null_mut());
    }
    // `ensure_object_prototype_shape` may have installed a shape-only sentinel
    // for allocation-free prototype-chain proofs. It was never observable as
    // the intrinsic; retire its two roots before building the complete pair.
    // The build runs under `GcSuppressScope`, so the sentinel cannot move or be
    // collected in the rootless interval.
    if ctor == 0 && proto != 0 {
        crate::object::OBJECT_INTRINSIC_PROTO_PTR.store(0, Ordering::Release);
        crate::array::forget_object_prototype_intrinsic();
    }
    let built = build_object_intrinsics();
    if let Some((ctor, proto)) = built {
        crate::object::OBJECT_INTRINSIC_PTR.store(ctor as i64, Ordering::Release);
        crate::object::OBJECT_INTRINSIC_PROTO_PTR.store(proto as i64, Ordering::Release);
        crate::array::note_object_prototype_intrinsic(proto as usize);
    }
    OBJECT_INTRINSICS_BUILDING.with(|b| b.store(false, Ordering::Release));
    built.unwrap_or((std::ptr::null_mut(), std::ptr::null_mut()))
}

/// A shape-owned stand-in for an as-yet unmaterialized `%Object.prototype%`.
///
/// Ordinary objects name the realm default prototype in their ShapeId before
/// any observable use has to allocate the intrinsic. A store proof still has
/// to account for Annex B's inherited `__proto__` accessor, but constructing
/// the complete intrinsic just to inspect that fact allocates its constructor
/// and method closures. Keep the blocking property as an accessor entry on a
/// real shaped object instead. The ordinary prototype accessors never expose
/// this sentinel: [`ensure_object_intrinsics`] replaces it with the complete
/// pair before returning an observable prototype.
pub(crate) fn ensure_object_prototype_shape() -> *mut ObjectHeader {
    let current = crate::object::OBJECT_INTRINSIC_PROTO_PTR.load(Ordering::Acquire);
    if current != 0 {
        return current as *mut ObjectHeader;
    }
    if OBJECT_INTRINSICS_BUILDING.with(|b| b.swap(true, Ordering::AcqRel)) {
        return std::ptr::null_mut();
    }

    // No collection may see the raw object before the intrinsic root is
    // published. Unlike the complete intrinsic this sentinel is not immortal:
    // replacing the root lets the next collection reclaim it.
    let _no_move = crate::gc::GcSuppressScope::new();
    let proto = js_object_alloc(0, 0);
    if !proto.is_null() {
        let key = crate::string::js_string_from_bytes(b"__proto__".as_ptr(), 9);
        js_object_set_field_by_name(proto, key, f64::from_bits(crate::value::TAG_UNDEFINED));
        super::super::set_builtin_accessor_descriptor(
            proto as usize,
            "__proto__".to_string(),
            super::super::AccessorDescriptor { get: 0, set: 0 },
            crate::object::PropertyAttrs::new(true, false, true),
        );
        crate::object::OBJECT_INTRINSIC_PROTO_PTR.store(proto as i64, Ordering::Release);
        crate::array::note_object_prototype_intrinsic(proto as usize);
    }
    OBJECT_INTRINSICS_BUILDING.with(|b| b.store(false, Ordering::Release));
    proto
}

/// The `%Object%` / `%Object.prototype%` pair for the realm whose global is
/// `global`: this thread's own pair (see [`ensure_object_intrinsics`]) when
/// `global` is the thread's realm global, a fresh pair for any other realm
/// (a `vm` context or an eval realm), whose intrinsics are its own.
pub(crate) fn object_intrinsics_for_realm(global: *mut ObjectHeader) -> ObjectPair {
    if is_thread_realm_global(global) {
        return ensure_object_intrinsics();
    }
    build_object_intrinsics().unwrap_or((std::ptr::null_mut(), std::ptr::null_mut()))
}

/// `%Object.prototype%` as NaN-boxed pointer bits, built on first use.
pub(crate) fn object_prototype_intrinsic_bits() -> Option<u64> {
    let (_, proto) = ensure_object_intrinsics();
    (!proto.is_null()).then(|| crate::value::js_nanbox_pointer(proto as i64).to_bits())
}

fn build_object_intrinsics() -> Option<ObjectPair> {
    // The same no-move window and immortal-layout scope as the realm
    // bootstrap (see `populate_global_this_builtins`): the constructor and
    // prototype are held as raw pointers across every allocating install
    // below, and both live for the life of the realm.
    let _no_move = crate::gc::GcSuppressScope::new();
    let _immortal = crate::gc::ImmortalLayoutScope::new();
    let info = crate::fn_info!(global_this_object_thunk, 1; with_declared(1));
    let closure_ptr = crate::closure::js_closure_alloc(info, 0);
    if closure_ptr.is_null() {
        return None;
    }
    install_builtin_constructor_statics("Object", closure_ptr);
    super::super::native_module::set_bound_native_closure_name(closure_ptr, "Object");
    if let Some(len) = builtin_constructor_spec_length("Object") {
        super::super::native_module::set_builtin_closure_length(closure_ptr as usize, len);
    }
    for key in ["name", "length"] {
        super::super::set_builtin_property_attrs(
            closure_ptr as usize,
            key.to_string(),
            super::super::PropertyAttrs::new(false, false, true),
        );
    }
    let proto_obj = js_object_alloc(0, 0);
    if proto_obj.is_null() {
        return None;
    }
    let ctor_value = crate::value::js_nanbox_pointer(closure_ptr as i64);
    let proto_key = crate::string::js_string_from_bytes(b"prototype".as_ptr(), 9);
    super::super::define_builtin_data_property(
        closure_ptr as *mut ObjectHeader,
        proto_key,
        crate::value::js_nanbox_pointer(proto_obj as i64),
        "prototype".to_string(),
        super::super::PropertyAttrs::new(false, false, false),
    );
    let ctor_key = crate::string::js_string_from_bytes(b"constructor".as_ptr(), 11);
    super::super::define_builtin_data_property(
        proto_obj,
        ctor_key,
        ctor_value,
        "constructor".to_string(),
        super::super::PropertyAttrs::new(true, false, true),
    );
    populate_builtin_prototype_methods("Object", proto_obj);
    Some((closure_ptr, proto_obj))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn on_a_fresh_thread(body: impl FnOnce() + Send + 'static) {
        std::thread::Builder::new()
            .stack_size(16 << 20)
            .spawn(body)
            .expect("spawn object-intrinsic test thread")
            .join()
            .expect("object-intrinsic test thread panicked");
    }

    /// A declared class's prototype reaches %Object.prototype% without
    /// building the realm global, and the realm global then adopts the very
    /// same %Object% / %Object.prototype% rather than building a second pair.
    #[test]
    fn object_prototype_is_built_without_the_realm_global_and_adopted_by_it() {
        on_a_fresh_thread(|| {
            assert!(!crate::object::global_this_is_materialized());
            let class_parent = crate::object::class_registry::global_object_prototype_bits()
                .expect("%Object.prototype% for a class prototype");
            let default_parent = crate::object::prototype_chain::default_object_prototype_bits()
                .expect("%Object.prototype% for an ordinary object");
            assert_eq!(class_parent, default_parent);
            assert!(
                !crate::object::global_this_is_materialized(),
                "reaching %Object.prototype% built the realm global"
            );
            let (ctor, proto) = ensure_object_intrinsics();
            let proto_addr = proto as usize;
            assert_eq!(
                crate::array::object_prototype_addr_if_resolved(),
                proto_addr
            );
            assert!(crate::array::object_prototype_addr_matches(proto_addr));
            // Complete before any realm global: statics, prototype methods.
            let keys = crate::closure::closure_get_dynamic_prop(ctor as usize, "keys");
            assert!(
                JSValue::from_bits(keys.to_bits()).is_pointer(),
                "Object.keys missing"
            );
            let has_own = crate::string::js_string_from_bytes(b"hasOwnProperty".as_ptr(), 14);
            let method = js_object_get_field_by_name(proto, has_own);
            assert!(method.is_pointer(), "hasOwnProperty missing");

            crate::object::js_get_global_this();
            let global_object = js_get_global_this_builtin_value(b"Object".as_ptr(), 6);
            let (ctor_now, proto_now) = ensure_object_intrinsics();
            assert_eq!(
                global_object.to_bits(),
                crate::value::js_nanbox_pointer(ctor_now as i64).to_bits(),
                "globalThis.Object is not the intrinsic"
            );
            let global_proto =
                crate::closure::closure_get_dynamic_prop(ctor_now as usize, "prototype");
            assert_eq!(
                global_proto.to_bits(),
                crate::value::js_nanbox_pointer(proto_now as i64).to_bits()
            );
            assert_eq!(crate::array::object_prototype_addr(), proto_now as usize);
        });
    }
}
