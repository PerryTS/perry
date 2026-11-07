//! A computed-key method call on a primitive receiver (`s[m](o)`, `m` not a
//! constant) runs the primitive's methods: their argument coercion calls a
//! program `valueOf` (SOUND7 D2: the `IdxMCall` guard that attaches the
//! primitive key rule held the same `idx_done` entry as the rule's own run,
//! so the method never ran and `valueOf` was excluded while Node ran it).
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
    let id = hir.functions.iter().find(|f| f.name == "hit").expect("hit").id;
    pf.body_reach.bodies.iter().any(|(_, i, _)| *i == id)
}

#[test]
fn a_computed_method_of_a_primitive_coerces_its_object_argument() {
    assert!(hit_runs(
        r#"
function hit() { return 1; }
const o: any = { valueOf() { hit(); return 1; } };
const m = ["charAt", "at"][Math.random() > 2 ? 1 : 0];
const s: any = "abc";
console.log(s[m](o));
"#
    ));
}

#[test]
fn a_computed_method_through_a_parameter_receiver_coerces_its_argument() {
    assert!(hit_runs(
        r#"
function hit() { return 1; }
function call1(s: any, m: string, a: any): any { return s[m](a); }
const o: any = { valueOf() { hit(); return 1; } };
const M: string[] = ["substr", "charAt"];
for (let k = 0; k < M.length; k++) console.log(call1("ab" + k, M[k], o));
"#
    ));
}

/// Control: a constant key names the method (passes on the base).
#[test]
fn a_constant_method_of_a_primitive_coerces_its_object_argument() {
    assert!(hit_runs(
        r#"
function hit() { return 1; }
const o: any = { valueOf() { hit(); return 1; } };
const s: any = "abc";
console.log(s.charAt(o));
"#
    ));
}
