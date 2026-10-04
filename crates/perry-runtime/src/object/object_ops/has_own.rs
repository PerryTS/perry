//! `Object.is`, `Object.hasOwn`, and `Object.prototype.propertyIsEnumerable`.
use super::*;

/// Object.is(a, b) — SameValue algorithm
/// Like ===, except: NaN === NaN (true) and +0 !== -0 (false).
/// Returns NaN-boxed boolean.
#[no_mangle]
pub extern "C" fn js_object_is(a: f64, b: f64) -> f64 {
    const TAG_TRUE: u64 = 0x7FFC_0000_0000_0004;
    const TAG_FALSE: u64 = 0x7FFC_0000_0000_0003;
    let a_bits = a.to_bits();
    let b_bits = b.to_bits();

    // Handle NaN: SameValue treats NaN as equal to NaN
    let a_jsval = crate::JSValue::from_bits(a_bits);
    let b_jsval = crate::JSValue::from_bits(b_bits);

    if a_jsval.is_number() && b_jsval.is_number() {
        let an = a_jsval.as_number();
        let bn = b_jsval.as_number();
        if an.is_nan() && bn.is_nan() {
            return f64::from_bits(TAG_TRUE);
        }
        // Distinguish +0 / -0 by bit pattern
        if an == 0.0 && bn == 0.0 {
            if a_bits == b_bits {
                return f64::from_bits(TAG_TRUE);
            }
            return f64::from_bits(TAG_FALSE);
        }
        if an == bn {
            return f64::from_bits(TAG_TRUE);
        }
        return f64::from_bits(TAG_FALSE);
    }

    // For strings, do content comparison. #1781: accept inline SSO short
    // strings on either side. Two SSO operands with equal content already
    // match via the bit-pattern fallback below, but a mixed SSO/heap pair
    // (same content, different representation — e.g. a JSON-parsed value vs
    // a heap literal) would not. Materialize via the unified decoder so the
    // comparison is representation-independent.
    if a_jsval.is_any_string() && b_jsval.is_any_string() {
        let result = crate::string::js_string_equals(
            crate::value::js_get_string_pointer_unified(f64::from_bits(a_bits))
                as *const crate::StringHeader,
            crate::value::js_get_string_pointer_unified(f64::from_bits(b_bits))
                as *const crate::StringHeader,
        );
        if result != 0 {
            return f64::from_bits(TAG_TRUE);
        }
        return f64::from_bits(TAG_FALSE);
    }

    // For everything else, bit-pattern equality
    if a_bits == b_bits {
        f64::from_bits(TAG_TRUE)
    } else {
        f64::from_bits(TAG_FALSE)
    }
}

/// `Object.hasOwn(o, k)` for an ordinary object `o` and a string `k`,
/// answered by `o`'s shape: `Some(present)`, or `None` when the shape does not
/// answer and the generic arms below decide.
///
/// The receiver is a POINTER-tagged value above the handle band whose `+4`
/// word names an ordinary record of this agent (shape rule 3: no other cell
/// kind holds a ShapeId there, so no header read is needed to know it is a
/// `GC_TYPE_OBJECT`), and the record answers for its own keys
/// (`shapes::plain_own_key_present`: no class object, dictionary, tombstone,
/// accessor, private key, `process.env`, arguments object or module
/// namespace). The generic arms are then silent for it but for the
/// per-object facts tested here, each only on the answer it can change:
///
/// * the handle, Proxy, symbol-key, coercing-key and string-receiver arms
///   cannot apply (tags and the handle floor);
/// * a class id the runtime assigns (`FIRST_RUNTIME_CLASS_ID` and up: the
///   builtin bands, with boxed primitives, the String wrapper's index keys,
///   native modules and builtin instances, and the synthetic prototype-object
///   classes) declines;
/// * an Array-subclass elements store (its meta record) declines;
/// * PRESENT: a runtime-internal key of a class instance is no property
///   (`own_key_hidden_bytes`; every such spelling starts with `_` or `#`),
///   and `%Function.prototype%`'s installed `Object.prototype` thunks are
///   not its own (it is runtime-born, so never of kind `Ordinary`);
/// * ABSENT: a class's declared or evaluated prototype owns its vtable
///   methods and `constructor` without listing them.
///
/// No user code and no allocation, except that the %Function.prototype%
/// test may resolve that intrinsic's memo once, as the generic arm does; no
/// receiver word is read after it.
/// The first class id that is not a codegen-assigned one: the builtin band
/// `0x7FFF_FF00..=0x7FFF_FFFF`, then the synthetic and `0xFFFF_0000..` bands
/// above it (`class_registry::prototype_objects`).
const FIRST_RUNTIME_CLASS_ID: u32 = 0x7FFF_FF00;

