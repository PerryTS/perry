use super::*;
use crate::JSValue;
use std::collections::HashMap;

pub(crate) fn ensure_function_prototype_object(
    func_value: f64,
    class_id: u32,
) -> *mut ObjectHeader {
    if class_id == 0 {
        return std::ptr::null_mut();
    }
    // A `Temporal.<X>` constructor pre-populates its `prototype` (a real object
    // with the type's accessor getters / methods) during globalThis init and
    // stamps it on the closure's `prototype` dynamic prop — but intentionally
    // NOT in the GC-scanned class-prototype cache (rooting an init-time arena
    // object there dangles across the test-suite's arena-fixture swaps). So when
    // `new Temporal.X()` / a reflective `.prototype` read lands here, return that
    // pre-set object as-is instead of allocating a fresh empty one (which would
    // overwrite the populated prototype). Gated on `temporal_ctor_kind` so the
    // ordinary class-prototype flow (which relies on the cache for method
    // registration) is unaffected.
    if super::super::global_this::temporal_ctor_kind(func_value).is_some() {
        let fv_bits = func_value.to_bits();
        let fp = (fv_bits & crate::value::POINTER_MASK) as usize;
        if fp != 0 {
            let dyn_proto = crate::closure::closure_get_dynamic_prop(fp, "prototype");
            let dp = JSValue::from_bits(dyn_proto.to_bits());
            if dp.is_pointer() {
                let pp = dp.as_pointer::<ObjectHeader>();
                if !pp.is_null() {
                    return pp as *mut ObjectHeader;
                }
            }
        }
    }
    let existing = class_prototype_object(class_id);
    if !existing.is_null() {
        return existing;
    }

    let scope = crate::gc::RuntimeHandleScope::new();
    let func_handle = scope.root_nanbox_f64(func_value);
    let proto = js_object_alloc(0, 0);
    if proto.is_null() {
        return proto;
    }
    let proto_handle = scope.root_raw_mut_ptr(proto);

    let constructor_key = scope.root_string_ptr(crate::string::js_string_from_bytes(
        b"constructor".as_ptr(),
        "constructor".len() as u32,
    ));
    proto_handle.with_mut_ptr::<ObjectHeader, _>(|proto| {
        constructor_key.with_const_ptr::<crate::StringHeader, _>(|key| {
            js_object_set_field_by_name(proto, key, func_handle.get_nanbox_f64())
        })
    });
    proto_handle.with_mut_ptr::<ObjectHeader, _>(|proto| {
        set_builtin_property_attrs(
            proto as usize,
            "constructor".to_string(),
            PropertyAttrs::new(true, false, true),
        )
    });

    if let Some(object_proto_bits) = global_object_prototype_bits() {
        let object_proto = scope.root_nanbox_u64(object_proto_bits);
        proto_handle.with_mut_ptr::<ObjectHeader, _>(|proto| {
            super::super::prototype_chain::object_set_static_prototype(
                proto as usize,
                object_proto.get_nanbox_u64(),
            )
        });
    }

    proto_handle.with_mut_ptr::<ObjectHeader, _>(|proto| {
        class_prototype_object_root_store(class_id, proto)
    });

    // #5024: methods registered before the prototype object materialized
    // (`F.prototype.m = v` typically runs long before any reflective
    // `F.prototype` read) live only in CLASS_PROTOTYPE_METHODS. Backfill
    // them as ordinary own properties so enumeration sees them; later
    // registrations write through via class_prototype_method_root_store.
    let registered_bits: Vec<(String, u64)> = {
        CLASS_PROTOTYPE_METHODS.with(|table| {
            let guard = table.read().unwrap();
            guard
                .as_ref()
                .and_then(|map| map.get(&class_id))
                .map(|per_class| per_class.iter().map(|(k, &v)| (k.clone(), v)).collect())
                .unwrap_or_default()
        })
    };
    // The copied side-table values are no longer themselves scanner roots.
    // Root the whole snapshot before the first mirrored property can allocate.
    let registered: Vec<_> = registered_bits
        .into_iter()
        .map(|(name, value_bits)| (name, scope.root_nanbox_u64(value_bits)))
        .collect();
    for (name, value) in registered {
        let enumerable = class_prototype_method_is_enumerable(class_id, &name);
        proto_handle.with_mut_ptr::<ObjectHeader, _>(|proto| unsafe {
            mirror_prototype_method_on_object(proto, &name, value.get_nanbox_u64(), enumerable)
        });
    }

    // #5477: the bound `events.EventEmitter` / `EventEmitterAsyncResource` export's
    // synthetic prototype must carry the EventEmitter methods (`emit`/`on`/`once`/
    // …) so the `Object.setPrototypeOf(x, EventEmitter.prototype)` mixin pattern
    // (pino's logger prototype) gives `x` a working `emit`/`on`. The installed
    // closures read IMPLICIT_THIS, so a plain object that merely inherits this
    // prototype dispatches against ITSELF (listener state is keyed by the receiver
    // object, not a captured instance). Mirrors what `Stream.prototype` already
    // does. This proto is cached (`class_prototype_object_root_store` above), so
    // the install runs once.
    // Routed through the armed ops table (see `nm_namespace_hooks`): bound
    // native-callable closures only exist once `callable_exports` minted one
    // (which arms), so binaries without module imports link neither the
    // probe nor the EventEmitter prototype machinery.
    if let Some(ops) = super::super::nm_ee_ops() {
        proto_handle.with_mut_ptr::<ObjectHeader, _>(|proto| unsafe {
            (ops.ee_prototype_install)(func_handle.get_nanbox_f64(), proto)
        });
    }

    let func_bits = func_handle.get_nanbox_u64();
    if (func_bits >> 48) == 0x7FFD {
        let func_ptr = (func_bits & crate::value::POINTER_MASK) as usize;
        if func_ptr != 0 {
            crate::closure::closure_set_dynamic_prop(
                func_ptr,
                "prototype",
                proto_handle.with_mut_ptr::<ObjectHeader, _>(|proto| {
                    crate::value::js_nanbox_pointer(proto as i64)
                }),
            );
            set_builtin_property_attrs(
                func_ptr,
                "prototype".to_string(),
                PropertyAttrs::new(true, false, false),
            );
        }
    }

    proto_handle.with_mut_ptr::<ObjectHeader, _>(|proto| proto)
}

