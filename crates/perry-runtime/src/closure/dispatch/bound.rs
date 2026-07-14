//! Bound-method / bound-function dispatch, `Function.prototype.bind`, and the
//! `call`/`apply`/`bind`-as-value reification helpers.

use super::super::*;
use super::*;

/// Dispatch a bound method call with the given arguments.
/// Extracts the namespace object and method name from the closure captures,
/// then calls js_native_call_method with the packed arguments.
#[inline]
pub unsafe fn dispatch_bound_method(closure: *const ClosureHeader, args: &[f64]) -> f64 {
    let mut namespace_obj = js_closure_get_capture_f64(closure, 0);
    let method_name_ptr = js_closure_get_capture_ptr(closure, 1) as *const i8;
    let method_name_len = js_closure_get_capture_ptr(closure, 2) as usize;

    // #6173: a SYMBOL-keyed class method read as a value — there is no name to
    // re-resolve; the captures carry the already-resolved func_ptr + arity
    // meta (see `SYMBOL_BOUND_METHOD_NAME` for the layout). Discriminated by
    // pointer identity with the static marker, and it MUST run before any
    // name-based interpretation of the captures below (slots 3/4 are not part
    // of the name layout).
    if method_name_ptr == crate::object::SYMBOL_BOUND_METHOD_NAME.as_ptr() as *const i8 {
        return dispatch_symbol_bound_method(closure, namespace_obj, args);
    }

    // Private-method value (`const f = this.#m; f.call(o)`): a `#`-named method
    // read off an instance yields the OWNER class's method function. Unlike a
    // public method, its invocation must dispatch the OWNER's `#m` body with the
    // call-time `this` — NOT re-resolve `#m` on the receiver's own class (a plain
    // object has no `#m`, so the by-name path throws "#m is not a function").
    // The brand check already happened at the READ site (`this.#m`), so here we
    // simply bind the owner's body to whatever `this` the call supplies (spec:
    // `PrivateMethodOrAccessorAdd` installs the shared function; calling it does
    // no brand check). The canonical closure captures the owner class's
    // prototype-ref in slot 0, which carries the owner id.
    if method_name_len > 0 && !method_name_ptr.is_null() {
        if let Ok(name) = std::str::from_utf8(std::slice::from_raw_parts(
            method_name_ptr as *const u8,
            method_name_len,
        )) {
            if name.starts_with('#') {
                if let Some(owner_id) = crate::object::class_prototype_ref_id(namespace_obj) {
                    if let Some((func_ptr, param_count, has_synth_args, has_rest)) =
                        crate::object::lookup_class_method_in_chain(owner_id, name)
                    {
                        // The call-time `this` (IMPLICIT_THIS) is the receiver the
                        // private method body runs against — for `f.call(o)` it is
                        // `o`, for a bare `f()` it is undefined.
                        let call_this = crate::object::js_implicit_this_get();
                        return crate::object::call_vtable_method(
                            func_ptr,
                            call_this.to_bits() as i64,
                            args.as_ptr(),
                            args.len(),
                            param_count,
                            has_synth_args,
                            has_rest,
                        );
                    }
                }
            }
        }
    }

    // Canonical class method value (test262 method identity): a class method is
    // a single shared function object whose captured receiver is the OWNER
    // class's prototype-ref — a marker, not the real `this`. The actual receiver
    // is the call-site `this` (IMPLICIT_THIS): for `const f = c.m; f()` that is
    // the spec `this`, and for `this.m = this.m.bind(this)` the outer
    // `dispatch_bound_function` has already set IMPLICIT_THIS to the instance so
    // the rebind targets the right object. Ordinary `obj.method(args)` calls do
    // NOT reach here (they lower straight to `js_native_call_method`), so this
    // only governs method-as-value invocations.
    namespace_obj = crate::object::canonical_bound_method_receiver(namespace_obj);

    // A bound-method VALUE (`const f = obj.method`) is resolved at READ time and
    // must always invoke that method — even if `obj.method` is later reassigned.
    // The ubiquitous `this.m = this.m.bind(this)` (zod's `ZodType` constructor,
    // React class components, …) self-shadows: the own property `m` becomes the
    // bound function whose target is THIS value, so re-resolving `m` by name here
    // finds the own property and recurses until the call-depth guard returns the
    // null object — observed by user code as `obj.m()` yielding `[object Object]`.
    //
    // For a class-instance receiver, dispatch straight through the vtable,
    // bypassing any own data property of the same name (snapshot semantics).
    // Non-instances (namespace objects; functions captured by a `.bind`/`.call`/
    // `.apply` reify) yield None and fall through to the by-name path unchanged,
    // so this only affects reads of genuine prototype methods.
    if let Some(result) = crate::object::try_dispatch_instance_method_value(
        namespace_obj,
        method_name_ptr,
        method_name_len,
        args.as_ptr(),
        args.len(),
    ) {
        return result;
    }

    crate::object::js_native_call_method(
        namespace_obj,
        method_name_ptr,
        method_name_len,
        args.as_ptr(),
        args.len(),
    )
}

