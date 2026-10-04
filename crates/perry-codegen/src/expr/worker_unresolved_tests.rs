//! #11450: an unresolvable `new Worker(x)` is an expression, so its consumer
//! keeps lowering. Every consumer shape must yield valid IR. The site asks the
//! runtime worker entry table for the file, which throws only if no entry
//! matches.
use perry_hir::{types::Type, Expr, Stmt};

fn unresolved_worker() -> Expr {
    Expr::WorkerNew {
        paths: vec![],
        filename: Box::new(Expr::String("unresolved-worker".into())),
        options: None,
        is_eval: false,
        partial: false,
    }
}

fn local(id: u32, name: &str, ty: Type, init: Expr) -> Stmt {
    Stmt::Let {
        id,
        name: name.into(),
        ty,
        mutable: true,
        init: Some(init),
    }
}

/// Compile `body` as a function next to a module-global array (id 1), then
/// parse AND verify the emitted module with LLVM.
fn assert_valid_ir(case: &str, body: Vec<Stmt>) {
    let mut module = perry_hir::Module::new("worker_unresolved");
    module.init = vec![local(
        1,
        "globals",
        Type::Array(Box::new(Type::Any)),
        Expr::Array(vec![]),
    )];
    module.functions.push(perry_hir::Function {
        id: 2,
        name: "consume".into(),
        type_params: vec![],
        params: vec![],
        return_type: Type::Any,
        body,
        is_async: false,
        is_generator: false,
        is_strict: true,
        is_exported: false,
        captures: vec![],
        decorators: vec![],
        was_plain_async: false,
        was_unrolled: false,
    });
    let ir = String::from_utf8(
        crate::compile_module(&module, crate::temp_root_coverage::entry_opts())
            .unwrap_or_else(|e| panic!("{case}: compile_module failed: {e:#}")),
    )
    .unwrap();
    assert!(
        ir.contains("@js_worker_threads_worker_new_by_spec("),
        "{case}: the unresolved Worker did not fall back to the entry table"
    );
    let llvm = inkwell::context::Context::create();
    let parsed = crate::inprocess::parse_ir_text(&llvm, &ir, case)
        .unwrap_or_else(|e| panic!("{case}: {e:#}\n{ir}"));
    parsed
        .verify()
        .unwrap_or_else(|e| panic!("{case}: LLVM verifier: {}\n{ir}", e.to_string()));
}

#[test]
fn unresolved_worker_consumers_emit_valid_ir() {
    let arr = Type::Array(Box::new(Type::Any));
    let cases: Vec<(&str, Vec<Stmt>)> = vec![
        (
            "array_push",
            vec![
                local(3, "ws", arr.clone(), Expr::Array(vec![])),
                Stmt::Expr(Expr::ArrayPush {
                    array_id: 3,
                    value: Box::new(unresolved_worker()),
                    field_writeback: None,
                }),
                Stmt::Return(Some(Expr::LocalGet(3))),
            ],
        ),
        (
            "property_set",
            vec![
                local(3, "o", Type::Any, Expr::Object(vec![])),
                Stmt::Expr(Expr::PropertySet {
                    object: Box::new(Expr::LocalGet(3)),
                    property: "w".into(),
                    value: Box::new(unresolved_worker()),
                }),
                Stmt::Return(Some(Expr::LocalGet(3))),
            ],
        ),
        (
            "map_set",
            vec![
                local(3, "m", Type::Any, Expr::MapNew),
                Stmt::Expr(Expr::MapSet {
                    map: Box::new(Expr::LocalGet(3)),
                    key: Box::new(Expr::Integer(1)),
                    value: Box::new(unresolved_worker()),
                }),
                Stmt::Return(Some(Expr::LocalGet(3))),
            ],
        ),
        (
            "global_index_set",
            vec![Stmt::Expr(Expr::IndexSet {
                object: Box::new(Expr::LocalGet(1)),
                index: Box::new(Expr::Integer(0)),
                value: Box::new(unresolved_worker()),
            })],
        ),
        (
            "local_index_set",
            vec![
                local(3, "ws", arr, Expr::Array(vec![])),
                Stmt::Expr(Expr::IndexSet {
                    object: Box::new(Expr::LocalGet(3)),
                    index: Box::new(Expr::Integer(0)),
                    value: Box::new(unresolved_worker()),
                }),
                Stmt::Return(Some(Expr::LocalGet(3))),
            ],
        ),
    ];
    for (case, body) in cases {
        assert_valid_ir(case, body);
    }
}
