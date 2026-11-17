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
