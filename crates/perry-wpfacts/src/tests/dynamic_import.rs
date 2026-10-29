//! DECISIONS 226: an `import()` site holds the namespace of each module its
//! resolved `paths` name (through the importing module's dynamic import
//! entry, as codegen's dispatch resolves it) and ⊤ for the no-match fallback.
//! `paths` holds the specifier as written (`./m.js`), not a module path, so a
//! lookup by path alone missed every relative site: `make` kept its one
//! module-local caller and `base.mk` was proven T0 although `import("./m.js")`
//! calls it with another receiver (the binary printed 5 for 105).

use super::*;
use crate::SiteFacts;

const M: &str = r#"
export const V = 1;
export function make(base: { mk(k: number): { k: number } }) { return (k: number) => base.mk(k); }
const f = make({ mk: (k) => ({ k }) });
const p = { r: 2 };
console.log(f(1).k, p.r);
"#;

/// The FCT repro: the importer calls `make` with a second receiver.
const LITERAL: &str = r#"import "./m.js";
import("./m.js").then((m: any) => console.log(m.make({ other: 7, mk: (k: number) => ({ k: k + 100 }) })(5).k));"#;
const COMPUTED: &str = r#"import "./m.js";
const spec = "./m" + ".js";
import(spec).then((m: any) => console.log(m.make({ other: 7, mk: (k: number) => ({ k: k + 100 }) })(5).k));"#;
/// A plain call of the export read off the namespace (no `this`): the edge
/// itself, not the fallback's escape, brings the second receiver.
const READ: &str = r#"import("./m.js").then((m: any) => { const r = m.make; console.log(r({ other: 7, mk: (k: number) => ({ k: k + 100 }) })(5).k); });"#;
/// The importer only reads a value: `make` keeps its one caller.
const CONTROL: &str = r#"import("./m.js").then((m: any) => console.log(m.V));"#;

/// The program as the compile driver hands it over: every `import()` site's
/// `paths` filled with `paths` (`collect_modules`), each target an import
/// entry (`is_dynamic_target` on a static import of the same source, else
/// a new `is_dynamic` edge), relative sources resolved to `<name>.ts`.
/// `WPFACTS_TEST_SABOTAGE` reaches the analysis (the red control).
fn program(main: &str, paths: &[&str], units: bool, sabotage: Option<&str>) -> ProgramFacts {
    let hirs: Vec<(String, perry_hir::Module)> = [("m", M), ("main", main)]
        .iter()
        .map(|(name, src)| {
            let path = format!("{name}.ts");
            let module = perry_parser::parse_typescript(src, &path).expect("parses");
            let mut hir = perry_hir::lower_module(&module, name, &path).expect("lowers");
            let mut sites = 0;
            perry_hir::for_each_dynamic_import_mut(&mut hir, &mut |e| {
                if let perry_hir::Expr::DynamicImport { paths: p, .. } = e {
                    *p = paths.iter().map(|s| s.to_string()).collect();
                    sites += 1;
                }
            });
            if sites > 0 {
                for source in paths {
                    if let Some(i) = hir
                        .imports
                        .iter_mut()
                        .find(|i| i.source == *source && !i.is_dynamic)
                    {
                        i.is_dynamic_target = true;
                        continue;
                    }
                    hir.imports.push(perry_hir::Import {
                        source: source.to_string(),
                        specifiers: Vec::new(),
                        is_native: !source.starts_with("./"),
                        module_kind: Default::default(),
                        resolved_path: None,
                        type_only: false,
                        runtime_erased: false,
                        is_dynamic: true,
                        is_dynamic_target: false,
                        is_deferred_require: false,
                        is_adopted_require: false,
                    });
                }
            }
            for imp in &mut hir.imports {
                if let Some(n) = imp.source.strip_prefix("./") {
                    imp.resolved_path = Some(format!("{}.ts", n.trim_end_matches(".js")));
                }
            }
            (path, hir)
        })
        .collect();
    let list: Vec<(String, &perry_hir::Module)> =
        hirs.iter().map(|(p, h)| (p.clone(), h)).collect();
    let cfg = FactsConfig {
        sabotage: sabotage
            .map(str::to_string)
            .or_else(|| std::env::var("WPFACTS_TEST_SABOTAGE").ok()),
        ..Default::default()
    };
    if units {
        crate::compute_units(&list, &cfg)
    } else {
        crate::compute(&list, &cfg)
    }
}

fn mk(pf: &ProgramFacts) -> Vec<&SiteFacts> {
    pf.sites
        .iter()
        .filter(|s| s.module == 0 && s.key == "mk" && s.is_call)
        .collect()
}

fn proven(s: &SiteFacts) -> bool {
    !matches!(s.verdict, Verdict::No(_))
}

/// `base.mk` proven with m.ts's receiver alone: the wrong fact.
fn one_receiver(pf: &ProgramFacts) -> bool {
    mk(pf)
        .iter()
        .any(|s| proven(s) && s.receivers.iter().all(|r| r.starts_with("m.ts")))
}

#[test]
fn an_imported_module_is_called_through_its_namespace() {
    // main-red: on main every site below proved `base.mk` T0 on m.ts's
    // receiver (the literal one miscompiled: 5 for 105).
    for (what, main) in [("literal", LITERAL), ("computed", COMPUTED), ("read", READ)] {
        for units in [false, true] {
            let pf = program(main, &["./m.js"], units, None);
            assert!(!mk(&pf).is_empty(), "{what}: no .mk site\n{}", pf.dump_text());
            assert!(
                !one_receiver(&pf),
                "{what} units={units}: base.mk proven on m.ts's receiver alone\n{}",
                pf.dump_text()
            );
        }
    }
}

#[test]
fn a_namespace_read_without_a_call_keeps_the_proof() {
    // The control: the edge adds what the importer does, nothing more. Reading
    // `m.V` neither calls nor escapes `make`; m.ts's other sites keep theirs.
    let pf = program(CONTROL, &["./m.js"], false, None);
    let s = mk(&pf);
    assert!(
        !s.is_empty() && s.iter().all(|s| proven(s)),
        "{}",
        pf.dump_text()
    );
    let r: Vec<_> = pf.sites.iter().filter(|s| s.module == 0 && s.key == "r").collect();
    assert!(!r.is_empty() && r.iter().all(|s| proven(s)), "{}", pf.dump_text());
}

#[test]
fn a_mixed_set_keeps_the_module_and_the_fallback() {
    // A builtin target has no compiled module: the fallback's ⊤ holds it,
    // next to the compiled target's namespace.
    let pf = program(READ, &["./m.js", "node:fs"], false, None);
    assert!(!one_receiver(&pf), "{}", pf.dump_text());
}

#[test]
fn the_sabotaged_mapping_proves_wrongly() {
    // The red control (`PERRY_FACTS_SABOTAGE=dynimport`): without the
    // mapping each site holds ⊤ alone and the wrong fact comes back, so the
    // tests above can fail.
    for main in [LITERAL, COMPUTED, READ] {
        let pf = program(main, &["./m.js"], false, Some("dynimport"));
        assert!(one_receiver(&pf), "{}", pf.dump_text());
    }
}

#[test]
fn per_unit_is_never_stronger_than_whole() {
    for main in [LITERAL, COMPUTED, READ, CONTROL] {
        let w = program(main, &["./m.js"], false, None);
        let u = program(main, &["./m.js"], true, None);
        let s = super::body_activation::stronger_sites(&u, &w);
        assert!(s.is_empty(), "{s:?}\n{}", u.dump_text());
    }
}
