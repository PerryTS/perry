//! GGC10: a method call on another unit's function value reads the own
//! key its unit wrote (`k.parse = k`, marked's export) and passes the
//! arguments on. Per unit is never stronger than whole: the options literal
//! handed to `k`, which hands it to a ⊤ callee, escapes in both modes.

use super::*;

const B: &str = r#"class F {
    parse = this.mk();
    mk() { return (n: any, s: any) => { const r = { ...s }; return n + r.async; }; }
}
const E = new F();
function k(l: any, e: any) { return E.parse(l, e); }
k.parse = k;
(globalThis as any).ext(E);
export const marked = k;"#;

const MAIN: &str = r#"import { marked } from "./b";
const html = marked.parse("x", { async: false });
console.log(html);"#;

fn lit_escape(pf: &ProgramFacts) -> Option<String> {
    let t: Vec<_> = pf
        .templates
        .iter()
        .filter(|t| t.kind == "litclass" && t.birth_keys == ["async"])
        .collect();
    assert_eq!(t.len(), 1, "one options literal\n{}", pf.dump_text());
    t[0].escape.clone()
}

#[test]
fn method_on_another_units_function_reads_its_own_key() {
    let mods = [("b", B), ("main", MAIN)];
    let (w, u) = (whole_of(&mods), units_of(&mods));
    let (ew, eu) = (lit_escape(&w), lit_escape(&u));
    assert!(
        ew.is_some(),
        "whole: the literal reaches a ⊤ callee\n{}",
        w.dump_text()
    );
    assert!(
        eu.is_some(),
        "per unit stronger than whole: whole {ew:?}\n{}",
        u.dump_text()
    );
}

const C: &str = r#"import { marked } from "./b";
(marked as any).use = (o: any) => { (globalThis as any).ext(o); return 0; };
export const ready = 1;"#;

const MAIN3: &str = r#"import { marked } from "./b";
import { ready } from "./c";
const r = (marked as any).use({ async: ready });
console.log(r);"#;

/// A third unit stores a key its owner never stores: the reader holding
/// the function sees it (whole) or the store is a census report that
/// leaves every own key unknown (per unit). Never stronger than whole.
#[test]
fn a_third_units_store_on_the_function_is_seen() {
    let mods = [("b", B), ("c", C), ("main", MAIN3)];
    let (w, u) = (whole_of(&mods), units_of(&mods));
    let (ew, eu) = (lit_escape(&w), lit_escape(&u));
    assert!(
        ew.is_some(),
        "whole: the literal reaches ext\n{}",
        w.dump_text()
    );
    assert!(
        eu.is_some(),
        "per unit stronger than whole: whole {ew:?}\n{}",
        u.dump_text()
    );
}

const BT: &str = r#"export function f(x: any) { return (f as any).k({ own: x }); }"#;

const CT: &str = r#"import { f } from "./bt";
(f as any).k = (globalThis as any).ext();
export const set = 1;"#;

const MAINT: &str = r#"import { f } from "./bt";
import { set } from "./ct";
const r = (f as any).k({ held: set });
console.log(r, f(2));"#;

fn escape_of(pf: &ProgramFacts, key: &str) -> Option<String> {
    let t: Vec<_> = pf
        .templates
        .iter()
        .filter(|t| t.kind == "litclass" && t.birth_keys == [key])
        .collect();
    assert_eq!(t.len(), 1, "one `{key}` literal\n{}", pf.dump_text());
    t[0].escape.clone()
}

/// DECISIONS 241, can fail: a unit stores ⊤ on another unit's function;
/// its owner and a third unit read that key as ⊤, so what they pass to it
/// escapes, as in whole. Without the published store neither holder sees
/// it and both literals stay (per unit stronger than whole).
#[test]
fn a_unknown_store_on_another_units_function_is_unknown_to_every_holder() {
    let mods = [("bt", BT), ("ct", CT), ("main", MAINT)];
    let (w, u) = (whole_of(&mods), units_of(&mods));
    for key in ["own", "held"] {
        let (ew, eu) = (escape_of(&w, key), escape_of(&u, key));
        assert!(ew.is_some(), "whole: `{key}` reaches ⊤\n{}", w.dump_text());
        assert!(
            eu.is_some(),
            "per unit: `{key}` must escape as in whole {ew:?}\n{}",
            u.dump_text()
        );
    }
}
