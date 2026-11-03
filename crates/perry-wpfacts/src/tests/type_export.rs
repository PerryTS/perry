//! An exported type alias or interface is erased: it is no runtime export,
//! the module namespace has no such key, and the facts model no value for it
//! (`Module::is_type_only_export_binding`, the predicate codegen reads too). Modeled
//! as an export of an unresolved name, it put ⊤ into the namespace, and every
//! computed read of the namespace was unproven.

use super::*;

fn proven(pf: &ProgramFacts, key: &str) -> bool {
    let v: Vec<_> = pf.sites.iter().filter(|s| s.key == key).collect();
    !v.is_empty()
        && v.iter()
            .all(|s| matches!(s.verdict, Verdict::T0Mono | Verdict::T0Set))
}

fn whole(mods: &[(&str, &str)]) -> ProgramFacts {
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
    let mut cfg = FactsConfig::default();
    cfg.max_work = Some(50_000_000);
    let list: Vec<(String, &perry_hir::Module)> =
        hirs.iter().map(|(p, h)| (p.clone(), h)).collect();
    let pf = crate::compute(&list, &cfg);
    assert!(!pf.stats.aborted, "{}", pf.dump_text());
    pf
}

const MOD: &str = "export type T = 'a' | 'b';\n\
     export interface I { readonly v: number }\n\
     interface J { w: number }\n\
     export { J };\n\
     export class C { v = 1; w = 2; }\n";

const MAIN: &str = "import * as E from './e';\n\
     const k = process.argv[2] ?? 'C';\n\
     const K = (E as any)[k];\n\
     const c = new K();\n\
     console.log(c.v, c.w);\n";

#[test]
fn type_exports_put_no_top_into_the_namespace() {
    let pf = whole(&[("e", MOD), ("main", MAIN)]);
    let dump = pf.dump_text();
    assert!(proven(&pf, "v"), "c.v unproven:\n{dump}");
    assert!(proven(&pf, "w"), "c.w unproven:\n{dump}");
}

/// The namespace's own keys are its runtime exports: `C`, never `T`, `I`
/// or `J` (an own key shadows `export *`, so a phantom one is wrong too).
#[test]
fn type_exports_are_no_namespace_keys() {
    let module = perry_parser::parse_typescript(MOD, "e.ts").expect("parses");
    let hir = perry_hir::lower_module(&module, "e", "e.ts").expect("lowers");
    let mut g = crate::gen::Gen::new(vec![("e.ts", &hir)], false, false);
    g.register_all();
    g.link_exports();
    let ns = g.mods[0].ns_tok;
    let own = |g: &mut crate::gen::Gen, n: &str| {
        let a = g.m.atom(n);
        g.m.ns_own.contains(&(ns, a))
    };
    assert!(own(&mut g, "C"));
    for n in ["T", "I", "J"] {
        assert!(!own(&mut g, n), "type export {n} is a namespace key");
    }
}

/// The predicate: a type declaration binds the name only when no value of the
/// same name does (declaration merging keeps the export).
#[test]
fn a_merged_value_keeps_the_export() {
    let src = "export interface A { x: number }\n\
               export class A { x = 1 }\n\
               export type B = number;\n\
               export interface F { (): void }\n\
               export function F() {}\n\
               export enum G { X }\n";
    let module = perry_parser::parse_typescript(src, "m.ts").expect("parses");
    let hir = perry_hir::lower_module(&module, "m", "m.ts").expect("lowers");
    assert!(!hir.is_type_only_export_binding("A"));
    assert!(hir.is_type_only_export_binding("B"));
    assert!(!hir.is_type_only_export_binding("F"));
    assert!(!hir.is_type_only_export_binding("G"));
}
