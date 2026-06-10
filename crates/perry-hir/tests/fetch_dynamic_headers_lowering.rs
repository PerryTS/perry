//! Regression test for #4932: `fetch()` dropped request headers when the
//! `headers` option was a dynamically-built object (a variable, a spread
//! literal, or a call such as `Object.assign`/`new Headers`/`JSON.parse`).
//!
//! Only an object *literal* with plain string/ident keys gets statically
//! extracted into `FetchWithOptions::headers`. Everything else must be
//! captured in `FetchWithOptions::headers_dynamic` and serialized at runtime,
//! otherwise the headers silently vanish.

use perry_diagnostics::SourceCache;
use perry_hir::{lower_module, Expr};
use perry_parser::parse_typescript_with_cache;

fn lower_src(src: &str) -> anyhow::Result<perry_hir::Module> {
    let mut cache = SourceCache::new();
    let parsed = parse_typescript_with_cache(src, "fetch_dynamic_headers.ts", &mut cache)?;
    lower_module(&parsed.module, "test", "fetch_dynamic_headers.ts")
}

/// Locate the single top-level `FetchWithOptions` node and report
/// `(static_header_pairs, headers_dynamic.is_some())`.
fn find_fetch(module: &perry_hir::Module) -> (usize, bool) {
    fn walk(e: &Expr, out: &mut Option<(usize, bool)>) {
        if let Expr::FetchWithOptions {
            headers,
            headers_dynamic,
            ..
        } = e
        {
            *out = Some((headers.len(), headers_dynamic.is_some()));
        }
        perry_hir::walker::walk_expr_children(e, &mut |c| walk(c, out));
    }
    let mut result = None;
    for stmt in &module.init {
        if let perry_hir::Stmt::Expr(e) = stmt {
            walk(e, &mut result);
        }
    }
    result.unwrap_or_else(|| panic!("no FetchWithOptions found in module: {module:#?}"))
}

#[test]
fn literal_headers_are_extracted_statically() {
    let module = lower_src(
        r#"fetch("http://x/", { method: "POST", headers: { "Authorization": "Bearer x" }, body: "b" });"#,
    )
    .expect("fetch with literal headers should lower");

    let (static_pairs, has_dynamic) = find_fetch(&module);
    assert_eq!(
        static_pairs, 1,
        "literal headers should be extracted statically"
    );
    assert!(
        !has_dynamic,
        "literal headers must not route through the dynamic path"
    );
}

#[test]
fn variable_headers_are_captured_as_dynamic() {
    let module = lower_src(
        r#"
        const h: Record<string, string> = {};
        h["Authorization"] = "Bearer x";
        fetch("http://x/", { method: "POST", headers: h, body: "b" });
        "#,
    )
    .expect("fetch with variable headers should lower");

    let (static_pairs, has_dynamic) = find_fetch(&module);
    assert_eq!(static_pairs, 0, "a variable headers value has no static pairs");
    assert!(
        has_dynamic,
        "a property-assigned headers object must be captured in headers_dynamic (#4932)"
    );
}

#[test]
fn spread_literal_headers_are_captured_as_dynamic() {
    // `{ ...h }` is an object literal, but its spread prop cannot be enumerated
    // statically, so it must fall back to the runtime path.
    let module = lower_src(
        r#"
        const h: Record<string, string> = {};
        h["Authorization"] = "Bearer x";
        fetch("http://x/", { headers: { ...h } });
        "#,
    )
    .expect("fetch with spread headers should lower");

    let (static_pairs, has_dynamic) = find_fetch(&module);
    assert_eq!(static_pairs, 0);
    assert!(
        has_dynamic,
        "spread-literal headers must be captured in headers_dynamic (#4932)"
    );
}

#[test]
fn call_headers_are_captured_as_dynamic() {
    // `Object.assign({}, h)` / `JSON.parse(...)` / `new Headers(h)` etc.
    let module = lower_src(
        r#"
        const h: Record<string, string> = {};
        h["Authorization"] = "Bearer x";
        fetch("http://x/", { headers: Object.assign({}, h) });
        "#,
    )
    .expect("fetch with computed headers should lower");

    let (static_pairs, has_dynamic) = find_fetch(&module);
    assert_eq!(static_pairs, 0);
    assert!(
        has_dynamic,
        "call-produced headers must be captured in headers_dynamic (#4932)"
    );
}
