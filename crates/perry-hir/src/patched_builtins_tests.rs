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

// The scanner also carries array-iteration safety facts in a pseudo-namespace.
// These are distinct from a patch to a named namespace/prototype member.
fn assert_array_iteration_facts(set: &PatchedBuiltins, expected: &[&str]) {
    let facts: Vec<_> = set
        .iter()
        .filter(|(namespace, _)| namespace == ARRAY_ITERATION)
        .map(|(_, member)| member.as_str())
        .collect();
    assert_eq!(facts, expected, "{set:?}");
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
    assert_array_iteration_facts(&s, &["elements", "write"]);
    assert_eq!(
        s.iter()
            .filter(|(ns, _)| ns != ARRAY_ITERATION && ns != NS_ESCAPED)
            .count(),
        5,
        "{s:?}"
    );
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
    assert_array_iteration_facts(&s, &["elements", "write"]);
    assert_eq!(s.len(), 6, "{s:?}");
    assert_eq!(
        patched_prototype_methods(&s),
        vec![
            "add",
            "bind",
            "get",
            "push",
            ARRAY_ITERATOR_WRITABLE,
            ARRAY_ELEMENT_DESCRIPTORS
        ]
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
    assert_array_iteration_facts(&s, &["elements", "write"]);
    assert_eq!(s.len(), 2, "no named method was patched: {s:?}");
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
    assert_eq!(
        patched_prototype_methods(&s),
        vec![ANY_MEMBER, ARRAY_ITERATOR_WRITABLE]
    );
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
        (globalThis as any)["x"] = 1;
        Object.assign(globalThis, { foo: 1 });
        "#,
    );
    assert_array_iteration_facts(&s, &["write"]);
    // NXCALL3: `process.stdout.write = f` is a store into node:process.
    assert!(s.contains(&pair("node:process", "stdout.write")), "{s:?}");
    assert_eq!(
        s.len(),
        2,
        "ordinary writes add no namespace/prototype patch: {s:?}"
    );
}

/// DEC 255: a store into the global object under a key the scan cannot read
/// may rebind any namespace. It records the global pseudo-namespace (not
/// "every namespace replaced"), and every direct namespace call is GUARDED on
/// `Pristine(ns)`: the intrinsic stays behind the runtime's bit.
#[test]
fn non_static_global_store_guards_namespace_calls() {
    for src in [
        "const k: any = process.argv[2]; (globalThis as any)[k] = {};",
        "Object.assign(globalThis, (process as any).shims);",
        "const k: any = process.argv[2]; Object.defineProperty(globalThis, k, { value: 1 });",
        "const k: any = process.argv[2]; Reflect.set(globalThis, k, 1);",
        "const k: any = process.argv[2]; [(globalThis as any)[k]] = [1];",
    ] {
        let s = scan(src);
        assert!(
            s.contains(&pair(GLOBAL_ANY_KEY, ANY_MEMBER)),
            "{src}: {s:?}"
        );
        let m = lower_with_patches(&format!(
            "{src}\nconst r = Object.keys({{ a: 1 }});\nconst t = Math.max(1, 2);"
        ));
        // `r` and `t` are the program's last two declarations.
        let lets = m
            .init
            .iter()
            .filter(|s| matches!(s, Stmt::Let { init: Some(_), .. }))
            .count();
        let r = init_of(&m, lets - 2);
        assert!(
            r.starts_with("Conditional { condition: NamespacePatched(\"Object\")"),
            "{src}: {r}"
        );
        assert!(r.contains("property: \"keys\""), "generic arm: {r}");
        let t = init_of(&m, lets - 1);
        assert!(
            t.contains("NamespacePatched(\"Math\")") && t.contains("MathMax"),
            "{src}: {t}"
        );
    }
    // A static non-namespace key names no namespace: no guard.
    let m = lower_with_patches(
        "(globalThis as any).foo = 1; Object.assign(globalThis, { bar: 2 });\nconst r = Object.keys({ a: 1 });",
    );
    assert!(
        !init_of(&m, 0).contains("NamespacePatched"),
        "{}",
        init_of(&m, 0)
    );
}

