//! A generated message table (tsc's `Diagnostics`): one exported literal of
//! many `diag(...)` entries, wide enough that the splitter moves its tail
//! slots into chunk functions (`Expr::LiteralChunks`). The chunks
//! initialize the unpublished birth, so per unit they are part of the
//! birth's unit: another unit's reads of the table prove as in the whole
//! program, and no layout is broken.
use super::*;
use perry_hir::Module;
use perry_wpfacts::{FactsConfig, ProgramFacts, Verdict};

const ENTRIES: usize = 800;

fn lower(path: &str, source: &str) -> Module {
    let ast = perry_parser::parse_typescript(source, path).unwrap();
    perry_hir::lower_module(&ast, path, path).unwrap()
}

fn modules(split: bool) -> BTreeMap<PathBuf, Module> {
    let types = "export enum Category { Warning, Error }";
    let table = format!(
        "import {{ Category }} from \"./types\";\n\
         function diag(code: number, category: Category, key: string, message: string) {{\n\
         \x20   return {{ code, category, key, message }};\n\
         }}\n\
         export const Diagnostics = {{\n{}}};\n",
        (0..ENTRIES)
            .map(|i| format!(
                "    M{i}: diag({i}, Category.Error, \"M{i}_{i}\", \"message {i}.\"),\n"
            ))
            .collect::<String>()
    );
    let reader = "import { Diagnostics } from \"./diag\";\n\
         function size(m: { code: number; message: string }) { return m.code + m.message.length; }\n\
         export function run() { return Diagnostics.M5.code + Diagnostics.M700.code + size(Diagnostics.M400); }\n\
         console.log(run());\n";
    let mut out = BTreeMap::new();
    for (p, src) in [
        ("types", types),
        ("diag", table.as_str()),
        ("reader", reader),
    ] {
        let path = format!("/fixture/{p}.ts");
        let mut m = lower(&path, src);
        for imp in &mut m.imports {
            imp.resolved_path = Some(format!("/fixture{}.ts", &imp.source[1..]));
        }
        if split {
            perry_codegen::codegen::split_regions::split_large_bodies(&mut m);
        }
        out.insert(PathBuf::from(path), m);
    }
    out
}

fn facts(split: bool, units: bool) -> ProgramFacts {
    let modules = modules(split);
    if split {
        let d = &modules[Path::new("/fixture/diag.ts")];
        assert!(
            d.functions.len() > 2,
            "the table must be chunked: {} functions",
            d.functions.len()
        );
    }
    let inputs = facts_inputs(&modules);
    let list = inputs
        .iter()
        .map(|(p, m)| (p.clone(), m.as_ref()))
        .collect::<Vec<_>>();
    let cfg = FactsConfig {
        max_work: Some(200_000_000),
        ..Default::default()
    };
    let pf = if units {
        perry_wpfacts::compute_units(&list, &cfg)
    } else {
        perry_wpfacts::compute(&list, &cfg)
    };
    assert!(!pf.stats.aborted, "{}", pf.dump_text());
    pf
}

/// The reader's sites of `key`, each proven.
fn proven(pf: &ProgramFacts, key: &str) -> bool {
    let reader = pf
        .module_paths
        .iter()
        .position(|p| p == "/fixture/reader.ts")
        .unwrap() as u32;
    let sites: Vec<_> = pf
        .sites
        .iter()
        .filter(|s| s.module == reader && s.key == key)
        .collect();
    assert!(
        !sites.is_empty(),
        "no reader site {key}\n{}",
        pf.dump_text()
    );
    sites
        .iter()
        .all(|s| matches!(s.verdict, Verdict::T0Mono | Verdict::T0Set))
}

#[test]
fn a_chunked_table_proves_per_unit_as_whole() {
    let whole = facts(true, false);
    let units = facts(true, true);
    // A head slot, a chunked slot, and fields of their values.
    for k in ["M5", "M700", "M400", "code", "message"] {
        assert!(proven(&whole, k), "whole {k}\n{}", whole.dump_text());
        assert!(
            proven(&units, k),
            "per unit {k}: broken {:?}\n{}",
            units.broken,
            units.dump_text()
        );
    }
    assert!(units.broken.is_empty(), "{:?}", units.broken);
}

/// The control: unchunked, per unit already proves the table's reads.
#[test]
fn an_unchunked_table_proves_per_unit() {
    let units = facts(false, true);
    for k in ["M5", "M700", "M400", "code", "message"] {
        assert!(
            proven(&units, k),
            "per unit {k}: broken {:?}\n{}",
            units.broken,
            units.dump_text()
        );
    }
}
