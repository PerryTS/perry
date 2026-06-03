use perry_diagnostics::SourceCache;
use perry_hir::{lower_module, Expr, Stmt};
use perry_parser::parse_typescript_with_cache;

fn lower_src(src: &str) -> anyhow::Result<perry_hir::Module> {
    let mut cache = SourceCache::new();
    let parsed = parse_typescript_with_cache(src, "global_builtin_shadowing.ts", &mut cache)?;
    lower_module(&parsed.module, "test", "global_builtin_shadowing.ts")
}

#[test]
fn local_isfinite_helper_zero_arg_call_does_not_use_global_builtin_arity() {
    let module = lower_src(
        r#"
        function isFinite(annotations?: unknown) {
          return annotations === undefined;
        }
        const result = isFinite();
        "#,
    )
    .expect("local isFinite helper should shadow the global builtin");

    let func_id = module
        .functions
        .iter()
        .find(|func| func.name == "isFinite")
        .map(|func| func.id)
        .expect("local helper function should be registered");

    let result_init = module
        .init
        .iter()
        .find_map(|stmt| match stmt {
            Stmt::Let {
                name,
                init: Some(init),
                ..
            } if name == "result" => Some(init),
            _ => None,
        })
        .expect("result binding should be lowered");

    assert!(
        matches!(
            result_init,
            Expr::Call { callee, .. } if matches!(callee.as_ref(), Expr::FuncRef(id) if *id == func_id)
        ),
        "{result_init:?}"
    );
}

#[test]
fn unshadowed_global_isfinite_zero_arg_call_keeps_builtin_arity_error() {
    let err = lower_src("const result = isFinite();")
        .expect_err("unshadowed global isFinite() should keep the builtin arity diagnostic");
    assert!(
        err.to_string().contains("isFinite requires one argument"),
        "{err}"
    );
}
