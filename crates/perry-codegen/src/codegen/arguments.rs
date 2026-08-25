use std::collections::HashSet;

use perry_hir::{Expr, Param, Stmt};

use crate::block::LlBlock;
use crate::expr::{nanbox_pointer_inline, FnCtx};
use crate::nanbox::double_literal;
use crate::types::{DOUBLE, I32, I64, PTR};

pub(crate) enum ArgumentsCallee<'a> {
    Undefined,
    FunctionWrapper(&'a str),
    CurrentClosure,
}

pub(crate) fn add_arguments_mapped_boxes(params: &[Param], boxed_vars: &mut HashSet<u32>) {
    for (_, param_id) in mapped_arguments_params(params) {
        boxed_vars.insert(param_id);
    }
}

pub(crate) fn store_param_slot(
    blk: &mut LlBlock,
    param: &Param,
    boxed_vars: &HashSet<u32>,
    arg_name: &str,
) -> String {
    let boxed_param = boxed_vars.contains(&param.id) && param.arguments_object.is_none();
    let slot = blk.alloca(if boxed_param { I64 } else { DOUBLE });
    if boxed_param {
        let arg_bits = blk.bitcast_double_to_i64(arg_name);
        let box_ptr = blk.call(I64, "js_box_alloc_bits", &[(I64, &arg_bits)]);
        blk.store(I64, &box_ptr, &slot);
    } else {
        blk.store(DOUBLE, arg_name, &slot);
    }
    slot
}

pub(crate) fn materialize_arguments_object(
    ctx: &mut FnCtx<'_>,
    params: &[Param],
    body: Option<&[Stmt]>,
    callee: ArgumentsCallee<'_>,
) {
    let Some(synth_param) = params.iter().find(|p| p.arguments_object.is_some()) else {
        return;
    };
    // Call lowering has already bundled every supplied argument into the
    // synthesized slot as a marked Array. When the only observable operation
    // is `arguments.length`, that bundle has exactly the required value and a
    // full ECMAScript Arguments object would only add allocation, mapped-index
    // setup, and GC pressure. Keep the existing conservative materialization
    // path for every other use (including callers that cannot provide a body).
    if body.is_some_and(|body| arguments_used_only_for_length(body, synth_param.id)) {
        return;
    }
    let Some(meta) = synth_param.arguments_object.as_ref() else {
        return;
    };
    let Some(arguments_slot) = ctx.locals.get(&synth_param.id).cloned() else {
        return;
    };
    let restricted = if meta.restricted_callee { "1" } else { "0" };
    let raw_args = ctx.block().load(DOUBLE, &arguments_slot);
    let callee_value = if meta.restricted_callee {
        double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED))
    } else {
        match callee {
            ArgumentsCallee::Undefined => {
                double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED))
            }
            ArgumentsCallee::FunctionWrapper(wrapper) => {
                let wrap_ref = format!("@{}", wrapper);
                let closure_ptr =
                    ctx.block()
                        .call(I64, "js_closure_alloc_singleton", &[(PTR, &wrap_ref)]);
                nanbox_pointer_inline(ctx.block(), &closure_ptr)
            }
            ArgumentsCallee::CurrentClosure => {
                // #7055: through the shadow-rooted slot when there is one — the
                // raw `%this_closure` register is not a GC root.
                let ptr = crate::expr::try_current_closure_ptr_value(ctx)
                    .unwrap_or_else(|| "%this_closure".to_string());
                nanbox_pointer_inline(ctx.block(), &ptr)
            }
        }
    };
    let args_obj = ctx.block().call(
        I64,
        "js_arguments_object_alloc",
        &[
            (DOUBLE, &raw_args),
            (DOUBLE, &callee_value),
            (I32, restricted),
        ],
    );
    for (arg_index, param_id) in mapped_arguments_params(params) {
        if let Some(param_slot) = ctx.locals.get(&param_id).cloned() {
            let box_ptr = ctx.block().load(I64, &param_slot);
            ctx.block().call_void(
                "js_arguments_object_map_index",
                &[
                    (I64, &args_obj),
                    (I32, &arg_index.to_string()),
                    (I64, &box_ptr),
                ],
            );
        }
    }
    let boxed_args = nanbox_pointer_inline(ctx.block(), &args_obj);
    ctx.block().store(DOUBLE, &boxed_args, &arguments_slot);
}

