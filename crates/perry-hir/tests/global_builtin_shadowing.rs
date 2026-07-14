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

/// The sibling of `local_isfinite_helper_zero_arg_call_does_not_use_global_builtin_arity`:
/// with NO local shadow, `isFinite()` must route to the global BUILTIN, not to a
/// user function.
///
/// This used to assert a compile-time "isFinite requires one argument" error, using
/// that diagnostic as a proxy for "the builtin path was taken". But the diagnostic
/// itself was wrong: a missing argument is not an error in JS — the parameter is
/// simply `undefined`, and `isFinite()` evaluates to `false` in Node (it never
/// throws). Rejecting it made perry refuse to compile legal JS. Assert the routing
/// directly instead: the call lowers to the `IsFinite` intrinsic with the omitted
/// argument as `Expr::Undefined`.
#[test]
fn unshadowed_global_isfinite_zero_arg_call_lowers_to_builtin_with_undefined() {
    let module = lower_src("const result = isFinite();")
        .expect("unshadowed global isFinite() is legal JS and must lower, not error");

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
        matches!(result_init, Expr::IsFinite(arg) if matches!(arg.as_ref(), Expr::Undefined)),
        "zero-arg isFinite() should reach the builtin intrinsic with an undefined \
         argument (Node evaluates it to false), got: {result_init:?}"
    );
}
