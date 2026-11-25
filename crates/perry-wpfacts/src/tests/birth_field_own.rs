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

/// A Proxy `new Proxy(target, handler)` with its parts: target node holding
/// an object, handler node holding `handler` (an object, or ⊤).
fn proxy(m: &mut Model, handler: Tok) -> (u32, Tok, Tok) {
    let (_, tt) = arr(m);
    let target = m.node_l("proxy-target");
    m.add_tok(target, tt);
    let h = m.node_l("proxy-handler");
    m.add_tok(h, handler);
    let p = m.proxies.len() as u32;
    m.proxies.push(crate::solve::proxy::ProxyRec {
        target,
        handler: h,
        ops: Default::default(),
        callable: false,
        calls: Vec::new(),
    });
    (p, m.tok(TokKind::Proxy(p)), tt)
}

/// Rule PXBIRTH: a Proxy the holder holds is a foreign value of the holder:
/// the birth defines on the new literal (CreateDataPropertyOrThrow on the
/// literal, 13.2.5.5 / 13.2.4.1), so no internal method of the Proxy runs
/// (no [[DefineOwnProperty]] 10.5.6, no trap, no forward). The Proxy does
/// not escape and its target gets none of the content, whether the
/// handler is a program object or ⊤ (no handler code is reached).
#[test]
fn birth_field_runs_nothing_on_a_proxy_the_holder_holds() {
    for top_handler in [false, true] {
        let mut m = copy_solver_model();
        let (born, tb) = arr(&mut m);
        let h = match top_handler {
            false => arr(&mut m).1,
            true => crate::model::TOP,
        };
        let (p, tp, tt) = proxy(&mut m, h);
        let holder = m.node_l("holder");
        m.add_tok(holder, tb);
        m.add_tok(holder, tp);
        let src = m.node_l("src");
        m.add_prim(src, prim::NUM);
        m.cons(holder, Cons::BirthField { key: Key::any(), src, tm: born });
        m.solve();
        let fb = m.field_any(tb);
        assert!(m.nodes[fb as usize].prim & prim::NUM != 0, "the born literal gets its content");
        assert!(
            !m.proxies[p as usize]
                .ops
                .contains_key(&crate::solve::proxy::ProxyOp::Escape),
            "the Proxy does not escape (top handler: {top_handler})"
        );
        assert!(!m.is_escaped(tt), "the Proxy target does not escape");
        let ft = m.field_any(tt);
        assert_eq!(m.nodes[ft as usize].prim & prim::NUM, 0, "the Proxy target gets none");
    }
}

/// Control: a ⊤ sink on a node holding the Proxy is still its escape
/// (rule PXESC): the target escapes.
#[test]
fn proxy_at_a_top_sink_still_escapes() {
    let mut m = copy_solver_model();
    let (_, th) = arr(&mut m);
    let (p, tp, tt) = proxy(&mut m, th);
    let n = m.node_l("sink");
    m.add_tok(n, tp);
    let reason = m.reason("test sink");
    m.cons(n, Cons::TopSink { reason });
    m.solve();
    assert!(m.proxies[p as usize]
        .ops
        .contains_key(&crate::solve::proxy::ProxyOp::Escape));
    assert!(m.is_escaped(tt), "the Proxy target escapes");
}