/// Floor of the synthetic class-id range (see [`NEXT_SYNTHETIC_CLASS_ID`]).
///
/// RULE 2 (single-path object model): a class id must never land inside the
/// ShapeId range `[SHAPE_ID_BASE, SHAPE_ID_END)`. The read path tells a
/// STAMPED object from an UNSTAMPED one by range alone — the `+4` word holds
/// `parent_class_id` until a shape is stamped over it, and
/// `shapes::object_shape_stamp` answers with that word exactly when
/// `shapes::is_shape_id` accepts it. This counter used to start at
/// `0x8000_0000`, the ShapeId floor, so synthetic id *n* and ShapeId *n* were
/// the same u32: an instance born with a synthetic PARENT id
/// (`class X extends someFunction`, resolved through
/// `dynamic_value_class_id` -> `function_class_id`, then written to `+4` by
/// `js_object_alloc_class_dynamic_parent`) claimed a live, unrelated shape,
/// whose ordered keys and live-inline-slot bound then described its layout.
/// The band above the ShapeId range is unused: codegen ids start at 1, the
/// builtin bands are `0x7FFF_FF00..=0x7FFF_FFFF` and `0xFFFF_0000..`.
pub(crate) const SYNTHETIC_CLASS_ID_BASE: u32 = 0xC000_0000;
/// Exclusive end of the synthetic range: the first reserved builtin id.
pub(crate) const SYNTHETIC_CLASS_ID_END: u32 = 0xFFFF_0000;

const _: () = assert!(
    SYNTHETIC_CLASS_ID_BASE >= crate::object::shapes::SHAPE_ID_END,
    "rule 2: synthetic class ids must not overlap the ShapeId range"
);
const _: () = assert!(
    SYNTHETIC_CLASS_ID_END > SYNTHETIC_CLASS_ID_BASE,
    "the synthetic class-id range must be non-empty"
);

per_test_global! {
    /// Synthetic class id allocator for prototype-object classes. Allocated
    /// from `[SYNTHETIC_CLASS_ID_BASE, SYNTHETIC_CLASS_ID_END)` so the ids
    /// stay separate from codegen-assigned ids (which start from 1 and grow
    /// by module), from the reserved builtin bands, AND from the ShapeId
    /// range. Mint through [`alloc_synthetic_class_id`], never by a bare
    /// `fetch_add` — the bound is the invariant.
    pub static NEXT_SYNTHETIC_CLASS_ID: std::sync::atomic::AtomicU32 =
        std::sync::atomic::AtomicU32::new(SYNTHETIC_CLASS_ID_BASE);
}

/// Mint the next synthetic class id, parked at the end of the range rather
/// than wrapped.
///
/// A bare `fetch_add` could walk out of the range in two ways, both of which
/// break a rule the read path depends on: past `SYNTHETIC_CLASS_ID_END` it
/// collides with the reserved builtin ids (`0xFFFF_0000..`), and on u32 wrap
/// it lands back in — among others — the ShapeId range. Exhaustion is
/// unreachable in practice (2^30 ids, one per distinct
/// function constructor / `F.prototype = X` function, not per instance), so
/// saturating is the conservative answer: `0` means "no synthetic id", which
/// every caller already handles as "stays parentless".
pub(crate) fn alloc_synthetic_class_id() -> u32 {
    use std::sync::atomic::Ordering;
    loop {
        let id = NEXT_SYNTHETIC_CLASS_ID.load(Ordering::Relaxed);
        if id >= SYNTHETIC_CLASS_ID_END {
            NEXT_SYNTHETIC_CLASS_ID.store(SYNTHETIC_CLASS_ID_END, Ordering::Relaxed);
            return 0;
        }
        if NEXT_SYNTHETIC_CLASS_ID
            .compare_exchange_weak(id, id + 1, Ordering::Relaxed, Ordering::Relaxed)
            .is_ok()
        {
            debug_assert!(
                !crate::object::shapes::is_shape_id(id),
                "rule 2: a synthetic class id must never be readable as a ShapeId"
            );
            return id;
        }
    }
}

#[cfg(test)]
pub(crate) fn test_alloc_synthetic_class_id() -> u32 {
    alloc_synthetic_class_id()
}

#[cfg(test)]
#[path = "prototype_objects/parent_class_object_tests.rs"]
mod parent_class_object_tests;

/// The `[[Prototype]]` object recorded for a SYNTHETIC class id — one of the
/// ids `Object.create(proto)` (#809) and `F.prototype = obj` (#711) allocate
/// from [`NEXT_SYNTHETIC_CLASS_ID`]. That link is the authoritative prototype
/// of every instance stamped with the id.
///
/// Unlike [`class_prototype_object`], this refuses a DECLARED class id, whose
/// entry in the same table is the parent CLASS OBJECT of a class-expression
/// subclass (#1788/#6552) rather than a prototype.
pub(crate) fn synthetic_class_prototype_object(class_id: u32) -> *mut ObjectHeader {
    if class_id < SYNTHETIC_CLASS_ID_BASE
        || class_id >= NEXT_SYNTHETIC_CLASS_ID.load(std::sync::atomic::Ordering::Relaxed)
    {
        return std::ptr::null_mut();
    }
    class_prototype_object(class_id)
}

