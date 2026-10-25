//! Per-unit mode (decision 151 P1): the other units are unknown compiled
//! code. What a unit publishes reaches them, what they may do comes back.

use super::*;

/// Design §16: the owner performed another unit's new key on one of its
/// literal layouts: the layout adds a key whose field holds an unknown value.
pub(super) fn gains_key(pf: &ProgramFacts, key: &str) -> bool {
    pf.templates.iter().any(|t| {
        t.adds_keys > 0
            && t.fields
                .iter()
                // Rewritten for DECISIONS 73 (principle: a store effect carries the kinds
                // it stores): the field holds the stored primitive, not ⊤.
                .any(|f| f.key == key && !f.readonly && f.writers_post > 0)
    })
}

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
    let pf = units_of(&[
        ("t", src),
        (
            "u",
            "import { P, get } from './t'; console.log(get(new P(2)));",
        ),
    ]);
    assert!(!proven(&pf, "x"), "{}", pf.dump_text());
    // A published class is a contract, not an escape (§11.2), and so is the
    // published function `get`: `u` passing a `P` to it is a use its
    // contract describes (`get` only reads it), so the census breaks
    // nothing; `p.x` holds only under the entry guard (E0, §11.4).
    assert!(pf.broken.is_empty(), "{}", pf.dump_text());
    assert!(tmpl(&pf, "P").fixed_layout, "{}", pf.dump_text());
    assert_eq!(verdicts(&pf, "x"), ["E0"], "{}", pf.dump_text());
    // Units are declaration units `path#uK` (§12.3): both modules solved.
    assert_eq!(modules_of_units(&pf), ["t.ts", "u.ts"], "{:?}", pf.units);
}

/// The modules the units belong to (a unit path is `module#uK`, §12.3).
fn modules_of_units(pf: &ProgramFacts) -> Vec<String> {
    let mut m: Vec<String> = pf
        .units
        .iter()
        .map(|(p, _)| crate::decl_units::module_of(p).to_string())
        .collect();
    m.sort();
    m.dedup();
    m
}

/// The work of every unit of module `path` (its declaration units, §12.3).
fn module_work(pf: &ProgramFacts, path: &str) -> u64 {
    let w: Vec<u64> = pf
        .units
        .iter()
        .filter(|(q, _)| crate::decl_units::module_of(q) == path)
        .map(|(_, s)| s.work)
        .collect();
    assert!(!w.is_empty(), "no unit of {path}: {:?}", pf.units);
    w.iter().sum()
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
    // `get`, `P` and the statement are declaration units of their own
    // (§12.3). `get` is closed (decision 13: never exported, only called
    // directly), so `p` holds what its one caller passes, a `P` through
    // P's contract: `p.x` holds without an entry guard (T0; an exported
    // `get` holds under the guard, E0, `exports_are_entries_of_other_units`).
    assert_eq!(verdicts(&pf, "x"), ["T0Mono"], "{}", pf.dump_text());
    assert!(tmpl(&pf, "P").fixed_layout, "{}", pf.dump_text());
    assert!(pf.broken.is_empty(), "{:?}", pf.broken);
}

/// Several modules, one unit each; `./x` imports resolve to `x.ts`.
fn units_of(mods: &[(&str, &str)]) -> ProgramFacts {
    units_with(mods, &FactsConfig::default())
}

fn units_with(mods: &[(&str, &str)], cfg: &FactsConfig) -> ProgramFacts {
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
    crate::compute_units(&list, cfg)
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
    let hidden = units_of(&[
        ("a", a),
        (
            "m",
            "import { newInstance } from './a'; console.log(newInstance().run());",
        ),
    ]);
    // `Bounce` names `Ball` from another declaration unit (§12.3): Ball is a
    // contract to it, so `this.x` in `bounce` is proven under the entry
    // guard (E0, §11.4), not unconditionally; nothing generic.
    assert!(
        verdicts(&hidden, "x")
            .iter()
            .all(|v| v == "T0Mono" || v == "E0"),
        "{}",
        hidden.dump_text()
    );
    let named = units_of(&[
        ("a", a),
        ("m", "import { Ball } from './a'; console.log(new Ball(1));"),
    ]);
    assert!(!proven(&named, "x"), "{}", named.dump_text());
}

/// The census broke the literal layout born with keys `keys`.
fn literal_broken(pf: &ProgramFacts, keys: &[&str]) -> bool {
    pf.templates
        .iter()
        .filter(|t| t.birth_keys == keys)
        .any(|t| pf.broken.contains(&t.display))
}

fn verdicts(pf: &ProgramFacts, key: &str) -> Vec<String> {
    pf.sites
        .iter()
        .filter(|s| s.key == key)
        .map(|s| format!("{:?}", s.verdict))
        .collect()
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
    let pf = units_of(&[
        ("a", a),
        (
            "m",
            "import { D } from './a'; const d = new D([1]); console.log(d.has(1));",
        ),
    ]);
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
    let pf = units_of(&[
        ("z", &DUAL.replace("./a", "./z")),
        (
            "m",
            "import { D } from './z'; const d = new D([1]); console.log(d.has(1));",
        ),
    ]);
    let mi = pf.module_paths.iter().position(|p| p == "m.ts").unwrap() as u32;
    let site = pf
        .sites
        .iter()
        .find(|s| s.module == mi && s.key == "has")
        .expect("the call in m");
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
    assert!(
        verdicts(&pf, "neg").iter().all(|v| v.starts_with("No")),
        "{}",
        pf.dump_text()
    );
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
    // Every declaration unit of `a` and `m` sees D (§12.3: a module's work
    // is its units' work).
    let w = |p: &str| module_work(&pf, p);
    assert_eq!(
        pf.round_work[1],
        w("a.ts") + w("m.ts"),
        "{:?} {:?}",
        pf.round_work,
        pf.units
    );
    assert!(
        verdicts(&pf, "neg").iter().all(|v| v.starts_with("No")),
        "{}",
        pf.dump_text()
    );
    let mi = pf.module_paths.iter().position(|p| p == "c.ts").unwrap() as u32;
    let site = pf
        .sites
        .iter()
        .find(|s| s.module == mi && s.key == "has")
        .expect("the call in c");
    assert_eq!(site.verdict, crate::Verdict::T0Mono, "{}", pf.dump_text());
}

#[test]
fn a_broken_namespace_field_function_resolves_only_the_units_that_see_it() {
    // zod `util.assertIs`: a function the `util` namespace IIFE stores on
    // the exported object is a contract function through the value contract
    // of `util` (decision 4), and no `fn_contracts_out` names it. `m` lets it
    // escape, so the census breaks it; its defining unit (`a`) owns the
    // display, so round 2 solves `a` and `m` only, not the unrelated `b`/`c`
    // (design §11.3). An unowned display would make every unit dirty.
    let a = "export var util: any;\n(function (util: any) {\n  function get(x: any) { return x.k; }\n  util.get = get;\n})(util || (util = {}));\n";
    let m = "import { util } from './a'; (globalThis as any).g = util.get; console.log(util.get({ k: 1 }));";
    let e = DUAL.replace("class D", "class E").replace("neg", "other");
    let pf = units_of(&[
        ("a", a),
        ("m", m),
        ("b", &e),
        (
            "c",
            "import { E } from './b'; const e = new E([1]); console.log(e.has(1));",
        ),
    ]);
    assert!(
        pf.broken.iter().any(|d| d == "fn:a.ts:get"),
        "{}",
        pf.dump_text()
    );
    // Rewritten for NSS (principle: `util = {}` and `util.get = get` are
    // effects of their own units, the write effect and the store effect,
    // each taking the round after the unit making it): the rounds after the
    // first solve `a` and `m` only, as the same program without `b` and `c`
    // does round for round.
    let am = units_of(&[("a", a), ("m", m)]);
    assert!(pf.round_work.len() >= 2, "{:?}", pf.round_work);
    assert_eq!(
        pf.round_work[1..],
        am.round_work[1..],
        "{:?} {:?}",
        pf.round_work,
        am.round_work
    );
}

#[test]
fn a_unit_over_the_bound_is_not_solved_again() {
    // `o` imports `a` but is over the work bound: it claims and publishes
    // nothing whatever its inputs, so when `m` breaks D it keeps its result
    // (design §11.3) and round 2 costs `a` + `m` only.
    // One declaration (§12.3: separate top-level statements would be
    // separate small units): the body of `big`, called twice (a body run
    // once is module scope for the cut, N5 §12.5).
    let big: String = (0..400)
        .map(|i| format!("const o{i} = {{ v: new D([{i}]) }}; console.log(o{i}.v.has({i}));\n"))
        .collect();
    let o = format!("import {{ D }} from './a';\nfunction big() {{\n{big}}}\nbig();\nbig();\n");
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
    let list: Vec<(String, &perry_hir::Module)> =
        hirs.iter().map(|(p, h)| (p.clone(), h)).collect();
    // The bound sits between the small units and `o` (measured unbounded).
    let free = crate::compute_units(&list, &FactsConfig::default());
    let wk = |p: &str| module_work(&free, p);
    assert!(
        wk("o.ts") > 4 * wk("a.ts").max(wk("m.ts")),
        "{:?}",
        free.units
    );
    let bound = 2 * wk("a.ts").max(wk("m.ts"));
    let pf = crate::compute_units(
        &list,
        &FactsConfig {
            max_work: Some(bound),
            ..FactsConfig::default()
        },
    );
    let aborted = |p: &str| {
        pf.units
            .iter()
            .any(|(q, s)| crate::decl_units::module_of(q) == p && s.aborted)
    };
    assert!(
        aborted("o.ts") && !aborted("a.ts") && !aborted("m.ts"),
        "{:?}",
        pf.units
    );
    assert_eq!(pf.broken, ["a.ts:D"], "{}", pf.dump_text());
    // Round 2 is `a`, `m` and the units of `o` under the bound (a unit
    // depends on every unit of each module its module imports, §12.6).
    let small_o: u64 = pf
        .units
        .iter()
        .filter(|(q, s)| crate::decl_units::module_of(q) == "o.ts" && !s.aborted)
        .map(|(_, s)| s.work)
        .sum();
    assert_eq!(
        pf.round_work[1],
        module_work(&pf, "a.ts") + module_work(&pf, "m.ts") + small_o,
        "{:?} {:?}",
        pf.round_work,
        pf.units
    );
}

#[test]
fn a_foreign_receiver_of_a_contract_method_escapes() {
    // `D.prototype.has` called on an object that is not a D: D's unit runs
    // it on that object unseen (`ContractThis`), so the object escapes.
    let m = "import { D } from './a'; const d = new D([1]); const h = d.has; const o = { neg: new Map() }; console.log(h.call(o, 1), o.neg.size);";
    let pf = units_of(&[("a", DUAL), ("m", m)]);
    let site = pf
        .sites
        .iter()
        .find(|s| s.module == 1 && s.key == "neg")
        .expect("o.neg in m");
    assert!(
        matches!(site.verdict, crate::Verdict::No(_)),
        "{}",
        pf.dump_text()
    );
}

#[test]
fn what_a_method_does_to_its_receiver_happens_to_the_instance() {
    // Another unit calls `grow` on an instance it constructed: the key
    // `extra` is added to a D after birth, so D's layout is not fixed; and
    // `self` hands the instance back to unknown code: D escapes (§11.2).
    let a = "export class D { pos: number; constructor() { this.pos = 1; } grow(v: number) { (this as any).extra = v; } }";
    let pf = units_of(&[
        ("a", a),
        (
            "m",
            "import { D } from './a'; const d = new D(); d.grow(2); console.log(d.pos);",
        ),
    ]);
    assert!(!tmpl(&pf, "D").fixed_layout, "{}", pf.dump_text());
    let b = "export class D { pos: number; constructor() { this.pos = 1; } me() { return this; } }";
    let pf = units_of(&[
        ("a", b),
        ("m", "import { D } from './a'; console.log(new D().me());"),
    ]);
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
    let v = |module: u32, key: &str| {
        pf.sites
            .iter()
            .filter(|s| s.module == module && s.key == key)
            .map(|s| format!("{:?}", s.verdict))
            .collect::<Vec<_>>()
    };
    assert_eq!(v(1, "n"), ["T0Mono"], "{}", pf.dump_text());
    assert_eq!(v(1, "warm"), ["T0Mono"], "{}", pf.dump_text());
    assert_eq!(v(0, "n"), ["E0"], "{}", pf.dump_text());
    assert_eq!(v(0, "warm"), ["E0"], "{}", pf.dump_text());
}

#[test]
fn what_an_argument_reaches_is_used_apart_from_the_argument() {
    // A contract carries two use sets per parameter (§11.2): the argument
    // and what it reaches. `header` converting `it.n` is no conversion of
    // `it` (a_function_contract_carries_its_result_and_borrowed_arguments);
    // soundness: a callee that keeps what its argument reaches lets that
    // escape, the argument itself only read.
    let b = "export function keep(p: any) { (globalThis as any).k = p.inner; }";
    // In one function: a module-scope `o` would be ⊤ to the reader's
    // declaration unit (§12.3) and the test could not fail; called twice,
    // so N5 does not run it as module scope.
    let m = "import { keep } from './b'; function main() { const o = { inner: { v: 1 } }; keep(o); (globalThis as any).k.v = 'x'; console.log(o.inner.v); } main(); main();";
    let pf = units_of(&[("b", b), ("m", m)]);
    assert!(!proven(&pf, "v"), "{}", pf.dump_text());
}

#[test]
fn an_argument_the_result_may_be_comes_back_to_the_caller() {
    // A result transfer (§11.2 "the return"): `id` returns its argument, so
    // the caller's store through the result is a store into its own `Box`.
    let b = "export function id(p: any) { return p; }";
    let m = "import { id } from './b'; class Box { v: any = 1; } const o = new Box(); const r = id(o); r.v = 'x'; console.log(o.v);";
    let pf = units_of(&[("b", b), ("m", m)]);
    assert!(!field(tmpl(&pf, "Box"), "v").readonly, "{}", pf.dump_text());
}

#[test]
fn a_class_contract_carries_its_statics() {
    // A static method is part of the class contract (§11.2): another unit's
    // `C.make()` calls it through the contract and gets a C.
    let t = "export class C { v = 1; static make() { return new C(); } }";
    let pf = units_of(&[
        ("t", t),
        (
            "m",
            "import { C } from './t'; const c = C.make(); console.log(c.v);",
        ),
    ]);
    assert!(pf.broken.is_empty(), "{}", pf.dump_text());
    assert!(
        verdicts(&pf, "v")
            .iter()
            .all(|v| v == "T0Mono" || v == "E0"),
        "{}",
        pf.dump_text()
    );
    assert!(!verdicts(&pf, "v").is_empty(), "{}", pf.dump_text());
}

#[test]
fn a_given_out_instance_receives_what_other_units_methods_do() {
    // Soundness of a given-out instance (`Model::give_out`): `make` gives
    // `keep` to another unit, which may call `set` on it with anything; so
    // `keep.v` may hold an unknown value, and `peek` returns it. The
    // caller's store through that result sinks its literal.
    // `peek` is a static: in `keep`'s declaration unit, which sees the
    // given-out instance itself (another unit would see a stand-in).
    let t = "let keep: any; export class C { v: any = 1; set(x: any) { this.v = x; } static make() { keep = new C(); return keep; } static peek() { return keep.v; } }";
    // In one function: a module-scope `own` read by another declaration
    // unit would be ⊤ there anyway (§12.3), and the test could not fail;
    // called twice, so N5 does not run it as module scope.
    let m = "import { C } from './t'; function main() { C.make(); const own = { n: 1, warm: 2 }; C.peek().k = own; console.log(own.warm); } main(); main();";
    let pf = units_of(&[("t", t), ("m", m)]);
    assert!(!proven(&pf, "warm"), "{}", pf.dump_text());
}

#[test]
fn a_class_a_contract_returns_carries_its_methods() {
    // A contract result may be an instance of a published class (`mk`);
    // its reader sees the whole class contract, methods included, even
    // when it never imports the class: `put` hands its argument to the
    // defining unit, so the caller's literal leaves.
    // `mk` and `C` share a declaration unit (both write `n`): `mk`'s
    // result is `C` itself, not another unit's stand-in.
    let t = "let n = 0; export class C { v: any = 1; constructor() { n++; } put(x: any) { this.v = x; } } export function mk() { n++; return new C(); }";
    let a = "import { C } from './t'; console.log(new C().v);";
    let m = "import { mk } from './t'; function main() { const own = { n: 1, warm: 2 }; mk().put(own); console.log(own.warm); } main(); main();";
    let pf = units_of(&[("t", t), ("a", a), ("m", m)]);
    assert!(!proven(&pf, "warm"), "{}", pf.dump_text());
}

