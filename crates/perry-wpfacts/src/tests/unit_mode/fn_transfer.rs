//! UEB: a function value passed to another unit's function is a relative
//! transfer, not an escape. The callee's contract describes what its body
//! does with the function (calls it, through a reach, a receiver, `.call`,
//! a third unit's function), relative to its parameters; the passing unit,
//! which holds the function, replays that on it (DECISIONS 140/144).
//! Only a use that hands the function to code no contract describes escapes.
//!
//! Every fixture: `a` passes its own `cb` to `b`'s function; `cb` reads `x.v`
//! of what it is called with (the `P` instance `a` also passes); `cb` is a
//! closure of `main`, so it is in the passing unit. Per unit
//! never proves a site the whole program does not.

use super::*;

const A_PLAIN: &str = r#"import { use } from "./b";
class P { v = 1; }
export function main() {
    function cb(x: any) { return x.v; }
    return use(cb, new P());
}
console.log(main());"#;

const A_LIT: &str = r#"import { use } from "./b";
class P { v = 1; }
export function main() {
    function cb(x: any) { return x.v; }
    return use({ cb, o: new P() });
}
console.log(main());"#;

/// The `x.v` sites of `a.ts`.
fn cb_proven(pf: &ProgramFacts) -> bool {
    let v: Vec<_> = pf
        .sites
        .iter()
        .filter(|s| s.key == "v" && pf.module_paths[s.module as usize] == "a.ts")
        .collect();
    assert!(!v.is_empty(), "no a.ts .v site\n{}", pf.dump_text());
    v.iter()
        .all(|s| matches!(s.verdict, Verdict::T0Mono | Verdict::T0Set))
}

/// Per unit never stronger than whole; whole proves `x.v` (`want_whole`).
fn never_stronger(mods: &[(&str, &str)], want_whole: bool) -> (ProgramFacts, ProgramFacts) {
    let (u, w) = (units_of(mods), whole_of(mods));
    let stronger: Vec<_> = proven_set(&u)
        .difference(&proven_set(&w))
        .cloned()
        .collect();
    assert!(
        stronger.is_empty(),
        "per unit stronger than whole: {stronger:?}\n{}",
        u.dump_text()
    );
    assert_eq!(cb_proven(&w), want_whole, "whole\n{}", w.dump_text());
    (u, w)
}

/// Per unit proves `x.v` as whole does: the transfer is relative.
fn transfers(mods: &[(&str, &str)]) {
    let (u, _) = never_stronger(mods, true);
    assert!(cb_proven(&u), "per unit\n{}", u.dump_text());
}

/// Per unit as whole on `x.v` (whatever whole says), never stronger.
fn same_as_whole(mods: &[(&str, &str)]) {
    let (u, w) = (units_of(mods), whole_of(mods));
    let stronger: Vec<_> = proven_set(&u)
        .difference(&proven_set(&w))
        .cloned()
        .collect();
    assert!(
        stronger.is_empty(),
        "per unit stronger than whole: {stronger:?}\n{}",
        u.dump_text()
    );
    assert_eq!(
        cb_proven(&u),
        cb_proven(&w),
        "per unit vs whole\n{}",
        u.dump_text()
    );
}

/// Open shape (UEB census: no mass on tssrc/fastify_inject): pinned at its
/// current per-unit verdict, never stronger than whole; a fix flips it.
fn gap(mods: &[(&str, &str)]) {
    let (u, _) = never_stronger(mods, true);
    assert!(
        !cb_proven(&u),
        "gap closed: make it a `transfers` test\n{}",
        u.dump_text()
    );
}

/// Per unit leaves `x.v` unproven, as whole does: unknown code calls `cb`.
fn escapes(mods: &[(&str, &str)]) {
    let (u, _) = never_stronger(mods, false);
    assert!(!cb_proven(&u), "per unit\n{}", u.dump_text());
}

// ------------------------------------------------------------ use kinds

/// Control (DECISIONS 140): `f(o)`; green on main.
#[test]
fn a_direct_call_of_the_parameter_runs_in_the_passing_unit() {
    let b = "export function use(f: any, o: any) { return f(o); }";
    transfers(&[("a", A_PLAIN), ("b", b)]);
}

/// Control (DECISIONS 144): `opts.cb(o)`; green on main.
#[test]
fn a_method_call_through_the_parameter_runs_in_the_passing_unit() {
    let b = "export function use(opts: any) { return opts.cb(opts.o); }";
    transfers(&[("a", A_LIT), ("b", b)]);
}

