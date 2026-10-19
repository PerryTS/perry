//! Function summaries (design 2026-10-19-CW, two layers).
//!
//! A function's parameter-derived nodes (its slice `D`) are the function's
//! effect over its interface terms. After generation, every summarizable
//! function gets one summary: the constraints that touch `D`, with `D`
//! nodes as slots. Each binding instantiates the summary over fresh slot
//! nodes (effect layer), so callers never merge through the shared
//! parameter nodes. The shared body stays as the observation layer: the
//! `D` nodes are marked `obs`, receive every call's arguments, and only
//! run read rules (read-site facts), never effects.
//!
//! Whether a function is summarized follows from its body alone; nothing
//! depends on a size, count or depth.
use crate::model::*;
use crate::pts::{FxHashMap, FxHashSet};
use std::rc::Rc;

/// A summary term: a slot index with this bit, else an absolute node.
pub const SLOT: u32 = 1 << 31;
/// A call template reference inside a summary's constraints.
pub const CALL: u32 = 1 << 31;

#[derive(Clone, Copy, Debug)]
pub enum Deferred {
    Bind(CallId, Tok),
    Escape(FnIdx, u32),
    Group(Tok, Key, FnIdx, bool),
    RetObjects(CallId, FnIdx, NodeId),
    /// `this` of `call` into `f` (`call_tok`): the call's own operand, or
    /// the given token.
    This(CallId, FnIdx, Option<Tok>),
    /// `this` from a method lookup on a receiver token (`this_to`).
    ThisTo(FnIdx, Tok),
    /// Unknown code calls `f`: its `this` is ⊤.
    EscapeThis(FnIdx, u32),
    /// A birth: `new` of callee token at a call, or of a builtin by name.
    /// Births wait for summaries so a summary's call is born per instance,
    /// never once into the shared body (whose nodes seed every instance).
    Construct(CallId, Tok),
    NewNamed(Atom, CallId),
}

#[derive(Default, Clone, Debug)]
pub struct Summary {
    pub nslots: u32,
    /// Slot index -> the shared (observation) node it was made from.
    pub orig: Vec<NodeId>,
    /// Per parameter: a slot term, an absolute node (a captured parameter:
    /// bound as today), or `NO_NODE` (unused).
    pub params: Vec<NodeId>,
    /// The result term (slot, absolute node or `NO_NODE`).
    pub ret: NodeId,
    /// The `this` term (slot, absolute node or `NO_NODE`: an arrow, or a
    /// `this` a closure captured, keeps the shared node).
    pub this: NodeId,
    pub seeds: Vec<(u32, Vec<Tok>, u16)>,
    pub edges: Vec<(NodeId, NodeId)>,
    pub cons: Vec<(NodeId, Cons)>,
    /// (template call, the call with terms in place of nodes).
    pub calls: Vec<(CallId, CallSite)>,
}

#[derive(Clone, Debug)]
pub struct Inst {
    pub f: FnIdx,
    /// The call site this instance is for (a program call, or the source
    /// site of a call an instance made): what the instance allocates is
    /// born in that context. Shared, unknown-code and accessor instances
    /// have none.
    pub host: Option<CallId>,
    pub params: Vec<NodeId>,
    pub ret: NodeId,
    pub this: NodeId,
}

#[derive(Default)]
pub struct SumState {
    /// Summaries exist: bindings run directly instead of being queued.
    pub ready: bool,
    pub deferred: Vec<Deferred>,
    pub of: FxHashMap<FnIdx, Rc<Summary>>,
    pub insts: Vec<Inst>,
    /// (kind, call site/node, function) -> instance. Kinds: 0 a program
    /// call, 6 a call made for a source call site (a callback called inside
    /// the instance of the program call that passed it, a builtin calling
    /// one, or a template call of any other instance), 2 unknown-code entry,
    /// 3 setter group, 4 getter group, 5 shared (a call inside the callee's
    /// SCC, or a call no call site is behind). An instance never keys on another
    /// instance, so the count is fixed by the program.
    pub inst_key: FxHashMap<(u8, u32, FnIdx), u32>,
    /// Calls that exist when summaries are built: the program's calls.
    pub n_prog: usize,
    /// A call an instance made: the summary call (template) it renames.
    pub tmpl_of: FxHashMap<CallId, CallId>,
    /// A call made after summaries exist: the source call site behind it
    /// (the template it renames, or the site of the call whose solving
    /// made it).
    pub origin: FxHashMap<CallId, CallId>,
    /// A call an instance made (or one made while solving such a call):
    /// the host site of that instance (`Inst::host`).
    pub host: FxHashMap<CallId, CallId>,
    /// (source site, host site) -> birth context id (`TokKind::Obj`).
    pub ctx_ids: FxHashMap<(CallId, CallId), CallId>,
    /// (template call, callee): expanded into every summary that holds the
    /// template; its instances never bind it again.
    pub composed: FxHashSet<(CallId, FnIdx)>,
    /// Template call -> the summarized function whose body makes it.
    pub owner: FxHashMap<CallId, FnIdx>,
    /// Summarized function -> its SCC in the static call graph.
    pub scc: FxHashMap<FnIdx, u32>,
    pub call_inst: FxHashMap<(CallId, FnIdx), u32>,
    /// Calls that belong to a summary: the shared body never runs them.
    pub templates: FxHashSet<CallId>,
    pub stats: String,
    /// Instances whose interface exists but whose entries are not applied
    /// yet: filled from the solver's top level (`fill_instances`), never
    /// nested inside the rule application that asked for them.
    pub pending: Vec<(u32, Vec<NodeId>)>,
    /// Summarized function -> the node of its `this` values no call is
    /// behind (literal methods, accessor groups): feeds every instance.
    pub this_any: FxHashMap<FnIdx, NodeId>,
    /// Summarized function -> its instances.
    pub insts_of: FxHashMap<FnIdx, Vec<u32>>,
    /// TEMP CW s7: what made the kind-6 instances of module factories.
    pub temp_why: FxHashMap<String, usize>,
}

