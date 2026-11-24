//! A literal born in a summarized body gets its own content per instance
//! (`Cons::BirthField`, summary/build.rs `literal_births`). The rule sits on
//! the holder node, which may hold more than the born literal: the for-of
//! iterator's node also holds the iterable (gen_rest/collections.rs
//! `Expr::GetIterator`), and its slot is fed by the template's own source
//! once the body is woken. The birth writes only its own literal's tokens:
//! the loop value never lands in the iterated object's elements.

use super::*;
use crate::model::{prim, Cons, Key, Model, Template, TmplKind, Tok, TokKind, NO_CTX};

fn arr(m: &mut Model) -> (u32, Tok) {
    let tm = m.tmpls.len() as u32;
    m.tmpls.push(Template {
        kind: TmplKind::Arr {
            module: 0,
            site: tm,
            hole_free: true,
            plain: false,
        },
        display: format!("arr{tm}"),
        birth_keys: vec![],
        definite: vec![],
        declared: Vec::new(),
        reserved: Vec::new(),
        inert_definite: Vec::new(),
        inert_reads: Vec::new(),
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
    (tm, m.tok(TokKind::Obj(tm, NO_CTX)))
}

/// The rule itself: a holder that holds the born literal and a foreign
/// object writes the birth's content into the literal only.
#[test]
fn birth_field_writes_only_the_born_literal() {
    let mut m = copy_solver_model();
    let (born, tb) = arr(&mut m);
    let (_, tf) = arr(&mut m);
    let holder = m.node_l("holder");
    m.add_tok(holder, tb);
    m.add_tok(holder, tf);
    let src = m.node_l("src");
    m.add_prim(src, prim::NUM);
    let key = Key::any();
    m.cons(holder, Cons::BirthField { key, src, tm: born });
    m.solve();
    let fb = m.field_any(tb);
    let ff = m.field_any(tf);
    assert!(
        m.nodes[fb as usize].prim & prim::NUM != 0,
        "the born literal gets its content"
    );
    assert_eq!(
        m.nodes[ff as usize].prim & prim::NUM,
        0,
        "a foreign value the holder holds gets none"
    );
}
