//! The abstract model: tokens (abstract values), templates (birth units),
//! functions, nodes, and the constraint vocabulary the solver understands.
//!
//! Every relation here has a one-line text form in the relations dump
//! (`dump::write_relations`), which `scripts/wpfacts_recompute.py` re-solves
//! independently from the same rules (design 6.6).

use crate::pts::{FxHashMap, Pts};

pub type NodeId = u32;
pub type Tok = u32;
pub type TmplId = u32;
pub type FnIdx = u32;
pub type Atom = u32;
pub type CallId = u32;
pub type SiteId = u32;

/// Token 0 is ⊤: a value produced or touched by code the analysis does not
/// model. Its presence in a node means "any escaped object or anything else".
pub const TOP: Tok = 0;

/// Primitive kind bits carried next to the object tokens of a node.
pub mod prim {
    pub const NUM: u16 = 1 << 0;
    pub const STR: u16 = 1 << 1;
    pub const BOOL: u16 = 1 << 2;
    pub const NULL: u16 = 1 << 3;
    pub const UNDEF: u16 = 1 << 4;
    pub const BIGINT: u16 = 1 << 5;
    pub const SYMBOL: u16 = 1 << 6;
    /// An i32-ranged number (only a subset of NUM; slice 1 never proves it,
    /// the bit exists so the dump format is stable for slice 3).
    pub const ALL: u16 = NUM | STR | BOOL | NULL | UNDEF | BIGINT | SYMBOL;

    pub fn name(bits: u16) -> String {
        let mut v = Vec::new();
        for (b, n) in [
            (NUM, "num"),
            (STR, "str"),
            (BOOL, "bool"),
            (NULL, "null"),
            (UNDEF, "undef"),
            (BIGINT, "bigint"),
            (SYMBOL, "symbol"),
        ] {
            if bits & b != 0 {
                v.push(n);
            }
        }
        v.join("|")
    }
}

