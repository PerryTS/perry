//! Locals whose value is always a **Number, a BigInt or `undefined`**: the
//! keys an owned typed array can be indexed with through a guarded inline
//! access without ever reaching its `buffer` getter.
//!
//! ## Why this property, and not "Number"
//!
//! `let k = perm[0]` reads an `Int32Array` element. The read is a Number, or
//! `undefined` when the index is out of bounds, so `number_by_construction`
//! (correctly) never admits `k`, and every `perm[k]` used to leave the
//! storage-proven view tier for a runtime call. The guarded tier
//! (`expr/proven_view_guarded.rs`) does not need a Number: it tests the key at
//! run time and sends anything that is not an exact in-bounds integer to the
//! complete dynamic `[[Get]]`/`[[Set]]`. What it needs is that this exit can
//! never *move the array's storage*.
//!
//! A storage-proven view caches its data pointer, and the runtime rebinds a
//! plain typed array onto a fresh `ArrayBuffer` the first time `.buffer` is
//! observed (`typedarray_view::js_typed_array_backing_buffer`). After that the
//! cached inline pointer is stale. The key `"buffer"`, or an object whose
//! `toString` returns it, reaches that getter through `ta[key]`; that is why
//! an unproven runtime-key read invalidates the view (`index_get.rs`). A
//! Number or BigInt key is a canonical numeric index, which an integer-indexed
//! exotic object answers itself without consulting its prototype. `undefined`
//! becomes the key `"undefined"`, an ordinary lookup, the same class as
//! `ta.foo`: neither names the `buffer` getter. So for these three types the
//! exit leaves the cached pointer valid.
//!
//! ## The proof
//!
//! A greatest fixpoint over the scanned body's `let`-bound locals: start
//! optimistic, drop a local when its initialiser or any `LocalSet` right-hand
//! side is not judged a key, repeat until stable. `Update` (`x++`) always
//! yields a Number or BigInt. A `let x;` without an initialiser is `undefined`
//! and stays admitted. Declared types are never evidence.
//!
//! Fail-closed exclusions: parameters (their incoming value is unconstrained,
//! and `var x` can reuse a parameter's id), ids bound by a closure parameter,
//! a `catch` clause, a box pre-allocation or a `with` fallback
//! (`spec_abi_sites::non_expression_bound_locals`), closure-boxed locals and
//! module globals (their writes live outside this walk).

use std::collections::{HashMap, HashSet};

use perry_hir::{BinaryOp, Expr, Param, Stmt, UnaryOp};

/// Leaf facts for [`expr_is_numeric_key`]. The collector answers them from
/// the function's fact sets; codegen answers them from its `FnCtx`.
pub(crate) struct KeyLeaves<'a> {
    /// `e` is proven to evaluate to a Number (never `undefined`).
    pub number: &'a dyn Fn(&Expr) -> bool,
    /// Local `id` always holds a Number, BigInt or `undefined`.
    pub key_local: &'a dyn Fn(u32) -> bool,
    /// Local `id` is an immutable binding of a fresh, owned numeric typed
    /// array or buffer, so reading it with a Number/BigInt key yields a
    /// Number, a BigInt or `undefined`.
    pub owned_view: &'a dyn Fn(u32) -> bool,
}

/// `e` always evaluates to a Number, a BigInt or `undefined` (or throws).
pub(crate) fn expr_is_numeric_key(e: &Expr, leaves: &KeyLeaves<'_>) -> bool {
    if (leaves.number)(e) {
        return true;
    }
    match e {
        Expr::Integer(_) | Expr::Number(_) | Expr::Undefined => true,
        Expr::LocalGet(id) => (leaves.key_local)(*id),
        Expr::IndexGet { object, index } => {
            matches!(object.as_ref(), Expr::LocalGet(id) if (leaves.owned_view)(*id))
                && expr_is_number_or_bigint(index, leaves)
        }
        _ => expr_is_number_or_bigint(e, leaves),
    }
}

/// `e` always evaluates to a Number or a BigInt (or throws): never
/// `undefined`, never a string, never an object.
fn expr_is_number_or_bigint(e: &Expr, leaves: &KeyLeaves<'_>) -> bool {
    if (leaves.number)(e) {
        return true;
    }
    match e {
        Expr::Integer(_) | Expr::Number(_) => true,
        // `ToNumeric(x) ± 1`.
        Expr::Update { .. } => true,
        // `-x` and `~x` are Number or BigInt; `+x` is a Number or throws.
        Expr::Unary {
            op: UnaryOp::Neg | UnaryOp::BitNot | UnaryOp::Pos,
            ..
        } => true,
        // `+` concatenates when either side is a string or an object that
        // converts to one. Over two keys it is numeric addition: Number +
        // `undefined` is NaN, and a BigInt mixed with anything else throws.
        Expr::Binary {
            op: BinaryOp::Add,
            left,
            right,
        } => expr_is_numeric_key(left, leaves) && expr_is_numeric_key(right, leaves),
        // Every other binary operator applies ToNumeric to both sides.
        Expr::Binary { .. } => true,
        _ => false,
    }
}

