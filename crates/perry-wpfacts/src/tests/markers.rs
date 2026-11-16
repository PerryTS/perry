//! Shared-set adoption and argument markers: how markers flow, link and cost solve work.
use super::*;

#[test]
fn a_node_that_left_out_a_top_marker_never_takes_a_shared_set() {
    use crate::model::{TokKind, TOP};
    // DECISIONS 42: `d` holds ⊤, so the marker `mk` (reads as ⊤) is left
    // out of it; `d` no longer holds all of `s`. Equal sizes then prove
    // nothing: sharing `s`'s set would drop ⊤ from `d` (`copy_to`), and
    // so would adopting a later delivery (`try_copy_delivery`). `e` has no
    // ⊤: the marker reaches it as before.
    let mut m = copy_solver_model();
    let mk = m.tok(TokKind::ModNs(900));
    let a = m.tok(TokKind::ModNs(901));
    let b = m.tok(TokKind::ModNs(902));
    let (s, d, e) = (m.node_l("s"), m.node_l("d"), m.node_l("e"));
    m.set_marker_top(mk);
    m.add(s, &[mk, a], 0);
    m.add_tok(d, TOP);
    m.edge(s, d);
    m.edge(s, e);
    m.solve();
    let has = |m: &crate::model::Model, n, t| m.nodes[n as usize].pts.contains_tok(t);
    assert!(
        has(&m, d, TOP) && has(&m, d, a) && !has(&m, d, mk),
        "copy_to"
    );
    assert!(has(&m, e, mk) && has(&m, e, a), "no ⊤ beside: delivered");
    m.add_tok(s, b);
    m.solve();
    assert!(has(&m, d, TOP) && has(&m, d, b), "try_copy_delivery");
}

#[test]
fn a_delivery_never_adopts_a_marker_that_turned_top_beside_a_top() {
    use crate::model::{TokKind, TOP};
    // DECISIONS 42: `x` serves its successors before its rules, so `mk`
    // reaches `s` beside ⊤ while it does not read as ⊤ yet; then `x`'s
    // sink records the escape and `mk` reads as ⊤. `d` holds `s`'s old
    // set, ⊤ included, so sizes allow adopting `s`'s set, which would put
    // `mk` beside ⊤ in `d`. The delivery goes through `add_tokens`, which
    // leaves it out.
    let mut m = copy_solver_model();
    let mk = m.tok(TokKind::ModNs(900));
    let a = m.tok(TokKind::ModNs(901));
    m.param_marker.insert(mk, (0, 0));
    let (x, s, d) = (m.node_l("x"), m.node_l("s"), m.node_l("d"));
    // The nodes of one run: a contract parameter's marker is a token there
    // (in storage it would cross as a member of the node's marker, MRK).
    for n in [x, s, d] {
        m.nodes[n as usize].frame = 1;
    }
    m.add(s, &[a], 0);
    m.add_tok(s, TOP);
    m.edge(s, d);
    m.edge(x, s);
    m.sink_node(x, 0);
    m.solve();
    m.add(x, &[mk], 0);
    m.solve();
    let has = |m: &crate::model::Model, n, t| m.nodes[n as usize].pts.contains_tok(t);
    assert!(m.marker_is_top(mk), "the sink made the marker read as ⊤");
    assert!(has(&m, s, mk), "it reached s before it read as ⊤");
    assert!(
        has(&m, d, TOP) && has(&m, d, a) && !has(&m, d, mk),
        "left out beside ⊤"
    );
}

