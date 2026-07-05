use super::*;
use crate::object::*;
use crate::{ArrayHeader, JSValue};
use std::cell::{Cell, RefCell, UnsafeCell};
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicI64, AtomicU64, AtomicU8, Ordering};
use std::sync::RwLock;

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

    let proto = js_object_alloc(0, 0);
    if proto.is_null() {
        return proto;
    }

    let constructor_key =
        crate::string::js_string_from_bytes(b"constructor".as_ptr(), "constructor".len() as u32);
    js_object_set_field_by_name(proto, constructor_key, func_value);
    set_builtin_property_attrs(
        proto as usize,
        "constructor".to_string(),
        PropertyAttrs::new(true, false, true),
    );

    if let Some(object_proto_bits) = global_object_prototype_bits() {
        super::super::prototype_chain::object_set_static_prototype(
            proto as usize,
            object_proto_bits,
        );
    }

    class_prototype_object_root_store(class_id, proto);

    // #5024: methods registered before the prototype object materialized
    // (`F.prototype.m = v` typically runs long before any reflective
    // `F.prototype` read) live only in CLASS_PROTOTYPE_METHODS. Backfill
    // them as ordinary own properties so enumeration sees them; later
    // registrations write through via class_prototype_method_root_store.
    let registered: Vec<(String, u64)> = {
        let guard = CLASS_PROTOTYPE_METHODS.read().unwrap();
        guard
            .as_ref()
            .and_then(|map| map.get(&class_id))
            .map(|per_class| per_class.iter().map(|(k, &v)| (k.clone(), v)).collect())
            .unwrap_or_default()
    };
    for (name, value_bits) in registered {
        let enumerable = class_prototype_method_is_enumerable(class_id, &name);
        unsafe { mirror_prototype_method_on_object(proto, &name, value_bits, enumerable) };
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
    if let Some((module, method)) =
        unsafe { super::super::native_module::bound_native_callable_module_and_method(func_value) }
    {
        if module.trim_start_matches("node:") == "events"
            && matches!(
                method.as_str(),
                "EventEmitter" | "EventEmitterAsyncResource"
            )
        {
            crate::node_stream::install_event_emitter_prototype_methods(proto);
        }
    }

    let func_bits = func_value.to_bits();
    if (func_bits >> 48) == 0x7FFD {
        let func_ptr = (func_bits & crate::value::POINTER_MASK) as usize;
        if func_ptr != 0 {
            crate::closure::closure_set_dynamic_prop(
                func_ptr,
                "prototype",
                crate::value::js_nanbox_pointer(proto as i64),
            );
            set_builtin_property_attrs(
                func_ptr,
                "prototype".to_string(),
                PropertyAttrs::new(true, false, false),
            );
        }
    }

    proto
}

/// Synthetic class id allocator for prototype-object classes. High bit
/// set (0x8000_0000+) to keep them separate from codegen-assigned ids
/// (which start from 1 and grow by module). u32 wraparound is not a
/// concern in practice — would require ~2 billion `Function.prototype = X`
/// statements at module init.
pub static NEXT_SYNTHETIC_CLASS_ID: std::sync::atomic::AtomicU32 =
    std::sync::atomic::AtomicU32::new(0x8000_0000);

