//! Summarization: indexing, slicing, planning and building the summaries.

use super::*;

/// Index of the constraints and calls that mention owned nodes.
struct Index {
    preds: FxHashMap<NodeId, Vec<NodeId>>,
    /// node -> (holder, cons index) for constraints with it as a field.
    refs: FxHashMap<NodeId, Vec<(NodeId, u32)>>,
    /// call -> (holder, cons index) for constraints naming it.
    call_refs: FxHashMap<CallId, Vec<(NodeId, u32)>>,
    /// node -> calls with it as an operand (this/args/spread/ret).
    calls_of: FxHashMap<NodeId, Vec<CallId>>,
}

fn build_index(m: &Model) -> Index {
    let owned = |x: NodeId| {
        x != NO_NODE && (x as usize) < m.node_fn.len() && m.node_fn[x as usize] != NO_FN
    };
    let mut ix = Index {
        preds: FxHashMap::default(),
        refs: FxHashMap::default(),
        call_refs: FxHashMap::default(),
        calls_of: FxHashMap::default(),
    };
    for (c, cs) in m.calls.iter().enumerate() {
        for x in call_nodes(cs) {
            if owned(x) {
                let v = ix.calls_of.entry(x).or_default();
                if v.last() != Some(&(c as CallId)) {
                    v.push(c as CallId);
                }
            }
        }
    }
    for (a, node) in m.nodes.iter().enumerate() {
        let a = a as NodeId;
        for &b in node.succ.iter() {
            if owned(b) {
                ix.preds.entry(b).or_default().push(a);
            }
        }
        for (i, c) in node.cons.iter().enumerate() {
            let (ns, ks) = cons_fields(c);
            for x in ns {
                if owned(x) {
                    ix.refs.entry(x).or_default().push((a, i as u32));
                }
            }
            for k in ks {
                let cs = &m.calls[k as usize];
                if call_nodes(cs).into_iter().any(owned) {
                    ix.call_refs.entry(k).or_default().push((a, i as u32));
                }
            }
        }
    }
    ix
}

/// Lexical nesting of closures.
struct Lex {
    parent: Vec<FnIdx>,
    depth: Vec<u32>,
    children: Vec<Vec<FnIdx>>,
}

impl Lex {
    fn new(m: &Model) -> Lex {
        let parent: Vec<FnIdx> = m.funcs.iter().map(|f| f.parent).collect();
        let mut depth = vec![0u32; parent.len()];
        let mut children = vec![Vec::new(); parent.len()];
        // A closure is registered while its parent's body is walked, so the
        // parent's index is smaller.
        for f in 0..parent.len() {
            let p = parent[f];
            if p != NO_FN {
                debug_assert!((p as usize) < f);
                depth[f] = depth[p as usize] + 1;
                children[p as usize].push(f as FnIdx);
            }
        }
        Lex {
            parent,
            depth,
            children,
        }
    }

    /// `a` lexically encloses `g` (strictly).
    fn encloses(&self, a: FnIdx, g: FnIdx) -> bool {
        let mut p = self.parent[g as usize];
        while p != NO_FN {
            if p == a {
                return true;
            }
            p = self.parent[p as usize];
        }
        false
    }
}