///
/// # Safety
/// [`shape_may_answer`] holds for `obj_bits` and `key_bits`.
#[inline(never)]
unsafe fn ordinary_own_key_present(obj_bits: u64, key_bits: u64) -> Option<bool> {
    let key = crate::JSValue::from_bits(key_bits);
    let addr = (obj_bits & crate::value::POINTER_MASK) as usize;
    let obj = addr as *const ObjectHeader;
    let mut sso = [0u8; crate::value::SHORT_STRING_MAX_LEN];
    let text = crate::string::js_string_key_bytes(key, &mut sso)?;
    let (present, kind) = super::super::shapes::plain_own_key_present(
        super::super::shapes::ordinary_dir_addr(),
        (*obj).parent_class_id,
        key_bits,
        text,
    )?;
    let class_id = (*obj).class_id;
    if class_id >= FIRST_RUNTIME_CLASS_ID
        || !crate::array::subclass_elements::elements_of(obj).is_null()
    {
        return None;
    }
    if present {
        if class_id != 0
            && matches!(text.first(), Some(b'_' | b'#'))
            && super::super::field_get_set::is_internal_runtime_key_bytes(text)
        {
            return None;
        }
        if kind != super::super::shapes::ShapeObjectKind::Ordinary
            && addr == crate::array::function_prototype_addr()
        {
            return None;
        }
    } else if class_id != 0
        && (super::super::class_value::class_decl_prototype_link(class_id) as usize == addr
            || super::super::field_get_set::class_evaluation_prototype_class_id(addr).is_some())
    {
        // `class_registry::class_id_for_decl_prototype_object` without its
        // header read: the receiver is already known to be an object.
        return None;
    }
    Some(present)
}

/// Object.hasOwn(obj, key) - check if obj has its own property `key`.
#[no_mangle]
pub extern "C" fn js_object_has_own(obj_value: f64, key_value: f64) -> f64 {
    const TAG_TRUE: u64 = 0x7FFC_0000_0000_0004;
    const TAG_FALSE: u64 = 0x7FFC_0000_0000_0003;
    let (obj_bits, key_bits) = (obj_value.to_bits(), key_value.to_bits());
    // SAFETY: both read only a POINTER-tagged receiver above the handle floor
    // (shape rule 3) and a string key's own bytes.
    unsafe {
        if shape_may_answer(obj_bits, key_bits) {
            if let Some(present) = ordinary_own_key_present(obj_bits, key_bits) {
                return f64::from_bits(if present { TAG_TRUE } else { TAG_FALSE });
            }
        }
    }
    object_has_own_generic(obj_value, key_value)
}

/// The register-only gate in front of [`ordinary_own_key_present`]: a
/// POINTER-tagged receiver above the handle floor whose `+4` word is in the
/// ShapeId range, and a string key. Every other receiver (an array, whose
/// `+4` is its capacity; a handle; a primitive) goes straight to the generic
/// arms without the shape answer's frame.
///
/// # Safety
/// `obj_bits` is a live value.
#[inline(always)]
unsafe fn shape_may_answer(obj_bits: u64, key_bits: u64) -> bool {
    let key = crate::JSValue::from_bits(key_bits);
    obj_bits >> 48 == 0x7FFD
        && (key.is_string() || key.is_short_string())
        && (obj_bits & crate::value::POINTER_MASK) as usize >= perry_abi::RECEIVER_HANDLE_FLOOR
        && super::super::shapes::is_shape_id(
            (*((obj_bits & crate::value::POINTER_MASK) as *const ObjectHeader)).parent_class_id,
        )
}

