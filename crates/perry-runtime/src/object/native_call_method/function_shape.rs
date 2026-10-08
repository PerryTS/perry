//! Method calls on a function object, answered from its SHAPE.
//!
//! A closure on a described Function shape (`closure::shape`: base or keyed,
//! not FunctionDictionary) has exactly the own keys its shape lists and
//! inherits from the prototype its shape names. So for a key the list lacks
//! the answer is the prototype's own slot, and the call is decided by that
//! slot's VALUE:
//!
//! 1. the receiver's +4 word is a Function ShapeId naming Function.prototype
//!    whose key list lacks the key (ownership proven by the tracked header);
//! 2. one lookup: the key's data slot on `Function.prototype`;
//! 3. identity by value: the slot still holds the intrinsic `bind` / `call` /
//!    `apply` closure (its code pointer) — then run exactly what the by-name
//!    tower runs for those three (`dispatch_function_proto_method`).
//!
//! Anything else (another value in the slot — a patch —, an accessor, a key
//! the prototype lacks, a FunctionDictionary receiver) is `None`: the caller
//! runs its full path unchanged. No name decides anything; the slot does.
use crate::closure::ClosureHeader;
use crate::value::JSValue;

/// Try the shape-proven path for `object.<name>(args)`.
///
/// # Safety
/// `args_ptr` is valid for `args_len` reads (or null with `args_len == 0`).
#[inline]
pub(crate) unsafe fn try_function_shape_method_call(
    object: f64,
    name: &[u8],
    args_ptr: *const f64,
    args_len: usize,
) -> Option<f64> {
    let bits = object.to_bits();
    if bits >> 48 != 0x7FFD {
        return None;
    }
    let addr = (bits & crate::value::POINTER_MASK) as usize;
    if !crate::value::addr_class::is_above_handle_band(addr)
        || !crate::value::addr_class::is_valid_obj_ptr(addr as *const u8)
        || !addr.is_multiple_of(std::mem::align_of::<ClosureHeader>())
    {
        return None;
    }
    // (1) One compare on the shape word, then prove the cell really is a
    // closure (the word alone is a claim; the tracked header is the proof).
    // The band test first: every non-function receiver (a Map's capacity, an
    // object's ordinary ShapeId) leaves here on one compare, before the
    // agent's base id is even loaded.
    let word = *((addr as *const u8).add(crate::closure::CLOSURE_SHAPE_OFFSET) as *const u32);
    if !crate::object::shapes::is_exotic_shape_id(word) {
        return None;
    }
    if !crate::closure::is_closure_ptr(addr) {
        return None;
    }
    // A DESCRIBED Function shape (base or keyed) whose prototype is
    // Function.prototype, and whose own key list does not hold `name` —
    // then the receiver has no own `name` and inherits it from the prototype.
    if word == crate::closure::shape::function_dictionary_shape() {
        return dictionary_function_proto_method_call(object, addr, name, args_ptr, args_len);
    }
    if !crate::closure::shape::function_shape_inherits_from_function_prototype(word, name) {
        // A class constructor (`C.m()` on a class value): the class arm —
        // statics, callable static data, Function.prototype methods. Reached
        // only once the ordinary function test failed, so no other receiver
        // pays for it.
        return function_shape_decline(object, addr, name, args_ptr, args_len);
    }
    // (2) The prototype the shape names, and its own data slot for the key.
    let proto =
        crate::closure::shape::FUNCTION_PROTOTYPE_PTR.load(std::sync::atomic::Ordering::Acquire);
    if proto == 0 {
        return None;
    }
    let value =
        crate::object::native_get::try_data_get_bytes(JSValue::pointer(proto as *mut u8), name)?;
    // (3) Identity by the slot's value.
    if !value.is_pointer() {
        return patched_slot_call(object, value, name, args_ptr, args_len);
    }
    let method = value.as_pointer::<ClosureHeader>();
    let func = crate::closure::get_valid_func_ptr(method);
    let Some(which) = crate::object::global_this::function_prototype_intrinsic_of(func) else {
        return patched_slot_call(object, value, name, args_ptr, args_len);
    };
    #[cfg(test)]
    FUNCTION_SHAPE_HITS.with(|c| c.set(c.get() + 1));
    super::common_methods::dispatch_function_proto_method(object, which, args_ptr, args_len)
}

