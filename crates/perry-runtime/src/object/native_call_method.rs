//! `js_native_call_method` — the runtime dispatch tower for
//! dynamic method calls on any-typed receivers. Also the apply/spread
//! and computed-key variants (`js_native_call_method_apply`,
//! `js_native_call_method_str_key`).
//!
//! Split out of `object/mod.rs` (issue #1103). Pure relocation — no
//! logic changes.

use super::*;

unsafe fn call_primitive_closure_value(
    receiver: f64,
    value: JSValue,
    args_ptr: *const f64,
    args_len: usize,
) -> Option<f64> {
    if value.is_undefined() {
        return None;
    }
    let bits = value.bits();
    if (bits & crate::value::TAG_MASK) != crate::value::POINTER_TAG {
        return None;
    }
    let ptr = (bits & crate::value::POINTER_MASK) as usize;
    if !crate::closure::is_closure_ptr(ptr) {
        return None;
    }
    // OrdinaryCallBindThis: a strict callee observes the raw primitive
    // receiver (`Number.prototype.f = function(){"use strict"; return
    // typeof this}` must see `"number"` for `(5).f()`); only a sloppy
    // callee gets the ToObject wrapper — boxed ONCE up front so writes
    // through `this` land on the wrapper the body later observes.
    let func_ptr = crate::closure::get_valid_func_ptr(ptr as *const crate::closure::ClosureHeader);
    let strict_callee =
        !func_ptr.is_null() && crate::closure::is_registered_strict_function(func_ptr);
    let this_receiver = if strict_callee {
        receiver
    } else {
        crate::object::js_object_coerce(receiver)
    };
    let bound = crate::closure::clone_closure_rebind_this(bits, this_receiver);
    let prev_this = crate::object::js_implicit_this_set(this_receiver);
    let result = crate::closure::js_native_call_value(f64::from_bits(bound), args_ptr, args_len);
    crate::object::js_implicit_this_set(prev_this);
    Some(result)
}

unsafe fn call_primitive_builtin_prototype_method(
    receiver: f64,
    builtin_name: &[u8],
    method_name: &str,
    args_ptr: *const f64,
    args_len: usize,
) -> Option<f64> {
    let ctor =
        crate::object::js_get_global_this_builtin_value(builtin_name.as_ptr(), builtin_name.len());
    let ctor_value = JSValue::from_bits(ctor.to_bits());
    if !ctor_value.is_pointer() {
        return None;
    }
    let registered = crate::object::class_registry::js_get_function_prototype_method(
        ctor,
        method_name.as_ptr(),
        method_name.len(),
    );
    if let Some(result) = call_primitive_closure_value(
        receiver,
        JSValue::from_bits(registered.to_bits()),
        args_ptr,
        args_len,
    ) {
        return Some(result);
    }
    let ctor_ptr = ctor_value.as_pointer::<crate::closure::ClosureHeader>() as usize;
    let proto = crate::closure::closure_get_dynamic_prop(ctor_ptr, "prototype");
    let proto_value = JSValue::from_bits(proto.to_bits());
    if !proto_value.is_pointer() {
        return None;
    }
    let proto_ptr = proto_value.as_pointer::<ObjectHeader>();
    if proto_ptr.is_null() {
        return None;
    }
    let key = crate::string::js_string_from_bytes(method_name.as_ptr(), method_name.len() as u32);
    let value = js_object_get_field_by_name(proto_ptr, key);
    call_primitive_closure_value(receiver, value, args_ptr, args_len)
}

/// A *user-installed* method on a builtin's prototype object (e.g.
/// `Number.prototype.toLocaleString = function () { … }`). Returns the patched
/// closure value, or `None` when the property is absent / not a real closure /
/// the no-op-backed builtin placeholder — i.e. `None` means "the native
/// builtin behavior is still in effect".
unsafe fn builtin_proto_user_method(builtin_name: &[u8], method_name: &str) -> Option<JSValue> {
    let ctor =
        crate::object::js_get_global_this_builtin_value(builtin_name.as_ptr(), builtin_name.len());
    let ctor_value = JSValue::from_bits(ctor.to_bits());
    if !ctor_value.is_pointer() {
        return None;
    }
    let ctor_ptr = ctor_value.as_pointer::<crate::closure::ClosureHeader>() as usize;
    let proto = crate::closure::closure_get_dynamic_prop(ctor_ptr, "prototype");
    let proto_value = JSValue::from_bits(proto.to_bits());
    if !proto_value.is_pointer() {
        return None;
    }
    let proto_ptr = proto_value.as_pointer::<ObjectHeader>();
    if proto_ptr.is_null() {
        return None;
    }
    let key = crate::string::js_string_from_bytes(method_name.as_ptr(), method_name.len() as u32);
    let value = js_object_get_field_by_name(proto_ptr, key);
    if (value.bits() & crate::value::TAG_MASK) != crate::value::POINTER_TAG {
        return None;
    }
    let ptr = (value.bits() & crate::value::POINTER_MASK) as usize;
    if !crate::closure::is_closure_ptr(ptr) {
        return None;
    }
    if (*(ptr as *const crate::closure::ClosureHeader)).func_ptr
        == super::global_this::global_this_builtin_noop_thunk as *const u8
    {
        return None;
    }
    Some(value)
}

/// Call a method on an object with dynamic dispatch
/// This is used for runtime method calls when the method cannot be resolved statically.
/// object: NaN-boxed f64 containing an object pointer
/// method_name_ptr: pointer to the method name string (raw bytes, not StringHeader)
/// method_name_len: length of the method name
/// args_ptr: pointer to array of f64 arguments
/// args_len: number of arguments
/// Returns the result as f64
///
/// NOTE: This function is named js_native_call_method to avoid symbol collision
/// with js_call_method in perry-jsruntime which handles V8 JavaScript values.

/// Apply form for method calls with spread arguments on dynamically-typed
/// receivers (refs #421). Reads `args_array_handle` (a JS array containing
/// v0.5.754: dispatch `obj[strKey](args)` — computed-key method call.
/// `name_handle` is a StringHeader pointer (already-unboxed). Extracts
/// the bytes/length from the header and forwards to
/// `js_native_call_method`. Refs #420 / drizzle's
/// `this.session[isOneTimeQuery ? "prepareOneTimeQuery" :
/// "prepareQuery"](...)` chain.
#[no_mangle]
pub unsafe extern "C" fn js_native_call_method_str_key(
    object: f64,
    name_handle: i64,
    args_ptr: *const f64,
    args_len: usize,
) -> f64 {
    if name_handle == 0 {
        return f64::from_bits(crate::value::TAG_UNDEFINED);
    }
    let str_ptr = name_handle as *const crate::StringHeader;
    let bytes_ptr = (str_ptr as *const i8).add(std::mem::size_of::<crate::StringHeader>());
    let bytes_len = (*str_ptr).byte_len as usize;
    js_native_call_method(object, bytes_ptr, bytes_len, args_ptr, args_len)
}

/// Dispatch `obj[key](args)` where `key` is a *runtime value* whose static type
/// is not provably a string (`cur._op`, `arr[i]`, a `let`-rebound key, etc.).
///
/// JS binds `this = obj` for any `obj[k](...)` call regardless of how `k` is
/// computed. The static-string fast path (`js_native_call_method_str_key`)
/// covers literal/typed-string keys; this is the dynamic-key sibling. Without
/// it, codegen fell through to a plain closure-call that dropped `this`, so a
/// method stored as a class *field* (or any property closure) reached via a
/// dynamic key read `this === undefined`. This is the dispatch half of #321 —
/// effect's `FiberRuntime` op loop is exactly `this[(cur)._op](cur)`.
///
/// String keys delegate to the full `js_native_call_method` dispatch tower
/// (own-field scan + prototype/class-id chain, all `this`-binding). Symbol
/// keys read the symbol property; other keys go through the polymorphic index
/// read. In every case the resolved callable is invoked with `this` bound.
#[no_mangle]
pub unsafe extern "C" fn js_native_call_method_value(
    object: f64,
    key: f64,
    args_ptr: *const f64,
    args_len: usize,
) -> f64 {
    let key_jsval = JSValue::from_bits(key.to_bits());
    let is_symbol_key = crate::symbol::js_is_symbol(key) != 0;

    if is_symbol_key {
        let sym_key = crate::symbol::sym_key_from_f64(key);
        if sym_key != 0 {
            let bits = object.to_bits();
            let top16 = bits >> 48;
            if top16 == 0x7FFE {
                let class_id = (bits & 0xFFFF_FFFF) as u32;
                let is_prototype_ref = crate::object::class_prototype_ref_id(object).is_some();
                if is_prototype_ref {
                    if let Some((func_ptr, param_count, has_rest)) =
                        lookup_class_symbol_method_in_chain(class_id, sym_key, false)
                    {
                        return call_vtable_method(
                            func_ptr,
                            object.to_bits() as i64,
                            args_ptr,
                            args_len,
                            param_count,
                            // Computed symbol methods never synthesize an
                            // `arguments` object, but DO carry a `has_rest`
                            // flag for a trailing user rest param.
                            false,
                            has_rest,
                        );
                    }
                } else {
                    if let Some((func_ptr, param_count, has_rest)) =
                        lookup_class_symbol_method_in_chain(class_id, sym_key, true)
                    {
                        let prev_this = crate::object::js_implicit_this_set(object);
                        let result = call_registered_static_method(
                            func_ptr,
                            args_ptr,
                            args_len,
                            param_count,
                            has_rest,
                        );
                        crate::object::js_implicit_this_set(prev_this);
                        return result;
                    }
                }
            } else if is_class_object_value(object) {
                let obj = JSValue::from_bits(bits).as_pointer::<ObjectHeader>();
                let class_id = js_object_get_class_id(obj);
                if let Some((func_ptr, param_count, has_rest)) =
                    lookup_class_symbol_method_in_chain(class_id, sym_key, true)
                {
                    let prev_this = crate::object::js_implicit_this_set(object);
                    let result = call_registered_static_method(
                        func_ptr,
                        args_ptr,
                        args_len,
                        param_count,
                        has_rest,
                    );
                    crate::object::js_implicit_this_set(prev_this);
                    return result;
                }
            } else if key_jsval.is_pointer() || JSValue::from_bits(bits).is_pointer() {
                let obj_val = JSValue::from_bits(bits);
                if obj_val.is_pointer() {
                    let obj = obj_val.as_pointer::<ObjectHeader>();
                    if !obj.is_null() && is_valid_obj_ptr(obj as *const u8) {
                        let class_id = js_object_get_class_id(obj);
                        if class_id != 0 {
                            if let Some((func_ptr, param_count, has_rest)) =
                                lookup_class_symbol_method_in_chain(class_id, sym_key, false)
                            {
                                let this_i64 = obj as i64;
                                return call_vtable_method(
                                    func_ptr,
                                    this_i64,
                                    args_ptr,
                                    args_len,
                                    param_count,
                                    // Computed symbol methods never synthesize an
                                    // `arguments` object, but DO carry `has_rest`.
                                    false,
                                    has_rest,
                                );
                            }
                        }
                    }
                }
            }
        }
    }

    let property_key = if is_symbol_key {
        key
    } else {
        crate::object::js_to_property_key(key)
    };
    if !is_symbol_key && crate::symbol::js_is_symbol(property_key) != 0 {
        return js_native_call_method_value(object, property_key, args_ptr, args_len);
    }

    // String key (incl. SSO short strings): forward to the dispatch tower,
    // which both finds own-field closures and binds `this`.
    let property_key_jsval = JSValue::from_bits(property_key.to_bits());
    if property_key_jsval.is_any_string() {
        let str_ptr =
            crate::value::js_get_string_pointer_unified(property_key) as *const crate::StringHeader;
        if !str_ptr.is_null() {
            let bytes_ptr = (str_ptr as *const i8).add(std::mem::size_of::<crate::StringHeader>());
            let bytes_len = (*str_ptr).byte_len as usize;
            return js_native_call_method(object, bytes_ptr, bytes_len, args_ptr, args_len);
        }
    }

    // `str[Symbol.iterator]()` — a primitive string carries no symbol property
    // slot, so the symbol-property read below would return undefined. Route the
    // well-known iterator symbol on a string receiver to the string method
    // dispatcher, which builds a real String iterator object.
    if is_symbol_key {
        let iter_wk = crate::symbol::well_known_symbol("iterator");
        let is_iterator_symbol = !iter_wk.is_null() && {
            let iter_f64 = f64::from_bits(JSValue::pointer(iter_wk as *const u8).bits());
            crate::symbol::sym_key_from_f64(key) == crate::symbol::sym_key_from_f64(iter_f64)
        };
        if is_iterator_symbol {
            let obj_val = JSValue::from_bits(object.to_bits());
            if obj_val.is_any_string() {
                // `str[Symbol.iterator]()` — a primitive string carries no symbol
                // property slot, so route to the string method dispatcher which
                // builds a real String iterator object.
                let name = b"Symbol.iterator";
                return js_native_call_method(
                    object,
                    name.as_ptr() as *const i8,
                    name.len(),
                    args_ptr,
                    args_len,
                );
            }
            if obj_val.is_pointer() {
                let obj = obj_val.as_pointer::<ObjectHeader>();
                // `arguments[Symbol.iterator]()` — an arguments exotic object
                // implements the Array iterator protocol but stores no symbol
                // slot. `js_get_iterator` materializes it to an array iterator.
                if !obj.is_null() && crate::object::is_arguments_object(obj) {
                    return crate::symbol::js_get_iterator(object);
                }
            }
        }
    }

    // Non-string key: read the property value, then invoke it with `this`
    // bound to the receiver (the codegen `Expr::This` fallback reads
    // `IMPLICIT_THIS` when there's no lexical `this`).
    let field = if is_symbol_key {
        crate::symbol::js_object_get_symbol_property(object, key)
    } else {
        crate::object::js_object_get_index_polymorphic(object.to_bits() as i64, property_key)
    };
    let fv = JSValue::from_bits(field.to_bits());
    if fv.is_undefined() || fv.is_null() {
        return f64::from_bits(crate::value::TAG_UNDEFINED);
    }

    // #321 (effect Context/Layer): a symbol-keyed method INHERITED via
    // `Object.create(proto)` is stored under the *prototype's* identity, and
    // object-literal computed-key methods bake their receiver into a reserved
    // `this` capture slot at construction time (see
    // `symbol.rs::js_object_set_symbol_method` /
    // `dynamic_props.rs::clone_closure_rebind_this`). So when `o = Object.create(P)`
    // resolves `o[SYM]()`, the closure we get back carries `this === P`, not
    // `this === o`, and `IMPLICIT_THIS` alone can't override the baked-in slot.
    // When the symbol method is NOT an OWN property of the receiver (i.e. it was
    // inherited through the prototype chain), rebind its `this` slot to the
    // receiver before invoking. `clone_closure_rebind_this` is a no-op for
    // non-`captures_this` closures and for non-closure values, so own methods
    // (whose slot is already the receiver), effect's Tag-class symbol *statics*
    // (plain data values), and any closure that doesn't read `this` are all left
    // untouched — keeping the #1758/#36/#321 closure-proto-chain paths intact.
    let field = if is_symbol_key && crate::symbol::own_symbol_property(object, key).is_none() {
        f64::from_bits(crate::closure::clone_closure_rebind_this(
            field.to_bits(),
            object,
        ))
    } else {
        field
    };

    let prev_this = IMPLICIT_THIS.with(|c| c.replace(object.to_bits()));
    let result = crate::closure::js_native_call_value(field, args_ptr, args_len);
    IMPLICIT_THIS.with(|c| c.set(prev_this));
    result
}

/// every regular + spread arg already concatenated by codegen), materialises
/// the f64 elements into a temporary `Vec<f64>`, and forwards to
/// `js_native_call_method`. Lets the caller use a single uniform shape for
/// `recv.method(...args)` without exposing array layout to the dispatcher.
#[no_mangle]
pub unsafe extern "C" fn js_native_call_method_apply(
    object: f64,
    method_name_ptr: *const i8,
    method_name_len: usize,
    args_array_handle: i64,
) -> f64 {
    let arr = args_array_handle as *const crate::array::ArrayHeader;
    let len = if arr.is_null() {
        0
    } else {
        crate::array::js_array_length(arr) as usize
    };
    let buf: Vec<f64> = (0..len)
        .map(|i| crate::array::js_array_get_f64(arr, i as u32))
        .collect();
    let (args_ptr, args_len) = if buf.is_empty() {
        (std::ptr::null::<f64>(), 0_usize)
    } else {
        (buf.as_ptr(), buf.len())
    };
    js_native_call_method(object, method_name_ptr, method_name_len, args_ptr, args_len)
}

#[inline]
fn root_string_arg_handle<'scope>(
    scope: &'scope crate::gc::RuntimeHandleScope,
    arg_handles: &[crate::gc::RuntimeHandle<'scope>],
    index: usize,
) -> Option<crate::gc::RuntimeHandle<'scope>> {
    let value = arg_handles.get(index)?.get_nanbox_f64();
    let ptr = crate::value::js_get_string_pointer_unified(value) as *const crate::StringHeader;
    if ptr.is_null() {
        None
    } else {
        Some(scope.root_string_ptr(ptr))
    }
}

fn throw_type_error_message(message: &[u8]) -> ! {
    let msg = crate::string::js_string_from_bytes(message.as_ptr(), message.len() as u32);
    let err = crate::error::js_typeerror_new(msg);
    crate::exception::js_throw(crate::value::js_nanbox_pointer(err as i64))
}

pub(crate) fn throw_object_value_of_nullish_receiver() -> ! {
    throw_type_error_message(b"Cannot convert undefined or null to object")
}

pub(crate) fn throw_object_to_locale_string_nullish_receiver() -> ! {
    throw_type_error_message(b"Object.prototype.toLocaleString called on null or undefined")
}

fn throw_object_to_string_not_function() -> ! {
    crate::error::js_throw_type_error_not_a_function(
        std::ptr::null(),
        0,
        b"toString".as_ptr(),
        "toString".len(),
    )
}

#[inline]
unsafe fn gc_pointer_and_type_from_value(value: f64) -> Option<(*const u8, u8)> {
    let jsval = JSValue::from_bits(value.to_bits());
    let ptr = if jsval.is_pointer() {
        jsval.as_pointer::<u8>()
    } else {
        let bits = value.to_bits();
        if (bits >> 48) == 0 && bits >= (crate::gc::GC_HEADER_SIZE as u64) + 0x1000 {
            bits as *const u8
        } else {
            return None;
        }
    };
    if ptr.is_null() || (ptr as usize) < crate::gc::GC_HEADER_SIZE + 0x1000 {
        return None;
    }
    let addr = ptr as usize;
    if crate::buffer::is_any_array_buffer(addr) {
        return Some((ptr, crate::gc::GC_TYPE_BUFFER));
    }
    if crate::buffer::is_uint8array_buffer(addr) {
        return Some((ptr, crate::gc::GC_TYPE_BUFFER));
    }
    if crate::typedarray::lookup_typed_array_kind(addr).is_some() {
        return Some((ptr, crate::gc::GC_TYPE_TYPED_ARRAY));
    }
    if !is_valid_obj_ptr(ptr as *const u8) {
        return None;
    }
    if crate::set::is_registered_set(addr)
        || crate::map::is_registered_map(addr)
        || crate::regex::is_regex_pointer(ptr as *const u8)
        || crate::symbol::is_registered_symbol(addr)
    {
        return None;
    }
    let gc_header = (ptr as *const u8).sub(crate::gc::GC_HEADER_SIZE) as *const crate::gc::GcHeader;
    Some((ptr, (*gc_header).obj_type))
}

#[inline]
pub(crate) unsafe fn object_ptr_from_value(value: f64) -> Option<*mut ObjectHeader> {
    let (ptr, gc_type) = gc_pointer_and_type_from_value(value)?;
    if gc_type == crate::gc::GC_TYPE_OBJECT {
        Some(ptr as *mut ObjectHeader)
    } else {
        None
    }
}

/// #4795: resolve `obj[Symbol.dispose]` / `obj[Symbol.asyncDispose]` for the
/// `using`-disposal method names when the disposer is stored under the
/// well-known-symbol key (object literals, dynamically-assigned). Returns
/// `None` (so the caller falls through to vtable / native-handle dispatch)
/// when `object` is not a heap object or has no symbol-keyed disposer.
unsafe fn try_symbol_dispose_dispatch(
    object: f64,
    method_name: &str,
    args_ptr: *const f64,
    args_len: usize,
) -> Option<f64> {
    // Only real heap objects store symbol-keyed methods. Native handles and
    // primitives return None here and fall through to the existing dispatch.
    let _obj = object_ptr_from_value(object)?;
    let want_async = method_name == "__perry_async_dispose__";
    let shorts: &[&str] = if want_async {
        &["asyncDispose", "dispose"]
    } else {
        &["dispose"]
    };
    for short in shorts {
        let sym = crate::symbol::well_known_symbol(short);
        if sym.is_null() {
            continue;
        }
        let sym_f64 = f64::from_bits(JSValue::pointer(sym as *const u8).bits());
        let method = crate::symbol::js_object_get_symbol_property(object, sym_f64);
        let mjsv = JSValue::from_bits(method.to_bits());
        if method.to_bits() != crate::value::TAG_UNDEFINED && !mjsv.is_null() && mjsv.is_pointer() {
            let prev = IMPLICIT_THIS.with(|c| c.replace(object.to_bits()));
            let result = crate::closure::js_native_call_value(method, args_ptr, args_len);
            IMPLICIT_THIS.with(|c| c.set(prev));
            return Some(result);
        }
    }
    None
}

/// Does `obj` (a real heap object) expose a callable disposer? Checks the
/// well-known-symbol keys, the renamed class-method names, and the class
/// vtable. `want_async` additionally accepts `[Symbol.asyncDispose]` /
/// `__perry_async_dispose__` (with the spec sync fallback).
unsafe fn object_has_dispose_method(obj: *mut ObjectHeader, object: f64, want_async: bool) -> bool {
    // Symbol-keyed disposers (object literals, dynamic assignment).
    let syms: &[&str] = if want_async {
        &["asyncDispose", "dispose"]
    } else {
        &["dispose"]
    };
    for short in syms {
        let sym = crate::symbol::well_known_symbol(short);
        if sym.is_null() {
            continue;
        }
        let sym_f64 = f64::from_bits(JSValue::pointer(sym as *const u8).bits());
        let m = crate::symbol::js_object_get_symbol_property(object, sym_f64);
        let mjsv = JSValue::from_bits(m.to_bits());
        if m.to_bits() != crate::value::TAG_UNDEFINED && !mjsv.is_null() && mjsv.is_pointer() {
            return true;
        }
    }
    // String-keyed / vtable disposers (class instances). The renamed class
    // method `[Symbol.dispose]` → `__perry_dispose__` lives in the vtable.
    let names: &[&str] = if want_async {
        &["__perry_async_dispose__", "__perry_dispose__"]
    } else {
        &["__perry_dispose__"]
    };
    let class_id = (*obj).class_id;
    for name in names {
        let key = crate::string::js_string_from_bytes(name.as_ptr(), name.len() as u32);
        if !key.is_null() {
            let v = js_object_get_field_by_name(obj as *const ObjectHeader, key);
            if !v.is_undefined() && !v.is_null() {
                return true;
            }
        }
        if class_id != 0 {
            if let Ok(registry) = CLASS_VTABLE_REGISTRY.read() {
                if let Some(ref reg) = *registry {
                    if let Some(vtable) = reg.get(&class_id) {
                        if vtable.methods.contains_key(*name) {
                            return true;
                        }
                    }
                }
            }
        }
    }
    false
}

/// #4795: dispatch a `DisposableStack` / `AsyncDisposableStack` instance method
/// reached through the generic (dynamic) call path. Returns `None` for
/// non-stack receivers / unknown methods so the caller continues normal
/// dispatch.
unsafe fn try_disposable_stack_method_dispatch(
    object: f64,
    method_name: &str,
    args_ptr: *const f64,
    args_len: usize,
) -> Option<f64> {
    use crate::disposable::{CLASS_ID_ASYNC_DISPOSABLE_STACK, CLASS_ID_DISPOSABLE_STACK};
    let obj = object_ptr_from_value(object)?;
    let class_id = (*obj).class_id;
    let is_async = class_id == CLASS_ID_ASYNC_DISPOSABLE_STACK;
    if class_id != CLASS_ID_DISPOSABLE_STACK && !is_async {
        return None;
    }
    let arg0 = if args_len > 0 && !args_ptr.is_null() {
        *args_ptr
    } else {
        f64::from_bits(crate::value::TAG_UNDEFINED)
    };
    let arg1 = if args_len > 1 && !args_ptr.is_null() {
        *args_ptr.add(1)
    } else {
        f64::from_bits(crate::value::TAG_UNDEFINED)
    };
    let r = match method_name {
        "use" if is_async => crate::disposable::js_async_disposable_stack_use(obj, arg0),
        "use" => crate::disposable::js_disposable_stack_use(obj, arg0),
        "adopt" => crate::disposable::js_disposable_stack_adopt(obj, arg0, arg1),
        "defer" => crate::disposable::js_disposable_stack_defer(obj, arg0),
        "move" => crate::disposable::js_disposable_stack_move(obj),
        "dispose" if !is_async => crate::disposable::js_disposable_stack_dispose(obj),
        "disposeAsync" if is_async => {
            crate::disposable::js_async_disposable_stack_dispose_async(obj)
        }
        "@@__perry_wk_dispose" if !is_async => crate::disposable::js_disposable_stack_dispose(obj),
        "@@__perry_wk_asyncDispose" if is_async => {
            crate::disposable::js_async_disposable_stack_dispose_async(obj)
        }
        _ => return None,
    };
    Some(r)
}

