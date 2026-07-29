//! Flow analysis: locals whose runtime value provably can **never** be a
//! BigInt.
//!
//! Motivation: bcryptjs's `_encipher` Feistel accumulators (`l` / `r`) are
//! initialized from an `Int32Array` element and then only ever bitwise-updated
//! (`l ^= …`), yet their declared type is erased to `Any` (the `let l =
//! lr[off]` inference does not propagate the typed-array element type). Because
//! `Any` could hold a BigInt in general, `type_analysis::is_numeric_expr`
//! rejects them and every `^`/`&`/`>>`/`>>>` on `l` / `r` bailed to the
//! BigInt-aware `js_dynamic_bit*` runtime helper — one opaque call per Feistel
//! round.
//!
//! This collector proves such locals are non-BigInt structurally: a local is
//! non-BigInt when **every** assignment to it (its initializer and every
//! `LocalSet`) is an expression whose `ToNumeric` cannot yield a BigInt. The
//! result seeds `type_analysis::is_provably_not_bigint`, which gates the inline
//! non-BigInt bitwise fast path (`expr/binary.rs`).
//!
//! SOUNDNESS: the analysis is a greatest fixpoint (start optimistic, remove any
//! local with a not-provably-non-BigInt write, propagate to a fixed point). It
//! may under-approximate (miss some genuinely-non-BigInt locals) but never
//! over-approximates. Leaf `LocalGet`s are judged against the current set or
//! their declared type; declared types are TRUSTED here exactly as
//! `is_numeric_expr` already trusts them (Perry does not runtime-validate
//! declared types — a documented limitation).

use std::collections::{HashMap, HashSet};

use perry_hir::types::Type as HirType;
use perry_hir::{Expr, Param, Stmt, UnaryOp};

use crate::type_analysis::is_numeric_typed_array_class;

/// Compute the set of local ids whose value is provably never a BigInt.
pub fn collect_not_bigint_locals(
    stmts: &[Stmt],
    params: &[Param],
    binding_types: &HashMap<u32, HirType>,
) -> HashSet<u32> {
    // Declared-type map (params + let bindings). Used to judge a `LocalGet`
    // leaf whose id is not one of the analyzed (written) candidates.
    let mut types: HashMap<u32, HirType> = binding_types.clone();
    for p in params {
        types.entry(p.id).or_insert_with(|| p.ty.clone());
    }

    // Every write (Let init + `LocalSet` rhs) per candidate local. Descends
    // into closure bodies so a `LocalSet` to an ENCLOSING local inside a
    // closure is captured (LocalIds are unique per function, so the write is
    // recorded against the enclosing id).
    let mut writes: HashMap<u32, Vec<&Expr>> = HashMap::new();
    let mut candidates: HashSet<u32> = HashSet::new();
    collect_writes(stmts, &mut writes, &mut candidates);

    // Greatest fixpoint. Start with every candidate assumed non-BigInt, then
    // repeatedly drop any candidate that has a write whose rhs is not provably
    // non-BigInt under the current assumption. A rhs that references the
    // candidate itself (`l = l ^ x`) short-circuits on the non-self operand, so
    // accumulators converge; a genuine BigInt write drops the local (and the
    // taint then propagates to anything defined from it).
    let mut not_bigint = candidates;
    loop {
        let mut remove: Vec<u32> = Vec::new();
        for &id in &not_bigint {
            let all_ok = writes
                .get(&id)
                // A declared-but-never-assigned `let x;` is `undefined` — a
                // non-BigInt — so no writes means the local stays.
                .map(|ws| {
                    ws.iter()
                        .all(|rhs| expr_not_bigint(rhs, &types, &not_bigint))
                })
                .unwrap_or(true);
            if !all_ok {
                remove.push(id);
            }
        }
        if remove.is_empty() {
            break;
        }
        for id in remove {
            not_bigint.remove(&id);
        }
    }
    not_bigint
}

