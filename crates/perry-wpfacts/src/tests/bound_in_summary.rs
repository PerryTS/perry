//! A bound function made inside a summarized body (`bindTo(o) { return
//! leak.bind(null, o) }`) keeps its bound arguments and bound receiver:
//! they flow like the arguments of the bind call, so a target that leaks
//! one of them leaks the caller's value.

use super::*;

fn run(src: &str, units: bool) -> ProgramFacts {
    let module = perry_parser::parse_typescript(src, "m.ts").expect("parses");
    let hir = perry_hir::lower_module(&module, "m", "m.ts").expect("lowers");
    let list = vec![("m.ts".to_string(), &hir)];
    match units {
        true => crate::compute_units(&list, &FactsConfig::default()),
        false => crate::compute(&list, &FactsConfig::default()),
    }
}

fn proven(pf: &ProgramFacts, key: &str) -> bool {
    let s: Vec<_> = pf.sites.iter().filter(|s| s.key == key).collect();
    assert!(!s.is_empty(), "no site .{key}");
    s.iter()
        .all(|s| matches!(s.verdict, Verdict::T0Mono | Verdict::T0Set))
}

/// `body` makes the bound function in `bindTo(o)`, `call` calls it as `g`.
fn program(body: &str, call: &str) -> String {
    format!(
        r#"
function leak(o: Host): Host {{ (globalThis as any).g = o; return o; }}
function leakThis(this: Host): Host {{ (globalThis as any).g = this; return this; }}
function same(o: Host): Host {{ return o; }}
function bindTo(o: Host): any {{ {body} }}
class Host {{
  k = 2;
  m = 3;
}}
const h = new Host();
const g = bindTo(h);
{call};
console.log(h.m);
"#
    )
}

fn leaks(body: &str, call: &str) {
    let src = program(body, call);
    let w = run(&src, false);
    assert!(!proven(&w, "m"), "whole program\n{}", w.dump_text());
    assert!(
        !proven(&run(&src, true), "m"),
        "per unit never stronger than whole"
    );
}

// Node: prints `3`; `globalThis.g` is h.
#[test]
fn a_bound_argument_made_in_a_summary_leaks_where_its_target_does() {
    leaks("return leak.bind(null, o);", "g()");
}

#[test]
fn a_bound_receiver_made_in_a_summary_leaks_where_its_target_does() {
    leaks("return leakThis.bind(o);", "g()");
}

#[test]
fn a_bound_function_made_in_a_summary_and_constructed_leaks_its_argument() {
    leaks("return leak.bind(null, o);", "new g()");
}

#[test]
fn a_nested_bind_in_a_summary_keeps_the_inner_bound_argument() {
    leaks("return leak.bind(null, o).bind(null);", "g()");
}

#[test]
fn a_bind_of_a_bound_function_in_a_summary_keeps_the_outer_bound_argument() {
    leaks(
        "const f: any = leak.bind(null); return f.bind(null, o);",
        "g()",
    );
}

// Negative: the same shapes with a target that keeps its argument stay
// proven, whole and per unit.
#[test]
fn a_bound_argument_made_in_a_summary_that_stays_local_is_proven() {
    let src = program("return same.bind(null, o);", "g()");
    assert!(proven(&run(&src, false), "m"), "whole program");
}