/// Prove that replacing the synthesized Arguments object with its raw argument
/// bundle cannot be observed. The proof is deliberately fail-closed: erase
/// exact `arguments.length` reads from a cloned body, then ask HIR's canonical
/// local-reference collector whether *any* reference to the synthetic local
/// remains. That collector knows about specialized local-bearing expressions
/// such as `ArrayPop(id)` in addition to ordinary `LocalGet` nodes.
fn arguments_used_only_for_length(body: &[Stmt], arguments_id: u32) -> bool {
    let mut stripped = body.to_vec();
    let mut length_reads = 0usize;
    for stmt in &mut stripped {
        erase_arguments_length_reads_stmt(stmt, arguments_id, &mut length_reads);
    }
    if length_reads == 0 {
        return false;
    }

    let mut refs = Vec::new();
    let mut visited_closures = HashSet::new();
    for stmt in &stripped {
        perry_hir::collect_local_refs_stmt(stmt, &mut refs, &mut visited_closures);
    }
    !refs.contains(&arguments_id)
}

fn erase_arguments_length_reads_stmt(stmt: &mut Stmt, arguments_id: u32, reads: &mut usize) {
    match stmt {
        Stmt::Let { init, .. } => {
            if let Some(init) = init {
                erase_arguments_length_reads_expr(init, arguments_id, reads);
            }
        }
        Stmt::Expr(expr) | Stmt::Throw(expr) => {
            erase_arguments_length_reads_expr(expr, arguments_id, reads);
        }
        Stmt::Return(expr) => {
            if let Some(expr) = expr {
                erase_arguments_length_reads_expr(expr, arguments_id, reads);
            }
        }
        Stmt::If {
            condition,
            then_branch,
            else_branch,
        } => {
            erase_arguments_length_reads_expr(condition, arguments_id, reads);
            erase_arguments_length_reads_stmts(then_branch, arguments_id, reads);
            if let Some(else_branch) = else_branch {
                erase_arguments_length_reads_stmts(else_branch, arguments_id, reads);
            }
        }
        Stmt::While { condition, body } => {
            erase_arguments_length_reads_expr(condition, arguments_id, reads);
            erase_arguments_length_reads_stmts(body, arguments_id, reads);
        }
        Stmt::DoWhile { body, condition } => {
            erase_arguments_length_reads_stmts(body, arguments_id, reads);
            erase_arguments_length_reads_expr(condition, arguments_id, reads);
        }
        Stmt::For {
            init,
            condition,
            update,
            body,
        } => {
            if let Some(init) = init {
                erase_arguments_length_reads_stmt(init, arguments_id, reads);
            }
            if let Some(condition) = condition {
                erase_arguments_length_reads_expr(condition, arguments_id, reads);
            }
            if let Some(update) = update {
                erase_arguments_length_reads_expr(update, arguments_id, reads);
            }
            erase_arguments_length_reads_stmts(body, arguments_id, reads);
        }
        Stmt::Labeled { body, .. } => {
            erase_arguments_length_reads_stmt(body, arguments_id, reads);
        }
        Stmt::Try {
            body,
            catch,
            finally,
        } => {
            erase_arguments_length_reads_stmts(body, arguments_id, reads);
            if let Some(catch) = catch {
                erase_arguments_length_reads_stmts(&mut catch.body, arguments_id, reads);
            }
            if let Some(finally) = finally {
                erase_arguments_length_reads_stmts(finally, arguments_id, reads);
            }
        }
        Stmt::Switch {
            discriminant,
            cases,
        } => {
            erase_arguments_length_reads_expr(discriminant, arguments_id, reads);
            for case in cases {
                if let Some(test) = &mut case.test {
                    erase_arguments_length_reads_expr(test, arguments_id, reads);
                }
                erase_arguments_length_reads_stmts(&mut case.body, arguments_id, reads);
            }
        }
        Stmt::Break
        | Stmt::Continue
        | Stmt::LabeledBreak(_)
        | Stmt::LabeledContinue(_)
        | Stmt::PreallocateBoxes(_)
        | Stmt::PreallocateTdzBoxes(_)
        | Stmt::ReleaseBoxes(_) => {}
    }
}

fn erase_arguments_length_reads_stmts(stmts: &mut [Stmt], arguments_id: u32, reads: &mut usize) {
    for stmt in stmts {
        erase_arguments_length_reads_stmt(stmt, arguments_id, reads);
    }
}

