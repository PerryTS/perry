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
/// The birth context of an object no program site is behind (a literal
/// born in a context-free instance, a birth of a call the solver made for
/// no site): one token per template. Birth context ids count down from it.
pub const BORN: CallId = NO_CTX - 1;
/// Marks a literal template (not a call site) in a birth context key.
const LIT: u32 = 1 << 31;
/// The layout of no object receiver (a primitive, `undefined`, a function
/// without a `this` term).
pub const LAY_NONE: u32 = 0;
/// The lexical context of a kind-0 instance: its program site.
const LAY_SITE: u32 = 5 << 28;
/// The lexical context of a kind-5 instance: the object it constructs.
const LAY_BIRTH: u32 = 6 << 28;

#[derive(Clone, Copy, Debug)]
pub enum Deferred {
    Bind(CallId, Tok),
    Escape(Tok, u32),
    Group(Tok, Key, Tok, bool),
    RetObjects(CallId, Tok, NodeId),
    /// `this` of `call` into function token `t` (`call_tok`): the call's
    /// own operand, or the given token.
    This(CallId, Tok, Option<Tok>),
    /// `this` from a method lookup on a receiver token (`this_to`).
    ThisTo(Tok, Tok),
    /// Unknown code calls the function: its `this` is ⊤.
    EscapeThis(Tok, u32),
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
    /// The slots of the function's outer terms (`SumState::outer_of`
    /// order): nodes of enclosing summaries it captured. An instance takes
    /// them from its closure context, never fresh.
    pub outer: Vec<u32>,
    /// Closures this body creates with outer terms: (slot of the closure
    /// value, closure, its outer terms as terms of this summary). Each
    /// instance seeds the slot with the closure token of its own context.
    pub closures: Vec<(u32, FnIdx, Vec<NodeId>)>,
    /// Literals this body births: (slot of the birth node, template). Each
    /// instance seeds its own site token there; the template token stays
    /// on the observation nodes.
    pub births: Vec<(u32, TmplId)>,
}

#[derive(Clone, Debug)]
pub struct Inst {
    pub f: FnIdx,
    /// The function token bound (its closure context included).
    pub tok: Tok,
    /// The instance's lexical context: its receiver layout, its program
    /// site (kind 0), or for a function without a `this` term its closure
    /// context's. The closures it makes take their context from it.
    pub lay: u32,
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
    /// (kind, layout/site/node, function token) -> instance. Kinds: 0 a
    /// plain call at a program site outside every summary (the unsummarized
    /// body's composition: the callee's effect per site, as `compose`
    /// expands a static call into a summary), 1 every other binding (the
    /// receiver's layout for a function with a `this` term, else
    /// `LAY_NONE`), 2 unknown-code entry, 3 setter group, 4 getter group,
    /// 5 a birth run (the constructor chain or field initializer of the
    /// object token being born, i.e. of its birth site). Nothing in a key
    /// is another instance: the count is (sites + layouts) x functions,
    /// fixed by the program.
    pub inst_key: FxHashMap<(u8, u32, Tok), u32>,
    /// Calls that exist when summaries are built: the program's calls.
    pub n_prog: usize,
    /// A call an instance made: the summary call (template) it renames.
    pub tmpl_of: FxHashMap<CallId, CallId>,
    /// A call the solver made (`solver_call`): the program site behind the
    /// call it was made for, where what it allocates is born.
    pub origin: FxHashMap<CallId, CallId>,
    /// (template call, callee): expanded into every summary that holds the
    /// template; its instances never bind it again.
    pub composed: FxHashSet<(CallId, FnIdx)>,
    /// Summarized function -> its SCC in the static call graph.
    pub scc: FxHashMap<FnIdx, u32>,
    /// (call, instance) pairs whose arguments and result are wired.
    pub bound: FxHashSet<(CallId, u32)>,
    /// (call, function token) whose receiver node binds per token
    /// (`Cons::ThisInst`).
    pub this_inst: FxHashSet<(CallId, Tok)>,
    /// A call an instance made -> the instance's lexical context
    /// (`Inst::lay`): what the call births is per (site, context).
    pub call_lay: FxHashMap<CallId, u32>,
    /// (birth site or literal template, lexical context) -> birth context
    /// id: bounded by sites x contexts, never by instances.
    pub birth_ids: FxHashMap<(u32, u32), CallId>,
    /// Plain calls (`Cons::Call` on the callee value) at program sites
    /// outside every summary: kind-0 keys.
    pub site_calls: FxHashSet<CallId>,
    /// Calls that belong to a summary: the shared body never runs them.
    pub templates: FxHashSet<CallId>,
    pub stats: String,
    /// Instances whose interface exists but whose entries are not applied
    /// yet: filled from the solver's top level (`fill_instances`), never
    /// nested inside the rule application that asked for them.
    pub pending: Vec<(u32, Vec<NodeId>)>,
    /// Summarized function -> the node of its `this` values no call is
    /// behind (literal methods, accessor groups): each value binds its
    /// layout's instance.
    pub this_any: FxHashMap<Tok, NodeId>,
    /// Summarized closure -> its outer terms (nodes of enclosing slices).
    pub outer_of: FxHashMap<FnIdx, Vec<NodeId>>,
    /// Closure context id -> the nodes of the closure's outer terms.
    pub ctx_nodes: Vec<Vec<NodeId>>,
    /// Closure context id -> the lexical context it was made under.
    pub ctx_lay: Vec<u32>,
    /// (closure, lexical context of the instance that made it) -> context
    /// id. A context is fixed by the program (closures x (sites +
    /// layouts)), never by an instance, so closure tokens never multiply
    /// through instances.
    pub ctx_of: FxHashMap<(FnIdx, u32), CallId>,
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
        CjsPublish { dst } => CjsPublish { dst: n(dst) },
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
        BirthField { key, src } => BirthField { key, src: n(src) },
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
        Spread { into, site } => Spread { into: n(into), site },
        SpreadFrom { src, site } => SpreadFrom { src, site },
        ThisInst { call, ft } => ThisInst { call: k(call), ft },
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
            | CjsPublish { .. }
            | Spread { .. }
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
            | BirthField { .. }
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
        | CjsPublish { dst }
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