/// [`js_object_has_own`] for every receiver and key the shape does not answer
/// for. Out of line, like the shape answer, so neither pays the other's frame.
#[inline(never)]
fn object_has_own_generic(obj_value: f64, key_value: f64) -> f64 {
    const TAG_TRUE: u64 = 0x7FFC_0000_0000_0004;
    const TAG_FALSE: u64 = 0x7FFC_0000_0000_0003;
    unsafe {
        let obj_js = crate::JSValue::from_bits(obj_value.to_bits());
        if obj_js.is_undefined() || obj_js.is_null() {
            super::super::has_own_helpers::throw_to_object_nullish_type_error();
        }
        if let Some((_, elements)) = crate::array::subclass_elements::backed_value(obj_value) {
            if let Some(elements_key) = crate::array::subclass_elements::key_of_value(key_value) {
                return f64::from_bits(
                    if crate::array::subclass_elements::has_own_key(elements, elements_key) {
                        TAG_TRUE
                    } else {
                        TAG_FALSE
                    },
                );
            }
        }

        // A POINTER_TAG registry handle (zlib stream, fetch Request/Response/
        // Headers/Blob, …) is not an address and must never be dereferenced. Its
        // TYPED surface is prototype accessors (not own properties), but a
        // user-attached expando IS an own property (#6363) — `handle.foo = v` or
        // `Object.defineProperty(handle, …)`. Consult the expando table rather
        // than reporting a flat false. Proxies also live in the handle band but
        // are served by the trap branch below (which runs before any deref), so
        // leave their sub-band alone.
        if obj_js.is_pointer() {
            let addr = obj_js.as_pointer::<u8>() as usize;
            if crate::value::addr_class::is_handle_band(addr)
                && !crate::value::addr_class::is_proxy_id_band(addr)
            {
                let key_value = super::super::js_to_property_key(key_value);
                if crate::symbol::js_is_symbol(key_value) != 0 {
                    let present = crate::symbol::js_object_has_own_symbol(obj_value, key_value);
                    return f64::from_bits(if present { TAG_TRUE } else { TAG_FALSE });
                }
                let present = super::super::metadata_key_to_string(key_value)
                    .map(|name| {
                        super::super::handle_expando::handle_expando_has(addr as i64, &name)
                    })
                    .unwrap_or(false);
                return f64::from_bits(if present { TAG_TRUE } else { TAG_FALSE });
            }
        }

        // ToPropertyKey(V): fold an object argument (e.g. one whose `toString`
        // returns a Symbol) into its canonical key before the symbol/string
        // split. A no-op for keys that are already primitives.
        //
        // #6935: for an object key that fold runs USER JS, which allocates and
        // can trigger a GC that **evacuates** the receiver. `obj_value` — and
        // the `obj_js` tag view taken from it above — were raw NaN-boxed Rust
        // locals across the call, so root the receiver and re-derive both.
        let (obj_value, obj_js, key_value) =
            if super::super::property_key_coercion_is_inert(key_value) {
                (obj_value, obj_js, key_value)
            } else {
                let scope = crate::gc::RuntimeHandleScope::new();
                let obj_handle = scope.root_heap_word_u64(obj_value.to_bits());
                let key_value = super::super::js_to_property_key(key_value);
                let obj_value = f64::from_bits(obj_handle.get_heap_word_u64());
                (
                    obj_value,
                    crate::JSValue::from_bits(obj_value.to_bits()),
                    key_value,
                )
            };

        // A Proxy is a small registered id, not a heap object — route
        // `hasOwnProperty` through `[[GetOwnProperty]]` (a present own property
        // is one whose descriptor is not undefined) rather than dereferencing
        // the fake pointer. (Proxy crash cluster.)
        if crate::proxy::js_proxy_is_proxy(obj_value) != 0 {
            let desc = crate::proxy::js_reflect_get_own_property_descriptor(obj_value, key_value);
            return f64::from_bits(if desc.to_bits() != crate::value::TAG_UNDEFINED {
                TAG_TRUE
            } else {
                TAG_FALSE
            });
        }

        // Symbol-keyed lookup: route through SYMBOL_PROPERTIES side table.
        if crate::symbol::js_is_symbol(key_value) != 0 {
            let sym_key = crate::symbol::sym_key_from_f64(key_value);
            if let Some(class_id) = super::super::class_ref_id(obj_value) {
                let is_prototype = super::super::class_prototype_ref_id(obj_value).is_some();
                let present = if is_prototype {
                    let proto = super::super::class_registry::class_decl_prototype_value(class_id);
                    crate::symbol::js_object_has_own_symbol(proto, key_value)
                } else {
                    crate::symbol::class_static_symbol_lookup(class_id, key_value).is_some()
                        || super::super::class_registry::class_has_own_symbol_member(
                            class_id, sym_key, true,
                        )
                };
                return f64::from_bits(if present { TAG_TRUE } else { TAG_FALSE });
            }
            let present = crate::symbol::js_object_has_own_symbol(obj_value, key_value);
            return f64::from_bits(if present { TAG_TRUE } else { TAG_FALSE });
        }

        // #6943: `js_string_coerce` allocates for every non-heap-string key and
        // runs a user `toString` / `valueOf` for an object key, so it can
        // trigger a GC that **evacuates**. `obj_value` — and the `obj_js` tag
        // view derived from it at the top of this function — were raw Rust
        // locals across the call, and every arm below dereferences one or the
        // other. The already-heap-string key, which is what
        // `o.hasOwnProperty("x")` compiles to for names past the SSO bound,
        // keeps the pre-fix path verbatim.
        let key_js = crate::JSValue::from_bits(key_value.to_bits());
        let (obj_value, obj_js, key_str) = if key_js.is_string() {
            // A heap string is its own coercion (`js_string_coerce` answers
            // it with the same pointer), so the call is skipped.
            (
                obj_value,
                obj_js,
                key_js.as_string_ptr() as *mut crate::StringHeader,
            )
        } else if crate::builtins::string_coerce_is_inert(key_value) {
            (
                obj_value,
                obj_js,
                crate::builtins::js_string_coerce(key_value),
            )
        } else {
            let scope = crate::gc::RuntimeHandleScope::new();
            let obj_handle = scope.root_heap_word_u64(obj_value.to_bits());
            let key_str = crate::builtins::js_string_coerce(key_value);
            let obj_value = f64::from_bits(obj_handle.get_heap_word_u64());
            (
                obj_value,
                crate::JSValue::from_bits(obj_value.to_bits()),
                key_str,
            )
        };
        if key_str.is_null() {
            return f64::from_bits(TAG_FALSE);
        }

        if obj_js.is_any_string() {
            let present =
                super::super::has_own_helpers::string_primitive_own_key_present(obj_value, key_str);
            return f64::from_bits(if present { TAG_TRUE } else { TAG_FALSE });
        }

        // The receiver's managed header, read ONCE through the
        // ownership-proving reader (an arbitrary receiver word is never read
        // at `addr - 8` unchecked). Its type decides which of the
        // kind-specific probes below can apply; none of them re-derives it.
        // `None` means no allocator-owned header: a non-pointer word, or
        // foreign / untracked memory such as a foreign-backed Buffer. The
        // read follows every allocating coercion above, and a move never
        // changes a header's type.
        let header = if obj_js.is_pointer() {
            let addr = obj_js.as_pointer::<u8>() as usize;
            if crate::value::addr_class::is_above_handle_band(addr) {
                crate::value::addr_class::try_read_tracked_gc_header(addr).map(|h| &*h.as_ptr())
            } else {
                None
            }
        } else {
            None
        };

        match header {
            // An array is none of the kinds below (Buffer, typed array, class
            // value, function, exotic cell, %Function.prototype%, native
            // module): it answers from its elements and own keys.
            Some(h) if h.obj_type == crate::gc::GC_TYPE_ARRAY => {
                let present = super::super::has_own_helpers::array_own_key_present(
                    obj_js.as_pointer::<u8>() as *const crate::array::ArrayHeader,
                    key_str,
                );
                return f64::from_bits(if present { TAG_TRUE } else { TAG_FALSE });
            }
            // An ordinary object is no Buffer, typed array, function or
            // exotic cell, and no class constructor value (those are
            // immediates or functions). Only a class object, which owns
            // `prototype` without storing it, needs a look before the object
            // arms below.
            Some(h) if h.obj_type == crate::gc::GC_TYPE_OBJECT => {
                let ptr = obj_js.as_pointer::<u8>();
                if super::super::class_registry::parent_static::is_class_object_with_header(ptr, h)
                    && super::super::has_own_helpers::str_from_string_header(key_str).is_some_and(
                        |key| {
                            super::super::field_get_set::class_object_has_prototype_property(
                                key.as_bytes(),
                            )
                        },
                    )
                {
                    return f64::from_bits(TAG_TRUE);
                }
            }
            _ => {
                let tracked_type = header.map(|h| h.obj_type);
                if let Some(present) =
                    has_own_of_non_ordinary_kind(obj_value, obj_js, tracked_type, key_str)
                {
                    return f64::from_bits(if present { TAG_TRUE } else { TAG_FALSE });
                }
            }
        }

        let obj = extract_obj_ptr(obj_value);
        if obj.is_null() || (obj as usize) < 0x10000 {
            return f64::from_bits(TAG_FALSE);
        }

        // `%Function.prototype%` is an ordinary object — `is_closure_ptr` is
        // false for it, so the closure branch above never sees it — but it's
        // installed with a full set of generic `Object.prototype` methods as
        // real own data properties so they dispatch when called directly on
        // it. Per spec (20.2.3) it only *inherits* these from
        // `Object.prototype`; they are not its own (test262 built-ins/
        // Function/prototype/S15.3.4_A4) — UNLESS user code has since
        // overwritten the slot (`Object.defineProperty(Function.prototype,
        // "valueOf", …)`), which per spec DOES create a genuine own property.
        // Distinguish the two by checking whether the currently-installed
        // value is still the exact install-time thunk closure: an explicit
        // redefine always stores a different value (a new closure, or a
        // non-function entirely), never literally the same `func_ptr`.
        if super::super::global_this::is_function_prototype_object_value(obj_value) {
            if let Some(key) = super::super::has_own_helpers::str_from_string_header(key_str) {
                // The thunk `install_noop_proto_methods` (the installer
                // Function.prototype goes through) gives each name: the real
                // `hasOwnProperty` / `propertyIsEnumerable` / `isPrototypeOf`
                // and Annex B accessor thunks, and the shared
                // `global_this_builtin_noop_thunk` for the rest.
                let expected_thunk: Option<*const u8> = match key {
                    "hasOwnProperty" => Some(
                        super::super::global_this::object_prototype_has_own_property_thunk
                            as *const u8,
                    ),
                    "propertyIsEnumerable" => Some(
                        super::super::global_this::object_prototype_property_is_enumerable_thunk
                            as *const u8,
                    ),
                    "toLocaleString" | "valueOf" => {
                        Some(super::super::global_this::global_this_builtin_noop_thunk as *const u8)
                    }
                    "isPrototypeOf" => Some(
                        super::super::global_this::object_prototype_is_prototype_of_thunk
                            as *const u8,
                    ),
                    "__defineGetter__" => Some(
                        super::super::global_this::object_prototype_define_getter_thunk
                            as *const u8,
                    ),
                    "__defineSetter__" => Some(
                        super::super::global_this::object_prototype_define_setter_thunk
                            as *const u8,
                    ),
                    "__lookupGetter__" => Some(
                        super::super::global_this::object_prototype_lookup_getter_thunk
                            as *const u8,
                    ),
                    "__lookupSetter__" => Some(
                        super::super::global_this::object_prototype_lookup_setter_thunk
                            as *const u8,
                    ),
                    _ => None,
                };
                if let Some(expected) = expected_thunk {
                    let current = js_object_get_field_by_name(obj, key_str);
                    let still_default_shim = if current.is_pointer() {
                        let cur_ptr = current.as_pointer::<u8>() as usize;
                        crate::closure::is_closure_ptr(cur_ptr)
                            && crate::closure::get_valid_func_ptr(
                                cur_ptr as *const crate::closure::ClosureHeader,
                            ) == expected
                    } else {
                        false
                    };
                    if still_default_shim {
                        return f64::from_bits(TAG_FALSE);
                    }
                }
            }
        }

        if (*obj).class_id == super::super::native_module::NATIVE_MODULE_CLASS_ID {
            let present = super::super::native_module::read_native_module_name(obj)
                .as_deref()
                .zip(super::super::has_own_helpers::str_from_string_header(
                    key_str,
                ))
                .map(|(module, key)| {
                    super::super::native_module::native_module_vtable()
                        .is_some_and(|vt| (vt.has_enumerable_key)(module, key))
                })
                .unwrap_or(false);
            return f64::from_bits(if present { TAG_TRUE } else { TAG_FALSE });
        }

        if (obj as usize) >= crate::gc::GC_HEADER_SIZE + 0x1000 {
            let gc_header =
                (obj as *const u8).sub(crate::gc::GC_HEADER_SIZE) as *const crate::gc::GcHeader;
            if (*gc_header).obj_type == crate::gc::GC_TYPE_ARRAY {
                let present = super::super::has_own_helpers::array_own_key_present(
                    obj as *const crate::array::ArrayHeader,
                    key_str,
                );
                return f64::from_bits(if present { TAG_TRUE } else { TAG_FALSE });
            }
        }

        if (*obj).class_id == NATIVE_MODULE_CLASS_ID {
            let Some(key_name) = super::super::has_own_helpers::str_from_string_header(key_str)
            else {
                return f64::from_bits(TAG_FALSE);
            };
            let present = read_native_module_name(obj)
                .as_deref()
                .is_some_and(|module_name| {
                    super::super::native_module::native_module_vtable()
                        .is_some_and(|vt| (vt.has_enumerable_key)(module_name, key_name))
                });
            return f64::from_bits(if present { TAG_TRUE } else { TAG_FALSE });
        }

        // Perry's hidden runtime-internal keys of a class instance sit in the
        // keys_array but are never reflectable own properties, so
        // `Object.hasOwn` must report false. A private field (#11791) is an
        // entry of the key list, not a property: the lookup below reads its
        // entry where it finds the key.
        if (*obj).class_id != 0 {
            if let Some(key) = super::super::has_own_helpers::str_from_string_header(key_str) {
                if super::super::field_get_set::is_internal_runtime_key(key) {
                    return f64::from_bits(TAG_FALSE);
                }
            }
        }

        if super::own_property_present(obj, key_str) {
            return f64::from_bits(TAG_TRUE);
        }

        // A class-declaration prototype object: methods live in the class
        // vtable, yet they ARE own properties of `C.prototype` —
        // `getOwnPropertyDescriptor` already reflects them, so
        // `hasOwnProperty` must agree (test262 class/definition/*-prop-desc,
        // which assert via `verifyProperty` → `hasOwnProperty`). Accessors are
        // real accessor properties of the prototype, found by the own-key
        // check above.
        if let Some(cid) =
            super::super::class_registry::class_id_for_decl_prototype_object(obj as usize)
        {
            if let Some(key) = super::super::has_own_helpers::str_from_string_header(key_str) {
                if !super::super::class_registry::class_proto_key_deleted(cid, key)
                    && (key == "constructor"
                        || (!key.starts_with('#')
                            && super::super::native_module::class_has_own_method(cid, key)))
                {
                    return f64::from_bits(TAG_TRUE);
                }
            }
        }

        f64::from_bits(TAG_FALSE)
    }
}

