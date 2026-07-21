use super::super::*;
use super::*;

/// #4795: resolve `obj[Symbol.dispose]` / `obj[Symbol.asyncDispose]` for the
/// `using`-disposal method names when the disposer is stored under the
/// well-known-symbol key (object literals, dynamically-assigned). Returns
/// `None` (so the caller falls through to vtable / native-handle dispatch)
/// when `object` is not a heap object or has no symbol-keyed disposer.
pub(super) unsafe fn try_symbol_dispose_dispatch(
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
pub(super) unsafe fn object_has_dispose_method(
    obj: *mut ObjectHeader,
    object: f64,
    want_async: bool,
) -> bool {
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
pub(super) unsafe fn try_disposable_stack_method_dispatch(
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
pub(super) unsafe fn js_using_check_disposable(object: f64, want_async: bool) -> f64 {
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
