//! A prototype given to a runtime object (`Object.setPrototypeOf` on a native
//! module export, a native instance, an intrinsic prototype) escapes: the
//! runtime's lookups on that object reach the given object's methods
//! (SOUND7 D1: a literal method found that way was excluded while Node ran
//! it; node-suite live-prototype, http2 compat/prototype,
//! native_constructor_call_layout, genmodel_fixed_state).
use super::*;

fn hit_runs(src: &str) -> bool {
    let module = perry_parser::parse_typescript(src, "t.ts").expect("parses");
    let hir = perry_hir::lower_module(&module, "t", "t.ts").expect("lowers");
    let cfg = FactsConfig {
        max_work: Some(50_000_000),
        ..FactsConfig::default()
    };
    let pf = crate::compute(&[("t.ts".to_string(), &hir)], &cfg);
    assert!(pf.body_reach.complete, "activated");
    let id = hir
        .functions
        .iter()
        .find(|f| f.name == "hit")
        .expect("hit")
        .id;
    pf.body_reach.bodies.iter().any(|(_, i, _)| *i == id)
}

#[test]
fn a_literal_given_to_a_native_constructor_runs_through_its_subclass() {
    assert!(hit_runs(
        r#"
import { EventEmitter } from "events";
import { Readable } from "stream";
function hit() { return 1; }
Object.setPrototypeOf(EventEmitter, { apply() { hit(); return "x"; } });
console.log((Readable as any).apply({}, []));
"#
    ));
}

#[test]
fn a_literal_given_to_a_native_instance_runs_as_its_method() {
    assert!(hit_runs(
        r#"
import { EventEmitter } from "events";
function hit() { return 1; }
const e: any = new EventEmitter();
Object.setPrototypeOf(e, { on() { hit(); return "c"; } });
console.log(e.on());
"#
    ));
}

#[test]
fn a_literal_given_to_an_intrinsic_prototype_runs_for_its_functions() {
    assert!(hit_runs(
        r#"
function hit() { return 1; }
function* src(v: number) { yield v; }
const gfp: any = Object.getPrototypeOf(src);
Object.setPrototypeOf(gfp, { apply() { hit(); return 1; } });
console.log((src as any).apply(undefined, [1]));
"#
    ));
}

/// Control: a literal given to a program function (passes on the base).
#[test]
fn a_literal_given_to_a_program_function_runs_as_its_method() {
    assert!(hit_runs(
        r#"
function hit() { return 1; }
function f() { return 1; }
Object.setPrototypeOf(f, { apply() { hit(); return 2; } });
console.log((f as any).apply(null, []));
"#
    ));
}

/// Control: a literal given to a builtin function value: the model walks
/// the builtin function's own prototype link (`proto_node`), so its method
/// runs without an escape.
#[test]
fn a_literal_given_to_a_builtin_function_runs_as_its_method() {
    assert!(hit_runs(
        r#"
function hit() { return 1; }
const f: any = [1].map;
Object.setPrototypeOf(f, { apply() { hit(); return 2; } });
console.log(f.apply(null, []));
"#
    ));
}

/// Only the given prototype's members escape, not the prototype itself: a
/// function given as the prototype of a native instance is never called by
/// a lookup on that instance.
#[test]
fn a_function_given_to_a_native_instance_stays_dormant() {
    assert!(!hit_runs(
        r#"
import { EventEmitter } from "events";
function hit() { return 1; }
function g() { hit(); }
const e: any = new EventEmitter();
Object.setPrototypeOf(e, g);
console.log(typeof e.on);
"#
    ));
}