#[test]
fn a_tdz_seed_writes_its_local_in_the_local_unit() {
    // A TDZ seed initializes its locals (§12.3: the seed statement is the
    // binding's own, joined with it): `a`'s unit sees the seeded value, as the whole
    // program does, so `k` is not proven to hold only numbers. A seed in a
    // unit of its own would write a stand-in of `a` and be lost.
    let src = "(function () { const g = () => { a = a; const o = { k: a }; return o.k; }; try { console.log(g()); } catch (e) {} let a = 1; console.log(g()); })();";
    let kind = |pf: &ProgramFacts| {
        pf.dump_text()
            .lines()
            .find(|l| l.starts_with("  F k "))
            .map(|l| {
                l.split_whitespace()
                    .find(|w| w.starts_with("kind="))
                    .unwrap_or("")
                    .to_string()
            })
    };
    let whole = facts(src);
    assert_eq!(
        kind(&whole).as_deref(),
        Some("kind=num|undef"),
        "{}",
        whole.dump_text()
    );
    let pf = units(src);
    assert_eq!(kind(&pf), kind(&whole), "{}", pf.dump_text());
}

#[test]
fn a_function_value_read_from_another_unit_is_a_function_contract() {
    // A module-scope binding holding a function (a flattened CommonJS
    // body's `let f = function ...`, sonic-boom's releaseWritingBuf) is a
    // function contract (§11.2): `f` only reads its argument, so the
    // caller's literal stays its own.
    let t = "export const f = (o: any) => o.n;";
    let m = "import { f } from './t'; function main() { const own = { n: 1, warm: 2 }; console.log(f(own)); console.log(own.warm); } main(); main();";
    let pf = units_of(&[("t", t), ("m", m)]);
    assert!(proven(&pf, "warm"), "{}", pf.dump_text());
    assert!(!verdicts(&pf, "warm").is_empty(), "{}", pf.dump_text());
    // Soundness: a function value that keeps its argument lets it escape.
    let t = "export const g = (o: any) => { (globalThis as any).k = o; };";
    let m = "import { g } from './t'; function main() { const own = { n: 1, warm: 2 }; g(own); console.log(own.warm); } main(); main();";
    let pf = units_of(&[("t", t), ("m", m)]);
    assert!(!proven(&pf, "warm"), "{}", pf.dump_text());
}

#[test]
fn a_store_into_another_units_prototype_breaks_the_class() {
    // Soundness: a store into a stand-in class's prototype (or the class
    // object) is beyond the contract, the census breaks the class (§11.3).
    let t = "export class C { v = 1; get() { return this.v; } }";
    let m = "import { C } from './t'; (C as any).prototype.get = function () { return 'x'; }; console.log(new C().get());";
    let pf = units_of(&[("t", t), ("m", m)]);
    assert_eq!(pf.broken, ["t.ts:C"], "{}", pf.dump_text());
}

#[test]
fn a_unit_view_carries_only_its_own_table_entries() {
    // Each module-wide item table is split over the views (§12.6 step 1): an
    // exported object is in exactly one unit's table, its owner's.
    let src = "export const cfg = { a: 1 }; export function g() { return 2; } export function h() { return 3; }";
    let m = perry_parser::parse_typescript(src, "a.ts").expect("parses");
    let hir = perry_hir::lower_module(&m, "a", "a.ts").expect("lowers");
    assert_eq!(hir.exported_objects, ["cfg"]);
    let vs = crate::decl_units::views("a.ts", &hir);
    assert!(vs.len() >= 3, "{}", vs.len());
    assert_eq!(
        vs.iter()
            .filter(|(v, _)| !v.exported_objects.is_empty())
            .count(),
        1
    );
}

#[test]
fn a_unit_view_is_indices_into_the_one_module() {
    // Phase 03m (CWN3M): a view borrows the module it is cut from. A view
    // holds no `Module` (the type has none to hold: the mechanism is the
    // type); its items are indices into the one module, and the views
    // together partition the module's declarations: every function, global
    // and init statement is in exactly one view, a literal shape is seen by
    // every unit that births it (one class, never a copy). On tscwork the
    // per-unit module clones were 16.4 GB of the facts step's 23 GB.
    let src = "export const cfg = { a: 1 }; export function g() { return { b: 2 }; } export function h() { return g().b + cfg.a; } export function k() { return { b: 3 }; }";
    let m = perry_parser::parse_typescript(src, "a.ts").expect("parses");
    let hir = perry_hir::lower_module(&m, "a", "a.ts").expect("lowers");
    let vs = crate::decl_units::views("a.ts", &hir);
    assert!(vs.len() >= 3, "{}", vs.len());
    let sorted = |f: &dyn Fn(&crate::decl_units::UnitView) -> Vec<usize>| {
        let mut all: Vec<usize> = vs.iter().flat_map(|(v, _)| f(v)).collect();
        all.sort_unstable();
        all
    };
    assert_eq!(
        sorted(&|v| v.funcs.clone()),
        (0..hir.functions.len()).collect::<Vec<_>>()
    );
    assert_eq!(
        sorted(&|v| v.globals.clone()),
        (0..hir.globals.len()).collect::<Vec<_>>()
    );
    assert_eq!(
        sorted(&|v| v.init.clone()),
        (0..hir.init.len()).collect::<Vec<_>>()
    );
    let mut shapes_seen = 0;
    for (k, c) in hir.classes.iter().enumerate() {
        let n = vs.iter().filter(|(v, _)| v.classes.contains(&k)).count();
        if c.is_literal_shape() {
            assert!(n >= 1, "shape {} seen by no unit", c.name);
            shapes_seen += n;
        } else {
            assert_eq!(n, 1, "class {} in {n} views", c.name);
        }
    }
    // `{ b: 2 }` and `{ b: 3 }` share one literal shape, born in two units.
    assert!(shapes_seen >= 2, "{shapes_seen}");
    // Every index resolves in the one module (a view reads through it).
    for (v, _) in &vs {
        for &k in &v.funcs {
            assert!(k < hir.functions.len());
        }
        for &k in &v.classes {
            assert!(k < hir.classes.len());
        }
    }
}

#[test]
fn an_argument_the_callee_writes_escapes() {
    // `bump` stores into what it is given: the caller's literal escapes.
    let b = "export function bump(o: { n: number }) { o.n = o.n + 1; }";
    // In one function: a module-scope `own` would be ⊤ to the reader's
    // declaration unit (§12.3) and the test could not fail; called twice,
    // so N5 does not run it as module scope.
    let m = "import { bump } from './b'; function main() { const own = { n: 1, warm: 2 }; bump(own); console.log(own.warm); } main(); main();";
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
    assert!(
        ids.len() >= 2 && ids.iter().all(|c| c.is_some() && *c == ids[0]),
        "{}",
        pf.dump_text()
    );
    assert!(
        pf.templates.iter().any(|t| t.prefix_stable),
        "{}",
        pf.dump_text()
    );
    assert!(
        !pf.stable_classes().contains_key(&ids[0].unwrap()),
        "{}",
        pf.dump_text()
    );
}

#[test]
fn a_result_that_is_not_a_literal_layout_escapes() {
    // `mk` returns an instance of `C`, a class of another declaration unit:
    // `mk`'s unit holds C's stand-in and forwards it with C's contract
    // (§17.10 4, DECISIONS 56; this expectation was "the result escapes"
    // before C's contract was forwarded). The principle the test keeps:
    // `m` may run `set` on it with anything, so C's own unit models that
    // (its methods run on the receiver with ⊤ arguments): `x` takes any
    // kind and is written after construction. Its layout does not change.
    let a = "class C { x: number; constructor() { this.x = 1; } set(v: any) { this.x = v; } } export function mk() { return new C(); }";
    let pf = units_of(&[("a", a), ("m", "import { mk } from './a'; mk().set('s');")]);
    let x = field(tmpl(&pf, "C"), "x");
    assert!(!x.readonly && x.writers_post > 0, "{}", pf.dump_text());
    assert!(x.kind.top && !x.kind.fixed(), "{}", pf.dump_text());
}

#[test]
fn a_module_value_read_from_another_unit_keeps_its_layout() {
    // `cfg` and its readers are separate declaration units (§12.3): the
    // binding is a value contract (§12), its layout the defining unit's
    // proof, so the readers' sites stay proven.
    let src = r#"
        const cfg = { a: 1, b: 2 };
        function useA() { return cfg.a; }
        function useB() { return cfg.b; }
        console.log(useA() + useB());
        "#;
    let pf = units(src);
    assert!(pf.units.len() >= 3, "{}", pf.dump_text());
    assert!(proven(&pf, "a") && proven(&pf, "b"), "{}", pf.dump_text());
    assert!(pf.broken.is_empty(), "{:?}", pf.broken);
    // A reader that stores a new key through an alias of the binding (not
    // a join, §12.1): rewritten for design §16 (principle: a literal
    // layout's new key is a shape transition its defining unit performs,
    // not a census break). The owner's layout gains `c` (unknown value);
    // the birth key `a` keeps its slot.
    let src = r#"
        const cfg = { a: 1, b: 2 };
        function useA() { return cfg.a; }
        function grow() { const o: any = cfg; o.c = 3; }
        grow();
        console.log(useA());
        "#;
    let pf = units(src);
    assert!(
        !verdicts(&pf, "a").is_empty() && proven(&pf, "a"),
        "{}",
        pf.dump_text()
    );
    assert!(pf.broken.is_empty(), "{:?}", pf.broken);
    assert!(gains_key(&pf, "c"), "{}", pf.dump_text());
}

#[test]
fn a_namespace_import_sees_only_the_real_exports() {
    // What a declaration unit exports to the other units of its module is
    // the cut's plumbing (§12.3), not an export of the module: a namespace
    // import sees the module's own exports, so `cfg` stays a value contract
    // and does not escape through the namespace object.
    let t = r#"
        const cfg = { a: 1, b: 2 };
        export function useA() { return cfg.a; }
        "#;
    let pf = units_of(&[
        ("t", t),
        ("u", "import * as m from './t'; console.log(m.useA());"),
    ]);
    assert!(pf.units.len() >= 3, "{:?}", pf.units);
    assert!(proven(&pf, "a"), "{}", pf.dump_text());
    // Soundness: a real export stays on the namespace (§18.2: `m.cfg` is
    // what `import { cfg }` binds, cfg's value contract). A store through it
    // into the existing field `a` is a store effect (decision 17): the
    // layout holds and `a` holds an unknown value. A store of a new key is
    // the owner's shape transition (rewritten for design §16, principle: a
    // literal layout's new key is performed by its defining unit).
    let t = r#"
        export const cfg = { a: 1, b: 2 };
        export function useA() { return cfg.a; }
        "#;
    let u = "import * as m from './t'; (m.cfg as any).a = 'x'; console.log(m.useA());";
    let pf = units_of(&[("t", t), ("u", u)]);
    assert!(pf.broken.is_empty(), "{:?}\n{}", pf.broken, pf.dump_text());
    let cfg = pf
        .templates
        .iter()
        .find(|t| t.birth_keys == ["a", "b"])
        .unwrap_or_else(|| panic!("cfg in\n{}", pf.dump_text()));
    let a = cfg.fields.iter().find(|f| f.key == "a").expect("field a");
    // Rewritten for DECISIONS 73 (principle: a store effect carries the kinds
    // it stores): the field holds the stored primitive, not ⊤.
    assert!(
        !a.readonly && a.kind.prims & crate::model::prim::STR != 0,
        "{}",
        pf.dump_text()
    );
    let u = "import * as m from './t'; (m.cfg as any).z = 'x'; console.log(m.useA());";
    let pf = units_of(&[("t", t), ("u", u)]);
    assert!(pf.broken.is_empty(), "{:?}\n{}", pf.broken, pf.dump_text());
    assert!(gains_key(&pf, "z"), "{}", pf.dump_text());
}

#[test]
fn a_once_invoked_wrapper_splits() {
    // A body that runs exactly once at a module-scope statement is module
    // scope for the cut (N5, §12.5): the CommonJS factory and a bundle's
    // `((module2) => {…})(module)` inside it. Its declarations are units
    // of their own, so `a`'s private literal keeps its proof.
    let wrapped = |inner: &str| {
        format!(
            r#"
        function __perry_cjs_factory() {{
            const module: any = {{ exports: {{}} }};
            {inner}
        }}
        __perry_cjs_factory();
        "#
        )
    };
    let body = r#"((module2) => {
                function a() { const o = { k: 1 }; return o.k; }
                function b() { return 2; }
                module2.exports = { a, b };
            })(module);"#;
    let pf = units(&wrapped(body));
    assert!(pf.units.len() >= 4, "{:?}", pf.units);
    assert!(proven(&pf, "k"), "{}", pf.dump_text());
    // Only this shape: a literal called through a name twice, or a body
    // that returns, stays inside its declaration (`a` no statement of the
    // module scope).
    let at_scope = |src: &str| {
        let m = perry_parser::parse_typescript(&wrapped(src), "t.ts").expect("parses");
        let hir = perry_hir::lower_module(&m, "t", "t.ts").expect("lowers");
        let flat = crate::decl_units::run_once_flat(&hir).unwrap_or(hir);
        flat.init
            .iter()
            .any(|s| matches!(s, perry_hir::Stmt::Let { name, .. } if name == "a"))
    };
    assert!(at_scope(body));
    // The pipeline's CommonJS shape: the factory is a local function named
    // only at its call, the wrapper's and the factory's final `return` the
    // value of `_cjs`.
    let cjs = |inner: &str| {
        format!(
            r#"
        const _cjs = (function () {{
            function __perry_cjs_factory() {{
                const module: any = {{ exports: {{}} }};
                {inner}
                return module.exports;
            }}
            const record: any = {{ factory: __perry_cjs_factory }};
            return __perry_cjs_factory();
        }})();
        console.log(_cjs);
        "#
        )
    };
    let pf = units(&cjs(body));
    assert!(pf.units.len() >= 4, "{:?}", pf.units);
    assert!(proven(&pf, "k"), "{}", pf.dump_text());
    let twice = r#"const w = (module2) => { function a() { return 1; } module2.exports = { a }; };
            w(module); w(module);"#;
    let ret = r#"((module2) => { function a() { return 1; } if (module2) return; module2.exports = { a }; })(module);"#;
    let this = r#"((module2) => { function a() { return 1; } module2.exports = { a, t: this }; })(module);"#;
    // A local function that calls itself runs more than once.
    // Another function the record holds stays a declaration.
    let held = r#"const r = (function () { function f() { function a() { return 1; } return a; } const rec: any = { f }; return f(); })();"#;
    assert!(!at_scope(held));
    let recur = r#"const r = (function () { function f(n: any) { function a() { return 1; } return n ? f(0) : a; } return f(1); })();"#;
    assert!(!at_scope(twice) && !at_scope(ret) && !at_scope(this) && !at_scope(recur));
}

#[test]
fn a_bundle_splits_into_declaration_units() {
    // One module, three declarations no cycle joins (§12.3): three units
    // and the statement; A's private layout keeps its proof in its unit.
    let src = r#"
        class A { x: number; constructor() { this.x = 1; } get() { return this.x; } }
        class B { y: number; constructor() { this.y = 2; } }
        function use() { return new A().get() + new B().y; }
        console.log(use());
        "#;
    let pf = units(src);
    assert!(pf.units.len() >= 4, "{:?}", pf.units);
    assert!(pf.broken.is_empty(), "{:?}", pf.broken);
    assert!(tmpl(&pf, "A").fixed_layout, "{}", pf.dump_text());
}

#[test]
fn a_store_through_a_class_name_joins_it_a_write_of_a_binding_does_not() {
    // Rewritten for NSS (principle: the cut follows data dependencies; a
    // write of another unit's binding is the binding's write effect, a store
    // through a binding a store effect into the object it holds, neither a
    // join). A store through a class's name is the class's state (§12.1):
    // one unit. Two functions writing one `let` are units of their own,
    // ordered after the binding's unit as its readers are.
    let n = |src: &str| units(src).units.len();
    assert_eq!(n("class C { v = 1; } C.create = () => new C();"), 1);
    assert_eq!(
        n("let k = 0; function inc() { k++; } function dec() { k--; }"),
        3
    );
    // A read alone is one way: the reader is a unit of its own.
    assert_eq!(n("const k = 0; function get() { return k; }"), 2);
    // In a once-run wrapper (N5), as a CommonJS body, where `D` is a
    // captured local of the wrapper: a closure writing `D` is a unit of its
    // own, as one reading it is.
    let wrapped = |body: &str| format!("(function () {{ {body} }})();");
    let reads = n(&wrapped(
        "var D: any; D = D || {}; const f = () => D.a; const g = () => D.b; console.log(f(), g());",
    ));
    let writes = n(&wrapped("var D: any; D = D || {}; const f = () => { D = 1; }; const g = () => D.b; console.log(f(), g());"));
    assert_eq!(writes, reads, "reads {reads} writes {writes}");
}

