//! Per-unit mode: a method of an object that hands its `this` to a callee
//! the solver cannot see (a value read from the global object, a parameter,
//! a value read from ⊤) escapes its receiver. Whole escapes the receiver
//! of such a call ("argument of a call of an unknown function value"); per
//! unit must escape it too, in the unit that owns the object and holds the
//! method, whichever unit calls the method. Per unit never claims more than
//! whole.

use super::unit_alias::assert_not_stronger;
use super::*;

fn tmpl<'p>(pf: &'p ProgramFacts, path: &str) -> &'p crate::derive::TemplateFacts {
    pf.templates
        .iter()
        .find(|t| t.display.starts_with(path) && t.fields.iter().any(|f| f.key == "n"))
        .unwrap_or_else(|| panic!("no {path} template with field n\n{}", pf.dump_text()))
}

/// Both modes: the object escaped, its `n` claims nothing.
fn escaped_both(mods: &[(&str, &str)]) {
    let (u, w) = assert_not_stronger(mods);
    for (mode, pf) in [("whole", &w), ("per unit", &u)] {
        let t = tmpl(pf, "a.ts");
        let n = t.fields.iter().find(|f| f.key == "n").unwrap();
        assert!(
            t.escape.is_some() && !n.readonly,
            "{mode}: a.ts's object kept its claims after its method passed `this` to an unknown callee: escape={:?} n={n:?}\n{}",
            t.escape,
            pf.dump_text()
        );
    }
}

/// Both modes: the object did not escape, `n` stays readonly.
fn proven_both(mods: &[(&str, &str)]) {
    let (u, w) = assert_not_stronger(mods);
    for (mode, pf) in [("whole", &w), ("per unit", &u)] {
        let t = tmpl(pf, "a.ts");
        let n = t.fields.iter().find(|f| f.key == "n").unwrap();
        assert!(
            t.escape.is_none() && n.readonly,
            "{mode}: a.ts's object lost its claims: escape={:?} n={n:?}\n{}",
            t.escape,
            pf.dump_text()
        );
    }
}

const READER: &str = r#"
import { lit } from "./a";
lit.createAllType();
console.log(lit.n + 1);
"#;

#[test]
fn a_literal_method_passing_this_to_a_global_callee_escapes_the_literal() {
    let a = r#"
export const lit = { n: 1, createAllType() { return (globalThis as any).zz(this); } };
"#;
    escaped_both(&[("a", a), ("b", READER)]);
}

#[test]
fn a_literal_method_passing_this_to_a_value_read_from_top_escapes_the_literal() {
    let a = r#"import "unseen-host";

const ext = (globalThis as any).ext;
export const lit = { n: 1, createAllType() { return ext(this); } };
"#;
    escaped_both(&[("a", a), ("b", READER)]);
}

#[test]
fn a_literal_method_passing_this_to_its_parameter_escapes_the_literal() {
    let a = r#"
export const lit = { n: 1, createAllType(f) { return f(this); } };
"#;
    let b = r#"import "unseen-host";

import { lit } from "./a";
lit.createAllType((globalThis as any).zz);
console.log(lit.n + 1);
"#;
    escaped_both(&[("a", a), ("b", b)]);
}

#[test]
fn a_class_method_passing_this_to_a_global_callee_escapes_the_instance() {
    let a = r#"
export class K {
  n = 1;
  createAllType() { return (globalThis as any).zz(this); }
}
"#;
    let b = r#"
import { K } from "./a";
const k = new K();
k.createAllType();
console.log(k.n + 1);
"#;
    escaped_both(&[("a", a), ("b", b)]);
}

#[test]
fn a_literal_method_passing_this_to_a_known_own_function_keeps_the_literal() {
    let a = r#"
function keep(o) { return o.n; }
export const lit = { n: 1, createAllType() { return keep(this); } };
"#;
    proven_both(&[("a", a), ("b", READER)]);
}

#[test]
fn a_literal_method_not_using_this_keeps_the_literal() {
    let a = r#"
export const lit = { n: 1, createAllType() { return (globalThis as any).zz(2); } };
"#;
    proven_both(&[("a", a), ("b", READER)]);
}

#[test]
fn a_static_method_passing_this_to_a_global_callee_escapes_the_class() {
    let a = r#"
export class K {
  n = 1;
  static createAllType() { return (globalThis as any).zz(this); }
}
"#;
    let b = r#"
import { K } from "./a";
K.createAllType();
const k = new K();
console.log(k.n + 1);
"#;
    // Cannot fail on main: per unit the class escapes already ("described
    // by no contract"). It guards the class object reaching every instance
    // of the summarized static body (`Model::this_any`).
    escaped_both(&[("a", a), ("b", b)]);
}
