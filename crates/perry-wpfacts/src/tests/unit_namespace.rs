//! Per-unit mode: a namespace import of another unit's module (design
//! §18.2): the namespace object's keys are the module's export names, and
//! each key holds what a named import of it binds.

use super::*;

fn units_of(mods: &[(&str, &str)]) -> ProgramFacts {
    let hirs: Vec<(String, perry_hir::Module)> = mods
        .iter()
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
        .collect();
    let list: Vec<(String, &perry_hir::Module)> =
        hirs.iter().map(|(p, h)| (p.clone(), h)).collect();
    crate::compute_units(&list, &FactsConfig::default())
}

fn verdicts(pf: &ProgramFacts, key: &str) -> Vec<String> {
    pf.sites
        .iter()
        .filter(|s| s.key == key)
        .map(|s| format!("{:?}", s.verdict))
        .collect()
}

const DEF: &str = "export const E = { A: 1, B: 2 };\nexport function f() { return 1; }";

#[test]
fn a_namespace_key_holds_what_a_named_import_binds() {
    // `p.E` is `import { E }`: the same contract, so `.A` holds on E's
    // layout through the namespace as through the named import.
    let named = units_of(&[
        ("a", DEF),
        ("m", "import { E } from './a'; function g() { return E.A; } console.log(g());"),
    ]);
    assert_eq!(verdicts(&named, "A"), ["T0Mono"], "{}", named.dump_text());
    let ns = units_of(&[
        ("a", DEF),
        ("m", "import * as p from './a'; function g() { return p.E.A + p.f(); } console.log(g());"),
    ]);
    assert_eq!(verdicts(&ns, "A"), ["T0Mono"], "{}", ns.dump_text());
    // A key that is no export reads `undefined` (the namespace has no
    // such key): nothing is proven through it.
    let pf = units_of(&[
        ("a", DEF),
        ("m", "import * as p from './a'; function g() { return (p as any).Z.A; } console.log(g());"),
    ]);
    assert!(verdicts(&pf, "A").iter().all(|v| v.starts_with("No")), "{}", pf.dump_text());
}

#[test]
fn a_namespace_with_an_export_star_from_outside_is_open() {
    // Soundness: `export *` from a module outside the program may add any
    // key: the namespace is open, every key may be unknown.
    let a = "export const E = { A: 1 };\nexport * from 'outside';";
    let pf = units_of(&[
        ("a", a),
        ("m", "import * as p from './a'; function g() { return (p as any).Q.A; } console.log(g());"),
    ]);
    let s: Vec<_> = pf.sites.iter().filter(|s| s.key == "A").collect();
    assert!(!s.is_empty() && s.iter().all(|s| s.top), "{}", pf.dump_text());
    // An `export *` inside the program adds the source's keys: `p.F` is a
    // key, holding what `import { F } from './a'` binds (⊤: a re-export
    // has no contract at `a`), not only `undefined`.
    let pf = units_of(&[
        ("b", "export const F = { A: 1 };"),
        ("a", "export * from './b';"),
        ("m", "import * as p from './a'; function g() { return p.F.A; } console.log(g());"),
    ]);
    let s: Vec<_> = pf.sites.iter().filter(|s| s.key == "A").collect();
    assert!(!s.is_empty() && s.iter().all(|s| s.top), "{}", pf.dump_text());
}