#[test]
fn a_let_is_its_id_not_its_name() {
    // A binding is its declaration, not its spelling (§12.1): two bodies a
    // once-run wrapper brings to module scope (N5) each declare their own
    // `T`, two bindings of one name, two units; a `var` declared twice is
    // one binding: both statements are one unit.
    let n = |src: &str| units(src).units.len();
    let wrapped = |body: &str| format!("(function () {{ {body} }})();");
    let two = |a: &str, b: &str| {
        wrapped(&format!(
            "((m: number) => {{ const {a}: any = {{ a: m }}; console.log({a}.a); }})(1); \
         ((m: number) => {{ const {b}: any = {{ b: m }}; console.log({b}.b); }})(2);"
        ))
    };
    let same = n(&two("T", "T"));
    let apart = n(&two("T", "U"));
    assert_eq!(same, apart, "same name {same} distinct names {apart}");
    assert!(apart >= 4, "{apart}");
    let var_twice = units("var v: any = 1; var v: any = 3;");
    assert_eq!(var_twice.units.len(), 1, "{:?}", var_twice.units);
}

#[test]
fn a_literal_shape_is_copied_into_each_unit_that_births_it() {
    // The shape-only class of a closed object literal is a layout, not a
    // declaration (§12.1): each unit birthing it has its own copy, so a
    // literal and the reads on it stay one unit's proof.
    let src = r#"
        function mk() { return { a: 1, b: 2 }; }
        function use() { const o = { a: 3, b: 4 }; return o.a; }
        console.log(mk().b + use());
        "#;
    let pf = units(src);
    assert!(pf.units.len() >= 3, "{}", pf.dump_text());
    assert!(proven(&pf, "a"), "{}", pf.dump_text());
    assert!(pf.broken.is_empty(), "{:?}", pf.broken);
}

#[test]
fn copies_of_one_emitted_closure_join_their_units() {
    // Closure creation copies retain FuncId and share one emitted body.
    // All callers and captures belong to one solve for that emitted body.
    let src = r#"
        function a() { return [1, 2].filter((x) => !!x).length; }
        function b() { return 2; }
        console.log(a() + b());
        "#;
    let module = perry_parser::parse_typescript(src, "t.ts").expect("parses");
    let mut hir = perry_hir::lower_module(&module, "t", "t.ts").expect("lowers");
    let base = crate::decl_units::decl_units(&hir).len();
    let ka = hir.functions.iter().position(|f| f.name == "a").unwrap();
    let kb = hir.functions.iter().position(|f| f.name == "b").unwrap();
    hir.functions[kb].body = hir.functions[ka].body.clone();
    assert_eq!(crate::decl_units::decl_units(&hir).len(), base - 1);
    let src = r#"
        function outer() { let s = { a: 1 }; function inner() { return s.a; } return inner(); }
        console.log(outer());
        "#;
    let pf = units(src);
    assert!(proven(&pf, "a"), "{}", pf.dump_text());
}

#[test]
fn a_literal_method_called_from_another_unit_keeps_its_target() {
    // A published literal's field holding only a function of its unit is a
    // function contract (§11.2 applied to the field, §12 value contracts):
    // the reader's call keeps its single target.
    for src in [
        r#"
        const util = { f: (m: any) => m, g: (m: any) => m };
        function a(x: any) { return util.f(x); }
        function b(x: any) { return util.g(x); }
        console.log(a("s"), b(1));
        "#,
        r#"
        var eu: any;
        (function (eu: any) { eu.f = (m: any) => m; })(eu || (eu = {}));
        function a(x: any) { return eu.f(x); }
        console.log(a("s"));
        "#,
    ] {
        let whole = facts(src);
        assert!(proven(&whole, "f"), "{}", whole.dump_text());
        let pf = units(src);
        assert!(pf.units.len() >= 3, "{}", pf.dump_text());
        assert!(proven(&pf, "f"), "{}", pf.dump_text());
    }
}

#[test]
fn a_unit_publishes_only_its_own_exports() {
    // A declaration unit's view carries only its own items' entries of the
    // module's item tables (§12.6 step 1): the other units of `a` must not
    // publish `cfg` (as ⊤, they cannot see it) over its owner's contract.
    let a = "export const cfg = { a: 1 }; export function g() { return 2; } export function h() { return 3; }";
    let pf = units_of(&[
        ("a", a),
        (
            "m",
            "import { cfg, g, h } from './a'; console.log(cfg.a + g() + h());",
        ),
    ]);
    assert!(pf.units.len() >= 4, "{:?}", pf.units);
    assert!(proven(&pf, "a"), "{}", pf.dump_text());
}

#[test]
fn a_result_holding_another_units_layout_forwards_its_contract() {
    // `norm` returns `defaults` (another declaration unit's value) or its
    // own literal (qs normalizeStringifyOptions): both layouts are proofs
    // of their defining units, so the caller's reads stay proven.
    let src = r#"
        const defaults = { zz: 1, yy: 2 };
        function norm(o: any) { if (!o) { return defaults; } return { zz: o.k, yy: 0 }; }
        function main(o: any) { const x = norm(o); console.log(x.zz); }
        main(undefined); main({ k: 3 });
        "#;
    let pf = units(src);
    assert!(pf.units.len() >= 3, "{}", pf.dump_text());
    assert!(
        proven(&pf, "zz") && !verdicts(&pf, "zz").is_empty(),
        "{}",
        pf.dump_text()
    );
    // The caller storing a new key into what it got: rewritten for design
    // §16 (principle: a literal layout's new key is a shape transition its
    // defining unit performs). Both layouts gain `q`; `zz` keeps its slot.
    let src = r#"
        const defaults = { zz: 1, yy: 2 };
        function norm(o: any) { if (!o) { return defaults; } return { zz: o.k, yy: 0 }; }
        function main(o: any) { const x: any = norm(o); x.q = 1; console.log(x.zz); }
        main(undefined); main({ k: 3 });
        "#;
    let pf = units(src);
    assert!(pf.broken.is_empty(), "{:?}\n{}", pf.broken, pf.dump_text());
    assert!(
        pf.templates
            .iter()
            .filter(|t| t.fields.iter().any(|f| f.key == "zz"))
            // Rewritten for DECISIONS 73 (principle: a store effect carries the kinds
            // it stores): `q` holds the stored number, not ⊤.
            .all(|t| t.adds_keys > 0
                && t.fields
                    .iter()
                    .any(|f| f.key == "q" && f.kind.prims & crate::model::prim::NUM != 0)),
        "{}",
        pf.dump_text()
    );
}

#[test]
fn a_closed_function_receives_what_its_callers_pass() {
    // Decision 13 (qs: object-inspect's indentedJoin): `join` is never
    // exported and only ever called directly, so all its calls are known;
    // `main` (another declaration unit) passes `mkIndent`'s literal, so
    // `indent.prev` / `indent.base` hold on that layout instead of ⊤.
    let src = r#"
        function mkIndent() { return { prev: 'a', base: 'b' }; }
        function join(xs: any, indent: any) { return xs + indent.prev + indent.base; }
        function main(x: any) { const ind = mkIndent(); console.log(join(x, ind)); }
        main(1); main(2);
        "#;
    let pf = units(src);
    assert!(pf.units.len() >= 3, "{}", pf.dump_text());
    assert!(
        proven(&pf, "prev") && proven(&pf, "base"),
        "{}",
        pf.dump_text()
    );
    assert!(!verdicts(&pf, "prev").is_empty(), "{}", pf.dump_text());
    assert!(pf.broken.is_empty(), "{:?}", pf.broken);
    // A closed caller passing its own parameter on (object-inspect's
    // collectionOf → indentedJoin) passes what its callers pass.
    let chain = r#"
        function mkIndent() { return { prev: 'a', base: 'b' }; }
        function join(xs: any, indent: any) { return xs + indent.prev + indent.base; }
        function mid(xs: any, indent: any) { return join(xs, indent); }
        function main(x: any) { const ind = mkIndent(); console.log(mid(x, ind)); }
        main(1); main(2);
        "#;
    let pf = units(chain);
    assert!(pf.units.len() >= 4, "{}", pf.dump_text());
    assert!(
        proven(&pf, "prev") && !verdicts(&pf, "prev").is_empty(),
        "{}",
        pf.dump_text()
    );
    // A `var` bound to a function literal (side-channel-list's
    // `var listGetNode = function …`): its hoisted placeholder holds
    // undefined, no argument: still closed.
    // In a once-run wrapper, as a CommonJS body (N5 brings it to module
    // scope).
    let var_fn = r#"
        (function () {
            function mkIndent() { return { prev: 'a', base: 'b' }; }
            function main(x: any) { const ind = mkIndent(); console.log(join(x, ind)); }
            var join = function (xs: any, indent: any) { return xs + indent.prev + indent.base; };
            main(1); main(2);
        })();
        "#;
    let pf = units(var_fn);
    assert!(
        proven(&pf, "prev") && !verdicts(&pf, "prev").is_empty(),
        "{}",
        pf.dump_text()
    );
    // Interpreted code that reaches only globals and exports (qs: a
    // `Function(...)` site) cannot name `join`: it stays closed. A direct
    // eval naming module bindings could call it with anything: not closed.
    let module = perry_parser::parse_typescript(src, "t.ts").expect("parses");
    let hir = perry_hir::lower_module(&module, "t", "t.ts").expect("lowers");
    let mut cfg = FactsConfig::default();
    cfg.dynamic_code_reach.record_global("Function".into());
    let pf = crate::compute_units(&[("t.ts".to_string(), &hir)], &cfg);
    assert!(proven(&pf, "prev"), "{}", pf.dump_text());
    cfg.dynamic_code_reach.record_direct("t.ts", "eval".into());
    let pf = crate::compute_units(&[("t.ts".to_string(), &hir)], &cfg);
    assert!(!proven(&pf, "prev"), "{}", pf.dump_text());
    // The callee storing a new key into what it got: rewritten for design
    // §16 (principle: a literal layout's new key is a shape transition its
    // defining unit performs). The caller's layout gains `q`, `prev` holds;
    // a computed-key store is beyond the contract: the census breaks it.
    let src = r#"
        function mkIndent() { return { prev: 'a', base: 'b' }; }
        function join(xs: any, indent: any) { indent.q = 1; return xs + indent.prev; }
        function main(x: any) { const ind = mkIndent(); console.log(join(x, ind)); }
        main(1); main(2);
        "#;
    let pf = units(src);
    assert!(pf.broken.is_empty(), "{:?}\n{}", pf.broken, pf.dump_text());
    assert!(gains_key(&pf, "q"), "{}", pf.dump_text());
    let pf = units(&src.replace("indent.q = 1;", "indent[xs] = 1;"));
    assert!(!proven(&pf, "prev"), "{}", pf.dump_text());
    // Soundness: a function also used as a value is not closed: here it
    // escapes in its own unit (unknown code may call it with anything),
    // where its parameters would otherwise hold only the callers' layouts.
    let src = r#"
        function mkIndent() { return { prev: 'a', base: 'b' }; }
        function join(xs: any, indent: any) { (globalThis as any).j = join; return xs + indent.prev; }
        function main(x: any) { const ind = mkIndent(); console.log(join(x, ind)); }
        main(1); main(2);
        "#;
    let pf = units(src);
    assert!(!proven(&pf, "prev"), "{}", pf.dump_text());
    // Soundness: an exported function is not closed: another module calls
    // it with its own objects.
    let t = r#"
        function mkIndent() { return { prev: 'a', base: 'b' }; }
        export function join(xs: any, indent: any) { return xs + indent.prev; }
        function main(x: any) { const ind = mkIndent(); console.log(join(x, ind)); }
        main(1); main(2);
        "#;
    let u = "import { join } from './t'; function go() { console.log(join(1, { prev: 2, other: 3 })); } go(); go();";
    let pf = units_of(&[("t", t), ("u", u)]);
    assert!(!proven(&pf, "prev"), "{}", pf.dump_text());
}

#[test]
fn a_store_into_an_existing_field_is_a_store_effect() {
    // Decision 17 (qs: side-channel-list): `setNode` (a closed function,
    // its own unit) stores into the `next` field of `mkHead`'s literal.
    // That key exists, so the layout stays fixed: a store effect, not a
    // census report; the head is not broken and not escaped, so the reads
    // through it hold on its layout in every unit.
    let src = r#"
        function mkHead() { return { next: undefined as any }; }
        function setNode(list: any, k: any) { const n = { key: k }; list.next = n; return list.next; }
        function main(x: any) { const h = mkHead(); setNode(h, x); console.log(h.next); }
        main(1); main(2);
        "#;
    let pf = units(src);
    assert!(pf.units.len() >= 3, "{}", pf.dump_text());
    // Rewritten for NSS (principle: a store effect carries the stored
    // value's contract, as a published binding's value): the head is not
    // broken; the node's layout is, where `main` logs it beyond its
    // contract.
    assert!(!literal_broken(&pf, &["next"]), "{:?}", pf.broken);
    assert!(
        !verdicts(&pf, "next").is_empty() && proven(&pf, "next"),
        "{}",
        pf.dump_text()
    );
    // The stored value escapes: other units read that field as unknown.
    let node = pf
        .templates
        .iter()
        .find(|t| t.birth_keys == ["key"])
        .expect("node literal");
    assert!(node.escape.is_some(), "{}", pf.dump_text());
    // Soundness: the owner performs the effect. `main` (the head's unit)
    // reads `h.next.key`; its literal put `{ key: 1 }` there, but `setNode`
    // stored another object: `.key` must not hold on the inner layout.
    let src = r#"
        function setNode(list: any, k: any) { list.next = { other: k, key: 's' }; }
        function main(x: any) { const h = { next: { key: 1 } as any }; setNode(h, x); console.log(h.next.key); }
        main(1); main(2);
        "#;
    let pf = units(src);
    assert!(pf.broken.is_empty(), "{:?}", pf.broken);
    assert!(
        !verdicts(&pf, "key").is_empty() && !proven(&pf, "key"),
        "{}",
        pf.dump_text()
    );
    // What an argument reaches is not its literal's class (qs: listSet's
    // `node.value = value`, node = listGetNode(objects)): an object
    // literal's `constructor` is Object's, so a store into a value reached
    // from the head stores into no class object or prototype of its layout
    // (no census report), and the owner's sink of what the head reaches
    // does not escape the head through its prototype.
    let src = r#"
        function first(list: any) { return list.next; }
        function setv(list: any, v: any) { const n = first(list); n.value = v; }
        function main(x: any) { const h = { next: undefined as any }; setv(h, x); console.log(h.next); }
        main(1); main(2);
        "#;
    let pf = units(src);
    assert!(pf.broken.is_empty(), "{:?}", pf.broken);
    assert!(
        !verdicts(&pf, "next").is_empty() && proven(&pf, "next"),
        "{}",
        pf.dump_text()
    );
    // A new key: rewritten for design §16 (principle: a literal layout's new
    // key is a shape transition its defining unit performs, not a census
    // report). The head gains `extra`; a computed key still opens it (§5).
    let src = r#"
        function mkHead() { return { next: undefined as any }; }
        function setNode(list: any, k: any) { list.extra = k; }
        function main(x: any) { const h = mkHead(); setNode(h, x); console.log(h.next); }
        main(1); main(2);
        "#;
    let pf = units(src);
    assert!(pf.broken.is_empty(), "{:?}\n{}", pf.broken, pf.dump_text());
    assert!(gains_key(&pf, "extra"), "{}", pf.dump_text());
    let pf = units(&src.replace("list.extra = k;", "list[k] = k;"));
    assert!(!pf.broken.is_empty(), "{}", pf.dump_text());
}