/// A call of what the parameter reaches, read out first: `const g = opts.cb; g(o)`.
#[test]
fn a_call_through_a_reach_of_the_parameter_is_an_open_gap() {
    let b = "export function use(opts: any) { const g = opts.cb; return g(opts.o); }";
    gap(&[("a", A_LIT), ("b", b)]);
}

/// Forwarded to a third unit's function, which calls it: the third unit's
/// contract hands the call back to `b` (DECISIONS 140), whose replay is the
/// stand-in's run, so `b` describes the argument relative to its own
/// parameter and `a` runs `cb` on its `P`. Red on main ("passed to a call
/// of an unknown function value": the replayed call's argument read as
/// another run's marker).
#[test]
fn a_function_forwarded_to_a_third_units_function_runs_in_the_passing_unit() {
    let b = r#"import { run } from "./c";
export function use(f: any, o: any) { return run(f, o); }"#;
    let c = "export function run(g: any, o: any) { return g(o); }";
    transfers(&[("a", A_PLAIN), ("b", b), ("c", c)]);
}

/// `f.call(thisArg, o)` and `f.apply(thisArg, [o])`: per unit as whole.
#[test]
fn a_call_or_apply_of_the_parameter_is_as_whole() {
    let b = "export function use(f: any, o: any) { return f.call(undefined, o); }";
    same_as_whole(&[("a", A_PLAIN), ("b", b)]);
    let b = "export function use(f: any, o: any) { return f.apply(undefined, [o]); }";
    same_as_whole(&[("a", A_PLAIN), ("b", b)]);
}

/// Stored into the callee's own object and called from there.
#[test]
fn a_function_stored_into_the_callees_object_is_an_open_gap() {
    let b = "export function use(f: any, o: any) { const h = { f }; return h.f(o); }";
    gap(&[("a", A_PLAIN), ("b", b)]);
}

/// Stored into an instance of the callee's class (a constructor parameter
/// kept as a field), then called through it.
#[test]
fn a_function_kept_by_the_callees_instance_is_an_open_gap() {
    let b = r#"class H { constructor(public f: any) {} go(o: any) { return this.f(o); } }
export function use(f: any, o: any) { return new H(f).go(o); }"#;
    gap(&[("a", A_PLAIN), ("b", b)]);
}

/// Put into an array (a literal element and a spread), called as an element.
#[test]
fn a_function_put_into_the_callees_array_is_an_open_gap() {
    let b = "export function use(f: any, o: any) { const fs = [f]; return fs[0](o); }";
    gap(&[("a", A_PLAIN), ("b", b)]);
}

// ------------------------------------------------------------ soundness

/// The callee keeps the function where its own later calls, from unknown
/// code, run it: `fire` escapes, so `cb` runs on unknown values.
#[test]
fn a_function_the_callee_keeps_for_an_unknown_caller_escapes() {
    let b = r#"let saved: any;
export function use(f: any, o: any) { saved = f; return f(o); }
export function fire(x: any) { return saved(x); }
(globalThis as any).fire = fire;"#;
    escapes(&[("a", A_PLAIN), ("b", b)]);
}

/// The callee stores the function where unknown code reads it.
#[test]
fn a_function_the_callee_hands_to_unknown_code_escapes() {
    let b = r#"export function use(f: any, o: any) { (globalThis as any).hook = f; return f(o); }"#;
    escapes(&[("a", A_PLAIN), ("b", b)]);
    let b = r#"export function use(f: any, o: any) { (globalThis as any).keep({ f }); return 0; }"#;
    escapes(&[("a", A_PLAIN), ("b", b)]);
}

/// Forwarded to a third unit that hands it to unknown code.
#[test]
fn a_function_forwarded_to_a_unit_that_lets_it_go_escapes() {
    let b = r#"import { run } from "./c";
export function use(f: any, o: any) { return run(f, o); }"#;
    let c =
        r#"export function run(g: any, o: any) { (globalThis as any).register(g); return g(o); }"#;
    escapes(&[("a", A_PLAIN), ("b", b), ("c", c)]);
}

/// Recursion between two units, each passing the function back.
#[test]
fn a_function_passed_back_and_forth_between_two_units_runs_in_the_passing_unit() {
    let a = r#"import { pb } from "./b";
class P { v = 1; }
export function main() {
    function cb(x: any) { return x.v; }
    return pb(cb, new P(), 3);
}
console.log(main());"#;
    let b = r#"import { pc } from "./c";
export function pb(f: any, o: any, n: number): any { return n > 0 ? pc(f, o, n - 1) : f(o); }"#;
    let c = r#"import { pb } from "./b";
export function pc(f: any, o: any, n: number): any { return pb(f, o, n); }"#;
    let (u, _) = never_stronger(&[("a", a), ("b", b), ("c", c)], true);
    let _ = u;
}

