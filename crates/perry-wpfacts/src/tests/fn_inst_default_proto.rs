//! An instance of a constructor function inherits from the function's own
//! prototype object whether or not the program reads `f.prototype`: an
//! accessor defined on `Object.getPrototypeOf(inst)` runs on a store into a
//! fresh instance (SOUND10 P, node-suite prop_plan_cache_invalidation: the
//! default prototype was made only by a `f.prototype` read, so the
//! instance's chain was empty and the setter was claimed inactive while Node
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

fn program(holder: &str) -> String {
    format!(
        r#"
function hit() {{ return 1; }}
function G(this: any) {{ this.a = 0; }}
const g2: any = new (G as any)();
Object.defineProperty({holder}, "w", {{ set: function (v: any) {{ hit(); }} }});
const g3: any = new (G as any)();
g3.w = 7;
console.log(g3.a);
"#
    )
}

#[test]
fn an_accessor_on_the_instance_prototype_runs() {
    let missed = [
        "Object.getPrototypeOf(g2)",
        "Reflect.getPrototypeOf(g2)",
        "g2.__proto__",
    ]
    .into_iter()
    .filter(|holder| !hit_runs(&program(holder)))
    .collect::<Vec<_>>();
    assert!(missed.is_empty(), "{missed:?}");
}

/// Control: the same setter on an object off the chain never runs.
#[test]
fn an_accessor_off_the_chain_does_not_run() {
    assert!(!hit_runs(&program("({} as any)")));
}