    /// The program site behind `call`: itself for a call of the program,
    /// the template a call an instance made renames, or the site a call the
    /// solver made was made for.
    fn origin_of(&self, call: CallId) -> Option<CallId> {
        if (call as usize) < self.sum.n_prog {
            return Some(call);
        }
        self.sum.tmpl_of.get(&call).or_else(|| self.sum.origin.get(&call)).copied()
    }

    /// A call the solver makes for `by` (a builtin calling a callback, a
    /// bound target, a step of a protocol): what it allocates is born at
    /// `by`'s program site.
    pub(crate) fn solver_call(&mut self, by: CallId, cs: CallSite) -> CallId {
        let id = self.calls.len() as CallId;
        self.calls.push(cs);
        if self.sum.ready {
            if let Some(o) = self.origin_of(by) {
                self.sum.origin.insert(id, o);
            }
            if let Some(&l) = self.sum.call_lay.get(&by) {
                self.sum.call_lay.insert(id, l);
            }
        }
        id
    }

    /// The birth context of an object `call` makes: its program site (one
    /// token per birth site), within the lexical context of the instance
    /// that made the call (a method's births are per receiver layout: the
    /// container a layout's method makes is that layout's), or `BORN` when
    /// no program site is behind it (a protocol touch).
    pub(crate) fn birth_ctx(&mut self, call: CallId) -> CallId {
        if (call as usize) < self.sum.n_prog || !self.sum.ready {
            return call;
        }
        let Some(site) = self.origin_of(call) else {
            return BORN;
        };
        site
    }

    /// The birth context of (site or literal template, lexical context).
    fn birth_id(&mut self, site: u32, lay: u32) -> CallId {
        let n = self.sum.birth_ids.len() as CallId;
        *self.sum.birth_ids.entry((site, lay)).or_insert(BORN - 1 - n)
    }

    /// `f` is part of the birth of object `t`: the constructor or field
    /// initializer of its template or of an ancestor, or the function a
    /// constructor-function template is made from. Such a run binds per
    /// object token (one per birth site), like the birth itself.
    fn constructs(&self, f: FnIdx, t: Tok) -> bool {
        let TokKind::Obj(tm, _) = self.toks[t as usize] else {
            return false;
        };
        if self.fn_inst.get(&f) == Some(&tm) {
            return true;
        }
        let mut c = Some(tm);
        while let Some(x) = c {
            let tx = &self.tmpls[x as usize];
            if tx.ctor == Some(f) || tx.field_init == Some(f) {
                return true;
            }
            c = tx.parent;
        }
        false
    }

    /// The layout of receiver token `t`: its template for an object (every
    /// birth site shares it), the class object or prototype of a template,
    /// the function for a function value (every closure context shares it:
    /// a context is keyed by a layout, so a layout never names a context),
    /// the token itself for every other value (each is one value).
    pub(crate) fn layout_of(&self, t: Tok) -> u32 {
        match self.toks[t as usize] {
            TokKind::Obj(tm, _) => (1 << 28) | tm,
            TokKind::ClassObj(tm) => (2 << 28) | tm,
            TokKind::Proto(tm) => (3 << 28) | tm,
            TokKind::Func(f, _) => (7 << 28) | f,
            _ => (4 << 28) | t,
        }
    }

    pub(crate) fn summarized(&self, f: FnIdx) -> bool {
        self.sum.of.contains_key(&f)
    }

