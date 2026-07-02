//! `Stmt::If` lowering.

use std::collections::{HashMap, HashSet};

use super::*;

use crate::lower_conditional::lower_truthy;
use crate::native_value::NativeRep;

#[derive(Clone)]
struct NativeArenaOwnerAliasSnapshot {
    known: HashMap<u32, u32>,
    ambiguous: HashSet<u32>,
}

#[derive(Clone, Copy, Eq, PartialEq)]
enum NativeArenaOwnerAliasState {
    Known(u32),
    Ambiguous,
    None,
}

impl NativeArenaOwnerAliasSnapshot {
    fn capture(ctx: &FnCtx<'_>) -> Self {
        Self {
            known: ctx.native_arena_owner_aliases.clone(),
            ambiguous: ctx.native_arena_ambiguous_owner_aliases.clone(),
        }
    }

    fn restore(&self, ctx: &mut FnCtx<'_>) {
        ctx.native_arena_owner_aliases = self.known.clone();
        ctx.native_arena_ambiguous_owner_aliases = self.ambiguous.clone();
    }

    fn state_for(&self, id: u32) -> NativeArenaOwnerAliasState {
        if let Some(owner_id) = self.known.get(&id).copied() {
            NativeArenaOwnerAliasState::Known(owner_id)
        } else if self.ambiguous.contains(&id) {
            NativeArenaOwnerAliasState::Ambiguous
        } else {
            NativeArenaOwnerAliasState::None
        }
    }
}

fn merge_native_arena_owner_aliases(ctx: &mut FnCtx<'_>, exits: &[NativeArenaOwnerAliasSnapshot]) {
    if exits.len() == 1 {
        exits[0].restore(ctx);
        return;
    }

    let mut known = HashMap::new();
    let mut ambiguous = HashSet::new();
    let mut ids = HashSet::new();
    for exit in exits {
        ids.extend(exit.known.keys().copied());
        ids.extend(exit.ambiguous.iter().copied());
    }

    for id in ids {
        let first = exits
            .first()
            .map(|exit| exit.state_for(id))
            .unwrap_or(NativeArenaOwnerAliasState::None);
        if exits.iter().all(|exit| exit.state_for(id) == first) {
            match first {
                NativeArenaOwnerAliasState::Known(owner_id) => {
                    known.insert(id, owner_id);
                }
                NativeArenaOwnerAliasState::Ambiguous => {
                    ambiguous.insert(id);
                }
                NativeArenaOwnerAliasState::None => {}
            }
        } else {
            ambiguous.insert(id);
        }
    }

    ctx.native_arena_owner_aliases = known;
    ctx.native_arena_ambiguous_owner_aliases = ambiguous;
}

/// If-else lowering using explicit then/else/merge blocks.
///
/// Truthiness uses `lower_truthy` which dispatches to either an inline
/// `fcmp one cond, 0.0` (statically-numeric conditions) or a runtime
/// `js_is_truthy` call (NaN-boxed booleans, strings, objects, unions).
/// Try to evaluate a condition at compile time using known constants.
/// Returns `Some(true)` or `Some(false)` if the condition can be folded,
/// `None` if it depends on runtime values.
fn try_const_fold_condition(ctx: &FnCtx<'_>, condition: &perry_hir::Expr) -> Option<bool> {
    use perry_hir::{CompareOp, Expr, LogicalOp};
    match condition {
        Expr::Compare { op, left, right } => {
            // Try to extract a known constant from one side and a literal
            // from the other.
            let (const_val, literal_val) = match (left.as_ref(), right.as_ref()) {
                (Expr::LocalGet(id), Expr::Integer(n)) => {
                    (ctx.compile_time_constants.get(id)?, *n as f64)
                }
                (Expr::Integer(n), Expr::LocalGet(id)) => {
                    (ctx.compile_time_constants.get(id)?, *n as f64)
                }
                (Expr::LocalGet(id), Expr::Number(n)) => (ctx.compile_time_constants.get(id)?, *n),
                (Expr::Number(n), Expr::LocalGet(id)) => (ctx.compile_time_constants.get(id)?, *n),
                _ => return None,
            };
            let c = *const_val;
            Some(match op {
                CompareOp::Eq | CompareOp::LooseEq => c == literal_val,
                CompareOp::Ne | CompareOp::LooseNe => c != literal_val,
                CompareOp::Lt => c < literal_val,
                CompareOp::Le => c <= literal_val,
                CompareOp::Gt => c > literal_val,
                CompareOp::Ge => c >= literal_val,
            })
        }
        Expr::Logical { op, left, right } => {
            let l = try_const_fold_condition(ctx, left)?;
            match op {
                LogicalOp::And => {
                    if !l {
                        Some(false)
                    } else {
                        try_const_fold_condition(ctx, right)
                    }
                }
                LogicalOp::Or => {
                    if l {
                        Some(true)
                    } else {
                        try_const_fold_condition(ctx, right)
                    }
                }
                LogicalOp::Coalesce => None,
            }
        }
        _ => None,
    }
}

