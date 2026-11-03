use super::*;

/// The summarized functions a summary's call `i` reaches through a callee
/// node that is not a slot: `(i, functions)` for its own calls.
pub(super) fn static_callees(m: &Model, s: &Summary) -> Vec<(usize, Vec<FnIdx>)> {
    let mut out: Vec<(usize, Vec<FnIdx>)> = Vec::new();
    for (h, c) in &s.cons {
        let Cons::Call { call } = *c else { continue };
        if call == NO_NODE || call & CALL == 0 || is_slot(*h) || *h == NO_NODE {
            continue;
        }
        let mut gs: Vec<FnIdx> = m.nodes[*h as usize]
            .pts
            .to_sorted()
            .into_iter()
            .filter_map(|t| match m.toks[t as usize] {
                // A closure with outer terms is bound per context, never
                // expanded into a caller.
                TokKind::Func(g, _) if m.sum.of.get(&g).is_some_and(|s| s.outer.is_empty()) => {
                    Some(g)
                }
                _ => None,
            })
            .collect();
        gs.sort_unstable();
        gs.dedup();
        if !gs.is_empty() {
            out.push(((call & !CALL) as usize, gs));
        }
    }
    out.sort_unstable();
    out
}

/// Expand every call to a summarized function outside the caller's SCC
/// into the caller's summary (design CW2 3.2), one level (design §14.3):
/// the callee's own body, before its composition. The static calls inside
/// it are copied as calls its caller's instances bind: below a site's own
/// callee, the shape is the context, not the call string.
/// Returns (expanded calls, SCCs).
pub(super) fn compose(
    m: &mut Model,
    obs_of: &dyn Fn(FnIdx, &Model) -> Vec<NodeId>,
) -> (usize, usize) {
    let mut fs: Vec<FnIdx> = m.sum.of.keys().copied().collect();
    fs.sort_unstable();
    let direct: FxHashMap<FnIdx, Vec<(usize, Vec<FnIdx>)>> = fs
        .iter()
        .map(|&f| (f, static_callees(m, &m.sum.of[&f])))
        .collect();
    // Tarjan, iterative.
    let mut index: FxHashMap<FnIdx, u32> = FxHashMap::default();
    let mut low: FxHashMap<FnIdx, u32> = FxHashMap::default();
    let mut on: FxHashSet<FnIdx> = FxHashSet::default();
    let mut stack: Vec<FnIdx> = Vec::new();
    let mut order: Vec<Vec<FnIdx>> = Vec::new();
    let mut next = 0u32;
    for &root in &fs {
        if index.contains_key(&root) {
            continue;
        }
        let succ = |f: FnIdx| -> Vec<FnIdx> {
            let mut v: Vec<FnIdx> = direct[&f]
                .iter()
                .flat_map(|(_, gs)| gs.iter().copied())
                .collect();
            v.sort_unstable();
            v.dedup();
            v
        };
        let mut work: Vec<(FnIdx, Vec<FnIdx>, usize)> = vec![(root, succ(root), 0)];
        index.insert(root, next);
        low.insert(root, next);
        next += 1;
        stack.push(root);
        on.insert(root);
        while let Some((f, ss, i)) = work.last_mut() {
            let f = *f;
            if *i < ss.len() {
                let g = ss[*i];
                *i += 1;
                if !index.contains_key(&g) {
                    index.insert(g, next);
                    low.insert(g, next);
                    next += 1;
                    stack.push(g);
                    on.insert(g);
                    let sg = succ(g);
                    work.push((g, sg, 0));
                } else if on.contains(&g) {
                    let l = low[&f].min(index[&g]);
                    low.insert(f, l);
                }
                continue;
            }
            work.pop();
            if let Some((p, _, _)) = work.last() {
                let l = low[p].min(low[&f]);
                low.insert(*p, l);
            }
            if low[&f] == index[&f] {
                let mut c = Vec::new();
                loop {
                    let x = stack.pop().unwrap();
                    on.remove(&x);
                    c.push(x);
                    if x == f {
                        break;
                    }
                }
                c.sort_unstable();
                order.push(c);
            }
        }
    }
    for (i, c) in order.iter().enumerate() {
        for &f in c {
            m.sum.scc.insert(f, i as u32);
        }
    }
    let own_of: FxHashMap<FnIdx, Rc<Summary>> = m.sum.of.clone();
    let mut n = 0usize;
    for c in &order {
        for &f in c {
            let mut s: Summary = (*m.sum.of[&f]).clone();
            let mut any = false;
            for (ci, gs) in &direct[&f] {
                for &g in gs {
                    if m.sum.scc[&g] == m.sum.scc[&f] {
                        continue;
                    }
                    s.expanded.push((*ci as u32, g));
                    let sg = own_of[&g].clone();
                    let caps: Vec<Option<NodeId>> = (0..m.funcs[g as usize].params.len())
                        .map(|i| m.capture_param(g, i))
                        .collect();
                    expand(&mut s, *ci, &sg, &obs_of(g, m), &caps);
                    any = true;
                    n += 1;
                }
            }
            if any {
                compress(&mut s);
                m.sum.of.insert(f, Rc::new(s));
            }
        }
    }
    m.sum.ncomposed = n;
    (n, order.len())
}