    /// The instance for key, made on first use.
    fn instance(&mut self, key: (u8, u32, Tok)) -> u32 {
        if let Some(&i) = self.sum.inst_key.get(&key) {
            return i;
        }
        let TokKind::Func(f, ctx) = self.toks[key.2 as usize] else {
            unreachable!("instance of a non-function token")
        };
        let s = self.sum.of[&f].clone();
        let inst = self.sum.insts.len() as u32;
        // An instance is solve work: the work bound covers instances too.
        self.work += s.nslots as u64 + 1;
        // Outer slots are the context's nodes (the enclosing instance's);
        // a token without a context observes through the shared nodes.
        let mut outer_at = vec![u32::MAX; s.nslots as usize];
        for (j, &sl) in s.outer.iter().enumerate() {
            outer_at[sl as usize] = j as u32;
        }
        let base: Vec<NodeId> = (0..s.nslots)
            .map(|i| match outer_at[i as usize] {
                u32::MAX => { let n = self.node_named("inst", || format!("inst{inst}.{i}")); self.cw9_inst_fn.insert(n, (f, (key.0 as u32) << 28)); if self.cwl_trace.is_some() { self.cwl_slot.insert(n, s.orig[i as usize]); } n }
                _ if ctx == NO_CTX => s.orig[i as usize],
                j => self.sum.ctx_nodes[ctx as usize][j as usize],
            })
            .collect();
        let tr = |x: NodeId| if is_slot(x) { base[(x & !SLOT) as usize] } else { x };
        let this = tr(s.this);
        let lay = match key.0 {
            0 => LAY_SITE | key.1,
            5 => LAY_BIRTH | key.1,
            1 if this != NO_NODE => key.1,
            2 if this != NO_NODE => self.layout_of(TOP),
            _ if ctx == NO_CTX => LAY_NONE,
            _ => self.sum.ctx_lay[ctx as usize],
        };
        self.sum.insts.push(Inst {
            f,
            tok: key.2,
            lay,
            params: s.params.iter().map(|&p| tr(p)).collect(),
            ret: tr(s.ret),
            this,
        });
        self.sum.inst_key.insert(key, inst);
        self.sum.pending.push((inst, base));
        // TEMP CWL: what keeps making instances.
        if self.profile.is_some() && inst % 5000 == 4999 {
            let mut h: FxHashMap<(u8, u32), u32> = FxHashMap::default();
            let mut tk: FxHashMap<String, u32> = FxHashMap::default();
            for &(k, l, _) in self.sum.inst_key.keys() {
                *h.entry((k, if k == 1 || k == 5 { l >> 28 } else { 9 })).or_default() += 1;
                if (k == 1 && (l >> 28 == 4 || l >> 28 == 7)) || k == 5 {
                    let t = if k == 5 { l } else { l & 0x0fff_ffff };
                    let nm = if k == 1 && l >> 28 == 7 { format!("F:{}", self.funcs[t as usize].name) } else { self.tok_name(t) };
                    *tk.entry(nm.split('@').next().unwrap_or("").chars().take(60).collect()).or_default() += 1;
                }
            }
            let mut hv: Vec<_> = h.into_iter().collect();
            hv.sort();
            let mut tv: Vec<_> = tk.into_iter().collect();
            tv.sort_by_key(|x| std::cmp::Reverse(x.1));
            tv.truncate(8);
            eprintln!("CWL grow inst={} toks={} ctx={} calls={} work={} kinds={hv:?} tokrecv={tv:?} last={:?} f={}", inst + 1, self.toks.len(), self.sum.ctx_nodes.len(), self.calls.len(), self.work, key, self.funcs[f as usize].name);
            if let Some(tn) = self.cwl_trace {
                self.cwl_trace_node(tn);
            }
            let mut wv: Vec<_> = self.cwl_thisinst.iter().collect();
            wv.sort_by_key(|x| std::cmp::Reverse(*x.1));
            for ((why, wf), n) in wv.iter().take(10) { eprintln!("CWL thisinst why={why} fn={} n={n}", self.funcs[*wf as usize].name); }
            let mut rv: Vec<_> = self.cwl_recv.iter().collect();
            rv.sort_by_key(|x| std::cmp::Reverse(x.1 .0));
            for ((rf, shape, lbl, prot, bs, prog), (n, first, site)) in rv.iter().take(12) {
                eprintln!("CWL recv fn={} this={shape} ret={lbl} protocol={prot} boundsynth={bs} progcall={prog} n={n} first={first} site={site}", self.funcs[*rf as usize].name);
            }
        }
        inst
    }

    /// A call an instance made from a template expanded into the summary:
    /// the instance already holds `f`'s effect.
    fn composed_away(&self, call: CallId, f: FnIdx) -> bool {
        self.sum.tmpl_of.get(&call).is_some_and(|&t| self.sum.composed.contains(&(t, f)))
    }

