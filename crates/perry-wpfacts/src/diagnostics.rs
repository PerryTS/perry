//! Observational dump data. Never add constraints, nodes, tokens or proof inputs here.
use crate::gen::{Fx, Gen};
use crate::ledger::Unknown;
use crate::model::*;
use crate::pts::FxHashMap;
use perry_hir::Expr;
use std::collections::{BTreeMap, BTreeSet};

#[derive(Default)]
pub(crate) struct Trace {
    pub census: Option<BirthCensus>,
    pub pred: BTreeMap<NodeId, (NodeId, String)>,
    pub contract_inputs: BTreeMap<NodeId, (String, String)>,
    pub seeds: BTreeMap<NodeId, String>,
    pub bound_origins: BTreeMap<u32, CallId>,
    pub source_calls: BTreeSet<CallId>,
    pub raw: BTreeMap<CallId, BTreeSet<Tok>>,
    pub builtins: BTreeMap<CallId, BTreeSet<String>>,
    /// The tokens whose `Model::escape` is running, innermost last.
    pub escaping: Vec<Tok>,
    /// The call whose transfer is running (`Model::diagnostic_callee`).
    pub cur_call: Option<CallId>,
    /// The node whose rule is being applied (`Model::apply_tokens`).
    pub cause_node: Option<NodeId>,
    /// Why each `TopSink` node sinks: the token whose escape made it, the
    /// call, and the function generation was walking.
    pub sink_by: BTreeMap<NodeId, SinkBy>,
    /// Each token's first escape (where it first became escaped).
    pub esc_first: BTreeMap<Tok, EscFirst>,
    /// EUC1 (dump only): each call bound to an unknown callee, with the callee node whose rule bound it.
    pub unknown_calls: BTreeMap<CallId, (Option<NodeId>, bool)>,
    /// Every (token, cause) of an escape attempt, taking effect or not.
    pub esc_causes: BTreeSet<(Tok, Cause)>,
    /// Every (function, transfer kind, cause) that reached a body, with the
    /// first call it was seen at.
    pub reach: BTreeMap<(FnIdx, &'static str, Cause), Option<CallId>>,
    /// A summary instance's fresh slot node -> the summary's own node of
    /// that slot: what an instance holds without a recorded predecessor it
    /// holds as its summary does.
    pub inst_orig: BTreeMap<NodeId, NodeId>,
    /// UEB census: the operation each parameter marker first escaped by
    /// (`Model::mark_escape_why`), and the stack of those running.
    pub esc_why: BTreeMap<Tok, (String, Option<NodeId>)>,
    pub esc_ctx: Vec<String>,
    /// UEB census: a stand-in function's parameter node its contract sinks
    /// (node, callee, index, reach key: `Some("")` the whole reach).
    pub stand_in_params: Vec<(NodeId, String, u32, Option<String>)>,
    /// `Model::top_predecessors`, built when a chain needs it.
    pub top_in: std::cell::OnceCell<FxHashMap<NodeId, Vec<NodeId>>>,
    /// ROOTS1: the escaped function whose entry run (unknown code calls
    /// it: `this`/parameters ⊤) is seeding, innermost last.
    pub seed_by: Vec<Tok>,
    /// ROOTS1: a ⊤ seed made while the escape of a token ran (an entry run
    /// of an escaped function, a field of an escaped object): that token.
    pub top_by: BTreeMap<NodeId, Tok>,
    /// ROOTS2: the escaped object a ⊤ seed is made into the fields of
    /// (`Model::add_top_held`): the seed is that object's escape's, also
    /// when it is made outside that escape (from the worklist).
    pub seed_holder: Vec<Tok>,
    /// ROOTS2: the callee token of each call bound to an unknown callee
    /// (`TOP` or an undescribed builtin function), first one per call.
    pub unknown_callee: BTreeMap<CallId, Tok>,
    /// ROOTS1: each entry run from unknown code: function -> (its token,
    /// reason, the escaping token that held it, if any).
    pub entry_runs: BTreeMap<FnIdx, (Tok, u32, Option<Tok>)>,
    /// ROOTS1: a throw region joined to ESCRAISE -> the escaped function.
    pub raise_by: BTreeMap<NodeId, Tok>,
    /// ROOTS1: a walk node joined to ESCACCESSORS -> the escaped holder.
    pub acc_src: BTreeMap<NodeId, Tok>,
    /// ROOTS1: a sunk node (`Model::sink_node`: what reaches it goes to
    /// unknown code) -> the node whose rule sank it (an unknown callee or
    /// receiver, a prototype chain holding ⊤).
    pub store_top: BTreeMap<NodeId, NodeId>,
}

/// Diagnostic data only; never consulted by ordinary facts.
#[derive(Default)]
pub(crate) struct BirthCensus {
    pub injections: BTreeSet<(
        NodeId,
        String,
        u32,
        String,
        NodeId,
        u32,
        Tok,
        u32,
        Option<(Tok, Atom)>,
    )>,
    pub top_edges: BTreeSet<(NodeId, NodeId)>,
    pub escapes: BTreeSet<(Tok, NodeId, u32, Tok, u32, Option<(Tok, Atom)>)>,
    pub sinks: BTreeSet<(NodeId, NodeId, u32, Tok, u32, Option<(Tok, Atom)>)>,
    /// (token, key, source node, call, parent, proto): `proto` is the prototype value node
    /// a `settle_native_given` write is made for (`NO_NODE` for every other write).
    pub writes: BTreeSet<(Tok, u32, NodeId, u32, Tok, NodeId)>,
    /// The operation that gave `t` the prototype `src` (`Model::given_prototype`): its cause node and call.
    pub given: BTreeMap<(Tok, NodeId), (NodeId, u32)>,
    /// The prototype whose delayed give `settle_native_given` is writing (census only).
    pub settling_proto: Option<NodeId>,
    pub reads: BTreeSet<(Tok, Atom, bool, u32)>,
    /// The receiver a missed intrinsic method call was looked up on, by call.
    pub read_origins: BTreeMap<u32, Tok>,
    pub pending_gate: Option<(Tok, Atom, bool, u32)>,
    pub active_gate: Option<(Tok, Atom)>,
    pub injecting: bool,
    pub suppress: Option<String>,
    pub suppressed: usize,
    /// The first write that opened each native-write gate (holder, key or
    /// `u32::MAX` for every key), in the order the solver opened them.
    pub openers: BTreeMap<(Tok, u32), Opener>,
    /// One clock for every census event that may cause a later one: gate
    /// openers, first ⊤ arrivals, first escapes. A cause chain follows
    /// strictly smaller numbers, so it ends at a seed, never in a loop.
    pub seq: u32,
    /// Each node's first ⊤ arrival (`birth-arrival`).
    pub arrivals: BTreeMap<NodeId, Arrival>,
    /// The node an injection (`add_top`/`add_tok`) is seeding, with the
    /// gate it replays: its arrival names no copy source.
    pub inject_at: Option<(NodeId, Option<(Tok, Atom)>)>,
    /// The store (site, receiver node) whose rule is running.
    pub store_ctx: Option<(u32, Option<NodeId>)>,
}

/// A node's first ⊤ (`birth-arrival`): the copy source it came from, or the
/// rule context of the injection that seeded it.
pub(crate) struct Arrival {
    pub seq: u32,
    pub src: NodeId,
    pub cause: NodeId,
    pub call: u32,
    pub parent: Tok,
    pub injected: bool,
    pub gate: Option<(Tok, Atom)>,
}

/// The write that first opened a gate (`birth-opener`).
pub(crate) struct Opener {
    pub seq: u32,
    pub what: String,
    pub site: u32,
    pub recv: NodeId,
    pub cause: NodeId,
    pub call: u32,
    pub parent: Tok,
    pub func: FnIdx,
}

impl BirthCensus {
    pub fn tick(&mut self) -> u32 {
        self.seq += 1;
        self.seq
    }
    pub fn node_gate(&mut self, n: NodeId) -> Option<(Tok, Atom)> {
        self.active_gate.or_else(|| {
            let (t, a, call, id) = self.pending_gate?;
            if !call && id == n {
                self.pending_gate = None;
                Some((t, a))
            } else {
                None
            }
        })
    }
}

impl Model {
    pub(crate) fn census_native_read(&mut self, t: Tok, a: Atom, miss: NativeMiss) {
        if !self.diagnostic.as_ref().is_some_and(|d| d.census.is_some()) {
            return;
        }
        let written = crate::builtins::census_native_written(self, t, a);
        let c = self.diagnostic.as_mut().unwrap().census.as_mut().unwrap();
        let (call, id) = match miss {
            NativeMiss::Load(n)
            | NativeMiss::Lookup(n, _)
            | NativeMiss::Global(n, _)
            | NativeMiss::Store(n, _) => (false, n),
            NativeMiss::Call(_, n) | NativeMiss::PrimCall(_, n) | NativeMiss::New(n) => (true, n),
        };
        if let NativeMiss::Call(origin, n) = miss {
            c.read_origins.insert(n, origin);
        }
        c.reads.insert((t, a, call, id));
        c.pending_gate = written.then_some((t, a, call, id));
    }
    /// The first write that opens gate (`t`, `k`): `note_native_write` for
    /// a builtin prototype, ⊤ or a namespace. A store names its site and
    /// receiver node; ordinary function-object fields open no global gate.
    pub(crate) fn census_opener(
        &mut self,
        t: Tok,
        k: Option<Atom>,
        what: &str,
        store: Option<(u32, Option<NodeId>)>,
    ) {
        let func = self.cur_fn;
        let Some(d) = self.diagnostic.as_mut() else {
            return;
        };
        let Some(c) = d.census.as_mut() else {
            return;
        };
        let key = (t, k.unwrap_or(u32::MAX));
        if c.openers.contains_key(&key) {
            return;
        }
        let (site, recv) = store.or(c.store_ctx).unwrap_or((u32::MAX, None));
        let seq = c.tick();
        c.openers.insert(
            key,
            Opener {
                seq,
                what: what.into(),
                site,
                recv: recv.unwrap_or(NO_NODE),
                cause: d.cause_node.unwrap_or(NO_NODE),
                call: d.cur_call.unwrap_or(u32::MAX),
                parent: d.escaping.last().copied().unwrap_or(u32::MAX),
                func,
            },
        );
    }
    /// The store rule's site while it notes a native write (census only).
    pub(crate) fn census_store_ctx(&mut self, ctx: Option<(u32, Option<NodeId>)>) {
        if let Some(c) = self.diagnostic.as_mut().and_then(|d| d.census.as_mut()) {
            c.store_ctx = ctx;
        }
    }
    pub(crate) fn census_native_write(&mut self, t: Tok, k: Option<Atom>) {
        let Some(d) = self.diagnostic.as_mut() else {
            return;
        };
        let Some(c) = d.census.as_mut() else {
            return;
        };
        c.writes.insert((
            t,
            k.unwrap_or(u32::MAX),
            d.cause_node.unwrap_or(NO_NODE),
            d.cur_call.unwrap_or(u32::MAX),
            d.escaping.last().copied().unwrap_or(u32::MAX),
            c.settling_proto.unwrap_or(NO_NODE),
        ));
    }
    /// The operation giving `t` the prototype `src`, the first time (census only).
    pub(crate) fn census_given(&mut self, t: Tok, src: NodeId) {
        let Some(d) = self.diagnostic.as_mut() else {
            return;
        };
        let Some(c) = d.census.as_mut() else {
            return;
        };
        c.given.entry((t, src)).or_insert((
            d.cause_node.unwrap_or(NO_NODE),
            d.cur_call.unwrap_or(u32::MAX),
        ));
    }
    /// What gave `t` the prototype `src`: (cause node, call), `None` when unrecorded.
    pub(crate) fn census_given_by(&self, t: Tok, src: NodeId) -> Option<(NodeId, u32)> {
        let c = self.diagnostic.as_ref()?.census.as_ref()?;
        c.given.get(&(t, src)).copied()
    }
    /// Mark the writes made next as the delayed give of `proto` (census only).
    pub(crate) fn census_settling(&mut self, proto: Option<NodeId>) {
        if let Some(c) = self.diagnostic.as_mut().and_then(|d| d.census.as_mut()) {
            c.settling_proto = proto;
        }
    }
}

/// The operation an event belongs to: a function body's (`Fn`), a token's
/// escape (`Tok`), or none known (`Unknown`).
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub(crate) enum Cause {
    Fn(FnIdx),
    Tok(Tok),
    Unknown,
}

#[derive(Clone, Copy)]
pub(crate) struct SinkBy {
    pub by: Option<Tok>,
    pub call: Option<CallId>,
    pub gen_fn: FnIdx,
}

pub(crate) struct EscFirst {
    pub reason: u32,
    pub cause: Cause,
    pub call: Option<CallId>,
    pub node: Option<NodeId>,
    /// The census clock when this escape began (census only).
    pub seq: Option<u32>,
}

#[derive(Default)]
pub(crate) struct Capture {
    pub values: FxHashMap<usize, NodeId>,
    pub calls: FxHashMap<NodeId, CallId>,
    pub namespace_receivers: BTreeSet<NodeId>,
    sites: Vec<Site>,
}
struct Site {
    module: u32,
    function: FnIdx,
    start: u32,
    end: u32,
    kind: &'static str,
    key: String,
    origin: &'static str,
    receiver: Option<NodeId>,
    index: Option<NodeId>,
    callee: Option<NodeId>,
    result: NodeId,
    call: Option<CallId>,
    /// The HIR expression's address (`ledger::census` compares the
    /// recorded sites with the HIR).
    addr: usize,
}

impl Model {
    /// ROOTS1: unknown code calls escaped `ft` (an entry run): what its
    /// run seeds is that escape's.
    pub(crate) fn diagnostic_entry_begin(&mut self, ft: Tok, reason: u32) {
        if self.diagnostic.is_none() {
            return;
        }
        let f = self.tok_fn(ft);
        let d = self.diagnostic.as_mut().unwrap();
        let holder = d.escaping.iter().rev().find(|&&x| x != ft).copied();
        d.entry_runs.entry(f).or_insert((ft, reason, holder));
        d.seed_by.push(ft);
    }
    pub(crate) fn diagnostic_entry_end(&mut self) {
        if let Some(d) = &mut self.diagnostic {
            d.seed_by.pop();
        }
    }
    /// ROOTS2: a ⊤ seed into a field of escaped `holder`.
    #[track_caller]
    pub(crate) fn add_top_held(
        &mut self,
        n: NodeId,
        holder: Tok,
        why: impl FnOnce(&Self) -> String,
    ) {
        if let Some(d) = &mut self.diagnostic {
            d.seed_holder.push(holder);
        }
        self.add_top(n, why);
        if let Some(d) = &mut self.diagnostic {
            d.seed_holder.pop();
        }
    }
    /// ROOTS2: the call ids a reason names (`[call N]`, `[call N,M]`).
    pub(crate) fn reason_calls(&self, reason: u32) -> Vec<CallId> {
        let s = self.reasons.get(reason as usize).map_or("", |s| s);
        let Some(i) = s.rfind("[call ") else {
            return Vec::new();
        };
        s[i + 6..]
            .split(']')
            .next()
            .unwrap_or("")
            .split(',')
            .filter_map(|x| x.trim().parse().ok())
            .collect()
    }
    /// ROOTS1: the escape an escape-first record's unknown value came out
    /// of: the token whose escape seeded the ⊤ at its deciding node (first
    /// arrivals walked back to the seed), the holder an escaped accessor was
    /// read from, the function whose throw region reached ESCRAISE. Dump only.
    fn diagnostic_esc_via(&self, t: Tok, e: &EscFirst) -> (Option<Tok>, String) {
        let Some(d) = self.diagnostic.as_ref() else {
            return (None, "null".into());
        };
        let reason = self.reasons.get(e.reason as usize).map_or("", |s| s);
        let tops = |n: NodeId| {
            n != NO_NODE && (n as usize) < self.nodes.len() && self.nodes[n as usize].pts.contains_tok(TOP)
        };
        // The deciding node: the rule that sank the escape's node, the node
        // itself, the unknown callee of its call; the first holding ⊤.
        // ROOTS2: the receiver of (or a value passed to) a call of an
        // unknown function value escapes because the callee is unknown: the
        // callee decides, not what else the value's node holds. A callee
        // that is an undescribed builtin function is the end itself.
        let callee_first = true; /*ROOTS2-GREEN*/
        let mut callee_dn = None;
        if callee_first
            && (reason.starts_with("receiver of a call of an unknown function value")
                || reason.starts_with("passed to a call of an unknown function value"))
        {
            for c in self.reason_calls(e.reason) {
                match d.unknown_calls.get(&c) {
                    Some(&(Some(n), true)) if tops(n) => {
                        callee_dn = Some(n);
                        break;
                    }
                    Some(&(n, false)) => {
                        let ct = d.unknown_callee.get(&c).copied();
                        return (
                            None,
                            format!(
                                "{{\"calleeBuiltin\":{},\"at\":{},\"call\":{c},\"atFunction\":{}}}",
                                opt(ct),
                                opt(n),
                                opt(n.and_then(|n| self.diagnostic_owner(n)))
                            ),
                        );
                    }
                    _ => {}
                }
            }
        }
        let dn = callee_dn.or_else(|| {
            [
                e.node.and_then(|n| d.store_top.get(&n).copied()),
                e.node,
                e.call.and_then(|c| d.unknown_calls.get(&c)).and_then(|(n, _)| *n),
            ]
            .into_iter()
            .flatten()
            .find(|&n| tops(n))
        });
        if let Some(n) = dn {
            let (seed, why) = self.diagnostic_root(n);
            let by = d.top_by.get(&seed).copied();
            let entry = by.and_then(|b| self.tok_fn_opt(b));
            return (
                by,
                format!(
                    "{{\"at\":{n},\"seed\":{seed},\"why\":{},\"by\":{},\"entryOf\":{}}}",
                    quote(&why),
                    by.map_or("null".into(), |b| b.to_string()),
                    entry.map_or("null".into(), |f| f.to_string())
                ),
            );
        }
        if reason == "accessor function of a value unknown code holds" {
            for (&n, &o) in &d.acc_src {
                if self.nodes[n as usize].pts.contains_tok(t) {
                    return (Some(o), format!("{{\"accessorOf\":{o}}}"));
                }
            }
        }
        if reason == "thrown into unknown code" {
            for (&n, &f) in &d.raise_by {
                if self.nodes[n as usize].pts.contains_tok(t) {
                    return (Some(f), format!("{{\"raisedBy\":{f}}}"));
                }
            }
        }
        (None, "null".into())
    }
    fn tok_fn_opt(&self, t: Tok) -> Option<FnIdx> {
        Some(self.tok_fn(t)).filter(|&f| f != NO_FN)
    }
    pub(crate) fn diagnostic_predecessor(&mut self, dst: NodeId, src: NodeId, reason: u32) {
        let ruled = src == NO_NODE
            || self
                .reasons
                .get(reason as usize)
                .is_some_and(|s| !s.is_empty());
        if let Some(d) = &mut self.diagnostic {
            if ruled && !d.pred.contains_key(&dst) {
                if let Some(&b) = d.seed_by.last().or(d.escaping.last()) {
                    d.top_by.entry(dst).or_insert(b);
                }
            }
            if let Some(c) = &mut d.census {
                if !c.arrivals.contains_key(&dst) {
                    let inj = c.inject_at.take().filter(|(n, _)| *n == dst);
                    let seq = c.tick();
                    c.arrivals.insert(
                        dst,
                        Arrival {
                            seq,
                            src: if inj.is_some() { NO_NODE } else { src },
                            cause: d.cause_node.unwrap_or(NO_NODE),
                            call: d.cur_call.unwrap_or(u32::MAX),
                            parent: d.escaping.last().copied().unwrap_or(u32::MAX),
                            injected: inj.is_some(),
                            gate: inj.and_then(|(_, g)| g),
                        },
                    );
                }
            }
            d.pred.entry(dst).or_insert_with(|| {
                (
                    src,
                    self.reasons
                        .get(reason as usize)
                        .cloned()
                        .unwrap_or_default(),
                )
            });
        }
    }
    /// The origin text of a ⊤ whose chain has no recorded seed: the node it
    /// ended at (its label) and how the chain broke. `ledger::classify`
    /// reads it as `untraced-top`; the ledger reports the labels.
    fn untraced(&self, n: NodeId, what: &str) -> String {
        let label = self.labels.get(n as usize).copied().unwrap_or("");
        format!("untraced: {what} at {label}")
    }

