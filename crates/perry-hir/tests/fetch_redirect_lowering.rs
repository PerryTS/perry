//! RequestInit.redirect remains part of the complete evaluated init (#10474).
use perry_diagnostics::SourceCache;
use perry_hir::{lower_module, Expr, Stmt};
use perry_parser::parse_typescript_with_cache;

fn redirect(src: &str) -> Option<Expr> {
    let mut cache = SourceCache::new();
    let parsed = parse_typescript_with_cache(src, "redirect.ts", &mut cache).unwrap();
    let module = lower_module(&parsed.module, "test", "redirect.ts").unwrap();
    let mut found = None;
    fn walk(expr: &Expr, module: &perry_hir::Module, found: &mut Option<Expr>) {
        if let Expr::Call { callee, args, .. } = expr {
            if callee.is_global_fetch_callee() {
                *found = match args.get(1) {
                    Some(Expr::Object(fields)) => fields
                        .iter()
                        .find(|(key, _)| key == "redirect")
                        .map(|(_, value)| value.clone()),
                    Some(Expr::New {
                        class_name, args, ..
                    }) => module
                        .classes
                        .iter()
                        .find(|class| &class.name == class_name)
                        .and_then(|class| {
                            class
                                .fields
                                .iter()
                                .position(|field| field.name == "redirect")
                        })
                        .and_then(|index| args.get(index))
                        .cloned(),
                    _ => None,
                };
            }
        }
        perry_hir::walker::walk_expr_children(expr, &mut |child| walk(child, module, found));
    }
    for stmt in &module.init {
        if let Stmt::Expr(expr) = stmt {
            walk(expr, &module, &mut found);
        }
    }
    found
}

#[test]
fn redirect_literal_is_preserved() {
    assert!(
        matches!(redirect(r#"fetch("http://x/", {redirect: "manual"});"#), Some(Expr::String(mode)) if mode == "manual")
    );
}
#[test]
fn redirect_shorthand_is_preserved() {
    assert!(redirect(r#"const redirect = "error"; fetch("http://x/", {redirect});"#).is_some());
}
#[test]
fn absent_redirect_stays_absent() {
    assert!(redirect(r#"fetch("http://x/", {method: "GET"});"#).is_none());
}
