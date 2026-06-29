use super::super::*;
use super::disposal::*;
use super::object_proto::*;
use super::proto_dispatch::*;
use super::typed_array::*;
use super::*;

pub(super) unsafe fn dispatch_common(
    root_scope: &crate::gc::RuntimeHandleScope,
    object_handle: &crate::gc::RuntimeHandle,
    arg_handles: &[crate::gc::RuntimeHandle],
    object: f64,
    method_name: &str,
    method_name_ptr: *const i8,
    method_name_len: usize,
    args_ptr: *const f64,
    args_len: usize,
) -> Option<f64> {
    let jsval = JSValue::from_bits(object.to_bits());
    let raw_bits = object.to_bits();
    let refreshed_args = || crate::gc::RuntimeHandleScope::refreshed_nanbox_f64_slice(arg_handles);
    let _ = (root_scope, object_handle, &refreshed_args, raw_bits, jsval);
    let _ = (method_name_ptr, method_name_len);
    // Handle common method calls
    match method_name {
        // Function.prototype.bind(thisArg, ...boundArgs) — create a distinct
        // bound function with a fixed `this`, prepended partial args, and an
        // adjusted `.name`/`.length` (#2840). For closure receivers route to
        // the runtime bind helper; non-closure receivers fall back to the
        // prior conservative behavior of returning the receiver unchanged.
        "bind" => {
            let raw_ptr = (object.to_bits() & 0x0000_FFFF_FFFF_FFFF) as usize;
            if jsval.is_pointer() && crate::closure::is_closure_ptr(raw_ptr) {
                return Some(crate::closure::js_function_bind(object, args_ptr, args_len));
            }
            // #3662: a non-callable `this` (primitive or recognized plain
            // object) is a spec `TypeError` — `Function.prototype.bind.call(x)`.
            // Ambiguous pointers (possible native callables) keep the prior
            // conservative return-unchanged behavior.
            if fn_proto_receiver_not_callable(object) {
                throw_fn_proto_not_callable("bind");
            }
            return Some(object);
        }

        // `obj.hasOwnProperty(key)` — duck-types as truthy for any
        // non-null/undefined receiver where the field-scan and class
        // dispatch above couldn't find a user-defined override. Walking
        // the actual key set on every shape (ObjectHeader fields,
        // closure dynamic props, array keys, …) is more work than this
        // entry point is meant to do; ramda's `_clone` / `_has` only
        // need a non-throwing return so the surrounding pattern doesn't
        // fall into the spec gap. Pre-fix, the chained
        // `Object.prototype.hasOwnProperty.call(obj, key)` reads
        // `Object.prototype.hasOwnProperty` as `undefined` from the
        // empty proto and threw `value is not a function` at module
        // init in `_clone.js` / `_isArguments.js`.
        "hasOwnProperty" => {
            if jsval.is_undefined() || jsval.is_null() {
                return Some(f64::from_bits(JSValue::bool(false).bits()));
            }
            // ToPropertyKey(V) (19.1.3.3 step 1) BEFORE the string coercion
            // below: an object argument whose `toString`/`valueOf` yields a
            // Symbol must be treated as that Symbol, not stringified to
            // "[object Object]" (test262 hasOwnProperty/symbol_property_*). A
            // resolved Symbol key routes through the symbol-aware own-property
            // check in the canonical entry point. The conversion runs once here
            // so a user `toString` is invoked exactly once.
            let key_value = if args_len >= 1 && !args_ptr.is_null() {
                *args_ptr
            } else {
                f64::from_bits(crate::value::TAG_UNDEFINED)
            };
            let key_value = crate::object::js_to_property_key(key_value);
            if crate::symbol::js_is_symbol(key_value) != 0 {
                return Some(super::object_ops::js_object_has_own(object, key_value));
            }
            if (object.to_bits() >> 48) == 0x7FFE {
                let key_str = crate::builtins::js_string_coerce(key_value);
                let class_id = (object.to_bits() & 0xFFFF_FFFF) as u32;
                let present = if key_str.is_null() {
                    false
                } else {
                    super::has_own_helpers::str_from_string_header(key_str)
                        .map(|key| {
                            matches!(key, "length" | "name" | "prototype")
                                && !super::class_registry::class_is_key_deleted(class_id, key)
                        })
                        .unwrap_or(false)
                };
                return Some(f64::from_bits(JSValue::bool(present).bits()));
            }
            if jsval.is_pointer() {
                let key_str = crate::builtins::js_string_coerce(key_value);
                if key_str.is_null() {
                    return Some(f64::from_bits(JSValue::bool(false).bits()));
                }
                if let Some(class_id) = super::class_ref_id(object) {
                    let present = super::has_own_helpers::str_from_string_header(key_str)
                        .map(|key| {
                            if super::class_registry::class_is_key_deleted(class_id, key) {
                                false
                            } else if key == "name"
                                && super::class_registry::lookup_static_method_in_chain(
                                    class_id, key,
                                )
                                .is_none()
                            {
                                super::class_registry::class_name_for_id(class_id).is_some()
                            } else {
                                CLASS_DYNAMIC_PROPS.with(|m| {
                                    m.borrow()
                                        .get(&class_id)
                                        .is_some_and(|props| props.contains_key(key))
                                }) || super::class_registry::lookup_static_method_in_chain(
                                    class_id, key,
                                )
                                .is_some()
                            }
                        })
                        .unwrap_or(false);
                    return Some(f64::from_bits(JSValue::bool(present).bits()));
                }
                // #3655: a closure receiver (functions ARE objects). Report
                // the built-in `name`/`length` (+ constructor `prototype`)
                // and user props as own; honor `delete`. Without this, the
                // `is_valid_obj_ptr`-false fallthrough returned `true` for
                // *every* key (so a deleted slot still looked present).
                let raw = jsval.as_pointer::<u8>() as usize;
                if crate::buffer::is_registered_buffer(raw) {
                    let present = super::has_own_helpers::buffer_own_key_present(
                        raw as *const crate::buffer::BufferHeader,
                        key_str,
                    );
                    return Some(f64::from_bits(JSValue::bool(present).bits()));
                }
                if crate::closure::is_closure_ptr(raw) {
                    let present = super::has_own_helpers::str_from_string_header(key_str)
                        .map(|k| super::has_own_helpers::closure_own_key_present(raw, k))
                        .unwrap_or(false);
                    return Some(f64::from_bits(JSValue::bool(present).bits()));
                }
                // Date / RegExp / Error exotic receivers: own expando props
                // (side tables) + per-kind builtin own slots.
                if let Some(kind) = super::exotic_expando::exotic_expando_kind(raw) {
                    use super::exotic_expando::ExoticKind;
                    let present = super::has_own_helpers::str_from_string_header(key_str)
                        .map(|key| {
                            super::exotic_expando::exotic_has_own_property(kind, raw, key)
                                || match kind {
                                    ExoticKind::RegExp => key == "lastIndex",
                                    ExoticKind::Error => crate::error::js_error_has_own_property(
                                        raw as *mut crate::error::ErrorHeader,
                                        key,
                                    ),
                                    ExoticKind::Date
                                    | ExoticKind::Temporal
                                    | ExoticKind::Promise => false,
                                }
                        })
                        .unwrap_or(false);
                    return Some(f64::from_bits(JSValue::bool(present).bits()));
                }
                if raw >= crate::gc::GC_HEADER_SIZE + 0x1000 {
                    let gc_header = (raw as *const u8).sub(crate::gc::GC_HEADER_SIZE)
                        as *const crate::gc::GcHeader;
                    if (*gc_header).obj_type == crate::gc::GC_TYPE_ERROR {
                        let present = super::has_own_helpers::str_from_string_header(key_str)
                            .map(|key| {
                                crate::error::js_error_has_own_property(
                                    raw as *mut crate::error::ErrorHeader,
                                    key,
                                )
                            })
                            .unwrap_or(false);
                        return Some(f64::from_bits(JSValue::bool(present).bits()));
                    }
                    if (*gc_header).obj_type == crate::gc::GC_TYPE_ARRAY {
                        let present = super::has_own_helpers::array_own_key_present(
                            raw as *const crate::array::ArrayHeader,
                            key_str,
                        );
                        return Some(f64::from_bits(JSValue::bool(present).bits()));
                    }
                }
                let obj_ptr = jsval.as_pointer::<ObjectHeader>();
                if !obj_ptr.is_null() && is_valid_obj_ptr(obj_ptr as *const u8) {
                    // perry's hidden `__perry_collection_backing__` runtime-internal
                    // field lives in a class instance's keys_array but is never a
                    // reflectable own property — `hasOwnProperty` must report false.
                    if (*obj_ptr).class_id != 0 {
                        if let Some(key) = super::has_own_helpers::str_from_string_header(key_str) {
                            if crate::object::field_get_set::is_internal_runtime_key(key) {
                                return Some(f64::from_bits(JSValue::bool(false).bits()));
                            }
                        }
                    }
                    return Some(f64::from_bits(
                        JSValue::bool(own_key_present(obj_ptr as *mut ObjectHeader, key_str))
                            .bits(),
                    ));
                }
            }
            return Some(f64::from_bits(JSValue::bool(true).bits()));
        }

        // `obj.propertyIsEnumerable(key)` — same shape as
        // `hasOwnProperty`, but descriptor-aware for ordinary objects so
        // non-enumerable properties installed by Error.captureStackTrace /
        // Object.defineProperty report false.
        "propertyIsEnumerable" => {
            if jsval.is_undefined() || jsval.is_null() {
                return Some(f64::from_bits(JSValue::bool(false).bits()));
            }
            if !jsval.is_pointer() {
                return Some(f64::from_bits(JSValue::bool(false).bits()));
            }
            let key_value = if args_len >= 1 && !args_ptr.is_null() {
                *args_ptr
            } else {
                f64::from_bits(crate::value::TAG_UNDEFINED)
            };
            // ToPropertyKey(V) (19.1.3.4 step 1): an object argument whose
            // `toString`/`valueOf` yields a Symbol must be treated as that
            // Symbol (test262 propertyIsEnumerable/symbol_property_*), invoking
            // the user conversion exactly once.
            let key_value = crate::object::js_to_property_key(key_value);
            // Symbol keys must not be string-coerced — route through the
            // canonical entry, which consults the SYMBOL_PROPERTIES side
            // table (mirrors hasOwnProperty's symbol arm).
            if crate::symbol::js_is_symbol(key_value) != 0 {
                return Some(super::object_ops::js_object_property_is_enumerable(
                    object, key_value,
                ));
            }
            let key_str = crate::builtins::js_string_coerce(key_value);
            if key_str.is_null() {
                return Some(f64::from_bits(JSValue::bool(false).bits()));
            }
            // #3655: closure receiver — built-in slots are non-enumerable,
            // user props default enumerable. Mirrors the `js_object_property_is_enumerable`
            // entry point (the `.call`-lowered shape).
            let raw = jsval.as_pointer::<u8>() as usize;
            if crate::buffer::is_registered_buffer(raw) {
                let enumerable = super::has_own_helpers::str_from_string_header(key_str)
                    .and_then(super::canonical_array_index)
                    .is_some_and(|idx| {
                        let buf = raw as *const crate::buffer::BufferHeader;
                        idx < (*buf).length
                    });
                return Some(f64::from_bits(JSValue::bool(enumerable).bits()));
            }
            if crate::closure::is_closure_ptr(raw) {
                let Some(key_name) = super::has_own_helpers::str_from_string_header(key_str) else {
                    return Some(f64::from_bits(JSValue::bool(false).bits()));
                };
                if !super::has_own_helpers::closure_own_key_present(raw, key_name) {
                    return Some(f64::from_bits(JSValue::bool(false).bits()));
                }
                if matches!(key_name, "name" | "length" | "prototype") {
                    return Some(f64::from_bits(JSValue::bool(false).bits()));
                }
                let enumerable = get_property_attrs(raw, key_name)
                    .map(|attrs| attrs.enumerable())
                    .unwrap_or(true);
                return Some(f64::from_bits(JSValue::bool(enumerable).bits()));
            }
            if raw >= crate::gc::GC_HEADER_SIZE + 0x1000 {
                let gc_header =
                    (raw as *const u8).sub(crate::gc::GC_HEADER_SIZE) as *const crate::gc::GcHeader;
                if (*gc_header).obj_type == crate::gc::GC_TYPE_ERROR {
                    let Some(key_name) = super::has_own_helpers::str_from_string_header(key_str)
                    else {
                        return Some(f64::from_bits(JSValue::bool(false).bits()));
                    };
                    let enumerable = crate::error::js_error_builtin_own_property_is_enumerable(
                        raw as *mut crate::error::ErrorHeader,
                        key_name,
                    )
                    .unwrap_or(false);
                    return Some(f64::from_bits(JSValue::bool(enumerable).bits()));
                }
                if (*gc_header).obj_type == crate::gc::GC_TYPE_ARRAY {
                    let Some(key_name) = super::has_own_helpers::str_from_string_header(key_str)
                    else {
                        return Some(f64::from_bits(JSValue::bool(false).bits()));
                    };
                    if key_name == "length" {
                        return Some(f64::from_bits(JSValue::bool(false).bits()));
                    }
                    if !super::has_own_helpers::array_own_key_present(
                        raw as *const crate::array::ArrayHeader,
                        key_str,
                    ) {
                        return Some(f64::from_bits(JSValue::bool(false).bits()));
                    }
                    let enumerable = if crate::object::canonical_array_index(key_name).is_some() {
                        true
                    } else {
                        get_property_attrs(raw, key_name)
                            .map(|attrs| attrs.enumerable())
                            .unwrap_or(true)
                    };
                    return Some(f64::from_bits(JSValue::bool(enumerable).bits()));
                }
            }
            let obj_ptr = jsval.as_pointer::<ObjectHeader>();
            if obj_ptr.is_null() || !is_valid_obj_ptr(obj_ptr as *const u8) {
                return Some(f64::from_bits(JSValue::bool(false).bits()));
            }
            let name_ptr = (key_str as *const u8).add(std::mem::size_of::<crate::StringHeader>());
            let name_len = (*key_str).byte_len as usize;
            let key_name = match std::str::from_utf8(std::slice::from_raw_parts(name_ptr, name_len))
            {
                Ok(s) => s,
                Err(_) => return Some(f64::from_bits(JSValue::bool(false).bits())),
            };
            if (*obj_ptr).class_id == NATIVE_MODULE_CLASS_ID {
                if let Some(module_name) = read_native_module_name(obj_ptr) {
                    return Some(f64::from_bits(
                        JSValue::bool(native_module_has_enumerable_key(&module_name, key_name))
                            .bits(),
                    ));
                }
            }
            // perry's hidden `__perry_*` runtime-internal own keys (the
            // `class … extends Map/Set` backing field) live in the instance
            // keys_array but are never observable — report non-enumerable.
            if (*obj_ptr).class_id != 0
                && crate::object::field_get_set::is_internal_runtime_key(key_name)
            {
                return Some(f64::from_bits(JSValue::bool(false).bits()));
            }
            if !own_key_present(obj_ptr as *mut ObjectHeader, key_str) {
                return Some(f64::from_bits(JSValue::bool(false).bits()));
            }
            let enumerable = get_property_attrs(obj_ptr as usize, key_name)
                .map(|attrs| attrs.enumerable())
                .unwrap_or(true);
            return Some(f64::from_bits(JSValue::bool(enumerable).bits()));
        }

        // `obj.isPrototypeOf(v)` — true iff `obj` appears in `v`'s modeled
        // prototype chain. Object.create links live in Perry's synthetic
        // class/prototype side table; closure/static prototype links use
        // `Object.getPrototypeOf` state. Primitive/nullish receivers or
        // arguments are never a match.
        "isPrototypeOf" => {
            let arg = if args_len >= 1 && !args_ptr.is_null() {
                *args_ptr
            } else {
                f64::from_bits(crate::value::TAG_UNDEFINED)
            };
            return Some(f64::from_bits(
                JSValue::bool(js_object_is_prototype_of_value(object, arg)).bits(),
            ));
        }

        // Annex B §B.2.2 Object.prototype accessor helpers.
        "__defineGetter__" | "__defineSetter__" => {
            let undef = f64::from_bits(crate::value::TAG_UNDEFINED);
            let key = if args_len >= 1 && !args_ptr.is_null() {
                *args_ptr
            } else {
                undef
            };
            let func = if args_len >= 2 && !args_ptr.is_null() {
                *args_ptr.add(1)
            } else {
                undef
            };
            return Some(if method_name == "__defineGetter__" {
                super::js_object_define_getter(object, key, func)
            } else {
                super::js_object_define_setter(object, key, func)
            });
        }
        "__lookupGetter__" | "__lookupSetter__" => {
            let undef = f64::from_bits(crate::value::TAG_UNDEFINED);
            let key = if args_len >= 1 && !args_ptr.is_null() {
                *args_ptr
            } else {
                undef
            };
            return Some(if method_name == "__lookupGetter__" {
                super::js_object_lookup_getter(object, key)
            } else {
                super::js_object_lookup_setter(object, key)
            });
        }

        // `Object.prototype.valueOf` returns the receiver after ToObject.
        // Perry does not box primitives here; preserving the existing
        // primitive return keeps #2058's bound primitive method reads working,
        // while ordinary objects now get the inherited default instead of
        // falling through to "valueOf is not a function".
        "valueOf" => {
            // A user-defined own `valueOf` wins over the default, mirroring the
            // `toLocaleString` arm below. `Object(x)` returns `x` unchanged, so
            // `Object(x).valueOf()` must run x's own `valueOf`
            // (test262 built-ins/Object/S9.9_A6). The explicit-base form
            // `Object.prototype.valueOf.call(x)` goes through
            // `object_prototype_value_of_thunk` instead and correctly skips this
            // own-property lookup.
            let own =
                crate::object::js_object_get_own_field_or_undef(object, b"valueOf".as_ptr(), 7);
            if let Some(result) = call_primitive_closure_value(
                object,
                JSValue::from_bits(own.to_bits()),
                args_ptr,
                args_len,
            ) {
                return Some(result);
            }
            return Some(js_object_default_value_of(object));
        }

        // `Object.prototype.toLocaleString` invokes the receiver's
        // `toString`. If no custom method is present, fall back to the
        // default `[object Tag]` string. Primitive receivers delegate to
        // their existing `toString` behavior.
        "toLocaleString" => {
            return Some(js_object_default_to_locale_string(object));
        }

        // Function.prototype.call(thisArg, ...args) — invoke the receiver
        // closure with `thisArg` bound as `this` and the remaining args
        // passed positionally. Ramda's curry helpers (`_curry1`, `_curry2`,
        // `_curry3`) build their dispatch chain around
        // `fn.apply(this, arguments)` / `fn.call(this, x)`, so without these
        // arms ramda fails immediately on the first curried export.
        "call" => {
            // Proxy receiver (#3656): `p.call(thisArg, ...args)` routes through
            // the proxy `apply` trap (or, absent a trap, forwards to the target).
            if crate::proxy::js_proxy_is_proxy(object) == 1 {
                let this_arg = if args_len >= 1 && !args_ptr.is_null() {
                    *args_ptr
                } else {
                    f64::from_bits(crate::value::TAG_UNDEFINED)
                };
                let mut arr = crate::array::js_array_alloc(0);
                if args_len > 1 && !args_ptr.is_null() {
                    for i in 1..args_len {
                        arr = crate::array::js_array_push_f64(arr, *args_ptr.add(i));
                    }
                }
                let arr_box =
                    f64::from_bits(0x7FFD_0000_0000_0000 | (arr as u64 & 0x0000_FFFF_FFFF_FFFF));
                return Some(crate::proxy::js_proxy_apply(object, this_arg, arr_box));
            }
            let raw_ptr = (object.to_bits() & 0x0000_FFFF_FFFF_FFFF) as usize;
            if crate::closure::is_closure_ptr(raw_ptr) {
                let this_arg = if args_len >= 1 && !args_ptr.is_null() {
                    crate::closure::coerce_call_this(object, *args_ptr)
                } else {
                    f64::from_bits(crate::value::TAG_UNDEFINED)
                };
                let rest_ptr = if args_len > 1 && !args_ptr.is_null() {
                    args_ptr.add(1)
                } else {
                    std::ptr::null()
                };
                let rest_len = args_len.saturating_sub(1);
                let prev_this = IMPLICIT_THIS.with(|c| c.replace(this_arg.to_bits()));
                // Static bound-method value (`C.m.call(x)`): arm the one-shot
                // static-`this` override so the method body sees `x` instead
                // of the lexical class-ref (static private brand checks).
                let static_target = super::native_module::is_static_bound_method_value(object);
                if static_target {
                    super::static_this_arm(this_arg);
                }
                // A concise/object-literal method reads `this` from a baked
                // capture slot, not IMPLICIT_THIS; rebind to the explicit
                // `.call(thisArg)` receiver (no-op for arrows / plain fns).
                let call_target = crate::closure::rebind_explicit_this(object, this_arg);
                let result = crate::closure::js_native_call_value(call_target, rest_ptr, rest_len);
                if static_target {
                    super::static_this_disarm();
                }
                IMPLICIT_THIS.with(|c| c.set(prev_this));
                // #4973: `http.Server.call(this, handler)` — the inherits
                // pattern. Alias the explicit `this` object to the handle the
                // native class export constructed.
                super::native_this_alias::maybe_alias_explicit_this_construction(
                    object, this_arg, result,
                );
                return Some(result);
            }
            // #3662: `Function.prototype.call.call(x, …)` on a non-callable
            // `this` throws a `TypeError`; ambiguous pointers fall through.
            if fn_proto_receiver_not_callable(object) {
                throw_fn_proto_not_callable("call");
            }
        }

        // Function.prototype.apply(thisArg, argsArray) — invoke the receiver
        // closure with `thisArg` bound as `this` and the elements of
        // `argsArray` spread as positional arguments. `argsArray` may be
        // null / undefined (treat as no args). Mirrors `js_native_call_method_apply`
        // but for the `Function.prototype.apply` path rather than the
        // dynamic-spread method-call codegen path.
        "apply" => {
            // Proxy receiver (#3656): `p.apply(thisArg, argsArray)` routes
            // through the proxy `apply` trap (or forwards to the target).
            if crate::proxy::js_proxy_is_proxy(object) == 1 {
                let this_arg = if args_len >= 1 && !args_ptr.is_null() {
                    *args_ptr
                } else {
                    f64::from_bits(crate::value::TAG_UNDEFINED)
                };
                let supplied = if args_len >= 2 && !args_ptr.is_null() {
                    *args_ptr.add(1)
                } else {
                    f64::from_bits(crate::value::TAG_UNDEFINED)
                };
                // Pass a real (possibly empty) array as the argArray — a
                // null/undefined argsArray means "no arguments".
                let args_box = if JSValue::from_bits(supplied.to_bits()).is_pointer() {
                    supplied
                } else {
                    let arr = crate::array::js_array_alloc(0);
                    f64::from_bits(0x7FFD_0000_0000_0000 | (arr as u64 & 0x0000_FFFF_FFFF_FFFF))
                };
                return Some(crate::proxy::js_proxy_apply(object, this_arg, args_box));
            }
            let raw_ptr = (object.to_bits() & 0x0000_FFFF_FFFF_FFFF) as usize;
            if crate::closure::is_closure_ptr(raw_ptr) {
                let this_arg = if args_len >= 1 && !args_ptr.is_null() {
                    crate::closure::coerce_call_this(object, *args_ptr)
                } else {
                    f64::from_bits(crate::value::TAG_UNDEFINED)
                };
                let args_arr_val = if args_len >= 2 && !args_ptr.is_null() {
                    *args_ptr.add(1)
                } else {
                    f64::from_bits(crate::value::TAG_UNDEFINED)
                };
                let args_arr_jsval = JSValue::from_bits(args_arr_val.to_bits());
                // The argArray may arrive NaN-boxed (POINTER_TAG) or as a
                // legacy RAW i64 pointer bit-cast to f64 (a function's
                // synthetic `arguments` array local) — top 16 bits zero.
                let args_arr_bits = args_arr_val.to_bits();
                let arr_raw: usize = if args_arr_jsval.is_pointer() {
                    // A Symbol is POINTER_TAG'd but is a primitive, not an
                    // Object — Type(argArray) is not Object, so reject it
                    // below rather than treating its payload as an array
                    // pointer (test262 apply/argarray-not-object `Symbol()`).
                    if crate::symbol::js_is_symbol(args_arr_val) != 0 {
                        0
                    } else {
                        (args_arr_bits & 0x0000_FFFF_FFFF_FFFF) as usize
                    }
                } else if (args_arr_bits >> 48) == 0 && args_arr_bits >= 0x1000 {
                    args_arr_bits as usize
                } else {
                    0
                };
                // Spec CreateListFromArrayLike: a non-nullish, non-object
                // argArray (`fn.apply(null, true)` / `NaN` / `'1,2,3'` /
                // `Symbol()`) is a TypeError. null/undefined mean "no
                // arguments".
                if arr_raw == 0 && !args_arr_jsval.is_undefined() && !args_arr_jsval.is_null() {
                    throw_type_error_message(b"CreateListFromArrayLike called on non-object");
                }
                let buf: Vec<f64> = if arr_raw != 0 {
                    if let Some(values) = crate::object::arguments_object_to_vec(
                        arr_raw as *const crate::object::ObjectHeader,
                    ) {
                        values
                    } else {
                        let arr_ptr = arr_raw as *const crate::array::ArrayHeader;
                        let n = crate::array::js_array_length(arr_ptr) as usize;
                        (0..n)
                            .map(|i| crate::array::js_array_get_f64(arr_ptr, i as u32))
                            .collect()
                    }
                } else {
                    Vec::new()
                };
                let (call_args_ptr, call_args_len) = if buf.is_empty() {
                    (std::ptr::null::<f64>(), 0_usize)
                } else {
                    (buf.as_ptr(), buf.len())
                };
                let prev_this = IMPLICIT_THIS.with(|c| c.replace(this_arg.to_bits()));
                // Static bound-method value — see the matching `call` arm.
                let static_target = super::native_module::is_static_bound_method_value(object);
                if static_target {
                    super::static_this_arm(this_arg);
                }
                // Rebind a concise/object-literal method's baked `this` slot to
                // the explicit `.apply(thisArg)` receiver (no-op for arrows /
                // plain fns) — see the matching `call` arm.
                let apply_target = crate::closure::rebind_explicit_this(object, this_arg);
                let result = crate::closure::js_native_call_value(
                    apply_target,
                    call_args_ptr,
                    call_args_len,
                );
                if static_target {
                    super::static_this_disarm();
                }
                IMPLICIT_THIS.with(|c| c.set(prev_this));
                // #4973: `http.Server.apply(this, args)` — same inherits
                // pattern as the `call` arm above.
                super::native_this_alias::maybe_alias_explicit_this_construction(
                    object, this_arg, result,
                );
                return Some(result);
            }
            // #3662: `Function.prototype.apply.call(x, …)` on a non-callable
            // `this` throws a `TypeError`; ambiguous pointers fall through.
            if fn_proto_receiver_not_callable(object) {
                throw_fn_proto_not_callable("apply");
            }
        }

        // Common string methods on string values
        "toString" => {
            // A class REFERENCE (INT32-tagged registered class id) is a
            // function value: `C.toString()` must produce function source,
            // not the numeric rendering of its class id ("1"). Perry doesn't
            // retain class source text, so emit the NativeFunction form —
            // Test262's assertToStringOrNativeFunction accepts it.
            if super::class_prototype_ref_id(object).is_none() {
                if let Some(cid) = super::native_module::class_ref_id(object) {
                    let name = super::class_registry::class_name_for_id(cid).unwrap_or_default();
                    let s = format!("function {name}() {{ [native code] }}");
                    let str_ptr = crate::string::js_string_from_bytes(s.as_ptr(), s.len() as u32);
                    return Some(f64::from_bits(JSValue::string_ptr(str_ptr).bits()));
                }
            }
            if let Some((_, payload)) = crate::builtins::boxed_primitive_payload(object) {
                let payload_jsv = JSValue::from_bits(payload.to_bits());
                match crate::builtins::boxed_primitive_to_string_tag(object) {
                    Some("String") => return Some(payload),
                    Some("Number") => {
                        let n = if payload_jsv.is_number() {
                            payload_jsv.as_number()
                        } else {
                            payload
                        };
                        let s = if n.fract() == 0.0 && n.abs() < (i64::MAX as f64) {
                            (n as i64).to_string()
                        } else {
                            n.to_string()
                        };
                        let str_ptr =
                            crate::string::js_string_from_bytes(s.as_ptr(), s.len() as u32);
                        return Some(f64::from_bits(JSValue::string_ptr(str_ptr).bits()));
                    }
                    Some("Boolean") => {
                        let s = if payload_jsv.is_bool() && payload_jsv.as_bool() {
                            "true"
                        } else {
                            "false"
                        };
                        let str_ptr =
                            crate::string::js_string_from_bytes(s.as_ptr(), s.len() as u32);
                        return Some(f64::from_bits(JSValue::string_ptr(str_ptr).bits()));
                    }
                    Some("BigInt") if payload_jsv.is_bigint() => {
                        let ptr = payload_jsv.as_bigint_ptr();
                        let str_ptr = crate::bigint::js_bigint_to_string(ptr);
                        return Some(f64::from_bits(JSValue::string_ptr(str_ptr).bits()));
                    }
                    Some("Symbol") => {
                        let str_ptr = crate::symbol::js_symbol_to_string(payload);
                        return Some(f64::from_bits(
                            JSValue::string_ptr(str_ptr as *mut _).bits(),
                        ));
                    }
                    _ => {}
                }
            }
            if jsval.is_string() {
                return Some(object);
            } else if jsval.is_bigint() {
                let ptr = jsval.as_bigint_ptr();
                let str_ptr = crate::bigint::js_bigint_to_string(ptr);
                return Some(f64::from_bits(JSValue::string_ptr(str_ptr).bits()));
            } else if jsval.is_number() {
                let n = jsval.as_number();
                // #3146 + #2864: honour an explicit radix argument. With no
                // argument (or an explicit `undefined`) use the default decimal
                // formatting; otherwise delegate to the canonical radix path,
                // which ToNumber/ToInteger-coerces + validates the radix (spec
                // `RangeError` outside 2..=36) and formats via the shortest-
                // round-trip V8 algorithm (`double_to_radix_string`).
                let radix_arg = refreshed_args().first().copied();
                let has_radix = match radix_arg {
                    None => false,
                    Some(r) => !JSValue::from_bits(r.to_bits()).is_undefined(),
                };
                if has_radix {
                    let str_ptr =
                        crate::value::js_jsvalue_to_string_radix(object, radix_arg.unwrap());
                    return Some(f64::from_bits(JSValue::string_ptr(str_ptr).bits()));
                }
                let s = if n.fract() == 0.0 && n.abs() < (i64::MAX as f64) {
                    (n as i64).to_string()
                } else {
                    n.to_string()
                };
                let str_ptr = crate::string::js_string_from_bytes(s.as_ptr(), s.len() as u32);
                return Some(f64::from_bits(JSValue::string_ptr(str_ptr).bits()));
            } else if jsval.is_bool() {
                let s = if jsval.as_bool() { "true" } else { "false" };
                let str_ptr = crate::string::js_string_from_bytes(s.as_ptr(), s.len() as u32);
                return Some(f64::from_bits(JSValue::string_ptr(str_ptr).bits()));
            }
            // #3146: `undefined.toString()` / `null.toString()` must throw a
            // TypeError (property read on a nullish base), not return the
            // string "undefined"/"null". Falling through this arm without a
            // `return` reaches the nullish-receiver throw below, which raises
            // `Cannot read properties of <undefined|null> (reading 'toString')`.
        }

        // Array methods - delegate to array runtime
        "push" if jsval.is_pointer() => {
            let arr_ptr =
                jsval.as_pointer::<crate::array::ArrayHeader>() as *mut crate::array::ArrayHeader;
            // Spec §23.1.3.21: length is Set even with 0 args, so guards fire regardless
            if crate::array::array_is_frozen(arr_ptr) {
                crate::collection_iter::throw_type_error("Cannot mutate a frozen array");
            }
            crate::array::guard_writable_length(arr_ptr);
            let mut arr = arr_ptr;
            if !args_ptr.is_null() {
                for i in 0..args_len {
                    let val = *args_ptr.add(i);
                    arr = crate::array::js_array_push_f64(arr, val);
                }
            }
            return Some(crate::array::js_array_length(arr) as f64);
        }
        "pop" if jsval.is_pointer() => {
            let arr =
                jsval.as_pointer::<crate::array::ArrayHeader>() as *mut crate::array::ArrayHeader;
            return Some(crate::array::js_array_pop_f64(arr));
        }
        "length" if jsval.is_pointer() => {
            let arr = jsval.as_pointer::<crate::array::ArrayHeader>();
            return Some(crate::array::js_array_length(arr) as f64);
        }

        _ => {}
    }

    None
}
