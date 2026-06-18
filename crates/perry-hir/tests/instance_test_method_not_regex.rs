//! Batch-3 functional-correctness fix (semver / minimatch source-compile):
//! `receiver.test(arg)` on an `Any`/`Unknown`/untyped local must NOT be
//! lowered to the RegExp `Expr::RegExpTest` codegen fast path. `.test()` is
//! also a common INSTANCE method name (semver's `Comparator.test` /
//! `Range.test`), and an imported class instance is typed `Any` at the call
//! site. The old heuristic (`Type::Any | Type::Unknown | unwrap_or(true)`)
//! mis-lowered `comparator.test(v)` to `js_regexp_test(comparator-as-string)`,
//! silently returning a bogus boolean and never running the real method body —
//! so `semver.satisfies(...)` always returned `false`.
//!
//! The runtime already routes a genuine RegExp receiver's `.test()`/`.exec()`
//! through dynamic method dispatch (`dispatch_regex_receiver_method`, #1731),
//! so falling through to a normal method call is correct for BOTH a regex
//! value and a class instance. A regex *literal* receiver still takes the fast
//! path.

use perry_diagnostics::SourceCache;
use perry_hir::{lower_module, Module};
use perry_parser::parse_typescript_with_cache;

fn lower(src: &str) -> Module {
    let mut cache = SourceCache::new();
    let parsed = parse_typescript_with_cache(src, "/tmp/instance_test_method.ts", &mut cache)
        .expect("parse failed");
    lower_module(&parsed.module, "test", "/tmp/instance_test_method.ts").expect("lower failed")
}

/// True if any function/init body in the module debug-prints a `RegExpTest`
/// node. Using the Debug rendering keeps the test independent of the internal
/// walker API surface.
fn module_has_regexp_test(module: &Module) -> bool {
    format!("{:#?}", module).contains("RegExpTest")
}

#[test]
fn untyped_receiver_test_call_is_not_regexp_test() {
    // `c` is an untyped local (the `as any` mirror of an imported class
    // instance). `c.test(10)` must lower to a normal method call, not a
    // RegExpTest fast path.
    let src = r#"
        const c: any = makeComparator();
        const r = c.test(10);
    "#;
    let module = lower(src);
    assert!(
        !module_has_regexp_test(&module),
        "`c.test(10)` on an untyped local must NOT lower to RegExpTest"
    );
}

#[test]
fn member_receiver_test_call_is_not_regexp_test() {
    // A member-access receiver (`this.set[i].test(f)` shape) must also fall
    // through to dynamic dispatch.
    let src = r#"
        function f(obj: any, file: any) {
            return obj.matcher.test(file);
        }
    "#;
    let module = lower(src);
    assert!(
        !module_has_regexp_test(&module),
        "`obj.matcher.test(file)` must NOT lower to RegExpTest"
    );
}

#[test]
fn regex_literal_receiver_test_call_still_uses_fast_path() {
    // A regex *literal* receiver has positive evidence and keeps the fast path.
    let src = r#"
        const hit = /foo/.test("foobar");
    "#;
    let module = lower(src);
    assert!(
        module_has_regexp_test(&module),
        "`/foo/.test(...)` (regex literal) should still lower to RegExpTest"
    );
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
