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

mod build;
use build::build_summaries;

/// A summary term: a slot index with this bit, else an absolute node.
pub const SLOT: u32 = 1 << 31;
/// A call template reference inside a summary's constraints.
pub const CALL: u32 = 1 << 31;
/// The birth context of an object no program site is behind (a literal
/// born in a context-free instance, a birth of a call the solver made for
/// no site): one token per template. Birth context ids count down from it.
pub const BORN: CallId = NO_CTX - 1;
/// The layout of no object receiver (a primitive, `undefined`, a function
/// without a `this` term).
pub const LAY_NONE: u32 = 0;
/// The lexical context of a kind-0 instance: its program site.
const LAY_SITE: u32 = 5 << 28;
/// The lexical context of a kind-5 instance: the object it constructs.
const LAY_BIRTH: u32 = 6 << 28;
/// A closure context key for a closure whose captured cells are its
/// creator's context's (`CAP_OUTER`): the creator's context id with this
/// bit. Lexical contexts never set it (kinds stop at 7 << 28).
const CTX_KEY: u32 = 1 << 31;
/// What a closure a summary creates captures (design §14.2), a static
/// property of (summary, closure): a slot of the creator's own frame,
/// only the creator's outer terms, or neither (shared or module nodes).
pub const CAP_OWN: u8 = 0;
pub const CAP_OUTER: u8 = 1;
pub const CAP_NONE: u8 = 2;

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
    /// value, closure, its outer terms as terms of this summary, what they
    /// capture: `CAP_*`). Each instance seeds the slot with the closure
    /// token of the context its captured cells are.
    pub closures: Vec<(u32, FnIdx, Vec<NodeId>, u8)>,
    /// Literals this body births: (slot of the birth node, template). Each
    /// instance seeds its own site token there; the template token stays
    /// on the observation nodes.
    pub births: Vec<(u32, TmplId)>,
    /// Calls expanded into this summary: (index in `calls`, callee). Only
    /// this summary's direct calls; the calls copied in with a callee's
    /// body bind instances (design §14.3).
    pub expanded: Vec<(u32, FnIdx)>,
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
    /// (call an instance made, callee): the call's summary expanded the
    /// callee, so the instance already holds its effect and never binds it.
    pub composed: FxHashSet<(CallId, FnIdx)>,
    /// Calls `compose` expanded.
    pub ncomposed: usize,
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
    /// (closure, key) -> context id. The key is what the captured cells
    /// differ by (design §14.2): the creating instance's lexical context
    /// for a closure capturing its own frame, its closure context
    /// (`CTX_KEY | ctx`) for one capturing only outer terms, `CTX_KEY |
    /// NO_CTX` for one capturing neither. A context is fixed by the
    /// program (closures x (sites + layouts + contexts)), never by an
    /// instance, so closure tokens never multiply through instances.
    pub ctx_of: FxHashMap<(FnIdx, u32), CallId>,
}

fn is_slot(x: NodeId) -> bool {
    x != NO_NODE && x & SLOT != 0
}

