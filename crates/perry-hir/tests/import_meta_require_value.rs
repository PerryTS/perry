use perry_hir::{lower_module, Expr, Stmt};
use perry_parser::parse_typescript;

#[test]
fn import_meta_require_values_share_one_module_scoped_loader() {
    let parsed = parse_typescript(
        r#"
        const load = import.meta.require;
        const computed = import.meta['require'];
        const { require: destructured } = import.meta;
        function getLoader() { return import.meta.require; }
        export { load, computed, destructured, getLoader };
    "#,
        "/tmp/owner/loader.js",
    )
    .unwrap();
    let hir = lower_module(&parsed, "loader", "/tmp/owner/loader.js").unwrap();
    let loaders: Vec<_> = hir
        .init
        .iter()
        .filter_map(|stmt| match stmt {
            Stmt::Let {
                id,
                init:
                    Some(Expr::NativeMethodCall {
                        module,
                        method,
                        args,
                        ..
                    }),
                ..
            } if module == "module" && method == "createRequire" => Some((*id, args)),
            _ => None,
        })
        .collect();
    assert_eq!(loaders.len(), 1);
    let (loader, args) = loaders[0];
    assert!(matches!(&args[0], Expr::String(url) if url == "file:///tmp/owner/loader.js"));
    for name in ["load", "computed"] {
        assert!(hir.init.iter().any(|stmt| matches!(stmt,
            Stmt::Let { name: n, init: Some(Expr::LocalGet(id)), .. } if n == name && *id == loader)));
    }
    let get = hir
        .functions
        .iter()
        .find(|f| f.name == "getLoader")
        .unwrap();
    assert!(get
        .body
        .iter()
        .any(|stmt| matches!(stmt, Stmt::Return(Some(Expr::LocalGet(id))) if *id == loader)));
}

#[test]
fn direct_import_meta_require_retains_static_graph_dispatch() {
    let parsed = parse_typescript("import.meta.require('./leaf.js');", "entry.js").unwrap();
    let hir = lower_module(&parsed, "entry", "entry.js").unwrap();
    assert!(hir.init.iter().any(|stmt| matches!(
        stmt,
        Stmt::Expr(Expr::DynamicImport {
            synchronous: true,
            ..
        })
    )));
    assert!(!hir.init.iter().any(|stmt| matches!(stmt,
        Stmt::Let { init: Some(Expr::NativeMethodCall { method, .. }), .. } if method == "createRequire")));
}