    /// Bind `call` (or no call: `NO_NODE`) to summarized `ft` for receiver
    /// `x` (or the receiver primitives `prim`): the instance of the
    /// receiver's layout gets the receiver, and the call's arguments and
    /// result once per (call, instance).
    pub(crate) fn bind_layout(&mut self, call: CallId, ft: Tok, x: Option<Tok>, prim: u16) {
        let f = self.tok_fn(ft);
        if call != NO_NODE && self.composed_away(call, f) {
            return;
        }
        let lay = match x {
            Some(t) if self.has_this_term(f) => self.layout_of(t),
            _ => LAY_NONE,
        };
        // TEMP CWL: how a function or bound value becomes a receiver.
        if lay >> 28 == 4 || lay >> 28 == 7 {
            if let Some(t) = x {
                if self.cwl_inst_probe {
                    let (shape, lbl, site, prot, bs) = if call == NO_NODE {
                        ("none".to_string(), "-", 0, false, false)
                    } else {
                        let cs = &self.calls[call as usize];
                        (format!("{:?}", cs.this).chars().take(12).collect(), self.labels[cs.ret as usize], cs.site, self.protocol_calls.contains(&call), self.bound_synth.contains(&call))
                    };
                    let mut nm = self.tok_name(t);
                    let prog = (call as usize) < self.sum.n_prog;
                    if call != NO_NODE {
                        if let ThisArg::Node(n) = self.calls[call as usize].this {
                            let mut k = [0u32; 6];
                            for x in self.nodes[n as usize].pts.to_sorted() { k[match self.toks[x as usize] { TokKind::Obj(..) => 0, TokKind::Func(..) => 1, TokKind::Bound(_) => 2, TokKind::ClassObj(_) | TokKind::Proto(_) => 3, TokKind::Top => 4, _ => 5 }] += 1; }
                            let st = (site as usize) < self.stores.len();
                            nm = format!("{nm} thisnode={n} label={} kinds(obj,fn,bound,cls,top,other)={k:?} site={} in {} tmpl={:?} callee_owner={:?}", self.labels[n as usize], if st { self.stores[site as usize].desc.clone() } else { "-".into() }, if st && self.stores[site as usize].func != u32::MAX { self.funcs[self.stores[site as usize].func as usize].name.clone() } else { "-".into() }, self.sum.tmpl_of.get(&call), self.sum.tmpl_of.get(&call).and_then(|t| self.calls.get(*t as usize)).map(|c| self.labels[c.ret as usize]));
                        }
                    }
                    if let Some(&tc) = self.sum.tmpl_of.get(&call) {
                        if self.cwl_seen.len() < 3 && self.cwl_seen.insert(tc) {
                            let fname = |m: &Self, n: NodeId| { let f = m.node_fn[n as usize]; if f == NO_FN { "-".to_string() } else { m.funcs[f as usize].name.clone() } };
                            let cs = self.calls[tc as usize].clone();
                            let tn = match cs.this { ThisArg::Node(n) => format!("Node({n}) label={} fn={}", self.labels[n as usize], fname(self, n)), ref x => format!("{x:?}") };
                            eprintln!("CWL tmplcall {tc} this={tn} args={:?} ret={} fn={}", cs.args, cs.ret, fname(self, cs.ret));
                            let mut k = 0;
                            for (n, node) in self.nodes.iter().enumerate() {
                                for c in &node.cons {
                                    let (_, ks) = cons_fields(c);
                                    if ks.contains(&tc) {
                                        k += 1;
                                        if k <= 12 { eprintln!("CWL tmplcons {} on {}#{n} fn={} toks={}", c.kind_name(), self.labels[n], fname(self, n as NodeId), node.pts.len()); }
                                    }
                                }
                            }
                            eprintln!("CWL tmplcons total={k}");
                        }
                    }
                    let shape = format!("{shape} why={}", self.cwl_why);
                    let e = self.cwl_recv.entry((f, shape, lbl, prot, bs, prog)).or_insert((0, String::new(), 0));
                    e.0 += 1;
                    if e.1.is_empty() { e.1 = nm; e.2 = site; }
                }
            }
        }
        if let (Some(t), Some(_)) = (x, self.cwl_trace) {
            if matches!(self.toks[t as usize], TokKind::Func(..)) && lay != LAY_NONE && self.cwl_firstfn.len() < 4 && self.cwl_firstfn.insert(f) {
                let (tn, site, tc) = if call == NO_NODE { (NO_NODE, String::new(), None) } else {
                    let cs = &self.calls[call as usize];
                    let st = (cs.site as usize) < self.stores.len();
                    (match cs.this { ThisArg::Node(n) => n, _ => NO_NODE },
                     if st { format!("{} in {}", self.stores[cs.site as usize].desc, if self.stores[cs.site as usize].func != u32::MAX { self.funcs[self.stores[cs.site as usize].func as usize].name.clone() } else { "-".into() }) } else { "-".into() },
                     self.sum.tmpl_of.get(&call).copied())
                };
                eprintln!("CWL firstfn method={} recv={} call={call} prog={} tmpl={tc:?} site={site} thisnode={tn} work={}", self.funcs[f as usize].name, self.tok_name(t), (call as usize) < self.sum.n_prog, self.work);
                if tn != NO_NODE { self.cwl_trace_tok(tn, t); }
            }
        }
        let key = match x {
            _ if self.sum.site_calls.contains(&call) => (0, call, ft),
            Some(t) if self.constructs(f, t) => (5, t, ft),
            _ => (1, lay, ft),
        };
        let inst = self.instance(key);
        let it = self.sum.insts[inst as usize].this;
        if it != NO_NODE {
            if let Some(t) = x {
                self.add_tok(it, t);
            }
            if prim != 0 {
                self.add_prim(it, prim);
            }
        }
        if call != NO_NODE && self.sum.bound.insert((call, inst)) {
            self.wire_args(call, inst);
        }
    }

    /// The `this` term of summarized `f`'s instances, if it has one.
    fn has_this_term(&self, f: FnIdx) -> bool {
        self.sum.of.get(&f).is_some_and(|s| s.this != NO_NODE)
    }

    /// `this` of `call` into `f`: the shared node observes it; `f`'s
    /// instance for the call gets the explicit token (the call's own
    /// operand reaches it in `bind_summarized`).
    pub(crate) fn this_into(&mut self, call: CallId, ft: Tok, this_tok: Option<Tok>) {
        if !self.sum.ready {
            self.sum.deferred.push(Deferred::This(call, ft, this_tok));
            return;
        }
        let f = self.tok_fn(ft);
        let fthis = self.funcs[f as usize].this;
        match (this_tok, &self.calls[call as usize].this) {
            (Some(x), _) => self.add_tok(fthis, x),
            (None, &ThisArg::Tok(x)) => self.add_tok(fthis, x),
            (None, &ThisArg::Node(n)) => self.edge(n, fthis),
            (None, ThisArg::Undef) => self.add_prim(fthis, prim::UNDEF),
        }
        if !self.has_this_term(f) || self.sum.templates.contains(&call) {
            return;
        }
        // The instance half: each receiver binds its layout's instance.
        match (this_tok, self.calls[call as usize].this.clone()) {
            (Some(x), _) | (None, ThisArg::Tok(x)) => self.bind_layout(call, ft, Some(x), 0),
            (None, ThisArg::Undef) => self.bind_layout(call, ft, None, prim::UNDEF),
            // A call bound through a method lookup gets each receiver from
            // the lookup (`this_calls`).
            (None, ThisArg::Node(_)) if self.relays.contains_key(&call) => {}
            (None, ThisArg::Node(n)) => {
                if self.sum.this_inst.insert((call, ft)) {
                    *self.cwl_thisinst.entry((self.cwl_why, f)).or_default() += 1;
                    self.cons(n, Cons::ThisInst { call, ft });
                }
            }
        }
    }

