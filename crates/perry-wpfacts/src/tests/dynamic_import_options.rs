//! `import(spec, options)` reads `options.with` and every enumerable own
//! value of it before loading anything (EvaluateImportCall step 10, #13488).
//! A getter there runs from the runtime, so its body must be reached: an
//! unreached body compiles to a trap.
use super::*;

fn hit_runs(src: &str) -> bool {
    let module = perry_parser::parse_typescript(src, "t.ts").expect("parses");
    let hir = perry_hir::lower_module(&module, "t", "t.ts").expect("lowers");
    let pf = crate::compute(&[("t.ts".to_string(), &hir)], &FactsConfig::default());
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
fn import_options_getters_run() {
    for (name, src) in [
        (
            "getter for `with`",
            r#"function hit() { return undefined; }
const options = { get with() { return hit(); } };
import("./m.js", options).then(() => {}, () => {});"#,
        ),
        (
            "getter on an attribute",
            r#"function hit() { return "json"; }
const options = { with: { get type() { return hit(); } } };
import("./m.js", options).then(() => {}, () => {});"#,
        ),
    ] {
        assert!(hit_runs(src), "{name}: the getter's body is never reached");
    }
}
