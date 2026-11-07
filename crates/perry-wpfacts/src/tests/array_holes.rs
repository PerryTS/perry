//! An Array's iteration reads each index below its length: an element, or
//! `undefined` at a hole. A birth that fills every index (a literal without
//! an elision, an array record) has none; appends (`push`: the element key)
//! keep it so; a store under an index or a computed key, a `length` store,
//! an index `defineProperty`, a spread into it and unknown code's writes
//! may leave one. An index read (`a[i]`) may be past the end and keeps
//! `undefined` either way.

use super::*;
use crate::model::{
    prim, Cons, Key, Model, NodeId, StoreSite, Template, TmplKind, Tok, TokKind, NO_CTX,
};

fn arr(m: &mut Model, hole_free: bool) -> Tok {
    let tm = m.tmpls.len() as u32;
    m.tmpls.push(Template {
        kind: TmplKind::Arr {
            module: 0,
            site: tm,
            hole_free,
        },
        display: format!("arr{tm}"),
        birth_keys: vec![],
        definite: vec![],
        declared: Vec::new(),
        reserved: Vec::new(),
        parent: None,
        ctor: None,
        native_parent: None,
        accessor_keys: vec![],
        has_computed_members: false,
        field_init: None,
        proto_members: vec![],
        parent_value: None,
        parent_links: Vec::new(),
        null_proto: false,
        chain_miss: false,
        chain_miss_open: false,
        species: None,
        per_evaluation: false,
        static_methods: Vec::new(),
    });
    let t = m.tok(TokKind::Obj(tm, NO_CTX));
    let fa = m.field_any(t);
    m.add_prim(fa, prim::STR);
    t
}

fn site(m: &mut Model) -> u32 {
    m.stores.push(StoreSite {
        module: 0,
        func: 0,
        desc: "test".into(),
        ctor_phase_of: None,
        birth: false,
        ctor_fn: None,
        rooted: false,
        deep: false,
        key_value: None,
        in_loop: None,
    });
    (m.stores.len() - 1) as u32
}

/// The iteration element of `t`, solved.
fn iterated(m: &mut Model, t: Tok) -> NodeId {
    let d = m.node_l("iterated");
    let ik = m.iter_key();
    m.load_from(t, t, ik, d);
    d
}

fn holds_undef(m: &Model, n: NodeId) -> bool {
    m.nodes[n as usize].prim & prim::UNDEF != 0
}

fn store(m: &mut Model, t: Tok, key: Key) {
    let v = m.node_l("v");
    m.add_prim(v, prim::STR);
    let s = site(m);
    m.store_into(t, key, v, s);
}

#[test]
fn a_dense_birth_iterates_its_elements_only() {
    let mut m = copy_solver_model();
    let t = arr(&mut m, true);
    let d = iterated(&mut m, t);
    m.solve();
    assert_eq!(
        m.nodes[d as usize].prim,
        prim::STR,
        "a dense array has no hole"
    );
    // An index read may be past the end.
    let e = m.node_l("elem");
    let ek = m.elem_key();
    m.load_from(t, t, ek, e);
    m.solve();
    assert!(holds_undef(&m, e), "an index read keeps undefined");
}

#[test]
fn a_birth_not_known_dense_may_hold_a_hole() {
    let mut m = copy_solver_model();
    let t = arr(&mut m, false);
    let d = iterated(&mut m, t);
    m.solve();
    assert!(holds_undef(&m, d));
}

#[test]
fn an_append_leaves_no_hole() {
    let mut m = copy_solver_model();
    let t = arr(&mut m, true);
    let d = iterated(&mut m, t);
    let ek = m.elem_key();
    store(&mut m, t, ek);
    m.solve();
    assert_eq!(
        m.nodes[d as usize].prim,
        prim::STR,
        "push fills the next index"
    );
}

/// Each write that may leave a hole, after the read was made.
#[test]
fn index_length_and_computed_stores_may_leave_a_hole() {
    for k in ["3", "length", "*any"] {
        let mut m = copy_solver_model();
        let t = arr(&mut m, true);
        let d = iterated(&mut m, t);
        m.solve();
        assert!(!holds_undef(&m, d), "{k}: dense before the store");
        let key = match k {
            "*any" => Key::any(),
            _ => Key::Named(m.atom(k)),
        };
        store(&mut m, t, key);
        m.solve();
        assert!(holds_undef(&m, d), "{k}: the store may leave a hole");
    }
}

#[test]
fn a_named_non_index_store_leaves_no_hole() {
    let mut m = copy_solver_model();
    let t = arr(&mut m, true);
    let d = iterated(&mut m, t);
    let key = Key::Named(m.atom("tag"));
    store(&mut m, t, key);
    m.solve();
    assert!(!holds_undef(&m, d));
}

#[test]
fn define_index_delete_and_escape_may_leave_a_hole() {
    for what in ["define", "delete", "escape"] {
        let mut m = copy_solver_model();
        let t = arr(&mut m, true);
        let d = iterated(&mut m, t);
        let n = m.node_l("o");
        m.add_tok(n, t);
        let r = m.reason("test");
        let key = m.atom("2");
        match what {
            "define" => m.cons(
                n,
                Cons::DefineKey {
                    key,
                    data: true,
                    reason: r,
                },
            ),
            "delete" => m.cons(n, Cons::DeleteKey { key, reason: r }),
            _ => m.escape(t, r),
        }
        m.solve();
        assert!(holds_undef(&m, d), "{what}: may leave a hole");
    }
}
