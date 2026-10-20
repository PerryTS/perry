//! Per-unit mode (decision 151 P1): the other units are unknown compiled
//! code. What a unit publishes reaches them, what they may do comes back.

use super::*;

fn proven(pf: &ProgramFacts, key: &str) -> bool {
    pf.sites
        .iter()
        .filter(|s| s.key == key)
        .all(|s| matches!(s.verdict, Verdict::T0Mono | Verdict::T0Set))
}

fn units(src: &str) -> ProgramFacts {
    let module = perry_parser::parse_typescript(src, "t.ts").expect("parses");
    let hir = perry_hir::lower_module(&module, "t", "t.ts").expect("lowers");
    crate::compute_units(&[("t.ts".to_string(), &hir)], &FactsConfig::default())
}

#[test]
fn exports_are_entries_of_other_units() {
    // Another unit may call `get` with anything and may hold `P`.
    let src = r#"
        export class P { x: number; constructor(x: number) { this.x = x; } }
        export function get(p: P) { return p.x; }
        console.log(get(new P(1)));
        "#;
    let whole = facts(src);
    assert!(proven(&whole, "x"), "{}", whole.dump_text());
    let pf = units(src);
    assert!(!proven(&pf, "x"), "{}", pf.dump_text());
    assert!(!tmpl(&pf, "P").fixed_layout, "{}", pf.dump_text());
    assert_eq!(pf.units.len(), 1);
    assert!(pf.sites.iter().all(|s| s.module == 0));
}

#[test]
fn private_layouts_stay_proven() {
    // Nothing published: the unit's own objects keep their facts.
    let src = r#"
        class P { x: number; constructor(x: number) { this.x = x; } }
        function get(p: P) { return p.x; }
        console.log(get(new P(1)));
        "#;
    let pf = units(src);
    assert!(proven(&pf, "x"), "{}", pf.dump_text());
    assert!(tmpl(&pf, "P").fixed_layout, "{}", pf.dump_text());
}
