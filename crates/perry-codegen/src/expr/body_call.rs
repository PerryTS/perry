//! The only builder of calls to a JS BODY (`perry_abi::JS_BODY_*`):
//! `double body(i64 callee, i64 this, double a0, ...)` — a compiled closure
//! body (`perry_closure_*`, its `$generic` / `$trusted_boxes` /
//! versioned-loop clones), a value wrapper (`__perry_wrap_*`), or any body
//! reached through a function object's code pointer.
//!
//! Every emitted call of such a body goes through [`emit_js_body_call`], and
//! every definition of one declares its parameters through
//! [`js_body_params`], so the body ABI is spelled here and nowhere else.
//! Private typed clones (`$typed_*`, raw-rep parameters) are not JS bodies
//! and are called by their own emitters. `scripts/check_js_body_call_funnel.py`
//! refuses an indirect call built anywhere else.
//!
//! The receiver (`this`) is passed as NaN-boxed bits in an INTEGER register
//! (owner decision D1), so every floating-point argument register stays free
//! for JS arguments. Stage 1 of this-as-a-parameter: a caller passes exactly
//! the receiver the implicit-`this` cell holds for the call, and bodies still
//! read the cell (`PERRY_THIS_WITNESS=1` builds check that the two agree).

use crate::block::LlBlock;
use crate::types::{LlvmType, DOUBLE, I64};

/// The callee parameter of every JS body definition (`perry_abi::JS_BODY_CALLEE_PARAM`).
pub(crate) const JS_BODY_CALLEE: &str = "%this_closure";
/// The receiver parameter of every JS body definition
/// (`perry_abi::JS_BODY_THIS_PARAM`): NaN-boxed `this` bits, `i64`.
pub(crate) const JS_BODY_THIS: &str = "%js_this";

const _: () = assert!(crate::runtime_abi::JS_BODY_CALLEE_PARAM == 0);
const _: () = assert!(crate::runtime_abi::JS_BODY_THIS_PARAM == 1);
const _: () = assert!(crate::runtime_abi::JS_BODY_FIRST_ARG_PARAM == 2);
const _: () = assert!(crate::runtime_abi::JS_BODY_FIXED_PARAMS == 2);

/// The native parameter list of a JS body DEFINITION: the callee, the
/// receiver, then one `double` per JS parameter name given.
pub(crate) fn js_body_params<I, S>(js_params: I) -> Vec<(LlvmType, String)>
where
    I: IntoIterator<Item = S>,
    S: Into<String>,
{
    let mut out = vec![
        (I64, JS_BODY_CALLEE.to_string()),
        (I64, JS_BODY_THIS.to_string()),
    ];
    out.extend(js_params.into_iter().map(|name| (DOUBLE, name.into())));
    out
}

/// The native parameter TYPES of a JS body taking `js_arity` JS arguments,
/// for a declaration of a body defined elsewhere.
pub(crate) fn js_body_param_types(js_arity: usize) -> Vec<LlvmType> {
    let mut out = vec![I64, I64];
    out.extend(std::iter::repeat_n(DOUBLE, js_arity));
    out
}

/// What is being called.
#[derive(Clone, Copy)]
pub(crate) enum JsBody<'a> {
    /// A body symbol, without the leading `@`.
    Symbol(&'a str),
    /// A code pointer value (e.g. loaded from a function object or a site).
    Pointer(&'a str),
}

/// The native argument list of a JS body call: the callee, the receiver
/// bits, then the JS arguments, in `perry_abi::JS_BODY_*` order.
fn js_body_args<'a>(
    callee: &'a str,
    this_bits: &'a str,
    args: &'a [String],
) -> Vec<(LlvmType, &'a str)> {
    let mut out: Vec<(LlvmType, &str)> = Vec::with_capacity(args.len() + 2);
    out.push((I64, callee));
    out.push((I64, this_bits));
    out.extend(args.iter().map(|a| (DOUBLE, a.as_str())));
    out
}

/// Emit `body(callee, this, args...)` and return the result value.
/// `this_bits` is an `i64` value (NaN-boxed receiver bits).
pub(crate) fn emit_js_body_call(
    blk: &mut LlBlock,
    body: JsBody<'_>,
    callee: &str,
    this_bits: &str,
    args: &[String],
) -> String {
    let native = js_body_args(callee, this_bits, args);
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
    this_bits: &str,
    args: &[String],
) -> String {
    let native = js_body_args(callee, this_bits, args);
    blk.call_indirect_gc_leaf(DOUBLE, code_ptr, &native)
}

/// The receiver bits for a call whose callee binds `this` to `undefined`
/// (and whose implicit-`this` cell the caller set to `undefined`, or whose
/// callee is an arrow and never reads it).
pub(crate) const JS_THIS_UNDEFINED: &str = crate::nanbox::TAG_UNDEFINED_I64;

/// The receiver bits the implicit-`this` cell holds right now: what a callee
/// entered here would read from the cell. For a direct call whose caller
/// does not bind the cell (stage 1 passes the cell's value unchanged).
pub(crate) fn current_this_bits(ctx: &mut crate::expr::FnCtx<'_>) -> String {
    if let Some(cell) = crate::rooting::implicit_this_cell_ptr(ctx) {
        return ctx.block().load(I64, &cell);
    }
    let blk = ctx.block();
    let value = blk.call(DOUBLE, "js_implicit_this_get", &[]);
    blk.bitcast_double_to_i64(&value)
}

/// Whether this compile emits the stage-1 witness (`PERRY_THIS_WITNESS=1`, a
/// COMPILE-time knob — a product build emits nothing and pays nothing).
pub(crate) fn this_witness_enabled() -> bool {
    std::env::var_os("PERRY_THIS_WITNESS").is_some_and(|v| v == "1")
}

/// The site-name constant a witness call names its body by, added to
/// `llmod` BEFORE the body is defined (a definition borrows the module):
/// `Some((global, byte_len))` in a witness build, `None` otherwise.
pub(crate) fn this_witness_site(
    llmod: &mut crate::module::LlModule,
    site: &str,
) -> Option<(String, usize)> {
    this_witness_enabled().then(|| llmod.add_string_constant(site))
}

/// In a witness build (`site` from [`this_witness_site`]), emit
/// `js_this_param_witness(%js_this, site, len)` at the entry of a body that
/// reads the implicit-`this` cell: the runtime compares the parameter with
/// the cell and names every body whose caller passed a different receiver.
/// A GC leaf. Nothing when `site` is `None`.
pub(crate) fn emit_this_param_witness(blk: &mut LlBlock, site: Option<&(String, usize)>) {
    let Some((global, len)) = site else {
        return;
    };
    blk.call_void(
        "js_this_param_witness",
        &[
            (I64, JS_BODY_THIS),
            (crate::types::PTR, &format!("@{global}")),
            (I64, &len.to_string()),
        ],
    );
}