/// The parameter-derived nodes of `f`: what its parameters, its `this`, the
/// closure values in `roots` and the outer terms in `through` (walked, not
/// members) reach by copy, every field of a rule on such a node, and the
/// results of calls with such an operand. Only nodes `f` owns and no
/// unsummarizable function captured.
fn slice(
    m: &Model,
    ix: &Index,
    f: FnIdx,
    roots: &[NodeId],
    through: &[NodeId],
    pinned: &FxHashSet<NodeId>,
) -> Vec<NodeId> {
    let fi = &m.funcs[f as usize];
    let ok = |y: NodeId| {
        y != NO_NODE && m.node_fn[y as usize] == f && y != fi.args_all && !pinned.contains(&y)
    };
    let mut seen: FxHashSet<NodeId> = FxHashSet::default();
    let mut d = Vec::new();
    let mut work: Vec<NodeId> = through.to_vec();
    let this = (!fi.lexical_this).then_some(fi.this);
    for &p in fi.params.iter().chain(this.iter()).chain(roots) {
        if ok(p) && seen.insert(p) {
            d.push(p);
            work.push(p);
        }
    }
    let mut i = 0;
    while i < work.len() {
        let x = work[i];
        i += 1;
        let mut add = |y: NodeId, d: &mut Vec<NodeId>, work: &mut Vec<NodeId>| {
            if ok(y) && seen.insert(y) {
                d.push(y);
                work.push(y);
            }
        };
        let node = &m.nodes[x as usize];
        for &y in node.succ.iter() {
            add(y, &mut d, &mut work);
        }
        for c in &node.cons {
            let (ns, ks) = cons_fields(c);
            for y in ns {
                add(y, &mut d, &mut work);
            }
            for k in ks {
                add(m.calls[k as usize].ret, &mut d, &mut work);
            }
        }
        if let Some(rs) = ix.refs.get(&x) {
            for &(h, ci) in rs {
                let (ns, ks) = cons_fields(&m.nodes[h as usize].cons[ci as usize]);
                for y in ns {
                    add(y, &mut d, &mut work);
                }
                for k in ks {
                    add(m.calls[k as usize].ret, &mut d, &mut work);
                }
            }
        }
        if let Some(cs) = ix.calls_of.get(&x) {
            for &k in cs {
                add(m.calls[k as usize].ret, &mut d, &mut work);
            }
        }
    }
    d
}

struct Entries {
    cons: Vec<(NodeId, u32)>,
    calls: Vec<CallId>,
}

/// A literal a body births at generation: its template token, the body's
/// nodes holding it, and its own field nodes with what generation wrote
/// into them directly.
struct Birth {
    tok: Tok,
    tm: TmplId,
    holders: Vec<NodeId>,
    /// (key, field node, writers, generation prims).
    fields: Vec<(Key, NodeId, Vec<NodeId>, u16)>,
    own: Vec<NodeId>,
}

/// Literals each function births whose template token only its own body
/// holds, with fields only generation's direct writes fill: per instance.
fn literal_births(m: &Model, cand: &[bool]) -> FxHashMap<FnIdx, Vec<Birth>> {
    let mut holders: FxHashMap<Tok, Vec<NodeId>> = FxHashMap::default();
    for (n, node) in m.nodes.iter().enumerate() {
        if node.pts.is_empty() {
            continue;
        }
        for t in node.pts.to_sorted() {
            if let TokKind::Obj(tm, NO_CTX) = m.toks[t as usize] {
                if m.tmpl_fn.contains_key(&tm) {
                    holders.entry(t).or_default().push(n as NodeId);
                }
            }
        }
    }
    let mut fields_of: FxHashMap<Tok, Vec<(Key, NodeId)>> = FxHashMap::default();
    for (&(t, a), &n) in &m.fields {
        if holders.contains_key(&t) {
            fields_of.entry(t).or_default().push((Key::Named(a), n));
        }
    }
    for (&t, &n) in &m.field_any {
        if holders.contains_key(&t) {
            fields_of.entry(t).or_default().push((Key::Any, n));
        }
    }
    let mut own_of: FxHashMap<NodeId, Tok> = FxHashMap::default();
    for (&t, v) in &fields_of {
        for &(_, n) in v {
            own_of.insert(n, t);
        }
    }
    for t in holders.keys() {
        for map in [&m.all_fields, &m.proto_node] {
            if let Some(&n) = map.get(t) {
                own_of.insert(n, *t);
            }
        }
    }
    let mut preds: FxHashMap<NodeId, Vec<NodeId>> = FxHashMap::default();
    for (a, node) in m.nodes.iter().enumerate() {
        for &b in node.succ.iter() {
            if own_of.contains_key(&b) {
                preds.entry(b).or_default().push(a as NodeId);
            }
        }
    }
    let mut out: FxHashMap<FnIdx, Vec<Birth>> = FxHashMap::default();
    let mut ts: Vec<Tok> = holders.keys().copied().collect();
    ts.sort_unstable();
    for t in ts {
        let TokKind::Obj(tm, _) = m.toks[t as usize] else {
            continue;
        };
        let f = m.tmpl_fn[&tm];
        if !cand[f as usize] {
            continue;
        }
        let hs = &holders[&t];
        if hs.iter().any(|&h| m.node_fn[h as usize] != f) {
            continue;
        }
        let mut own: Vec<NodeId> = own_of
            .iter()
            .filter(|(_, &x)| x == t)
            .map(|(&n, _)| n)
            .collect();
        own.sort_unstable();
        let is_own = |n: NodeId| own_of.get(&n) == Some(&t);
        let mut ok = own.iter().all(|&n| m.nodes[n as usize].cons.is_empty());
        let mut fields: Vec<(Key, NodeId, Vec<NodeId>, u16)> = Vec::new();
        let mut fl = fields_of.get(&t).cloned().unwrap_or_default();
        fl.sort_unstable_by_key(|x| x.1);
        for (k, n) in fl {
            let mut ws = Vec::new();
            for &p in preds.get(&n).map(|v| v.as_slice()).unwrap_or(&[]) {
                if is_own(p) {
                    continue;
                }
                let pf = m.node_fn[p as usize];
                if pf != f && pf != NO_FN {
                    ok = false;
                }
                ws.push(p);
            }
            fields.push((k, n, ws, m.nodes[n as usize].prim));
        }
        // Only the template's own nodes reach its other nodes.
        for map in [&m.all_fields, &m.proto_node] {
            if let Some(&n) = map.get(&t) {
                if preds.get(&n).is_some_and(|v| v.iter().any(|&p| !is_own(p))) {
                    ok = false;
                }
            }
        }
        if ok {
            out.entry(f).or_default().push(Birth {
                tok: t,
                tm,
                holders: hs.clone(),
                fields,
                own,
            });
        }
    }
    out
}

