//! `Object.create`, `Object.getPrototypeOf`, and the globalThis-builtin lookup.
use super::super::*;
use super::*;

/// Look up the canonical NaN-boxed value of a built-in constructor /
/// namespace stored on `globalThis` (the singleton populated by
/// `populate_global_this_builtins`). Used by `instance.constructor`
/// reads and by bare `Date`/`Array`/`Object` identifier resolution so
/// both forms produce the same closure-pointer value — that's what
/// `instance.constructor === Date` (date-fns's `constructFrom`,
/// drizzle's `is(value, ctor)` duck checks, ...) hinges on.
///
/// Returns NaN-boxed undefined if the name isn't one of the populated
/// built-ins or the singleton hasn't been initialized yet.
#[no_mangle]
pub extern "C" fn js_get_global_this_builtin_value(name_ptr: *const u8, name_len: usize) -> f64 {
    if name_ptr.is_null() || name_len == 0 {
        return f64::from_bits(crate::value::TAG_UNDEFINED);
    }
    let name_bytes = unsafe { std::slice::from_raw_parts(name_ptr, name_len) };
    let name = match std::str::from_utf8(name_bytes) {
        Ok(s) => s,
        Err(_) => return f64::from_bits(crate::value::TAG_UNDEFINED),
    };
    // Force the singleton init the first time so the lookup below has
    // a populated field bag.
    let global_this_f64 = js_get_global_this();
    let global_obj = crate::value::js_nanbox_get_pointer(global_this_f64) as *const ObjectHeader;
    if global_obj.is_null() {
        return f64::from_bits(crate::value::TAG_UNDEFINED);
    }
    let key = crate::string::js_string_from_bytes(name.as_ptr(), name.len() as u32);
    let value = js_object_get_field_by_name(global_obj, key);
    let bits = value.bits();
    f64::from_bits(bits)
}

/// Object.create(proto) — create empty object. Perry ignores prototype; Object.create(null) returns {}.
#[no_mangle]
pub extern "C" fn js_object_create(proto_value: f64) -> f64 {
    // #809: actually wire up the prototype. Pre-fix this ignored its
    // argument entirely, so `Object.create(Proto)` returned a bare empty
    // object — `inst.method()` / `inst.prop` saw nothing and threw
    // `TypeError: <m> is not a function`. Reuse the #711 prototype-object
    // machinery: allocate a synthetic class_id, map it to `proto` in
    // CLASS_PROTOTYPE_OBJECTS, and stamp the new object with that id. The
    // chain walk in `js_object_get_field_by_name` (the `class_id != 0`
    // branch) then resolves missing own props/methods off `proto`.
    //
    // `Object.create(null)` (or a non-object proto / a builtin-backed
    // Set/Map/Regex source Perry can't model as a prototype) falls back
    // to the original behavior: a plain prototype-less object.
    const POINTER_TAG: u64 = 0x7FFD_0000_0000_0000;

    // `Object.create(proxy)` — a Proxy is a small registered id, not a real
    // heap pointer, so the synthetic-class-id modeling below (which stores a
    // REAL prototype pointer) can't represent it, and the `is_valid_obj_ptr`
    // check would reject it outright (falling back to a plain, prototype-less
    // object — wrong: reads/writes/`in` on the result must still route through
    // the proxy). Record it in the SAME observable `[[Prototype]]` side table
    // `Object.setPrototypeOf` uses instead: a plain (class_id 0, non-null-proto)
    // object whose prototype hop the generic chain walks (`ordinary_has_property`,
    // `own_set_descriptor`'s `prototype_of_for_set`, field-get) already resolve
    // through the proxy's traps. (test262 has/call-in-prototype.js,
    // has/call-object-create.js, set/call-parameters-prototype.js.)
    if crate::proxy::js_proxy_is_proxy(proto_value) != 0 {
        let obj = js_object_alloc(0, 0);
        crate::object::prototype_chain::object_set_static_prototype(
            obj as usize,
            proto_value.to_bits(),
        );
        return f64::from_bits((obj as u64) | POINTER_TAG);
    }

    let mut class_id: u32 = 0;
    let proto_bits = proto_value.to_bits();
    if (proto_bits & 0xFFFF_0000_0000_0000) == POINTER_TAG {
        let proto_ptr = crate::value::js_nanbox_get_pointer(proto_value) as *mut ObjectHeader;
        if !proto_ptr.is_null() && (proto_ptr as usize) > 0x10000 {
            let proto_addr = proto_ptr as usize;
            let modellable = !(crate::set::is_registered_set(proto_addr)
                || crate::map::is_registered_map(proto_addr)
                || crate::regex::is_regex_pointer(proto_ptr as *const u8));
            let valid = modellable && is_valid_obj_ptr(proto_ptr as *const u8);
            if valid {
                let cid =
                    NEXT_SYNTHETIC_CLASS_ID.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                class_prototype_object_root_store(cid, proto_ptr);
                unsafe { js_register_class_id(cid) };
                // #1805: link the synthetic class_id into the original class's
                // inheritance chain. `Object.getPrototypeOf(instance)` returns
                // the instance pointer itself in Perry's model (see
                // `js_object_get_prototype_of`), so `proto_ptr` here is a real
                // class instance whose `class_id` field IS the user class's
                // id. Registering it as the synthetic cid's parent lets
                // `js_instanceof`'s `get_parent_class_id` walk reach the
                // original class and match — without this, the chain stopped
                // at the unregistered synthetic id and `Object.create(proto)
                // instanceof C` was always false even though property /
                // getter dispatch through the chain worked correctly.
                let parent_class_id = unsafe { (*proto_ptr).class_id };
                if parent_class_id != 0 && parent_class_id != cid {
                    register_class(cid, parent_class_id);
                }
                class_id = cid;
            }
        }
    }
    // #1175: when `proto_value` is null/undefined/non-object, the resulting
    // object has no [[Prototype]]. Stamp OBJ_FLAG_NULL_PROTO so
    // `Object.getPrototypeOf(Object.create(null))` returns null (it
    // previously returned the object itself).
    let null_proto = class_id == 0;
    let obj = if null_proto {
        js_object_alloc_null_proto(class_id, 0)
    } else {
        js_object_alloc(class_id, 0)
    };
    // Return NaN-boxed pointer
    f64::from_bits((obj as u64) | 0x7FFD_0000_0000_0000)
}

