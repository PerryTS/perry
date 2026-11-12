//! `family_reach`: one test per rule (L0 to L4), each with its negative
//! twin, and the invariants of the realm index the rules read.
use super::*;
use crate::family_reach::FamilyReach;
use perry_runtime_manifest::families;

fn facts_cfg(src: &str, cfg: &FactsConfig) -> ProgramFacts {
    let path = "main.ts".to_string();
    let module = perry_parser::parse_typescript(src, &path).expect("parses");
    let hir = perry_hir::lower_module(&module, "main", &path).expect("lowers");
    let list: Vec<(String, &perry_hir::Module)> = vec![(path, &hir)];
    crate::compute(&list, cfg)
}

fn reach(src: &str) -> FamilyReach {
    let mut cfg = FactsConfig::default();
    cfg.max_work.get_or_insert(50_000_000);
    facts_cfg(src, &cfg)
        .family_reach
        .expect("the fact is computed for a whole program")
}

fn reason(r: &FamilyReach, family: &str) -> String {
    r.families
        .iter()
        .find(|f| f.family == family)
        .unwrap_or_else(|| panic!("no family {family}"))
        .reason
        .clone()
}

fn live(r: &FamilyReach) -> Vec<&'static str> {
    r.families
        .iter()
        .filter(|f| f.live)
        .map(|f| f.family)
        .collect()
}

/// A closed program that touches no family: classes, arrays, a loop,
/// `console.log`, one member of `process`.
const PLAIN: &str = "class P { x: number; constructor(x: number) { this.x = x; } }\n\
     const a: P[] = [];\n\
     for (let i = 0; i < 10; i++) a.push(new P(i));\n\
     let s = 0;\n\
     for (const p of a) s += p.x;\n\
     console.log('plain ' + s + ' ' + process.argv.length);\n";

#[test]
fn a_closed_program_that_names_no_family_has_every_family_never() {
    let r = reach(PLAIN);
    assert_eq!(live(&r), Vec::<&str>::new(), "{:?}", r.dump_lines());
    assert_eq!(r.never(), families::all_families());
    assert_eq!(r.families.len(), families::RUNTIME_FAMILIES.len());
    assert!(!r.live("typedarray"));
    // A family this fact does not list is not claimed.
    assert!(r.live("no_such_family"));
}

#[test]
fn the_dump_has_one_line_per_family() {
    let mut cfg = FactsConfig::default();
    cfg.max_work.get_or_insert(50_000_000);
    let text = facts_cfg(PLAIN, &cfg).dump_text();
    let lines: Vec<&str> = text
        .lines()
        .filter(|l| l.starts_with("# family_reach "))
        .collect();
    assert_eq!(lines.len(), families::RUNTIME_FAMILIES.len());
    assert!(lines.contains(&"# family_reach typedarray never reason=none"));
}

#[test]
fn l0_an_incomplete_solve_has_every_family_live() {
    // Past the work bound the solve is widened or aborted.
    let mut cfg = FactsConfig::default();
    cfg.max_work = Some(1);
    let r = facts_cfg(PLAIN, &cfg).family_reach.expect("computed");
    assert_eq!(r.never(), 0);
    assert!(
        reason(&r, "typedarray").starts_with("L0:"),
        "{}",
        reason(&r, "typedarray")
    );
    // A per-unit run publishes no fact: every family is live to a reader.
    let path = "main.ts".to_string();
    let module = perry_parser::parse_typescript(PLAIN, &path).expect("parses");
    let hir = perry_hir::lower_module(&module, "main", &path).expect("lowers");
    let list: Vec<(String, &perry_hir::Module)> = vec![(path, &hir)];
    let mut cfg = FactsConfig::default();
    cfg.max_work.get_or_insert(50_000_000);
    assert!(crate::compute_units(&list, &cfg).family_reach.is_none());
}

#[test]
fn l1_an_unknown_value_makes_every_family_live() {
    // A member of `process` the realm does not declare: an unknown value.
    let r = reach("const h = (process as any).noSuchMember;\nconsole.log(typeof h);\n");
    assert_eq!(r.never(), 0, "{:?}", r.dump_lines());
    assert!(
        reason(&r, "regex_engine").starts_with("L1:"),
        "{}",
        reason(&r, "regex_engine")
    );
    // Twin: a global nothing declares is `undefined`, a known value.
    let r = reach("const h = (globalThis as any).someHostThing;\nconsole.log(typeof h);\n");
    assert!(!r.live("typedarray"), "{:?}", r.dump_lines());
    assert!(!r.live("regex_engine"), "{:?}", r.dump_lines());
}

