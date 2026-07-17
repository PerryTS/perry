//! Post-construction capture write-back.
//!
//! After a `new ClassName(args…)` or `Reflect.construct(…)` that inlines a
//! constructor which may mutate captured outer locals, this module reads the
//! updated value back from the instance's `__perry_cap_<id>` fields and stores
//! it to the outer local's LLVM alloca slot so the mutation is visible to the
//! caller.

use crate::expr::FnCtx;
use crate::nanbox::POINTER_MASK_I64;
use crate::types::{DOUBLE, I32, I64};

/// After construction of a class whose constructor may have mutated captured
/// outer locals (via `++captured_var`, `captured_var = x`, etc.), read back
/// each `__perry_cap_<id>` field from the freshly-constructed instance and
/// store the updated value to the outer local's LLVM slot.
///
/// ECMAScript requires shared-binding semantics for captured variables: a
/// class nested inside a function shares the SAME binding as the outer scope.
/// Perry's capture mechanism passes the captured value as an extra constructor
/// param (by value), so `++called` inside the constructor only updates the
/// constructor-local copy. After construction completes, this helper reads the
/// mutated value back from `this.__perry_cap_*` and writes it to the outer
/// local's alloca slot, making the mutation visible to the caller.
///
/// `obj_handle` is the raw i64 object pointer (not nanboxed).
///
/// `new_args` is the full `args` slice from the HIR `New` node — the LAST
/// `n_cap_params` elements are the cap args in the same order as the
/// constructor's `__perry_cap_*` params.  Used to resolve the *current*-scope
/// local ID for each capture, which may differ from the numeric suffix in
/// `__perry_cap_<id>` when the class was defined inside a function that was
/// later inlined into a different scope (where local IDs are alpha-renamed).
pub(crate) fn emit_class_capture_writeback(
    ctx: &mut FnCtx<'_>,
    class: &perry_hir::Class,
    obj_handle: &str,
    new_args: &[perry_hir::Expr],
) {
    // Cap params are synthesized in `synthesize_class_captures` as extra
    // constructor params with name `__perry_cap_<outer_id>`. They are NOT in
    // `class.fields` — they are PropertySet stmts on `this` in the ctor body.
    // Iterate ctor params to find which outer locals were captured.
    let Some(ctor) = class.constructor.as_ref() else {
        return;
    };
    // Collect the cap params (those with __perry_cap_ prefix) in declaration
    // order so we can index them positionally against new_args.
    let cap_params: Vec<_> = ctor
        .params
        .iter()
        .filter(|p| p.name.starts_with("__perry_cap_"))
        .collect();
    // The cap args occupy the last cap_params.len() slots of new_args.
    let cap_args_start = new_args.len().saturating_sub(cap_params.len());

    for (cap_idx, param) in cap_params.iter().enumerate() {
        let Some(name_outer_id) = perry_hir::cap_fields::cap_field_outer_id(&param.name) else {
            continue;
        };
        // Prefer position-based lookup using new_args: the arg at index
        // cap_args_start + cap_idx is a LocalGet whose id is the
        // *current-scope* id — correct even when the class was inlined into a
        // different scope and local IDs were alpha-renamed.
        let current_scope_id: Option<u32> = new_args
            .get(cap_args_start + cap_idx)
            .and_then(|arg| {
                if let perry_hir::Expr::LocalGet(id) = arg {
                    Some(*id)
                } else {
                    None
                }
            })
            // Fallback to the numeric id parsed from the cap name for
            // call sites that don't supply new_args (empty slice).
            .or(Some(name_outer_id));
        let Some(outer_id) = current_scope_id else {
            continue;
        };
        // Only write back to locals that are actually in scope (same-function
        // construction). Cross-module construction has no accessible outer local.
        let Some(outer_slot) = ctx.locals.get(&outer_id).cloned() else {
            continue;
        };
        // Read the updated capture value from the instance field.
        let field_name = &param.name;
        let key_idx = ctx.strings.intern(field_name);
        let key_handle_global = format!("@{}", ctx.strings.entry(key_idx).handle_global);
        let key_box = ctx.block().load(DOUBLE, &key_handle_global);
        let key_bits = ctx.block().bitcast_double_to_i64(&key_box);
        let key_handle = ctx.block().and(I64, &key_bits, POINTER_MASK_I64);
        let val = ctx.block().call(
            DOUBLE,
            "js_object_get_field_by_name_f64",
            &[(I64, obj_handle), (I64, &key_handle)],
        );
        // Store the updated value. Handle boxed locals (shared across multiple
        // closures) via js_box_set; plain locals via a direct slot store.
        // `outer_id` here is the current-scope id (resolved via new_args or
        // the __perry_cap_ suffix), so boxed_vars / i32_counter_slots lookups
        // correctly resolve to the current context's tracking structures.
        if ctx.boxed_vars.contains(&outer_id) {
            let box_dbl = ctx.block().load(DOUBLE, &outer_slot);
            let box_ptr = ctx.block().bitcast_double_to_i64(&box_dbl);
            ctx.block()
                .call_void("js_box_set", &[(I64, &box_ptr), (DOUBLE, &val)]);
        } else {
            ctx.block().store(DOUBLE, &val, &outer_slot);
            // If this local also has an i32 fast-path slot (counter / integer
            // local), keep it in sync. Use fptosi→i64→trunc→i32 to handle
            // unsigned values safely (direct fptosi→i32 is UB above INT32_MAX).
            if let Some(i32_slot) = ctx.i32_counter_slots.get(&outer_id).cloned() {
                let v_i64 = ctx.block().fptosi(DOUBLE, &val, crate::types::I64);
                let v_i32 = ctx.block().trunc(crate::types::I64, &v_i64, I32);
                ctx.block().store(I32, &v_i32, &i32_slot);
            }
        }
    }
}