/// The recursion where one side lets the function go: unknown.
#[test]
fn a_function_passed_back_and_forth_and_let_go_escapes() {
    let a = r#"import { pb } from "./b";
class P { v = 1; }
export function main() {
    function cb(x: any) { return x.v; }
    return pb(cb, new P(), 3);
}
console.log(main());"#;
    let b = r#"import { pc } from "./c";
export function pb(f: any, o: any, n: number): any { return n > 0 ? pc(f, o, n - 1) : f(o); }"#;
    let c = r#"import { pb } from "./b";
export function pc(f: any, o: any, n: number): any { (globalThis as any).g = f; return pb(f, o, n); }"#;
    escapes(&[("a", a), ("b", b), ("c", c)]);
}

/// `arguments` and a rest parameter: the callee calls the function it
/// reads from them with an unknown value.
#[test]
fn a_function_read_from_arguments_or_rest_stays_sound() {
    let b =
        r#"export function use(f: any, o: any) { return arguments[0]((globalThis as any).u); }"#;
    escapes(&[("a", A_PLAIN), ("b", b)]);
    let b = r#"export function use(...fs: any[]) { return fs[0]((globalThis as any).u); }"#;
    escapes(&[("a", A_PLAIN), ("b", b)]);
    let b = r#"export function use(...fs: any[]) { return fs[0](fs[1]); }"#;
    same_as_whole(&[("a", A_PLAIN), ("b", b)]);
}

/// The passing unit itself lets the function go: unknown whatever the
/// callee does.
#[test]
fn a_function_its_own_unit_lets_go_escapes() {
    let a = r#"import { use } from "./b";
class P { v = 1; }
export function main() {
    function cb(x: any) { return x.v; }
    (globalThis as any).k = cb;
    return use(cb, new P());
}
console.log(main());"#;
    let b = "export function use(f: any, o: any) { const h = { f }; return h.f(o); }";
    escapes(&[("a", a), ("b", b)]);
}

/// A third unit over the work bound runs the forwarded function: it reports
/// what it consumed, so the function is unknown there; never stronger.
#[test]
fn a_function_forwarded_to_a_unit_over_the_bound_stays_sound() {
    let b = r#"import { run } from "./c";
export function use(f: any, o: any) { return run(f, o); }"#;
    let mut c = String::from("export function run(g: any, o: any) { let s = 0;\n");
    for i in 0..300 {
        c.push_str(&format!(
            "  const q{i} = {{ k{i}: {i}, f: g }}; s += q{i}.k{i};\n"
        ));
    }
    c.push_str("  return g(o) + s; }");
    let mods = [("a", A_PLAIN), ("b", b), ("c", c.as_str())];
    let hirs = hirs_of(&mods);
    let list: Vec<(String, &perry_hir::Module)> =
        hirs.iter().map(|(p, h)| (p.clone(), h)).collect();
    let free = crate::compute_units(&list, &FactsConfig::default());
    let wk = |p: &str| -> u64 {
        free.units
            .iter()
            .filter(|(q, _)| crate::decl_units::module_of(q) == p)
            .map(|(_, s)| s.work)
            .sum()
    };
    let bound = 2 * wk("a.ts").max(wk("b.ts"));
    assert!(wk("c.ts") > bound, "{:?}", free.units);
    let u = crate::compute_units(
        &list,
        &FactsConfig {
            max_work: Some(bound),
            ..FactsConfig::default()
        },
    );
    assert!(
        u.units
            .iter()
            .any(|(q, s)| crate::decl_units::module_of(q) == "c.ts" && s.aborted),
        "{:?}",
        u.units
    );
    let w = whole_of(&mods);
    let stronger: Vec<_> = proven_set(&u)
        .difference(&proven_set(&w))
        .cloned()
        .collect();
    assert!(
        stronger.is_empty(),
        "per unit stronger than whole: {stronger:?}\n{}",
        u.dump_text()
    );
    assert!(!cb_proven(&u), "per unit over the bound\n{}", u.dump_text());
}

/// Two functions of `b` forward to the same third-unit function, one of
/// them run by unknown code with unknown values: the stand-in's one body
/// joins both calls, never stronger than whole.
#[test]
fn two_forwarders_one_run_by_unknown_code_stay_sound() {
    let b = r#"import { run } from "./c";
export function use(f: any, o: any) { return run(f, o); }
export function use2(g: any, p: any) { return run(g, p); }
(globalThis as any).use2 = use2;"#;
    let c = "export function run(g: any, o: any) { return g(o); }";
    same_as_whole(&[("a", A_PLAIN), ("b", b), ("c", c)]);
}