#[test]
fn a_parameter_node_holds_its_argument_marker() {
    use crate::model::{FuncInfo, TokKind, MARK_READ, NO_CTX, NO_FN};
    // DECISIONS 44, the rule itself: a function's parameter node `p` holds
    // its argument marker in place of the markers that arrive, through a
    // delivery too (`try_copy_delivery` would adopt `s`'s set: `p` held all
    // of it). A marker that arrives after the argument marker was used
    // takes the uses so far.
    let mut m = copy_solver_model();
    let (this, ret, args, p) = (
        m.node_l("this"),
        m.node_l("ret"),
        m.node_l("args"),
        m.node_l("p"),
    );
    let token = m.tok(TokKind::Func(0, NO_CTX));
    m.funcs.push(FuncInfo {
        #[cfg(test)]
        body_packet: Default::default(),
        module: 0,
        func_id: 0,
        raise: u32::MAX,
        name: "f".into(),
        params: vec![p],
        rest: None,
        this,
        ret,
        args_all: args,
        has_arguments: false,
        arguments: None,
        own_test: crate::model::OwnTestBody::Unknown,
        token,
        ctor_of: None,
        lexical_this: false,
        no_summary: true,
        foreign_params: true,
        gen_proto: None,
        gen_fn_protos: None,
        gen_async: false,
        gen_reyields: false,
        proto: None,
        captures: Vec::new(),
        forwards_super: false,
        parent: NO_FN,
        forwarded: false,
        ret_param: None,
    });
    let (m1, m2) = (m.param_marker_tok(0, 0), m.param_marker_tok(0, 1));
    let a = m.tok(TokKind::ModNs(7));
    let s = m.node_l("s");
    m.add_tok(s, a);
    m.edge(s, p);
    m.solve();
    let has = |m: &crate::model::Model, n, t| m.nodes[n as usize].pts.contains_tok(t);
    assert!(has(&m, p, a), "a real token passes");
    m.add_tok(s, m1);
    m.solve();
    let r = m.arg_marker_of[&p];
    assert!(
        has(&m, p, r) && !has(&m, p, m1),
        "the argument marker in place of the marker"
    );
    assert_eq!(m.marker_members(r), vec![m1]);
    m.mark_use(r, MARK_READ);
    assert_eq!(
        m.marker_use[&m1] & MARK_READ,
        MARK_READ,
        "a member takes the uses"
    );
    m.add_tok(s, m2);
    m.solve();
    assert!(!has(&m, p, m2));
    assert_eq!(
        m.marker_use[&m2] & MARK_READ,
        MARK_READ,
        "a late member takes the uses so far"
    );
}

#[test]
fn a_store_into_a_marker_makes_no_field_of_it() {
    use crate::model::{FuncInfo, Key, TokKind, MARK_ESCAPE, NO_CTX, NO_FN};
    // DECISIONS 41, the rule itself (the unit-level growth test,
    // unit_mode::a_store_into_an_argument_keeps_no_field_of_the_marker, is
    // also held by DECISIONS 42): a marker stands for another unit's value,
    // this unit has no storage for it. DECISIONS 84 refines "the store is
    // its escape": the store is a key of its contract (the reader stores
    // an unknown value there), and the stored value escapes.
    let mut m = copy_solver_model();
    let (this, ret, args) = (m.node_l("this"), m.node_l("ret"), m.node_l("args"));
    let token = m.tok(TokKind::Func(0, NO_CTX));
    m.funcs.push(FuncInfo {
        #[cfg(test)]
        body_packet: Default::default(),
        module: 0,
        func_id: 0,
        raise: u32::MAX,
        name: "f".into(),
        params: Vec::new(),
        rest: None,
        this,
        ret,
        args_all: args,
        has_arguments: false,
        arguments: None,
        own_test: crate::model::OwnTestBody::Unknown,
        token,
        ctor_of: None,
        lexical_this: false,
        no_summary: true,
        foreign_params: true,
        gen_proto: None,
        gen_fn_protos: None,
        gen_async: false,
        gen_reyields: false,
        proto: None,
        captures: Vec::new(),
        forwards_super: false,
        parent: NO_FN,
        forwarded: false,
        ret_param: None,
    });
    let mk = m.param_marker_tok(0, 0);
    let v = m.node_l("v");
    let o = m.tok(TokKind::ModNs(7));
    m.add_tok(v, o);
    let x = m.atom("x");
    let site = m.anon_post_site("test store");
    m.store_into(mk, Key::Named(x), v, site);
    m.solve();
    assert!(!m.marker_use.get(&mk).is_some_and(|u| u & MARK_ESCAPE != 0));
    assert_eq!(m.param_stores.get(&(0, 0)), Some(&vec![x]));
    // DECISIONS 176: a value of this unit stored into the argument goes to
    // the reader as a result does: the flow holds it, and the contract
    // check after the solve (`Gen::contract_check`, none in this bare
    // model) describes it or lets it escape alone.
    let vals = m.store_flows.values().find_map(|fl| fl.vals);
    assert!(
        vals.is_some_and(|n| m.nodes[n as usize].pts.to_sorted().contains(&o)),
        "the stored value is the flow's"
    );
    assert!(!m.fields.contains_key(&(mk, x)), "a field of the marker");
}

