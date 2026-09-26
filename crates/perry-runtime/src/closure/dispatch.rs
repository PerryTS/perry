//! Closure dispatch: per-arity `js_closure_callN` entry points,
//! validation (`get_valid_func_ptr`), the not-callable error path,
//! `js_native_call_value`, and the V8 trampoline bridges
//! `js_closure_call_array` / `js_closure_call_apply_with_spread`.
//!
//! The implementation is split across sibling modules under `dispatch/`:
//! - `bound`: bound-method/bound-function dispatch + `Function.prototype.bind`
//! - `errors`: the not-callable throw path + #922 circuit breaker
//! - `validate`: closure-pointer validation (`get_valid_func_ptr`, GC stubs)
//! - `calln`: per-arity `js_closure_callN` FFI entry points
//! - `direct`: hoisted per-arity dispatch for callback loops (#8180)
//! - `value_call`: the dynamic value-call / V8-trampoline / spread bridges

use super::*;

mod bound;
mod calln;
mod direct;
mod errors;
mod validate;
mod value_call;

pub(crate) use bound::{
    bound_function_lazy_name, bound_method_source_func_ptr, coerce_call_this, rebind_explicit_this,
    rebind_explicit_this_allocates, reify_function_method_value,
};
pub use bound::{dispatch_bound_function, dispatch_bound_method, js_function_bind};

pub(crate) use errors::reset_throw_not_callable_counter;
pub use errors::throw_not_callable;

pub use validate::{clean_closure_ptr, dispatch_proxy_callee_or_throw, get_valid_func_ptr};

pub use calln::{
    js_closure_call0, js_closure_call1, js_closure_call10, js_closure_call11, js_closure_call12,
    js_closure_call13, js_closure_call14, js_closure_call15, js_closure_call16,
    js_closure_call1_receiverless, js_closure_call2, js_closure_call3, js_closure_call4,
    js_closure_call5, js_closure_call6, js_closure_call7, js_closure_call8, js_closure_call9,
};
pub use direct::{DirectCall1, DirectCall2, DirectCall3, DirectCall4};

pub use value_call::{
    js_closure_call_apply_with_spread, js_closure_call_array, js_native_call_value,
};

/// Whether a body declaring `declared` parameters must go through
/// `dispatch_with_arity` instead of the caller's `n`-argument signature.
///
/// On native C ABIs a body simply reads the first `declared` of `n` arguments,
/// so only a shortfall (`declared > n`) needs the padded path. A wasm
/// `call_indirect` must match its target's type exactly (a mismatch traps), so
/// on WASI any difference goes through the body's own signature, which drops
/// the surplus arguments (#11379).
#[inline(always)]
pub(crate) const fn arity_needs_dispatch(declared: u32, n: u32) -> bool {
    #[cfg(not(target_os = "wasi"))]
    {
        declared > n
    }
    #[cfg(target_os = "wasi")]
    {
        declared != n
    }
}

/// The arity to dispatch a body with: its registered arity, or on WASI its
/// real parameter count (see `registry::wasi_exact_arity`).
#[inline(always)]
pub(crate) fn dispatch_arity(func_ptr: *const u8) -> Option<u32> {
    #[cfg(target_os = "wasi")]
    if let Some(params) = super::registry::wasi_body_params(func_ptr) {
        return Some(params);
    }
    lookup_closure_arity(func_ptr)
}
