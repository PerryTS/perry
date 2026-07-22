//! `Object.defineProperty` and its class-prototype-method installation helper.
use super::super::*;
use super::*;

/// #2159 helper: install a `target_cid.method` entry from an
/// `Object.defineProperty(C.prototype, name, descriptor)` call.
///
/// The descriptor's `value` came in two main shapes in practice:
///
/// 1. A `BOUND_METHOD_FUNC_PTR` closure returned by `getOwnPropertyDescriptor`
///    on a sibling class (drizzle's `applyMixins(Base, [Mixin])`: the
///    `getOwnPropertyDescriptor(Mixin.prototype, name)` value reads as
///    `js_class_method_bind(Mixin_class_ref, name)`). Dispatching that bound
///    closure would re-enter `js_native_call_method` against the class-ref —
///    a class object reaches the *static* dispatch arm, not the instance
///    method, so calling it would return the wrong thing. Instead we look up
///    the raw vtable entry on the source class and copy it onto the target
///    class's vtable directly, so future `inst.method(args)` dispatches via
///    the regular chain walk with `this = inst`.
///
/// 2. A user-supplied closure (e.g. `Object.defineProperty(C.prototype, "m",
///    { value: function () { … } })`). Route through the same per-class
///    prototype-method side table that `js_register_prototype_method` (#838)
///    uses, so the `inst.m` / `inst.m()` lookup paths in
///    `field_get_set.rs` / `native_call_method.rs` find it after the regular
///    vtable miss.
unsafe fn define_class_prototype_method(target_cid: u32, name: &str, value_bits: u64) {
    use crate::closure::{ClosureHeader, BOUND_METHOD_FUNC_PTR, CLOSURE_MAGIC};
    use crate::object::class_registry::{ClassVTable, VTableMethodEntry, CLASS_VTABLE_REGISTRY};

    // Reject undefined / null / numeric values up front — those aren't
    // methods and shouldn't make it onto the prototype side tables.
    let value = f64::from_bits(value_bits);
    let jsv = crate::JSValue::from_bits(value_bits);
    if !jsv.is_pointer() {
        return;
    }
    let ptr = jsv.as_pointer::<u8>() as usize;
    if ptr < 0x1000 {
        return;
    }

    // Shape (1): BOUND_METHOD closure. Extract source class-ref + method
    // name from the captures (see `js_class_method_bind`), then copy the
    // source class's vtable entry (or any inherited entry up the parent
    // chain) onto `target_cid`.
    if crate::closure::is_closure_ptr(ptr) {
        let closure = ptr as *const ClosureHeader;
        if (*closure).type_tag == CLOSURE_MAGIC && (*closure).func_ptr == BOUND_METHOD_FUNC_PTR {
            let recv = crate::closure::js_closure_get_capture_f64(closure, 0);
            let recv_value = crate::JSValue::from_bits(recv.to_bits());
            let source_cid = super::super::class_ref_id(recv).or_else(|| {
                recv_value.is_pointer().then(|| {
                    super::super::class_registry::class_id_for_decl_prototype_object(
                        recv_value.as_pointer::<u8>() as usize,
                    )
                })?
            });
            if let Some(source_cid) = source_cid {
                if let Some((func_ptr, param_count, has_synthetic_arguments, has_rest)) =
                    super::super::lookup_class_method_in_chain(source_cid, name)
                {
                    let mut guard = CLASS_VTABLE_REGISTRY.write().unwrap();
                    if guard.is_none() {
                        *guard = Some(std::collections::HashMap::new());
                    }
                    let reg = guard.as_mut().unwrap();
                    let vtable = reg.entry(target_cid).or_insert_with(|| ClassVTable {
                        methods: std::collections::HashMap::new(),
                        getters: std::collections::HashMap::new(),
                        setters: std::collections::HashMap::new(),
                    });
                    vtable.methods.insert(
                        name.to_string(),
                        VTableMethodEntry {
                            func_ptr,
                            param_count,
                            has_synthetic_arguments,
                            has_rest,
                        },
                    );
                    drop(guard);
                    super::super::class_registry::js_register_class_id(target_cid);
                    crate::typed_feedback::invalidate_method_change(target_cid);
                    return;
                }
            }
        }
    }

    // Shape (2): any other callable value (user closure, regular function).
    // Mirror the `Class.prototype.method = fn` direct-assignment path so the
    // existing `lookup_prototype_method` walks find it.
    super::super::class_registry::js_register_prototype_method(
        target_cid,
        name.as_ptr(),
        name.len(),
        value,
    );
}