fn act_map(
    a: ChainAct,
    n: &mut impl FnMut(NodeId) -> NodeId,
    k: &mut impl FnMut(CallId) -> CallId,
) -> ChainAct {
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
        Store { key, src, site } => Store {
            key,
            src: n(src),
            site,
        },
        Call { call } => Call { call: k(call) },
        CallThis { call, this } => CallThis {
            call: k(call),
            this,
        },
        New { call } => New { call: k(call) },
        NewNamed { name, call } => NewNamed {
            name,
            call: k(call),
        },
        MCall { key, call } => MCall { key, call: k(call) },
        Chain { origin, key, act } => Chain {
            origin,
            key,
            act: act_map(act, &mut n, &mut k),
        },
        TopSink { reason } => TopSink { reason },
        HandOver { reason, ctor, arg } => HandOver { reason, ctor, arg },
        OwnOnly { f, dst } => OwnOnly { f, dst: n(dst) },
        StoreOut {
            f,
            dst,
            reach,
            via,
            key,
            reason,
            at,
            to,
        } => StoreOut {
            f,
            dst,
            reach,
            via,
            key,
            reason,
            at,
            to,
        },
        ContractThis {
            tmpl,
            reason,
            statics,
            this,
        } => ContractThis {
            tmpl,
            reason,
            statics,
            this,
        },
        OverrideThis { tmpl, key, reason } => OverrideThis { tmpl, key, reason },
        TopIfAny { dst, from, reason } => TopIfAny {
            dst: n(dst),
            from: n(from),
            reason,
        },
        Hard { kind, reason } => Hard { kind, reason },
        AddsKeys => AddsKeys,
        Implicit { reason } => Implicit { reason },
        CreateProto { reason } => CreateProto { reason },
        ChainUnion { key, dst } => ChainUnion { key, dst: n(dst) },
        ProtoClosure { dst } => ProtoClosure { dst: n(dst) },
        ProtoMethods { origin, key, call } => ProtoMethods {
            origin,
            key,
            call: k(call),
        },
        AnyAccessors { origin, act } => AnyAccessors {
            origin,
            act: act_map(act, &mut n, &mut k),
        },
        StoreTopProto { origin, src } => StoreTopProto {
            origin,
            src: n(src),
        },
        Super { start, key, act } => Super {
            start,
            key,
            act: act_map(act, &mut n, &mut k),
        },
        SuperCtor { class, call } => SuperCtor {
            class,
            call: k(call),
        },
        SuperCtorParent {
            class,
            call,
            origin,
        } => SuperCtorParent {
            class,
            call: k(call),
            origin,
        },
        ThisFrom { recv } => ThisFrom { recv: n(recv) },
        BirthField { key, src } => BirthField { key, src: n(src) },
        Accessor {
            holder,
            key,
            setter,
        } => Accessor {
            holder: n(holder),
            key,
            setter,
        },
        AccessorInstall { key, f, setter } => AccessorInstall { key, f, setter },
        ProtoFrom { src } => ProtoFrom { src: n(src) },
        ProtoOf { dst } => ProtoOf { dst: n(dst) },
        IterNext { dst } => IterNext { dst: n(dst) },
        NewTargetHard { reason } => NewTargetHard { reason },
        IdxLoad { idx, dst } => IdxLoad {
            idx: n(idx),
            dst: n(dst),
        },
        IdxKey { obj, dst } => IdxKey { obj, dst: n(dst) },
        Arith { dst, add } => Arith { dst: n(dst), add },
        ArrayLenArg { dst } => ArrayLenArg { dst: n(dst) },
        ShareBuffer { ta } => ShareBuffer { ta },
        ThisTo { origin } => ThisTo { origin },
        CallArgs { call } => CallArgs { call: k(call) },
        Spread { into, site } => Spread {
            into: n(into),
            site,
        },
        SpreadFrom { src, site } => SpreadFrom { src, site },
        ThisInst { call, ft } => ThisInst { call: k(call), ft },
        BuiltinCallee => BuiltinCallee,
        HolderStep { key, pass, stop } => HolderStep {
            key,
            pass: n(pass),
            stop: n(stop),
        },
        VisitHolders { origin, key, act } => VisitHolders {
            origin,
            key,
            act: act_map(act, &mut n, &mut k),
        },
        VisitStop { origin, key, act } => VisitStop {
            origin,
            key,
            act: act_map(act, &mut n, &mut k),
        },
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
    map_cons(
        c,
        |x| {
            ns.push(x);
            x
        },
        |k| {
            ks.push(k);
            k
        },
    );
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
            | HandOver { .. }
            | ContractThis { .. }
            | OverrideThis { .. }
            | TopIfAny { .. }
            | Hard { .. }
            | AddsKeys
            | Implicit { .. }
            | IdxLoad { .. }
            | IdxKey { .. }
            | Arith { .. }
            | ArrayLenArg { .. }
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
        | ArrayLenArg { dst }
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
            Cons::Chain {
                act: ChainAct::Call(call),
                ..
            }
            | Cons::AnyAccessors {
                act: ChainAct::Call(call),
                ..
            }
            | Cons::Super {
                act: ChainAct::Call(call),
                ..
            }
            | Cons::VisitHolders {
                act: ChainAct::Call(call),
                ..
            }
            | Cons::VisitStop {
                act: ChainAct::Call(call),
                ..
            } => call,
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
        self.sum
            .tmpl_of
            .get(&call)
            .or_else(|| self.sum.origin.get(&call))
            .copied()
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
        let mut work = vec![tm];
        let mut seen: Vec<TmplId> = Vec::new();
        while let Some(x) = work.pop() {
            if seen.contains(&x) {
                continue;
            }
            seen.push(x);
            let tx = &self.tmpls[x as usize];
            if tx.ctor == Some(f) || tx.field_init == Some(f) {
                return true;
            }
            work.extend(tx.parent);
            work.extend(tx.parent_links.iter().copied());
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
                u32::MAX => self.node_named("inst", || format!("inst{inst}.{i}")),
                _ if ctx == NO_CTX => s.orig[i as usize],
                j => self.sum.ctx_nodes[ctx as usize][j as usize],
            })
            .collect();
        let tr = |x: NodeId| {
            if is_slot(x) {
                base[(x & !SLOT) as usize]
            } else {
                x
            }
        };
        let this = tr(s.this);
        // Its parameters hold argument markers (DECISIONS 44).
        for (j, &p) in s.params.iter().enumerate() {
            if is_slot(p) {
                self.nodes[tr(p) as usize].param = true;
                self.param_at.insert(tr(p), (f, j as u32));
            }
        }
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
        inst
    }

    /// A call an instance made at a call its summary expanded `f` into:
    /// the instance already holds `f`'s effect.
    fn composed_away(&self, call: CallId, f: FnIdx) -> bool {
        self.sum.composed.contains(&(call, f))
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
    /// DECISIONS 116: a contract method published with sealed classes'
    /// receivers only.
    pub(crate) fn sealed_method(&self, f: FnIdx) -> bool {
        self.contract_fns.contains(&f)
            && self.published_this.get(&f).is_some_and(|rs| {
                !rs.is_empty()
                    && rs.iter().all(|r| {
                        self.recv_class
                            .get(r)
                            .is_some_and(|c| self.sealed.contains(c))
                    })
            })
    }

    /// DECISIONS 125: a method run on published receivers runs on an
    /// instance `t` this unit gave out when the join names `t`'s class (its
    /// own token `Obj(tm, NO_CTX)` is in the join, `Gen::published_receivers`).
    pub(crate) fn join_holds(&self, f: FnIdx, t: Tok) -> bool {
        let Some(&n) = self.this_args.get(&f) else {
            return true;
        };
        let Some(tm) = self.tmpl_of(t) else {
            return false;
        };
        self.tok_index
            .get(&TokKind::Obj(tm, NO_CTX))
            .is_some_and(|&w| self.nodes[n as usize].pts.contains_tok(w))
    }

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
        let insts = self.published_this.get(&f).cloned().unwrap_or_default();
        // DECISIONS 116: a method of sealed classes only, published through
        // its contract, runs on the instances this unit gave out (and the
        // ones it gives out later, `give_out`): no other unit constructs or
        // extends them, and a reader's `ContractThis` admits nothing else.
        if self.pub_reason == Some(reason) && self.sealed_method(f) {
            let given: Vec<Tok> = insts
                .iter()
                .flat_map(|r| self.contract_recv.get(r).cloned().unwrap_or_default())
                .collect();
            for &d in &dsts {
                for &t in &given {
                    self.add_tok(d, t);
                }
            }
            self.given_this.entry(f).or_default().extend(dsts);
            return;
        }
        // A published method's receiver is one more contract parameter
        // (DECISIONS 57): its marker records what the body does to it, and
        // stands for instances of the classes it is published with
        // (DECISIONS 59).
        let mk = (!insts.is_empty() && self.contract_fns.contains(&f)).then(|| {
            let mk = self.param_marker_tok(f, u32::MAX);
            let classes: Vec<TmplId> = insts
                .iter()
                .filter_map(|r| self.recv_class.get(r).copied())
                .collect();
            self.register_virt(mk, &classes);
            mk
        });
        // Published through its contract, the receiver is the class's
        // instances (the receiver token) or what the marker stands for: a
        // reader's `ContractThis` admits nothing else, and reports a use
        // beyond that (DECISIONS 85). Any other escape lets unknown code
        // call it with any receiver.
        let contract_only = mk.is_some() && self.pub_reason == Some(reason);
        let joined = contract_only && self.this_args.contains_key(&f);
        if joined {
            self.given_this
                .entry(f)
                .or_default()
                .extend(dsts.iter().copied());
        }
        for d in dsts {
            if !contract_only {
                self.add_top(d, why);
                self.add_prim(d, prim::ALL);
            }
            // DECISIONS 107: the receivers the running units published.
            // DECISIONS 125: they include the instances this unit gave out
            // (a published class's instance another unit holds may be one
            // of them), and the ones it gives out later (`give_out`).
            match self.this_args.get(&f) {
                Some(&n) if joined => {
                    self.edge(n, d);
                    let given: Vec<Tok> = insts
                        .iter()
                        .flat_map(|r| self.contract_recv.get(r).cloned().unwrap_or_default())
                        .filter(|&t| self.join_holds(f, t))
                        .collect();
                    for t in given {
                        self.add_tok(d, t);
                    }
                }
                _ => {
                    for &i in &insts {
                        self.add_tok(d, i);
                    }
                }
            }
            if let Some(mk) = mk {
                self.add_tok(d, mk);
            }
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
        let tr = |x: NodeId| {
            if is_slot(x) {
                base[(x & !SLOT) as usize]
            } else {
                x
            }
        };
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
        for &(i, g) in &s.expanded {
            self.sum.composed.insert((kmap[i as usize], g));
        }
        for &(slot, ref toks, prim) in &s.seeds {
            self.add(base[slot as usize], toks, prim);
        }
        // Closures born here carry this instance's nodes for their outer
        // terms, in the context those cells differ by (design §14.2): its
        // lexical context when they include its own frame, its closure
        // context when they are its outer terms only, else one context.
        let ctx = match self.toks[self.sum.insts[inst as usize].tok as usize] {
            TokKind::Func(_, c) => c,
            _ => NO_CTX,
        };
        for (slot, g, terms, cap) in &s.closures {
            let nodes: Vec<NodeId> = terms.iter().map(|&x| tr(x)).collect();
            let (key, clay) = match *cap {
                CAP_OWN => (lay, lay),
                CAP_OUTER if ctx != NO_CTX => (CTX_KEY | ctx, self.sum.ctx_lay[ctx as usize]),
                _ => (CTX_KEY | NO_CTX, LAY_NONE),
            };
            let t = self.closure_tok(*g, key, clay, &nodes);
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
            let c2 = map_cons(c, tr, |k| {
                if k != NO_NODE && k & CALL != 0 {
                    kmap[(k & !CALL) as usize]
                } else {
                    k
                }
            });
            self.cons(tr(*h), c2);
        }
    }

    /// The token of closure `g` made by an instance whose captured cells
    /// are keyed `key` (`SumState::ctx_of`), under lexical context `lay`,
    /// whose nodes for `g`'s outer terms are `nodes`. The context's own
    /// nodes are those terms for every instance of that key, both ways:
    /// what the closure writes into a captured local reaches the instance
    /// that made it.
    pub(crate) fn closure_tok(&mut self, g: FnIdx, key: u32, lay: u32, nodes: &[NodeId]) -> Tok {
        let ctx = match self.sum.ctx_of.get(&(g, key)) {
            Some(&c) => c,
            None => {
                let c = self.sum.ctx_nodes.len() as CallId;
                let cn: Vec<NodeId> = (0..nodes.len())
                    .map(|j| self.node_named("ctx", || format!("ctx{c}.{j}")))
                    .collect();
                self.sum.ctx_of.insert((g, key), c);
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
        // A hidden capture parameter holds its class environment slot,
        // not what the call passes (DECISIONS 117, 127).
        let caps: Vec<Option<NodeId>> =
            (0..terms.len()).map(|i| self.capture_param(f, i)).collect();
        for i in 0..terms.len() {
            let mut dsts = Vec::with_capacity(2);
            if terms[i] != NO_NODE {
                dsts.push(terms[i]);
            }
            if self.nodes[fparams[i] as usize].obs {
                dsts.push(fparams[i]);
            }
            for &d in &dsts {
                if let Some(env) = caps[i] {
                    self.edge(env, d);
                } else if i < nargs {
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
            self.cons(
                s,
                Cons::Load {
                    key: Key::Any,
                    dst: elems,
                },
            );
            for i in 0..terms.len() {
                if caps[i].is_some() {
                    continue;
                }
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
        let contract = self.contract_fns.contains(&f);
        // DECISIONS 119: published through its contract only, a parameter
        // is its marker, which stands for the whole argument (an object,
        // a function or a primitive): no ⊤ and no primitive kinds beside it.
        // The reader replays what the body does to it; emitters read it as
        // ⊤ (`Model::bare_marker`).
        let contract_only = contract && self.pub_reason == Some(reason);
        // A closed function's callers are known: what they pass, through
        // its publication only (decision 19).
        let closed = if self.closed_pub_reason == Some(reason) {
            self.closed_params.get(&f).cloned()
        } else {
            None
        };
        for (i, &p) in fparams.iter().enumerate() {
            // A hidden capture parameter holds its class environment slot
            // whoever constructs (DECISIONS 117).
            if let Some(env) = self.capture_param(f, i) {
                for d in [terms[i], p] {
                    if d != NO_NODE {
                        self.edge(env, d);
                    }
                }
                continue;
            }
            let mk = contract.then(|| self.param_marker_tok(f, i as u32));
            for d in [terms[i], p] {
                if d != NO_NODE {
                    match closed.as_ref().and_then(|c| c.get(i).copied()) {
                        Some(a) => self.edge(a, d),
                        None => {
                            if !contract_only {
                                self.add_top(d, why);
                                self.add_prim(d, prim::ALL);
                            }
                        }
                    }
                    if let Some(mk) = mk {
                        self.add_tok(d, mk);
                    }
                    // DECISIONS 148: what the running units pass, beside it.
                    if contract_only {
                        if let Some(&a) = self.method_params.get(&f).and_then(|v| v.get(i)) {
                            if a != NO_NODE {
                                self.edge(a, d);
                            }
                        }
                    }
                }
            }
        }
        let r = self.sum.insts[inst as usize].ret;
        if r != NO_NODE {
            if contract {
                // Design §11.2: the result is checked after the solve.
                self.contract_rets.push((f, r));
            } else {
                self.sink_node(r, reason);
            }
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
                let fed = s.seeds.iter().any(|x| x.0 == sl)
                    || s.edges.iter().any(|e| e.1 == s.this && !is_slot(e.0));
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
fn compose(m: &mut Model, obs_of: &dyn Fn(FnIdx, &Model) -> Vec<NodeId>) -> (usize, usize) {
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
fn expand(s: &mut Summary, ci: usize, g: &Summary, gobs: &[NodeId], caps: &[Option<NodeId>]) {
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
