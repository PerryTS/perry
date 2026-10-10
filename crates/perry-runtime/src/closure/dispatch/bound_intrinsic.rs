//! A bound call/apply adapter's internal-slot layouts. Resolve the intrinsic
//! and its function operand once, at bind birth. The original target remains
//! in slot 0 for name/length/construct semantics; slot 5 is the call operand.
//! The immutable info distinguishes these layouts from an ordinary bind.
use super::*;
use crate::value::JSValue;

pub(in crate::closure) static CALL_INFO: JsFunctionInfo =
    unsafe { JsFunctionInfo::from_code(BOUND_FUNCTION_FUNC_PTR, 0) };
pub(in crate::closure) static APPLY_INFO: JsFunctionInfo =
    unsafe { JsFunctionInfo::from_code(BOUND_FUNCTION_FUNC_PTR, 0) };

#[derive(Clone, Copy)]
pub(super) struct Resolved {
    pub info: &'static JsFunctionInfo,
}

/// Only a body reached through its ordinary ABI can eliminate the adapter.
/// Exotic targets and captured-this methods keep the shared forwarder.
pub(super) unsafe fn resolve(
    adapter: *const JsFunctionInfo,
    operand: &crate::gc::RuntimeHandle<'_>,
    partials: usize,
) -> Option<Resolved> {
    if partials != 0 {
        return None;
    }
    let which = crate::object::function_prototype_intrinsic_of(adapter.as_ref()?.code)?;
    let info = match which {
        "call" => &CALL_INFO,
        "apply" => &APPLY_INFO,
        _ => return None,
    };
    let operand_value = JSValue::from_bits(operand.get_nanbox_f64().to_bits());
    if !operand_value.is_pointer() {
        return None;
    }
    let operand = operand_value.as_pointer::<ClosureHeader>();
    if !crate::closure::is_closure_ptr(operand as usize)
        || crate::closure::closure_reads_this_from_capture(operand)
    {
        return None;
    }
    let body = (*operand).info.as_ref()?;
    let code = body.code;
    if code.is_null()
        || code == BOUND_FUNCTION_FUNC_PTR
        || code == BOUND_METHOD_FUNC_PTR
        || code == crate::object::global_this_builtin_noop_thunk as *const u8
        || code == crate::object::global_this_array_thunk as *const u8
        || crate::closure::shape::is_class_info((*operand).info)
    {
        return None;
    }
    Some(Resolved { info })
}

/// Only resolved adapters have this six-slot birth. Keep it separate from
/// ordinary bind's five-slot allocation and installer, including their frame.
/// Resolution excludes partial arguments, so slot 2 is always empty.
#[cold]
#[inline(never)]
pub(super) unsafe fn allocate(
    resolved: Resolved,
    target: &crate::gc::RuntimeHandle<'_>,
    operand: &crate::gc::RuntimeHandle<'_>,
    name: &crate::gc::RuntimeHandle<'_>,
    length: u64,
) -> *mut ClosureHeader {
    let bound = js_closure_alloc(resolved.info, super::bound::BOUND_FUNCTION_CAPTURES + 1)
        as *mut ClosureHeader;
    // The allocation may move all three roots. Read them only afterwards.
    let target = target.get_nanbox_f64().to_bits();
    let operand = operand.get_nanbox_f64().to_bits();
    let name = name.get_nanbox_f64().to_bits();
    crate::closure::closure_install_boxed_captures(
        bound,
        &[target, operand, 0, name, length, operand],
    );
    bound
}

