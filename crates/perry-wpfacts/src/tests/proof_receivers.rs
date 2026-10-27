//! A site proof names the receivers it is over: a site whose only receiver
//! tokens are closed markers with nothing beside them has no receiver, per
//! unit as in the whole program.

use super::*;

fn run(mods: &[(&str, &str)], unit: bool) -> ProgramFacts {
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
    let cfg = FactsConfig::default();
    match unit {
        true => crate::compute_units(&list, &cfg),
        false => crate::compute(&list, &cfg),
    }
}

/// Proven sites (by module, offset, key) that name no receiver and no body.
fn vacuous(pf: &ProgramFacts) -> Vec<String> {
    pf.sites
        .iter()
        .filter(|s| !matches!(s.verdict, Verdict::No(_)))
        .filter(|s| s.receivers.is_empty() && s.targets.is_empty())
        .map(|s| format!("{} {} .{} {:?}", s.module, s.byte_offset, s.key, s.verdict))
        .collect()
}

/// Sites `u` proves that `w` does not.
fn stronger(u: &ProgramFacts, w: &ProgramFacts) -> Vec<String> {
    u.sites
        .iter()
        .filter(|s| !matches!(s.verdict, Verdict::No(_)))
        .filter(|s| {
            w.sites.iter().any(|x| {
                (x.module, x.byte_offset, &x.key, x.is_call) == (s.module, s.byte_offset, &s.key, s.is_call)
                    && matches!(x.verdict, Verdict::No(_))
            })
        })
        .map(|s| format!("{} {} .{} {:?}", s.module, s.byte_offset, s.key, s.verdict))
        .collect()
}

const CLASS: &str =
    "export class C { k: number; constructor() { this.k = 1; } m() { return this.k; } }";

#[test]
fn a_method_no_unit_calls_proves_no_read_of_its_receiver() {
    // No unit calls `m`: its receiver marker has nothing beside it, so the
    // read `this.k` has no receiver per unit; the whole program sees the
    // escaped instances (unknown code may call `m` on anything).
    for m in [
        "import { C } from './a'; console.log(1);",
        "import { C } from './a'; export function mk() { return new C(); }",
    ] {
        let mods = [("a", CLASS), ("m", m)];
        let (u, w) = (run(&mods, true), run(&mods, false));
        assert_eq!(vacuous(&u), Vec::<String>::new(), "{}", u.dump_text());
        assert_eq!(stronger(&u, &w), Vec::<String>::new(), "{}\n---\n{}", u.dump_text(), w.dump_text());
        // What it does prove is the typed body behind the entry guard
        // `class_id(this) == C` (design §11.4), as the whole program does.
        let s = u.sites.iter().find(|s| s.key == "k").expect("the read");
        assert!(matches!(s.verdict, Verdict::E0), "{s:?}");
        assert_eq!(s.receivers, vec!["a.ts:C".to_string()]);
    }
}

#[test]
fn a_method_only_the_runtime_calls_proves_no_read_of_its_receiver() {
    // `String(c)` calls `toString` from the runtime: no unit publishes its
    // receivers. The read in `m` (no call at all) is the same case.
    let a = "export class C { k: number; constructor() { this.k = 1; } m() { return this.k; } toString() { return 's' + this.k; } }";
    let mods = [("a", a), ("m", "import { C } from './a'; console.log(String(new C()));")];
    let (u, w) = (run(&mods, true), run(&mods, false));
    assert_eq!(vacuous(&u), Vec::<String>::new(), "{}", u.dump_text());
    assert_eq!(stronger(&u, &w), Vec::<String>::new(), "{}\n---\n{}", u.dump_text(), w.dump_text());
}

#[test]
fn a_called_method_still_proves_its_read() {
    // The rule only drops proofs over nothing: a unit that calls `m` on
    // `new C()` publishes the receiver, and the read is T0 over `C`.
    let mods = [("a", CLASS), ("m", "import { C } from './a'; const c = new C(); console.log(c.m());")];
    let u = run(&mods, true);
    let s = u.sites.iter().find(|s| s.key == "k").expect("the read");
    assert!(matches!(s.verdict, Verdict::T0Mono), "{s:?}\n{}", u.dump_text());
    assert_eq!(s.receivers, vec!["a.ts:C".to_string()]);
}
