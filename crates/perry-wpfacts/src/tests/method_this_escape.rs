//! Per-unit mode: a method of an object that hands its `this` to a callee
//! the solver cannot see (a value read from the global object, a parameter,
//! a value read from ⊤) escapes its receiver. Whole escapes the receiver
//! of such a call ("argument of a call of an unknown function value"); per
//! unit must escape it too, in the unit that owns the object and holds the
//! method, whichever unit calls the method. Per unit never claims more than
//! whole.

use super::*;

fn tmpl<'p>(pf: &'p ProgramFacts, path: &str) -> &'p crate::derive::TemplateFacts {
    pf.templates
        .iter()
        .find(|t| t.display.starts_with(path) && t.fields.iter().any(|f| f.key == "n"))
        .unwrap_or_else(|| panic!("no {path} template with field n\n{}", pf.dump_text()))
}


/// Whole program (the product mode): the object escaped, its `n` claims
/// nothing.
fn escaped_whole(mods: &[(&str, &str)]) {
    let w = whole(mods);
    let t = tmpl(&w, "a.ts");
    let n = t.fields.iter().find(|f| f.key == "n").unwrap();
    assert!(
        t.escape.is_some() && !n.readonly,
        "whole: a.ts's object kept its claims after its method passed `this` to an unknown callee: escape={:?} n={n:?}\n{}",
        t.escape,
        w.dump_text()
    );
}


const READER: &str = r#"
import { lit } from "./a";
lit.createAllType();
console.log(lit.n + 1);
"#;



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
    // Whole program only: per unit (`compute_units`, retired 2026-11-03,
    // reachable only through `PERRY_FACTS_UNITS=1` and tests) misses this
    // escape for a literal whose method calls its parameter with `this`.
    escaped_whole(&[("a", a), ("b", b)]);
}

#[test]
fn a_literal_made_in_a_function_passing_this_to_its_parameter_escapes_the_literal() {
    let a = r#"
function mk() { return { n: 1, createAllType(f: any) { return f(this); } }; }
export const lit = mk();
"#;
    let b = r#"import "unseen-host";

import { lit } from "./a";
lit.createAllType((globalThis as any).zz);
console.log(lit.n + 1);
"#;
    escaped_whole(&[("a", a), ("b", b)]);
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
    escaped_whole(&[("a", a), ("b", b)]);
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
    escaped_whole(&[("a", a), ("b", b)]);
}

/// Whole-program facts (the product mode) of a multi-module program.
fn whole(mods: &[(&str, &str)]) -> ProgramFacts {
    let h: Vec<(String, perry_hir::Module)> = mods
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
    let list: Vec<(String, &perry_hir::Module)> = h.iter().map(|(p, m)| (p.clone(), m)).collect();
    crate::compute(&list, &FactsConfig::default())
}

#[test]
fn a_literal_method_not_using_this_keeps_the_literal() {
    let a = r#"
export const lit = { n: 1, createAllType() { return (globalThis as any).zz(2); } };
"#;
    proven_whole(&[("a", a), ("b", READER)]);
}

#[test]
fn a_literal_method_passing_this_to_a_global_callee_escapes_the_literal() {
    let a = r#"
export const lit = { n: 1, createAllType() { return (globalThis as any).zz(this); } };
"#;
    escaped_whole(&[("a", a), ("b", READER)]);
}

#[test]
fn a_literal_method_passing_this_to_a_known_own_function_keeps_the_literal() {
    let a = r#"
function keep(o) { return o.n; }
export const lit = { n: 1, createAllType() { return keep(this); } };
"#;
    proven_whole(&[("a", a), ("b", READER)]);
}

#[test]
fn a_literal_method_passing_this_to_a_value_read_from_top_escapes_the_literal() {
    let a = r#"import "unseen-host";

const ext = (globalThis as any).ext;
export const lit = { n: 1, createAllType() { return ext(this); } };
"#;
    escaped_whole(&[("a", a), ("b", READER)]);
}

/// Whole program: the object did not escape, `n` stays readonly.
fn proven_whole(mods: &[(&str, &str)]) {
    let w = whole(mods);
    let t = tmpl(&w, "a.ts");
    let n = t.fields.iter().find(|f| f.key == "n").unwrap();
    assert!(
        t.escape.is_none() && n.readonly,
        "whole: a.ts's object lost its claims: escape={:?} n={n:?}\n{}",
        t.escape,
        w.dump_text()
    );
}
