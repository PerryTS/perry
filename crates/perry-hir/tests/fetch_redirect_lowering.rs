//! Regression coverage for #10474: RequestInit.redirect must survive AST to
//! HIR lowering so native codegen can select follow, manual, or error behavior.

use perry_diagnostics::SourceCache;
use perry_hir::{lower_module, Expr};
use perry_parser::parse_typescript_with_cache;

fn lower_src(src: &str) -> anyhow::Result<perry_hir::Module> {
    let mut cache = SourceCache::new();
    let parsed = parse_typescript_with_cache(src, "fetch_redirect.ts", &mut cache)?;
    lower_module(&parsed.module, "test", "fetch_redirect.ts")
}

fn redirect_expr(module: &perry_hir::Module) -> Option<Expr> {
    fn walk(expr: &Expr, found: &mut Option<Expr>) {
        if let Expr::FetchWithOptions { redirect, .. } = expr {
            *found = redirect.as_deref().cloned();
        }
        perry_hir::walker::walk_expr_children(expr, &mut |child| walk(child, found));
    }

    let mut found = None;
    for stmt in &module.init {
        if let perry_hir::Stmt::Expr(expr) = stmt {
            walk(expr, &mut found);
        }
    }
    found
}

#[test]
fn redirect_literal_is_preserved() {
    let module = lower_src(r#"fetch("http://example.test/start", { redirect: "manual" });"#)
        .expect("fetch redirect should lower");

    assert!(matches!(
        redirect_expr(&module),
        Some(Expr::String(mode)) if mode == "manual"
    ));
}

#[test]
fn redirect_shorthand_is_preserved() {
    let module = lower_src(
        r#"
        const redirect = "error";
        fetch("http://example.test/start", { redirect });
        "#,
    )
    .expect("fetch redirect shorthand should lower");

    assert!(
        redirect_expr(&module).is_some(),
        "shorthand redirect option must remain attached to FetchWithOptions"
    );
}