/// Global builtin namespaces and constructors the HIR can name as a value.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum BuiltinNs {
    Object,
    Reflect,
    Array,
    Json,
    Math,
    Promise,
    Console,
    GlobalThis,
    Process,
    Error,
    Symbol,
    Number,
    String,
    Boolean,
    Function,
    Other,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum TokKind {
    Top,
    /// An instance of a birth template.
    Obj(TmplId),
    /// A function value (closure, function declaration, method body).
    Func(FnIdx),
    /// `f.bind(t, ...)` result: index into `Model::bound`.
    Bound(u32),
    /// The class constructor value of a class template.
    ClassObj(TmplId),
    /// The prototype object of a class / constructor-function template.
    Proto(TmplId),
    /// `Object.prototype` (and every builtin prototype, collapsed).
    ObjectProto,
    /// A global builtin namespace / constructor.
    Builtin(BuiltinNs),
    /// An opaque runtime function value (a builtin method read as a value).
    BuiltinFn,
    /// A runtime-created container that holds only primitives (split/match
    /// results, `Object.keys` arrays …). Shared by every such container.
    PrimBox,
    /// A module's namespace / `module.exports` object.
    ModNs(u32),
    /// The prototype of a native parent class (`extends EventEmitter`),
    /// whose methods have summaries in `builtins.rs`.
    NativeProto(String),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TmplKind {
    /// A HIR class (user class, `__AnonShape_*` literal class, class
    /// expression template). `literal_shape` = `Class::is_literal_shape()`.
    Class {
        module: u32,
        name: String,
        class_id: u32,
        literal_shape: bool,
    },
    /// An object literal site that stays an `Expr::Object` / `ObjectSpread`.
    Lit { module: u32, site: SiteId },
    /// Instances of `new f()` on a constructor-function body.
    FnInst { func: FnIdx },
    /// `Object.create(p)` result at a site (a separate template; E10 makes
    /// the prototype's owners HARD as well).
    ObjCreate { module: u32, site: SiteId },
    /// An array literal / array-producing site (flow only).
    Arr { module: u32, site: SiteId },
    /// A builtin object created at a site (Map, Set, Promise, Date, Error,
    /// RegExp, typed array, ArrayBuffer, iterator …). Flow only.
    BObj {
        module: u32,
        site: SiteId,
        what: &'static str,
    },
}

impl TmplKind {
    /// Birth templates the facts are about (design section 2).
    pub fn is_fact_unit(&self) -> bool {
        matches!(
            self,
            TmplKind::Class { .. } | TmplKind::Lit { .. } | TmplKind::FnInst { .. }
        )
    }
}

#[derive(Clone, Debug)]
pub struct Template {
    pub kind: TmplKind,
    pub display: String,
    /// Birth keys in birth order (root → leaf for classes).
    pub birth_keys: Vec<Atom>,
    /// Birth keys that are definitely present at birth.
    pub definite: Vec<Atom>,
    /// Parent template (class `extends`), when statically known.
    pub parent: Option<TmplId>,
    /// The constructor function of a class template (None: implicit ctor).
    pub ctor: Option<FnIdx>,
    /// Native parent (`extends EventEmitter/Error/...`): `(module, class)`.
    pub native_parent: Option<(String, String)>,
    /// Accessor names declared on the class (getters/setters): never data
    /// fields (E25).
    pub accessor_keys: Vec<Atom>,
    /// The prototype carries a computed-key member (symbol methods, etc.).
    pub has_computed_members: bool,
    /// Synthetic function whose `this` runs the class field initializers.
    pub field_init: Option<FnIdx>,
    /// Keys the class defines on its prototype at class creation (methods,
    /// accessors, `constructor`): a lookup that finds one stops there.
    pub proto_members: Vec<Atom>,
}

#[derive(Clone, Debug)]
pub struct FuncInfo {
    pub module: u32,
    pub func_id: u32,
    pub name: String,
    pub params: Vec<NodeId>,
    /// The rest parameter's array node (its elements receive extra args).
    pub rest: Option<NodeId>,
    pub this: NodeId,
    pub ret: NodeId,
    /// Holds every argument (the `arguments` object's elements + extra args).
    pub args_all: NodeId,
    pub token: Tok,
    /// This function is the constructor of that class template.
    pub ctor_of: Option<TmplId>,
    /// Arrow function / `captures_this` closure: `this` is the enclosing one.
    pub lexical_this: bool,
}

#[derive(Clone, Debug)]
pub enum ThisArg {
    Undef,
    Node(NodeId),
    Tok(Tok),
}

#[derive(Clone, Debug)]
pub struct CallSite {
    pub this: ThisArg,
    pub args: Vec<NodeId>,
    /// Spread argument arrays: their elements may land in any parameter.
    pub spread: Vec<NodeId>,
    pub ret: NodeId,
    pub site: SiteId,
}

/// A property key: a named atom, or any key (computed).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Key {
    Named(Atom),
    Any,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ChainAct {
    Load(NodeId),
    Call(CallId),
    Store(NodeId),
}

/// HARD escape families (design section 4).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Hard {
    E5DefineProperty,
    E6SetPrototypeOf,
    E7Delete,
    E8Freeze,
    E9Proxy,
    E10ObjectCreate,
    E11ReflectConstruct,
    E15With,
    E21NativeParent,
    B1Exposed,
    B2BufferResize,
    B3Transfer,
    B4Resizable,
}

impl Hard {
    pub fn code(self) -> &'static str {
        match self {
            Hard::E5DefineProperty => "E5",
            Hard::E6SetPrototypeOf => "E6",
            Hard::E7Delete => "E7",
            Hard::E8Freeze => "E8",
            Hard::E9Proxy => "E9",
            Hard::E10ObjectCreate => "E10",
            Hard::E11ReflectConstruct => "E11",
            Hard::E15With => "E15",
            Hard::E21NativeParent => "E21",
            Hard::B1Exposed => "B1",
            Hard::B2BufferResize => "B2",
            Hard::B3Transfer => "B3",
            Hard::B4Resizable => "B4",
        }
    }
}

