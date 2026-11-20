//! A class's own [[Prototype]] is the value its `extends` evaluated
//! (ClassDefinitionEvaluation), also for a built-in parent the model
//! summarizes (`Error`, `TypeError`, ...): a function the program stores on
//! that constructor is what a static read or call on the class (or on a
//! class below it) finds. Red on main for the summarized parents: the class
//! object's chain went straight to `Function.prototype`, so `X.g()` had no
//! callee and the stored body was never activated, while Node ran it
//! (check mode: Activation, inactive body; THREE3 fixtures n4-n6).
use super::*;

/// Whether `hit` can run in `src`.
fn hit_runs(src: &str) -> bool {
    let module = perry_parser::parse_typescript(src, "t.ts").expect("parses");
    let hir = perry_hir::lower_module(&module, "t", "t.ts").expect("lowers");
    let cfg = FactsConfig {
        max_work: Some(50_000_000),
        ..FactsConfig::default()
    };
    let pf = crate::compute(&[("t.ts".to_string(), &hir)], &cfg);
    assert!(pf.body_reach.complete, "activated");
    let id = (hir.functions.iter())
        .find(|f| f.name == "hit")
        .expect("hit")
        .id;
    pf.body_reach.bodies.iter().any(|(_, i, _)| *i == id)
}

const PARENTS: &[&str] = &[
    "Error",
    "TypeError",
    "RangeError",
    "SyntaxError",
    "ReferenceError",
    "Array",
    "Promise",
    "Function",
    "Object",
    "Map",
];

/// THREE3 n6: a direct static call on the class.
#[test]
fn a_function_stored_on_a_builtin_parent_runs_through_the_class() {
    for p in PARENTS {
        let src = format!(
            "function hit() {{ return 5; }}\n\
             class X extends {p} {{}}\n\
             ({p} as any).g = function () {{ return hit(); }};\n\
             console.log((X as any).g());\n"
        );
        assert!(hit_runs(&src), "extends {p}");
    }
}

/// THREE3 n5: the class reaches the call as a value.
#[test]
fn a_function_stored_on_a_builtin_parent_runs_through_a_class_value() {
    for p in PARENTS {
        let src = format!(
            "function hit() {{ return 5; }}\n\
             class X extends {p} {{}}\n\
             function cg(c: any) {{ return c.g(); }}\n\
             ({p} as any).g = function () {{ return hit(); }};\n\
             console.log(cg(X));\n"
        );
        assert!(hit_runs(&src), "extends {p}");
    }
}

/// THREE3 n4: two program classes below the one with the built-in parent.
#[test]
fn a_function_stored_on_a_builtin_parent_runs_through_a_subclass_chain() {
    for p in PARENTS {
        let src = format!(
            "function hit() {{ return 5; }}\n\
             class X extends {p} {{}}\n\
             class Y extends X {{}}\n\
             class Z extends Y {{ static own() {{ return 3; }} }}\n\
             function cg(c: any) {{ return c.g(); }}\n\
             ({p} as any).g = function () {{ return hit(); }};\n\
             console.log(cg(Z), Z.own());\n"
        );
        assert!(hit_runs(&src), "extends {p}");
    }
}

/// Node: the parent is the value the global held when the class was
/// evaluated, here a program class replacing `Error`.
#[test]
fn a_replaced_global_parent_is_the_class_objects_prototype() {
    let src = "function hit() { return 5; }\n\
               (globalThis as any).Error = class { static g() { return hit(); } };\n\
               class X extends Error {}\n\
               console.log((X as any).g());\n";
    assert!(hit_runs(src));
}

/// Control (the test can fail): with no static call on the class the stored
/// function is not run.
#[test]
fn a_function_stored_on_a_builtin_parent_and_not_called_does_not_run() {
    for p in ["Error", "Map"] {
        let src = format!(
            "function hit() {{ return 5; }}\n\
             class X extends {p} {{}}\n\
             ({p} as any).g = function () {{ return hit(); }};\n\
             console.log(typeof X);\n"
        );
        assert!(!hit_runs(&src), "extends {p}");
    }
}