/// #4795: validate a `using` / `await using` initializer at declaration time.
/// `null` / `undefined` are accepted (no-op disposal). Any other non-object,
/// or an object lacking a callable `[Symbol.dispose]` / `[Symbol.asyncDispose]`,
/// throws `TypeError`. Native runtime handles (timers, sqlite, …) that expose
/// dispose through name dispatch are accepted.
unsafe fn js_using_check_disposable(object: f64, want_async: bool) -> f64 {
    let jsv = JSValue::from_bits(object.to_bits());
    let undef = f64::from_bits(crate::value::TAG_UNDEFINED);
    if jsv.is_null() || jsv.is_undefined() {
        return undef;
    }
    let throw_not_object = |kind: &str| -> ! {
        let msg = format!("Value used in a `using` declaration is not an object: {kind}");
        let s = crate::string::js_string_from_bytes(msg.as_ptr(), msg.len() as u32);
        let err = crate::error::js_typeerror_new(s);
        crate::exception::js_throw(f64::from_bits(JSValue::pointer(err as *const u8).bits()))
    };
    // Non-object primitives (number / boolean / string / bigint) are never
    // disposable. Strings are string-tagged (not pointer-tagged) and fall here.
    if !jsv.is_pointer() {
        throw_not_object("primitive");
    }
    let raw = (object.to_bits() & 0x0000_FFFF_FFFF_FFFF) as usize;
    if crate::symbol::is_registered_symbol(raw) {
        throw_not_object("symbol");
    }
    if let Some(obj) = object_ptr_from_value(object) {
        if object_has_dispose_method(obj, object, want_async) {
            return undef;
        }
        let sym = if want_async {
            "Symbol.asyncDispose"
        } else {
            "Symbol.dispose"
        };
        let msg = format!("The value used in a `using` declaration must have a {sym} method");
        let s = crate::string::js_string_from_bytes(msg.as_ptr(), msg.len() as u32);
        let err = crate::error::js_typeerror_new(s);
        crate::exception::js_throw(f64::from_bits(JSValue::pointer(err as *const u8).bits()))
    }
    // Pointer-shaped but not a GC heap object (native runtime handle). These
    // dispatch dispose through `js_native_call_method` name handling; accept.
    undef
}

unsafe fn object_has_null_proto_flag(object: *const ObjectHeader) -> bool {
    let gc_header =
        (object as *const u8).sub(crate::gc::GC_HEADER_SIZE) as *const crate::gc::GcHeader;
    ((*gc_header)._reserved & crate::gc::OBJ_FLAG_NULL_PROTO) != 0
}

unsafe fn call_object_to_string_method(object: f64) -> Option<f64> {
    let scope = crate::gc::RuntimeHandleScope::new();
    let object_handle = scope.root_nanbox_f64(object);
    let receiver = object_handle.get_nanbox_f64();
    let obj_ptr = object_ptr_from_value(receiver)?;
    let key = crate::string::js_string_from_bytes(b"toString".as_ptr(), 8);
    let key_handle = scope.root_string_ptr(key);
    let key_ptr = key_handle.get_raw_const_ptr::<crate::StringHeader>();
    let method = js_object_get_field_by_name(obj_ptr as *const ObjectHeader, key_ptr);
    if method.is_undefined() {
        if own_key_present(obj_ptr, key_ptr) || object_has_null_proto_flag(obj_ptr) {
            throw_object_to_string_not_function();
        }
        return None;
    }
    if method.is_null() {
        throw_object_to_string_not_function();
    }
    let method_bits = method.bits();
    if (method_bits & 0xFFFF_0000_0000_0000) != crate::value::POINTER_TAG {
        throw_object_to_string_not_function();
    }
    let method_ptr = (method_bits & 0x0000_FFFF_FFFF_FFFF) as usize;
    if !crate::closure::is_closure_ptr(method_ptr) {
        throw_object_to_string_not_function();
    }
    let bound = crate::closure::clone_closure_rebind_this(method_bits, receiver);
    let prev_this = crate::object::js_implicit_this_set(receiver);
    let result = crate::closure::js_native_call_value(f64::from_bits(bound), std::ptr::null(), 0);
    crate::object::js_implicit_this_set(prev_this);
    Some(result)
}

pub(crate) unsafe fn js_object_default_value_of(receiver: f64) -> f64 {
    let jsval = JSValue::from_bits(receiver.to_bits());
    if jsval.is_undefined() || jsval.is_null() {
        throw_object_value_of_nullish_receiver();
    }
    if let Some((_, payload)) = crate::builtins::boxed_primitive_payload(receiver) {
        return payload;
    }
    // Spec 20.1.3.7: `Object.prototype.valueOf` returns ToObject(this). A
    // primitive receiver (`Object.prototype.valueOf.call(true)`) yields its
    // wrapper object (`typeof` must report "object"), not the primitive.
    // Object receivers (including the fused boxed-wrapper arm above, which
    // serves the `Object(5).valueOf()` Number.prototype.valueOf resolution)
    // pass through unchanged.
    if !jsval.is_pointer() {
        return crate::object::js_object_coerce(receiver);
    }
    receiver
}

pub(crate) unsafe fn js_object_default_to_locale_string(receiver: f64) -> f64 {
    let jsval = JSValue::from_bits(receiver.to_bits());
    if jsval.is_undefined() || jsval.is_null() {
        throw_object_to_locale_string_nullish_receiver();
    }
    // #2808: numbers use `Number.prototype.toLocaleString` (thousands
    // separators), so a number element / receiver formats as `1,000.5` rather
    // than the bare `toString` form. Locale/option-aware grouping is not yet
    // modeled — the default-locale grouping matches Node's en-US output for
    // the common integer/decimal cases.
    if jsval.is_number() {
        let s = crate::date::js_number_to_locale_string(jsval.as_number());
        return f64::from_bits(JSValue::string_ptr(s).bits());
    }
    // #2808: a Date value uses `Date.prototype.toLocaleString` (date+time
    // rendering) rather than `[object Date]`.
    if crate::date::is_date_value(receiver) {
        let ts = crate::date::date_cell_timestamp(receiver);
        let s = crate::date::js_date_to_locale_string(ts);
        return f64::from_bits(JSValue::string_ptr(s).bits());
    }
    if !jsval.is_pointer() {
        return js_native_call_method(
            receiver,
            b"toString".as_ptr() as *const i8,
            "toString".len(),
            std::ptr::null(),
            0,
        );
    }
    // An own `toLocaleString` closure wins over the default rendering —
    // notably `%TypedArray%.prototype.toLocaleString()` invoked as a method ON
    // the prototype object itself must run the installed brand-check thunk
    // (which throws for the non-TypedArray receiver, test262
    // toLocaleString/invoked-as-method).
    {
        let own = crate::object::js_object_get_own_field_or_undef(
            receiver,
            b"toLocaleString".as_ptr(),
            14,
        );
        let own_value = JSValue::from_bits(own.to_bits());
        if let Some(result) = call_primitive_closure_value(receiver, own_value, std::ptr::null(), 0)
        {
            return result;
        }
    }
    if let Some(result) = call_object_to_string_method(receiver) {
        return result;
    }
    crate::object::js_object_to_string(receiver)
}

/// #4546: codegen entry point for `value.toLocaleString()` when the
/// receiver's static type is unknown (plain object, string, boolean) — the
/// `Expr::DateToLocaleString` LLVM arm used to mis-route every non-number
/// receiver to `js_date_to_locale_string`, yielding a 1970-epoch
/// "Invalid Date" string. Dispatches on the runtime tag (number → grouping,
/// Date → date string, object → custom/`[object Object]`). Returns an
/// already-NaN-boxed value.
#[no_mangle]
pub extern "C" fn js_value_to_locale_string(receiver: f64) -> f64 {
    unsafe { js_object_default_to_locale_string(receiver) }
}

/// Shared implementation for `Object.prototype.isPrototypeOf`.
pub(crate) unsafe fn js_object_is_prototype_of_value(receiver: f64, target: f64) -> bool {
    // The receiver (and every link in the target's `[[Prototype]]` chain) is
    // compared by raw heap address. Exotic-typed prototype objects —
    // `Array.prototype` is itself a GC_TYPE_ARRAY, `Uint8Array.prototype` a
    // typed-array proto — are NOT `GC_TYPE_OBJECT`, so resolving them with
    // `object_ptr_from_value` (which only accepts GC_TYPE_OBJECT) returned
    // `None` and the walk bailed. #4549: use the raw GC pointer instead.
    let heap_addr = |v: f64| -> Option<usize> {
        gc_pointer_and_type_from_value(v).map(|(ptr, _)| ptr as usize)
    };
    let receiver_addr = match heap_addr(receiver) {
        Some(addr) => addr,
        None => return false,
    };

    if crate::date::is_date_value(target) {
        let ctor = crate::object::js_get_global_this_builtin_value(b"Date".as_ptr(), 4);
        let ctor_ptr = crate::value::js_nanbox_get_pointer(ctor) as usize;
        if ctor_ptr == 0 {
            return false;
        }
        let proto = crate::closure::closure_get_dynamic_prop(ctor_ptr, "prototype");
        if let Some(proto_addr) = heap_addr(proto) {
            return proto_addr == receiver_addr;
        }
        return false;
    }

    // A RegExp's `[[Prototype]]` chain is `RegExp.prototype → Object.prototype`.
    // The RegExpHeader isn't a plain GC_TYPE_OBJECT with a registered class
    // prototype, so the generic class-id walk below misses it (which is why
    // `RegExp.prototype.isPrototypeOf(re)` returned false). Handle it directly.
    {
        let tv = JSValue::from_bits(target.to_bits());
        if tv.is_pointer() && crate::regex::is_regex_pointer(tv.as_pointer::<u8>()) {
            for name in ["RegExp", "Object"] {
                let proto = crate::object::builtin_prototype_value(name);
                if let Some(proto_addr) = heap_addr(proto) {
                    if proto_addr == receiver_addr {
                        return true;
                    }
                }
            }
            return false;
        }
    }

    let target_jsval = JSValue::from_bits(target.to_bits());
    if !target_jsval.is_pointer() && gc_pointer_and_type_from_value(target).is_none() {
        return false;
    }

    if let Some(target_ptr) = object_ptr_from_value(target) {
        let has_instance_prototype =
            crate::object::prototype_chain::object_static_prototype(target_ptr as usize).is_some();
        if target_ptr as usize == receiver_addr {
            return false;
        }
        // A `new Func()` instance snapshots the function's current
        // `.prototype` via the object prototype side table. Honor that
        // per-instance chain before consulting the synthetic class map,
        // because later `Func.prototype = other` must not rewrite older
        // instances.
        if !has_instance_prototype {
            let mut cid = crate::object::js_object_get_class_id(target_ptr as *const ObjectHeader);
            let mut depth = 0usize;
            let mut visited: [u32; 32] = [0; 32];
            while cid != 0 && depth < visited.len() {
                if visited[..depth].contains(&cid) {
                    break;
                }
                visited[depth] = cid;

                let proto_obj = crate::object::class_registry::class_prototype_object(cid);
                let mut next_cid = 0;
                if !proto_obj.is_null() {
                    if proto_obj as usize == receiver_addr {
                        return true;
                    }
                    next_cid =
                        crate::object::js_object_get_class_id(proto_obj as *const ObjectHeader);
                }

                if next_cid != 0 && next_cid != cid {
                    cid = next_cid;
                    depth += 1;
                    continue;
                }

                match crate::object::class_registry::get_parent_class_id(cid) {
                    Some(parent_id) if parent_id != 0 && parent_id != cid => {
                        cid = parent_id;
                        depth += 1;
                    }
                    _ => break,
                }
            }
        }
    } else {
        let (_, target_gc_type) = match gc_pointer_and_type_from_value(target) {
            Some(info) => info,
            None => return false,
        };
        // #4549: arrays and typed arrays are objects whose `[[Prototype]]`
        // chain is modeled (`Array.prototype` → `Object.prototype`,
        // `Uint8Array.prototype` → `%TypedArray%.prototype` →
        // `Object.prototype`), so they must reach the generic walk below.
        // Previously only closures/errors were allowed, so
        // `Array.prototype.isPrototypeOf([1, 2])` and
        // `Object.prototype.isPrototypeOf([])` wrongly returned `false`.
        // #4554: ArrayBuffer / SharedArrayBuffer use BufferHeader storage
        // without a GcHeader for small buffers, but they still have a modeled
        // prototype chain via `js_object_get_prototype_of`.
        if target_gc_type != crate::gc::GC_TYPE_CLOSURE
            && target_gc_type != crate::gc::GC_TYPE_ERROR
            && target_gc_type != crate::gc::GC_TYPE_ARRAY
            && target_gc_type != crate::gc::GC_TYPE_TYPED_ARRAY
            && target_gc_type != crate::gc::GC_TYPE_BUFFER
        {
            return false;
        }
    }

    let mut current = target;
    for _ in 0..32 {
        let current_addr = heap_addr(current);
        let proto = crate::object::js_object_get_prototype_of(current);
        let proto_jsval = JSValue::from_bits(proto.to_bits());
        if proto_jsval.is_null() || proto_jsval.is_undefined() {
            break;
        }
        let proto_addr = match heap_addr(proto) {
            Some(addr) => addr,
            None => break,
        };
        if current_addr == Some(proto_addr) {
            break;
        }
        if proto_addr == receiver_addr {
            return true;
        }
        current = proto;
    }

    false
}

/// Dispatch a `%TypedArray%` instance method on an already-resolved
/// `TypedArrayHeader` pointer. Returns `Some(result)` when handled, `None` when
/// the method isn't a typed-array method (caller falls through to the generic
/// dispatch tower / catch-all). Shared between the raw-pointer (#654) and
/// NaN-boxed POINTER_TAG receiver paths so a `Uint8Array` local reaches the
/// element-typed `js_typed_array_*` helpers regardless of how codegen boxed
/// the receiver. Issues #2797 / #2798 / #2799 added the callback-bearing arms.
pub(super) unsafe fn dispatch_typed_array_method(
    ta: *mut crate::typedarray::TypedArrayHeader,
    method_name: &str,
    args_ptr: *const f64,
    args_len: usize,
) -> Option<f64> {
    let arg0 = || -> f64 {
        if args_len >= 1 && !args_ptr.is_null() {
            *args_ptr
        } else {
            f64::NAN
        }
    };
    // #4091: validate the 1st argument is callable, throwing a spec `TypeError`
    // otherwise (this dynamic dispatch tower is the inline-`new` /
    // `Uint8Array`-local path, where the boxed callback is still available).
    // `map` uses %TypedArray%.prototype.map's distinct non-callable rendering.
    let validate_cb = |map_form: bool| -> *const crate::closure::ClosureHeader {
        let boxed = if args_len >= 1 && !args_ptr.is_null() {
            *args_ptr
        } else {
            f64::from_bits(crate::value::TAG_UNDEFINED)
        };
        let p = if map_form {
            crate::array::js_validate_array_map_callback(ta as i64, boxed)
        } else {
            crate::array::js_validate_array_callback(boxed)
        };
        p as *const crate::closure::ClosureHeader
    };
    let r = match method_name {
        "length" => crate::typedarray::js_typed_array_length(ta) as f64,
        "at" => crate::typedarray::js_typed_array_at(ta, arg0()),
        "sort" => {
            // #2796: validate the comparator (function | undefined) before sorting.
            let cmp = if args_len >= 1 && !args_ptr.is_null() {
                crate::array::js_validate_array_comparator(*args_ptr)
                    as *const crate::closure::ClosureHeader
            } else {
                std::ptr::null()
            };
            let result = if cmp.is_null() {
                crate::typedarray::js_typed_array_sort_default(ta)
            } else {
                crate::typedarray::js_typed_array_sort_with_comparator(ta, cmp)
            };
            f64::from_bits(JSValue::pointer(result as *mut u8).bits())
        }
        "toSorted" => {
            let cmp = if args_len >= 1 && !args_ptr.is_null() {
                crate::array::js_validate_array_comparator(*args_ptr)
                    as *const crate::closure::ClosureHeader
            } else {
                std::ptr::null()
            };
            let result = if cmp.is_null() {
                crate::typedarray::js_typed_array_to_sorted_default(ta)
            } else {
                crate::typedarray::js_typed_array_to_sorted_with_comparator(ta, cmp)
            };
            f64::from_bits(JSValue::pointer(result as *mut u8).bits())
        }
        "toReversed" => f64::from_bits(
            JSValue::pointer(crate::typedarray::js_typed_array_to_reversed(ta) as *mut u8).bits(),
        ),
        // #2879: bulk `set(source, offset?)` and `copyWithin`.
        "set" => {
            let source = arg0();
            let offset = if args_len >= 2 && !args_ptr.is_null() {
                *args_ptr.add(1)
            } else {
                0.0
            };
            crate::typedarray::js_typed_array_set_from(ta, source, offset)
        }
        "copyWithin" => {
            let target = arg0();
            let start = if args_len >= 2 && !args_ptr.is_null() {
                *args_ptr.add(1)
            } else {
                0.0
            };
            let end = if args_len >= 3 && !args_ptr.is_null() {
                *args_ptr.add(2)
            } else {
                f64::from_bits(crate::value::TAG_UNDEFINED)
            };
            f64::from_bits(
                JSValue::pointer(crate::typedarray::js_typed_array_copy_within(
                    ta, target, start, end,
                ) as *mut u8)
                .bits(),
            )
        }
        "with" => {
            let idx = arg0();
            let val = if args_len >= 2 && !args_ptr.is_null() {
                *args_ptr.add(1)
            } else {
                f64::NAN
            };
            f64::from_bits(
                JSValue::pointer(crate::typedarray::js_typed_array_with(ta, idx, val) as *mut u8)
                    .bits(),
            )
        }
        "findLast" => crate::typedarray::js_typed_array_find_last(ta, validate_cb(false)),
        "findLastIndex" => {
            crate::typedarray::js_typed_array_find_last_index(ta, validate_cb(false))
        }
        // #2797/#2798/#2799: callback-bearing %TypedArray% methods. The codegen
        // lowerers only fire for receivers it can statically prove are plain
        // Arrays; a `Uint8Array` local reaches this dynamic dispatch tower,
        // where these arms previously fell through to the undefined catch-all
        // (so `ta.map`/`ta.reduce`/`ta.find` silently no-op'd).
        "map" => {
            let result = crate::typedarray::js_typed_array_map(ta, validate_cb(true));
            f64::from_bits(JSValue::pointer(result as *mut u8).bits())
        }
        "filter" => {
            let result = crate::typedarray::js_typed_array_filter(ta, validate_cb(false));
            f64::from_bits(JSValue::pointer(result as *mut u8).bits())
        }
        "forEach" => crate::typedarray::js_typed_array_for_each(ta, validate_cb(false)),
        "some" => crate::typedarray::js_typed_array_some(ta, validate_cb(false)),
        "every" => crate::typedarray::js_typed_array_every(ta, validate_cb(false)),
        "find" => crate::typedarray::js_typed_array_find(ta, validate_cb(false)),
        "findIndex" => crate::typedarray::js_typed_array_find_index(ta, validate_cb(false)),
        "values" | "Symbol.iterator" | "@@iterator" => {
            let iter =
                crate::array::js_array_values_iter_obj(ta as *const crate::array::ArrayHeader);
            if iter == 0 {
                f64::from_bits(crate::value::TAG_UNDEFINED)
            } else {
                f64::from_bits(JSValue::pointer(iter as *mut u8).bits())
            }
        }
        "keys" => {
            let iter = crate::array::js_array_keys_iter_obj(ta as *const crate::array::ArrayHeader);
            if iter == 0 {
                f64::from_bits(crate::value::TAG_UNDEFINED)
            } else {
                f64::from_bits(JSValue::pointer(iter as *mut u8).bits())
            }
        }
        "entries" => {
            let iter =
                crate::array::js_array_entries_iter_obj(ta as *const crate::array::ArrayHeader);
            if iter == 0 {
                f64::from_bits(crate::value::TAG_UNDEFINED)
            } else {
                f64::from_bits(JSValue::pointer(iter as *mut u8).bits())
            }
        }
        "reduce" | "reduceRight" => {
            let cb = validate_cb(false);
            // initial value present only when a 2nd arg was passed.
            let (has_init, init) = if args_len >= 2 && !args_ptr.is_null() {
                (1, *args_ptr.add(1))
            } else {
                (0, f64::NAN)
            };
            if method_name == "reduce" {
                crate::typedarray::js_typed_array_reduce(ta, cb, has_init, init)
            } else {
                crate::typedarray::js_typed_array_reduce_right(ta, cb, has_init, init)
            }
        }
        // Non-callback search / view / join methods. These reach this tower
        // through the brand-checking `%TypedArray%.prototype` value-path thunks
        // (`typed_array_proto_thunks`); the receiver-typed fast path lowers them
        // via dedicated codegen. The array search helpers (`js_array_*_jsvalue`)
        // detect a registered TypedArray receiver and read its typed store, so a
        // `TypedArrayHeader*` cast to `ArrayHeader*` is sound here.
        "indexOf" | "lastIndexOf" | "includes" => {
            // Absent searchElement is `undefined`, NOT the NaN sentinel —
            // `new Float64Array([NaN]).includes()` must be false (SameValueZero
            // against undefined), and NaN never `===`-matches for indexOf.
            let value = if args_len >= 1 && !args_ptr.is_null() {
                *args_ptr
            } else {
                f64::from_bits(crate::value::TAG_UNDEFINED)
            };
            let (has_from, from) = if args_len >= 2 && !args_ptr.is_null() {
                (1, *args_ptr.add(1))
            } else {
                (0, f64::NAN)
            };
            let arr = ta as *const crate::array::ArrayHeader;
            match method_name {
                "indexOf" => {
                    crate::array::js_array_indexOf_jsvalue(arr, value, from, has_from) as f64
                }
                "lastIndexOf" => {
                    crate::array::js_array_last_index_of_jsvalue(arr, value, from, has_from) as f64
                }
                _ => f64::from_bits(
                    JSValue::bool(
                        crate::array::js_array_includes_jsvalue(arr, value, from, has_from) != 0,
                    )
                    .bits(),
                ),
            }
        }
        "join" => {
            let sep = arg0();
            let s = crate::typedarray::js_typed_array_join_value(ta, sep);
            f64::from_bits(JSValue::string_ptr(s).bits())
        }
        // `%TypedArray%.prototype.toLocaleString` (§23.2.3.32): for each
        // element, `? ToString(? Invoke(element, "toLocaleString"))`, joined by
        // ",". When the user has NOT replaced `Number.prototype.toLocaleString`
        // (or `BigInt.prototype...` for the bigint kinds) the result is the
        // default comma-separated join, which Perry's plain `join` matches —
        // keep that fast path. With a patch installed, run the spec loop so
        // the user function is invoked per element (its result then goes
        // through ordinary ToString, running `toString`/`valueOf` and
        // propagating abrupt completions).
        "toLocaleString" => {
            let kind = crate::typedarray::lookup_typed_array_kind(ta as usize);
            let is_bigint = matches!(
                kind,
                Some(crate::typedarray::KIND_BIGINT64) | Some(crate::typedarray::KIND_BIGUINT64)
            );
            let builtin: &[u8] = if is_bigint { b"BigInt" } else { b"Number" };
            match builtin_proto_user_method(builtin, "toLocaleString") {
                None => {
                    let s = crate::typedarray::js_typed_array_join_value(
                        ta,
                        f64::from_bits(crate::value::TAG_UNDEFINED),
                    );
                    f64::from_bits(JSValue::string_ptr(s).bits())
                }
                Some(patched) => {
                    let len = crate::typedarray::js_typed_array_length(ta);
                    let mut out = String::new();
                    for k in 0..len {
                        if k > 0 {
                            out.push(',');
                        }
                        let elem = crate::typedarray::js_typed_array_get(ta, k);
                        let r = call_primitive_closure_value(elem, patched, std::ptr::null(), 0)
                            .unwrap_or(f64::from_bits(crate::value::TAG_UNDEFINED));
                        let s_hdr = crate::builtins::js_string_coerce(r);
                        out.push_str(
                            super::has_own_helpers::str_from_string_header(s_hdr).unwrap_or(""),
                        );
                    }
                    let s = crate::string::js_string_from_bytes(out.as_ptr(), out.len() as u32);
                    f64::from_bits(JSValue::string_ptr(s).bits())
                }
            }
        }
        "slice" => {
            // `ToIntegerOrInfinity` each index (runs `valueOf`/`Symbol.toPrimitive`,
            // which may throw) — `js_typed_array_slice` then does the relative-index
            // clamp. `end` absent / `undefined` → slice to the end (`i32::MAX`).
            let to_idx = |v: f64| -> i32 {
                let n = crate::builtins::js_number_coerce(v);
                if n.is_nan() {
                    0
                } else if n >= i32::MAX as f64 {
                    i32::MAX
                } else if n <= i32::MIN as f64 {
                    i32::MIN
                } else {
                    n.trunc() as i32
                }
            };
            let start = if args_len >= 1 && !args_ptr.is_null() {
                to_idx(*args_ptr)
            } else {
                0
            };
            let end = if args_len >= 2
                && !args_ptr.is_null()
                && !JSValue::from_bits((*args_ptr.add(1)).to_bits()).is_undefined()
            {
                to_idx(*args_ptr.add(1))
            } else {
                i32::MAX
            };
            let result = crate::typedarray::js_typed_array_slice(ta, start, end);
            f64::from_bits(JSValue::pointer(result as *mut u8).bits())
        }
        "subarray" => {
            let (has_begin, begin) = if args_len >= 1 && !args_ptr.is_null() {
                (1, *args_ptr)
            } else {
                (0, f64::NAN)
            };
            let (has_end, end) = if args_len >= 2 && !args_ptr.is_null() {
                (1, *args_ptr.add(1))
            } else {
                (0, f64::NAN)
            };
            let result =
                crate::typedarray::js_typed_array_subarray(ta, has_begin, begin, has_end, end);
            f64::from_bits(JSValue::pointer(result as *mut u8).bits())
        }
        "reverse" => {
            let result = crate::typedarray::js_typed_array_reverse(ta);
            f64::from_bits(JSValue::pointer(result as *mut u8).bits())
        }
        "fill" => {
            let value = arg0();
            let (has_start, start) = if args_len >= 2 && !args_ptr.is_null() {
                (1, *args_ptr.add(1))
            } else {
                (0, f64::NAN)
            };
            let (has_end, end) = if args_len >= 3 && !args_ptr.is_null() {
                (1, *args_ptr.add(2))
            } else {
                (0, f64::NAN)
            };
            let result =
                crate::typedarray::js_typed_array_fill(ta, value, has_start, start, has_end, end);
            f64::from_bits(JSValue::pointer(result as *mut u8).bits())
        }
        _ => return None,
    };
    Some(r)
}