/// Complex constraints, attached to the node whose new tokens trigger them.
#[derive(Clone, Debug)]
pub enum Cons {
    /// dst ⊇ read(t, key) for every t in the node.
    Load { key: Key, dst: NodeId },
    /// write(t, key) ⊇ src for every t in the node.
    Store { key: Key, src: NodeId, site: SiteId },
    /// The node is a callee.
    Call { call: CallId },
    /// The node is a callee whose `this` is fixed to one receiver token
    /// (a method found on that receiver or its chain).
    CallThis { call: CallId, this: Tok },
    /// The node is a `new` callee.
    New { call: CallId },
    /// The node is a method-call receiver (`recv.key(...)`).
    MCall { key: Key, call: CallId },
    /// The node is `ProtoNode(cur)`: continue a lookup for `origin`.
    Chain {
        origin: Tok,
        key: Key,
        act: ChainAct,
    },
    /// Every token escapes (goes to ⊤), with the reason.
    TopSink { reason: u32 },
    /// Every object token reaches a HARD operation.
    Hard { kind: Hard, reason: u32 },
    /// Every token gains keys after birth (computed store …).
    AddsKeys,
    /// Builtin protocol touch: ToPrimitive / iterator / toJSON / then …
    Implicit { reason: u32 },
    /// The node holds `Object.create` prototype arguments (E10).
    CreateProto { reason: u32 },
    /// The node is `ProtoNode(t)`: dst ⊇ the values of `key` on each
    /// prototype (and further up), shared by every receiver of `t`.
    ChainUnion { key: Key, dst: NodeId },
    /// The node is `ProtoNode(t)`: dst ⊇ every prototype, transitively.
    ProtoClosure { dst: NodeId },
    /// The node is the prototype closure of receiver `origin`: builtin
    /// prototype methods (Object.prototype, native parents) run for it.
    ProtoMethods { origin: Tok, key: Key, call: CallId },
    /// The node is the prototype closure of `origin`: accessors of every
    /// holder on it run for a computed-key lookup with `origin` as `this`.
    AnyAccessors { origin: Tok, act: ChainAct },
    /// The node is the prototype closure of `origin`: an unknown (⊤)
    /// prototype may hold a setter, so the stored value and `origin` escape.
    StoreTopProto { origin: Tok, src: NodeId },
    /// The node is `this`: continue a `super.key` lookup at `start` with
    /// each `this` token as the receiver.
    Super { start: Tok, key: Key, act: ChainAct },
    /// The node is `this` of a constructor of `class`: run the parent
    /// constructor chain for each instance (`super(...)`).
    SuperCtor { class: TmplId, call: CallId },
    /// The node holds method closures installed on `recv` (literal methods):
    /// their `this` is `recv`.
    ThisFrom { recv: NodeId },
    /// The node holds getter/setter functions installed on the objects of
    /// `holder` under `key`.
    Accessor {
        holder: NodeId,
        key: Atom,
        setter: bool,
    },
    /// Internal: install accessor `f` on every token of the node.
    AccessorInstall { key: Atom, f: FnIdx, setter: bool },
    /// The node holds objects whose prototype set includes `src`'s values.
    ProtoFrom { src: NodeId },
    /// dst ⊇ prototypes of the node's objects (`Object.getPrototypeOf`).
    ProtoOf { dst: NodeId },
    /// dst ⊇ the `next` method of each iterator in the node.
    IterNext { dst: NodeId },
    /// The node holds `Reflect.construct` new-targets (E11).
    NewTargetHard { reason: u32 },
    /// The node is the object of `o[idx]` with a computed key: dst ⊇ its
    /// elements/fields; the full keyed lookup (prototype chain, builtin
    /// members) runs only if `idx` can hold a non-number key (`IdxKey`).
    IdxLoad { idx: NodeId, dst: NodeId },
    /// The node is the key of `o[k]` for object token `obj`: a string,
    /// symbol or object key runs the full lookup.
    IdxKey { obj: Tok, dst: NodeId },
    /// The node is an arithmetic operand: dst ⊇ the kinds its values turn
    /// into (`+` when `add`: strings concatenate).
    Arith { dst: NodeId, add: bool },
    /// The node holds the source of a typed array `ta`: a buffer or view
    /// there means `ta` shares (and so exposes) that buffer (B1).
    ShareBuffer { ta: Tok },
}