    /// The ⊤-holding nodes with a copy edge into each node (built once per
    /// model, only for a chain that has no recorded arrival).
    fn top_predecessors(&self) -> &FxHashMap<NodeId, Vec<NodeId>> {
        let d = self.diagnostic.as_ref().expect("a diagnostic model");
        d.top_in.get_or_init(|| {
            let mut m: FxHashMap<NodeId, Vec<NodeId>> = FxHashMap::default();
            for (s, v) in self.nodes.iter().enumerate() {
                if v.pts.contains_tok(TOP) {
                    for &b in &v.succ {
                        m.entry(b).or_default().push(s as NodeId);
                    }
                }
            }
            for v in m.values_mut() {
                v.sort_unstable();
                v.dedup();
            }
            m
        })
    }

    fn diagnostic_root(&self, start: NodeId) -> (NodeId, String) {
        let Some(d) = &self.diagnostic else {
            return (start, String::new());
        };
        let mut n = start;
        let mut seen = BTreeSet::new();
        // The node that led the walk to `n`.
        let mut prev: Option<NodeId> = None;
        // An instance slot with no recorded arrival holds what its
        // summary's node holds (a slot without one, `NO_NODE`, ends here).
        let summary = |n: NodeId| {
            d.inst_orig.get(&n).copied().filter(|&o| {
                o != n
                    && self
                        .nodes
                        .get(o as usize)
                        .is_some_and(|v| v.pts.contains_tok(TOP))
            })
        };
        // Where the first arrival is not recorded (a set adopted whole, a
        // seed copied from a summary, an edge added before the value): the
        // lowest ⊤-holding node with a copy edge into `at` that the walk has
        // not been through holds it as well.
        let other = |at: NodeId, seen: &BTreeSet<NodeId>| {
            self.top_predecessors()
                .get(&at)
                .and_then(|v| v.iter().copied().find(|s| !seen.contains(s)))
        };
        loop {
            if !seen.insert(n) {
                // A chain that came back to a node: leave through another
                // predecessor of the node that led here.
                match prev.and_then(|p| other(p, &seen).map(|o| (p, o))) {
                    Some((p, o)) => {
                        n = o;
                        prev = Some(p);
                        continue;
                    }
                    None => return (n, "cycle".into()),
                }
            }
            if let Some(why) = d.seeds.get(&n) {
                return (n, why.clone());
            }
            let Some((src, why)) = d.pred.get(&n) else {
                if let Some(o) = summary(n) {
                    prev = Some(n);
                    n = o;
                    continue;
                }
                match other(n, &seen) {
                    Some(o) => {
                        prev = Some(n);
                        n = o;
                        continue;
                    }
                    None => return (n, self.untraced(n, "no arrival recorded")),
                }
            };
            if !why.is_empty() {
                return (n, why.clone());
            }
            if *src == NO_NODE || !self.nodes[*src as usize].pts.contains_tok(TOP) {
                if let Some(o) = summary(n) {
                    prev = Some(n);
                    n = o;
                    continue;
                }
                match other(n, &seen) {
                    Some(o) => {
                        prev = Some(n);
                        n = o;
                        continue;
                    }
                    None => {
                        let what = if *src == NO_NODE {
                            "arrival outside a rule"
                        } else {
                            "source no longer holds it"
                        };
                        return (n, self.untraced(n, what));
                    }
                }
            }
            prev = Some(n);
            n = *src;
        }
    }

