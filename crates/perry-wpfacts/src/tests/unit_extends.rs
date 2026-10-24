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
        // DECISIONS 107: the unit running `get` publishes its receivers
        // (R's instances), so `this.k` is proven exactly, as the whole
        // program proves it, not only under the entry guard.
        assert_eq!(verdicts(&pf, "k"), ["T0Mono"], "{}", pf.dump_text());
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

/// DECISIONS 112: every object literal of one key set in a module is one
/// shape. Two units of `a` birth `{ x, y }` (and `{ o }`), and `b` reads the
/// layout by display between them in level order (f, then g, then h). Each
/// owner overwrote the entry: `b` read f's, h left its own, and every
/// round re-solved `b` (effect's JsonSchema.ts `{ dialect, schema,
/// definitions }` never converged). The entry is the owners' join: the
/// rounds end, bounded by the test.
#[test]
fn a_literal_shape_born_in_two_units_is_one_joined_layout() {
    let a = "import { g } from './b';\nexport function f() { return { o: { x: 1, y: 2 } }; }\nexport function h() { return { o: { x: g(), y: 'z' } }; }";
    let b = "import { f } from './a'; export function g() { const r = f(); return r.o.x + r.o.y; }";
    let m = "import { h } from './a'; import { g } from './b'; console.log(h().o.x, g());";
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let pf = units_of(&[("a", a), ("b", b), ("m", m)]);
        let _ = tx.send((
            pf.rounds,
            pf.broken.clone(),
            verdicts(&pf, "y"),
            pf.dump_text(),
        ));
    });
    let (rounds, broken, y, dump) = rx
        .recv_timeout(std::time::Duration::from_secs(60))
        .expect("the per-unit rounds converge");
    assert!(broken.is_empty(), "{broken:?}\n{dump}");
    assert!(rounds <= 4, "rounds {rounds}\n{dump}");
    // `y` sits in one slot of the one shape in every birth: b's read is
    // proven (b holds the one layout beside its own view of it: T0Set).
    assert!(y.iter().any(|v| v.starts_with("T0")), "{y:?}\n{dump}");
}

/// DECISIONS 108: a function's contract is published and read under its
/// name, so the name must be the function's identity. Two classes of one
/// module each with a `contains` method (som.js's Set and IdentitySet) are
/// two functions: named alike, the readers of one recorded the other's
/// contract and every round re-solved them (sa_DeltaBlue never converged).
/// The run is bounded by the test, not by the driver: it must end.
#[test]
fn methods_of_one_name_in_two_classes_are_two_contracts() {
    let a = "export class S0 { items: any[]; constructor() { this.items = []; } contains(o: any) { return this.items.indexOf(o) >= 0; } add(o: any) { if (!this.contains(o)) { this.items.push(o); } } }\nexport class Id extends S0 { contains(o: any) { return this.items.length > 0 ? 'y' : 'n'; } }\nexport function newSet() { return new S0(); }\nexport function newId() { return new Id(); }";
    let b = "import { newSet } from './a'; export function useSet(o: any) { const s = newSet(); s.add(o); return s.contains(o); }";
    let c = "import { newId } from './a'; export function useId(o: any) { const s = newId(); s.add(o); return s.contains(o); }";
    let m = "import { useSet } from './b'; import { useId } from './c'; console.log(useSet(1), useId(2));";
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let pf = units_of(&[("a", a), ("b", b), ("c", c), ("m", m)]);
        let _ = tx.send((
            pf.rounds,
            pf.broken.clone(),
            verdicts(&pf, "contains"),
            pf.dump_text(),
        ));
    });
    let (rounds, broken, contains, dump) = rx
        .recv_timeout(std::time::Duration::from_secs(60))
        .expect("the per-unit rounds converge");
    // `Id` overrides `contains` of S0 from another unit: the census reports
    // it (DECISIONS 97); nothing else breaks.
    assert!(
        broken.iter().all(|d| d.starts_with("override ")),
        "{broken:?}\n{dump}"
    );
    assert!(rounds <= 4, "rounds {rounds}\n{dump}");
    assert!(!contains.is_empty(), "{dump}");
}