/// #11886: the prototype's data slot for `name` holds something other than
/// a `Function.prototype` intrinsic (`Function.prototype.call = f`, written
/// where the compiler could not see it). The slot decides the call, which
/// the by-name tower would not do (it matches the NAME and runs the
/// intrinsic): a function is called as an ordinary method with the function
/// receiver as `this`, and a value that is not callable is the spec's
/// `TypeError`. A no-op-backed built-in method that re-dispatches by this
/// same name is left to the tower: calling it here would re-enter this path
/// (#11700).
#[cold]
#[inline(never)]
unsafe fn patched_slot_call(
    object: f64,
    value: JSValue,
    name: &[u8],
    args_ptr: *const f64,
    args_len: usize,
) -> Option<f64> {
    let value = f64::from_bits(value.bits());
    if !crate::object::value_is_callable(value) {
        crate::closure::throw_not_callable();
    }
    let addr = (value.to_bits() & crate::value::POINTER_MASK) as usize;
    if !JSValue::from_bits(value.to_bits()).is_pointer() || !crate::closure::is_closure_ptr(addr) {
        return None;
    }
    let name = std::str::from_utf8(name).ok()?;
    if super::proto_dispatch::is_self_redispatching_proto_method(value, name) {
        return None;
    }
    #[cfg(test)]
    FUNCTION_SHAPE_PATCHED_CALLS.with(|c| c.set(c.get() + 1));
    let scope = crate::gc::RuntimeHandleScope::new();
    let receiver_h = scope.root_nanbox_f64(object);
    let value_h = scope.root_nanbox_f64(value);
    Some(crate::closure::native_call_value_this(
        value_h.get_nanbox_f64(),
        crate::closure::JsThis::from_f64(receiver_h.get_nanbox_f64()),
        args_ptr,
        args_len,
    ))
}

/// Run the `Function.prototype` intrinsic `which` (`"call"`, `"apply"` or
/// `"bind"`) on the function `object`, once the slot's value has named it.
///
/// # Safety
/// `args_ptr` is valid for `args_len` reads.
#[inline]
pub(crate) unsafe fn run_function_intrinsic(
    object: f64,
    which: &str,
    args_ptr: *const f64,
    args_len: usize,
) -> Option<f64> {
    if which == "call" || which == "apply" {
        if let Some(result) = super::compiled_target::compiled_target_call(
            which == "apply",
            object,
            args_ptr,
            args_len,
        ) {
            return Some(result);
        }
    }
    super::common_methods::dispatch_function_proto_method(object, which, args_ptr, args_len)
}

/// `bind`/`call`/`apply` on a FunctionDictionary receiver — one whose
/// `[[Prototype]]` may be RECORDED (`Object.setPrototypeOf(fn, p)`). The
/// shape answers nothing, so the key is resolved the ordinary way: an own
/// property declines to the full path; otherwise the inherited value comes
/// from the receiver's ACTUAL prototype (`reify_function_method_value`, which
/// reads `getPrototypeOf(fn)`). The intrinsic runs the tower's semantics; any
/// other callable (`p.call`) is invoked with the function as `this`.
/// The shape arm declined an inherited Function.prototype method: a class
/// constructor goes to the class arm; anything else back to the tower. Out of
/// line so the inlined arm stays small.
#[cold]
#[inline(never)]
unsafe fn function_shape_decline(
    object: f64,
    addr: usize,
    name: &[u8],
    args_ptr: *const f64,
    args_len: usize,
) -> Option<f64> {
    if crate::closure::shape::is_class_info((*(addr as *const ClosureHeader)).info) {
        return super::primitive_methods::class_value_method_call(object, name, args_ptr, args_len);
    }
    None
}

unsafe fn dictionary_function_proto_method_call(
    object: f64,
    addr: usize,
    name: &[u8],
    args_ptr: *const f64,
    args_len: usize,
) -> Option<f64> {
    let method: &'static [u8] = match name {
        b"call" => b"call",
        b"apply" => b"apply",
        b"bind" => b"bind",
        _ => return None,
    };
    let key = std::str::from_utf8(method).ok()?;
    if crate::closure::closure_has_own_dynamic_prop(addr, key)
        || crate::object::get_accessor_descriptor(addr, key).is_some()
    {
        return None;
    }
    let scope = crate::gc::RuntimeHandleScope::new();
    let receiver_h = scope.root_nanbox_f64(object);
    let value = crate::closure::reify_function_method_value(object, method);
    let value_h = scope.root_nanbox_f64(value);
    let jv = JSValue::from_bits(value.to_bits());
    if !jv.is_pointer() {
        return None;
    }
    let func = crate::closure::get_valid_func_ptr(jv.as_pointer::<ClosureHeader>());
    if let Some(which) = crate::object::global_this::function_prototype_intrinsic_of(func) {
        return super::common_methods::dispatch_function_proto_method(
            receiver_h.get_nanbox_f64(),
            which,
            args_ptr,
            args_len,
        );
    }
    if func.is_null() {
        return None;
    }
    // A user callable inherited from the recorded prototype: an ordinary
    // method call with the function as `this`.
    Some(crate::closure::native_call_value_this(
        value_h.get_nanbox_f64(),
        crate::closure::JsThis::from_f64(receiver_h.get_nanbox_f64()),
        args_ptr,
        args_len,
    ))
}