pub(super) unsafe fn dispatch(closure: *const ClosureHeader, args: &[f64]) -> Option<f64> {
    let info = (*closure).info;
    let apply = if std::ptr::eq(info, &CALL_INFO) {
        false
    } else if std::ptr::eq(info, &APPLY_INFO) {
        true
    } else {
        return None;
    };
    let target = js_closure_get_capture_f64(closure, 5);
    let this_arg = args
        .first()
        .copied()
        .unwrap_or(f64::from_bits(crate::value::TAG_UNDEFINED));
    let mut packed = [0.0; 4];
    let call_args = if apply {
        let list = args
            .get(1)
            .copied()
            .unwrap_or(f64::from_bits(crate::value::TAG_UNDEFINED));
        let Some(n) = packed_apply_values(list, &mut packed) else {
            // Getters, proxies, holes and wide lists retain CreateListFromArrayLike.
            // The original apply adapter is still slot 0; generic bind dispatch
            // will enter it with the function operand in slot 1.
            return None;
        };
        &packed[..n]
    } else {
        args.get(1..).unwrap_or(&[])
    };
    let operand = JSValue::from_bits(target.to_bits()).as_pointer::<ClosureHeader>();
    let body = &*(*operand).info;
    if crate::closure::info_receives_primitive_this(body)
        || !super::explicit_this::receiver_may_box(target, this_arg)
    {
        return Some(call_resolved_body(
            operand,
            body,
            JsThis::from_f64(this_arg),
            call_args,
        ));
    }
    Some(super::explicit_this::forward_with_explicit_this(
        target,
        this_arg,
        call_args,
        super::explicit_this::ReceiverBinding::Coerce,
        |call| {
            let closure = JSValue::from_bits(call.target.to_bits()).as_pointer::<ClosureHeader>();
            call_resolved_body(
                closure,
                &*(*closure).info,
                JsThis::from_f64(call.this),
                call.args,
            )
        },
    ))
}

/// Bind birth proved that this operand needs no captured-receiver rebind.
/// Keep its direct entry within this layout; general ConstFn callers use the
/// existing forwarding path and may still carry a captured receiver.
#[inline]
unsafe fn call_resolved_body(
    closure: *const ClosureHeader,
    info: &'static JsFunctionInfo,
    this: JsThis,
    args: &[f64],
) -> f64 {
    if info.flags & crate::closure::FN_REST_MASK == 0 && args.len() <= 6 && info.params <= 6 {
        let mut a = [f64::from_bits(crate::value::TAG_UNDEFINED); 6];
        a[..args.len()].copy_from_slice(args);
        use crate::closure::body_call::js_body_call;
        let code = info.code;
        #[cfg(target_os = "wasi")]
        let width = usize::from(info.params);
        #[cfg(not(target_os = "wasi"))]
        let width = args.len().max(usize::from(info.params));
        return match width {
            0 => js_body_call!(code, closure, this),
            1 => js_body_call!(code, closure, this, a[0]),
            2 => js_body_call!(code, closure, this, a[0], a[1]),
            3 => js_body_call!(code, closure, this, a[0], a[1], a[2]),
            4 => js_body_call!(code, closure, this, a[0], a[1], a[2], a[3]),
            5 => js_body_call!(code, closure, this, a[0], a[1], a[2], a[3], a[4]),
            _ => js_body_call!(code, closure, this, a[0], a[1], a[2], a[3], a[4], a[5]),
        };
    }
    super::value_call::call_compiled_body_this(closure, info, this, args.as_ptr(), args.len())
}