/// The function-scope fixpoint described in the module docs.
#[allow(clippy::too_many_arguments)]
pub(crate) fn collect_numeric_key_locals(
    stmts: &[Stmt],
    params: &[Param],
    boxed_vars: &HashSet<u32>,
    module_globals: &HashMap<u32, String>,
    number_locals: &HashSet<u32>,
    owned_views: &HashSet<u32>,
) -> HashSet<u32> {
    let mut writes: HashMap<u32, Vec<Option<&Expr>>> = HashMap::new();
    let mut let_bound = HashSet::new();
    super::not_bigint_locals::collect_writes(stmts, &mut writes, &mut let_bound);
    let foreign = super::spec_abi_sites::non_expression_bound_locals(stmts);
    let param_ids: HashSet<u32> = params.iter().map(|p| p.id).collect();
    let mut set: HashSet<u32> = let_bound
        .into_iter()
        .filter(|id| {
            !param_ids.contains(id)
                && !foreign.contains(id)
                && !boxed_vars.contains(id)
                && !module_globals.contains_key(id)
                && !number_locals.contains(id)
        })
        .collect();
    loop {
        let snapshot = set.clone();
        let number = |e: &Expr| matches!(e, Expr::LocalGet(id) if number_locals.contains(id));
        let key_local = |id: u32| snapshot.contains(&id) || number_locals.contains(&id);
        let owned_view = |id: u32| owned_views.contains(&id);
        let leaves = KeyLeaves {
            number: &number,
            key_local: &key_local,
            owned_view: &owned_view,
        };
        set.retain(|id| {
            writes.get(id).is_some_and(|ws| {
                ws.iter().all(|w| match w {
                    None => true,
                    Some(e) => expr_is_numeric_key(e, &leaves),
                })
            })
        });
        if set.len() == snapshot.len() {
            return set;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use perry_hir::types::Type as HirType;

    fn let_stmt(id: u32, init: Option<Expr>) -> Stmt {
        Stmt::Let {
            id,
            name: format!("v{id}"),
            ty: HirType::Number,
            mutable: true,
            init,
        }
    }

    fn set(id: u32, rhs: Expr) -> Stmt {
        Stmt::Expr(Expr::LocalSet(id, Box::new(rhs)))
    }

    fn read(view: u32, index: Expr) -> Expr {
        Expr::IndexGet {
            object: Box::new(Expr::LocalGet(view)),
            index: Box::new(index),
        }
    }

    fn run(stmts: &[Stmt], owned: &[u32]) -> HashSet<u32> {
        collect_numeric_key_locals(
            stmts,
            &[],
            &HashSet::new(),
            &HashMap::new(),
            &HashSet::new(),
            &owned.iter().copied().collect(),
        )
    }

    #[test]
    fn owned_view_reads_and_their_arithmetic_are_keys() {
        // const perm = <owned 9>; let k = perm[0]; let j = k; j = j - 1; k = perm[0];
        let stmts = vec![
            let_stmt(1, Some(read(9, Expr::Integer(0)))),
            let_stmt(2, Some(Expr::LocalGet(1))),
            set(
                2,
                Expr::Binary {
                    op: BinaryOp::Sub,
                    left: Box::new(Expr::LocalGet(2)),
                    right: Box::new(Expr::Integer(1)),
                },
            ),
            set(1, read(9, Expr::Integer(0))),
            let_stmt(3, None),
        ];
        let got = run(&stmts, &[9]);
        assert_eq!(got, [1, 2, 3].into_iter().collect());
    }

    #[test]
    fn a_string_write_anywhere_drops_the_local_and_its_dependents() {
        // let k = perm[0]; let j = k; k = "buffer";
        let stmts = vec![
            let_stmt(1, Some(read(9, Expr::Integer(0)))),
            let_stmt(2, Some(Expr::LocalGet(1))),
            set(1, Expr::String("buffer".into())),
        ];
        assert!(run(&stmts, &[9]).is_empty());
    }

    #[test]
    fn a_read_from_an_unowned_receiver_is_not_a_key() {
        // `arr` (8) is not an owned typed array: its element can be anything.
        let stmts = vec![let_stmt(1, Some(read(8, Expr::Integer(0))))];
        assert!(run(&stmts, &[9]).is_empty());
    }

    #[test]
    fn an_owned_read_keyed_by_a_maybe_undefined_local_is_not_a_key() {
        // let k = perm[0]; let t = perm[k];  — `k` may be undefined, and
        // `perm["undefined"]` is an ordinary lookup whose result is unbounded.
        let stmts = vec![
            let_stmt(1, Some(read(9, Expr::Integer(0)))),
            let_stmt(2, Some(read(9, Expr::LocalGet(1)))),
        ];
        let got = run(&stmts, &[9]);
        assert!(got.contains(&1) && !got.contains(&2), "{got:?}");
    }

    #[test]
    fn concatenation_with_a_non_key_is_not_a_key() {
        let stmts = vec![
            let_stmt(1, Some(Expr::Integer(0))),
            set(
                1,
                Expr::Binary {
                    op: BinaryOp::Add,
                    left: Box::new(Expr::LocalGet(1)),
                    right: Box::new(Expr::String("x".into())),
                },
            ),
        ];
        assert!(run(&stmts, &[]).is_empty());
    }

    #[test]
    fn parameters_are_never_admitted() {
        // `var x` re-declaring parameter 1 reuses its id; the slot holds the
        // caller's argument before that `Let` runs.
        let params = vec![Param {
            id: 1,
            name: "x".into(),
            ty: HirType::Number,
            default: None,
            decorators: Vec::new(),
            is_rest: false,
            arguments_object: None,
        }];
        let stmts = vec![let_stmt(1, None)];
        let got = collect_numeric_key_locals(
            &stmts,
            &params,
            &HashSet::new(),
            &HashMap::new(),
            &HashSet::new(),
            &HashSet::new(),
        );
        assert!(got.is_empty());
    }
}