/// Register a function's prototype object. Called by codegen-emitted
/// init code whenever the HIR detects `<expr>.prototype = <expr>` at
/// the assignment-statement level (lower_expr_assignment Member arm).
///
/// Returns the synthetic class_id allocated for this function (0 if
/// validation fails). The synthetic id is folded into CLASS_REGISTRY
/// when a class extends `func` via the #711 dynamic-parent path.
#[no_mangle]
pub extern "C" fn js_set_function_prototype(func: f64, proto: f64) -> u32 {
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
    {
        let read = FUNCTION_CLASS_IDS.read().unwrap();
        if let Some(map) = read.as_ref() {
            if let Some(&existing) = map.get(&func_bits) {
                // Update the prototype object (allow re-pointing)
                // without changing the class_id.
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
        }
    }
    let new_cid = NEXT_SYNTHETIC_CLASS_ID.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    {
        let mut write = FUNCTION_CLASS_IDS.write().unwrap();
        if write.is_none() {
            *write = Some(HashMap::new());
        }
        write.as_mut().unwrap().insert(func_bits, new_cid);
    }
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
    if let Ok(read) = CLASS_PROTOTYPE_OBJECTS.read() {
        if let Some(map) = read.as_ref() {
            return map.get(&class_id).copied().unwrap_or(0) as *mut ObjectHeader;
        }
    }
    std::ptr::null_mut()
}

/// #711 / #809: resolve `key` by walking the synthetic-class-id prototype
/// chain (`CLASS_PROTOTYPE_OBJECTS`), recursing into each prototype object
/// as a normal field lookup. Used both when a receiver's own keys miss AND
/// when it has no `keys_array` at all (an `Object.create(proto)` result, or
/// a `Function.prototype = obj` instance with no own props). Returns the
/// first defined, non-null field found on the chain.
pub(crate) unsafe fn resolve_proto_chain_field(
    class_id: u32,
    key: *const crate::StringHeader,
) -> Option<JSValue> {
    resolve_proto_chain_field_inner(class_id, key, None)
}

pub(crate) unsafe fn resolve_proto_chain_field_with_receiver(
    class_id: u32,
    key: *const crate::StringHeader,
    receiver: f64,
) -> Option<JSValue> {
    resolve_proto_chain_field_inner(class_id, key, Some(receiver))
}

unsafe fn inherited_proto_accessor_value(
    proto_obj: *mut ObjectHeader,
    key: *const crate::StringHeader,
    receiver: f64,
) -> Option<JSValue> {
    if key.is_null() || !ACCESSORS_IN_USE.with(|c| c.get()) {
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

unsafe fn resolve_proto_chain_field_inner(
    class_id: u32,
    key: *const crate::StringHeader,
    receiver: Option<f64>,
) -> Option<JSValue> {
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
        // Check the decl-proto object for an ACCESSOR only: it is allocated
        // WITH this `class_id` (`js_object_alloc(class_id, 0)`), so routing its
        // DATA reads back through `js_object_get_field_by_name` would re-enter
        // this same walk for the same id and recurse infinitely (a Transform
        // subclass's `_read` lookup stack-overflowed → SIGSEGV). Class methods /
        // data are already covered by the vtable + `class_prototype_object`
        // path below, so the accessor-only probe here is sufficient.
        if let Some(receiver) = receiver {
            let decl_proto = class_decl_prototype_object(cid);
            if !decl_proto.is_null() {
                if let Some(value) = inherited_proto_accessor_value(decl_proto, key, receiver) {
                    return Some(value);
                }
            }
        }
        let proto_obj = class_prototype_object(cid);
        if !proto_obj.is_null() {
            if let Some(receiver) = receiver {
                if let Some(value) = inherited_proto_accessor_value(proto_obj, key, receiver) {
                    return Some(value);
                }
            }
            let field_val = if let Some(receiver) = receiver {
                let previous_this = js_implicit_this_set(receiver);
                // The recursive `get_field(proto_obj, key)` re-derives a class
                // getter's `this` from `proto_obj`; stash the real instance so an
                // inherited getter (object-literal `get x()` on an
                // `Object.create(proto)` prototype) binds `this` to the instance.
                let prev_override =
                    super::super::field_get_set::accessor_receiver_override_begin(receiver);
                let value = js_object_get_field_by_name(proto_obj as *const _, key);
                super::super::field_get_set::accessor_receiver_override_end(prev_override);
                js_implicit_this_set(previous_this);
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
pub(crate) unsafe fn resolve_proto_chain_symbol(class_id: u32, sym_f64: f64) -> Option<f64> {
    let mut cid = class_id;
    let mut depth = 0usize;
    let mut visited: [u32; 32] = [0; 32];
    while depth < 32 {
        if visited[..depth].contains(&cid) {
            break;
        }
        visited[depth] = cid;
        let proto_obj = class_prototype_object(cid);
        let mut next_cid: u32 = 0;
        if !proto_obj.is_null() {
            let proto_f64 = f64::from_bits(JSValue::pointer(proto_obj as *const u8).bits());
            // OWN lookup only — this fn IS the chain walk, so recursing into
            // the full chain-walking getter would re-walk per prototype.
            if let Some(v) = crate::symbol::own_symbol_property(proto_f64, sym_f64) {
                return Some(v);
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
    if let Ok(read) = FUNCTION_CLASS_IDS.read() {
        if let Some(map) = read.as_ref() {
            return map.get(&bits).copied().unwrap_or(0);
        }
    }
    0
}

pub(crate) fn function_value_for_class_id(class_id: u32) -> Option<f64> {
    if class_id == 0 {
        return None;
    }
    FUNCTION_CLASS_IDS.read().ok().and_then(|guard| {
        guard.as_ref().and_then(|map| {
            map.iter()
                .find_map(|(&bits, &cid)| (cid == class_id).then_some(f64::from_bits(bits)))
        })
    })
}