impl Cons {
    pub fn kind_name(&self) -> &'static str {
        match self {
            Cons::Load { .. } => "load",
            Cons::Store { .. } => "store",
            Cons::Call { .. } => "call",
            Cons::CallThis { .. } => "callthis",
            Cons::New { .. } => "new",
            Cons::MCall { .. } => "mcall",
            Cons::Chain { .. } => "chain",
            Cons::TopSink { .. } => "top",
            Cons::Hard { .. } => "hard",
            Cons::AddsKeys => "addskeys",
            Cons::Implicit { .. } => "implicit",
            Cons::CreateProto { .. } => "createproto",
            Cons::ChainUnion { .. } => "chainunion",
            Cons::ProtoClosure { .. } => "protoclosure",
            Cons::ProtoMethods { .. } => "protomethods",
            Cons::AnyAccessors { .. } => "anyaccessors",
            Cons::StoreTopProto { .. } => "storetopproto",
            Cons::Super { .. } => "super",
            Cons::SuperCtor { .. } => "superctor",
            Cons::ThisFrom { .. } => "thisfrom",
            Cons::Accessor { .. } => "accessor",
            Cons::AccessorInstall { .. } => "accinstall",
            Cons::ProtoFrom { .. } => "protofrom",
            Cons::ProtoOf { .. } => "protoof",
            Cons::IterNext { .. } => "iternext",
            Cons::NewTargetHard { .. } => "newtargethard",
            Cons::Arith { .. } => "arith",
            Cons::IdxLoad { .. } => "idxload",
            Cons::IdxKey { .. } => "idxkey",
            Cons::ShareBuffer { .. } => "sharebuffer",
        }
    }
}

#[derive(Default, Clone, Debug)]
pub struct Node {
    pub pts: Pts,
    pub prim: u16,
    pub delta: Vec<Tok>,
    pub dprim: u16,
    pub succ: Vec<NodeId>,
    pub cons: Vec<Cons>,
    pub in_wl: bool,
    /// The set grew past the widening bound: it holds only ⊤ from now on,
    /// and every token that reaches it escapes.
    pub widened: bool,
}

/// A store site: where, and whether it is a construction-phase writer for
/// instances of a class whose constructor chain contains it.
#[derive(Clone, Debug)]
pub struct StoreSite {
    pub module: u32,
    pub func: FnIdx,
    pub desc: String,
    /// `this.f = v` in the constructor of this template, before `this`
    /// leaks (construction phase).
    pub ctor_phase_of: Option<TmplId>,
    /// A literal birth initializer (construction by definition).
    pub birth: bool,
    /// `this.f = v` in this function's body before `this` leaks: a
    /// construction-phase writer for `new f()` instances.
    pub ctor_fn: Option<FnIdx>,
}

#[derive(Default, Clone, Debug)]
pub struct FieldWriters {
    pub ctor: Vec<SiteId>,
    pub post: Vec<SiteId>,
}