#[test]
fn a_key_another_unit_adds_to_a_literal_is_a_shape_transition() {
    // Design §16, DECISIONS 66 (tssrc destructuring.ts:487
    // `flattenContext.hasTransformedPriorElement = true` in a closed callee
    // two calls below the literal's unit): a named store of a new data key
    // into another unit's literal layout is a store effect its defining
    // unit performs. The owner's layout gains the key (an anonymous store
    // of an unknown value after birth), so the birth keys hold their slots
    // in every unit and the census breaks nothing.
    let src = r#"
        interface Ctx { level: number; visitor: any; seen?: boolean }
        function inner(c: Ctx, x: any) { if (x) { c.seen = true; } return c.level + c.visitor; }
        function outer(c: Ctx, x: any) { return inner(c, x) + c.level; }
        function main(x: any) { const c: Ctx = { level: x, visitor: 2 }; console.log(outer(c, x) + c.visitor); }
        main(1); main(2);
        "#;
    let pf = units(src);
    assert!(pf.units.len() >= 3, "{}", pf.dump_text());
    assert!(pf.broken.is_empty(), "{:?}\n{}", pf.broken, pf.dump_text());
    for k in ["level", "visitor"] {
        assert!(
            verdicts(&pf, k).len() >= 2 && proven(&pf, k),
            "{k}: {:?}\n{}",
            verdicts(&pf, k),
            pf.dump_text()
        );
    }
    // Soundness: the owner holds the transition. Its layout adds a key and
    // the added field holds an unknown value (the stored one escaped).
    let c = pf
        .templates
        .iter()
        .find(|t| t.birth_keys == ["level", "visitor"])
        .unwrap_or_else(|| panic!("ctx literal in\n{}", pf.dump_text()));
    assert!(c.adds_keys > 0, "{}", pf.dump_text());
    let seen = c
        .fields
        .iter()
        .find(|f| f.key == "seen")
        .expect("field seen");
    // Rewritten for DECISIONS 73 (principle: a store effect carries the kinds
    // it stores): the field holds the stored primitive, not ⊤.
    assert!(
        !seen.readonly && seen.kind.prims & crate::model::prim::BOOL != 0,
        "{}",
        pf.dump_text()
    );
    // A computed-key store still opens the layout: a census report (§5).
    let src = r#"
        function grow(c: any, k: any) { c[k] = 1; }
        function main(x: any) { const c = { level: x }; grow(c, x); console.log(c.level); }
        main(1); main(2);
        "#;
    let pf = units(src);
    assert!(!pf.broken.is_empty(), "{}", pf.dump_text());
    // `__proto__` is no data key: it changes the prototype (census report).
    let src = r#"
        function grow(c: any, p: any) { c.__proto__ = p; }
        function main(x: any) { const c = { level: x }; grow(c, x); console.log(c.level); }
        main(1); main(2);
        "#;
    let pf = units(src);
    assert!(!pf.broken.is_empty(), "{}", pf.dump_text());
}

#[test]
fn a_function_value_passed_to_a_closed_callee_keeps_its_arguments() {
    // Decision 19 (tssrc moduleNameResolver: `loadModuleFromFileNoPackageId`
    // passed as `loader` to closed `tryLoadModuleUsingOptionalResolutionSettings`,
    // which calls it with `state`): `load` is referenced only as a direct
    // callee or as an argument at a call of a closed function, so it is
    // closed too; `run` (its own unit) publishes what its calls of the
    // function value pass, and `state.flag` holds on `mkState`'s literal.
    let src = r#"
        function mkState() { return { flag: 'a', host: 'b' }; }
        function load(x: any, state: any) { return x + state.flag + state.host; }
        function run(x: any, loader: any, state: any) { return loader(x, state); }
        function main(x: any) { const st = mkState(); console.log(run(x, load, st)); }
        main(1); main(2);
        "#;
    let pf = units(src);
    assert!(pf.units.len() >= 4, "{}", pf.dump_text());
    assert!(
        proven(&pf, "flag") && proven(&pf, "host"),
        "{}",
        pf.dump_text()
    );
    assert!(!verdicts(&pf, "flag").is_empty(), "{}", pf.dump_text());
    // A test must be able to fail: with no published arguments the
    // parameters of `load` hold nothing and the reads are vacuously T0 on
    // no layout; the proof must name the literal the calls of `run` pass.
    assert!(
        pf.sites
            .iter()
            .filter(|s| s.key == "flag" || s.key == "host")
            .all(|s| !s.receivers.is_empty()),
        "{}",
        pf.dump_text()
    );
    assert!(pf.broken.is_empty(), "{:?}", pf.broken);
    // Soundness: the callee lets the function value escape (unknown code
    // may call it with anything): the census breaks it, its parameters ⊤.
    let src = r#"
        function mkState() { return { flag: 'a', host: 'b' }; }
        function load(x: any, state: any) { return x + state.flag + state.host; }
        function run(x: any, loader: any, state: any) { (globalThis as any).l = loader; return loader(x, state); }
        function main(x: any) { const st = mkState(); console.log(run(x, load, st)); }
        main(1); main(2);
        "#;
    let pf = units(src);
    assert!(!proven(&pf, "flag"), "{}", pf.dump_text());
    // Soundness: the callee returns it; its caller calls it with a number.
    let src = r#"
        function mkState() { return { flag: 'a', host: 'b' }; }
        function load(x: any, state: any) { return x + state.flag + state.host; }
        function run(x: any, loader: any, state: any) { loader(x, state); return loader; }
        function main(x: any) { const st = mkState(); const f = run(x, load, st); console.log(f(x, 5)); }
        main(1); main(2);
        "#;
    let pf = units(src);
    assert!(!proven(&pf, "flag"), "{}", pf.dump_text());
    // Soundness: the callee passes it to unknown code (a ⊤ callee; a
    // modeled builtin such as forEach calls it with what it models).
    let src = r#"
        function mkState() { return { flag: 'a', host: 'b' }; }
        function load(x: any, state: any) { return x + state.flag + state.host; }
        function run(x: any, loader: any, state: any) { (globalThis as any).hook(loader); return loader(x, state); }
        function main(x: any) { const st = mkState(); console.log(run(x, load, st)); }
        main(1); main(2);
        "#;
    let pf = units(src);
    assert!(!proven(&pf, "flag"), "{}", pf.dump_text());
    // Soundness: a closed function its own unit passes to a closed callee
    // of another unit. Rewritten for decision 23 (which reverses 22a): it
    // is described there, so the callee's calls of it publish what they
    // pass and its parameter holds that too; `run` passes another literal,
    // so `.flag` is proven on no single layout. (A number, the old case,
    // is a primitive receiver: T0 judges object receivers only, as for the
    // undefined every closed parameter holds.)
    let src = r#"
        function mkState() { return { flag: 'a', host: 'b' }; }
        function run(x: any, loader: any, state: any) { return loader(x, { other: 5 }); }
        function load(x: any, state: any) { if (x > 5) return run(x - 1, load, state); return x + state.flag; }
        function main(x: any) { const st = mkState(); console.log(load(x, st)); }
        main(1); main(10);
        "#;
    let pf = units(src);
    assert!(!proven(&pf, "flag"), "{}", pf.dump_text());
}

#[test]
fn an_own_function_value_passed_to_a_closed_callee_is_described() {
    // Decision 23 (tssrc moduleNameResolver: tryResolve's own arrow
    // `const loader` passed to closed `tryLoadModuleUsingOptionalResolutionSettings`,
    // which another unit also calls with a sibling's function): the local
    // literal is closed too, so `run`'s parameter stays described for every
    // caller; `load` keeps its arguments and so does the local arrow.
    let src = r#"
        function mkState() { return { flag: 'a', host: 'b' }; }
        function load(x: any, state: any) { return x + state.flag; }
        function run(x: any, loader: any, state: any) { return loader(x, state); }
        function viaLocal(x: any) { const st = mkState(); const loader = (y: any, s: any) => y + s.host; return run(x, loader, st); }
        function main(x: any) { const st = mkState(); console.log(run(x, load, st)); console.log(viaLocal(x)); }
        main(1); main(2);
        "#;
    let pf = units(src);
    assert!(pf.units.len() >= 5, "{}", pf.dump_text());
    assert!(pf.broken.is_empty(), "{:?}", pf.broken);
    assert!(
        !verdicts(&pf, "flag").is_empty() && !verdicts(&pf, "host").is_empty(),
        "{}",
        pf.dump_text()
    );
    assert!(
        proven(&pf, "flag") && proven(&pf, "host"),
        "{}",
        pf.dump_text()
    );
    // A test must be able to fail: the proofs name the literal passed.
    assert!(
        pf.sites
            .iter()
            .filter(|s| s.key == "flag" || s.key == "host")
            .all(|s| !s.receivers.is_empty()),
        "{}",
        pf.dump_text()
    );
    // Soundness: the callee lets the local escape (unknown code may call
    // it with anything): broken in the census, its parameters ⊤.
    let src = r#"
        function mkState() { return { flag: 'a', host: 'b' }; }
        function run(x: any, loader: any, state: any) { (globalThis as any).l = loader; return loader(x, state); }
        function viaLocal(x: any) { const st = mkState(); const loader = (y: any, s: any) => y + s.host; return run(x, loader, st); }
        viaLocal(1); viaLocal(2);
        "#;
    let pf = units(src);
    assert!(
        !verdicts(&pf, "host").is_empty() && !proven(&pf, "host"),
        "{}",
        pf.dump_text()
    );
    // Soundness: its own function stores it (a reference that is no call
    // and no argument of a closed call): not closed, unknown as before.
    let src = r#"
        function mkState() { return { flag: 'a', host: 'b' }; }
        function run(x: any, loader: any, state: any) { return loader(x, state); }
        function viaLocal(x: any) { const st = mkState(); const loader = (y: any, s: any) => y + s.host; (globalThis as any).k = loader; return run(x, loader, st); }
        viaLocal(1); viaLocal(2);
        "#;
    let pf = units(src);
    assert!(
        !verdicts(&pf, "host").is_empty() && !proven(&pf, "host"),
        "{}",
        pf.dump_text()
    );
    // Two closed locals of one name are two functions (tssrc: three
    // `loader`s in moduleNameResolver.ts): the one a callee lets escape is
    // broken, the other keeps its arguments (each passes its own literal:
    // the escaping call lets its argument escape too).
    let src = r#"
        function mkA() { return { host: 'b', port: 1 }; }
        function mkB() { return { flag: 'a' }; }
        function run(x: any, loader: any, state: any) { return loader(x, state); }
        function leak(x: any, loader: any, state: any) { (globalThis as any).l = loader; return loader(x, state); }
        function a(x: any) { const st = mkA(); const loader = (y: any, s: any) => y + s.host; return run(x, loader, st); }
        function b(x: any) { const st = mkB(); const loader = (y: any, s: any) => y + s.flag; return leak(x, loader, st); }
        a(1); a(2); b(1); b(2);
        "#;
    let pf = units(src);
    assert!(
        !verdicts(&pf, "flag").is_empty() && !proven(&pf, "flag"),
        "{}",
        pf.dump_text()
    );
    assert!(
        !verdicts(&pf, "host").is_empty() && proven(&pf, "host"),
        "{}",
        pf.dump_text()
    );
    assert!(
        pf.sites
            .iter()
            .filter(|s| s.key == "host")
            .all(|s| !s.receivers.is_empty()),
        "{}",
        pf.dump_text()
    );
    // Soundness: the callee calls it with another literal for `s`.
    let src = r#"
        function mkState() { return { flag: 'a', host: 'b' }; }
        function run(x: any, loader: any, state: any) { loader(x, { other: 5 }); return loader(x, state); }
        function viaLocal(x: any) { const st = mkState(); const loader = (y: any, s: any) => y + s.host; return run(x, loader, st); }
        viaLocal(1); viaLocal(2);
        "#;
    let pf = units(src);
    assert!(
        !verdicts(&pf, "host").is_empty() && !proven(&pf, "host"),
        "{}",
        pf.dump_text()
    );
}

#[test]
fn a_unit_is_ordered_after_the_units_owning_the_names_it_imports() {
    // Decision 21. `a` and `b` import each other, but no name cycle exists:
    // `useD` (b) names `D`, `R` (a) names `useD`, `D` names nothing. By
    // whole modules every unit of `a` comes after every unit of `b` and back,
    // so the driver solved `useD` before `D` and `d.has` read no contract
    // (⊤). Ordered by the owners of the names, `D`'s contract exists first.
    let a = r#"
        import { useD } from './b';
        export class D { pos: Map<number, number>; constructor() { this.pos = new Map(); } has(k: number) { return this.pos.has(k); } }
        export class R { run() { return useD(); } }
        "#;
    let b =
        "import { D } from './a'; export function useD() { const d = new D(); return d.has(1); }";
    let pf = units_of(&[
        ("a", a),
        ("b", b),
        ("m", "import { R } from './a'; console.log(new R().run());"),
    ]);
    let mi = pf.module_paths.iter().position(|p| p == "b.ts").unwrap() as u32;
    let site = pf
        .sites
        .iter()
        .find(|s| s.module == mi && s.key == "has")
        .expect("the call in b");
    assert_eq!(site.verdict, crate::Verdict::T0Mono, "{}", pf.dump_text());
}

#[test]
fn a_closed_function_its_own_unit_lets_escape_has_top_parameters() {
    // Decision 22(b): a closed function's publication is no escape, any
    // other escape is real and gives ⊤ parameters. `run` and `load` call
    // each other (one unit, no census between them); `run` stores the
    // function value it is passed, so unknown code may call `load` with
    // anything: `state.flag` is not proven on `mkState`'s literal. (Its
    // arguments still name the literal, so the closed contract alone would
    // prove it: the escape must override it.)
    let src = r#"
        function mkState() { return { flag: 'a', host: 'b' }; }
        function run(x: any, loader: any, state: any) { (globalThis as any).l = loader; if (x < 0) return load(x, state); return loader(x, state); }
        function load(x: any, state: any) { if (x > 5) return run(x - 1, load, state); return x + state.flag; }
        function main(x: any) { const st = mkState(); console.log(load(x, st)); }
        main(1); main(10);
        "#;
    let pf = units(src);
    assert!(
        !verdicts(&pf, "flag").is_empty() && !proven(&pf, "flag"),
        "{}",
        pf.dump_text()
    );
    // The same without the store: proven, so the case above can fail.
    let pf = units(&src.replace("(globalThis as any).l = loader; ", ""));
    assert!(
        proven(&pf, "flag")
            && pf
                .sites
                .iter()
                .filter(|s| s.key == "flag")
                .all(|s| !s.receivers.is_empty()),
        "{}",
        pf.dump_text()
    );
}

#[test]
fn a_closed_function_without_a_summary_its_own_unit_lets_escape_has_top_parameters() {
    // Decision 22(b) on the call path (`Model::escape_fn_params`): the twin
    // of the test above for a closed function the solver does not summarize.
    // `load` calls a local function through `.call`, which no summary rule
    // covers (summary refused: "call … by no rule"), so unknown code calling
    // `load` reaches its plain parameters. `run` stores `load`: a real escape,
    // ⊤ parameters, `state.flag` not proven on `mkState`'s literal.
    let src = r#"
        function mkState() { return { flag: 'a', host: 'b' }; }
        function run(x: any, loader: any, state: any) { (globalThis as any).l = loader; if (x < 0) return load(x, state); return loader(x, state); }
        function load(x: any, state: any): any { if (x > 5) return run(x - 1, load, state); function z(this: any, y: any) { return this.host + y; } z.call(state, x); return x + state.flag; }
        function main(x: any) { const st = mkState(); console.log(load(x, st)); }
        main(1); main(10);
        "#;
    let pf = units(src);
    assert!(
        !verdicts(&pf, "flag").is_empty() && !proven(&pf, "flag"),
        "{}",
        pf.dump_text()
    );
    // The same without the store: proven, so the case above can fail.
    let pf = units(&src.replace("(globalThis as any).l = loader; ", ""));
    assert!(
        proven(&pf, "flag")
            && pf
                .sites
                .iter()
                .filter(|s| s.key == "flag")
                .all(|s| !s.receivers.is_empty()),
        "{}",
        pf.dump_text()
    );
}

#[test]
fn a_function_a_tdz_check_reads_stays_closed() {
    // effect Cron.ts: `parse` calls the module-scope arrow `parseSegment`
    // defined after it, so every call carries a TDZ check
    // (`js_tdz_check_binding(parseSegment, ..)`). The check only tests the
    // slot for the dead-zone hole and yields undefined: it is no reference
    // to the function value, so `parseSegment` is closed (decision 13) and
    // its parameter holds what its calls pass (`opts.max` on the literals).
    let src = r#"
        const secondOptions = { min: 0, max: 59 };
        const hourOptions = { min: 0, max: 23 };
        export const parse = (s: string) => {
          return [parseSegment(s, secondOptions), parseSegment(s, hourOptions)];
        };
        const parseSegment = (input: string, opts: any): number => {
          return opts.max - opts.min + input.length;
        };
        console.log(parse("a"));
        "#;
    let pf = units(src);
    assert!(
        proven(&pf, "max")
            && pf
                .sites
                .iter()
                .filter(|s| s.key == "max")
                .all(|s| !s.receivers.is_empty()),
        "{}",
        pf.dump_text()
    );
    // A real reference besides the calls lets it escape: not proven, so the
    // case above can fail.
    let pf = units(&src.replace(
        "console.log(parse(\"a\"));",
        "console.log(parse(\"a\")); (globalThis as any).f = parseSegment;",
    ));
    assert!(
        !verdicts(&pf, "max").is_empty() && !proven(&pf, "max"),
        "{}",
        pf.dump_text()
    );
}

