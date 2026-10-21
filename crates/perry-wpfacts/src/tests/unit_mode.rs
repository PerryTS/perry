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
    // Another unit may call `get` with anything and may hold `P`. Only what
    // another unit can name is published (design §11.1): the importer names
    // both, so both are entries of unknown code.
    let src = r#"
        export class P { x: number; constructor(x: number) { this.x = x; } }
        export function get(p: P) { return p.x; }
        console.log(get(new P(1)));
        "#;
    let whole = facts(src);
    assert!(proven(&whole, "x"), "{}", whole.dump_text());
    let pf = units_of(&[("t", src), ("u", "import { P, get } from './t'; console.log(get(new P(2)));")]);
    assert!(!proven(&pf, "x"), "{}", pf.dump_text());
    // A published class is a contract, not an escape (§11.2), but `u`
    // passes a `P` to `get`, a function of another unit with no contract:
    // the census breaks `P` (§11.3) and its layout is no longer fixed.
    assert_eq!(pf.broken, ["t.ts:P"], "{}", pf.dump_text());
    assert!(!tmpl(&pf, "P").fixed_layout, "{}", pf.dump_text());
    assert_eq!(pf.units.len(), 2);
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

/// Several modules, one unit each; `./x` imports resolve to `x.ts`.
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
    let list: Vec<(String, &perry_hir::Module)> = hirs.iter().map(|(p, h)| (p.clone(), h)).collect();
    crate::compute_units(&list, &FactsConfig::default())
}

#[test]
fn an_export_no_unit_names_stays_inside() {
    // `Ball` is exported (as the CommonJS wrap exports every hoisted class)
    // but no module imports it: its methods run only inside its unit.
    // Importing it publishes it (§11.1), and the reads go generic.
    let a = r#"
        class Ball { x: number; constructor(r: number) { this.x = r; } bounce() { this.x += 1; return this.x; } }
        class Bounce { run() { const b = new Ball(3); return b.bounce(); } }
        export { Ball };
        export const newInstance = () => new Bounce();
        "#;
    let hidden = units_of(&[("a", a), ("m", "import { newInstance } from './a'; console.log(newInstance().run());")]);
    assert!(proven(&hidden, "x"), "{}", hidden.dump_text());
    let named = units_of(&[("a", a), ("m", "import { Ball } from './a'; console.log(new Ball(1));")]);
    assert!(!proven(&named, "x"), "{}", named.dump_text());
}

fn verdicts(pf: &ProgramFacts, key: &str) -> Vec<String> {
    pf.sites.iter().filter(|s| s.key == key).map(|s| format!("{:?}", s.verdict)).collect()
}

#[test]
fn a_published_class_is_a_contract_not_an_escape() {
    // `D` is imported by name: other units construct it and call its
    // methods (design §11.2). Its constructor's own instance stays proven;
    // a method's `this` is any receiver, so `this.neg` is proven only behind
    // the entry guard `class_id(this) == D` (E0, §11.4).
    let a = r#"
        export class D {
            pos: Map<number, number>; neg: Map<number, number>;
            constructor(o: number[]) { this.pos = new Map(); this.neg = new Map(); o.forEach((x) => { this.pos.set(x, x); }); }
            has(k: number) { return this.neg.has(k); }
        }
        "#;
    let pf = units_of(&[("a", a), ("m", "import { D } from './a'; const d = new D([1]); console.log(d.has(1));")]);
    assert!(proven(&pf, "pos"), "{}", pf.dump_text());
    assert_eq!(verdicts(&pf, "neg"), ["E0"], "{}", pf.dump_text());
    assert!(tmpl(&pf, "D").prefix_stable, "{}", pf.dump_text());
}

const DUAL: &str = r#"
    export class D {
        pos: Map<number, number>; neg: Map<number, number>;
        constructor(o: number[]) { this.pos = new Map(); this.neg = new Map(); o.forEach((x) => { this.pos.set(x, x); }); }
        has(k: number) { return this.neg.has(k); }
    }
    "#;

#[test]
fn an_importer_sees_a_class_through_its_contract() {
    // `new D(..)` in another unit is an instance of D's template with the
    // facts D's unit proved, and `d.has` resolves through its prototype
    // (design §11.2); nothing beyond the contract happens: one round.
    // `z.ts` sorts after its importer: the driver solves it first anyway
    // (dependency order), so the contract exists when `m` is solved.
    let pf = units_of(&[("z", &DUAL.replace("./a", "./z")), ("m", "import { D } from './z'; const d = new D([1]); console.log(d.has(1));")]);
    let mi = pf.module_paths.iter().position(|p| p == "m.ts").unwrap() as u32;
    let site = pf.sites.iter().find(|s| s.module == mi && s.key == "has").expect("the call in m");
    assert_eq!(site.verdict, crate::Verdict::T0Mono, "{}", pf.dump_text());
    assert_eq!((pf.rounds, pf.broken.len()), (1, 0));
}