fn is_slot(x: NodeId) -> bool {
    x != NO_NODE && x & SLOT != 0
}

fn act_map(a: ChainAct, n: &mut impl FnMut(NodeId) -> NodeId, k: &mut impl FnMut(CallId) -> CallId) -> ChainAct {
    match a {
        ChainAct::Load(x) => ChainAct::Load(n(x)),
        ChainAct::Store(x) => ChainAct::Store(n(x)),
        ChainAct::Call(c) => ChainAct::Call(k(c)),
    }
}

/// Rename every node and call field of a constraint (every variant).
pub fn map_cons(
    c: &Cons,
    mut n: impl FnMut(NodeId) -> NodeId,
    mut k: impl FnMut(CallId) -> CallId,
) -> Cons {
    use Cons::*;
    match *c {
        Load { key, dst } => Load { key, dst: n(dst) },
        OwnValues { dst } => OwnValues { dst: n(dst) },
        Store { key, src, site } => Store { key, src: n(src), site },
        Call { call } => Call { call: k(call) },
        CallThis { call, this } => CallThis { call: k(call), this },
        New { call } => New { call: k(call) },
        NewNamed { name, call } => NewNamed { name, call: k(call) },
        MCall { key, call } => MCall { key, call: k(call) },
        Chain { origin, key, act } => Chain { origin, key, act: act_map(act, &mut n, &mut k) },
        TopSink { reason } => TopSink { reason },
        TopIfAny { dst, from, reason } => TopIfAny { dst: n(dst), from: n(from), reason },
        Hard { kind, reason } => Hard { kind, reason },
        AddsKeys => AddsKeys,
        Implicit { reason } => Implicit { reason },
        CreateProto { reason } => CreateProto { reason },
        ChainUnion { key, dst } => ChainUnion { key, dst: n(dst) },
        ProtoClosure { dst } => ProtoClosure { dst: n(dst) },
        ProtoMethods { origin, key, call } => ProtoMethods { origin, key, call: k(call) },
        AnyAccessors { origin, act } => AnyAccessors { origin, act: act_map(act, &mut n, &mut k) },
        StoreTopProto { origin, src } => StoreTopProto { origin, src: n(src) },
        Super { start, key, act } => Super { start, key, act: act_map(act, &mut n, &mut k) },
        SuperCtor { class, call } => SuperCtor { class, call: k(call) },
        SuperCtorParent { class, call, origin } => SuperCtorParent { class, call: k(call), origin },
        ThisFrom { recv } => ThisFrom { recv: n(recv) },
        Accessor { holder, key, setter } => Accessor { holder: n(holder), key, setter },
        AccessorInstall { key, f, setter } => AccessorInstall { key, f, setter },
        ProtoFrom { src } => ProtoFrom { src: n(src) },
        ProtoOf { dst } => ProtoOf { dst: n(dst) },
        IterNext { dst } => IterNext { dst: n(dst) },
        NewTargetHard { reason } => NewTargetHard { reason },
        IdxLoad { idx, dst } => IdxLoad { idx: n(idx), dst: n(dst) },
        IdxKey { obj, dst } => IdxKey { obj, dst: n(dst) },
        Arith { dst, add } => Arith { dst: n(dst), add },
        ShareBuffer { ta } => ShareBuffer { ta },
        ThisTo { origin } => ThisTo { origin },
        CallArgs { call } => CallArgs { call: k(call) },
        BuiltinCallee => BuiltinCallee,
        HolderStep { key, pass, stop } => HolderStep { key, pass: n(pass), stop: n(stop) },
        VisitHolders { origin, key, act } => VisitHolders { origin, key, act: act_map(act, &mut n, &mut k) },
        VisitStop { origin, key, act } => VisitStop { origin, key, act: act_map(act, &mut n, &mut k) },
    }
}

pub fn map_call(c: &CallSite, mut n: impl FnMut(NodeId) -> NodeId) -> CallSite {
    CallSite {
        this: match c.this {
            ThisArg::Node(x) => ThisArg::Node(n(x)),
            ThisArg::Tok(t) => ThisArg::Tok(t),
            ThisArg::Undef => ThisArg::Undef,
        },
        args: c.args.iter().map(|&x| n(x)).collect(),
        spread: c.spread.iter().map(|&x| n(x)).collect(),
        ret: n(c.ret),
        site: c.site,
    }
}

fn cons_fields(c: &Cons) -> (Vec<NodeId>, Vec<CallId>) {
    let mut ns = Vec::new();
    let mut ks = Vec::new();
    map_cons(c, |x| {
        ns.push(x);
        x
    }, |k| {
        ks.push(k);
        k
    });
    (ns, ks)
}

fn call_nodes(c: &CallSite) -> Vec<NodeId> {
    let mut v = Vec::new();
    map_call(c, |x| {
        v.push(x);
        x
    });
    v
}