/// `proto_obj` (the [`class_prototype_object`] entry for `class_id`) when it
/// is the parent CLASS OBJECT of a class-expression subclass (#1788/#6552)
/// rather than a prototype. A synthetic id's entry is a real prototype even
/// when that prototype is itself a class object (`Object.create(C)`).
fn declared_parent_class_object(
    class_id: u32,
    proto_obj: *mut ObjectHeader,
) -> Option<*mut ObjectHeader> {
    if proto_obj.is_null()
        || (SYNTHETIC_CLASS_ID_BASE..SYNTHETIC_CLASS_ID_END).contains(&class_id)
        || !is_class_object_ptr(proto_obj as *const u8)
    {
        return None;
    }
    Some(proto_obj)
}

/// [`class_prototype_object`] for a walk that serves an INSTANCE. Null where
/// that entry is a declared class's parent class object: its statics are not
/// on the instance's prototype chain (#10890).
pub(crate) fn instance_class_prototype_object(class_id: u32) -> *mut ObjectHeader {
    let proto_obj = class_prototype_object(class_id);
    if declared_parent_class_object(class_id, proto_obj).is_some() {
        return std::ptr::null_mut();
    }
    proto_obj
}

/// Perform ordinary `.prototype` assignment, then synchronize the synthetic
/// class metadata used when a class extends a function (#711, #9365).
#[no_mangle]
pub extern "C" fn js_set_prototype_property(receiver: f64, value: f64, strict: i32) -> f64 {
    let scope = crate::gc::RuntimeHandleScope::new();
    let receiver = scope.root_nanbox_f64(receiver);
    let value = scope.root_nanbox_f64(value);
    let key = scope.root_string_ptr(crate::string::js_string_from_bytes(
        b"prototype".as_ptr(),
        9,
    ));
    let key = key
        .with_const_ptr::<crate::StringHeader, _>(|key| crate::value::js_nanbox_string(key as i64));
    crate::proxy::js_put_value_set(
        receiver.get_nanbox_f64(),
        key,
        value.get_nanbox_f64(),
        receiver.get_nanbox_f64(),
        strict,
    );

    // Synchronize from the actual own property. A sloppy rejected write or an
    // accessor must not install the attempted RHS as a class prototype, and
    // synchronization must not reset the property's descriptor attributes.
    // These probes and side-table updates have no JS/GC safepoints; the receiver
    // keeps its own prototype live throughout. Ownership is checked before any
    // header read so proxies and other synthetic pointer values are harmless.
    let func = receiver.get_nanbox_f64();
    let func_value = JSValue::from_bits(func.to_bits());
    if func_value.is_pointer() {
        let func_ptr = func_value.as_pointer::<u8>() as usize;
        let header = unsafe { crate::value::addr_class::try_read_tracked_gc_header(func_ptr) };
        if header
            .is_some_and(|header| unsafe { header.as_ref().obj_type == crate::gc::GC_TYPE_CLOSURE })
            && get_accessor_descriptor(func_ptr, "prototype").is_none()
        {
            if let Some(proto) = crate::closure::closure_get_own_dynamic_prop(func_ptr, "prototype")
            {
                let proto = JSValue::from_bits(proto.to_bits());
                if proto.is_pointer() {
                    let proto_ptr = proto.as_pointer::<ObjectHeader>() as *mut ObjectHeader;
                    let header = unsafe {
                        crate::value::addr_class::try_read_tracked_gc_header(proto_ptr as usize)
                    };
                    if header.is_some_and(|header| unsafe {
                        header.as_ref().obj_type == crate::gc::GC_TYPE_OBJECT
                    }) {
                        let class_id = synthetic_class_id_for_function(func);
                        class_prototype_object_root_store(class_id, proto_ptr);
                        crate::typed_feedback::invalidate_method_change(class_id);
                        crate::object::prop_plan::prop_plan_epoch_bump();
                    }
                }
            }
        }
    }
    value.get_nanbox_f64()
}