#[test]
fn a_store_beyond_the_contract_breaks_the_class() {
    // The importer stores into a D instance: a census report (§11.3). D is
    // then published as an escape, so neither unit proves anything on it.
    let pf = units_of(&[("a", DUAL), ("m", "import { D } from './a'; const d = new D([1]); (d as any).extra = 1; console.log(d.has(1));")]);
    assert_eq!(pf.broken, ["a.ts:D"], "{}", pf.dump_text());
    assert_eq!(pf.rounds, 2);
    assert!(!proven(&pf, "pos"), "{}", pf.dump_text());
    assert!(!proven(&pf, "has"), "{}", pf.dump_text());
    assert!(verdicts(&pf, "neg").iter().all(|v| v.starts_with("No")), "{}", pf.dump_text());
}

#[test]
fn a_broken_contract_resolves_only_the_units_that_see_it() {
    // `m` breaks D (a census report against `a`): `a` and its importer `m`
    // are solved again (design §11.3, contracts travel only along import
    // edges). `b`/`c` import nothing of `a`: their inputs are unchanged, so
    // round 2 keeps their results and costs exactly `a` + `m`.
    let pf = units_of(&[
        ("a", DUAL),
        ("m", "import { D } from './a'; const d = new D([1]); (d as any).extra = 1; console.log(d.has(1));"),
        ("b", &DUAL.replace("class D", "class E").replace("neg", "other")),
        ("c", "import { E } from './b'; const e = new E([1]); console.log(e.has(1));"),
    ]);
    assert_eq!(pf.broken, ["a.ts:D"], "{}", pf.dump_text());
    assert_eq!(pf.round_work.len(), 2);
    let w = |p: &str| pf.units.iter().find(|(q, _)| q == p).map(|(_, s)| s.work).expect("unit");
    assert_eq!(pf.round_work[1], w("a.ts") + w("m.ts"), "{:?} {:?}", pf.round_work, pf.units);
    assert!(verdicts(&pf, "neg").iter().all(|v| v.starts_with("No")), "{}", pf.dump_text());
    let mi = pf.module_paths.iter().position(|p| p == "c.ts").unwrap() as u32;
    let site = pf.sites.iter().find(|s| s.module == mi && s.key == "has").expect("the call in c");
    assert_eq!(site.verdict, crate::Verdict::T0Mono, "{}", pf.dump_text());
}

#[test]
fn a_unit_over_the_bound_is_not_solved_again() {
    // `o` imports `a` but is over the work bound: it claims and publishes
    // nothing whatever its inputs, so when `m` breaks D it keeps its result
    // (design §11.3) and round 2 costs `a` + `m` only.
    let big: String = (0..400).map(|i| format!("const o{i} = {{ v: new D([{i}]) }}; console.log(o{i}.v.has({i}));\n")).collect();
    let o = format!("import {{ D }} from './a';\n{big}");
    let mods = [
        ("a", DUAL),
        ("m", "import { D } from './a'; const d = new D([1]); (d as any).extra = 1; console.log(d.has(1));"),
        ("o", o.as_str()),
    ];
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
    let list: Vec<(String, &perry_hir::Module)> = hirs.iter().map(|(p, h)| (p.clone(), h)).collect();
    // The bound sits between the small units and `o` (measured unbounded).
    let free = crate::compute_units(&list, &FactsConfig::default());
    let wk = |p: &str| free.units.iter().find(|(q, _)| q == p).map(|(_, s)| s.work).expect("unit");
    assert!(wk("o.ts") > 4 * wk("a.ts").max(wk("m.ts")), "{:?}", free.units);
    let bound = 2 * wk("a.ts").max(wk("m.ts"));
    let pf = crate::compute_units(&list, &FactsConfig { max_work: Some(bound), ..FactsConfig::default() });
    let u = |p: &str| pf.units.iter().find(|(q, _)| q == p).map(|(_, s)| s.clone()).expect("unit");
    assert!(u("o.ts").aborted && !u("a.ts").aborted && !u("m.ts").aborted, "{:?}", pf.units);
    assert_eq!(pf.broken, ["a.ts:D"], "{}", pf.dump_text());
    assert_eq!(pf.round_work[1], u("a.ts").work + u("m.ts").work, "{:?} {:?}", pf.round_work, pf.units);
}

#[test]
fn a_foreign_receiver_of_a_contract_method_escapes() {
    // `D.prototype.has` called on an object that is not a D: D's unit runs
    // it on that object unseen (`ContractThis`), so the object escapes.
    let m = "import { D } from './a'; const d = new D([1]); const h = d.has; const o = { neg: new Map() }; console.log(h.call(o, 1), o.neg.size);";
    let pf = units_of(&[("a", DUAL), ("m", m)]);
    let site = pf.sites.iter().find(|s| s.module == 1 && s.key == "neg").expect("o.neg in m");
    assert!(matches!(site.verdict, crate::Verdict::No(_)), "{}", pf.dump_text());
}

