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
//! for JS arguments. The parameter is the ONLY way a body learns its
//! receiver: there is no thread-local `this` cell for a caller to set or a
//! body to read.

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

/// Enter a native argument-list body without constructing a rest Array.
/// The body owns rooting any arguments it retains across collection.
pub(crate) fn emit_native_args_body_call(
    blk: &mut LlBlock,
    code: &str,
    callee: &str,
    this_bits: &str,
    args: &str,
    argc: &str,
) -> String {
    blk.call_indirect(
        DOUBLE,
        code,
        &[
            (I64, callee),
            (I64, this_bits),
            (crate::types::PTR, args),
            (I64, argc),
        ],
    )
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

/// Call a site accessor entry's getter through its code address
/// (`perry_abi::PIC_HOLDER_GETTER_WORD`) as `double get(double this, i64
/// pair)`: a compiled class INSTANCE getter, a method body `double get(double
/// this)` that never reads the over-applied pair (the convention the runtime
/// calls it with, `perry-runtime/src/closure/body_call.rs`,
/// `js_method_body_fn!`), or the runtime's closure-getter entry, which calls
/// the getter closure the pair holds. `code_ptr` is a `ptr`, `this_box` the
/// NaN-boxed receiver as a `double`, `pair` the entry's pair word (`i64`).
/// The getter can run any JS: this is a collecting, possibly throwing call.
pub(crate) fn emit_accessor_getter_call(
    blk: &mut LlBlock,
    code_ptr: &str,
    this_box: &str,
    pair: &str,
) -> String {
    blk.call_indirect(DOUBLE, code_ptr, &[(DOUBLE, this_box), (I64, pair)])
}

/// Call a compiled class INSTANCE setter through its code address (a store
/// site's compiled-setter entry, `perry_abi::SETTER_SITE_CODE_OFFSET`): a
/// method body `double set(double this, double v)`, the convention the runtime
/// calls the same entry with. Its result is ignored: the assignment's value
/// is `v`. A collecting, possibly throwing call.
pub(crate) fn emit_class_setter_call(
    blk: &mut LlBlock,
    code_ptr: &str,
    this_box: &str,
    value: &str,
) -> String {
    blk.call_indirect(DOUBLE, code_ptr, &[(DOUBLE, this_box), (DOUBLE, value)])
}

/// The receiver bits for a call whose callee binds `this` to `undefined`
/// (or whose callee is an arrow and never reads it).
pub(crate) const JS_THIS_UNDEFINED: &str = crate::nanbox::TAG_UNDEFINED_I64;

/// The value of `this` in code that has no receiver binding of its own and no
/// `this` slot: module top-level code. `undefined` in strict code; in sloppy
/// code OrdinaryCallBindThis's `undefined -> globalThis`, through the same
/// `js_this_coerce_sloppy` a sloppy body's receiver prologue calls (a
/// safepoint). A function, method or closure body that reads `this` must bind
/// it from its receiver parameter (or a lexical capture) at entry instead: a
/// body reaching this has lost the receiver its caller passed.
pub(crate) fn unbound_this_value(ctx: &mut crate::expr::FnCtx<'_>) -> String {
    let undefined = crate::nanbox::double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED));
    if ctx.is_strict_fn {
        return undefined;
    }
    ctx.block()
        .call(DOUBLE, "js_this_coerce_sloppy", &[(DOUBLE, &undefined)])
}

/// The receiver prologue of a SLOPPY body that reads `this` (stage 2 of
/// this-as-a-parameter): `slot` is the body's rooted entry `this` slot, which
/// already holds the receiver parameter's bits (stored before the body's
/// first safepoint, so a moving collection rewrites it). Applies
/// OrdinaryCallBindThis once, in place: an object receiver is kept inline;
/// anything else (`undefined`, `null`, a primitive, a class ref) goes through
/// `js_this_coerce_sloppy`, the only safepoint here. A strict body needs no
/// prologue: its receiver is the parameter as passed.
pub(crate) fn emit_sloppy_receiver_coercion(ctx: &mut crate::expr::FnCtx<'_>, slot: &str) {
    let (value, is_object) = {
        let blk = ctx.block();
        let value = blk.load(DOUBLE, slot);
        let bits = blk.bitcast_double_to_i64(&value);
        let top16 = blk.lshr(I64, &bits, "48");
        let is_object = blk.icmp_eq(I64, &top16, crate::nanbox::POINTER_TAG_TOP16_I64);
        (value, is_object)
    };
    let slow_idx = ctx.new_block("this_prologue.coerce");
    let done_idx = ctx.new_block("this_prologue.done");
    let slow_label = ctx.block_label(slow_idx);
    let done_label = ctx.block_label(done_idx);
    ctx.block().cond_br(&is_object, &done_label, &slow_label);
    ctx.current_block = slow_idx;
    {
        let blk = ctx.block();
        let coerced = blk.call(DOUBLE, "js_this_coerce_sloppy", &[(DOUBLE, &value)]);
        blk.store(DOUBLE, &coerced, slot);
        blk.br(&done_label);
    }
    ctx.current_block = done_idx;
}

/// The body of a top-level function that reads its dynamic `this`: the
/// function compiled with the receiver as a leading `i64 %js_this`
/// parameter. Its value wrapper passes the receiver it was given; the public
/// `perry_fn_*` symbol every direct call (and every other module) names is a
/// forwarder passing `undefined` — a direct call binds no receiver.
pub(crate) fn receiver_body_name(public_name: &str) -> String {
    format!("{public_name}$this")
}
