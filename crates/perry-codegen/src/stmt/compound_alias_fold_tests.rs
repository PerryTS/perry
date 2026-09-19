//! #10743: the compound-assignment alias fold, and the shapes it declines.
//!
//! `a[i] += 1` is lowered by HIR's `hoist_compound_member_assign` into two
//! immutable alias `Let`s plus the store, so the base and the key are each
//! evaluated exactly once and before the right-hand side. The classic
//! range-loop matcher admits exactly ONE statement, so the idiomatic spelling
//! could never reach the tier that makes the expanded `a[i] = a[i] + 1` fast:
//! measured 277 instructions per element against 24 for the expanded form on
//! the same array, and annotating the array changed nothing, because the
//! obstacle is the statement count rather than type information.
//!
//! The canonical body below is transcribed from a `--print-hir` dump of
//! `for (let i = 0; i < 400; i++) a[i] += 1;`, not guessed:
//!
//! ```text
//! Let { id: 5, name: "__cmpd_base_5", mutable: false, init: Some(LocalGet(1)) }
//! Let { id: 6, name: "__cmpd_key_6",  mutable: false, init: Some(LocalGet(4)) }
//! Expr(IndexSet { object: LocalGet(5), index: LocalGet(6),
//!                 value: Binary { Add, IndexGet { LocalGet(5), LocalGet(6) },
//!                                 Integer(1) } })
//! ```
//!
//! Every `declines_*` test here is a guard's witness: it is the test that goes
//! red when that condition is deleted from the fold.

#![cfg(test)]

use perry_hir::types::Type;
use perry_hir::{BinaryOp, Expr, Stmt};

use super::loops::packed_f64_range_loop_compound_alias_fold;

const ARRAY: u32 = 1;
const COUNTER: u32 = 4;
const BASE_TEMP: u32 = 5;
const KEY_TEMP: u32 = 6;

fn temp(id: u32, name: &str, mutable: bool, init: Expr) -> Stmt {
    Stmt::Let {
        id,
        name: name.to_string(),
        ty: Type::Number,
        mutable,
        init: Some(init),
    }
}

/// `__cmpd_base_5[__cmpd_key_6] = __cmpd_base_5[__cmpd_key_6] + 1`
fn alias_store() -> Stmt {
    Stmt::Expr(Expr::IndexSet {
        object: Box::new(Expr::LocalGet(BASE_TEMP)),
        index: Box::new(Expr::LocalGet(KEY_TEMP)),
        value: Box::new(Expr::Binary {
            op: BinaryOp::Add,
            left: Box::new(Expr::IndexGet {
                object: Box::new(Expr::LocalGet(BASE_TEMP)),
                index: Box::new(Expr::LocalGet(KEY_TEMP)),
            }),
            right: Box::new(Expr::Integer(1)),
        }),
    })
}

/// What the store must fold to: `a[i] = a[i] + 1`, the shape the tier already
/// admits and already beats node on.
fn expanded_store() -> Stmt {
    Stmt::Expr(Expr::IndexSet {
        object: Box::new(Expr::LocalGet(ARRAY)),
        index: Box::new(Expr::LocalGet(COUNTER)),
        value: Box::new(Expr::Binary {
            op: BinaryOp::Add,
            left: Box::new(Expr::IndexGet {
                object: Box::new(Expr::LocalGet(ARRAY)),
                index: Box::new(Expr::LocalGet(COUNTER)),
            }),
            right: Box::new(Expr::Integer(1)),
        }),
    })
}

fn canonical_body() -> Vec<Stmt> {
    vec![
        temp(BASE_TEMP, "__cmpd_base_5", false, Expr::LocalGet(ARRAY)),
        temp(KEY_TEMP, "__cmpd_key_6", false, Expr::LocalGet(COUNTER)),
        alias_store(),
    ]
}

fn debug(stmts: &[Stmt]) -> String {
    format!("{stmts:?}")
}

#[test]
fn folds_the_canonical_compound_assignment_to_the_expanded_store() {
    let folded =
        packed_f64_range_loop_compound_alias_fold(&canonical_body()).expect("shape must fold");
    assert_eq!(
        debug(&folded),
        debug(std::slice::from_ref(&expanded_store())),
        "the fold must produce exactly the expanded spelling"
    );
}

#[test]
fn folds_an_arithmetic_key_initialiser() {
    // `a[i * 2 + 1] += 1` spills the whole index expression into the key temp.
    let key = Expr::Binary {
        op: BinaryOp::Add,
        left: Box::new(Expr::Binary {
            op: BinaryOp::Mul,
            left: Box::new(Expr::LocalGet(COUNTER)),
            right: Box::new(Expr::Integer(2)),
        }),
        right: Box::new(Expr::Integer(1)),
    };
    let body = vec![
        temp(BASE_TEMP, "__cmpd_base_5", false, Expr::LocalGet(ARRAY)),
        temp(KEY_TEMP, "__cmpd_key_6", false, key.clone()),
        alias_store(),
    ];
    let folded = packed_f64_range_loop_compound_alias_fold(&body).expect("shape must fold");
    let text = debug(&folded);
    assert!(
        !text.contains("LocalGet(5)") && !text.contains("LocalGet(6)"),
        "no alias id may survive the fold: {text}"
    );
    assert!(
        text.contains("Mul"),
        "the key tree must be substituted: {text}"
    );
}