/// #3716: a built-in *prototype method* read off its prototype and called *as
/// a value* (rather than as `recv.method(...)`) routes through
/// `js_native_call_value`, which would invoke the shared no-op thunk
/// (`global_this_builtin_noop_thunk`) and return `undefined`. This is the final
/// link in the "uncurry-this" idiom `Function.prototype.call.bind(method)`: the
/// `Function.prototype.call` thunk stashes the intended receiver in
/// `IMPLICIT_THIS`, then calls the bound `method` value — which until now no-op'd.
///
/// When the invoked closure is a no-op-backed built-in proto method, recover its
/// recorded method name and re-dispatch through the real `js_native_call_method`
/// tower using the current `IMPLICIT_THIS` as the receiver. Returns `None` for
/// any other closure so normal dispatch proceeds untouched.
///
/// Gated on a recorded built-in `.length` so bare no-op-backed global
/// constructors (`const O = SomeCtor; O()`), which never call
/// `set_builtin_closure_length`, are excluded.
pub(crate) unsafe fn try_dispatch_value_called_proto_method(
    closure: *const crate::closure::ClosureHeader,
    args_ptr: *const f64,
    args_len: usize,
) -> Option<f64> {
    if closure.is_null() {
        return None;
    }
    if (*closure).func_ptr != super::global_this::global_this_builtin_noop_thunk as *const u8 {
        return None;
    }
    if super::native_module::builtin_closure_length(closure as usize).is_none() {
        return None;
    }
    let name_val = crate::closure::closure_get_dynamic_prop(closure as usize, "name");
    let name_jsv = JSValue::from_bits(name_val.to_bits());
    if !name_jsv.is_any_string() {
        return None;
    }
    // `js_string_coerce` normalizes SSO short strings (e.g. "bind", "join") to a
    // heap StringHeader so the byte read below is valid for inline-stored names.
    let name_hdr = crate::builtins::js_string_coerce(name_val);
    let name = super::has_own_helpers::str_from_string_header(name_hdr)?;
    let receiver = f64::from_bits(IMPLICIT_THIS.with(|c| c.get()));
    Some(js_native_call_method(
        receiver,
        name.as_ptr() as *const i8,
        name.len(),
        args_ptr,
        args_len,
    ))
}

/// #3662: classify a `Function.prototype.{apply,call,bind}` receiver. Returns
/// `true` when the receiver is *definitively not callable* — any primitive
/// (`undefined`/`null`/number/bool/string/bigint/symbol) or a recognized
/// ordinary heap object — so the spec brand check must throw a `TypeError`.
/// An *ambiguous* pointer (e.g. a native-callable value that isn't a real
/// closure) returns `false` so the caller keeps its prior conservative
/// behavior, mirroring the additive collection-thunk approach in #3662.
unsafe fn fn_proto_receiver_not_callable(object: f64) -> bool {
    let jsval = JSValue::from_bits(object.to_bits());
    if !jsval.is_pointer() {
        return true; // primitive — never callable
    }
    let raw = (object.to_bits() & 0x0000_FFFF_FFFF_FFFF) as usize;
    if crate::closure::is_closure_ptr(raw) {
        return false; // a real closure is callable
    }
    // A recognized ordinary object (plain object, array, Map, …) is not
    // callable. Unrecognized pointers stay ambiguous (return false).
    is_valid_obj_ptr(raw as *const u8)
}

/// #3662: throw the spec `TypeError` for a `Function.prototype.{apply,call,
/// bind}` invoked on a non-callable `this`. Test262's brand-check tests assert
/// only the error *type*; the wording mirrors V8/Node (`bind` has its own
/// distinct message). Never returns.
#[cold]
fn throw_fn_proto_not_callable(method: &str) -> ! {
    let message = if method == "bind" {
        "Bind must be called on a function".to_string()
    } else {
        format!("Function.prototype.{method} was called on a value that is not a function")
    };
    let msg = crate::string::js_string_from_bytes(message.as_ptr(), message.len() as u32);
    let err = crate::error::js_typeerror_new(msg);
    crate::exception::js_throw(crate::value::js_nanbox_pointer(err as i64))
}

/// Dispatch `receiver.<method>(args)` straight through the class vtable,
/// bypassing any own data property of the same name. Returns `None` when the
/// receiver is not a class instance whose prototype chain defines `method`, so
/// the caller falls back to the ordinary by-name lookup.
///
/// Used by bound-method VALUE dispatch (`dispatch_bound_method`): a method
/// captured at READ time (`const f = obj.m`) must keep invoking that method even
/// after `obj.m` is reassigned — the ubiquitous `this.m = this.m.bind(this)`
/// pattern. Re-resolving by name would find the own (bound) property and recurse
/// until the call-depth guard returns the null object.
pub(crate) unsafe fn try_dispatch_instance_method_value(
    receiver: f64,
    method_name_ptr: *const i8,
    method_name_len: usize,
    args_ptr: *const f64,
    args_len: usize,
) -> Option<f64> {
    if method_name_ptr.is_null() || method_name_len == 0 {
        return None;
    }
    let jsval = JSValue::from_bits(receiver.to_bits());
    if !jsval.is_pointer() {
        return None;
    }
    let raw = crate::value::js_nanbox_get_pointer(receiver) as usize;
    if crate::value::addr_class::is_handle_band(raw) {
        return None;
    }
    let ptr = raw as *const ObjectHeader;
    // `js_object_get_class_id` returns 0 for anything that isn't a user class
    // instance (null/non-pointer, Set/Map/Regex headers, closures, namespaces).
    let class_id = crate::object::js_object_get_class_id(ptr);
    if class_id == 0 {
        return None;
    }
    let name = std::str::from_utf8(std::slice::from_raw_parts(
        method_name_ptr as *const u8,
        method_name_len,
    ))
    .ok()?;
    let (func_ptr, param_count, has_synthetic_arguments, has_rest) =
        crate::object::class_registry::lookup_class_method_in_chain(class_id, name)?;
    Some(crate::object::class_registry::call_vtable_method(
        func_ptr,
        receiver.to_bits() as i64,
        args_ptr,
        args_len,
        param_count,
        has_synthetic_arguments,
        has_rest,
    ))
}

