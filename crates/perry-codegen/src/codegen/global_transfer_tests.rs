use super::*;
use perry_hir::types::Type;
use perry_hir::Expr;

#[test]
fn structural_leaf_kinds_and_unknown_initializers_are_candidates() {
    let call = Expr::Binary {
        op: perry_hir::BinaryOp::Add,
        left: Box::new(Expr::String("a".into())),
        right: Box::new(Expr::Number(1.0)),
    };
    assert!(may_hold_leaf(
        &Expr::String("s".into()),
        Some(&Type::String)
    ));
    assert!(may_hold_leaf(&call, Some(&Type::BigInt)));
    assert!(
        may_hold_leaf(&call, None),
        "unknown kinds are decided by the producer's tag"
    );
}

#[test]
fn proven_nonleaf_kinds_and_structural_allocations_are_not_candidates() {
    assert!(!may_hold_leaf(&Expr::Number(1.0), Some(&Type::Number)));
    assert!(!may_hold_leaf(&Expr::Bool(true), Some(&Type::Boolean)));
    assert!(!may_hold_leaf(&Expr::Object(Vec::new()), None));
    assert!(!may_hold_leaf(&Expr::Array(Vec::new()), None));
}

#[test]
fn generated_symbols_are_keyed_by_producer_and_binding() {
    let t = GlobalTransfer::new("helper_ts", 7, 2);
    assert_eq!(t.canonical, "perry_global_helper_ts__7");
    assert_eq!(t.cell, "perry_gpub_helper_ts__7");
    assert_eq!(t.agent_block, "perry_gtc_helper_ts");
    assert_eq!(t.slot, 2);
    assert_eq!(t.accessor, "perry_gread_helper_ts__7");
}
