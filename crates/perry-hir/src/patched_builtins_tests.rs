use std::sync::Arc;

use perry_diagnostics::SourceCache;
use perry_parser::parse_typescript_with_cache;

use super::*;
use crate::ir::{Expr, Stmt};
use crate::lower_module;

fn scan(src: &str) -> PatchedBuiltins {
    let mut cache = SourceCache::new();
    let parsed = parse_typescript_with_cache(src, "test.ts", &mut cache).expect("parse");
    let mut out = PatchedBuiltins::new();
    scan_module(&parsed.module, &mut out);
    out
}

fn pair(r: &str, m: &str) -> (String, String) {
    (r.to_string(), m.to_string())
}

#[test]
fn scans_member_writes_on_namespaces() {
    let s = scan(
        r#"
        console.log = () => {};
        (Math as any)["max"] = () => 0;
        globalThis.JSON.stringify = () => "";
        Object.defineProperty(Reflect, "ownKeys", { value: () => [] });
        Object.assign(Number, { isNaN() { return false; } });
        "#,
    );
    assert!(s.contains(&pair("console", "log")));
    assert!(s.contains(&pair("Math", "max")));
    assert!(s.contains(&pair("JSON", "stringify")));
    assert!(s.contains(&pair("Reflect", "ownKeys")));
    assert!(s.contains(&pair("Number", "isNaN")));
    assert_eq!(s.len(), 5, "{s:?}");
}

#[test]
fn dynamic_key_and_whole_namespace_writes_are_wildcards() {
    let s = scan(
        r#"
        for (const m of ["log", "error"]) (console as any)[m] = () => {};
        globalThis.JSON = { parse: () => 1 } as any;
        "#,
    );
    assert!(s.contains(&pair("console", ANY_MEMBER)));
    assert!(s.contains(&pair("JSON", ANY_MEMBER)));
}

#[test]
fn prototype_writes_are_not_namespace_patches() {
    let s = scan(
        r#"
        Array.prototype.join = function () { return "J"; };
        Object.defineProperty(String.prototype, "trim", { value: () => "" });
        "#,
    );
    assert!(s.is_empty(), "{s:?}");
}

#[test]
fn reads_and_ordinary_objects_are_not_patches() {
    let s = scan(
        r#"
        const f = console.log;
        const o = { log() {} };
        o.log = () => {};
        Math.max(1, 2);
        process.stdout.write = (() => true) as any;
        const k = "x";
        (globalThis as any)[k] = 1;
        Object.assign(globalThis, { foo: 1 });
        "#,
    );
    assert!(s.is_empty(), "{s:?}");
}

fn lower_with_patches(src: &str) -> crate::Module {
    let src = src.to_string();
    std::thread::Builder::new()
        .stack_size(32 * 1024 * 1024)
        .spawn(move || {
            let mut cache = SourceCache::new();
            let parsed = parse_typescript_with_cache(&src, "test.ts", &mut cache).expect("parse");
            let mut set = PatchedBuiltins::new();
            scan_module(&parsed.module, &mut set);
            set_patched_builtins(Arc::new(set));
            let module = lower_module(&parsed.module, "test", "test.ts").expect("lower");
            clear_patched_builtins();
            module
        })
        .expect("spawn")
        .join()
        .expect("lowering thread")
}

/// The init statement `const r = <init>;` for local `r`.
fn init_of(module: &crate::Module, idx: usize) -> String {
    let lets: Vec<_> = module
        .init
        .iter()
        .filter_map(|s| match s {
            Stmt::Let { init: Some(e), .. } => Some(e),
            _ => None,
        })
        .collect();
    format!("{:?}", lets[idx])
}

fn is_dynamic_member_call(dbg: &str, property: &str) -> bool {
    dbg.starts_with("Call {") && dbg.contains(&format!("property: \"{property}\""))
}

#[test]
fn patched_namespace_call_lowers_dynamically() {
    let m = lower_with_patches(
        r#"
        (Math as any).max = () => "MINE";
        const r = Math.max(1, 2);
        const s = Math.min(1, 2);
        "#,
    );
    let r = init_of(&m, 0);
    assert!(is_dynamic_member_call(&r, "max"), "{r}");
    let s = init_of(&m, 1);
    assert!(
        s.starts_with("MathMin"),
        "unpatched sibling keeps the intrinsic: {s}"
    );
}

#[test]
fn unpatched_program_keeps_intrinsics() {
    let m = lower_with_patches("const r = Math.max(1, 2);");
    let r = init_of(&m, 0);
    assert!(r.starts_with("MathMax"), "{r}");
    assert!(!matches!(
        m.init.first(),
        Some(Stmt::Expr(Expr::Call { .. }))
    ));
}

#[test]
fn patched_console_call_lowers_dynamically() {
    let m = lower_with_patches(
        r#"
        const seen: any[] = [];
        console.error = (...a: any[]) => { seen.push(a); };
        console.error("x");
        "#,
    );
    let last = format!("{:?}", m.init.last().expect("stmt"));
    assert!(
        last.contains("Call {") && last.contains("property: \"error\""),
        "{last}"
    );
    assert!(!last.contains("NativeMethodCall"), "{last}");
}
