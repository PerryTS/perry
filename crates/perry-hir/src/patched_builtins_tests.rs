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
    assert!(!s.contains(&pair("Array", "join")), "{s:?}");
    assert!(!s.contains(&pair("Array", ANY_MEMBER)), "{s:?}");
    assert!(!s.iter().any(|(ns, _)| ns.starts_with("String")), "{s:?}");
}

#[test]
fn scans_builtin_prototype_method_writes() {
    let s = scan(
        r#"
        Array.prototype.push = function () { return 0; };
        (Map.prototype as any)["get"] = () => 1;
        globalThis.Function.prototype.bind = function () { return this; };
        Object.defineProperty(Set.prototype, "add", { value: () => 0 });
        String.prototype.trim = () => "";
        class Foo {}
        (Foo.prototype as any).push = () => 0;
        "#,
    );
    assert!(s.contains(&pair("Array.prototype", "push")));
    assert!(s.contains(&pair("Map.prototype", "get")));
    assert!(s.contains(&pair("Function.prototype", "bind")));
    assert!(s.contains(&pair("Set.prototype", "add")));
    assert_eq!(s.len(), 4, "{s:?}");
    assert_eq!(
        patched_prototype_methods(&s),
        vec!["add", "bind", "get", "push"]
    );
}

#[test]
fn write_through_a_prototype_alias_is_a_patch() {
    let s = scan(
        r#"
        const AP: any = Array.prototype;
        AP.push = function () { return 0; };
        const o: any = {};
        o.pop = () => 0;
        "#,
    );
    assert!(s.contains(&pair("Array.prototype", "push")), "{s:?}");
    assert_eq!(s.len(), 1, "{s:?}");
}

#[test]
fn numeric_key_prototype_write_is_an_index_not_every_method() {
    let s = scan(
        r#"
        Object.defineProperty(Array.prototype, 7, { get() { return 1; }, configurable: true });
        (Array.prototype as any)[3] = 1;
        "#,
    );
    assert!(s.is_empty(), "{s:?}");
}

#[test]
fn dynamic_key_prototype_write_patches_every_method() {
    let s = scan(
        r#"
        const k = "push";
        (Array.prototype as any)[k] = () => 0;
        "#,
    );
    assert!(s.contains(&pair("Array.prototype", ANY_MEMBER)), "{s:?}");
    assert_eq!(patched_prototype_methods(&s), vec![ANY_MEMBER]);
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

#[test]
fn patched_prototype_method_call_lowers_dynamically() {
    let m = lower_with_patches(
        r#"
        (Array.prototype as any).push = function () { return 0; };
        const a: number[] = [1];
        const r = a.push(2);
        const mp = new Map<string, number>();
        const g = mp.get("k");
        "#,
    );
    let r = init_of(&m, 1);
    assert!(is_dynamic_member_call(&r, "push"), "{r}");
    let g = init_of(&m, 3);
    assert!(
        g.starts_with("MapGet"),
        "an unpatched method keeps its intrinsic: {g}"
    );
}

#[test]
fn unpatched_program_keeps_array_push_intrinsic() {
    let m = lower_with_patches(
        r#"
        const a: number[] = [1];
        const r = a.push(2);
        "#,
    );
    let r = init_of(&m, 1);
    assert!(r.starts_with("ArrayPush"), "{r}");
}
