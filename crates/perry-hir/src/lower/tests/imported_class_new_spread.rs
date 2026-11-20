use crate::ir::{CallArg, Expr, Module, Stmt};

fn lower(source: &str) -> Module {
    let parsed = perry_parser::parse_typescript(source, "imported-new.ts").unwrap();
    crate::lower::lower_module(&parsed, "imported-new", "imported-new.ts").unwrap()
}

fn result(module: &Module) -> &Expr {
    module
        .init
        .iter()
        .find_map(|stmt| match stmt {
            Stmt::Let { name, init, .. } if name == "result" => init.as_ref(),
            _ => None,
        })
        .expect("result is initialized")
}

#[test]
fn imported_constructors_preserve_spread_positions() {
    for class in ["Plain", "Path"] {
        for (arguments, count, mixed) in [
            ("...['kind']", 1, false),
            ("...['a', 'b']", 2, false),
            ("...['kind'], 'x'", 1, true),
            ("...[]", 0, false),
        ] {
            let hir = lower(&format!(
                "import {{ {class} }} from './classes.ts'; const result = new {class}({arguments});"
            ));
            let Expr::NewDynamicSpread { callee, args, .. } = result(&hir) else {
                panic!("{class}({arguments}) loses spread: {:?}", result(&hir));
            };
            assert!(matches!(callee.as_ref(), Expr::ExternFuncRef { name, .. } if name == class));
            assert_eq!(args.len(), if mixed { 2 } else { 1 });
            assert!(
                matches!(&args[0], CallArg::Spread(Expr::Array(values, _)) if values.len() == count)
            );
            if mixed {
                assert!(matches!(&args[1], CallArg::Expr(Expr::String(value)) if value == "x"));
            }
        }
    }
}

#[test]
fn imported_constructors_without_spread_keep_static_arguments() {
    let hir = lower("import { Plain } from './classes.ts'; const result = new Plain('kind', 'x');");
    assert!(matches!(result(&hir), Expr::New { class_name, args, .. }
        if class_name == "Plain" && args.len() == 2
            && matches!(&args[0], Expr::String(value) if value == "kind")
            && matches!(&args[1], Expr::String(value) if value == "x")));
}

#[test]
fn native_constructors_keep_export_specific_routing() {
    let hir = lower("import { Worker as Thread } from 'node:worker_threads'; const result = new Thread('entry.cjs');");
    assert!(matches!(result(&hir), Expr::WorkerNew { filename, .. }
        if matches!(filename.as_ref(), Expr::String(value) if value == "entry.cjs")));
}