#[derive(Default)]
pub struct Model {
    pub toks: Vec<TokKind>,
    pub tok_index: FxHashMap<TokKind, Tok>,
    pub tmpls: Vec<Template>,
    pub funcs: Vec<FuncInfo>,
    pub nodes: Vec<Node>,
    pub node_names: Vec<String>,
    pub labels: Vec<&'static str>,
    /// Points-to entries ever inserted (work bound; see `max_pts`).
    pub pts_entries: u64,
    pub max_pts: u64,
    /// Solve work so far and its bound (0 = none); see `FactsConfig::max_work`.
    pub work: u64,
    pub max_work: u64,
    pub max_nodes: usize,
    /// Widening bound on one points-to set (0 = none).
    pub widen_k: usize,
    pub widen_queue: Vec<Tok>,
    pub widened_nodes: u64,
    pub atoms: Vec<String>,
    pub atom_index: FxHashMap<String, Atom>,
    pub calls: Vec<CallSite>,
    pub stores: Vec<StoreSite>,
    /// (target fns node, bound this, bound args)
    pub bound: Vec<(NodeId, NodeId, Vec<NodeId>)>,
    pub fields: FxHashMap<(Tok, Atom), NodeId>,
    pub field_any: FxHashMap<Tok, NodeId>,
    pub all_fields: FxHashMap<Tok, NodeId>,
    pub proto_node: FxHashMap<Tok, NodeId>,
    pub field_keys: FxHashMap<Tok, Vec<Atom>>,
    /// accessors[(proto-or-object tok, key)] = (getter fns, setter fns)
    pub accessors: FxHashMap<(Tok, Atom), (Vec<FnIdx>, Vec<FnIdx>)>,
    pub accessor_keys: FxHashMap<Tok, Vec<Atom>>,
    pub writers: FxHashMap<(TmplId, Atom), FieldWriters>,
    pub any_writers: FxHashMap<TmplId, FieldWriters>,
    pub adds_keys: FxHashMap<TmplId, u32>,
    pub hard: FxHashMap<Tok, (Hard, u32)>,
    pub escaped: FxHashMap<Tok, u32>,
    pub reasons: Vec<String>,
    pub reason_index: FxHashMap<String, u32>,
    pub top_node: NodeId,
    pub prim_nodes: [NodeId; 8],
    pub all_prim_node: NodeId,
    pub top_tok_node: NodeId,
    pub wl: std::collections::VecDeque<NodeId>,
    // dedup of bindings / chain walks
    pub bind_done: crate::pts::FxHashSet<(CallId, Tok, Tok)>,
    pub chain_done: crate::pts::FxHashSet<(NodeId, Tok, Key, ChainAct)>,
    pub implicit_done: crate::pts::FxHashSet<Tok>,
    pub construct_done: crate::pts::FxHashSet<(CallId, Tok)>,
    pub idx_done: crate::pts::FxHashSet<(NodeId, Tok, NodeId, u8)>,
    pub callthis_done: crate::pts::FxHashSet<(NodeId, CallId, Tok)>,
    pub sunk_nodes: crate::pts::FxHashSet<NodeId>,
    pub any_walk_done: crate::pts::FxHashSet<(NodeId, Tok, ChainAct)>,
    pub any_walks: Vec<(NodeId, Tok, Tok, ChainAct)>,
    /// Calls the runtime makes for a protocol (ToPrimitive, iteration …).
    pub protocol_calls: crate::pts::FxHashSet<CallId>,
    /// Shared prototype-chain value nodes: (token, key) → node.
    pub cv_nodes: FxHashMap<(Tok, Key), NodeId>,
    /// Prototype closure nodes: token → node.
    pub cp_nodes: FxHashMap<Tok, NodeId>,
    /// Shared chain unions that stopped at a declared member: resumed when
    /// that prototype is mutated.
    pub stopped_shared: FxHashMap<Tok, Vec<(Key, NodeId)>>,
    /// Keys some accessor may be defined under (known at generation): only
    /// lookups of these keys walk the chain per receiver.
    pub accessor_atoms: crate::pts::FxHashSet<Atom>,

    /// The relations dump, if requested: one line per primitive relation.
    pub relations: Option<Vec<String>>,
    /// Relation phase tag: b'G' generation (inputs), b'S' solver rules
    /// (re-derived by the checker), b'B' builtin summaries (axioms).
    pub rel_phase: u8,
    pub stats_cons: u64,
    pub site_tmpls: FxHashMap<(CallId, &'static str), Tok>,
    /// Typed-array owner templates born by `TypedArrayNew` with a known
    /// element kind (`perry_hir::TYPED_ARRAY_KIND_*`).
    pub ta_elem: FxHashMap<TmplId, u8>,
    pub resolvers: FxHashMap<NodeId, Tok>,
    pub thrown: NodeId,
    /// E14: interpreted code can run (and patch `Object.prototype`).
    pub dyn_eval: Option<String>,
    pub bound_index: FxHashMap<(CallId, Tok), u32>,
    /// Lookups that visited a holder token: replayed when an accessor is
    /// installed on it later.
    pub visits: FxHashMap<Tok, crate::pts::FxHashSet<(Tok, Key, ChainAct)>>,
    /// An accessor was installed on `Object.prototype` while solving: every
    /// lookup in the program may run it. Kills every fact (derive).
    pub objproto_accessor: bool,
    /// Solve-time budget; when exceeded the solve stops and the program
    /// gets no facts at all (every fact off: the sound fallback).
    pub budget: Option<(std::time::Instant, std::time::Duration)>,
    pub aborted: bool,
    pub pops: u64,
    /// Calls/`new` of a ⊤ callee: with the interpreter linked, one could be
    /// the Function constructor or `vm` (E14).
    pub top_calls: u64,
    /// The receiver token a builtin method summary runs for.
    pub cur_this: Option<Tok>,
    /// Lookups that stopped at a declared prototype member: resumed if that
    /// prototype later becomes HARD or escapes.
    pub stopped: FxHashMap<Tok, Vec<(Tok, Key, ChainAct)>>,
    /// Optional per-constraint-kind profile (PERRY_FACTS_PROFILE).
    pub profile: Option<FxHashMap<&'static str, (u64, std::time::Duration)>>,
    pub fn_inst: FxHashMap<FnIdx, TmplId>,
    /// Birth keys / definite keys of `new f()` instances, from the
    /// construction-phase analysis of `f`'s body.
    pub fn_birth: FxHashMap<FnIdx, (Vec<Atom>, Vec<Atom>)>,
    /// Diagnostics (`PERRY_FACTS_EXPLAIN=1`): where ⊤ first entered each
    /// node: (source node or `NO_NODE`, reason id). Never read by facts.
    pub explain: bool,
    pub top_pred: FxHashMap<NodeId, (NodeId, u32)>,
    pub cur_src: NodeId,
    pub cur_why: u32,
    pub top_vals: FxHashMap<String, NodeId>,
}

pub const NO_NODE: NodeId = u32::MAX;

impl Model {
    pub fn atom(&mut self, s: &str) -> Atom {
        if let Some(&a) = self.atom_index.get(s) {
            return a;
        }
        let a = self.atoms.len() as Atom;
        self.atoms.push(s.to_string());
        self.atom_index.insert(s.to_string(), a);
        a
    }