    /// `this` of `f` from a method lookup on receiver `origin`: the shared
    /// node observes it; the instances get the call's receiver operand.
    pub(crate) fn this_lookup(&mut self, ft: Tok, origin: Tok) {
        if !self.sum.ready {
            self.sum.deferred.push(Deferred::ThisTo(ft, origin));
            return;
        }
        let f = self.tok_fn(ft);
        let this = self.funcs[f as usize].this;
        self.add_tok(this, origin);
    }

    /// The node that takes `this` values of `f` no call is behind: every
    /// instance of `f` (and its shared instance, so the effect runs even
    /// if no call ever does) sees them.
    pub(crate) fn this_any(&mut self, ft: Tok) -> NodeId {
        let f = self.tok_fn(ft);
        let fthis = self.funcs[f as usize].this;
        if !self.sum.ready || !self.has_this_term(f) {
            return fthis;
        }
        if let Some(&u) = self.sum.this_any.get(&ft) {
            return u;
        }
        let u = self.node_named("this-any", || format!("this-any({f})"));
        self.sum.this_any.insert(ft, u);
        self.edge(u, fthis);
        // The receiver-less instance runs the effect even if no value does.
        if self.instantiable(ft) {
            self.instance((1, LAY_NONE, ft));
        }
        self.cons(u, Cons::ThisInst { call: NO_NODE, ft });
        u
    }

    /// An instance of `ft` has its outer terms: a closure with outer terms
    /// is instantiated only through a context (the token without one lives
    /// on observation nodes only, which never run effects).
    fn instantiable(&self, ft: Tok) -> bool {
        match self.toks[ft as usize] {
            TokKind::Func(f, NO_CTX) => self.sum.of.get(&f).is_some_and(|s| s.outer.is_empty()),
            TokKind::Func(_, _) => true,
            _ => false,
        }
    }

    /// Unknown code calls `f`: its `this` is ⊤ (the unknown-code instance's
    /// and the shared node).
    pub(crate) fn escape_this(&mut self, ft: Tok, reason: u32) {
        if !self.sum.ready {
            self.sum.deferred.push(Deferred::EscapeThis(ft, reason));
            return;
        }
        let f = self.tok_fn(ft);
        let why = |m: &Self| format!("this of escaped function: {}", m.reasons[reason as usize]);
        let mut dsts = vec![self.funcs[f as usize].this];
        if self.has_this_term(f) {
            let inst = self.instance((2, 0, ft));
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
            // Instances queue more instances: the work bound holds here too,
            // not only between worklist pops.
            if self.max_work != 0 && self.work > self.max_work {
                self.aborted = true;
                self.sum.pending.clear();
                return;
            }
            let (inst, base) = std::mem::take(&mut self.sum.pending[i]);
            i += 1;
            self.fill_instance(inst, &base);
        }
        self.sum.pending.clear();
    }