/// Copy `g`'s summary into `s` over fresh slots and wire call `ci`'s
/// operands to it the way a binding would: arguments into the parameters
/// (and into `g`'s observation parameters `gobs`), missing ones undefined,
/// spread elements into every parameter, `g`'s result into the call's.
/// A hidden capture parameter (`caps`) takes its class environment slot,
/// not the call's argument (DECISIONS 117, 127).
pub(super) fn expand(
    s: &mut Summary,
    ci: usize,
    g: &Summary,
    gobs: &[NodeId],
    caps: &[Option<NodeId>],
) {
    let off = s.nslots;
    let coff = s.calls.len() as u32;
    let tr = |x: NodeId| {
        if is_slot(x) {
            SLOT | (off + (x & !SLOT))
        } else {
            x
        }
    };
    let kr = |k: CallId| {
        if k != NO_NODE && k & CALL != 0 {
            CALL | (coff + (k & !CALL))
        } else {
            k
        }
    };
    s.nslots += g.nslots;
    s.orig.extend_from_slice(&g.orig);
    for (i, t, p) in &g.seeds {
        s.seeds.push((off + i, t.clone(), *p));
    }
    for &(a, b) in &g.edges {
        s.edges.push((tr(a), tr(b)));
    }
    for (h, c) in &g.cons {
        s.cons.push((tr(*h), map_cons(c, tr, kr)));
    }
    for (t, cs) in &g.calls {
        s.calls.push((*t, map_call(cs, tr)));
    }
    for (c, h, terms, cap) in &g.closures {
        s.closures
            .push((off + c, *h, terms.iter().map(|&x| tr(x)).collect(), *cap));
    }
    for &(i, h) in &g.expanded {
        s.expanded.push((coff + i, h));
    }
    for &(b, tm) in &g.births {
        s.births.push((off + b, tm));
    }
    let call = s.calls[ci].1.clone();
    let fresh = |s: &mut Summary| {
        let i = s.nslots;
        s.nslots += 1;
        s.orig.push(NO_NODE);
        SLOT | i
    };
    let dsts: Vec<Vec<NodeId>> = (0..g.params.len())
        .map(|i| {
            [tr(g.params[i]), gobs.get(i).copied().unwrap_or(NO_NODE)]
                .into_iter()
                .filter(|&d| d != NO_NODE)
                .collect()
        })
        .collect();
    let mut undef = NO_NODE;
    for (i, ds) in dsts.iter().enumerate() {
        for &d in ds {
            if let Some(env) = caps.get(i).copied().flatten() {
                s.edges.push((env, d));
            } else if i < call.args.len() {
                if call.args[i] != NO_NODE {
                    s.edges.push((call.args[i], d));
                }
            } else {
                if undef == NO_NODE {
                    undef = fresh(s);
                    s.seeds.push((undef & !SLOT, Vec::new(), prim::UNDEF));
                }
                s.edges.push((undef, d));
            }
        }
    }
    for &sp in &call.spread {
        let elems = fresh(s);
        s.cons.push((
            sp,
            Cons::Load {
                key: Key::Any,
                dst: elems,
            },
        ));
        for (i, ds) in dsts.iter().enumerate() {
            if caps.get(i).copied().flatten().is_some() {
                continue;
            }
            for &d in ds {
                s.edges.push((elems, d));
            }
        }
    }
    let tdsts: Vec<NodeId> = [
        tr(g.this),
        gobs.get(g.params.len()).copied().unwrap_or(NO_NODE),
    ]
    .into_iter()
    .filter(|&d| d != NO_NODE)
    .collect();
    if !tdsts.is_empty() {
        let src = match call.this {
            ThisArg::Node(n) => n,
            ThisArg::Tok(x) => {
                let n = fresh(s);
                s.seeds.push((n & !SLOT, vec![x], 0));
                n
            }
            ThisArg::Undef => {
                let n = fresh(s);
                s.seeds.push((n & !SLOT, Vec::new(), prim::UNDEF));
                n
            }
        };
        for d in tdsts {
            s.edges.push((src, d));
        }
    }
    let r = tr(g.ret);
    if r != NO_NODE && call.ret != NO_NODE {
        s.edges.push((r, call.ret));
    }
}

