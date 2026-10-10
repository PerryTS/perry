//! The byte proof's receiver identity is the existing precise root.
use perry_hir::{types::Type, Expr, Stmt};

#[test]
fn receiver_identity_after_a_collecting_call_uses_the_relocated_root() {
    let _pin = crate::testing::NativeRootsPin::native();
    let read = || {
        Stmt::Expr(Expr::IndexGet {
            object: Box::new(Expr::LocalGet(1)),
            index: Box::new(Expr::Integer(0)),
        })
    };
    let module = crate::temp_root_coverage::module_with_init(
        "byte_identity_root",
        vec![
            Stmt::Let {
                id: 1,
                name: "view".into(),
                ty: Type::Named("Uint32Array".into()),
                mutable: true,
                init: Some(Expr::TypedArrayNew {
                    kind: perry_hir::TYPED_ARRAY_KIND_UINT32,
                    arg: Some(Box::new(Expr::Integer(64))),
                }),
            },
            Stmt::While {
                condition: Expr::Bool(true),
                body: vec![
                    read(),
                    Stmt::Expr(Expr::Call {
                        callee: Box::new(Expr::GlobalGet(100)),
                        args: vec![],
                        type_args: vec![],
                        byte_offset: 0,
                    }),
                    read(),
                ],
            },
        ],
    );
    let ir = String::from_utf8(
        crate::compile_module(&module, crate::temp_root_coverage::entry_opts()).unwrap(),
    )
    .unwrap();
    assert!(
        ir.contains("bytes.access.check"),
        "exercise receiver identity revalidation"
    );
    let rewritten = crate::inprocess::statepoint_rewritten_ir(
        &ir,
        &crate::codegen::default_target_triple(),
        "byte_identity_root",
    )
    .unwrap();
    let file = std::env::temp_dir().join(format!("perry-byte-identity-{}.ll", std::process::id()));
    std::fs::write(&file, rewritten).unwrap();
    let workspace = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .parent()
        .unwrap();
    let output = std::process::Command::new("python3")
        .arg(workspace.join("scripts/gc_root_dominance_check.py"))
        .args([
            "--statepoints",
            "--moving-only",
            "--max-stale",
            "0",
            "--min-statepoints",
            "1",
            "--min-live-bundles",
            "1",
            "--min-relocates",
            "1",
        ])
        .arg(&file)
        .current_dir(workspace)
        .output()
        .unwrap();
    std::fs::remove_file(file).unwrap();
    assert!(
        output.status.success(),
        "the receiver comparison must not carry unrooted identity across collection:\n{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn a_pure_lexical_binding_refreshes_the_byte_proof_on_every_iteration() {
    let _pin = crate::testing::NativeRootsPin::native();
    let array_type = Type::Named("Uint32Array".into());
    let module = crate::temp_root_coverage::module_with_init(
        "byte_pure_binding",
        vec![
            Stmt::Let {
                id: 2,
                name: "owner".into(),
                ty: array_type.clone(),
                mutable: false,
                init: Some(Expr::TypedArrayNew {
                    kind: perry_hir::TYPED_ARRAY_KIND_UINT32,
                    arg: Some(Box::new(Expr::Integer(64))),
                }),
            },
            Stmt::Let {
                id: 3,
                name: "views".into(),
                ty: Type::Array(Box::new(array_type.clone())),
                mutable: false,
                init: Some(Expr::Array(vec![Expr::LocalGet(2)])),
            },
            Stmt::While {
                condition: Expr::Bool(true),
                body: vec![
                    Stmt::Let {
                        id: 1,
                        name: "view".into(),
                        ty: array_type,
                        mutable: false,
                        init: Some(Expr::IndexGet {
                            object: Box::new(Expr::LocalGet(3)),
                            index: Box::new(Expr::Integer(0)),
                        }),
                    },
                    Stmt::Expr(Expr::IndexGet {
                        object: Box::new(Expr::LocalGet(1)),
                        index: Box::new(Expr::Integer(0)),
                    }),
                ],
            },
        ],
    );
    let ir = String::from_utf8(
        crate::compile_module(&module, crate::temp_root_coverage::entry_opts()).unwrap(),
    )
    .unwrap();
    assert!(
        ir.contains("bytes.access.revalidate"),
        "exercise a byte proof"
    );
    let rewritten = crate::inprocess::statepoint_rewritten_ir(
        &ir,
        &crate::codegen::default_target_triple(),
        "byte_pure_binding",
    )
    .unwrap();
    let body = crate::testing::root_slots::function_slice(&rewritten, "main");
    assert!(
        !body.lines().any(|line| line.contains("phi i8")),
        "a lexical binding cannot inherit an earlier iteration's clean/rejected proof state:\n{body}"
    );
}
