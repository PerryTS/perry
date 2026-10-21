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
    let pf = units_of(&[
        ("a", "export var util: any;\n(function (util: any) {\n  function get(x: any) { return x.k; }\n  util.get = get;\n})(util || (util = {}));\n"),
        ("m", "import { util } from './a'; (globalThis as any).g = util.get; console.log(util.get({ k: 1 }));"),
        ("b", &DUAL.replace("class D", "class E").replace("neg", "other")),
        ("c", "import { E } from './b'; const e = new E([1]); console.log(e.has(1));"),
    ]);
    assert!(
        pf.broken.iter().any(|d| d == "fn:a.ts:get"),
        "{}",
        pf.dump_text()
    );
    assert_eq!(pf.round_work.len(), 2, "{:?}", pf.round_work);
    let w = |p: &str| module_work(&pf, p);
    assert_eq!(
        pf.round_work[1],
        w("a.ts") + w("m.ts"),
        "{:?} {:?}",
        pf.round_work,
        pf.units
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
    // A TDZ seed is a write of its locals (§12.3: a write joins the unit
    // of what it writes): `a`'s unit sees the seeded value, as the whole
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
    // `mk` returns an instance of a class with methods: not a literal
    // layout, so the result escapes as before (§11.2) and `m` may run
    // `set` on it with anything.
    let a = "class C { x: number; constructor() { this.x = 1; } set(v: any) { this.x = v; } } export function mk() { return new C(); }";
    let pf = units_of(&[("a", a), ("m", "import { mk } from './a'; mk().set('s');")]);
    assert!(!tmpl(&pf, "C").prefix_stable, "{}", pf.dump_text());
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
    // Soundness: a reader that stores through an alias of the binding (not
    // a join, §12.1) uses it beyond the contract: the census breaks the
    // layout and no site on it stays proven.
    let src = r#"
        const cfg = { a: 1, b: 2 };
        function useA() { return cfg.a; }
        function grow() { const o: any = cfg; o.c = 3; }
        grow();
        console.log(useA());
        "#;
    let pf = units(src);
    assert!(!proven(&pf, "a"), "{}", pf.dump_text());
    assert_eq!(pf.broken.len(), 1, "{:?}", pf.broken);
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
    // Soundness: a real export stays on the namespace; a store through it
    // breaks the layout.
    let t = r#"
        export const cfg = { a: 1, b: 2 };
        export function useA() { return cfg.a; }
        "#;
    let u = "import * as m from './t'; (m.cfg as any).a = 'x'; console.log(m.useA());";
    let pf = units_of(&[("t", t), ("u", u)]);
    assert!(!proven(&pf, "a"), "{}", pf.dump_text());
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
fn a_store_through_a_declaration_joins_its_unit() {
    // A write is shared state (§12.1): a store through a declaration's name
    // and the declaration are one unit, and two functions writing one `let`
    // are one unit with it, both ways.
    let n = |src: &str| units(src).units.len();
    assert_eq!(n("class C { v = 1; } C.create = () => new C();"), 1);
    assert_eq!(
        n("let k = 0; function inc() { k++; } function dec() { k--; }"),
        1
    );
    // A read alone is one way: the reader is a unit of its own.
    assert_eq!(n("const k = 0; function get() { return k; }"), 2);
    // Also a closure reading a binding written elsewhere (a captured local
    // that needs a box is no write of the closure); one writing it joins.
    // In a once-run wrapper (N5), as a CommonJS body: there `D` is a
    // captured local of the wrapper, not a module-scope name.
    let wrapped = |body: &str| format!("(function () {{ {body} }})();");
    let reads = n(&wrapped(
        "var D: any; D = D || {}; const f = () => D.a; const g = () => D.b; console.log(f(), g());",
    ));
    let writes = n(&wrapped("var D: any; D = D || {}; const f = () => { D = 1; }; const g = () => D.b; console.log(f(), g());"));
    assert_eq!(writes + 1, reads, "reads {reads} writes {writes}");
}

#[test]
fn a_let_is_its_id_not_its_name() {
    // A binding is its declaration, not its spelling (§12.1): two bodies a
    // once-run wrapper brings to module scope (N5) each declare their own
    // `T`, two bindings of one name, two units; a `var` declared twice is
    // one binding, and a write through either statement joins both.
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
fn an_inlined_copy_does_not_join_its_units() {
    // An inner local is its declaring item's own (§12.1): a body copied into
    // another item (inlining keeps the HIR ids) declares the same locals
    // again, a second binding, not shared state, so the two items stay
    // apart. A function reading a local it does not declare (a capture of
    // the enclosing function's local) shares it: one unit.
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
    assert_eq!(crate::decl_units::decl_units(&hir).len(), base);
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
    // Soundness: the caller storing into what it got breaks both layouts.
    let src = r#"
        const defaults = { zz: 1, yy: 2 };
        function norm(o: any) { if (!o) { return defaults; } return { zz: o.k, yy: 0 }; }
        function main(o: any) { const x: any = norm(o); x.q = 1; console.log(x.zz); }
        main(undefined); main({ k: 3 });
        "#;
    let pf = units(src);
    assert!(!proven(&pf, "zz"), "{}", pf.dump_text());
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
    // Soundness: the callee storing into what it got is beyond the
    // argument's contract: the census breaks the caller's layout.
    let src = r#"
        function mkIndent() { return { prev: 'a', base: 'b' }; }
        function join(xs: any, indent: any) { indent.q = 1; return xs + indent.prev; }
        function main(x: any) { const ind = mkIndent(); console.log(join(x, ind)); }
        main(1); main(2);
        "#;
    let pf = units(src);
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
    assert!(pf.broken.is_empty(), "{:?}", pf.broken);
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
    // A new key opens the layout: still a census report (§5).
    let src = r#"
        function mkHead() { return { next: undefined as any }; }
        function setNode(list: any, k: any) { list.extra = k; }
        function main(x: any) { const h = mkHead(); setNode(h, x); console.log(h.next); }
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
    // The callee adding a key to the literal breaks its layout: `base` is
    // not proven, so the case above can fail. (Principle: the census
    // decides; the old negative, `globalThis.f = h.toPath` in the caller,
    // no longer keeps a field function out of the contract, decision 29.)
    let pf = units(&src.replace(
        "const k = host.base;",
        "const k = host.base; host.extra = 1;",
    ));
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