#[no_mangle]
pub unsafe extern "C" fn js_native_call_method(
    object: f64,
    method_name_ptr: *const i8,
    method_name_len: usize,
    args_ptr: *const f64,
    args_len: usize,
) -> f64 {
    // Get the method name (parsed early for depth guard logging)
    let method_name_owned = if method_name_ptr.is_null() || method_name_len == 0 {
        String::new()
    } else {
        let bytes = std::slice::from_raw_parts(method_name_ptr as *const u8, method_name_len);
        String::from_utf8_lossy(bytes).into_owned()
    };
    let method_name = method_name_owned.as_str();
    let root_scope = crate::gc::RuntimeHandleScope::new();
    let object_handle = root_scope.root_nanbox_f64(object);
    let original_args: Vec<f64> = if args_len > 0 && !args_ptr.is_null() {
        std::slice::from_raw_parts(args_ptr, args_len).to_vec()
    } else {
        Vec::new()
    };
    let arg_handles = root_scope.root_nanbox_f64_slice(&original_args);
    let refreshed_args = || crate::gc::RuntimeHandleScope::refreshed_nanbox_f64_slice(&arg_handles);
    let object = object_handle.get_nanbox_f64();
    let jsval = JSValue::from_bits(object.to_bits());
    // RAII recursion depth guard: prevent stack overflow from circular module deps.
    // The guard auto-decrements on drop, covering all ~20 return points in this function.
    // When max depth is hit, return a pointer to a static empty object instead of undefined.
    // This prevents crashes when callers NaN-unbox the result and dereference it as a pointer.
    let _depth_guard = match CallMethodDepthGuard::enter(method_name) {
        Some(g) => g,
        None => {
            crate::object::class_registry::report_dispatch_miss(
                "call-method (recursion-depth guard)",
                object,
                method_name,
                "empty object",
            );
            let null_obj_ptr = &NULL_OBJECT_BYTES as *const NullObjectBytes as *mut u8;
            return f64::from_bits(JSValue::pointer(null_obj_ptr).bits());
        }
    };

    // #4795: `using` / `await using` desugars disposal to
    // `obj.__perry_dispose__()` / `obj.__perry_async_dispose__()`. Class
    // instances resolve these through the renamed vtable method (handled by
    // the generic dispatch below) and native handles (timers, sqlite) special-
    // case the names. But objects that store `[Symbol.dispose]` /
    // `[Symbol.asyncDispose]` under the well-known-symbol key — object literals
    // and dynamically-assigned disposers — won't match the string method name
    // and would fall through to "is not a function". Resolve the symbol-keyed
    // disposer here, with the spec async→sync fallback, before that happens.
    if matches!(method_name, "__perry_dispose__" | "__perry_async_dispose__") {
        if let Some(result) = try_symbol_dispose_dispatch(object, method_name, args_ptr, args_len) {
            return result;
        }
    }
    // #4795: `using x = e` emits `x.__perry_using_check__(isAsync)` at the
    // declaration point so a non-disposable resource throws `TypeError` there
    // (spec `CreateDisposableResource` / `GetDisposeMethod`), before the block
    // body runs — not later at disposal time.
    if method_name == "__perry_using_check__" {
        let want_async =
            args_len > 0 && !args_ptr.is_null() && { crate::value::js_is_truthy(*args_ptr) != 0 };
        return js_using_check_disposable(object, want_async);
    }
    // Generic `Array.prototype` mutators borrowed onto a plain array-like
    // object (`Array.prototype.splice.call(obj, …)` whose synthesized member
    // call dispatches by name with no own method). The dense array arms further
    // down cast any pointer receiver to `ArrayHeader`, corrupting a real
    // object's layout. Route a plain-object receiver to the spec-generic engine.
    // Returns `None` for real arrays / typed arrays / buffers / primitives, and
    // for objects that own a user method of this name — the hot paths and user
    // methods are untouched. (The `obj.pop = Array.prototype.pop` borrow shape
    // is handled by the real prototype-method thunks instead.)
    if matches!(
        method_name,
        "pop" | "shift" | "push" | "unshift" | "reverse" | "splice" | "sort" | "concat"
    ) {
        if let Some(result) =
            crate::array::try_object_arraylike_mutator(object, method_name, args_ptr, args_len)
        {
            return result;
        }
    }
    // A plain object whose [[Prototype]] chain contains a real array
    // (`function foo() {}; foo.prototype = new Array(1, 2, 3); new foo()`)
    // inherits the `Array.prototype` methods through that array, but the
    // field-scan dispatch below finds no own/proto slot for them and threw
    // "<m> is not a function" (test262 filter/15.4.4.20-6-*,
    // some/15.4.4.17-8-*, map/15.4.4.19-9-3). Route the generic array-like
    // engine; receivers with an own user method or no array on the chain
    // fall through unchanged.
    if matches!(
        method_name,
        "forEach"
            | "map"
            | "filter"
            | "some"
            | "every"
            | "find"
            | "findIndex"
            | "findLast"
            | "findLastIndex"
            | "reduce"
            | "reduceRight"
            | "indexOf"
            | "lastIndexOf"
            | "includes"
            | "at"
            | "join"
            | "slice"
            | "sort"
            | "concat"
    ) {
        if let Some(result) =
            crate::array::try_array_proto_chain_method(object, method_name, args_ptr, args_len)
        {
            return result;
        }
    }
    // #4795: dynamic dispatch for `DisposableStack` / `AsyncDisposableStack`
    // instance methods. The codegen fast path handles statically-typed stack
    // locals, but a stack held in an `any`-typed value — e.g. the result of
    // `stack.move()` — reaches the generic dispatcher, where the class id has
    // no user vtable and would otherwise surface "dispose is not a function".
    // Gated on the method name first so unrelated dynamic calls don't pay the
    // `object_ptr_from_value` class-id probe.
    if matches!(
        method_name,
        "use"
            | "adopt"
            | "defer"
            | "move"
            | "dispose"
            | "disposeAsync"
            | "@@__perry_wk_dispose"
            | "@@__perry_wk_asyncDispose"
    ) {
        if let Some(result) =
            try_disposable_stack_method_dispatch(object, method_name, args_ptr, args_len)
        {
            return result;
        }
    }

    {
        let raw_addr = if jsval.is_pointer() {
            crate::value::js_nanbox_get_pointer(object) as usize
        } else if (object.to_bits() >> 48) == 0 {
            object.to_bits() as usize
        } else {
            0
        };
        // Fetch, stream, and other runtime objects use small tagged handles that
        // are pointer-shaped but not heap allocations. Avoid asking the closure
        // probe to dereference those handles as addresses.
        if crate::value::addr_class::is_above_handle_band(raw_addr)
            && crate::closure::is_closure_ptr(raw_addr)
            && !crate::closure::closure_is_key_deleted(raw_addr, method_name)
            // apply/call/bind/toString on a closure receiver have dedicated
            // spec-accurate arms below; the dynamic-prop read would resolve
            // them through the Function.prototype expando fallback to the
            // GENERIC thunks, which lose arguments-object argArrays
            // (`G.apply(this, arguments)`).
            && !matches!(method_name, "apply" | "call" | "bind" | "toString")
        {
            let dyn_val = crate::closure::closure_get_dynamic_prop(raw_addr, method_name);
            if dyn_val.to_bits() != crate::value::TAG_UNDEFINED {
                let prev_this = IMPLICIT_THIS.with(|c| c.replace(object.to_bits()));
                let result = crate::closure::js_native_call_value(dyn_val, args_ptr, args_len);
                IMPLICIT_THIS.with(|c| c.set(prev_this));
                return result;
            }
            // `fn.length()` / `fn.name()` — the own slots hold a number /
            // string, never a callable; calling one is a TypeError
            // (`f.length is not a function`), not a read.
            if matches!(method_name, "length" | "name") {
                crate::error::js_throw_type_error_not_a_function(
                    std::ptr::null(),
                    0,
                    method_name.as_ptr(),
                    method_name.len(),
                );
            }
        }
    }

    // A method stored as an own accessor — `{ get next() { return fn } }` or
    // `Object.defineProperty(o, "next", { get })` — must invoke the getter
    // (this = receiver) to obtain the method function, then call THAT. The big
    // dispatch below reads the raw field slot, which holds no callable for an
    // accessor-only property, so a fused `o.next(args)` mis-resolved to
    // undefined (decomposed `const f = o.next; f(args)` worked because the read
    // goes through the getter-aware property path). Hit by `yield*` over a
    // sync/async iterator whose `next`/`value`/`done` are getters (test262
    // yield-star-* with `get next()`). `get_accessor_descriptor` is a cheap
    // keyed HashMap lookup (no deref), gated on the accessor hot-path flag so
    // non-accessor programs skip it entirely.
    if jsval.is_pointer() && crate::object::ACCESSORS_IN_USE.with(|c| c.get()) {
        let obj_usize = crate::value::js_nanbox_get_pointer(object) as usize;
        if crate::value::addr_class::is_above_handle_band(obj_usize) {
            if let Some(acc) = crate::object::get_accessor_descriptor(obj_usize, method_name) {
                if acc.get != 0 {
                    let getter = (acc.get & crate::value::POINTER_MASK)
                        as *const crate::closure::ClosureHeader;
                    if !getter.is_null() {
                        let prev_getter_this = IMPLICIT_THIS.with(|c| c.replace(object.to_bits()));
                        let method_fn = crate::closure::js_closure_call0(getter);
                        let bound =
                            crate::closure::clone_closure_rebind_this(method_fn.to_bits(), object);
                        IMPLICIT_THIS.with(|c| c.set(object.to_bits()));
                        let result = crate::closure::js_native_call_value(
                            f64::from_bits(bound),
                            args_ptr,
                            args_len,
                        );
                        IMPLICIT_THIS.with(|c| c.set(prev_getter_this));
                        return result;
                    }
                }
            }
        }
    }

    // Check if this is a JS handle (V8 object from JS runtime)
    if crate::value::is_js_handle(object) {
        let func_ptr =
            crate::value::JS_HANDLE_CALL_METHOD.load(std::sync::atomic::Ordering::SeqCst);
        if !func_ptr.is_null() {
            let func: unsafe extern "C" fn(f64, *const i8, usize, *const f64, usize) -> f64 =
                std::mem::transmute(func_ptr);
            let result = func(object, method_name_ptr, method_name_len, args_ptr, args_len);
            return result;
        }
        return f64::from_bits(0x7FF8_0000_0000_0001); // undefined
    }

    // #4661 follow-up: a *fused* method call `proxy.method(args)` on a Proxy
    // receiver. The decomposed form `const f = proxy.method; f(args)` already
    // works because the property read routes through `js_proxy_get`. The fused
    // form, however, reaches this generic dispatcher with the proxy id intact.
    // Proxy ids encode to small pointer-tagged values (band 0xF0000..0x100000),
    // so without this guard the receiver is misclassified as a native-module
    // *integer handle* by the `raw_ptr < 0x100000` small-handle dispatch below
    // (when an app links a native handle dispatcher, e.g. mysql2 / Fastify),
    // which returns null for an unknown id — silently dropping the call.
    //
    // Mirror the spec: `Get(proxy, "method")` (honors the get trap / forwards
    // through the target's prototype chain) then `Call(method, proxy, args)`
    // with `this` bound to the proxy itself.
    if crate::proxy::js_proxy_is_proxy(object) == 1 {
        let key = crate::string::js_string_from_bytes(
            method_name_ptr as *const u8,
            method_name_len as u32,
        );
        let key_box = f64::from_bits(JSValue::string_ptr(key).bits());
        let key_handle = root_scope.root_nanbox_f64(key_box);
        let method_value =
            crate::proxy::js_proxy_get(object_handle.get_nanbox_f64(), key_handle.get_nanbox_f64());
        let method_handle = root_scope.root_nanbox_f64(method_value);
        let args = refreshed_args();
        // Bind `this` to the proxy for the duration of the call, matching the
        // receiver semantics of a normal `obj.method(args)` invocation.
        let prev_this = IMPLICIT_THIS.with(|c| c.replace(object_handle.get_nanbox_f64().to_bits()));
        let result = crate::closure::js_native_call_value(
            method_handle.get_nanbox_f64(),
            args.as_ptr(),
            args.len(),
        );
        IMPLICIT_THIS.with(|c| c.set(prev_this));
        return result;
    }

    // Temporal cell (#4686): `duration.add(x)`, `instant.toString()`, etc. A
    // `Temporal.*` value is a NaN-boxed pointer to a custom cell with no
    // codegen fast-path, so every method call funnels through here. The router
    // throws `TypeError` for an unknown method name on a real Temporal receiver.
    if crate::temporal::is_temporal_value(object) {
        let args = refreshed_args();
        return crate::temporal::dispatch::call_method(object, method_name, &args);
    }

    if (object.to_bits() >> 48) == 0x7FFE {
        let class_id = (object.to_bits() & 0xFFFF_FFFF) as u32;
        if crate::object::class_prototype_ref_id(object).is_some() {
            if let Some((func_ptr, param_count, has_synthetic_arguments, has_rest)) =
                crate::object::class_registry::lookup_class_method_in_chain(class_id, method_name)
            {
                return crate::object::class_registry::call_vtable_method(
                    func_ptr,
                    object.to_bits() as i64,
                    args_ptr,
                    args_len,
                    param_count,
                    has_synthetic_arguments,
                    has_rest,
                );
            }
        } else if class_id != 0
            && crate::object::class_registry::lookup_static_method_in_chain(class_id, method_name)
                .is_some()
        {
            let args = refreshed_args();
            return crate::object::class_registry::js_class_static_method_call(
                object_handle.get_nanbox_f64(),
                method_name_ptr as *const u8,
                method_name_len,
                args.as_ptr(),
                args.len(),
            );
        }
    }

    if method_name == "toString" && jsval.is_pointer() {
        // #4101: `fn.toString()` — reconstruct the function's source from the
        // codegen-registered text (or a synthesized native form), rather than
        // falling through to the generic `"[object Object]"`.
        let raw_addr = crate::value::js_nanbox_get_pointer(object) as usize;
        if crate::value::addr_class::is_above_handle_band(raw_addr)
            && crate::closure::is_closure_ptr(raw_addr)
        {
            if let Some(result) = crate::value::function_to_string_method_result(object) {
                return result;
            }
            let func_ptr = (*(raw_addr as *const crate::closure::ClosureHeader)).func_ptr as usize;
            let s = crate::builtins::function_source_for_func_ptr(func_ptr);
            let str_ptr = crate::string::js_string_from_bytes(s.as_ptr(), s.len() as u32);
            return f64::from_bits(JSValue::string_ptr(str_ptr).bits());
        }
        let raw = crate::value::js_nanbox_get_pointer(object) as *const u8;
        if !raw.is_null() && crate::object::is_valid_obj_ptr(raw) {
            unsafe {
                let gc = raw.sub(crate::gc::GC_HEADER_SIZE) as *const crate::gc::GcHeader;
                if (*gc).obj_type == crate::gc::GC_TYPE_ERROR {
                    let s = crate::error::js_error_to_string(raw as *mut crate::error::ErrorHeader);
                    return f64::from_bits(JSValue::string_ptr(s).bits());
                }
            }
        }
    }

    // Primitive-wrapper prototypes (`Number.prototype`, `Boolean.prototype`,
    // `BigInt.prototype`) carry a brand default value (+0 / false / 0n) for
    // valueOf/toString, matching V8. They are ordinary objects with no
    // [[*Data]] slot, so `boxed_primitive_payload` below misses them; without
    // this a fused `Number.prototype.valueOf()` returned the prototype object
    // itself (test262 Number/prototype/valueOf/S15.7.4.4_*).
    if jsval.is_pointer() && matches!(method_name, "valueOf" | "toString" | "toLocaleString") {
        use crate::object::builtin_prototype_value;
        let ob = object.to_bits();
        if ob == builtin_prototype_value("Number").to_bits() {
            match method_name {
                "valueOf" => return 0.0,
                _ => {
                    let radix = if args_len >= 1 && !args_ptr.is_null() {
                        *args_ptr
                    } else {
                        f64::from_bits(crate::value::TAG_UNDEFINED)
                    };
                    let s = crate::value::js_jsvalue_to_string_radix(0.0, radix);
                    return f64::from_bits(JSValue::string_ptr(s).bits());
                }
            }
        }
        if ob == builtin_prototype_value("Boolean").to_bits() {
            match method_name {
                "valueOf" => return f64::from_bits(crate::value::TAG_FALSE),
                _ => {
                    let s = crate::string::js_string_from_bytes(b"false".as_ptr(), 5);
                    return f64::from_bits(JSValue::string_ptr(s).bits());
                }
            }
        }
    }

    if let Some((_, payload)) = crate::builtins::boxed_primitive_payload(object) {
        match method_name {
            "valueOf" => return payload,
            "toString" | "toLocaleString" => {
                let payload_jsv = JSValue::from_bits(payload.to_bits());
                match crate::builtins::boxed_primitive_to_string_tag(object) {
                    Some("String") => return payload,
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
                        return f64::from_bits(JSValue::string_ptr(str_ptr).bits());
                    }
                    Some("Boolean") => {
                        let s = if payload_jsv.is_bool() && payload_jsv.as_bool() {
                            b"true".as_slice()
                        } else {
                            b"false".as_slice()
                        };
                        let str_ptr =
                            crate::string::js_string_from_bytes(s.as_ptr(), s.len() as u32);
                        return f64::from_bits(JSValue::string_ptr(str_ptr).bits());
                    }
                    Some("BigInt") => {
                        let big = crate::value::JSValue::from_bits(payload.to_bits());
                        if big.is_bigint() {
                            let ptr = crate::bigint::clean_bigint_ptr(
                                (payload.to_bits() & 0x0000_FFFF_FFFF_FFFF)
                                    as *const crate::bigint::BigIntHeader,
                            );
                            let str_ptr = crate::bigint::js_bigint_to_string(ptr);
                            return f64::from_bits(JSValue::string_ptr(str_ptr).bits());
                        }
                    }
                    Some("Symbol") => {
                        let str_ptr =
                            crate::symbol::js_symbol_to_string(payload) as *mut crate::StringHeader;
                        return f64::from_bits(JSValue::string_ptr(str_ptr).bits());
                    }
                    _ => {}
                }
            }
            _ => {}
        }
    }

    if crate::web_storage::is_storage_value(object_handle.get_nanbox_f64()) {
        let args = refreshed_args();
        if let Some(result) = crate::web_storage::dispatch_storage_method(
            object_handle.get_nanbox_f64(),
            method_name,
            &args,
        ) {
            return result;
        }
    }

    // #1758 / epic #1785: a class-object VALUE reaching the *dynamic*
    // dispatcher is a STATIC method call. This happens when the static
    // analyzer couldn't prove the receiver is a class object — e.g.
    // `class X extends (make(...) as any).annotations(y) {}` where the
    // `make()` factory call wasn't inlined to a `ClassExprFresh` (so the
    // `.annotations` receiver lowers to a generic Call result), or any
    // `(expr-returning-a-class-object).staticMethod()`. The compile-time
    // static-dispatch tower (property_get.rs) binds `this` via
    // IMPLICIT_THIS; the generic field-scan path below does NOT, so
    // `this.<staticField>` (effect's `annotations() { make(this.ast, ...) }`)
    // read `undefined`. Route to `js_class_static_method_call`, which binds
    // `this` to the receiver and walks the class_id parent chain — but only
    // when the method actually resolves in the static chain, so an own
    // function-valued static field still falls through to the generic path.
    if crate::object::class_registry::is_class_object_value(object) {
        let class_id = crate::object::js_object_get_class_id(jsval.as_pointer::<ObjectHeader>());
        if class_id != 0
            && crate::object::class_registry::lookup_static_method_in_chain(class_id, method_name)
                .is_some()
        {
            let args = refreshed_args();
            return crate::object::class_registry::js_class_static_method_call(
                object_handle.get_nanbox_f64(),
                method_name_ptr as *const u8,
                method_name_len,
                args.as_ptr(),
                args.len(),
            );
        }
    }

    // Issue #489 followup: Promise's `then` / `catch` / `finally` are
    // intrinsic — when the dynamic dispatch path lands a `.then(cb)` on
    // a Promise (drizzle's `mysql-proxy/session.js`:
    // `this.client(...).then(({rows}) => rows)` where the static
    // analyzer couldn't prove the receiver is a Promise), route directly
    // to `js_promise_then` / `js_promise_catch` / `js_promise_finally`.
    // Without this, the field-scan + class-id walks below find nothing
    // and return undefined — drizzle's `MySqlRemoteSession.all` then
    // resolves to undefined and downstream `data[0].insertId` accesses
    // silently fail.
    if matches!(method_name, "then" | "catch" | "finally")
        && crate::promise::js_value_is_promise(object_handle.get_nanbox_f64()) != 0
    {
        let promise_ptr = (object_handle.get_nanbox_f64().to_bits() & 0x0000_FFFF_FFFF_FFFF)
            as *mut crate::Promise;
        let promise_handle = root_scope.root_raw_mut_ptr(promise_ptr);
        let args = refreshed_args();
        let arg0_box = if !args.is_empty() {
            args[0]
        } else {
            f64::from_bits(crate::value::TAG_UNDEFINED)
        };
        let arg1_box = if args.len() >= 2 {
            args[1]
        } else {
            f64::from_bits(crate::value::TAG_UNDEFINED)
        };
        // Closures arrive here in two shapes:
        //  - NaN-boxed `POINTER_TAG | (closure_ptr & 0x0000_FFFF_FFFF_FFFF)`
        //    (the codegen `js_closure_alloc_singleton` + OR-with-tag form)
        //  - Raw `*ClosureHeader` bit-cast to f64 — the convention used
        //    by `js_assimilate_thenable` when it propagates
        //    `then(resolve, reject)` callbacks through a user-defined
        //    `then` method's param slots (see `promise.rs:2438-2442`).
        // Accept both. TAG_UNDEFINED / null / non-pointer values stay
        // null so `js_promise_then` treats the handler as missing.
        let extract_closure = |v: f64| -> crate::promise::ClosurePtr {
            let b = v.to_bits();
            let candidate = if (b & 0xFFFF_0000_0000_0000) == 0x7FFD_0000_0000_0000 {
                b & 0x0000_FFFF_FFFF_FFFF
            } else if (b & 0xFFFF_0000_0000_0000) == 0 {
                b
            } else {
                0
            };
            if candidate < 0x10000 {
                std::ptr::null()
            } else {
                candidate as crate::promise::ClosurePtr
            }
        };
        let result = match method_name {
            "then" => crate::promise::js_promise_then(
                promise_handle.get_raw_mut_ptr(),
                extract_closure(arg0_box),
                extract_closure(arg1_box),
            ),
            "catch" => crate::promise::js_promise_catch(
                promise_handle.get_raw_mut_ptr(),
                extract_closure(arg0_box),
            ),
            "finally" => crate::promise::js_promise_finally(
                promise_handle.get_raw_mut_ptr(),
                extract_closure(arg0_box),
            ),
            _ => unreachable!(),
        };
        return f64::from_bits(JSValue::pointer(result as *mut u8).bits());
    }

    // `regex.test(str)` / `regex.exec(str)` on an *untyped* receiver — e.g.
    // hono's RegExpRouter does `buildWildcardRegExp(k).test(path)`, a call on a
    // function result the codegen `Expr::RegExpTest` fast path can't see; without
    // this it throws `test is not a function`, breaking Hono `app.use('*', …)`
    // (#1731). The helper returns None for non-regex so generic dispatch resumes.
    if matches!(method_name, "test" | "exec" | "toString") && jsval.is_pointer() {
        let undef = f64::from_bits(crate::value::TAG_UNDEFINED);
        let arg0 = refreshed_args().first().copied().unwrap_or(undef);
        let p = jsval.as_pointer::<u8>();
        if let Some(r) = crate::regex::dispatch_regex_receiver_method(p, method_name, arg0) {
            return r;
        }
    }

    // `RegExp.prototype.compile(pattern, flags)` (Annex B) re-initializes the
    // receiver in place. Needs both args, so it is dispatched here rather than
    // through the single-arg `dispatch_regex_receiver_method`.
    if method_name == "compile" && jsval.is_pointer() {
        let p = jsval.as_pointer::<u8>();
        if crate::regex::is_regex_pointer(p) {
            let undef = f64::from_bits(crate::value::TAG_UNDEFINED);
            let args = refreshed_args();
            let pat = args.first().copied().unwrap_or(undef);
            let flags = args.get(1).copied().unwrap_or(undef);
            return crate::regex::js_regexp_compile_value(
                p as *mut crate::regex::RegExpHeader,
                pat,
                flags,
            );
        }
    }

    // Node timer handles are represented in Perry as small integer ids
    // NaN-boxed as pointers. Provide the common Timeout/Immediate methods
    // directly so `timeout.ref().unref().hasRef()` style probes behave like
    // Node without having to allocate a full JS wrapper object per timer.
    //
    // Gated on (a) tag == POINTER_TAG (0x7FFD) to avoid catching strings /
    // int32 / nullish tags, and (b) the id being a known timer so unrelated
    // small handles (UI widgets, drizzle, native instances) fall through
    // to the normal dispatch.
    {
        let bits = object.to_bits();
        let top16 = bits >> 48;
        if top16 == 0x7FFD {
            let id = (bits & 0x0000_FFFF_FFFF_FFFF) as i64;
            // Timer ids and `perry-ffi` registry handles share the pointer-tagged
            // small-integer band and both count from 1, so a bare id can be
            // ambiguous (e.g. an HTTP/2 server handle 1 vs a `setTimeout` id 1
            // alive at the same time). A live registered handle is the
            // authoritative interpretation — it owns a real Rust object and its
            // method surface (`close`/`ref`/`unref`/…) — so yield to the handle
            // dispatch below rather than swallow `server.close()` as
            // `clearTimeout`. A genuine timer whose id does not also name a live
            // handle still resolves here.
            if crate::timer::is_known_timer_id(id) && !super::class_handles::ffi_handle_exists(id) {
                match method_name {
                    "ref" => {
                        crate::timer::js_timer_ref(id);
                        return object;
                    }
                    "unref" => {
                        crate::timer::js_timer_unref(id);
                        return object;
                    }
                    "hasRef" => {
                        return if crate::timer::js_timer_has_ref(id) != 0 {
                            f64::from_bits(JSValue::bool(true).bits())
                        } else {
                            f64::from_bits(JSValue::bool(false).bits())
                        };
                    }
                    "refresh" => {
                        crate::timer::js_timer_refresh(id);
                        return object;
                    }
                    "close" => {
                        crate::timer::clearTimeout(id);
                        crate::timer::clearInterval(id);
                        crate::timer::clearImmediate(id);
                        return object;
                    }
                    // `__perry_dispose__` is the class-member form; the
                    // well-known `Symbol.dispose` computed form lowers to
                    // `@@__perry_wk_dispose`. Both clear the timer (#1213).
                    "__perry_dispose__" | "@@__perry_wk_dispose" => {
                        crate::timer::clearTimeout(id);
                        crate::timer::clearInterval(id);
                        crate::timer::clearImmediate(id);
                        return f64::from_bits(JSValue::undefined().bits());
                    }
                    "@@__perry_wk_toPrimitive" | "valueOf" => return id as f64,
                    _ => {}
                }
            }
        }
    }

    // A `DateCell` is a NaN-boxed pointer but NOT an `ObjectHeader`, so a date
    // receiver must never reach the generic object dispatch below — that path
    // reinterprets the cell's bytes as an object and returns garbage. Every
    // `Date.prototype` method (getters, setters, `toISOString`, `toJSON`,
    // `toString`, …) is installed on `Date.prototype` and reads the
    // `IMPLICIT_THIS` receiver, so resolve the method there and dispatch with
    // `this` bound to the cell. Previously only `toString` was routed this way;
    // every other dynamic/computed call (`date[m](...)`, `Reflect.apply`) fell
    // through and silently dropped setter mutations — e.g. dayjs's
    // `this.$d[l]($)` made `.add()`/`.date(n)` no-ops (#5133).
    if crate::date::is_date_value(object) {
        let ctor = crate::object::js_get_global_this_builtin_value(b"Date".as_ptr(), 4);
        let ctor_ptr = crate::value::js_nanbox_get_pointer(ctor) as usize;
        if ctor_ptr != 0 {
            let proto = crate::closure::closure_get_dynamic_prop(ctor_ptr, "prototype");
            if let Some(proto_ptr) = object_ptr_from_value(proto) {
                let key = crate::string::js_string_from_bytes(
                    method_name_ptr as *const u8,
                    method_name_len as u32,
                );
                let value = crate::object::js_object_get_field_by_name(proto_ptr, key);
                if !value.is_undefined() {
                    let value_f64 = f64::from_bits(value.bits());
                    let prev_this = IMPLICIT_THIS.with(|c| c.replace(object.to_bits()));
                    let result =
                        crate::closure::js_native_call_value(value_f64, args_ptr, args_len);
                    IMPLICIT_THIS.with(|c| c.set(prev_this));
                    return result;
                }
            }
        }
        if method_name == "toString" {
            let string = crate::date::js_date_to_string(object);
            return f64::from_bits(JSValue::string_ptr(string).bits());
        }
    }

    // Symbols: Symbol.for() pointers are Box-leaked (no GcHeader), so the
    // ObjectHeader path below would dereference garbage. Detect symbols
    // up front via the side-table.
    if jsval.is_pointer() {
        let raw_ptr = (object.to_bits() & 0x0000_FFFF_FFFF_FFFF) as usize;
        if crate::symbol::is_registered_symbol(raw_ptr) {
            let sym_f64 = object;
            return match method_name {
                "toString" => {
                    let s = crate::symbol::js_symbol_to_string(sym_f64);
                    f64::from_bits(JSValue::string_ptr(s as *mut crate::StringHeader).bits())
                }
                "valueOf" => sym_f64,
                "description" => {
                    f64::from_bits(crate::symbol::js_symbol_description(sym_f64).to_bits())
                }
                _ => f64::from_bits(crate::value::TAG_UNDEFINED),
            };
        }
    }

    // Handle BigInt method calls (NaN-boxed with BIGINT_TAG 0x7FFA)
    if jsval.is_bigint() {
        let bigint_ptr = crate::bigint::clean_bigint_ptr(
            (object.to_bits() & 0x0000_FFFF_FFFF_FFFF) as *const crate::bigint::BigIntHeader,
        );
        match method_name {
            "isZero" => {
                let result = crate::bigint::js_bigint_is_zero(bigint_ptr);
                return f64::from_bits(JSValue::bool(result != 0).bits());
            }
            "isNeg" | "isNegative" => {
                let result = crate::bigint::js_bigint_is_negative(bigint_ptr);
                return f64::from_bits(JSValue::bool(result != 0).bits());
            }
            "toNumber" => {
                return crate::bigint::js_bigint_to_f64(bigint_ptr);
            }
            "toString" => {
                // #2864: ToNumber/ToInteger-coerce + validate the radix
                // (RangeError for out-of-range), `None`/no-arg → decimal.
                let radix = if args_len > 0 && !args_ptr.is_null() {
                    crate::value::coerce_validate_radix(*args_ptr)
                } else {
                    None
                };
                let result_ptr = match radix {
                    Some(r) => crate::bigint::js_bigint_to_string_radix(bigint_ptr, r),
                    None => crate::bigint::js_bigint_to_string(bigint_ptr),
                };
                return f64::from_bits(JSValue::string_ptr(result_ptr).bits());
            }
            "add" | "sub" | "mul" | "div" | "mod" | "umod" | "pow" | "and" | "or" | "xor"
            | "shln" | "shrn" | "maskn" | "eq" | "lt" | "lte" | "gt" | "gte" | "cmp"
            | "fromTwos" | "toTwos" => {
                let args = refreshed_args();
                return dispatch_bigint_binary_method(
                    bigint_ptr,
                    method_name,
                    args.as_ptr(),
                    args.len(),
                );
            }
            _ => {
                // Unknown BigInt method - fall through to general dispatch
            }
        }
    }

    // Check for raw handle integer: Perry may bit-cast an i64 handle directly to f64,
    // producing a subnormal float (bits == handle_id, no NaN-box tag). Untagged values
    // in the handle band are raw handle IDs from Perry's integer-typed handle parameters.
    let raw_bits = object.to_bits();
    if crate::value::addr_class::is_small_handle(raw_bits as usize) {
        if let Some(dispatch) = handle_method_dispatch() {
            let args = refreshed_args();
            return dispatch(
                raw_bits as i64,
                method_name.as_ptr(),
                method_name.len(),
                args.as_ptr(),
                args.len(),
            );
        }
        return f64::from_bits(0x7FF8_0000_0000_0001); // undefined
    }

    // #1545: Web Streams handles are returned as `id as f64` (a normal float),
    // so their `to_bits()` is large and the raw-handle check above misses them.
    // When the receiver is a finite whole number and the stdlib probe confirms
    // it's a live stream handle, route the call through the same handle
    // dispatcher (which carries the stream method arms). Gating on the probe
    // means a genuine numeric receiver calling an unknown method still falls
    // through to the `(number).x is not a function` TypeError below.
    if object.is_finite() && object > 0.0 && object.fract() == 0.0 {
        let id = object as usize;
        if let Some(probe) = stream_handle_probe() {
            if probe(id) {
                if let Some(dispatch) = handle_method_dispatch() {
                    let args = refreshed_args();
                    return dispatch(
                        id as i64,
                        method_name.as_ptr(),
                        method_name.len(),
                        args.as_ptr(),
                        args.len(),
                    );
                }
            }
        }
    }

    // Issue #654: typed-array method dispatch. The codegen for
    // `new Float64Array(...)` (and the other typed-array constructors)
    // returns the raw heap pointer bitcast to f64 — no POINTER_TAG —
    // so neither `is_pointer()` nor the handle dispatch above catches
    // it. Detect via the `TYPED_ARRAY_REGISTRY` side table and route
    // common methods (`sort`, `at`, `toSorted`, `toReversed`, `with`,
    // `findLast`, `findLastIndex`) to their `js_typed_array_*` runtime
    // helpers. Without this arm `(a: Float64Array).sort()` reached the
    // `(number).sort is not a function` catch-all because raw pointer
    // bits classify as `is_number()` (top16 outside the tagged range).
    {
        let top16 = raw_bits >> 48;
        if top16 == 0 && raw_bits >= 0x10000 {
            let addr = raw_bits as usize;
            if crate::typedarray::lookup_typed_array_kind(addr).is_some() {
                let ta = addr as *mut crate::typedarray::TypedArrayHeader;
                if let Some(r) = dispatch_typed_array_method(ta, method_name, args_ptr, args_len) {
                    return r;
                }
            }
        }
    }

    // Issue #514 followup: string method dispatch on any-typed receivers.
    // When `(s: any).at(-1)` / `.slice(1)` / etc. lower through the
    // dispatch tower and `s` actually holds a string, we need to route
    // to the matching `js_string_*` runtime helper. Without this, the
    // primitive-method TypeError catch-all (issue #510 fix below) fires
    // for every legitimate string method call on a `(s: any)` parameter,
    // breaking hono's `mergePath` template-literal logic that mixes
    // `s?.[0]` (handled by `js_dyn_index_get`, issue #514) with
    // `s?.at(-1)` and `s?.slice(1)`. Static call sites for typed string
    // receivers continue to use the inline `js_string_*` paths in
    // `lower_string_method.rs`; this dispatch only catches fallthroughs
    // where codegen couldn't statically prove the type.
    if jsval.is_string() || jsval.is_short_string() {
        let s_ptr = crate::value::js_get_string_pointer_unified(object_handle.get_nanbox_f64())
            as *const crate::StringHeader;
        if !s_ptr.is_null() {
            // NOTE: user-defined `String.prototype` methods on primitive string
            // receivers are routed through the `primitive_kind` fallback below
            // (after native string-method dispatch). Intercepting here, *before*
            // native dispatch, re-enters `js_native_call_method` via the #4100
            // brand-check re-dispatch thunk installed on `String.prototype`
            // (e.g. `replace`), causing unbounded recursion.
            let s_handle = root_scope.root_string_ptr(s_ptr);
            let receiver_string = || s_handle.get_raw_const_ptr::<crate::StringHeader>();
            let arg_at = |i: usize| -> Option<f64> {
                if i < args_len {
                    arg_handles.get(i).map(|handle| handle.get_nanbox_f64())
                } else {
                    None
                }
            };
            // Index/position args follow `ToIntegerOrInfinity` (ToNumber, then
            // truncate, clamping ±Infinity to i32 bounds) so a boolean
            // (`slice(false, true)` → 0,1), numeric string (`"2"`), or `{ valueOf
            // }` object coerces like Node instead of being read as NaN→0. Plain
            // numbers/int32 take the fast path inside the helper. A missing arg
            // is 0 (the per-method default end/length is applied by the arm).
            let arg_i32 = |i: usize| -> i32 {
                match arg_at(i) {
                    Some(v) => crate::string::js_string_index_to_i32(v),
                    None => 0,
                }
            };
            match method_name {
                "toCryptoKey" if crate::buffer::asymmetric_key_meta(s_ptr as usize).is_some() => {
                    let ptr = crate::value::JS_NATIVE_WEBCRYPTO_DISPATCH
                        .load(std::sync::atomic::Ordering::SeqCst);
                    if ptr.is_null() {
                        return f64::from_bits(JSValue::undefined().bits());
                    }
                    let dispatch: unsafe extern "C" fn(*const u8, usize, *const f64, usize) -> f64 =
                        std::mem::transmute(ptr);
                    let key_value = f64::from_bits(JSValue::string_ptr(s_ptr as *mut _).bits());
                    let undefined = f64::from_bits(crate::value::TAG_UNDEFINED);
                    let dispatch_args = [
                        key_value,
                        arg_at(0).unwrap_or(undefined),
                        arg_at(1).unwrap_or(undefined),
                        arg_at(2).unwrap_or(undefined),
                    ];
                    return dispatch(
                        b"keyObjectToCryptoKey".as_ptr(),
                        "keyObjectToCryptoKey".len(),
                        dispatch_args.as_ptr(),
                        dispatch_args.len(),
                    );
                }
                "export" if crate::buffer::asymmetric_key_meta(s_ptr as usize).is_some() => {
                    // Minimal asymmetric KeyObject-surrogate export surface.
                    // The native crypto layer stores PEM-backed RSA/EC keys
                    // and internal Ed/X surrogates as heap strings. For the
                    // high-value Node parity shape (`format: "pem"`), the
                    // stored string is already the exported representation.
                    return object;
                }
                "equals" if crate::buffer::asymmetric_key_meta(s_ptr as usize).is_some() => {
                    if args_len == 0 || args_ptr.is_null() {
                        return f64::from_bits(JSValue::bool(false).bits());
                    }
                    let other = unsafe { *args_ptr };
                    let other_ptr = crate::value::js_get_string_pointer_unified(other)
                        as *const crate::StringHeader;
                    if other_ptr.is_null()
                        || crate::buffer::asymmetric_key_meta(other_ptr as usize).is_none()
                    {
                        return f64::from_bits(JSValue::bool(false).bits());
                    }
                    let eq = crate::string::js_string_equals(s_ptr, other_ptr) != 0;
                    return f64::from_bits(JSValue::bool(eq).bits());
                }
                "at" => {
                    return crate::string::js_string_at(s_ptr, arg_i32(0));
                }
                // `str[Symbol.iterator]()` returns a real String iterator object
                // (codepoint-aware, surrogate pairs collapse to one element) so
                // `Object.getPrototypeOf(''[Symbol.iterator]())` resolves to
                // `%StringIteratorPrototype%` and generic `.next()` drivers work.
                "Symbol.iterator" | "@@iterator" => {
                    return crate::string::string_values_iter(receiver_string());
                }
                "charAt" => {
                    let result = crate::string::js_string_char_at(s_ptr, arg_i32(0));
                    if result.is_null() {
                        return f64::from_bits(JSValue::undefined().bits());
                    }
                    return f64::from_bits(JSValue::string_ptr(result).bits());
                }
                "charCodeAt" => {
                    return crate::string::js_string_char_code_at(s_ptr, arg_i32(0));
                }
                "slice" => {
                    // Coerce args first (`arg_i32` may run user `valueOf` and move
                    // the receiver under GC), then re-fetch the rooted receiver.
                    // An `undefined` end means `len` (spec), not `ToInteger(0)`.
                    let start = if args_len >= 1 { arg_i32(0) } else { 0 };
                    let end_arg = match arg_at(1) {
                        Some(v) if !JSValue::from_bits(v.to_bits()).is_undefined() => {
                            Some(arg_i32(1))
                        }
                        _ => None,
                    };
                    let s = receiver_string();
                    let len_i32 = unsafe { (*s).byte_len } as i32;
                    let end = end_arg.unwrap_or(len_i32);
                    let result = crate::string::js_string_slice(s, start, end);
                    if result.is_null() {
                        return f64::from_bits(JSValue::undefined().bits());
                    }
                    return f64::from_bits(JSValue::string_ptr(result).bits());
                }
                "toString" | "valueOf" => return object_handle.get_nanbox_f64(),
                // Issue #519 follow-up: hono's matcher.js does
                // `path2.match(matcher[0])` where `path2` is a string and
                // `matcher[0]` is a regex. The HIR optimistic
                // `Expr::StringMatch` lowering only fires when the regex
                // arg is a literal or a static `RegExp`-typed Ident — for
                // a `Member` or `Element` access (matcher[0]) it falls
                // through to the dynamic dispatch, which then ended up at
                // the issue #510 catch-all (`(string).match is not a
                // function`) because no runtime arm handled `match`.
                "match" | "matchAll" => {
                    // Missing arg ⇒ `undefined` (→ empty `/(?:)/` regex).
                    let pattern_val =
                        arg_at(0).unwrap_or_else(|| f64::from_bits(JSValue::undefined().bits()));
                    if method_name == "matchAll" {
                        let result_ptr =
                            crate::regex::js_string_match_all_value(s_ptr, pattern_val);
                        if result_ptr.is_null() {
                            return f64::from_bits(JSValue::null().bits());
                        }
                        return f64::from_bits(JSValue::pointer(result_ptr as *mut u8).bits());
                    }
                    // Coerce a non-RegExp arg via `RegExpCreate(ToString(arg))`
                    // (a string pattern / `undefined` / `{ toString }` object),
                    // matching the codegen path.
                    let result_ptr = crate::regex::js_string_match_value(s_ptr, pattern_val);
                    if result_ptr.is_null() {
                        return f64::from_bits(JSValue::null().bits());
                    }
                    return f64::from_bits(JSValue::pointer(result_ptr as *mut u8).bits());
                }
                "search" => {
                    let regex_val =
                        arg_at(0).unwrap_or_else(|| f64::from_bits(JSValue::undefined().bits()));
                    let i32_v = crate::regex::js_string_search_value(s_ptr, regex_val);
                    // Return a RAW `f64` (not NaN-boxed INT32_TAG): a boxed-int
                    // result fails `aString.search(x) === 5` strict-equality
                    // against a plain number literal. Mirrors the `indexOf`
                    // arm's `as f64` convention.
                    return i32_v as f64;
                }
                // Refs #421 — common string methods on any-typed receivers.
                // Hono's compiled JS (and most npm packages with stripped TS
                // types) does `request.url.indexOf("/")` where `url` is in
                // any-typed position because the type annotation on
                // `(request) =>` was erased at bundle time. Without these
                // arms, the v0.5.593 catch-all throws `(string).indexOf is
                // not a function`. Each arm extracts the search-string
                // argument and calls the existing `js_string_*` runtime
                // helper. Static call sites for typed string receivers keep
                // their inline paths in `lower_string_method.rs` and don't
                // come through this dispatcher.
                "concat" => {
                    let acc_handle = root_scope.root_string_ptr(receiver_string());
                    for i in 0..args_len {
                        let value = arg_at(i)
                            .unwrap_or_else(|| f64::from_bits(JSValue::undefined().bits()));
                        let result = crate::string::js_string_concat_value(
                            acc_handle.get_raw_const_ptr::<crate::StringHeader>(),
                            value,
                        );
                        acc_handle.set_raw_const_ptr(result as *const crate::StringHeader);
                    }
                    let result = acc_handle.get_raw_const_ptr::<crate::StringHeader>()
                        as *mut crate::StringHeader;
                    return f64::from_bits(JSValue::string_ptr(result).bits());
                }
                "indexOf" | "includes" | "lastIndexOf" | "startsWith" | "endsWith" => {
                    let search_arg_to_string = |method_id: i32| -> *const crate::StringHeader {
                        let value = arg_at(0)
                            .unwrap_or_else(|| f64::from_bits(JSValue::undefined().bits()));
                        crate::string::js_string_search_value_to_string(value, method_id)
                            as *const crate::StringHeader
                    };
                    let needle_raw = match method_name {
                        "includes" => search_arg_to_string(0),
                        "startsWith" => search_arg_to_string(1),
                        "endsWith" => search_arg_to_string(2),
                        // indexOf / lastIndexOf apply `ToString(searchString)` with
                        // no RegExp TypeError: `s.indexOf(undefined)` searches for
                        // "undefined", `s.indexOf({toString(){…}})` uses the result.
                        _ => {
                            let value = arg_at(0)
                                .unwrap_or_else(|| f64::from_bits(JSValue::undefined().bits()));
                            crate::value::js_jsvalue_to_string(value) as *const crate::StringHeader
                        }
                    };
                    // ToString above may run user code (object `toString`/`valueOf`)
                    // and move either string under GC — root the needle and re-read
                    // the receiver before the byte-level helpers below.
                    let needle_h = if needle_raw.is_null() {
                        None
                    } else {
                        Some(root_scope.root_string_ptr(needle_raw))
                    };
                    let needle = needle_h
                        .as_ref()
                        .map(|h| h.get_raw_const_ptr::<crate::StringHeader>())
                        .unwrap_or(std::ptr::null());
                    let s_ptr = receiver_string();
                    // Integer-returning methods MUST return raw `i as f64` (not
                    // NaN-boxed INT32_TAG) — otherwise downstream comparisons
                    // like `idx < url.length` fail because NaN-boxed values
                    // are NaN and any comparison with NaN returns false. The
                    // typed string-method path in `lower_string_method.rs`
                    // uses `sitofp` (signed-int-to-float) for the same reason.
                    // Boolean-returning methods stay as TAG_TRUE/FALSE since
                    // codegen's `js_is_truthy` and explicit `=== true/false`
                    // checks both unbox these tags correctly (and Node's
                    // `Array.prototype.includes` etc. on plain values
                    // already use this representation).
                    if needle.is_null() {
                        // Match Node: `s.indexOf(undefined)` → -1, includes → false.
                        return match method_name {
                            "indexOf" | "lastIndexOf" => -1.0_f64,
                            "includes" | "startsWith" | "endsWith" => {
                                f64::from_bits(JSValue::bool(false).bits())
                            }
                            _ => f64::from_bits(JSValue::undefined().bits()),
                        };
                    }
                    return match method_name {
                        "indexOf" => {
                            let from = if args_len >= 2 { arg_i32(1) } else { 0 };
                            crate::string::js_string_index_of_from(s_ptr, needle, from) as f64
                        }
                        "includes" => {
                            let from = if args_len >= 2 { arg_i32(1) } else { 0 };
                            let i = crate::string::js_string_index_of_from(s_ptr, needle, from);
                            f64::from_bits(JSValue::bool(i >= 0).bits())
                        }
                        "lastIndexOf" => {
                            if args_len >= 2 {
                                let pos = unsafe { *args_ptr.add(1) };
                                crate::string::js_string_last_index_of_from(s_ptr, needle, pos, 1)
                                    as f64
                            } else {
                                crate::string::js_string_last_index_of(s_ptr, needle) as f64
                            }
                        }
                        "startsWith" => {
                            let at = if args_len >= 2 { arg_i32(1) } else { 0 };
                            let b = crate::string::js_string_starts_with_at(s_ptr, needle, at);
                            f64::from_bits(JSValue::bool(b != 0).bits())
                        }
                        "endsWith" => {
                            let len_i32 = unsafe { (*s_ptr).byte_len } as i32;
                            let at = if args_len >= 2 { arg_i32(1) } else { len_i32 };
                            let b = crate::string::js_string_ends_with_at(s_ptr, needle, at);
                            f64::from_bits(JSValue::bool(b != 0).bits())
                        }
                        _ => f64::from_bits(JSValue::undefined().bits()),
                    };
                }
                "toUpperCase" => {
                    let r = crate::string::js_string_to_upper_case(s_ptr);
                    return f64::from_bits(JSValue::string_ptr(r).bits());
                }
                "toLowerCase" => {
                    let r = crate::string::js_string_to_lower_case(s_ptr);
                    return f64::from_bits(JSValue::string_ptr(r).bits());
                }
                "trim" => {
                    let r = crate::string::js_string_trim(s_ptr);
                    return f64::from_bits(JSValue::string_ptr(r).bits());
                }
                "trimStart" | "trimLeft" => {
                    let r = crate::string::js_string_trim_start(s_ptr);
                    return f64::from_bits(JSValue::string_ptr(r).bits());
                }
                "trimEnd" | "trimRight" => {
                    let r = crate::string::js_string_trim_end(s_ptr);
                    return f64::from_bits(JSValue::string_ptr(r).bits());
                }
                "substring" => {
                    // An `undefined` end means `len` (spec), not `ToInteger(0)`.
                    let start = if args_len >= 1 { arg_i32(0) } else { 0 };
                    let end_arg = match arg_at(1) {
                        Some(v) if !JSValue::from_bits(v.to_bits()).is_undefined() => {
                            Some(arg_i32(1))
                        }
                        _ => None,
                    };
                    let s = receiver_string();
                    let len_i32 = unsafe { (*s).byte_len } as i32;
                    let end = end_arg.unwrap_or(len_i32);
                    let r = crate::string::js_string_substring(s, start, end);
                    return f64::from_bits(JSValue::string_ptr(r).bits());
                }
                "substr" => {
                    // Legacy substr(start, length); negative start from end,
                    // 2nd arg is a length. i32::MIN = "length omitted" (#2897).
                    let start = if args_len >= 1 { arg_i32(0) } else { 0 };
                    let length = if args_len >= 2 { arg_i32(1) } else { i32::MIN };
                    let s = receiver_string();
                    let r = crate::string::js_string_substr(s, start, length);
                    return f64::from_bits(JSValue::string_ptr(r).bits());
                }
                "toLocaleLowerCase" => {
                    let locales =
                        arg_at(0).unwrap_or_else(|| f64::from_bits(JSValue::undefined().bits()));
                    let r = crate::string::js_string_to_locale_lower_case(s_ptr, locales);
                    return f64::from_bits(JSValue::string_ptr(r).bits());
                }
                "toLocaleUpperCase" => {
                    let locales =
                        arg_at(0).unwrap_or_else(|| f64::from_bits(JSValue::undefined().bits()));
                    let r = crate::string::js_string_to_locale_upper_case(s_ptr, locales);
                    return f64::from_bits(JSValue::string_ptr(r).bits());
                }
                "repeat" => {
                    let n = arg_at(0).unwrap_or(0.0);
                    let r = crate::string::js_string_repeat(s_ptr, n);
                    if r.is_null() {
                        return f64::from_bits(JSValue::undefined().bits());
                    }
                    return f64::from_bits(JSValue::string_ptr(r).bits());
                }
                "split" => {
                    // Issue #567: optional 2nd arg `limit`.
                    let limit = if let Some(v) = arg_at(1) {
                        let jsv = JSValue::from_bits(v.to_bits());
                        if jsv.is_undefined() || jsv.is_null() {
                            -1
                        } else {
                            let n = crate::builtins::js_number_coerce(
                                arg_handles
                                    .get(1)
                                    .map(|handle| handle.get_nanbox_f64())
                                    .unwrap_or(v),
                            );
                            if n.is_nan() || n < 0.0 {
                                0
                            } else if n > i32::MAX as f64 {
                                i32::MAX
                            } else {
                                n as i32
                            }
                        }
                    } else {
                        -1
                    };
                    // `split(undefined)` (or no separator) yields the whole string
                    // as a single element — NOT a per-character split (which is what
                    // an empty-string separator does), and NOT [] (`limit === 0`).
                    let sep_undefined = match arg_at(0) {
                        None => true,
                        Some(v) => JSValue::from_bits(v.to_bits()).is_undefined(),
                    };
                    if sep_undefined {
                        let s = receiver_string();
                        let arr = if limit == 0 {
                            crate::array::js_array_alloc(0)
                        } else {
                            let a = crate::array::js_array_alloc(0);
                            crate::array::js_array_push_f64(
                                a,
                                f64::from_bits(
                                    JSValue::string_ptr(s as *mut crate::StringHeader).bits(),
                                ),
                            )
                        };
                        return f64::from_bits(JSValue::pointer(arr as *mut u8).bits());
                    }
                    // A RegExp separator must be passed through as its raw pointer so
                    // `js_string_split_n` detects it (by GC header) and delegates to
                    // the regex splitter. Any other value is ToString-coerced.
                    let v0 = arg_at(0).unwrap();
                    let jv0 = JSValue::from_bits(v0.to_bits());
                    let sep_is_regex =
                        jv0.is_pointer() && crate::regex::is_regex_pointer(jv0.as_pointer::<u8>());
                    let (sep, _sep_h) = if sep_is_regex {
                        (jv0.as_pointer::<crate::StringHeader>(), None)
                    } else {
                        let coerced =
                            crate::builtins::js_string_coerce(v0) as *const crate::StringHeader;
                        let h = root_scope.root_string_ptr(coerced);
                        let p = h.get_raw_const_ptr::<crate::StringHeader>();
                        (p, Some(h))
                    };
                    let s = receiver_string();
                    let arr = crate::string::js_string_split_n(s, sep, limit);
                    return f64::from_bits(JSValue::pointer(arr as *mut u8).bits());
                }
                "replace" | "replaceAll" => {
                    // Two-arg shape: (pattern, replacement). pattern can be a
                    // string OR a RegExp; replacement is a string OR a function.
                    // Function replacements route to the callback helpers so
                    // `str.replace(x, fn)` observes Node's callback argument
                    // shape and receiver binding.
                    let pat_handle = root_string_arg_handle(&root_scope, &arg_handles, 0);
                    let repl_handle = root_string_arg_handle(&root_scope, &arg_handles, 1);
                    let pat_str = || {
                        pat_handle
                            .as_ref()
                            .map(|handle| handle.get_raw_const_ptr::<crate::StringHeader>())
                            .unwrap_or(std::ptr::null())
                    };
                    let repl_str = || {
                        repl_handle
                            .as_ref()
                            .map(|handle| handle.get_raw_const_ptr::<crate::StringHeader>())
                            .unwrap_or(std::ptr::null())
                    };
                    if let (Some(pat_val), Some(repl_val)) = (arg_at(0), arg_at(1)) {
                        let pat_jsv = JSValue::from_bits(pat_val.to_bits());
                        let repl_jsv = JSValue::from_bits(repl_val.to_bits());
                        if repl_jsv.is_pointer() {
                            let repl_raw = (repl_val.to_bits() & 0x0000_FFFF_FFFF_FFFF) as usize;
                            if crate::closure::is_closure_ptr(repl_raw) {
                                if pat_jsv.is_pointer() {
                                    let regex_ptr =
                                        pat_jsv.as_pointer::<crate::regex::RegExpHeader>();
                                    if !regex_ptr.is_null()
                                        && crate::regex::is_regex_pointer(regex_ptr as *const u8)
                                    {
                                        let r = if method_name == "replaceAll" {
                                            crate::regex::js_string_replace_all_regex_fn(
                                                receiver_string(),
                                                regex_ptr,
                                                repl_val,
                                            )
                                        } else {
                                            crate::regex::js_string_replace_regex_fn(
                                                receiver_string(),
                                                regex_ptr,
                                                repl_val,
                                            )
                                        };
                                        return f64::from_bits(JSValue::string_ptr(r).bits());
                                    }
                                }
                                let r = if method_name == "replaceAll" {
                                    crate::regex::js_string_replace_all_string_fn(
                                        receiver_string(),
                                        pat_str(),
                                        repl_val,
                                    )
                                } else {
                                    crate::regex::js_string_replace_string_fn(
                                        receiver_string(),
                                        pat_str(),
                                        repl_val,
                                    )
                                };
                                return f64::from_bits(JSValue::string_ptr(r).bits());
                            }
                        }
                    }
                    // Detect RegExp pattern: NaN-boxed pointer to a RegExpHeader.
                    if let Some(v) = arg_at(0) {
                        let jsv = JSValue::from_bits(v.to_bits());
                        if jsv.is_pointer() {
                            let regex_ptr = jsv.as_pointer::<crate::regex::RegExpHeader>();
                            if !regex_ptr.is_null()
                                && crate::regex::is_regex_pointer(regex_ptr as *const u8)
                            {
                                let r = if method_name == "replaceAll" {
                                    crate::regex::js_string_replace_all_regex(
                                        receiver_string(),
                                        regex_ptr,
                                        repl_str(),
                                    )
                                } else {
                                    crate::regex::js_string_replace_regex(
                                        receiver_string(),
                                        regex_ptr,
                                        repl_str(),
                                    )
                                };
                                return f64::from_bits(JSValue::string_ptr(r).bits());
                            }
                        }
                    }
                    let r = if method_name == "replaceAll" {
                        crate::regex::js_string_replace_all_string(
                            receiver_string(),
                            pat_str(),
                            repl_str(),
                        )
                    } else {
                        crate::regex::js_string_replace_string(
                            receiver_string(),
                            pat_str(),
                            repl_str(),
                        )
                    };
                    return f64::from_bits(JSValue::string_ptr(r).bits());
                }
                // Methods with only a codegen fast path (no native arm) — needed
                // so generic-`this` reflective calls (`String.prototype.padStart.
                // call(boxed, …)`, routed through `string_proto_thunks` after
                // coercing `this` to a string) and `(s: any).padStart(…)` dynamic
                // dispatch resolve to the runtime helper instead of the TypeError
                // catch-all. Argument coercion mirrors `lower_string_method.rs`.
                "padStart" | "padEnd" => {
                    let target_len = arg_at(0).unwrap_or(0.0);
                    // ToString(fillString) when present and not undefined; absent /
                    // undefined leaves a null ptr so the helper defaults to " ".
                    let pad = match arg_at(1) {
                        Some(v) if !JSValue::from_bits(v.to_bits()).is_undefined() => {
                            crate::builtins::js_string_coerce(v) as *const crate::StringHeader
                        }
                        _ => std::ptr::null(),
                    };
                    let s = receiver_string();
                    let r = if method_name == "padStart" {
                        crate::string::js_string_pad_start(s, target_len, pad)
                    } else {
                        crate::string::js_string_pad_end(s, target_len, pad)
                    };
                    return f64::from_bits(JSValue::string_ptr(r).bits());
                }
                "normalize" => {
                    let form =
                        arg_at(0).unwrap_or_else(|| f64::from_bits(JSValue::undefined().bits()));
                    let r = crate::string::js_string_normalize(receiver_string(), form);
                    return f64::from_bits(JSValue::string_ptr(r).bits());
                }
                "localeCompare" => {
                    // ToString(that) is required even for undefined ("undefined").
                    // Root it — `js_string_validate_locales` below may allocate.
                    let other_raw = crate::builtins::js_string_coerce(
                        arg_at(0).unwrap_or_else(|| f64::from_bits(JSValue::undefined().bits())),
                    );
                    let other_h = root_scope.root_string_ptr(other_raw);
                    // `locales` (2nd arg) validated for its RangeError side effect.
                    if let Some(loc) = arg_at(1) {
                        let jv = JSValue::from_bits(loc.to_bits());
                        if !jv.is_undefined() {
                            crate::string::js_string_validate_locales(loc);
                        }
                    }
                    let s = receiver_string();
                    let other = other_h.get_raw_const_ptr::<crate::StringHeader>();
                    // Returns a plain f64 (-1/0/1) — NOT NaN-tagged.
                    return if let Some(opts) = arg_at(2) {
                        crate::string::js_string_locale_compare_opts(s, other, opts)
                    } else {
                        crate::string::js_string_locale_compare(s, other)
                    };
                }
                "isWellFormed" => {
                    return crate::string::js_string_is_well_formed(receiver_string());
                }
                "toWellFormed" => {
                    let r = crate::string::js_string_to_well_formed(receiver_string());
                    return f64::from_bits(JSValue::string_ptr(r).bits());
                }
                // Annex B §B.2.2 HTML wrapper methods. No-arg tag wrappers;
                // the receiver body is never escaped.
                "big" | "blink" | "bold" | "fixed" | "italics" | "small" | "strike" | "sub"
                | "sup" => {
                    let s = receiver_string();
                    let r = match method_name {
                        "big" => crate::string::js_string_big(s),
                        "blink" => crate::string::js_string_blink(s),
                        "bold" => crate::string::js_string_bold(s),
                        "fixed" => crate::string::js_string_fixed(s),
                        "italics" => crate::string::js_string_italics(s),
                        "small" => crate::string::js_string_small(s),
                        "strike" => crate::string::js_string_strike(s),
                        "sub" => crate::string::js_string_sub(s),
                        "sup" => crate::string::js_string_sup(s),
                        _ => unreachable!(),
                    };
                    return f64::from_bits(JSValue::string_ptr(r).bits());
                }
                // Annex B §B.2.2 HTML wrappers that take an attribute value;
                // a missing arg coerces `undefined` -> "undefined", and `"`
                // in the value is escaped to `&quot;`.
                "anchor" | "link" | "fontcolor" | "fontsize" => {
                    let value = crate::builtins::js_string_coerce(
                        arg_at(0).unwrap_or_else(|| f64::from_bits(JSValue::undefined().bits())),
                    );
                    let value_h = root_scope.root_string_ptr(value);
                    let s = receiver_string();
                    let v = value_h.get_raw_const_ptr::<crate::StringHeader>();
                    let r = match method_name {
                        "anchor" => crate::string::js_string_anchor(s, v),
                        "link" => crate::string::js_string_link(s, v),
                        "fontcolor" => crate::string::js_string_fontcolor(s, v),
                        "fontsize" => crate::string::js_string_fontsize(s, v),
                        _ => unreachable!(),
                    };
                    return f64::from_bits(JSValue::string_ptr(r).bits());
                }
                _ => {} // not a handled string method — fall through to TypeError catch-all
            }
        }
    }

    // Check if this is a handle-based object (small integer, not a real heap pointer)
    // Handles are used by Fastify, ioredis, and other native modules that store
    // objects in a registry and use integer IDs to reference them.
    if jsval.is_pointer() {
        let raw_ptr = jsval.as_pointer::<u8>() as usize;
        if crate::value::addr_class::is_small_handle(raw_ptr) {
            // This is a handle, not a real memory pointer - dispatch to stdlib
            if let Some(dispatch) = handle_method_dispatch() {
                return dispatch(
                    raw_ptr as i64,
                    method_name.as_ptr(),
                    method_name.len(),
                    args_ptr,
                    args_len,
                );
            }
            // No dispatcher registered, return undefined
            return f64::from_bits(0x7FF8_0000_0000_0001);
        }

        // Guard: null pointer (raw_ptr == 0) means null POINTER_TAG (0x7FFD_0000_0000_0000)
        // Produced by codegen bugs (uninitialized I64 NaN-boxed). Return undefined instead of crashing.
        if raw_ptr == 0 {
            eprintln!(
                "[NULL_PTR_METHOD_CALL] js_native_call_method: null pointer object for method '{}'",
                method_name
            );
            return f64::from_bits(crate::value::TAG_UNDEFINED);
        }

        // Buffer / Uint8Array dispatch — buffers are allocated raw without
        // a GcHeader, so the GC type check below would read random bytes
        // before the buffer storage and may accidentally match GC_TYPE_OBJECT.
        // Detect buffers via the BUFFER_REGISTRY first and route through the
        // dedicated dispatcher.
        if crate::buffer::is_registered_buffer(raw_ptr) {
            return dispatch_buffer_method(raw_ptr, method_name, args_ptr, args_len);
        }

        // TypedArray method dispatch for NaN-boxed (POINTER_TAG) receivers.
        // The raw-pointer path above (#654) only fires when codegen leaves the
        // typed-array pointer untagged; a `Uint8Array` local loaded as a value
        // is NaN-boxed with POINTER_TAG and reaches here instead. Route the
        // callback-bearing + immutable methods to the shared helper before the
        // GC_TYPE_ARRAY check below (which only matches plain arrays).
        // Issues #2797 / #2798 / #2799.
        if crate::typedarray::lookup_typed_array_kind(raw_ptr).is_some() {
            let ta = raw_ptr as *mut crate::typedarray::TypedArrayHeader;
            if let Some(r) = dispatch_typed_array_method(ta, method_name, args_ptr, args_len) {
                return r;
            }
        }

        // Builtin-prototype borrowing is lowered to a direct receiver call
        // (`[].slice.call(arguments, 1)` -> `arguments.slice(1)`). Arguments
        // objects do not expose Array methods as properties, but this dynamic
        // dispatch path preserves the borrowed Array.prototype.slice behavior.
        if method_name == "slice" {
            if let Some(args_arr) =
                crate::object::arguments_object_to_array(raw_ptr as *const ObjectHeader)
            {
                let undefined = f64::from_bits(crate::value::TAG_UNDEFINED);
                let arg_value = |i: usize| -> f64 {
                    if i < args_len && !args_ptr.is_null() {
                        *args_ptr.add(i)
                    } else {
                        undefined
                    }
                };
                let result =
                    crate::array::js_array_slice_values(args_arr, arg_value(0), arg_value(1));
                return f64::from_bits(JSValue::pointer(result as *mut u8).bits());
            }
        }

        // Array method dispatch: when the object is a real or lazy array at runtime,
        // dispatch callback-bearing array methods directly to the array runtime helpers.
        // This covers the `anyTypedVar.map(fn)` / `anyTypedVar.filter(fn)` pattern where
        // the HIR lowering conservatively skipped Expr::ArrayMap/Filter because the
        // receiver's static type was `any` and the method name overlaps with user-class
        // method names — see the `is_class_overlapping_method` guard in expr_call.rs
        // (issue #267). The GC type check here ensures we only intercept when the
        // value is actually an array; user-class instances with a `.map` closure field
        // fall through to the object-field scan below unchanged.
        if raw_ptr >= crate::gc::GC_HEADER_SIZE + 0x1000 {
            let arr_gc_hdr =
                (raw_ptr as *const u8).sub(crate::gc::GC_HEADER_SIZE) as *const crate::gc::GcHeader;
            let arr_obj_type = (*arr_gc_hdr).obj_type;
            if arr_obj_type == crate::gc::GC_TYPE_ARRAY
                || arr_obj_type == crate::gc::GC_TYPE_LAZY_ARRAY
            {
                // A user-stored callable own property on the array
                // (`arr.getClass = Object.prototype.toString; arr.getClass()`,
                // `arr.myFn = function(){...}; arr.myFn()`) must win over the
                // built-in array method arms below. Array named properties live
                // in the ARRAY_NAMED_PROPS side table, NOT in `keys_array`, so
                // the generic own-field scan further down never finds them and
                // `arr.<name>()` wrongly fell through to a built-in (e.g.
                // `arr.toString()` shadowed by a stored `getClass` resolved as
                // the array's own toString). Check the side table first and, if
                // the stored value is callable, invoke it with `this` = arr.
                let arr = raw_ptr as *const crate::array::ArrayHeader;
                if let Some(stored) =
                    crate::array::array_named_property_get_by_name(arr, method_name)
                {
                    let stored_ptr = crate::value::js_nanbox_get_pointer(stored) as usize;
                    if crate::closure::is_closure_ptr(stored_ptr) {
                        let recv_bits = jsval.bits();
                        let prev_this = IMPLICIT_THIS.with(|c| c.replace(recv_bits));
                        let result =
                            crate::closure::js_native_call_value(stored, args_ptr, args_len);
                        IMPLICIT_THIS.with(|c| c.set(prev_this));
                        return result;
                    }
                }
                match method_name {
                    "toString" => {
                        let arr = raw_ptr as *const crate::array::ArrayHeader;
                        let s = crate::array::js_array_join_value(
                            arr,
                            f64::from_bits(crate::value::TAG_UNDEFINED),
                        );
                        return f64::from_bits(JSValue::string_ptr(s).bits());
                    }
                    "map" if args_len >= 1 && !args_ptr.is_null() => {
                        let arr = raw_ptr as *const crate::array::ArrayHeader;
                        // #4091: throw TypeError for a non-callable callback.
                        let cb_ptr =
                            crate::array::js_validate_array_map_callback(arr as i64, *args_ptr)
                                as *const crate::closure::ClosureHeader;
                        let result = crate::array::js_array_map(arr, cb_ptr);
                        return f64::from_bits(JSValue::pointer(result as *mut u8).bits());
                    }
                    "filter" if args_len >= 1 && !args_ptr.is_null() => {
                        let arr = raw_ptr as *const crate::array::ArrayHeader;
                        // #4091: throw TypeError for a non-callable callback.
                        let cb_ptr = crate::array::js_validate_array_callback(*args_ptr)
                            as *const crate::closure::ClosureHeader;
                        let result = crate::array::js_array_filter(arr, cb_ptr);
                        return f64::from_bits(JSValue::pointer(result as *mut u8).bits());
                    }
                    // Issue #493 followup: dispatch `forEach` on any-typed
                    // arrays the same way as map/filter. Codegen's HIR-level
                    // `Expr::ArrayForEach` only fires for receivers it can
                    // statically prove are arrays — rest params and other
                    // dynamically-typed receivers fall through to the runtime
                    // dispatch tower, where this arm now intercepts. Without
                    // it, `args.forEach(cb)` (where `args` is a closure rest
                    // param threaded across module boundaries) silently
                    // no-op'd, breaking hono's route-registration loop and
                    // any other code that does the same arrow-rest-forEach
                    // pattern.
                    "forEach" if args_len >= 1 && !args_ptr.is_null() => {
                        let arr = raw_ptr as *const crate::array::ArrayHeader;
                        // #4091: throw TypeError for a non-callable callback.
                        let cb_ptr = crate::array::js_validate_array_callback(*args_ptr)
                            as *const crate::closure::ClosureHeader;
                        crate::array::js_array_forEach(arr, cb_ptr);
                        return f64::from_bits(crate::value::TAG_UNDEFINED);
                    }
                    // Issue #291: defensive `slice` arm for arrays that
                    // reach the generic dispatch tower (e.g. when the
                    // receiver is `Expr::Logical` / `Expr::Conditional` /
                    // `any`-typed `Expr::Call` and codegen's
                    // `is_array_expr` returned false). Without this arm
                    // the fallthrough returned the static `NULL_OBJECT_BYTES`
                    // sentinel and the next chained operation segfaulted.
                    "slice" => {
                        let arr = raw_ptr as *const crate::array::ArrayHeader;
                        let undefined = f64::from_bits(crate::value::TAG_UNDEFINED);
                        let arg_value = |i: usize| -> f64 {
                            if i < args_len && !args_ptr.is_null() {
                                *args_ptr.add(i)
                            } else {
                                undefined
                            }
                        };
                        let result = if let Some(args_arr) =
                            crate::object::arguments_object_to_array(
                                raw_ptr as *const crate::object::ObjectHeader,
                            ) {
                            crate::array::js_array_slice_values(
                                args_arr,
                                arg_value(0),
                                arg_value(1),
                            )
                        } else {
                            crate::array::js_array_slice_values(arr, arg_value(0), arg_value(1))
                        };
                        return f64::from_bits(JSValue::pointer(result as *mut u8).bits());
                    }
                    // Issue #321 (effect Context/Layer): defensive `splice`
                    // arm for any-typed arrays that reach the generic dispatch
                    // tower. The sibling `slice`/`sort`/`reverse` arms exist
                    // but `splice` was missing, so effect's FiberRuntime op
                    // queue (`(arr as any).splice(start, deleteCount)`) threw
                    // "splice is not a function". Mirrors JS semantics:
                    // mutates the receiver in place and returns a new array of
                    // the removed elements. Extra args after deleteCount are
                    // inserted at `start`.
                    "splice" => {
                        let arr = raw_ptr as *mut crate::array::ArrayHeader;
                        // ToIntegerOrInfinity with i32 clamping: NaN → 0,
                        // +Infinity → i32::MAX (clamps to len downstream),
                        // -Infinity → i32::MIN (relative-from-end → 0). The
                        // old `is_infinite() → 0` made `splice(Infinity, 3)`
                        // delete from the front (test262 S15.4.4.12_A2.1_T3).
                        let arg_i32 = |i: usize| -> i32 {
                            if i < args_len && !args_ptr.is_null() {
                                crate::array::js_array_splice_delete_count(*args_ptr.add(i))
                            } else {
                                0
                            }
                        };
                        let start = if args_len >= 1 { arg_i32(0) } else { 0 };
                        // Per spec: splice() deletes nothing, while
                        // splice(start) deletes through the end.
                        let delete_count = if args_len == 0 {
                            0
                        } else if args_len == 1 {
                            i32::MAX
                        } else {
                            arg_i32(1)
                        };
                        // Items to insert are args[2..].
                        let items: Vec<f64> = if args_len > 2 && !args_ptr.is_null() {
                            std::slice::from_raw_parts(args_ptr.add(2), args_len - 2).to_vec()
                        } else {
                            Vec::new()
                        };
                        let items_ptr = if items.is_empty() {
                            std::ptr::null()
                        } else {
                            items.as_ptr()
                        };
                        let mut out_arr: *mut crate::array::ArrayHeader = std::ptr::null_mut();
                        let deleted = crate::array::js_array_splice(
                            arr,
                            start,
                            delete_count,
                            items_ptr,
                            items.len() as u32,
                            &mut out_arr,
                        );
                        return f64::from_bits(JSValue::pointer(deleted as *mut u8).bits());
                    }
                    "shift" => {
                        let arr = raw_ptr as *mut crate::array::ArrayHeader;
                        return crate::array::js_array_shift_f64(arr);
                    }
                    "unshift" => {
                        // #2814: zero-arg returns current length (no mutation);
                        // 1+ args insert all items at the front in source order.
                        // Route the zero-arg case through `js_array_unshift_variadic`
                        // (count 0) as well, so a non-writable `length` still throws
                        // the spec TypeError (`Set(O,"length",…)` always runs).
                        let arr = raw_ptr as *mut crate::array::ArrayHeader;
                        let count = if args_ptr.is_null() {
                            0
                        } else {
                            args_len as u32
                        };
                        let result = crate::array::js_array_unshift_variadic(arr, args_ptr, count);
                        return crate::array::js_array_length(result) as f64;
                    }
                    // Issue #515 followup: defensive `with` arm for arrays that
                    // reach the generic dispatch tower because the HIR fold
                    // bailed (untyped receiver, chained call returning Array,
                    // etc.). Without this arm, tightening the HIR fold to
                    // ignore unknown-type receivers would silently break
                    // legitimate `(arr: any).with(idx, val)` callers.
                    "with" if args_len >= 2 && !args_ptr.is_null() => {
                        let arr = raw_ptr as *const crate::array::ArrayHeader;
                        let index = *args_ptr;
                        let value = *args_ptr.add(1);
                        let result = crate::array::js_array_with(arr, index, value);
                        return f64::from_bits(JSValue::pointer(result as *mut u8).bits());
                    }
                    // Issue #546 followup: defensive `some` / `every` /
                    // `find` / `findIndex` / `findLast` / `findLastIndex`
                    // arms for any-typed receivers that escape the HIR
                    // fast-path. The `is_class_overlapping_method` guard
                    // (expr_call.rs ~2621) bails on Any-typed locals — so
                    // a destructured `const { arr } = entry; arr.some(cb)`
                    // (where `arr` lost its `EntityId<any>[]` type through
                    // destructuring) silently fell through to the object
                    // field-scan and returned the array itself, producing
                    // `typeof = object` instead of a boolean. The hooks
                    // module in @codehz/ecs hits this exact pattern in
                    // `triggerMultiComponentHooks`, so on_set never fired.
                    "some" if args_len >= 1 && !args_ptr.is_null() => {
                        let arr = raw_ptr as *const crate::array::ArrayHeader;
                        // #4091: throw TypeError for a non-callable callback.
                        let cb_ptr = crate::array::js_validate_array_callback(*args_ptr)
                            as *const crate::closure::ClosureHeader;
                        return crate::array::js_array_some(arr, cb_ptr);
                    }
                    "every" if args_len >= 1 && !args_ptr.is_null() => {
                        let arr = raw_ptr as *const crate::array::ArrayHeader;
                        // #4091: throw TypeError for a non-callable callback.
                        let cb_ptr = crate::array::js_validate_array_callback(*args_ptr)
                            as *const crate::closure::ClosureHeader;
                        return crate::array::js_array_every(arr, cb_ptr);
                    }
                    "find" if args_len >= 1 && !args_ptr.is_null() => {
                        let arr = raw_ptr as *const crate::array::ArrayHeader;
                        // #4091: throw TypeError for a non-callable callback.
                        let cb_ptr = crate::array::js_validate_array_callback(*args_ptr)
                            as *const crate::closure::ClosureHeader;
                        return crate::array::js_array_find(arr, cb_ptr);
                    }
                    "findIndex" if args_len >= 1 && !args_ptr.is_null() => {
                        let arr = raw_ptr as *const crate::array::ArrayHeader;
                        // #4091: throw TypeError for a non-callable callback.
                        let cb_ptr = crate::array::js_validate_array_callback(*args_ptr)
                            as *const crate::closure::ClosureHeader;
                        let idx = crate::array::js_array_findIndex(arr, cb_ptr);
                        return idx as f64;
                    }
                    "findLast" if args_len >= 1 && !args_ptr.is_null() => {
                        let arr = raw_ptr as *const crate::array::ArrayHeader;
                        // #4091: throw TypeError for a non-callable callback.
                        let cb_ptr = crate::array::js_validate_array_callback(*args_ptr)
                            as *const crate::closure::ClosureHeader;
                        return crate::array::js_array_find_last(arr, cb_ptr);
                    }
                    "findLastIndex" if args_len >= 1 && !args_ptr.is_null() => {
                        let arr = raw_ptr as *const crate::array::ArrayHeader;
                        // #4091: throw TypeError for a non-callable callback.
                        let cb_ptr = crate::array::js_validate_array_callback(*args_ptr)
                            as *const crate::closure::ClosureHeader;
                        let idx = crate::array::js_array_find_last_index(arr, cb_ptr);
                        return idx as f64;
                    }
                    // Issue #587: `str.split(sep).map(fn).sort()` returned ""
                    // because chained `.sort()` falls through HIR's array-fold
                    // (the `"sort" if !args.is_empty()` arm in expr_call.rs
                    // requires a comparator) and lands here. Without these
                    // arms the very-end fallthrough returns NULL_OBJECT_BYTES,
                    // which JSON.stringify renders as "". The s3-lite-client
                    // SigV4 canonical-query-string builder
                    // (`.split("&").map(...).sort().join("&")`) was the
                    // load-bearing user impact. Same gap for `.reverse()` —
                    // tracked by issue #587's regressions list. Adding
                    // `reduce` / `reduceRight` / `flat` / `flatMap` / `concat`
                    // / `indexOf` / `includes` / `at` / `fill` while we're
                    // here defensively, since they have the same shape and
                    // share the HIR-fold escape risk for chained-call
                    // receivers.
                    "sort" => {
                        let arr = raw_ptr as *mut crate::array::ArrayHeader;
                        // #2796: validate comparator (function | undefined) before sorting.
                        let result = if args_len >= 1 && !args_ptr.is_null() {
                            let cb_ptr = crate::array::js_validate_array_comparator(*args_ptr)
                                as *const crate::closure::ClosureHeader;
                            crate::array::js_array_sort_with_comparator(arr, cb_ptr)
                        } else {
                            crate::array::js_array_sort_default(arr)
                        };
                        return f64::from_bits(JSValue::pointer(result as *mut u8).bits());
                    }
                    "reverse" => {
                        let arr = raw_ptr as *mut crate::array::ArrayHeader;
                        let result = crate::array::js_array_reverse(arr);
                        return f64::from_bits(JSValue::pointer(result as *mut u8).bits());
                    }
                    "reduce" if args_len >= 1 && !args_ptr.is_null() => {
                        let arr = raw_ptr as *const crate::array::ArrayHeader;
                        // #4091: throw TypeError for a non-callable callback.
                        let cb_ptr = crate::array::js_validate_array_callback(*args_ptr)
                            as *const crate::closure::ClosureHeader;
                        let (has_init, init) = if args_len >= 2 {
                            (1i32, *args_ptr.add(1))
                        } else {
                            (0i32, f64::from_bits(crate::value::TAG_UNDEFINED))
                        };
                        return crate::array::js_array_reduce(arr, cb_ptr, has_init, init);
                    }
                    "reduceRight" if args_len >= 1 && !args_ptr.is_null() => {
                        let arr = raw_ptr as *const crate::array::ArrayHeader;
                        // #4091: throw TypeError for a non-callable callback.
                        let cb_ptr = crate::array::js_validate_array_callback(*args_ptr)
                            as *const crate::closure::ClosureHeader;
                        let (has_init, init) = if args_len >= 2 {
                            (1i32, *args_ptr.add(1))
                        } else {
                            (0i32, f64::from_bits(crate::value::TAG_UNDEFINED))
                        };
                        return crate::array::js_array_reduce_right(arr, cb_ptr, has_init, init);
                    }
                    "flat" => {
                        // #2800: honor the optional depth argument. Omitted →
                        // depth 1 (legacy `js_array_flat`); supplied → route to
                        // the depth-aware helper, which applies JS number
                        // coercion (NaN/≤0 → 0, +Infinity → fully flat).
                        let arr = raw_ptr as *const crate::array::ArrayHeader;
                        let result = if args_len >= 1 && !args_ptr.is_null() {
                            crate::array::js_array_flat_depth(arr, *args_ptr)
                        } else {
                            crate::array::js_array_flat(arr)
                        };
                        return f64::from_bits(JSValue::pointer(result as *mut u8).bits());
                    }
                    "flatMap" if args_len >= 1 && !args_ptr.is_null() => {
                        let arr = raw_ptr as *const crate::array::ArrayHeader;
                        // #4091: throw TypeError for a non-callable callback.
                        let cb_ptr = crate::array::js_validate_array_callback(*args_ptr)
                            as *const crate::closure::ClosureHeader;
                        let result = crate::array::js_array_flatMap(arr, cb_ptr);
                        return f64::from_bits(JSValue::pointer(result as *mut u8).bits());
                    }
                    "concat" => {
                        // #2805: non-mutating, variadic concat with
                        // Symbol.isConcatSpreadable handling.
                        let arr = raw_ptr as *const crate::array::ArrayHeader;
                        let result =
                            crate::array::js_array_concat_variadic(arr, args_ptr, args_len as i32);
                        return f64::from_bits(JSValue::pointer(result as *mut u8).bits());
                    }
                    "indexOf" if args_len >= 1 && !args_ptr.is_null() => {
                        // #2804: honor the optional fromIndex (2nd arg).
                        let arr = raw_ptr as *const crate::array::ArrayHeader;
                        let value = *args_ptr;
                        let (from_index, has_from) = if args_len >= 2 {
                            (*args_ptr.add(1), 1)
                        } else {
                            (0.0, 0)
                        };
                        return crate::array::js_array_indexOf_jsvalue(
                            arr, value, from_index, has_from,
                        ) as f64;
                    }
                    "includes" if args_len >= 1 && !args_ptr.is_null() => {
                        // #2804: honor the optional fromIndex (2nd arg).
                        let arr = raw_ptr as *const crate::array::ArrayHeader;
                        let value = *args_ptr;
                        let (from_index, has_from) = if args_len >= 2 {
                            (*args_ptr.add(1), 1)
                        } else {
                            (0.0, 0)
                        };
                        let r = crate::array::js_array_includes_jsvalue(
                            arr, value, from_index, has_from,
                        );
                        return f64::from_bits(JSValue::bool(r != 0).bits());
                    }
                    "lastIndexOf" if args_len >= 1 && !args_ptr.is_null() => {
                        let arr = raw_ptr as *const crate::array::ArrayHeader;
                        let value = *args_ptr;
                        // Optional fromIndex (2nd arg); absent → has_from=0.
                        let (from_index, has_from) = if args_len >= 2 {
                            (*args_ptr.add(1), 1)
                        } else {
                            (0.0, 0)
                        };
                        return crate::array::js_array_last_index_of_jsvalue(
                            arr, value, from_index, has_from,
                        ) as f64;
                    }
                    "at" if args_len >= 1 && !args_ptr.is_null() => {
                        let arr = raw_ptr as *const crate::array::ArrayHeader;
                        return crate::array::js_array_at(arr, *args_ptr);
                    }
                    "fill" if args_len >= 1 && !args_ptr.is_null() => {
                        // #2801: honor the optional start/end range. One arg →
                        // whole-array fill; 2+ args → range fill with the
                        // supplied start and (defaulting to +Infinity →
                        // clamps to length) end, mirroring the static path.
                        let arr = raw_ptr as *mut crate::array::ArrayHeader;
                        let value = *args_ptr;
                        let result = if args_len >= 2 {
                            let start = *args_ptr.add(1);
                            let end = if args_len >= 3 {
                                *args_ptr.add(2)
                            } else {
                                f64::INFINITY
                            };
                            crate::array::js_array_fill_range(arr, value, start, end)
                        } else {
                            crate::array::js_array_fill(arr, value)
                        };
                        return f64::from_bits(JSValue::pointer(result as *mut u8).bits());
                    }
                    "copyWithin" if args_len >= 1 && !args_ptr.is_null() => {
                        // #2802: dynamic dispatch for Array.prototype.copyWithin.
                        // Mirrors the static codegen path: require `target`,
                        // default omitted `start` to 0, pass has_end=0 when
                        // `end` is omitted. Mutates and returns the receiver.
                        let arr = raw_ptr as *mut crate::array::ArrayHeader;
                        let target = *args_ptr;
                        let start = if args_len >= 2 { *args_ptr.add(1) } else { 0.0 };
                        let (has_end, end) = if args_len >= 3 {
                            (1, *args_ptr.add(2))
                        } else {
                            (0, 0.0)
                        };
                        let result =
                            crate::array::js_array_copy_within(arr, target, start, has_end, end);
                        return f64::from_bits(JSValue::pointer(result as *mut u8).bits());
                    }
                    "join" => {
                        let arr = raw_ptr as *const crate::array::ArrayHeader;
                        let separator = if args_len >= 1 && !args_ptr.is_null() {
                            *args_ptr
                        } else {
                            f64::from_bits(crate::value::TAG_UNDEFINED)
                        };
                        let s = crate::array::js_array_join_value(arr, separator);
                        return f64::from_bits(JSValue::string_ptr(s).bits());
                    }
                    // #321: a value-level `arr[Symbol.iterator]()` resolves to
                    // the array's bound `values` method (see symbol.rs), and
                    // `arr.values()`/`.keys()`/`.entries()` reaching the runtime
                    // dispatch tower (not codegen's eager `Expr::ArrayValues`
                    // fast path) must return a real `.next()`-bearing iterator,
                    // not an eager array clone. Effect's `Chunk[Symbol.iterator]`
                    // delegates to `backing.array[Symbol.iterator]()` and then
                    // `Array.from`/`Arr.reduce` drive `.next()` on the result;
                    // without this the call returned `undefined` and surfaced as
                    // `Cannot read properties of undefined (reading '_tag')`.
                    "values" | "Symbol.iterator" | "@@iterator" => {
                        return crate::array::array_values_iter(object);
                    }
                    "keys" => {
                        return crate::array::array_keys_iter(object);
                    }
                    "entries" => {
                        return crate::array::array_entries_iter(object);
                    }
                    // #2803: ES2023 immutable methods reaching the dynamic
                    // dispatch tower (`(arr as any).toSorted()`, computed
                    // `arr[m]()`, chained-call receivers that escape the HIR
                    // fold). Each returns a NEW array and leaves the receiver
                    // unchanged, mirroring the static codegen helpers.
                    "toReversed" => {
                        let arr = raw_ptr as *const crate::array::ArrayHeader;
                        let result = crate::array::js_array_to_reversed(arr);
                        return f64::from_bits(JSValue::pointer(result as *mut u8).bits());
                    }
                    "toSorted" => {
                        let arr = raw_ptr as *const crate::array::ArrayHeader;
                        // #2796: validate comparator (function | undefined);
                        // a null/undefined comparator routes to the default
                        // (string) sort inside js_array_to_sorted_with_comparator.
                        let cmp_ptr = if args_len >= 1 && !args_ptr.is_null() {
                            crate::array::js_validate_array_comparator(*args_ptr)
                                as *const crate::closure::ClosureHeader
                        } else {
                            std::ptr::null()
                        };
                        let result = crate::array::js_array_to_sorted_with_comparator(arr, cmp_ptr);
                        return f64::from_bits(JSValue::pointer(result as *mut u8).bits());
                    }
                    "toSpliced" => {
                        let arr = raw_ptr as *const crate::array::ArrayHeader;
                        // Per spec / #2794: toSpliced() inserts/deletes nothing,
                        // toSpliced(start) deletes through the end. NaN-coercion
                        // for the f64 start/deleteCount is handled in the helper.
                        let start = if args_len >= 1 { *args_ptr } else { 0.0 };
                        let delete_count = if args_len == 0 {
                            0.0
                        } else if args_len == 1 {
                            f64::INFINITY
                        } else {
                            *args_ptr.add(1)
                        };
                        let items: Vec<f64> = if args_len > 2 && !args_ptr.is_null() {
                            std::slice::from_raw_parts(args_ptr.add(2), args_len - 2).to_vec()
                        } else {
                            Vec::new()
                        };
                        let items_ptr = if items.is_empty() {
                            std::ptr::null()
                        } else {
                            items.as_ptr()
                        };
                        let result = crate::array::js_array_to_spliced(
                            arr,
                            start,
                            delete_count,
                            items_ptr,
                            items.len() as u32,
                        );
                        return f64::from_bits(JSValue::pointer(result as *mut u8).bits());
                    }
                    // #2808: Array.prototype.toLocaleString — calls each
                    // non-nullish element's own toLocaleString(locales, options),
                    // renders nullish/hole elements as empty fields, and joins
                    // with commas. Routed here for any-typed / computed receivers.
                    "toLocaleString" => {
                        let arr = raw_ptr as *const crate::array::ArrayHeader;
                        let locales = if args_len >= 1 && !args_ptr.is_null() {
                            *args_ptr
                        } else {
                            f64::from_bits(crate::value::TAG_UNDEFINED)
                        };
                        let options = if args_len >= 2 && !args_ptr.is_null() {
                            *args_ptr.add(1)
                        } else {
                            f64::from_bits(crate::value::TAG_UNDEFINED)
                        };
                        let s = crate::array::js_array_to_locale_string(arr, locales, options);
                        return f64::from_bits(JSValue::string_ptr(s).bits());
                    }
                    _ => {} // not a handled array method — fall through to object dispatch
                }
            }
        }

        // Check if this is a native module namespace object (e.g., fs, os, path)
        let obj = jsval.as_pointer::<ObjectHeader>();
        // Validate GcHeader to confirm this is actually an object before reading class_id
        let gc_header =
            (obj as *const u8).sub(crate::gc::GC_HEADER_SIZE) as *const crate::gc::GcHeader;
        if (*gc_header).obj_type == crate::gc::GC_TYPE_OBJECT {
            if (*obj).class_id == NATIVE_MODULE_CLASS_ID {
                // #853: the `is_valid_obj_ptr` guard that used to live after
                // this return was dead — the early return claims the path
                // unconditionally. Removed.
                return crate::object::native_module::call_native_module_dispatch_hook(
                    obj,
                    method_name,
                    args_ptr,
                    args_len,
                );
            }
            // Issue #1206: Buffer iterators returned from `buf.values()` etc.
            // have a dedicated class id so `.next()` lands here and dispatches
            // to the iterator-protocol helper without paying the generic
            // closure-field scan below.
            if (*obj).class_id == crate::buffer::BUFFER_ITERATOR_CLASS_ID {
                return crate::buffer::dispatch_buffer_iterator_method(
                    obj as *mut ObjectHeader,
                    method_name,
                );
            }
            // #321: array iterators returned from a value-level
            // `arr.values()`/`.keys()`/`.entries()`/`[Symbol.iterator]()`
            // carry a dedicated class id so `.next()` lands in the iterator
            // dispatcher (matching the Buffer iterator above).
            if (*obj).class_id == crate::array::ARRAY_ITERATOR_CLASS_ID {
                return crate::array::dispatch_array_iterator_method(
                    obj as *mut ObjectHeader,
                    method_name,
                );
            }
            if let Some(result) =
                crate::node_test::dispatch_object_method((*obj).class_id, method_name)
            {
                return result;
            }
            // #2856: Map/Set iterators returned from a value-level
            // `m.entries()`/`.keys()`/`.values()` / `s.entries()` etc. carry
            // dedicated class ids so `.next()` lands in the matching iterator
            // dispatcher (mirroring the array iterator above).
            if (*obj).class_id == crate::collection_iter_object::MAP_ITERATOR_CLASS_ID {
                return crate::collection_iter_object::dispatch_map_iterator_method(
                    obj as *mut ObjectHeader,
                    method_name,
                );
            }
            if (*obj).class_id == crate::collection_iter_object::SET_ITERATOR_CLASS_ID {
                return crate::collection_iter_object::dispatch_set_iterator_method(
                    obj as *mut ObjectHeader,
                    method_name,
                );
            }
            if (*obj).class_id == crate::string::STRING_ITERATOR_CLASS_ID {
                return crate::string::dispatch_string_iterator_method(
                    obj as *mut ObjectHeader,
                    method_name,
                );
            }
            if (*obj).class_id == crate::regex::REGEXP_STRING_ITERATOR_CLASS_ID {
                return crate::regex::dispatch_regexp_string_iterator_method(
                    obj as *mut ObjectHeader,
                    method_name,
                );
            }
            // #2874: lazy iterator-helper objects (`Iterator.from(x)` and the
            // chain it produces: `.map`/`.filter`/`.take`/`.drop`/`.flatMap`/
            // `.toArray`/`.forEach`/`.reduce`/`.some`/`.every`/`.find`/`.next`).
            if (*obj).class_id == crate::iterator_helpers::ITERATOR_HELPER_CLASS_ID {
                return crate::iterator_helpers::dispatch_iterator_helper_method(
                    obj as *mut ObjectHeader,
                    method_name,
                    args_ptr,
                    args_len,
                );
            }

            // #2874: an iterator-helper method (`map`/`filter`/`take`/…) on a
            // RAW iterator object — a generator, the runtime array/Map/Set
            // iterators, or any `{ next() }`. Node resolves these on
            // `Iterator.prototype`; wrap the iterator in an identity helper and
            // dispatch there. Skipped when the object defines the name as an own
            // callable field (the user's own method wins). Runs before the
            // own-field scan so the cheap has-own check below stays in sync.
            if crate::iterator_helpers::is_iterator_helper_method(method_name) {
                let has_own = {
                    let mk = crate::string::js_string_from_bytes(
                        method_name.as_ptr(),
                        method_name.len() as u32,
                    );
                    let fv = js_object_get_field_by_name(obj as *const _, mk);
                    let fp =
                        crate::value::js_nanbox_get_pointer(f64::from_bits(fv.bits())) as usize;
                    !fv.is_undefined() && crate::closure::is_closure_ptr(fp)
                };
                if let Some(result) = crate::iterator_helpers::maybe_dispatch_helper_on_iterator(
                    obj as *mut ObjectHeader,
                    method_name,
                    args_ptr,
                    args_len,
                    has_own,
                ) {
                    return result;
                }
            }

            // Scan object fields for a callable property (closure stored via IndexSet)
            let keys = (*obj).keys_array;
            if !keys.is_null() {
                let keys_ptr = keys as usize;
                if (keys_ptr as u64) >> 48 == 0 && keys_ptr >= 0x10000 {
                    let key_count = crate::array::js_array_length(keys) as usize;
                    if key_count <= 65536 {
                        let method_bytes = method_name.as_bytes();
                        for i in 0..key_count {
                            let key_val = crate::array::js_array_get(keys, i as u32);
                            if crate::string::js_string_key_matches_bytes(key_val, method_bytes) {
                                let field_val = js_object_get_field(obj as *mut _, i as u32);
                                // Always try the field as a callable —
                                // `js_native_call_value` validates
                                // CLOSURE_MAGIC internally and safely
                                // returns undefined for non-callables.
                                // The previous `is_pointer()` gate bailed
                                // on raw-pointer-bit values (e.g. the
                                // Promise executor's resolve/reject
                                // closures — stored as
                                // `transmute(ptr → f64)` without a
                                // POINTER_TAG). That turned
                                // `box.resolve(val)` into a no-op that
                                // returned the raw pointer bits instead
                                // of invoking `js_promise_resolve`, so
                                // the outer `await` hung forever
                                // (issue #87).
                                //
                                // Issue #519: bind `this` to the receiver
                                // for the duration of the call. Non-arrow
                                // function bodies read `this` from
                                // IMPLICIT_THIS (codegen Expr::This
                                // fallback when this_stack is empty);
                                // without this save/set/restore, the
                                // body sees `this = undefined` and any
                                // `this.foo()` call falls through to the
                                // issue #510 catch-all "(undefined).foo
                                // is not a function" TypeError. Hono's
                                // RegExpRouter.match (imported function
                                // assigned as a class field) hit this.
                                let recv_bits = jsval.bits();
                                let prev_this = IMPLICIT_THIS.with(|c| c.replace(recv_bits));
                                let result = crate::closure::js_native_call_value(
                                    f64::from_bits(field_val.bits()),
                                    args_ptr,
                                    args_len,
                                );
                                IMPLICIT_THIS.with(|c| c.set(prev_this));
                                return result;
                            }
                        }
                    }
                }
            }

            // Vtable lookup for class instances — fast path via per-callsite IC
            let class_id = (*obj).class_id;
            if class_id != 0 {
                if let Some((func_ptr, param_count, has_synthetic_arguments, has_rest)) =
                    vtable_ic_lookup(class_id, method_name_ptr as usize)
                {
                    let this_i64 = jsval.as_pointer::<u8>() as i64;
                    return call_vtable_method(
                        func_ptr,
                        this_i64,
                        args_ptr,
                        args_len,
                        param_count,
                        has_synthetic_arguments,
                        has_rest,
                    );
                }
                // Refs #420: walk the parent chain via the class registry. Per
                // JS spec, `subInstance.method()` for a method defined on a
                // parent dispatches to the parent's implementation — drizzle's
                // `serial("id").primaryKey()` where primaryKey is on
                // ColumnBuilder (grandparent) but the receiver is a
                // PgSerialBuilder (grandchild). The codegen-side dispatch tower
                // in `lower_call.rs` only registers classes the importing module
                // knows about; for not-by-name-imported subclasses (return
                // values of imported functions) we depend on this runtime walk.
                //
                // DEADLOCK SAFETY: resolve the target under the registry READ
                // lock, then DROP the lock before invoking the method body.
                // A user method body can lazily init a module (function-local
                // `require()` — Next.js `getServerImpl()` → `require('./next-
                // server')`) whose top-level `class` declarations call
                // `js_register_class_method` → a registry WRITE lock. std
                // `RwLock` is not re-entrant, so holding the read guard across
                // the call deadlocked the (single) main thread.
                enum ResolvedMethod {
                    Vtable {
                        func_ptr: usize,
                        param_count: u32,
                        has_synthetic_arguments: bool,
                        has_rest: bool,
                        this_i64: i64,
                    },
                    // #711 part 2 / #321: a method that is an own-property of a
                    // registered prototype object (`Function.prototype = X`,
                    // effect's `EffectPrototype.pipe`).
                    ProtoClosure {
                        field_bits: u64,
                    },
                }
                let mut resolved_method: Option<ResolvedMethod> = None;
                if let Ok(registry) = CLASS_VTABLE_REGISTRY.read() {
                    if let Some(ref reg) = *registry {
                        let mut cur_cid = class_id;
                        let mut depth = 0u32;
                        while depth < 32 {
                            if let Some(vtable) = reg.get(&cur_cid) {
                                if let Some(entry) = vtable.methods.get(method_name) {
                                    vtable_ic_insert(
                                        class_id,
                                        method_name_ptr as usize,
                                        entry.func_ptr,
                                        entry.param_count,
                                        entry.has_synthetic_arguments,
                                        entry.has_rest,
                                    );
                                    resolved_method = Some(ResolvedMethod::Vtable {
                                        func_ptr: entry.func_ptr,
                                        param_count: entry.param_count,
                                        has_synthetic_arguments: entry.has_synthetic_arguments,
                                        has_rest: entry.has_rest,
                                        this_i64: jsval.as_pointer::<u8>() as i64,
                                    });
                                    break;
                                }
                            }
                            let proto_obj = class_prototype_object(cur_cid);
                            if !proto_obj.is_null() {
                                let method_key = crate::string::js_string_from_bytes(
                                    method_name.as_ptr(),
                                    method_name.len() as u32,
                                );
                                let field_val = js_object_get_field_by_name(
                                    proto_obj as *const _,
                                    method_key as *const crate::StringHeader,
                                );
                                if !field_val.is_undefined() && !field_val.is_null() {
                                    resolved_method = Some(ResolvedMethod::ProtoClosure {
                                        field_bits: field_val.bits(),
                                    });
                                    break;
                                }
                            }
                            match get_parent_class_id(cur_cid) {
                                Some(pid) if pid != 0 => {
                                    cur_cid = pid;
                                    depth += 1;
                                }
                                _ => break,
                            }
                        }
                    }
                }
                // Registry guard released — safe to run the method body (which
                // may register classes via lazy module init).
                match resolved_method {
                    Some(ResolvedMethod::Vtable {
                        func_ptr,
                        param_count,
                        has_synthetic_arguments,
                        has_rest,
                        this_i64,
                    }) => {
                        return call_vtable_method(
                            func_ptr,
                            this_i64,
                            args_ptr,
                            args_len,
                            param_count,
                            has_synthetic_arguments,
                            has_rest,
                        );
                    }
                    Some(ResolvedMethod::ProtoClosure { field_bits }) => {
                        // #321 (effect Context/Layer/Scope): rebind the closure's
                        // `this` slot to the receiver — `clone_closure_rebind_this`
                        // is a no-op for closures that don't capture `this` and for
                        // non-closure values, so those paths are unaffected.
                        let bound = crate::closure::clone_closure_rebind_this(
                            field_bits,
                            f64::from_bits(jsval.bits()),
                        );
                        let prev_this = IMPLICIT_THIS.with(|c| c.replace(jsval.bits()));
                        let result = crate::closure::js_native_call_value(
                            f64::from_bits(bound),
                            args_ptr,
                            args_len,
                        );
                        IMPLICIT_THIS.with(|c| c.set(prev_this));
                        return result;
                    }
                    None => {}
                }
                // #809: independent prototype-object resolution. The walk
                // above only runs when `CLASS_VTABLE_REGISTRY` is `Some` —
                // a program with no user classes that only does
                // `Object.create(objLiteral).method()` has an empty/None
                // registry, so `inst.method()` never reached
                // `class_prototype_object` and threw `<m> is not a
                // function`. Resolve the method off the synthetic-class-id
                // prototype chain directly (reuses the same helper as
                // `js_object_get_field_by_name`), then invoke it with
                // `this` bound to the receiver.
                let method_key = crate::string::js_string_from_bytes(
                    method_name.as_ptr(),
                    method_name.len() as u32,
                );
                if let Some(field_val) =
                    resolve_proto_chain_field(class_id, method_key as *const crate::StringHeader)
                {
                    if !field_val.is_undefined() && !field_val.is_null() {
                        // #321 (effect Context/Layer/Scope): the closure we
                        // just resolved is an *inherited* method — by
                        // construction `resolve_proto_chain_field` only walks
                        // the prototype chain (the receiver's OWN fields are
                        // handled by the earlier keys-array scan), so this is
                        // never an own method. Object-literal methods are
                        // lowered with `captures_this:true` and have their
                        // reserved (last) capture slot patched to the literal
                        // object — i.e. the PROTOTYPE — at construction time
                        // (see `expr.rs::lower_object_literal` /
                        // `symbol.rs::js_object_set_symbol_method`). So when
                        // `o = Object.create(P)` resolves `o.method()`, the
                        // closure carries `this === P`, not `this === o`, and
                        // setting `IMPLICIT_THIS = o` can't override the
                        // baked-in slot that the body reads. Rebind the slot
                        // to the receiver before invoking. This mirrors the
                        // symbol-keyed fix (#1969) for the string-keyed
                        // static-member call path. `clone_closure_rebind_this`
                        // is a no-op for non-`captures_this` closures and for
                        // non-closure values, so inherited *data* properties
                        // and arrow/`this`-free function values are untouched.
                        let bound = crate::closure::clone_closure_rebind_this(
                            field_val.bits(),
                            f64::from_bits(jsval.bits()),
                        );
                        let prev_this = IMPLICIT_THIS.with(|c| c.replace(jsval.bits()));
                        let result = crate::closure::js_native_call_value(
                            f64::from_bits(bound),
                            args_ptr,
                            args_len,
                        );
                        IMPLICIT_THIS.with(|c| c.set(prev_this));
                        return result;
                    }
                }

                // Issue #838: JS-classic `Class.prototype.method = fn`
                // method dispatch. The vtable / proto-object walks above
                // cover ES-class methods and synthetic-prototype-object
                // shapes; this arm catches the case where the method
                // only exists in `CLASS_PROTOTYPE_METHODS`. Bind `this`
                // to the receiver and call the stored closure.
                if let Some(method_value) = lookup_prototype_method(class_id, method_name) {
                    let prev_this = IMPLICIT_THIS.with(|c| c.replace(jsval.bits()));
                    let result =
                        crate::closure::js_native_call_value(method_value, args_ptr, args_len);
                    IMPLICIT_THIS.with(|c| c.set(prev_this));
                    return result;
                }
            }
        }
    }

    // Check Map/Set registries for raw or NaN-boxed pointers.
    // Maps/Sets are allocated with plain alloc (no GcHeader), so they can't be
    // dispatched through the ObjectHeader path below.
    {
        let check_ptr = if jsval.is_pointer() {
            (raw_bits & 0x0000_FFFF_FFFF_FFFF) as usize
        } else if !object.is_nan()
            && crate::value::addr_class::is_above_handle_band(raw_bits as usize)
            && (raw_bits >> 48) == 0
        {
            raw_bits as usize
        } else {
            0
        };
        if check_ptr >= 0x10000 {
            if crate::map::is_registered_map(check_ptr) {
                let map = check_ptr as *mut crate::map::MapHeader;
                let args = if !args_ptr.is_null() && args_len > 0 {
                    std::slice::from_raw_parts(args_ptr, args_len)
                } else {
                    &[]
                };
                return match method_name {
                    "get" if !args.is_empty() => crate::map::js_map_get(map, args[0]),
                    "set" if args.len() >= 2 => {
                        let result = crate::map::js_map_set(map, args[0], args[1]);
                        f64::from_bits(JSValue::pointer(result as *mut u8).bits())
                    }
                    "has" if !args.is_empty() => {
                        let r = crate::map::js_map_has(map, args[0]);
                        f64::from_bits(JSValue::bool(r != 0).bits())
                    }
                    "delete" if !args.is_empty() => {
                        let r = crate::map::js_map_delete(map, args[0]);
                        f64::from_bits(JSValue::bool(r != 0).bits())
                    }
                    "clear" => {
                        crate::map::js_map_clear(map);
                        f64::from_bits(crate::value::TAG_UNDEFINED)
                    }
                    "size" => crate::map::js_map_size(map) as f64,
                    // #2856: value-level iterator methods return real iterator
                    // OBJECTS (not arrays), dispatched via class id.
                    "entries" => f64::from_bits(
                        JSValue::pointer(
                            crate::collection_iter_object::js_map_entries_iter_obj(map) as *mut u8,
                        )
                        .bits(),
                    ),
                    "keys" => f64::from_bits(
                        JSValue::pointer(
                            crate::collection_iter_object::js_map_keys_iter_obj(map) as *mut u8
                        )
                        .bits(),
                    ),
                    "values" => f64::from_bits(
                        JSValue::pointer(
                            crate::collection_iter_object::js_map_values_iter_obj(map) as *mut u8,
                        )
                        .bits(),
                    ),
                    "forEach" if !args.is_empty() => {
                        let this_arg = args
                            .get(1)
                            .copied()
                            .unwrap_or(f64::from_bits(crate::value::TAG_UNDEFINED));
                        crate::map::js_map_foreach(map, args[0], this_arg);
                        f64::from_bits(crate::value::TAG_UNDEFINED)
                    }
                    _ => f64::from_bits(crate::value::TAG_UNDEFINED),
                };
            }
            if crate::set::is_registered_set(check_ptr) {
                let set = check_ptr as *mut crate::set::SetHeader;
                let args = if !args_ptr.is_null() && args_len > 0 {
                    std::slice::from_raw_parts(args_ptr, args_len)
                } else {
                    &[]
                };
                return match method_name {
                    "add" if !args.is_empty() => {
                        let result = crate::set::js_set_add(set, args[0]);
                        f64::from_bits(JSValue::pointer(result as *mut u8).bits())
                    }
                    "has" if !args.is_empty() => {
                        let r = crate::set::js_set_has(set, args[0]);
                        f64::from_bits(JSValue::bool(r != 0).bits())
                    }
                    "delete" if !args.is_empty() => {
                        let r = crate::set::js_set_delete(set, args[0]);
                        f64::from_bits(JSValue::bool(r != 0).bits())
                    }
                    "clear" => {
                        crate::set::js_set_clear(set);
                        f64::from_bits(crate::value::TAG_UNDEFINED)
                    }
                    "size" => crate::set::js_set_size(set) as f64,
                    // #2856: dynamic Set iterator methods previously fell
                    // through to `undefined` (only add/has/delete/clear/size
                    // were handled). Return real iterator objects; `entries`
                    // yields `[v, v]` pairs.
                    "values" | "keys" => f64::from_bits(
                        JSValue::pointer(
                            crate::collection_iter_object::js_set_values_iter_obj(set) as *mut u8,
                        )
                        .bits(),
                    ),
                    "entries" => f64::from_bits(
                        JSValue::pointer(
                            crate::collection_iter_object::js_set_entries_iter_obj(set) as *mut u8,
                        )
                        .bits(),
                    ),
                    "forEach" if !args.is_empty() => {
                        let this_arg = args
                            .get(1)
                            .copied()
                            .unwrap_or(f64::from_bits(crate::value::TAG_UNDEFINED));
                        crate::set::js_set_foreach(set, args[0], this_arg);
                        f64::from_bits(crate::value::TAG_UNDEFINED)
                    }
                    // #2872: ES2024 Set composition methods. union/intersection/
                    // difference/symmetricDifference return a new Set; the
                    // is* predicates return a boolean.
                    "union" if !args.is_empty() => f64::from_bits(
                        JSValue::pointer(crate::set::js_set_union(set, args[0]) as *mut u8).bits(),
                    ),
                    "intersection" if !args.is_empty() => f64::from_bits(
                        JSValue::pointer(crate::set::js_set_intersection(set, args[0]) as *mut u8)
                            .bits(),
                    ),
                    "difference" if !args.is_empty() => f64::from_bits(
                        JSValue::pointer(crate::set::js_set_difference(set, args[0]) as *mut u8)
                            .bits(),
                    ),
                    "symmetricDifference" if !args.is_empty() => f64::from_bits(
                        JSValue::pointer(
                            crate::set::js_set_symmetric_difference(set, args[0]) as *mut u8
                        )
                        .bits(),
                    ),
                    "isSubsetOf" if !args.is_empty() => f64::from_bits(
                        JSValue::bool(crate::set::js_set_is_subset_of(set, args[0]) != 0).bits(),
                    ),
                    "isSupersetOf" if !args.is_empty() => f64::from_bits(
                        JSValue::bool(crate::set::js_set_is_superset_of(set, args[0]) != 0).bits(),
                    ),
                    "isDisjointFrom" if !args.is_empty() => f64::from_bits(
                        JSValue::bool(crate::set::js_set_is_disjoint_from(set, args[0]) != 0)
                            .bits(),
                    ),
                    _ => f64::from_bits(crate::value::TAG_UNDEFINED),
                };
            }
            // Buffer / Uint8Array dispatch — allocated raw, not behind a
            // GcHeader, so it can't be discovered through the ObjectHeader
            // path below. Tracked in BUFFER_REGISTRY. Routes Node-style
            // numeric read/write/search/swap method family through
            // `crate::buffer` helpers.
            if crate::buffer::is_registered_buffer(check_ptr) {
                return dispatch_buffer_method(check_ptr, method_name, args_ptr, args_len);
            }
        }
    }

    // Handle raw pointer values without NaN-box tags.
    // Perry sometimes bitcasts I64 pointers to F64 without NaN-boxing (POINTER_TAG).
    // These appear as subnormal floats with bits in the valid heap address range
    // (above the handle band, below 0x0000_FFFF_FFFF_FFFF, upper 16 bits = 0).
    if !jsval.is_pointer()
        && !object.is_nan()
        && crate::value::addr_class::is_above_handle_band(raw_bits as usize)
        && (raw_bits >> 48) == 0
    {
        // Looks like a raw heap pointer — re-wrap as POINTER_TAG and retry
        let reboxed = f64::from_bits(0x7FFD_0000_0000_0000u64 | raw_bits);
        let reboxed_jsval = JSValue::from_bits(reboxed.to_bits());
        let obj = reboxed_jsval.as_pointer::<ObjectHeader>();
        // Validate GcHeader before accessing
        let gc_header =
            (obj as *const u8).sub(crate::gc::GC_HEADER_SIZE) as *const crate::gc::GcHeader;
        if (*gc_header).obj_type == crate::gc::GC_TYPE_OBJECT {
            // Check for native module namespace
            if (*obj).class_id == NATIVE_MODULE_CLASS_ID {
                // #853: same dead-after-return as the first arm above.
                return crate::object::native_module::call_native_module_dispatch_hook(
                    obj,
                    method_name,
                    args_ptr,
                    args_len,
                );
            }
            // Issue #1206: same class-id check as the NaN-boxed path above
            // so a raw-pointer iterator value (uncommon, but possible after
            // a bitcast) still routes through the iterator dispatcher.
            if (*obj).class_id == crate::buffer::BUFFER_ITERATOR_CLASS_ID {
                return crate::buffer::dispatch_buffer_iterator_method(
                    obj as *mut ObjectHeader,
                    method_name,
                );
            }
            // #321: same array-iterator class-id check as the NaN-boxed path.
            if (*obj).class_id == crate::array::ARRAY_ITERATOR_CLASS_ID {
                return crate::array::dispatch_array_iterator_method(
                    obj as *mut ObjectHeader,
                    method_name,
                );
            }
            // #2856: same Map/Set-iterator class-id checks as the NaN-boxed path.
            if (*obj).class_id == crate::collection_iter_object::MAP_ITERATOR_CLASS_ID {
                return crate::collection_iter_object::dispatch_map_iterator_method(
                    obj as *mut ObjectHeader,
                    method_name,
                );
            }
            if (*obj).class_id == crate::collection_iter_object::SET_ITERATOR_CLASS_ID {
                return crate::collection_iter_object::dispatch_set_iterator_method(
                    obj as *mut ObjectHeader,
                    method_name,
                );
            }
            if (*obj).class_id == crate::string::STRING_ITERATOR_CLASS_ID {
                return crate::string::dispatch_string_iterator_method(
                    obj as *mut ObjectHeader,
                    method_name,
                );
            }
            if (*obj).class_id == crate::regex::REGEXP_STRING_ITERATOR_CLASS_ID {
                return crate::regex::dispatch_regexp_string_iterator_method(
                    obj as *mut ObjectHeader,
                    method_name,
                );
            }
            // #2874: lazy iterator-helper objects, same as the NaN-boxed path.
            if (*obj).class_id == crate::iterator_helpers::ITERATOR_HELPER_CLASS_ID {
                return crate::iterator_helpers::dispatch_iterator_helper_method(
                    obj as *mut ObjectHeader,
                    method_name,
                    args_ptr,
                    args_len,
                );
            }

            // Field name scan on this object
            let keys = (*obj).keys_array;
            if !keys.is_null() {
                let keys_ptr = keys as usize;
                if (keys_ptr as u64) >> 48 == 0 && keys_ptr >= 0x10000 {
                    let key_count = crate::array::js_array_length(keys) as usize;
                    if key_count <= 65536 {
                        let method_bytes = method_name.as_bytes();
                        for i in 0..key_count {
                            let key_val = crate::array::js_array_get(keys, i as u32);
                            if crate::string::js_string_key_matches_bytes(key_val, method_bytes) {
                                let field_val = js_object_get_field(obj as *mut _, i as u32);
                                if field_val.is_pointer() {
                                    return crate::closure::js_native_call_value(
                                        f64::from_bits(field_val.bits()),
                                        args_ptr,
                                        args_len,
                                    );
                                }
                            }
                        }
                    }
                }
            }

            // Vtable lookup — fast path via per-callsite IC
            let class_id = (*obj).class_id;
            if class_id != 0 {
                if let Some((func_ptr, param_count, has_synthetic_arguments, has_rest)) =
                    vtable_ic_lookup(class_id, method_name_ptr as usize)
                {
                    let this_i64 = raw_bits as i64;
                    return call_vtable_method(
                        func_ptr,
                        this_i64,
                        args_ptr,
                        args_len,
                        param_count,
                        has_synthetic_arguments,
                        has_rest,
                    );
                }
                if let Ok(registry) = CLASS_VTABLE_REGISTRY.read() {
                    if let Some(ref reg) = *registry {
                        // Refs #420: parent-chain walk (mirror of the path
                        // above for raw pointer instances).
                        let mut cur_cid = class_id;
                        let mut depth = 0u32;
                        while depth < 32 {
                            if let Some(vtable) = reg.get(&cur_cid) {
                                if let Some(entry) = vtable.methods.get(method_name) {
                                    vtable_ic_insert(
                                        class_id,
                                        method_name_ptr as usize,
                                        entry.func_ptr,
                                        entry.param_count,
                                        entry.has_synthetic_arguments,
                                        entry.has_rest,
                                    );
                                    let this_i64 = raw_bits as i64;
                                    return call_vtable_method(
                                        entry.func_ptr,
                                        this_i64,
                                        args_ptr,
                                        args_len,
                                        entry.param_count,
                                        entry.has_synthetic_arguments,
                                        entry.has_rest,
                                    );
                                }
                            }
                            match get_parent_class_id(cur_cid) {
                                Some(pid) if pid != 0 => {
                                    cur_cid = pid;
                                    depth += 1;
                                }
                                _ => break,
                            }
                        }
                    }
                }
            }
        }
    }

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
                return crate::closure::js_function_bind(object, args_ptr, args_len);
            }
            // #3662: a non-callable `this` (primitive or recognized plain
            // object) is a spec `TypeError` — `Function.prototype.bind.call(x)`.
            // Ambiguous pointers (possible native callables) keep the prior
            // conservative return-unchanged behavior.
            if fn_proto_receiver_not_callable(object) {
                throw_fn_proto_not_callable("bind");
            }
            return object;
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
                return f64::from_bits(JSValue::bool(false).bits());
            }
            if (object.to_bits() >> 48) == 0x7FFE {
                let key_value = if args_len >= 1 && !args_ptr.is_null() {
                    *args_ptr
                } else {
                    f64::from_bits(crate::value::TAG_UNDEFINED)
                };
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
                return f64::from_bits(JSValue::bool(present).bits());
            }
            if jsval.is_pointer() {
                let key_value = if args_len >= 1 && !args_ptr.is_null() {
                    *args_ptr
                } else {
                    f64::from_bits(crate::value::TAG_UNDEFINED)
                };
                let key_str = crate::builtins::js_string_coerce(key_value);
                if key_str.is_null() {
                    return f64::from_bits(JSValue::bool(false).bits());
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
                    return f64::from_bits(JSValue::bool(present).bits());
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
                    return f64::from_bits(JSValue::bool(present).bits());
                }
                if crate::closure::is_closure_ptr(raw) {
                    let present = super::has_own_helpers::str_from_string_header(key_str)
                        .map(|k| super::has_own_helpers::closure_own_key_present(raw, k))
                        .unwrap_or(false);
                    return f64::from_bits(JSValue::bool(present).bits());
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
                                    ExoticKind::Date | ExoticKind::Temporal => false,
                                }
                        })
                        .unwrap_or(false);
                    return f64::from_bits(JSValue::bool(present).bits());
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
                        return f64::from_bits(JSValue::bool(present).bits());
                    }
                    if (*gc_header).obj_type == crate::gc::GC_TYPE_ARRAY {
                        let present = super::has_own_helpers::array_own_key_present(
                            raw as *const crate::array::ArrayHeader,
                            key_str,
                        );
                        return f64::from_bits(JSValue::bool(present).bits());
                    }
                }
                let obj_ptr = jsval.as_pointer::<ObjectHeader>();
                if !obj_ptr.is_null() && is_valid_obj_ptr(obj_ptr as *const u8) {
                    return f64::from_bits(
                        JSValue::bool(own_key_present(obj_ptr as *mut ObjectHeader, key_str))
                            .bits(),
                    );
                }
            }
            return f64::from_bits(JSValue::bool(true).bits());
        }

        // `obj.propertyIsEnumerable(key)` — same shape as
        // `hasOwnProperty`, but descriptor-aware for ordinary objects so
        // non-enumerable properties installed by Error.captureStackTrace /
        // Object.defineProperty report false.
        "propertyIsEnumerable" => {
            if jsval.is_undefined() || jsval.is_null() {
                return f64::from_bits(JSValue::bool(false).bits());
            }
            if !jsval.is_pointer() {
                return f64::from_bits(JSValue::bool(false).bits());
            }
            let key_value = if args_len >= 1 && !args_ptr.is_null() {
                *args_ptr
            } else {
                f64::from_bits(crate::value::TAG_UNDEFINED)
            };
            // Symbol keys must not be string-coerced — route through the
            // canonical entry, which consults the SYMBOL_PROPERTIES side
            // table (mirrors hasOwnProperty's symbol arm).
            if crate::symbol::js_is_symbol(key_value) != 0 {
                return super::object_ops::js_object_property_is_enumerable(object, key_value);
            }
            let key_str = crate::builtins::js_string_coerce(key_value);
            if key_str.is_null() {
                return f64::from_bits(JSValue::bool(false).bits());
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
                        idx < (*buf).length as u32
                    });
                return f64::from_bits(JSValue::bool(enumerable).bits());
            }
            if crate::closure::is_closure_ptr(raw) {
                let Some(key_name) = super::has_own_helpers::str_from_string_header(key_str) else {
                    return f64::from_bits(JSValue::bool(false).bits());
                };
                if !super::has_own_helpers::closure_own_key_present(raw, key_name) {
                    return f64::from_bits(JSValue::bool(false).bits());
                }
                if matches!(key_name, "name" | "length" | "prototype") {
                    return f64::from_bits(JSValue::bool(false).bits());
                }
                let enumerable = get_property_attrs(raw, key_name)
                    .map(|attrs| attrs.enumerable())
                    .unwrap_or(true);
                return f64::from_bits(JSValue::bool(enumerable).bits());
            }
            if raw >= crate::gc::GC_HEADER_SIZE + 0x1000 {
                let gc_header =
                    (raw as *const u8).sub(crate::gc::GC_HEADER_SIZE) as *const crate::gc::GcHeader;
                if (*gc_header).obj_type == crate::gc::GC_TYPE_ERROR {
                    let Some(key_name) = super::has_own_helpers::str_from_string_header(key_str)
                    else {
                        return f64::from_bits(JSValue::bool(false).bits());
                    };
                    let enumerable = crate::error::js_error_builtin_own_property_is_enumerable(
                        raw as *mut crate::error::ErrorHeader,
                        key_name,
                    )
                    .unwrap_or(false);
                    return f64::from_bits(JSValue::bool(enumerable).bits());
                }
                if (*gc_header).obj_type == crate::gc::GC_TYPE_ARRAY {
                    let Some(key_name) = super::has_own_helpers::str_from_string_header(key_str)
                    else {
                        return f64::from_bits(JSValue::bool(false).bits());
                    };
                    if key_name == "length" {
                        return f64::from_bits(JSValue::bool(false).bits());
                    }
                    if !super::has_own_helpers::array_own_key_present(
                        raw as *const crate::array::ArrayHeader,
                        key_str,
                    ) {
                        return f64::from_bits(JSValue::bool(false).bits());
                    }
                    let enumerable = if crate::object::canonical_array_index(key_name).is_some() {
                        true
                    } else {
                        get_property_attrs(raw, key_name)
                            .map(|attrs| attrs.enumerable())
                            .unwrap_or(true)
                    };
                    return f64::from_bits(JSValue::bool(enumerable).bits());
                }
            }
            let obj_ptr = jsval.as_pointer::<ObjectHeader>();
            if obj_ptr.is_null() || !is_valid_obj_ptr(obj_ptr as *const u8) {
                return f64::from_bits(JSValue::bool(false).bits());
            }
            let name_ptr = (key_str as *const u8).add(std::mem::size_of::<crate::StringHeader>());
            let name_len = (*key_str).byte_len as usize;
            let key_name = match std::str::from_utf8(std::slice::from_raw_parts(name_ptr, name_len))
            {
                Ok(s) => s,
                Err(_) => return f64::from_bits(JSValue::bool(false).bits()),
            };
            if (*obj_ptr).class_id == NATIVE_MODULE_CLASS_ID {
                if let Some(module_name) = read_native_module_name(obj_ptr) {
                    return f64::from_bits(
                        JSValue::bool(native_module_has_enumerable_key(&module_name, key_name))
                            .bits(),
                    );
                }
            }
            if !own_key_present(obj_ptr as *mut ObjectHeader, key_str) {
                return f64::from_bits(JSValue::bool(false).bits());
            }
            let enumerable = get_property_attrs(obj_ptr as usize, &key_name)
                .map(|attrs| attrs.enumerable())
                .unwrap_or(true);
            return f64::from_bits(JSValue::bool(enumerable).bits());
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
            return f64::from_bits(
                JSValue::bool(js_object_is_prototype_of_value(object, arg)).bits(),
            );
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
            return if method_name == "__defineGetter__" {
                super::js_object_define_getter(object, key, func)
            } else {
                super::js_object_define_setter(object, key, func)
            };
        }
        "__lookupGetter__" | "__lookupSetter__" => {
            let undef = f64::from_bits(crate::value::TAG_UNDEFINED);
            let key = if args_len >= 1 && !args_ptr.is_null() {
                *args_ptr
            } else {
                undef
            };
            return if method_name == "__lookupGetter__" {
                super::js_object_lookup_getter(object, key)
            } else {
                super::js_object_lookup_setter(object, key)
            };
        }

        // `Object.prototype.valueOf` returns the receiver after ToObject.
        // Perry does not box primitives here; preserving the existing
        // primitive return keeps #2058's bound primitive method reads working,
        // while ordinary objects now get the inherited default instead of
        // falling through to "valueOf is not a function".
        "valueOf" => {
            return js_object_default_value_of(object);
        }

        // `Object.prototype.toLocaleString` invokes the receiver's
        // `toString`. If no custom method is present, fall back to the
        // default `[object Tag]` string. Primitive receivers delegate to
        // their existing `toString` behavior.
        "toLocaleString" => {
            return js_object_default_to_locale_string(object);
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
                return crate::proxy::js_proxy_apply(object, this_arg, arr_box);
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
                let rest_len = if args_len > 1 { args_len - 1 } else { 0 };
                let prev_this = IMPLICIT_THIS.with(|c| c.replace(this_arg.to_bits()));
                // Static bound-method value (`C.m.call(x)`): arm the one-shot
                // static-`this` override so the method body sees `x` instead
                // of the lexical class-ref (static private brand checks).
                let static_target = super::native_module::is_static_bound_method_value(object);
                if static_target {
                    super::static_this_arm(this_arg);
                }
                let result = crate::closure::js_native_call_value(object, rest_ptr, rest_len);
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
                return result;
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
                return crate::proxy::js_proxy_apply(object, this_arg, args_box);
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
                    (args_arr_bits & 0x0000_FFFF_FFFF_FFFF) as usize
                } else if (args_arr_bits >> 48) == 0 && args_arr_bits >= 0x1000 {
                    args_arr_bits as usize
                } else {
                    0
                };
                // Spec CreateListFromArrayLike: a non-nullish, non-object
                // argArray (`fn.apply(null, true)` / `NaN` / `'1,2,3'`) is a
                // TypeError. null/undefined mean "no arguments".
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
                let result =
                    crate::closure::js_native_call_value(object, call_args_ptr, call_args_len);
                if static_target {
                    super::static_this_disarm();
                }
                IMPLICIT_THIS.with(|c| c.set(prev_this));
                // #4973: `http.Server.apply(this, args)` — same inherits
                // pattern as the `call` arm above.
                super::native_this_alias::maybe_alias_explicit_this_construction(
                    object, this_arg, result,
                );
                return result;
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
                    return f64::from_bits(JSValue::string_ptr(str_ptr).bits());
                }
            }
            if let Some((_, payload)) = crate::builtins::boxed_primitive_payload(object) {
                let payload_jsv = JSValue::from_bits(payload.to_bits());
                match crate::builtins::boxed_primitive_to_string_tag(object) {
                    Some("String") => return payload,
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
                        return f64::from_bits(JSValue::string_ptr(str_ptr).bits());
                    }
                    Some("Boolean") => {
                        let s = if payload_jsv.is_bool() && payload_jsv.as_bool() {
                            "true"
                        } else {
                            "false"
                        };
                        let str_ptr =
                            crate::string::js_string_from_bytes(s.as_ptr(), s.len() as u32);
                        return f64::from_bits(JSValue::string_ptr(str_ptr).bits());
                    }
                    Some("BigInt") if payload_jsv.is_bigint() => {
                        let ptr = payload_jsv.as_bigint_ptr();
                        let str_ptr = crate::bigint::js_bigint_to_string(ptr);
                        return f64::from_bits(JSValue::string_ptr(str_ptr).bits());
                    }
                    Some("Symbol") => {
                        let str_ptr = crate::symbol::js_symbol_to_string(payload);
                        return f64::from_bits(JSValue::string_ptr(str_ptr as *mut _).bits());
                    }
                    _ => {}
                }
            }
            if jsval.is_string() {
                return object;
            } else if jsval.is_bigint() {
                let ptr = jsval.as_bigint_ptr();
                let str_ptr = crate::bigint::js_bigint_to_string(ptr);
                return f64::from_bits(JSValue::string_ptr(str_ptr).bits());
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
                    return f64::from_bits(JSValue::string_ptr(str_ptr).bits());
                }
                let s = if n.fract() == 0.0 && n.abs() < (i64::MAX as f64) {
                    (n as i64).to_string()
                } else {
                    n.to_string()
                };
                let str_ptr = crate::string::js_string_from_bytes(s.as_ptr(), s.len() as u32);
                return f64::from_bits(JSValue::string_ptr(str_ptr).bits());
            } else if jsval.is_bool() {
                let s = if jsval.as_bool() { "true" } else { "false" };
                let str_ptr = crate::string::js_string_from_bytes(s.as_ptr(), s.len() as u32);
                return f64::from_bits(JSValue::string_ptr(str_ptr).bits());
            }
            // #3146: `undefined.toString()` / `null.toString()` must throw a
            // TypeError (property read on a nullish base), not return the
            // string "undefined"/"null". Falling through this arm without a
            // `return` reaches the nullish-receiver throw below, which raises
            // `Cannot read properties of <undefined|null> (reading 'toString')`.
        }

        // Array methods - delegate to array runtime
        "push" if jsval.is_pointer() => {
            let mut arr =
                jsval.as_pointer::<crate::array::ArrayHeader>() as *mut crate::array::ArrayHeader;
            if !args_ptr.is_null() {
                for i in 0..args_len {
                    let val = *args_ptr.add(i);
                    arr = crate::array::js_array_push_f64(arr, val);
                }
            }
            return crate::array::js_array_length(arr) as f64;
        }
        "pop" if jsval.is_pointer() => {
            let arr =
                jsval.as_pointer::<crate::array::ArrayHeader>() as *mut crate::array::ArrayHeader;
            return crate::array::js_array_pop_f64(arr);
        }
        "length" if jsval.is_pointer() => {
            let arr = jsval.as_pointer::<crate::array::ArrayHeader>();
            return crate::array::js_array_length(arr) as f64;
        }

        _ => {}
    }

    // If it's an object with a method stored as a closure in a field,
    // try to find and call it
    if jsval.is_pointer() {
        let obj = jsval.as_pointer::<ObjectHeader>();

        // Validate this is an ObjectHeader, not some other heap type.
        // Check GcHeader first (reliable for heap objects), then fallback to ObjectHeader.object_type
        // for static/const objects that don't have GcHeaders.
        // Guard: ensure we can safely read GC_HEADER_SIZE bytes before obj
        if (obj as usize) < crate::gc::GC_HEADER_SIZE + 0x1000 {
            return 0.0;
        }

        // AsyncResource handles are raw Box pointers under POINTER_TAG, not
        // GC heap objects — recognize them by registry membership BEFORE the
        // gc_header read below (which would read foreign allocator memory).
        // Covers receivers whose static type the codegen lost, e.g. a
        // closure-captured `let resource: AsyncResource` (#789).
        if let Some(r) = crate::async_hooks::try_async_resource_method_dispatch(
            obj as i64,
            method_name,
            args_ptr,
            args_len,
        ) {
            return r;
        }

        let gc_header =
            (obj as *const u8).sub(crate::gc::GC_HEADER_SIZE) as *const crate::gc::GcHeader;
        let gc_type = (*gc_header).obj_type;

        // Issue #618: closure receivers (GC_TYPE_CLOSURE=4 OR
        // CLOSURE_MAGIC-marked GC_TYPE_OBJECT slot) — look up the method
        // name in the closure's dynamic-prop side-table. If a callable
        // closure is stored there (via the IIFE-namespace pattern
        // `((sql2) => { sql2.identifier = ...; })(sql)`), dispatch
        // through `js_native_call_value`. Pre-fix this path returned the
        // NULL_OBJECT_BYTES stub for any method call on a closure, so
        // the call result was an empty object stub instead of the
        // dynamic-prop closure's return value.
        let is_closure = gc_type == crate::gc::GC_TYPE_CLOSURE
            || *((obj as *const u8).add(12) as *const u32) == crate::closure::CLOSURE_MAGIC;
        if is_closure {
            let dyn_val = crate::closure::closure_get_dynamic_prop(obj as usize, method_name);
            if dyn_val.to_bits() != crate::value::TAG_UNDEFINED {
                let recv_bits = jsval.bits();
                let prev_this = IMPLICIT_THIS.with(|c| c.replace(recv_bits));
                let result = crate::closure::js_native_call_value(dyn_val, args_ptr, args_len);
                IMPLICIT_THIS.with(|c| c.set(prev_this));
                return result;
            }
            let null_obj_ptr = &NULL_OBJECT_BYTES as *const NullObjectBytes as *mut u8;
            return f64::from_bits(JSValue::pointer(null_obj_ptr).bits());
        }

        if let Some(r) = crate::builtins::try_console_instance_method_dispatch(
            obj,
            method_name,
            args_ptr,
            args_len,
        ) {
            return r;
        }

        // #1387: synthesized `PerformanceEntry#toJSON()`. Entry objects are
        // plain shaped objects with no stored `toJSON` field, so the
        // field-scan dispatch below would miss it. A bound-method read (from
        // the property-get intercept) routes here via `dispatch_bound_method`,
        // and a direct `entry.toJSON()` call lands here too — both serialize
        // the entry's fields into a plain object. Safe to read the header:
        // `obj` is a validated heap object (gc_type read above).
        if method_name == "toJSON"
            && gc_type == crate::gc::GC_TYPE_OBJECT
            && crate::perf_hooks::is_perf_entry_object(obj)
        {
            return crate::perf_hooks::perf_entry_to_json(object);
        }

        // WeakMap/WeakSet dynamic method dispatch (issue #1757/#1758): these
        // are GcHeader-backed objects stamped with a reserved class_id, so a
        // WeakMap reaching here through an `any`-typed binding (effect's
        // `globalValue(() => new WeakMap())`) still routes has/get/set/delete/
        // add to the js_weak* helpers instead of throwing "has is not a
        // function". The class_id guard + routing live in weakref.rs.
        if let Some(r) =
            crate::weakref::try_weak_method_dispatch(obj, object, method_name, args_ptr, args_len)
        {
            return r;
        }

        if gc_type != crate::gc::GC_TYPE_OBJECT {
            // Only accept object_type == 1 (OBJECT_TYPE_REGULAR)
            let object_type = (*obj).object_type;
            // Closes #645: when a method falls through every dispatcher
            // and returns NULL_OBJECT_BYTES (e.g. drizzle's
            // `this.client.prepare(...)` where `this.client` resolved to
            // a heap-object that doesn't dispatch any method named
            // "prepare"), the result gets stored as `this.stmt` and the
            // chained `this.stmt.raw().all(...)` re-enters this function
            // with `obj` pointing at NULL_OBJECT_BYTES — a static stub in
            // the binary's data segment, NOT the macOS userspace heap
            // range that `is_valid_obj_ptr` requires (HEAP_MIN ==
            // 0x200_0000_0000). Pre-fix this returned a literal `0.0`,
            // which the codegen interprets as the IEEE-754 number zero,
            // so the next chained method saw a number receiver and
            // threw `(number).<method> is not a function`. Returning the
            // null-object stub matches every other catch-all in this
            // function and keeps `typeof === "object"` so chained
            // operations propagate consistently instead of mid-chain
            // numeric arithmetic on bit patterns. Truly garbage pointers
            // benefit too — chained calls hit a stable null stub instead
            // of mysterious numeric values.
            if !is_valid_obj_ptr(obj as *const u8) {
                let null_obj_ptr = &NULL_OBJECT_BYTES as *const NullObjectBytes as *mut u8;
                return f64::from_bits(JSValue::pointer(null_obj_ptr).bits());
            }
            if object_type != crate::error::OBJECT_TYPE_REGULAR {
                let null_obj_ptr = &NULL_OBJECT_BYTES as *const NullObjectBytes as *mut u8;
                return f64::from_bits(JSValue::pointer(null_obj_ptr).bits());
            }
        }

        let keys = (*obj).keys_array;

        if !keys.is_null() {
            // Validate keys_array pointer before dereferencing
            let keys_ptr = keys as usize;
            if (keys_ptr as u64) >> 48 != 0 || keys_ptr < 0x10000 {
                let null_obj_ptr = &NULL_OBJECT_BYTES as *const NullObjectBytes as *mut u8;
                return f64::from_bits(JSValue::pointer(null_obj_ptr).bits());
            }
            // Issue #62 phase B: removed macOS "ASCII-like pointer" heuristic —
            // mimalloc + arena strings produce valid heap pointers with bytes
            // 32-39 in the 0x20-0x7E range, causing false positives. The call
            // into `js_object_get_field_by_name` below performs its own
            // GcHeader-based validation.

            // Search for the method in the object's fields
            let key_count = crate::array::js_array_length(keys) as usize;
            // Sanity check key_count
            if key_count > 65536 {
                let null_obj_ptr = &NULL_OBJECT_BYTES as *const NullObjectBytes as *mut u8;
                return f64::from_bits(JSValue::pointer(null_obj_ptr).bits());
            }
            // Compare method_name bytes directly against each stored key
            // instead of allocating a transient StringHeader via
            // js_string_from_bytes — that allocation showed up as ~10% of
            // perf-comprehensive's hot-path samples (one alloc per
            // dynamic-dispatch method call × N keys-array lookups).
            let method_bytes = method_name.as_bytes();
            for i in 0..key_count {
                let key_val = crate::array::js_array_get(keys, i as u32);
                if crate::string::js_string_key_matches_bytes(key_val, method_bytes) {
                    // Found the method — delegate to `js_native_call_value`
                    // which handles both NaN-boxed pointers (POINTER_TAG)
                    // and raw-pointer-bits (e.g. the resolve/reject
                    // closures from `js_promise_new_with_executor`,
                    // transmuted `i64 → f64` so their bits live outside
                    // the NaN range). The earlier `is_pointer()` gate
                    // bailed on the raw-pointer case: `{ resolve }` on a
                    // plain object caused `box.resolve(x)` to land here,
                    // the tag check failed, we fell through to vtable
                    // lookup, and returned NULL_OBJECT_BYTES without
                    // invoking `js_promise_resolve` → the awaiter hung
                    // forever (issue #87). `js_native_call_value`
                    // validates CLOSURE_MAGIC before calling the func
                    // pointer, so non-callable field values (numbers,
                    // strings, booleans) safely return undefined.
                    let field_val = js_object_get_field(obj as *mut _, i as u32);
                    let bound = crate::closure::clone_closure_rebind_this(
                        field_val.bits(),
                        f64::from_bits(jsval.bits()),
                    );
                    let prev_this = IMPLICIT_THIS.with(|c| c.replace(jsval.bits()));
                    let result = crate::closure::js_native_call_value(
                        f64::from_bits(bound),
                        args_ptr,
                        args_len,
                    );
                    IMPLICIT_THIS.with(|c| c.set(prev_this));
                    return result;
                }
            }
        }

        let method_key =
            crate::string::js_string_from_bytes(method_name.as_ptr(), method_name.len() as u32);
        if !method_key.is_null() {
            if let Some(field_val) =
                super::prototype_chain::resolve_inherited_field(obj as usize, method_key)
            {
                if !field_val.is_undefined() && !field_val.is_null() {
                    let bound = crate::closure::clone_closure_rebind_this(
                        field_val.bits(),
                        f64::from_bits(jsval.bits()),
                    );
                    let prev_this = IMPLICIT_THIS.with(|c| c.replace(jsval.bits()));
                    let result = crate::closure::js_native_call_value(
                        f64::from_bits(bound),
                        args_ptr,
                        args_len,
                    );
                    IMPLICIT_THIS.with(|c| c.set(prev_this));
                    return result;
                }
            }
        }

        // Vtable lookup: check if this class has a registered method in the vtable
        let class_id = (*obj).class_id;
        if class_id != 0 {
            if let Ok(registry) = CLASS_VTABLE_REGISTRY.read() {
                if let Some(ref reg) = *registry {
                    if let Some(vtable) = reg.get(&class_id) {
                        if let Some(entry) = vtable.methods.get(method_name) {
                            let this_i64 = jsval.as_pointer::<u8>() as i64;
                            return call_vtable_method(
                                entry.func_ptr,
                                this_i64,
                                args_ptr,
                                args_len,
                                entry.param_count,
                                entry.has_synthetic_arguments,
                                entry.has_rest,
                            );
                        }
                    }
                }
            }
        }
    }

    // Issue #510: throw `TypeError: <expr> is not a function` when
    // the receiver is a non-string primitive (number / int32 / bool /
    // bigint) and dispatch above didn't fire. Node auto-boxes
    // primitives via Number/Boolean/BigInt prototypes; when the
    // prototype lookup yields undefined, the call site throws.
    // Without primitive auto-boxing, Perry must surface the same
    // diagnostic at dispatch time — silently returning the
    // null-object sentinel (the historical fall-through below) lets
    // typo'd method calls run as no-ops, masking real bugs.
    //
    // Strings don't reach this catch-all in the typical case —
    // codegen's `lower_string_method` intercepts string-typed
    // receivers and throws there directly (matching ABI). The string
    // arm is left in here for the rare path where a string flows
    // through dynamic dispatch (e.g. raw NaN-boxed receiver from a
    // Map.get() result the user typed as `any`).
    //
    // Real-object receivers keep the `NULL_OBJECT_BYTES`
    // fall-through. Many existing call paths use this dispatcher as
    // a generic shortcut and rely on the silent null-object return
    // for unknown methods; tightening that is tracked separately.
    //
    // Issue #511: `undefined` / `null` receivers must throw a node-shaped
    // `TypeError: Cannot read properties of <kind> (reading '<method>')`
    // and exit 1. Codegen's `Expr::PropertyGet` lowering already throws
    // on the bare property read (`obj.foo`, issue #462), but the
    // `Call { callee: PropertyGet }` shortcut in `lower_call.rs`
    // routes `obj.foo()` straight to `js_native_call_method` without
    // re-evaluating the receiver through PropertyGet — so the codegen
    // gate never fires for the call form. Without this arm, `x.foo()`
    // on `undefined` silently returned `NULL_OBJECT_BYTES` and the
    // process exited 0, breaking CI gates that rely on non-zero exit
    // for uncaught errors. Earlier toString/bind/push/pop/length match
    // arms intentionally short-circuit before this point so existing
    // Perry code that calls those on `undefined`/`null` keeps working
    // (Perry-ism — Node throws there too, but tightening that breaks
    // unrelated callers; the typo case below is what we want to surface).
    if jsval.is_undefined() || jsval.is_null() {
        let is_null_u32 = if jsval.is_null() { 1u32 } else { 0u32 };
        crate::error::js_throw_type_error_property_access(
            is_null_u32,
            method_name.as_ptr(),
            method_name.len(),
        );
    }
    // Issue #687: INT32-NaN-boxed value whose payload is a registered
    // class id — i.e. a `ClassRef` produced by `Expr::ClassRef` codegen.
    // Effect's `Schema.NonNegative.pipe(int()).annotations({...})` chains
    // produce a ClassRef out of the first `.pipe()` (via the codegen-side
    // defensive no-op in `lower_call.rs::Expr::ClassRef`) and the chained
    // `.annotations(...)` reaches us with that ClassRef as the receiver.
    // Treat it as a chainable no-op: return the receiver so further
    // `.method(...)` calls stay typed-class-shaped during module init.
    // The result isn't semantically equivalent to Effect's transformed
    // schema, but it advances Schema.ts__init past sites that previously
    // threw `(number).<method> is not a function`. Paired with the
    // codegen-side fix in `lower_call.rs` for the simpler
    // `ClassRef.method()` shape.
    if jsval.is_int32() {
        let payload = jsval.as_int32() as u32;
        if payload != 0 {
            let guard = REGISTERED_CLASS_IDS.read().unwrap();
            if let Some(set) = guard.as_ref() {
                if set.contains(&payload) {
                    if let Ok(registry) = CLASS_VTABLE_REGISTRY.read() {
                        if let Some(ref reg) = *registry {
                            if let Some(vtable) = reg.get(&payload) {
                                if let Some(entry) = vtable.methods.get(method_name) {
                                    let undefined_this =
                                        f64::from_bits(crate::value::TAG_UNDEFINED);
                                    return call_vtable_method(
                                        entry.func_ptr,
                                        undefined_this.to_bits() as i64,
                                        args_ptr,
                                        args_len,
                                        entry.param_count,
                                        entry.has_synthetic_arguments,
                                        entry.has_rest,
                                    );
                                }
                            }
                        }
                    }
                    return object;
                }
            }
        }
    }
    let primitive_kind: Option<&'static str> = if jsval.is_any_string() {
        Some("string")
    } else if jsval.is_int32() || jsval.is_number() {
        Some("number")
    } else if jsval.is_bool() {
        Some("boolean")
    } else if jsval.is_bigint() {
        Some("bigint")
    } else {
        None
    };
    if let Some(kind) = primitive_kind {
        let builtin_name = match kind {
            "string" => Some(b"String".as_slice()),
            "number" => Some(b"Number".as_slice()),
            "boolean" => Some(b"Boolean".as_slice()),
            "bigint" => Some(b"BigInt".as_slice()),
            _ => None,
        };
        if let Some(name) = builtin_name {
            if let Some(result) = call_primitive_builtin_prototype_method(
                object,
                name,
                method_name,
                args_ptr,
                args_len,
            ) {
                return result;
            }
        }
        crate::error::js_throw_type_error_not_a_function(
            kind.as_ptr(),
            kind.len(),
            method_name.as_ptr(),
            method_name.len(),
        );
    }

    // Issue #648: real-object receivers also throw when the method
    // doesn't exist anywhere in the dispatch chain (no field-stored
    // closure, no class vtable entry, no prototype walk hit). Pre-fix
    // this catch-all returned `NULL_OBJECT_BYTES` so codegen wouldn't
    // SIGSEGV when it NaN-unboxed the result and dereferenced it as a
    // pointer — but that masked typo'd method calls as silent no-ops
    // and was the single largest source of cascading parity failures
    // (`test_parity_timers` hung waiting on `timers.setTimeout` which
    // silently no-op'd; many other parity tests truncated mid-script
    // when an unimplemented binding's method silently no-op'd inside
    // the surrounding async path). Now we throw the standard `<prop>
    // is not a function` TypeError, which `try`/`catch` catches (per
    // #596's exception-routing fix).
    // Even though this path throws a catchable TypeError, frameworks with broad
    // `try`/`catch` (effect's fiber runtime) swallow it into a die defect that
    // surfaces far downstream as a stray `{}` — hiding the real call site. Print
    // a located report first so `PERRY_DISPATCH_DIAG=1` names the missing
    // method+receiver before the throw is caught.
    // `class X extends Request/Response`: the body methods (`text`/`json`/
    // `arrayBuffer`/`blob`/`bytes`/`formData`/`clone`) live on the underlying
    // native fetch handle, not the JS prototype chain. All user-defined
    // dispatch (own fields, vtable, prototype walk) has missed by here, so a
    // subclass that overrides one of these still wins; only genuinely
    // inherited body methods reach this forward. Refs Hono `c.req.text()`.
    if matches!(
        method_name,
        "text" | "json" | "arrayBuffer" | "blob" | "bytes" | "formData" | "clone"
    ) && jsval.is_pointer()
    {
        let raw = crate::value::js_nanbox_get_pointer(object) as usize;
        if let Some(id) = crate::object::fetch_subclass_handle_id(raw) {
            if let Some(dispatch) = handle_method_dispatch() {
                let args = refreshed_args();
                return dispatch(
                    id,
                    method_name.as_ptr(),
                    method_name.len(),
                    args.as_ptr(),
                    args.len(),
                );
            }
        }
    }

    // #4973: inherits-pattern instances (`http.Server.call(this, …)`) forward
    // method calls that missed every user-defined dispatch layer (own fields,
    // vtable, prototype walk) to their aliased native handle, so
    // `server.listen(...)` / `server.on(...)` on the plain-object `this`
    // behave as calls on the underlying server. See native_this_alias.rs.
    if super::native_this_alias::alias_active() {
        if let Some(handle_val) = super::native_this_alias::alias_handle_for_object(object) {
            // Dispatch through the PRIMARY handle dispatcher only: the alias
            // handle is known to be an http(s) server handle, and the
            // composite's extension dispatchers (ext-net) may own an
            // id-colliding socket that would claim shared names like
            // `address`/`on` first.
            if let Some(dispatch) = super::class_handles::handle_method_dispatch_primary() {
                let handle = (handle_val.to_bits() & crate::value::POINTER_MASK) as i64;
                let args = refreshed_args();
                return dispatch(
                    handle,
                    method_name_ptr as *const u8,
                    method_name_len,
                    args.as_ptr(),
                    args.len(),
                );
            }
        }
    }

    crate::object::class_registry::report_dispatch_miss(
        "call-method (no method/field/proto match)",
        object,
        method_name,
        "throws \"<m> is not a function\"",
    );
    crate::error::js_throw_type_error_not_a_function(
        std::ptr::null(),
        0,
        method_name.as_ptr(),
        method_name.len(),
    );
}