/// #6173: invoke a symbol-bound class-method closure. `receiver` is capture
/// slot 0 (a NaN-boxed instance/prototype-ref, or the INT32 class ref for a
/// static method); the resolved func_ptr and packed param_count/has_rest/
/// is_static meta live in slots 3/4 (see `SYMBOL_BOUND_METHOD_NAME`).
/// Mirrors the direct-call symbol dispatch in `js_native_call_method_value`.
unsafe fn dispatch_symbol_bound_method(
    closure: *const ClosureHeader,
    receiver: f64,
    args: &[f64],
) -> f64 {
    let func_ptr = js_closure_get_capture_ptr(closure, 3) as usize;
    let meta = js_closure_get_capture_ptr(closure, 4) as u64;
    if func_ptr == 0 {
        // A mis-shaped closure (e.g. a 3-capture name closure whose name
        // pointer somehow aliased the marker) reads bounds-checked zeros here
        // — fail soft rather than calling a null fn pointer.
        return f64::from_bits(crate::value::TAG_UNDEFINED);
    }
    let param_count = (meta & 0xFFFF_FFFF) as u32;
    let has_rest = (meta >> 32) & 1 == 1;
    let is_static = (meta >> 33) & 1 == 1;
    if is_static {
        // Bind IMPLICIT_THIS to the class ref for the duration, exactly like
        // the direct-call path. The one-shot static-`this` override (armed by
        // the Function.prototype call/apply arms for a static bound-method
        // value) still wins in the static-method prologue.
        let prev_this = crate::object::js_implicit_this_set(receiver);
        let result = crate::object::call_registered_static_method(
            func_ptr,
            args.as_ptr(),
            args.len(),
            param_count,
            has_rest,
        );
        crate::object::js_implicit_this_set(prev_this);
        result
    } else {
        // Computed symbol methods never synthesize an `arguments` object but
        // DO carry `has_rest` — mirrors the direct-call path.
        crate::object::call_vtable_method(
            func_ptr,
            receiver.to_bits() as i64,
            args.as_ptr(),
            args.len(),
            param_count,
            false,
            has_rest,
        )
    }
}