/// The constraint kinds generation attaches to program nodes: the summary
/// language. The solver's own relays are rebuilt by applying these.
fn gen_kind(c: &Cons) -> bool {
    use Cons::*;
    matches!(
        c,
        Load { .. }
            | OwnValues { .. }
            | Store { .. }
            | Call { .. }
            | New { .. }
            | NewNamed { .. }
            | SuperCtor { .. }
            | MCall { .. }
            | TopSink { .. }
            | TopIfAny { .. }
            | Hard { .. }
            | AddsKeys
            | Implicit { .. }
            | IdxLoad { .. }
            | IdxKey { .. }
            | Arith { .. }
            | IterNext { .. }
            | ProtoOf { .. }
            | ThisFrom { .. }
            | CreateProto { .. }
            | NewTargetHard { .. }
            | ShareBuffer { .. }
            | Accessor { .. }
            | ProtoFrom { .. }
    )
}

/// Read rules: their only output is `dst`; per-token effects (getters,
/// protocols) are the same for every call.
fn read_dst(c: &Cons) -> Option<NodeId> {
    use Cons::*;
    match *c {
        Load { dst, .. }
        | OwnValues { dst }
        | IdxLoad { dst, .. }
        | IdxKey { dst, .. }
        | Arith { dst, .. }
        | IterNext { dst }
        | ProtoOf { dst } => Some(dst),
        _ => None,
    }
}

impl Model {
    /// The shared body skips this rule on `n`: an effect on an observation
    /// node, or a call that belongs to a summary.
    pub(crate) fn sum_skip(&self, n: NodeId, c: &Cons) -> bool {
        if !self.sum.ready {
            return false;
        }
        if self.nodes[n as usize].obs {
            let dst = match *c {
                Cons::TopIfAny { dst, .. } => Some(dst),
                _ => read_dst(c),
            };
            return !dst.is_some_and(|d| self.nodes[d as usize].obs);
        }
        if self.sum.templates.is_empty() {
            return false;
        }
        let call = match *c {
            Cons::Call { call }
            | Cons::CallThis { call, .. }
            | Cons::New { call }
            | Cons::NewNamed { call, .. }
            | Cons::MCall { call, .. }
            | Cons::ProtoMethods { call, .. }
            | Cons::SuperCtor { call, .. }
            | Cons::SuperCtorParent { call, .. }
            | Cons::CallArgs { call } => call,
            Cons::Chain { act: ChainAct::Call(call), .. }
            | Cons::AnyAccessors { act: ChainAct::Call(call), .. }
            | Cons::Super { act: ChainAct::Call(call), .. }
            | Cons::VisitHolders { act: ChainAct::Call(call), .. }
            | Cons::VisitStop { act: ChainAct::Call(call), .. } => call,
            _ => return false,
        };
        self.sum.templates.contains(&call)
    }

    /// The source call site behind `call`: itself for a call of the program.
    fn origin_of(&self, call: CallId) -> Option<CallId> {
        if (call as usize) < self.sum.n_prog {
            Some(call)
        } else {
            self.sum.origin.get(&call).copied()
        }
    }

    /// Calls made while solving `call` have the same call site behind them
    /// (a builtin calling a callback for it).
    pub(crate) fn note_origin(&mut self, n0: usize, call: CallId) {
        if !self.sum.ready || self.calls.len() == n0 {
            return;
        }
        if let Some(o) = self.origin_of(call) {
            for i in n0..self.calls.len() {
                self.sum.origin.entry(i as CallId).or_insert(o);
            }
        }
        if let Some(&h) = self.sum.host.get(&call) {
            for i in n0..self.calls.len() {
                self.sum.host.entry(i as CallId).or_insert(h);
            }
        }
    }

    /// The birth context of an object `call` makes: a program call is its
    /// own (one token per birth site). A call an instance made is its
    /// source site within the instance's host site, so a constructor's
    /// allocations are per `new` site, not one for every instance. Calls
    /// made while solving another take that call's context.
    pub(crate) fn birth_ctx(&mut self, call: CallId) -> CallId {
        // TEMP CW s7 toggles: T2 = template tokens only, T1 = no host.
        if std::env::var_os("CW_T2").is_some() {
            return NO_CTX;
        }
        if std::env::var_os("CW_T1").is_some() && (call as usize) >= self.sum.n_prog && self.sum.ready {
            return self.sum.tmpl_of.get(&call).copied().unwrap_or_else(|| self.origin_of(call).unwrap_or(call));
        }
        if (call as usize) < self.sum.n_prog || !self.sum.ready {
            return call;
        }
        let src = match self.sum.tmpl_of.get(&call) {
            Some(&t) => t,
            None => self.origin_of(call).unwrap_or(call),
        };
        let Some(&h) = self.sum.host.get(&call) else {
            return src;
        };
        let n = self.sum.ctx_ids.len() as CallId;
        // Context ids count down from below `NO_CTX`: never a call id.
        *self.sum.ctx_ids.entry((src, h)).or_insert(NO_CTX - 1 - n)
    }

    pub(crate) fn summarized(&self, f: FnIdx) -> bool {
        self.sum.of.contains_key(&f)
    }

    /// The instance for key, made on first use.
    fn instance(&mut self, key: (u8, u32, FnIdx)) -> u32 {
        if let Some(&i) = self.sum.inst_key.get(&key) {
            return i;
        }
        let f = key.2;
        let s = self.sum.of[&f].clone();
        let inst = self.sum.insts.len() as u32;
        let base: Vec<NodeId> = (0..s.nslots)
            .map(|i| {
                let n = self.node_named("inst", || format!("inst{inst}.{i}"));
                n
            })
            .collect();
        let tr = |x: NodeId| if is_slot(x) { base[(x & !SLOT) as usize] } else { x };
        let this = tr(s.this);
        self.sum.insts.push(Inst {
            f,
            host: matches!(key.0, 0 | 6).then_some(key.1),
            params: s.params.iter().map(|&p| tr(p)).collect(),
            ret: tr(s.ret),
            this,
        });
        self.sum.inst_key.insert(key, inst);
        self.sum.insts_of.entry(f).or_default().push(inst);
        self.sum.pending.push((inst, base));
        if let Some(&u) = self.sum.this_any.get(&f) {
            if this != NO_NODE {
                self.edge(u, this);
            }
        }
        inst
    }