/// Legacy function-prototype registration ABI. New codegen uses
/// `js_set_prototype_property` to preserve ordinary property semantics.
///
/// Returns the synthetic class_id allocated for this function (0 if
/// validation fails). The synthetic id is folded into CLASS_REGISTRY
/// when a class extends `func` via the #711 dynamic-parent path.
#[no_mangle]
pub extern "C" fn js_set_function_prototype(func: f64, proto: f64) -> u32 {
    // `F.prototype = obj` re-shapes the chain of every future instance —
    // flush cached store plans (`object::prop_plan`).
    crate::object::prop_plan::prop_plan_epoch_bump();
    let func_bits = func.to_bits();
    let func_tag = func_bits & 0xFFFF_0000_0000_0000;
    let proto_bits = proto.to_bits();
    let proto_tag = proto_bits & 0xFFFF_0000_0000_0000;
    const POINTER_TAG: u64 = 0x7FFD_0000_0000_0000;
    // The function must be a heap-allocated pointer. Anything else (a
    // primitive `<not-a-function>.prototype = X`) is a no-op — preserves the
    // pre-fix baseline where it was just a property write on a non-function.
    if func_tag != POINTER_TAG {
        return 0;
    }
    // A function may legitimately have a *primitive* (e.g. `null`) prototype:
    // `function f() {} f.prototype = null` — it just doesn't establish an
    // `instanceof` chain. Store it as a plain `prototype` data property so reads
    // reflect it (test262 `GetPrototypeFromConstructor` falls back to the
    // default when `newTarget.prototype` is not an object). Without this the
    // write was dropped and the stale auto-created prototype object lingered.
    if proto_tag != POINTER_TAG {
        let func_ptr = (func_bits & crate::value::POINTER_MASK) as usize;
        if func_ptr != 0 && crate::closure::is_closure_ptr(func_ptr) {
            crate::closure::closure_set_dynamic_prop(func_ptr, "prototype", proto);
            set_builtin_property_attrs(
                func_ptr,
                "prototype".to_string(),
                PropertyAttrs::new(true, false, false),
            );
        }
        return 0;
    }
    // Validate the proto pointer points at a real Object. If it's a
    // builtin header (Set/Map/Regex) or null, bail — Perry can't
    // currently model those as prototype sources.
    let proto_ptr = crate::value::js_nanbox_get_pointer(proto) as *mut ObjectHeader;
    if proto_ptr.is_null() {
        return 0;
    }
    let proto_addr = proto_ptr as usize;
    if crate::set::is_registered_set(proto_addr)
        || crate::map::is_registered_map(proto_addr)
        || crate::regex::is_regex_pointer(proto_ptr as *const u8)
    {
        return 0;
    }
    unsafe {
        if !is_valid_obj_ptr(proto_ptr as *const u8) {
            return 0;
        }
        let gc_header =
            (proto_ptr as *const u8).sub(crate::gc::GC_HEADER_SIZE) as *const crate::gc::GcHeader;
        let obj_type = (*gc_header).obj_type;
        // `foo.prototype = new Array(...)` — a real-array prototype can't join
        // the class-id machinery (it has no ObjectHeader), but it must not be
        // DROPPED: store it as the closure's `prototype` dynamic prop so reads
        // reflect it and `js_new_function_construct` links instances to it
        // (test262 filter/15.4.4.20-6-*, some/15.4.4.17-8-*, map/15.4.4.19-9-3).
        // `foo.prototype = someOtherFunction` — a function/closure-valued
        // prototype can't join the class-id machinery either (it has no
        // ObjectHeader): store it the same way as the array case so
        // `js_new_function_construct`'s `linked_user_proto` check links new
        // instances to it (test262 built-ins/Function/prototype/apply/
        // S15.3.4.3_A1_T1, call/S15.3.4.4_A1_T1 — `FACTORY.prototype =
        // Function()` was silently dropped, so `new FACTORY` instances kept
        // the auto-created empty prototype instead of inheriting the real
        // function's methods).
        if obj_type == crate::gc::GC_TYPE_ARRAY
            || obj_type == crate::gc::GC_TYPE_LAZY_ARRAY
            || obj_type == crate::gc::GC_TYPE_CLOSURE
        {
            let func_ptr = (func_bits & crate::value::POINTER_MASK) as usize;
            if func_ptr != 0 && crate::closure::is_closure_ptr(func_ptr) {
                crate::closure::closure_set_dynamic_prop(func_ptr, "prototype", proto);
                set_builtin_property_attrs(
                    func_ptr,
                    "prototype".to_string(),
                    PropertyAttrs::new(true, false, false),
                );
            }
            return 0;
        }
        if obj_type != crate::gc::GC_TYPE_OBJECT {
            return 0;
        }
    }

    // Allocate or reuse a synthetic class id for this function value.
    // The same `function Base() {}` ident can be assigned a prototype
    // multiple times in pathological code; we keep the FIRST mapping
    // and quietly ignore subsequent calls so existing parent edges
    // don't dangle.
    let existing = FUNCTION_CLASS_IDS.with(|table| {
        let read = table.read().unwrap();
        if let Some(map) = read.as_ref() {
            if let Some(&existing) = map.get(&func_bits) {
                return Some(existing);
            }
        }
        None
    });
    if let Some(existing) = existing {
        // Update the prototype object (allow re-pointing) without changing the
        // class_id.
        class_prototype_object_root_store(existing, proto_ptr);
        let func_ptr = (func_bits & crate::value::POINTER_MASK) as usize;
        if func_ptr != 0 {
            crate::closure::closure_set_dynamic_prop(func_ptr, "prototype", proto);
            set_builtin_property_attrs(
                func_ptr,
                "prototype".to_string(),
                PropertyAttrs::new(true, false, false),
            );
        }
        crate::typed_feedback::invalidate_method_change(existing);
        return existing;
    }
    let new_cid = alloc_synthetic_class_id();
    FUNCTION_CLASS_IDS.with(|table| {
        let mut write = table.write().unwrap();
        if write.is_none() {
            *write = Some(HashMap::new());
        }
        write.as_mut().unwrap().insert(func_bits, new_cid);
    });
    class_prototype_object_root_store(new_cid, proto_ptr);
    let func_ptr = (func_bits & crate::value::POINTER_MASK) as usize;
    if func_ptr != 0 {
        crate::closure::closure_set_dynamic_prop(func_ptr, "prototype", proto);
        set_builtin_property_attrs(
            func_ptr,
            "prototype".to_string(),
            PropertyAttrs::new(true, false, false),
        );
    }
    // Register the synthetic id so REGISTERED_CLASS_IDS-gated paths
    // (e.g., the #687 ClassRef-as-receiver short-circuit) recognize it.
    unsafe { js_register_class_id(new_cid) };
    crate::typed_feedback::invalidate_method_change(new_cid);
    new_cid
}

/// Lookup helper for the dispatch chain walk: returns the prototype
/// object pointer for a synthetic class id, or null if none.
#[inline]
pub(crate) fn class_prototype_object(class_id: u32) -> *mut ObjectHeader {
    // #7757: a monomorphized specialization shares its GENERIC's prototype
    // object — there is exactly one `Gen.prototype` at runtime, since
    // TypeScript erases the type arguments. This is the second of the two
    // prototype registries (the other is `CLASS_DECL_PROTOTYPE_OBJECTS`); both
    // must redirect or `x.constructor` and `Object.getPrototypeOf` disagree
    // about whether the specialization is `Gen`.
    let class_id = crate::object::class_generic_origin(class_id).unwrap_or(class_id);
    CLASS_PROTOTYPE_OBJECTS.with(|table| {
        if let Ok(read) = table.read() {
            if let Some(map) = read.as_ref() {
                return map.get(&class_id).copied().unwrap_or(0) as *mut ObjectHeader;
            }
        }
        std::ptr::null_mut()
    })
}

