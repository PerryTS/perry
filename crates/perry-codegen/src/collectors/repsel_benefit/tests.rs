//! Unit tests for the dispatch-tower routing threshold (#7142).

use super::*;
use perry_hir::types::Type as HirType;
use perry_hir::{BinaryOp, Stmt};

fn get(id: u32) -> Expr {
    Expr::LocalGet(id)
}

fn bin(op: BinaryOp, l: Expr, r: Expr) -> Expr {
    Expr::Binary {
        op,
        left: Box::new(l),
        right: Box::new(r),
    }
}

fn this_get(field: &str) -> Expr {
    Expr::PropertyGet {
        byte_end: 0,
        object: Box::new(Expr::This),
        property: field.to_string(),
        byte_offset: 0,
    }
}

fn method(body: Vec<Stmt>) -> perry_hir::Function {
    perry_hir::Function {
        id: 1,
        name: "m".to_string(),
        type_params: Vec::new(),
        params: Vec::new(),
        return_type: HirType::Number,
        body,
        is_async: false,
        is_generator: false,
        is_strict: false,
        is_exported: false,
        captures: Vec::new(),
        decorators: Vec::new(),
        was_plain_async: false,
        was_unrolled: false,
    }
}

fn declared(names: &[&str]) -> HashSet<String> {
    names.iter().map(|s| (*s).to_string()).collect()
}

/// One declared-field site is the break-even, not a win: the route swaps the
/// body's single inline shape check for one at the call site and adds a branch
/// plus a second call site on top. Two is the first count that deletes work.
#[test]
fn tower_route_break_even_is_one_field_site() {
    let fields = declared(&["a", "c"]);

    let one = method(vec![Stmt::Return(Some(this_get("a")))]);
    assert_eq!(
        proven_receiver_clone_benefit_sites(&one, &fields, &HashSet::new()),
        1
    );
    assert!(!tower_route_profitable(&one, &fields, &HashSet::new()));

    let two = method(vec![Stmt::Return(Some(bin(
        BinaryOp::Add,
        this_get("a"),
        this_get("c"),
    )))]);
    assert_eq!(
        proven_receiver_clone_benefit_sites(&two, &fields, &HashSet::new()),
        2
    );
    assert!(tower_route_profitable(&two, &fields, &HashSet::new()));
}

/// Only DECLARED chain fields get a fixed slot; anything else keeps its by-name
/// lowering inside the clone too, so it is not work the route deletes and must
/// not be counted toward the threshold.
#[test]
fn tower_route_counts_only_declared_fields() {
    let fields = declared(&["a"]);
    let m = method(vec![
        Stmt::Expr(this_get("a")),
        Stmt::Expr(this_get("expando")),
        Stmt::Expr(this_get("alsoNotDeclared")),
    ]);
    assert_eq!(
        proven_receiver_clone_benefit_sites(&m, &fields, &HashSet::new()),
        1
    );
    assert!(!tower_route_profitable(&m, &fields, &HashSet::new()));
}

/// A field access on something that is not `this` is somebody else's receiver
/// and is not affected by the clone at all.
#[test]
fn tower_route_ignores_non_this_receivers() {
    let fields = declared(&["a"]);
    let m = method(vec![
        Stmt::Expr(this_get("a")),
        Stmt::Expr(Expr::PropertyGet {
            byte_end: 0,
            object: Box::new(get(9)),
            property: "a".to_string(),
            byte_offset: 0,
        }),
    ]);
    assert_eq!(
        proven_receiver_clone_benefit_sites(&m, &fields, &HashSet::new()),
        1
    );
}

/// Writes count as well as reads — a `this.f = …` site pays the same inline
/// check (plus the frozen/plain-value conjuncts) in the public body.
#[test]
fn tower_route_counts_writes() {
    let fields = declared(&["a", "b"]);
    let m = method(vec![
        Stmt::Expr(Expr::PropertySet {
            object: Box::new(Expr::This),
            property: "a".to_string(),
            value: Box::new(Expr::Number(1.0)),
            byte_offset: 0,
            byte_end: 0,
        }),
        Stmt::Expr(Expr::PropertyUpdate {
            object: Box::new(Expr::This),
            property: "b".to_string(),
            op: BinaryOp::Add,
            prefix: false,
            strict: true,
            byte_offset: 0,
            byte_end: 0,
        }),
    ]);
    assert_eq!(
        proven_receiver_clone_benefit_sites(&m, &fields, &HashSet::new()),
        2
    );
    assert!(tower_route_profitable(&m, &fields, &HashSet::new()));
}

/// A proven-receiver clone also removes the public method guard at a nested
/// `this.helper()` boundary. It counts only when the resolved helper has its
/// own clone capability; otherwise that boundary remains generic.
#[test]
fn tower_route_counts_nested_proven_receiver_clone() {
    let fields = declared(&["a"]);
    let nested = Expr::Call {
        byte_end: 0,
        callee: Box::new(Expr::PropertyGet {
            byte_end: 0,
            object: Box::new(Expr::This),
            property: "helper".to_string(),
            byte_offset: 0,
        }),
        args: Vec::new(),
        type_args: Vec::new(),
        byte_offset: 0,
    };
    let m = method(vec![Stmt::Expr(this_get("a")), Stmt::Expr(nested)]);

    assert_eq!(
        proven_receiver_clone_benefit_sites(&m, &fields, &HashSet::new()),
        1
    );
    assert!(!tower_route_profitable(&m, &fields, &HashSet::new()));

    let methods = declared(&["helper"]);
    assert_eq!(
        proven_receiver_clone_benefit_sites(&m, &fields, &methods),
        2
    );
    assert!(tower_route_profitable(&m, &fields, &methods));
}