    /// The instance `call` binds for summarized `f` (None: the template call
    /// was expanded into the caller's summary).
    fn inst_for(&mut self, call: CallId, f: FnIdx) -> Option<u32> {
        if let Some(&i) = self.sum.call_inst.get(&(call, f)) {
            return Some(i);
        }
        // A call an instance made from a template expanded into the
        // summary: the instance already holds `f`'s effect.
        let t = self.sum.tmpl_of.get(&call).copied();
        if t.is_some_and(|t| self.sum.composed.contains(&(t, f))) {
            return None;
        }
        // A call inside `f`'s SCC (recursion), or one no program call is
        // behind (protocol touch, bound merge, timer, implicit call): the
        // shared instance. Every other call: the instance for the source call
        // site behind it, so the count is sites x targets, fixed by the
        // program.
        let in_scc = t.is_some_and(|t| {
            self.sum.owner.get(&t).and_then(|o| self.sum.scc.get(o)) == self.sum.scc.get(&f)
        });
        let key = if (call as usize) < self.sum.n_prog {
            (0, call, f)
        } else {
            match self.sum.origin.get(&call) {
                Some(&o) if !in_scc => {
                    if !self.sum.inst_key.contains_key(&(6, o, f)) && self.funcs[f as usize].name.contains("__perry_cjs_factory") {
                        let tt = t.map(|t| {
                            let ow = self.sum.owner.get(&t).map(|g| self.funcs[*g as usize].name.clone()).unwrap_or_default();
                            let tc = &self.calls[t as usize];
                            format!("tmpl owner={ow} site={} args={}", tc.site, tc.args.len())
                        });
                        let w = format!(
                            "{} bsynth={} proto={}",
                            tt.unwrap_or_else(|| format!("derived site={} args={}", self.calls[call as usize].site, self.calls[call as usize].args.len())),
                            self.bound_synth.contains(&call),
                            self.protocol_calls.contains(&call)
                        );
                        *self.sum.temp_why.entry(w).or_default() += 1;
                    }
                    (6, o, f)
                }
                _ => (5, 0, f),
            }
        };
        let inst = self.instance(key);
        self.sum.call_inst.insert((call, f), inst);
        Some(inst)
    }

    /// The `this` term of summarized `f`'s instances, if it has one.
    fn has_this_term(&self, f: FnIdx) -> bool {
        self.sum.of.get(&f).is_some_and(|s| s.this != NO_NODE)
    }

    /// `this` of `call` into `f`: the shared node observes it; `f`'s
    /// instance for the call gets the explicit token (the call's own
    /// operand reaches it in `bind_summarized`).
    pub(crate) fn this_into(&mut self, call: CallId, f: FnIdx, this_tok: Option<Tok>) {
        if !self.sum.ready {
            self.sum.deferred.push(Deferred::This(call, f, this_tok));
            return;
        }
        let fthis = self.funcs[f as usize].this;
        match (this_tok, &self.calls[call as usize].this) {
            (Some(x), _) => self.add_tok(fthis, x),
            (None, &ThisArg::Tok(x)) => self.add_tok(fthis, x),
            (None, &ThisArg::Node(n)) => self.edge(n, fthis),
            (None, ThisArg::Undef) => self.add_prim(fthis, prim::UNDEF),
        }
        if let Some(x) = this_tok {
            if self.has_this_term(f) && !self.sum.templates.contains(&call) {
                if let Some(i) = self.inst_for(call, f) {
                    let it = self.sum.insts[i as usize].this;
                    self.add_tok(it, x);
                }
            }
        }
    }

    /// `this` of `f` from a method lookup on receiver `origin`: the shared
    /// node observes it; the instances get the call's receiver operand.
    pub(crate) fn this_lookup(&mut self, f: FnIdx, origin: Tok) {
        if !self.sum.ready {
            self.sum.deferred.push(Deferred::ThisTo(f, origin));
            return;
        }
        let this = self.funcs[f as usize].this;
        self.add_tok(this, origin);
    }

    /// The node that takes `this` values of `f` no call is behind: every
    /// instance of `f` (and its shared instance, so the effect runs even
    /// if no call ever does) sees them.
    pub(crate) fn this_any(&mut self, f: FnIdx) -> NodeId {
        let fthis = self.funcs[f as usize].this;
        if !self.sum.ready || !self.has_this_term(f) {
            return fthis;
        }
        if let Some(&u) = self.sum.this_any.get(&f) {
            return u;
        }
        let u = self.node_named("this-any", || format!("this-any({f})"));
        self.sum.this_any.insert(f, u);
        self.edge(u, fthis);
        for i in self.sum.insts_of.get(&f).cloned().unwrap_or_default() {
            let it = self.sum.insts[i as usize].this;
            self.edge(u, it);
        }
        self.instance((5, 0, f));
        u
    }