/// #711 / #809: resolve `key` by walking the synthetic-class-id prototype
/// chain (`CLASS_PROTOTYPE_OBJECTS`), recursing into each prototype object
/// as a normal field lookup. Used both when a receiver's own keys miss AND
/// when it has no `keys_array` at all (an `Object.create(proto)` result, or
/// a `Function.prototype = obj` instance with no own props). Returns the
/// first defined, non-null field found on the chain.
/// The receiver-less form, whose ONE caller is the CONSTRUCTOR-side read in
/// `js_object_get_field_by_name` (`C.foo` on an INT32 class ref). It therefore
/// refuses a name that is a declared INSTANCE member — see `constructor_side`.
pub(crate) unsafe fn resolve_proto_chain_field(
    class_id: u32,
    key: *const crate::StringHeader,
) -> Option<JSValue> {
    resolve_proto_chain_field_inner(class_id, key, None, true)
}

pub(crate) unsafe fn resolve_proto_chain_field_with_receiver(
    class_id: u32,
    key: *const crate::StringHeader,
    receiver: f64,
) -> Option<JSValue> {
    resolve_proto_chain_field_inner(class_id, key, Some(receiver), false)
}

unsafe fn inherited_proto_accessor_value(
    proto_obj: *mut ObjectHeader,
    key: *const crate::StringHeader,
    receiver: f64,
) -> Option<JSValue> {
    if key.is_null() || !crate::object::object_has_descriptors(proto_obj as usize) {
        return None;
    }
    let key_ptr = (key as *const u8).add(std::mem::size_of::<crate::StringHeader>());
    let key_len = (*key).byte_len as usize;
    let name = std::str::from_utf8(std::slice::from_raw_parts(key_ptr, key_len)).ok()?;
    let acc = get_accessor_descriptor(proto_obj as usize, name)?;
    if acc.get == 0 {
        return Some(JSValue::undefined());
    }
    // Route through `invoke_accessor_getter` rather than a bare
    // `js_implicit_this_set` + `js_closure_call0`. A getter installed via
    // `Object.defineProperty(Class.prototype, name, { get })` is an ORDINARY
    // method closure whose body reads `this` from its captured receiver slot —
    // not from IMPLICIT_THIS — so merely setting IMPLICIT_THIS left the getter
    // observing the prototype it lives on instead of the instance (winston's
    // `get transports()` saw the prototype, whose `this._readableState` is
    // undefined → "Cannot convert undefined or null to object").
    // `invoke_accessor_getter` clones the closure with `this` rebound to the
    // real receiver (and applies strict/sloppy coercion), matching the
    // own-accessor read path.
    Some(super::super::field_get_set::invoke_accessor_getter(
        acc.get, receiver,
    ))
}

/// Read the actual prototype objects of a class whose parent is a fresh class
/// evaluation. The template-id walk below follows the parent's shared class
/// registry entry; that entry cannot see writes to this evaluation's
/// `Base.prototype` (such as an Effect tagged error's `name`).
unsafe fn evaluated_parent_instance_field(
    decl_proto: *mut ObjectHeader,
    key: *const crate::StringHeader,
    receiver: f64,
) -> Option<JSValue> {
    if decl_proto.is_null() || key.is_null() {
        return None;
    }
    let mut link = Some(crate::value::js_nanbox_pointer(decl_proto as i64).to_bits());
    for _ in 0..32 {
        let bits = link?;
        if bits == crate::value::TAG_NULL {
            return None;
        }
        let value = f64::from_bits(bits);
        if crate::proxy::js_proxy_is_proxy(value) != 0 {
            return super::super::prototype_chain::resolve_inherited_field_from_prototype(
                decl_proto as usize,
                bits,
                key,
            );
        }
        let addr = match bits >> 48 {
            0x7FFD => (bits & crate::value::POINTER_MASK) as usize,
            0 if crate::value::addr_class::is_above_handle_band(bits as usize) => bits as usize,
            _ => return None,
        };
        let Some(header) = crate::value::addr_class::try_read_gc_header(addr) else {
            return None;
        };
        if header.obj_type != crate::gc::GC_TYPE_OBJECT {
            return None;
        }
        let proto = addr as *mut ObjectHeader;
        if let Some(value) = inherited_proto_accessor_value(proto, key, receiver) {
            return Some(value);
        }
        if let Some(value) = super::super::field_get_set::own_data_field_by_name(proto, key) {
            return Some(value);
        }
        link = super::super::prototype_chain::object_static_prototype(addr);
    }
    None
}