    /// A summary instance keeps its template call identity; a call the
    /// solver makes for a program call (a builtin calling a callback, a
    /// bound target) reports that program call (`Model::solver_call`).
    /// `CallSite::site` is an allocation context, and synthetic protocol
    /// calls may use zero: it must never be used to attribute their callees
    /// to source call zero.
    pub(crate) fn diagnostic_source_call(&self, call: CallId) -> Option<CallId> {
        let d = self.diagnostic.as_ref()?;
        let mut c = call;
        loop {
            if d.source_calls.contains(&c) {
                return Some(c);
            }
            // Each step goes to an earlier call: the template's, or the one
            // the solver made this call for.
            let next = *self
                .sum
                .tmpl_of
                .get(&c)
                .or_else(|| self.sum.origin.get(&c))?;
            if next >= c {
                return None;
            }
            c = next;
        }
    }
    /// The function a node's rules run for: its body's owner, or the
    /// function of the run (instance or shared run) it belongs to.
    pub(crate) fn diagnostic_owner(&self, n: NodeId) -> Option<FnIdx> {
        if n == NO_NODE || n as usize >= self.nodes.len() {
            return None;
        }
        let f = self.node_fn[n as usize];
        if f != NO_FN {
            return Some(f);
        }
        let fr = self.nodes[n as usize].frame;
        if fr == 0 {
            None
        } else if fr & FN_RUN != 0 {
            Some(fr & !FN_RUN)
        } else {
            self.sum.insts.get((fr - 1) as usize).map(|i| i.f)
        }
    }
    fn diagnostic_call_owner(&self, call: Option<CallId>) -> Option<FnIdx> {
        let c = call?;
        let cs = self.calls.get(c as usize)?;
        self.diagnostic_owner(cs.ret).or_else(|| {
            let src = self.diagnostic_source_call(c)?;
            self.diagnostic_owner(self.calls.get(src as usize)?.ret)
        })
    }
    fn diagnostic_sink_cause(&self, n: Option<NodeId>) -> Option<Cause> {
        let d = self.diagnostic.as_ref()?;
        let s = d.sink_by.get(&n?)?;
        if let Some(t) = s.by {
            return Some(Cause::Tok(t));
        }
        if let Some(f) = self.diagnostic_call_owner(s.call) {
            return Some(Cause::Fn(f));
        }
        (s.gen_fn != NO_FN).then_some(Cause::Fn(s.gen_fn))
    }
    /// The operation an escape attempt now belongs to.
    fn diagnostic_escape_cause(&self) -> Cause {
        let Some(d) = self.diagnostic.as_ref() else {
            return Cause::Unknown;
        };
        if let Some(f) = d.cause_node.and_then(|n| self.diagnostic_owner(n)) {
            return Cause::Fn(f);
        }
        if let Some(&t) = d.escaping.last() {
            return Cause::Tok(t);
        }
        if let Some(c) = self.diagnostic_sink_cause(d.cause_node) {
            return c;
        }
        if let Some(f) = self.diagnostic_call_owner(d.cur_call) {
            return Cause::Fn(f);
        }
        if self.cur_fn != NO_FN {
            return Cause::Fn(self.cur_fn);
        }
        Cause::Unknown
    }
    /// The operation a transfer into a body (by `call`) belongs to.
    fn diagnostic_reach_cause(&self, call: Option<CallId>) -> Cause {
        let Some(d) = self.diagnostic.as_ref() else {
            return Cause::Unknown;
        };
        if let Some(f) = self.diagnostic_call_owner(call) {
            return Cause::Fn(f);
        }
        if let Some(&t) = d.escaping.last() {
            return Cause::Tok(t);
        }
        if let Some(f) = d.cause_node.and_then(|n| self.diagnostic_owner(n)) {
            return Cause::Fn(f);
        }
        if let Some(c) = self.diagnostic_sink_cause(d.cause_node) {
            return c;
        }
        if self.cur_fn != NO_FN {
            return Cause::Fn(self.cur_fn);
        }
        Cause::Unknown
    }
    /// Escape of `t` begins: the cause of this attempt, and of its first
    /// escape when it takes effect.
    pub(crate) fn diagnostic_escape_begin(&mut self, t: Tok, reason: u32) -> Option<EscFirst> {
        self.diagnostic.as_ref()?;
        let was = self.is_escaped(t);
        let cause = self.diagnostic_escape_cause();
        // ROOTS2: the call the reason names, else the current one.
        let rc = self.reason_calls(reason).first().copied();
        let d = self.diagnostic.as_mut()?;
        d.esc_causes.insert((t, cause));
        if let Some(c) = &mut d.census {
            c.escapes.insert((
                t,
                d.cause_node.unwrap_or(NO_NODE),
                d.cur_call.unwrap_or(u32::MAX),
                d.escaping.last().copied().unwrap_or(u32::MAX),
                reason,
                c.active_gate,
            ));
        }
        let seq = if was {
            None
        } else {
            d.census.as_mut().map(|c| c.tick())
        };
        let cand = (!was).then(|| EscFirst {
            reason,
            cause,
            call: rc.or(d.cur_call),
            node: d.cause_node,
            seq,
        });
        d.escaping.push(t);
        cand
    }
    pub(crate) fn diagnostic_escape_end(&mut self, t: Tok, cand: Option<EscFirst>) {
        let now = self.is_escaped(t);
        let Some(d) = self.diagnostic.as_mut() else {
            return;
        };
        d.escaping.pop();
        if let Some(c) = cand.filter(|_| now) {
            d.esc_first.entry(t).or_insert(c);
            // Diagnostic (`PERRY_WPF_STOP_AFTER_ESCAPES=N`): the compile's
            // pass ends after its Nth first escape; the dump holds what
            // was collected so far (`aborted` true, nothing claimed).
            static STOP: std::sync::OnceLock<usize> = std::sync::OnceLock::new();
            let stop = *STOP.get_or_init(|| {
                std::env::var("PERRY_WPF_STOP_AFTER_ESCAPES")
                    .ok()
                    .and_then(|v| v.parse().ok())
                    .unwrap_or(0)
            });
            if stop != 0 && d.esc_first.len() >= stop && self.act.as_ref().is_some_and(|a| a.fixed) {
                self.aborted = true;
            }
        }
    }
    /// Marks `t` escaped (`mark_use`) by operation `why`; the dump names
    /// the first one per parameter marker (UEB census, dump only).
    pub(crate) fn mark_escape_why(&mut self, t: Tok, why: &dyn Fn(&Self) -> String) {
        if self.diagnostic.is_none() {
            return self.mark_use(t, crate::model::MARK_ESCAPE);
        }
        let w = why(self);
        self.diagnostic.as_mut().unwrap().esc_ctx.push(w);
        self.mark_use(t, crate::model::MARK_ESCAPE);
        self.diagnostic.as_mut().unwrap().esc_ctx.pop();
    }
    pub(crate) fn diagnostic_sink(&mut self, n: NodeId, reason: u32) {
        let gen_fn = self.cur_fn;
        if let Some(d) = self.diagnostic.as_mut() {
            let s = SinkBy {
                by: d.escaping.last().copied(),
                call: d.cur_call,
                gen_fn,
            };
            d.sink_by.entry(n).or_insert(s);
            if let Some(c) = &mut d.census {
                c.sinks.insert((
                    n,
                    d.cause_node.unwrap_or(NO_NODE),
                    d.cur_call.unwrap_or(u32::MAX),
                    d.escaping.last().copied().unwrap_or(u32::MAX),
                    reason,
                    c.active_gate,
                ));
            }
        }
    }
    /// A transfer reached the body of `f` (`call`: `u32::MAX` for none).
    pub(crate) fn diagnostic_reach(&mut self, f: FnIdx, kind: &'static str, call: CallId) {
        if self.diagnostic.is_none() {
            return;
        }
        let call = (call != u32::MAX).then_some(call);
        let cause = self.diagnostic_reach_cause(call);
        if let Some(d) = self.diagnostic.as_mut() {
            d.reach.entry((f, kind, cause)).or_insert(call);
        }
    }
    pub(crate) fn diagnostic_callee(&mut self, call: CallId, tok: Tok) {
        if let Some(d) = self.diagnostic.as_mut() {
            d.cur_call = Some(call);
        }
        let Some(source) = self.diagnostic_source_call(call) else {
            return;
        };
        self.diagnostic
            .as_mut()
            .unwrap()
            .raw
            .entry(source)
            .or_default()
            .insert(tok);
    }
    pub(crate) fn diagnostic_builtin(&mut self, call: CallId, name: impl FnOnce(&Self) -> String) {
        let Some(source) = self.diagnostic_source_call(call) else {
            return;
        };
        let name = name(self);
        self.diagnostic
            .as_mut()
            .unwrap()
            .builtins
            .entry(source)
            .or_default()
            .insert(name);
    }
}

impl Gen<'_> {
    pub(crate) fn diagnostic_expr(&mut self, fx: &Fx, e: &Expr, result: NodeId) {
        let Some(d) = &mut self.diagnostic else {
            return;
        };
        let get = |e: &Expr| d.values.get(&(e as *const Expr as usize)).copied();
        let (start, end, kind, key, receiver, index, callee) = match e {
            Expr::PropertyGet {
                object,
                property,
                byte_offset,
                byte_end,
            } => (
                *byte_offset,
                *byte_end,
                "property",
                property.clone(),
                get(object),
                None,
                None,
            ),
            Expr::IndexGet {
                object,
                index,
                element_offset: _,
                byte_offset,
                byte_end,
            } => (
                *byte_offset,
                *byte_end,
                "element",
                "[]".into(),
                get(object),
                get(index),
                None,
            ),
            Expr::Call {
                callee,
                byte_offset,
                byte_end,
                ..
            }
            | Expr::CallSpread {
                callee,
                byte_offset,
                byte_end,
                ..
            } => {
                let (recv, idx, key) = match callee.as_ref() {
                    Expr::PropertyGet {
                        object, property, ..
                    } => (get(object), None, property.clone()),
                    Expr::IndexGet { object, index, .. } => (get(object), get(index), "[]".into()),
                    _ => (None, None, "()".into()),
                };
                (
                    *byte_offset,
                    *byte_end,
                    "value-call",
                    key,
                    recv,
                    idx,
                    get(callee),
                )
            }
            Expr::PropertySet {
                object,
                property,
                byte_offset,
                byte_end,
                ..
            } => (
                *byte_offset,
                *byte_end,
                "write",
                property.clone(),
                get(object),
                None,
                None,
            ),
            Expr::PutValueSet {
                target,
                key,
                byte_offset,
                byte_end,
                ..
            } => match key.as_ref() {
                Expr::String(k) => (
                    *byte_offset,
                    *byte_end,
                    "write",
                    k.clone(),
                    get(target),
                    None,
                    None,
                ),
                _ => (
                    *byte_offset,
                    *byte_end,
                    "element-write",
                    "[]".into(),
                    get(target),
                    get(key),
                    None,
                ),
            },
            Expr::PropertyUpdate {
                object,
                property,
                byte_offset,
                byte_end,
                ..
            } => (
                *byte_offset,
                *byte_end,
                "update",
                property.clone(),
                get(object),
                None,
                None,
            ),
            Expr::IndexSet {
                object,
                index,
                byte_offset,
                byte_end,
                ..
            } => (
                *byte_offset,
                *byte_end,
                "element-write",
                "[]".into(),
                get(object),
                get(index),
                None,
            ),
            Expr::IndexUpdate {
                object,
                index,
                byte_offset,
                byte_end,
                ..
            } => (
                *byte_offset,
                *byte_end,
                "element-update",
                "[]".into(),
                get(object),
                get(index),
                None,
            ),
            Expr::New {
                class_name,
                byte_offset,
                byte_end,
                ..
            } => (
                *byte_offset,
                *byte_end,
                "new",
                class_name.clone(),
                None,
                None,
                None,
            ),
            Expr::NewDynamic {
                callee,
                byte_offset,
                byte_end,
                ..
            }
            | Expr::NewDynamicSpread {
                callee,
                byte_offset,
                byte_end,
                ..
            } => (
                *byte_offset,
                *byte_end,
                "new",
                "new".into(),
                None,
                None,
                get(callee),
            ),
            _ => {
                d.values.insert(e as *const Expr as usize, result);
                return;
            }
        };
        // Namespace lowering may not walk the syntactic receiver as an expression.
        let receiver = receiver.or_else(|| {
            self.read_sites
                .iter()
                .rev()
                .find(|s| s.module == fx.module && s.byte_offset == start && s.func == fx.f)
                .map(|s| s.recv)
        });
        let namespace = receiver.is_some_and(|n| d.namespace_receivers.contains(&n));
        let origin = if namespace {
            if end == 0 {
                "synthesized-namespace"
            } else {
                "namespace"
            }
        } else if start == 0 {
            "synthesized"
        } else if end == 0 {
            "transformed"
        } else {
            "source"
        };
        d.sites.push(Site {
            module: fx.module,
            function: fx.f,
            start,
            end,
            kind,
            key,
            origin,
            receiver,
            index,
            callee,
            result,
            call: d.calls.get(&result).copied(),
            addr: e as *const Expr as usize,
        });
        d.values.insert(e as *const Expr as usize, result);
    }
}

