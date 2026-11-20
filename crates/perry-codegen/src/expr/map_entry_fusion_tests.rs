//! Map get-then-set fusion (`map_entry_fusion`): which shapes fuse, and that
//! every call between the `get` and the `set` zeroes the entry slot first.
use crate::temp_root_coverage::main_ir_for as ir_for;
use perry_hir::types::Type;
use perry_hir::{BinaryOp, Expr, Stmt};
use std::sync::Mutex;

// The sabotage test flips a process-wide switch the call-emission choke point reads.
static SWITCH: Mutex<()> = Mutex::new(());

const M: u32 = 1;
const M2: u32 = 2;
const K: u32 = 3;
const K2: u32 = 4;
const CUR: u32 = 5;

fn local(id: u32, mutable: bool, init: Expr) -> Stmt {
    Stmt::Let {
        id,
        name: format!("v{id}"),
        ty: Type::Any,
        mutable,
        init: Some(init),
    }
}

fn get(m: u32, k: u32) -> Expr {
    Expr::MapGet {
        map: Box::new(Expr::LocalGet(m)),
        key: Box::new(Expr::LocalGet(k)),
    }
}

fn set(m: u32, k: u32, value: Expr) -> Expr {
    Expr::MapSet {
        map: Box::new(Expr::LocalGet(m)),
        key: Box::new(Expr::LocalGet(k)),
        value: Box::new(value),
    }
}

fn plus_one(e: Expr) -> Expr {
    Expr::Binary {
        op: BinaryOp::Add,
        left: Box::new(e),
        right: Box::new(Expr::Number(1.0)),
    }
}

fn prologue() -> Vec<Stmt> {
    vec![
        local(M, false, Expr::MapNew),
        local(M2, false, Expr::MapNew),
        local(K, false, Expr::String("k".into())),
        local(K2, false, Expr::String("j".into())),
    ]
}

fn fused(ir: &str) -> bool {
    ir.contains("@js_map_get_entry") && ir.contains("map_entry.set.fast")
}

/// Text from the get call to the first later occurrence of `callee`.
fn between<'a>(ir: &'a str, callee: &str) -> &'a str {
    let from = ir.find("@js_map_get_entry").expect("a fused get");
    let rest = &ir[from..];
    let to = rest
        .find(&format!("@{callee}("))
        .unwrap_or_else(|| panic!("no call to {callee}:\n{ir}"));
    &rest[..to]
}

#[test]
fn the_one_expression_form_fuses() {
    let _g = SWITCH.lock().unwrap_or_else(|e| e.into_inner());
    let mut stmts = prologue();
    stmts.push(Stmt::Expr(set(M, K, plus_one(get(M, K)))));
    let ir = ir_for("pip_fuse_expr", stmts);
    assert!(fused(&ir), "one-expression form did not fuse:\n{ir}");
}

#[test]
fn the_two_statement_form_fuses() {
    let _g = SWITCH.lock().unwrap_or_else(|e| e.into_inner());
    let mut stmts = prologue();
    stmts.push(local(CUR, false, get(M, K)));
    stmts.push(Stmt::Expr(set(M, K, plus_one(Expr::LocalGet(CUR)))));
    let ir = ir_for("pip_fuse_stmt", stmts);
    assert!(fused(&ir), "two-statement form did not fuse:\n{ir}");
}

#[test]
fn a_different_key_or_receiver_does_not_fuse() {
    let _g = SWITCH.lock().unwrap_or_else(|e| e.into_inner());
    for (name, get_m, get_k, set_m, set_k) in [
        ("pip_nofuse_key", M, K, M, K2),
        ("pip_nofuse_recv", M, K, M2, K),
    ] {
        let mut stmts = prologue();
        stmts.push(local(CUR, false, get(get_m, get_k)));
        stmts.push(Stmt::Expr(set(set_m, set_k, plus_one(Expr::LocalGet(CUR)))));
        let ir = ir_for(name, stmts);
        assert!(!ir.contains("@js_map_get_entry"), "{name} fused:\n{ir}");
        assert!(!ir.contains("map_entry.set"), "{name} fused:\n{ir}");
        let mut stmts = prologue();
        stmts.push(Stmt::Expr(set(set_m, set_k, plus_one(get(get_m, get_k)))));
        let ir = ir_for(name, stmts);
        assert!(!ir.contains("@js_map_get_entry"), "{name} fused:\n{ir}");
    }
}