/// Slices, outer terms and entries of every summarizable function under
/// one set of pinned (shared) captured nodes.
struct Plan {
    slices: FxHashMap<FnIdx, Vec<NodeId>>,
    /// Slice node -> its function, live (not refused) functions only.
    dof: FxHashMap<NodeId, FnIdx>,
    outer: FxHashMap<FnIdx, Vec<NodeId>>,
    refused: FxHashSet<FnIdx>,
    why: FxHashMap<FnIdx, String>,
    entries: FxHashMap<FnIdx, Entries>,
}

/// Where node `y` sits for the summary of `f`.
#[derive(Clone, Copy, PartialEq)]
enum Role {
    Own,
    /// A slice of a closure nested in `f`: that closure's summary owns it.
    Inner,
    /// An outer term of `f`: an enclosing slice's node `f` captured.
    Outer,
    Abs,
    Other(FnIdx),
}

fn role(
    lex: &Lex,
    dof: &FxHashMap<NodeId, FnIdx>,
    outer: &FxHashSet<NodeId>,
    f: FnIdx,
    y: NodeId,
) -> Role {
    match dof.get(&y) {
        None => Role::Abs,
        Some(&a) if a == f => Role::Own,
        Some(&a) if lex.encloses(f, a) => Role::Inner,
        Some(_) if outer.contains(&y) => Role::Outer,
        Some(&a) => Role::Other(a),
    }
}

/// A rule touching `f`'s slice: `Ok(true)` it is `f`'s, `Ok(false)` a nested
/// closure's (the innermost slice it touches owns it), `Err(a)` it also
/// touches an unrelated slice of `a`.
fn disposition(
    m: &Model,
    lex: &Lex,
    dof: &FxHashMap<NodeId, FnIdx>,
    outer: &FxHashSet<NodeId>,
    f: FnIdx,
    h: NodeId,
    c: &Cons,
) -> Result<bool, FnIdx> {
    let (ns, ks) = cons_fields(c);
    let mut nodes: Vec<NodeId> = std::iter::once(h).chain(ns).collect();
    for k in ks {
        nodes.extend(call_nodes(&m.calls[k as usize]));
    }
    let roles: Vec<Role> = nodes.iter().map(|&y| role(lex, dof, outer, f, y)).collect();
    if roles.contains(&Role::Inner) {
        return Ok(false);
    }
    for r in roles {
        if let Role::Other(a) = r {
            return Err(a);
        }
    }
    Ok(true)
}

