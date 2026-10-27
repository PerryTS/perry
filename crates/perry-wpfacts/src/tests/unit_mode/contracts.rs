use super::*;

#[test]
pub(super) fn exports_are_entries_of_other_units() {
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

#[test]
pub(super) fn private_layouts_stay_proven() {
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

#[test]
#[ignore = "DECISIONS 122: append is an exported function its importers only call; per unit its parameters are open contract markers (decision 13 covers module-private functions only)"]
pub(super) fn a_parameter_stored_into_another_parameters_reach_is_a_flow() {
    // AWFY Richards `RBObject.append(packet, queue)`: `m.link = p` stores
    // the first argument into what the second reaches. DECISIONS 118: a
    // flow of the contract the reader replays; DECISIONS 119: `m` is the
    // argument's marker alone (no ⊤ beside it to store `p` into).
    let b = "export function append(p: any, q: any) { p.link = null; if (q === null) return p; let m = q; while (m.link !== null) m = m.link; m.link = p; return q; }";
    let m = "import { append } from './b'; function main() { const a = { link: null as any, v: 1 }; const c = { link: null as any, v: 2 }; const q = append(a, c); console.log(q.v, a.v, c.link.v); } main(); main();";
    assert_parity(&[("b", b), ("m", m)]);
}

#[test]
pub(super) fn a_subclass_published_with_another_units_parent_forwards_it() {
    // DECISIONS 120 (B): publishing `S` publishes its parent `R`, a stand-in
    // in `b`: it leaves with `a`'s contract, no census report breaks `R`.
    let a = "export class R { v = 0; get() { return this.v; } }";
    let b = "import { R } from './a'; export class S extends R { constructor() { super(); this.v = 1; } }";
    let m = "import { S } from './b'; function main() { const s = new S(); console.log(s.get(), s.v); } main(); main();";
    assert_parity(&[("a", a), ("b", b), ("m", m)]);
}

#[test]
pub(super) fn an_export_no_unit_names_stays_inside() {
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
    // DECISIONS 157: an imported class takes its constructions from the
    // units holding it (its stand-in publishes what each `new` passes):
    // m passes a number, so the constructor's `this.x = r` stays proven
    // (it went generic while other units constructed it with ⊤ arguments).
    assert!(proven(&named, "x"), "{}", named.dump_text());
}

#[test]
pub(super) fn a_published_class_is_a_contract_not_an_escape() {
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
    // DECISIONS 107: `m` publishes what it runs `has` on (D's instances):
    // the receivers are the shapes the running units saw, so `this.neg` is
    // proven exactly, as in the whole program.
    assert_eq!(verdicts(&pf, "neg"), ["T0Mono"], "{}", pf.dump_text());
    assert!(tmpl(&pf, "D").prefix_stable, "{}", pf.dump_text());
}

#[test]
pub(super) fn an_importer_sees_a_class_through_its_contract() {
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
    // DECISIONS 107: `m` publishes the receivers it runs `has` on, and `z`
    // is solved again with them (the reader-to-owner channel, a round as
    // decision 13's arguments); nothing breaks. Rewritten for DECISIONS 125
    // (principle: a contract states what the body does to the receivers it
    // runs on): with them `this.neg` is the constructor's Map, `has` passes
    // `k` to `Map.has`, so its parameter contract changes in round 2 and `m`
    // reads it in round 3.
    assert_eq!((pf.rounds, pf.broken.len()), (3, 0));
}

#[test]
pub(super) fn a_store_beyond_the_contract_breaks_the_class() {
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
pub(super) fn a_broken_contract_resolves_only_the_units_that_see_it() {
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
    // Rewritten for DECISIONS 125 (principle: a break solves only the units
    // that see it): `b`/`c` take their own receiver rounds (DECISIONS 107),
    // so the run takes the rounds of the slower pair solved alone, no more.
    assert_eq!(pf.round_work.len(), alone_rounds(&[("a", DUAL), ("m", "import { D } from './a'; const d = new D([1]); (d as any).extra = 1; console.log(d.has(1));")]));
    // Every declaration unit of `a` and `m` sees D (§12.3: a module's work
    // is its units' work).
    let w = |p: &str| module_work(&pf, p);
    // `b` and `c` are solved again for their own reason, not D's: `c`
    // publishes the receivers it runs `E.has` on (DECISIONS 107), `b` takes
    // them (its contract gets exact), and `c` reads that contract. After the
    // break round 2 costs `a` and `m` once more and what `b` and `c` cost
    // in their round 2 solved alone. Rewritten for #13294 (decision 151:
    // every unit replays the shared objects' writes): `b`'s receiver round
    // is round 3 now, so round 2 is no longer every module's last solve
    // (was: round 2 = `a` + `m` + `b` + `c`).
    let e = DUAL.replace("class D", "class E").replace("neg", "other");
    let bc = units_of(&[
        ("b", &e),
        (
            "c",
            "import { E } from './b'; const e = new E([1]); console.log(e.has(1));",
        ),
    ]);
    // Round 3 solves `b` alone, which replays the other units' shared
    // writes too (decision 151), so it costs its own last solve, not quite
    // its cost alone.
    assert_eq!(
        pf.round_work[1],
        bc.round_work[1] + w("a.ts") + w("m.ts"),
        "{:?} {:?} {:?}",
        pf.round_work,
        bc.round_work,
        pf.units
    );
    assert_eq!(pf.round_work.len(), 3, "{:?}", pf.round_work);
    assert_eq!(
        pf.round_work[2],
        w("b.ts"),
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
pub(super) fn a_broken_namespace_field_function_resolves_only_the_units_that_see_it() {
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
    // Rewritten for DECISIONS 108 (principle: a function's name is its
    // identity): a nested function declaration carries its function id.
    assert!(
        pf.broken.iter().any(|d| d.starts_with("fn:a.ts:get#")),
        "{}",
        pf.dump_text()
    );
    // DECISIONS 125: as in `a_broken_contract_resolves_only_the_units_that_see_it`.
    assert_eq!(
        pf.round_work.len(),
        alone_rounds(&[("a", "export var util: any;\n(function (util: any) {\n  function get(x: any) { return x.k; }\n  util.get = get;\n})(util || (util = {}));\n"), ("m", "import { util } from './a'; (globalThis as any).g = util.get; console.log(util.get({ k: 1 }));")]),
        "{:?}",
        pf.round_work
    );
    let w = |p: &str| module_work(&pf, p);
    // Merged head (main's write effects and ResolveExport): round 2 solves
    // fewer units than `a`, `m`, `b` and `c` together; the rounds are pinned.
    // Rewritten for DECISIONS 196 (each reader an argument marker gains is
    // solve work): [699, 725, 908, 406, 334] -> [709, 731, 914, 410, 334];
    // for #13294 (decision 151: every unit replays the shared objects'
    // writes) -> [709, 779, 956, 443, 392]; for DECISIONS 209 (a broken
    // function read through a value contract is its stand-in, not ⊤: the
    // units of `a` holding `util.get`, #u1 145 -> 181 and #u3 37 -> 62,
    // solve its contract and its escaped object; `b`, `c`, `m` and round 2
    // unchanged) -> [709, 779, 956, 504, 428]; for DECISIONS 210 (a
    // published function's properties leave with it but its `prototype`,
    // whose template no longer escapes) -> the pin below.
    assert!(pf.round_work[1] < w("a.ts") + w("m.ts") + w("b.ts") + w("c.ts"));
    assert_eq!(pf.round_work, [695, 772, 949, 497, 428], "{:?}", pf.units);
}

#[test]
pub(super) fn a_unit_over_the_bound_is_not_solved_again() {
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
    // An aborted unit reports every contract it consumed (§11.3); `o` calls
    // `D.has`, a published method with its own contract (DECISIONS 57).
    // Rewritten for DECISIONS 108 (principle: a function's name is its
    // identity): a method is named by its class.
    assert_eq!(
        pf.broken,
        ["a.ts:D", "fn:a.ts:D::has"],
        "{}",
        pf.dump_text()
    );
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
pub(super) fn a_foreign_receiver_of_a_contract_method_escapes() {
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
pub(super) fn what_a_method_does_to_its_receiver_happens_to_the_instance() {
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
    // The unknown code is `ext`: printing the instance is no escape, in the
    // whole program as per unit (per-unit facts are whole-program facts,
    // owner 2026-10-23; the expectation was round-1 pessimism, DECISIONS 125).
    let b = "export class D { pos: number; constructor() { this.pos = 1; } me() { return this; } }";
    let mods = [
        ("a", b),
        (
            "m",
            "import { D } from './a'; declare const ext: any; ext(new D().me());",
        ),
    ];
    let pf = units_of(&mods);
    assert!(!tmpl(&pf, "D").prefix_stable, "{}", pf.dump_text());
    assert!(!tmpl(&whole_of(&mods), "D").prefix_stable);
}

#[test]
pub(super) fn a_function_contract_carries_its_result_and_borrowed_arguments() {
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
pub(super) fn what_an_argument_reaches_is_used_apart_from_the_argument() {
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
pub(super) fn an_argument_the_result_may_be_comes_back_to_the_caller() {
    // A result transfer (§11.2 "the return"): `id` returns its argument, so
    // the caller's store through the result is a store into its own `Box`.
    let b = "export function id(p: any) { return p; }";
    let m = "import { id } from './b'; class Box { v: any = 1; } const o = new Box(); const r = id(o); r.v = 'x'; console.log(o.v);";
    let pf = units_of(&[("b", b), ("m", m)]);
    assert!(!field(tmpl(&pf, "Box"), "v").readonly, "{}", pf.dump_text());
}

#[test]
pub(super) fn a_class_contract_carries_its_statics() {
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
pub(super) fn a_given_out_instance_receives_what_other_units_methods_do() {
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
    // `m` does call `set` with an unknown value on the instance `make`
    // gave it (without that call `keep.v` holds 1 and the whole program
    // proves `warm`; per-unit facts are whole-program facts, owner
    // 2026-10-23). `set` runs on published receivers (DECISIONS 107) and on
    // the instances given out (DECISIONS 125).
    let m = "import { C } from './t'; declare const ext: any; function main() { C.make().set(ext); const own = { n: 1, warm: 2 }; C.peek().k = own; console.log(own.warm); } main(); main();";
    let pf = units_of(&[("t", t), ("m", m)]);
    assert!(!proven(&pf, "warm"), "{}", pf.dump_text());
    assert!(!proven(&whole_of(&[("t", t), ("m", m)]), "warm"));
}

#[test]
pub(super) fn a_class_a_contract_returns_carries_its_methods() {
    // A contract result may be an instance of a published class (`mk`);
    // its reader sees the whole class contract, methods included, even
    // when it never imports the class: `put` is a method of `C` there.
    // `mk` and `C` share a declaration unit (both write `n`): `mk`'s
    // result is `C` itself, not another unit's stand-in.
    // DECISIONS 148 (rewritten expectation; was "the caller's literal
    // leaves"): `m` publishes what it passes to `put`, so `C`'s unit holds
    // the literal's stand-in, inside its contract (any other unit's use of
    // it is a census report), and per-unit facts are whole-program facts
    // (owner 2026-10-23): `own.warm` is proven in both.
    let t = "let n = 0; export class C { v: any = 1; constructor() { n++; } put(x: any) { this.v = x; } } export function mk() { n++; return new C(); }";
    let a = "import { C } from './t'; console.log(new C().v);";
    let m = "import { mk } from './t'; function main() { const own = { n: 1, warm: 2 }; mk().put(own); console.log(own.warm); } main(); main();";
    let mods = [("t", t), ("a", a), ("m", m)];
    let (pf, wp) = (units_of(&mods), whole_of(&mods));
    assert!(proven(&pf, "put"), "{}", pf.dump_text());
    assert_eq!(
        proven(&pf, "warm"),
        proven(&wp, "warm"),
        "{}",
        pf.dump_text()
    );
    assert!(pf.broken.is_empty(), "{}", pf.dump_text());
}

#[test]
pub(super) fn a_tdz_seed_writes_its_local_in_the_local_unit() {
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
pub(super) fn a_function_value_read_from_another_unit_is_a_function_contract() {
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
pub(super) fn a_store_into_another_units_prototype_breaks_the_class() {
    // Soundness: a store into a stand-in class's prototype (or the class
    // object) is beyond the contract, the census breaks the class (§11.3).
    let t = "export class C { v = 1; get() { return this.v; } }";
    let m = "import { C } from './t'; (C as any).prototype.get = function () { return 'x'; }; console.log(new C().get());";
    let pf = units_of(&[("t", t), ("m", m)]);
    assert_eq!(pf.broken, ["t.ts:C"], "{}", pf.dump_text());
}

#[test]
pub(super) fn a_unit_view_carries_only_its_own_table_entries() {
    // Each module-wide item table is split over the views (§12.6 step 1): an
    // exported object is in exactly one unit's table, its owner's.
    let src = "export const cfg = { a: 1 }; export function g() { return 2; } export function h() { return 3; }";
    let m = perry_parser::parse_typescript(src, "a.ts").expect("parses");
    let hir = perry_hir::lower_module(&m, "a", "a.ts").expect("lowers");
    assert_eq!(hir.exported_objects, ["cfg"]);
    let vs = crate::decl_units::views("a.ts", &hir, &|_| true);
    assert!(vs.len() >= 3, "{}", vs.len());
    assert_eq!(
        vs.iter()
            .filter(|(v, _)| !v.exported_objects.is_empty())
            .count(),
        1
    );
}

#[test]
pub(super) fn a_unit_view_is_indices_into_the_one_module() {
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
    let vs = crate::decl_units::views("a.ts", &hir, &|_| true);
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
pub(super) fn an_argument_the_callee_writes_is_written_by_the_reader() {
    // `bump` stores a number into what it is given. DECISIONS 118: that
    // store is a flow of its contract, and the reader runs it on its own
    // argument, as the whole program runs `bump`: the caller's literal does
    // not escape, so `own.warm` is proven exactly as the whole program
    // proves it (it was an escape before 118).
    let b = "export function bump(o: { n: number }) { o.n = o.n + 1; }";
    // In one function: a module-scope `own` would be ⊤ to the reader's
    // declaration unit (§12.3) and the test could not fail; called twice,
    // so N5 does not run it as module scope.
    let m = "import { bump } from './b'; function main() { const own = { n: 1, warm: 2 }; bump(own); console.log(own.warm); } main(); main();";
    let mods = [("b", b), ("m", m)];
    let (pf, wp) = (units_of(&mods), whole_of(&mods));
    assert!(proven(&wp, "warm"), "{}", wp.dump_text());
    assert!(proven(&pf, "warm"), "{}", pf.dump_text());
    // DECISIONS 176: a value of the callee's own unit stored into the
    // argument goes to the reader as a result does: the reader's field
    // holds the stand-in of its layout, so `own.n.k` is proven as the
    // whole program proves it (it was ⊤ before 176).
    let b = "export function bump(o: { n: any }) { o.n = { k: 1 }; }";
    let m = "import { bump } from './b'; function main() { const own = { n: 1 as any, warm: 2 }; bump(own); console.log(own.n.k); } main(); main();";
    let mods = [("b", b), ("m", m)];
    let (pf, wp) = (units_of(&mods), whole_of(&mods));
    assert!(proven(&wp, "k"), "{}", wp.dump_text());
    assert!(proven(&pf, "k"), "{}", pf.dump_text());
}

#[test]
pub(super) fn a_shared_class_id_is_stable_only_when_every_template_is() {
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
pub(super) fn a_result_that_is_not_a_literal_layout_escapes() {
    // `mk` returns an instance of `C`, a class of another declaration unit:
    // `mk`'s unit holds C's stand-in and forwards it with C's contract
    // (§17.10 4, DECISIONS 56; this expectation was "the result escapes"
    // before C's contract was forwarded). The principle the test keeps:
    // `m` runs `set` on it, so C's own unit models that: `x` is written
    // after construction and its layout does not change. DECISIONS 148
    // (rewritten expectation; was "⊤ arguments, `x` takes any kind"): the
    // units running `set` publish what they pass (`m`: a string), so `x`
    // takes number|string, as in the whole program; a caller passing a
    // value no contract describes leaves the parameter ⊤ (guard test
    // a_published_methods_parameter_some_caller_passes_unknown_stays_a_marker).
    let a = "class C { x: number; constructor() { this.x = 1; } set(v: any) { this.x = v; } } export function mk() { return new C(); }";
    let mods = [("a", a), ("m", "import { mk } from './a'; mk().set('s');")];
    let (pf, wp) = (units_of(&mods), whole_of(&mods));
    let x = field(tmpl(&pf, "C"), "x");
    assert!(!x.readonly && x.writers_post > 0, "{}", pf.dump_text());
    let xw = field(tmpl(&wp, "C"), "x");
    assert_eq!(
        format!("{:?}", x.kind),
        format!("{:?}", xw.kind),
        "{}",
        pf.dump_text()
    );
    assert!(!x.kind.fixed(), "{}", pf.dump_text());
}

#[test]
pub(super) fn a_module_value_read_from_another_unit_keeps_its_layout() {
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
pub(super) fn a_namespace_import_sees_only_the_real_exports() {
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
pub(super) fn a_once_invoked_wrapper_splits() {
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
            __perry_register_path_module_partial("t.cjs", {{}}, __perry_cjs_factory);
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
pub(super) fn a_bundle_splits_into_declaration_units() {
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
pub(super) fn a_store_through_a_class_name_joins_it_a_write_of_a_binding_does_not() {
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
pub(super) fn a_let_is_its_id_not_its_name() {
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
pub(super) fn a_literal_shape_is_copied_into_each_unit_that_births_it() {
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
pub(super) fn copies_of_one_emitted_closure_join_their_units() {
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
pub(super) fn a_literal_method_called_from_another_unit_keeps_its_target() {
    // A published literal's field holding only a function of its unit is a
    // function contract (§11.2 applied to the field, §12 value contracts):
    // the reader's call keeps its single target.
    let src = r#"
        const util = { f: (m: any) => m, g: (m: any) => m };
        function a(x: any) { return util.f(x); }
        function b(x: any) { return util.g(x); }
        console.log(a("s"), b(1));
        "#;
    let whole = facts(src);
    assert!(proven(&whole, "f"), "{}", whole.dump_text());
    let pf = units(src);
    assert!(pf.units.len() >= 3, "{}", pf.dump_text());
    assert!(proven(&pf, "f"), "{}", pf.dump_text());
    // A member stored after birth (TypeScript's namespace): the reader's
    // field holds the function, not ⊤, but the call is refused in both
    // modes, as its read is: no slot, `undefined` before the store.
    let src = r#"
        var eu: any;
        (function (eu: any) { eu.f = (m: any) => m; })(eu || (eu = {}));
        function a(x: any) { return eu.f(x); }
        console.log(a("s"));
        "#;
    let why = "key not a definite birth field";
    let whole = facts(src);
    assert!(refused(&whole, "f", why), "{}", whole.dump_text());
    let pf = units(src);
    assert!(pf.units.len() >= 3, "{}", pf.dump_text());
    assert!(holds_fn(&pf, "f"), "{}", pf.dump_text());
    assert!(refused(&pf, "f", why), "{}", pf.dump_text());
}

#[test]
pub(super) fn a_unit_publishes_only_its_own_exports() {
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
pub(super) fn a_result_holding_another_units_layout_forwards_its_contract() {
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
pub(super) fn a_closed_function_receives_what_its_callers_pass() {
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
pub(super) fn a_store_into_an_existing_field_is_a_store_effect() {
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
    assert!(pf.broken.is_empty(), "{:?}\n{}", pf.broken, pf.dump_text());
    assert!(
        !verdicts(&pf, "next").is_empty() && proven(&pf, "next"),
        "{}",
        pf.dump_text()
    );
    // The stored value does not escape (DECISIONS 88, design §17.16 (c),
    // (c')): the effect carries its value contract, and the head leaves its
    // unit with that contract under `next`; every reader's use is checked
    // against it (census). A contract describes what a field holds as it
    // describes what a binding holds.
    let node = pf
        .templates
        .iter()
        .find(|t| t.birth_keys == ["key"])
        .expect("node literal");
    assert!(node.escape.is_none(), "{}", pf.dump_text());
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
pub(super) fn a_key_another_unit_adds_to_a_literal_is_a_shape_transition() {
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
