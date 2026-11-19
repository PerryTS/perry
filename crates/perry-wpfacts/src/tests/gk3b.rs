//! GLOBK3 (GLOB2 G3): `Object.defineProperty(globalThis, k, d)` under a
//! computed key proven never a Symbol, with no setter, defines one
//! string-named global binding and writes nothing else (ECMA-262 20.1.2.4,
//! 10.1.6.3, 9.1.1.4): every global binding read sees {its original} ∪
//! {the descriptor's `value`, what a call of its `get` returns}; no builtin
//! member and no symbol-keyed property is written.
use super::*;
use perry_hir::Module;

fn hirs(mods: &[(&str, &str)]) -> Vec<(String, Module)> {
    mods.iter()
        .map(|(name, src)| {
            let path = format!("{name}.ts");
            let module = perry_parser::parse_typescript(src, &path).expect("parses");
            let mut hir = perry_hir::lower_module(&module, name, &path).expect("lowers");
            for imp in &mut hir.imports {
                if let Some(n) = imp.source.strip_prefix("./") {
                    imp.resolved_path = Some(format!("{n}.ts"));
                }
            }
            (path, hir)
        })
        .collect()
}

fn facts_mods(mods: &[(&str, &str)]) -> ProgramFacts {
    let h = hirs(mods);
    let list: Vec<(String, &Module)> = h.iter().map(|(p, m)| (p.clone(), m)).collect();
    let mut cfg = FactsConfig::default();
    cfg.max_work.get_or_insert(50_000_000);
    crate::compute(&list, &cfg)
}

/// The verdict of the first site `.key` at or after `marker` in `src`
/// (keys are unique to the module that holds the marker).
fn verdict_in(pf: &ProgramFacts, src: &str, key: &str, marker: &str) -> Verdict {
    let at = src.find(marker).expect("marker") as u32;
    pf.sites
        .iter()
        .filter(|s| s.key == key && s.byte_offset >= at)
        .min_by_key(|s| s.byte_offset)
        .unwrap_or_else(|| panic!("site .{key} after {marker:?} in\n{}", pf.dump_text()))
        .verdict
        .clone()
}

fn verdict(src: &str, key: &str, marker: &str) -> (Verdict, String) {
    let pf = facts_mods(&[("main", src)]);
    (verdict_in(&pf, src, key, marker), pf.dump_text())
}

fn proven(v: &Verdict) -> bool {
    !matches!(v, Verdict::No(_))
}

fn not_dead(v: &Verdict) -> bool {
    !format!("{v:?}").contains("Dead")
}

/// `define` runs, then `n` goes to builtin members (`forEach`,
/// `console.warn`, `Date.now`): with the realm held, `n` escapes.
fn program(define: &str) -> String {
    format!(
        "class N {{ v = 1; }}
        class A {{ a = 1; }}
        const g = globalThis as Record<string, unknown>;
        {define}
        const n = new N();
        [n].forEach((x) => console.log(x.v));
        console.warn(n);
        const t = Date.now();
        console.log(/*A*/ n.v, n instanceof N, t > 0);"
    )
}

