//! An escaped function that is not summarized: unknown code that calls it
//! gets the result of its own call only (ECMA-262 10.2.1 [[Call]]: the
//! body runs with that call's `this` and arguments). What the body returns
//! from the caller's own `this` or an argument is what that caller passed;
//! the results the program's known calls got went to those calls. What the
//! body returns from anything else (a field, a closure or module variable,
//! a birth, a callee's result, a parameter the body writes) unknown callers
//! may obtain: it escapes. Strict code: sloppy mapped `arguments` aliases
//! the parameters with the arguments object's fields, which the body may
//! write: a parameter there is sunk as before.
use super::*;

fn facts_of(src: &str) -> ProgramFacts {
    let path = "main.ts".to_string();
    let module = perry_parser::parse_typescript(src, &path).expect("parses");
    let hir = perry_hir::lower_module(&module, "main", &path).expect("lowers");
    let list: Vec<(String, &perry_hir::Module)> = vec![(path, &hir)];
    let mut cfg = FactsConfig::default();
    cfg.max_work.get_or_insert(50_000_000);
    crate::compute(&list, &cfg)
}

/// Is the first site of `.key` after `marker` proven?
fn proven_at(src: &str, key: &str, marker: &str) -> (bool, String) {
    let pf = facts_of(src);
    let at = src.find(marker).expect("marker") as u32;
    let s = pf
        .sites
        .iter()
        .filter(|s| s.key == key && s.byte_offset >= at)
        .min_by_key(|s| s.byte_offset)
        .unwrap_or_else(|| panic!("site .{key} after {marker:?} in\n{}", pf.dump_text()));
    (!matches!(s.verdict, Verdict::No(_)), pf.dump_text())
}

/// `add` reads `arguments` (as three.js `Object3D::add` does): no summary.
fn returns_this(extra: &str) -> String {
    format!(
        "\"use strict\";
        export {{}};
        class Kid {{ k = 1; }}
        class N {{
            tag = 7;
            kids: any[] = [];
            add(o: any) {{ if (arguments.length > 1) return this; const t = /*B*/ this.tag; if (t) this.kids.push(o); return this; }}
        }}
        const a = new N();
        const r = a.add(new Kid());
        new Function(process.argv[2])(N.prototype.add);
        {extra}
        console.log(/*A*/ a.tag, r.tag);"
    )
}

/// node: prints `7 7`. Unknown code calling `add` gets its own `this`
/// back, never `a`: `a.tag` stays proven.
#[test]
fn a_method_returning_this_hands_no_known_receiver_out() {
    let (p, dump) = proven_at(&returns_this(""), "tag", "/*A*/");
    assert!(p, "a escaped through the shared return of add\n{dump}");
}

/// Unknown code calls the escaped `add` with any `this`: the read in its
/// body holds an unknown receiver (proven only behind its entry guard).
#[test]
fn the_escaped_method_still_runs_on_an_unknown_receiver() {
    let src = returns_this("");
    let pf = facts_of(&src);
    let at = src.find("/*B*/").expect("marker") as u32;
    let s = pf
        .sites
        .iter()
        .filter(|s| s.key == "tag" && s.byte_offset >= at)
        .min_by_key(|s| s.byte_offset)
        .expect("site");
    let dump = pf.dump_text();
    let line = format!("S 0 {} .tag ", s.byte_offset);
    assert!(
        dump.lines().any(|l| l.starts_with(&line) && l.contains(" top=1 ")),
        "this of the escaped add holds no unknown receiver\n{dump}"
    );
}

/// Control: `a` itself handed to unknown code escapes.
#[test]
fn a_known_receiver_handed_out_escapes() {
    let (p, dump) = proven_at(
        &returns_this("new Function(process.argv[3])(a);"),
        "tag",
        "/*A*/",
    );
    assert!(!p, "a is handed to unknown code\n{dump}");
}

fn returns_param(body: &str) -> String {
    format!(
        "\"use strict\";
        export {{}};
        class S {{ s = 1; }}
        const secret = new S();
        function id(x: any) {{ if (arguments.length > 1) return 0; {body} }}
        const y = id(secret);
        new Function(process.argv[2])(id);
        console.log(/*A*/ secret.s, y.s);"
    )
}

/// node: prints `1 1`. `id(u)` by unknown code returns `u`: `secret`,
/// which only the program's own call passed, stays the program's.
#[test]
#[ignore = "open (OVMB24): with `arguments` read (no summary) S still escapes through the unknown call; not traced; the same function without `arguments` (summarized) is green"]
fn a_function_returning_its_parameter_hands_no_known_argument_out() {
    let (p, dump) = proven_at(&returns_param("return x;"), "s", "/*A*/");
    assert!(p, "secret escaped through the shared return of id\n{dump}");
}

/// Control: a parameter the body writes (`id()` returns `secret`).
#[test]
fn a_parameter_the_body_writes_is_returned_to_unknown_callers() {
    let (p, dump) = proven_at(
        &returns_param("if (!x) x = secret; return x;"),
        "s",
        "/*A*/",
    );
    assert!(!p, "id() returns secret to unknown code\n{dump}");
}

/// Control: a value from shared state: a module variable.
#[test]
fn a_returned_module_value_escapes() {
    let (p, dump) = proven_at(&returns_param("return secret;"), "s", "/*A*/");
    assert!(!p, "unknown callers get secret\n{dump}");
}

/// Unknown code holds only `N.prototype.get`: a call of it on an object of
/// its own returns that object's field, and `a` (the only holder of
/// `registry`) never reaches it. The program's own `a.get()` runs in its
/// own instance (a strict `arguments` is per run), so `registry` stays.
#[test]
fn a_returned_field_of_this_stays_with_a_receiver_unknown_code_never_holds() {
    let src = "export {};
        class Reg { r = 1; }
        const registry = new Reg();
        class N { registry = registry; get() { if (arguments.length > 1) return 0; return this.registry; } }
        const a = new N();
        a.get();
        new Function(process.argv[2])(N.prototype.get);
        console.log(/*A*/ registry.r);";
    let (p, dump) = proven_at(src, "r", "/*A*/");
    assert!(p, "unknown code never holds `a`\n{dump}");
}

/// Control (open twin): a rest parameter beside `arguments` keeps one shared
/// body, whose escape run's `this` is every receiver: a field of `this`
/// (`return this.registry`, the registry a module object), read through the
/// known receiver, is sunk as before.
#[test]
fn a_returned_field_of_this_escapes() {
    let src = "export {};
        class Reg { r = 1; }
        const registry = new Reg();
        class N { registry = registry; get(...xs: any[]) { if (arguments.length > xs.length) return 0; return this.registry; } }
        const a = new N();
        a.get();
        new Function(process.argv[2])(N.prototype.get);
        console.log(/*A*/ registry.r);";
    let (p, dump) = proven_at(src, "r", "/*A*/");
    assert!(!p, "the registry is returned to unknown code\n{dump}");
}

/// Control: a fresh object holding a closure variable.
#[test]
fn a_fresh_object_holding_a_closure_variable_escapes() {
    let src = "export {};
        class S { s = 1; }
        function maker() {
            const cap = new S();
            function mk(x: any) { if (arguments.length > 1) return 0; return { v: cap, x }; }
            mk(1);
            new Function(process.argv[2])(mk);
            return cap;
        }
        const c = maker();
        console.log(/*A*/ c.s);";
    let (p, dump) = proven_at(src, "s", "/*A*/");
    assert!(!p, "cap is reachable from what mk returns\n{dump}");
}