#[test]
fn a_literal_passed_to_a_closed_function_carries_its_field_functions() {
    // tssrc program.ts: `createProgram` passes the literal `{ compilerHost:
    // host, getSymlinkCache, toPath, ... }` to the closed
    // `updateHostForUseSourceOfProjectReferenceRedirect`, which calls
    // `host.toPath(..)`. The argument contract (decision 13) describes the
    // literal's field functions as a value contract does (`note_field_fns`):
    // the callee calls them through their contract, the literal does not
    // escape there, so its layout is not broken and `host.base` holds.
    let src = r#"
        function update(host: any) { const k = host.base; return host.toPath(k) + host.base; }
        function main(x: any) { const h = { base: 'b', toPath: (s: string) => s + x }; console.log(update(h)); }
        main(1); main(2);
        "#;
    let pf = units(src);
    assert!(pf.units.len() >= 2, "{}", pf.dump_text());
    assert!(
        proven(&pf, "base")
            && pf
                .sites
                .iter()
                .filter(|s| s.key == "base")
                .all(|s| !s.receivers.is_empty()),
        "{}",
        pf.dump_text()
    );
    assert!(pf.broken.is_empty(), "{:?}", pf.broken);
    // The callee storing a computed key into the literal breaks its layout
    // (rewritten for design §16: a named new key is now the owner's shape
    // transition): `base` is not proven, so the case above can fail. (Principle: the census
    // decides; the old negative, `globalThis.f = h.toPath` in the caller,
    // no longer keeps a field function out of the contract, decision 29.)
    let pf = units(&src.replace("const k = host.base;", "const k = host.base; host[k] = 1;"));
    assert!(
        !verdicts(&pf, "base").is_empty() && !proven(&pf, "base"),
        "{}",
        pf.dump_text()
    );
    // Here `toPath` escaped in the caller before it became a contract
    // function (through the callee's previous contract: what its argument
    // reaches escapes). It still gets parameter markers then: the
    // `fn_contract_of` debug assertion checks every contract function's
    // parameters carry them (sabotage arm 76).
}

#[test]
fn parallel_levels_give_the_serial_facts() {
    // Decision 32: `z` and `b` import nothing (level 0), `c` and `e` import
    // only `z` (level 1), `m` imports `c` (level 2). A level is solved in parallel against one frozen
    // input: the facts are those of the serial solve, and `z`'s contract
    // still reaches its importers (the call in `c` is proven through it).
    let mods = [
        ("z", DUAL),
        ("b", "export const k = 2;"),
        ("c", "import { D } from './z'; const d = new D([3]); console.log(d.has(3)); export const n = 3;"),
        ("e", "import { D } from './z'; const d = new D([4]); console.log(d.has(4));"),
        ("m", "import { D } from './z'; import { k } from './b'; import { n } from './c'; console.log(new D([k, n]).has(5));"),
    ];
    let run = |jobs| {
        let pf = units_with(
            &mods,
            &FactsConfig {
                jobs,
                ..FactsConfig::default()
            },
        );
        let facts = format!(
            "{:?}\n{:?}\n{:?} {:?} {}",
            pf.templates, pf.sites, pf.broken, pf.round_work, pf.rounds
        );
        (facts, pf)
    };
    let (serial, _) = run(1);
    let (parallel, pf) = run(4);
    let ci = pf.module_paths.iter().position(|p| p == "c.ts").unwrap() as u32;
    let site = pf
        .sites
        .iter()
        .find(|s| s.module == ci && s.key == "has")
        .expect("the call in c");
    assert_eq!(site.verdict, Verdict::T0Mono, "{}", pf.dump_text());
    assert!(pf.broken.is_empty(), "{}", pf.dump_text());
    assert_eq!(serial, parallel);
}

#[test]
fn the_object_key_follows_what_an_importer_observes() {
    // Design §13.2: a module's object-cache key (`digest_for`) holds the
    // templates its sites observe, contracted imports included. A layout
    // change in `a` that `m` reads changes `m`'s key; an edit `m` does not
    // observe keeps it, so `m`'s object is reused.
    let user = "import { P } from './a'; const p = new P(); console.log(p.x);";
    let a = "export class P { x: number; constructor() { this.x = 1; } }\n";
    let grown =
        "export class P { x: number; y: number; constructor() { this.x = 1; this.y = 2; } }\n";
    let other = format!("{a}export function helper() {{ const o = {{ k: 1 }}; return o.k; }}\n");
    let base = units_of(&[("a", a), ("m", user)]);
    let grown = units_of(&[("a", grown), ("m", user)]);
    let other = units_of(&[("a", &other), ("m", user)]);
    assert_ne!(
        base.digest_for("a.ts"),
        other.digest_for("a.ts"),
        "{}",
        other.dump_text()
    );
    assert_ne!(
        base.digest_for("m.ts"),
        grown.digest_for("m.ts"),
        "{}",
        grown.dump_text()
    );
    assert_eq!(
        base.digest_for("m.ts"),
        other.digest_for("m.ts"),
        "{}",
        other.dump_text()
    );
}

/// The points-to entries of the largest unit: `n` exported functions that
/// share a helper storing one argument into the other.
fn marker_store_entries(n: usize) -> u64 {
    let mut src = String::from(
        "class S { static last: any = 0; }\nfunction h(a: any, b: any) { a.x = b; S.last = b; }\nfunction mid(a: any, b: any) { h(a, b); return a.x; }\n",
    );
    for i in 0..n {
        src.push_str(&format!(
            "export function f{i}(a: any, b: any) {{ S.last = a; return mid(a, b); }}\n"
        ));
    }
    // Another module calls every export: each is a contract (§11.1).
    let names: Vec<String> = (0..n).map(|i| format!("f{i}")).collect();
    let calls: Vec<String> = names
        .iter()
        .map(|f| format!("console.log({f}(1, 2));"))
        .collect();
    let user = format!(
        "import {{ {} }} from './t'; {}",
        names.join(", "),
        calls.join(" ")
    );
    let pf = units_of(&[("t", src.as_str()), ("u", user.as_str())]);
    pf.units
        .iter()
        .map(|(_, s)| s.pts_entries)
        .max()
        .unwrap_or(0)
}

#[test]
fn a_store_into_an_argument_keeps_no_field_of_the_marker() {
    // DECISIONS 41: a parameter marker is another unit's value; a store into
    // it is its contract's escape and keeps no field. With a field, the
    // shared helper `h` stores every caller's `b` marker into every
    // caller's `a` marker: the unit grows with the square of the callers.
    let (e8, e16, e32) = (
        marker_store_entries(8),
        marker_store_entries(16),
        marker_store_entries(32),
    );
    let (d1, d2) = (e16 - e8, e32 - e16);
    assert!(
        d2 < 3 * d1,
        "entries {e8} {e16} {e32}: quadratic in the callers"
    );
}

/// Pts entries of the largest unit when `n` exported functions pass their
/// argument through a wrapper into a shared helper that stores into it
/// (the contract's escape) and returns it; `S.last` (the class's state: a
/// store through a class name joins it) keeps them in one unit.
fn escaped_marker_entries(n: usize) -> u64 {
    let mut src = String::from(
        "class S { static last: any = 0; }\nfunction h(a: any) { a.k = 1; S.last = 0; return a; }\nfunction mid(a: any) { return h(a); }\n",
    );
    for i in 0..n {
        src.push_str(&format!(
            "export function f{i}(a: any) {{ S.last = {i}; return mid(a); }}\n"
        ));
    }
    let names: Vec<String> = (0..n).map(|i| format!("f{i}")).collect();
    let calls: Vec<String> = names
        .iter()
        .map(|f| format!("console.log({f}(1));"))
        .collect();
    let user = format!(
        "import {{ {} }} from './t'; {}",
        names.join(", "),
        calls.join(" ")
    );
    let pf = units_of(&[("t", src.as_str()), ("u", user.as_str())]);
    pf.units
        .iter()
        .map(|(_, s)| s.pts_entries)
        .max()
        .unwrap_or(0)
}

#[test]
fn an_escaped_marker_is_the_top_beside_it() {
    // DECISIONS 42: once a contract records an argument's escape, its
    // consumer sinks the argument and reads nothing else of it, so the
    // marker says no more than the ⊤ beside it. Delivered anyway, the
    // shared `h` returns every caller's marker to every caller (`mid` is
    // copied per caller, `h` is not): the unit grows with the square of
    // the callers.
    let (e8, e16, e32) = (
        escaped_marker_entries(8),
        escaped_marker_entries(16),
        escaped_marker_entries(32),
    );
    let (d1, d2) = (e16 - e8, e32 - e16);
    assert!(
        d2 < 3 * d1,
        "entries {e8} {e16} {e32}: quadratic in the callers"
    );
}

/// A module whose exported `make` returns, besides its own literal, what
/// `put`'s caller passed: `held` delivers `put`'s marker to `make`'s
/// result before `poke`'s store records the escape (a node's successors
/// are served before its rules). `S`'s statics (the class's state: a store
/// through a class name joins it) keep them in one unit.
const HELD: &str = r#"
    class S { static held: any = 0; static seen = 0; }
    const lit = { n: 1 };
    export function put(p: any) { S.seen = 1; S.held = p; }
    export function poke() { S.seen = 2; S.held.k = 1; }
    export function make(x: any) { S.seen = 3; if (x) { return S.held; } return lit; }
    export function rd() { return lit.n; }
    "#;

#[test]
fn an_escaped_marker_in_a_result_is_another_units_value() {
    // DECISIONS 42, the reader: `put`'s marker reads as ⊤ in `make`'s
    // result (another unit's value), so the result stays checked and
    // `lit` is given out by its layout, not escaped with the result.
    let m = "import { put, poke, make, rd } from './b'; put({ a: 1 }); poke(); const r = make(0); console.log(r.n, rd());";
    let pf = units_of(&[("b", HELD), ("m", m)]);
    let lit = pf
        .templates
        .iter()
        .find(|t| t.birth_keys == ["n"])
        .unwrap_or_else(|| panic!("lit in\n{}", pf.dump_text()));
    assert!(lit.escape.is_none(), "{}", pf.dump_text());
}

#[test]
fn an_escaped_marker_of_another_function_is_no_result_transfer() {
    // DECISIONS 42, the contract: `put`'s marker in `make`'s result is
    // ⊤, not `make`'s parameter 0 coming back. Read as `put`'s parameter
    // index, the caller's `o` would come back as `r` and the store would
    // reach `Box.v`.
    let m = "import { put, poke, make } from './b'; class Box { v: any = 1; } put({ a: 1 }); poke(); function main() { const o = new Box(); const r = make(o); r.v = 'x'; console.log(o.v); } main(); main();";
    let pf = units_of(&[("b", HELD), ("m", m)]);
    assert!(field(tmpl(&pf, "Box"), "v").readonly, "{}", pf.dump_text());
}

/// Pts entries of the largest unit when `n` exported functions pass their
/// argument through a wrapper into a shared helper that only reads it and
/// returns it (no escape: the markers do not read as ⊤).
fn shared_helper_entries(n: usize) -> u64 {
    let mut src = String::from(
        "class S { static last: any = 0; }\nfunction h(a: any) { S.last = a.v; return a; }\nfunction mid(a: any) { return h(a); }\n",
    );
    for i in 0..n {
        src.push_str(&format!(
            "export function f{i}(a: any) {{ S.last = {i}; return mid(a); }}\n"
        ));
    }
    let names: Vec<String> = (0..n).map(|i| format!("f{i}")).collect();
    let calls: Vec<String> = names
        .iter()
        .map(|f| format!("console.log({f}({{ v: 1 }}));"))
        .collect();
    let user = format!(
        "import {{ {} }} from './t'; {}",
        names.join(", "),
        calls.join(" ")
    );
    let pf = units_of(&[("t", src.as_str()), ("u", user.as_str())]);
    pf.units
        .iter()
        .map(|(_, s)| s.pts_entries)
        .max()
        .unwrap_or(0)
}

#[test]
fn a_shared_body_holds_one_marker_per_parameter() {
    // DECISIONS 44: a parameter node holds its argument marker in place of
    // the markers its callers pass. Without it the shared `h` returns every
    // caller's marker to every caller (`mid` is copied per caller, `h` is
    // not), and nothing escapes, so DECISIONS 42 does not apply: the unit
    // grows with the square of the callers.
    let (e8, e16, e32) = (
        shared_helper_entries(8),
        shared_helper_entries(16),
        shared_helper_entries(32),
    );
    let (d1, d2) = (e16 - e8, e32 - e16);
    assert!(
        d2 < 3 * d1,
        "entries {e8} {e16} {e32}: quadratic in the callers"
    );
}

#[test]
fn a_use_in_a_shared_body_is_a_use_of_each_callers_argument() {
    // DECISIONS 44: the argument marker's uses are its members' uses. `keep`
    // reaches `h` through `mid` (shared with `keep2`); `h` lets what its
    // argument reaches escape, so the caller's `o.inner` is not its own.
    let b = "function h(p: any) { (globalThis as any).k = p.inner; }\nfunction mid(p: any) { h(p); }\nexport function keep(p: any) { mid(p); }\nexport function keep2(p: any) { mid(p); }";
    let m = "import { keep, keep2 } from './b'; function main() { const o = { inner: { v: 1 } }; keep(o); keep2({ inner: 0 }); (globalThis as any).k.v = 'x'; console.log(o.inner.v); } main(); main();";
    let pf = units_of(&[("b", b), ("m", m)]);
    assert!(!proven(&pf, "v"), "{}", pf.dump_text());
}

#[test]
fn a_keyed_use_in_a_recursion_is_a_use_of_each_callers_argument() {
    // Design §18.4 with DECISIONS 44 when the shared bodies recurse: `h`
    // and `mid` pass their argument to each other, so their argument
    // markers stand for each other (one cycle of the member graph). `h`
    // lets `p.inner` escape; the keyed use reaches `keep`'s parameter
    // through the cycle (settled after the solve), so the caller's
    // `o.inner` is not its own.
    let b = "function h(p: any, n: number) { if (n > 0) mid(p, n - 1); (globalThis as any).k = p.inner; }\nfunction mid(p: any, n: number) { h(p, n); }\nexport function keep(p: any) { mid(p, 2); }\nexport function keep2(p: any) { mid(p, 2); }";
    let m = "import { keep, keep2 } from './b'; function main() { const o = { inner: { v: 1 } }; keep(o); keep2({ inner: 0 }); (globalThis as any).k.v = 'x'; console.log(o.inner.v); } main(); main();";
    let pf = units_of(&[("b", b), ("m", m)]);
    assert!(!proven(&pf, "v"), "{}", pf.dump_text());
}

#[test]
fn a_computed_key_use_in_a_shared_body_is_a_use_of_each_callers_argument() {
    // DECISIONS 44 under design §18.4: a named key reaches through its keyed
    // reach marker; a computed key through the marker's reach marker, which
    // an argument marker forwards to each marker it stands for. `h` lets
    // `p[k]` escape, so the caller's `o.inner` is not its own.
    let b = "function h(p: any, k: string) { (globalThis as any).k = p[k]; }\nfunction mid(p: any, k: string) { h(p, k); }\nexport function keep(p: any, k: string) { mid(p, k); }\nexport function keep2(p: any, k: string) { mid(p, k); }";
    let m = "import { keep, keep2 } from './b'; function main() { const o = { inner: { v: 1 } }; keep(o, 'inner'); keep2({ inner: 0 }, 'inner'); (globalThis as any).k.v = 'x'; console.log(o.inner.v); } main(); main();";
    let pf = units_of(&[("b", b), ("m", m)]);
    assert!(!proven(&pf, "v"), "{}", pf.dump_text());
}

#[test]
fn an_argument_a_shared_body_returns_comes_back_to_the_caller() {
    // DECISIONS 44: a result holding an argument marker holds the markers
    // it stands for. `id` returns its argument through the shared `h`, so
    // the caller's store through the result is a store into its own `Box`.
    let b = "function h(p: any) { return p; }\nfunction mid(p: any) { return h(p); }\nexport function id(p: any) { return mid(p); }\nexport function id2(p: any) { return mid(p); }";
    let m = "import { id, id2 } from './b'; class Box { v: any = 1; } const o = new Box(); const r = id(o); id2(0); r.v = 'x'; console.log(o.v);";
    let pf = units_of(&[("b", b), ("m", m)]);
    assert!(!field(tmpl(&pf, "Box"), "v").readonly, "{}", pf.dump_text());
}

/// Every function writes `S.last`, so they are one unit (the writers of a
/// binding join its declaration, decl_units.rs).
const JOIN: &str = "class S { static last: any = 0; }\n";

/// Pts entries of the largest unit when `n` exported functions each read
/// their own key of what a shared helper returns, and then any key of it.
fn relative_result_entries(n: usize) -> u64 {
    let mut src = format!(
        "{JOIN}function h(a: any) {{ S.last = 0; return a; }}\nfunction id(a: any) {{ S.last = 1; return h(a); }}\n"
    );
    for i in 0..n {
        src.push_str(&format!(
            "export function f{i}(a: any, k: string) {{ S.last = {i}; const r = id(a); const x = r.k{i}; return [x, r[k]]; }}\n"
        ));
    }
    let names: Vec<String> = (0..n).map(|i| format!("f{i}")).collect();
    let calls: Vec<String> = names
        .iter()
        .map(|f| format!("console.log({f}({{ v: 1 }}, 'v'));"))
        .collect();
    let user = format!(
        "import {{ {} }} from './t'; {}",
        names.join(", "),
        calls.join(" ")
    );
    let pf = units_of(&[("t", src.as_str()), ("u", user.as_str())]);
    pf.units
        .iter()
        .map(|(_, s)| s.pts_entries)
        .max()
        .unwrap_or(0)
}