/// Dispatch a `Function.prototype.bind` result (BOUND_FUNCTION_FUNC_PTR
/// sentinel). Reads the bound target/this/partial-args from the closure
/// captures, prepends the bound args to the call-time args, sets
/// `IMPLICIT_THIS` to the bound receiver, and invokes the target closure.
/// Refs #2840.
#[inline]
pub unsafe fn dispatch_bound_function(closure: *const ClosureHeader, args: &[f64]) -> f64 {
    let target = js_closure_get_capture_f64(closure, 0);
    let bound_this = js_closure_get_capture_f64(closure, 1);
    let bound_args_ptr = js_closure_get_capture_ptr(closure, 2) as *const crate::array::ArrayHeader;

    // Collect the partial-applied (bound) leading args, then append the
    // call-time args. `g = f.bind(obj, 2); g(3)` calls `f` with `(2, 3)`.
    let mut combined: Vec<f64> = Vec::with_capacity(args.len() + 4);
    if !bound_args_ptr.is_null() {
        let n = crate::array::js_array_length(bound_args_ptr) as usize;
        for i in 0..n {
            combined.push(crate::array::js_array_get_f64(bound_args_ptr, i as u32));
        }
    }
    combined.extend_from_slice(args);

    // A bound concise/object-literal method reads `this` from its baked capture
    // slot, not IMPLICIT_THIS — rebind it to the bound receiver so the bound
    // `this` is honored (arrows/non-captures_this targets are returned as-is).
    let target = rebind_explicit_this(target, bound_this);
    let prev_this = crate::object::js_implicit_this_set(bound_this);
    let (call_ptr, call_len) = if combined.is_empty() {
        (std::ptr::null::<f64>(), 0usize)
    } else {
        (combined.as_ptr(), combined.len())
    };
    let result = js_native_call_value(target, call_ptr, call_len);
    crate::object::js_implicit_this_set(prev_this);
    result
}

/// OrdinaryCallBindThis for the `call`/`apply`/`bind` entry points: box a
/// primitive `thisArg` to its wrapper object ONCE, up front, so writes the
/// callee makes through `this` land on the same object it later returns
/// (`Function("this.touched = true; return this;").apply(1)` must yield a
/// Number wrapper with `.touched`). Per-access boxing inside the callee
/// created a fresh wrapper per `this` expression, losing the writes.
///
/// Boxing is gated on the CALLEE: only a *sloppy user* function coerces its
/// `this`. A strict callee observes the raw primitive (`fun.call("")` under
/// `"use strict"` must see `this instanceof String === false`), and built-in
/// thunks (no registered source) do their own receiver coercion — handing
/// them a pre-boxed wrapper would change generic-`this` method semantics.
/// `undefined`/`null` pass through (sloppy global substitution happens
/// elsewhere), as do existing objects.
pub(crate) fn coerce_call_this(target: f64, this_arg: f64) -> f64 {
    let jv = crate::value::JSValue::from_bits(this_arg.to_bits());
    // A class ref (#5515) is an INT32-tagged class id but is the constructor
    // OBJECT, not a primitive — `f.call(C)` binds `this` to C, so leave it
    // unchanged rather than boxing it as a Number alongside undefined/null/ptr.
    if jv.is_undefined()
        || jv.is_null()
        || jv.is_pointer()
        || crate::object::class_ref_id(this_arg).is_some()
    {
        return this_arg;
    }
    let tj = crate::value::JSValue::from_bits(target.to_bits());
    if !tj.is_pointer() {
        return this_arg;
    }
    let mut closure = tj.as_pointer::<ClosureHeader>();
    // Look through bound-function wrappers to the ultimate target — the
    // bound `this` is what reaches it, so its strictness decides.
    for _ in 0..8 {
        if closure.is_null() || unsafe { (*closure).type_tag } != CLOSURE_MAGIC {
            return this_arg;
        }
        if std::ptr::eq(unsafe { (*closure).func_ptr }, BOUND_FUNCTION_FUNC_PTR) {
            let inner = unsafe { js_closure_get_capture_f64(closure, 0) };
            let ij = crate::value::JSValue::from_bits(inner.to_bits());
            if !ij.is_pointer() {
                return this_arg;
            }
            closure = ij.as_pointer::<ClosureHeader>();
            continue;
        }
        break;
    }
    let func_ptr = get_valid_func_ptr(closure);
    if func_ptr.is_null()
        || crate::builtins::function_source_for_ptr(func_ptr as usize).is_none()
        || crate::closure::is_registered_strict_function(func_ptr)
    {
        return this_arg;
    }
    crate::object::js_object_coerce(this_arg)
}

