//! SNAPK2d: the `process.env` / `process.argv` census kinds come from the
//! read's own enclosing lowering. 71710bc707 kept address-keyed lists of
//! store / grain sites, noted when the parent was lowered and taken when the
//! read was. A parent lowered without its read (a materialised receiver is
//! re-read, not re-lowered) left its entry behind, and a later module's read
//! at the freed address took the earlier module's kind.

use perry_hir::types::Type;
use perry_hir::{BinaryOp, Expr, Module, Stmt};

fn ir_of(m: &Module) -> String {
    String::from_utf8(
        crate::compile_module(
            m,
            crate::CompileOptions {
                emit_ir_only: true,
                is_entry_module: true,
                target: Some("x86_64-unknown-linux-gnu".into()),
                ..Default::default()
            },
        )
        .unwrap(),
    )
    .unwrap()
}

fn local(id: u32, init: Expr) -> Stmt {
    Stmt::Let {
        id,
        name: format!("k2d_{id}"),
        ty: Type::Any,
        mutable: true,
        init: Some(init),
    }
}

fn env() -> Expr {
    Expr::EnvGet("K2D".into())
}

fn argv_length() -> Expr {
    Expr::PropertyGet {
        object: Box::new(Expr::ProcessArgv),
        property: "length".into(),
        byte_offset: 0,
        byte_end: 0,
    }
}

fn argv_at(i: i64) -> Expr {
    Expr::IndexGet {
        object: Box::new(Expr::ProcessArgv),
        index: Box::new(Expr::Integer(i)),
        byte_offset: 0,
        byte_end: 0,
        element_offset: 0,
    }
}

/// `receiver.at(0)`: a folded builtin whose receiver is materialised once and
/// re-read by both arms of its own-override guard.
fn at0(receiver: Expr) -> Expr {
    Expr::ArrayAt {
        array: Box::new(receiver),
        index: Box::new(Expr::Integer(0)),
    }
}

fn concat(e: Expr) -> Expr {
    Expr::Binary {
        op: BinaryOp::Add,
        left: Box::new(e),
        right: Box::new(Expr::String(String::new())),
    }
}

/// `receiver.has(0)` as a folded `Map` method: the same materialised receiver.
fn has0(receiver: Expr) -> Expr {
    Expr::MapHas {
        map: Box::new(receiver),
        key: Box::new(Expr::Integer(0)),
    }
}

/// `(kind, index)` of every census call of `hook` in `ir`, in order.
fn kinds(ir: &str, hook: &str) -> Vec<String> {
    let call = format!("call void @{hook}(");
    ir.lines()
        .filter_map(|l| l.split_once(&call).map(|(_, a)| a.trim_end().to_owned()))
        .collect()
}

/// Module A: store / grain parents that are materialised receivers. Module B,
/// built after A is dropped so its nodes reuse A's freed addresses: only
/// observe env reads and whole-array argv reads.
#[test]
fn env_argv_kinds_never_cross_reads() {
    std::env::set_var("PERRY_SNAP_CENSUS", "1");
    for round in 0..8 {
        let mut a = Module::new("k2d_a");
        for i in 0..16u32 {
            a.init
                .push(local(6 * i, at0(Expr::Array(vec![env()], None))));
            a.init.push(local(6 * i + 1, at0(argv_length())));
            a.init.push(local(6 * i + 2, at0(argv_at(1))));
            a.init
                .push(local(6 * i + 3, has0(Expr::Array(vec![env()], None))));
            a.init.push(local(6 * i + 4, has0(argv_length())));
            a.init.push(local(6 * i + 5, has0(argv_at(1))));
        }
        let ir_a = ir_of(&a);
        drop(a);
        assert!(!kinds(&ir_a, "js_snap_env_site").is_empty(), "{ir_a}");
        let mut b = Module::new("k2d_b");
        for i in 0..96u32 {
            b.init.push(local(2 * i, concat(env())));
            b.init.push(local(2 * i + 1, concat(Expr::ProcessArgv)));
        }
        let ir_b = ir_of(&b);
        let env_b = kinds(&ir_b, "js_snap_env_site");
        let argv_b = kinds(&ir_b, "js_snap_argv_site");
        assert_eq!(env_b.len(), 96, "round {round}: one hook per env read");
        assert_eq!(argv_b.len(), 96, "round {round}: one hook per argv read");
        let stray_env = env_b.iter().filter(|k| *k != "i32 0)").count();
        let stray_argv = argv_b.iter().filter(|k| *k != "i32 0, i64 0)").count();
        assert_eq!(
            (stray_env, stray_argv),
            (0, 0),
            "round {round}: B has only observe env reads and escaping argv reads"
        );
    }
    // The kinds still follow the read's own parent.
    let mut c = Module::new("k2d_c");
    c.init
        .push(local(0, Expr::Array(vec![env(), concat(env())], None)));
    c.init.push(local(1, argv_length()));
    c.init.push(local(2, argv_at(2)));
    c.init.push(local(3, concat(Expr::ProcessArgv)));
    let ir_c = ir_of(&c);
    std::env::remove_var("PERRY_SNAP_CENSUS");
    assert_eq!(
        kinds(&ir_c, "js_snap_env_site"),
        ["i32 1)", "i32 0)"],
        "array element stores, operand observes: {ir_c}"
    );
    assert_eq!(
        kinds(&ir_c, "js_snap_argv_site"),
        ["i32 1, i64 0)", "i32 2, i64 2)", "i32 0, i64 0)"],
        "length, element 2, escape: {ir_c}"
    );
}
