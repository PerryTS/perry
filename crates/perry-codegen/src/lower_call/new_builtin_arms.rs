//! Built-in / imported-function construction arms of `new X(args...)`.
//!
//! Split out of `new.rs` (file-size gate) - pure move, no behavior change.
//! Runs before the `ctx.classes` lookup in `lower_new_impl_inner`.

use anyhow::Result;
use perry_hir::Expr;

use super::lower_builtin_new;
use crate::expr::{lower_expr, FnCtx};
use crate::nanbox::double_literal;
use crate::rooting::{self, RootedGroup};
use crate::types::DOUBLE;

/// For a `class_name` that is not a user class in `ctx.classes`, lower the
/// built-in Web/Node constructors (and aliased built-in imports). Returns
/// `Ok(None)` when no built-in arm claims the name, so the caller continues
/// with class resolution.
pub(super) fn lower_non_class_new<'a>(
    ctx: &mut FnCtx<'_>,
    class_name: &str,
    args: &'a [Expr],
    group: &mut RootedGroup<'a>,
) -> Result<Option<String>> {
    if matches!(class_name, "Crypto" | "CryptoKey" | "SubtleCrypto") {
        for a in args {
            let _ = lower_expr(ctx, a)?;
        }
        return Ok(Some(ctx.block().call(
            DOUBLE,
            "js_webcrypto_illegal_constructor",
            &[],
        )));
    }
    if let Some((submod_key, exported_name)) =
        ctx.import_function_node_submodule.get(class_name).cloned()
    {
        if submod_key == "readline_promises" && exported_name == "Readline" {
            // #6986: `output` was live in an SSA register across `options`'
            // lowering AND across every `extra`'s — each an arbitrary
            // expression — before `js_readline_promises_readline_new` read
            // it. Same repair as the main class loop below: adopt into the
            // enclosing group as each operand is lowered (never after the
            // list — that publishes an already-dangling pointer, #6969),
            // and re-read at the call.
            //
            // The `undefined` fillers are literals, not operands: nothing
            // to root, and `Arg::Plain` keeps them out of the group so an
            // absent argument still costs no slot.
            let output = match args.first() {
                Some(first) => {
                    let collects = rooting::any_operand_may_collect(ctx, args[1..].iter());
                    Some(group.lower(ctx, first, collects)?)
                }
                None => None,
            };
            let options = match args.get(1) {
                Some(second) => {
                    let collects = rooting::any_operand_may_collect(ctx, args[2..].iter());
                    Some(group.lower(ctx, second, collects)?)
                }
                None => None,
            };
            for extra in args.iter().skip(2) {
                let _ = lower_expr(ctx, extra)?;
            }
            ctx.pending_declares.push((
                "js_readline_promises_readline_new".to_string(),
                DOUBLE,
                vec![DOUBLE, DOUBLE],
            ));
            let undef = double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED));
            let output = match output {
                Some(i) => group.reread(ctx, i)?,
                None => undef.clone(),
            };
            let options = match options {
                Some(i) => group.reread(ctx, i)?,
                None => undef,
            };
            return Ok(Some(ctx.block().call(
                DOUBLE,
                "js_readline_promises_readline_new",
                &[(DOUBLE, &output), (DOUBLE, &options)],
            )));
        }
    }
    if let Some(val) = lower_builtin_new(ctx, class_name, args, group)? {
        return Ok(Some(val));
    }
    // Aliased built-in import: a minified bundle renames a node built-in
    // constructor (`import { AsyncLocalStorage as xQ5 } from "async_hooks";
    // new xQ5()`). The syntactic callee is the alias `xQ5`, so the
    // canonical-name arms in `lower_builtin_new` (keyed on
    // `"AsyncLocalStorage"`) never fired and `new xQ5()` fell through to the
    // empty-object placeholder — the instance had no `.run`/`.getStore`, so
    // `xQ5().getStore()` threw `TypeError: getStore is not a function`.
    // Recover the original export name and retry. The alias is only present
    // here when it was NOT already a user-defined class (the enclosing
    // `!ctx.classes.contains_key(class_name)` guard), so a renamed import
    // can't shadow a real local class.
    if let Some(original) = ctx.imported_class_original_names.get(class_name).cloned() {
        if original != class_name {
            if let Some(val) = lower_builtin_new(ctx, &original, args, group)? {
                return Ok(Some(val));
            }
        }
    }
    Ok(None)
}