/// Apply reads own indexed data; it never consults the iterator protocol.
/// An ordinary Array's existing descriptor/prototype flags and present packed
/// elements prove these Gets cannot execute user code. Everything else uses
/// CreateListFromArrayLike through the original adapter.
unsafe fn packed_apply_values(value: f64, out: &mut [f64; 4]) -> Option<usize> {
    let value = JSValue::from_bits(value.to_bits());
    if value.is_undefined() || value.is_null() {
        return Some(0);
    }
    if !value.is_pointer() {
        return None;
    }
    let array = crate::array::clean_arr_ptr(value.as_pointer::<crate::array::ArrayHeader>());
    if array.is_null()
        || crate::array::array_receiver_gc_tag(array).0 != crate::gc::GC_TYPE_ARRAY
        || !crate::array::array_has_plain_shape_resolved(array)
    {
        return None;
    }
    let count = (*array).length as usize;
    if count > out.len() {
        return None;
    }
    let elements = crate::array::array_elements_ptr(array) as *const u64;
    for (i, slot) in out.iter_mut().enumerate().take(count) {
        let bits = *elements.add(i);
        if bits == crate::value::TAG_HOLE {
            return None;
        }
        // GC_STORE_AUDIT(STACK): caller stack, copied without a safepoint.
        *slot = f64::from_bits(bits);
    }
    Some(count)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::object::shapes::{shape_object_kind_by_id, ShapeObjectKind};

    extern "C" fn body(_c: *const ClosureHeader, this: JsThis, a: f64, b: f64) -> f64 {
        this.as_f64() + a + b
    }

    #[test]
    fn bind_birth_types_the_operand_and_forwarding_allocates_no_closures() {
        let _lock = crate::gc::global_side_table_test_lock();
        let _triggers = crate::gc::GcTriggerThresholdTestGuard::suppress_automatic_triggers();
        let profiling =
            crate::promise::MT_PROFILE_ENABLED.swap(true, std::sync::atomic::Ordering::Relaxed);
        let target = js_closure_alloc(crate::fn_info!(body, 2; with_flags(FN_STRICT)), 0);
        let target_value = crate::value::js_nanbox_pointer(target as i64);
        let adapter_value =
            unsafe { crate::closure::reify_function_method_value(target_value, b"call") };
        let args = [target_value];
        let bound = unsafe { js_function_bind(adapter_value, args.as_ptr(), 1) };
        let closure = JSValue::from_bits(bound.to_bits()).as_pointer::<ClosureHeader>();
        assert_eq!(
            shape_object_kind_by_id(unsafe { (*closure).shape_id }),
            Some(ShapeObjectKind::FunctionBoundCall)
        );
        assert_eq!(unsafe { (*closure).capture_count }, 6);
        assert_eq!(
            js_closure_get_capture_f64(closure, 5).to_bits(),
            target_value.to_bits()
        );
        let before = crate::closure::CLOSURE_ALLOC_COUNT.load(std::sync::atomic::Ordering::Relaxed);
        assert!(before > 0, "allocation counting must be live");
        let heap_bytes = crate::arena::arena_live_allocated_bytes();
        for _ in 0..1000 {
            assert_eq!(unsafe { dispatch(closure, &[10.0, 2.0, 3.0]) }, Some(15.0));
        }
        assert_eq!(
            crate::closure::CLOSURE_ALLOC_COUNT.load(std::sync::atomic::Ordering::Relaxed),
            before
        );
        assert_eq!(crate::arena::arena_live_allocated_bytes(), heap_bytes);
        crate::promise::MT_PROFILE_ENABLED.store(profiling, std::sync::atomic::Ordering::Relaxed);
        // Internal-slot schema also survives a keyed Function shape transition.
        crate::closure::closure_set_dynamic_prop(closure as usize, "extra", 1.0);
        assert_eq!(
            shape_object_kind_by_id(unsafe { (*closure).shape_id }),
            Some(ShapeObjectKind::FunctionBoundCall)
        );
        assert_eq!(unsafe { dispatch(closure, &[10.0, 2.0, 3.0]) }, Some(15.0));
    }

    #[test]
    fn bound_apply_snapshots_packed_values_on_the_stack() {
        let _lock = crate::gc::global_side_table_test_lock();
        let _triggers = crate::gc::GcTriggerThresholdTestGuard::suppress_automatic_triggers();
        let profiling =
            crate::promise::MT_PROFILE_ENABLED.swap(true, std::sync::atomic::Ordering::Relaxed);
        let target = js_closure_alloc(crate::fn_info!(body, 2; with_flags(FN_STRICT)), 0);
        let target_value = crate::value::js_nanbox_pointer(target as i64);
        let apply = crate::object::js_get_global_this();
        let _ = apply; // materialize intrinsic Function.prototype
        let adapter =
            unsafe { crate::closure::reify_function_method_value(target_value, b"apply") };
        let bound = unsafe { js_function_bind(adapter, &target_value, 1) };
        let closure = JSValue::from_bits(bound.to_bits()).as_pointer::<ClosureHeader>();
        assert_eq!(
            shape_object_kind_by_id(unsafe { (*closure).shape_id }),
            Some(ShapeObjectKind::FunctionBoundApply)
        );
        let array = crate::array::js_array_alloc(2);
        unsafe {
            crate::array::js_array_push_f64(array, 2.0);
            crate::array::js_array_push_f64(array, 3.0);
        }
        let list = crate::value::js_nanbox_pointer(array as i64);
        let before = crate::closure::CLOSURE_ALLOC_COUNT.load(std::sync::atomic::Ordering::Relaxed);
        assert!(before > 0, "allocation counting must be live");
        let heap_bytes = crate::arena::arena_live_allocated_bytes();
        for _ in 0..1000 {
            assert_eq!(unsafe { dispatch(closure, &[10.0, list]) }, Some(15.0));
        }
        assert_eq!(
            crate::closure::CLOSURE_ALLOC_COUNT.load(std::sync::atomic::Ordering::Relaxed),
            before
        );
        assert_eq!(crate::arena::arena_live_allocated_bytes(), heap_bytes);
        crate::promise::MT_PROFILE_ENABLED.store(profiling, std::sync::atomic::Ordering::Relaxed);
    }
}
