//! `Object.create`, `Object.getPrototypeOf`, and the globalThis-builtin lookup.
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
    //
    // #7497: `globalThis` must be ROOTED across the key allocation, and its
    // address re-read AFTER it. `js_string_from_bytes` allocates a fresh
    // string on EVERY call (this lookup interns nothing), so it can trigger a
    // copying minor that evacuates the `globalThis` object. `THREAD_GLOBAL_THIS`
    // is a registered root and gets rewritten — but a raw `*const ObjectHeader`
    // already read out of it does not, and `js_object_get_field_by_name` then
    // dereferences retired from-space. That is the whole of #7497: every
    // element of a `Promise.all` runs `Promise.resolve`, which asks
    // `is_default_promise_constructor` for `globalThis.Promise` through here,
    // so one 50 000-element `Promise.all` performs 50 000 of these lookups and
    // one of them straddles the collection.
    let scope = crate::gc::RuntimeHandleScope::new();
    let global_handle = scope.root_nanbox_f64(js_get_global_this());
    // #9761: this lookup used to MINT the name string on every call — the
    // comment below still records why that allocation is a collection point.
    // It is now the canonical interned header, so the allocation happens once
    // per thread per name instead of once per lookup: on the compiled cc TUI
    // this single site was 133 MB of the 990 MB a 3300-character reply
    // allocates (every primitive method call asks for `globalThis.String`).
    let (key, global_this_f64) =
        global_handle.across_nanbox(|| crate::string::canonical_key(name.as_bytes()));
    let global_obj = crate::value::js_nanbox_get_pointer(global_this_f64) as *const ObjectHeader;
    if global_obj.is_null() {
        return f64::from_bits(crate::value::TAG_UNDEFINED);
    }
    // Nothing between the re-read above and this call allocates, so
    // `global_obj` is still the post-collection address here.
    let value = js_object_get_field_by_name(global_obj, key);
    let bits = value.bits();
    f64::from_bits(bits)
}

/// Object.create(proto) — create an empty object with an owner-traced prototype.
#[no_mangle]
pub extern "C" fn js_object_create(proto_value: f64) -> f64 {
    // Keep the existing accepted prototype kinds, but store their identity on
    // the instance instead of permanently rooting them under a new class id.
    let valid = crate::proxy::js_proxy_is_proxy(proto_value) != 0
        || crate::typedarray_props::typed_array_addr_from_value(proto_value).is_some()
        || {
            let value = crate::value::JSValue::from_bits(proto_value.to_bits());
            if value.is_pointer() {
                let ptr = value.as_pointer::<ObjectHeader>();
                let addr = ptr as usize;
                crate::value::addr_class::is_above_handle_band(addr)
                    && !crate::set::is_registered_set(addr)
                    && !crate::map::is_registered_map(addr)
                    && !crate::regex::is_regex_pointer(ptr as *const u8)
                    && is_valid_obj_ptr(ptr as *const u8)
            } else {
                false
            }
        };
    if !valid {
        return crate::value::js_nanbox_pointer(js_object_alloc_null_proto(0, 0) as i64);
    }
    let scope = crate::gc::RuntimeHandleScope::new();
    let proto = scope.root_nanbox_f64(proto_value);
    let obj = scope.root_raw_mut_ptr(js_object_alloc(0, 0));
    // The link is a self-rooting entry point: it roots the owner and the
    // prototype before its meta-record allocation, so the handle is re-read
    // afterwards for the post-collection address.
    obj.with_mut_ptr::<ObjectHeader, _>(|owner| {
        crate::object::prototype_chain::object_link_created_prototype(
            owner as usize,
            proto.get_nanbox_u64(),
        )
    });
    obj.with_mut_ptr::<ObjectHeader, _>(|owner| crate::value::js_nanbox_pointer(owner as i64))
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
    let proto = get_prototype_of_resolved(obj_value);
    // #10086: this is the ONE place a prototype object reaches user code, so
    // it is also the only place the array-iterator prototype can escape to be
    // patched. Publishing here is what lets the `for…of` index loop and the
    // array-destructuring fast arm decline a possibly-patched
    // `%ArrayIteratorPrototype%.next`, which neither can observe otherwise.
    crate::object::iterator_prototypes::note_iterator_prototype_exposed(proto);
    proto
}

