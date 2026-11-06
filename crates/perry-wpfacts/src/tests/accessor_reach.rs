//! An export namespace built from accessor-descriptor literals (esbuild's
//! `__export` / `__defProp(target, "k", { get: () => k, enumerable: true })`,
//! npm typescript.js's 2,244 exports): a getter runs only for a key a lookup
//! reached, and the getter a define installs is the literal's own `get`, at
//! every key form. Reading `ns.e00` runs `e00`'s getter and `e00` only; a
//! program that never reads the namespace runs no getter; a computed read,
//! a key walk, an escape or `JSON.stringify` run every getter.

use super::*;

const NS: &str = r#"
const ns: any = {};
Object.defineProperty(ns, "e00", { get: () => (g00(), e00), enumerable: true });
Object.defineProperty(ns, "e01", { get: () => (g01(), e01), enumerable: true });
Object.defineProperty(ns, "e02", { get: () => (g02(), e02), enumerable: true });
Object.defineProperty(ns, "e03", { get: () => (g03(), e03), enumerable: true });
Object.defineProperty(ns, "e04", { get: () => (g04(), e04), enumerable: true });
function e00(x: number) { return x + 1; }
function e01(x: number) { return x + 2; }
function e02(x: number) { return x + 3; }
function e03(x: number) { return x + 4; }
function e04(x: number) { return x + 5; }
function g00() { return 0; }
function g01() { return 1; }
function g02() { return 2; }
function g03() { return 3; }
function g04() { return 4; }
"#;

const EXPORTS: [&str; 5] = ["e00", "e01", "e02", "e03", "e04"];
/// `gNN` runs exactly when the getter of `eNN` runs.
const GETTERS: [&str; 5] = ["g00", "g01", "g02", "g03", "g04"];

/// What can run in `NS` + `main`: the exports and the number of getters.
fn active(main: &str) -> (Vec<String>, usize) {
    let src = format!("{NS}\n{main}");
    let module = perry_parser::parse_typescript(&src, "t.ts").expect("parses");
    let hir = perry_hir::lower_module(&module, "t", "t.ts").expect("lowers");
    let list = [("t.ts".to_string(), &hir)];
    let pf = crate::compute(&list, &FactsConfig::default());
    assert!(pf.body_reach.complete, "activated");
    let on = &pf.body_reach.bodies;
    let id_of = |e: &str| hir.functions.iter().find(|f| f.name == e).map(|f| f.id);
    let runs = |id: u32| on.iter().any(|(_, i, _)| *i == id);
    let exports: Vec<String> = EXPORTS
        .iter()
        .filter(|e| id_of(e).is_some_and(runs))
        .map(|e| e.to_string())
        .collect();
    let getters = GETTERS.iter().filter(|g| id_of(g).is_some_and(runs)).count();
    (exports, getters)
}

#[test]
fn a_named_read_runs_its_getter_only() {
    // The join (TSA3 m1): every same-shape descriptor literal shares one
    // constructor, so a read of the built descriptor's `get` held all five
    // getters, and `ns.e00` reached every export.
    let (exports, getters) = active("console.log(ns.e00(1));");
    assert_eq!(exports, ["e00"]);
    assert_eq!(getters, 1);
}

#[test]
fn an_unread_namespace_runs_no_getter() {
    // TSA3 m7: installing an accessor no lookup reached ran it.
    let (exports, getters) = active("console.log(1);");
    assert!(exports.is_empty(), "{exports:?}");
    assert_eq!(getters, 0);
}

#[test]
fn every_read_that_can_name_any_key_runs_every_getter() {
    for main in [
        // A computed key.
        "const k = \"e0\" + String(Date.now() % 5); console.log(ns[k](1));",
        // A key walk.
        "for (const k of Object.keys(ns)) console.log(ns[k](1));",
        // An escape to unknown code.
        "import \"unseen-host\"; const g = (globalThis as any)[\"x\" + \"y\"]; if (g) g(ns); console.log(0);",
    ] {
        let (exports, getters) = active(main);
        assert_eq!(exports, EXPORTS, "{main}");
        assert_eq!(getters, 5, "{main}");
    }
    // JSON.stringify runs every getter and calls none of the functions.
    let (exports, getters) = active("console.log(JSON.stringify(ns));");
    assert!(exports.is_empty(), "{exports:?}");
    assert_eq!(getters, 5);
}