/// DEC 255 hunk 3: a parenthesized or TS-typed target that names a namespace
/// (`(Object as any) = w`) is the store `Object = w` is. The lowering peels
/// the target to the ident, so the scan does too, and the namespace call
/// reads the binding.
#[test]
fn parenthesized_and_ts_as_global_targets_rebind_the_namespace() {
    for src in [
        "(Object as any) = w;",
        "(Object) = w;",
        "Object! = w;",
        "(<any>Object) = w;",
        "(Object satisfies any) = w;",
    ] {
        let s = scan(&format!("const w: any = {{}};\n{src}"));
        assert!(s.contains(&pair("Object", ANY_MEMBER)), "{src}: {s:?}");
        let m = lower_with_patches(&format!(
            "const w: any = {{}};\n{src}\nconst r = Object.keys({{ a: 1 }});"
        ));
        // `r` is the program's last declaration.
        let lets = m
            .init
            .iter()
            .filter(|s| matches!(s, Stmt::Let { init: Some(_), .. }))
            .count();
        let r = init_of(&m, lets - 1);
        assert!(is_dynamic_member_call(&r, "keys"), "{src}: {r}");
    }
    // The same wrapped target inside a destructuring assignment is the same
    // store. (The lowering rejects this form today, so only the scan is read.)
    let s = scan("const w: any = {};\n[(Object as any)] = [w];");
    assert!(s.contains(&pair("Object", ANY_MEMBER)), "{s:?}");
    // A wrapped target that names a local is no store into the global object:
    // the call keeps its intrinsic.
    let m = lower_with_patches(
        "let x: any = 1;\nconst w: any = {};\n(x as any) = w;\n(x) = w;\nconst r = Object.keys({ a: 1 });",
    );
    let lets = m
        .init
        .iter()
        .filter(|s| matches!(s, Stmt::Let { init: Some(_), .. }))
        .count();
    let r = init_of(&m, lets - 1);
    assert!(
        !is_dynamic_member_call(&r, "keys") && !r.contains("NamespacePatched"),
        "{r}"
    );
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
        is_dynamic_member_call(&g, "get"),
        "the reified Map prototype is read by ordinary property lookup: {g}"
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

/// #10848: a write through a local bound to the namespace patches it too.
#[test]
fn scans_member_writes_through_a_namespace_alias() {
    let s = scan(
        r#"
        const M: any = Math;
        M.min = () => "mine";
        const C = globalThis.console;
        Object.assign(C, { info() {} });
        const notNs = { max: 1 };
        notNs.max = 2;
        "#,
    );
    assert!(s.contains(&pair("Math", "min")), "{s:?}");
    assert!(s.contains(&pair("console", "info")), "{s:?}");
    assert_array_iteration_facts(&s, &["write"]);
    assert_eq!(
        s.iter()
            .filter(|(ns, _)| ns != ARRAY_ITERATION && ns != NS_ESCAPED)
            .count(),
        2,
        "{s:?}"
    );
}

/// #11772: patching `Symbol.iterator` is not patching `push`/"any member".
#[test]
fn symbol_iterator_patch_is_not_any_member() {
    let s = scan(
        r#"
        (Array.prototype as any)[Symbol.iterator] = function* () { yield 1; };
        "#,
    );
    assert!(s.contains(&pair("Array.prototype", "@@iterator")), "{s:?}");
    assert!(!s.contains(&pair("Array.prototype", "*")), "{s:?}");
    assert!(!s.contains(&pair("Array.prototype", "push")), "{s:?}");
}

#[test]
fn parsed_constant_sources_contribute_static_prototype_writes() {
    let set = scan("eval(\"Array.prototype.join = function () { return 'changed'; }\");");
    assert!(set.contains(&("Array.prototype".into(), "join".into())));
    assert!(set.dynamic_code_reach.is_empty());
    let set = scan("Function(\"Map.prototype.get = function () {};\");");
    assert!(set.contains(&("Map.prototype".into(), "get".into())));
}

fn lower_with_global_reach(src: &str) -> crate::Module {
    let src = src.to_string();
    std::thread::Builder::new()
        .stack_size(32 * 1024 * 1024)
        .spawn(move || {
            let mut cache = SourceCache::new();
            let parsed = parse_typescript_with_cache(&src, "test.ts", &mut cache).expect("parse");
            let mut set = PatchedBuiltins::new();
            scan_module(&parsed.module, &mut set);
            set.dynamic_code_reach
                .record_global("new Function(...) @ test".to_string());
            set_patched_builtins(Arc::new(set));
            let module = lower_module(&parsed.module, "test", "test.ts").expect("lower");
            clear_patched_builtins();
            module
        })
        .expect("spawn")
        .join()
        .expect("lowering thread")
}

/// #12643: under global dynamic reach a namespace call keeps its intrinsic
/// behind `NamespacePatched`, with the dynamic call as the other arm.
#[test]
fn reachable_namespace_call_is_guarded() {
    let m = lower_with_global_reach(
        r#"
        const r = Math.max(1, 2);
        const k = Object.keys({ a: 1 });
        const t = typeof JSON.stringify;
        "#,
    );
    let r = init_of(&m, 0);
    assert!(
        r.starts_with("Conditional { condition: NamespacePatched(\"Math\")"),
        "{r}"
    );
    assert!(r.contains("MathMax"), "intrinsic arm: {r}");
    assert!(r.contains("property: \"max\""), "dynamic arm: {r}");
    let k = init_of(&m, 1);
    assert!(k.contains("NamespacePatched(\"Object\")"), "{k}");
    let t = init_of(&m, 2);
    assert!(
        t.contains("NamespacePatched(\"JSON\")") && t.contains("TypeOf"),
        "{t}"
    );
}

/// A definition in the arguments exists once: such a call is the one
/// dynamic call, and a program write still wins over the guard.
#[test]
fn reachable_namespace_call_with_closure_or_program_write_is_dynamic() {
    let m = lower_with_global_reach(
        r#"
        (Math as any).min = () => 0;
        const r = Array.from([1], (x: number) => x + 1);
        const s = Math.min(1, 2);
        "#,
    );
    let r = init_of(&m, 0);
    assert!(!r.contains("NamespacePatched"), "{r}");
    let s = init_of(&m, 1);
    assert!(is_dynamic_member_call(&s, "min"), "{s}");
}

#[test]
fn unreachable_namespace_call_has_no_guard() {
    let m = lower_with_patches("const r = Math.max(1, 2); const t = typeof JSON.parse;");
    assert!(!init_of(&m, 0).contains("NamespacePatched"));
    assert_eq!(init_of(&m, 1), "String(\"function\")");
}

/// The `Expr` statement at `idx` of the module init.
fn expr_stmt_of(module: &crate::Module, idx: usize) -> String {
    let exprs: Vec<_> = module
        .init
        .iter()
        .filter_map(|s| match s {
            Stmt::Expr(e) => Some(e),
            _ => None,
        })
        .collect();
    format!("{:?}", exprs[idx])
}

#[test]
fn scans_global_function_rebinds() {
    let s = scan(
        r#"
        (globalThis as any).setTimeout = (f: any) => 0;
        global.setInterval = (f: any) => 0;
        Object.defineProperty(globalThis, "clearTimeout", { value: () => {} });
        Object.assign(globalThis, { queueMicrotask() {} });
        setImmediate = (f: any) => 0;
        const g = globalThis as any;
        g.clearImmediate = () => {};
        "#,
    );
    for name in [
        "setTimeout",
        "setInterval",
        "clearTimeout",
        "queueMicrotask",
        "setImmediate",
        "clearImmediate",
    ] {
        assert!(s.contains(&pair(GLOBAL_FUNCTION, name)), "{name}: {s:?}");
    }
    assert!(
        !s.contains(&pair(GLOBAL_FUNCTION, "clearInterval")),
        "{s:?}"
    );
    // A read, or a store onto an ordinary object, rebinds nothing.
    let s = scan("const t = globalThis.setTimeout; const o: any = {}; o.setTimeout = t;");
    assert!(!s.iter().any(|(ns, _)| ns == GLOBAL_FUNCTION), "{s:?}");
}

#[test]
fn rebound_timer_call_reads_the_global_binding() {
    let m = lower_with_patches(
        r#"
        const orig = globalThis.setTimeout;
        (globalThis as any).setTimeout = (f: any, ms: any) => orig(f, ms);
        setTimeout(() => {}, 1);
        setInterval(() => {}, 1);
        queueMicrotask(() => {});
        "#,
    );
    // Statement 0 is the store itself.
    let call = expr_stmt_of(&m, 1);
    assert!(
        call.starts_with("Call { callee: PropertyGet {")
            && call.contains("object: GlobalGet(0), property: \"setTimeout\""),
        "rebound setTimeout reads the binding: {call}"
    );
    let untouched = expr_stmt_of(&m, 2);
    assert!(
        untouched.contains("ExternFuncRef { name: \"setInterval\""),
        "unwritten sibling keeps the direct call: {untouched}"
    );
    let micro = expr_stmt_of(&m, 3);
    assert!(micro.starts_with("QueueMicrotask("), "{micro}");
}

#[test]
fn rebound_queue_microtask_call_reads_the_global_binding() {
    let m = lower_with_patches(
        r#"
        globalThis.queueMicrotask = (f: any) => {};
        queueMicrotask(() => {});
        "#,
    );
    let call = expr_stmt_of(&m, 1);
    assert!(
        !call.starts_with("QueueMicrotask(") && call.contains("property: \"queueMicrotask\""),
        "{call}"
    );
}

#[test]
fn unwritten_program_keeps_direct_timer_calls() {
    let m = lower_with_patches(
        r#"
        setTimeout(() => {}, 1);
        clearTimeout(0 as any);
        queueMicrotask(() => {});
        const k = typeof setTimeout;
        "#,
    );
    let call = expr_stmt_of(&m, 0);
    assert!(
        call.contains("ExternFuncRef { name: \"setTimeout\""),
        "{call}"
    );
    let clear = expr_stmt_of(&m, 1);
    assert!(
        clear.contains("ExternFuncRef { name: \"clearTimeout\""),
        "{clear}"
    );
    assert!(expr_stmt_of(&m, 2).starts_with("QueueMicrotask("));
    assert_eq!(init_of(&m, 0), "String(\"function\")");
}

#[test]
fn non_static_global_store_rebinds_timers() {
    let m = lower_with_patches(
        r#"
        const k: any = process.argv[2];
        (globalThis as any)[k] = () => 0;
        setTimeout(() => {}, 1);
        "#,
    );
    let call = expr_stmt_of(&m, 1);
    assert!(
        call.contains("property: \"setTimeout\"")
            && !call.contains("ExternFuncRef { name: \"setTimeout\""),
        "{call}"
    );
}

/// A computed store into the global object may rebind a global FUNCTION, but
/// never turns a program class or other non-global-function name into a
/// global read.
#[test]
fn any_key_global_store_rebinds_only_global_functions() {
    let s = scan("const k: any = process.argv[2]; (globalThis as any)[k] = 0;");
    assert!(s.contains(&pair(GLOBAL_ANY_KEY, ANY_MEMBER)), "{s:?}");
    set_patched_builtins(Arc::new(s));
    let foo = global_function_patched("Foo");
    let st = global_function_patched("setTimeout");
    clear_patched_builtins();
    assert!(!foo);
    assert!(st);
}

#[test]
fn dynamic_code_reach_rebinds_only_global_functions() {
    let mut s = PatchedBuiltins::new();
    s.dynamic_code_reach
        .record_global("new Function(...) @ test".to_string());
    set_patched_builtins(Arc::new(s));
    let foo = global_function_patched("Foo");
    let st = global_function_patched("setTimeout");
    clear_patched_builtins();
    assert!(!foo);
    assert!(st);
}

// NXCALL3: node-core namespaces under the same program-wide no-store fact.
#[test]
fn node_core_namespace_writes_and_escapes() {
    let s = scan(
        r#"
        const path = require("path");
        import * as fs from "node:fs";
        const { promises: fsp } = require("fs");
        path.join = () => "";
        path.posix.relative = () => "";
        Object.assign(fs, { existsSync: () => true });
        delete fsp.readFile;
        function f(o: any) {}
        f(require("os"));
        const { sep } = path;
        typeof path;
        path.resolve("a");
        (0, path.normalize)("a");
        "#,
    );
    for (k, m) in [
        ("node:path", "join"),
        ("node:path", "relative"),
        ("node:fs", "existsSync"),
        ("node:fs", "promises.readFile"),
        ("node:os", "$esc"),
    ] {
        assert!(s.contains(&pair(k, m)), "{k} {m}: {s:?}");
    }
    assert!(
        !s.iter().any(|(k, m)| k == "node:path"
            && matches!(m.as_str(), "*" | "resolve" | "sep" | "normalize")),
        "{s:?}"
    );
    let set = &s;
    assert!(node_ns::member_patched(set, "path", &["join"]));
    assert!(node_ns::member_patched(
        set,
        "node:path",
        &["posix", "join"]
    ));
    assert!(node_ns::member_patched(set, "path", &["win32", "relative"]));
    assert!(!node_ns::member_patched(set, "path", &["resolve"]));
    assert!(node_ns::member_patched(
        set,
        "fs",
        &["promises", "readFile"]
    ));
    assert!(node_ns::member_patched(set, "fs/promises", &["readFile"]));
    assert!(!node_ns::member_patched(set, "fs", &["promises", "stat"]));
    // Handed to `f`, which stores nothing: an escape is not a write.
    assert!(!node_ns::member_patched(set, "os", &["cpus"]));
}

#[test]
fn node_core_namespace_handed_on_is_an_escape_not_a_write() {
    let s = scan(
        r#"
        import path from "path";
        const w = path.win32;
        export { w };
        module.exports = { fs: require("fs") };
        export * from "util";
        take(process);
        "#,
    );
    // `path.win32` folds into the root on the host it equals.
    for (k, m) in [
        ("node:path", "$esc"),
        ("node:fs", "$esc"),
        ("node:util", "$esc"),
        ("node:process", "$esc"),
    ] {
        assert!(s.contains(&pair(k, m)), "{k} {m}: {s:?}");
    }
    assert!(
        !s.iter()
            .any(|(k, m)| k.starts_with("node:") && m.ends_with('*')),
        "{s:?}"
    );
    assert!(!node_ns::member_patched(&s, "path", &["join"]));
    assert!(!node_ns::member_patched(
        &s,
        "fs",
        &["promises", "readFile"]
    ));
    assert!(!node_ns::member_patched(
        &s,
        "process",
        &["hrtime", "bigint"]
    ));
}

#[test]
fn node_core_store_through_a_handed_namespace_is_a_write() {
    let s = scan(
        r#"
        const path = require("path"), fs = require("fs");
        function patch(m) { m.basename = () => "x"; }
        patch(path);
        const holder = { f: fs };
        holder.f.existsSync = () => true;
        Object.defineProperty(other(), "readdirSync", { value: 1 });
        ({}).dirname = 1;
        Object.assign({}, { relative: 1 });
        "#,
    );
    assert!(node_ns::member_patched(&s, "path", &["basename"]));
    assert!(node_ns::member_patched(&s, "path", &["posix", "basename"]));
    assert!(!node_ns::member_patched(&s, "path", &["join"]));
    assert!(node_ns::member_patched(&s, "fs", &["existsSync"]));
    assert!(node_ns::member_patched(&s, "fs", &["readdirSync"]));
    // A store onto a value the expression creates reaches no namespace.
    assert!(!node_ns::member_patched(&s, "path", &["dirname"]));
    assert!(!node_ns::member_patched(&s, "path", &["relative"]));
    // `os` never escaped: a same-named store elsewhere does not reach it.
    let t = scan(
        r#"
        const os = require("os");
        os.cpus();
        thing.cpus = 1;
        "#,
    );
    assert!(!node_ns::member_patched(&t, "os", &["cpus"]));
}

#[test]
fn node_core_namespace_reads_are_not_patches() {
    let s = scan(
        r#"
        const path = require("path");
        const { join } = path;
        const j = path.join;
        path.join("a", "b");
        process.nextTick(() => {});
        if (typeof process !== "undefined") {}
        "#,
    );
    assert!(
        !s.iter()
            .any(|(k, m)| k.starts_with("node:") && !m.ends_with(".*")),
        "{s:?}"
    );
    assert!(!node_ns::member_patched(&s, "path", &["join"]));
    assert!(!node_ns::member_patched(&s, "process", &["nextTick"]));
}

#[test]
fn node_core_escape_in_the_open_loader_arm_counts_only_when_open() {
    let mut s = scan(
        r#"
        import _path from "path";
        function require(specifier) {
            if (__perry_cjs_open()) { if (specifier === "path") { return _path; } }
            throw new Error("closed");
        }
        require(name).join = () => "";
        "#,
    );
    assert!(s.contains(&pair("node:path", "$open")), "{s:?}");
    assert!(!node_ns::member_patched(&s, "path", &["join"]));
    assert!(promote_open_node_escapes(&mut s));
    assert!(s.contains(&pair("node:path", "$esc")), "{s:?}");
    // The computed require hands `path` to program code that stores `join`.
    assert!(node_ns::member_patched(&s, "path", &["join"]));
    assert!(!node_ns::member_patched(&s, "path", &["resolve"]));
    assert!(!promote_open_node_escapes(&mut s));
}