/// `call`/`apply`/`bind`/`Reflect.apply` supply an EXPLICIT `this`. A concise /
/// object-literal method (and object-literal accessor / symbol method) is
/// lowered with `captures_this` and its reserved (last) capture slot baked to
/// the *defining object* at construction time — so its body reads `this` from
/// that slot, NOT from `IMPLICIT_THIS`. Setting `IMPLICIT_THIS` therefore can't
/// redirect such a method to an explicit receiver: the baked slot wins, and the
/// explicit `this` is silently ignored.
///
/// This is exactly the shape schema/validation libraries (zod's `$constructor`)
/// rely on: `inst[k] = proto[k].bind(inst)` over the prototype's own keys, and
/// `this.m = this.m.bind(this)` in a base-class constructor. Without rebinding,
/// every `inst.clone()` / `inst.check()` / `inst.optional()` ran with `this`
/// pinned to the prototype, dropping the instance — cascading to mis-built
/// schema values downstream.
///
/// Clone the target with its `this` slot rebound to `this_arg` so an explicit
/// receiver is honored. Returns `target` UNCHANGED for:
///   - arrow functions (lexical `this` — they must ignore an explicit receiver,
///     yet still carry `CAPTURES_THIS_FLAG`, so they are excluded explicitly);
///   - non-`captures_this` functions (they already read `IMPLICIT_THIS`);
///   - bound functions and non-closure values
///     (`clone_closure_rebind_this` no-ops on these — it only rewrites a
///     `CAPTURES_THIS` slot).
#[inline]
pub(crate) fn rebind_explicit_this(target: f64, this_arg: f64) -> f64 {
    let bits = target.to_bits();
    if bits & 0xFFFF_0000_0000_0000 != 0x7FFD_0000_0000_0000 {
        return target;
    }
    let ptr = (bits & 0x0000_FFFF_FFFF_FFFF) as usize;
    // Reject the `[0, 0x100000)` native-handle band BEFORE probing the pointer:
    // Fetch/http/axios/fastify ids (`0x40000+`) are NaN-boxed with POINTER_TAG
    // but are not heap pointers — `closure_is_arrow` (and the downstream
    // `clone_closure_rebind_this`) would dereference one and SIGSEGV (#4740).
    if ptr < 0x100000 {
        return target;
    }
    // Arrows capture lexical `this`; an explicit receiver must not override it.
    // (They carry CAPTURES_THIS_FLAG, so `clone_closure_rebind_this` alone would
    // wrongly rewrite their lexical-this slot.)
    if crate::closure::closure_is_arrow(ptr as *const ClosureHeader) {
        return target;
    }
    f64::from_bits(crate::closure::clone_closure_rebind_this(bits, this_arg))
}

/// Read a callable's own `name` *property* as a Rust `String`, if present and a
/// String value. Covers names installed by `Object.defineProperty(fn, "name",
/// …)` and the `"bound …"` name a prior `.bind()` stores, neither of which is
/// visible through the declared-name func-ptr registry. Returns `None` when no
/// such property exists or it isn't a String.
unsafe fn read_function_name_property(closure_ptr: usize) -> Option<String> {
    use crate::value::JSValue;
    let name_val = crate::closure::closure_get_dynamic_prop(closure_ptr, "name");
    let name_jv = JSValue::from_bits(name_val.to_bits());
    if !name_jv.is_any_string() {
        return None;
    }
    let hdr = crate::builtins::js_string_coerce(name_val);
    crate::object::has_own_helpers::str_from_string_header(hdr).map(str::to_owned)
}

