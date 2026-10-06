//! RegExp and user instances share ordinary method calls.

use perry_diagnostics::SourceCache;
use perry_hir::{lower_module, Module};
use perry_parser::parse_typescript_with_cache;

fn lower(src: &str) -> Module {
    let mut cache = SourceCache::new();
    let parsed = parse_typescript_with_cache(src, "/tmp/instance_test_method.ts", &mut cache)
        .expect("parse failed");
    lower_module(&parsed.module, "test", "/tmp/instance_test_method.ts").expect("lower failed")
}

fn module_has_method_call(module: &Module, name: &str) -> bool {
    fn has(expr: &perry_hir::Expr, name: &str) -> bool {
        if let perry_hir::Expr::Call { callee, .. } = expr {
            if matches!(callee.as_ref(), perry_hir::Expr::PropertyGet { property, .. } if property == name)
            {
                return true;
            }
        }
        let mut found = false;
        perry_hir::walker::walk_expr_children(expr, &mut |child| found |= has(child, name));
        found
    }
    let mut found = false;
    for stmt in &module.init {
        found |= perry_hir::walker::stmt_any_expr(stmt, &mut |expr| has(expr, name));
    }
    found
}

#[test]
fn every_test_receiver_uses_an_ordinary_method_call() {
    for source in [
        "const c: any = makeComparator(); const hit = c.test(10);",
        "const obj: any = getObject(); const hit = obj.matcher.test('file');",
        "const hit = /foo/.test('foobar');",
        "const r: RegExp = /foo/; const hit = r.test('foobar');",
    ] {
        assert!(module_has_method_call(&lower(source), "test"), "{source}");
    }
}

#[test]
fn typed_exec_uses_an_ordinary_method_call() {
    assert!(module_has_method_call(
        &lower("const r: RegExp = /foo/; const match = r.exec('foo');"),
        "exec"
    ));
}

/// True if any body debug-prints a `StringMatch`/`StringMatchAll` node.
fn module_has_string_match(module: &Module) -> bool {
    let dbg = format!("{:#?}", module);
    dbg.contains("StringMatch")
}

#[test]
fn chained_new_dot_match_is_not_string_match() {
    // minimatch's `minimatch(p, pat)` arrow returns
    // `new Minimatch(pat).match(p)` — a chained `new X(arg).match(arg)` where
    // `arg` is an untyped param. `.match()` here is an INSTANCE method, not
    // `String.prototype.match(regex)`. The old heuristic (untyped arg ⇒
    // regex) lowered it to `StringMatch(new Minimatch(pat), p)`, so the call
    // returned `null` instead of the boolean match result.
    let src = r#"
        function f(p: any, pat: any) {
            return new Matcher(pat).match(p);
        }
    "#;
    let module = lower(src);
    assert!(
        !module_has_string_match(&module),
        "`new Matcher(pat).match(p)` must NOT lower to StringMatch"
    );
}

#[test]
fn string_match_regex_literal_still_uses_fast_path() {
    // A regex *literal* arg keeps the StringMatch fast path.
    let src = r#"
        const m = "abc123".match(/\d+/);
    "#;
    let module = lower(src);
    assert!(
        module_has_string_match(&module),
        "`\"...\".match(/\\d+/)` (regex literal arg) should still lower to StringMatch"
    );
}
