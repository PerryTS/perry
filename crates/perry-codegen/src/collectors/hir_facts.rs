use perry_hir::{Expr, Stmt};
use std::collections::HashSet;

/// Reusable HIR facts consumed by hot-loop lowering paths.
///
/// This keeps integer, bounded-index, and helper-return facts in one place
/// instead of making every codegen entry point rediscover the same source
/// shapes independently.
pub(crate) struct HirFacts {
    pub integer_locals: HashSet<u32>,
    pub index_used_locals: HashSet<u32>,
    pub strictly_i32_bounded_locals: HashSet<u32>,
    pub known_noalias_buffer_locals: HashSet<u32>,
}

pub(crate) fn collect_hir_facts(
    stmts: &[Stmt],
    flat_const_ids: &HashSet<u32>,
    clamp_fn_ids: &HashSet<u32>,
) -> HirFacts {
    let integer_locals =
        super::integer_locals::collect_integer_locals(stmts, flat_const_ids, clamp_fn_ids);
    let index_used_locals = super::index_uses::collect_index_used_locals(stmts);
    let strictly_i32_bounded_locals = super::i32_locals::collect_strictly_i32_bounded_locals(
        stmts,
        &integer_locals,
        flat_const_ids,
        clamp_fn_ids,
    );
    let known_noalias_buffer_locals = collect_known_noalias_buffer_locals(stmts);
    HirFacts {
        integer_locals,
        index_used_locals,
        strictly_i32_bounded_locals,
        known_noalias_buffer_locals,
    }
}

fn collect_known_noalias_buffer_locals(stmts: &[Stmt]) -> HashSet<u32> {
    let mut out = HashSet::new();
    collect_owned_buffer_lets(stmts, &mut out);
    out
}

fn collect_owned_buffer_lets(stmts: &[Stmt], out: &mut HashSet<u32>) {
    for stmt in stmts {
        match stmt {
            Stmt::Let {
                id,
                mutable,
                init: Some(init),
                ..
            } => {
                if !*mutable && is_owned_u8_buffer_alloc(init) {
                    out.insert(*id);
                }
            }
            Stmt::If {
                then_branch,
                else_branch,
                ..
            } => {
                collect_owned_buffer_lets(then_branch, out);
                if let Some(else_branch) = else_branch {
                    collect_owned_buffer_lets(else_branch, out);
                }
            }
            Stmt::While { body, .. } | Stmt::DoWhile { body, .. } => {
                collect_owned_buffer_lets(body, out);
            }
            Stmt::For { init, body, .. } => {
                if let Some(init) = init {
                    collect_owned_buffer_lets(std::slice::from_ref(init.as_ref()), out);
                }
                collect_owned_buffer_lets(body, out);
            }
            Stmt::Labeled { body, .. } => {
                collect_owned_buffer_lets(std::slice::from_ref(body.as_ref()), out);
            }
            Stmt::Try {
                body,
                catch,
                finally,
            } => {
                collect_owned_buffer_lets(body, out);
                if let Some(catch) = catch {
                    collect_owned_buffer_lets(&catch.body, out);
                }
                if let Some(finally) = finally {
                    collect_owned_buffer_lets(finally, out);
                }
            }
            Stmt::Switch { cases, .. } => {
                for case in cases {
                    collect_owned_buffer_lets(&case.body, out);
                }
            }
            Stmt::Let { init: None, .. }
            | Stmt::Expr(_)
            | Stmt::Return(_)
            | Stmt::Break
            | Stmt::Continue
            | Stmt::LabeledBreak(_)
            | Stmt::LabeledContinue(_)
            | Stmt::Throw(_)
            | Stmt::PreallocateBoxes(_) => {}
        }
    }
}

fn is_owned_u8_buffer_alloc(expr: &Expr) -> bool {
    matches!(
        expr,
        Expr::BufferAlloc { .. } | Expr::BufferAllocUnsafe(_) | Expr::Uint8ArrayNew(_)
    )
}