    fn fill_instance(&mut self, inst: u32, base: &[NodeId]) {
        let f = self.sum.insts[inst as usize].f;
        let s = self.sum.of[&f].clone();
        let lay = self.sum.insts[inst as usize].lay;
        let tr = |x: NodeId| if is_slot(x) { base[(x & !SLOT) as usize] } else { x };
        let mut kmap = Vec::with_capacity(s.calls.len());
        for (t, cs) in s.calls.iter() {
            let id = self.calls.len() as CallId;
            self.calls.push(map_call(cs, tr));
            self.sum.tmpl_of.insert(id, *t);
            if lay != LAY_NONE {
                self.sum.call_lay.insert(id, lay);
            }
            if let Some(&a) = self.call_lit.get(t) {
                self.call_lit.insert(id, a);
            }
            kmap.push(id);
        }
        for &(slot, ref toks, prim) in &s.seeds {
            self.add(base[slot as usize], toks, prim);
        }
        // Closures born here carry this instance's nodes for their outer
        // terms, in the context of its lexical layout.
        for (slot, g, terms) in &s.closures {
            let nodes: Vec<NodeId> = terms.iter().map(|&x| tr(x)).collect();
            let t = self.closure_tok(*g, lay, &nodes);
            self.add_tok(base[*slot as usize], t);
        }
        // Literals born here: one token per (literal, lexical context).
        for &(slot, tm) in &s.births {
            let ctx = BORN;
            let t = self.site_tok(tm, ctx);
            self.add_tok(base[slot as usize], t);
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

    /// The token of closure `g` made by an instance of lexical context
    /// `lay` (`Inst::lay`), whose nodes for `g`'s outer terms are `nodes`.
    /// The context's own nodes are those terms for every instance of that
    /// context, both ways: what the closure writes into a captured local
    /// reaches the instance that made it.
    pub(crate) fn closure_tok(&mut self, g: FnIdx, lay: u32, nodes: &[NodeId]) -> Tok {
        let ctx = match self.sum.ctx_of.get(&(g, lay)) {
            Some(&c) => c,
            None => {
                let c = self.sum.ctx_nodes.len() as CallId;
                let cn: Vec<NodeId> = (0..nodes.len()).map(|j| self.node_named("ctx", || format!("ctx{c}.{j}"))).collect();
                self.sum.ctx_of.insert((g, lay), c);
                self.sum.ctx_nodes.push(cn);
                self.sum.ctx_lay.push(lay);
                c
            }
        };
        let cn = self.sum.ctx_nodes[ctx as usize].clone();
        for (&x, &c) in nodes.iter().zip(cn.iter()) {
            self.edge(x, c);
            self.edge(c, x);
        }
        self.tok(TokKind::Func(g, ctx))
    }

    /// Arguments and result of `call` into summarized function token `ft`.
    /// A function with a `this` term binds per receiver (`this_into`), the
    /// others their one instance.
    pub(crate) fn bind_summarized(&mut self, call: CallId, ft: Tok) {
        if !self.has_this_term(self.tok_fn(ft)) {
            self.bind_layout(call, ft, None, 0);
        }
    }

    /// Arguments and result of `call` into instance `inst`.
    fn wire_args(&mut self, call: CallId, inst: u32) {
        let f = self.sum.insts[inst as usize].f;
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

    /// The result node `call` sees from `f`: none for a summarized `f`
    /// (each instance's result is wired into the call's), else its own.
    pub(crate) fn ret_for(&self, _call: CallId, ft: Tok) -> NodeId {
        let f = self.tok_fn(ft);
        if self.summarized(f) {
            NO_NODE
        } else {
            self.funcs[f as usize].ret
        }
    }

    /// Unknown code calls summarized `f`: unknown arguments, the result escapes.
    pub(crate) fn escape_summarized(&mut self, ft: Tok, reason: u32) {
        let f = self.tok_fn(ft);
        let inst = self.instance((2, 0, ft));
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
    pub(crate) fn group_summarized(&mut self, ft: Tok, sp: NodeId, ar: NodeId, setter: bool) {
        let f = self.tok_fn(ft);
        if setter {
            let inst = self.instance((3, sp, ft));
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
            let inst = self.instance((4, ar, ft));
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
        for n in 0..self.nodes.len() {
            if self.nodes[n].obs {
                continue;
            }
            for c in &self.nodes[n].cons {
                if let Cons::Call { call } = *c {
                    if (call as usize) < self.sum.n_prog && !self.sum.templates.contains(&call) {
                        self.sum.site_calls.insert(call);
                    }
                }
            }
        }
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
                let ft = self.funcs[f as usize].token;
                let fed = s.seeds.iter().any(|x| x.0 == sl) || s.edges.iter().any(|e| e.1 == s.this && !is_slot(e.0));
                if fed && self.instantiable(ft) {
                    self.instance((1, LAY_NONE, ft));
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
        Lex { parent, depth, children }
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
        let TokKind::Obj(tm, _) = m.toks[t as usize] else { continue };
        let f = m.tmpl_fn[&tm];
        if !cand[f as usize] {
            continue;
        }
        let hs = &holders[&t];
        if hs.iter().any(|&h| m.node_fn[h as usize] != f) {
            continue;
        }
        let mut own: Vec<NodeId> = own_of.iter().filter(|(_, &x)| x == t).map(|(&n, _)| n).collect();
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
            out.entry(f).or_default().push(Birth { tok: t, tm, holders: hs.clone(), fields, own });
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

fn role(lex: &Lex, dof: &FxHashMap<NodeId, FnIdx>, outer: &FxHashSet<NodeId>, f: FnIdx, y: NodeId) -> Role {
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
    let mut order: Vec<FnIdx> = (0..m.funcs.len() as FnIdx).filter(|&f| cand[f as usize]).collect();
    order.sort_by_key(|&f| (lex.depth[f as usize], f));
    let mut slices: FxHashMap<FnIdx, Vec<NodeId>> = FxHashMap::default();
    let mut all_dof: FxHashMap<NodeId, FnIdx> = FxHashMap::default();
    for &f in &order {
        let through: Vec<NodeId> =
            capt.get(&f).unwrap_or(&none).iter().copied().filter(|x| all_dof.contains_key(x)).collect();
        // A body with no interface term is never instanced (module and
        // initializer bodies run once, unattributed): it keeps no slice.
        if slice(m, ix, f, &[], &through, pinned).is_empty() {
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
                let has_outer = capt
                    .get(&g)
                    .is_some_and(|v| v.iter().any(|x| dset.contains(x) || all_dof.contains_key(x)));
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
                            why.entry(f)
                                .or_insert_with(|| format!("solver rule {} on {}", c.kind_name(), m.labels[h as usize]));
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
                        why.entry(f).or_insert_with(|| "touches another summarized slice".into());
                        bad_now.insert(a);
                    }
                }
            };
            for &x in d {
                for i in 0..m.nodes[x as usize].cons.len() {
                    consider(x, i as u32, true, &mut bad, &mut why, &mut bad_now, &mut cons);
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
                        note("touches another summarized slice".into(), &mut bad, &mut why);
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
                        note("touches another summarized slice".into(), &mut bad, &mut why);
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
            return Plan { slices, dof, outer, refused, why, entries };
        }
    }
}

fn build_summaries(m: &mut Model) {
    let ix = build_index(m);
    let lex = Lex::new(m);
    let cand: Vec<bool> =
        m.funcs.iter().map(|fi| !fi.no_summary && fi.rest.is_none() && !fi.has_arguments).collect();
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
            let broken = chain.iter().any(|&p| {
                pl.refused.contains(&p) || (Some(p) != h && !pl.slices.contains_key(&p))
            });
            if broken {
                add.push(*x);
            }
        }
        if add.is_empty() {
            break pl;
        }
        pinned.extend(add);
    };
    let Plan { slices, dof, outer, refused, why, entries } = pl;
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
                .filter(|&h| !ix.preds.get(&h).is_some_and(|ps| ps.iter().any(|p| b.holders.contains(p))))
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
            for &cv in cvs.get(&g).map(|v| v.as_slice()).unwrap_or(&[]) {
                if let Some(&sl) = slot.get(&cv) {
                    s.closures.push((sl, g, og.iter().map(|&x| enc(x)).collect()));
                    strip.insert(m.funcs[g as usize].token);
                }
            }
        }
        for &x in d {
            let node = &m.nodes[x as usize];
            let toks: Vec<Tok> = node.pts.to_sorted().into_iter().filter(|t| !strip.contains(t)).collect();
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
        s.this = if m.funcs[f as usize].lexical_this { NO_NODE } else { enc(m.funcs[f as usize].this) };
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
                // A closure with outer terms is bound per context, never
                // expanded into a caller.
                TokKind::Func(g, _) if m.sum.of.get(&g).is_some_and(|s| s.outer.is_empty()) => Some(g),
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
    for (c, h, terms) in &g.closures {
        s.closures.push((off + c, *h, terms.iter().map(|&x| tr(x)).collect()));
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
    // Outer slots are the context's nodes; closure values and their terms
    // are read when an instance is filled.
    for &o in &s.outer {
        pinned[o as usize] = true;
    }
    for &(b, _) in &s.births {
        pinned[b as usize] = true;
    }
    for (c, _, terms) in &s.closures {
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
    s.outer = s.outer.iter().map(|&o| map[o as usize]).collect();
    for (b, _) in s.births.iter_mut() {
        *b = map[*b as usize];
    }
    for (c, _, terms) in s.closures.iter_mut() {
        *c = map[*c as usize];
        for x in terms.iter_mut() {
            *x = rn(*x);
        }
    }
    s.nslots = orig.len() as u32;
    s.orig = orig;
}

// TEMP CWL: walk the first-arrival chain of a few tokens of one node back to
// the constraint that put them there.
impl Model {
    pub fn cwl_trace_node(&self, tn: NodeId) {
        let mut picked: Vec<Tok> = Vec::new();
        let mut kinds = [0u32; 3];
        let mut toks = Vec::new();
        self.nodes[tn as usize].pts.sorted_into(&mut toks);
        for &t in &toks {
            let k = match &self.toks[t as usize] {
                TokKind::Func(..) => 0,
                TokKind::Obj(..) => 1,
                TokKind::ClassObj(..) => 2,
                _ => continue,
            };
            if kinds[k] < 2 {
                kinds[k] += 1;
                picked.push(t);
            }
        }
        let nm = |s: &Self, n: NodeId| {
            let mut f = s.node_fn[n as usize];
            let mut extra = String::new();
            if let Some(&(ff, k)) = s.cw9_inst_fn.get(&n) {
                f = ff;
                extra = format!(" k{}", k >> 28);
            }
            if let Some(&o) = s.cwl_slot.get(&n) {
                extra += &format!(" orig={}#{o}", s.labels[o as usize]);
            }
            if let Some(((h, a), _)) = s.fields.iter().find(|(_, &x)| x == n) {
                extra += &format!(" {}.{}", s.tok_name(*h), s.atoms[*a as usize]);
            }
            if let Some((h, _)) = s.field_any.iter().find(|(_, &x)| x == n) {
                extra += &format!(" {}{}.*", s.tok_name(*h), s.cwl_tokdesc(*h));
            }
            if let Some((h, _)) = s.all_fields.iter().find(|(_, &x)| x == n) {
                extra += &format!(" {}{}.**", s.tok_name(*h), s.cwl_tokdesc(*h));
            }
            let fname = if f == NO_FN { "-".to_string() } else { s.funcs[f as usize].name.rsplit(':').next().unwrap_or("").to_string() };
            format!("{}#{n}[{fname}{extra}]({})", s.labels[n as usize], s.nodes[n as usize].pts.len())
        };
        {
            let fs: Vec<String> = self.funcs.iter().filter(|f| f.this == tn).map(|f| f.name.clone()).collect();
            let is: Vec<String> = self.sum.inst_key.iter().filter(|(_, &i)| self.sum.insts[i as usize].this == tn || self.sum.insts[i as usize].params.contains(&tn)).map(|(k, &i)| format!("inst{i} key=({},{:#x},{}) lay={:#x} this={} params={:?}", k.0, k.1, self.tok_name(k.2), self.sum.insts[i as usize].lay, self.sum.insts[i as usize].this, self.sum.insts[i as usize].params)).collect();
            eprintln!("CWL traceowner node={tn} shared_this_of={fs:?} insts={is:?}");
            let mut tm17: Vec<String> = Vec::new();
            for (i, k) in self.toks.iter().enumerate() {
                if let TokKind::Obj(tm, c) = k {
                    if self.tmpls[*tm as usize].display.contains("c6dcb161") {
                        let fun = if (*c as usize) < self.calls.len() { let r = self.calls[*c as usize].ret; let f = self.node_fn[r as usize]; if f == NO_FN { "-".to_string() } else { self.funcs[f as usize].name.rsplit(':').next().unwrap_or("").to_string() } } else { "-".into() };
                        tm17.push(format!("tok{i} {} fn={fun}", self.cwl_tokdesc(i as Tok)));
                    }
                }
            }
            let ncalls_tm: Vec<String> = Vec::new();
            eprintln!("CWL tm17 toks={tm17:?} {ncalls_tm:?}");
        }
        for t in picked {
            self.cwl_trace_tok(tn, t);
        }
    }
    pub fn cwl_tokdesc(&self, h: Tok) -> String {
        let TokKind::Obj(tm, c) = self.toks[h as usize] else { return String::new() };
        let site = |c: CallId| {
            let cs = &self.calls[c as usize];
            let st = (cs.site as usize) < self.stores.len();
            if st { format!("{} in {}", self.stores[cs.site as usize].desc, if self.stores[cs.site as usize].func != u32::MAX { self.funcs[self.stores[cs.site as usize].func as usize].name.rsplit(':').next().unwrap_or("").to_string() } else { "-".into() }) } else { format!("call{c}") }
        };
        let d = if c == NO_CTX { "NO_CTX".to_string() } else if c == BORN { "BORN".into() } else if (c as usize) < self.sum.n_prog { format!("prog {}", site(c)) } else if let Some(((s, l), _)) = self.sum.birth_ids.iter().find(|(_, &v)| v == c) { if s & LIT != 0 { format!("lit tm{} lay={l:#x}", s & !LIT) } else { format!("site {} lay={l:#x}", site(*s)) } } else { format!("ctx{c}") };
        let nsites = self.toks.iter().filter(|k| matches!(k, TokKind::Obj(x, _) if *x == tm)).count();
        format!("{{tm{tm} {d} ntoks_of_tm={nsites}}}")
    }
    pub fn cwl_trace_tok(&self, tn: NodeId, t: Tok) {
        let nm = |s: &Self, n: NodeId| {
            let mut f = s.node_fn[n as usize];
            let mut extra = String::new();
            if let Some(&(ff, k)) = s.cw9_inst_fn.get(&n) {
                f = ff;
                extra = format!(" k{}", k >> 28);
            }
            if let Some(&o) = s.cwl_slot.get(&n) {
                extra += &format!(" orig={}#{o}", s.labels[o as usize]);
            }
            if let Some(((h, a), _)) = s.fields.iter().find(|(_, &x)| x == n) {
                extra += &format!(" {}.{}", s.tok_name(*h), s.atoms[*a as usize]);
            }
            if let Some((h, _)) = s.field_any.iter().find(|(_, &x)| x == n) {
                extra += &format!(" {}{}.*", s.tok_name(*h), s.cwl_tokdesc(*h));
            }
            if let Some((h, _)) = s.all_fields.iter().find(|(_, &x)| x == n) {
                extra += &format!(" {}{}.**", s.tok_name(*h), s.cwl_tokdesc(*h));
            }
            let fname = if f == NO_FN { "-".to_string() } else { s.funcs[f as usize].name.rsplit(':').next().unwrap_or("").to_string() };
            format!("{}#{n}[{fname}{extra}]({})", s.labels[n as usize], s.nodes[n as usize].pts.len())
        };
        {
            let mut out = vec![format!("tok={} lay={:#x}", self.tok_name(t), self.layout_of(t))];
            let mut n = tn;
            let mut seen = crate::pts::FxHashSet::default();
            for _ in 0..40 {
                if !seen.insert(n) { out.push("LOOP".into()); break; }
                match self.cwl_prov.get(&(n, t)) {
                    None => { out.push(format!("{} <no prov>", nm(self, n))); break; }
                    Some(&(src, why)) => {
                        out.push(format!("{} <-{why}-", nm(self, n)));
                        if why == "thisto" {
                            let vc: Vec<String> = self.via_calls.iter().filter(|((c, _), _)| *c == src).take(3).map(|((_, o), cs)| {
                                let c0 = cs[0];
                                let cs0 = &self.calls[c0 as usize];
                                let tc = self.sum.tmpl_of.get(&c0).copied();
                                let st = (cs0.site as usize) < self.stores.len();
                                format!("origin={} ncalls={} call{c0} prog={} tmpl={tc:?} site={} in {} args={:?} ret={}", self.tok_name(*o), cs.len(), (c0 as usize) < self.sum.n_prog, if st { self.stores[cs0.site as usize].desc.clone() } else { "-".into() }, if st && self.stores[cs0.site as usize].func != u32::MAX { self.funcs[self.stores[cs0.site as usize].func as usize].name.clone() } else { "-".into() }, cs0.args.iter().map(|&a| nm(self, a)).collect::<Vec<_>>(), nm(self, cs0.ret))
                            }).collect();
                            out.push(format!("[viacalls {vc:?}]"));
                        }
                        if src == NO_NODE { out.push("SEED".into()); break; }
                        if !self.cwl_prov.contains_key(&(src, t)) {
                            out.push(format!("{} [cons on src: {:?}; tok not in src]", nm(self, src), self.nodes[src as usize].cons.iter().take(4).map(|c| c.kind_name()).collect::<Vec<_>>()));
                            break;
                        }
                        n = src;
                    }
                }
            }
            eprintln!("CWL trace {}", out.join(" "));
        }
    }
}