/// `net.Socket.prototype`, read from the same cached bound export that
/// user code sees as `net.Socket`, so the two are identical.
///
/// This is an ordinary `[[Get]]`, not a read of an already-installed dynamic
/// prop: the prototype object is created on first access, and a
/// `getPrototypeOf(socket)` can run before user code has ever read
/// `net.Socket.prototype`.
fn net_socket_prototype_value() -> f64 {
    let ctor = crate::object::bound_native_callable_export_value("net", "Socket");
    if !crate::value::JSValue::from_bits(ctor.to_bits()).is_pointer() {
        return f64::from_bits(crate::value::TAG_UNDEFINED);
    }
    let scope = crate::gc::RuntimeHandleScope::new();
    let ctor = scope.root_nanbox_f64(ctor);
    let key = b"prototype";
    let key = crate::string::js_string_from_bytes(key.as_ptr(), key.len() as u32);
    let key = scope.root_nanbox_f64(crate::value::js_nanbox_string(key as i64));
    crate::proxy::js_reflect_get(
        ctor.get_nanbox_f64(),
        key.get_nanbox_f64(),
        ctor.get_nanbox_f64(),
    )
}

/// The resolution itself; see [`js_object_get_prototype_of`].
fn get_prototype_of_resolved(obj_value: f64) -> f64 {
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
            // WHATWG fetch values are registry handles rather than heap
            // ObjectHeaders, but they still have the intrinsic prototype for
            // their interface.  Axios walks this chain to distinguish a
            // Headers iterable from a plain record; returning null here made
            // it copy zero response headers even though `headers.entries()`
            // itself worked.
            if let Some(probe) = super::super::fetch_handle_kind_probe() {
                let builtin = match unsafe { probe(raw_addr as usize) } {
                    1 => Some("Response"),
                    2 => Some("Request"),
                    3 => Some("Headers"),
                    4 => Some("Blob"),
                    5 => Some("File"),
                    6 => Some("FormData"),
                    _ => None,
                };
                if let Some(name) = builtin {
                    let proto = crate::object::builtin_prototype_value(name);
                    if proto.to_bits() != crate::value::TAG_UNDEFINED {
                        return proto;
                    }
                }
            }
            // A live `net.Socket` is a registry handle too. `instanceof
            // net.Socket` already recognises it through the provider's probe
            // (bundled or external net), but this path answered `null`, so
            // `Object.getPrototypeOf(socket).constructor` threw. undici's
            // `util.destroy(socket, err)` does exactly that on every socket
            // error path, which masked the real error with
            // "Cannot read properties of null (reading 'constructor')".
            let is_net_socket = super::super::class_registry::net_socket_handle_probe()
                .map(|probe| unsafe { probe(raw_addr as i64) })
                .unwrap_or(false);
            if is_net_socket {
                let proto = net_socket_prototype_value();
                if crate::value::JSValue::from_bits(proto.to_bits()).is_pointer() {
                    return proto;
                }
            }
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
        if let Some(class_id) = crate::object::weak_class_id_from_receiver(receiver) {
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
        // A native ArrayBuffer/SharedArrayBuffer constructed with a distinct
        // newTarget (subclassing / Reflect.construct) records that custom
        // [[Prototype]] in the same side table as typed arrays. Honor it before
        // falling back to the intrinsic buffer prototype.
        if let Some(proto_bits) = super::super::prototype_chain::object_static_prototype(addr) {
            if proto_bits != crate::value::TAG_NULL {
                return Some(f64::from_bits(proto_bits));
            }
        }
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
    let node_buffer_prototype = |addr: usize| -> Option<f64> {
        if !crate::buffer::is_node_buffer(addr) {
            return None;
        }
        let proto = crate::object::builtin_prototype_value("Buffer");
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
        // An explicit `Object.setPrototypeOf(Ctor, obj)` wins over every
        // derived answer below — it IS the constructor's [[Prototype]].
        if super::super::class_prototype_ref_id(obj_value).is_none() {
            let static_proto = super::super::class_registry::class_static_prototype(class_id);
            if !static_proto.is_null() {
                return f64::from_bits(
                    crate::value::js_nanbox_pointer(static_proto as i64).to_bits(),
                );
            }
            if super::super::class_registry::class_static_prototype_is_nulled(class_id) {
                return f64::from_bits(TAG_NULL);
            }
        }
        if super::super::class_prototype_ref_id(obj_value).is_none() {
            // A class whose heritage is a runtime function value has no Perry
            // parent class id. Its constructor's [[Prototype]] is that exact
            // function object (not Function.prototype), as observed by
            // Object.getPrototypeOf(D) and static `super` lookup.
            let dynamic_parent = super::super::js_get_dynamic_parent_value(class_id);
            let parent_value = crate::value::JSValue::from_bits(dynamic_parent.to_bits());
            if !parent_value.is_undefined() && !parent_value.is_null() {
                return dynamic_parent;
            }
        }
        if let Some(parent_id) = get_parent_class_id(class_id) {
            // #8343 followup: `NATIVE_MODULE_CLASS_ID` (0xFFFFFFFE) is a
            // sentinel, not a real class. A prior `Object.create(proto)` whose
            // `proto` was a native-module namespace object registered it as a
            // synthetic class's parent. Returning the raw sentinel as an
            // INT32-tagged class ref (`-2`) trips `Object.create` with
            // `TypeError: Object prototype may only be an Object or null: -2`.
            // Treat it as a root: a native-module namespace's [[Prototype]] is
            // %Object.prototype%, so the synthetic class whose proto was that
            // namespace inherits Object.prototype too.
            if parent_id != 0 && parent_id != super::super::native_module::NATIVE_MODULE_CLASS_ID {
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
        // #9304: ArrayHeader growth leaves aliases pointing at a forwarding
        // stub while the address-keyed [[Prototype]] entry follows the live
        // allocation. Canonicalize before any registry lookup.
        let raw_addr = crate::value::resolve_forwarding(raw_addr as usize) as u64;
        if raw_addr != 0 && raw_addr >= (crate::gc::GC_HEADER_SIZE as u64) + 0x1000 {
            if let Some(proto) = typed_array_instance_prototype(raw_addr as usize) {
                return proto;
            }
            if let Some(proto) = buffer_backed_prototype(raw_addr as usize) {
                return proto;
            }
            if let Some(proto) = node_buffer_prototype(raw_addr as usize) {
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
                // A RegExp's internal prototype does not depend on its
                // observable constructor property. Resolve it before the
                // generic constructor probe, which would recurse through Get.
                if (*gc).obj_type == crate::gc::GC_TYPE_REGEXP {
                    return crate::object::builtin_prototype_value("RegExp");
                }
                // #2145: per-kind typed-array `.prototype` objects share a
                // single `%TypedArray%.prototype` parent. Resolved off the
                // cached intrinsic pointer (also a GC root) so the chain holds
                // through copying GC.
                // Bit 8 means "per-kind TypedArray prototype" only on a
                // `GC_TYPE_OBJECT`; on an array it is `GC_ARRAY_NAMED_PROPS`.
                if (*gc).obj_type == crate::gc::GC_TYPE_OBJECT
                    && (*gc)._reserved & crate::gc::OBJ_FLAG_TYPED_ARRAY_PROTO != 0
                {
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
                // #9502: a fresh class value is a constructor, not an instance
                // of its template. Its [[Prototype]] is the evaluated parent;
                // `.prototype` is a separate object with a separate chain.
                if super::super::class_registry::is_class_object_ptr(obj as *const u8) {
                    if let Some(parent) =
                        super::super::class_registry::class_object_pinned_parent(obj)
                    {
                        if !matches!(
                            parent.to_bits(),
                            crate::value::TAG_NULL | crate::value::TAG_UNDEFINED
                        ) {
                            return parent;
                        }
                    }
                    return function_prototype_or_null();
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
                // A declared class keeps its reflective prototype even when its
                // id is ALSO marked as an anon shape. Class ids are allocated
                // per module, so one module's anon-shape id can collide with
                // another's declared class (observed: Effect's monomorphized
                // `Union$AST`, whose instances started reporting
                // `Object.prototype` once an unrelated module's init registered
                // the same number). `Object.create(Object.getPrototypeOf(ast),
                // descriptors)` — the standard prototype-preserving clone, used
                // by SchemaAST's `modifyOwnPropertyDescriptors` — then produced
                // objects with none of the class's methods. A registered class
                // name plus a non-empty prototype vtable is positive evidence of
                // a real declared class, so prefer it over the collision.
                let instance_class_id = (*obj).class_id;
                if (*gc).obj_type == crate::gc::GC_TYPE_OBJECT
                    && instance_class_id != 0
                    && (!is_anon_shape_class_id(instance_class_id)
                        || super::super::class_registry::declared_class_outranks_anon_shape(
                            instance_class_id,
                        ))
                {
                    if let Some(proto) =
                        super::super::class_registry::class_decl_prototype_value_for_instance_class(
                            (*obj).class_id,
                        )
                    {
                        return proto;
                    }
                }
                // #10478: `Object.create(proto)` (and a plain-function `new
                // F()`) records the exact `[[Prototype]]` object under the
                // instance's SYNTHETIC class id. That link is authoritative, so
                // prefer it over the `constructor`-derived guess below, which
                // reads `obj.constructor` and answers `ctor.prototype`. The
                // guess only happened to miss while an `Object.create` result's
                // inherited `constructor` resolved to a bogus class ref; once it
                // correctly answers `Object` / `A`, the guess returns
                // `Object.prototype` / `A.prototype` and drops `proto` itself —
                // with its inherited accessors, descriptors and non-writable
                // slots. (The same lookup runs further down for receivers that
                // reach it; this one only moves it ahead of the guess.)
                if (*gc).obj_type == crate::gc::GC_TYPE_OBJECT {
                    let synth_proto =
                        super::super::class_registry::synthetic_class_prototype_object(
                            (*obj).class_id,
                        );
                    if !synth_proto.is_null() && synth_proto as usize != raw_addr as usize {
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
                    // #3986: `Object.create(proto)` and `new F()` (a plain
                    // function ctor, whose instances carry a synthetic
                    // function-prototype class id) record the actual
                    // [[Prototype]] object pointer in CLASS_PROTOTYPE_OBJECTS
                    // keyed by that synthetic class id. Return the exact stored
                    // pointer so `Object.getPrototypeOf(o) === proto` holds by
                    // identity (test262 built-ins/Object/create/15.2.3.5-*,
                    // S9.9 ToObject identity). Declared ES classes use the
                    // separate CLASS_DECL_PROTOTYPE_OBJECTS table handled just
                    // above, so this does not perturb the
                    // `getPrototypeOf(instance) === instance` model their
                    // `.constructor` resolution relies on. Without this the
                    // synthetic-class instance fell through to the
                    // `return obj_value` self-prototype fallback below.
                    let synth_proto =
                        super::super::class_registry::class_prototype_object((*obj).class_id);
                    if !synth_proto.is_null() {
                        return f64::from_bits(
                            crate::value::js_nanbox_pointer(synth_proto as i64).to_bits(),
                        );
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
        // Module-level arrays use raw pointers and need the same canonical key.
        let bits = crate::value::resolve_forwarding(bits as usize) as u64;
        if let Some(proto) = typed_array_instance_prototype(bits as usize) {
            return proto;
        }
        if let Some(proto) = buffer_backed_prototype(bits as usize) {
            return proto;
        }
        if let Some(proto) = node_buffer_prototype(bits as usize) {
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
            if (*gc).obj_type == crate::gc::GC_TYPE_REGEXP {
                return crate::object::builtin_prototype_value("RegExp");
            }
            // Bit 8 means "per-kind TypedArray prototype" only on a
            // `GC_TYPE_OBJECT`; on an array it is `GC_ARRAY_NAMED_PROPS`.
            if (*gc).obj_type == crate::gc::GC_TYPE_OBJECT
                && (*gc)._reserved & crate::gc::OBJ_FLAG_TYPED_ARRAY_PROTO != 0
            {
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
                // #3986: `Object.create(proto)` and `new F()` (a plain
                // function ctor, whose instances carry a synthetic
                // function-prototype class id) record the actual
                // [[Prototype]] object pointer in CLASS_PROTOTYPE_OBJECTS
                // keyed by that synthetic class id. Return the exact stored
                // pointer so `Object.getPrototypeOf(o) === proto` holds by
                // identity (test262 built-ins/Object/create/15.2.3.5-*,
                // S9.9 ToObject identity). Declared ES classes use the
                // separate CLASS_DECL_PROTOTYPE_OBJECTS table handled just
                // above, so this does not perturb the
                // `getPrototypeOf(instance) === instance` model their
                // `.constructor` resolution relies on. Without this the
                // synthetic-class instance fell through to the
                // `return obj_value` self-prototype fallback below.
                let synth_proto =
                    super::super::class_registry::class_prototype_object((*obj).class_id);
                if !synth_proto.is_null() {
                    return f64::from_bits(
                        crate::value::js_nanbox_pointer(synth_proto as i64).to_bits(),
                    );
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