#[test]
fn what_a_method_does_to_its_receiver_happens_to_the_instance() {
    // Another unit calls `grow` on an instance it constructed: the key
    // `extra` is added to a D after birth, so D's layout is not fixed; and
    // `self` hands the instance back to unknown code: D escapes (§11.2).
    let a = "export class D { pos: number; constructor() { this.pos = 1; } grow(v: number) { (this as any).extra = v; } }";
    let pf = units_of(&[("a", a), ("m", "import { D } from './a'; const d = new D(); d.grow(2); console.log(d.pos);")]);
    assert!(!tmpl(&pf, "D").fixed_layout, "{}", pf.dump_text());
    let b = "export class D { pos: number; constructor() { this.pos = 1; } me() { return this; } }";
    let pf = units_of(&[("a", b), ("m", "import { D } from './a'; console.log(new D().me());")]);
    assert!(!tmpl(&pf, "D").prefix_stable, "{}", pf.dump_text());
}

const BENCH: &str = r#"
    export interface Iters { n: number; warm: number; }
    export function iters(d: number, w: number): Iters { return { n: d, warm: w }; }
    export function header(name: string, it: Iters): void { console.log(name + " n=" + it.n + " warm=" + it.warm); }
    "#;

#[test]
fn a_function_contract_carries_its_result_and_borrowed_arguments() {
    // `iters` returns a literal layout (its contract names it), `header`
    // only reads and converts its argument: the caller's objects stay its
    // own (design §11.2). Inside `header`, `it: Iters` picks the program's
    // templates with keys n and warm as entry-guard candidates (§11.4).
    let m = "import { iters, header } from './b'; const it = iters(5, 2); header('x', it); const own = { n: 1, warm: 2 }; header('y', own); console.log(it.n, own.warm);";
    let pf = units_of(&[("b", BENCH), ("m", m)]);
    let v = |module: u32, key: &str| pf.sites.iter().filter(|s| s.module == module && s.key == key).map(|s| format!("{:?}", s.verdict)).collect::<Vec<_>>();
    assert_eq!(v(1, "n"), ["T0Mono"], "{}", pf.dump_text());
    assert_eq!(v(1, "warm"), ["T0Mono"], "{}", pf.dump_text());
    assert_eq!(v(0, "n"), ["E0"], "{}", pf.dump_text());
    assert_eq!(v(0, "warm"), ["E0"], "{}", pf.dump_text());
}

#[test]
fn an_argument_the_callee_writes_escapes() {
    // `bump` stores into what it is given: the caller's literal escapes.
    let b = "export function bump(o: { n: number }) { o.n = o.n + 1; }";
    let m = "import { bump } from './b'; const own = { n: 1, warm: 2 }; bump(own); console.log(own.warm);";
    let pf = units_of(&[("b", b), ("m", m)]);
    assert!(!proven(&pf, "warm"), "{}", pf.dump_text());
}

#[test]
fn a_shared_class_id_is_stable_only_when_every_template_is() {
    // Two modules' literal classes with one class id (perry gives
    // structurally identical literal classes one id): one escapes, so the id
    // is not stable for either (`ProgramFacts::stable_classes`).
    let a = "export function f(o: { n: number }) { return o.n; } const x = { n: 1 }; console.log(f(x));";
    let b = "const y = { n: 2 }; (globalThis as any).keep = y; console.log(y.n);";
    let pf = units_of(&[("a", a), ("b", b)]);
    let ids: Vec<Option<u32>> = pf.templates.iter().map(|t| t.class_id).collect();
    assert!(ids.len() >= 2 && ids.iter().all(|c| c.is_some() && *c == ids[0]), "{}", pf.dump_text());
    assert!(pf.templates.iter().any(|t| t.prefix_stable), "{}", pf.dump_text());
    assert!(!pf.stable_classes().contains_key(&ids[0].unwrap()), "{}", pf.dump_text());
}

#[test]
fn a_result_that_is_not_a_literal_layout_escapes() {
    // `mk` returns an instance of a class with methods: not a literal
    // layout, so the result escapes as before (§11.2) and `m` may run
    // `set` on it with anything.
    let a = "class C { x: number; constructor() { this.x = 1; } set(v: any) { this.x = v; } } export function mk() { return new C(); }";
    let pf = units_of(&[("a", a), ("m", "import { mk } from './a'; mk().set('s');")]);
    assert!(!tmpl(&pf, "C").prefix_stable, "{}", pf.dump_text());
}