    /// Record one relation line, tagged with the current phase.
    #[inline]
    pub fn rel(&mut self, line: impl FnOnce() -> String) {
        if let Some(r) = self.relations.as_mut() {
            let mut l = String::with_capacity(48);
            l.push(self.rel_phase as char);
            l.push(' ');
            l.push_str(&line());
            r.push(l);
        }
    }

    pub fn reason(&mut self, s: &str) -> u32 {
        if let Some(&r) = self.reason_index.get(s) {
            return r;
        }
        let r = self.reasons.len() as u32;
        self.reasons.push(s.to_string());
        self.reason_index.insert(s.to_string(), r);
        r
    }

    pub fn tok(&mut self, k: TokKind) -> Tok {
        if let Some(&t) = self.tok_index.get(&k) {
            return t;
        }
        let t = self.toks.len() as Tok;
        self.toks.push(k.clone());
        self.tok_index.insert(k, t);
        t
    }

    pub fn node(&mut self, name: impl FnOnce() -> String) -> NodeId {
        self.node_named("other", name)
    }

    /// A node with a static label (diagnostics: the biggest points-to sets
    /// are reported by label).
    pub fn node_named(&mut self, label: &'static str, name: impl FnOnce() -> String) -> NodeId {
        let id = self.nodes.len() as NodeId;
        if self.max_nodes != 0 && self.nodes.len() > self.max_nodes {
            // Work bound: the program gets no facts (checked by the solver).
            self.aborted = true;
        }
        self.nodes.push(Node::default());
        self.labels.push(label);
        if self.relations.is_some() {
            self.node_names.push(name());
        }
        id
    }

    pub fn node_l(&mut self, label: &'static str) -> NodeId {
        self.node_named(label, || label.to_string())
    }

    /// A key as one whitespace-free relation token.
    pub fn key_name(&self, k: Key) -> String {
        match k {
            Key::Named(a) => esc_key(&self.atoms[a as usize]),
            Key::Any => "*".to_string(),
        }
    }

    pub fn tok_name(&self, t: Tok) -> String {
        match &self.toks[t as usize] {
            TokKind::Top => "TOP".into(),
            TokKind::Obj(tm) => format!("obj:{}", self.tmpls[*tm as usize].display),
            TokKind::Func(f) => format!("fn:{}", self.funcs[*f as usize].name),
            TokKind::Bound(b) => format!("bound:{b}"),
            TokKind::ClassObj(tm) => format!("class:{}", self.tmpls[*tm as usize].display),
            TokKind::Proto(tm) => format!("proto:{}", self.tmpls[*tm as usize].display),
            TokKind::ObjectProto => "Object.prototype".into(),
            TokKind::Builtin(b) => format!("builtin:{b:?}"),
            TokKind::BuiltinFn => "builtin-fn".into(),
            TokKind::PrimBox => "primbox".into(),
            TokKind::ModNs(m) => format!("ns:{m}"),
            TokKind::NativeProto(n) => format!("native:{n}"),
        }
    }

