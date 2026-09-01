//! `new <ErrorSubclass>(msg)` — the spec default Error-init for a class with
//! no own constructor whose ancestor walk terminates at a native Error family
//! base (#573).
//!
//! Split out of `new.rs` to keep that file under the 2,000-line CI gate
//! (`scripts/check_file_size.sh`); the body is a pure move, plus the #9410
//! `stack` install.

use perry_hir::Class;

use crate::expr::FnCtx;
use crate::nanbox::POINTER_MASK_I64;
use crate::types::{DOUBLE, I64};

/// Stamp `message`, `name` and `stack` onto the freshly allocated instance of
/// an Error-family subclass, mirroring the `SuperCall` Error-like arm in
/// `expr/this_super_call.rs`.
///
/// Returns `true` when the class's `extends` chain does terminate at an Error
/// family base and the init was emitted — the caller then skips its
/// imported-ctor fallback. Returns `false` (emitting nothing) otherwise.
pub(super) fn emit_default_error_init(
    ctx: &mut FnCtx,
    class: &Class,
    lowered_args: &[String],
) -> bool {
    // Trace the chain to find the first Error-like ancestor name.
    let mut error_kind: Option<String> = None;
    let mut cur = class.extends_name.clone();
    let mut depth = 0usize;
    while let Some(pname) = cur {
        if matches!(
            pname.as_str(),
            "Error"
                | "TypeError"
                | "RangeError"
                | "ReferenceError"
                | "SyntaxError"
                | "URIError"
                | "EvalError"
                | "AggregateError"
        ) {
            error_kind = Some(pname);
            break;
        }
        cur = ctx
            .classes
            .get(pname.as_str())
            .and_then(|c| c.extends_name.clone());
        depth += 1;
        if depth > 32 {
            break;
        }
    }
    if let Some(kind) = error_kind {
        let this_slot_for_err = ctx.this_stack.last().cloned().unwrap_or_default();
        let blk = ctx.block();
        let this_box = blk.load(DOUBLE, &this_slot_for_err);
        let this_bits = blk.bitcast_double_to_i64(&this_box);
        let this_handle = blk.and(I64, &this_bits, POINTER_MASK_I64);
        if let Some(msg_val) = lowered_args.first() {
            let key_idx = ctx.strings.intern("message");
            let key_handle_global =
                format!("@{}", ctx.strings.entry(key_idx).handle_global);
            let blk = ctx.block();
            let key_box = blk.load(DOUBLE, &key_handle_global);
            let key_bits = blk.bitcast_double_to_i64(&key_box);
            let key_raw = blk.and(I64, &key_bits, POINTER_MASK_I64);
            // Spec: built-in Error sets `message` non-enumerable via
            // DefinePropertyOrThrow (Test262 NativeError/*-message).
            blk.call_void(
                "js_object_set_field_by_name_nonenum",
                &[(I64, &this_handle), (I64, &key_raw), (DOUBLE, msg_val)],
            );
        }
        let name_idx = ctx.strings.intern("name");
        let name_handle_global = format!("@{}", ctx.strings.entry(name_idx).handle_global);
        let name_val_idx = ctx.strings.intern(&kind);
        let name_val_global = format!("@{}", ctx.strings.entry(name_val_idx).handle_global);
        let blk = ctx.block();
        let name_key_box = blk.load(DOUBLE, &name_handle_global);
        let name_key_bits = blk.bitcast_double_to_i64(&name_key_box);
        let name_key_raw = blk.and(I64, &name_key_bits, POINTER_MASK_I64);
        let name_val_box = blk.load(DOUBLE, &name_val_global);
        blk.call_void(
            "js_object_set_field_by_name",
            &[
                (I64, &this_handle),
                (I64, &name_key_raw),
                (DOUBLE, &name_val_box),
            ],
        );
        // #9410: `stack`. This arm stamps `message` and `name` onto an
        // ordinary class instance; nothing ever filled `stack`, so
        // `new MyError("x").stack` was `undefined` where the base
        // `new Error("x").stack` is a string. The runtime installs a
        // lazily-formatted own accessor and captures the FRAME here,
        // at the construction site.
        let blk = ctx.block();
        // Reload `this`: the `message`/`name` stamps above can
        // collect, so the earlier `this_box` may be stale (#8770).
        let this_for_stack = blk.load(DOUBLE, &this_slot_for_err);
        blk.call_void(
            "js_error_subclass_capture_stack",
            &[(DOUBLE, &this_for_stack)],
        );
        return true;
    }
    false
}
