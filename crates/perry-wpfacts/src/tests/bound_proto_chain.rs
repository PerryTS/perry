//! A bound function's [[Prototype]] is its target's (ECMA-262 10.4.1.3
//! BoundFunctionCreate step 1: `target.[[GetPrototypeOf]]()`), not
//! `%Function.prototype%`. An async function's is
//! `%AsyncFunction.prototype%`, a generator function's
//! `%GeneratorFunction.prototype%`: a program store there succeeds (neither
//! is add-locked as `Function.prototype` is, DEC 250) and a read through the
//! bound function finds it. A bound class constructor's is the class's
//! parent constructor; a bound plain function's stays `Function.prototype`.
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

const HEAD: &str = "function hit() { return 1; }\n\
async function a(): Promise<number> { return 1; }\n\
function* g(): Generator<number> { yield 1; }\n";

/// The store goes through `Object.getPrototypeOf(bound)` (the bound value's
/// own [[Prototype]], the target's) and the read through the bound value.
#[test]
fn a_bound_async_function_reads_its_targets_prototype() {
    assert!(hit_runs(&format!(
        "{HEAD}const b: any = a.bind(null);\nconst P: any = Object.getPrototypeOf(b);\nP.hook = hit;\nb.hook();\n"
    )));
}

#[test]
fn a_bound_generator_function_reads_its_targets_prototype() {
    assert!(hit_runs(&format!(
        "{HEAD}const b: any = g.bind(null);\nconst P: any = Object.getPrototypeOf(b);\nP.hook = hit;\nb.hook();\n"
    )));
}

/// A bound async function made from a bound async function: the chain is
/// the innermost target's.
#[test]
fn a_bound_bound_async_function_reads_its_targets_prototype() {
    assert!(hit_runs(&format!(
        "{HEAD}const b: any = a.bind(null).bind(null);\nconst P: any = Object.getPrototypeOf(b);\nP.hook = hit;\nb.hook();\n"
    )));
}

/// The store goes through the async function's own prototype; the read
/// through its bound value sees the same object.
#[test]
fn a_bound_async_function_sees_a_store_on_the_targets_prototype() {
    assert!(hit_runs(&format!(
        "{HEAD}const b: any = a.bind(null);\nconst P: any = Object.getPrototypeOf(a);\nP.hook = hit;\nb.hook();\n"
    )));
}

/// A bound class constructor's [[Prototype]] is the class's parent
/// constructor: its static method is found through the bound value.
#[test]
fn a_bound_class_reads_its_parent_constructor() {
    assert!(hit_runs(&format!(
        "{HEAD}class A {{ static hook(): number {{ return hit(); }} }}\nclass B extends A {{}}\n\
const b: any = B.bind(null);\nb.hook();\n"
    )));
    assert!(hit_runs(&format!(
        "{HEAD}class A {{ static hook(): number {{ return hit(); }} }}\nclass B extends A {{}}\n\
const b: any = B.bind(null);\nObject.getPrototypeOf(b).hook();\n"
    )));
}

/// Control: a bound plain function's prototype stays `Function.prototype`,
/// whose `call` invokes its receiver (the prototype itself), never its
/// argument; and a bound bound plain function's.
#[test]
fn a_bound_plain_function_prototype_stays_function_prototype() {
    assert!(!hit_runs(&format!(
        "{HEAD}function pf() {{ return 0; }}\nconsole.log(Object.getPrototypeOf(pf.bind(null)).call(hit));"
    )));
    assert!(!hit_runs(&format!(
        "{HEAD}function pf() {{ return 0; }}\nconsole.log(Object.getPrototypeOf(pf.bind(null).bind(null)).call(hit));"
    )));
}

/// A bound async-generator function's [[Prototype]] is
/// `%AsyncGeneratorFunction.prototype%`, whose `prototype` is
/// `%AsyncGeneratorPrototype%`; its `next` called on `undefined` returns a
/// rejected promise, so the reject handler runs (as for the unbound
/// function, `fn_intrinsic_proto`). Read through `Function.prototype`, the
/// bound value finds no `prototype` and the handler is claimed inactive.
#[test]
fn a_bound_async_generator_function_prototype_is_its_targets() {
    assert!(hit_runs(&format!(
        "{HEAD}async function* ag() {{ yield 1; }}\n\
const AGP: any = Object.getPrototypeOf(ag.bind(null)).prototype;\n\
AGP.next.call(undefined).then(() => 0, () => hit());\n"
    )));
}