#[test]
fn a_shared_body_returns_each_caller_its_own_argument() {
    // DECISIONS 44 (b): the shared `h` (and `id`) return their argument
    // marker; at each call's result it is that caller's own argument. Without
    // it every caller gets the shared marker back, reads its own key of it
    // (one keyed reach marker per caller on the shared marker) and then any
    // key of it (all of them): the unit grows with the square of the callers.
    let (e8, e16, e32) = (
        relative_result_entries(8),
        relative_result_entries(16),
        relative_result_entries(32),
    );
    let (d1, d2) = (e16 - e8, e32 - e16);
    assert!(
        d1 > 0 && d2 < 3 * d1,
        "entries {e8} {e16} {e32}: quadratic in the callers"
    );
}

/// One exported function `both` of `b.ts` (one unit: everything below is
/// its body), whose inner callers share one method instance (a method of a
/// local object: never composed into the callers' summaries), and `m`.
fn both(body: &str, m: &str) -> ProgramFacts {
    let b = format!("export function both(o: any, c: any) {{ {body} }}");
    units_of(&[("b", b.as_str()), ("m", m)])
}

#[test]
fn a_caller_gets_back_only_what_it_passed() {
    // DECISIONS 44 (b): `id` and `id2` return their argument through the
    // shared `H.h`. `id`'s result is `id`'s argument alone, so `both`
    // returns `o`, not `c`, and the store through it is a store into the
    // `Box`, not into the `Crate`.
    let pf = both(
        "const H = { h(p: any) { return p; } }; function id(p: any) { return H.h(p); } function id2(p: any) { return H.h(p); } id2(c); return id(o);",
        "import { both } from './b'; class Box { v: any = 1; } class Crate { v: any = 1; } const r = both(new Box(), new Crate()); r.v = 'x'; console.log(r.v);",
    );
    assert!(!field(tmpl(&pf, "Box"), "v").readonly, "{}", pf.dump_text());
    assert!(
        field(tmpl(&pf, "Crate"), "v").readonly,
        "{}",
        pf.dump_text()
    );
}

#[test]
fn an_argument_a_closure_keeps_is_every_callers() {
    // DECISIONS 44 (b): `H.h` keeps its first argument in a closure and
    // returns it on every later call, so `b`'s result is `a`'s argument.
    // The marker entered a closure cell (it left its run): its family comes
    // back as every caller's argument, and `both` may return `o`.
    let pf = both(
        "let saved: any = null; const H = { h(p: any) { if (!saved) saved = () => p; return saved(); } }; function a(p: any) { return H.h(p); } function b(p: any) { return H.h(p); } a(o); return b(c);",
        "import { both } from './b'; class Box { v: any = 1; } both(new Box(), 0).v = 'x'; console.log(1);",
    );
    assert!(!field(tmpl(&pf, "Box"), "v").readonly, "{}", pf.dump_text());
}

#[test]
fn an_argument_the_heap_keeps_is_every_callers() {
    // DECISIONS 44 (b): as above through an object field: the marker entered
    // the heap (it left its run).
    let pf = both(
        "const keep: any = { p: null }; const H = { h(p: any) { if (!keep.p) keep.p = p; return keep.p; } }; function a(p: any) { return H.h(p); } function b(p: any) { return H.h(p); } a(o); return b(c);",
        "import { both } from './b'; class Box { v: any = 1; } both(new Box(), 0).v = 'x'; console.log(1);",
    );
    assert!(!field(tmpl(&pf, "Box"), "v").readonly, "{}", pf.dump_text());
}

#[test]
fn a_recursion_returns_what_its_argument_reaches() {
    // DECISIONS 44 (b) through a recursion: `W.walk` returns its argument
    // or, through itself, what its argument reaches under `next`. At the
    // recursive call's result the keyed reach marker of `next` is the
    // caller's `n.next`, and a reach marker reaches itself under every key,
    // so the substitution ends. `both` may return the `Tail`.
    let pf = both(
        "const W = { walk(n: any): any { return n.next ? W.walk(n.next) : n; } }; function last(p: any) { return W.walk(p); } function last2(p: any) { return W.walk(p); } last2(c); return last(o);",
        "import { both } from './b'; class Head { v: any = 1; next: any = null; } class Tail { v: any = 1; } const h = new Head(); h.next = new Tail(); both(h, 0).v = 'x'; console.log(h.v);",
    );
    assert!(
        !field(tmpl(&pf, "Tail"), "v").readonly,
        "{}",
        pf.dump_text()
    );
    assert!(pf.units.iter().all(|(_, s)| !s.aborted), "{:?}", pf.units);
}

#[test]
fn a_written_parameter_is_not_the_callers_argument() {
    // DECISIONS 44 (b): `U.up` writes its parameter (`n = n.parent`), so its
    // marker stands for the caller's `n.parent` too and is never replaced
    // by the caller's argument alone: `both` may return the `Parent`.
    let pf = both(
        "const U = { up(n: any) { n = n.parent; return n; } }; function top(p: any) { return U.up(p); } function top2(p: any) { return U.up(p); } top2(c); return top(o);",
        "import { both } from './b'; class Parent { v: any = 1; } class Child { v: any = 1; parent: any = new Parent(); } both(new Child(), 0).v = 'x'; console.log(1);",
    );
    assert!(
        !field(tmpl(&pf, "Parent"), "v").readonly,
        "{}",
        pf.dump_text()
    );
}

#[test]
fn an_argument_marker_that_arrives_late_is_lifted() {
    // DECISIONS 44 (b): `b` passes `H.h` the result of `H.g`, whose marker
    // arrives after `H.h`'s result already holds its family (from `a`'s
    // call): the call's argument rule lifts it then. `both` returns `c`.
    let pf = both(
        "const H = { h(p: any) { return p; }, g(p: any) { return p; } }; function a(p: any) { return H.h(p); } function b(p: any) { return H.h(H.g(p)); } a(o); return b(c);",
        "import { both } from './b'; class Box { v: any = 1; } both(0, new Box()).v = 'x'; console.log(1);",
    );
    assert!(!field(tmpl(&pf, "Box"), "v").readonly, "{}", pf.dump_text());
}

#[test]
fn another_units_function_returns_each_caller_its_own_argument() {
    // DECISIONS 44 (b) through another unit's function: `pass` (unit `p`)
    // returns its argument and touches what it reaches under `k` (its
    // stand-in's touch node takes nothing back). Its stand-in is one body
    // for every call: its parameter's marker is the caller's argument at
    // each call's result, so `id`'s result is `id`'s argument alone.
    let p = "export function pass(x: any) { const s = '' + x.k; return x; }";
    let c = format!("{JOIN}import {{ pass }} from './p';\nfunction h(q: any) {{ S.last = 0; return pass(q); }}\nexport function id(q: any) {{ S.last = 1; return h(q); }}\nexport function id2(q: any) {{ S.last = 2; return h(q); }}");
    let m = "import { id, id2 } from './c'; class Box { v: any = 1; } class Crate { v: any = 1; } const o = new Box(); const c = new Crate(); const r = id(o); id2(c); r.v = 'x'; console.log(o.v, c.v);";
    let pf = units_of(&[("p", p), ("c", c.as_str()), ("m", m)]);
    assert!(!field(tmpl(&pf, "Box"), "v").readonly, "{}", pf.dump_text());
    assert!(
        field(tmpl(&pf, "Crate"), "v").readonly,
        "{}",
        pf.dump_text()
    );
}

#[test]
fn a_result_an_argument_reaches_is_the_callers_reach() {
    // An argument marker's keyed reach reads as what its members reach: `up`
    // (its own unit, a closed function) returns `x.parent`, which its
    // argument marker holds as a keyed reach marker of itself. Read as
    // nothing, `up`'s contract would say it returns its argument alone and
    // the store through `top`'s result would miss the `Parent`.
    let b = "function up(x: any) { x = x.parent; return x; }\nexport function top(x: any) { return up(x); }\nexport function top2(x: any) { return up(x); }";
    let m = "import { top, top2 } from './b'; class Parent { v: any = 1; } class Child { v: any = 1; parent: any = new Parent(); } const c = new Child(); top2(0); top(c).v = 'x'; console.log(c.v);";
    let pf = units_of(&[("b", b), ("m", m)]);
    assert!(
        !field(tmpl(&pf, "Parent"), "v").readonly,
        "{}",
        pf.dump_text()
    );
}

#[test]
fn an_aborted_unit_checks_no_contracts() {
    // 113-CWL3: past the bound a unit claims nothing (sound), so checking
    // its contracts only spends time: a walk over every argument marker's
    // members per result token. checker.ts#u18 spent 93 s there after its
    // 25 s solve hit the bound.
    let (b, m) = (
        "export function get(p: any) { const q = { v: p.k }; return q; }",
        "import { get } from './b'; console.log(get({ k: 1 }).v);",
    );
    let of = |pf: &ProgramFacts| {
        pf.units
            .iter()
            .find(|(p, _)| p.starts_with("b.ts"))
            .map(|(_, s)| (s.aborted, s.checks))
            .expect("the unit of get")
    };
    let free = of(&units_of(&[("b", b), ("m", m)]));
    assert!(
        !free.0 && free.1 >= 1,
        "unbounded, the unit checks its contracts: {free:?}"
    );
    let cfg = FactsConfig {
        max_work: Some(10),
        ..Default::default()
    };
    let bounded = of(&units_with(&[("b", b), ("m", m)], &cfg));
    assert!(bounded.0, "the bound held: {bounded:?}");
    assert_eq!(bounded.1, 0, "an aborted unit checked its contracts");
}

/// Pts entries of the largest unit when `n` other-unit functions let what
/// their argument reaches escape, each called with the head of one list of
/// `n` objects.
fn reach_closure_entries(n: usize) -> u64 {
    let mut b = String::new();
    for i in 0..n {
        b.push_str(&format!(
            "export function keep{i}(p: any) {{ (globalThis as any).k{i} = p.next; }}\n"
        ));
    }
    let names: Vec<String> = (0..n).map(|i| format!("keep{i}")).collect();
    let mut m = format!("import {{ {} }} from './b';\nfunction main() {{\n  const o0: any = {{ next: null, v: 0 }};\n", names.join(", "));
    for i in 1..=n {
        m.push_str(&format!(
            "  const o{i}: any = {{ next: o{}, v: {i} }};\n",
            i - 1
        ));
    }
    for f in &names {
        m.push_str(&format!("  {f}(o{n});\n"));
    }
    m.push_str(&format!("  console.log(o{n}.v);\n}}\nmain(); main();\n"));
    let pf = units_of(&[("b", b.as_str()), ("m", m.as_str())]);
    pf.units
        .iter()
        .map(|(_, s)| s.pts_entries)
        .max()
        .unwrap_or(0)
}

#[test]
fn what_other_units_let_escape_of_reached_values_is_one_closure() {
    // DECISIONS 45: an escape is transitive, so the unit's one escape node
    // takes the arguments' fields. A closure per contract parameter holds
    // the whole list once per callee: the unit grows with the square.
    let (e8, e16, e32) = (
        reach_closure_entries(8),
        reach_closure_entries(16),
        reach_closure_entries(32),
    );
    // Linear: doubling the callees again adds twice as much (a closure per
    // parameter: 258 418 738 became 376 784 1984, d2 = 2.9 d1).
    let (d1, d2) = (e16 - e8, e32 - e16);
    assert!(
        4 * d2 <= 9 * d1,
        "entries {e8} {e16} {e32}: quadratic in the callees"
    );
}

#[test]
fn another_units_touch_of_a_deep_reached_value_runs_its_protocol() {
    // DECISIONS 45: a touch is not transitive; the unit's touch node closes
    // over the fields of what it holds. `show` converts `p.a.b`, two fields
    // down: the literal `w`'s `valueOf` runs on `w` and writes `n`. (A class
    // method would not do: its own unit writes the field whether or not
    // anyone calls it.)
    let b = "export function show(p: any) { return '' + p.a.b; }";
    let m = "import { show } from './b'; function main() { const w = { n: 1, valueOf() { this.n = 'x'; return 0; } }; const o = { a: { b: w } }; show(o); console.log(w.n); } main(); main();";
    let pf = units_of(&[("b", b), ("m", m)]);
    let w = pf
        .templates
        .iter()
        .find(|t| t.birth_keys == ["n", "valueOf"])
        .expect("w literal");
    assert!(!field(w, "n").readonly, "{}", pf.dump_text());
}

/// ee-first (express, D16): `callback` reads `arguments` and is passed to
/// closed `listener` as `done`; the `onevent` it returns escapes through
/// `ee.on` and calls `done`.
const EE_FIRST: &str = r#"
    export default function first(stuff: any, done: any) {
        var cleanups: any[] = [];
        for (var i = 0; i < stuff.length; i++) {
            var arr = stuff[i];
            var ee = arr[0];
            for (var j = 1; j < arr.length; j++) {
                var event = arr[j];
                var fn = listener(event, callback);
                ee.on(event, fn);
                cleanups.push({ ee: ee, event: event, fn: fn });
            }
        }
        function callback() {
            cleanup();
            done.apply(null, arguments);
        }
        function cleanup() {
            var x;
            for (var i = 0; i < cleanups.length; i++) {
                x = cleanups[i];
                x.ee.removeListener(x.event, x.fn);
            }
        }
        function thunk(fn: any) {
            done = fn;
        }
        thunk.cancel = cleanup;
        return thunk;
    }
    function listener(event: any, done: any) {
        return function onevent(this: any, arg1: any) {
            var args = new Array(arguments.length);
            var ee = this;
            var err = event === "error" ? arg1 : null;
            for (var i = 0; i < args.length; i++) {
                args[i] = arguments[i];
            }
            done(err, ee, event, args);
        };
    }
"#;

/// Runs `f` on its own thread; `None` when it has not returned in `secs`.
fn bounded<T: Send + 'static>(secs: u64, f: impl FnOnce() -> T + Send + 'static) -> Option<T> {
    let (tx, rx) = std::sync::mpsc::channel();
    // The parser and lowering recurse: a test thread's stack, not 2 MiB.
    std::thread::Builder::new()
        .stack_size(64 << 20)
        .spawn(move || {
            let _ = tx.send(f());
        })
        .expect("spawns");
    rx.recv_timeout(std::time::Duration::from_secs(secs)).ok()
}

#[test]
fn an_argument_contract_ascends_so_the_units_reach_a_fixpoint() {
    // D16 (express, ee-first): `callback` reads `arguments` (its argument
    // contract is ⊤) and is passed to closed `listener` as `done`. With ⊤
    // from `first`, `listener.done` is unknown, so no call reaches
    // `callback` and its contract drops to none; then `callback` is closed,
    // its function contract is published, `listener.done` holds it, the
    // escaped `onevent` calls it: ⊤ again. Rebuilt from the latest solves
    // only, the contract cycled with period 3 and the driver never ended.
    // Joined with last round's (it only grows), the run ends: rounds 1-3
    // pass the contracts on, round 4 changes nothing. Bounded by the test,
    // so a driver that cycles fails here instead of hanging the suite.
    let m = r#"import first from "./ee";
        const t = first([[process, "exit"]], function (err: any) { console.log(err); });
        t.cancel();"#;
    let pf = bounded(60, move || units_of(&[("ee", EE_FIRST), ("m", m)]))
        .expect("D16: the per-unit fixpoint did not end in 60 s");
    assert_eq!(pf.rounds, 4, "rounds {:?}", pf.round_work);
}