/// Structural, context-free judgment mirroring
/// `type_analysis::is_provably_not_bigint`: can this expression's value never be
/// a BigInt? `LocalGet` leaves resolve against `set` (optimistic membership) or
/// a concrete non-BigInt declared type.
fn expr_not_bigint(e: &Expr, types: &HashMap<u32, HirType>, set: &HashSet<u32>) -> bool {
    match e {
        // Non-BigInt literals.
        Expr::Undefined
        | Expr::Null
        | Expr::Number(_)
        | Expr::Integer(_)
        | Expr::Bool(_)
        | Expr::String(_)
        | Expr::WtfString(_) => true,
        // The only literal BigInt forms.
        Expr::BigInt(_) | Expr::BigIntCoerce(_) => false,

        // Definitionally non-BigInt value producers.
        Expr::TypeOf(_)
        | Expr::Void(_)
        | Expr::Compare { .. }
        | Expr::InstanceOf { .. }
        | Expr::In { .. }
        | Expr::PrivateBrandCheck { .. }
        | Expr::NumberCoerce(_)
        | Expr::StringCoerce(_)
        | Expr::BooleanCoerce(_)
        | Expr::IsNaN(_)
        | Expr::IsFinite(_)
        | Expr::NumberIsNaN(_)
        | Expr::NumberIsFinite(_)
        | Expr::NumberIsInteger(_)
        | Expr::IsUndefinedOrBareNan(_)
        | Expr::DateNow => true,

        // Typed-array / numeric-array element reads yield Number | undefined,
        // never a BigInt.
        Expr::Uint8ArrayGet { .. } | Expr::BufferIndexGet { .. } => true,
        Expr::IndexGet { object, .. } => index_receiver_is_numeric(object, types),

        // `!x` → boolean; `+x` → Number-or-throw (never a BigInt VALUE).
        // `-x` / `~x` preserve BigInt.
        Expr::Unary { op, operand } => match op {
            UnaryOp::Not | UnaryOp::Pos => true,
            UnaryOp::Neg | UnaryOp::BitNot => expr_not_bigint(operand, types, set),
        },

        // Arithmetic / bitwise binary ops yield a BigInt only when BOTH
        // operands `ToNumeric` to BigInt, so the result is non-BigInt as soon
        // as EITHER operand is. (`BinaryOp` has only arithmetic/bitwise
        // variants.)
        Expr::Binary { left, right, .. } => {
            expr_not_bigint(left, types, set) || expr_not_bigint(right, types, set)
        }

        // Selection: non-BigInt when every branch that can become the value is.
        Expr::Conditional {
            then_expr,
            else_expr,
            ..
        } => expr_not_bigint(then_expr, types, set) && expr_not_bigint(else_expr, types, set),
        Expr::Logical { left, right, .. } => {
            expr_not_bigint(left, types, set) && expr_not_bigint(right, types, set)
        }

        // A `LocalSet` used as an expression evaluates to the assigned value.
        Expr::LocalSet(_, rhs) => expr_not_bigint(rhs, types, set),

        // Leaf locals: proven by the running assumption or a concrete
        // non-BigInt declared type. `Update` (`i++`) yields `ToNumeric(i) ± 1`,
        // which preserves BigInt-ness, so it resolves exactly like `LocalGet`.
        Expr::LocalGet(id) | Expr::Update { id, .. } => {
            set.contains(id) || type_is_not_bigint(types.get(id))
        }

        // Everything else is unproven → conservatively BigInt-possible.
        _ => false,
    }
}

/// A concrete declared type that can never hold a BigInt. `Any`, `BigInt`,
/// `Named` (an object could `ToPrimitive` to a BigInt), and unions are
/// deliberately excluded.
fn type_is_not_bigint(t: Option<&HirType>) -> bool {
    matches!(
        t,
        Some(
            HirType::Number
                | HirType::Int32
                | HirType::Boolean
                | HirType::String
                | HirType::StringLiteral(_)
        )
    )
}

