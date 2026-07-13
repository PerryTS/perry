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
//! - `value_call`: the dynamic value-call / V8-trampoline / spread bridges

use super::*;

mod bound;
mod calln;
mod errors;
mod validate;
mod value_call;

pub(crate) use bound::{coerce_call_this, rebind_explicit_this, reify_function_method_value};
pub use bound::{dispatch_bound_function, dispatch_bound_method, js_function_bind};

pub(crate) use errors::reset_throw_not_callable_counter;
pub use errors::throw_not_callable;

pub use validate::{clean_closure_ptr, dispatch_proxy_callee_or_throw, get_valid_func_ptr};

pub(crate) use calln::{
    dispatch_registered_call, dispatch_rest_or_declared_arity, resolve_call2_direct,
};
pub use calln::{
    js_closure_call0, js_closure_call1, js_closure_call10, js_closure_call11, js_closure_call12,
    js_closure_call13, js_closure_call14, js_closure_call15, js_closure_call16, js_closure_call2,
    js_closure_call3, js_closure_call4, js_closure_call5, js_closure_call6, js_closure_call7,
    js_closure_call8, js_closure_call9,
};

pub use value_call::{
    js_closure_call_apply_with_spread, js_closure_call_array, js_native_call_value,
};