#[test]
fn a_store_into_a_class_field_is_a_store_effect() {
    // CW2 C (binarytrees_arena per unit): `make` (its own unit) does
    // `a.size++` and the top level `s.size = 0` on an `Arena` of the class
    // unit. `size` is a definite data field of every Arena from its birth,
    // so the store adds no key and opens nothing: a store effect (decision
    // 17), as for a literal layout. The class is not broken, and `.left` /
    // `.right` hold on its layout in every unit.
    let src = r#"
        class Arena {
          left: Int32Array; right: Int32Array; size: number = 0;
          constructor(n: number) { this.left = new Int32Array(n); this.right = new Int32Array(n); }
        }
        function make(a: Arena, d: number): number {
          let l = -1; let r = -1;
          if (d > 0) { l = make(a, d - 1); r = make(a, d - 1); }
          const i = a.size++; a.left[i] = l; a.right[i] = r; return i;
        }
        function check(a: Arena, i: number): number {
          const l = a.left[i]; if (l < 0) return 1; return 1 + check(a, l) + check(a, a.right[i]);
        }
        const s = new Arena(64);
        const t = make(s, 3);
        console.log(check(s, t));
        s.size = 0;
        console.log(check(s, make(s, 2)));
        "#;
    let pf = units(src);
    assert!(pf.units.len() >= 3, "{}", pf.dump_text());
    assert!(pf.broken.is_empty(), "{:?}\n{}", pf.broken, pf.dump_text());
    for k in ["left", "right"] {
        assert!(
            !verdicts(&pf, k).is_empty() && proven(&pf, k),
            "{k}\n{}",
            pf.dump_text()
        );
    }
    // The owner performs the effect: `size` has writers after birth.
    assert!(
        !field(tmpl(&pf, "Arena"), "size").readonly,
        "{}",
        pf.dump_text()
    );
    assert!(
        field(tmpl(&pf, "Arena"), "left").readonly,
        "{}",
        pf.dump_text()
    );
    // Soundness: the owner reads what the other unit stored. `get` (a method
    // of the class unit) reads `this.v.key`; the constructor put `{ key: 1 }`
    // there, but `put` stored another layout: `.key` must not hold on it.
    let src = r#"
        class Box { v: any; constructor() { this.v = { key: 1 }; } get() { return this.v.key; } }
        function put(b: Box, x: any) { b.v = { other: x, key: 's' }; }
        function main(x: any) { const b = new Box(); put(b, x); console.log(b.get()); }
        main(1); main(2);
        "#;
    let pf = units(src);
    // Rewritten for NSS (a store effect carries the stored value's
    // contract): `Box` is not broken; `put`'s literal may be.
    assert!(
        !pf.broken.iter().any(|d| d == "t.ts:Box"),
        "{:?}",
        pf.broken
    );
    assert!(
        !verdicts(&pf, "key").is_empty() && !proven(&pf, "key"),
        "{}",
        pf.dump_text()
    );
    // Soundness: a store of a key the class does not hold from birth adds a
    // key (the layout changes): a census report, the class is broken.
    let src = r#"
        class C { x: number = 1; }
        function grow(c: C) { (c as any).y = 2; }
        function main() { const c = new C(); grow(c); console.log(c.x); }
        main(); main();
        "#;
    let pf = units(src);
    assert!(
        pf.broken.iter().any(|d| d.ends_with(":C")),
        "{:?}\n{}",
        pf.broken,
        pf.dump_text()
    );
}

#[test]
fn a_closed_class_constructor_holds_what_its_news_pass() {
    // Design §18.13 (binarytrees_arena per unit): `Arena` is no export, and
    // every naming of it is a `new`, in units other than its own. Its
    // constructor's `n` holds the numbers those `new`s pass, not ⊤ (which
    // might be a SharedArrayBuffer): `left`/`right` are typed arrays over
    // their own buffers, `ta_stable` as in the whole-program solve.
    let src = r#"
        class Arena {
          left: Int32Array; right: Int32Array; size: number = 0;
          constructor(n: number) { this.left = new Int32Array(n); this.right = new Int32Array(n); }
        }
        function make(a: Arena, d: number): number {
          let l = -1; let r = -1;
          if (d > 0) { l = make(a, d - 1); r = make(a, d - 1); }
          const i = a.size++; a.left[i] = l; a.right[i] = r; return i;
        }
        function check(a: Arena, i: number): number {
          const l = a.left[i]; if (l < 0) return 1; return 1 + check(a, l) + check(a, a.right[i]);
        }
        function fresh(k: number): Arena { return new Arena(1 << k); }
        const s = fresh(6);
        console.log(check(s, make(s, 3)));
        "#;
    let pf = units(src);
    assert!(pf.broken.is_empty(), "{:?}\n{}", pf.broken, pf.dump_text());
    for k in ["left", "right"] {
        assert!(
            field(tmpl(&pf, "Arena"), k).ta_stable,
            "{k}\n{}",
            pf.dump_text()
        );
    }
    // Soundness: another unit constructs it through an instance's
    // `constructor` with a shared buffer. The stand-in class object is the
    // instance's `constructor` there, so the construction reaches the
    // stand-in constructor's parameters and the owner sees the buffer.
    let src = r#"
        class Arena {
          left: Int32Array;
          constructor(n: any) { this.left = new Int32Array(n); }
        }
        function fresh(k: number): Arena { return new Arena(k); }
        function twin(a: Arena): number { const C: any = a.constructor; const t = new C(new SharedArrayBuffer(64)); return t.left[0]; }
        function main() { const s = fresh(8); console.log(s.left[0] + twin(s)); }
        main();
        "#;
    let pf = units(src);
    // Not vacuous: the class holds its contract (nothing reports it).
    assert!(pf.broken.is_empty(), "{:?}\n{}", pf.broken, pf.dump_text());
    assert!(
        !field(tmpl(&pf, "Arena"), "left").ta_stable,
        "{}",
        pf.dump_text()
    );
    // Across modules: `b` gives instances out (no export of the class); `m`
    // constructs one through `constructor` with a shared buffer. Today `m`'s
    // census already reports the class there (its stand-in escapes), so the
    // constructor's parameters are ⊤ twice over; `ctor_args_out` would carry
    // the buffer if it did not.
    let b = r#"
        class Arena {
          left: Int32Array;
          constructor(n: any) { this.left = new Int32Array(n); }
        }
        export function fresh(k: number): Arena { return new Arena(k); }
        export function first(a: Arena): number { return a.left[0]; }
        "#;
    let m = r#"
        import { fresh, first } from './b';
        function main() {
          const s = fresh(8);
          const C: any = s.constructor;
          const t = new C(new SharedArrayBuffer(64));
          console.log(first(s) + first(t));
        }
        main();
        "#;
    let pf = units_of(&[("b", b), ("m", m)]);
    assert!(
        !field(tmpl(&pf, "Arena"), "left").ta_stable,
        "{}",
        pf.dump_text()
    );
}

#[test]
fn a_store_effect_carries_the_kinds_it_stores() {
    // DECISIONS 73/74 (binarytrees_arena per unit): `make` does `a.size++`
    // on another unit's Arena. The store effect carries the stored kinds and
    // the reader reads the field's published kinds, so `size` stays a number
    // in its owner, as in the whole-program solve (not ⊤).
    let src = r#"
        class Arena {
          left: Int32Array; right: Int32Array; size: number = 0;
          constructor(n: number) { this.left = new Int32Array(n); this.right = new Int32Array(n); }
        }
        function make(a: Arena, d: number): number {
          let l = -1; let r = -1;
          if (d > 0) { l = make(a, d - 1); r = make(a, d - 1); }
          const i = a.size++; a.left[i] = l; a.right[i] = r; return i;
        }
        function check(a: Arena, i: number): number {
          const l = a.left[i]; if (l < 0) return 1; return 1 + check(a, l) + check(a, a.right[i]);
        }
        function fresh(k: number): Arena { return new Arena(1 << k); }
        const s = fresh(6);
        console.log(check(s, make(s, 3)));
        "#;
    let pf = units(src);
    assert!(pf.broken.is_empty(), "{:?}\n{}", pf.broken, pf.dump_text());
    let k = &field(tmpl(&pf, "Arena"), "size").kind;
    assert!(
        !k.top && k.objs.is_empty() && k.prims == crate::model::prim::NUM,
        "{k:?}\n{}",
        pf.dump_text()
    );
    // Soundness: another unit stores a string into it; the owner's field
    // holds strings too, and a reader adding to it may make strings.
    let src = r#"
        class Arena {
          size: number = 0;
          constructor() {}
        }
        function bump(a: Arena): void { (a as any).size = "x"; }
        function grow(a: Arena): number { const i = a.size++; return i; }
        function fresh(): Arena { return new Arena(); }
        const s = fresh();
        bump(s);
        console.log(grow(s));
        "#;
    let pf = units(src);
    assert!(pf.broken.is_empty(), "{:?}\n{}", pf.broken, pf.dump_text());
    let k = &field(tmpl(&pf, "Arena"), "size").kind;
    assert!(
        !k.top && k.prims & crate::model::prim::STR != 0,
        "{k:?}\n{}",
        pf.dump_text()
    );
    // An object stored by another unit widens the field as before.
    let src = r#"
        class Arena {
          size: number = 0;
          constructor() {}
        }
        function bump(a: Arena): void { (a as any).size = {}; }
        function fresh(): Arena { return new Arena(); }
        const s = fresh();
        bump(s);
        console.log(s.size);
        "#;
    let pf = units(src);
    let k = &field(tmpl(&pf, "Arena"), "size").kind;
    assert!(k.top || !k.objs.is_empty(), "{k:?}\n{}", pf.dump_text());
}

#[test]
fn an_escaped_arguments_keyed_reach_reads_as_unknown() {
    // DECISIONS 42 with design §18.4: `f` lets its argument escape, so what
    // it returns from `p.inner` may be anything unknown code stored there:
    // the caller's `r.v` has ⊤ among its receivers.
    // Two orders: the escape is seen before the read of `p.inner`, or (through
    // `keep`, another unit's function) after it.
    for b in [
        "export function f(p: any) { const x = p.inner; (globalThis as any).g = p; return x; }",
        "export function keep(q: any) { (globalThis as any).g = q; }\nexport function f(p: any) { const x = p.inner; keep(p); return x; }",
    ] {
        let m = "import { f } from './b'; function main() { const o = { inner: { v: 1 } }; const r = f(o); console.log(r.v); } main(); main();";
        let pf = units_of(&[("b", b), ("m", m)]);
        let s: Vec<_> = pf.sites.iter().filter(|s| s.key == "v").collect();
        assert!(!s.is_empty() && s.iter().all(|s| s.top), "{}", pf.dump_text());
    }
}

#[test]
fn an_escaped_keyed_reach_in_a_result_is_another_units_value() {
    // DECISIONS 42 with design §18.4: `put` keeps what its argument reaches
    // under `inner` in `held`, and that value escapes: (1) with the argument
    // itself, (2) under the key, through shared helpers (argument markers).
    // `make`'s result is then another unit's value (⊤), not `make`'s own
    // parameter 0's reach coming back.
    for b in [
        r#"
        let held: any = 0;
        export function put(p: any) { held = p.inner; (globalThis as any).g = p; }
        export function make(x: any) { if (x) { return held; } return 0; }
        "#,
        r#"
        let held: any = 0;
        function h(s: any) { (globalThis as any).g = s.inner; }
        function mid(q: any) { h(q); }
        export function put(p: any) { held = p.inner; mid(p); }
        export function put2(p: any) { mid(p); }
        export function make(x: any) { if (x) { return held; } return 0; }
        "#,
    ] {
        let m = "import { put, make } from './b'; put({ inner: { a: 1 } }); function main() { const o = { w: { q: 1 } }; const r = make(o); console.log(r.q); } main(); main();";
        let pf = units_of(&[("b", b), ("m", m)]);
        let s: Vec<_> = pf.sites.iter().filter(|s| s.key == "q").collect();
        assert!(
            !s.is_empty() && s.iter().all(|s| s.top),
            "{}",
            pf.dump_text()
        );
    }
}

#[test]
fn a_closed_parameter_binding_holds_its_contract_classes_only() {
    // DECISIONS 76 (binarytrees_arena per unit): `make`'s and `check`'s `a`
    // hold the contract's `Arena` plus the parameter markers beside it
    // (DECISIONS 74, 75). A marker beside its values adds no value of its
    // own, so the binding is not `other`: codegen gets an unchecked receiver
    // proof and reads `a.left` / `a.right` / `a.size` at their slots.
    let src = r#"
        class Arena {
          left: Int32Array; right: Int32Array; size: number = 0;
          constructor(n: number) { this.left = new Int32Array(n); this.right = new Int32Array(n); }
        }
        function make(a: Arena, d: number): number {
          let l = -1; let r = -1;
          if (d > 0) { l = make(a, d - 1); r = make(a, d - 1); }
          const i = a.size++; a.left[i] = l; a.right[i] = r; return i;
        }
        function check(a: Arena, i: number): number {
          const l = a.left[i]; if (l < 0) return 1; return 1 + check(a, l) + check(a, a.right[i]);
        }
        const s = new Arena(64);
        console.log(check(s, make(s, 3)));
        "#;
    let pf = units(src);
    assert!(pf.broken.is_empty(), "{:?}\n{}", pf.broken, pf.dump_text());
    let held: Vec<_> = pf.bindings[0]
        .locals
        .iter()
        .filter(|(_, b)| !b.class_ids.is_empty())
        .collect();
    // Not vacuous: `s` and both parameters hold the class.
    assert!(held.len() >= 3, "{held:?}");
    for (id, b) in held {
        assert!(!b.other, "local {id}: {b:?}\n{}", pf.dump_text());
        // DECISIONS 77: every call passes `a`, so it is never undefined
        // (no nullish receiver test, `a.size++` updates the slot).
        assert!(
            b.prims & crate::model::prim::UNDEF == 0,
            "local {id}: {b:?}\n{}",
            pf.dump_text()
        );
    }
    // Negative: another unit's call passes fewer arguments; the missing
    // parameter holds undefined (the solver's rule at the stand-in).
    let src = r#"
        class Arena { size: number = 0; }
        function f(a: Arena, d: number): number { return a.size + d; }
        const s = new Arena();
        console.log(f(s));
        "#;
    let pf = units(src);
    assert!(pf.broken.is_empty(), "{:?}\n{}", pf.broken, pf.dump_text());
    assert!(
        pf.bindings[0]
            .locals
            .iter()
            .any(|(_, b)| b.class_ids.is_empty() && b.prims & crate::model::prim::UNDEF != 0),
        "{:?}\n{}",
        pf.bindings[0].locals,
        pf.dump_text()
    );
}

#[test]
fn a_returned_literal_carries_its_field_functions() {
    // DECISIONS 78 (tssrc completions.ts:1376 `uniqueNames.has`, :1503
    // `printer.printAndFormatNode`): a contract function returns a literal of
    // its unit whose fields hold its functions. The result is the same layout
    // contract as a published literal (`note_field_fns`): the caller calls
    // `has` through its contract, so the literal does not escape as the
    // receiver of an unknown call and its layout is not broken.
    let src = r#"
        function mk(m: Set<string>) {
          return { has: (k: string) => m.has(k), add: (k: string) => { m.add(k); } };
        }
        function use(x: string) { const r = mk(new Set()); r.add(x); return r.has(x); }
        console.log(use("a"), use("b"));
        "#;
    let pf = units(src);
    assert!(pf.units.len() >= 2, "{}", pf.dump_text());
    assert!(pf.broken.is_empty(), "{:?}\n{}", pf.broken, pf.dump_text());
    assert!(
        proven(&pf, "has") && proven(&pf, "add"),
        "{}",
        pf.dump_text()
    );
    // A field function returning its own literal (a builder): the result
    // layout's field functions name the same layout again, and the reader
    // makes its stand-in once (tssrc overflowed the stack without that).
    let src = r#"
        function mk(m: Set<string>) {
          const o = { add: (k: string) => { m.add(k); return o; }, has: (k: string) => m.has(k) };
          return o;
        }
        function use(x: string) { return mk(new Set()).add(x).add("y").has(x); }
        console.log(use("a"), use("b"));
        "#;
    let pf = units(src);
    assert!(pf.units.len() >= 2, "{}", pf.dump_text());
    assert!(proven(&pf, "has"), "{}", pf.dump_text());
}

#[test]
fn a_forwarded_result_layout_leaves_no_fields() {
    // DECISIONS 79 (tssrc tsbuildPublic.ts: `createSolutionBuilder` returns
    // `createSolutionBuilderWorker(..)`, a literal whose closure returns the
    // `info` literal of a third unit). The forwarding unit's result is the
    // worker's layout, a stand-in forwarded with its owner's contract: its
    // fields do not leave there, so `info`'s layout is not broken and the
    // reader's `i.project` is proven.
    let src = r#"
        function next(n: number) { return { project: "p" + n, status: n }; }
        function worker(n: number) {
          return { getNext: (k: number) => next(n + k), close: () => n };
        }
        function create(n: number) { return worker(n); }
        function use(n: number) { const i = create(n).getNext(1); return i.project + i.status; }
        console.log(use(1), use(2));
        "#;
    let pf = units(src);
    assert!(pf.units.len() >= 4, "{}", pf.dump_text());
    assert!(pf.broken.is_empty(), "{:?}\n{}", pf.broken, pf.dump_text());
    assert!(
        proven(&pf, "project") && proven(&pf, "status"),
        "{}",
        pf.dump_text()
    );
}

