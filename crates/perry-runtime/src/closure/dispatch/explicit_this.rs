//! Forwarding a call with an explicit `this`: the one mechanism behind
//! `Function.prototype.call` / `apply` (as values and as methods), bound
//! functions, `Reflect.apply` and generic receiver-bearing value calls.
//!
//! Before the callee runs, two steps can allocate: OrdinaryCallBindThis boxes
//! a primitive receiver for a sloppy callee (`coerce_call_this`), and a
//! concise / object-literal method that keeps `this` in a capture is cloned
//! with the explicit receiver (`rebind_explicit_this`). The callee, the
//! receiver and every argument are live across both. When neither step can
//! allocate (the receiver cannot be boxed and the callee is not a
//! re-bindable `this`-capturing method) the values go straight through;
//! otherwise they are held in handles and re-read after each step.

use super::*;

/// How the receiver is bound before the call.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum ReceiverBinding {
    /// `call` / `apply`: OrdinaryCallBindThis for the callee, so a sloppy
    /// callee sees a primitive receiver boxed.
    Coerce,
    /// The receiver is passed as given: a bound function's was bound by
    /// `bind`, and `Reflect.apply` leaves it to the callee.
    AsGiven,
}

/// A call ready to run: the callee (rebound when it captures `this`), the
/// bound receiver and the arguments, all current. Nothing allocates between
/// [`forward_with_explicit_this`] producing it and the continuation using it.
pub(crate) struct ExplicitThisCall<'a> {
    pub target: f64,
    pub this: f64,
    pub args: &'a [f64],
}

/// Whether binding `this_arg` for `target` can box it. Nullish never boxes.
/// A pointer boxes only when it is a Symbol and the callee is sloppy; the
/// Symbol test runs first, because for an ordinary object it is an inline
/// address-filter miss while the callee test validates the closure. Any other
/// primitive is treated as boxing.
#[inline]
pub(crate) fn receiver_may_box(target: f64, this_arg: f64) -> bool {
    let value = crate::value::JSValue::from_bits(this_arg.to_bits());
    if value.is_undefined() || value.is_null() {
        return false;
    }
    if value.is_pointer() {
        return unsafe { crate::symbol::js_is_symbol(this_arg) } != 0
            && callee_boxes_primitive_this(target);
    }
    true
}

/// Bind the explicit receiver `this_arg` for `target`, rebind a
/// `this`-capturing method, and run `call` with the result. `args` only has
/// to be valid until this returns; it may live in plain Rust memory.
///
/// # Safety
/// `args` holds live JS values.
#[inline]
pub(crate) unsafe fn forward_with_explicit_this<R>(
    target: f64,
    this_arg: f64,
    args: &[f64],
    binding: ReceiverBinding,
    call: impl FnOnce(ExplicitThisCall<'_>) -> R,
) -> R {
    let may_box = binding == ReceiverBinding::Coerce && receiver_may_box(target, this_arg);
    if !may_box && !rebind_explicit_this_allocates(target, this_arg) {
        return call(ExplicitThisCall {
            target,
            this: this_arg,
            args,
        });
    }
    forward_rooted(target, this_arg, args, binding, call)
}

#[cold]
#[inline(never)]
unsafe fn forward_rooted<R>(
    target: f64,
    this_arg: f64,
    args: &[f64],
    binding: ReceiverBinding,
    call: impl FnOnce(ExplicitThisCall<'_>) -> R,
) -> R {
    let scope = crate::gc::RuntimeHandleScope::new();
    let target_h = scope.root_nanbox_f64(target);
    let arg_handles = scope.root_nanbox_f64_slice(args);
    // `coerce_call_this` reads its receiver before it allocates the wrapper.
    let bound_this = match binding {
        ReceiverBinding::Coerce => coerce_call_this(target_h.get_nanbox_f64(), this_arg),
        ReceiverBinding::AsGiven => this_arg,
    };
    let this_h = scope.root_nanbox_f64(bound_this);
    crate::gc::collection_point("explicit_this.coerced");
    // `rebind_explicit_this` roots the callee and receiver it is given.
    let rebound = rebind_explicit_this(target_h.get_nanbox_f64(), this_h.get_nanbox_f64());
    let rebound_h = scope.root_nanbox_f64(rebound);
    crate::gc::collection_point("explicit_this.rebound");
    let args = crate::gc::RuntimeHandleScope::refreshed_nanbox_f64_slice(&arg_handles);
    call(ExplicitThisCall {
        target: rebound_h.get_nanbox_f64(),
        this: this_h.get_nanbox_f64(),
        args: &args,
    })
}

/// [`forward_with_explicit_this`] straight into the generic value call.
///
/// # Safety
/// As [`forward_with_explicit_this`].
pub(crate) unsafe fn call_with_explicit_this(
    target: f64,
    this_arg: f64,
    args: &[f64],
    binding: ReceiverBinding,
) -> f64 {
    forward_with_explicit_this(target, this_arg, args, binding, |call| {
        let args_ptr = if call.args.is_empty() {
            std::ptr::null()
        } else {
            call.args.as_ptr()
        };
        super::value_call::dispatch_explicit_this_call(
            call.target,
            crate::closure::JsThis::from_f64(call.this),
            args_ptr,
            call.args.len(),
        )
    })
}