    pub fn anon_post_site(&mut self, desc: &str) -> SiteId {
        let id = self.stores.len() as SiteId;
        self.stores.push(StoreSite {
            module: u32::MAX,
            func: u32::MAX,
            desc: desc.to_string(),
            ctor_phase_of: None,
            birth: false,
            ctor_fn: None,
        });
        id
    }

    pub fn anon_birth_site(&mut self, desc: &str) -> SiteId {
        let id = self.stores.len() as SiteId;
        self.stores.push(StoreSite {
            module: u32::MAX,
            func: u32::MAX,
            desc: desc.to_string(),
            ctor_phase_of: None,
            birth: true,
            ctor_fn: None,
        });
        id
    }

    /// Metadata for the independent checker: every token, template,
    /// function, call site, store site and reason (tag `M`).
    pub fn dump_meta(&mut self) {
        if self.relations.is_none() {
            return;
        }
        let mut out: Vec<String> = Vec::new();
        for (i, a) in self.atoms.iter().enumerate() {
            out.push(format!("M atom {i} {}", esc_key(a)));
        }
        for (i, r) in self.reasons.iter().enumerate() {
            out.push(format!("M reason {i} {}", r.replace(['\n', '\r'], " ")));
        }
        for (i, k) in self.toks.iter().enumerate() {
            let d = match k {
                TokKind::Top => "top -".to_string(),
                TokKind::Obj(t) => format!("obj {t}"),
                TokKind::Func(f) => format!("func {f}"),
                TokKind::Bound(b) => format!("bound {b}"),
                TokKind::ClassObj(t) => format!("classobj {t}"),
                TokKind::Proto(t) => format!("proto {t}"),
                TokKind::ObjectProto => "objectproto -".into(),
                TokKind::Builtin(b) => format!("builtin {b:?}"),
                TokKind::BuiltinFn => "builtinfn -".into(),
                TokKind::PrimBox => "primbox -".into(),
                TokKind::ModNs(m) => format!("modns {m}"),
                TokKind::NativeProto(n) => format!("nativeproto {n}"),
            };
            out.push(format!("M tok {i} {d}"));
        }
        let join = |v: &[u32]| {
            if v.is_empty() {
                "-".to_string()
            } else {
                v.iter()
                    .map(|x| x.to_string())
                    .collect::<Vec<_>>()
                    .join(",")
            }
        };
        for (i, t) in self.tmpls.iter().enumerate() {
            let kind = match &t.kind {
                TmplKind::Class { class_id, .. } => format!("class {class_id}"),
                TmplKind::Lit { .. } => "lit -".into(),
                TmplKind::FnInst { func } => format!("fninst {func}"),
                TmplKind::ObjCreate { .. } => "objcreate -".into(),
                TmplKind::Arr { .. } => "arr -".into(),
                TmplKind::BObj { what, .. } => format!("bobj {what}"),
            };
            out.push(format!(
                "M tmpl {i} {kind} birth={} definite={} parent={} ctor={} native={} acc={} computed={} fieldinit={} pm={} display={}",
                join(&t.birth_keys),
                join(&t.definite),
                t.parent.map(|p| p.to_string()).unwrap_or_else(|| "-".into()),
                t.ctor.map(|p| p.to_string()).unwrap_or_else(|| "-".into()),
                t.native_parent.as_ref().map(|(_, n)| n.clone()).unwrap_or_else(|| "-".into()),
                join(&t.accessor_keys),
                t.has_computed_members as u8,
                t.field_init.map(|p| p.to_string()).unwrap_or_else(|| "-".into()),
                join(&t.proto_members),
                t.display.replace(' ', "\\s"),
            ));
        }
        for (i, f) in self.funcs.iter().enumerate() {
            out.push(format!(
                "M func {i} tok={} this={} ret={} args={} rest={} lexical={} ctor_of={} params={}",
                f.token,
                f.this,
                f.ret,
                f.args_all,
                f.rest.map(|r| r.to_string()).unwrap_or_else(|| "-".into()),
                f.lexical_this as u8,
                f.ctor_of
                    .map(|r| r.to_string())
                    .unwrap_or_else(|| "-".into()),
                join(&f.params),
            ));
        }
        for (f, (b, d)) in &self.fn_birth {
            out.push(format!(
                "M fnbirth {f} birth={} definite={}",
                join(b),
                join(d)
            ));
        }
        for (i, c) in self.calls.iter().enumerate() {
            let this = match c.this {
                ThisArg::Undef => "u".to_string(),
                ThisArg::Node(n) => format!("n{n}"),
                ThisArg::Tok(t) => format!("t{t}"),
            };
            out.push(format!(
                "M call {i} this={this} ret={} args={} spread={}",
                c.ret,
                join(&c.args),
                join(&c.spread)
            ));
        }
        for (i, s) in self.stores.iter().enumerate() {
            out.push(format!(
                "M store {i} birth={} ctor_phase_of={} ctor_fn={}",
                s.birth as u8,
                s.ctor_phase_of
                    .map(|r| r.to_string())
                    .unwrap_or_else(|| "-".into()),
                s.ctor_fn
                    .map(|r| r.to_string())
                    .unwrap_or_else(|| "-".into()),
            ));
        }
        for (i, (t, th, a)) in self.bound.iter().enumerate() {
            out.push(format!("M bound {i} target={t} this={th} args={}", join(a)));
        }
        out.push(format!(
            "M special thrown={} toptok={}",
            self.thrown, self.top_tok_node
        ));
        self.relations.as_mut().unwrap().extend(out);
    }