/// `js_object_has_own` for a receiver whose tracked header type
/// (`tracked_type`) is neither an ordinary object nor an array: a class
/// value, a Buffer / ArrayBuffer / DataView / typed array, a function, an
/// exotic cell, or a receiver with no tracked header (`None`). Answers
/// `None` when no kind-specific rule applies and the ordinary object arms
/// decide.
///
/// Each probe runs only for the header types that can reach it: a Buffer,
/// ArrayBuffer, DataView or typed array is a `GC_TYPE_BUFFER`,
/// `GC_TYPE_TYPED_ARRAY` or native-view cell, or has no tracked header; a
/// function is `GC_TYPE_CLOSURE`; an exotic kind IS its header type.
unsafe fn has_own_of_non_ordinary_kind(
    obj_value: f64,
    obj_js: crate::JSValue,
    tracked_type: Option<u8>,
    key_str: *const crate::StringHeader,
) -> Option<bool> {
    let exotic = tracked_type.and_then(super::super::exotic_expando::exotic_kind_of_gc_type);
    let buffer_like = match tracked_type {
        None => true,
        Some(t) => t != crate::gc::GC_TYPE_CLOSURE && exotic.is_none(),
    };

    if buffer_like {
        if let Some(present) = registered_buffer_index_own_property_present(obj_value, key_str) {
            return Some(present);
        }
    }

    // A class constructor value is an immediate or a function.
    if matches!(tracked_type, None | Some(crate::gc::GC_TYPE_CLOSURE)) {
        if let Some(class_id) = super::super::class_ref_id(obj_value) {
            let present = super::super::has_own_helpers::str_from_string_header(key_str)
                .map(|key| {
                    if super::super::field_get_set::is_internal_runtime_key(key) {
                        false
                    } else if super::super::class_registry::class_static_key_deleted(class_id, key)
                    {
                        false
                    } else if matches!(key, "length" | "prototype") {
                        true
                    } else if key == "name"
                        && !crate::object::class_value::class_static_owns_method(class_id, key)
                    {
                        super::super::class_registry::class_name_for_id(class_id).is_some()
                    } else {
                        let has_public_data =
                            crate::object::class_value::class_static_get(class_id, key).is_some();
                        has_public_data
                            || (!key.starts_with('#')
                                && (crate::object::class_value::class_static_owns_method(
                                    class_id, key,
                                ) || crate::object::class_value::class_static_has_own_accessor(
                                    class_id, key,
                                )))
                    }
                })
                .unwrap_or(false);
            return Some(present);
        }
    }

    // A class object owns `prototype` without storing it. A tracked class
    // object is `GC_TYPE_OBJECT` and was answered by the caller.
    if tracked_type.is_none()
        && super::super::class_registry::is_class_object_value(obj_value)
        && super::super::has_own_helpers::str_from_string_header(key_str).is_some_and(|key| {
            super::super::field_get_set::class_object_has_prototype_property(key.as_bytes())
        })
    {
        return Some(true);
    }

    if let Some(addr) = buffer_like
        .then(|| crate::typedarray_props::typed_array_addr_from_value(obj_value))
        .flatten()
    {
        return Some(crate::typedarray_props::typed_array_has_own_property(
            addr as *const crate::typedarray::TypedArrayHeader,
            key_str,
        ));
    }

    if !obj_js.is_pointer() {
        return None;
    }
    let ptr = obj_js.as_pointer::<u8>() as usize;
    if buffer_like && crate::buffer::is_registered_buffer(ptr) {
        return Some(super::super::has_own_helpers::buffer_own_key_present(
            ptr as *const crate::buffer::BufferHeader,
            key_str,
        ));
    }
    // Date / RegExp / Error / Temporal / Promise / Map / Set cells: own
    // expando props (side tables) + per-kind builtin own slots.
    if let Some(kind) = exotic {
        use super::super::exotic_expando::ExoticKind;
        return Some(
            super::super::has_own_helpers::str_from_string_header(key_str)
                .map(|key| {
                    super::super::exotic_expando::exotic_has_own_property(kind, ptr, key)
                        || match kind {
                            ExoticKind::RegExp => key == "lastIndex",
                            ExoticKind::Error => crate::error::js_error_has_own_property(
                                ptr as *mut crate::error::ErrorHeader,
                                key,
                            ),
                            ExoticKind::Date
                            | ExoticKind::Temporal
                            | ExoticKind::Promise
                            | ExoticKind::Map
                            | ExoticKind::Set => false,
                        }
                })
                .unwrap_or(false),
        );
    }
    // #3655: functions/closures carry built-in own `name`/`length` (and
    // `prototype` for constructors) plus any user-attached props. Route them
    // here instead of through `extract_obj_ptr`/`own_key_present`, which
    // would read `keys_array` off a closure (out of bounds).
    if matches!(tracked_type, None | Some(crate::gc::GC_TYPE_CLOSURE))
        && crate::closure::is_closure_ptr(ptr)
    {
        return Some(
            super::super::has_own_helpers::str_from_string_header(key_str)
                .map(|k| super::super::has_own_helpers::closure_own_key_present(ptr, k))
                .unwrap_or(false),
        );
    }
    if buffer_like && crate::typedarray::lookup_typed_array_kind(ptr).is_some() {
        return Some(crate::typedarray_props::typed_array_has_own_property(
            ptr as *const crate::typedarray::TypedArrayHeader,
            key_str,
        ));
    }
    None
}