#[test]
fn declines_a_mutable_alias() {
    // Guard: `mutable: false`. A writable binding is not an alias -- nothing
    // here proves its value at the store is the value it was bound to.
    let mut body = canonical_body();
    if let Stmt::Let { mutable, .. } = &mut body[0] {
        *mutable = true;
    }
    assert!(packed_f64_range_loop_compound_alias_fold(&body).is_none());
}

#[test]
fn declines_a_user_named_binding() {
    // Guard: the `__cmpd_` name. The fold's argument rests on these temps
    // being the compiler's own compound-assign spills, read only by the one
    // statement they were minted for. A user `const` in the loop body belongs
    // to the general multi-statement tier (#10741), not here.
    let mut body = canonical_body();
    if let Stmt::Let { name, .. } = &mut body[0] {
        *name = "userConst".to_string();
    }
    assert!(packed_f64_range_loop_compound_alias_fold(&body).is_none());
}

#[test]
fn declines_an_initialiser_outside_the_stable_grammar() {
    // Guard: `packed_f64_range_loop_alias_init_is_stable`. An element read is
    // not re-evaluation-safe the way a local read is -- the folded statement
    // evaluates the key tree twice.
    let mut body = canonical_body();
    if let Stmt::Let { init, .. } = &mut body[1] {
        *init = Some(Expr::IndexGet {
            object: Box::new(Expr::LocalGet(ARRAY)),
            index: Box::new(Expr::LocalGet(COUNTER)),
        });
    }
    assert!(packed_f64_range_loop_compound_alias_fold(&body).is_none());
}

#[test]
fn declines_a_body_longer_than_two_aliases_and_a_store() {
    let mut body = canonical_body();
    body.insert(0, temp(7, "__cmpd_base_7", false, Expr::LocalGet(ARRAY)));
    assert!(packed_f64_range_loop_compound_alias_fold(&body).is_none());
}

#[test]
fn declines_a_body_with_no_aliases() {
    // A single statement is already the shape the tier takes; the fold must
    // not claim it, or it would clear and rebuild an access map for nothing.
    assert!(
        packed_f64_range_loop_compound_alias_fold(std::slice::from_ref(&expanded_store()))
            .is_none()
    );
}

#[test]
fn declines_a_repeated_alias_id() {
    // Two bindings for one id would make the substitution order-dependent.
    let body = vec![
        temp(BASE_TEMP, "__cmpd_base_5", false, Expr::LocalGet(ARRAY)),
        temp(BASE_TEMP, "__cmpd_key_5", false, Expr::LocalGet(COUNTER)),
        alias_store(),
    ];
    assert!(packed_f64_range_loop_compound_alias_fold(&body).is_none());
}

#[test]
fn declines_when_the_last_statement_is_not_an_expression() {
    let body = vec![
        temp(BASE_TEMP, "__cmpd_base_5", false, Expr::LocalGet(ARRAY)),
        temp(KEY_TEMP, "__cmpd_key_6", false, Expr::LocalGet(COUNTER)),
        Stmt::Return(Some(Expr::LocalGet(BASE_TEMP))),
    ];
    assert!(packed_f64_range_loop_compound_alias_fold(&body).is_none());
}

#[test]
fn declines_an_alias_without_an_initialiser() {
    let body = vec![
        Stmt::Let {
            id: BASE_TEMP,
            name: "__cmpd_base_5".to_string(),
            ty: Type::Number,
            mutable: false,
            init: None,
        },
        temp(KEY_TEMP, "__cmpd_key_6", false, Expr::LocalGet(COUNTER)),
        alias_store(),
    ];
    assert!(packed_f64_range_loop_compound_alias_fold(&body).is_none());
}

#[test]
fn the_logical_assignment_shape_folds_but_stays_unversionable() {
    // `a[i] ||= 3` spills the same two aliases but ends in `Expr::Logical`,
    // whose right operand is the store. The fold is shape-agnostic, so it
    // rewrites the statement -- and the classic body walk then declines it,
    // because `packed_f64_range_loop_pure_expr_collect` has no `IndexSet` arm.
    // This test pins the second half of that sentence: if a future widening
    // admits `Logical`, the short-circuit semantics have to be re-argued.
    let body = vec![
        temp(BASE_TEMP, "__cmpd_base_5", false, Expr::LocalGet(ARRAY)),
        temp(KEY_TEMP, "__cmpd_key_6", false, Expr::LocalGet(COUNTER)),
        Stmt::Expr(Expr::Logical {
            op: perry_hir::LogicalOp::Or,
            left: Box::new(Expr::IndexGet {
                object: Box::new(Expr::LocalGet(BASE_TEMP)),
                index: Box::new(Expr::LocalGet(KEY_TEMP)),
            }),
            right: Box::new(Expr::IndexSet {
                object: Box::new(Expr::LocalGet(BASE_TEMP)),
                index: Box::new(Expr::LocalGet(KEY_TEMP)),
                value: Box::new(Expr::Integer(3)),
            }),
        }),
    ];
    let folded = packed_f64_range_loop_compound_alias_fold(&body).expect("shape folds");
    let mut accesses = std::collections::BTreeMap::new();
    assert!(
        !super::loops::packed_f64_range_loop_body_collect(
            &folded,
            COUNTER,
            None,
            &mut accesses,
            None,
        ),
        "a logical compound assignment must not be admitted by the classic walk"
    );
}
