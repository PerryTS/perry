//! DECISIONS 235 (refinement): the Function constructor enters a value only
//! under a key that may be `constructor`. An element read of ⊤ (an index,
//! an iteration or callback element) is under a number, which never names
//! `constructor`: it reads ⊤, not the Function constructor, so a later
//! call of that value is no dynamic code.

use super::*;
use crate::model::{BuiltinNs, Key, Model, NodeId, TokKind, TOP};

fn holds_function_ctor(m: &Model, n: NodeId) -> bool {
    m.nodes[n as usize]
        .pts
        .to_sorted()
        .iter()
        .any(|&t| matches!(m.toks[t as usize], TokKind::Builtin(BuiltinNs::Function)))
}

#[test]
fn an_element_of_top_is_not_the_function_constructor() {
    let mut m = copy_solver_model();
    let el = m.node_l("el");
    let ek = m.elem_key();
    m.load_from(TOP, TOP, ek, el);
    assert!(
        m.nodes[el as usize].pts.contains_tok(TOP),
        "an element of ⊤ is ⊤"
    );
    assert!(
        !holds_function_ctor(&m, el),
        "a number key never names `constructor`"
    );
    // An iteration element of ⊤ is an element read.
    let it = m.node_l("it");
    let ik = m.iter_key();
    m.load_from(TOP, TOP, ik, it);
    assert!(m.nodes[it as usize].pts.contains_tok(TOP));
    assert!(!holds_function_ctor(&m, it));
}

#[test]
fn a_key_that_may_be_constructor_of_top_stays_the_function_constructor() {
    let mut m = copy_solver_model();
    let c = m.node_l("ctor");
    let a = m.atom("constructor");
    m.load_from(TOP, TOP, Key::Named(a), c);
    assert!(holds_function_ctor(&m, c));
    let any = m.node_l("any");
    m.load_from(TOP, TOP, Key::Any, any);
    assert!(holds_function_ctor(&m, any));
}
