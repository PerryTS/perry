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
use crate::delta::Tokens;
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
/// The lexical context of a method instance on one object: the receiver
/// token (`recv_lay`), the kind `layout_of` gives one-value tokens.
const LAY_TOK: u32 = 4 << 28;
const LAY_MASK: u32 = (1 << 28) - 1;
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
    /// A for-in body token run over an object token (`Model::for_in_tok`).
    ForIn(CallId, Tok, Tok),
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
    /// Per parameter: a slot nothing of the body writes (no edge, rule,
    /// call result or seed puts a value into it), so it holds only what
    /// the calls pass (DECISIONS 44 (b), `relative_params`).
    pub rel: Vec<bool>,
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
    /// expands a static call into a summary), 1 every other binding (for a
    /// function with a `this` term the receiver: an object as its maker
    /// key, else its layout, `recv_lay`; without one `LAY_NONE`), 2 unknown-code
    /// entry, 3 setter group, 4 getter group, 5 a birth run (the
    /// constructor chain or field initializer of the object token being
    /// born), 6 a for-in body's run for one key (the key's atom, FVA 4.3b).
    /// Nothing in a key is another instance: the count is (sites +
    /// object tokens + layouts) x functions, and the object tokens are the
    /// program's birth sites under their maker contexts (`made_of`).
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
    /// Maker birth contexts: (birth site, maker key) -> context id,
    /// counting down from `BORN`. An object a method or constructor of
    /// object `o` births is `o`'s (`made_ctx`), so a container's backing
    /// store is per container, and a part a constructor makes is per
    /// whole. Construction nests (a part of a part of `o` is `o`'s); a
    /// receiver a method made stands for the object its chain of method
    /// calls started from (`maker_key`), so results of results never form
    /// a chain. A site already on the maker chain folds onto the object
    /// born there.
    pub made_of: FxHashMap<(CallId, Tok), CallId>,
    /// Maker context `BORN - 1 - i` -> (its birth site, its maker object,
    /// made by a method of the maker rather than its construction).
    pub made: Vec<(CallId, Tok, bool)>,
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
        DeleteKey { key, reason } => DeleteKey { key, reason },
        AddsKeys => AddsKeys,
        DefineKey { key, data, reason } => DefineKey { key, data, reason },
        DefineAll {
            o,
            descs,
            value,
            site,
            reason,
        } => DefineAll {
            o: n(o),
            descs: n(descs),
            value: n(value),
            site,
            reason,
        },
        DefineDesc {
            o,
            key,
            site,
            reason,
        } => DefineDesc {
            o: n(o),
            key,
            site,
            reason,
        },
        Implicit { reason, proto } => Implicit { reason, proto },
        CreateProto { reason } => CreateProto { reason },
        ChainUnion { key, dst, of } => ChainUnion {
            key,
            dst: n(dst),
            of,
        },
        ProtoGiven { protos } => ProtoGiven { protos: n(protos) },
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
        PlainKey { origin, act } => PlainKey {
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
        ProtoValues { dst } => ProtoValues { dst: n(dst) },
        IterNext { dst } => IterNext { dst: n(dst) },
        NewTargetHard { reason } => NewTargetHard { reason },
        IdxLoad { idx, dst } => IdxLoad {
            idx: n(idx),
            dst: n(dst),
        },
        IdxKey { obj, dst } => IdxKey { obj, dst: n(dst) },
        IdxMCall { idx, call } => IdxMCall {
            idx: n(idx),
            call: k(call),
        },
        IdxKeyCall { obj, prim, call } => IdxKeyCall {
            obj,
            prim,
            call: k(call),
        },
        ForInBody { obj, call } => ForInBody {
            obj: n(obj),
            call: k(call),
        },
        ForInKeys { ft, call } => ForInKeys { ft, call: k(call) },
        DefineKeyed {
            o,
            reason,
            accessor,
        } => DefineKeyed {
            o: n(o),
            reason,
            accessor,
        },
        StoreKeyed { o, src, site } => StoreKeyed {
            o: n(o),
            src: n(src),
            site,
        },
        AccessorKeyed { holder, f, setter } => AccessorKeyed {
            holder: n(holder),
            f: n(f),
            setter,
        },
        Arith { dst, add } => Arith { dst: n(dst), add },
        ArrayLenArg { dst } => ArrayLenArg { dst: n(dst) },
        ShareBuffer { ta } => ShareBuffer { ta },
        ThisTo { origin } => ThisTo { origin },
        CallArgs { call } => CallArgs { call: k(call) },
        RetInto { call, run } => RetInto { call: k(call), run },
        ArgInto { call, run, i } => ArgInto {
            call: k(call),
            run,
            i,
        },
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
            | DeleteKey { .. }
            | DefineKey { .. }
            | DefineAll { .. }
            | AddsKeys
            | Implicit { .. }
            | IdxLoad { .. }
            | IdxKey { .. }
            | IdxMCall { .. }
            | IdxKeyCall { .. }
            | ForInBody { .. }
            | DefineKeyed { .. }
            | StoreKeyed { .. }
            | AccessorKeyed { .. }
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
        | ProtoOf { dst }
        | ProtoValues { dst } => Some(dst),
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
            | Cons::IdxMCall { call, .. }
            | Cons::IdxKeyCall { call, .. }
            | Cons::ProtoMethods { call, .. }
            | Cons::SuperCtor { call, .. }
            | Cons::SuperCtorParent { call, .. }
            | Cons::CallArgs { call }
            | Cons::ForInBody { call, .. }
            | Cons::ForInKeys { call, .. } => call,
            Cons::Chain {
                act: ChainAct::Call(call),
                ..
            }
            | Cons::AnyAccessors {
                act: ChainAct::Call(call),
                ..
            }
            | Cons::PlainKey {
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
    /// token per birth site), as made by the object whose method or
    /// constructor instance made the call (`made_ctx`: the store a
    /// container's method makes is that container's), or `BORN` when no
    /// program site is behind it (a protocol touch).
    pub(crate) fn birth_ctx(&mut self, call: CallId) -> CallId {
        if (call as usize) < self.sum.n_prog || !self.sum.ready {
            return call;
        }
        let Some(site) = self.origin_of(call) else {
            return BORN;
        };
        match self.sum.call_lay.get(&call).copied() {
            Some(l) => self.made_ctx(site, l),
            None => site,
        }
    }

    /// The object lexical context `lay` names (a constructor run or a
    /// method instance of one object), and whether it is a method's.
    fn lay_object(&self, lay: u32) -> Option<(Tok, bool)> {
        let t = (lay & LAY_MASK) as Tok;
        let by_method = match lay & !LAY_MASK {
            LAY_BIRTH => false,
            LAY_TOK => true,
            _ => return None,
        };
        matches!(self.toks.get(t as usize), Some(TokKind::Obj(..))).then_some((t, by_method))
    }

    /// The birth context of what `site` births inside lexical context
    /// `lay`: the site alone, or the site as made by the object `lay` names.
    /// If that object, or an object on its maker chain, was itself born at
    /// `site`, the birth is that object's context again (a recursive maker
    /// folds onto its ancestor).
    pub(crate) fn made_ctx(&mut self, site: CallId, lay: u32) -> CallId {
        let Some((o, by_method)) = self.lay_object(lay) else {
            return site;
        };
        let mut x = o;
        while let TokKind::Obj(_, c) = self.toks[x as usize] {
            // The walk is solve work: the bound covers a chain that grows.
            self.work += 1;
            match self.made_entry(c) {
                Some((s, _, _)) if s == site => return c,
                Some((_, m, _)) => x = m,
                None if c == site => return c,
                None => break,
            }
        }
        let key = (site, self.maker_key(o, by_method));
        if let Some(&c) = self.sum.made_of.get(&key) {
            return c;
        }
        let c = BORN - 1 - self.sum.made.len() as CallId;
        self.sum.made.push((site, o, by_method));
        self.sum.made_of.insert(key, c);
        c
    }

    /// The maker as a context key: the object itself, except that a
    /// method's receiver which a method made stands for the object that
    /// chain of method calls started from (what a method makes from a
    /// method's result is that origin's, never per the chain of calls).
    fn maker_key(&self, mut o: Tok, by_method: bool) -> Tok {
        if by_method {
            while let TokKind::Obj(_, c) = self.toks[o as usize] {
                match self.made_entry(c) {
                    Some((_, m, true)) => o = m,
                    _ => break,
                }
            }
        }
        o
    }

    /// The (site, maker, by a method) of maker context `c`, if it is one.
    fn made_entry(&self, c: CallId) -> Option<(CallId, Tok, bool)> {
        if c >= BORN {
            return None;
        }
        self.sum.made.get((BORN - 1 - c) as usize).copied()
    }

    /// The lexical context of a method instance for receiver `t`: an
    /// object is its maker key's context (what the method births is that
    /// object's), every other value its layout. A receiver a method made
    /// runs its methods as the object its chain of method calls started
    /// from, as it births as that object's (`maker_key`): every object of
    /// a persistent structure births the same (site, origin) objects, so
    /// one instance per origin holds them all. What a method reads from
    /// its receiver stays per receiver (argument markers, DECISIONS 44).
    /// An argument marker is no object of this unit and births nothing
    /// here: its runs share the context of a call with no receiver
    /// (DECISIONS 201). There is one per node, and a run's closure context
    /// is storage that takes its own marker, so a context per marker made
    /// a new run per run.
    pub(crate) fn recv_lay(&self, t: Tok) -> u32 {
        if marker_top_bit(&self.arg_bits, t) {
            return LAY_NONE;
        }
        match self.toks[t as usize] {
            TokKind::Obj(..) if t <= LAY_MASK => LAY_TOK | self.maker_key(t, true),
            _ => self.layout_of(t),
        }
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
                u32::MAX => {
                    let n = self.node_named("inst", || format!("inst{inst}.{i}"));
                    // A fresh slot holds one run's values.
                    self.nodes[n as usize].frame = inst + 1;
                    n
                }
                _ if ctx == NO_CTX => s.orig[i as usize],
                j => self.sum.ctx_nodes[ctx as usize][j as usize],
            })
            .collect();
        // A for-in body's run for one key (FVA 4.3b): its copy of the key
        // variable holds that key.
        if key.0 == 6 {
            for (j, o) in s.orig.iter().enumerate() {
                if self.for_in_key_vars.contains(o)
                    && self.nodes[base[j] as usize].frame == inst + 1
                {
                    self.key_nodes.insert(base[j], key.1);
                }
            }
        }
        let tr = |x: NodeId| {
            if is_slot(x) {
                base[(x & !SLOT) as usize]
            } else {
                x
            }
        };
        let this = tr(s.this);
        // Its result: what it returns of no parameter leaves as one marker
        // (MRK, `Model::crosses`).
        let ret = tr(s.ret);
        if ret != NO_NODE && self.nodes[ret as usize].frame == inst + 1 {
            self.nodes[ret as usize].result = true;
        }
        // Its parameters hold argument markers (DECISIONS 44), relative
        // ones while only the calls write them (44 (b)).
        for (j, &p) in s.params.iter().enumerate() {
            if is_slot(p) {
                let n = &mut self.nodes[tr(p) as usize];
                n.param = true;
                n.rel = s.rel.get(j).is_some_and(|&r| r) && n.frame == inst + 1;
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
            ret,
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
    /// receiver (`recv_lay`: the object's maker key, else its layout) gets the
    /// receiver, and the call's arguments and result once per (call,
    /// instance).
    pub(crate) fn bind_layout(&mut self, call: CallId, ft: Tok, x: Option<Tok>, prim: u16) {
        let f = self.tok_fn(ft);
        if call != NO_NODE && self.composed_away(call, f) {
            return;
        }
        let lay = match x {
            Some(t) if self.has_this_term(f) => self.recv_lay(t),
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

    /// The nodes `this` of function value `ft` reaches its body through:
    /// the shared node, and its instance's when the body is summarized.
    pub(crate) fn this_nodes(&mut self, ft: Tok) -> Vec<NodeId> {
        let f = self.tok_fn(ft);
        let mut dsts = vec![self.funcs[f as usize].this];
        if self.has_this_term(f) {
            let inst = self.instance((2, 0, ft));
            dsts.push(self.sum.insts[inst as usize].this);
        }
        dsts
    }

    pub(crate) fn escape_this(&mut self, ft: Tok, reason: u32) {
        if !self.sum.ready {
            self.sum.deferred.push(Deferred::EscapeThis(ft, reason));
            return;
        }
        let f = self.tok_fn(ft);
        let why = |m: &Self| format!("this of escaped function: {}", m.reasons[reason as usize]);
        let dsts = self.this_nodes(ft);
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

    /// Run for-in body `ft` (through template call `call`) over the iterated
    /// value `t` (FVA 4.3b): once per key of a literal whose keys are exactly
    /// its birth keys (`for_in_keys`), each run's key parameter holding its
    /// one key, else once with every key.
    pub(crate) fn for_in_tok(&mut self, call: CallId, ft: Tok, t: Tok) {
        if !self.sum.ready {
            self.sum.deferred.push(Deferred::ForIn(call, ft, t));
            return;
        }
        let f = self.tok_fn(ft);
        let keys = match self.summarized(f) && self.instantiable(ft) {
            true => self.for_in_keys(f, t, None),
            false => None,
        };
        let Some(keys) = keys else {
            return self.for_in_every_key(call, ft);
        };
        for k in keys {
            let inst = self.instance((6, k, ft));
            match self.sum.insts[inst as usize].params.first().copied() {
                // The run reads no key.
                Some(NO_NODE) => {}
                // A fresh slot: the run's key (its key variable's copy holds
                // the key's atom, `instance`).
                Some(p) if self.nodes[p as usize].frame == inst + 1 => self.add_prim(p, prim::STR),
                _ => return self.for_in_every_key(call, ft),
            }
        }
        self.for_in_runs.push((call, ft, t));
    }

    /// Run for-in body `ft` once with every key (the loop as it is).
    pub(crate) fn for_in_every_key(&mut self, call: CallId, ft: Tok) {
        if self.for_in_any.insert((call, ft)) {
            self.call_tok(call, ft, None);
        }
    }

    /// The keys a for-in run of body `f` over `t` visits, when they are
    /// exactly `t`'s: a literal not escaped and not HARD, with no key added
    /// after birth or stored under a computed key, no computed member, no
    /// `__proto__`, whose chain gets no enumerable key from the program
    /// (nothing writes `Object.prototype`, `object_proto_written`); a body
    /// defining a property under its key also needs each key to be one
    /// generation named (`for_in_define_keys`). `late`: the keys stored into
    /// each template (`settle_for_in`; the check before it only knows the
    /// cheap facts).
    fn for_in_keys(
        &self,
        f: FnIdx,
        t: Tok,
        late: Option<&FxHashMap<TmplId, Vec<Atom>>>,
    ) -> Option<Vec<Atom>> {
        let TokKind::Obj(tm, _) = self.toks[t as usize] else {
            return None;
        };
        let tpl = &self.tmpls[tm as usize];
        let literal = matches!(
            tpl.kind,
            TmplKind::Lit { .. }
                | TmplKind::Class {
                    literal_shape: true,
                    ..
                }
        );
        if !literal
            || tpl.has_computed_members
            || self.is_escaped(t)
            || self.hard.contains_key(&t)
            || self.adds_keys.get(&tm).is_some_and(|&n| n > 0)
            || self.any_writers.contains_key(&tm)
            || self.object_proto_written()
        {
            return None;
        }
        let mut keys: Vec<Atom> = Vec::new();
        let own = tpl.birth_keys.iter().chain(&tpl.accessor_keys);
        // The literal's methods, not its prototype's `constructor` (which
        // is not enumerable).
        let methods = tpl
            .proto_members
            .iter()
            .filter(|&&a| self.atoms[a as usize] != "constructor");
        for &a in own.chain(methods) {
            let name = self.atoms[a as usize].as_str();
            if name == "__proto__" {
                return None;
            }
            // A symbol is no for-in key.
            if !name.starts_with(crate::builtins::WK_PREFIX) && !keys.contains(&a) {
                keys.push(a);
            }
        }
        if late.is_some_and(|l| {
            l.get(&tm)
                .is_some_and(|ws| ws.iter().any(|a| !keys.contains(a)))
        }) {
            return None;
        }
        if self.for_in_defines.contains(&f)
            && keys.iter().any(|a| !self.for_in_define_keys.contains(a))
        {
            return None;
        }
        Some(keys)
    }

    /// May `Object.prototype`, the chain of every literal, hold a key the
    /// program wrote (`native_writes` on it: a store, an accessor, E5/E6/E7,
    /// a reach into unmodeled code, another unit's write)? A for-in over a
    /// literal visits such a key when it is enumerable. A write into ⊤ is
    /// no such write: ⊤ holds no builtin prototype (`escape`: a runtime
    /// object reaches unknown code as itself, and what may write it is
    /// noted on it), and a literal whose keys are exactly its own is not
    /// escaped, so not in ⊤ either. The write into ⊤ stays a fact about
    /// ⊤'s members, which lookups through ⊤ already read as anything.
    fn object_proto_written(&self) -> bool {
        self.native_writes
            .keys()
            .any(|&(w, _)| matches!(self.toks[w as usize], TokKind::ObjectProto))
    }

    /// Once the worklist is empty: a literal a for-in body ran per key over
    /// that gained a key after birth (or escaped, or turned HARD) since runs
    /// the body once more with every key. True when one did.
    pub(crate) fn settle_for_in(&mut self) -> bool {
        if self.for_in_runs.is_empty() {
            return false;
        }
        let tms: crate::pts::FxHashSet<TmplId> = self
            .for_in_runs
            .iter()
            .filter_map(|&(_, _, t)| self.tmpl_of(t))
            .collect();
        let mut late: FxHashMap<TmplId, Vec<Atom>> = FxHashMap::default();
        for &(tm, a) in self.writers.keys().chain(self.proto_writers.keys()) {
            if tms.contains(&tm) {
                late.entry(tm).or_default().push(a);
            }
        }
        let mut more = false;
        for (call, ft, t) in self.for_in_runs.clone() {
            if self.for_in_any.contains(&(call, ft)) {
                continue;
            }
            let f = self.tok_fn(ft);
            if self.for_in_keys(f, t, Some(&late)).is_none() {
                self.for_in_every_key(call, ft);
                more = true;
            }
        }
        more
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
        // Literals born here: one token per (literal, maker object), or one
        // per literal in an instance no object is the context of.
        for &(slot, tm) in &s.births {
            let ctx = self.made_ctx(BORN, lay);
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
        if r == NO_NODE {
            return;
        }
        let cret = self.calls[c].ret;
        if !self.rel_into(call, inst + 1, &terms) {
            // A summary consisting only of an unwritten parameter copied
            // to its return passes this call's value back. Its shared
            // instance still observes every argument, but the caller must
            // not receive other calls' values (e.g. an identity helper
            // returning two distinct function arguments).
            let s = &self.sum.of[&f];
            let copy = match s.edges.as_slice() {
                [(from, to)]
                    if *to == s.ret
                        && is_slot(*from)
                        && is_slot(*to)
                        && s.seeds.is_empty()
                        && s.cons.is_empty()
                        && s.calls.is_empty()
                        && s.births.is_empty()
                        && s.closures.is_empty()
                        && self.calls[c].spread.is_empty() =>
                {
                    s.params
                        .iter()
                        .position(|p| p == from)
                        .filter(|&i| s.rel[i] && caps[i].is_none())
                }
                _ => None,
            };
            if let Some(i) = copy {
                match self.calls[c].args.get(i).copied() {
                    Some(a) => self.edge(a, cret),
                    None => self.add_prim(cret, prim::UNDEF),
                }
            } else {
                self.edge(r, cret);
            }
        }
    }

    /// Relative transfer (DECISIONS 44 (b)): with markers in the unit and
    /// no spread argument, what run `run`'s result returns of a relative
    /// parameter's marker family is `call`'s argument there. False when
    /// the call takes the result as it is (a copy edge).
    pub(crate) fn rel_into(&mut self, call: CallId, run: u32, params: &[NodeId]) -> bool {
        let c = call as usize;
        let n = params.len().min(self.calls[c].args.len());
        let rel: Vec<usize> = (0..n)
            .filter(|&i| params[i] != NO_NODE && self.nodes[params[i] as usize].rel)
            .collect();
        if self.param_marker.is_empty() || !self.calls[c].spread.is_empty() || rel.is_empty() {
            return false;
        }
        let r = self.run_ret(run);
        let cret = self.calls[c].ret;
        self.rel(|| format!("copy {r} {cret}"));
        self.cons(r, Cons::RetInto { call, run });
        for i in rel {
            let a = self.calls[c].args[i];
            self.cons(
                a,
                Cons::ArgInto {
                    call,
                    run,
                    i: i as u32,
                },
            );
        }
        true
    }

    /// The result node and the parameters of run `run`.
    fn run_ret(&self, run: u32) -> NodeId {
        match run & FN_RUN {
            0 => self.sum.insts[(run - 1) as usize].ret,
            _ => self.funcs[(run & !FN_RUN) as usize].ret,
        }
    }

    fn run_params(&self, run: u32) -> &[NodeId] {
        match run & FN_RUN {
            0 => &self.sum.insts[(run - 1) as usize].params,
            _ => &self.funcs[(run & !FN_RUN) as usize].params,
        }
    }

    /// `Cons::RetInto`: run `run`'s result values `toks` reach `call`'s
    /// result. A token of the family of the run's argument marker for
    /// parameter i (while it stays in its run) is that parameter's value at
    /// this call: for each marker `m` of the call's argument i, the same
    /// token of `m` (`Model::lift`). Every other token arrives as itself.
    pub(crate) fn ret_into(&mut self, call: CallId, run: u32, toks: Tokens<'_>, prim: u16) {
        let cret = self.calls[call as usize].ret;
        let mut raw: Vec<Tok> = Vec::new();
        let mut lifted: Vec<Tok> = Vec::new();
        let mut args: Vec<Option<Vec<Tok>>> = Vec::new();
        for &t in toks {
            let (b, h) = self.fam.get(t as usize).copied().unwrap_or((0, 0));
            if h != run {
                raw.push(t);
                continue;
            }
            let Some(i) = self.param_of(run, b) else {
                raw.push(t);
                continue;
            };
            if args.len() <= i {
                args.resize(i + 1, None);
            }
            if args[i].is_none() {
                args[i] = Some(self.arg_markers(call, i));
            }
            for &m in args[i].as_ref().unwrap() {
                // Each lifted token is solve work, as a delivery is.
                self.work += 1;
                let x = self.lift(m, b, t);
                lifted.push(x);
            }
        }
        raw.extend(lifted);
        self.add(cret, &raw, prim);
    }

    /// `Cons::ArgInto`: argument i of `call` gains `toks`; each marker
    /// among them is what run `run`'s result returns of the parameter's
    /// marker family so far (`ret_into` lifts the later ones).
    pub(crate) fn arg_into(&mut self, call: CallId, run: u32, i: u32, toks: Tokens<'_>) {
        let ms: Vec<Tok> = toks
            .iter()
            .copied()
            .filter(|&t| self.is_marker(t))
            .collect();
        if ms.is_empty() {
            return;
        }
        let p = self.run_params(run)[i as usize];
        let Some(&b) = self.arg_marker_of.get(&p) else {
            return;
        };
        if self.fam_home(b) != run {
            return;
        }
        let r = self.run_ret(run);
        let held: Vec<Tok> = self
            .family(b)
            .into_iter()
            .filter(|&t| self.nodes[r as usize].pts.contains_tok(t))
            .collect();
        if held.is_empty() {
            return;
        }
        let mut out = Vec::with_capacity(ms.len() * held.len());
        for &m in &ms {
            for &t in &held {
                self.work += 1;
                out.push(self.lift(m, b, t));
            }
        }
        let cret = self.calls[call as usize].ret;
        self.add(cret, &out, 0);
    }

    /// The parameter of run `run` whose argument marker is `b`.
    fn param_of(&self, run: u32, b: Tok) -> Option<usize> {
        self.run_params(run)
            .iter()
            .position(|p| self.arg_marker_of.get(p) == Some(&b))
    }

    /// The markers argument i of `call` holds now (none for an argument
    /// the call does not pass: the parameter is `undefined` there).
    fn arg_markers(&self, call: CallId, i: usize) -> Vec<Tok> {
        let Some(&a) = self.calls[call as usize].args.get(i) else {
            return Vec::new();
        };
        let mut v = Vec::new();
        self.nodes[a as usize].pts.sorted_into(&mut v);
        v.retain(|&t| self.is_marker(t));
        v
    }

    /// A compiled function without a summary has one body for every call:
    /// that body is its run (`FN_RUN | f`), so its parameters' markers are
    /// tokens there and its result returns them to each call relative to
    /// it (DECISIONS 44 (b)), not as a crossing marker (MRK). Each
    /// parameter gets a binding the calls write and nothing in the body
    /// does; the body's variable takes the binding and the writes (as
    /// `bind_written_params` does for a summary).
    fn run_unsummarized(&mut self) {
        let one = |m: &Self, f: FnIdx| {
            f != NO_FN && !m.summarized(f) && m.funcs[f as usize].func_id != u32::MAX
        };
        for n in 0..self.nodes.len() {
            let f = self.node_fn[n];
            if one(self, f) && !self.nodes[n].obs && self.nodes[n].frame == 0 {
                self.nodes[n].frame = FN_RUN | f;
            }
        }
        for f in 0..self.funcs.len() as FnIdx {
            if !one(self, f) {
                continue;
            }
            let run = FN_RUN | f;
            let ret = self.funcs[f as usize].ret;
            if ret != NO_NODE {
                self.nodes[ret as usize].frame = run;
                self.nodes[ret as usize].result = true;
            }
            for i in 0..self.funcs[f as usize].params.len() {
                let p = self.funcs[f as usize].params[i];
                if p == NO_NODE {
                    continue;
                }
                let b = self.node_l("param-bind");
                self.node_fn[b as usize] = f;
                self.nodes[b as usize].frame = run;
                self.nodes[b as usize].rel = true;
                self.nodes[p as usize].frame = run;
                self.edge(b, p);
                self.funcs[f as usize].params[i] = b;
            }
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
            }
            // Beyond its publication (an export or a published literal's
            // field, DECISIONS 210, or known closed callers, DECISIONS 19),
            // unknown code calls it and retains its result.
            if !contract
                || (self.pub_reason != Some(reason)
                    && self.field_fn_reason != Some(reason)
                    && closed.is_none())
            {
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
        self.run_unsummarized();
        self.mark_params();
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
                Deferred::ForIn(call, ft, t) => self.for_in_tok(call, ft, t),
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

#[path = "summary/compose.rs"]
mod compose;
use compose::compose;
pub use compose::compress;