/// The HIR sites this analysis recorded: address, kind, start.
pub(crate) fn recorded(g: &Gen) -> Vec<(usize, &'static str, u32)> {
    g.diagnostic.as_ref().map_or_else(Vec::new, |d| {
        d.sites.iter().map(|s| (s.addr, s.kind, s.start)).collect()
    })
}

impl Model {
    /// LEDGER: the first unknown value of operand `n` and why it is
    /// unknown. ⊤ names the seed of its first-arrival path; a contract
    /// function's parameter marker is the unit edge; no value at all is an
    /// unexecuted or dead operand. `None`: every value is known.
    fn diagnostic_unknown(&self, n: NodeId) -> Option<(Result<Unknown, String>, NodeId)> {
        let v = &self.nodes[n as usize];
        if v.pts.contains_tok(TOP) {
            let (root, origin) = self.diagnostic_root(n);
            return Some((crate::ledger::classify(&origin).ok_or(origin), root));
        }
        let toks = v.pts.to_sorted();
        if toks.is_empty() && v.prim == 0 {
            return Some((Ok(Unknown::NoValues), n));
        }
        // An argument marker stands for the parameter markers it reaches
        // (DECISIONS 44).
        let marker = toks.iter().find(|&&t| self.is_marker(t))?;
        let param = self
            .marker_members(*marker)
            .into_iter()
            .find_map(|t| self.param_marker.get(&t).copied());
        Some((
            Ok(match param {
                Some((_, u32::MAX)) => Unknown::UnitEdgeThis,
                Some(_) => Unknown::UnitEdgeParameter,
                None => Unknown::UnitEdgeMarker,
            }),
            n,
        ))
    }

    /// The `causes` field of a site record: one entry per open operand.
    fn diagnostic_causes(&self, roles: &[(&str, Option<NodeId>)]) -> String {
        let mut out = Vec::new();
        for (role, n) in roles {
            let Some(n) = n else {
                continue;
            };
            let Some((cause, root)) = self.diagnostic_unknown(*n) else {
                continue;
            };
            let (cause, origin) = match cause {
                Ok(c) => (c.name(), self.diagnostic_root(root).1),
                Err(origin) => ("unclassified", origin),
            };
            out.push(format!(
                "{{\"role\":{},\"cause\":{},\"root\":{root},\"rootFunction\":{},\"origin\":{}}}",
                quote(role),
                quote(cause),
                opt(self.diagnostic_owner(root)),
                quote(&origin)
            ));
        }
        format!("[{}]", out.join(","))
    }
}

pub(crate) fn quote(s: &str) -> String {
    let mut out = String::from("\"");
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if c < ' ' => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}
fn nums<T: std::fmt::Display>(v: impl IntoIterator<Item = T>) -> String {
    format!(
        "[{}]",
        v.into_iter()
            .map(|x| x.to_string())
            .collect::<Vec<_>>()
            .join(",")
    )
}
fn opt(n: Option<u32>) -> String {
    n.map_or_else(|| "null".into(), |n| n.to_string())
}
fn strings(v: impl IntoIterator<Item = String>) -> String {
    format!(
        "[{}]",
        v.into_iter()
            .map(|x| quote(&x))
            .collect::<Vec<_>>()
            .join(",")
    )
}

