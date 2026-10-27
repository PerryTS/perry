//! #7524: constructing a builtin reached through a VARIABLE ALIAS.
//!
//! `const ET = EventTarget; new ET()` produced an instance with no surface —
//! `typeof inst.addEventListener === "undefined"`. The direct `new EventTarget()`
//! form is lowered by codegen straight to the factory, so only the indirect
//! shapes were wrong: the alias routes through the globalThis value, whose
//! closure is the shared `global_this_builtin_noop_thunk`. That thunk allocates
//! a bare object and never stamps the class id or attaches the per-kind state,
//! so the #6301 prototype-chain fallback had nothing to resolve against.
//!
//! Each arm dispatches to the same factory the direct form uses.
//!
//! `FormData` is deliberately absent: it is owned by perry-stdlib, which this
//! crate cannot call into, so it needs a registered dispatch hook rather than an
//! arm here. Subclassing (`class A extends AbortController {}`) is also out of
//! scope — a native base installs its surface through a separate, per-builtin
//! mechanism, which `EventTarget` has and the others do not (still open on
//! #7524).
//!
//! The `Map`/`Set`/`WeakMap`/`WeakSet`/`WeakRef` arms moved here verbatim from
//! `construct.rs`: they are the same category (a builtin constructed from a
//! value rather than by name), and `construct.rs` sat one line under the
//! 2000-line CI cap, so it had no room for the new arms.
use super::super::global_this_tables::BuiltinConstructorIdentity;

/// Names this module constructs. Kept beside `construct` so the match in
/// `construct.rs` and the arms here cannot drift apart.
#[allow(dead_code)] // consumed only when the indirect-constructor surface is enabled
pub(crate) fn handles(name: BuiltinConstructorIdentity) -> bool {
    matches!(
        name,
        BuiltinConstructorIdentity::EventTarget
            | BuiltinConstructorIdentity::AbortController
            | BuiltinConstructorIdentity::TextEncoder
            | BuiltinConstructorIdentity::URLSearchParams
            | BuiltinConstructorIdentity::DisposableStack
            | BuiltinConstructorIdentity::WeakRef
    )
}

/// Construct `name` with `args`. Only called for names `handles` accepts.
#[allow(dead_code)] // paired with `handles` above
pub(crate) fn construct(name: BuiltinConstructorIdentity, args: &[f64]) -> f64 {
    match name {
        BuiltinConstructorIdentity::EventTarget => {
            let target = crate::event_target::js_event_target_new();
            return crate::value::js_nanbox_pointer(target as i64);
        }
        BuiltinConstructorIdentity::AbortController => {
            let controller = crate::event_target::abort::js_abort_controller_new();
            return crate::value::js_nanbox_pointer(controller as i64);
        }
        BuiltinConstructorIdentity::TextEncoder => {
            // Stateless: a non-null sentinel, NaN-boxed with POINTER_TAG so
            // `typeof enc === "object"` holds (mirrors `Expr::TextEncoderNew`).
            return crate::value::js_nanbox_pointer(crate::text::js_text_encoder_new());
        }
        BuiltinConstructorIdentity::URLSearchParams => {
            let init = args.first().copied();
            let init_str = match init {
                Some(v) if crate::value::JSValue::from_bits(v.to_bits()).is_any_string() => {
                    crate::value::js_get_string_pointer_unified(v) as *mut crate::StringHeader
                }
                _ => std::ptr::null_mut(),
            };
            let params = crate::url_hooks::url_search_params_new(init_str);
            return crate::value::js_nanbox_pointer(params as i64);
        }
        BuiltinConstructorIdentity::DisposableStack => {
            let stack = crate::disposable::js_disposable_stack_new();
            return crate::value::js_nanbox_pointer(stack as i64);
        }
        BuiltinConstructorIdentity::WeakRef => {
            let target = args
                .first()
                .copied()
                .unwrap_or_else(|| f64::from_bits(crate::value::TAG_UNDEFINED));
            let wr = crate::weakref::js_weakref_new(target);
            return crate::value::js_nanbox_pointer(wr as i64);
        }
        _ => f64::from_bits(crate::value::TAG_UNDEFINED),
    }
}