/// `constructor_side`: this walk serves a read on the class CONSTRUCTOR, so a
/// name that is a declared INSTANCE member must not resolve through it.
///
/// #9404: declared instance methods are MIRRORED onto the reflective
/// `C.prototype` object as own data fields (see the `own_data_field_by_name`
/// arm below). That is right for an instance read, which is what the
/// `_with_receiver` form serves. It is wrong for the CONSTRUCTOR-side read the
/// receiver-less form serves: in JS a class object does not expose its
/// prototype methods as statics — `class C { m(){} }` has `C.m === undefined`,
/// because `m` lives on `C.prototype`. Walking into the decl-prototype from a
/// static lookup made every instance method resolve on the class ref, so
/// `typeof C.m` answered "function", `C.m === C.prototype.m` was true, and a
/// static body's `typeof this.m` answered "function" where node says
/// "undefined" — the surviving half of #9404 after the codegen predicate was
/// made honest. `js_object_has_property` already had this gate (`"m" in C` was
/// correctly false), and the sibling `is_prototype_ref` gate two hundred lines
/// up in `get_field_by_name.rs` plugged the same hole on the direct-vtable
/// door for #1021/NestJS; this is that door's chain-walk twin.
///
/// The exclusion is keyed on `class_instance_has_member` — the exact
/// "is this a prototype method / getter / setter of the chain" predicate —
/// plus the `constructor` back-edge, and NOT on "skip the decl-prototype
/// entirely": a blanket skip would also hide a user's own
/// `Object.defineProperty(C.prototype, ...)` data field from this walk.
///
/// #9467: `constructor` is the one decl-prototype data field that is NOT an
/// instance member by `class_instance_has_member`'s definition, and walking
/// into it from the constructor side answered `C.constructor === C`. Node says
/// `C.constructor === Function` — a constructor object's own chain is
/// `C → Function.prototype`, with no back-edge to `C`; only
/// `C.prototype.constructor` and `(new C()).constructor` are `C`. The
/// `Function` answer already existed as the tail fallback of the class-ref arm
/// in `get_field_by_name.rs`; this walk simply returned first. The wrong
/// answer used to be load-bearing: perry handed a decorator on an INSTANCE
/// member the class itself where the spec hands it `Class.prototype`, so
/// NestJS-style `Reflect.defineMetadata(k, v, target.constructor)` landed on
/// `C` only because `C.constructor === C`. Two divergences that cancelled;
/// fixing either alone broke decorator metadata
/// (`test_decorators_nest_common_canary`,
/// `test_decorators_legacy_property_metadata`). Both halves were fixed
/// together — the decorator target in
/// `perry-hir/src/lower/decorators.rs::member_decorator_target` and the
/// `constructor` clause below; `test_decorators_target_prototype_9467` pins
/// the pair.
///
/// The `class_prototype_object` step below is never skipped: for a subclass of
/// a class-EXPRESSION value it holds the parent CLASS OBJECT (#1788/#6552),
/// which is genuinely on the constructor's static chain.
unsafe fn resolve_proto_chain_field_inner(
    class_id: u32,
    key: *const crate::StringHeader,
    receiver: Option<f64>,
    constructor_side: bool,
) -> Option<JSValue> {
    if let Some(receiver) = receiver {
        let receiver_value = JSValue::from_bits(receiver.to_bits());
        if receiver_value.is_pointer() {
            let receiver_obj = receiver_value.as_pointer::<ObjectHeader>();
            if !receiver_obj.is_null() {
                if let Some(pin) = instance_pinned_constructing_class(receiver_obj) {
                    // A factory-created class can be evaluated again after
                    // this instance was built. Its template class id then
                    // points at the later evaluation. Resolve through this
                    // instance's pinned class object and its own prototype.
                    let scope = crate::gc::RuntimeHandleScope::new();
                    let receiver = scope.root_nanbox_f64(receiver);
                    let pin = scope.root_nanbox_f64(pin);
                    let key = scope.root_string_ptr(key as *mut crate::StringHeader);
                    let pin_obj = JSValue::from_bits(pin.get_nanbox_f64().to_bits())
                        .as_pointer::<ObjectHeader>();
                    let proto_value =
                        super::super::field_get_set::class_object_prototype_value(pin_obj);
                    let proto = JSValue::from_bits(proto_value.bits()).as_pointer::<ObjectHeader>();
                    if !proto.is_null() {
                        let proto = scope.root_raw_mut_ptr(proto as *mut ObjectHeader);
                        if let Some(value) = proto.with_mut_ptr::<ObjectHeader, _>(|proto| {
                            key.with_const_ptr::<crate::StringHeader, _>(|key| {
                                evaluated_parent_instance_field(
                                    proto,
                                    key,
                                    receiver.get_nanbox_f64(),
                                )
                            })
                        }) {
                            return Some(value);
                        }
                    }
                }
            }
        }
    }
    // Resolved once: `class_instance_has_member` already walks the parent
    // chain, so a parent's instance method is excluded from a subclass's
    // constructor read too.
    let skip_decl_prototype = constructor_side && !key.is_null() && {
        let key_ptr = crate::string::string_data(key);
        let key_len = (*key).byte_len as usize;
        std::str::from_utf8(std::slice::from_raw_parts(key_ptr, key_len))
            .map(|name| {
                // #9467: `C.prototype.constructor` is `C`; `C.constructor` is
                // `Function` (the class-ref arm's tail fallback), never `C`.
                name == "constructor" || crate::object::class_instance_has_member(class_id, name)
            })
            .unwrap_or(false)
    };
    let mut cid = class_id;
    let mut depth = 0usize;
    while depth < 32 {
        // The reflective `ClassName.prototype` object
        // (`CLASS_DECL_PROTOTYPE_OBJECTS`) is where a user
        // `Object.defineProperty(ClassName.prototype, name, { get })` installs
        // its accessor — distinct from the #711/#809 synthetic-proto cache
        // (`CLASS_PROTOTYPE_OBJECTS`) that the rest of this walk reads. The
        // instance-read walk historically only consulted the latter, so such a
        // getter was invisible to `instance.name` (winston:
        // `Object.defineProperty(Logger.prototype, 'transports', { get })`,
        // read as `this.transports`, came back `undefined` → `.length` threw).
        // Decl-proto object (`CLASS_DECL_PROTOTYPE_OBJECTS`) is where a user
        // `Object.defineProperty(ClassName.prototype, name, { get })` installs
        // its accessor AND where a runtime `C.prototype[k] = v` (computed /
        // dynamic index write) stores an OWN data field. It is allocated WITH
        // this `class_id` (`js_object_alloc(class_id, 0)`), so routing DATA
        // reads back through `js_object_get_field_by_name` would re-enter this
        // same walk for the same id and recurse infinitely (a Transform
        // subclass's `_read` lookup stack-overflowed → SIGSEGV).
        //
        // Accessors: fire with the instance as receiver.
        // Own data fields: read via `own_data_field_by_name` only (no class-id
        // re-walk) so a computed prototype write is visible as
        // `(new C()).name` (#6945). Class methods / vtable data still come
        // from the vtable + `class_prototype_object` path below.
        let decl_proto = if skip_decl_prototype {
            std::ptr::null_mut()
        } else {
            class_decl_prototype_object(cid)
        };
        if !decl_proto.is_null() {
            if let Some(receiver) = receiver {
                if let Some(value) = inherited_proto_accessor_value(decl_proto, key, receiver) {
                    return Some(value);
                }
            }
            // #6945: own data property written onto the reflective prototype
            // (computed / dynamic-key set) — never re-enter by-name get.
            if let Some(value) =
                unsafe { super::super::field_get_set::own_data_field_by_name(decl_proto, key) }
            {
                if !value.is_undefined() {
                    // Declared methods are mirrored onto the template's shared
                    // reflective prototype. For a ClassExprFresh instance,
                    // substitute the method closure owned by this particular
                    // class evaluation; otherwise separate factory calls share
                    // a lexical private brand and cross-calls incorrectly pass.
                    if let Some(receiver) = receiver {
                        let key_ptr = crate::string::string_data(key);
                        let key_len = (*key).byte_len as usize;
                        if let Ok(name) =
                            std::str::from_utf8(std::slice::from_raw_parts(key_ptr, key_len))
                        {
                            let name = name.to_string();
                            let scope = crate::gc::RuntimeHandleScope::new();
                            let value = scope.root_nanbox_u64(value.bits());
                            let receiver = scope.root_heap_word_u64(receiver.to_bits());
                            if super::super::native_module::class_has_own_method(cid, &name)
                                && value.get_nanbox_u64()
                                    == super::super::native_module::class_prototype_method_value_for_name(
                                        cid, &name,
                                    )
                                    .to_bits()
                            {
                                let receiver = f64::from_bits(receiver.get_heap_word_u64());
                                if let Some(brand) =
                                    super::super::private_evaluation_brand_value(receiver)
                                {
                                    let brand_obj = crate::value::JSValue::from_bits(brand.to_bits())
                                        .as_pointer::<ObjectHeader>();
                                    if !brand_obj.is_null()
                                        && js_object_get_class_id(brand_obj) == cid
                                    {
                                        let method = super::super::native_module::class_evaluation_method_value_for_name(
                                            cid, &name, brand,
                                        );
                                        return Some(JSValue::from_bits(method.to_bits()));
                                    }
                                }
                            }
                            return Some(JSValue::from_bits(value.get_nanbox_u64()));
                        }
                    }
                    return Some(value);
                }
            }
        }
        let mut proto_obj = class_prototype_object(cid);
        if let Some(receiver) = receiver {
            if let Some(parent_class) = declared_parent_class_object(cid, proto_obj) {
                // #10890: for a DECLARED class id this entry is the parent
                // CLASS OBJECT (#1788), which is on the constructor's static
                // chain and never on an instance's. Reading it for an instance
                // returned the parent's statics, and `name` returned the
                // class binding's own name (Effect's `out` / `Base`) instead
                // of the `name` written to that class's `prototype`. An
                // instance read goes to that evaluation's prototype object.
                if let Some(evaluated) =
                    super::super::field_get_set::class_object_materialized_prototype(parent_class)
                {
                    if let Some(value) = evaluated_parent_instance_field(evaluated, key, receiver) {
                        return Some(value);
                    }
                }
                proto_obj = std::ptr::null_mut();
            }
        }
        if !proto_obj.is_null() {
            if let Some(receiver) = receiver {
                if let Some(value) = inherited_proto_accessor_value(proto_obj, key, receiver) {
                    return Some(value);
                }
            }
            let field_val = if let Some(receiver) = receiver {
                let this_scope = crate::gc::RuntimeHandleScope::new(); // #9445
                let previous_this = this_scope.root_nanbox_f64(js_implicit_this_set(receiver));
                // The recursive `get_field(proto_obj, key)` re-derives a class
                // getter's `this` from `proto_obj`; stash the real instance so an
                // inherited getter (object-literal `get x()` on an
                // `Object.create(proto)` prototype) binds `this` to the instance.
                let prev_override =
                    super::super::field_get_set::accessor_receiver_override_begin(receiver);
                let value = js_object_get_field_by_name(proto_obj as *const _, key);
                super::super::field_get_set::accessor_receiver_override_end(prev_override);
                js_implicit_this_set(previous_this.get_nanbox_f64());
                value
            } else {
                js_object_get_field_by_name(proto_obj as *const _, key)
            };
            if !field_val.is_undefined() && !field_val.is_null() {
                return Some(field_val);
            }
        }
        match get_parent_class_id(cid) {
            Some(p) if p != 0 && p != cid => {
                cid = p;
                depth += 1;
            }
            _ => break,
        }
    }
    None
}

