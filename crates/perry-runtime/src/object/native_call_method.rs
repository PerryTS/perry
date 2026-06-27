//! `js_native_call_method` — the runtime dispatch tower for
//! dynamic method calls on any-typed receivers. Also the apply/spread
//! and computed-key variants (`js_native_call_method_apply`,
//! `js_native_call_method_str_key`).
//!
//! Split out of `object/mod.rs` (issue #1103). Pure relocation — no
//! logic changes.

use super::*;

mod collection_methods;
mod common_methods;
mod disposal;
mod handle_methods;
mod object_proto;
mod primitive_methods;
mod proto_dispatch;
mod string_methods;
mod typed_array;

use disposal::{
    js_using_check_disposable, try_disposable_stack_method_dispatch, try_symbol_dispose_dispatch,
};
pub use object_proto::js_value_to_locale_string;
pub(crate) use object_proto::{
    js_object_default_to_locale_string, js_object_default_value_of, js_object_is_prototype_of_value,
};
pub(crate) use proto_dispatch::{
    try_dispatch_instance_method_value, try_dispatch_value_called_proto_method,
};
pub(super) use typed_array::dispatch_typed_array_method;

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

/// Apply form of `obj[key](...args)` — the spread-call sibling of
/// `js_native_call_method_value`. `key` is a *runtime value* (computed member
/// access, e.g. `receiver[prop](...args)`) and `args_array_handle` is a JS
/// array holding every regular + spread arg already concatenated by codegen.
///
/// Without this, a CallSpread whose callee is a computed member (`IndexGet`)
/// fell through to the plain closure-spread path (`js_closure_call_apply_with_spread`)
/// which dropped `this`, so the invoked method saw `this` = a field-less
/// prototype stub instead of `obj` (NestJS `receiver[prop](...args)` inside its
/// exception-zone proxy — the instance's data fields and inherited methods all
/// read as `undefined`). Materialise the array to a temp buffer and forward to
/// `js_native_call_method_value`, which resolves the method by key and binds
/// `this = obj`.
#[no_mangle]
pub unsafe extern "C" fn js_native_call_method_value_apply(
    object: f64,
    key: f64,
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
    js_native_call_method_value(object, key, args_ptr, args_len)
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

/// #wall4: null-safe variant used ONLY by the unknown-native-method fallback in
/// codegen (`lower_call/native/mod.rs`). The HIR can mis-classify a receiver's
/// class so an `obj.method()` reaches that fallback; dispatching via
/// `js_native_call_method` is correct for a REAL receiver (fixes the Next.js
/// `e.indexOf` mis-typed-as-FormData case where `e` is a real array). But a
/// genuinely undefined/null receiver must NOT hard-throw "Cannot read
/// properties of undefined" — the prior `0.0` sentinel let such call sites limp,
/// and Next's `app-page-turbo.runtime.prod.js` TOP-LEVEL has a nullish-receiver
/// `.indexOf` that, if it throws, aborts the entire module load (then the
/// `_not-found` page can't be required → HTTP 500). Returns the SAME `0.0`
/// sentinel as the old fallback for a nullish receiver (preserving the exact
/// pre-fix non-crashing behavior — `undefined` instead broke downstream code
/// that expected a number); otherwise dispatches identically.
#[no_mangle]
pub unsafe extern "C" fn js_native_call_method_nullsafe(
    object: f64,
    method_name_ptr: *const i8,
    method_name_len: usize,
    args_ptr: *const f64,
    args_len: usize,
) -> f64 {
    let v = crate::value::JSValue::from_bits(object.to_bits());
    if v.is_undefined() || v.is_null() {
        return 0.0;
    }
    // Property-read recovery (scoped to this nullsafe entrypoint, which codegen
    // emits ONLY for the native-instance member-access fallback in
    // `lower_call/native/native_instance_branch.rs`). A bare member READ
    // `recv.<prop>` on a native-instance-classified receiver lowers to a 0-arg
    // `NativeMethodCall` so FFI getters dispatch. When the receiver's RUNTIME
    // type is actually a string — mis-tagged via a stale/aliased native-instance
    // class, the same shape documented for the closure-captured array registered
    // as `FormData` — "length" has no callable method and the dispatcher would
    // throw `(string).length is not a function`, aborting e.g. an inlined
    // string-width/wrap-ansi text-measurement loop (`H += chunk.length`).
    //
    // A string's `length` is a data property, never a method, so return its
    // value (the read carries no args). This is gated to the nullsafe (member-
    // read fallback) path on purpose: a genuine `("abc" as any).length()` call
    // lowers to the plain `js_native_call_method` entrypoint, which still throws
    // the spec-required TypeError. Native classes with a real FFI `length`
    // getter (cheerio selections) are objects, not primitives, and dispatch
    // through their own arm, so they are unaffected.
    if args_len == 0 && method_name_len == 6 && !method_name_ptr.is_null() {
        let name = std::slice::from_raw_parts(method_name_ptr as *const u8, 6);
        if name == b"length" && v.is_any_string() {
            let ptr =
                crate::value::js_get_string_pointer_unified(object) as *const crate::StringHeader;
            if !ptr.is_null() {
                return (*ptr).utf16_len as f64;
            }
        }
    }
    js_native_call_method(object, method_name_ptr, method_name_len, args_ptr, args_len)
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
        // No JS-handle dispatcher: return JS `undefined`. The literal must be
        // TAG_UNDEFINED (0x7FFC_..._0001); an earlier copy used the bit pattern
        // 0x7FF8_..._0001, which is a *signaling NaN* (a JS number), not
        // undefined. A method call that fell through here (e.g. an iterator's
        // `.next()` whose receiver reached this path) then returned that sNaN,
        // which the `for…of` lazy-loop's `js_iterator_result_validate` rejected
        // with "Iterator result is not an object".
        return f64::from_bits(crate::value::TAG_UNDEFINED);
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
        // #5196: a generic, non-mutating `Array.prototype` method on a Proxy
        // (`proxyArray.map(fn)`). `Array.prototype.map` etc. iterate `this`
        // through `[[Get]]`/`length`; routing the spec-generic engine over the
        // proxy fires its `get` trap for `length` and each index. The fused
        // path below (Get(proxy,"method") → Call) instead resolves the built-in
        // method value and re-enters this dispatcher by name — recursing until
        // the depth guard and surfacing the original `Cannot convert undefined
        // or null to object`. The generic engine is the same one used for
        // plain array-like objects whose prototype chain holds a real array.
        let args = refreshed_args();
        if let Some(result) = crate::array::dispatch_arraylike_read_method(
            object,
            method_name,
            args.as_ptr(),
            args.len(),
        ) {
            return result;
        }
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

    if let Some(r) = primitive_methods::dispatch_primitive(
        &root_scope,
        &object_handle,
        &arg_handles,
        object,
        method_name,
        method_name_ptr,
        method_name_len,
        args_ptr,
        args_len,
    ) {
        return r;
    }

    if let Some(r) = string_methods::dispatch_string(
        &root_scope,
        &object_handle,
        &arg_handles,
        object,
        method_name,
        method_name_ptr,
        method_name_len,
        args_ptr,
        args_len,
    ) {
        return r;
    }

    if let Some(r) = handle_methods::dispatch_handle(
        &root_scope,
        &object_handle,
        &arg_handles,
        object,
        method_name,
        method_name_ptr,
        method_name_len,
        args_ptr,
        args_len,
    ) {
        return r;
    }

    if let Some(r) = collection_methods::dispatch_map_set(
        &root_scope,
        &object_handle,
        &arg_handles,
        object,
        method_name,
        method_name_ptr,
        method_name_len,
        args_ptr,
        args_len,
    ) {
        return r;
    }

    if let Some(r) = collection_methods::dispatch_raw_pointer(
        &root_scope,
        &object_handle,
        &arg_handles,
        object,
        method_name,
        method_name_ptr,
        method_name_len,
        args_ptr,
        args_len,
    ) {
        return r;
    }

    if let Some(r) = common_methods::dispatch_common(
        &root_scope,
        &object_handle,
        &arg_handles,
        object,
        method_name,
        method_name_ptr,
        method_name_len,
        args_ptr,
        args_len,
    ) {
        return r;
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
        // NOTE: a bare member READ `str.length` mis-lowered to a 0-arg method
        // call is recovered in `js_native_call_method_nullsafe` (the entrypoint
        // codegen emits for the native-instance member-read fallback), NOT here:
        // this plain entrypoint serves genuine `("abc" as any).length()` calls,
        // which must keep throwing the spec-required TypeError.
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

    // `class X extends Temporal.<Type>`: the prototype methods (`add`/`abs`/
    // `toString`/…) dispatch via the Temporal brand on the underlying cell, not
    // the JS prototype chain. All user-defined dispatch (own fields, vtable,
    // prototype walk) has missed by here, so a subclass override still wins;
    // only genuinely inherited Temporal methods reach this forward. Route them
    // to the stashed cell (`temporal_subclass_cell`). (#5587)
    #[cfg(feature = "temporal")]
    if jsval.is_pointer() {
        let raw = crate::value::js_nanbox_get_pointer(object) as usize;
        if let Some(cell) = crate::object::temporal_subclass_cell(raw) {
            let args = refreshed_args();
            return crate::temporal::dispatch::call_method(cell, method_name, &args);
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

#[cfg(test)]
mod undefined_fallback_tests {
    //! Regression: a method call that falls through to a "no dispatcher →
    //! return undefined" path must hand back JS `undefined`
    //! (`TAG_UNDEFINED` = 0x7FFC_..._0001), NOT the bit pattern
    //! 0x7FF8_..._0001. The latter is a *signaling NaN* — i.e. a JS *number* —
    //! so it slips past every "is this an object?" check. In a `for…of` loop
    //! the lazy desugar validates each `iter.next()` result with
    //! `js_iterator_result_validate`; an sNaN there is reported as the
    //! confusing `TypeError: Iterator result is not an object`. This bit
    //! ~9 fallback returns across `native_call_method.rs` and the stdlib handle
    //! dispatcher; the test pins the JS-handle arm (its dispatcher is null in
    //! unit tests, so the fallback is taken deterministically).

    #[test]
    fn js_handle_method_with_no_dispatcher_returns_real_undefined() {
        // A JS-handle-tagged receiver. `JS_HANDLE_CALL_METHOD` is unset in the
        // test process, so `js_native_call_method` takes the no-dispatcher
        // fallback that previously returned an sNaN.
        let handle = f64::from_bits(crate::value::JS_HANDLE_TAG | 7);
        let method = b"next";
        let result = unsafe {
            super::js_native_call_method(
                handle,
                method.as_ptr() as *const i8,
                method.len(),
                std::ptr::null(),
                0,
            )
        };

        // Must be exactly JS `undefined`, and crucially must NOT be a number —
        // an sNaN would pass `is_number()` and masquerade as a value.
        assert_eq!(
            result.to_bits(),
            crate::value::TAG_UNDEFINED,
            "no-dispatcher handle method call must return TAG_UNDEFINED, got {:#018x}",
            result.to_bits()
        );
        assert!(
            !crate::value::JSValue::from_bits(result.to_bits()).is_number(),
            "fallback result must not classify as a JS number (sNaN regression)"
        );

        // And it must satisfy the iterator-result validator's object check the
        // same way real `undefined` does (i.e. it is correctly *rejected* as a
        // non-object, rather than crashing or being misread as a value).
        assert_ne!(
            result.to_bits(),
            0x7FF8_0000_0000_0001,
            "must not be the signaling-NaN sentinel that tripped for…of"
        );
    }
}

#[cfg(test)]
mod primitive_dataprop_recovery_tests {
    //! Regression: a bare `str.length` member READ can be mis-lowered to a 0-arg
    //! `NativeMethodCall` when the HIR mis-classifies the receiver as a
    //! native-instance type (stale/aliased class tag — e.g. wrap-ansi's
    //! per-character `.length` inside an inlined string-width loop). Codegen
    //! emits that fallback through `js_native_call_method_nullsafe`, where the
    //! runtime receiver is really a string with no callable `length` method, so
    //! this used to throw `(string).length is not a function` and abort the TUI
    //! render. `length` on a string is a data property, so the nullsafe
    //! (member-read fallback) entrypoint now returns its value (UTF-16 length).
    //! The plain `js_native_call_method` entrypoint, which serves genuine
    //! `("abc" as any).length()` calls, keeps throwing the spec TypeError.

    fn string_value(bytes: &[u8]) -> f64 {
        let s = crate::string::js_string_from_bytes(bytes.as_ptr(), bytes.len() as u32);
        f64::from_bits(crate::value::STRING_TAG | (s as u64 & crate::value::POINTER_MASK))
    }

    #[test]
    fn nullsafe_string_length_member_read_returns_length() {
        let recv = string_value(b"hello\xC3\xA9"); // "helloé" → 6 UTF-16 code units
        let method = b"length";
        let result = unsafe {
            super::js_native_call_method_nullsafe(
                recv,
                method.as_ptr() as *const i8,
                method.len(),
                std::ptr::null(),
                0,
            )
        };
        assert_eq!(
            result, 6.0,
            "string.length member-read recovery must return UTF-16 length"
        );
    }
}
