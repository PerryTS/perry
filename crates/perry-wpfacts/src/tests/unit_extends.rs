//! Per-unit mode: another unit's class instances handed on, and classes
//! along `extends` across units (design §17.10).

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

const MAKE: &str = "class R { k: number; constructor() { this.k = 1; } get() { return this.k + 1; } }\nexport function newInstance() { return new R(); }";

#[test]
fn a_result_holding_another_units_instance_forwards_its_contract() {
    // `R` and `newInstance` are separate declaration units (§12.3): the
    // result of `newInstance` is R's stand-in instance there, forwarded with
    // R's contract (§17.10 4), so R stays a contract: `this.k` is proven
    // under R's entry guard and the caller's `s.get()` resolves.
    for m in [
        "import { newInstance } from './a'; const s = newInstance(); console.log(s.get());",
        "import { newInstance } from './a'; function f() { const s = newInstance(); return s.get(); } console.log(f());",
    ] {
        let pf = units_of(&[("a", MAKE), ("m", m)]);
        assert!(pf.broken.is_empty(), "{:?}\n{}", pf.broken, pf.dump_text());
        assert_eq!(verdicts(&pf, "k"), ["E0"], "{}", pf.dump_text());
        assert_eq!(verdicts(&pf, "get"), ["T0Mono"], "{}", pf.dump_text());
    }
    // Soundness: the caller stores into what it got. `k` is a definite data
    // field of every R from its birth, so the store adds no key: a store
    // effect R's unit performs (decision 17, extended to class instances in
    // CW2 C), not a census report. R's layout holds, so `this.k` keeps its
    // slot; its value is the caller's: `k` is written after birth and holds
    // an unknown value.
    let m = "import { newInstance } from './a'; function f() { const s: any = newInstance(); s.k = 'x'; return s.get(); } console.log(f());";
    let pf = units_of(&[("a", MAKE), ("m", m)]);
    assert!(pf.broken.is_empty(), "{:?}", pf.broken);
    let k = field(tmpl(&pf, "R"), "k");
    // Rewritten for DECISIONS 73 (principle: a store effect carries the kinds
    // it stores): the field holds the stored primitive, not ⊤.
    assert!(
        !k.readonly && k.kind.prims & crate::model::prim::STR != 0,
        "{}",
        pf.dump_text()
    );
    // A store of a key R does not hold from birth is beyond the contract:
    // the census breaks R and `this.k` is no longer proven.
    let m = "import { newInstance } from './a'; function f() { const s: any = newInstance(); s.j = 'x'; return s.get(); } console.log(f());";
    let pf = units_of(&[("a", MAKE), ("m", m)]);
    assert!(
        pf.broken.iter().any(|d| d.ends_with(":R")),
        "{:?}",
        pf.broken
    );
    assert!(
        verdicts(&pf, "k").iter().all(|v| v.starts_with("No")),
        "{}",
        pf.dump_text()
    );
}