/// `Function.prototype.bind(thisArg, ...boundArgs)` — create a distinct bound
/// function closure. Captures the target closure value, the bound `this`, and
/// the partial-applied leading args (as a JS array). The returned closure uses
/// the BOUND_FUNCTION_FUNC_PTR sentinel; `js_closure_callN` /
/// `js_native_call_value` route it through `dispatch_bound_function`.
///
/// `.name` is set to `"bound " + target.name` and `.length` to
/// `max(0, target.length - boundArgs.length)`, matching Node. Refs #2840.
#[no_mangle]
pub unsafe extern "C" fn js_function_bind(
    target_value: f64,
    args_ptr: *const f64,
    args_len: usize,
) -> f64 {
    use crate::value::JSValue;

    let target_jv = JSValue::from_bits(target_value.to_bits());
    // Spec brand check: `Function.prototype.bind` on a non-callable receiver
    // throws a TypeError. Callable non-closures (small native function
    // handles, proxies wrapping callables) keep the prior conservative
    // pass-through — they can't be wrapped in a BOUND_FUNCTION closure yet.
    if !crate::object::value_is_callable(target_value)
        && crate::proxy::js_proxy_is_proxy(target_value) != 1
    {
        let message = b"Bind must be called on a function";
        let msg = crate::string::js_string_from_bytes(message.as_ptr(), message.len() as u32);
        let err = crate::error::js_typeerror_new(msg);
        crate::exception::js_throw(crate::value::js_nanbox_pointer(err as i64));
    }
    if !target_jv.is_pointer() {
        return target_value;
    }
    let target_closure = target_jv.as_pointer::<ClosureHeader>();
    if target_closure.is_null() || (*target_closure).type_tag != CLOSURE_MAGIC {
        return target_value;
    }

    let bound_this = if args_len >= 1 && !args_ptr.is_null() {
        coerce_call_this(target_value, *args_ptr)
    } else {
        f64::from_bits(crate::value::TAG_UNDEFINED)
    };
    let bound_arg_count = args_len.saturating_sub(1);

    // Build the partial-args array (NaN-boxed values copied as-is).
    let bound_args_arr: *mut crate::array::ArrayHeader = if bound_arg_count > 0 {
        let arr = crate::array::js_array_alloc(bound_arg_count as u32);
        let mut cur = arr;
        for i in 0..bound_arg_count {
            cur = crate::array::js_array_push_f64(cur, *args_ptr.add(1 + i));
        }
        cur
    } else {
        std::ptr::null_mut()
    };

    // Allocate the bound closure with 3 capture slots.
    let bound = crate::closure::js_closure_alloc(BOUND_FUNCTION_FUNC_PTR, 3);
    js_closure_set_capture_f64(bound, 0, target_value);
    js_closure_set_capture_f64(bound, 1, bound_this);
    js_closure_set_capture_ptr(bound, 2, bound_args_arr as i64);

    // Spec `.length` = max(0, ToIntegerOrInfinity(Get(target, "length")) -
    // boundArgs.length). An `Object.defineProperty(fn, "length", {value})`
    // override (own dynamic prop) wins over the registered declared length,
    // and the value may be NaN (→ 0), ±Infinity, or beyond int32.
    let target_len_f =
        match crate::closure::closure_get_own_dynamic_prop(target_closure as usize, "length") {
            Some(v) => {
                let jv = JSValue::from_bits(v.to_bits());
                if jv.is_int32() {
                    jv.as_int32() as f64
                } else if jv.is_number() {
                    jv.as_number()
                } else {
                    0.0
                }
            }
            None => crate::closure::closure_length(target_closure).unwrap_or(0) as f64,
        };
    let target_len_f = if target_len_f.is_nan() {
        0.0
    } else {
        target_len_f.trunc()
    };
    let bound_len = (target_len_f - bound_arg_count as f64).max(0.0);
    if bound_len.is_finite() && bound_len <= u32::MAX as f64 {
        crate::object::set_builtin_closure_length(bound as usize, bound_len as u32);
    } else {
        // +Infinity (or beyond u32): store as an own dynamic prop, which the
        // `.length` read path prefers over the registered builtin length.
        crate::closure::closure_set_dynamic_prop(
            bound as usize,
            "length",
            f64::from_bits(JSValue::number(bound_len).bits()),
        );
    }

    // Spec `.name` = "bound " + targetName, where targetName is `Get(Target,
    // "name")` (the empty string when that is not a String). Read the target's
    // `name` *property* first — it reflects an `Object.defineProperty(fn,
    // "name", …)` override and a previous `.bind()`'s `"bound …"` name (so
    // `f.bind().bind().name` chains to `"bound bound …"`). Fall back to the
    // declared name from the func-ptr registry for plain named functions, which
    // don't materialize a `name` data property.
    let target_name = read_function_name_property(target_closure as usize)
        .or_else(|| crate::builtins::function_name_for_ptr((*target_closure).func_ptr as usize))
        .unwrap_or_default();
    let bound_name = format!("bound {target_name}");
    let name_ptr =
        crate::string::js_string_from_bytes(bound_name.as_ptr(), bound_name.len() as u32);
    let name_value = f64::from_bits(JSValue::string_ptr(name_ptr).bits());
    crate::closure::closure_set_dynamic_prop(bound as usize, "name", name_value);
    // Spec attributes for a function's own `name`/`length`:
    // { writable: false, enumerable: false, configurable: true }. Without
    // these the dynamic-prop `name` slot defaults to enumerable and shows
    // up in for-in / Object.keys (Test262 bind/instance-name*).
    crate::object::set_builtin_property_attrs(
        bound as usize,
        "name".to_string(),
        crate::object::PropertyAttrs::new(false, false, true),
    );
    crate::object::set_builtin_property_attrs(
        bound as usize,
        "length".to_string(),
        crate::object::PropertyAttrs::new(false, false, true),
    );

    crate::gc::runtime_write_barrier_root_heap_word(bound as u64);
    f64::from_bits(JSValue::pointer(bound as *mut u8).bits())
}