/// #6363: `[[DefineOwnProperty]]` for a native HANDLE receiver — a pointer-tagged
/// small registry id (zlib stream, fetch Request/Response/Headers/Blob, crypto
/// hash, …), not a heap `ObjectHeader`.
///
/// Handles get their arbitrary own-property storage from the
/// `handle_expando` side table — the same one a plain `handle.foo = v` write
/// lands in (perry-stdlib's `js_handle_property_set_dispatch` falls back to it
/// once every typed setter has passed). This ROUTES the descriptor there rather
/// than merely tolerating the call: the value round-trips through `handle.key`,
/// the `writable`/`enumerable`/`configurable` bits and any `get`/`set` pair are
/// recorded in the ordinary descriptor side tables (keyed by the handle id), and
/// `getOwnPropertyDescriptor` / `Object.keys` / `delete` read them back. A
/// define that returned `true` and dropped the property would be the same silent
/// no-op the throw replaced.
///
/// The caller has already established that `hid` is in the handle band and that
/// the receiver is not a Proxy (proxies are handle-band ids too, and are routed
/// to their `[[DefineOwnProperty]]` trap earlier).
unsafe fn define_property_on_handle(
    obj_value: f64,
    hid: i64,
    key_value: f64,
    descriptor_value: f64,
) -> f64 {
    use crate::object::descriptor_state::{
        set_accessor_descriptor, set_property_attrs, AccessorDescriptor,
    };
    use crate::object::handle_expando as hx;

    // A handle IS an object in Node, so the ordinary descriptor validation
    // applies: the descriptor must be an object, data and accessor fields can't
    // be mixed, and a present `get`/`set` must be callable.
    if !value_is_object_like(descriptor_value) || crate::symbol::js_is_symbol(descriptor_value) != 0
    {
        let desc = describe_value_for_type_error(descriptor_value);
        throw_object_type_error_with_suffix("Property description must be an object: ", &desc);
    }
    validate_property_descriptor(descriptor_value);

    // A Symbol key goes to the symbol side table (`SYMBOL_PROPERTIES`), which is
    // keyed by the NaN-box payload — a handle id works there unchanged, and it's
    // the table `handle[sym]` reads back from. String-coercing the symbol would
    // file the value under a `"Symbol(x)"` STRING name, unreachable by the
    // symbol-keyed reader. (This is the Next.js `PATCHED_SET_HEADER` shape.)
    if crate::symbol::js_is_symbol(key_value) != 0 {
        let has_get = desc_has_field(descriptor_value, b"get");
        let has_set = desc_has_field(descriptor_value, b"set");
        if has_get || has_set {
            let get_field = desc_read_field(descriptor_value, b"get");
            let set_field = desc_read_field(descriptor_value, b"set");
            let get_bits = if !has_get || get_field.is_undefined() {
                0
            } else {
                crate::closure::clone_closure_rebind_this(get_field.bits(), obj_value)
            };
            let set_bits = if !has_set || set_field.is_undefined() {
                0
            } else {
                crate::closure::clone_closure_rebind_this(set_field.bits(), obj_value)
            };
            crate::symbol::set_symbol_accessor_property(obj_value, key_value, get_bits, set_bits);
        } else if desc_has_field(descriptor_value, b"value") {
            let value_field = desc_read_field(descriptor_value, b"value");
            crate::symbol::js_object_set_symbol_property(
                obj_value,
                key_value,
                f64::from_bits(value_field.bits()),
            );
        }
        let read_flag = |name: &[u8]| -> Option<bool> {
            desc_has_field(descriptor_value, name).then(|| {
                crate::value::js_is_truthy(f64::from_bits(
                    desc_read_field(descriptor_value, name).bits(),
                )) != 0
            })
        };
        crate::symbol::set_symbol_property_attrs(
            crate::symbol::obj_key_from_f64(obj_value),
            crate::symbol::sym_key_from_f64(key_value),
            PropertyAttrs::new(
                read_flag(b"writable").unwrap_or(has_get || has_set),
                read_flag(b"enumerable").unwrap_or(false),
                read_flag(b"configurable").unwrap_or(false),
            ),
        );
        return obj_value;
    }

    let Some(key) = super::super::metadata_key_to_string(key_value) else {
        return obj_value;
    };

    // The property's CURRENT shape, captured before any mutation below.
    let had_accessor = hx::handle_expando_accessor(hid, &key).is_some();
    let had_data = hx::handle_expando_data_get(hid, &key).is_some();
    // Spec retention (ValidateAndApplyPropertyDescriptor): redefining an
    // EXISTING own property keeps the attributes the descriptor omits; a brand
    // new one defaults them to `false`.
    let existing: Option<PropertyAttrs> =
        (had_accessor || had_data).then(|| hx::handle_expando_attrs(hid, &key));

    let has_get = desc_has_field(descriptor_value, b"get");
    let has_set = desc_has_field(descriptor_value, b"set");
    let has_accessor = has_get || has_set;
    let has_value = desc_has_field(descriptor_value, b"value");

    if has_accessor {
        // The descriptor literal's `get()`/`set()` shorthands were lowered with
        // their reserved `this` slot pointing at the DESCRIPTOR object; rebind to
        // the handle so the accessor sees the right receiver — same clone the
        // ordinary object path does.
        let get_field = desc_read_field(descriptor_value, b"get");
        let set_field = desc_read_field(descriptor_value, b"set");
        let get_bits = if !has_get || get_field.is_undefined() {
            0
        } else {
            crate::closure::clone_closure_rebind_this(get_field.bits(), obj_value)
        };
        let set_bits = if !has_set || set_field.is_undefined() {
            0
        } else {
            crate::closure::clone_closure_rebind_this(set_field.bits(), obj_value)
        };
        set_accessor_descriptor(
            hid as usize,
            key.clone(),
            AccessorDescriptor {
                get: get_bits,
                set: set_bits,
            },
        );
    } else {
        // A data descriptor (`value`/`writable`) OR a generic one
        // (`{ enumerable: true }` alone). Drop any accessor that used to occupy
        // the key so the store can't fire a stale setter.
        if had_accessor {
            crate::object::descriptor_state::ACCESSOR_DESCRIPTORS.with(|m| {
                m.borrow_mut().remove(&(hid as usize, key.clone()));
            });
        }
        // [[Value]] is the descriptor's when present; otherwise it defaults to
        // `undefined` for a BRAND-NEW key or an accessor→data conversion (neither
        // has a data value to retain). Only a generic redefine of an EXISTING data
        // property keeps the current value.
        //
        // The `undefined` store matters beyond the value itself: it is what makes
        // the key an own property at all. `Object.defineProperty(h, "g", { enumerable:
        // true })` creates `g` in Node (`getOwnPropertyDescriptor` → `{value: undefined,
        // writable: false, enumerable: true, configurable: false}`, `"g" in h` → true);
        // recording only the attribute bits would leave every own-key probe reporting
        // it absent.
        let new_value = if has_value {
            Some(f64::from_bits(
                desc_read_field(descriptor_value, b"value").bits(),
            ))
        } else if had_accessor || !had_data {
            Some(f64::from_bits(crate::value::TAG_UNDEFINED))
        } else {
            None
        };
        if let Some(v) = new_value {
            hx::handle_expando_set(hid, &key, v);
        }
    }

    let read_flag = |name: &[u8]| -> Option<bool> {
        desc_has_field(descriptor_value, name).then(|| {
            crate::value::js_is_truthy(f64::from_bits(
                desc_read_field(descriptor_value, name).bits(),
            )) != 0
        })
    };
    set_property_attrs(
        hid as usize,
        key,
        PropertyAttrs::new(
            read_flag(b"writable")
                .unwrap_or_else(|| existing.map(|a| a.writable()).unwrap_or(has_accessor)),
            read_flag(b"enumerable")
                .unwrap_or_else(|| existing.map(|a| a.enumerable()).unwrap_or(false)),
            read_flag(b"configurable")
                .unwrap_or_else(|| existing.map(|a| a.configurable()).unwrap_or(false)),
        ),
    );
    obj_value
}