    /// Unknown code calls `f`: its `this` is ⊤ (the unknown-code instance's
    /// and the shared node).
    pub(crate) fn escape_this(&mut self, f: FnIdx, reason: u32) {
        if !self.sum.ready {
            self.sum.deferred.push(Deferred::EscapeThis(f, reason));
            return;
        }
        let why = |m: &Self| format!("this of escaped function: {}", m.reasons[reason as usize]);
        let mut dsts = vec![self.funcs[f as usize].this];
        if self.has_this_term(f) {
            let inst = self.instance((2, 0, f));
            dsts.push(self.sum.insts[inst as usize].this);
        }
        for d in dsts {
            self.add_top(d, why);
            self.add_prim(d, prim::ALL);
        }
    }

    /// Apply the entries of every pending instance (a worklist: instances
    /// these entries ask for are queued behind, never built recursively).
    pub(crate) fn fill_instances(&mut self) {
        let mut i = 0;
        while i < self.sum.pending.len() {
            let (inst, base) = std::mem::take(&mut self.sum.pending[i]);
            i += 1;
            self.fill_instance(inst, &base);
        }
        self.sum.pending.clear();
    }

    fn fill_instance(&mut self, inst: u32, base: &[NodeId]) {
        let f = self.sum.insts[inst as usize].f;
        let s = self.sum.of[&f].clone();
        let host = self.sum.insts[inst as usize].host;
        let tr = |x: NodeId| if is_slot(x) { base[(x & !SLOT) as usize] } else { x };
        // Calls whose callee or receiver is an input term (a parameter or
        // `this`): what they bind is what the caller passed.
        let inputs: FxHashSet<NodeId> = s.params.iter().chain([&s.this]).copied().filter(|&x| is_slot(x)).collect();
        let mut by_input: FxHashSet<u32> = FxHashSet::default();
        for (h, c) in &s.cons {
            if inputs.contains(h) {
                let (_, ks) = cons_fields(c);
                for k in ks {
                    if k != NO_NODE && k & CALL != 0 {
                        by_input.insert(k & !CALL);
                    }
                }
            }
        }
        let mut kmap = Vec::with_capacity(s.calls.len());
        for (ci, (t, cs)) in s.calls.iter().enumerate() {
            let id = self.calls.len() as CallId;
            self.calls.push(map_call(cs, tr));
            self.sum.tmpl_of.insert(id, *t);
            // Keyed by the instance's host when it calls an input (the
            // callback or receiver the caller passed), else by its own
            // source site: one level of call-site context either way, so
            // instances never multiply through instances.
            let key_site = match host {
                Some(h) if by_input.contains(&(ci as u32)) => h,
                _ => *t,
            };
            self.sum.origin.insert(id, key_site);
            if let Some(h) = host {
                self.sum.host.insert(id, h);
            }
            if let Some(&a) = self.call_lit.get(t) {
                self.call_lit.insert(id, a);
            }
            kmap.push(id);
        }
        for &(slot, ref toks, prim) in &s.seeds {
            self.add(base[slot as usize], toks, prim);
        }
        for &(a, b) in &s.edges {
            self.edge(tr(a), tr(b));
        }
        for (i, (t, cs)) in s.calls.iter().enumerate() {
            // The shared result node observes every instance's result.
            if is_slot(cs.ret) {
                let r = self.calls[kmap[i] as usize].ret;
                let orig = self.calls[*t as usize].ret;
                self.edge(r, orig);
            }
        }
        for (h, c) in &s.cons {
            let c2 = map_cons(c, tr, |k| if k != NO_NODE && k & CALL != 0 { kmap[(k & !CALL) as usize] } else { k });
            self.cons(tr(*h), c2);
        }
    }

    /// Arguments and result of `call` into summarized `f`: its instance.
    pub(crate) fn bind_summarized(&mut self, call: CallId, f: FnIdx) {
        let Some(inst) = self.inst_for(call, f) else {
            return;
        };
        let it = self.sum.insts[inst as usize].this;
        if it != NO_NODE {
            match self.calls[call as usize].this {
                ThisArg::Node(n) => self.edge(n, it),
                ThisArg::Tok(x) => self.add_tok(it, x),
                ThisArg::Undef => self.add_prim(it, prim::UNDEF),
            }
        }
        let terms = self.sum.insts[inst as usize].params.clone();
        let fparams = self.funcs[f as usize].params.clone();
        let c = call as usize;
        let nargs = self.calls[c].args.len();
        for i in 0..terms.len() {
            let mut dsts = Vec::with_capacity(2);
            if terms[i] != NO_NODE {
                dsts.push(terms[i]);
            }
            if self.nodes[fparams[i] as usize].obs {
                dsts.push(fparams[i]);
            }
            for &d in &dsts {
                if i < nargs {
                    let a = self.calls[c].args[i];
                    self.edge(a, d);
                } else {
                    self.add_prim(d, prim::UNDEF);
                }
            }
        }
        for j in 0..self.calls[c].spread.len() {
            let s = self.calls[c].spread[j];
            let elems = self.node_l("spread-elems");
            self.cons(s, Cons::Load { key: Key::Any, dst: elems });
            for i in 0..terms.len() {
                if terms[i] != NO_NODE {
                    self.edge(elems, terms[i]);
                }
                if self.nodes[fparams[i] as usize].obs {
                    self.edge(elems, fparams[i]);
                }
            }
        }
        let r = self.sum.insts[inst as usize].ret;
        if r != NO_NODE {
            let cret = self.calls[c].ret;
            self.edge(r, cret);
        }
    }

    /// The result node `call` sees from `f` (its instance's, if summarized).
    pub(crate) fn ret_for(&self, call: CallId, f: FnIdx) -> NodeId {
        match self.sum.call_inst.get(&(call, f)) {
            Some(&i) => self.sum.insts[i as usize].ret,
            None => self.funcs[f as usize].ret,
        }
    }

