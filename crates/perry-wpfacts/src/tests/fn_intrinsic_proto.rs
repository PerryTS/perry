//! The prototype of a program function is the model's `Function.prototype`
//! only when the function is born with it (`born_with_function_proto`): an
//! async-generator function's is `%AsyncGeneratorFunction.prototype%`,
//! whose `prototype` is `%AsyncGeneratorPrototype%`, and its `next` called
//! on `undefined` returns a rejected promise (SOUND10 G, node-suite
//! generator-intrinsics: the read went through `Function.prototype`, found
//! no `prototype`, and the reject handler was claimed inactive while Node
//! ran it).
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

const HEAD: &str = "function hit() { return 1; }\nasync function* ag() { yield 1; }\n";

#[test]
fn an_async_generator_function_prototype_is_not_function_prototype() {
    let missed = [
        (
            "getPrototypeOf",
            "const AGP: any = Object.getPrototypeOf(ag).prototype; AGP.next.call(undefined).then(() => 0, () => hit());",
        ),
        (
            "__proto__",
            "const AGP: any = (ag as any).__proto__.prototype; AGP.next.call(undefined).then(() => 0, () => hit());",
        ),
    ]
    .into_iter()
    .filter(|(_, body)| !hit_runs(&format!("{HEAD}{body}")))
    .map(|(what, _)| what)
    .collect::<Vec<_>>();
    assert!(missed.is_empty(), "{missed:?}");
}

/// Control: a plain function's prototype stays `Function.prototype`, whose
/// `call` invokes its receiver (the prototype itself), never its argument.
#[test]
fn a_plain_function_prototype_stays_described() {
    assert!(!hit_runs(&format!(
        "{HEAD}function pf() {{ return 0; }}\nconsole.log(Object.getPrototypeOf(pf).call(hit));"
    )));
}
