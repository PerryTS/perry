/// `call` / `apply` on a compiled JavaScript body whose explicit receiver
/// needs no boxing and that keeps no re-bindable `this` capture: exactly what
/// the intrinsic does for it, with none of its probes. A compiled body
/// ([`crate::codegen_abi::FN_COMPILED_BODY`]) is never a Proxy, a built-in, a
/// bound or native-module function or a class constructor, so the
/// intrinsic's native special cases (stream / http construction, static
/// bound methods, value-called built-ins) cannot apply, and the call is the
/// ordinary compiled-body call with the arguments padded to its parameters.
/// `apply`'s argument list comes from a real Array, an `arguments` object,
/// or `null` / `undefined`; any other array-like (and everything else) is
/// `None`: the intrinsic's full arm runs.
///
/// # Safety
/// `recv` is a live closure; `args_ptr` holds `argc` values.
pub(crate) unsafe fn compiled_target_call(
    apply: bool,
    recv: f64,
    args_ptr: *const f64,
    argc: usize,
) -> Option<f64> {
    let bits = recv.to_bits();
    if bits & !crate::value::POINTER_MASK != crate::value::POINTER_TAG {
        return None;
    }
    let addr = (bits & crate::value::POINTER_MASK) as usize;
    if !crate::closure::is_closure_ptr(addr) {
        return None;
    }
    let closure = addr as *const crate::closure::ClosureHeader;
    let info = (*closure).info.as_ref()?;
    let undefined = f64::from_bits(crate::value::TAG_UNDEFINED);
    let arg = |i: usize| {
        if i < argc {
            *args_ptr.add(i)
        } else {
            undefined
        }
    };
    let this_arg = arg(0);
    if !receiver_passes_unchanged(recv, closure, info, this_arg) {
        return None;
    }
    let this = crate::closure::JsThis::from_f64(this_arg);
    if !apply {
        let rest = if argc > 1 {
            args_ptr.add(1)
        } else {
            std::ptr::null()
        };
        return Some(enter_compiled_body(
            closure,
            info,
            this,
            rest,
            argc.saturating_sub(1),
        ));
    }
    let list = arg(1).to_bits();
    let values: Vec<f64> = if list == crate::value::TAG_UNDEFINED || list == crate::value::TAG_NULL
    {
        Vec::new()
    } else if list & !crate::value::POINTER_MASK == crate::value::POINTER_TAG {
        let addr = (list & crate::value::POINTER_MASK) as usize;
        if !crate::value::addr_class::is_above_handle_band(addr) {
            return None;
        }
        match crate::value::addr_class::try_read_gc_header(addr) {
            Some(h) if h.obj_type == crate::gc::GC_TYPE_ARRAY => {
                let arr = addr as *const crate::array::ArrayHeader;
                let n = crate::array::js_array_length(arr);
                (0..n)
                    .map(|i| crate::array::js_array_get_f64(arr, i))
                    .collect()
            }
            Some(h) if h.obj_type == crate::gc::GC_TYPE_OBJECT => {
                crate::object::arguments_object_to_vec(addr as *const crate::object::ObjectHeader)?
            }
            _ => return None,
        }
    } else {
        return None;
    };
    let (ptr, len) = if values.is_empty() {
        (std::ptr::null(), 0)
    } else {
        (values.as_ptr(), values.len())
    };
    Some(enter_compiled_body(closure, info, this, ptr, len))
}

/// Enter a compiled body with `n` arguments: directly, padded with
/// `undefined` to the parameters it declares, when it has no rest parameter
/// and both counts are small (the per-arity dispatcher's direct case);
/// otherwise through the compiled-body call, which bundles rest arguments.
///
/// # Safety
/// `closure` is a live closure on `info`, a compiled body; `args_ptr` holds
/// `n` values.
#[inline]
unsafe fn enter_compiled_body(
    closure: *const crate::closure::ClosureHeader,
    info: &'static crate::closure::JsFunctionInfo,
    this: crate::closure::JsThis,
    args_ptr: *const f64,
    n: usize,
) -> f64 {
    const DIRECT: usize = 6;
    let params = usize::from(info.params);
    if info.flags & crate::closure::FN_REST_MASK != 0 || n > DIRECT || params > DIRECT {
        return crate::closure::call_compiled_body_this(closure, info, this, args_ptr, n);
    }
    let mut a = [f64::from_bits(crate::value::TAG_UNDEFINED); DIRECT];
    for (i, slot) in a.iter_mut().enumerate().take(n) {
        *slot = *args_ptr.add(i);
    }
    let code = info.code;
    use crate::closure::body_call::js_body_call;
    match n.max(params) {
        0 => js_body_call!(code, closure, this),
        1 => js_body_call!(code, closure, this, a[0]),
        2 => js_body_call!(code, closure, this, a[0], a[1]),
        3 => js_body_call!(code, closure, this, a[0], a[1], a[2]),
        4 => js_body_call!(code, closure, this, a[0], a[1], a[2], a[3]),
        5 => js_body_call!(code, closure, this, a[0], a[1], a[2], a[3], a[4]),
        _ => js_body_call!(code, closure, this, a[0], a[1], a[2], a[3], a[4], a[5]),
    }
}

/// A compiled body, an explicit receiver `coerce_call_this` returns
/// unchanged (an object, `undefined`, `null`, or any value for a strict
/// body), and no `this` capture `rebind_explicit_this` would clone.
#[inline(always)]
unsafe fn receiver_passes_unchanged(
    recv: f64,
    closure: *const crate::closure::ClosureHeader,
    info: &crate::closure::JsFunctionInfo,
    this_arg: f64,
) -> bool {
    use crate::closure::{FN_ARROW, FN_STRICT};
    use crate::codegen_abi::FN_COMPILED_BODY;
    if info.flags & FN_COMPILED_BODY == 0 {
        return false;
    }
    // The explicit-`this` forwarder's own boxing test, so a Symbol receiver
    // of a sloppy body is boxed here exactly as on every other path.
    let this_unchanged =
        info.flags & FN_STRICT != 0 || !crate::closure::receiver_may_box(recv, this_arg);
    let count = (*closure).capture_count;
    let rebinds = count
        & (crate::closure::CAPTURES_THIS_FLAG | crate::closure::NO_THIS_REBIND_FLAG)
        == crate::closure::CAPTURES_THIS_FLAG
        && crate::closure::real_capture_count(count) > 0
        && info.flags & FN_ARROW == 0;
    this_unchanged && !rebinds
}