/// `Object.prototype.propertyIsEnumerable.call(obj, key)` (#2891).
#[no_mangle]
pub extern "C" fn js_object_property_is_enumerable(obj_value: f64, key_value: f64) -> f64 {
    const TAG_TRUE: u64 = 0x7FFC_0000_0000_0004;
    const TAG_FALSE: u64 = 0x7FFC_0000_0000_0003;
    unsafe {
        let obj_jv = crate::JSValue::from_bits(obj_value.to_bits());
        if obj_jv.is_null() || obj_jv.is_undefined() {
            super::super::has_own_helpers::throw_to_object_nullish_type_error();
        }

        // ToPropertyKey(V): fold an object argument (e.g. one whose `toString`
        // returns a Symbol) into its canonical key before the symbol/string
        // split. A no-op for keys that are already primitives.
        //
        // #6935: root the receiver across the (GC-capable) fold — see
        // `js_object_has_own` above for the full reasoning.
        let (obj_value, key_value) = if super::super::property_key_coercion_is_inert(key_value) {
            (obj_value, key_value)
        } else {
            let scope = crate::gc::RuntimeHandleScope::new();
            let obj_handle = scope.root_heap_word_u64(obj_value.to_bits());
            let key_value = super::super::js_to_property_key(key_value);
            (f64::from_bits(obj_handle.get_heap_word_u64()), key_value)
        };

        // Proxy receiver: resolve the descriptor via `[[GetOwnProperty]]` and
        // report its `enumerable` attribute (absent property → false) rather
        // than dereferencing the fake pointer. (Proxy crash cluster.)
        if crate::proxy::js_proxy_is_proxy(obj_value) != 0 {
            let desc = crate::proxy::js_reflect_get_own_property_descriptor(obj_value, key_value);
            if desc.to_bits() == crate::value::TAG_UNDEFINED {
                return f64::from_bits(TAG_FALSE);
            }
            let desc_ptr = extract_obj_ptr(desc);
            if desc_ptr.is_null() {
                return f64::from_bits(TAG_FALSE);
            }
            let enum_key = crate::string::js_string_from_bytes(b"enumerable".as_ptr(), 10);
            let enum_v = js_object_get_field_by_name(desc_ptr as *const ObjectHeader, enum_key);
            return f64::from_bits(
                if crate::value::js_is_truthy(f64::from_bits(enum_v.bits())) != 0 {
                    TAG_TRUE
                } else {
                    TAG_FALSE
                },
            );
        }

        // Symbol-keyed lookup: route through the SYMBOL_PROPERTIES side
        // table (mirrors js_object_has_own) — string-coercing a Symbol key
        // below would never match and reported every symbol prop as
        // non-enumerable.
        if crate::symbol::js_is_symbol(key_value) != 0 {
            let bits = obj_value.to_bits();
            if crate::object::class_value::legacy_class_value_word(bits).is_some() {
                // ClassRef receivers: statics live in the class registry and
                // are non-enumerable like builtin statics.
                return f64::from_bits(TAG_FALSE);
            }
            if !crate::symbol::js_object_has_own_symbol(obj_value, key_value) {
                return f64::from_bits(TAG_FALSE);
            }
            let owner = (obj_value.to_bits() & crate::value::POINTER_MASK) as usize;
            let sym = (key_value.to_bits() & crate::value::POINTER_MASK) as usize;
            let enumerable = crate::symbol::symbol_property_is_enumerable(owner, sym);
            return f64::from_bits(if enumerable { TAG_TRUE } else { TAG_FALSE });
        }

        // #6943: root the receiver across the GC-capable key coercion — see
        // `js_object_has_own` above for the full reasoning.
        // `obj_jv` must be re-derived alongside `obj_value`: it is the tag view
        // taken at the top of this function, and the arms below both TEST it
        // (`is_any_string`) and DEREFERENCE it (`as_pointer`), so leaving it on
        // pre-coercion bits reintroduces exactly the hazard this change closes.
        let (obj_value, obj_jv, key_str) = if crate::builtins::string_coerce_is_inert(key_value) {
            (
                obj_value,
                obj_jv,
                crate::builtins::js_string_coerce(key_value),
            )
        } else {
            let scope = crate::gc::RuntimeHandleScope::new();
            let obj_handle = scope.root_heap_word_u64(obj_value.to_bits());
            let key_str = crate::builtins::js_string_coerce(key_value);
            let obj_value = f64::from_bits(obj_handle.get_heap_word_u64());
            (
                obj_value,
                crate::JSValue::from_bits(obj_value.to_bits()),
                key_str,
            )
        };
        if key_str.is_null() {
            return f64::from_bits(TAG_FALSE);
        }

        // ClassRef receiver (INT32-tagged constructor, not a heap object): the
        // only enumerable own string keys are the static FIELDS recorded in
        // CLASS_DYNAMIC_PROPS — `length`/`name`/`prototype` and static
        // methods/accessors are non-enumerable. `extract_obj_ptr` below would
        // null out on the INT32 payload and report every key non-enumerable, so
        // `verifyProperty(C, "f", …)`'s isEnumerable check failed (test262
        // class/elements static-field-declaration & friends).
        if let Some(class_id) = super::super::class_ref_id(obj_value) {
            if super::super::class_prototype_ref_id(obj_value).is_none() {
                if let Some(key_name) =
                    super::super::has_own_helpers::str_from_string_header(key_str)
                {
                    let is_static_field =
                        !super::super::field_get_set::is_internal_runtime_key(key_name)
                            && super::super::class_registry::class_own_static_field_value(
                                class_id, key_name,
                            )
                            .is_some();
                    // #10480: a declared static accessor is non-enumerable by
                    // ClassBody default, but a generic descriptor can flip it
                    // (Object.defineProperty(C, "x", { enumerable: true })).
                    let is_enumerable_static_accessor =
                        crate::object::class_value::class_static_own_accessor(class_id, key_name)
                            .is_some_and(|(_, enumerable, _)| enumerable);
                    return f64::from_bits(if is_static_field || is_enumerable_static_accessor {
                        TAG_TRUE
                    } else {
                        TAG_FALSE
                    });
                }
            }
        }

        // String primitives: index keys in range are enumerable own props;
        // "length" is a non-enumerable own prop; everything else absent.
        if obj_jv.is_any_string() {
            let present =
                super::super::has_own_helpers::string_primitive_own_key_present(obj_value, key_str);
            if !present {
                return f64::from_bits(TAG_FALSE);
            }
            let name_ptr = (key_str as *const u8).add(std::mem::size_of::<crate::StringHeader>());
            let name_len = (*key_str).byte_len as usize;
            let is_length = std::str::from_utf8(std::slice::from_raw_parts(name_ptr, name_len))
                .map(|s| s == "length")
                .unwrap_or(false);
            return f64::from_bits(if is_length { TAG_FALSE } else { TAG_TRUE });
        }

        if let Some(present) = registered_buffer_index_own_property_present(obj_value, key_str) {
            return f64::from_bits(if present { TAG_TRUE } else { TAG_FALSE });
        }

        if let Some(addr) = crate::typedarray_props::typed_array_addr_from_value(obj_value) {
            let enumerable = crate::typedarray_props::typed_array_property_is_enumerable(
                addr as *const crate::typedarray::TypedArrayHeader,
                key_str,
            );
            return f64::from_bits(if enumerable { TAG_TRUE } else { TAG_FALSE });
        }

        // Date / RegExp / Error exotic instances: expando/accessor own props
        // report their side-table enumerability (default true for plain
        // expando writes); builtin own slots are non-enumerable.
        if let Some((addr, kind)) =
            super::super::exotic_expando::exotic_expando_kind_of_value(obj_value)
        {
            let Some(key_name) = super::super::has_own_helpers::str_from_string_header(key_str)
            else {
                return f64::from_bits(TAG_FALSE);
            };
            if !super::super::exotic_expando::exotic_has_own_property(kind, addr, key_name) {
                return f64::from_bits(TAG_FALSE);
            }
            let enumerable = super::super::get_property_attrs(addr, key_name)
                .map(|a| a.enumerable())
                .unwrap_or_else(|| {
                    super::super::exotic_expando::exotic_default_enumerable(kind, key_name)
                });
            return f64::from_bits(if enumerable { TAG_TRUE } else { TAG_FALSE });
        }

        // #3655: functions/closures. Built-in `name`/`length`/`prototype` are
        // non-enumerable; user-attached props default to enumerable.
        if obj_jv.is_pointer() {
            let ptr = obj_jv.as_pointer::<u8>() as usize;
            if crate::closure::is_closure_ptr(ptr) {
                let Some(key_name) = super::super::has_own_helpers::str_from_string_header(key_str)
                else {
                    return f64::from_bits(TAG_FALSE);
                };
                if !super::super::has_own_helpers::closure_own_key_present(ptr, key_name) {
                    return f64::from_bits(TAG_FALSE);
                }
                if matches!(key_name, "name" | "length" | "prototype") {
                    let enumerable = super::super::get_property_attrs(ptr, key_name)
                        .map(|attrs| attrs.enumerable())
                        .unwrap_or(false);
                    return f64::from_bits(if enumerable { TAG_TRUE } else { TAG_FALSE });
                }
                let enumerable = super::super::get_property_attrs(ptr, key_name)
                    .map(|attrs| attrs.enumerable())
                    .unwrap_or(true);
                return f64::from_bits(if enumerable { TAG_TRUE } else { TAG_FALSE });
            }
            if crate::typedarray::lookup_typed_array_kind(ptr).is_some() {
                let enumerable = crate::typedarray_props::typed_array_property_is_enumerable(
                    ptr as *const crate::typedarray::TypedArrayHeader,
                    key_str,
                );
                return f64::from_bits(if enumerable { TAG_TRUE } else { TAG_FALSE });
            }
        }

        let obj = extract_obj_ptr(obj_value);
        if obj.is_null() || (obj as usize) < 0x10000 {
            return f64::from_bits(TAG_FALSE);
        }
        let name_ptr = (key_str as *const u8).add(std::mem::size_of::<crate::StringHeader>());
        let name_len = (*key_str).byte_len as usize;
        let key_name = match std::str::from_utf8(std::slice::from_raw_parts(name_ptr, name_len)) {
            Ok(s) => s,
            Err(_) => return f64::from_bits(TAG_FALSE),
        };
        if let Some(result) = super::super::array_property_is_enumerable(obj, key_str, key_name) {
            return result;
        }
        if !is_valid_obj_ptr(obj as *const u8) {
            return f64::from_bits(TAG_FALSE);
        }
        if (*obj).class_id == NATIVE_MODULE_CLASS_ID {
            if let Some(module_name) = read_native_module_name(obj) {
                return f64::from_bits(
                    if native_module_has_enumerable_key(&module_name, key_name) {
                        TAG_TRUE
                    } else {
                        TAG_FALSE
                    },
                );
            }
        }
        // Perry's hidden `__perry_*` runtime-internal own keys (e.g. the
        // `class … extends Map/Set` backing field) physically live in a class
        // instance's keys_array but must never be observable, so report them as
        // non-enumerable like private (`#`) elements.
        if super::super::field_get_set::own_key_hidden_bytes(obj, key_name.as_bytes()) {
            return f64::from_bits(TAG_FALSE);
        }
        if !own_key_present(obj, key_str) {
            return f64::from_bits(TAG_FALSE);
        }
        let enumerable = super::super::get_property_attrs(obj as usize, key_name)
            .map(|attrs| attrs.enumerable())
            .unwrap_or(true);
        f64::from_bits(if enumerable { TAG_TRUE } else { TAG_FALSE })
    }
}

#[cfg(feature = "keepalive-anchors")]
#[used(compiler)]
static KEEP_PROPERTY_IS_ENUMERABLE: extern "C" fn(f64, f64) -> f64 =
    js_object_property_is_enumerable;