/// Object.getPrototypeOf(obj):
/// - For an INT32-tagged class ref (top16 == 0x7FFE) — return the parent
///   class ref via CLASS_REGISTRY's parent_class_id chain, or null at
///   the root. Drizzle's `is(value, type)` chain walks this.
/// - For an object instance with a registered class_id — return the
///   class ref. Conceptually JS returns `Class.prototype`; Perry doesn't
///   maintain prototype objects, but drizzle's chain consumes
///   `Object.getPrototypeOf(value).constructor`, and class_ref's
///   `.constructor` synthesizes back to the same class ref via the
///   constructor intercept (v0.5.746). So returning the class ref here
///   makes that chain produce `value.constructor` as Node would.
/// - Other receivers — null.
/// Refs #420 / #618 followup.
#[no_mangle]
pub extern "C" fn js_object_get_prototype_of(obj_value: f64) -> f64 {
    const TAG_NULL: u64 = 0x7FFC_0000_0000_0002;
    // #2820: `Object.getPrototypeOf(null | undefined)` throws TypeError
    // (`Cannot convert undefined or null to object`). Class refs and heap
    // objects fall through to the existing resolution below.
    {
        let jv = crate::value::JSValue::from_bits(obj_value.to_bits());
        if jv.is_null() || jv.is_undefined() {
            throw_object_type_error(b"Cannot convert undefined or null to object");
        }
    }
    // A Proxy is a small registered id, NOT a heap object — the handle path
    // below would mis-read it and return `null`. Route it to the proxy
    // `[[GetPrototypeOf]]` (handler trap, else the target's prototype) so
    // `Object.getPrototypeOf(proxy)` matches the target. drizzle aliases columns
    // as `new Proxy(column, …)` and `is(value, type)` reads
    // `getPrototypeOf(value).constructor`, which crashed on `null.constructor`.
    if crate::proxy::js_proxy_is_proxy(obj_value) != 0 {
        return crate::proxy::js_proxy_get_prototype_of(obj_value);
    }
    // A Temporal value is a NaN-boxed opaque cell, not an `ObjectHeader` — the
    // heap-object resolution below would deref its boxed payload as a class id
    // and crash. Its reflective prototype IS `Temporal.<Type>.prototype`, and
    // `assert.sameValue(Object.getPrototypeOf(result), construct.prototype)`
    // (the test262 subclassing-ignored shape) requires that object, not `null`.
    // Resolve it via the live namespace; fall back to `null` only if Temporal
    // isn't reachable. (#5587)
    #[cfg(feature = "temporal")]
    if crate::temporal::is_temporal_value(obj_value) {
        if let Some(kind) = crate::temporal::temporal_kind(obj_value) {
            let proto = crate::object::global_this::temporal_kind_prototype(kind);
            if crate::value::JSValue::from_bits(proto.to_bits()).is_pointer() {
                return proto;
            }
        }
        return f64::from_bits(TAG_NULL);
    }
    // ES2015 ToObject(primitive): `Object.getPrototypeOf(0 | "s" | true |
    // 1n | sym)` resolves to the wrapper class prototype, not a TypeError /
    // null (15.2.3.2-1*).
    {
        let jv = crate::value::JSValue::from_bits(obj_value.to_bits());
        // An INT32-tagged value may be a class ref (same 0x7FFE tag as small
        // integers) — those must keep flowing to the class resolution below.
        let is_class_ref = (obj_value.to_bits() >> 48) == 0x7FFE
            && super::super::class_ref_id(obj_value).is_some();
        let wrapper = if is_class_ref {
            None
        } else if jv.is_number() {
            Some("Number")
        } else if jv.is_any_string() {
            Some("String")
        } else if jv.is_bool() {
            Some("Boolean")
        } else if jv.is_bigint() {
            Some("BigInt")
        } else if unsafe { crate::symbol::js_is_symbol(obj_value) } != 0 {
            Some("Symbol")
        } else {
            None
        };
        if let Some(name) = wrapper {
            let proto = crate::object::builtin_prototype_value(name);
            if proto.to_bits() != crate::value::TAG_UNDEFINED {
                return proto;
            }
            return f64::from_bits(TAG_NULL);
        }
    }
    let bits = obj_value.to_bits();
    let top16 = bits >> 48;
    if top16 == 0x7FFD {
        let raw_addr = bits & 0x0000_FFFF_FFFF_FFFF;
        if crate::value::addr_class::is_small_handle(raw_addr as usize) {
            if let Some(dispatch) = super::super::class_registry::handle_prototype_dispatch() {
                let proto = unsafe { dispatch(raw_addr as i64) };
                if proto.to_bits() != crate::value::TAG_UNDEFINED {
                    return proto;
                }
            }
            return f64::from_bits(TAG_NULL);
        }
    }
    let collection_prototype = |addr: usize| -> Option<f64> {
        if crate::map::is_registered_map(addr) {
            let proto = crate::object::builtin_prototype_value("Map");
            if proto.to_bits() != crate::value::TAG_UNDEFINED {
                return Some(proto);
            }
        }
        if crate::set::is_registered_set(addr) {
            let proto = crate::object::builtin_prototype_value("Set");
            if proto.to_bits() != crate::value::TAG_UNDEFINED {
                return Some(proto);
            }
        }
        // #5834: WeakMap/WeakSet instances are `GC_TYPE_OBJECT`s stamped with
        // a reserved `class_id` (CLASS_ID_WEAKMAP/CLASS_ID_WEAKSET), not a
        // registered declared-class id, so the generic class-id prototype
        // walk further down never resolves them and instance receivers fell
        // through to `return obj_value` (i.e. `getPrototypeOf(wm) === wm`).
        // `weak_class_id_from_receiver` pre-validates the address via the
        // GC-header read (safe for a garbage/foreign pointer) before
        // comparing `class_id`, matching the `is_registered_map`/
        // `is_registered_set` safety bar above.
        let receiver = crate::value::js_nanbox_pointer(addr as i64);
        if let Some(class_id) = crate::weakref::weak_class_id_from_receiver(receiver) {
            let name = if class_id == crate::weakref::CLASS_ID_WEAKMAP {
                "WeakMap"
            } else {
                "WeakSet"
            };
            let proto = crate::object::builtin_prototype_value(name);
            if proto.to_bits() != crate::value::TAG_UNDEFINED {
                return Some(proto);
            }
        }
        None
    };
    let buffer_backed_prototype = |addr: usize| -> Option<f64> {
        let name = if crate::buffer::is_array_buffer(addr) {
            "ArrayBuffer"
        } else if crate::buffer::is_shared_array_buffer(addr) {
            "SharedArrayBuffer"
        } else {
            return None;
        };
        let proto = crate::object::builtin_prototype_value(name);
        if proto.to_bits() != crate::value::TAG_UNDEFINED {
            Some(proto)
        } else {
            None
        }
    };
    let buffer_backed_uint8array_prototype = |addr: usize| -> Option<f64> {
        if !crate::buffer::is_uint8array_buffer(addr) {
            return None;
        }
        let proto = crate::object::builtin_prototype_value("Uint8Array");
        if proto.to_bits() != crate::value::TAG_UNDEFINED {
            Some(proto)
        } else {
            None
        }
    };
    let typed_array_instance_prototype = |addr: usize| -> Option<f64> {
        let kind = crate::typedarray::lookup_typed_array_kind(addr)?;
        // A `Reflect.construct(TA, …, newTarget)` view with a custom
        // `[[Prototype]]` (spec `GetPrototypeFromConstructor`) resolves to the
        // recorded prototype rather than the default per-kind prototype. The
        // link is stored in the GC-tracked static-prototype side table.
        if let Some(proto_bits) = super::super::prototype_chain::object_static_prototype(addr) {
            if proto_bits != crate::value::TAG_NULL {
                return Some(f64::from_bits(proto_bits));
            }
        }
        let proto = crate::object::builtin_prototype_value(crate::typedarray::name_for_kind(kind));
        if proto.to_bits() != crate::value::TAG_UNDEFINED {
            Some(proto)
        } else {
            None
        }
    };
    let function_prototype_or_null = || {
        let proto = crate::object::builtin_prototype_value("Function");
        if proto.to_bits() != crate::value::TAG_UNDEFINED {
            proto
        } else {
            f64::from_bits(TAG_NULL)
        }
    };
    if top16 == 0x7FFE {
        let class_id = (bits & 0xFFFF_FFFF) as u32;
        if let Some(parent_id) = get_parent_class_id(class_id) {
            if parent_id != 0 {
                let parent_bits = 0x7FFE_0000_0000_0000u64 | (parent_id as u64);
                return f64::from_bits(parent_bits);
            }
        }
        // Root of the class hierarchy. In JS `Object.getPrototypeOf` of a base
        // class *constructor* is `Function.prototype`, and of a base class's
        // `.prototype` object is `Object.prototype` — NOT null. class-transformer
        // walks `Object.getPrototypeOf(target.prototype.constructor)` and then
        // dereferences `.prototype` on the result; the old `null` made that
        // `null.prototype` throw, blocking class-transformer/class-validator on
        // any flat (no-`extends`) DTO. (#420 followup)
        if super::super::class_prototype_ref_id(obj_value).is_some() {
            let proto = crate::object::builtin_prototype_value("Object");
            if proto.to_bits() != crate::value::TAG_UNDEFINED {
                return proto;
            }
            return f64::from_bits(TAG_NULL);
        }
        return function_prototype_or_null();
    }
    // Heap-pointer receiver — return the input value itself. For
    // class-id-tagged instances, `.constructor` then returns the class
    // ref (via the constructor intercept in js_object_get_field_by_name,
    // v0.5.746), making `getPrototypeOf(v).constructor === v.constructor`.
    // For object literals / arrays / other non-class-tagged heap values,
    // `.constructor` returns undefined, which collapses drizzle's
    // `if (cls)` chain to false safely (instead of throwing on
    // `null.constructor` if we returned null). Drizzle's
    // `is(value, type)` chain calls this on every chunk including
    // arrays of values, so the array case is load-bearing.
    //
    // Two NaN-shapes cover the heap-pointer case:
    //  - top16 == 0x7FFD: NaN-boxed POINTER_TAG (typical function-local).
    //  - top16 == 0x0000 with raw_addr large enough: module-level object
    //    literals get stored as raw I64 pointers (no NaN-boxing) per the
    //    "Module-level variables" note in CLAUDE.md, so we accept that
    //    form here too.
    if top16 == 0x7FFD {
        let raw_addr = bits & 0x0000_FFFF_FFFF_FFFF;
        if raw_addr != 0 && raw_addr >= (crate::gc::GC_HEADER_SIZE as u64) + 0x1000 {
            if let Some(proto) = typed_array_instance_prototype(raw_addr as usize) {
                return proto;
            }
            if let Some(proto) = buffer_backed_prototype(raw_addr as usize) {
                return proto;
            }
            if let Some(proto) = buffer_backed_uint8array_prototype(raw_addr as usize) {
                return proto;
            }
            if let Some(proto) = collection_prototype(raw_addr as usize) {
                return proto;
            }
            // #2820: an explicit `Object.setPrototypeOf(obj, proto)` recorded
            // in the side-table takes precedence — return exactly what was set
            // (including `null`).
            if let Some(proto_bits) =
                super::super::prototype_chain::object_static_prototype(raw_addr as usize)
            {
                return f64::from_bits(proto_bits);
            }
            unsafe {
                let obj = raw_addr as *const ObjectHeader;
                let gc = gc_header_for(obj);
                // #1175: objects allocated with a null prototype
                // (Object.create(null), querystring.parse) report null here.
                if (*gc)._reserved & crate::gc::OBJ_FLAG_NULL_PROTO != 0 {
                    return f64::from_bits(TAG_NULL);
                }
                // #2145: per-kind typed-array `.prototype` objects share a
                // single `%TypedArray%.prototype` parent. Resolved off the
                // cached intrinsic pointer (also a GC root) so the chain holds
                // through copying GC.
                if (*gc)._reserved & crate::gc::OBJ_FLAG_TYPED_ARRAY_PROTO != 0 {
                    let p = crate::object::typed_array_intrinsic_proto_ptr();
                    if !p.is_null() {
                        return f64::from_bits(crate::value::js_nanbox_pointer(p as i64).to_bits());
                    }
                }
                if (*gc).obj_type == crate::gc::GC_TYPE_ERROR {
                    let err = raw_addr as *const crate::error::ErrorHeader;
                    if let Some(proto) = error_kind_prototype_value((*err).error_kind) {
                        return proto;
                    }
                }
                if (*gc).obj_type == crate::gc::GC_TYPE_ARRAY {
                    if let Some(proto) =
                        super::super::array_get_prototype_of_addr(raw_addr as usize)
                    {
                        return proto;
                    }
                }
                // #489 / #2145: a function/constructor receiver has no
                // walkable [[Prototype]] in Perry's model UNLESS its
                // closure-static-prototype side-table has been set
                // (`Object.setPrototypeOf(closure, parent)` — effect's
                // TagClass and Perry's `%TypedArray%`-chain typed-array
                // constructors use this). Returning the recorded parent
                // satisfies drizzle's `cls = getPrototypeOf(cls)` walk
                // (which terminates when the parent has no further
                // recorded proto) and the test262 `__proto__` chain. When
                // no static prototype is recorded, return null to break
                // the would-be `getPrototypeOf(cls) === cls` self-cycle.
                if (*gc).obj_type == crate::gc::GC_TYPE_CLOSURE {
                    if let Some(proto_bits) =
                        crate::closure::closure_static_prototype(raw_addr as usize)
                    {
                        return f64::from_bits(proto_bits);
                    }
                    // #3664: a generator/async-generator function's
                    // [[Prototype]] is `%Generator%` / `%AsyncGenerator%`.
                    if let Some(proto) =
                        crate::object::generator_function_proto_of(raw_addr as usize)
                    {
                        return proto;
                    }
                    return function_prototype_or_null();
                }
                if let Some(proto_bits) =
                    super::super::prototype_chain::object_static_prototype(raw_addr as usize)
                {
                    return f64::from_bits(proto_bits);
                }
                // Fast [[Prototype]] for a DECLARED-class instance: resolve
                // directly from the class id instead of the generic
                // `constructor_dynamic_prototype` probe, which reads the
                // `constructor` field by name and therefore does a LINEAR scan
                // over the instance's own keys (O(own-key-count)) before missing
                // and continuing to the prototype. On a wide build —
                // `const o = new C(); for (i) o["k"+i] = i` — that scan grows by
                // one each iteration, making any reflective getPrototypeOf on the
                // instance O(n²). The class-id table at line ~2810 below already
                // returns this exact prototype for the same instances; hoisting it
                // here is semantically identical (same declared-class prototype
                // object) but O(1). Gated on a REAL declared class id only
                // (`class_decl_prototype_value_for_instance_class` returns None for
                // class_id 0 / anonymous-shape / unregistered ids), so synthetic
                // function-ctor instances and plain objects keep the existing
                // `constructor`-based resolution unchanged.
                if (*gc).obj_type == crate::gc::GC_TYPE_OBJECT
                    && (*obj).class_id != 0
                    && !is_anon_shape_class_id((*obj).class_id)
                {
                    if let Some(proto) =
                        super::super::class_registry::class_decl_prototype_value_for_instance_class(
                            (*obj).class_id,
                        )
                    {
                        return proto;
                    }
                }
                // #3986: `Object.create(proto)` / `new F()` (plain function
                // ctor) instances carry a synthetic class id whose prototype
                // object is stored keyed by that id. Return the exact stored
                // object so prototype identity is preserved. Gated on
                // GC_TYPE_OBJECT so non-object heap values don't misresolve.
                if (*gc).obj_type == crate::gc::GC_TYPE_OBJECT {
                    let synth_proto =
                        super::super::class_registry::class_prototype_object((*obj).class_id);
                    if !synth_proto.is_null() {
                        return f64::from_bits(
                            crate::value::js_nanbox_pointer(synth_proto as i64).to_bits(),
                        );
                    }
                }
                if let Some(proto) = constructor_dynamic_prototype(obj) {
                    return proto;
                }
                if (*gc).obj_type == crate::gc::GC_TYPE_OBJECT
                    && ((*obj).class_id == 0 || is_anon_shape_class_id((*obj).class_id))
                {
                    if let Some(proto_bits) =
                        super::super::prototype_chain::default_object_prototype_for_owner(
                            raw_addr as usize,
                        )
                    {
                        return f64::from_bits(proto_bits);
                    }
                    return f64::from_bits(TAG_NULL);
                }
                // Built-in iterator instances (Array/Map/Set/String iterators)
                // share a `%...IteratorPrototype%` singleton. Their instances
                // normally carry it as a recorded static prototype (returned
                // above), but resolve by class id too so the chain holds even if
                // the static-prototype side-table entry was dropped.
                if (*gc).obj_type == crate::gc::GC_TYPE_OBJECT {
                    if let Some(proto) =
                        super::super::iterator_prototype_for_class_id((*obj).class_id)
                    {
                        return proto;
                    }
                    if let Some(proto) =
                        super::super::class_registry::class_decl_prototype_value_for_instance_class(
                            (*obj).class_id,
                        )
                    {
                        return proto;
                    }
                }
                // A native-module namespace object (`require("path")` etc.,
                // class_id NATIVE_MODULE_CLASS_ID, the `__module__`-tagged
                // object) is an ordinary object whose [[Prototype]] is
                // %Object.prototype% — NOT itself. The `return obj_value` self-
                // prototype fallback below makes turbopack's `interopEsm`
                // proto-chain walk (`for(cur=raw; !LEAF.includes(cur);
                // cur=getProto(cur))`) never terminate — getProto keeps
                // returning the same object, so it creates export getters
                // forever (the Next.js standalone startup runaway: unbounded
                // memory growth, no `✓ Ready`). Return Object.prototype so the
                // walk reaches a LEAF_PROTOTYPE and stops.
                if (*obj).class_id == super::super::native_module::NATIVE_MODULE_CLASS_ID {
                    let proto = crate::object::builtin_prototype_value("Object");
                    if proto.to_bits() != crate::value::TAG_UNDEFINED {
                        return proto;
                    }
                    return f64::from_bits(TAG_NULL);
                }
            }
            return obj_value;
        }
    }
    if top16 == 0 && bits >= (crate::gc::GC_HEADER_SIZE as u64) + 0x1000 {
        if let Some(proto) = typed_array_instance_prototype(bits as usize) {
            return proto;
        }
        if let Some(proto) = buffer_backed_prototype(bits as usize) {
            return proto;
        }
        if let Some(proto) = buffer_backed_uint8array_prototype(bits as usize) {
            return proto;
        }
        if let Some(proto) = collection_prototype(bits as usize) {
            return proto;
        }
        // #2820: explicit setPrototypeOf side-table takes precedence.
        if let Some(proto_bits) =
            super::super::prototype_chain::object_static_prototype(bits as usize)
        {
            return f64::from_bits(proto_bits);
        }
        unsafe {
            let obj = bits as *const ObjectHeader;
            let gc = gc_header_for(obj);
            if (*gc)._reserved & crate::gc::OBJ_FLAG_NULL_PROTO != 0 {
                return f64::from_bits(TAG_NULL);
            }
            if (*gc)._reserved & crate::gc::OBJ_FLAG_TYPED_ARRAY_PROTO != 0 {
                let p = crate::object::typed_array_intrinsic_proto_ptr();
                if !p.is_null() {
                    return f64::from_bits(crate::value::js_nanbox_pointer(p as i64).to_bits());
                }
            }
            if (*gc).obj_type == crate::gc::GC_TYPE_ERROR {
                let err = bits as *const crate::error::ErrorHeader;
                if let Some(proto) = error_kind_prototype_value((*err).error_kind) {
                    return proto;
                }
            }
            if (*gc).obj_type == crate::gc::GC_TYPE_ARRAY {
                if let Some(proto) = super::super::array_get_prototype_of_addr(bits as usize) {
                    return proto;
                }
            }
            // #489 / #2145: function/constructor receiver — see the
            // 0x7FFD branch above. Return the recorded static
            // prototype if any, else null to break the chain-walk
            // self-cycle.
            if (*gc).obj_type == crate::gc::GC_TYPE_CLOSURE {
                if let Some(proto_bits) = crate::closure::closure_static_prototype(bits as usize) {
                    return f64::from_bits(proto_bits);
                }
                // #3664: generator/async-generator [[Prototype]] resolution.
                if let Some(proto) = crate::object::generator_function_proto_of(bits as usize) {
                    return proto;
                }
                return function_prototype_or_null();
            }
            // #3986: synthetic-class instance (see the sibling site above) —
            // return its stored prototype object to preserve identity.
            if (*gc).obj_type == crate::gc::GC_TYPE_OBJECT {
                let synth_proto =
                    super::super::class_registry::class_prototype_object((*obj).class_id);
                if !synth_proto.is_null() {
                    return f64::from_bits(
                        crate::value::js_nanbox_pointer(synth_proto as i64).to_bits(),
                    );
                }
            }
            if let Some(proto) = constructor_dynamic_prototype(obj) {
                return proto;
            }
            if (*gc).obj_type == crate::gc::GC_TYPE_OBJECT
                && ((*obj).class_id == 0 || is_anon_shape_class_id((*obj).class_id))
            {
                if let Some(proto_bits) =
                    super::super::prototype_chain::default_object_prototype_for_owner(bits as usize)
                {
                    return f64::from_bits(proto_bits);
                }
                return f64::from_bits(TAG_NULL);
            }
            if (*gc).obj_type == crate::gc::GC_TYPE_OBJECT {
                if let Some(proto) = super::super::iterator_prototype_for_class_id((*obj).class_id)
                {
                    return proto;
                }
                if let Some(proto) =
                    super::super::class_registry::class_decl_prototype_value_for_instance_class(
                        (*obj).class_id,
                    )
                {
                    return proto;
                }
                // A native-module namespace object (`require("path")` etc.,
                // class_id NATIVE_MODULE_CLASS_ID, the `__module__`-tagged
                // object) is an ordinary object whose [[Prototype]] is
                // %Object.prototype% — NOT itself. The `return obj_value` self-
                // prototype fallback below makes turbopack's `interopEsm`
                // proto-chain walk (`for(cur=raw; !LEAF.includes(cur);
                // cur=getProto(cur))`) never terminate — getProto keeps
                // returning the same object, so it creates export getters
                // forever (the Next.js standalone startup runaway: unbounded
                // memory growth, no `✓ Ready`). Return Object.prototype so the
                // walk reaches a LEAF_PROTOTYPE and stops.
                if (*obj).class_id == super::super::native_module::NATIVE_MODULE_CLASS_ID {
                    let proto = crate::object::builtin_prototype_value("Object");
                    if proto.to_bits() != crate::value::TAG_UNDEFINED {
                        return proto;
                    }
                    return f64::from_bits(TAG_NULL);
                }
            }
        }
        return obj_value;
    }
    f64::from_bits(TAG_NULL)
}