/// RED (the app's first hold, fixture gl4_fx6): a getter under a
/// parameter key leaves the realm unheld: `Date.now` and `n.v` proven.
#[test]
fn gk3b_red_getter_param_key() {
    let src = program(
        "function install(name: string, make: () => unknown): void { Object.defineProperty(globalThis, name, { get: make, configurable: true }); }
        install(\"nativeThing\" + (process.argv[2] ?? \"\"), () => new A());",
    );
    let (v, dump) = verdict(&src, "v", "/*A*/");
    assert!(proven(&v), "n.v proven, got {v:?}\n{dump}");
}

/// RED: a data descriptor under an unknown String key.
#[test]
fn gk3b_red_value_unknown_key() {
    let src = program("Object.defineProperty(g, String(process.argv[2]), { value: A, configurable: true, writable: true });");
    let (v, dump) = verdict(&src, "v", "/*A*/");
    assert!(proven(&v), "n.v proven, got {v:?}\n{dump}");
}

/// KEEP (a): the getter runs at the read: its program object result is what
/// a global read yields (`.w` reads `leak`, not Dead). A read of
/// `globalThis` itself may be `leak` too (the key may be "globalThis"), so
/// the define may also run on `leak`: the old path there (not proven).
#[test]
fn gk3b_keep_getter_result_visible() {
    let src = "class L { w = 1; }
        const leak = new L();
        Object.defineProperty(globalThis, String(process.argv[2]), { get() { return leak; }, configurable: true });
        const got: any = (globalThis as any).someName;
        got.w = 5;
        console.log(/*B*/ got.w, /*C*/ leak.w);";
    let (v, dump) = verdict(src, "w", "/*B*/");
    assert!(not_dead(&v), "got.w reads leak, got {v:?}\n{dump}");
}

/// KEEP (c): a key that may be `undefined` (ToPropertyKey "undefined",
/// non-configurable: the define throws a TypeError) leaves `undefined`
/// itself unreplaced; the rule still applies.
#[test]
fn gk3b_keep_undefined_key() {
    let src = program(
        "const k: any = process.argv[2] === \"u\" ? undefined : process.argv[2];
        try { Object.defineProperty(g, k, { value: A }); } catch (e) { console.log(\"TypeError\"); }
        console.log(undefined === void 0);",
    );
    let (v, dump) = verdict(&src, "v", "/*A*/");
    assert!(proven(&v), "n.v proven, got {v:?}\n{dump}");
}

/// KEEP: `{ value }` define, then a delete under the same key: the read
/// after may still see the defined class (a define that may come later).
#[test]
fn gk3b_keep_value_then_delete() {
    let src = program(
        "const k = String(process.argv[2]);
        Object.defineProperty(g, k, { value: A, configurable: true });
        delete g[k];
        const C: any = (globalThis as any).Thing;
        console.log(/*C*/ new C().a);",
    );
    let (v, dump) = verdict(&src, "a", "/*C*/");
    assert!(not_dead(&v), "C may be A, got {v:?}\n{dump}");
}

/// KEEP (b): the old path where the rule does not apply.
#[test]
fn gk3b_keep_old_path() {
    for define in [
        // the key may be a Symbol (ToPropertyKey 7.1.19)
        "const k: any = process.argv[2] === \"s\" ? Symbol.iterator : process.argv[2];\n        Object.defineProperty(g, k, { get: () => A, configurable: true });",
        // a setter (program stores of the name would call it)
        "Object.defineProperty(g, String(process.argv[2]), { get: () => A, set(v: any) { console.log(v); }, configurable: true });",
        // not the global object alone
        "const o: any = process.argv[2] === \"o\" ? globalThis : Array.prototype;\n        Object.defineProperty(o, String(process.argv[3]), { get: () => A, configurable: true });",
        // defineProperties: stays on the old path
        "Object.defineProperties(g, { [String(process.argv[2])]: { get: () => A, configurable: true } });",
    ] {
        let src = program(define);
        let (v, dump) = verdict(&src, "v", "/*A*/");
        assert!(!proven(&v), "{define}: n escapes as before, got {v:?}\n{dump}");
    }
}

/// KEEP: define in one module, a bare-name read in another.
#[test]
fn gk3b_keep_cross_module_bare_name() {
    let lib = "export class A { zq = 1; }
        export function install(name: string): void { Object.defineProperty(globalThis, name, { get: () => A, configurable: true }); }";
    let main = "import { install } from \"./lib\";
        install(String(process.argv[2]));
        const C: any = Thing;
        const t = Date.now();
        console.log(/*C*/ new C().zq, t);";
    let pf = facts_mods(&[("lib", lib), ("main", main)]);
    let v = verdict_in(&pf, main, "zq", "/*C*/");
    assert!(
        not_dead(&v),
        "Thing may be A, got {v:?}\n{}",
        pf.dump_text()
    );
}

/// E5 exact key set: a `define(name, get)` helper over the global object;
/// `g` holds the program's `A`, then `{defs}`; `globalThis.Object` and `g`
/// are read after.
fn e5_src(defs: &str) -> String {
    format!(
        "class A {{ zz = 1; }}
        class B {{ zz = \"b\"; }}
        (globalThis as any).g = new A();
        const gg = globalThis as Record<string, unknown>;
        function define(name: string, get: () => unknown): void {{ Object.defineProperty(gg, name, {{ get, configurable: true }}); }}
        {defs}
        const O: any = (globalThis as any).Object; console.log(O.keys(new A()).length);
        console.log((globalThis as any).g.zz);"
    )
}

/// The `g.zz` read may see a `B` (or ⊤).
fn e5_g_sees_b(pf: &ProgramFacts) -> bool {
    pf.sites
        .iter()
        .filter(|s| s.key == "zz")
        .any(|s| s.top || s.receivers.iter().any(|r| r.ends_with(":B") || r.contains(":B#")))
}

fn e5_object_top(pf: &ProgramFacts) -> bool {
    pf.sites.iter().filter(|s| s.key == "keys").any(|s| s.top)
}

/// RED (E5 exact key set): the helper called with three literal names
/// defines those three globals and no other: `g` stays the program's `A`
/// and `globalThis.Object` the intrinsic. (Was: every global binding read
/// took what any computed-key define put there, the getters' `B`.)
#[test]
fn e5_exact_literal_names_write_only_those_keys() {
    let src = e5_src(
        "define(\"innerWidth\", () => new B()); define(\"innerHeight\", () => new B()); define(\"screen\", () => new B());",
    );
    let pf = facts_mods(&[("main", &src)]);
    assert!(!e5_g_sees_b(&pf), "g is A\n{}", pf.dump_text());
    assert!(!e5_object_top(&pf), "Object known\n{}", pf.dump_text());
}

/// KEEP: one of the literal names is `g`: its read sees the getter's `B`.
#[test]
fn e5_exact_defined_name_is_read() {
    let src = e5_src("define(\"innerWidth\", () => 1); define(\"g\", () => new B());");
    let pf = facts_mods(&[("main", &src)]);
    assert!(e5_g_sees_b(&pf), "g may be B\n{}", pf.dump_text());
}

/// KEEP: a computed key from unknown input may be any name, `g` included.
#[test]
fn e5_unknown_key_stays_any_key() {
    let src = e5_src("define(String(process.argv[3]), () => new B());");
    let pf = facts_mods(&[("main", &src)]);
    assert!(e5_g_sees_b(&pf), "g may be B\n{}", pf.dump_text());
}

/// KEEP: a non-constant key beside the literals, before or after them,
/// widens the define to any key.
#[test]
fn e5_later_non_constant_widens() {
    let lits = "define(\"innerWidth\", () => new B()); define(\"screen\", () => new B());";
    let dynk = "define(String(process.argv[3]), () => new B());";
    for defs in [format!("{lits}\n{dynk}"), format!("{dynk}\n{lits}")] {
        let pf = facts_mods(&[("main", &e5_src(&defs))]);
        assert!(e5_g_sees_b(&pf), "{defs}: g may be B\n{}", pf.dump_text());
    }
}

/// CHW13 named define on the global object: `g` holds the program's `A`,
/// then `{defs}`; `globalThis.Object`, `g`, `navigator` and `thing` are
/// read after.
fn nd_src(defs: &str) -> String {
    format!(
        "class A {{ zz = 1; }}
        class B {{ zz = \"b\"; }}
        class N {{ nn = 1; }}
        (globalThis as any).g = new A();
        const gg = globalThis as Record<string, unknown>;
        {defs}
        const O: any = (globalThis as any).Object; console.log(O.keys(new A()).length);
        console.log((globalThis as any).g.zz);
        console.log(/*NAV*/(globalThis as any).navigator.nn);
        console.log(/*THING*/(globalThis as any).thing.nn);"
    )
}

/// The first `.nn` read after `marker`: (it may be an `N`, it is ⊤).
fn nd_nn(pf: &ProgramFacts, src: &str, marker: &str) -> (bool, bool) {
    let at = src.find(marker).expect("marker") as u32;
    let s = pf
        .sites
        .iter()
        .filter(|s| s.key == "nn" && s.byte_offset >= at)
        .min_by_key(|s| s.byte_offset)
        .unwrap_or_else(|| panic!("site .nn after {marker}\n{}", pf.dump_text()));
    let n = s.receivers.iter().any(|r| r.ends_with(":N") || r.contains(":N#"));
    (n, s.top)
}

/// RED (CHW13): a value define on the global object under a constant key
/// (`navigator`, as the app's browser shim does, and a fresh name) writes
/// that key only: `g` stays the program's `A`, `globalThis.Object` the
/// intrinsic, and a read of each defined key gets the defined `N`.
#[test]
fn nd_value_define_writes_that_key_only() {
    let src = nd_src(
        "Object.defineProperty(gg, \"navigator\", { configurable: true, enumerable: true, writable: true, value: new N() });
        Object.defineProperty(globalThis, \"thing\", { value: new N(), configurable: true });",
    );
    let pf = facts_mods(&[("main", &src)]);
    assert!(!e5_g_sees_b(&pf), "g is A\n{}", pf.dump_text());
    assert!(!e5_object_top(&pf), "Object known\n{}", pf.dump_text());
    for m in ["/*NAV*/", "/*THING*/"] {
        let (n, top) = nd_nn(&pf, &src, m);
        assert!(n && !top, "{m} reads the defined N\n{}", pf.dump_text());
    }
}

/// KEEP: an accessor define under a constant key: its getter's result
/// reaches the readers of that key (`navigator` too, whose native accessor
/// a plain store would not replace).
#[test]
fn nd_accessor_define_getter_reaches_readers() {
    let src = nd_src(
        "Object.defineProperty(gg, \"navigator\", { get: () => new N(), configurable: true });
        Object.defineProperty(gg, \"thing\", { get: () => new N(), configurable: true });",
    );
    let pf = facts_mods(&[("main", &src)]);
    for m in ["/*NAV*/", "/*THING*/"] {
        let (n, top) = nd_nn(&pf, &src, m);
        assert!(n || top, "{m} may read the getter's N\n{}", pf.dump_text());
    }
}

/// KEEP: a define of `g` itself (value or getter) is what `g` reads.
#[test]
fn nd_define_of_the_read_name_is_read() {
    for d in [
        "Object.defineProperty(gg, \"g\", { value: new B(), configurable: true });",
        "Object.defineProperty(gg, \"g\", { get: () => new B(), configurable: true });",
    ] {
        let pf = facts_mods(&[("main", &nd_src(d))]);
        assert!(e5_g_sees_b(&pf), "{d}: g may be B\n{}", pf.dump_text());
    }
}

/// KEEP: a constant-key define on ⊤ that is not the global object keeps
/// the ⊤ arm (`note_native_data_write` through ⊤ under the key): with the
/// realm not in ⊤'s reach, `g` and `Object` stay as they were (as on base).
#[test]
fn nd_define_on_top_keeps_today() {
    let src = nd_src(
        "const u: any = JSON.parse(String(process.argv[2])); Object.defineProperty(u, \"g\", { value: new B(), configurable: true });",
    );
    let pf = facts_mods(&[("main", &src)]);
    assert!(!e5_g_sees_b(&pf), "g is A\n{}", pf.dump_text());
    assert!(!e5_object_top(&pf), "Object known\n{}", pf.dump_text());
}