/// A separate JSONL section: legacy S/T/F records and their digests stay untouched.
/// Node/token IDs are local to the enclosing emitting analysis unit.
pub(crate) fn collect(g: &Gen) -> Vec<String> {
    let Some(d) = &g.diagnostic else {
        return Vec::new();
    };
    let m = &g.m;
    let mut out = vec![format!(
        "{{\"record\":\"analysis\",\"aborted\":{},\"work\":{},\"maxWork\":{}}}",
        m.aborted,
        m.work.get(),
        m.max_work
    )];
    let mut nodes = BTreeSet::new();
    let locations: BTreeMap<CallId, &Site> = d
        .sites
        .iter()
        .filter_map(|s| s.call.map(|c| (c, s)))
        .collect();
    for s in &d.sites {
        nodes.extend(
            [s.receiver, s.index, s.callee, Some(s.result)]
                .into_iter()
                .flatten(),
        );
        let call = s.call.map(|c| &m.calls[c as usize]);
        if let Some(c) = call {
            nodes.extend(c.args.iter().chain(&c.spread).copied());
        }
        let raw = s.call.and_then(|c| m.diagnostic.as_ref()?.raw.get(&c));
        let bi = s.call.and_then(|c| m.diagnostic.as_ref()?.builtins.get(&c));
        let causes = m.diagnostic_causes(&[
            ("receiver", s.receiver),
            ("index", s.index),
            ("callee", s.callee),
        ]);
        out.push(format!("{{\"record\":\"site\",\"file\":{},\"function\":{},\"start\":{},\"end\":{},\"kind\":{},\"key\":{},\"origin\":{},\"causes\":{causes},\"receiver\":{},\"index\":{},\"callee\":{},\"result\":{},\"call\":{},\"raw\":{},\"builtins\":{},\"args\":{},\"spread\":{}}}",
            quote(&g.modules[s.module as usize].0),s.function,s.start,s.end,quote(s.kind),quote(&s.key),quote(s.origin),
            opt(s.receiver),opt(s.index),opt(s.callee),s.result,opt(s.call),
            nums(raw.into_iter().flatten()),strings(bi.into_iter().flatten().cloned()),
            nums(call.into_iter().flat_map(|c|c.args.iter())),nums(call.into_iter().flat_map(|c|c.spread.iter()))));
    }
    for (i, f) in m.funcs.iter().enumerate() {
        nodes.extend(f.params.iter().chain([&f.this, &f.ret]));
        let module = g.modules.get(f.module as usize);
        let src = module.and_then(|(_, m)| m.closure_source_text.get(&f.func_id));
        let span = src.and_then(|s| s.source_span).unwrap_or((0, 0));
        let declaration_key = g
            .stand_in_fc
            .get(&f.token)
            .map(|c| c.name.clone())
            .unwrap_or_else(|| crate::fn_contract_name(g, i as FnIdx).0);
        out.push(format!("{{\"record\":\"function\",\"id\":{i},\"hirId\":{},\"declarationKey\":{},\"file\":{},\"name\":{},\"start\":{},\"end\":{},\"params\":{},\"this\":{},\"result\":{},\"rest\":{},\"parent\":{},\"escape\":{}}}",
            f.func_id,quote(&declaration_key),quote(module.map_or("",|(p,_)|*p)),quote(&f.name),span.0,span.1,nums(&f.params),f.this,f.ret,opt(f.rest),opt((f.parent!=NO_FN).then_some(f.parent)),
            quote(m.escaped.get(&f.token).map_or("",|r|&m.reasons[*r as usize]))));
    }
    for (i, t) in m.tmpls.iter().enumerate() {
        let (module, classid) = match &t.kind {
            TmplKind::Class {
                module, class_id, ..
            } => (Some(*module), Some(*class_id)),
            _ => (None, None),
        };
        let source = module
            .and_then(|mi| g.modules.get(mi as usize))
            .and_then(|(_, m)| m.class_source_text.get(&classid?));
        let span = source.map(|s| s.source_range()).unwrap_or(0..0);
        let mut ancestors = Vec::new();
        let mut next = t.parent;
        while let Some(a) = next {
            if ancestors.contains(&a) {
                break;
            }
            ancestors.push(a);
            next = m.tmpls[a as usize].parent;
        }
        out.push(format!("{{\"record\":\"layout\",\"id\":{i},\"name\":{},\"kind\":{},\"file\":{},\"start\":{},\"end\":{},\"parent\":{},\"parentLinks\":{},\"ancestors\":{},\"nativeParent\":{}}}",
            quote(&t.display),quote(&format!("{:?}",t.kind)),quote(module.map_or("",|mi|g.modules[mi as usize].0)),
            if source.is_some(){span.start+1}else{0},if source.is_some(){span.end+1}else{0},opt(t.parent),nums(&t.parent_links),nums(ancestors),quote(&format!("{:?}",t.native_parent))));
    }
    // Fields already reached by the solver, including independent any-key TOP.
    // Reading these maps must never call field()/load_from() and create flow.
    let mut fields: Vec<_> = m
        .fields
        .iter()
        .map(|(&(t, a), &n)| (t, m.atoms[a as usize].clone(), n))
        .collect();
    fields.extend(m.field_any.iter().map(|(&t, &n)| (t, "*".into(), n)));
    fields.sort();
    for (t, key, n) in fields {
        if m.nodes[n as usize].pts.contains_tok(TOP) {
            nodes.insert(n);
            out.push(format!(
                "{{\"record\":\"top-field\",\"token\":{t},\"key\":{},\"node\":{n}}}",
                quote(&key)
            ));
        }
    }
    for (i, t) in m.toks.iter().enumerate() {
        let (kind, layout, function, context) = match t {
            TokKind::Top => ("top", None, None, None),
            TokKind::Obj(l, c) => ("object", Some(*l), None, Some(*c)),
            TokKind::Func(f, c) => ("function", None, Some(*f), Some(*c)),
            TokKind::Bound(_) => ("bound", None, None, None),
            TokKind::ClassObj(l) => ("class", Some(*l), None, None),
            TokKind::Proto(l) => ("prototype", Some(*l), None, None),
            TokKind::Builtin(_) | TokKind::BuiltinFn(_) => ("builtin", None, None, None),
            TokKind::ModNs(_) => ("namespace", None, None, None),
            TokKind::ObjectProto | TokKind::NativeProto(..) => {
                ("native-prototype", None, None, None)
            }
        };
        let bound = if let TokKind::Bound(b) = t {
            let (target, this, args) = &m.bound[*b as usize];
            nodes.extend([target, this]);
            nodes.extend(args);
            let origin = m
                .diagnostic
                .as_ref()
                .and_then(|d| d.bound_origins.get(b))
                .and_then(|c| locations.get(c))
                .map_or("null".into(), |s| {
                    format!(
                        "{{\"file\":{},\"start\":{},\"end\":{}}}",
                        quote(g.modules[s.module as usize].0),
                        s.start,
                        s.end
                    )
                });
            format!(
                "{{\"target\":{target},\"this\":{this},\"args\":{},\"origin\":{origin}}}",
                nums(args)
            )
        } else {
            "null".into()
        };
        out.push(format!("{{\"record\":\"token\",\"id\":{i},\"kind\":{},\"name\":{},\"layout\":{},\"function\":{},\"context\":{},\"bound\":{},\"escape\":{},\"marker\":{},\"closedMarker\":{}}}",
            quote(kind),quote(&m.tok_name(i as Tok)),opt(layout),opt(function),opt(context),bound,
            quote(m.escaped.get(&(i as Tok)).map_or("",|r|&m.reasons[*r as usize])), m.is_marker(i as Tok), m.closed_marker(i as Tok)));
    }

    // FCT slice 0: where each token first escaped, every cause of an
    // escape attempt, and every transfer into a body with its cause.
    let cause = |c: &Cause| -> String {
        match c {
            Cause::Fn(f) => format!("{{\"function\":{f}}}"),
            Cause::Tok(t) => format!("{{\"token\":{t}}}"),
            Cause::Unknown => "null".into(),
        }
    };
    let src_call = |c: Option<CallId>| c.map(|c| m.diagnostic_source_call(c).unwrap_or(c));
    // An unknown call's `protocol`: a runtime protocol step the solver runs
    // on a token (`Model::protocol_calls`: ToPrimitive, iteration), one per
    // token and made for no one program call, so its `source` is itself.
    if let Some(d) = m.diagnostic.as_ref() {
        for (c, (n, top)) in &d.unknown_calls {
            let (root, origin) = n.map_or((NO_NODE, String::new()), |n| m.diagnostic_root(n));
            out.push(format!("{{\"record\":\"unknown-call\",\"call\":{c},\"source\":{},\"protocol\":{},\"node\":{},\"top\":{top},\"root\":{root},\"rootFunction\":{},\"label\":{},\"origin\":{}}}",
                opt(src_call(Some(*c))), m.protocol_calls.contains(c), opt(*n), opt(m.diagnostic_owner(root)), quote(n.and_then(|n| m.labels.get(n as usize).copied()).unwrap_or("")), quote(&origin)));
        }
        for (t, e) in &d.esc_first {
            // ROOTS1: an escape whose unknown value came out of an earlier
            // escape names it (`via`); a cause-less one takes it as cause.
            let (via, top) = m.diagnostic_esc_via(*t, e);
            // `op`: the operation the attempt ran in (the former cause).
            let c = via.map_or(e.cause, Cause::Tok);
            out.push(format!("{{\"record\":\"escape-first\",\"token\":{t},\"reason\":{},\"cause\":{},\"call\":{},\"node\":{},\"seq\":{},\"via\":{},\"op\":{},\"top\":{top}}}",
                quote(m.reasons.get(e.reason as usize).map_or("", |s| s)), cause(&c), opt(src_call(e.call)), opt(e.node), opt(e.seq), opt(via), cause(&e.cause)));
        }
        // ROOTS2: each call an escape names, with its source position.
        let mut named: BTreeSet<CallId> = d.unknown_calls.keys().copied().collect();
        for e in d.esc_first.values() {
            named.extend(e.call);
            named.extend(m.reason_calls(e.reason));
        }
        for c in named {
            let s = src_call(Some(c)).unwrap_or(c);
            let (file, start, func) = match locations.get(&s) {
                Some(l) => (
                    quote(g.modules[l.module as usize].0),
                    l.start.to_string(),
                    Some(l.function).filter(|&f| f != NO_FN),
                ),
                None => (
                    "null".to_string(),
                    "null".to_string(),
                    m.calls
                        .get(c as usize)
                        .and_then(|k| m.diagnostic_owner(k.ret)),
                ),
            };
            out.push(format!(
                "{{\"record\":\"call\",\"call\":{c},\"source\":{s},\"file\":{file},\"start\":{start},\"function\":{}}}",
                opt(func)
            ));
        }
        for (f, (ft, r, holder)) in &d.entry_runs {
            out.push(format!("{{\"record\":\"entry-run\",\"function\":{f},\"token\":{ft},\"reason\":{},\"holder\":{}}}",
                quote(m.reasons.get(*r as usize).map_or("", |s| s)), opt(*holder)));
        }
        for (t, (w, n)) in &d.esc_why {
            if let Some(&(f, i)) = m.param_marker.get(t) {
                out.push(format!("{{\"record\":\"param-escape\",\"token\":{t},\"function\":{f},\"index\":{i},\"marker\":{},\"why\":{},\"node\":{}}}",
                    quote(&m.tok_name(*t)), quote(w), opt(*n)));
            }
        }
        for (n, name, i, k) in &d.stand_in_params {
            out.push(format!("{{\"record\":\"stand-in-param\",\"node\":{n},\"callee\":{},\"index\":{i},\"key\":{}}}",
                quote(name), k.as_deref().map_or("null".into(), quote)));
        }
        for (t, c) in &d.esc_causes {
            out.push(format!(
                "{{\"record\":\"escape-cause\",\"token\":{t},\"cause\":{}}}",
                cause(c)
            ));
        }
        for ((f, kind, c), call) in &d.reach {
            out.push(format!(
                "{{\"record\":\"reach\",\"function\":{f},\"kind\":{},\"cause\":{},\"call\":{}}}",
                quote(kind),
                cause(c),
                opt(src_call(*call))
            ));
        }
    }

    // PUB: read existing ownership/member maps, without settling or creating flow.
    for (&t, &(f, index)) in &m.param_marker {
        out.push(format!(
            "{{\"record\":\"parameter-marker\",\"token\":{t},\"function\":{f},\"index\":{index}}}"
        ));
    }
    for (t, _) in m
        .toks
        .iter()
        .enumerate()
        .filter(|(t, _)| m.is_marker(*t as Tok))
    {
        let t = t as Tok;
        let home = m.arg_marker_node.get(&t).copied();
        nodes.extend(home);
        let param = home
            .and_then(|n| m.param_at.get(&n))
            .map_or("null".into(), |(f, i)| format!("[{f},{i}]"));
        let parent = m.path_parent.get(&t).map_or("null".into(), |(p, k)| {
            format!(
                "[{p},{}]",
                k.map_or("null".into(), |a| quote(&m.atoms[a as usize]))
            )
        });
        let keyed = m.keyed_parent.get(&t).map_or("null".into(), |(p, k)| {
            format!("[{p},{}]", quote(&m.atoms[*k as usize]))
        });
        out.push(format!("{{\"record\":\"marker-route\",\"token\":{t},\"bare\":{},\"beside\":{},\"home\":{},\"parameter\":{param},\"members\":{},\"reach\":{},\"reachBase\":{},\"keyedParent\":{keyed},\"pathParent\":{parent}}}",
            m.bare_marker(t),m.beside_values(t),opt(home),nums(m.arg_members.get(&t).into_iter().flatten()),opt(m.marker_reach.get(&t).copied()),opt(m.arg_reach_base.get(&t).copied())));
    }
    for (f, fi) in m.funcs.iter().enumerate() {
        let f = f as FnIdx;
        out.push(format!("{{\"record\":\"parameter-contract\",\"function\":{f},\"closed\":{},\"described\":{},\"closedValues\":{},\"contract\":{},\"foreign\":{}}}",
            m.closed_params.contains_key(&f),m.closed_described.get(&f).map_or("null".into(), |v| format!("{:?}",v)),nums(m.closed_params.get(&f).into_iter().flatten()),m.contract_fns.contains(&f),g.stand_in_fc.contains_key(&fi.token)));
        nodes.extend(m.closed_params.get(&f).into_iter().flatten());
    }
    if let Some(trace) = &m.diagnostic {
        for (&n, (input, why)) in &trace.contract_inputs {
            nodes.insert(n);
            out.push(format!(
                "{{\"record\":\"contract-input\",\"node\":{n},\"description\":{},\"why\":{why}}}",
                quote(input)
            ));
        }
        for (&n, (src, why)) in &trace.pred {
            out.push(format!(
                "{{\"record\":\"top-predecessor\",\"node\":{n},\"source\":{src},\"reason\":{}}}",
                quote(why)
            ));
        }
    }
    for (src, v) in m.nodes.iter().enumerate() {
        if !v.succ.is_empty() {
            out.push(format!(
                "{{\"record\":\"copy-edges\",\"source\":{src},\"targets\":{}}}",
                nums(&v.succ)
            ));
        }
    }
    if let Some(c) = m.diagnostic.as_ref().and_then(|d| d.census.as_ref()) {
        for (n, v) in m.nodes.iter().enumerate() {
            if v.pts.contains_tok(TOP) {
                nodes.insert(n as NodeId);
            }
        }
        out.push(format!(
            "{{\"record\":\"birth-meta\",\"suppressed\":{},\"globalInTop\":{},\"starKey\":{},\"numKey\":{},\"objectProto\":{}}}",
            c.suppressed,m.global_in_top,quote(crate::STAR_KEY),quote(NUM_KEY),opt(m.tok_index.get(&TokKind::ObjectProto).copied())
        ));
        for (id, call) in m.calls.iter().enumerate() {
            out.push(format!("{{\"record\":\"birth-call\",\"id\":{id},\"sourceCall\":{},\"result\":{},\"args\":{}}}", opt(m.diagnostic_source_call(id as CallId)),call.ret,nums(&call.args)));
        }
        for (&inst, &orig) in &m.diagnostic.as_ref().unwrap().inst_orig {
            out.push(format!(
                "{{\"record\":\"birth-edge\",\"source\":{orig},\"target\":{inst}}}"
            ));
        }
        for (n, file, line, why, source, call, escape, owner, gate) in &c.injections {
            out.push(format!("{{\"record\":\"birth-injection\",\"node\":{n},\"injector\":{},\"line\":{line},\"why\":{},\"source\":{source},\"call\":{call},\"escape\":{escape},\"owner\":{owner},\"gate\":{}}}", quote(file),quote(why),birth_gate(m,*gate)));
        }
        for (n, a) in &c.arrivals {
            out.push(format!("{{\"record\":\"birth-arrival\",\"node\":{n},\"seq\":{},\"source\":{},\"cause\":{},\"call\":{},\"parent\":{},\"injected\":{},\"gate\":{}}}",
                a.seq, opt((a.src != NO_NODE).then_some(a.src)), opt((a.cause != NO_NODE).then_some(a.cause)), opt((a.call != u32::MAX).then_some(a.call)),
                opt((a.parent != u32::MAX).then_some(a.parent)), a.injected, birth_gate(m, a.gate)));
        }
        for (a, b) in &c.top_edges {
            out.push(format!(
                "{{\"record\":\"birth-edge\",\"source\":{a},\"target\":{b}}}"
            ));
        }
        for (t, n, call, parent, reason, gate) in &c.escapes {
            out.push(format!("{{\"record\":\"birth-escape\",\"token\":{t},\"source\":{n},\"call\":{call},\"parent\":{parent},\"why\":{},\"gate\":{}}}",quote(&m.reasons[*reason as usize]),birth_gate(m,*gate)));
        }
        for (n, source, call, parent, reason, gate) in &c.sinks {
            out.push(format!("{{\"record\":\"birth-sink\",\"node\":{n},\"source\":{source},\"call\":{call},\"parent\":{parent},\"why\":{},\"gate\":{}}}",quote(&m.reasons[*reason as usize]),birth_gate(m,*gate)));
        }
        for (t, k, n, call, parent, proto) in &c.writes {
            let key = if *k == u32::MAX {
                "null".into()
            } else {
                quote(&m.atoms[*k as usize])
            };
            out.push(format!("{{\"record\":\"birth-write\",\"token\":{t},\"key\":{key},\"source\":{n},\"call\":{call},\"parent\":{parent},\"proto\":{},\"numeric\":{}}}", if *proto == NO_NODE { "null".into() } else { proto.to_string() }, *k != u32::MAX && crate::solve::numeric_key(&m.atoms[*k as usize])));
        }
        let mut opened: Vec<_> = c.openers.iter().collect();
        opened.sort_by_key(|(_, o)| o.seq);
        for (&(t, k), o) in opened {
            let key = if k == u32::MAX {
                "null".into()
            } else {
                quote(&m.atoms[k as usize])
            };
            let (desc, sfn) = m
                .stores
                .get(o.site as usize)
                .map_or((String::new(), u32::MAX), |s| (s.desc.clone(), s.func));
            let fname = |f: u32| {
                m.funcs
                    .get(f as usize)
                    .map_or(String::new(), |f| f.name.clone())
            };
            let (root, rwhy) = match o.recv {
                NO_NODE => (NO_NODE, String::new()),
                r => m.diagnostic_root(r),
            };
            if o.recv != NO_NODE {
                nodes.insert(o.recv);
            }
            out.push(format!("{{\"record\":\"birth-opener\",\"seq\":{},\"holder\":{t},\"holderName\":{},\"key\":{key},\"what\":{},\"site\":{},\"siteDesc\":{},\"siteFunction\":{},\"recv\":{},\"recvRoot\":{},\"recvWhy\":{},\"cause\":{},\"call\":{},\"parent\":{},\"function\":{}}}",
                o.seq, quote(&m.tok_name(t)), quote(&o.what), opt((o.site != u32::MAX).then_some(o.site)), quote(&desc), quote(&fname(sfn)),
                opt((o.recv != NO_NODE).then_some(o.recv)), opt((root != NO_NODE).then_some(root)), quote(&rwhy),
                opt((o.cause != NO_NODE).then_some(o.cause)), opt((o.call != u32::MAX).then_some(o.call)), o.parent, quote(&fname(o.func))));
        }
        for (t, k, call, id) in &c.reads {
            let origin = if *call {
                c.read_origins.get(id).map_or(String::from("null"), |o| {
                    format!("{o},\"originName\":{}", quote(&m.tok_name(*o)))
                })
            } else {
                String::from("null")
            };
            out.push(format!("{{\"record\":\"birth-read\",\"token\":{t},\"key\":{},\"isCall\":{call},\"id\":{id},\"origin\":{origin},\"opWrites\":{},\"numeric\":{},\"isGlobal\":{}}}",quote(&m.atoms[*k as usize]),crate::builtins::census_native_op_writes(m,*t),crate::solve::numeric_key(&m.atoms[*k as usize]),crate::builtins::global_object(m,*t)));
        }
    }
    let roots: Vec<_> = nodes.iter().map(|&n| m.diagnostic_root(n).0).collect();
    nodes.extend(roots);
    for n in nodes {
        let v = &m.nodes[n as usize];
        let toks = v.pts.to_sorted();
        let top = toks.contains(&TOP);
        let marker_top = toks.iter().any(|&t| m.bare_marker(t));
        let mut mask = v.prim as u32;
        for &t in &toks {
            if m.is_marker(t) {
                continue;
            }
            mask |= match m.toks[t as usize] {
                TokKind::Top => 0,
                TokKind::Func(..)
                | TokKind::Bound(_)
                | TokKind::ClassObj(_)
                | TokKind::BuiltinFn(_) => 256,
                TokKind::Builtin(ns)
                    if !matches!(
                        ns,
                        BuiltinNs::Json
                            | BuiltinNs::Math
                            | BuiltinNs::Console
                            | BuiltinNs::Stdout
                            | BuiltinNs::Stderr
                            | BuiltinNs::Stdin
                            | BuiltinNs::GlobalThis
                            | BuiltinNs::Process
                            | BuiltinNs::Other
                    ) =>
                {
                    256
                }
                _ => 128,
            };
        }
        let (root, origin) = m.diagnostic_root(n);
        out.push(format!("{{\"record\":\"node\",\"id\":{n},\"primitiveMask\":{},\"kindMask\":{mask},\"top\":{top},\"markerTop\":{marker_top},\"tokens\":{},\"label\":{},\"parameter\":{},\"observation\":{},\"topRoot\":{},\"topBy\":{},\"topOrigin\":{}}}",
            v.prim,nums(toks),quote(m.labels.get(n as usize).copied().unwrap_or("")),v.param,v.obs,opt(top.then_some(root)),opt(top.then(|| m.diagnostic.as_ref().and_then(|d| d.top_by.get(&root).copied())).flatten()),quote(&if top&&m.explain{m.top_origin(n)}else{origin})));
    }
    out
}