    /// Unknown code calls summarized `f`: unknown arguments, the result escapes.
    pub(crate) fn escape_summarized(&mut self, f: FnIdx, reason: u32) {
        let inst = self.instance((2, 0, f));
        let why = |m: &Self| format!("param of escaped function: {}", m.reasons[reason as usize]);
        let terms = self.sum.insts[inst as usize].params.clone();
        let fparams = self.funcs[f as usize].params.clone();
        for (i, &p) in fparams.iter().enumerate() {
            for d in [terms[i], p] {
                if d != NO_NODE {
                    self.add_top(d, why);
                    self.add_prim(d, prim::ALL);
                }
            }
        }
        let r = self.sum.insts[inst as usize].ret;
        if r != NO_NODE {
            self.sink_node(r, reason);
        }
    }

    /// Accessor group binding of summarized `f`: the group's setter value
    /// into the first parameter, or the getter result into the group's.
    pub(crate) fn group_summarized(&mut self, f: FnIdx, sp: NodeId, ar: NodeId, setter: bool) {
        if setter {
            let inst = self.instance((3, sp, f));
            if let Some(&p0) = self.funcs[f as usize].params.first() {
                let t = self.sum.insts[inst as usize].params[0];
                if t != NO_NODE {
                    self.edge(sp, t);
                }
                if self.nodes[p0 as usize].obs {
                    self.edge(sp, p0);
                }
            }
        } else {
            let inst = self.instance((4, ar, f));
            let r = self.sum.insts[inst as usize].ret;
            if r != NO_NODE {
                self.edge(r, ar);
            }
        }
    }

    /// After generation: summaries, observation nodes, then every queued
    /// binding.
    pub fn summarize_and_replay(&mut self) {
        let t0 = std::time::Instant::now();
        self.sum.n_prog = self.calls.len();
        build_summaries(self);
        self.sum.ready = true;
        let deferred = std::mem::take(&mut self.sum.deferred);
        let nd = deferred.len();
        for d in deferred {
            match d {
                Deferred::Bind(call, t) => self.bind_args(call, t),
                Deferred::Escape(f, reason) => self.escape_fn_params(f, reason),
                Deferred::Group(holder, key, f, setter) => self.group_bind(holder, key, f, setter),
                Deferred::RetObjects(call, f, dst) => self.ctor_ret_objects(call, f, dst),
                Deferred::This(call, f, x) => self.this_into(call, f, x),
                Deferred::ThisTo(f, origin) => self.this_lookup(f, origin),
                Deferred::EscapeThis(f, reason) => self.escape_this(f, reason),
                // A summary's call: its instances construct.
                Deferred::Construct(call, _) | Deferred::NewNamed(_, call)
                    if self.sum.templates.contains(&call) => {}
                Deferred::Construct(call, t) => self.construct(call, t),
                Deferred::NewNamed(name, call) => self.new_named(name, call),
            }
        }
        // A `this` with values from generation (static blocks, field
        // initializers, literal methods): the shared instance runs the
        // effect even when no call does.
        let mut fs: Vec<FnIdx> = self.sum.of.keys().copied().collect();
        fs.sort_unstable();
        for f in fs {
            let s = self.sum.of[&f].clone();
            if is_slot(s.this) {
                let sl = s.this & !SLOT;
                if s.seeds.iter().any(|x| x.0 == sl) || s.edges.iter().any(|e| e.1 == s.this && !is_slot(e.0)) {
                    self.instance((5, 0, f));
                }
            }
        }
        self.fill_instances();
        self.sum.stats = format!(
            "{} replayed {} in {:.1} ms",
            self.sum.stats,
            nd,
            t0.elapsed().as_secs_f64() * 1000.0
        );
    }
}

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
    let owned = |x: NodeId| x != NO_NODE && (x as usize) < m.node_fn.len() && m.node_fn[x as usize] != NO_FN;
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

/// The parameter-derived nodes of `f`: what its parameters reach by copy,
/// every field of a rule on such a node, and the results of calls with such
/// an operand. Only nodes `f` owns and no closure captured.
fn slice(m: &Model, ix: &Index, f: FnIdx) -> Vec<NodeId> {
    let fi = &m.funcs[f as usize];
    let ok = |y: NodeId| {
        y != NO_NODE
            && m.node_fn[y as usize] == f
            && (y != fi.this || !fi.lexical_this)
            && y != fi.args_all
            && !m.node_captured.contains(&y)
    };
    let mut seen: FxHashSet<NodeId> = FxHashSet::default();
    let mut d = Vec::new();
    let this = (!fi.lexical_this).then_some(fi.this);
    for &p in fi.params.iter().chain(this.iter()) {
        if ok(p) && seen.insert(p) {
            d.push(p);
        }
    }
    let mut i = 0;
    while i < d.len() {
        let x = d[i];
        i += 1;
        let mut add = |y: NodeId, d: &mut Vec<NodeId>| {
            if ok(y) && seen.insert(y) {
                d.push(y);
            }
        };
        let node = &m.nodes[x as usize];
        for &y in node.succ.iter() {
            add(y, &mut d);
        }
        for c in &node.cons {
            let (ns, ks) = cons_fields(c);
            for y in ns {
                add(y, &mut d);
            }
            for k in ks {
                add(m.calls[k as usize].ret, &mut d);
            }
        }
        if let Some(rs) = ix.refs.get(&x) {
            for &(h, ci) in rs {
                let (ns, ks) = cons_fields(&m.nodes[h as usize].cons[ci as usize]);
                for y in ns {
                    add(y, &mut d);
                }
                for k in ks {
                    add(m.calls[k as usize].ret, &mut d);
                }
            }
        }
        if let Some(cs) = ix.calls_of.get(&x) {
            for &k in cs {
                add(m.calls[k as usize].ret, &mut d);
            }
        }
    }
    d
}