/// Remove what an instance does not need: relays inside the slice become
/// direct edges, slots that reach nothing go, and read rules into such
/// slots go (the observation layer runs them on a superset of tokens).
pub fn compress(s: &mut Summary) {
    let n = s.nslots as usize;
    let sl = |x: NodeId| {
        if is_slot(x) {
            Some((x & !SLOT) as usize)
        } else {
            None
        }
    };
    let mut pinned = vec![false; n];
    for &p in s.params.iter().chain([&s.ret, &s.this]) {
        if let Some(i) = sl(p) {
            pinned[i] = true;
        }
    }
    // Outer slots are the context's nodes; closure values and their terms
    // are read when an instance is filled.
    for &o in &s.outer {
        pinned[o as usize] = true;
    }
    for &(b, _) in &s.births {
        pinned[b as usize] = true;
    }
    for (c, _, terms, _) in &s.closures {
        pinned[*c as usize] = true;
        for &x in terms {
            if let Some(i) = sl(x) {
                pinned[i] = true;
            }
        }
    }
    let mut seeded = vec![false; n];
    for &(i, _, _) in &s.seeds {
        seeded[i as usize] = true;
    }
    // Uses as a rule holder or field, or a call operand.
    let mut uses = vec![0u32; n];
    let mut cons: Vec<Option<(NodeId, Cons)>> = s.cons.drain(..).map(Some).collect();
    for (h, c) in cons.iter().flatten() {
        for x in std::iter::once(*h).chain(cons_fields(c).0) {
            if let Some(i) = sl(x) {
                uses[i] += 1;
            }
        }
    }
    for (_, cs) in &s.calls {
        for x in call_nodes(cs) {
            if let Some(i) = sl(x) {
                uses[i] += 1;
            }
        }
    }
    let mut out: Vec<FxHashSet<NodeId>> = vec![FxHashSet::default(); n];
    let mut inn: Vec<FxHashSet<NodeId>> = vec![FxHashSet::default(); n];
    let mut abs_edges: FxHashSet<(NodeId, NodeId)> = FxHashSet::default();
    let add_edge = |a: NodeId,
                    b: NodeId,
                    out: &mut Vec<FxHashSet<NodeId>>,
                    inn: &mut Vec<FxHashSet<NodeId>>,
                    abs_edges: &mut FxHashSet<(NodeId, NodeId)>| {
        if a == b {
            return;
        }
        match (sl(a), sl(b)) {
            (None, None) => {
                abs_edges.insert((a, b));
            }
            (ia, ib) => {
                if let Some(i) = ia {
                    out[i].insert(b);
                }
                if let Some(j) = ib {
                    inn[j].insert(a);
                }
            }
        }
    };
    for &(a, b) in &s.edges {
        add_edge(a, b, &mut out, &mut inn, &mut abs_edges);
    }
    let mut dead = vec![false; n];
    let mut changed = true;
    while changed {
        changed = false;
        // (c) read rules into slots that reach nothing.
        for k in 0..cons.len() {
            let Some((_, c)) = &cons[k] else { continue };
            let Some(i) = read_dst(c).and_then(sl) else {
                continue;
            };
            if pinned[i] || !out[i].is_empty() || uses[i] != 1 {
                continue;
            }
            let (h, c) = cons[k].take().unwrap();
            for x in std::iter::once(h).chain(cons_fields(&c).0) {
                if let Some(j) = sl(x) {
                    uses[j] -= 1;
                }
            }
            changed = true;
        }
        for i in 0..n {
            if dead[i] || pinned[i] || uses[i] != 0 {
                continue;
            }
            if out[i].is_empty() {
                // (b) reaches nothing.
                for a in std::mem::take(&mut inn[i]) {
                    if let Some(j) = sl(a) {
                        out[j].remove(&(SLOT | i as u32));
                    }
                }
                dead[i] = true;
                changed = true;
            } else if !seeded[i] && (inn[i].len() <= 1 || out[i].len() == 1) {
                // (a) a relay: predecessors straight to successors.
                let ins = std::mem::take(&mut inn[i]);
                let outs = std::mem::take(&mut out[i]);
                let me = SLOT | i as u32;
                for &a in &ins {
                    if let Some(j) = sl(a) {
                        out[j].remove(&me);
                    }
                }
                for &b in &outs {
                    if let Some(j) = sl(b) {
                        inn[j].remove(&me);
                    }
                }
                for &a in &ins {
                    for &b in &outs {
                        add_edge(a, b, &mut out, &mut inn, &mut abs_edges);
                    }
                }
                dead[i] = true;
                changed = true;
            }
        }
    }
    // Renumber the live slots.
    let mut map = vec![u32::MAX; n];
    let mut orig = Vec::new();
    for i in 0..n {
        if !dead[i] {
            map[i] = orig.len() as u32;
            orig.push(s.orig[i]);
        }
    }
    let rn = |x: NodeId| match sl(x) {
        Some(i) if map[i] != u32::MAX => SLOT | map[i],
        Some(_) => NO_NODE,
        None => x,
    };
    let mut edges: Vec<(NodeId, NodeId)> = Vec::new();
    for i in 0..n {
        if dead[i] {
            continue;
        }
        let mut v: Vec<NodeId> = out[i].iter().copied().collect();
        v.sort_unstable();
        for b in v {
            edges.push((rn(SLOT | i as u32), rn(b)));
        }
        let mut v: Vec<NodeId> = inn[i]
            .iter()
            .copied()
            .filter(|&a| sl(a).is_none())
            .collect();
        v.sort_unstable();
        for a in v {
            edges.push((a, rn(SLOT | i as u32)));
        }
    }
    let mut ae: Vec<_> = abs_edges.into_iter().collect();
    ae.sort_unstable();
    edges.extend(ae);
    s.edges = edges;
    s.cons = cons
        .into_iter()
        .flatten()
        .map(|(h, c)| (rn(h), map_cons(&c, rn, |k| k)))
        .collect();
    for (_, cs) in s.calls.iter_mut() {
        *cs = map_call(cs, rn);
    }
    s.seeds = s
        .seeds
        .drain(..)
        .filter(|(i, _, _)| !dead[*i as usize])
        .map(|(i, t, p)| (map[i as usize], t, p))
        .collect();
    s.params = s.params.iter().map(|&p| rn(p)).collect();
    s.ret = rn(s.ret);
    s.this = rn(s.this);
    s.outer = s.outer.iter().map(|&o| map[o as usize]).collect();
    for (b, _) in s.births.iter_mut() {
        *b = map[*b as usize];
    }
    for (c, _, terms, _) in s.closures.iter_mut() {
        *c = map[*c as usize];
        for x in terms.iter_mut() {
            *x = rn(*x);
        }
    }
    s.nslots = orig.len() as u32;
    s.orig = orig;
}