fn erase_arguments_length_reads_expr(expr: &mut Expr, arguments_id: u32, reads: &mut usize) {
    if matches!(
        expr,
        Expr::PropertyGet {
            object,
            property,
            ..
        } if property == "length" && matches!(object.as_ref(), Expr::LocalGet(id) if *id == arguments_id)
    ) {
        *expr = Expr::Undefined;
        *reads += 1;
        return;
    }

    // The shared expression walker visits closure parameter defaults but, by
    // design, not closure bodies. Visit the body explicitly so arrows that
    // capture an outer `arguments` binding participate in the same proof.
    if let Expr::Closure { body, .. } = expr {
        erase_arguments_length_reads_stmts(body, arguments_id, reads);
    }
    perry_hir::walker::walk_expr_children_mut(expr, &mut |child| {
        erase_arguments_length_reads_expr(child, arguments_id, reads)
    });
}

fn mapped_arguments_params(params: &[Param]) -> Vec<(u32, u32)> {
    params
        .iter()
        .filter_map(|p| p.arguments_object.as_ref())
        .flat_map(|meta| meta.mapped_parameter_ids.iter().copied())
        .collect()
}

#[cfg(test)]
mod length_only_tests {
    use super::arguments_used_only_for_length;
    use perry_hir::{Expr, Stmt};

    const ARGUMENTS: u32 = 17;

    fn length() -> Expr {
        Expr::PropertyGet {
            object: Box::new(Expr::LocalGet(ARGUMENTS)),
            property: "length".to_string(),
            byte_offset: 0,
        }
    }

    #[test]
    fn accepts_exact_length_reads_at_arbitrary_depth() {
        let body = vec![Stmt::Return(Some(Expr::Binary {
            op: perry_hir::BinaryOp::Add,
            left: Box::new(Expr::Integer(1)),
            right: Box::new(length()),
        }))];
        assert!(arguments_used_only_for_length(&body, ARGUMENTS));
    }

    #[test]
    fn rejects_identity_index_and_mixed_uses() {
        assert!(!arguments_used_only_for_length(
            &[Stmt::Return(Some(Expr::LocalGet(ARGUMENTS)))],
            ARGUMENTS
        ));
        assert!(!arguments_used_only_for_length(
            &[Stmt::Return(Some(Expr::IndexGet {
                object: Box::new(Expr::LocalGet(ARGUMENTS)),
                index: Box::new(Expr::Integer(0)),
            }))],
            ARGUMENTS
        ));
        assert!(!arguments_used_only_for_length(
            &[
                Stmt::Expr(length()),
                Stmt::Return(Some(Expr::LocalGet(ARGUMENTS))),
            ],
            ARGUMENTS
        ));
    }

    #[test]
    fn rejects_specialized_local_bearing_operations() {
        let body = vec![
            Stmt::Expr(length()),
            Stmt::Return(Some(Expr::ArrayPop(ARGUMENTS))),
        ];
        assert!(!arguments_used_only_for_length(&body, ARGUMENTS));
    }
}

/// Does `property`, resolved against `class_name`'s ancestry, declare a USER
/// `...rest` parameter — as opposed to (or in addition to) the trailing
/// `arguments` slot #677 synthesizes?
///
/// #8040/#8162. Both spellings lower as `Param { is_rest: true }`, so
/// `method_has_rest` is true for either, and `method_has_synthetic_arguments`
/// only names the synthesized slot — the PAIR still cannot distinguish
/// "synthesized `arguments` only" from "user rest AND synthesized `arguments`",
/// and those fill a different number of trailing slots (`m(a, ...rest)` with an
/// `arguments` read is `[a, rest, arguments]`: TWO arrays, from two offsets).
/// The discriminator is `arguments_object`, which the synthesized parameter
/// carries and nothing else does.
///
/// Read off the class HIR, so a class the current module has no HIR for (an
/// imported class) reports `false`, leaving those call sites on the
/// one-trailing-slot behavior they had — `method_has_synthetic_arguments`
/// still covers the imported synth-only shape via its interface bit.
pub(crate) fn method_has_user_rest(
    ctx: &crate::expr::FnCtx<'_>,
    class_name: &str,
    property: &str,
) -> bool {
    let mut walk = Some(class_name.to_string());
    while let Some(cur) = walk {
        let class = ctx.classes.get(&cur);
        if let Some(f) = class.and_then(|c| c.methods.iter().find(|m| m.name == *property)) {
            return f
                .params
                .iter()
                .any(|p| p.is_rest && p.arguments_object.is_none());
        }
        walk = class.and_then(|c| c.extends_name.clone());
    }
    false
}