/// True when `object` indexes a numeric typed array (`Int32Array` etc.) or a
/// declared numeric array (`number[]` / `Int32[]`). Mirrors the numeric-element
/// recognition in `type_analysis::is_numeric_expr`.
fn index_receiver_is_numeric(object: &Expr, types: &HashMap<u32, HirType>) -> bool {
    let Expr::LocalGet(id) = object else {
        return false;
    };
    match types.get(id) {
        Some(HirType::Named(name)) => is_numeric_typed_array_class(name),
        Some(HirType::Array(elem)) => matches!(**elem, HirType::Number | HirType::Int32),
        Some(HirType::Generic { base, type_args }) if base == "Array" && type_args.len() == 1 => {
            matches!(type_args[0], HirType::Number | HirType::Int32)
        }
        _ => false,
    }
}

/// Record every write (Let init + `LocalSet` rhs) per local, gather the set of
/// analyzed candidate ids, and descend into closure bodies.
fn collect_writes<'a>(
    stmts: &'a [Stmt],
    writes: &mut HashMap<u32, Vec<&'a Expr>>,
    candidates: &mut HashSet<u32>,
) {
    for s in stmts {
        match s {
            Stmt::Let { id, init, .. } => {
                candidates.insert(*id);
                if let Some(e) = init {
                    writes.entry(*id).or_default().push(e);
                    collect_writes_expr(e, writes, candidates);
                }
            }
            Stmt::Expr(e) | Stmt::Throw(e) => collect_writes_expr(e, writes, candidates),
            Stmt::Return(opt) => {
                if let Some(e) = opt {
                    collect_writes_expr(e, writes, candidates);
                }
            }
            Stmt::If {
                condition,
                then_branch,
                else_branch,
            } => {
                collect_writes_expr(condition, writes, candidates);
                collect_writes(then_branch, writes, candidates);
                if let Some(eb) = else_branch {
                    collect_writes(eb, writes, candidates);
                }
            }
            Stmt::While { condition, body } => {
                collect_writes_expr(condition, writes, candidates);
                collect_writes(body, writes, candidates);
            }
            Stmt::DoWhile { body, condition } => {
                collect_writes(body, writes, candidates);
                collect_writes_expr(condition, writes, candidates);
            }
            Stmt::For {
                init,
                condition,
                update,
                body,
            } => {
                if let Some(i) = init {
                    collect_writes(std::slice::from_ref(i.as_ref()), writes, candidates);
                }
                if let Some(c) = condition {
                    collect_writes_expr(c, writes, candidates);
                }
                if let Some(u) = update {
                    collect_writes_expr(u, writes, candidates);
                }
                collect_writes(body, writes, candidates);
            }
            Stmt::Labeled { body, .. } => {
                collect_writes(std::slice::from_ref(body.as_ref()), writes, candidates);
            }
            Stmt::Try {
                body,
                catch,
                finally,
            } => {
                collect_writes(body, writes, candidates);
                if let Some(c) = catch {
                    collect_writes(&c.body, writes, candidates);
                }
                if let Some(f) = finally {
                    collect_writes(f, writes, candidates);
                }
            }
            Stmt::Switch {
                discriminant,
                cases,
            } => {
                collect_writes_expr(discriminant, writes, candidates);
                for case in cases {
                    if let Some(t) = &case.test {
                        collect_writes_expr(t, writes, candidates);
                    }
                    collect_writes(&case.body, writes, candidates);
                }
            }
            Stmt::Break
            | Stmt::Continue
            | Stmt::LabeledBreak(_)
            | Stmt::LabeledContinue(_)
            | Stmt::PreallocateBoxes(_)
            | Stmt::PreallocateTdzBoxes(_) => {}
        }
    }
}