/// Existing contract summary, observed only after fact derivation.
pub(crate) fn value_brief(c: &crate::units::ValueContract) -> String {
    format!(
        "{{\"unknown\":{},\"why\":{},\"prim\":{},\"functions\":{},\"arrays\":{}}}",
        c.unknown,
        why_json(c),
        c.prim,
        strings(c.fns.iter().map(|f| f.name.clone())),
        strings(
            c.arrays
                .iter()
                .chain(&c.made_arrays)
                .chain(&c.arrayboxes)
                .cloned()
        )
    )
}

/// The reason an unknown value contract is unknown and the unit that
/// published it, as JSON (`null` when it is known or has no recorded reason).
pub(crate) fn why_json(c: &crate::units::ValueContract) -> String {
    match &c.why.0 {
        Some((r, u)) if c.unknown => format!(
            "{{\"reason\":{},\"unit\":{}}}",
            quote(r),
            u.as_deref().map_or("null".into(), quote)
        ),
        _ => "null".into(),
    }
}

fn birth_gate(m: &Model, gate: Option<(Tok, Atom)>) -> String {
    gate.map_or("null".into(), |(t, a)| {
        format!("{{\"token\":{t},\"key\":{}}}", quote(&m.atoms[a as usize]))
    })
}

/// Off-by-default read-out (`PERRY_WPF_KEYREAD=1`): at the end of the
/// solve, the decoded string set of every node of the functions along the
/// three.js track-name chain (GLTF track build, sanitize/unique names,
/// KeyframeTrack, PropertyBinding). `KF` = function, `KR` = node:
/// id, label, prim bits, holds ⊤, strings (constants, prefix/infix/suffix
/// entries, exclusion entries with their character sets, `ANY`).
pub(crate) fn keyread(g: &Gen) {
    if std::env::var_os("PERRY_WPF_KEYREAD").is_none() {
        return;
    }
    const NAMES: &[&str] = KEYREAD_NAMES;
    let m = &g.m;
    let mut want = vec![false; m.funcs.len()];
    for (i, f) in m.funcs.iter().enumerate() {
        let module = g.modules.get(f.module as usize);
        let file = module.map_or("", |(p, _)| *p);
        let hit = keyread_fn(&NAMES, &f.name, file)
            || NAMES.contains(&f.name.as_str())
            || f.name.contains("KeyframeTrack")
            || f.name.contains("PropertyBinding")
            || f.name.contains("AnimationClip")
            || (file.ends_with("GLTFLoader.js") && f.name.contains("nimation"));
        if !hit {
            continue;
        }
        want[i] = true;
        let src = module.and_then(|(_, m)| m.closure_source_text.get(&f.func_id));
        let span = src.and_then(|s| s.source_span).unwrap_or((0, 0));
        eprintln!("KF {i} {} {file} {} {} params={:?} this={} ret={}", f.name, span.0, span.1, f.params, f.this, f.ret);
    }
    let dec = |a: Atom| -> String {
        let s = &m.atoms[a as usize];
        let esc = |t: &str| t.chars().take(60).map(|c| if (c as u32) < 32 || c == ' ' { format!("\\x{:02x}", c as u32) } else { c.to_string() }).collect::<String>();
        if let Some(r) = s.strip_prefix(crate::solve::strs::EXCL_MARK) {
            let (h, t) = r.split_once('\u{1}').unwrap_or((r, ""));
            let e = u128::from_str_radix(h, 16).unwrap_or(0);
            let cs: String = (0..128u32).filter(|b| e >> b & 1 == 1).map(|b| esc(&char::from_u32(b).unwrap().to_string())).collect();
            format!("excl[{cs}]+{:?}", esc(t))
        } else if let Some(r) = s.strip_prefix(crate::solve::strs::PREFIX_MARK) {
            format!("prefix({:?})", esc(r))
        } else if let Some(r) = s.strip_prefix(crate::solve::strs::INFIX_MARK) {
            format!("infix({:?})", esc(r))
        } else if let Some(r) = s.strip_prefix(crate::solve::strs::SUFFIX_MARK) {
            format!("suffix({:?})", esc(r))
        } else {
            format!("{:?}", esc(s))
        }
    };
    for n in 0..m.nodes.len() {
        let f = m.node_fn.get(n).copied().unwrap_or(NO_FN);
        if f == NO_FN || !want.get(f as usize).copied().unwrap_or(false) {
            continue;
        }
        let nd = &m.nodes[n];
        let l = m.labels.get(n).copied().unwrap_or("?");
        let strs = match nd.strs {
            0 => "-".to_string(),
            STRS_TOP => "ANY".to_string(),
            s => m.str_atoms(s).iter().map(|&a| dec(a)).collect::<Vec<_>>().join(" | "),
        };
        eprintln!("KR {n} f{f} {l} prim={:#x} top={} ntok={} strs={strs}", nd.prim, nd.pts.contains_tok(TOP), nd.pts.len());
    }
}

const KEYREAD_NAMES: &[&str] = &[
    "sanitizeNodeName", "createUniqueName", "_createAnimationTracks", "loadAnimation",
    "parseTrackName", "bind", "findNode", "Composite", "create", "parse", "toJSON",
    "clone", "CreateFromMorphTargetSequence", "parseAnimation", "_getTrackTypeForValueTypeName",
    "getValueSize", "validate", "_bindTrack", "_lendBinding", "clipAction", "subclip",
];

/// The functions of the track-name chain (`keyread`, `keysnap_plan`).
fn keyread_fn(names: &[&str], name: &str, file: &str) -> bool {
    names.contains(&name)
        || name.contains("KeyframeTrack")
        || name.contains("PropertyBinding")
        || name.contains("AnimationClip")
        || (file.ends_with("GLTFLoader.js") && name.contains("nimation"))
}

/// One string-set entry decoded (constants, prefix/infix/suffix entries,
/// exclusion entries with their character sets).
fn key_dec(m: &Model, a: Atom) -> String {
    let s = &m.atoms[a as usize];
    let esc = |t: &str| t.chars().take(60).map(|c| if (c as u32) < 32 || c == ' ' { format!("\\x{:02x}", c as u32) } else { c.to_string() }).collect::<String>();
    if let Some(r) = s.strip_prefix(crate::solve::strs::EXCL_MARK) {
        let (h, t) = r.split_once('\u{1}').unwrap_or((r, ""));
        let e = u128::from_str_radix(h, 16).unwrap_or(0);
        let cs: String = (0..128u32).filter(|b| e >> b & 1 == 1).map(|b| esc(&char::from_u32(b).unwrap().to_string())).collect();
        format!("excl[{cs}]+{:?}", esc(t))
    } else if let Some(r) = s.strip_prefix(crate::solve::strs::PREFIX_MARK) {
        format!("prefix({:?})", esc(r))
    } else if let Some(r) = s.strip_prefix(crate::solve::strs::INFIX_MARK) {
        format!("infix({:?})", esc(r))
    } else if let Some(r) = s.strip_prefix(crate::solve::strs::SUFFIX_MARK) {
        format!("suffix({:?})", esc(r))
    } else {
        format!("{:?}", esc(s))
    }
}