fn build_summaries(m: &mut Model) {
    let ix = build_index(m);
    // 1. Slices of every candidate.
    let mut slices: FxHashMap<FnIdx, Vec<NodeId>> = FxHashMap::default();
    for f in 0..m.funcs.len() as FnIdx {
        let fi = &m.funcs[f as usize];
        if fi.no_summary || fi.rest.is_some() || fi.has_arguments || (fi.params.is_empty() && fi.lexical_this) {
            continue;
        }
        let d = slice(m, &ix, f);
        if !d.is_empty() {
            slices.insert(f, d);
        }
    }
    let mut dof: FxHashMap<NodeId, FnIdx> = FxHashMap::default();
    for (&f, d) in &slices {
        for &x in d {
            dof.insert(x, f);
        }
    }
    // 2. Entries; a slice touching another function's slice, a solver rule
    //    on a slice node, or a call with a slice operand that no rule runs
    //    keeps the function's shared binding.
    let mut refused: FxHashSet<FnIdx> = FxHashSet::default();
    let mut why: FxHashMap<FnIdx, String> = FxHashMap::default();
    struct Entries {
        cons: Vec<(NodeId, u32)>,
        calls: Vec<CallId>,
    }
    let mut entries: FxHashMap<FnIdx, Entries> = FxHashMap::default();
    let mut fs: Vec<FnIdx> = slices.keys().copied().collect();
    fs.sort_unstable();
    for &f in &fs {
        let d = &slices[&f];
        let mut bad = false;
        let other = |x: NodeId, bad: &mut bool, refused: &mut FxHashSet<FnIdx>| {
            if let Some(&g) = dof.get(&x) {
                if g != f {
                    *bad = true;
                    refused.insert(g);
                }
            }
        };
        let mut cons: Vec<(NodeId, u32)> = Vec::new();
        let mut cons_seen: FxHashSet<(NodeId, u32)> = FxHashSet::default();
        let mut calls: Vec<CallId> = Vec::new();
        let mut calls_seen: FxHashSet<CallId> = FxHashSet::default();
        let mut run_calls: FxHashSet<CallId> = FxHashSet::default();
        let mut take = |h: NodeId, i: u32, m: &Model, cons: &mut Vec<(NodeId, u32)>| {
            if cons_seen.insert((h, i)) {
                cons.push((h, i));
            }
            let _ = m;
        };
        for &x in d {
            for (i, c) in m.nodes[x as usize].cons.iter().enumerate() {
                if !gen_kind(c) {
                    bad = true;
                    why.entry(f).or_insert_with(|| format!("solver rule {} on {}", c.kind_name(), m.labels[x as usize]));
                }
                take(x, i as u32, m, &mut cons);
            }
            if let Some(rs) = ix.refs.get(&x) {
                for &(h, i) in rs {
                    if dof.get(&h) == Some(&f) {
                        continue;
                    }
                    if gen_kind(&m.nodes[h as usize].cons[i as usize]) {
                        take(h, i, m, &mut cons);
                    }
                }
            }
            for &y in m.nodes[x as usize].succ.iter() {
                other(y, &mut bad, &mut refused);
            }
            if let Some(ps) = ix.preds.get(&x) {
                for &p in ps {
                    other(p, &mut bad, &mut refused);
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
                            if gen_kind(&m.nodes[h as usize].cons[i as usize]) {
                                take(h, i, m, &mut cons);
                            }
                        }
                    }
                }
            }
        }
        // Every node and call the entries mention.
        for &(h, i) in &cons {
            other(h, &mut bad, &mut refused);
            let (ns, ks) = cons_fields(&m.nodes[h as usize].cons[i as usize]);
            for y in ns {
                other(y, &mut bad, &mut refused);
            }
            for k in ks {
                run_calls.insert(k);
                if calls_seen.insert(k) {
                    calls.push(k);
                }
            }
        }
        for &k in &calls {
            for y in call_nodes(&m.calls[k as usize]) {
                other(y, &mut bad, &mut refused);
            }
            if !run_calls.contains(&k) {
                bad = true;
                why.entry(f).or_insert_with(|| format!("call {k} run by no rule"));
            }
        }
        if bad {
            why.entry(f).or_insert_with(|| "touches another summarized slice".into());
            refused.insert(f);
        }
        entries.insert(f, Entries { cons, calls });
    }
    // 3. Summaries.
    let mut nsum = 0usize;
    let mut size = 0usize;
    for &f in &fs {
        if refused.contains(&f) {
            continue;
        }
        let d = &slices[&f];
        let e = &entries[&f];
        let mut slot: FxHashMap<NodeId, u32> = FxHashMap::default();
        for (i, &x) in d.iter().enumerate() {
            slot.insert(x, i as u32);
        }
        let enc = |x: NodeId| match slot.get(&x) {
            Some(&i) => SLOT | i,
            None => x,
        };
        let mut callix: FxHashMap<CallId, u32> = FxHashMap::default();
        let mut s = Summary { nslots: d.len() as u32, orig: d.clone(), ..Default::default() };
        for (i, &k) in e.calls.iter().enumerate() {
            callix.insert(k, i as u32);
            s.calls.push((k, map_call(&m.calls[k as usize], enc)));
        }
        let kenc = |k: CallId| match callix.get(&k) {
            Some(&i) => CALL | i,
            None => k,
        };
        for &x in d {
            let node = &m.nodes[x as usize];
            if !node.pts.is_empty() || node.prim != 0 {
                s.seeds.push((slot[&x], node.pts.to_sorted(), node.prim));
            }
            for &y in node.succ.iter() {
                s.edges.push((enc(x), enc(y)));
            }
            if let Some(ps) = ix.preds.get(&x) {
                for &p in ps {
                    if !slot.contains_key(&p) {
                        s.edges.push((p, enc(x)));
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
        s.this = if m.funcs[f as usize].lexical_this { NO_NODE } else { enc(m.funcs[f as usize].this) };
        if !is_slot(s.this) {
            s.this = NO_NODE;
        }
        compress(&mut s);
        size += s.edges.len() + s.cons.len() + s.calls.len() + s.seeds.len();
        nsum += 1;
        for &k in &e.calls {
            m.sum.templates.insert(k);
            m.sum.owner.insert(k, f);
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
    let size_after: usize = m
        .sum
        .of
        .values()
        .map(|s| s.edges.len() + s.cons.len() + s.calls.len() + s.seeds.len())
        .sum();
    // 4. Observation layer: the slice nodes of summarized functions.
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
                let set: crate::pts::FxHashSet<NodeId> =
                    node.succ.iter().copied().collect();
                node.succ_set = Some(Rc::new(set));
            }
        }
    }
    if m.explain {
        let mut v: Vec<_> = refused.iter().copied().collect();
        v.sort_unstable();
        for f in v {
            let w = why.get(&f).map_or("touched by another summarized slice", |s| s.as_str());
            eprintln!("perry: wpfacts explain: summary refused {}: {w}", m.funcs[f as usize].name);
        }
    }
    m.sum.stats = format!(
        "summaries {} of {} candidates ({} refused), size {}, composed {} calls over {} SCCs, size {}",
        nsum,
        fs.len(),
        refused.len(),
        size,
        ncomposed,
        nsccs,
        size_after
    );
}

/// The summarized functions a summary's call `i` reaches through a callee
/// node that is not a slot: `(i, functions)` for its own calls.
fn static_callees(m: &Model, s: &Summary) -> Vec<(usize, Vec<FnIdx>)> {
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
                TokKind::Func(g) if m.sum.of.contains_key(&g) => Some(g),
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
/// into the caller's summary (design CW2 3.2). SCCs come out of Tarjan's
/// walk callees first, so a callee's summary is final when it is copied.
/// Returns (expanded calls, SCCs).
fn compose(m: &mut Model, obs_of: &dyn Fn(FnIdx, &Model) -> Vec<NodeId>) -> (usize, usize) {
    let mut fs: Vec<FnIdx> = m.sum.of.keys().copied().collect();
    fs.sort_unstable();
    let direct: FxHashMap<FnIdx, Vec<(usize, Vec<FnIdx>)>> =
        fs.iter().map(|&f| (f, static_callees(m, &m.sum.of[&f]))).collect();
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
            let mut v: Vec<FnIdx> = direct[&f].iter().flat_map(|(_, gs)| gs.iter().copied()).collect();
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
                    let t = s.calls[*ci].0;
                    if !m.sum.composed.insert((t, g)) {
                        continue;
                    }
                    let sg = m.sum.of[&g].clone();
                    expand(&mut s, *ci, &sg, &obs_of(g, m));
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
    (n, order.len())
}

/// Copy `g`'s summary into `s` over fresh slots and wire call `ci`'s
/// operands to it the way a binding would: arguments into the parameters
/// (and into `g`'s observation parameters `gobs`), missing ones undefined,
/// spread elements into every parameter, `g`'s result into the call's.
fn expand(s: &mut Summary, ci: usize, g: &Summary, gobs: &[NodeId]) {
    let off = s.nslots;
    let coff = s.calls.len() as u32;
    let tr = |x: NodeId| if is_slot(x) { SLOT | (off + (x & !SLOT)) } else { x };
    let kr = |k: CallId| if k != NO_NODE && k & CALL != 0 { CALL | (coff + (k & !CALL)) } else { k };
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
            if i < call.args.len() {
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
        s.cons.push((sp, Cons::Load { key: Key::Any, dst: elems }));
        for ds in &dsts {
            for &d in ds {
                s.edges.push((elems, d));
            }
        }
    }
    let tdsts: Vec<NodeId> = [tr(g.this), gobs.get(g.params.len()).copied().unwrap_or(NO_NODE)]
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
    let sl = |x: NodeId| if is_slot(x) { Some((x & !SLOT) as usize) } else { None };
    let mut pinned = vec![false; n];
    for &p in s.params.iter().chain([&s.ret, &s.this]) {
        if let Some(i) = sl(p) {
            pinned[i] = true;
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
            let Some(i) = read_dst(c).and_then(sl) else { continue };
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
        let mut v: Vec<NodeId> = inn[i].iter().copied().filter(|&a| sl(a).is_none()).collect();
        v.sort_unstable();
        for a in v {
            edges.push((a, rn(SLOT | i as u32)));
        }
    }
    let mut ae: Vec<_> = abs_edges.into_iter().collect();
    ae.sort_unstable();
    edges.extend(ae);
    s.edges = edges;
    s.cons = cons.into_iter().flatten().map(|(h, c)| (rn(h), map_cons(&c, rn, |k| k))).collect();
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
    s.nslots = orig.len() as u32;
    s.orig = orig;
}
