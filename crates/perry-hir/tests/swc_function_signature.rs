use perry_hir::types::Type;
use perry_hir::{lower_module, Expr, Stmt};
use perry_parser::parse_typescript;

#[test]
fn type_only_this_keeps_signature_metadata_without_a_runtime_parameter() {
    let ast = parse_typescript(
        "const callback = function(this: string, value: number): number { return value; };",
        "signature.ts",
    )
    .unwrap();
    let hir = lower_module(&ast, "signature", "signature.ts").unwrap();
    let binding = hir
        .init
        .iter()
        .find(|stmt| matches!(stmt, Stmt::Let { name, .. } if name == "callback"))
        .unwrap();
    let Stmt::Let {
        ty: Type::Function(signature),
        init: Some(Expr::Closure { params, .. }),
        ..
    } = binding
    else {
        panic!("expected a function binding and closure");
    };
    assert_eq!(
        signature.params,
        vec![
            ("this".to_string(), Type::String, false),
            ("value".to_string(), Type::Number, false),
        ]
    );
    assert_eq!(params.len(), 1);
    assert_eq!(params[0].name, "value");
}
