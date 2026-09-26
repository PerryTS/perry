//! The only builder of calls to a JS BODY (`perry_abi::JS_BODY_*`):
//! `double body(i64 callee, double a0, ...)` — a compiled closure body
//! (`perry_closure_*`, its `$generic` / `$trusted_boxes` / versioned-loop
//! clones) or any body reached through a function object's code pointer.
//!
//! Every emitted call of such a body goes through [`emit_js_body_call`] so
//! that a change to the body ABI (the receiver becoming a parameter) is made
//! here and nowhere else. Private typed clones (`$typed_*`, raw-rep
//! parameters) are not JS bodies and are called by their own emitters.
//! `scripts/check_js_body_call_funnel.sh` refuses an indirect call built
//! anywhere else.

use crate::block::LlBlock;
use crate::types::{LlvmType, DOUBLE, I64};

/// What is being called.
#[derive(Clone, Copy)]
pub(crate) enum JsBody<'a> {
    /// A body symbol, without the leading `@`.
    Symbol(&'a str),
    /// A code pointer value (e.g. loaded from a function object or a site).
    Pointer(&'a str),
}

/// The native argument list of a JS body call: the callee, then the JS
/// arguments, in `perry_abi::JS_BODY_*` order.
fn js_body_args<'a>(callee: &'a str, args: &'a [String]) -> Vec<(LlvmType, &'a str)> {
    const _: () = assert!(crate::runtime_abi::JS_BODY_CALLEE_PARAM == 0);
    const _: () = assert!(crate::runtime_abi::JS_BODY_FIRST_ARG_PARAM == 1);
    let mut out: Vec<(LlvmType, &str)> = Vec::with_capacity(args.len() + 1);
    out.push((I64, callee));
    out.extend(args.iter().map(|a| (DOUBLE, a.as_str())));
    out
}

/// Emit `body(callee, args...)` and return the result value.
pub(crate) fn emit_js_body_call(
    blk: &mut LlBlock,
    body: JsBody<'_>,
    callee: &str,
    args: &[String],
) -> String {
    let native = js_body_args(callee, args);
    match body {
        JsBody::Symbol(name) => blk.call(DOUBLE, name, &native),
        JsBody::Pointer(ptr) => blk.call_indirect(DOUBLE, ptr, &native),
    }
}

/// [`emit_js_body_call`] through a code pointer whose caller-side native GC
/// values need not be relocated across the call (`LlBlock::call_indirect_gc_leaf`
/// states when that is sound).
pub(crate) fn emit_js_body_call_gc_leaf(
    blk: &mut LlBlock,
    code_ptr: &str,
    callee: &str,
    args: &[String],
) -> String {
    let native = js_body_args(callee, args);
    blk.call_indirect_gc_leaf(DOUBLE, code_ptr, &native)
}
