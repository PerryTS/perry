//! A method called on what a described parameter reaches: the
//! parameter's marker has its callers' values beside it (decision 13,
//! DECISIONS 148), so the call runs on those values and their
//! contracts carry it to their units (DECISIONS 143). The marker records a
//! read of the method's key (`Model::method_on`), as for a parameter's own
//! marker (DECISIONS 144), not an escape: an escape there made each caller
//! report its value and broke its layouts.

use super::*;

/// The keys `transform` reads off its parameter.
const KEYS: [&str; 2] = ["factory", "createIdentifier"];

const N: &str = r#"export class Factory {
    createIdentifier(name: string) { return { kind: 80, text: name, parent: undefined as any }; }
}"#;

/// The sites of module `t.ts` reading `key`.
fn t_sites<'a>(pf: &'a ProgramFacts, key: &str) -> Vec<&'a crate::SiteFacts> {
    let v: Vec<_> = pf
        .sites
        .iter()
        .filter(|s| s.key == key && pf.module_paths[s.module as usize] == "t.ts")
        .collect();
    assert!(!v.is_empty(), "no t.ts site {key}\n{}", pf.dump_text());
    v
}

fn t_proven(pf: &ProgramFacts, key: &str) -> bool {
    t_sites(pf, key)
        .iter()
        .all(|s| matches!(s.verdict, Verdict::T0Mono | Verdict::T0Set))
}

/// Per unit never proves a site the whole program does not, and breaks
/// no layout: the callee's calls on what it is passed are described.
fn reads_the_method(mods: &[(&str, &str)]) -> ProgramFacts {
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
    for k in KEYS {
        assert!(t_proven(&w, k), "whole {k}\n{}", w.dump_text());
    }
    assert!(u.broken.is_empty(), "{:?}\n{}", u.broken, u.dump_text());
    u
}

/// Decision 13: a closed function of a sibling unit. Before: 4 rounds,
/// `Factory`, `fn:…createIdentifier` and its result's layout broken.
#[test]
fn a_closed_functions_method_call_on_a_parameters_reach_is_a_read() {
    let t = r#"import { Factory } from "./n";
function transform(context: any) {
    const node = context.factory.createIdentifier("x");
    return node.kind;
}
export function run() { return transform({ factory: new Factory() }); }
console.log(run());"#;
    let u = reads_the_method(&[("n", N), ("t", t)]);
    assert!(t_proven(&u, "factory"), "{}", u.dump_text());
    assert_eq!(u.round_work.len(), 2, "{:?}", u.round_work);
}

/// DECISIONS 148: a published class's method run by another unit, its
/// parameter holding what that unit passes.
#[test]
fn a_published_methods_method_call_on_a_parameters_reach_is_a_read() {
    let t = r#"export class T {
    transform(context: any) {
        const node = context.factory.createIdentifier("x");
        return node.kind;
    }
}"#;
    let b = r#"import { T } from "./t";
import { Factory } from "./n";
console.log(new T().transform({ factory: new Factory() }));"#;
    reads_the_method(&[("n", N), ("t", t), ("b", b)]);
}