#[test]
fn a_reassigned_receiver_does_not_fuse() {
    let _g = SWITCH.lock().unwrap_or_else(|e| e.into_inner());
    let mut stmts = prologue();
    stmts[0] = local(M, true, Expr::MapNew);
    stmts.push(local(CUR, false, get(M, K)));
    stmts.push(Stmt::Expr(Expr::LocalSet(M, Box::new(Expr::MapNew))));
    stmts.push(Stmt::Expr(set(M, K, plus_one(Expr::LocalGet(CUR)))));
    let ir = ir_for("pip_nofuse_reassigned", stmts);
    assert!(
        !ir.contains("@js_map_get_entry"),
        "reassigned receiver fused:\n{ir}"
    );
}

#[test]
fn a_delete_or_clear_between_zeroes_the_slot_before_its_call() {
    let _g = SWITCH.lock().unwrap_or_else(|e| e.into_inner());
    for (name, mid, callee) in [
        (
            "pip_between_delete",
            Expr::MapDelete {
                map: Box::new(Expr::LocalGet(M)),
                key: Box::new(Expr::LocalGet(K)),
            },
            "js_map_delete",
        ),
        (
            "pip_between_clear",
            Expr::MapClear(Box::new(Expr::LocalGet(M))),
            "js_map_clear",
        ),
    ] {
        let mut stmts = prologue();
        stmts.push(local(CUR, false, get(M, K)));
        stmts.push(Stmt::Expr(mid));
        stmts.push(Stmt::Expr(set(M, K, plus_one(Expr::LocalGet(CUR)))));
        let ir = ir_for(name, stmts);
        let span = between(&ir, callee);
        assert!(
            span.contains("store i64 0, ptr"),
            "{name}: the slot cell is not zeroed before {callee}:\n{ir}"
        );
    }
}

#[test]
fn a_call_in_the_value_computation_zeroes_the_slot_before_it() {
    let _g = SWITCH.lock().unwrap_or_else(|e| e.into_inner());
    let mut stmts = prologue();
    let clear = Expr::MapClear(Box::new(Expr::LocalGet(M)));
    let value = Expr::Binary {
        op: BinaryOp::Add,
        left: Box::new(get(M, K)),
        right: Box::new(Expr::Sequence(vec![clear, Expr::Number(1.0)])),
    };
    stmts.push(Stmt::Expr(set(M, K, value)));
    let ir = ir_for("pip_value_call", stmts);
    assert!(fused(&ir), "{ir}");
    assert!(
        between(&ir, "js_map_clear").contains("store i64 0, ptr"),
        "the slot cell is not zeroed before the value computation's call:\n{ir}"
    );
}

#[test]
fn sabotage_skipping_the_call_edge_zeroing_is_caught() {
    let _g = SWITCH.lock().unwrap_or_else(|e| e.into_inner());
    std::env::set_var("PERRY_PIP_SABOTAGE", "map_entry_call_edge");
    let mut stmts = prologue();
    stmts.push(local(CUR, false, get(M, K)));
    stmts.push(Stmt::Expr(Expr::MapClear(Box::new(Expr::LocalGet(M)))));
    stmts.push(Stmt::Expr(set(M, K, plus_one(Expr::LocalGet(CUR)))));
    let ir = ir_for("pip_sabotage", stmts);
    std::env::remove_var("PERRY_PIP_SABOTAGE");
    let span = between(&ir, "js_map_clear");
    assert!(
        !span.contains("store i64 0, ptr"),
        "the sabotage did not remove the zeroing, so the checks above cannot fail:\n{ir}"
    );
}