#[cfg(test)]
thread_local! {
    /// Calls this path answered (tests prove the path FIRES, not just that
    /// the result is right — the tower gives the same result).
    pub(crate) static FUNCTION_SHAPE_HITS: std::cell::Cell<u64> = const { std::cell::Cell::new(0) };
    /// Calls a patched slot's user function answered (#11886).
    pub(crate) static FUNCTION_SHAPE_PATCHED_CALLS: std::cell::Cell<u64> = const { std::cell::Cell::new(0) };
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::Ordering;

    extern "C" fn target_body(_c: *const ClosureHeader, _this: crate::closure::JsThis) -> f64 {
        42.0
    }

    fn key(s: &str) -> *mut crate::StringHeader {
        crate::string::js_string_from_bytes(s.as_ptr(), s.len() as u32)
    }

    /// A prototype object whose `bind` slot holds a closure over `info` (the
    /// intrinsic thunk or anything else), published as this agent's
    /// Function.prototype.
    unsafe fn install_proto(
        info: *const crate::closure::JsFunctionInfo,
    ) -> *mut crate::object::ObjectHeader {
        let proto = crate::object::js_object_alloc(0, 0);
        let method = crate::closure::js_closure_alloc(info, 0);
        crate::object::js_object_set_field_by_name(
            proto,
            key("bind"),
            crate::value::js_nanbox_pointer(method as i64),
        );
        crate::closure::shape::FUNCTION_PROTOTYPE_PTR.store(proto as i64, Ordering::Release);
        proto
    }

    fn hits() -> u64 {
        FUNCTION_SHAPE_HITS.with(std::cell::Cell::get)
    }

    #[test]
    fn bind_on_a_base_shaped_function_is_answered_by_the_prototype_slot() {
        let _lock = crate::gc::global_side_table_test_lock();
        let _no_gc = crate::gc::GcSuppressScope::new();
        unsafe {
            let saved = crate::closure::shape::FUNCTION_PROTOTYPE_PTR.load(Ordering::Acquire);
            install_proto(crate::object::global_this::function_prototype_bind_thunk_for_test());
            let target = crate::closure::js_closure_alloc(crate::fn_info!(target_body, 0), 0);
            let target_v = crate::value::js_nanbox_pointer(target as i64);
            let this_arg = [f64::from_bits(crate::value::TAG_UNDEFINED)];
            let before = hits();
            let bound = try_function_shape_method_call(target_v, b"bind", this_arg.as_ptr(), 1)
                .expect("the shape path must answer bind on a base-shaped function");
            assert_eq!(hits(), before + 1, "the path must FIRE, not only agree");
            let bound_ptr = JSValue::from_bits(bound.to_bits()).as_pointer::<ClosureHeader>();
            assert_eq!((*bound_ptr).code(), crate::closure::BOUND_FUNCTION_FUNC_PTR);
            assert_eq!(
                crate::closure::js_closure_call0(bound_ptr, crate::closure::plain_call_receiver()),
                42.0
            );
            crate::closure::shape::FUNCTION_PROTOTYPE_PTR.store(saved, Ordering::Release);
        }
    }

    #[test]
    fn a_patched_prototype_slot_runs_and_an_own_key_declines() {
        let _lock = crate::gc::global_side_table_test_lock();
        let _no_gc = crate::gc::GcSuppressScope::new();
        unsafe {
            let saved = crate::closure::shape::FUNCTION_PROTOTYPE_PTR.load(Ordering::Acquire);
            // The slot holds a user function (#11886): identity by VALUE says
            // it is no intrinsic, and the slot decides the call: that
            // function runs, with the receiver as `this`.
            install_proto(crate::fn_info!(target_body, 0));
            let target = crate::closure::js_closure_alloc(crate::fn_info!(target_body, 0), 0);
            let target_v = crate::value::js_nanbox_pointer(target as i64);
            let this_arg = [f64::from_bits(crate::value::TAG_UNDEFINED)];
            let before = hits();
            let patched = FUNCTION_SHAPE_PATCHED_CALLS.with(std::cell::Cell::get);
            assert_eq!(
                try_function_shape_method_call(target_v, b"bind", this_arg.as_ptr(), 1),
                Some(42.0),
                "the patched slot's function must run"
            );
            assert_eq!(
                FUNCTION_SHAPE_PATCHED_CALLS.with(std::cell::Cell::get),
                patched + 1,
                "the patch path must FIRE"
            );

            // The intrinsic is back, but the receiver left its base shape.
            install_proto(crate::object::global_this::function_prototype_bind_thunk_for_test());
            crate::closure::closure_set_dynamic_prop(target as usize, "bind", 1.0);
            assert!(
                try_function_shape_method_call(target_v, b"bind", this_arg.as_ptr(), 1).is_none()
            );
            assert_eq!(hits(), before);
            crate::closure::shape::FUNCTION_PROTOTYPE_PTR.store(saved, Ordering::Release);
        }
    }

    #[test]
    fn a_non_function_receiver_with_a_forged_shape_word_declines() {
        let _lock = crate::gc::global_side_table_test_lock();
        let _no_gc = crate::gc::GcSuppressScope::new();
        unsafe {
            let obj = crate::object::js_object_alloc(0, 0);
            let saved = (*obj).parent_class_id;
            (*obj).parent_class_id = crate::closure::shape::function_base_shape(
                crate::closure::shape::FunctionProtoKind::Function,
            );
            let v = crate::value::js_nanbox_pointer(obj as i64);
            assert!(try_function_shape_method_call(v, b"bind", std::ptr::null(), 0).is_none());
            (*obj).parent_class_id = saved;
        }
    }
}
