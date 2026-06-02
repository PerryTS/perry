//! Options-object lowering: the "look at this options object literal and
//! decide what to do" family.
//!
//! Extracted from `lower_call.rs` (#1099, part of #1097) — pure move,
//! no behavior change. `extract_options_fields` is the shared core
//! extractor (plain `Expr::Object` *or* Phase-3 `__AnonShape_` synthesis);
//! `get_raw_string_ptr` / `build_headers_from_object` are the shared
//! field helpers. The per-native-API option-lowering surfaces live in
//! sub-modules, split by API at clean function boundaries:
//!
//! - `notification.rs` — perry/system `notificationSchedule({...})`
//! - `abort.rs`         — `AbortController` / `AbortSignal`
//! - `fetch.rs`         — Web Fetch family (fetch / axios / Headers /
//!                        Request / Response / blob / readable_stream)
//!
//! All three sub-modules + the core helpers are re-exported from the
//! parent `lower_call` module so the existing `super::<name>` /
//! `crate::lower_call::<name>` call sites keep resolving unchanged.

use anyhow::Result;
use perry_hir::Expr;

use crate::expr::{lower_expr, FnCtx};
use crate::types::{DOUBLE, I64};

mod abort;
mod fetch;
mod notification;
pub(in crate::lower_call) use abort::lower_abort_controller_call;
pub(in crate::lower_call) use fetch::lower_fetch_native_method;
pub(in crate::lower_call) use notification::lower_notification_schedule;

/// Extract a raw string pointer (i64) from a NaN-boxed JSValue via the
/// unified helper. Handles string literals, concat results, and any
/// other expression that produces a NaN-boxed double.
pub(in crate::lower_call) fn get_raw_string_ptr(ctx: &mut FnCtx<'_>, e: &Expr) -> Result<String> {
    let v = lower_expr(ctx, e)?;
    let blk = ctx.block();
    Ok(blk.call(I64, "js_get_string_pointer_unified", &[(DOUBLE, &v)]))
}

/// Build a Headers handle from an inline object literal `{ "k": "v", ... }`.
/// Returns the f64 handle (raw numeric, not NaN-boxed).
pub(in crate::lower_call) fn build_headers_from_object(
    ctx: &mut FnCtx<'_>,
    props: &[(String, Expr)],
) -> Result<String> {
    let h = ctx.block().call(DOUBLE, "js_headers_new", &[]);
    for (k, vexpr) in props {
        let key_expr = Expr::String(k.clone());
        let key_ptr = get_raw_string_ptr(ctx, &key_expr)?;
        let value = lower_expr(ctx, vexpr)?;
        let val_ptr = ctx
            .block()
            .call(I64, "js_jsvalue_to_string", &[(DOUBLE, &value)]);
        ctx.block().call(
            DOUBLE,
            "js_headers_set",
            &[(DOUBLE, &h), (I64, &key_ptr), (I64, &val_ptr)],
        );
    }
    Ok(h)
}

/// Phase 3 compat: extract `{key: value, ...}` pairs from an options
/// argument in a form that works whether the options literal reached us
/// as a plain `Expr::Object(props)` (pre-Phase-3 / spread/dynamic shapes)
/// or as an `Expr::New { class_name: "__AnonShape_N", args }` (Phase 3's
/// closed-shape synthesis path). For the anon-class form, `ctx.classes`
/// carries the class with its synthesized constructor — we pair each
/// constructor param name with its positional arg to recover the literal's
/// (key, value) view.
///
/// Returns `None` when the expression is neither shape — callers should
/// fall through to whatever they did before when the 2nd arg wasn't an
/// inline object.
pub(crate) fn extract_options_fields(ctx: &FnCtx<'_>, e: &Expr) -> Option<Vec<(String, Expr)>> {
    match e {
        Expr::Object(props) => Some(props.clone()),
        Expr::LocalGet(id) => ctx.option_object_locals.get(id).cloned(),
        Expr::New {
            class_name, args, ..
        } if class_name.starts_with("__AnonShape_") => {
            let class = ctx.classes.get(class_name)?;
            let ctor = class.constructor.as_ref()?;
            if ctor.params.len() != args.len() {
                return None;
            }
            let pairs: Vec<(String, Expr)> = ctor
                .params
                .iter()
                .zip(args.iter())
                .map(|(param, arg)| (param.name.clone(), arg.clone()))
                .collect();
            Some(pairs)
        }
        _ => None,
    }
}