/// KEYSNAP plan (`PERRY_WPF_KEYREAD=1`): the track-name chain's nodes,
/// snapshotted by `Model::keysnap_at` when the realm is first held and at
/// the first escape before it. Positions are printed once here (`KP`).
pub struct KeySnap {
    nodes: Vec<NodeId>,
    fn_file: FxHashMap<u32, String>,
    call_pos: FxHashMap<NodeId, (String, u32)>,
    at_hold: bool,
    at_esc: bool,
    /// KEYSNAP2: module files, the `name`/`uuid` loads of GLTFLoader's
    /// chain functions, the last unknown-callee call before the hold.
    mod_file: Vec<String>,
    loads: Vec<(String, u32, String, NodeId)>,
    unk: Option<(CallId, Tok, Tok)>,
    /// HOLDPOS: the source position (module, byte offset) of each load
    /// destination and call result node; the first arrival of each token in
    /// each node while the realm is not held (source node, reason, the
    /// solver code that added it).
    node_pos: FxHashMap<NodeId, (u32, u32)>,
    first: FxHashMap<(NodeId, Tok), (NodeId, u32, &'static std::panic::Location<'static>)>,
}

pub(crate) fn keysnap_plan(g: &mut Gen) {
    if std::env::var_os("PERRY_WPF_KEYREAD").is_none() {
        return;
    }
    let file_of = |mi: u32| g.modules.get(mi as usize).map_or("", |(p, _)| *p).to_string();
    let mut want = vec![false; g.m.funcs.len()];
    let mut fn_file = FxHashMap::default();
    for (i, f) in g.m.funcs.iter().enumerate() {
        let file = file_of(f.module);
        if keyread_fn(KEYREAD_NAMES, &f.name, &file)
            || (file.ends_with("three.core.js") && f.name.contains("_setValue"))
            || f.name == "generateUUID"
            || f.name.ends_with("createUniqueName")
        {
            want[i] = true;
            fn_file.insert(i as u32, file);
        }
    }
    let of = |n: NodeId| g.m.node_fn.get(n as usize).copied().unwrap_or(NO_FN);
    let is = |n: NodeId| { let f = of(n); f != NO_FN && want.get(f as usize).copied().unwrap_or(false) };
    let nodes: Vec<NodeId> = (0..g.m.nodes.len() as NodeId).filter(|&n| is(n)).collect();
    for s in &g.element_sites {
        if is(s.recv) || is(s.idx) {
            eprintln!("KP elem {} {} {:?} recv={} idx={} key={:?} f{}", file_of(s.module), s.byte_offset, s.op, s.recv, s.idx, s.key, of(s.recv));
        }
    }
    for &(mi, off, a, dst) in &g.load_sites {
        if is(dst) {
            eprintln!("KP load {} {off} .{} dst={dst} f{}", file_of(mi), g.m.atoms[a as usize], of(dst));
        }
    }
    let mut call_pos = FxHashMap::default();
    for &(mi, off, r) in &g.call_results_at {
        call_pos.insert(r, (file_of(mi), off));
    }
    let mod_file: Vec<String> = (0..g.modules.len() as u32).map(file_of).collect();
    let mut loads = Vec::new();
    for &(mi, off, a, dst) in &g.load_sites {
        let k = g.m.atoms[a as usize].to_string();
        let file = file_of(mi);
        if is(dst) && file.ends_with("GLTFLoader.js") && (k == "name" || k == "uuid") {
            loads.push((file, off, k, dst));
        }
    }
    let mut node_pos = FxHashMap::default();
    for &(mi, off, r) in &g.call_results_at {
        node_pos.entry(r).or_insert((mi, off));
    }
    for &(mi, off, _, dst) in &g.load_sites {
        node_pos.entry(dst).or_insert((mi, off));
    }
    g.m.keysnap = Some(Box::new(KeySnap {
        nodes, fn_file, call_pos, at_hold: false, at_esc: false, mod_file, loads, unk: None,
        node_pos, first: FxHashMap::default(),
    }));
}

impl Model {
    /// Print the KEYSNAP block once per occasion (`hold`: the realm is
    /// first held; else: the first escape while it is not).
    pub(crate) fn keysnap_at(&mut self, hold: bool, producer: &str, call: Option<CallId>, reason: u32, tok: Option<Tok>) {
        let Some(ks) = self.keysnap.as_mut() else { return };
        let done = if hold { &mut ks.at_hold } else { &mut ks.at_esc };
        if *done {
            return;
        }
        *done = true;
        let ks = self.keysnap.take().expect("checked");
        let what = if hold { "KEYSNAP at realm hold" } else { "KEYSNAP at first escape" };
        let kind = |m: &Model, t: Tok| -> String {
            let k = format!("{:?}", m.toks[t as usize]);
            let k: String = k.chars().take(48).collect();
            if crate::builtins::builtin_function(m, t) { format!("{k}[bfn]") } else { k }
        };
        eprintln!("{what}");
        eprintln!("KS producer {producer}");
        let rs = self.reasons.get(reason as usize).cloned().unwrap_or_else(|| "-".into());
        eprintln!("KS reason {reason} {rs:?}");
        let calls: Vec<CallId> = call.into_iter().chain(if reason == u32::MAX { vec![] } else { self.reason_calls(reason) }).collect();
        for c in calls.iter().take(4) {
            let ret = self.calls.get(*c as usize).map_or(NO_NODE, |cs| cs.ret);
            let pos = ks.call_pos.get(&ret).map_or(("?".to_string(), 0), |p| p.clone());
            eprintln!("KS call c{c} ret={ret} at {} {}", pos.0, pos.1);
        }
        if let Some(t) = tok {
            eprintln!("KS tok {t} {}", kind(self, t));
        }
        eprintln!("KS progress pops={} work={} escaped={} nodes={}", self.pops, self.work.get(), self.escaped.len(), self.nodes.len());
        for &n in &ks.nodes {
            let nd = &self.nodes[n as usize];
            let f = self.node_fn.get(n as usize).copied().unwrap_or(NO_FN);
            let l = self.labels.get(n as usize).copied().unwrap_or("?");
            let strs = match nd.strs {
                0 => "-".to_string(),
                STRS_TOP => "ANY".to_string(),
                s => self.str_atoms(s).iter().map(|&a| key_dec(self, a)).collect::<Vec<_>>().join(" | "),
            };
            let toks: Vec<Tok> = nd.pts.to_sorted();
            let mut hist: BTreeMap<String, usize> = BTreeMap::new();
            for &t in &toks {
                let k = kind(self, t);
                let k = k.split(['(', ' ', '{']).next().unwrap_or("").to_string() + if k.ends_with("[bfn]") { "[bfn]" } else { "" };
                *hist.entry(k).or_default() += 1;
            }
            let first: Vec<String> = toks.iter().take(12).map(|&t| format!("{t}:{}", kind(self, t))).collect();
            let file = ks.fn_file.get(&f).map_or("", |s| s.as_str());
            eprintln!(
                "KS {n} f{f} {file} {l} prim={:#x} top={} ntok={} kinds={hist:?} strs={strs} toks=[{}]",
                nd.prim, nd.pts.contains_tok(TOP), toks.len(), first.join(", ")
            );
        }
        self.keysnap2(&ks);
        if hold {
            self.keysnap3(&ks, calls.first().copied(), tok);
        }
        eprintln!("KS end");
        self.keysnap = Some(ks);
    }
}

impl Model {
    /// KEYSNAP2: the last call whose callee is an unknown function value
    /// (its receiver escapes next).
    pub(crate) fn keysnap_note(&mut self, call: CallId, callee: Tok, recv: Tok) {
        if let Some(ks) = self.keysnap.as_mut() {
            ks.unk = Some((call, callee, recv));
        }
    }

    fn ks2_kind(&self, t: Tok) -> String {
        let k: String = format!("{:?}", self.toks[t as usize]).chars().take(48).collect();
        if crate::builtins::builtin_function(self, t) { format!("{k}[bfn]") } else { k }
    }

    fn ks2_fn(&self, ks: &KeySnap, f: FnIdx) -> String {
        match self.funcs.get(f as usize) {
            Some(fi) => format!("{}@{}", fi.name, ks.mod_file.get(fi.module as usize).map_or("?", |s| s.as_str())),
            None => "-".into(),
        }
    }

    fn ks2_strs(&self, n: NodeId) -> String {
        match self.nodes[n as usize].strs {
            0 => "-".to_string(),
            STRS_TOP => "ANY".to_string(),
            s => self.str_atoms(s).iter().take(24).map(|&a| key_dec(self, a)).collect::<Vec<_>>().join(" | "),
        }
    }

