//! DECISIONS 215: a module namespace's named key is resolved by name
//! (ECMAScript ResolveExport): a name read through `export *` holds that
//! name of each source, never every export of them; a computed key holds
//! every key.

use super::*;

fn proven(pf: &ProgramFacts, key: &str) -> bool {
    let v: Vec<_> = pf.sites.iter().filter(|s| s.key == key).collect();
    !v.is_empty()
        && v.iter()
            .all(|s| matches!(s.verdict, Verdict::T0Mono | Verdict::T0Set))
}

/// The modules solved as one whole program; `./x` imports and re-export
/// sources resolve to `x.ts`.
fn whole(mods: &[(String, String)]) -> ProgramFacts {
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
    for (path, hir) in &hirs {
        for e in &hir.exports {
            let (perry_hir::Export::ReExport { source, .. }
            | perry_hir::Export::ExportAll { source }
            | perry_hir::Export::NamespaceReExport { source, .. }) = e
            else {
                continue;
            };
            if let Some(n) = source.strip_prefix("./") {
                cfg.export_sources
                    .entry(path.clone())
                    .or_default()
                    .insert(source.clone(), format!("{n}.ts"));
            }
        }
    }
    cfg.max_work = Some(50_000_000);
    let list: Vec<(String, &perry_hir::Module)> =
        hirs.iter().map(|(p, h)| (p.clone(), h)).collect();
    let pf = crate::compute(&list, &cfg);
    assert!(!pf.stats.aborted, "{}", pf.dump_text());
    pf
}

/// A barrel (`ts`) re-exporting `f` and `n` other modules of one function
/// each; `m` calls `f` at four sites, by name and through the namespace.
fn barrel(n: usize) -> ProgramFacts {
    let mut mods: Vec<(String, String)> = (0..n)
        .map(|i| {
            (
                format!("g{i}"),
                format!("export function g{i}() {{ return {{ B{i}: {i} }}; }}"),
            )
        })
        .collect();
    mods.push((
        "f".into(),
        "export function f() { return { A: 1 }; }".into(),
    ));
    let ts: String = mods
        .iter()
        .map(|(m, _)| format!("export * from './{m}';\n"))
        .collect();
    mods.push(("ts".into(), ts));
    mods.push((
        "m".into(),
        "import { f } from './ts';\nimport * as ts from './ts';\n\
         const o = f(); console.log(o.A);\nconst p = f(); console.log(p.A);\n\
         const q = ts.f(); console.log(q.A);\nconst r = ts.f(); console.log(r.A);"
            .into(),
    ));
    whole(&mods)
}

#[test]
fn a_name_through_export_star_holds_that_name_only() {
    let pf = barrel(8);
    assert!(proven(&pf, "A"), "{}", pf.dump_text());
    // A call site runs `f` alone: 24 more modules add their 24 functions'
    // own instances, none per call site (on the union of the barrel's
    // exports each of the four sites ran every function).
    let (small, big) = (pf.stats.instances, barrel(32).stats.instances);
    assert_eq!(big - small, 24, "instances {small} -> {big}");
}

#[test]
fn an_unresolved_export_star_opens_the_names_it_may_provide_only() {
    let a = "export * from 'outside';\nexport function f() { return { A: 1 }; }";
    let m = "import * as a from './a';\nimport { g } from './a';\n\
             const o = a.f(); console.log(o.A);\nconst p: any = g(); console.log(p.B);";
    let pf = whole(&[("a".into(), a.into()), ("m".into(), m.into())]);
    // `f` is the module's own export: the unknown source cannot shadow it.
    assert!(proven(&pf, "A"), "{}", pf.dump_text());
    // `g` may come from the unknown source.
    assert!(!proven(&pf, "B"), "{}", pf.dump_text());
}

#[test]
fn a_computed_key_of_a_namespace_reads_every_export() {
    let b = "export function f() { return { A: 1 }; }\nexport function g() { return { B: 2 }; }";
    let m = "import * as ts from './a';\ndeclare const k: string;\n\
             const o: any = (ts as any)[k](); console.log(o.A);";
    let pf = whole(&[
        ("b".into(), b.into()),
        ("a".into(), "export * from './b';".into()),
        ("m".into(), m.into()),
    ]);
    // `g` is among the callees: its result has no `A`.
    assert!(!proven(&pf, "A"), "{}", pf.dump_text());
}
