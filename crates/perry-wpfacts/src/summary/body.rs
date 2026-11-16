//! Inert encoding of a sealed body input. No live solver state is available
//! here: solved-value snapshots and global ownership choices belong to the
//! batch adapter, not to this construction interface.
use super::{
    call_nodes, cons_fields, is_slot, map_call, map_cons, read_dst, Summary, CALL, CAP_OWN, SLOT,
};
use crate::model::{CallId, CallSite, Cons, FnIdx, Key, NodeId, TmplId, Tok, NO_NODE};
use crate::pts::{FxHashMap, FxHashSet};
use super::REACH_ALL;

/// An environment operand addresses the supplied lexical port, not its
/// current contents. Absolute operands retain external node identity.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Operand {
    Slot(u32),
    Absolute(NodeId),
    Environment(u32),
}

impl Operand {
    fn lower(self, environment: &[u32]) -> NodeId {
        match self {
            Self::Slot(i) => SLOT | i,
            Self::Absolute(n) => n,
            Self::Environment(i) => SLOT | environment[i as usize],
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub(crate) enum CallOperand {
    Template(u32),
    Absolute(CallId),
}

impl CallOperand {
    fn lower(self) -> CallId {
        match self {
            Self::Template(i) => CALL | i,
            Self::Absolute(k) => k,
        }
    }
}

/// The rule's integer fields index these typed operands. Keeping the rule
/// shape uses the exhaustive Cons mapper without a second rule inventory.
pub(crate) struct BodyCons {
    rule: Cons,
    nodes: Vec<Operand>,
    calls: Vec<CallOperand>,
}

impl BodyCons {
    pub(crate) fn birth_field(key: Key, src: Operand) -> Self {
        Self::capture(
            &Cons::BirthField { key, src: 0 },
            |_| src,
            |_| unreachable!(),
        )
    }

    pub(crate) fn capture(
        rule: &Cons,
        mut node: impl FnMut(NodeId) -> Operand,
        mut call: impl FnMut(CallId) -> CallOperand,
    ) -> Self {
        let mut nodes = Vec::new();
        let mut calls = Vec::new();
        let rule = map_cons(
            rule,
            |n| {
                let i = nodes.len() as u32;
                nodes.push(node(n));
                i
            },
            |k| {
                let i = calls.len() as u32;
                calls.push(call(k));
                i
            },
        );
        Self { rule, nodes, calls }
    }

    fn lower(&self, environment: &[u32]) -> Cons {
        map_cons(
            &self.rule,
            |i| self.nodes[i as usize].lower(environment),
            |i| self.calls[i as usize].lower(),
        )
    }
}

pub(crate) struct BodyCall {
    call: CallSite,
    nodes: Vec<Operand>,
}

impl BodyCall {
    pub(crate) fn capture(call: &CallSite, mut node: impl FnMut(NodeId) -> Operand) -> Self {
        let mut nodes = Vec::new();
        let call = map_call(call, |n| {
            let i = nodes.len() as u32;
            nodes.push(node(n));
            i
        });
        Self { call, nodes }
    }

    fn lower(&self, environment: &[u32]) -> CallSite {
        map_call(&self.call, |i| self.nodes[i as usize].lower(environment))
    }
}

/// Complete owned input to one encoding operation. Seeds are supplied
/// explicitly; encoding cannot discover seeds by reading a caller or model.
/// `orig` identifies observation nodes only and is never dereferenced here.
/// The batch adapter supplies its existing global plan and solved snapshots;
/// that adapter does not establish body-local generation or admission.
pub(crate) struct BodyInput {
    pub orig: Vec<NodeId>,
    pub environment: Vec<u32>,
    pub params: Vec<Operand>,
    pub ret: Operand,
    pub this: Operand,
    /// The rest parameter's local (`Absolute(NO_NODE)`: none).
    pub rest: Operand,
    pub seeds: Vec<(u32, Vec<Tok>, u16, bool)>,
    pub edges: Vec<(Operand, Operand)>,
    pub cons: Vec<(Operand, BodyCons)>,
    pub calls: Vec<(CallId, BodyCall)>,
    pub closures: Vec<(u32, FnIdx, Vec<Operand>, u8)>,
    pub births: Vec<(u32, TmplId)>,
    /// Rule M1 reach masks, parallel to `edges`, `cons`, `calls` (empty:
    /// every run's; `Summary::edge_reach`).
    pub edge_reach: Vec<u64>,
    pub cons_reach: Vec<u64>,
    pub call_reach: Vec<u64>,
}

impl BodyInput {
    pub(crate) fn new(locals: &[NodeId], outer: &[NodeId]) -> Self {
        Self {
            orig: locals.iter().chain(outer).copied().collect(),
            environment: (locals.len() as u32..(locals.len() + outer.len()) as u32).collect(),
            params: Vec::new(),
            ret: Operand::Absolute(NO_NODE),
            this: Operand::Absolute(NO_NODE),
            rest: Operand::Absolute(NO_NODE),
            seeds: Vec::new(),
            edge_reach: Vec::new(),
            cons_reach: Vec::new(),
            call_reach: Vec::new(),
            edges: Vec::new(),
            cons: Vec::new(),
            calls: Vec::new(),
            closures: Vec::new(),
            births: Vec::new(),
        }
    }
}

/// Encode and compress without executing an edge, rule, binding or seed.
/// Publication and all effects require a separate live-model operation.
pub(crate) fn encode_body(input: BodyInput) -> Summary {
    let lower = |x: Operand| x.lower(&input.environment);
    // Rule M1: an edge's mask joins its copies' (one without a mask is
    // every run's).
    let mut edge_reach: FxHashMap<(NodeId, NodeId), u64> = FxHashMap::default();
    if !input.edge_reach.is_empty() {
        let mut all: FxHashSet<(NodeId, NodeId)> = FxHashSet::default();
        for (&(a, b), &m) in input.edges.iter().zip(&input.edge_reach) {
            let e = (lower(a), lower(b));
            if m == REACH_ALL {
                all.insert(e);
                edge_reach.remove(&e);
            } else if !all.contains(&e) {
                *edge_reach.entry(e).or_insert(0) |= m;
            }
        }
    }
    let mut s = Summary {
        nslots: input.orig.len() as u32,
        params: input.params.into_iter().map(lower).collect(),
        ret: lower(input.ret),
        this: lower(input.this),
        rest: lower(input.rest),
        seeds: input.seeds,
        edges: input
            .edges
            .into_iter()
            .map(|(a, b)| (lower(a), lower(b)))
            .collect(),
        cons: input
            .cons
            .into_iter()
            .map(|(h, c)| (lower(h), c.lower(&input.environment)))
            .collect(),
        calls: input
            .calls
            .into_iter()
            .map(|(k, c)| (k, c.lower(&input.environment)))
            .collect(),
        closures: input
            .closures
            .into_iter()
            .map(|(sl, f, terms, cap)| (sl, f, terms.into_iter().map(lower).collect(), cap))
            .collect(),
        births: input.births,
        orig: input.orig,
        outer: input.environment,
        edge_reach,
        cons_reach: input.cons_reach,
        call_reach: input.call_reach,
        ..Default::default()
    };
    compress(&mut s);
    s.maker = returns_own_closure(&s);
    s
}

/// Rule C: the result of `s` is a closure it creates over its own frame
/// (`CAP_OWN`), the closure value itself or a copy of it.
fn returns_own_closure(s: &Summary) -> bool {
    if !is_slot(s.ret) {
        return false;
    }
    let mut seen: FxHashSet<NodeId> = s
        .closures
        .iter()
        .filter(|c| c.3 == CAP_OWN)
        .map(|c| SLOT | c.0)
        .collect();
    if seen.is_empty() {
        return false;
    }
    let mut succ: FxHashMap<NodeId, Vec<NodeId>> = FxHashMap::default();
    for &(a, b) in &s.edges {
        succ.entry(a).or_default().push(b);
    }
    let mut work: Vec<NodeId> = seen.iter().copied().collect();
    while let Some(x) = work.pop() {
        if x == s.ret {
            return true;
        }
        for &y in succ.get(&x).map(|v| v.as_slice()).unwrap_or(&[]) {
            if seen.insert(y) {
                work.push(y);
            }
        }
    }
    false
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
    for &p in s.params.iter().chain([&s.ret, &s.this, &s.rest]) {
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
    for &(i, _, _, _) in &s.seeds {
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
                        // Rule M1: the relayed edge runs where both do.
                        if !s.edge_reach.is_empty() {
                            let mask = |e: (NodeId, NodeId)| {
                                s.edge_reach.get(&e).copied().unwrap_or(REACH_ALL)
                            };
                            let m = mask((a, me)) & mask((me, b));
                            let had = match (sl(a), sl(b)) {
                                (None, None) => abs_edges.contains(&(a, b)),
                                (Some(i), _) => out[i].contains(&b),
                                (None, Some(j)) => inn[j].contains(&a),
                            };
                            match s.edge_reach.get_mut(&(a, b)) {
                                Some(e) => *e |= m,
                                None if !had && m != REACH_ALL => {
                                    s.edge_reach.insert((a, b), m);
                                }
                                None => {}
                            }
                        }
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
    // Rule M1: the masks of the edges that stay, in the new numbering.
    if !s.edge_reach.is_empty() {
        let old = std::mem::take(&mut s.edge_reach);
        for (&(a, b), &m) in &old {
            if sl(a).is_some_and(|i| dead[i]) || sl(b).is_some_and(|i| dead[i]) {
                continue;
            }
            *s.edge_reach.entry((rn(a), rn(b))).or_insert(0) |= m;
        }
    }
    s.edges = edges;
    if !s.cons_reach.is_empty() {
        let cr = std::mem::take(&mut s.cons_reach);
        s.cons_reach = cr
            .into_iter()
            .zip(cons.iter())
            .filter(|(_, c)| c.is_some())
            .map(|(m, _)| m)
            .collect();
    }
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
        .filter(|(i, _, _, _)| !dead[*i as usize])
        .map(|(i, t, p, refuted)| (map[i as usize], t, p, refuted))
        .collect();
    s.params = s.params.iter().map(|&p| rn(p)).collect();
    s.ret = rn(s.ret);
    s.this = rn(s.this);
    s.rest = rn(s.rest);
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