    /// EXCL-OK: every string entry is an exclusion entry or a constant
    /// without `. [ ] : /`; OPEN otherwise ("-": no strings).
    fn ks2_verdict(&self, n: NodeId) -> &'static str {
        match self.nodes[n as usize].strs {
            0 => "-",
            STRS_TOP => "OPEN",
            s => {
                let ok = self.str_atoms(s).iter().all(|&a| {
                    let t = &self.atoms[a as usize];
                    if t.starts_with(crate::solve::strs::EXCL_MARK) {
                        true
                    } else if t.starts_with(crate::solve::strs::PREFIX_MARK)
                        || t.starts_with(crate::solve::strs::INFIX_MARK)
                        || t.starts_with(crate::solve::strs::SUFFIX_MARK)
                    {
                        false
                    } else {
                        !t.chars().any(|c| matches!(c, '.' | '[' | ']' | ':' | '/'))
                    }
                });
                if ok { "EXCL-OK" } else { "OPEN" }
            }
        }
    }

    fn ks2_node(&self, ks: &KeySnap, n: NodeId) -> String {
        let nd = &self.nodes[n as usize];
        let f = self.node_fn.get(n as usize).copied().unwrap_or(NO_FN);
        let l = self.labels.get(n as usize).copied().unwrap_or("?");
        let toks: Vec<Tok> = nd.pts.to_sorted();
        let mut hist: BTreeMap<String, usize> = BTreeMap::new();
        for &t in &toks {
            let k = self.ks2_kind(t);
            let k = k.split(['(', ' ', '{']).next().unwrap_or("").to_string() + if k.ends_with("[bfn]") { "[bfn]" } else { "" };
            *hist.entry(k).or_default() += 1;
        }
        let first: Vec<String> = toks.iter().take(16).map(|&t| format!("{t}:{}", self.ks2_kind(t))).collect();
        format!(
            "n{n} {} {l} prim={:#x} top={} ntok={} kinds={hist:?} strs={} toks=[{}]",
            self.ks2_fn(ks, f), nd.prim, nd.pts.contains_tok(TOP), toks.len(), self.ks2_strs(n), first.join(", ")
        )
    }

    fn keysnap2(&self, ks: &KeySnap) {
        let name_a = self.atoms.iter().position(|s| &**s == "name").map(|a| a as Atom);
        let want: crate::pts::FxHashSet<NodeId> = ks.loads.iter().map(|l| l.3).collect();
        let uc = ks.unk.map(|u| u.0);
        let mut recv_of: FxHashMap<NodeId, Vec<NodeId>> = FxHashMap::default();
        let mut stores: Vec<(NodeId, NodeId, SiteId)> = Vec::new();
        let mut callee: Vec<(NodeId, String)> = Vec::new();
        for (n, nd) in self.nodes.iter().enumerate() {
            let n = n as NodeId;
            for c in nd.cons.iter() {
                match c {
                    Cons::Load { dst, .. } if want.contains(dst) => recv_of.entry(*dst).or_default().push(n),
                    Cons::Store { key: Key::Named(a), src, site } if Some(*a) == name_a => stores.push((n, *src, *site)),
                    Cons::Call { call } if Some(*call) == uc => callee.push((n, "Call".into())),
                    Cons::New { call } | Cons::NewProgram { call } if Some(*call) == uc => callee.push((n, "New".into())),
                    Cons::CallThis { call, this } if Some(*call) == uc => callee.push((n, format!("CallThis this={this}"))),
                    Cons::MCall { key, call } if Some(*call) == uc => callee.push((n, format!("MCall(receiver) key={key:?}"))),
                    _ => {}
                }
            }
        }
        // A1: the name/uuid loads and their field nodes.
        let mut rset: crate::pts::FxHashSet<Tok> = crate::pts::FxHashSet::default();
        for (file, off, k, dst) in &ks.loads {
            eprintln!("KS2 load {file} {off} .{k} {}", self.ks2_node(ks, *dst));
            for &r in recv_of.get(dst).map_or(&[][..], |v| v.as_slice()) {
                eprintln!("KS2   recv {}", self.ks2_node(ks, r));
                let toks = self.nodes[r as usize].pts.to_sorted();
                let ka = self.atoms.iter().position(|s| &**s == k.as_str()).map(|a| a as Atom);
                let mut seen: crate::pts::FxHashSet<NodeId> = crate::pts::FxHashSet::default();
                for &t in &toks {
                    if k == "name" {
                        rset.insert(t);
                    }
                    if let Some(fnode) = ka.and_then(|a| self.fields.get(&(t, a)).copied()) {
                        if seen.insert(fnode) && seen.len() <= 24 {
                            eprintln!("KS2   field tok {t}:{} -> {}", self.ks2_kind(t), self.ks2_node(ks, fnode));
                        }
                    }
                }
                eprintln!("KS2   fields distinct={}", seen.len());
            }
        }
        // A2: every `name` store whose receiver meets the `node.name` receivers.
        let mut by_site: BTreeMap<SiteId, (NodeId, NodeId, usize)> = BTreeMap::new();
        for &(s, v, site) in &stores {
            let k = self.nodes[s as usize].pts.to_sorted().iter().filter(|t| rset.contains(t)).count();
            if k > 0 {
                by_site.entry(site).or_insert((s, v, k));
            }
        }
        eprintln!("KS2 name-stores total={} meeting={} rset={}", stores.len(), by_site.len(), rset.len());
        for (site, (s, v, k)) in &by_site {
            let st = self.stores.get(*site as usize);
            eprintln!(
                "KS2 store site={site} {} desc={:?} common={k} verdict={} recv=n{s}",
                st.map_or("-".into(), |st| self.ks2_fn(ks, st.func)),
                st.map_or("", |st| st.desc.as_str()),
                self.ks2_verdict(*v)
            );
            eprintln!("KS2   value {}", self.ks2_node(ks, *v));
        }
        // B1/B2: the unknown call.
        if let Some((c, t, x)) = ks.unk {
            if let Some(cs) = self.calls.get(c as usize) {
                let pos = |r: NodeId| ks.call_pos.get(&r).map_or("? 0".to_string(), |p| format!("{} {}", p.0, p.1));
                eprintln!(
                    "KS2 call c{c} this={:?} nargs={} nspread={} ret=n{} at {} site={} region=n{} fn={}",
                    cs.this, cs.args.len(), cs.spread.len(), cs.ret, pos(cs.ret), cs.site, cs.region,
                    self.ks2_fn(ks, self.node_fn.get(cs.ret as usize).copied().unwrap_or(NO_FN))
                );
                let site_ret = self.calls.get(cs.site as usize).map(|s| s.ret);
                if let Some(sr) = site_ret {
                    eprintln!("KS2   site-call c{} ret=n{sr} at {} fn={}", cs.site, pos(sr), self.ks2_fn(ks, self.node_fn.get(sr as usize).copied().unwrap_or(NO_FN)));
                }
                eprintln!(
                    "KS2   made_root={:?} keyed={:?} bound_synth={} protocol={} contract_new={} ctor={}",
                    self.made_root.get(&c),
                    self.solver_keyed.iter().filter(|(_, v)| v.0 == c).map(|(k, _)| *k).take(3).collect::<Vec<_>>(),
                    self.bound_synth.contains(&c), self.protocol_calls.contains(&c), self.contract_new.contains(&c), self.ctor_calls.contains(&c)
                );
                if let ThisArg::Node(tn) = cs.this {
                    eprintln!("KS2   this {}", self.ks2_node(ks, tn));
                }
                for &a in cs.args.iter().take(4) {
                    eprintln!("KS2   arg {}", self.ks2_node(ks, a));
                }
            }
            eprintln!("KS2 callee tok {t} {} name={}", self.ks2_kind(t), self.callee_name(t));
            eprintln!("KS2 recv tok {x} {} name={}", self.ks2_kind(x), self.tok_name(x));
            if let TokKind::Obj(tm, ctx) = self.toks[x as usize] {
                if let Some(tp) = self.tmpls.get(tm as usize) {
                    eprintln!("KS2 tmpl {tm} ctx={ctx} display={:?} kind={:?}", tp.display, tp.kind);
                }
            }
            for (n, how) in &callee {
                eprintln!("KS2 callee-node {how} {}", self.ks2_node(ks, *n));
            }
            let cn: crate::pts::FxHashSet<NodeId> = callee.iter().map(|c| c.0).collect();
            let mut k = 0;
            for (n, nd) in self.nodes.iter().enumerate() {
                for c in nd.cons.iter() {
                    if let Cons::Load { key, dst } = c {
                        if cn.contains(dst) && k < 8 {
                            k += 1;
                            let kk = match key { Key::Named(a) => self.atoms[*a as usize].to_string(), Key::Any(_) => "any".into() };
                            eprintln!("KS2 callee-from n{dst} <- load .{kk} recv {}", self.ks2_node(ks, n as NodeId));
                        }
                    }
                }
            }
        } else {
            eprintln!("KS2 call none (no unknown-callee call noted)");
        }
    }
}

impl Model {
    /// HOLDPOS: a token arrived in `n` for the first time (only while the
    /// realm is not held).
    pub(crate) fn keysnap_arrivals(&mut self, n: NodeId, fresh: &[Tok], at: &'static std::panic::Location<'static>) {
        let (src, why) = (self.cur_src, self.cur_why);
        if let Some(ks) = self.keysnap.as_mut() {
            for &t in fresh {
                ks.first.entry((n, t)).or_insert((src, why, at));
            }
        }
    }

    /// `file:line:col (byte off)` of a module byte offset.
    fn ks3_pos(&self, ks: &KeySnap, mi: u32, off: u32) -> String {
        let file = ks.mod_file.get(mi as usize).map_or("?", |s| s.as_str());
        let (line, col) = match std::fs::read(file) {
            Ok(b) => {
                let b = &b[..(off as usize).min(b.len())];
                let line = b.iter().filter(|&&c| c == b'\n').count() + 1;
                let col = b.len() - b.iter().rposition(|&c| c == b'\n').map_or(0, |p| p + 1) + 1;
                (line, col)
            }
            Err(_) => (0, 0),
        };
        format!("{file}:{line}:{col} (byte {off})")
    }

    fn ks3_node_pos(&self, ks: &KeySnap, n: NodeId) -> String {
        let f = self.diagnostic_owner(n).unwrap_or(NO_FN);
        let pos = ks.node_pos.get(&n).map_or("-".to_string(), |&(mi, off)| self.ks3_pos(ks, mi, off));
        format!("n{n} {} in {} at {pos}", self.labels.get(n as usize).copied().unwrap_or("?"), self.ks2_fn(ks, f))
    }

    /// The source call a (cloned, templated or solver-made) call stands
    /// for: `tmpl_of`, `origin`, then `site`, each to an earlier call,
    /// until one whose result node has a source position.
    fn ks3_call(&self, ks: &KeySnap, call: CallId) -> String {
        let mut c = call;
        let mut chain: Vec<String> = Vec::new();
        loop {
            let Some(cs) = self.calls.get(c as usize) else { return format!("c{c} not a call") };
            if let Some(&(mi, off)) = ks.node_pos.get(&cs.ret) {
                let f = self.diagnostic_owner(cs.ret).unwrap_or(NO_FN);
                return format!("source call c{c} via [{}] at {} in {}", chain.join(" "), self.ks3_pos(ks, mi, off), self.ks2_fn(ks, f));
            }
            let (next, how) = if let Some(&n) = self.sum.tmpl_of.get(&c) {
                (n, "tmpl_of")
            } else if let Some(&n) = self.sum.origin.get(&c) {
                (n, "origin")
            } else {
                (cs.site, "site")
            };
            if next >= c {
                let f = self.diagnostic_owner(cs.ret).unwrap_or(NO_FN);
                return format!(
                    "SYNTHESIZED call c{c} via [{}] (no source position; made_root={:?} protocol={} bound_synth={} ctor={} contract_new={}) run in {} ret {}",
                    chain.join(" "), self.made_root.get(&c), self.protocol_calls.contains(&c), self.bound_synth.contains(&c),
                    self.ctor_calls.contains(&c), self.contract_new.contains(&c), self.ks2_fn(ks, f), self.ks3_node_pos(ks, cs.ret)
                );
            }
            chain.push(format!("c{c}-{how}->c{next}"));
            c = next;
        }
    }

    /// How `src`'s rules deliver to `dst`: a copy edge, or the constraint
    /// naming it.
    fn ks3_edge(&self, src: NodeId, dst: NodeId) -> String {
        let Some(nd) = self.nodes.get(src as usize) else { return "seed (no source node)".into() };
        if nd.succ.contains(&dst) {
            return "copy edge".into();
        }
        let pat = [format!(": {dst},"), format!(": {dst} "), format!("({dst})")];
        for c in nd.cons.iter() {
            let d = format!("{c:?}");
            if pat.iter().any(|p| d.contains(p.as_str())) {
                return d.chars().take(120).collect();
            }
        }
        "rule of the source node (no constraint names the node)".into()
    }

    /// KS3: the call at the hold, its source position, its callee nodes and
    /// how their first tokens arrived (one hop).
    fn keysnap3(&self, ks: &KeySnap, call: Option<CallId>, tok: Option<Tok>) {
        let Some(c) = call else {
            eprintln!("KS3 no call at the hold");
            if let Some(t) = tok {
                eprintln!("KS3 tok {t} {} name={}", self.ks2_kind(t), self.tok_name(t));
            }
            return;
        };
        eprintln!("KS3 call c{c} {}", self.ks3_call(ks, c));
        let mut callee: Vec<(NodeId, String)> = Vec::new();
        for (n, nd) in self.nodes.iter().enumerate() {
            for k in nd.cons.iter() {
                let how = match k {
                    Cons::Call { call } if *call == c => "Call".to_string(),
                    Cons::New { call } | Cons::NewProgram { call } if *call == c => "New".to_string(),
                    Cons::CallThis { call, this } if *call == c => format!("CallThis this={this}"),
                    Cons::MCall { key, call } if *call == c => format!("MCall(receiver) key={key:?}"),
                    _ => continue,
                };
                callee.push((n as NodeId, how));
            }
        }
        if callee.is_empty() {
            eprintln!("KS3 no node has a call constraint for c{c}");
        }
        for (n, how) in callee.iter().take(4) {
            eprintln!("KS3 callee {how} {}", self.ks2_node(ks, *n));
            eprintln!("KS3   at {}", self.ks3_node_pos(ks, *n));
            for t in self.nodes[*n as usize].pts.to_sorted().into_iter().take(12) {
                let first = match ks.first.get(&(*n, t)) {
                    Some(&(src, why, at)) => {
                        let rs = self.reasons.get(why as usize).map_or("-", |s| s.as_str());
                        let f = at.file();
                        let f = f.rsplit_once("src/").map_or(f, |(_, x)| x);
                        let from = if src == NO_NODE { "seed".to_string() } else { self.ks3_node_pos(ks, src) };
                        format!("from {from} | edge {} | reason {rs:?} | added at {f}:{}", if src == NO_NODE { "-".into() } else { self.ks3_edge(src, *n) }, at.line())
                    }
                    None => "not recorded".into(),
                };
                eprintln!("KS3   tok {t} {} name={} first: {first}", self.ks2_kind(t), self.tok_name(t));
            }
        }
    }
}
