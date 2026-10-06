//! The direct-call argument vector for one concrete method implementation.
//!
//! Child module of `dynamic_dispatch.rs`; `use super::*` keeps the parent's
//! imports reachable.

use super::*;

/// Build the exact direct-call ABI for one concrete method implementation.
/// Virtual towers cannot share this vector: sibling overrides may disagree on
/// declared arity, user rest, or the compiler-synthesized `arguments` slot.
///
/// #8162: `has_synthetic_arguments` and `has_rest` alone cannot size the tail.
/// A body with BOTH a user `...rest` and an `arguments` read declares
/// `[a, rest, arguments]` — TWO trailing array slots, bundled from different
/// offsets over the same argument list. `has_user_rest` (false for a class
/// this module has no HIR for, which keeps the one-slot shape those calls
/// already had) is the bit that tells the two-slot case from synth-only.
pub(super) fn build_direct_method_args(
    ctx: &mut FnCtx<'_>,
    recv_box: &str,
    user_args: &[String],
    has_rest: bool,
    has_synthetic_arguments: bool,
    has_user_rest: bool,
    arguments_length_only: bool,
    declared_count: usize,
    undefined_lit: &str,
) -> Vec<String> {
    let mut direct_args = Vec::with_capacity(declared_count + 1);
    direct_args.push(recv_box.to_string());
    if has_synthetic_arguments || has_rest {
        let trailing_slots = if has_synthetic_arguments && has_user_rest {
            2
        } else {
            1
        };
        let fixed_user = declared_count.saturating_sub(trailing_slots);
        for index in 0..fixed_user {
            direct_args.push(
                user_args
                    .get(index)
                    .cloned()
                    .unwrap_or_else(|| undefined_lit.to_string()),
            );
        }
        // (first bundled index, mark as arguments object), in callee param
        // order: the user rest slot first (bundling from `fixed_user`), then
        // the synthesized `arguments` slot (from 0 — it must reflect EVERY
        // passed argument, and gets the marking without which the callee's
        // `arguments` is an ordinary Array). A lone trailing slot is the rest
        // shape unless the method synthesizes `arguments`.
        let mut bundles: Vec<(usize, bool)> = Vec::new();
        if has_user_rest || !has_synthetic_arguments {
            bundles.push((fixed_user, false));
        }
        if has_synthetic_arguments && arguments_length_only {
            debug_assert!(!has_user_rest);
            direct_args.push(double_literal(user_args.len() as f64));
        } else if has_synthetic_arguments {
            bundles.push((0, true));
        }
        for (from, mark) in bundles {
            let count = user_args.len().saturating_sub(from);
            let capacity = (count as u32).to_string();
            let mut bundle = ctx.block().call(I64, "js_array_alloc", &[(I32, &capacity)]);
            for value in user_args.iter().skip(from) {
                let block = ctx.block();
                bundle = block.call(I64, "js_array_push_f64", &[(I64, &bundle), (DOUBLE, value)]);
            }
            if mark {
                let block = ctx.block();
                bundle = block.call(I64, "js_array_mark_arguments_object", &[(I64, &bundle)]);
            }
            direct_args.push(nanbox_pointer_inline(ctx.block(), &bundle));
        }
    } else {
        direct_args.extend(user_args.iter().cloned());
        while direct_args.len() < declared_count + 1 {
            direct_args.push(undefined_lit.to_string());
        }
    }
    direct_args
}
