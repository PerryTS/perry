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
    let stronger: Vec<_> = proven_set(&u).difference(&proven_set(&w)).cloned().collect();
    assert!(stronger.is_empty(), "per unit stronger than whole: {stronger:?}\n{}", u.dump_text());
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
    // UEB: the caller's literal is described with its fields (its
    // `factory` holds n's `Factory`), so the whole body proves as whole.
    for k in KEYS {
        assert!(t_proven(&u, k), "{k}\n{}", u.dump_text());
    }
    assert!(t_proven(&u, "kind"), "{}", u.dump_text());
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

/// UEB: a literal handed to a closed callee is described with its fields;
/// what the callee writes into it, or into what a field holds, is never
/// hidden from the caller: per unit never stronger than whole.
#[test]
fn a_closed_callee_writing_into_a_handed_literal_stays_sound() {
    for body in [
        "context.factory = { createIdentifier(n: string) { return { kind: 1, text: n, parent: 0 }; } };",
        "context.factory.extra = 1;",
        "(globalThis as any).keep(context.factory);",
        "context.other = context.factory;",
    ] {
        let t = format!(r#"import {{ Factory }} from "./n";
function transform(context: any) {{
    {body}
    const node = context.factory.createIdentifier("x");
    return node.kind;
}}
export function run() {{ const c = {{ factory: new Factory() }}; const r = transform(c); return r + (c.factory as any).kind; }}
console.log(run());"#);
        let mods = [("n", N), ("t", t.as_str())];
        let (u, w) = (units_of(&mods), whole_of(&mods));
        let stronger: Vec<_> = proven_set(&u).difference(&proven_set(&w)).cloned().collect();
        assert!(stronger.is_empty(), "{body}: per unit stronger than whole: {stronger:?}\n{}", u.dump_text());
    }
}

/// UEB: a nested literal of the caller in a handed literal's field is
/// described too (its own fields), and a field holding the caller's own
/// function stays unknown to the callee (never stronger).
#[test]
fn a_nested_literal_in_a_handed_literal_is_described() {
    let t = r#"import { Factory } from "./n";
function transform(context: any) {
    const node = context.inner.factory.createIdentifier("x");
    return node.kind + context.f(1);
}
export function run() { return transform({ inner: { factory: new Factory() }, f: (x: number) => x }); }
console.log(run());"#;
    let mods = [("n", N), ("t", t)];
    let (u, w) = (units_of(&mods), whole_of(&mods));
    let stronger: Vec<_> = proven_set(&u).difference(&proven_set(&w)).cloned().collect();
    assert!(stronger.is_empty(), "per unit stronger than whole: {stronger:?}\n{}", u.dump_text());
    for k in ["inner", "factory", "createIdentifier", "kind"] {
        assert!(t_proven(&w, k), "whole {k}\n{}", w.dump_text());
        assert!(t_proven(&u, k), "unit {k}\n{}", u.dump_text());
    }
}