#[test]
fn l1_the_global_object_under_an_unknown_key_makes_every_family_live() {
    let r = reach(
        "const k = process.argv[2] ?? 'Uint8Array';\n\
         const C = (globalThis as any)[k];\nconsole.log(typeof C);\n",
    );
    assert_eq!(r.never(), 0, "{:?}", r.dump_lines());
    // Twin: a constant computed key is a static key, and names one global.
    let r = reach("const C = (globalThis as any)['Uint8' + 'Array'];\nconsole.log(typeof C);\n");
    assert!(r.live("typedarray"));
    assert!(!r.live("temporal"), "{:?}", r.dump_lines());
    assert!(!r.live("regex_engine"), "{:?}", r.dump_lines());
}

#[test]
fn listing_the_global_objects_keys_observes_no_family() {
    let r = reach("const ks = Object.keys(globalThis);\nconsole.log(ks.length >= 0);\n");
    assert!(!r.live("typedarray"), "{:?}", r.dump_lines());
}

#[test]
fn l2_a_birth_the_solve_resolves_is_live_by_the_solve() {
    let r = reach(
        "let n = 0;\n[4].forEach((x: number) => { n = new Uint8Array(x).length; });\nconsole.log(n);\n",
    );
    assert!(r.live("typedarray"));
    assert!(
        reason(&r, "typedarray").starts_with("L2:"),
        "{}",
        reason(&r, "typedarray")
    );
    // Twin: the same shape with an Array.
    let r = reach(
        "let n = 0;\n[4].forEach((x: number) => { n = new Array(x).length; });\nconsole.log(n);\n",
    );
    assert!(!r.live("typedarray"), "{:?}", r.dump_lines());
}

#[test]
fn l2_is_at_member_granularity_on_a_core_holder() {
    // `process.argv` is not `process`: the holder's declared births (a
    // Buffer from `process.stdin`) do not follow from one member.
    let r = reach("console.log(process.argv.length);\n");
    assert!(!r.live("typedarray"), "{:?}", r.dump_lines());
    // Twin: the holder as a value reaches every member.
    let r = reach("const p: any = process;\nconsole.log(typeof p);\n");
    assert!(r.live("typedarray"), "{:?}", r.dump_lines());
}

#[test]
fn l2_an_alias_of_a_constructor_is_live() {
    let r = reach(
        "const U = Uint8Array;\nfunction make(C: any, n: number) { return new C(n); }\n\
         console.log(make(U, 5).length);\n",
    );
    assert!(r.live("typedarray"));
    let r = reach(
        "const U = Array;\nfunction make(C: any, n: number) { return new C(n); }\n\
         console.log(make(U, 5).length);\n",
    );
    assert!(!r.live("typedarray"), "{:?}", r.dump_lines());
}

#[test]
fn an_observation_without_a_birth_is_live() {
    for src in [
        "console.log(typeof Uint8Array);\n",
        "console.log(Buffer.poolSize);\n",
        "console.log(typeof Object.getPrototypeOf(Int8Array));\n",
        "const x: any = [1];\nconsole.log(x instanceof Uint8Array);\n",
        "const x: any = [1];\nconsole.log(ArrayBuffer.isView(x));\n",
    ] {
        let r = reach(src);
        assert!(r.live("typedarray"), "{src}: {:?}", r.dump_lines());
    }
}

#[test]
fn l3_a_birth_in_a_body_generation_does_not_walk_is_live_by_the_text() {
    // The spread callback is not activated (reach class G): the solve holds
    // no typed-array template, the text names the constructor.
    let r = reach(
        "let n = 0;\n[4].forEach(...[(x: number) => { n = new Uint8Array(x).length; }]);\nconsole.log(n);\n",
    );
    assert!(r.live("typedarray"), "{:?}", r.dump_lines());
    // A function nothing calls: live by the text as well.
    let r =
        reach("function never(n: number) { return new Uint8Array(n).length; }\nconsole.log(1);\n");
    assert!(r.live("typedarray"));
    assert!(
        reason(&r, "typedarray").starts_with("L3:"),
        "{}",
        reason(&r, "typedarray")
    );
    // Twin.
    let r = reach("function never(n: number) { return new Array(n).length; }\nconsole.log(1);\n");
    assert!(!r.live("typedarray"), "{:?}", r.dump_lines());
}

