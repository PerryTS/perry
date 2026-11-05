//! A literal `require` of a compiled CommonJS module is a static edge (design
//! CJS_LINK 2026-10-26 §3): a read of the target's record binding, then its
//! `exports` slot. A computed specifier keeps the registry summary.

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

fn run(h: &[(String, Module)], unit: bool) -> ProgramFacts {
    let list: Vec<(String, &Module)> = h.iter().map(|(p, m)| (p.clone(), m)).collect();
    match unit {
        true => crate::compute_units(&list, &FactsConfig::default()),
        false => crate::compute(&list, &FactsConfig::default()),
    }
}

// The target's side of the wrapper: its record binding, assigned by the
// factory before the body.
fn target(class: &str, value: &str) -> String {
    format!(
        "class {class} {{ x = {value}; }}\nexport var __perry_cjs_record;\n(function () {{ __perry_cjs_record = {{ exports: new {class}(), loaded: false }}; }})();\n"
    )
}

const REQUIRER: &str = r#"
    import { __perry_cjs_record as __perry_cjs_rec_0 } from "./left";
    function __perry_require_static_0() {
        const record = __perry_cjs_init(__perry_cjs_rec_0);
        return record.exports;
    }
    const left = __perry_require_static_0();
    console.log(left.x);
"#;

fn x_sites(pf: &ProgramFacts, module: &str) -> Vec<crate::SiteFacts> {
    let mi = pf
        .module_paths
        .iter()
        .position(|m| m == module)
        .unwrap_or_else(|| panic!("{module} in {:?}", pf.module_paths)) as u32;
    pf.sites
        .iter()
        .filter(|s| s.key == "x" && s.module == mi)
        .cloned()
        .collect()
}

#[test]
fn a_static_edge_takes_its_target_record() {
    let h = hirs(&[
        ("left", &target("Left", "1")),
        ("right", &target("Right", "\"right\"")),
        ("main", REQUIRER),
    ]);
    let pf = run(&h, false);
    let sites = x_sites(&pf, "main.ts");
    assert_eq!(sites.len(), 1, "{}", pf.dump_text());
    assert_eq!(sites[0].verdict, Verdict::T0Mono, "{}", pf.dump_text());
    assert!(
        sites[0].receivers.iter().all(|r| r.ends_with(":Left")),
        "{:?}\n{}",
        sites[0].receivers,
        pf.dump_text()
    );
    // Per unit is never stronger than whole program.
    let pu = run(&h, true);
    for s in x_sites(&pu, "main.ts") {
        if s.verdict == Verdict::T0Mono {
            assert!(
                s.receivers.iter().all(|r| r.ends_with(":Left")),
                "{}",
                pu.dump_text()
            );
        }
    }
}

#[test]
fn a_computed_require_keeps_the_registry() {
    // A specifier the wrapper could not resolve sees every published record.
    let left = format!(
        "{}__perry_register_path_module(\"/cjsq-left.js\", __perry_cjs_record);\n",
        target("Left", "1")
    );
    let right = format!(
        "{}__perry_register_path_module(\"/cjsq-right.js\", __perry_cjs_record);\n",
        target("Right", "\"right\"")
    );
    let main = r#"
        function pick(n) { return n ? "/cjsq-left.js" : "/cjsq-right.js"; }
        const any = __perry_require_path_module(pick(Math.random()));
        console.log(any.x);
    "#;
    let h = hirs(&[("left", &left), ("right", &right), ("main", main)]);
    let pf = run(&h, false);
    let sites = x_sites(&pf, "main.ts");
    assert_eq!(sites.len(), 1, "{}", pf.dump_text());
    assert_ne!(sites[0].verdict, Verdict::T0Mono, "{}", pf.dump_text());
}