/// Object.defineProperty(obj, key, descriptor) — set the value AND record the
/// `writable` / `enumerable` / `configurable` attribute flags in the side table.
/// Returns the object (NaN-boxed pointer).
///
/// IMPORTANT: writes the value via `js_object_set_field_by_name` BEFORE recording
/// the descriptor — otherwise a `writable: false` descriptor would block its own
/// initial value from being stored.
#[no_mangle]
pub extern "C" fn js_object_define_property(
    obj_value: f64,
    key_value: f64,
    descriptor_value: f64,
) -> f64 {
    unsafe {
        // A Proxy receiver is a small registered id, not a heap object — it
        // fails the `value_is_object_like` test below (so it would wrongly throw
        // "called on non-object") and the ordinary paths would deref the fake
        // pointer and segfault. Per spec, Object.defineProperty(proxy, …):
        // validate the descriptor (ToPropertyDescriptor), invoke the
        // `[[DefineOwnProperty]]` trap, and throw a TypeError if it reports
        // failure. (Proxy crash cluster.)
        if crate::proxy::js_proxy_is_proxy(obj_value) != 0 {
            if !value_is_object_like(descriptor_value)
                || crate::symbol::js_is_symbol(descriptor_value) != 0
            {
                let desc = describe_value_for_type_error(descriptor_value);
                throw_object_type_error_with_suffix(
                    "Property description must be an object: ",
                    &desc,
                );
            }
            validate_property_descriptor(descriptor_value);
            let ok =
                crate::proxy::js_reflect_define_property(obj_value, key_value, descriptor_value);
            if crate::value::js_is_truthy(ok) == 0 {
                throw_object_type_error(b"'defineProperty' on proxy: trap returned falsish");
            }
            return obj_value;
        }

        // A numeric key defined on `Object.prototype` (data or accessor) shows
        // through array hole/OOB reads — flip the global flag.
        {
            let kb = key_value.to_bits();
            let is_numeric_key =
                (kb >> 48) == 0x7FFE || crate::value::JSValue::from_bits(kb).is_number() || {
                    let sp = crate::value::js_get_string_pointer_unified(key_value)
                        as *const crate::StringHeader;
                    !sp.is_null()
                        && super::super::has_own_helpers::str_from_string_header(sp)
                            .map(|n| !n.is_empty() && n.bytes().all(|b| b.is_ascii_digit()))
                            .unwrap_or(false)
                };
            if is_numeric_key {
                let ob = obj_value.to_bits();
                if (ob >> 48) == 0x7FFD {
                    crate::array::note_object_prototype_index_write(
                        (ob & crate::value::POINTER_MASK) as usize,
                    );
                }
            }
        }

        // #2817: ES Object.defineProperty validation.
        //   1. Target must be an object (or class-ref / function — all objects
        //      in Node). Primitives / null / undefined throw.
        //   2. Descriptor must be an object; otherwise
        //      `Property description must be an object: <desc>`.
        //   3. Accessor + data fields can't be mixed.
        //   4. Present `get`/`set` must be callable.
        let target_is_class_ref = super::super::class_ref_id(obj_value).is_some();
        if !target_is_class_ref && !value_is_object_like(obj_value) {
            // A native HANDLE target (a pointer-tagged registry id — a zlib
            // stream, a fetch Request/Response/Headers/Blob, a crypto hash, an
            // http ServerResponse, a timer) is not a heap `ObjectHeader`, so it
            // fails `value_is_object_like`. In Node these are ORDINARY,
            // extensible objects and `Object.defineProperty(handle, …)` is
            // everyday code (Next.js `patchSetHeaderWithCookieSupport` marks
            // `res` with a Symbol; libraries add non-enumerable metadata all the
            // time). Route the define to the handle's own-property storage
            // instead of throwing — see `define_property_on_handle`.
            //
            // #6363: the band test here was a hand-typed `p < 0x10000` — one zero
            // short of `HANDLE_BAND_MAX` (0x100000), so only the LOW common
            // registry (crypto, timers, sockets) was recognised. The fetch band
            // (0x40000..0xE0000) and the zlib band (0xE0000..0xF0000) fell past it
            // and hit the `throw` below: `Object.defineProperty(gzipStream, …)` /
            // `(headers, …)` raised a bogus TypeError. Use the centralized
            // predicate — the same correction `js_object_set_field_by_name` and
            // `js_delete_property` already carry.
            let jv = crate::value::JSValue::from_bits(obj_value.to_bits());
            let handle_id = jv
                .is_pointer()
                .then(|| jv.as_pointer::<u8>() as usize)
                .filter(|p| crate::value::addr_class::is_small_handle(*p));
            if let Some(hid) = handle_id {
                return define_property_on_handle(
                    obj_value,
                    hid as i64,
                    key_value,
                    descriptor_value,
                );
            }
            throw_object_type_error(b"Object.defineProperty called on non-object");
        }
        // A descriptor must be an Object; a Symbol is pointer-tagged but not an
        // object, so `ToPropertyDescriptor(Symbol())` throws (test262
        // property-description-must-be-an-object-not-symbol).
        if !value_is_object_like(descriptor_value)
            || crate::symbol::js_is_symbol(descriptor_value) != 0
        {
            let desc = describe_value_for_type_error(descriptor_value);
            throw_object_type_error_with_suffix("Property description must be an object: ", &desc);
        }
        validate_property_descriptor(descriptor_value);

        // TypedArrays are Integer-Indexed exotic objects: a canonical numeric
        // index key bypasses ordinary define entirely (validate the index, then
        // either write the element or reject with a TypeError).
        match super::super::typed_array_define_own_property(obj_value, key_value, descriptor_value)
        {
            super::super::TypedArrayDefineOutcome::Defined => return obj_value,
            super::super::TypedArrayDefineOutcome::Rejected => {
                throw_object_type_error(b"Cannot redefine property")
            }
            super::super::TypedArrayDefineOutcome::NotTypedArray => {}
        }

        // Date / RegExp / Error instances are exotic cells, not
        // `ObjectHeader`s — the ordinary define path below would bit-cast
        // them and corrupt memory. Route through the expando-aware
        // [[DefineOwnProperty]] (side-table storage + attrs + accessors).
        if let Some((addr, kind)) =
            super::super::exotic_expando::exotic_expando_kind_of_value(obj_value)
        {
            if crate::symbol::js_is_symbol(key_value) != 0 {
                let value_field = desc_read_field(descriptor_value, b"value");
                crate::symbol::js_object_set_symbol_property(
                    obj_value,
                    key_value,
                    f64::from_bits(value_field.bits()),
                );
                return obj_value;
            }
            if let Some(name) = super::super::metadata_key_to_string(key_value) {
                super::super::exotic_expando::exotic_define_own_property(
                    addr,
                    kind,
                    &name,
                    descriptor_value,
                );
            }
            return obj_value;
        }

        // #2159: when the receiver is a class-ref (`Class.prototype` evaluates
        // back to the class itself in Perry — see `class_ref_id` /
        // `js_object_get_own_property_descriptor`'s class-ref arm), route the
        // descriptor through the class-vtable / prototype-method side tables
        // so instance lookups (`new C().method`) see the new entry. Drizzle's
        // `applyMixins(Base, [Mixin])` copies methods between class
        // prototypes via `Object.defineProperty(Base.prototype, name,
        // Object.getOwnPropertyDescriptor(Mixin.prototype, name))` — pre-fix
        // the call hit `extract_obj_ptr → null` (a class-ref isn't a pointer)
        // and silently dropped the descriptor, so `await
        // db.select().from(x)` saw `instance.then === undefined` and `await`
        // unwrapped the builder unchanged.
        if let Some(target_cid) = super::super::class_ref_id(obj_value) {
            // `Object.defineProperty(C, Symbol.hasInstance, { value: fn })` (and
            // any symbol-keyed static define on a class): `metadata_key_to_string`
            // can't stringify a Symbol, so the value would be silently dropped.
            // Route it into the class static-symbol table (CLASS_STATIC_SYMBOLS) —
            // the same table `static [Symbol.hasInstance]` registers into and that
            // `js_instanceof` consults — so `x instanceof C` honors the user hook
            // (zod 4 installs its brand-check `@@hasInstance` exactly this way).
            if crate::symbol::js_is_symbol(key_value) != 0 {
                // Gate on descriptor-field *presence*, not on the value being
                // non-`undefined`: `Object.defineProperty(C, sym, { value: undefined })`
                // must still register an own entry. A generic redefine like
                // `{ enumerable: true }` (no `value`) leaves any existing entry intact.
                if desc_has_field(descriptor_value, b"value") {
                    let value_field = desc_read_field(descriptor_value, b"value");
                    crate::symbol::js_class_register_static_symbol(
                        target_cid,
                        key_value,
                        f64::from_bits(value_field.bits()),
                    );
                }
                return obj_value;
            }
            if let Some(name) = super::super::metadata_key_to_string(key_value) {
                let desc_ptr = extract_obj_ptr(descriptor_value);
                if !desc_ptr.is_null() {
                    let value_key = crate::string::js_string_from_bytes(b"value".as_ptr(), 5);
                    let value_field =
                        js_object_get_field_by_name(desc_ptr as *const ObjectHeader, value_key);
                    if !value_field.is_undefined() {
                        // #5024 followup: a `defineProperty` data descriptor is
                        // non-enumerable unless it explicitly sets
                        // `enumerable: true`. Record that so the prototype-object
                        // mirror (reflective `Object.keys`/`for-in`) doesn't
                        // surface it — `Class.prototype.m = fn` assignment, which
                        // routes through the same side table, stays enumerable.
                        super::super::class_registry::class_prototype_method_set_enumerable(
                            target_cid,
                            &name,
                            descriptor_enumerable(descriptor_value),
                        );
                        define_class_prototype_method(target_cid, &name, value_field.bits());
                    }
                }
            }
            return obj_value;
        }

        // Closures are object-like but not ObjectHeader-backed, so descriptor
        // writes have to route through the closure property side tables.
        let target_closure_ptr = {
            let value = crate::value::JSValue::from_bits(obj_value.to_bits());
            let raw = if value.is_pointer() {
                value.as_pointer::<u8>() as usize
            } else {
                let bits = obj_value.to_bits();
                if bits != 0 && bits <= 0x0000_FFFF_FFFF_FFFF && bits > 0x10000 {
                    bits as usize
                } else {
                    0
                }
            };
            if raw >= 0x10000 && crate::closure::is_closure_ptr(raw) {
                Some(raw)
            } else {
                None
            }
        };
        if let Some(closure_ptr) = target_closure_ptr {
            // A Symbol key on a function value (zod 4 installs its `instanceof`
            // brand check via `Object.defineProperty(ZodTypeFn, Symbol.hasInstance,
            // { value })`). Route into the SAME symbol side table
            // (`SYMBOL_PROPERTIES`, keyed by the closure pointer) that
            // `js_object_has_own_symbol` / `js_object_get_symbol_property` read —
            // string-coercing the symbol (below) would file it under a
            // "Symbol(...)" STRING key, unreachable by the symbol-keyed reader and
            // breaking the function-RHS `@@hasInstance` instanceof hook. Mirrors
            // the typed-array symbol-define branch below.
            if crate::symbol::js_is_symbol(key_value) != 0 {
                let desc_ptr = extract_obj_ptr(descriptor_value);
                if !desc_ptr.is_null() {
                    let has_get = desc_has_field(descriptor_value, b"get");
                    let has_set = desc_has_field(descriptor_value, b"set");
                    if has_get || has_set {
                        let get_field = desc_read_field(descriptor_value, b"get");
                        let set_field = desc_read_field(descriptor_value, b"set");
                        let get_bits = if !has_get || get_field.is_undefined() {
                            0
                        } else {
                            crate::closure::clone_closure_rebind_this(get_field.bits(), obj_value)
                        };
                        let set_bits = if !has_set || set_field.is_undefined() {
                            0
                        } else {
                            crate::closure::clone_closure_rebind_this(set_field.bits(), obj_value)
                        };
                        crate::symbol::set_symbol_accessor_property(
                            obj_value, key_value, get_bits, set_bits,
                        );
                    } else if desc_has_field(descriptor_value, b"value") {
                        // Only write a value when the descriptor actually carries
                        // one. A generic redefine like `{ enumerable: true }` must
                        // preserve the existing `fn[sym]` rather than clobber it
                        // with `undefined`. (`value: undefined` is honored — it is
                        // a present field.)
                        let value_field = desc_read_field(descriptor_value, b"value");
                        crate::symbol::js_object_set_symbol_property(
                            obj_value,
                            key_value,
                            f64::from_bits(value_field.bits()),
                        );
                    }
                }
                return obj_value;
            }
            let key_str = crate::builtins::js_string_coerce(key_value);
            if key_str.is_null() {
                return obj_value;
            }
            let key_rust: Option<String> = {
                let name_ptr =
                    (key_str as *const u8).add(std::mem::size_of::<crate::StringHeader>());
                let name_len = (*key_str).byte_len as usize;
                let name_bytes = std::slice::from_raw_parts(name_ptr, name_len);
                std::str::from_utf8(name_bytes).ok().map(|s| s.to_string())
            };
            let Some(key_rust) = key_rust else {
                return obj_value;
            };
            let desc_ptr = extract_obj_ptr(descriptor_value);
            if desc_ptr.is_null() {
                return obj_value;
            }

            // Spec retention: redefining an existing own property keeps the
            // attributes the descriptor omits (see the object-path comment).
            let existing_attrs: Option<PropertyAttrs> =
                if super::super::has_own_helpers::closure_own_key_present(closure_ptr, &key_rust) {
                    Some(
                        super::super::get_property_attrs(closure_ptr, &key_rust)
                            .unwrap_or_else(|| PropertyAttrs::new(true, true, true)),
                    )
                } else {
                    None
                };

            // ValidateAndApplyPropertyDescriptor: a non-configurable existing own
            // property of a function object can only be redefined within the
            // spec-permitted bounds (#2843). The built-in `name`/`length` slots
            // are configurable per spec, so a redefine of those still flows
            // through unguarded. The shared core mirrors the plain-object path.
            if let Some(cur_attrs) = existing_attrs {
                if !cur_attrs.configurable() {
                    let cur_accessor =
                        super::super::get_accessor_descriptor(closure_ptr, &key_rust);
                    let cur_value = if cur_accessor.is_none() {
                        crate::closure::closure_get_dynamic_prop(closure_ptr, &key_rust)
                    } else {
                        f64::from_bits(crate::value::TAG_UNDEFINED)
                    };
                    validate_nonconfigurable_redefine(
                        &key_rust,
                        cur_attrs,
                        cur_accessor,
                        cur_value,
                        descriptor_value,
                    );
                }
            }

            let get_key = crate::string::js_string_from_bytes(b"get".as_ptr(), 3);
            let set_key = crate::string::js_string_from_bytes(b"set".as_ptr(), 3);
            let get_field = js_object_get_field_by_name(desc_ptr as *const ObjectHeader, get_key);
            let set_field = js_object_get_field_by_name(desc_ptr as *const ObjectHeader, set_key);
            let has_accessor = !get_field.is_undefined() || !set_field.is_undefined();

            if has_accessor {
                let get_bits = if get_field.is_undefined() {
                    0
                } else {
                    crate::closure::clone_closure_rebind_this(get_field.bits(), obj_value)
                };
                let set_bits = if set_field.is_undefined() {
                    0
                } else {
                    crate::closure::clone_closure_rebind_this(set_field.bits(), obj_value)
                };
                set_accessor_descriptor(
                    closure_ptr,
                    key_rust.clone(),
                    AccessorDescriptor {
                        get: get_bits,
                        set: set_bits,
                    },
                );
            } else {
                let value_key = crate::string::js_string_from_bytes(b"value".as_ptr(), 5);
                let value_field =
                    js_object_get_field_by_name(desc_ptr as *const ObjectHeader, value_key);
                ACCESSOR_DESCRIPTORS.with(|m| {
                    m.borrow_mut().remove(&(closure_ptr, key_rust.clone()));
                });
                if !value_field.is_undefined() {
                    crate::closure::closure_set_dynamic_prop(
                        closure_ptr,
                        &key_rust,
                        f64::from_bits(value_field.bits()),
                    );
                }
            }

            let read_bool = |name: &[u8]| -> Option<bool> {
                let k = crate::string::js_string_from_bytes(name.as_ptr(), name.len() as u32);
                let v = js_object_get_field_by_name(desc_ptr as *const ObjectHeader, k);
                if v.is_undefined() {
                    None
                } else {
                    Some(crate::value::js_is_truthy(f64::from_bits(v.bits())) != 0)
                }
            };
            let writable = read_bool(b"writable")
                .unwrap_or_else(|| existing_attrs.map(|a| a.writable()).unwrap_or(has_accessor));
            let enumerable = read_bool(b"enumerable")
                .unwrap_or_else(|| existing_attrs.map(|a| a.enumerable()).unwrap_or(false));
            let configurable = read_bool(b"configurable")
                .unwrap_or_else(|| existing_attrs.map(|a| a.configurable()).unwrap_or(false));
            set_property_attrs(
                closure_ptr,
                key_rust,
                PropertyAttrs::new(writable, enumerable, configurable),
            );
            return obj_value;
        }

        if let Some(addr) = crate::typedarray_props::typed_array_addr_from_value(obj_value) {
            // A Symbol key on a TypedArray is an ORDINARY define — store it in
            // the symbol side tables (string-coercing it would file the value
            // under a "Symbol(x)" string name, unreachable via `ta[sym]`),
            // honoring accessor descriptors and recording the attributes
            // (defineProperty defaults absent fields to false, unlike a plain
            // `ta[sym] = v` write). Mirrors the generic symbol-define block.
            if crate::symbol::js_is_symbol(key_value) != 0 {
                let desc_ptr = extract_obj_ptr(descriptor_value);
                if desc_ptr.is_null() {
                    return obj_value;
                }
                let has_get = desc_has_field(descriptor_value, b"get");
                let has_set = desc_has_field(descriptor_value, b"set");
                let has_accessor = has_get || has_set;
                if has_accessor {
                    let get_field = desc_read_field(descriptor_value, b"get");
                    let set_field = desc_read_field(descriptor_value, b"set");
                    let get_bits = if !has_get || get_field.is_undefined() {
                        0
                    } else {
                        crate::closure::clone_closure_rebind_this(get_field.bits(), obj_value)
                    };
                    let set_bits = if !has_set || set_field.is_undefined() {
                        0
                    } else {
                        crate::closure::clone_closure_rebind_this(set_field.bits(), obj_value)
                    };
                    crate::symbol::set_symbol_accessor_property(
                        obj_value, key_value, get_bits, set_bits,
                    );
                } else {
                    let value_field = desc_read_field(descriptor_value, b"value");
                    crate::symbol::js_object_set_symbol_property(
                        obj_value,
                        key_value,
                        f64::from_bits(value_field.bits()),
                    );
                }
                let read_flag = |name: &[u8]| -> Option<bool> {
                    if !desc_has_field(descriptor_value, name) {
                        return None;
                    }
                    let v = desc_read_field(descriptor_value, name);
                    Some(crate::value::js_is_truthy(f64::from_bits(v.bits())) != 0)
                };
                let owner = crate::symbol::obj_key_from_f64(obj_value);
                let sym_key = crate::symbol::sym_key_from_f64(key_value);
                crate::symbol::set_symbol_property_attrs(
                    owner,
                    sym_key,
                    PropertyAttrs::new(
                        read_flag(b"writable").unwrap_or(has_accessor),
                        read_flag(b"enumerable").unwrap_or(false),
                        read_flag(b"configurable").unwrap_or(false),
                    ),
                );
                return obj_value;
            }
            let key_str = crate::builtins::js_string_coerce(key_value);
            if key_str.is_null() {
                return obj_value;
            }
            let key_rust: Option<String> = {
                let name_ptr =
                    (key_str as *const u8).add(std::mem::size_of::<crate::StringHeader>());
                let name_len = (*key_str).byte_len as usize;
                let name_bytes = std::slice::from_raw_parts(name_ptr, name_len);
                std::str::from_utf8(name_bytes).ok().map(|s| s.to_string())
            };
            if let Some(ref key_name) = key_rust {
                return crate::typedarray_props::typed_array_define_own_property(
                    obj_value,
                    addr as *mut crate::typedarray::TypedArrayHeader,
                    key_str,
                    key_name,
                    descriptor_value,
                );
            }
            return obj_value;
        }

        let obj = extract_obj_ptr(obj_value);
        if obj.is_null() {
            return obj_value;
        }
        // #1250: when the key is a Symbol, route into the symbol side
        // table (`SYMBOL_PROPERTIES`) the same way `obj[sym] = value`
        // does. Without this, `Object.defineProperty(obj, sym, ...)`
        // would drop the symbol and try to coerce it to a string,
        // which is exactly the failure mode reported for
        // `Object.defineProperty(obj, inspect.custom, …)`.
        let key_bits = key_value.to_bits();
        let key_tag = key_bits & 0xFFFF_0000_0000_0000;
        if key_tag == 0x7FFD_0000_0000_0000 {
            let raw_ptr = (key_bits & 0x0000_FFFF_FFFF_FFFF) as *const crate::symbol::SymbolHeader;
            if !raw_ptr.is_null()
                && (raw_ptr as usize) >= 0x1000
                && (*raw_ptr).magic == crate::symbol::SYMBOL_MAGIC
            {
                let desc_ptr = extract_obj_ptr(descriptor_value);
                if !desc_ptr.is_null() {
                    let get_key = crate::string::js_string_from_bytes(b"get".as_ptr(), 3);
                    let set_key = crate::string::js_string_from_bytes(b"set".as_ptr(), 3);
                    let get_field =
                        js_object_get_field_by_name(desc_ptr as *const ObjectHeader, get_key);
                    let set_field =
                        js_object_get_field_by_name(desc_ptr as *const ObjectHeader, set_key);
                    let has_get = own_key_present(desc_ptr, get_key);
                    let has_set = own_key_present(desc_ptr, set_key);
                    let has_accessor = has_get || has_set;
                    if has_accessor {
                        let get_bits = if !has_get || get_field.is_undefined() {
                            0
                        } else {
                            crate::closure::clone_closure_rebind_this(get_field.bits(), obj_value)
                        };
                        let set_bits = if !has_set || set_field.is_undefined() {
                            0
                        } else {
                            crate::closure::clone_closure_rebind_this(set_field.bits(), obj_value)
                        };
                        crate::symbol::set_symbol_accessor_property(
                            obj_value, key_value, get_bits, set_bits,
                        );
                    } else {
                        let value_key = crate::string::js_string_from_bytes(b"value".as_ptr(), 5);
                        if own_key_present(desc_ptr, value_key) {
                            let value_field = js_object_get_field_by_name(
                                desc_ptr as *const ObjectHeader,
                                value_key,
                            );
                            crate::symbol::js_object_set_symbol_property(
                                obj_value,
                                key_value,
                                f64::from_bits(value_field.bits()),
                            );
                        }
                    }
                    let read_bool = |name: &[u8]| -> Option<bool> {
                        let k =
                            crate::string::js_string_from_bytes(name.as_ptr(), name.len() as u32);
                        let v = js_object_get_field_by_name(desc_ptr as *const ObjectHeader, k);
                        if v.is_undefined() {
                            None
                        } else {
                            Some(crate::value::js_is_truthy(f64::from_bits(v.bits())) != 0)
                        }
                    };
                    let writable = read_bool(b"writable").unwrap_or(has_accessor);
                    let enumerable = read_bool(b"enumerable").unwrap_or(false);
                    let configurable = read_bool(b"configurable").unwrap_or(false);
                    crate::symbol::set_symbol_property_attrs(
                        obj as usize,
                        raw_ptr as usize,
                        PropertyAttrs::new(writable, enumerable, configurable),
                    );
                }
                return obj_value;
            }
        }
        // Extract key string
        let key_str = crate::builtins::js_string_coerce(key_value);
        if key_str.is_null() {
            return obj_value;
        }
        // Extract the key as a Rust string for the descriptor side-table lookup.
        let key_rust: Option<String> = {
            let name_ptr = (key_str as *const u8).add(std::mem::size_of::<crate::StringHeader>());
            let name_len = (*key_str).byte_len as usize;
            let name_bytes = std::slice::from_raw_parts(name_ptr, name_len);
            std::str::from_utf8(name_bytes).ok().map(|s| s.to_string())
        };
        // #4949 / #2159 follow-up: `ClassExprFresh.prototype` now materializes
        // the declared-class prototype object. Keep `Object.defineProperty` on
        // that live object wired to the same prototype-method side tables used
        // by the historical ClassRef path, so instances observe decorator/mixin
        // method replacements.
        if let Some(target_cid) =
            super::super::class_registry::class_id_for_decl_prototype_object(obj as usize)
        {
            if let Some(ref name) = key_rust {
                if desc_has_field(descriptor_value, b"value") {
                    let value_field = desc_read_field(descriptor_value, b"value");
                    if !value_field.is_undefined() {
                        // #5024 followup: defineProperty data descriptor is
                        // non-enumerable unless it sets `enumerable: true`. Mark
                        // it so the prototype-method enumeration mirror honours
                        // the descriptor instead of defaulting to enumerable
                        // (the `Class.prototype.m = fn` assignment default).
                        super::super::class_registry::class_prototype_method_set_enumerable(
                            target_cid,
                            name,
                            descriptor_enumerable(descriptor_value),
                        );
                        define_class_prototype_method(target_cid, name, value_field.bits());
                    }
                }
            }
        }
        if crate::typedarray::lookup_typed_array_kind(obj as usize).is_some() {
            if let Some(ref key_name) = key_rust {
                return crate::typedarray_props::typed_array_define_own_property(
                    obj_value,
                    obj as *mut crate::typedarray::TypedArrayHeader,
                    key_str,
                    key_name,
                    descriptor_value,
                );
            }
            return obj_value;
        }
        if let Some(ok) = super::super::define_array_property(
            obj,
            obj_value,
            key_str,
            key_rust.as_deref(),
            descriptor_value,
        ) {
            if ok {
                return obj_value;
            }
            // A rejected array `[[DefineOwnProperty]]` (e.g. redefining the
            // non-configurable / non-writable `length`, or a forbidden change to
            // a non-configurable index property) throws under
            // `Object.defineProperty`.
            let k = key_rust.as_deref().unwrap_or("length");
            throw_object_type_error_with_suffix("Cannot redefine property: ", k);
        }
        // #2843: enforce frozen / sealed / non-extensible invariants BEFORE any
        // mutation, so a rejected definition leaves the object untouched and the
        // thrown TypeError matches Node.
        if let Some(ref k) = key_rust {
            enforce_define_property_invariants(obj, key_str, k, descriptor_value);
        }
        super::super::mark_object_dynamic_shape_unknown(obj);
        // Extract descriptor object
        let desc_ptr = extract_obj_ptr(descriptor_value);
        if desc_ptr.is_null() {
            return obj_value;
        }

        // Spec (OrdinaryDefineOwnProperty / ValidateAndApplyPropertyDescriptor):
        // when the property ALREADY EXISTS as an own property, attribute fields
        // the descriptor omits must RETAIN the property's current values — they do
        // NOT reset to the new-property `false` default. Capture the current
        // attributes before any mutation below. `None` ⇒ the key is new, so the
        // historical all-`false` (writable defaults to `has_accessor`) applies.
        let existing_attrs: Option<PropertyAttrs> = key_rust.as_ref().and_then(|k| {
            // #6743: wide objects answer own-key presence via the O(1) sidecar
            // (repeated defines were O(N²) through this check); narrow or
            // non-indexable receivers keep the general path.
            let present = own_key_present_via_index(obj, key_str)
                .unwrap_or_else(|| super::super::obj_value_has_own_key(obj_value, key_value));
            if present {
                Some(
                    super::super::get_property_attrs(obj as usize, k)
                        .unwrap_or_else(|| PropertyAttrs::new(true, true, true)),
                )
            } else {
                None
            }
        });

        // Detect accessor descriptor (has `get` and/or `set`) vs. data
        // descriptor (has `value`/`writable`) by `ToPropertyDescriptor` field
        // PRESENCE (HasProperty — own OR inherited) on the descriptor object,
        // not by `is_undefined`: `{ get: undefined }` is an explicit (present)
        // accessor field, and an *inherited* `value`/`get` counts as present.
        let desc_has_get = desc_has_field(descriptor_value, b"get");
        let desc_has_set = desc_has_field(descriptor_value, b"set");
        let get_field = desc_read_field(descriptor_value, b"get");
        let set_field = desc_read_field(descriptor_value, b"set");
        let has_accessor = desc_has_get || desc_has_set;

        // The existing accessor (if the property is currently an accessor) —
        // used to retain `get`/`set` fields the redefining descriptor omits.
        let existing_accessor: Option<AccessorDescriptor> = key_rust
            .as_ref()
            .and_then(|k| super::super::get_accessor_descriptor(obj as usize, k));

        if has_accessor {
            // Store the accessor closures in the side table. Ensure the key is present
            // in the object's keys_array so lookups (hasOwn, getOwnPropertyDescriptor,
            // keys) can see it.
            ensure_key_in_keys_array(obj, key_str);
            if let Some(k) = key_rust.clone() {
                // Issue #450: spec says the getter/setter runs with `this === obj`
                // (the property access target). The user's descriptor literal
                // `{ get() {...}, set() {...} }` was lowered with `captures_this: true`
                // and had its reserved `this` slot patched to point to the *descriptor*
                // object at construction time — that's what every other object-literal
                // method does. Clone the closure once at defineProperty time and
                // rebind `this` to `obj`, so every subsequent get/set call sees the
                // correct receiver. Closures without CAPTURES_THIS_FLAG (e.g. arrow-form
                // `get: () => this._backing` written as a field rather than a method
                // shorthand) pass through unchanged.
                //
                // Spec retention (ValidateAndApplyPropertyDescriptor): redefining
                // an existing accessor with a descriptor that omits `get` (or
                // `set`) keeps the current accessor's `get` (or `set`). When the
                // current property is a data property being converted to an
                // accessor, omitted fields default to `undefined` (0).
                let recv_box = crate::value::js_nanbox_pointer(obj as i64);
                let prior = existing_accessor;
                let get_bits = if desc_has_get {
                    if get_field.is_undefined() {
                        0u64
                    } else {
                        crate::closure::clone_closure_rebind_this(get_field.bits(), recv_box)
                    }
                } else {
                    prior.map(|a| a.get).unwrap_or(0)
                };
                let set_bits = if desc_has_set {
                    if set_field.is_undefined() {
                        0u64
                    } else {
                        crate::closure::clone_closure_rebind_this(set_field.bits(), recv_box)
                    }
                } else {
                    prior.map(|a| a.set).unwrap_or(0)
                };
                set_accessor_descriptor(
                    obj as usize,
                    k,
                    AccessorDescriptor {
                        get: get_bits,
                        set: set_bits,
                    },
                );
            }
        } else {
            // Either a data descriptor (`value`/`writable` present) or a generic
            // descriptor (only `enumerable`/`configurable`). Detect by own-field
            // presence so `{ value: undefined }` (present) stores `undefined`,
            // while a generic descriptor on an existing accessor leaves it intact.
            let desc_has_value = desc_has_field(descriptor_value, b"value");
            let desc_has_writable = desc_has_field(descriptor_value, b"writable");
            let is_data = desc_has_value || desc_has_writable;

            if is_data {
                // Converting to / redefining as a data property. Clear any
                // existing accessor for this key so the write doesn't fire the
                // setter, and clear any stale per-key descriptor so a prior
                // `writable: false` doesn't reject the forced store below. The
                // final attributes are (re)applied a few lines down.
                if let Some(ref k) = key_rust {
                    ACCESSOR_DESCRIPTORS.with(|m| {
                        m.borrow_mut().remove(&(obj as usize, k.clone()));
                    });
                    clear_property_attrs(obj as usize, k);
                }
                let value_field = desc_read_field(descriptor_value, b"value");
                // Ensure the key exists; store the (possibly `undefined`) value
                // via `[[DefineOwnProperty]]`, bypassing the `[[Set]]` writability
                // / frozen guard (invariants already enforced above). When
                // `value` is omitted (a `{ writable: ... }`-only descriptor on a
                // brand-new property) the value defaults to `undefined`.
                if desc_has_value {
                    define_property_force_store_value(
                        obj,
                        key_str,
                        f64::from_bits(value_field.bits()),
                    );
                } else if existing_accessor.is_some() {
                    // Accessor → data with no `value`: the value becomes the
                    // data default `undefined`.
                    define_property_force_store_value(
                        obj,
                        key_str,
                        f64::from_bits(crate::value::TAG_UNDEFINED),
                    );
                } else {
                    ensure_key_in_keys_array(obj, key_str);
                }
            } else {
                // Generic descriptor: no value/writable/get/set. It only adjusts
                // enumerable/configurable and never converts the property kind.
                // Leave any existing accessor / data value untouched; just make
                // sure the key is present (for a brand-new generic define).
                ensure_key_in_keys_array(obj, key_str);
            }
        }

        // Read attribute flags from descriptor. JS defaults when omitted in
        // `Object.defineProperty` are `false` (NOT `true` like for direct assignment).
        let read_bool = |name: &[u8]| -> Option<bool> {
            let v = desc_read_field(descriptor_value, name);
            if v.is_undefined() {
                None
            } else {
                Some(crate::value::js_is_truthy(f64::from_bits(v.bits())) != 0)
            }
        };
        // Omitted attributes default to the EXISTING property's value when
        // redefining (spec retention, see `existing_attrs` above), else to
        // `false` for a new property. Accessor descriptors don't carry
        // `writable`; for a brand-new accessor we leave it `true` (via
        // `has_accessor`) so data lookups before the accessor override don't
        // reject a legitimate fallthrough write.
        //
        // Accessor → data conversion: the current property has no
        // [[Writable]], so an omitted `writable` defaults to FALSE (the
        // retained-attrs rule doesn't apply across the kind switch).
        let accessor_to_data = existing_accessor.is_some()
            && !has_accessor
            && (desc_has_field(descriptor_value, b"value")
                || desc_has_field(descriptor_value, b"writable"));
        let writable = read_bool(b"writable").unwrap_or_else(|| {
            if accessor_to_data {
                false
            } else {
                existing_attrs.map(|a| a.writable()).unwrap_or(has_accessor)
            }
        });
        let enumerable = read_bool(b"enumerable")
            .unwrap_or_else(|| existing_attrs.map(|a| a.enumerable()).unwrap_or(false));
        let configurable = read_bool(b"configurable")
            .unwrap_or_else(|| existing_attrs.map(|a| a.configurable()).unwrap_or(false));

        if let Some(k) = key_rust {
            set_property_attrs(
                obj as usize,
                k,
                PropertyAttrs::new(writable, enumerable, configurable),
            );
        }
        super::super::arguments_object_after_define(obj, key_str, descriptor_value);
        // Return the object
        obj_value
    }
}
