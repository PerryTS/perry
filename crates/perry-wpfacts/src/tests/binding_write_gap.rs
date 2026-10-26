//! Pinned per-unit gap (found by CJL, 2026-10-26; owner rule: per unit is
//! never stronger than whole program): a value written into a module binding
//! from a function unit is constructed by another unit without facts. The
//! function the value holds is a local of the writing function, so no
//! contract names it, and the importer's construction never reaches its
//! fields per unit: `raw` has no kind per unit, a number whole program.
//!
//! Witnesses on qb9: /root/CJL/tmp/rep2 (this ESM shape, stronger on main
//! 3f6304e97 too) and /root/CJL/tmp/rep (the CommonJS wrapper of a program
//! that is not closed, with its factory unflattened). Fixed by the follow-up
//! lane on top of P3B; until then these are ignored.

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

/// The kind of `field` of the template whose display ends with `tail`.
fn kind_of(pf: &ProgramFacts, tail: &str, field: &str) -> String {
    let t = pf
        .templates
        .iter()
        .find(|t| t.display.ends_with(tail))
        .unwrap_or_else(|| panic!("{tail} in\n{}", pf.dump_text()));
    let f = t
        .fields
        .iter()
        .find(|f| f.key == field)
        .unwrap_or_else(|| panic!("{tail}.{field} in\n{}", pf.dump_text()));
    format!("{:?}", f.kind)
}

fn per_unit_not_stronger(h: &[(String, Module)], tail: &str) {
    let (u, w) = (run(h, true), run(h, false));
    assert_eq!(
        kind_of(&u, tail, "raw"),
        kind_of(&w, tail, "raw"),
        "per unit:\n{}\nwhole program:\n{}",
        u.dump_text(),
        w.dump_text()
    );
}

#[test]
#[ignore = "per-unit gap: a value written into a module binding from a function unit is constructed by another unit without facts"]
fn a_value_written_into_a_module_binding_from_a_function_unit_is_constructed_by_another_unit_without_facts(
) {
    let a = r#"
        export var rec: any;
        function make() {
            function Reply(this: any, res: any) { this.raw = res; }
            rec = { exports: Reply };
        }
        make();
        make();
    "#;
    let main = r#"
        import { rec } from "./a";
        const R = rec.exports;
        const r = new R(2);
        console.log(r.raw);
    "#;
    per_unit_not_stronger(&hirs(&[("a", a), ("main", main)]), "a.ts:Reply");
}

#[test]
#[ignore = "per-unit gap: a value written into a module binding from a function unit is constructed by another unit without facts"]
fn the_commonjs_record_of_an_unflattened_factory_is_constructed_by_another_unit_without_facts() {
    // The wrapper's shape in a program that is not closed: the record binding
    // is written by the factory, which the run-once view does not flatten
    // (held beyond its record).
    let a = r#"
        export var __perry_cjs_record: any;
        function __perry_cjs_factory() {
            function Reply(this: any, res: any) { this.raw = res; }
            __perry_cjs_record = { exports: Reply, loaded: false };
            return __perry_cjs_record.exports;
        }
        (globalThis as any).again = __perry_cjs_factory;
        __perry_cjs_factory();
    "#;
    let main = r#"
        import { __perry_cjs_record as __perry_cjs_rec_0 } from "./a";
        const R = __perry_cjs_init(__perry_cjs_rec_0).exports;
        const r = new R(2);
        console.log(r.raw);
    "#;
    per_unit_not_stronger(&hirs(&[("a", a), ("main", main)]), "a.ts:Reply");
}