#[test]
fn an_argument_marker_leaves_beside_when_a_valued_member_links() {
    use crate::model::{Cons, FuncInfo, TokKind, NO_CTX, NO_FN};
    // DECISIONS 75: an argument marker stands for its members alone. With
    // none (or only described closed parameter markers) it adds no kinds to
    // arithmetic; once a member that stands for real values links in (`f`
    // is not a described closed function here), the arithmetic result it
    // skipped takes the object kinds (ToPrimitive), monotone.
    let mut m = copy_solver_model();
    let (this, ret, args) = (m.node_l("this"), m.node_l("ret"), m.node_l("args"));
    let token = m.tok(TokKind::Func(0, NO_CTX));
    m.funcs.push(FuncInfo {
        #[cfg(test)]
        body_packet: Default::default(),
        module: 0,
        func_id: 0,
        raise: u32::MAX,
        name: "f".into(),
        params: Vec::new(),
        rest: None,
        this,
        ret,
        args_all: args,
        has_arguments: false,
        arguments: None,
        own_test: crate::model::OwnTestBody::Unknown,
        token,
        ctor_of: None,
        lexical_this: false,
        no_summary: true,
        foreign_params: true,
        gen_proto: None,
        gen_fn_protos: None,
        gen_async: false,
        gen_reyields: false,
        proto: None,
        captures: Vec::new(),
        forwards_super: false,
        parent: NO_FN,
        forwarded: false,
        ret_param: None,
    });
    let mk = m.param_marker_tok(0, 0);
    // `r2` is a member of `r1` (an instance parameter passed on); `mk`
    // links into `r2` later, so `r1` leaves with it.
    let (n1, n2) = (m.node_l("p1"), m.node_l("p2"));
    let (src, dst) = (m.node_l("src"), m.node_l("dst"));
    let r1 = m.arg_marker(n1);
    let r2 = m.arg_marker(n2);
    m.link_marker(r2, r1);
    m.cons(src, Cons::Arith { dst, other: crate::model::NO_NODE });
    m.add_tok(src, r1);
    m.solve();
    assert_eq!(m.nodes[dst as usize].prim, 0, "beside: no kinds of its own");
    m.link_marker(mk, r2);
    m.solve();
    let k = m.nodes[dst as usize].prim;
    assert!(
        k & crate::model::prim::NUM != 0 && k & crate::model::prim::BIGINT != 0,
        "a valued member: the skipped result takes the object kinds ({k:#x})"
    );
}

/// A model with one function `f` whose parameter markers the tests make.
pub(super) fn marker_model() -> crate::model::Model {
    use crate::model::{FuncInfo, TokKind, NO_CTX, NO_FN};
    let mut m = copy_solver_model();
    let (this, ret, args) = (m.node_l("this"), m.node_l("ret"), m.node_l("args"));
    let token = m.tok(TokKind::Func(0, NO_CTX));
    m.funcs.push(FuncInfo {
        #[cfg(test)]
        body_packet: Default::default(),
        module: 0,
        func_id: 0,
        raise: u32::MAX,
        name: "f".into(),
        params: Vec::new(),
        rest: None,
        this,
        ret,
        args_all: args,
        has_arguments: false,
        arguments: None,
        own_test: crate::model::OwnTestBody::Unknown,
        token,
        ctor_of: None,
        lexical_this: false,
        no_summary: true,
        foreign_params: true,
        gen_proto: None,
        gen_fn_protos: None,
        gen_async: false,
        gen_reyields: false,
        proto: None,
        forwards_super: false,
        parent: NO_FN,
        captures: Vec::new(),
        forwarded: false,
        ret_param: None,
    });
    m
}