pub(crate) fn lower_if(
    ctx: &mut FnCtx<'_>,
    condition: &perry_hir::Expr,
    then_branch: &[Stmt],
    else_branch: Option<&[Stmt]>,
) -> Result<()> {
    // Compile-time constant folding: when the condition involves only
    // known constants (e.g., `__platform__ === 1`), skip the dead branch
    // entirely. This prevents emitting `declare`/`call` instructions for
    // extern FFI functions that only exist on other platforms.
    if let Some(is_true) = try_const_fold_condition(ctx, condition) {
        if is_true {
            lower_stmts(ctx, then_branch)?;
        } else if let Some(else_stmts) = else_branch {
            lower_stmts(ctx, else_stmts)?;
        }
        return Ok(());
    }

    let i1 = lower_if_condition_i1(ctx, condition)?;
    let alias_entry_snapshot = NativeArenaOwnerAliasSnapshot::capture(ctx);

    let then_idx = ctx.new_block("if.then");
    let else_idx = ctx.new_block("if.else");
    let merge_idx = ctx.new_block("if.merge");

    let then_label = ctx.block_label(then_idx);
    let else_label = ctx.block_label(else_idx);
    let merge_label = ctx.block_label(merge_idx);

    // Emit the branch in the incoming current block.
    ctx.block().cond_br(&i1, &then_label, &else_label);

    // Compile then branch.
    ctx.current_block = then_idx;
    let guard_scope_id = ctx.next_loop_proof_scope_id();
    let guarded = crate::expr::guarded_buffer_indices_for_condition(ctx, condition, guard_scope_id);
    ctx.guarded_buffer_index_pairs.extend(guarded);
    lower_stmts(ctx, then_branch)?;
    ctx.guarded_buffer_index_pairs
        .retain(|fact| fact.scope_id != guard_scope_id);
    let then_aliases = NativeArenaOwnerAliasSnapshot::capture(ctx);
    let then_reaches_merge = !ctx.block().is_terminated();
    if then_reaches_merge {
        ctx.block().br(&merge_label);
    }

    // Compile else branch. If there's no explicit else, the else block is
    // still created so both sides of the condBr have a valid target — it
    // just branches immediately to merge.
    alias_entry_snapshot.restore(ctx);
    ctx.current_block = else_idx;
    if let Some(else_stmts) = else_branch {
        lower_stmts(ctx, else_stmts)?;
    }
    let else_aliases = NativeArenaOwnerAliasSnapshot::capture(ctx);
    let else_reaches_merge = !ctx.block().is_terminated();
    if else_reaches_merge {
        ctx.block().br(&merge_label);
    }

    let mut alias_exits = Vec::new();
    if then_reaches_merge {
        alias_exits.push(then_aliases);
    }
    if else_reaches_merge {
        alias_exits.push(else_aliases);
    }
    merge_native_arena_owner_aliases(ctx, &alias_exits);

    // Continue emitting subsequent statements into the merge block.
    ctx.current_block = merge_idx;
    Ok(())
}

fn lower_if_condition_i1(ctx: &mut FnCtx<'_>, condition: &perry_hir::Expr) -> Result<String> {
    if let Some(lowered) = lower_expr_value(ctx, condition)? {
        if matches!(lowered.rep, NativeRep::I1) {
            return Ok(lowered.value);
        }
        let boxed = materialize_js_value(ctx, lowered, MaterializationReason::RuntimeApi);
        return Ok(lower_truthy(ctx, &boxed, condition));
    }

    let cond_val = lower_expr(ctx, condition)?;
    Ok(lower_truthy(ctx, &cond_val, condition))
}