    /// The accessors installed by generation (class getters/setters …),
    /// logged before solving as checker inputs.
    pub fn log_seed_accessors(&mut self) {
        if self.relations.is_none() {
            return;
        }
        let mut v: Vec<String> = Vec::new();
        for ((h, k), (g, s)) in &self.accessors {
            for f in g {
                v.push(format!("G accseed {h} {k} {f} 0"));
            }
            for f in s {
                v.push(format!("G accseed {h} {k} {f} 1"));
            }
        }
        v.sort();
        self.relations.as_mut().unwrap().extend(v);
    }

    /// Diagnostics: follow the first-arrival chain of ⊤ back to its origin.
    pub fn top_origin(&self, start: NodeId) -> String {
        let mut n = start;
        let mut seen: crate::pts::FxHashSet<NodeId> = Default::default();
        let mut hops = 0usize;
        loop {
            if !seen.insert(n) {
                return format!("(cycle after {hops} hops at {})", self.labels[n as usize]);
            }
            let Some(&(src, why)) = self.top_pred.get(&n) else {
                return format!("(no record at {})", self.labels[n as usize]);
            };
            if why != u32::MAX {
                return self.reasons[why as usize].replace(['\n', '\r'], " ");
            }
            if src == NO_NODE {
                return format!("(seed at {})", self.labels[n as usize]);
            }
            if !self.top_pred.contains_key(&src) {
                return format!("(rule on {} without TOP)", self.labels[src as usize]);
            }
            n = src;
            hops += 1;
        }
    }

    pub fn tmpl_of(&self, t: Tok) -> Option<TmplId> {
        match self.toks[t as usize] {
            TokKind::Obj(tm) => Some(tm),
            _ => None,
        }
    }
}

/// A property key as one whitespace-free token for the dump and the
/// relations (`scripts/wpfacts_recompute.py` `unesc` is the inverse):
/// `\\` `\*` `\s` `\n` `\r` `\t`, other whitespace or control characters as
/// `\u{hex}`, and the empty key as `\e`.
pub fn esc_key(s: &str) -> String {
    if s.is_empty() {
        return "\\e".to_string();
    }
    let mut o = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '\\' => o.push_str("\\\\"),
            // `*` alone is the any-key token; a key holding `*` is escaped
            // so a property literally named `*` stays a named key.
            '*' => o.push_str("\\*"),
            ' ' => o.push_str("\\s"),
            '\n' => o.push_str("\\n"),
            '\r' => o.push_str("\\r"),
            '\t' => o.push_str("\\t"),
            c if c.is_whitespace() || c.is_control() => {
                o.push_str(&format!("\\u{{{:x}}}", c as u32))
            }
            c => o.push(c),
        }
    }
    o
}