/// #1758: symbol-keyed analogue of [`resolve_proto_chain_field`]. Walks the
/// `CLASS_PROTOTYPE_OBJECTS` chain and, at each prototype object (a POINTER
/// class-object), looks up its OWN symbol property via `own_symbol_property`.
/// Lets a subclass whose parent is a class-expression value inherit the
/// parent's static *symbol* statics — e.g. effect's
/// `class BigIntFromSelf extends make(bigIntKeyword) {}` inheriting
/// `static [TypeId]`, which `Predicate.hasProperty(.., TypeId)` (`isSchema`)
/// and `u[TypeId]` both read. Returns the first defined value found.
///
/// #26 / #321: the walk must advance along TWO axes, because a synthetic
/// `Object.create(proto)` class id links to its prototype via the *proto
/// object's own class id*, not via `parent_class_id` (which only models the
/// `class A extends B` axis). effect's `Either.right(x)` builds
/// `Object.create(RightProto)` where `RightProto = Object.create(CommonProto)`
/// and `CommonProto[TypeId]` carries the brand. With only the
/// `parent_class_id` axis the walk stopped after the first prototype object
/// (`RightProto`), so `TypeId in either` / `either[TypeId]` missed the brand
/// two links up — making `ParseResult.isEither(...)` false for every struct
/// property parse (`S.is`/`decodeUnknownSync`/`encodeSync` on a `Struct`).
/// At each node we follow the proto object's own class id (the
/// `Object.create` prototype link) first, then fall back to
/// `parent_class_id` (the `extends` link); a `visited` set bounds cycles.
///
/// #10481: an accessor found on a prototype object runs with `this ===
/// receiver`, the object the read started from — never the prototype object
/// that holds it.
pub(crate) unsafe fn resolve_proto_chain_symbol(
    class_id: u32,
    sym_f64: f64,
    receiver: f64,
) -> Option<f64> {
    proto_chain_symbol_slot(class_id, sym_f64).map(|slot| slot.read(receiver))
}