#[test]
fn a_use_through_an_argument_marker_is_solve_work() {
    use crate::model::MARK_ESCAPE;
    // 113-CWL3: a use of an argument marker (DECISIONS 44), under a key
    // too (design §18.4), walks every marker it stands for. The walk is
    // solve work, so the work bound sees it: uncounted, checker.ts#u18
    // took 4.4x main's solve time for the same counted work.
    let mut m = copy_solver_model();
    // A parameter marker exists (else no use is recorded at all).
    m.param_marker.insert(u32::MAX - 1, (0, 0));
    let n0 = m.node_l("p0");
    let r = m.arg_marker(n0);
    let n = 64u64;
    for _ in 0..n {
        let ni = m.node_l("p");
        let ri = m.arg_marker(ni);
        m.link_marker(ri, r);
    }
    let a = m.atom("k");
    let k = m.keyed_reach_of(r, a);
    let w = m.work.get();
    m.mark_use(k, MARK_ESCAPE);
    assert!(
        m.work.get() - w >= n,
        "a keyed use walked {n} members for {} work",
        m.work.get() - w
    );
    let w = m.work.get();
    m.mark_use(r, MARK_ESCAPE);
    assert!(
        m.work.get() - w >= n,
        "a use walked {n} members for {} work",
        m.work.get() - w
    );
}

#[test]
fn a_keyed_use_reaches_every_marker_an_argument_marker_stands_for() {
    use crate::model::{MARK_ESCAPE, MARK_READ};
    // Design §18.4: argument marker `r2` stands for `r1` and `r3`, and `r1`
    // for `r2` (a recursion passing its argument on): `mk1` and `mk3`, the
    // markers they stand for, take every use of what `r1` and `r2` reach
    // under a key. `r1` is linked after the use of `inner` (the callee read
    // `p.inner` before the second caller's marker arrived), `r3` before.
    let mut m = marker_model();
    let (mk1, mk3) = (m.param_marker_tok(0, 0), m.param_marker_tok(0, 1));
    let (n1, n2, n3) = (m.node_l("p1"), m.node_l("p2"), m.node_l("p3"));
    let r1 = m.arg_marker(n1);
    let r2 = m.arg_marker(n2);
    let r3 = m.arg_marker(n3);
    let (inner, other) = (m.atom("inner"), m.atom("other"));
    m.link_marker(mk1, r1);
    m.link_marker(mk3, r3);
    m.link_marker(r3, r2);
    let k = m.keyed_reach_of(r2, inner);
    m.mark_use(k, MARK_ESCAPE);
    m.link_marker(r1, r2);
    m.link_marker(r2, r1);
    let k = m.keyed_reach_of(r1, other);
    m.mark_use(k, MARK_READ);
    m.settle_keyed_uses();
    for mk in [mk1, mk3] {
        assert_eq!(
            m.keyed_uses.get(&mk),
            Some(&vec![(inner, MARK_ESCAPE), (other, MARK_READ)]),
            "{:?}",
            m.keyed_uses
        );
    }
    // A keyed use is the contract's fact alone: the marker itself is not ⊤.
    assert!(!m.marker_is_top(mk1) && !m.marker_is_top(mk3));
}

#[test]
fn a_keyed_use_is_a_use_of_each_reach_marker_it_stands_for() {
    use crate::model::MARK_ESCAPE;
    // Design §18.4 with DECISIONS 42: what a parameter reaches arrives at a
    // shared body (`r` stands for the reach marker `pr`); the body lets
    // `p.k` escape, so `pr`, which reaches itself under every key, escaped
    // and reads as ⊤ during the solve. A member that links in after the
    // use (`pr2`) takes it too.
    let mut m = marker_model();
    let (mk, mk2) = (m.param_marker_tok(0, 0), m.param_marker_tok(0, 1));
    let (pr, pr2) = (m.marker_reach[&mk], m.marker_reach[&mk2]);
    let n0 = m.node_l("p");
    let r = m.arg_marker(n0);
    m.link_marker(pr, r);
    let a = m.atom("k");
    let k = m.keyed_reach_of(r, a);
    m.mark_use(k, MARK_ESCAPE);
    assert!(m.marker_is_top(pr), "the reach member escaped");
    assert!(!m.marker_is_top(pr2));
    m.link_marker(pr2, r);
    assert!(
        m.marker_is_top(pr2),
        "a late reach member takes the keyed use"
    );
}