/// Keepalive anchor for the `js_function_bind` symbol. The auto-optimize
/// whole-program LLVM rebuild dead-strips `#[no_mangle]` fns that are only
/// referenced from generated `.o` / other crates; this `#[used]` static
/// survives the bitcode pipeline. See project_auto_optimize_keepalive_3320.
#[used]
static KEEP_JS_FUNCTION_BIND: unsafe extern "C" fn(f64, *const f64, usize) -> f64 =
    js_function_bind;

/// Reify a `Function.prototype.{bind,call,apply}` (or any function method)
/// *read off a closure as a value* into a callable BOUND_METHOD closure. When
/// invoked it routes through `js_native_call_method(receiver, method, …)`, so
/// `f.bind`, `f.call`, `f.apply` behave as real functions instead of reading
/// back `undefined`.
///
/// Fixes the "uncurry-this" idiom `Function.prototype.call.bind(method)`
/// (#3716): reading `.bind` off the reified `Function.prototype.call` value
/// previously returned `undefined`, so the bound function was never created.
/// `receiver` must be a NaN-boxed closure pointer; `method` is a `'static`
/// byte slice (`b"bind"` / `b"call"` / `b"apply"`) whose pointer the
/// BOUND_METHOD captures verbatim.
pub(crate) unsafe fn reify_function_method_value(receiver: f64, method: &'static [u8]) -> f64 {
    let closure = js_closure_alloc(BOUND_METHOD_FUNC_PTR, 3);
    if closure.is_null() {
        return f64::from_bits(crate::value::TAG_UNDEFINED);
    }
    js_closure_set_capture_f64(closure, 0, receiver);
    js_closure_set_capture_ptr(closure, 1, method.as_ptr() as i64);
    js_closure_set_capture_ptr(closure, 2, method.len() as i64);
    // `.name` = the method name so `typeof v === "function"` and `v.name`
    // read back sensibly (e.g. `"bind"`).
    if let Ok(name) = std::str::from_utf8(method) {
        crate::object::set_bound_native_closure_name(closure, name);
        // Spec `.length` of the Function.prototype methods: call/bind take
        // `(thisArg, ...)` → 1, apply `(thisArg, argArray)` → 2. Built-in
        // methods are also not constructors — `new (f.apply)` is a TypeError
        // and they expose no own `.prototype`.
        let len = match name {
            "apply" => 2,
            "call" | "bind" => 1,
            _ => 0,
        };
        crate::object::set_builtin_closure_length(closure as usize, len);
        crate::object::set_builtin_closure_non_constructable(closure as usize);
    }
    crate::gc::runtime_write_barrier_root_heap_word(closure as u64);
    f64::from_bits(crate::value::JSValue::pointer(closure as *mut u8).bits())
}