#[test]
fn l3_a_type_only_mention_is_live() {
    // Codegen chooses typed-array lowering from the static type.
    let r = reach(
        "function g(a: Float64Array | null): boolean { return a === null; }\nconsole.log(g(null));\n",
    );
    assert!(r.live("typedarray"), "{:?}", r.dump_lines());
    let r = reach(
        "function g(a: number[] | null): boolean { return a === null; }\nconsole.log(g(null));\n",
    );
    assert!(!r.live("typedarray"), "{:?}", r.dump_lines());
}

#[test]
fn l3_a_static_member_key_names_the_member_of_a_family_on_a_core_holder() {
    let r = reach(
        "function f(s: string, on: boolean) { return on ? s.normalize('NFC') : s; }\nconsole.log(f('a', false));\n",
    );
    assert!(r.live("string_normalize"), "{:?}", r.dump_lines());
    assert!(!r.live("typedarray"));
    let r = reach(
        "function f(s: string, on: boolean) { return on ? s.trim() : s; }\nconsole.log(f('a', false));\n",
    );
    assert!(!r.live("string_normalize"), "{:?}", r.dump_lines());
}

#[test]
fn l3_a_computed_key_in_a_body_that_is_not_walked_names_every_prototype_member() {
    let r = reach("function dead(o: any, k: string) { return o[k]; }\nconsole.log(1);\n");
    assert!(r.live("string_normalize"), "{:?}", r.dump_lines());
    assert!(
        reason(&r, "string_normalize").starts_with("L3:a computed key"),
        "{}",
        reason(&r, "string_normalize")
    );
    // No byte cell is on a core prototype: the part is empty for them.
    assert!(!r.live("typedarray"), "{:?}", r.dump_lines());
    // Twin: no computed key.
    let r = reach("function dead(o: any) { return o.k; }\nconsole.log(1);\n");
    assert!(!r.live("string_normalize"), "{:?}", r.dump_lines());
}

#[test]
fn l4_a_live_family_makes_what_it_requires_live() {
    let r = reach("const t = performance.now();\nconsole.log(t >= 0);\n");
    assert!(r.live("perf_hooks"), "{:?}", r.dump_lines());
    for required in ["event_target", "thread", "typedarray"] {
        assert!(
            reason(&r, required).starts_with("L4:required by "),
            "{required}: {}",
            reason(&r, required)
        );
    }
    assert!(!r.live("regex_engine"));
}

#[test]
fn l4_an_observed_entry_makes_what_it_declares_born_live() {
    // `TextEncoder` declares the birth of a Uint8Array.
    let r = reach("console.log(new TextEncoder().encode('abc').length);\n");
    assert!(r.live("global_text"), "{:?}", r.dump_lines());
    assert!(r.live("typedarray"), "{:?}", r.dump_lines());
    // Twin: a core constructor declares none.
    let r = reach("console.log(new Map([[1, 2]]).size);\n");
    assert!(!r.live("typedarray"), "{:?}", r.dump_lines());
}

#[test]
fn every_realm_entry_is_classified() {
    for e in crate::native_effects::realm() {
        assert!(
            families::entry_families(e.name).is_some(),
            "{}: its holder `{}` is neither a family holder nor in CORE_HOLDERS / CORE_MODULES \
             (perry-runtime-manifest families.rs)",
            e.name,
            families::entry_root(e.name)
        );
        for b in e.births {
            assert!(
                families::entry_families(b).is_some(),
                "{} births {b}",
                e.name
            );
        }
    }
    // Every holder and member a row names exists in the realm.
    let names: std::collections::BTreeSet<&str> = crate::native_effects::realm()
        .iter()
        .map(|e| e.name)
        .collect();
    let roots: std::collections::BTreeSet<&str> =
        names.iter().map(|n| families::entry_root(n)).collect();
    for f in families::RUNTIME_FAMILIES {
        for h in f.holders {
            assert!(
                roots.contains(h),
                "{}: holder {h} is not in the realm",
                f.id
            );
        }
    }
}

#[test]
fn byte_cells_are_on_no_core_prototype() {
    // The computed-key part of the text rule is empty for the typed-array
    // family: none of its entries, and no producer of one, is a member of a
    // core prototype.
    let bytes = families::family_bit("typedarray");
    assert_eq!(crate::family_reach::index().computed_families() & bytes, 0);
    assert_ne!(
        crate::family_reach::index().computed_families() & families::family_bit("string_normalize"),
        0
    );
}
