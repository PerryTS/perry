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

/// The atom of the element key (`Model::elem_key`).
pub const ELEM: &str = "#elem";
/// The atom of the iteration key (`Model::iter_key`).
pub const ITER_ELEM: &str = "#iter";

/// Token 0 is ⊤: a value produced or touched by code the analysis does not
/// model. Its presence in a node means "any escaped object or anything else".
pub const TOP: Tok = 0;

/// The context of a template's own token (`TokKind::Obj`).
pub const NO_CTX: CallId = u32::MAX;

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
    /// `Map` / `WeakMap`: `new` builds a collection of the entries' contents.
    Map,
    /// `Set` / `WeakSet`: `new` builds a collection of the values.
    Set,
    /// The program's `require` from `module.createRequire` (one per
    /// program: every CommonJS wrapper shares it). A call resolves its
    /// constant specifier ([`Model::call_lit`]): a compiled module's path
    /// gives that module's exports; anything else is the unknown loader.
    Require,
    Other,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum TokKind {
    Top,
    /// An instance of a birth template, born at a context: `NO_CTX` for the
    /// template's own token (generation-time births, template-wide marks),
    /// else the `new` call that made it (one token per birth site; a call
    /// an instance made is that instance's own call).
    Obj(TmplId, CallId),
    /// A function value (closure, function declaration, method body), with
    /// its closure context: `NO_CTX`, or the context a closure created
    /// inside a summary instance carries (`Model::clo_ctx`: the instance's
    /// nodes for the closure's outer terms).
    Func(FnIdx, CallId),
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
    /// The body has an `arguments` object, the only reader of `args_all`:
    /// calls feed `args_all` only then (nobody else reads it).
    pub has_arguments: bool,
    pub token: Tok,
    /// This function is the constructor of that class template.
    pub ctor_of: Option<TmplId>,
    /// Arrow function / `captures_this` closure: `this` is the enclosing one.
    pub lexical_this: bool,
    /// Something outside the call protocol writes the parameters (an object
    /// literal IIFE, a computed accessor): never summarized.
    pub no_summary: bool,
    /// The step closure of a transformed generator: the generator object
    /// (`LinkGeneratorPrototype`); `next`/`return`/`throw` and the
    /// iteration protocol on it run the closure.
    pub gen_step: bool,
    /// The function whose body creates this closure (`NO_FN` for
    /// declarations, methods and builtins).
    pub parent: FnIdx,
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

/// A lookup that passed a native prototype not defining its key.
#[derive(Clone, Copy, Debug)]
pub enum NativeMiss {
    /// A value read into this node.
    Load(NodeId),
    /// A method call with this receiver.
    Call(Tok, CallId),
    /// A program-owned global (`builtins::global_user_key`) read into the
    /// first node; the second is the global's field.
    Global(NodeId, NodeId),
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
#[derive(Clone, Copy, Debug)]
pub enum Cons {
    /// dst ⊇ read(t, key) for every t in the node.
    Load { key: Key, dst: NodeId },
    /// Values of own properties only (enumerability over-approximated).
    OwnValues { dst: NodeId },
    /// `registerPathModule`: dst ⊇ a CommonJS record's live `exports` (the
    /// wrapper's record literal carries `__perry_cjs_record` at birth), or
    /// the published value itself for a raw publication.
    CjsPublish { dst: NodeId },
    /// write(t, key) ⊇ src for every t in the node.
    Store { key: Key, src: NodeId, site: SiteId },
    /// The node is a callee.
    Call { call: CallId },
    /// The node is a callee whose `this` is fixed to one receiver token
    /// (a method found on that receiver or its chain).
    CallThis { call: CallId, this: Tok },
    /// The node is a `new` callee.
    New { call: CallId },
    /// `new Name(...)` of a builtin constructor named in the source: runs
    /// once per call (the holder holds one marker token), so an instance
    /// that renames the call constructs its own object.
    NewNamed { name: Atom, call: CallId },
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
    /// Once anything (a token or a primitive) reaches the node, `dst` holds
    /// ⊤ and everything `from` holds: the result of a call that exists only
    /// when an argument reaches it.
    /// (`reason` `None`: only `from`, no ⊤.)
    TopIfAny { dst: NodeId, from: NodeId, reason: Option<u32> },
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
    /// A live constructor-prototype candidate runs on the same derived receiver.
    SuperCtorParent {
        class: TmplId,
        call: CallId,
        origin: Tok,
    },
    /// The node holds method closures installed on `recv` (literal methods):
    /// their `this` is `recv`.
    ThisFrom { recv: NodeId },
    /// A literal's own content: every token here gets `src` in field `key`
    /// (no store: a birth, as generation's direct field edges are). A
    /// summary's form of a literal born in its body.
    BirthField { key: Key, src: NodeId },
    /// The node holds getter/setter functions installed on the objects of
    /// `holder` under `key`.
    Accessor {
        holder: NodeId,
        key: Atom,
        setter: bool,
    },
    /// Internal: install accessor `f` on every token of the node.
    /// `f`: the accessor's function token (its closure context included).
    AccessorInstall { key: Atom, f: Tok, setter: bool },
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
    /// The node holds the methods a lookup found on receiver `origin`:
    /// each runs with `origin` as `this` (the `this` half of `CallThis`,
    /// shared by every call site that looks the same key up on `origin`).
    ThisTo { origin: Tok },
    /// The node is call `call`'s callee relay (the union of the method
    /// nodes its receivers found): arguments and result bind once per
    /// (call, function) (the argument half of `CallThis`).
    CallArgs { call: CallId },
    /// The node is the source of an object spread / `Object.assign` into
    /// the values of `into`: each source object copies its own keys, key
    /// by key, into each target (`Model::spread_into`), never through one
    /// any-key bucket.
    Spread { into: NodeId, site: SiteId },
    /// The node holds the targets of a spread from source token `src`.
    SpreadFrom { src: Tok, site: SiteId },
    /// The node is a receiver of summarized function token `ft` at `call`
    /// (or of no call: `NO_NODE`): each value binds the instance of its
    /// layout (`Model::bind_layout`).
    ThisInst { call: CallId, ft: Tok },
    /// The node is a method node some call sites bound through: a builtin
    /// namespace value in it runs per (call, receiver) (`callee_sites`).
    BuiltinCallee,
    /// The node holds prototypes on a named-key accessor lookup's chain:
    /// `pass` ⊇ the holders the lookup passes through (and continues
    /// above), `stop` ⊇ the prototypes where a declared member ends it.
    /// Built once per (start, key), shared by every receiver below it.
    HolderStep {
        key: Key,
        pass: NodeId,
        stop: NodeId,
    },
    /// The node is a lookup's pass-holder set: accessors there run for
    /// `origin` (and later ones replay through `visits`); ⊤ there is an
    /// unknown prototype.
    VisitHolders {
        origin: Tok,
        key: Key,
        act: ChainAct,
    },
    /// The node is a lookup's stop-holder set: accessors there run.
    VisitStop {
        origin: Tok,
        key: Key,
        act: ChainAct,
    },
}

impl Cons {
    pub fn kind_name(&self) -> &'static str {
        match self {
            Cons::Load { .. } => "load",
            Cons::OwnValues { .. } => "ownvalues",
            Cons::CjsPublish { .. } => "cjspublish",
            Cons::Store { .. } => "store",
            Cons::Call { .. } => "call",
            Cons::CallThis { .. } => "callthis",
            Cons::New { .. } => "new",
            Cons::NewNamed { .. } => "newnamed",
            Cons::MCall { .. } => "mcall",
            Cons::Chain { .. } => "chain",
            Cons::TopSink { .. } => "top",
            Cons::TopIfAny { .. } => "topifany",
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
            Cons::SuperCtorParent { .. } => "superctorparent",
            Cons::ThisFrom { .. } => "thisfrom",
            Cons::BirthField { .. } => "birthfield",
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
            Cons::ThisTo { .. } => "thisto",
            Cons::CallArgs { .. } => "callargs",
            Cons::Spread { .. } => "spread",
            Cons::SpreadFrom { .. } => "spreadfrom",
            Cons::ThisInst { .. } => "thisinst",
            Cons::BuiltinCallee => "builtincallee",
            Cons::HolderStep { .. } => "holderstep",
            Cons::VisitHolders { .. } => "visitholders",
            Cons::VisitStop { .. } => "visitstop",
        }
    }
}

#[derive(Default, Clone, Debug)]
pub struct Node {
    pub pts: Pts,
    pub prim: u16,
    pub delta: crate::delta::Delta,
    pub dprim: u16,
    pub succ: Vec<NodeId>,
    pub cons: Vec<Cons>,
    pub in_wl: bool,
    /// Successor set, once the list is long (exact edge dedup for hubs: a
    /// function's return node can be offered the same edge millions of
    /// times).
    pub succ_set: Option<std::rc::Rc<crate::pts::FxHashSet<NodeId>>>,
    /// An observation node: a parameter-derived node of a summarized
    /// function's shared body. It holds the union over every call for the
    /// read sites; only read rules apply to it, it never feeds a node that
    /// is not an observation node, and its effects are skipped.
    pub obs: bool,
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
    /// Template -> its birth-site tokens (`Obj(tm, ctx)`, ctx != NO_CTX).
    pub site_toks: FxHashMap<TmplId, Vec<Tok>>,
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
    pub accessors: FxHashMap<(Tok, Atom), (Vec<Tok>, Vec<Tok>)>,
    pub accessor_keys: FxHashMap<Tok, Vec<Atom>>,
    pub writers: FxHashMap<(TmplId, Atom), FieldWriters>,
    pub any_writers: FxHashMap<TmplId, FieldWriters>,
    pub adds_keys: FxHashMap<TmplId, u32>,
    pub hard: FxHashMap<Tok, (Hard, u32)>,
    pub escaped: FxHashMap<Tok, u32>,
    /// `escaped` as a bitset by token (the hot membership test).
    pub esc_bits: Vec<u64>,
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
    /// `CallThis` split (see `Model::bind_via`): `ThisTo` per (method node,
    /// receiver), the callee relay per call, the edges into it, and the
    /// (call, receiver) pairs bound through each method node.
    pub this_done: crate::pts::FxHashSet<(NodeId, Tok)>,
    pub relays: FxHashMap<CallId, NodeId>,
    pub relay_done: crate::pts::FxHashSet<(CallId, NodeId)>,
    pub callee_sites: FxHashMap<NodeId, Vec<(CallId, Tok)>>,
    /// The calls bound through each (method node, receiver): a summarized
    /// target's instance for the call takes exactly the receivers whose
    /// lookup found it.
    pub via_calls: FxHashMap<(NodeId, Tok), Vec<CallId>>,
    pub builtin_callee: crate::pts::FxHashSet<NodeId>,
    /// Shared accessor walks: (receiver, start, key, load/store/call) → the
    /// node the walk's accessors feed (load/call) or read from (store).
    pub walk_nodes: FxHashMap<(Tok, Tok, Key, u8), NodeId>,
    /// Store walks that have found no setter (and no unknown prototype) yet:
    /// the sites' source nodes wait here instead of flowing into the walk
    /// node, which nothing reads until then.
    pub walk_store_pending: FxHashMap<NodeId, Vec<NodeId>>,
    /// Named-key lookup chains: (start, key) → (pass holders, stop holders).
    pub holder_nodes: FxHashMap<(Tok, Key), (NodeId, NodeId)>,
    /// Holder chains that stopped at a declared member of this prototype:
    /// resumed when it becomes HARD or escapes.
    pub stopped_holders: FxHashMap<Tok, Vec<(Key, NodeId, NodeId)>>,
    /// Accessor groups: (holder, key or Any) → the shared nodes every
    /// lookup that reaches the holder's accessors uses (`AccGroup`).
    pub acc_groups: FxHashMap<(Tok, Key), AccGroup>,
    /// Reused token buffers (no allocation per rule application).
    pub bufs: Vec<Vec<Tok>>,
    pub sunk_nodes: crate::pts::FxHashSet<NodeId>,
    pub any_walk_done: crate::pts::FxHashSet<(NodeId, Tok, ChainAct)>,
    pub any_walks: Vec<(NodeId, Tok, Tok, ChainAct)>,
    /// Computed-key walks by holder: every (receiver, act) whose walk
    /// reached that holder (the receiver itself or a prototype in its
    /// closure), so installing an accessor replays only those.
    pub any_walk_holders: FxHashMap<Tok, Vec<(Tok, ChainAct)>>,
    /// Source token of a spread -> its targets (and the spread site): a key
    /// the source gains later is copied too (`Model::field`).
    pub spread_to: FxHashMap<Tok, Vec<(Tok, SiteId)>>,
    pub spread_done: crate::pts::FxHashSet<(Tok, Tok)>,
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
    /// Generation-time birth templates (literals, arrays, builtin objects)
    /// -> the function whose body makes them.
    pub tmpl_fn: FxHashMap<TmplId, FnIdx>,
    /// Template tokens of literals a summary births per instance: they live
    /// on observation nodes only, so their own nodes observe too.
    pub obs_toks: crate::pts::FxHashSet<Tok>,
    /// Typed-array owner templates born by `TypedArrayNew` with a known
    /// element kind (`perry_hir::TYPED_ARRAY_KIND_*`).
    pub ta_elem: FxHashMap<TmplId, u8>,
    pub resolvers: FxHashMap<NodeId, Tok>,
    pub thrown: NodeId,
    /// E14: interpreted code can run (and patch `Object.prototype`).
    pub dyn_eval: Option<String>,
    pub bound_index: FxHashMap<(CallId, Tok), u32>,
    /// Calls the solver makes through a bound value's target (`call_bound`,
    /// `construct`): a bound target reached by one of them goes through
    /// that bound value's one merged call (`Model::bound_merged`).
    pub bound_synth: crate::pts::FxHashSet<CallId>,
    /// (bound value, `new`) → its merged call.
    pub bound_merged: FxHashMap<(u32, bool), CallId>,
    /// Lookups that visited a holder token: replayed when an accessor is
    /// installed on it later.
    pub visits: FxHashMap<Tok, crate::pts::FxHashSet<(Tok, Key, ChainAct)>>,
    /// An accessor was installed on `Object.prototype` while solving: every
    /// lookup in the program may run it. Kills every fact (derive).
    pub objproto_accessor: bool,
    /// Why `%Function.prototype%.bind` (or the `name`/`length` it reads off
    /// its target) may not be the builtin's own data property: the first
    /// store, accessor, definition or deletion under one of those keys (or a
    /// computed key) whose target may be a builtin prototype (collapsed into
    /// `ObjectProto`) or a value from unmodeled code, which may hold any of
    /// them. `None` after the solve: compiled code never writes them
    /// ([`Model::note_fnproto_write`]).
    pub fnproto_write: Option<u32>,
    /// Native parent prototypes (`EventEmitter.prototype`, `Error.prototype`
    /// …) are runtime-owned, like `Object.prototype`: sealed static layouts
    /// whose keys the analysis knows ([`crate::builtins::native_proto_keys`]),
    /// so a lookup of a key one does not define continues up the chain
    /// instead of running an opaque member. Why one may be written anyway,
    /// per (target, key): the target is the prototype's own token,
    /// `ObjectProto` (a global native's prototype collapses into it) or ⊤ (a
    /// native module's values, interpreted code); key `None`: every key.
    pub native_writes: FxHashMap<(Tok, Option<Atom>), u32>,
    /// Lookups of a key their native prototype does not define: they run
    /// today's opaque member once that key may be written there.
    pub native_missing: Vec<(Tok, Atom, NativeMiss)>,
    /// A builtin function value has been written (`store_into`): the reason.
    /// Property reads of such values are ⊤ from then on; `bf_reads` are the
    /// reads made before.
    pub bf_written: Option<u32>,
    pub bf_reads: Vec<NodeId>,
    /// A call's constant first argument (a string), for callees whose
    /// result depends on it (`BuiltinNs::Require`). Compiler data.
    pub call_lit: FxHashMap<CallId, Atom>,
    /// The CommonJS registry: every published record (`CJSREG`), the
    /// records under an unknown path (`CJSUNKNOWNPATH`), and the records
    /// published under each constant path (`Gen::cjs_path_node`).
    pub cjs_union: NodeId,
    pub cjs_unknown: NodeId,
    pub cjs_paths: FxHashMap<Atom, NodeId>,
    /// `require.cache` / `require.extensions` of [`BuiltinNs::Require`].
    pub require_parts: Option<(Tok, Tok)>,
    /// Fields of the global object under program-owned keys.
    pub global_fields: Vec<(Atom, NodeId)>,
    /// A ⊤ value may be the global object: it reached unmodeled code, or
    /// dynamic code may hand it out (`Function('return this')()`). Until
    /// then a store into ⊤ cannot write a global.
    pub global_in_top: bool,
    /// Targets of a possible [[Prototype]] write (TOP included).
    pub super_relink: crate::pts::FxHashSet<Tok>,
    /// Source constructor of a type-specialized instance layout (compiler data).
    pub constructor_origins: FxHashMap<TmplId, TmplId>,
    pub constructor_this_done: crate::pts::FxHashSet<(CallId, TmplId, Tok)>,
    /// Wall-clock budget on generation and solve; when exceeded the pass
    /// stops and the program gets no facts at all (every fact off: the
    /// sound fallback).
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
    /// With the profile: propagation work by source-node label
    /// (delta × successors, delta × constraints).
    pub label_work: FxHashMap<&'static str, (u64, u64, u64)>,
    /// TEMP CW s9: per-node propagation work (profile only).
    pub cw9_node_work: FxHashMap<u32, u64>,
    pub cw9_inst_fn: FxHashMap<u32, (u32, u32)>,
    /// TEMP CWL: (fn, this shape, ret label, protocol, bound-synth, program call) -> (n, first receiver, site).
    pub cwl_recv: FxHashMap<(FnIdx, String, &'static str, bool, bool, bool), (u32, String, u32)>,
    pub cwl_seen: crate::pts::FxHashSet<CallId>,
    pub cwl_why: &'static str,
    /// TEMP CWL: first arrival of a token in a node: (source node, constraint kind).
    pub cwl_prov: FxHashMap<(NodeId, Tok), (NodeId, &'static str)>,
    pub cwl_trace: Option<NodeId>,
    pub cwl_slot: FxHashMap<NodeId, NodeId>,
    pub cwl_firstfn: crate::pts::FxHashSet<FnIdx>,
    pub cwl_tok: Option<Tok>,
    pub cwl_inst_probe: bool,
    pub cwl_tokhit: NodeId,
    pub cwl_tokfns: crate::pts::FxHashSet<FnIdx>,
    pub cwl_thisinst: FxHashMap<(&'static str, FnIdx), u32>,
    pub cw9_events: Vec<(u64, u32, u32, u32)>,
    pub cw9_shared: FxHashMap<(u32, u32, u8, u32), u32>,
    pub cw9_modkind: Vec<u8>,
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
    /// The function whose body generation is walking (`NO_FN` outside).
    pub cur_fn: FnIdx,
    /// Owner of each node: the function whose body made it (`Gen::tmp`,
    /// `Gen::tok_node`, `Gen::local`), `NO_FN` for every other node.
    pub node_fn: Vec<FnIdx>,
    /// (node, function): a function references a node another function
    /// owns (a local, a parameter, or the enclosing `this` of an arrow).
    pub captures: crate::pts::FxHashSet<(NodeId, FnIdx)>,
    pub sum: crate::summary::SumState,
}

/// The accessors of one holder under one key (or every key, `Key::Any`),
/// as shared nodes: a lookup from receiver `o` adds `o` to `gt` (getters'
/// `this`) and reads `ar` (getter results), or adds `o` to `st` (setters'
/// `this`) and feeds `sp` (setters' parameter). The accessors' own nodes
/// hang off these, so a new accessor connects once instead of replaying
/// every lookup, and a lookup costs O(1) instead of O(accessors).
#[derive(Clone, Debug)]
pub struct AccGroup {
    pub gt: NodeId,
    pub ar: NodeId,
    pub st: NodeId,
    pub sp: NodeId,
    pub has_setter: bool,
    /// Store sources waiting for a first setter.
    pub pending: Vec<NodeId>,
}

pub const NO_NODE: NodeId = u32::MAX;
pub const NO_FN: FnIdx = u32::MAX;

/// Marks a `super` walk's start in `Model::walk_nodes` keys (it begins with
/// a stop check at the start prototype, unlike a receiver's own walk).
pub const SUPER_BIT: Tok = 1 << 31;

impl Model {
    /// Record a write under `key` onto `t` that may reach
    /// `%Function.prototype%`'s `bind`, `name` or `length`
    /// ([`Model::fnproto_write`]). `key` `None`: an operation whose key the
    /// model does not keep (a definition, a deletion).
    pub fn note_fnproto_write(&mut self, t: Tok, key: Option<Key>, what: &str) {
        if self.fnproto_write.is_some()
            || !(t == TOP || matches!(self.toks[t as usize], TokKind::ObjectProto))
        {
            return;
        }
        let hits = match key {
            None | Some(Key::Any) => true,
            Some(Key::Named(a)) => matches!(
                self.atoms[a as usize].as_str(),
                "bind" | "name" | "length" | crate::STAR_KEY
            ),
        };
        if hits {
            let on = if t == TOP {
                "a value from unmodeled code"
            } else {
                "a builtin prototype"
            };
            self.fnproto_write = Some(self.reason(&format!("{what} on {on}")));
        }
    }

    /// The element key: an element read of an array-like (iteration, array
    /// callbacks) sees its elements, not the builtin methods a computed key
    /// could also name; on anything else it is a computed read (`Key::Any`).
    pub fn elem_key(&mut self) -> Key {
        Key::Named(self.atom(ELEM))
    }

    pub fn is_elem_key(&self, k: Key) -> bool {
        matches!(k, Key::Named(a) if self.atoms[a as usize] == ELEM)
    }

    /// The iteration key: what iterating a value yields WITHOUT its
    /// protocol (`Gen::iterate` models `[Symbol.iterator]().next().value`
    /// beside it): an array-like's elements; a program object or function
    /// yields nothing that way; a runtime object is a computed read.
    pub fn iter_key(&mut self) -> Key {
        Key::Named(self.atom(ITER_ELEM))
    }

    pub fn is_iter_key(&self, k: Key) -> bool {
        matches!(k, Key::Named(a) if self.atoms[a as usize] == ITER_ELEM)
    }

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

    pub fn constructor_origin(&self, mut class: TmplId) -> TmplId {
        for _ in 0..self.constructor_origins.len() {
            let Some(&origin) = self.constructor_origins.get(&class) else {
                break;
            };
            class = origin;
        }
        class
    }

    pub fn tok(&mut self, k: TokKind) -> Tok {
        if let Some(&t) = self.tok_index.get(&k) {
            return t;
        }
        let t = self.toks.len() as Tok;
        if let TokKind::Obj(tm, ctx) = k {
            if ctx != NO_CTX {
                self.site_toks.entry(tm).or_default().push(t);
            }
        }
        self.toks.push(k.clone());
        self.tok_index.insert(k, t);
        t
    }

    /// The template token of `tm` and every birth-site token of it.
    pub fn tmpl_toks(&self, tm: TmplId) -> Vec<Tok> {
        let mut v: Vec<Tok> = self.tok_index.get(&TokKind::Obj(tm, NO_CTX)).copied().into_iter().collect();
        if let Some(s) = self.site_toks.get(&tm) {
            v.extend_from_slice(s);
        }
        v
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
        self.node_fn.push(NO_FN);
        self.labels.push(label);
        if self.relations.is_some() {
            self.node_names.push(name());
        }
        id
    }

    /// `n` belongs to the body generation is walking.
    pub fn own(&mut self, n: NodeId) {
        self.node_fn[n as usize] = self.cur_fn;
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

    /// The function of a function token (`NO_FN` for any other token).
    pub fn tok_fn(&self, t: Tok) -> FnIdx {
        match self.toks[t as usize] {
            TokKind::Func(f, _) => f,
            _ => NO_FN,
        }
    }

    pub fn tok_name(&self, t: Tok) -> String {
        match &self.toks[t as usize] {
            TokKind::Top => "TOP".into(),
            TokKind::Obj(tm, _) => format!("obj:{}", self.tmpls[*tm as usize].display),
            TokKind::Func(f, _) => format!("fn:{}", self.funcs[*f as usize].name),
            TokKind::Bound(b) => format!("bound:{b}"),
            TokKind::ClassObj(tm) => format!("class:{}", self.tmpls[*tm as usize].display),
            TokKind::Proto(tm) => format!("proto:{}", self.tmpls[*tm as usize].display),
            TokKind::ObjectProto => "Object.prototype".into(),
            TokKind::Builtin(b) => format!("builtin:{b:?}"),
            TokKind::BuiltinFn => "builtin-fn".into(),
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
                TokKind::Obj(t, NO_CTX) => format!("obj {t}"),
                TokKind::Obj(t, c) => format!("obj {t}@{c}"),
                TokKind::Func(f, NO_CTX) => format!("func {f}"),
                TokKind::Func(f, c) => format!("func {f}@{c}"),
                TokKind::Bound(b) => format!("bound {b}"),
                TokKind::ClassObj(t) => format!("classobj {t}"),
                TokKind::Proto(t) => format!("proto {t}"),
                TokKind::ObjectProto => "objectproto -".into(),
                TokKind::Builtin(b) => format!("builtin {b:?}"),
                TokKind::BuiltinFn => "builtinfn -".into(),
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
                "M func {i} tok={} this={} ret={} args={} arguments={} rest={} lexical={} ctor_of={} params={}",
                f.token,
                f.this,
                f.ret,
                f.args_all,
                f.has_arguments as u8,
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
            for &t in g {
                v.push(format!("G accseed {h} {k} {} 0", self.tok_fn(t)));
            }
            for &t in s {
                v.push(format!("G accseed {h} {k} {} 1", self.tok_fn(t)));
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

    #[inline]
    pub fn is_escaped(&self, t: Tok) -> bool {
        self.esc_bits
            .get((t >> 6) as usize)
            .is_some_and(|w| w & (1 << (t & 63)) != 0)
    }

    pub fn tmpl_of(&self, t: Tok) -> Option<TmplId> {
        match self.toks[t as usize] {
            TokKind::Obj(tm, _) => Some(tm),
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