/// The walk behind [`resolve_proto_chain_symbol`], stopping at the nearest
/// prototype object that owns `sym_f64` without invoking an accessor there.
pub(crate) unsafe fn proto_chain_symbol_slot(
    class_id: u32,
    sym_f64: f64,
) -> Option<crate::symbol::OwnSymbolSlot> {
    proto_chain_symbol_slot_inner(class_id, sym_f64, false)
}

/// [`proto_chain_symbol_slot`] for a read on `obj` itself. A class object
/// inherits its parent class object's statics. Any other object (an instance
/// or a prototype) reads that parent's evaluated prototype instead (#10890).
pub(crate) unsafe fn object_proto_chain_symbol_slot(
    obj: *const ObjectHeader,
    sym_f64: f64,
) -> Option<crate::symbol::OwnSymbolSlot> {
    let class_id = crate::object::js_object_get_class_id(obj);
    if class_id == 0 {
        return None;
    }
    let instance = !is_class_object_ptr(obj as *const u8);
    proto_chain_symbol_slot_inner(class_id, sym_f64, instance)
}

unsafe fn proto_chain_symbol_slot_inner(
    class_id: u32,
    sym_f64: f64,
    instance: bool,
) -> Option<crate::symbol::OwnSymbolSlot> {
    let mut cid = class_id;
    let mut depth = 0usize;
    let mut visited: [u32; 32] = [0; 32];
    while depth < 32 {
        if visited[..depth].contains(&cid) {
            break;
        }
        visited[depth] = cid;
        let mut proto_obj = class_prototype_object(cid);
        let mut next_cid: u32 = 0;
        if instance {
            if let Some(parent_class) = declared_parent_class_object(cid, proto_obj) {
                if let Some(evaluated) =
                    super::super::field_get_set::class_object_materialized_prototype(parent_class)
                {
                    let evaluated = f64::from_bits(JSValue::pointer(evaluated as *const u8).bits());
                    if let Some(slot) = crate::symbol::own_symbol_slot(evaluated, sym_f64) {
                        return Some(slot);
                    }
                }
                // The `extends` axis below reaches the parent's template.
                proto_obj = std::ptr::null_mut();
            }
        }
        if !proto_obj.is_null() {
            let proto_f64 = f64::from_bits(JSValue::pointer(proto_obj as *const u8).bits());
            // OWN lookup only — this fn IS the chain walk, so recursing into
            // the full chain-walking getter would re-walk per prototype.
            if let Some(slot) = crate::symbol::own_symbol_slot(proto_f64, sym_f64) {
                return Some(slot);
            }
            // Prefer the `Object.create` prototype link: the next chain node
            // is the proto object's own class id (which maps to ITS proto in
            // CLASS_PROTOTYPE_OBJECTS). Falls back to `parent_class_id` below.
            next_cid = crate::object::js_object_get_class_id(proto_obj as *const ObjectHeader);
        }
        if next_cid != 0 && next_cid != cid {
            cid = next_cid;
            depth += 1;
            continue;
        }
        match get_parent_class_id(cid) {
            Some(p) if p != 0 && p != cid => {
                cid = p;
                depth += 1;
            }
            _ => break,
        }
    }
    None
}

/// Lookup the synthetic class id for a function value, if one was
/// registered via `js_set_function_prototype`.
#[inline]
pub(crate) fn function_class_id(value: f64) -> u32 {
    let bits = value.to_bits();
    FUNCTION_CLASS_IDS.with(|table| {
        if let Ok(read) = table.read() {
            if let Some(map) = read.as_ref() {
                return map.get(&bits).copied().unwrap_or(0);
            }
        }
        0
    })
}

pub(crate) fn function_value_for_class_id(class_id: u32) -> Option<f64> {
    if class_id == 0 {
        return None;
    }
    FUNCTION_CLASS_IDS.with(|table| {
        table.read().ok().and_then(|guard| {
            guard.as_ref().and_then(|map| {
                map.iter()
                    .find_map(|(&bits, &cid)| (cid == class_id).then_some(f64::from_bits(bits)))
            })
        })
    })
}

/// #5477: when `func_value` is the bound `events.EventEmitter` /
/// `EventEmitterAsyncResource` export, its synthetic prototype must carry the
/// EventEmitter methods (the `Object.setPrototypeOf(x, EventEmitter.prototype)`
/// mixin pattern — pino). Extracted verbatim; reached ONLY through
/// `NmEeOps::ee_prototype_install`.
pub(crate) unsafe fn nm_ee_prototype_install(
    func_value: f64,
    proto: *mut crate::object::ObjectHeader,
) {
    let scope = crate::gc::RuntimeHandleScope::new();
    let func_value = scope.root_nanbox_f64(func_value);
    let proto = scope.root_raw_mut_ptr(proto);
    if let Some((module, method)) =
        super::super::native_module::bound_native_callable_module_and_method(
            func_value.get_nanbox_f64(),
        )
    {
        if module.trim_start_matches("node:") == "events"
            && matches!(
                method.as_str(),
                "EventEmitter" | "EventEmitterAsyncResource"
            )
        {
            proto.with_mut_ptr::<crate::object::ObjectHeader, _>(|proto| {
                crate::node_stream::install_event_emitter_prototype_methods(proto);
                if method == "EventEmitterAsyncResource" {
                    crate::node_stream::install_event_emitter_async_resource_prototype(proto);
                }
            });
        }
    }
}
