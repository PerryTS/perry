use super::super::*;
use super::disposal::*;
use super::object_proto::*;
use super::typed_array::*;
use super::*;

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
    super::native_module::builtin_closure_length(closure as usize)?;
    let name_val = crate::closure::closure_get_dynamic_prop(closure as usize, "name");
    let name_jsv = JSValue::from_bits(name_val.to_bits());
    if !name_jsv.is_any_string() {
        return None;
    }
    // `js_string_coerce` normalizes SSO short strings (e.g. "bind", "join") to a
    // heap StringHeader so the byte read below is valid for inline-stored names.
    let name_hdr = crate::builtins::js_string_coerce(name_val);
    let name = super::has_own_helpers::str_from_string_header(name_hdr)?;
    // #5588: Function-family constructors (Function, GeneratorFunction,
    // AsyncGeneratorFunction) share the noop thunk and have builtin_closure_length
    // set, so this dispatch fires when any of them is reached via js_native_call_value
    // inside js_new_function_construct — treating the newly-allocated receiver as
    // the dispatch target. Exclude them: the noop thunk's undefined return lets
    // `new` fall back to the allocated object, which is what Object.seal tests
    // expect (they don't care whether the result is callable, only that sealing
    // doesn't throw).
    if matches!(
        name,
        "Function" | "GeneratorFunction" | "AsyncGeneratorFunction"
    ) {
        return None;
    }
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
pub(super) unsafe fn fn_proto_receiver_not_callable(object: f64) -> bool {
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
pub(super) fn throw_fn_proto_not_callable(method: &str) -> ! {
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
