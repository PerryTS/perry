use perry_hir::{Expr, Stmt};
use std::collections::HashSet;

/// Reusable HIR facts consumed by hot-loop lowering paths.
///
/// This keeps integer, bounded-index, and helper-return facts in one place
/// instead of making every codegen entry point rediscover the same source
/// shapes independently.
pub(crate) struct HirFacts {
    pub integer_locals: HashSet<u32>,
    pub unsigned_i32_locals: HashSet<u32>,
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
    let unsigned_i32_locals = super::i32_locals::collect_unsigned_i32_locals(stmts);
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
        unsigned_i32_locals,
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
    match expr {
        Expr::BufferAlloc { .. } | Expr::BufferAllocUnsafe(_) => true,
        Expr::Uint8ArrayNew(None) => true,
        Expr::Uint8ArrayNew(Some(size)) => is_fresh_uint8array_length_literal(size),
        _ => false,
    }
}

fn is_fresh_uint8array_length_literal(expr: &Expr) -> bool {
    match expr {
        Expr::Integer(n) => *n >= 0 && *n < i32::MAX as i64,
        Expr::Number(n) => n.is_finite() && n.fract() == 0.0 && *n >= 0.0 && *n < i32::MAX as f64,
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use perry_hir::BinaryOp;
    use perry_types::Type;

    fn const_let(id: u32, init: Expr) -> Stmt {
        Stmt::Let {
            id,
            name: format!("v{}", id),
            ty: Type::Named("Uint8Array".into()),
            mutable: false,
            init: Some(init),
        }
    }

    fn known_ids(stmts: Vec<Stmt>) -> HashSet<u32> {
        collect_known_noalias_buffer_locals(&stmts)
    }

    fn mutable_number_let(id: u32, init: Expr) -> Stmt {
        Stmt::Let {
            id,
            name: format!("v{}", id),
            ty: Type::Number,
            mutable: true,
            init: Some(init),
        }
    }

    fn ushr0(left: Expr) -> Expr {
        Expr::Binary {
            op: BinaryOp::UShr,
            left: Box::new(left),
            right: Box::new(Expr::Integer(0)),
        }
    }

    #[test]
    fn uint8array_literal_lengths_are_known_noalias_sources() {
        let ids = known_ids(vec![
            const_let(1, Expr::Uint8ArrayNew(None)),
            const_let(2, Expr::Uint8ArrayNew(Some(Box::new(Expr::Integer(8))))),
            const_let(3, Expr::Uint8ArrayNew(Some(Box::new(Expr::Number(16.0))))),
        ]);

        assert!(ids.contains(&1));
        assert!(ids.contains(&2));
        assert!(ids.contains(&3));
    }

    #[test]
    fn uint8array_non_literal_or_alias_possible_sources_are_not_noalias() {
        let ids = known_ids(vec![
            const_let(1, Expr::Uint8ArrayNew(Some(Box::new(Expr::LocalGet(99))))),
            const_let(2, Expr::Uint8ArrayNew(Some(Box::new(Expr::Integer(-1))))),
            const_let(3, Expr::Uint8ArrayNew(Some(Box::new(Expr::Number(3.5))))),
            const_let(4, Expr::Uint8ArrayNew(Some(Box::new(Expr::Number(-1.0))))),
            const_let(
                5,
                Expr::Uint8ArrayNew(Some(Box::new(Expr::Number(i32::MAX as f64)))),
            ),
        ]);

        assert!(ids.is_empty(), "unexpected noalias ids: {ids:?}");
    }

    #[test]
    fn mutable_ushr_zero_recurrence_is_unsigned_i32_not_signed_integer() {
        let facts = collect_hir_facts(
            &[
                const_let(1, ushr0(Expr::Integer(0x9E3779B9))),
                mutable_number_let(2, ushr0(Expr::LocalGet(1))),
                Stmt::Expr(Expr::LocalSet(
                    2,
                    Box::new(ushr0(Expr::Binary {
                        op: BinaryOp::BitXor,
                        left: Box::new(Expr::LocalGet(2)),
                        right: Box::new(Expr::Integer(0x1234)),
                    })),
                )),
            ],
            &HashSet::new(),
            &HashSet::new(),
        );

        assert!(facts.unsigned_i32_locals.contains(&2));
        assert!(!facts.integer_locals.contains(&2));
    }

    #[test]
    fn signed_write_disqualifies_unsigned_i32_local() {
        let facts = collect_hir_facts(
            &[
                mutable_number_let(2, ushr0(Expr::Integer(0x9E3779B9))),
                Stmt::Expr(Expr::LocalSet(
                    2,
                    Box::new(Expr::Binary {
                        op: BinaryOp::BitOr,
                        left: Box::new(Expr::LocalGet(2)),
                        right: Box::new(Expr::Integer(0)),
                    }),
                )),
            ],
            &HashSet::new(),
            &HashSet::new(),
        );

        assert!(!facts.unsigned_i32_locals.contains(&2));
    }
}