fn collect_writes_expr<'a>(
    e: &'a Expr,
    writes: &mut HashMap<u32, Vec<&'a Expr>>,
    candidates: &mut HashSet<u32>,
) {
    if let Expr::LocalSet(id, rhs) = e {
        writes.entry(*id).or_default().push(rhs.as_ref());
    }
    // Closure bodies are statements, not expression children, so descend
    // explicitly. A write to an enclosing local inside the closure body targets
    // the same (unique) LocalId, so it is correctly attributed.
    if let Expr::Closure { body, .. } = e {
        collect_writes(body, writes, candidates);
    }
    perry_hir::walker::walk_expr_children(e, &mut |child| {
        collect_writes_expr(child, writes, candidates)
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use perry_hir::BinaryOp;

    fn let_stmt(id: u32, init: Option<Expr>) -> Stmt {
        Stmt::Let {
            id,
            name: format!("v{id}"),
            ty: HirType::Any,
            mutable: true,
            init,
        }
    }

    fn set(id: u32, rhs: Expr) -> Stmt {
        Stmt::Expr(Expr::LocalSet(id, Box::new(rhs)))
    }

    fn xor(l: Expr, r: Expr) -> Expr {
        Expr::Binary {
            op: BinaryOp::BitXor,
            left: Box::new(l),
            right: Box::new(r),
        }
    }

    fn run(stmts: &[Stmt]) -> HashSet<u32> {
        collect_not_bigint_locals(stmts, &[], &HashMap::new())
    }

    #[test]
    fn self_referential_bitwise_accumulator_stays_non_bigint() {
        // let l = 0; l = l ^ 5;  — the self-reference short-circuits on the
        // non-`l` operand, so `l` is proven never-BigInt.
        let stmts = vec![
            let_stmt(1, Some(Expr::Integer(0))),
            set(1, xor(Expr::LocalGet(1), Expr::Integer(5))),
        ];
        let got = run(&stmts);
        assert!(got.contains(&1), "self-ref accumulator missing: {got:?}");
    }

    #[test]
    fn mutually_referencing_numeric_locals_stay_non_bigint() {
        // let a = 0; let b = 0; a = b ^ 1; b = a ^ 1;
        let stmts = vec![
            let_stmt(1, Some(Expr::Integer(0))),
            let_stmt(2, Some(Expr::Integer(0))),
            set(1, xor(Expr::LocalGet(2), Expr::Integer(1))),
            set(2, xor(Expr::LocalGet(1), Expr::Integer(1))),
        ];
        let got = run(&stmts);
        assert!(
            got.contains(&1) && got.contains(&2),
            "mutually-referencing locals missing: {got:?}"
        );
    }

    #[test]
    fn uninitialized_let_is_non_bigint() {
        // `let x;` — undefined, never written → non-BigInt.
        let got = run(&[let_stmt(1, None)]);
        assert!(got.contains(&1), "uninitialized local missing: {got:?}");
    }

    #[test]
    fn bigint_literal_write_invalidates_local() {
        // let y = 0; y = 5n;  — a BigInt write means `y` CAN be a BigInt.
        let stmts = vec![
            let_stmt(1, Some(Expr::Integer(0))),
            set(1, Expr::BigInt("5".to_string())),
        ];
        let got = run(&stmts);
        assert!(
            !got.contains(&1),
            "bigint-written local not excluded: {got:?}"
        );
    }

    #[test]
    fn bigint_init_and_copy_taint_propagate() {
        // let y = 5n; let w = y;  — `y` is a BigInt, and `w` copies it, so the
        // taint must propagate across the fixpoint and exclude BOTH.
        let stmts = vec![
            let_stmt(1, Some(Expr::BigInt("5".to_string()))),
            let_stmt(2, Some(Expr::LocalGet(1))),
        ];
        let got = run(&stmts);
        assert!(
            !got.contains(&1) && !got.contains(&2),
            "bigint taint did not propagate through copy: {got:?}"
        );
    }
}
