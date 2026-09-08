use crate::{dynamic_import::for_each_dynamic_import, Expr};

fn lower(source: &str) -> crate::Module {
    let ast = perry_parser::parse_typescript(source, "main.ts").unwrap();
    let hir = crate::lower::lower_module(&ast, "main", "main.ts").unwrap();
    crate::ir::clear_current_module_source();
    hir
}

#[test]
fn import_meta_require_spellings_use_synchronous_dispatch() {
    let hir = lower(
        r#"
        const require = (value: string) => value;
        import.meta.require("./first.js");
        import.meta["require"]("./second.js");
        ((import.meta as any)[("require")])("./third.js");
        import.meta.require(process.argv[2]);
        require("ordinary");
        const object = { require(value: string) { return value; } };
        object.require("ordinary");
        console.log(import.meta.url, import.meta.main);
    "#,
    );
    let mut args = Vec::new();
    for_each_dynamic_import(&hir, &mut |expr| {
        let Expr::DynamicImport {
            arg,
            synchronous,
            deferred_error,
            ..
        } = expr
        else {
            unreachable!();
        };
        assert!(*synchronous);
        assert!(deferred_error.is_none());
        args.push(arg.as_ref().clone());
    });
    assert_eq!(args.len(), 4);
    for (arg, path) in args.iter().zip(["./first.js", "./second.js", "./third.js"]) {
        assert!(matches!(arg, Expr::String(value) if value == path));
    }
    assert!(!matches!(args[3], Expr::Undefined));
}

#[test]
fn import_meta_require_native_literal_uses_the_existing_namespace() {
    let hir = lower("const os = import.meta.require('node:os');");
    assert!(hir.init.iter().any(|stmt| matches!(stmt,
        crate::Stmt::Let { init: Some(Expr::NativeModuleRef(name)), .. } if name == "os"
    )));
}