#[test]
fn a_store_effect_ascends_so_the_units_reach_a_fixpoint() {
    // D16, the store channel (decision 17): `b` calls the function field
    // `k` of `a`'s literal and stores into the result, `obj` itself: a
    // store effect on `obj.k`. `a` performs it (an unknown value), so `k`
    // no longer holds only a function and `b` reads it as unknown: no
    // store. Rebuilt from the latest solves only, the effect would drop,
    // `k` would hold the function again, and the driver would cycle.
    // Joined with last round's, the effect stays: two rounds.
    let a = "function k() { return o; } const o = { k: k }; export const obj = o;";
    let b = "import { obj } from './a'; const f = (obj as any).k; const y = f(); y.k = 1;";
    let pf = bounded(60, move || units_of(&[("a", a), ("b", b)]))
        .expect("D16: the per-unit fixpoint did not end in 60 s");
    assert_eq!(pf.rounds, 2, "rounds {:?}", pf.round_work);
    // `a` keeps performing the store: `k` is written after construction.
    let o = pf
        .templates
        .iter()
        .find(|t| t.birth_keys == ["k"])
        .expect("the literal");
    assert!(!field(o, "k").readonly, "{}", pf.dump_text());
}

#[test]
fn a_member_function_returning_its_own_layout_is_built_once() {
    // The reader's stand-ins of a layout and of a function its field holds
    // that returns the layout reach each other: each is registered before
    // its members are built (the stack overflowed).
    let a = "const o: any = { k: function () { return o; } }; export function make() { return o; }";
    let b = "import { make } from './a'; const x = make(); const f = x.k; const y = f(); y.k = 1;";
    let pf = bounded(60, move || units_of(&[("a", a), ("b", b)]))
        .expect("D16: the per-unit fixpoint did not end in 60 s");
    assert_eq!(pf.rounds, 2, "rounds {:?}", pf.round_work);
}

#[test]
fn a_nested_initializer_and_its_hoisted_var_are_one_emitted_binding() {
    let src = r#"
        const loaded = (function() {
            try { var imported = { value: 123 }; } catch (unused) {}
            return { get: () => imported.value };
        })();
        console.log(loaded.get());
    "#;
    let pf = units(src);
    assert!(pf.broken.is_empty(), "{:?}", pf.broken);
    // The one binding is published once even though its hoisted seed and
    // nested initializer are distinct init statements after run-once flatten.
    let bindings = pf.bindings_for("t.ts").unwrap();
    let mut ids: Vec<_> = bindings.locals.iter().map(|(id, _)| *id).collect();
    let count = ids.len();
    assert!(count > 0, "the captured module binding must be live");
    ids.sort_unstable();
    ids.dedup();
    assert_eq!(ids.len(), count);
}

// MRK: a marker is a token only inside its run; it crosses into a result,
// storage or another run as a member of that node's own marker.

#[test]
fn an_inner_helper_given_two_objects_holds_no_marker() {
    // No marker arrives at `h`'s parameter (both callers pass objects of
    // their own), so it holds the two objects alone and `p.x` is proven on
    // both layouts: a marker there would be a receiver no fact covers.
    let b = "class A { x: any = 1; a: any = 0; }\nclass B { x: any = 2; b: any = 0; }\nexport function run() { function h(p: any) { return p.x; } return h(new A()) + h(new B()); }";
    let pf = units_of(&[
        ("b", b),
        ("m", "import { run } from './b'; console.log(run());"),
    ]);
    assert_eq!(verdicts(&pf, "x"), ["T0Set"], "{}", pf.dump_text());
}

#[test]
fn an_inner_helper_published_through_a_literal_keeps_its_marker() {
    // `h` is published as a field of `api`: its parameter's marker stays a
    // token in its run, so its contract still returns parameter 0 and the
    // caller's store through the result is a store into its own `Box`, not
    // into the `Crate` it never passed. (In a function: a module-scope value
    // is ⊤ to the reader's declaration unit, §12.3.)
    let b = "function h(p: any) { return p; }\nexport const api = { h };";
    let m = "import { api } from './b'; class Box { v: any = 1; } class Crate { v: any = 1; } function main() { const o = new Box(); const c = new Crate(); api.h(o).v = 'x'; console.log(o.v, c.v); } main(); main();";
    let pf = units_of(&[("b", b), ("m", m)]);
    assert!(!field(tmpl(&pf, "Box"), "v").readonly, "{}", pf.dump_text());
    assert!(
        field(tmpl(&pf, "Crate"), "v").readonly,
        "{}",
        pf.dump_text()
    );
}

#[test]
fn a_boundary_parameters_field_read_in_a_helper_is_proven() {
    // `join` is closed (decision 13): its callers' literal is beside its
    // parameter's marker. The inner `rd` reads `i.prev`: its parameter's
    // marker stands for `join`'s, all of whose members are closed, so the
    // site is proven on the callers' layout.
    let src = r#"
        function mkIndent() { return { prev: 'a', base: 'b' }; }
        function join(xs: any, indent: any) { function rd(i: any) { return i.prev; } return xs + rd(indent) + indent.base; }
        function main(x: any) { const ind = mkIndent(); console.log(join(x, ind)); }
        main(1); main(2);
        "#;
    let pf = units(src);
    assert!(!verdicts(&pf, "prev").is_empty(), "{}", pf.dump_text());
    assert!(
        proven(&pf, "prev") && proven(&pf, "base"),
        "{}",
        pf.dump_text()
    );
}

#[test]
fn a_helpers_result_never_holds_another_helpers_argument() {
    // `H.h1` and `H.h2` both store their argument into one cell, whose
    // marker stands for both; `h1` returns its own argument. The cell's
    // marker never enters `h1`'s run as `h2`'s family, so `a`'s result is
    // `a`'s argument alone: the store through `both`'s result reaches the
    // `Box`, not the `Crate`.
    let pf = both(
        "const keep: any = { p: null }; const H = { h1(p: any) { keep.p = p; return p; }, h2(p: any) { keep.p = p; return p; } }; function a(p: any) { return H.h1(p); } function b(p: any) { return H.h2(p); } b(c); return a(o);",
        "import { both } from './b'; class Box { v: any = 1; } class Crate { v: any = 1; } const r = both(new Box(), new Crate()); r.v = 'x'; console.log(r.v);",
    );
    assert!(!field(tmpl(&pf, "Box"), "v").readonly, "{}", pf.dump_text());
    assert!(
        field(tmpl(&pf, "Crate"), "v").readonly,
        "{}",
        pf.dump_text()
    );
}

#[test]
fn what_a_cell_holds_is_used_as_its_members() {
    // `f` stores its argument into a cell; `g` hands the cell's value to
    // unknown code. The cell holds its own marker with `f`'s argument as a
    // member, so the escape at `g` is an escape of `f`'s parameter, and the
    // caller's `o.v` is not proven. (One unit: `mk`'s body. With the cell a
    // declaration of its own, `f`'s store into another unit's literal is
    // bug 3, unsound on main.)
    let b = "export function mk() { const cell: any = { s: null }; return { f(x: any) { cell.s = x; }, g() { (globalThis as any).k = cell.s; } }; }";
    let m = "import { mk } from './b'; class Box { v: any = 1; } function main() { const api = mk(); const o = new Box(); api.f(o); api.g(); (globalThis as any).k.v = 'x'; console.log(o.v); } main(); main();";
    let pf = units_of(&[("b", b), ("m", m)]);
    assert!(!verdicts(&pf, "v").is_empty(), "{}", pf.dump_text());
    assert!(!proven(&pf, "v"), "{}", pf.dump_text());
}

#[test]
fn a_parameter_a_cell_gives_back_is_another_calls() {
    // `f` returns what an earlier call stored: another call's argument, not
    // this one's. `n` passes a `Crate` first, `m` a `Box` and stores through
    // the result. Read as "returns parameter 0" `m` would store into its own
    // `Box` while the result is `n`'s `Crate`, which would stay read-only
    // (unsound). The result's marker stands for another call's argument: the
    // result is another unit's value and the argument leaves through it. (The
    // cell is a static of `S`, so it is in `f`'s unit: bug 3 otherwise.)
    let b = "class S { static last: any = 0; static cell: any = { s: null }; }\nexport function f(x: any) { S.last = 1; const o = S.cell.s; S.cell.s = x; return o; }";
    let n = "import { f } from './b'; class Crate { w: any = 0; v: any = 2; } function main() { const c = new Crate(); f(c); console.log(c.v); } main(); main();";
    let m = "import { f } from './b'; class Box { v: any = 1; } function main() { const r = f(new Box()); r.v = 'x'; console.log(r.v); } main(); main();";
    let pf = units_of(&[("b", b), ("n", n), ("m", m)]);
    assert!(
        !field(tmpl(&pf, "Crate"), "v").readonly,
        "{}",
        pf.dump_text()
    );
}

#[test]
fn a_written_parameter_is_each_callers_own() {
    // `first` writes its parameter (`n = id(n)`): its binding holds the
    // marker, the variable what `id` returns of it, a token of its run. So
    // `top` returns its own argument: the store through its result is a
    // store into the caller's `Box`, not into the `Crate` passed to `top2`.
    let b = format!("{JOIN}function id(x: any) {{ S.last = 0; return x; }}\nfunction first(n: any) {{ S.last = 1; n = id(n); return n; }}\nexport function top(x: any) {{ S.last = 2; return first(x); }}\nexport function top2(x: any) {{ S.last = 3; return first(x); }}");
    let b = b.as_str();
    let m = "import { top, top2 } from './b'; class Box { v: any = 1; } class Crate { v: any = 2; } function main() { const o = new Box(); const c = new Crate(); top(o).v = 'x'; top2(c); console.log(o.v, c.v); } main(); main();";
    let pf = units_of(&[("b", b), ("m", m)]);
    assert!(!field(tmpl(&pf, "Box"), "v").readonly, "{}", pf.dump_text());
    assert!(
        field(tmpl(&pf, "Crate"), "v").readonly,
        "{}",
        pf.dump_text()
    );
}

/// Pts entries of the largest unit when `n` exported functions store their
/// argument into one cell through helpers of their own, and `n` readers
/// each read their own key of what the cell holds.
fn shared_cell_entries(n: usize) -> u64 {
    // The cell is a static of `S` and every function names `S`: one unit (a
    // cell of its own declaration unit would be another unit's literal).
    let mut src = "class S { static last: any = 0; static cell: any = { s: null }; }\n".to_string();
    for i in 0..n {
        src.push_str(&format!(
            "function h{i}(a: any) {{ S.last = {i}; S.cell.s = a; }}\nexport function f{i}(a: any) {{ S.last = {i}; h{i}(a); }}\nexport function g{i}() {{ S.last = {i}; const x = S.cell.s; return x.k{i}; }}\n"
        ));
    }
    let names: Vec<String> = (0..n)
        .flat_map(|i| [format!("f{i}"), format!("g{i}")])
        .collect();
    let calls: Vec<String> = (0..n)
        .map(|i| format!("f{i}({{ k{i}: 1 }}); console.log(g{i}());"))
        .collect();
    let user = format!(
        "import {{ {} }} from './t'; {}",
        names.join(", "),
        calls.join(" ")
    );
    let pf = units_of(&[("t", src.as_str()), ("u", user.as_str())]);
    pf.units
        .iter()
        .map(|(_, s)| s.pts_entries)
        .max()
        .unwrap_or(0)
}

#[test]
fn a_shared_cell_holds_one_marker() {
    // MRK: every helper's argument crosses into the cell as a member of its
    // one marker; each reader gets that marker and its own key's reach.
    // With the families themselves in the cell each reader would get every
    // helper's family and its key of each: the square of the helpers.
    let (e8, e16, e32) = (
        shared_cell_entries(8),
        shared_cell_entries(16),
        shared_cell_entries(32),
    );
    let (d1, d2) = (e16 - e8, e32 - e16);
    assert!(
        d1 > 0 && d2 < 3 * d1,
        "entries {e8} {e16} {e32}: quadratic in the helpers"
    );
}

#[test]
fn a_primitive_method_on_a_receiver_beside_top_is_that_top() {
    // `x` is another unit's value: ⊤ with every primitive kind beside it.
    // `x.trim()` is a call of an unknown function value (the arguments
    // escape, the result is ⊤); the primitive method's summary of the same
    // call says nothing more. Its container of primitives would stand
    // beside the literal in `f`'s result and, no literal shape, sink the
    // result: the literal escaped for no reason.
    let b = "export function f(x: any, c: boolean) { const r = x.trim(); const o = { v: 1 }; return c ? r : o; }";
    let m = "import { f } from './b'; console.log(f((globalThis as any).u, true).v);";
    let pf = units_of(&[("b", b), ("m", m)]);
    let lit = pf
        .templates
        .iter()
        .find(|t| t.display.starts_with("b.ts:__AnonShape"))
        .unwrap_or_else(|| panic!("{}", pf.dump_text()));
    assert!(lit.escape.is_none(), "{}", pf.dump_text());
}

#[test]
fn a_written_parameter_without_a_summary_is_each_callers_own() {
    // `up` reads `arguments`, so it has no summary: one body for every call,
    // a run of its own whose parameters get bindings the calls write. It
    // writes its parameter (`n = n.parent`): the variable holds the marker
    // and what it reaches under `parent`, two tokens of its run, so `top`'s
    // result is what the caller's argument reaches. Read as the marker alone
    // (the parameter node's marker standing for both) the result would be
    // the caller's `Child` and the store through it would miss the `Parent`.
    let b = format!("{JOIN}function up(n: any) {{ S.last = 0; if (arguments.length > 1) return null; n = n.parent; return n; }}\nexport function top(x: any) {{ S.last = 1; return up(x); }}\nexport function top2(x: any) {{ S.last = 2; return up(x); }}");
    let b = b.as_str();
    let m = "import { top, top2 } from './b'; class Parent { v: any = 1; } class Child { v: any = 1; parent: any = new Parent(); } class Crate { v: any = 2; parent: any = null; } function main() { const c = new Child(); top(c).v = 'x'; top2(new Crate()); console.log(c.v); } main(); main();";
    let pf = units_of(&[("b", b), ("m", m)]);
    assert!(
        !field(tmpl(&pf, "Parent"), "v").readonly,
        "{}",
        pf.dump_text()
    );
    assert!(
        field(tmpl(&pf, "Crate"), "v").readonly,
        "{}",
        pf.dump_text()
    );
}

/// Pts entries of the largest unit when `n` exported functions store their
/// argument into a cell each, one function returns any of the cells, and
/// `n` readers each read their own key of what it returns.
fn result_holder_entries(n: usize) -> u64 {
    // The cells are statics of `S` (one unit with the functions); `pick` is
    // a method of a local object, never composed into its callers, so its
    // result node is the one every caller reads.
    let cells: Vec<String> = (0..n)
        .map(|i| format!("static c{i}: any = {{ s: null }};"))
        .collect();
    let mut src = format!("class S {{ static last: any = 0; {} }}\n", cells.join(" "));
    for i in 0..n {
        src.push_str(&format!(
            "export function f{i}(a: any) {{ S.last = {i}; S.c{i}.s = a; }}\n"
        ));
    }
    // `pick` returns its parameter `d` when no cell matches: its result is
    // a slot of its instance's run (it depends on a parameter), which the
    // cells' markers cross into.
    src.push_str("const P = { pick(k: number, d: any): any { S.last = -1;\n");
    for i in 0..n {
        src.push_str(&format!("if (k === {i}) return S.c{i}.s;\n"));
    }
    src.push_str("return d; } };\n");
    for i in 0..n {
        src.push_str(&format!(
            "export function g{i}(k: number, a: any) {{ S.last = {i}; const x = P.pick(k, a); return x.k{i}; }}\n"
        ));
    }
    let names: Vec<String> = (0..n)
        .flat_map(|i| [format!("f{i}"), format!("g{i}")])
        .collect();
    let calls: Vec<String> = (0..n)
        .map(|i| format!("f{i}({{ k{i}: 1 }}); console.log(g{i}({i}, {{ k{i}: 2 }}));"))
        .collect();
    let user = format!(
        "import {{ {} }} from './t'; {}",
        names.join(", "),
        calls.join(" ")
    );
    let pf = units_of(&[("t", src.as_str()), ("u", user.as_str())]);
    pf.units
        .iter()
        .map(|(_, s)| s.pts_entries)
        .max()
        .unwrap_or(0)
}

#[test]
fn a_result_holds_one_marker() {
    // MRK: `pick` returns what any of `n` cells holds; its result holds one
    // marker standing for the cells' markers, and each reader gets that one.
    // With the cells' markers returned as themselves each reader would get
    // all `n` and its key of each: the square of the cells.
    let (e8, e16, e32) = (
        result_holder_entries(8),
        result_holder_entries(16),
        result_holder_entries(32),
    );
    let (d1, d2) = (e16 - e8, e32 - e16);
    assert!(
        d1 > 0 && d2 < 3 * d1,
        "entries {e8} {e16} {e32}: quadratic in the cells"
    );
}