#[test]
fn a_repeated_keyed_use_costs_one_step() {
    use crate::model::MARK_READ;
    // A keyed use costs what the use names: once `p.k` was read, reading
    // it again (each `p.k.x` reads it) is the step on its keyed reach
    // marker alone, not a walk to the argument marker and its members.
    let mut m = marker_model();
    let mk = m.param_marker_tok(0, 0);
    let n0 = m.node_l("p");
    let r = m.arg_marker(n0);
    for _ in 0..64 {
        let ni = m.node_l("pi");
        let ri = m.arg_marker(ni);
        m.link_marker(mk, ri);
        m.link_marker(ri, r);
    }
    let a = m.atom("k");
    let k = m.keyed_reach_of(r, a);
    m.mark_use(k, MARK_READ);
    let reps = 100;
    let w = m.work.get();
    for _ in 0..reps {
        m.mark_use(k, MARK_READ);
    }
    assert!(
        m.work.get() - w <= reps,
        "{reps} repeated uses cost {} work",
        m.work.get() - w
    );
}

#[test]
fn a_keyed_use_in_a_recursion_walks_each_member_once_per_bit() {
    use crate::model::MARK_ESCAPE;
    // The checker.ts#u18 / typescript.js createTypeChecker fan-out: a ring
    // of `n` argument markers (helpers passing their argument around a
    // recursion) each stand for every other; `keys` keys of it are used.
    // Each key costs its own use; the bits cross each member once, so the
    // work is keys + n, not keys x n. After the solve every parameter
    // marker in the ring has every key.
    let mut m = marker_model();
    let n = 32u32;
    let keys = 64u64;
    let rs: Vec<_> = (0..n)
        .map(|_| {
            let p = m.node_l("p");
            m.arg_marker(p)
        })
        .collect();
    let mks: Vec<_> = (0..n).map(|i| m.param_marker_tok(0, i)).collect();
    for i in 0..n as usize {
        m.link_marker(mks[i], rs[i]);
        m.link_marker(rs[(i + 1) % n as usize], rs[i]);
    }
    let w = m.work.get();
    let atoms: Vec<_> = (0..keys).map(|i| m.atom(&format!("k{i}"))).collect();
    for &a in &atoms {
        let k = m.keyed_reach_of(rs[0], a);
        m.mark_use(k, MARK_ESCAPE);
    }
    let spent = m.work.get() - w;
    assert!(
        spent <= 3 * keys + 2 * n as u64 + 4,
        "{keys} keys on a ring of {n} cost {spent} work"
    );
    m.settle_keyed_uses();
    for mk in mks {
        let v = m.keyed_uses.get(&mk).map_or(0, |v| v.len());
        assert_eq!(v as u64, keys, "a ring member has {v} of {keys} keys");
    }
}

#[test]
fn a_protocol_touch_of_an_argument_marker_reads_nothing() {
    use crate::model::{Protocol, MARK_ESCAPE, MARK_TOUCH};
    // A marker has no accessors: a touch reads nothing off it (a read node
    // per touch would take a fresh marker each, `Model::crosses`, and JSON
    // would touch that one in turn without end). JSON's nested values are
    // what the marker reaches, touched; `toJSON` is what it reaches under
    // that key, called: an escape (DECISIONS 111).
    let mut m = copy_solver_model();
    m.param_marker.insert(u32::MAX - 1, (0, 0));
    let n0 = m.node_l("p0");
    let r = m.arg_marker(n0);
    let nodes = m.nodes.len();
    for p in [Protocol::Json, Protocol::IterResult, Protocol::ToPrimitive] {
        m.implicit(r, 0, p);
    }
    let reads = (nodes..m.nodes.len())
        .filter(|&n| m.labels[n].starts_with("implicit"))
        .count();
    assert_eq!(reads, 0, "a touch of a marker made read or call nodes");
    let reach = m.reach_of(r);
    assert!(m
        .marker_use
        .get(&reach)
        .is_some_and(|&u| u & MARK_TOUCH != 0));
    let a = m.atom("toJSON");
    let tj = m.marker_load(r, Some(a));
    assert!(m.marker_use.get(&tj).is_some_and(|&u| u & MARK_ESCAPE != 0));
}
