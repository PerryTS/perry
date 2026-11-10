//! A class expression evaluated in a function body is a fresh class object
//! per evaluation (`Expr::ClassExprFresh`): each evaluation runs the
//! template's static blocks with `this` that object (codegen calls the
//! block function directly). SOUND4: test_gap_11154_inline_class_self_binding,
//! test_parity_private_env_plain_fn, test_gap_class_expr_fresh_static_blocks_this.
use super::*;

/// The ids of the static block functions of every class of `src`, and the
/// facts.
fn blocks(src: &str) -> (Vec<u32>, crate::ProgramFacts) {
    let module = perry_parser::parse_typescript(src, "t.ts").expect("parses");
    let hir = perry_hir::lower_module(&module, "t", "t.ts").expect("lowers");
    let pf = crate::compute(&[("t.ts".to_string(), &hir)], &FactsConfig::default());
    assert!(pf.body_reach.complete, "activated");
    let ids = hir
        .classes
        .iter()
        .flat_map(|c| c.static_methods.iter())
        .filter(|m| m.name.starts_with("__perry_static_init_"))
        .map(|m| m.id)
        .collect();
    (ids, pf)
}

fn active(pf: &crate::ProgramFacts, id: u32) -> bool {
    pf.body_reach.bodies.iter().any(|(_, i, _)| *i == id)
}

#[test]
fn a_fresh_class_expression_runs_its_static_blocks() {
    let src = r#"const log: string[] = [];
function make(tag: string) {
  return class {
    static t = tag;
    static { log.push("a:" + this.t); }
    static { log.push("b"); }
  };
}
make("x");
make("y");
console.log(log.join(","));"#;
    let (ids, pf) = blocks(src);
    assert_eq!(ids.len(), 2, "two static blocks");
    for id in ids {
        assert!(active(&pf, id), "static block {id} inactive");
    }
}

/// Control: a factory nothing calls evaluates no class, so its blocks stay
/// inactive.
#[test]
fn an_unevaluated_class_expression_runs_no_static_block() {
    let src = r#"const log: string[] = [];
function make(tag: string) {
  return class {
    static t = tag;
    static { log.push("a:" + this.t); }
  };
}
console.log(log.length, typeof make);"#;
    let (ids, pf) = blocks(src);
    assert_eq!(ids.len(), 1, "one static block");
    assert!(
        !active(&pf, ids[0]),
        "static block of an unevaluated class active"
    );
}