fn plan(
    m: &Model,
    ix: &Index,
    lex: &Lex,
    cand: &[bool],
    cvs: &FxHashMap<FnIdx, Vec<NodeId>>,
    caps: &[(NodeId, Vec<FnIdx>)],
    births: &FxHashMap<FnIdx, Vec<Birth>>,
    pinned: &FxHashSet<NodeId>,
) -> Plan {
    // Captured nodes each function carries (its own captures and those of
    // closures nested in it), owned by an enclosing function.
    let mut capt: FxHashMap<FnIdx, Vec<NodeId>> = FxHashMap::default();
    for (x, chain) in caps {
        if pinned.contains(x) {
            continue;
        }
        for &p in chain {
            capt.entry(p).or_default().push(*x);
        }
    }
    for v in capt.values_mut() {
        v.sort_unstable();
        v.dedup();
    }
    let none: Vec<NodeId> = Vec::new();
    // 1. Slices, enclosing functions first (outer terms are their nodes).
    let mut order: Vec<FnIdx> = (0..m.funcs.len() as FnIdx)
        .filter(|&f| cand[f as usize])
        .collect();
    order.sort_by_key(|&f| (lex.depth[f as usize], f));
    let mut slices: FxHashMap<FnIdx, Vec<NodeId>> = FxHashMap::default();
    let mut all_dof: FxHashMap<NodeId, FnIdx> = FxHashMap::default();
    for &f in &order {
        let through: Vec<NodeId> = capt
            .get(&f)
            .unwrap_or(&none)
            .iter()
            .copied()
            .filter(|x| all_dof.contains_key(x))
            .collect();
        // A body with no interface term is never instanced (module and
        // initializer bodies run once, unattributed): it keeps no slice.
        // A body whose nested closure captures an enclosing slice's node
        // has one: each instance makes that closure from its own context,
        // even when none of its own nodes derives from the capture
        // (`() => () => x`; before design §14.2 rule 1, the arrow's
        // captured `this` gave it a slice by accident).
        let carries = lex.children[f as usize].iter().any(|g| {
            capt.get(g)
                .is_some_and(|v| v.iter().any(|x| all_dof.contains_key(x)))
        });
        if !carries && slice(m, ix, f, &[], &through, pinned).is_empty() {
            continue;
        }
        // A literal this body births is a value of the slice.
        let mut roots: Vec<NodeId> = Vec::new();
        for b in births.get(&f).map(|v| v.as_slice()).unwrap_or(&[]) {
            if !b.holders.iter().any(|x| pinned.contains(x)) {
                roots.extend(b.holders.iter().copied());
            }
        }
        let d = loop {
            let d = slice(m, ix, f, &roots, &through, pinned);
            let dset: FxHashSet<NodeId> = d.iter().copied().collect();
            // A closure with an outer term in this slice (or one this
            // function carries) is a value of the slice: each instance
            // makes its own.
            let mut more = false;
            for &g in &lex.children[f as usize] {
                let has_outer = capt.get(&g).is_some_and(|v| {
                    v.iter()
                        .any(|x| dset.contains(x) || all_dof.contains_key(x))
                });
                if !has_outer {
                    continue;
                }
                for &cv in cvs.get(&g).unwrap_or(&none) {
                    if !roots.contains(&cv) {
                        roots.push(cv);
                        more = true;
                    }
                }
            }
            if !more {
                break d;
            }
        };
        if !d.is_empty() {
            for &x in &d {
                all_dof.insert(x, f);
            }
            slices.insert(f, d);
        }
    }
    let mut fs: Vec<FnIdx> = slices.keys().copied().collect();
    fs.sort_unstable();
    // 2. Entries; refusals until none are new (a refused function's slice
    //    is plain nodes for every other summary).
    let mut refused: FxHashSet<FnIdx> = FxHashSet::default();
    let mut why: FxHashMap<FnIdx, String> = FxHashMap::default();
    loop {
        let mut dof: FxHashMap<NodeId, FnIdx> = FxHashMap::default();
        for &f in &fs {
            if !refused.contains(&f) {
                for &x in &slices[&f] {
                    dof.insert(x, f);
                }
            }
        }
        let mut outer: FxHashMap<FnIdx, Vec<NodeId>> = FxHashMap::default();
        for &f in &fs {
            if refused.contains(&f) {
                continue;
            }
            let v: Vec<NodeId> = capt
                .get(&f)
                .unwrap_or(&none)
                .iter()
                .copied()
                .filter(|x| dof.get(x).is_some_and(|&o| o != f && lex.encloses(o, f)))
                .collect();
            outer.insert(f, v);
        }
        let mut entries: FxHashMap<FnIdx, Entries> = FxHashMap::default();
        let mut bad_now: FxHashSet<FnIdx> = FxHashSet::default();
        for &f in &fs {
            if refused.contains(&f) {
                continue;
            }
            let d = &slices[&f];
            let oset: FxHashSet<NodeId> = outer[&f].iter().copied().collect();
            let mut bad = false;
            let note = |w: String, bad: &mut bool, why: &mut FxHashMap<FnIdx, String>| {
                *bad = true;
                why.entry(f).or_insert(w);
            };
            let mut cons: Vec<(NodeId, u32)> = Vec::new();
            let mut cons_seen: FxHashSet<(NodeId, u32)> = FxHashSet::default();
            let mut calls: Vec<CallId> = Vec::new();
            let mut calls_seen: FxHashSet<CallId> = FxHashSet::default();
            let mut run_calls: FxHashSet<CallId> = FxHashSet::default();
            let mut inner_calls: FxHashSet<CallId> = FxHashSet::default();
            let mut consider = |h: NodeId,
                                i: u32,
                                held: bool,
                                bad: &mut bool,
                                why: &mut FxHashMap<FnIdx, String>,
                                bad_now: &mut FxHashSet<FnIdx>,
                                cons: &mut Vec<(NodeId, u32)>| {
                let c = &m.nodes[h as usize].cons[i as usize];
                if !held && !gen_kind(c) {
                    return;
                }
                match disposition(m, lex, &dof, &oset, f, h, c) {
                    Ok(true) => {
                        if !gen_kind(c) {
                            *bad = true;
                            why.entry(f).or_insert_with(|| {
                                format!("solver rule {} on {}", c.kind_name(), m.labels[h as usize])
                            });
                        }
                        if cons_seen.insert((h, i)) {
                            cons.push((h, i));
                        }
                        let (_, ks) = cons_fields(c);
                        run_calls.extend(ks);
                    }
                    Ok(false) => {
                        let (_, ks) = cons_fields(c);
                        inner_calls.extend(ks);
                    }
                    Err(a) => {
                        *bad = true;
                        why.entry(f)
                            .or_insert_with(|| "touches another summarized slice".into());
                        bad_now.insert(a);
                    }
                }
            };
            for &x in d {
                for i in 0..m.nodes[x as usize].cons.len() {
                    consider(
                        x,
                        i as u32,
                        true,
                        &mut bad,
                        &mut why,
                        &mut bad_now,
                        &mut cons,
                    );
                }
                if let Some(rs) = ix.refs.get(&x) {
                    for &(h, i) in rs {
                        if dof.get(&h) == Some(&f) {
                            continue;
                        }
                        consider(h, i, false, &mut bad, &mut why, &mut bad_now, &mut cons);
                    }
                }
                let node = &m.nodes[x as usize];
                let preds = ix.preds.get(&x).map(|v| v.as_slice()).unwrap_or(&[]);
                for &y in node.succ.iter().chain(preds) {
                    if let Role::Other(a) = role(lex, &dof, &oset, f, y) {
                        note(
                            "touches another summarized slice".into(),
                            &mut bad,
                            &mut why,
                        );
                        bad_now.insert(a);
                    }
                }
                if let Some(cs) = ix.calls_of.get(&x) {
                    for &k in cs {
                        if calls_seen.insert(k) {
                            calls.push(k);
                        }
                        if let Some(rs) = ix.call_refs.get(&k) {
                            for &(h, i) in rs {
                                if dof.get(&h) == Some(&f) {
                                    continue;
                                }
                                consider(h, i, false, &mut bad, &mut why, &mut bad_now, &mut cons);
                            }
                        }
                    }
                }
            }
            for &(h, i) in &cons {
                let (_, ks) = cons_fields(&m.nodes[h as usize].cons[i as usize]);
                for k in ks {
                    if calls_seen.insert(k) {
                        calls.push(k);
                    }
                }
            }
            // A call only a nested closure's rule runs is that closure's.
            calls.retain(|k| run_calls.contains(k) || !inner_calls.contains(k));
            for &k in &calls {
                for y in call_nodes(&m.calls[k as usize]) {
                    if let Role::Other(a) = role(lex, &dof, &oset, f, y) {
                        note(
                            "touches another summarized slice".into(),
                            &mut bad,
                            &mut why,
                        );
                        bad_now.insert(a);
                    }
                }
                if !run_calls.contains(&k) {
                    note(format!("call {k} run by no rule"), &mut bad, &mut why);
                }
            }
            if bad {
                bad_now.insert(f);
            }
            entries.insert(f, Entries { cons, calls });
        }
        let before = refused.len();
        for f in bad_now {
            if slices.contains_key(&f) {
                refused.insert(f);
            }
        }
        if refused.len() == before {
            return Plan {
                slices,
                dof,
                outer,
                refused,
                why,
                entries,
            };
        }
    }
}

pub(super) fn build_summaries(m: &mut Model) {
    let ix = build_index(m);
    let lex = Lex::new(m);
    let cand: Vec<bool> = m
        .funcs
        .iter()
        .map(|fi| !fi.no_summary && fi.rest.is_none() && !fi.has_arguments)
        .collect();
    // Closure values: the generation node holding a closure's token in its
    // parent's body.
    let mut cvs: FxHashMap<FnIdx, Vec<NodeId>> = FxHashMap::default();
    for n in 0..m.nodes.len() {
        let o = m.node_fn[n];
        if o == NO_FN || m.nodes[n].pts.is_empty() {
            continue;
        }
        for t in m.nodes[n].pts.to_sorted() {
            if let TokKind::Func(g, _) = m.toks[t as usize] {
                if lex.parent[g as usize] == o {
                    cvs.entry(g).or_default().push(n as NodeId);
                }
            }
        }
    }
    let births = literal_births(m, &cand);
    // Capture chains: the capturing function up to (not including) the
    // node's owner. A node whose chain leaves the owner's nesting, or runs
    // through a function that cannot be summarized, stays shared.
    let mut capv: Vec<(NodeId, FnIdx)> = m.captures.iter().copied().collect();
    capv.sort_unstable();
    let mut caps: Vec<(NodeId, Vec<FnIdx>)> = Vec::new();
    let mut pinned: FxHashSet<NodeId> = FxHashSet::default();
    for (x, h) in capv {
        let o = m.node_fn[x as usize];
        if o == NO_FN {
            continue;
        }
        let mut chain = Vec::new();
        let mut p = h;
        let mut nested = true;
        while p != o {
            if p == NO_FN {
                nested = false;
                break;
            }
            chain.push(p);
            p = lex.parent[p as usize];
        }
        if !nested || chain.iter().any(|&p| !cand[p as usize]) {
            pinned.insert(x);
        }
        caps.push((x, chain));
    }
    // A captured node whose chain holds a refused function stays shared
    // too (that function's rules run in the shared body): re-plan.
    let mut rounds = 0;
    let pl = loop {
        rounds += 1;
        let pl = plan(m, &ix, &lex, &cand, &cvs, &caps, &births, &pinned);
        let mut add = Vec::new();
        for (x, chain) in &caps {
            if pinned.contains(x) || !pl.dof.contains_key(x) {
                continue;
            }
            let h = chain.first().copied();
            let broken = chain
                .iter()
                .any(|&p| pl.refused.contains(&p) || (Some(p) != h && !pl.slices.contains_key(&p)));
            if broken {
                add.push(*x);
            }
        }
        if add.is_empty() {
            break pl;
        }
        pinned.extend(add);
    };
    let Plan {
        slices,
        dof,
        outer,
        refused,
        why,
        entries,
    } = pl;
    let mut fs: Vec<FnIdx> = slices.keys().copied().collect();
    fs.sort_unstable();
    // 3. Summaries.
    let mut nsum = 0usize;
    let mut size = 0usize;
    let mut nouter = 0usize;
    let mut nclos = 0usize;
    let mut obs_toks: Vec<(Tok, Vec<NodeId>)> = Vec::new();
    for &f in &fs {
        if refused.contains(&f) {
            continue;
        }
        let d = &slices[&f];
        let of = &outer[&f];
        let oset: FxHashSet<NodeId> = of.iter().copied().collect();
        let e = &entries[&f];
        let mut slot: FxHashMap<NodeId, u32> = FxHashMap::default();
        for (i, &x) in d.iter().chain(of.iter()).enumerate() {
            slot.insert(x, i as u32);
        }
        let enc = |x: NodeId| match slot.get(&x) {
            Some(&i) => SLOT | i,
            None => x,
        };
        let mut callix: FxHashMap<CallId, u32> = FxHashMap::default();
        let mut s = Summary {
            nslots: (d.len() + of.len()) as u32,
            orig: d.iter().chain(of.iter()).copied().collect(),
            outer: (d.len() as u32..(d.len() + of.len()) as u32).collect(),
            ..Default::default()
        };
        for (i, &k) in e.calls.iter().enumerate() {
            callix.insert(k, i as u32);
            s.calls.push((k, map_call(&m.calls[k as usize], enc)));
        }
        let kenc = |k: CallId| match callix.get(&k) {
            Some(&i) => CALL | i,
            None => k,
        };
        // Closures born in this body with outer terms, and its literals:
        // per instance. Their generation tokens stay on the observation
        // nodes (seeds never copy them: they reached slots by
        // generation's online copies).
        let mut strip: FxHashSet<Tok> = FxHashSet::default();
        // The template token's own nodes: observation, never an instance's.
        let mut lit_own: FxHashSet<NodeId> = FxHashSet::default();
        let dset: FxHashSet<NodeId> = d.iter().copied().collect();
        for b in births.get(&f).map(|v| v.as_slice()).unwrap_or(&[]) {
            if !b.holders.iter().all(|x| dset.contains(x)) {
                continue;
            }
            strip.insert(b.tok);
            lit_own.extend(b.own.iter().copied());
            obs_toks.push((b.tok, b.own.clone()));
            let roots: Vec<NodeId> = b
                .holders
                .iter()
                .copied()
                .filter(|&h| {
                    !ix.preds
                        .get(&h)
                        .is_some_and(|ps| ps.iter().any(|p| b.holders.contains(p)))
                })
                .collect();
            for &h in &roots {
                s.births.push((slot[&h], b.tm));
            }
            for (k, _, ws, prim) in &b.fields {
                let mut srcs: Vec<NodeId> = ws.iter().map(|&w| enc(w)).collect();
                if *prim != 0 {
                    let sl = s.nslots;
                    s.nslots += 1;
                    s.orig.push(NO_NODE);
                    s.seeds.push((sl, Vec::new(), *prim));
                    srcs.push(SLOT | sl);
                }
                for &h in &roots {
                    for &src in &srcs {
                        s.cons.push((enc(h), Cons::BirthField { key: *k, src }));
                    }
                }
            }
        }
        for &g in &lex.children[f as usize] {
            let Some(og) = outer.get(&g) else { continue };
            if og.is_empty() || refused.contains(&g) || !slices.contains_key(&g) {
                continue;
            }
            // What the closure captures, by where its terms live here
            // (design §14.2): a slot of this frame, only outer terms, or
            // neither (shared or module nodes).
            let terms: Vec<NodeId> = og.iter().map(|&x| enc(x)).collect();
            let own = terms
                .iter()
                .any(|&x| is_slot(x) && (x & !SLOT) < d.len() as u32);
            let cap = if own {
                CAP_OWN
            } else if terms.iter().any(|&x| is_slot(x)) {
                CAP_OUTER
            } else {
                CAP_NONE
            };
            for &cv in cvs.get(&g).map(|v| v.as_slice()).unwrap_or(&[]) {
                if let Some(&sl) = slot.get(&cv) {
                    s.closures.push((sl, g, terms.clone(), cap));
                    strip.insert(m.funcs[g as usize].token);
                }
            }
        }
        for &x in d {
            let node = &m.nodes[x as usize];
            let toks: Vec<Tok> = node
                .pts
                .to_sorted()
                .into_iter()
                .filter(|t| !strip.contains(t))
                .collect();
            if !toks.is_empty() || node.prim != 0 {
                s.seeds.push((slot[&x], toks, node.prim));
            }
            for &y in node.succ.iter() {
                if !lit_own.contains(&y) && role(&lex, &dof, &oset, f, y) != Role::Inner {
                    s.edges.push((enc(x), enc(y)));
                }
            }
            if let Some(ps) = ix.preds.get(&x) {
                for &p in ps.iter().filter(|p| !lit_own.contains(p)) {
                    match role(&lex, &dof, &oset, f, p) {
                        Role::Own | Role::Inner | Role::Other(_) => {}
                        Role::Outer => s.edges.push((enc(p), enc(x))),
                        Role::Abs => s.edges.push((p, enc(x))),
                    }
                }
            }
        }
        for &(h, i) in &e.cons {
            let c = &m.nodes[h as usize].cons[i as usize];
            s.cons.push((enc(h), map_cons(c, enc, kenc)));
        }
        s.params = m.funcs[f as usize].params.iter().map(|&p| enc(p)).collect();
        s.ret = enc(m.funcs[f as usize].ret);
        s.this = if m.funcs[f as usize].lexical_this {
            NO_NODE
        } else {
            enc(m.funcs[f as usize].this)
        };
        if !is_slot(s.this) {
            s.this = NO_NODE;
        }
        compress(&mut s);
        size += s.edges.len() + s.cons.len() + s.calls.len() + s.seeds.len();
        nsum += 1;
        nouter += s.outer.len();
        nclos += s.closures.len();
        for &k in &e.calls {
            m.sum.templates.insert(k);
        }
        if !of.is_empty() {
            m.sum.outer_of.insert(f, of.clone());
        }
        m.sum.of.insert(f, Rc::new(s));
    }
    // 3b. Composition over the static call graph, callee SCCs first.
    let obs_of = |g: FnIdx, m: &Model| -> Vec<NodeId> {
        m.funcs[g as usize]
            .params
            .iter()
            .chain(std::iter::once(&m.funcs[g as usize].this))
            .map(|&p| if dof.get(&p) == Some(&g) { p } else { NO_NODE })
            .collect()
    };
    let (ncomposed, nsccs) = compose(m, &obs_of);
    for s in m.sum.of.values_mut() {
        let rel = relative_params(s);
        Rc::make_mut(s).rel = rel;
    }
    let size_after: usize = m
        .sum
        .of
        .values()
        .map(|s| s.edges.len() + s.cons.len() + s.calls.len() + s.seeds.len())
        .sum();
    // 4. Observation layer: the slice nodes of summarized functions, and the
    //    nodes of literal template tokens born per instance.
    let nlit = obs_toks.len();
    for (t, own) in obs_toks {
        m.obs_toks.insert(t);
        for n in own {
            m.nodes[n as usize].obs = true;
        }
    }
    for &f in &fs {
        if refused.contains(&f) {
            continue;
        }
        for &x in &slices[&f] {
            m.nodes[x as usize].obs = true;
        }
    }
    for &f in &fs {
        if refused.contains(&f) {
            continue;
        }
        for &x in &slices[&f] {
            let keep: Vec<bool> = {
                let node = &m.nodes[x as usize];
                node.succ.iter().map(|&y| m.nodes[y as usize].obs).collect()
            };
            let node = &mut m.nodes[x as usize];
            let mut it = keep.iter();
            node.succ.retain(|_| *it.next().unwrap());
            if node.succ_set.is_some() {
                let set: crate::pts::FxHashSet<NodeId> = node.succ.iter().copied().collect();
                node.succ_set = Some(Rc::new(set));
            }
        }
    }
    if m.explain {
        let mut v: Vec<_> = refused.iter().copied().collect();
        v.sort_unstable();
        for f in v {
            let w = why
                .get(&f)
                .map_or("touched by another summarized slice", |s| s.as_str());
            eprintln!(
                "perry: wpfacts explain: summary refused {}: {w}",
                m.funcs[f as usize].name
            );
        }
    }
    m.sum.stats = format!(
        "summaries {} of {} candidates ({} refused), size {}, outer terms {}, closure values {}, literal births {}, shared captures {}, plans {}, composed {} calls over {} SCCs, size {}",
        nsum,
        fs.len(),
        refused.len(),
        size,
        nouter,
        nclos,
        nlit,
        pinned.len(),
        rounds,
        ncomposed,
        nsccs,
        size_after
    );
}

/// Per parameter of summary `s`: a slot only the calls write (DECISIONS 44
/// (b)). Any edge, rule term, call result or seeded token naming the slot
/// may put a value there that is not this run's argument (`x = x.parent`),
/// so the parameter's marker would stand for that value too.
fn relative_params(s: &Summary) -> Vec<bool> {
    let mut written: FxHashSet<NodeId> = FxHashSet::default();
    for &(_, b) in &s.edges {
        written.insert(b);
    }
    for (h, c) in &s.cons {
        let (ns, _) = cons_fields(c);
        written.extend(ns.into_iter().filter(|x| x != h));
    }
    for (_, cs) in &s.calls {
        written.insert(cs.ret);
    }
    for (sl, toks, _) in &s.seeds {
        if !toks.is_empty() {
            written.insert(SLOT | sl);
        }
    }
    s.params
        .iter()
        .map(|&p| is_slot(p) && !written.contains(&p))
        .collect()
}
