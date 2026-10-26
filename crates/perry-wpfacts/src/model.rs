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
    /// An object literal site that stays an `Expr::Object` / `ObjectSpread`,
    /// or a TS enum's object (`enum_image`: its keys are the members, named
    /// by the enum, §18.2).
    Lit {
        module: u32,
        site: SiteId,
        enum_image: bool,
    },
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
    /// Birth keys the class declares (`ClassFieldOrigin::Declared`): known
    /// slots, absent until stored (each read pairs its load with the hole
    /// compare).
    pub declared: Vec<Atom>,
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
    /// A parent that is not a class of the unit at gen time (§17.2): the
    /// value of the `extends` expression and the load of its `prototype`.
    /// The class object's and the prototype's proto nodes take their tokens
    /// from them; after the solve one class token there becomes `parent`.
    pub parent_value: Option<(NodeId, NodeId)>,
    /// Classes whose constructor chain `super()` ran on this class's
    /// instances during the solve (the parent value's class tokens).
    pub parent_links: Vec<TmplId>,
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
    /// A class constructor of the shape `constructor(...a) { super(...a) }`
    /// (the spec default a derived class gets): the `new` call's own
    /// arguments construct the parent, position by position, not through
    /// one rest array whose elements reach every parameter of the parent
    /// (DECISIONS 49). Its body is not walked: `run_ctor_chain` runs the
    /// parent with the `new` call itself.
    pub forwards_super: bool,
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

/// What a published class's method did to its receiver (`Model::recv_log`).
#[derive(Clone, Copy, Debug)]
pub enum RecvEffect {
    Store(Key, NodeId, SiteId),
    Hard(Hard, u32),
    Escape(u32),
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

/// What a builtin protocol touch reads on the touched value: the keys it
/// looks up (getters included) and calls with the value as `this`, and the
/// values it reads. The site that issues the touch knows its protocol (the
/// operator, the syntax or the runtime helper); ECMA-262 fixes each key set.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Protocol {
    /// ToPrimitive (ECMA-262 7.1.1), and through it ToString, ToNumber,
    /// ToNumeric and ToPropertyKey: `@@toPrimitive`, else
    /// OrdinaryToPrimitive's `valueOf` and `toString`. What they return is a
    /// primitive or is discarded: nothing runs on it.
    ToPrimitive,
    /// GetIterator (7.4.3): `@@iterator`; it returns an iterator.
    Iterate,
    /// IteratorNext / IteratorClose: `next`, `return`, `throw`; they return
    /// iterator results.
    IterNext,
    /// IteratorComplete / IteratorValue: reads `done` and `value`, calls
    /// nothing.
    IterResult,
    /// SerializeJSONProperty (25.5.2.2): `toJSON`; what it returns and every
    /// value read off the object are serialized in turn.
    Json,
    /// InstanceofOperator (13.10.2): `@@hasInstance`. OrdinaryHasInstance
    /// reads `prototype`, which no program can make an accessor of a
    /// function (it is non-configurable).
    HasInstance,
    /// [[OwnPropertyKeys]] and for-in enumeration: no lookup on an ordinary
    /// object (a Proxy is ⊤).
    OwnKeys,
    /// A runtime helper that reflects over the value or mixes protocols:
    /// any key, and a function may be called back with anything.
    Any,
}

impl Protocol {
    /// The keys looked up and called; `None`: any key.
    pub fn keys(self) -> Option<&'static [&'static str]> {
        Some(match self {
            Protocol::ToPrimitive => &[WK_TO_PRIMITIVE, "valueOf", "toString"],
            Protocol::Iterate => &[crate::builtins::WK_ITERATOR],
            Protocol::IterNext => &["next", "return", "throw"],
            Protocol::Json => &["toJSON"],
            Protocol::HasInstance => &[WK_HAS_INSTANCE],
            Protocol::IterResult | Protocol::OwnKeys => &[],
            Protocol::Any => return None,
        })
    }

    /// The keys whose values are read (and touched again for JSON).
    pub fn reads(self) -> &'static [&'static str] {
        match self {
            Protocol::IterResult => &["done", "value"],
            _ => &[],
        }
    }

    /// The protocol run on what the called methods return.
    pub fn of_result(self) -> Option<Protocol> {
        match self {
            Protocol::Iterate => Some(Protocol::IterNext),
            Protocol::IterNext => Some(Protocol::IterResult),
            Protocol::Json => Some(Protocol::Json),
            Protocol::Any => Some(Protocol::Any),
            Protocol::ToPrimitive
            | Protocol::IterResult
            | Protocol::HasInstance
            | Protocol::OwnKeys => None,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Protocol::ToPrimitive => "toprimitive",
            Protocol::Iterate => "iterate",
            Protocol::IterNext => "iternext",
            Protocol::IterResult => "iterresult",
            Protocol::Json => "json",
            Protocol::HasInstance => "hasinstance",
            Protocol::OwnKeys => "ownkeys",
            Protocol::Any => "any",
        }
    }
}

/// `Symbol.toPrimitive` / `Symbol.hasInstance` as HIR folds them
/// (`Gen::read_key`).
pub const WK_TO_PRIMITIVE: &str = "@@__perry_wk_toPrimitive";
pub const WK_HAS_INSTANCE: &str = "@@__perry_wk_hasInstance";

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
    /// A parameter of a closed class's stand-in constructor (design §18.13,
    /// DECISIONS 109): an instance of another unit's contract (a stand-in)
    /// goes to the defining unit's constructor through the argument
    /// contract, which describes it there as the same stand-in, so that
    /// unit's census covers what the constructor does with it; any other
    /// token escapes, with the reason.
    HandOver { reason: u32 },
    /// The `this` of a contract stand-in of another unit's class `tmpl`
    /// (design §11.2): its instances are inside the contract; any other
    /// receiver escapes (that unit runs the method on it unseen).
    /// `this` of another unit's method: only that class's instances (and,
    /// for a static method, its class object); any other receiver escapes.
    ContractThis {
        tmpl: TmplId,
        reason: u32,
        statics: bool,
    },
    /// Once anything (a token or a primitive) reaches the node, `dst` holds
    /// ⊤ and everything `from` holds: the result of a call that exists only
    /// when an argument reaches it.
    /// (`reason` `None`: only `from`, no ⊤.)
    TopIfAny {
        dst: NodeId,
        from: NodeId,
        reason: Option<u32>,
    },
    /// Every object token reaches a HARD operation.
    Hard { kind: Hard, reason: u32 },
    /// `Object.defineProperty(o, key, {get/set})` with a constant key and an
    /// accessor descriptor (FVA 4.3a): on a token whose layout does not hold
    /// `key` as data the key becomes an accessor key (the getter and setter
    /// are installed by `Accessor`) and the layout gains it; on any other
    /// token (a key it holds as data, a class instance, ⊤) it is E5 HARD.
    DefineAccessor { key: Atom, reason: u32 },
    /// Every token gains keys after birth (computed store …).
    AddsKeys,
    /// Builtin protocol touch: what `proto` reads on every token.
    Implicit { reason: u32, proto: Protocol },
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
    /// The node is the value of a for-in body (FVA 4.3b, `gen/for_in.rs`):
    /// each body token runs per value of `obj` (`ForInKeys`), its runs bound
    /// through the template call `call` (one argument: the key).
    ForInBody { obj: NodeId, call: CallId },
    /// The node is the object for-in body `ft` iterates: a literal whose keys
    /// are exactly its birth keys runs the body once per key, that run's key
    /// parameter holding the one key (`Model::key_nodes`); any other value
    /// runs it once through `call`, with every key.
    ForInKeys { ft: Tok, call: CallId },
    /// The node is the key of `defineProperty(o, k, desc)` in a for-in body:
    /// a run's one key defines it as a constant key does (`DefineAccessor`
    /// for an accessor descriptor, else E5 HARD); any other key is E5 HARD.
    DefineKeyed {
        o: NodeId,
        reason: u32,
        accessor: bool,
    },
    /// The node is the key of a store into `o` in a for-in body: a run's one
    /// key stores under that key, any other under `Key::Any`.
    StoreKeyed {
        o: NodeId,
        src: NodeId,
        site: SiteId,
    },
    /// The node is the key of a definition whose descriptor's get (set) is
    /// `f`, on the objects of `holder`: a run's one key installs it under
    /// that key, any other under the computed-key star.
    AccessorKeyed {
        holder: NodeId,
        f: NodeId,
        setter: bool,
    },
    /// The node is an arithmetic operand: dst ⊇ the kinds its values turn
    /// into (`+` when `add`: strings concatenate).
    Arith { dst: NodeId, add: bool },
    /// The node is the only argument of `Array(x)` / `new Array(x)`: a Number
    /// is the length (the array has holes, no element), any other value is
    /// the one element (ECMA-262 Array). `dst` ⊇ its tokens and its
    /// non-number primitives.
    ArrayLenArg { dst: NodeId },
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
    /// The node is the result of run `run` (`Node::frame`), bound to
    /// `call`: the call's result takes each value, except that the family
    /// of the run's argument marker for a parameter is the caller's own
    /// argument there (DECISIONS 44 (b), `Model::ret_into`).
    RetInto { call: CallId, run: u32 },
    /// The node is argument `i` of `call`, bound to run `run`: each marker
    /// it gains is what the run's result returns of that parameter's marker
    /// family (DECISIONS 44 (b), `Model::arg_into`).
    ArgInto { call: CallId, run: u32, i: u32 },
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
            Cons::HandOver { .. } => "handover",
            Cons::ContractThis { .. } => "contractthis",
            Cons::TopIfAny { .. } => "topifany",
            Cons::Hard { .. } => "hard",
            Cons::AddsKeys => "addskeys",
            Cons::DefineAccessor { .. } => "defineaccessor",
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
            Cons::ArrayLenArg { .. } => "arraylenarg",
            Cons::IdxLoad { .. } => "idxload",
            Cons::IdxKey { .. } => "idxkey",
            Cons::ForInBody { .. } => "forinbody",
            Cons::ForInKeys { .. } => "forinkeys",
            Cons::DefineKeyed { .. } => "definekeyed",
            Cons::StoreKeyed { .. } => "storekeyed",
            Cons::AccessorKeyed { .. } => "accessorkeyed",
            Cons::ShareBuffer { .. } => "sharebuffer",
            Cons::ThisTo { .. } => "thisto",
            Cons::CallArgs { .. } => "callargs",
            Cons::RetInto { .. } => "retinto",
            Cons::ArgInto { .. } => "arginto",
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
    /// A delivery left out a marker that reads as ⊤ here (DECISIONS 42):
    /// this node no longer holds every value of its sources, so it never
    /// takes a source's set by sharing (`copy_to`, `try_copy_delivery`).
    pub dropped: bool,
    /// Successor set, once the list is long (exact edge dedup for hubs: a
    /// function's return node can be offered the same edge millions of
    /// times).
    pub succ_set: Option<std::rc::Rc<crate::pts::FxHashSet<NodeId>>>,
    /// An observation node: a parameter-derived node of a summarized
    /// function's shared body. It holds the union over every call for the
    /// read sites; only read rules apply to it, it never feeds a node that
    /// is not an observation node, and its effects are skipped.
    pub obs: bool,
    /// A parameter node (an instance's, a function's): it holds its
    /// argument marker in place of the markers that arrive (DECISIONS 44).
    pub param: bool,
    /// The run this node belongs to (DECISIONS 44 (b)): an instance + 1, or
    /// `FN_RUN | f` for a function with one body for every call (another
    /// unit's function); 0: none. Its fresh slots, the field nodes of its
    /// argument markers' families, the temporaries of its calls. What it
    /// holds is one run's.
    pub frame: u32,
    /// No value leaves the node (no successor, its rules only use what it
    /// holds): a value entering it reaches no run (DECISIONS 44 (b)).
    pub sink: bool,
    /// A parameter slot only the instance's calls write: its argument
    /// marker is the caller's argument at each call's result
    /// (DECISIONS 44 (b)).
    pub rel: bool,
    /// The result node of a run: what it returns of no parameter of the
    /// run leaves through it as one marker (MRK, `Model::crosses`).
    pub result: bool,
    /// A field node of a marker (its any-key, keyed and all-fields
    /// nodes): it holds the marker's reach markers as themselves.
    pub of_marker: bool,
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
    /// The same for the template's class object and its prototype: a static
    /// and an instance field of one name are different properties of
    /// different objects (FVA Q3), and a prototype member is neither.
    pub class_writers: FxHashMap<(TmplId, Atom), FieldWriters>,
    pub proto_writers: FxHashMap<(TmplId, Atom), FieldWriters>,
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
    pub implicit_done: crate::pts::FxHashSet<(Tok, Protocol)>,
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
    /// Classes linked to a parent at gen time whose `extends` expression is
    /// still evaluated (a class in a CommonJS factory, §17.4): the node the
    /// evaluated value reaches. After the solve it must be that parent.
    pub parent_checks: Vec<(TmplId, NodeId)>,
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
    /// The bodies of closure expressions and of function declarations
    /// (`bindings::BodyRef`: what codegen may call without the dispatcher).
    pub closures: crate::pts::FxHashSet<FnIdx>,
    pub decls: crate::pts::FxHashSet<FnIdx>,
    /// A class's declared named static fields (FVA 3.4,
    /// `derive::constant_statics`).
    pub static_fields: crate::pts::FxHashSet<(TmplId, Atom)>,
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
    /// Stores into a builtin function value at a node, with their reason:
    /// the node decides once the worklist is empty (`settle_bf_written`). The
    /// opaque builtin function value (one token for every one) runs as
    /// unknown code under every rule, so beside ⊤ it is that ⊤: a node that
    /// holds ⊤ by then stored into ⊤. Only a store at a node without ⊤ marks
    /// builtin function values written.
    pub bf_stores: Vec<(NodeId, u32)>,
    /// FVA 4.3b: a for-in body run's copy of its key variable -> the run's one
    /// key (the variable holds nothing else).
    pub key_nodes: FxHashMap<NodeId, Atom>,
    /// For-in bodies run per key: (call, body token, the literal), checked
    /// again once the worklist is empty (`settle_for_in`).
    pub for_in_runs: Vec<(CallId, Tok, Tok)>,
    /// (call, body token) run once with every key.
    pub for_in_any: crate::pts::FxHashSet<(CallId, Tok)>,
    /// (object node, body token, call) whose `ForInKeys` is attached.
    pub for_in_on: crate::pts::FxHashSet<(NodeId, Tok, CallId)>,
    /// (key node, operand, kind) a keyed rule (`DefineKeyed`, `StoreKeyed`,
    /// `AccessorKeyed`) has applied once.
    pub keyed_done: crate::pts::FxHashSet<(NodeId, NodeId, u8)>,
    /// For-in bodies that define a property under their key.
    pub for_in_defines: crate::pts::FxHashSet<FnIdx>,
    /// The key variable of every for-in body: a run's copy of it holds the
    /// run's key (`key_nodes`).
    pub for_in_key_vars: crate::pts::FxHashSet<NodeId>,
    /// The keys such a body may define, accessor keys from generation on
    /// (`Gen::note_for_in_define_keys`).
    pub for_in_define_keys: crate::pts::FxHashSet<Atom>,
    /// The node a `Cons::Store` runs on, while it runs.
    pub store_at: Option<NodeId>,
    /// A call's constant first argument (a string), for callees whose
    /// result depends on it (`BuiltinNs::Require`). Compiler data.
    pub call_lit: FxHashMap<CallId, Atom>,
    /// The CommonJS registry: every published record (`CJSREG`), the
    /// records under an unknown path (`CJSUNKNOWNPATH`), and the records
    /// published under each constant path (`Gen::cjs_path_node`).
    pub cjs_union: NodeId,
    pub cjs_unknown: NodeId,
    pub cjs_paths: FxHashMap<Atom, NodeId>,
    /// `require.cache` / `require.extensions` of [`BuiltinNs::Require`]:
    /// the runtime's own objects, opaque, when no CommonJS support module
    /// installs the program's (`cjs_cache`, `cjs_ext` hold them as well).
    pub require_parts: Option<(Tok, Tok)>,
    /// What `require.cache` / `require.extensions` / `require.main` are at
    /// run time: the support module's cache and the extensions the runtime
    /// returned to it (`__perry_runtime.cjsInstall`), the runtime's own
    /// objects otherwise, and the main record or `undefined`.
    pub cjs_cache: NodeId,
    pub cjs_ext: NodeId,
    pub cjs_main: NodeId,
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
    pub fn_inst: FxHashMap<FnIdx, TmplId>,
    /// Per-unit contracts (design §11.2): the methods of a published class
    /// run on its receiver token (`Model::publish_class`).
    pub published_this: FxHashMap<FnIdx, Tok>,
    /// A published class's receiver token -> the instances other units
    /// hold: the one they construct, then each one this unit gives them
    /// through a contract (`Model::give_out`). What a method does to its
    /// receiver happens to each of them.
    pub contract_recv: FxHashMap<Tok, Vec<Tok>>,
    /// What the methods did to a receiver token so far: replayed on an
    /// instance given out later.
    pub recv_log: FxHashMap<Tok, Vec<RecvEffect>>,
    /// A published class -> its receiver token.
    pub contract_recv_of: FxHashMap<TmplId, Tok>,
    /// Another unit's templates this unit sees through a contract, with the
    /// facts their defining unit published (design §11.2).
    pub foreign: FxHashMap<TmplId, crate::TemplateFacts>,
    /// Foreign templates this unit stored into (a census report).
    pub foreign_written: crate::pts::FxHashSet<TmplId>,
    /// Existing data fields of other units' literal layouts and class
    /// instances this unit stored into: store effects, not census reports
    /// (decision 17), with the node of each stored value (its kinds are
    /// published, DECISIONS 73).
    pub foreign_stores: crate::pts::FxHashSet<(TmplId, Atom, NodeId)>,
    /// Published functions with a contract (design §11.2): their results
    /// are checked after the solve instead of escaping.
    pub contract_fns: crate::pts::FxHashSet<FnIdx>,
    /// Closed functions (decision 13): per parameter the node holding what
    /// the other units' calls pass (`Gen::closed_params`), instead of ⊤.
    pub closed_params: FxHashMap<FnIdx, Vec<NodeId>>,
    /// Per closed function: which parameters every caller passes a
    /// described value (no unknown one) (decision 17).
    pub closed_described: FxHashMap<FnIdx, Vec<bool>>,
    /// Argument markers (and their reach markers) that stand only for
    /// markers beside their values (`beside_values`, DECISIONS 74, 75).
    /// A marker leaves it when a member that is not beside links in.
    pub beside_markers: crate::pts::FxHashSet<Tok>,
    /// Per beside argument marker: the argument markers it is a member of
    /// that are beside too (they leave with it, DECISIONS 75).
    pub beside_up: FxHashMap<Tok, Vec<Tok>>,
    /// Per beside argument marker: the arithmetic results that took no
    /// object kinds for it (DECISIONS 75); they take them if it leaves.
    pub beside_arith: FxHashMap<Tok, Vec<(NodeId, u16)>>,
    /// Decision 19: the reason a closed function is published to the
    /// module's other units (`Gen::unit_boundary`). Published that way its
    /// token does not escape: its parameters hold the closed contract's
    /// stand-ins (`escape_fn_params`). Any other escape is a real one
    /// (unknown code may call it): ⊤ parameters.
    pub closed_pub_reason: Option<u32>,
    pub closed_pub: crate::pts::FxHashSet<Tok>,
    pub contract_rets: Vec<(FnIdx, NodeId)>,
    /// A contract function's parameter markers: one object token per
    /// parameter standing for everything the caller passes (anything read
    /// from it is the marker again), and what the callee did with it
    /// (`MARK_*`).
    pub param_marker: FxHashMap<Tok, (FnIdx, u32)>,
    pub marker_of: FxHashMap<(FnIdx, u32), Tok>,
    pub marker_use: FxHashMap<Tok, u8>,
    /// The markers of what a parameter or argument reaches (not the
    /// argument itself).
    pub reach_marker: crate::pts::FxHashSet<Tok>,
    /// A parameter marker's reach marker.
    pub marker_reach: FxHashMap<Tok, Tok>,
    /// A parameter marker's reach markers by the named key it was read
    /// under (`p.k`, design §18.4): what the argument reaches through `k`.
    /// The reach marker above stands for what a computed key reaches.
    pub keyed_reach: FxHashMap<Tok, Vec<(Atom, Tok)>>,
    /// An argument marker's keyed reach marker -> (the argument marker, key).
    pub keyed_parent: FxHashMap<Tok, (Tok, Atom)>,
    /// A parameter marker's uses under a named key that reach it through
    /// argument markers, settled after the solve (`settle_keyed_uses`):
    /// the contract publishes them.
    pub keyed_uses: FxHashMap<Tok, Vec<(Atom, u8)>>,
    /// An argument marker: the uses some named key of it had, its own
    /// (its keyed reach markers') and its containers' (`keyed_use`). During
    /// the solve a keyed use matters to the markers it stands for only as
    /// these bits; the keys themselves are settled after it.
    pub keyed_any: FxHashMap<Tok, u8>,
    /// Markers that read as ⊤ (DECISIONS 42), a bit per token: the
    /// contract records the escape, so the consumer sinks the argument and
    /// reads nothing else of it; the marker says no more than the ⊤ beside
    /// it.
    pub marker_top: Vec<u64>,
    /// DECISIONS 44: a parameter node's argument marker. The markers its
    /// arguments carry arrive as this one token, which stands for each of
    /// them: they take its uses, and the readers after the solve read it as
    /// them (`marker_members`). One body shared by many callers then holds
    /// one marker per parameter, not every caller's.
    pub arg_marker_of: FxHashMap<NodeId, Tok>,
    /// An argument marker -> the markers it stands for.
    pub arg_members: FxHashMap<Tok, Vec<Tok>>,
    /// The parameter markers each argument marker stands for, settled
    /// over the member graph as the readers ask (`marker_members`); the
    /// graph only grows while solving, so a solve starts it over.
    pub members_settled: std::cell::RefCell<FxHashMap<Tok, std::rc::Rc<[Tok]>>>,
    pub arg_linked: crate::pts::FxHashSet<(Tok, Tok)>,
    /// Argument markers, a bit per token.
    pub arg_bits: Vec<u64>,
    /// DECISIONS 44 (b): per token of an argument marker's family (the
    /// marker, its reach marker, its keyed reach markers) the marker and
    /// its home, the instance whose relative parameter holds it (+ 1; 0:
    /// none, the family is never substituted).
    pub fam: Vec<(Tok, u32)>,
    /// The `this` node of each instance method / accessor of a class: the
    /// binding its entry guard tests (design §11.4).
    pub this_class: FxHashMap<NodeId, TmplId>,
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
/// The run of a function with one body for every call (`Node::frame`);
/// instance runs count from 1 below it.
pub const FN_RUN: u32 = 1 << 31;

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
        let mut v: Vec<Tok> = self
            .tok_index
            .get(&TokKind::Obj(tm, NO_CTX))
            .copied()
            .into_iter()
            .collect();
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

/// A contract parameter's marker was read through (the caller's getters
/// may run), touched by a builtin protocol (ToPrimitive, iteration …: the
/// caller's methods may run), or used beyond that (stored into, opened,
/// called, a method called on it, escaped: the argument escapes).
pub const MARK_READ: u8 = 1;

#[inline]
pub(crate) fn marker_top_bit(bits: &[u64], t: Tok) -> bool {
    bits.get((t >> 6) as usize)
        .is_some_and(|w| w >> (t & 63) & 1 != 0)
}
pub const MARK_TOUCH: u8 = 2;
pub const MARK_ESCAPE: u8 = 4;

impl Model {
    /// Record what the callee did to a parameter marker.
    #[inline]
    pub(crate) fn mark_use(&mut self, t: Tok, bits: u8) {
        if self.param_marker.is_empty() {
            return;
        }
        // An argument marker's use is a use of each marker it stands for.
        if self.is_arg_marker(t) {
            let mut st = vec![(t, bits)];
            while let Some((x, b)) = st.pop() {
                // Each marker the walk visits is solve work (113-CWL3).
                self.work += 1;
                if !self.is_arg_marker(x) {
                    self.mark_use(x, b);
                    continue;
                }
                // A use of what argument marker `p` reaches under one key is
                // recorded on that key's reach marker `x`, the token the use
                // names: a repeated use costs this step alone.
                if let Some(&(p, _)) = self.keyed_parent.get(&x) {
                    let u = self.marker_use.entry(x).or_default();
                    let new = b & !*u;
                    *u |= b;
                    if new != 0 {
                        self.keyed_use(p, new);
                    }
                    continue;
                }
                let u = self.marker_use.entry(x).or_default();
                let new = b & !*u;
                *u |= b;
                if new != 0 {
                    st.extend(self.arg_members[&x].iter().map(|&m| (m, new)));
                }
            }
            return;
        }
        let Some(&(f, i)) = self.param_marker.get(&t) else {
            return;
        };
        let u = self.marker_use.entry(t).or_default();
        let new = bits & !*u;
        *u |= bits;
        // An escaped argument is sunk by the contract's consumer, which
        // then reads none of its other uses, results or reached values
        // (gen.rs `stand_in_fn_of`): the marker, and for the argument
        // itself its reach marker, read as ⊤ from now on (DECISIONS 42).
        // A closed function's described parameter keeps its uses: its
        // contract drops the escape (decision 17), and its callers' values
        // are beside the marker, not ⊤.
        let described = self
            .closed_described
            .get(&f)
            .and_then(|d| d.get(i as usize))
            .is_some_and(|&d| d);
        if new & MARK_ESCAPE != 0 && !described {
            self.set_marker_top(t);
            if let Some(&r) = self.marker_reach.get(&t) {
                self.set_marker_top(r);
            }
        }
    }

    /// Parameter marker `t`'s reach marker for named key `a` (design
    /// §18.4): the value of `p.a`, and everything read from it. Its uses
    /// are published per key, so the caller applies them to its argument's
    /// `a` alone, not to everything the argument reaches (its prototype,
    /// its class).
    pub(crate) fn keyed_reach_of(&mut self, t: Tok, a: Atom) -> Tok {
        if let Some(&(_, r)) = self
            .keyed_reach
            .get(&t)
            .and_then(|v| v.iter().find(|(k, _)| *k == a))
        {
            return r;
        }
        let TokKind::Obj(tm, _) = self.toks[t as usize] else {
            unreachable!("a marker is an object token")
        };
        let display = format!(
            "{}:reach.{}",
            self.tmpls[tm as usize].display, self.atoms[a as usize]
        );
        let rm = self.marker_template(display);
        let r = self.tok(TokKind::Obj(rm, NO_CTX));
        // What an argument marker reaches is of its family (its field
        // nodes are of the same run).
        let home = self.fam_home(t);
        if home != 0 {
            self.set_fam(r, t, home);
        }
        self.reach_marker.insert(r);
        self.keyed_reach.entry(t).or_default().push((a, r));
        match self.param_marker.get(&t).copied() {
            Some(fi) => {
                self.param_marker.insert(r, fi);
            }
            // An argument marker's keyed reach: its uses are its members'
            // uses under the same key (DECISIONS 44), recorded on this token
            // and settled onto the parameter markers after the solve
            // (`settle_keyed_uses`), not as a token per member.
            None => {
                let w = (r >> 6) as usize;
                if self.arg_bits.len() <= w {
                    self.arg_bits.resize(w + 1, 0);
                }
                self.arg_bits[w] |= 1 << (r & 63);
                self.arg_members.insert(r, Vec::new());
                self.keyed_parent.insert(r, (t, a));
                if self.beside_markers.contains(&t) {
                    self.beside_markers.insert(r);
                }
            }
        }
        // A marker before its field node: that node is of the marker.
        let rany = self.field_any(r);
        self.add_tok(rany, r);
        r
    }

    /// A use `bits` of what argument marker `t` reaches through some named
    /// key (design §18.4). Each marker it stands for takes the bits: a
    /// reach marker as a use of itself (it reaches itself under every key),
    /// an argument marker passes them on, once per bit. A parameter marker
    /// takes nothing here: which keys it was used under is a fact for its
    /// contract alone, settled after the solve (`settle_keyed_uses`). So a
    /// key costs the use that names it, not a walk over every marker the
    /// argument marker stands for.
    pub(crate) fn keyed_use(&mut self, t: Tok, bits: u8) {
        let mut st = vec![(t, bits)];
        while let Some((x, b)) = st.pop() {
            // Each marker the walk visits is solve work (113-CWL3).
            self.work += 1;
            if self.reach_marker.contains(&x) {
                self.mark_use(x, b);
                continue;
            }
            if !self.is_arg_marker(x) {
                continue;
            }
            let u = self.keyed_any.entry(x).or_default();
            let new = b & !*u;
            *u |= b;
            if new != 0 {
                st.extend(self.arg_members[&x].iter().map(|&m| (m, new)));
            }
        }
    }

    /// The keyed uses of each parameter marker that some argument marker
    /// stands for (design §18.4, DECISIONS 44): the keys every argument
    /// marker standing for it, directly or through other argument markers,
    /// was used under. Read once the model is final, in one pass over the
    /// member graph: the markers of a cycle (a recursion passing its
    /// argument on) stand for each other and have one set of uses, so its
    /// strongly connected components are taken in topological order and
    /// each key set moves along each edge between them once.
    pub(crate) fn settle_keyed_uses(&mut self) {
        self.keyed_uses.clear();
        // The argument markers a keyed use walks through (`keyed_use`).
        let inner = |s: &Self, x: Tok| s.is_arg_marker(x) && !s.reach_marker.contains(&x);
        let mut ids: Vec<Tok> = self
            .arg_members
            .keys()
            .copied()
            .filter(|&x| inner(self, x))
            .collect();
        ids.sort_unstable();
        let at: FxHashMap<Tok, usize> = ids.iter().enumerate().map(|(i, &t)| (t, i)).collect();
        let n = ids.len();
        let succ: Vec<Vec<usize>> = ids
            .iter()
            .map(|x| {
                self.arg_members[x]
                    .iter()
                    .filter_map(|m| at.get(m).copied())
                    .collect()
            })
            .collect();
        // Tarjan, iterative: a component gets its number after every
        // component it reaches, so descending numbers are topological.
        const NONE: usize = usize::MAX;
        let (mut index, mut low, mut comp) = (vec![NONE; n], vec![0; n], vec![NONE; n]);
        let mut on = vec![false; n];
        let (mut stack, mut next, mut ncomp) = (Vec::new(), 0usize, 0usize);
        for s0 in 0..n {
            if index[s0] != NONE {
                continue;
            }
            let mut frames = vec![(s0, 0usize)];
            index[s0] = next;
            low[s0] = next;
            next += 1;
            stack.push(s0);
            on[s0] = true;
            while let Some(&(v, e)) = frames.last() {
                if let Some(&w) = succ[v].get(e) {
                    frames.last_mut().unwrap().1 += 1;
                    if index[w] == NONE {
                        index[w] = next;
                        low[w] = next;
                        next += 1;
                        stack.push(w);
                        on[w] = true;
                        frames.push((w, 0));
                    } else if on[w] {
                        low[v] = low[v].min(index[w]);
                    }
                    continue;
                }
                frames.pop();
                if let Some(&(u, _)) = frames.last() {
                    low[u] = low[u].min(low[v]);
                }
                if low[v] == index[v] {
                    while let Some(w) = stack.pop() {
                        on[w] = false;
                        comp[w] = ncomp;
                        if w == v {
                            break;
                        }
                    }
                    ncomp += 1;
                }
            }
        }
        let mut members_of: Vec<Vec<usize>> = vec![Vec::new(); ncomp];
        for v in 0..n {
            members_of[comp[v]].push(v);
        }
        let mut acc: Vec<FxHashMap<Atom, u8>> = vec![FxHashMap::default(); ncomp];
        let mut out: FxHashMap<Tok, FxHashMap<Atom, u8>> = FxHashMap::default();
        let merge = |into: &mut FxHashMap<Atom, u8>, from: &FxHashMap<Atom, u8>| {
            for (&a, &b) in from {
                *into.entry(a).or_default() |= b;
            }
        };
        for c in (0..ncomp).rev() {
            let mut set = std::mem::take(&mut acc[c]);
            // Its own uses: what its keyed reach markers were used for.
            for &v in &members_of[c] {
                for &(a, r) in self.keyed_reach.get(&ids[v]).into_iter().flatten() {
                    let u = self.marker_use.get(&r).copied().unwrap_or(0);
                    if u != 0 {
                        *set.entry(a).or_default() |= u;
                    }
                }
            }
            if set.is_empty() {
                continue;
            }
            for &v in &members_of[c] {
                for &m in &self.arg_members[&ids[v]] {
                    match at.get(&m) {
                        Some(&w) if comp[w] != c => merge(&mut acc[comp[w]], &set),
                        Some(_) => {}
                        None if self.param_marker.contains_key(&m)
                            && !self.reach_marker.contains(&m) =>
                        {
                            merge(out.entry(m).or_default(), &set)
                        }
                        None => {}
                    }
                }
            }
        }
        for (p, set) in out {
            let mut v: Vec<(Atom, u8)> = set.into_iter().collect();
            v.sort_unstable();
            self.keyed_uses.insert(p, v);
        }
    }

    pub(crate) fn set_marker_top(&mut self, t: Tok) {
        let w = (t >> 6) as usize;
        if self.marker_top.len() <= w {
            self.marker_top.resize(w + 1, 0);
        }
        self.marker_top[w] |= 1 << (t & 63);
    }

    /// A marker that reads as ⊤ (DECISIONS 42).
    #[inline]
    pub(crate) fn marker_is_top(&self, t: Tok) -> bool {
        marker_top_bit(&self.marker_top, t)
    }

    /// An escaped value beside ⊤ is that ⊤ (DECISIONS 42's reason, for a
    /// value): unknown code holds it, so its fields are ⊤ and sink what they
    /// get, every function reachable from it runs with ⊤, and its layout is
    /// escaped; a rule on it at a node that holds ⊤ does nothing the rule on
    /// ⊤ does not. Left out there, it never rides ⊤ through the unit. A
    /// marker is decided by `marker_is_top` (its escape may be no sink,
    /// decision 17).
    #[inline]
    pub(crate) fn top_beside(&self, t: Tok) -> bool {
        self.is_escaped(t) && !self.is_marker(t)
    }

    /// A marker that stands for no value of its own (DECISIONS 74): one of
    /// a closed function's described parameter (decision 17), what it
    /// reaches, or an argument marker all of whose members are such
    /// markers (DECISIONS 75), and what that reaches. The callers' values
    /// are the contract's, beside it in every node it reaches (they arrive
    /// together), so it adds no kinds.
    pub(crate) fn beside_values(&self, t: Tok) -> bool {
        match self.param_marker.get(&t) {
            Some(&(f, i)) => self
                .closed_described
                .get(&f)
                .and_then(|d| d.get(i as usize))
                .is_some_and(|&d| d),
            None => self.beside_markers.contains(&t),
        }
    }

    /// A closed function's parameter marker (decision 13): what its callers
    /// pass is the values beside it; the marker only records the uses for
    /// its contract.
    pub(crate) fn closed_marker(&self, t: Tok) -> bool {
        self.is_marker(t)
            && self.marker_members(t).iter().all(|x| {
                self.param_marker
                    .get(x)
                    .is_some_and(|(f, _)| self.closed_params.contains_key(f))
            })
    }

    /// A parameter marker or an argument marker (DECISIONS 44).
    #[inline]
    pub(crate) fn is_marker(&self, t: Tok) -> bool {
        self.param_marker.contains_key(&t) || self.is_arg_marker(t)
    }

    #[inline]
    pub(crate) fn is_arg_marker(&self, t: Tok) -> bool {
        marker_top_bit(&self.arg_bits, t)
    }

    /// The parameter markers token `t` stands for: itself, unless it is an
    /// argument marker (DECISIONS 44). What an argument marker reaches under
    /// a key (its keyed reach marker, whose uses are settled per key and
    /// which has no members of its own) is part of what it reaches: it
    /// stands for its reach marker's members.
    ///
    /// Settled once per marker over the member graph's strongly connected
    /// components (a recursion's markers stand for each other and have one
    /// set), each component's set the union of its members' leaves and its
    /// successors' sets, shared by its members: the readers after a solve
    /// ask for the same markers at every site, result and contract.
    pub(crate) fn marker_members(&self, t: Tok) -> Vec<Tok> {
        if !self.is_arg_marker(t) {
            return vec![t];
        }
        if let Some(s) = self.members_settled.borrow().get(&t) {
            return s.to_vec();
        }
        self.settle_members_from(t);
        self.members_settled.borrow()[&t].to_vec()
    }

    /// The successors of argument marker `x` in the member graph: its
    /// members, or for a keyed reach marker its argument marker's reach.
    fn member_succ(&self, x: Tok) -> Vec<Tok> {
        if let Some(&(p, _)) = self.keyed_parent.get(&x) {
            return vec![self.marker_reach[&p]];
        }
        self.arg_members.get(&x).cloned().unwrap_or_default()
    }

    /// Tarjan from `root` over the argument markers not settled yet; a
    /// settled marker and a parameter marker are leaves of the walk.
    fn settle_members_from(&self, root: Tok) {
        use crate::pts::{FxHashMap, FxHashSet};
        let mut index: FxHashMap<Tok, (u32, u32)> = FxHashMap::default();
        let mut on: FxHashSet<Tok> = FxHashSet::default();
        let mut stack: Vec<Tok> = Vec::new();
        let mut next = 0u32;
        // (marker, its successors, the next successor to visit)
        let mut frames: Vec<(Tok, Vec<Tok>, usize)> = Vec::new();
        let succ = self.member_succ(root);
        index.insert(root, (0, 0));
        next += 1;
        stack.push(root);
        on.insert(root);
        frames.push((root, succ, 0));
        while let Some(frame) = frames.last_mut() {
            let (v, ref ss, ref mut i) = *frame;
            if *i < ss.len() {
                let w = ss[*i];
                *i += 1;
                if !self.is_arg_marker(w) || self.members_settled.borrow().contains_key(&w) {
                    continue;
                }
                match index.get(&w) {
                    None => {
                        index.insert(w, (next, next));
                        next += 1;
                        stack.push(w);
                        on.insert(w);
                        let ws = self.member_succ(w);
                        frames.push((w, ws, 0));
                    }
                    Some(&(wi, _)) if on.contains(&w) => {
                        let e = index.get_mut(&v).unwrap();
                        e.1 = e.1.min(wi);
                    }
                    Some(_) => {}
                }
                continue;
            }
            let (vi, vlow) = index[&v];
            frames.pop();
            if let Some(&(u, _, _)) = frames.last() {
                let e = index.get_mut(&u).unwrap();
                e.1 = e.1.min(vlow);
            }
            if vlow != vi {
                continue;
            }
            // The component is complete: its leaves and its successors'
            // settled sets, one set for every member.
            let mut comp: Vec<Tok> = Vec::new();
            while let Some(w) = stack.pop() {
                on.remove(&w);
                comp.push(w);
                if w == v {
                    break;
                }
            }
            let mut set: Vec<Tok> = Vec::new();
            {
                let settled = self.members_settled.borrow();
                for &w in &comp {
                    for s in self.member_succ(w) {
                        if !self.is_arg_marker(s) {
                            set.push(s);
                        } else if let Some(ss) = settled.get(&s) {
                            set.extend(ss.iter().copied());
                        }
                        // else: a member of this component.
                    }
                }
            }
            set.sort_unstable();
            set.dedup();
            let rc: std::rc::Rc<[Tok]> = set.into();
            let mut settled = self.members_settled.borrow_mut();
            for w in comp {
                settled.insert(w, rc.clone());
            }
        }
    }

    /// Parameter node `n`'s argument marker, with its own reach marker.
    pub(crate) fn arg_marker(&mut self, n: NodeId) -> Tok {
        if let Some(&t) = self.arg_marker_of.get(&n) {
            return t;
        }
        let tm = self.marker_template(format!("args{n}"));
        let t = self.tok(TokKind::Obj(tm, NO_CTX));
        let rm = self.marker_template(format!("args{n}:reach"));
        let r = self.tok(TokKind::Obj(rm, NO_CTX));
        for x in [t, r] {
            let w = (x >> 6) as usize;
            if self.arg_bits.len() <= w {
                self.arg_bits.resize(w + 1, 0);
            }
            self.arg_bits[w] |= 1 << (x & 63);
            self.arg_members.insert(x, Vec::new());
        }
        self.marker_reach.insert(t, r);
        self.reach_marker.insert(r);
        self.arg_marker_of.insert(n, t);
        // A relative parameter's marker is the caller's argument at each
        // call's result while it stays in its run (DECISIONS 44 (b)).
        if self.nodes[n as usize].rel {
            let home = self.nodes[n as usize].frame;
            self.set_fam(t, t, home);
            self.set_fam(r, t, home);
        }
        // It stands for its members alone (DECISIONS 75): beside while it
        // has none, and while each one that links in is beside
        // (`link_marker`). The caller links the arriving markers before
        // the marker reaches any node.
        self.beside_markers.insert(t);
        self.beside_markers.insert(r);
        let any = self.field_any(t);
        self.add_tok(any, r);
        let rany = self.field_any(r);
        self.add_tok(rany, r);
        t
    }

    /// Argument marker `r` stands for marker `m` too: `m` takes every use
    /// of `r`, and what `m` reaches takes every use of what `r` reaches.
    pub(crate) fn link_marker(&mut self, m: Tok, r: Tok) {
        if m == r || !self.arg_linked.insert((r, m)) {
            return;
        }
        self.arg_members.get_mut(&r).unwrap().push(m);
        // A new link changes the sets settled so far (`marker_members`).
        if !self.members_settled.get_mut().is_empty() {
            self.members_settled.get_mut().clear();
        }
        if self.beside_markers.contains(&r) {
            match self.beside_values(m) {
                true if !self.param_marker.contains_key(&m) => {
                    self.beside_up.entry(m).or_default().push(r)
                }
                true => {}
                false => self.leave_beside(r),
            }
        }
        let u = self.marker_use.get(&r).copied().unwrap_or(0);
        if u != 0 {
            self.mark_use(m, u);
        }
        let reach = |s: &Self, x: Tok| s.marker_reach.get(&x).copied().unwrap_or(x);
        let (mr, rr) = (reach(self, m), reach(self, r));
        if (mr, rr) != (m, r) {
            self.link_marker(mr, rr);
        }
        // And per key (§18.4): what `m` reaches through a key takes the
        // uses of what `r` reaches through it so far, as bits (later ones:
        // `keyed_use`; the keys: `settle_keyed_uses`).
        let k = self.keyed_any.get(&r).copied().unwrap_or(0);
        if k != 0 {
            self.keyed_use(m, k);
        }
    }

    /// Argument marker `t` stands for a value of its own now (DECISIONS
    /// 75): so do what it reaches and the argument markers it is a member
    /// of, and each arithmetic result that skipped it takes the object
    /// kinds it would have taken. Beside only ever turns off: monotone.
    pub(crate) fn leave_beside(&mut self, t: Tok) {
        let mut st = vec![t];
        while let Some(x) = st.pop() {
            if !self.beside_markers.remove(&x) {
                continue;
            }
            for (dst, out) in self.beside_arith.remove(&x).unwrap_or_default() {
                self.add_prim(dst, out);
            }
            st.extend(self.beside_up.remove(&x).unwrap_or_default());
            st.extend(self.marker_reach.get(&x).copied());
            if let Some(ks) = self.keyed_reach.get(&x) {
                st.extend(ks.iter().map(|&(_, r)| r));
            }
        }
    }

    fn set_fam(&mut self, t: Tok, base: Tok, home: u32) {
        let i = t as usize;
        if self.fam.len() <= i {
            self.fam.resize(i + 1, (0, 0));
        }
        self.fam[i] = (base, home);
    }

    /// The home of token `t`'s argument marker family (+ 1), 0 for any
    /// other token (DECISIONS 44 (b)).
    #[inline]
    pub(crate) fn fam_home(&self, t: Tok) -> u32 {
        self.fam.get(t as usize).map_or(0, |&(_, h)| h)
    }

    /// Marker `t` arriving at node `n` crosses into it as a member of
    /// `n`'s own marker (MRK; DECISIONS 44 for parameters): a marker is a
    /// token only inside its run. A parameter takes every marker; a node
    /// of no run of `t`'s family (another run's node, storage) takes its
    /// family tokens; a result or storage takes the markers of no run
    /// (another node's marker, a contract parameter's). Storage is every
    /// node of no run but an observation node, a sink and a marker's own
    /// field nodes. Inside its run a family token stays itself, so a
    /// result can return it relative to each call (44 (b)).
    #[inline]
    pub(crate) fn crosses(&self, n: NodeId, t: Tok) -> bool {
        let node = &self.nodes[n as usize];
        if node.param {
            return self.is_marker(t);
        }
        if node.obs || node.sink || node.of_marker || !self.is_marker(t) {
            return false;
        }
        match self.fam_home(t) {
            0 => node.result || node.frame == 0,
            h => h != node.frame,
        }
    }

    /// What a load from marker `m` gives (solve.rs `field`): under a named
    /// key its keyed reach marker, under a computed key its reach marker;
    /// a reach marker reaches itself under every key.
    pub(crate) fn marker_load(&mut self, m: Tok, key: Option<Atom>) -> Tok {
        if self.reach_marker.contains(&m) {
            return m;
        }
        match key {
            Some(a) => self.keyed_reach_of(m, a),
            None => self.marker_reach[&m],
        }
    }

    /// What marker `m` reaches under a computed key: its reach marker, or
    /// itself for a reach marker.
    pub(crate) fn reach_of(&self, m: Tok) -> Tok {
        if self.reach_marker.contains(&m) {
            return m;
        }
        self.marker_reach.get(&m).copied().unwrap_or(m)
    }

    /// Token `t` of argument marker `b`'s family, with marker `m` in place
    /// of `b`: `m` itself, what `m` reaches, or what it reaches through the
    /// same key (DECISIONS 44 (b)).
    pub(crate) fn lift(&mut self, m: Tok, b: Tok, t: Tok) -> Tok {
        if t == b {
            return m;
        }
        let key = self.keyed_parent.get(&t).map(|&(_, a)| a);
        self.marker_load(m, key)
    }

    /// The run of `call` (`Node::frame`): the run its result node belongs
    /// to (a call an instance made has its own result slot). A temporary
    /// the solver makes for one call is of that run.
    pub(crate) fn call_run(&self, call: CallId) -> u32 {
        let Some(c) = self.calls.get(call as usize) else {
            return 0;
        };
        // Its operands are nodes of the run that made it.
        let this = match c.this {
            ThisArg::Node(n) => Some(n),
            _ => None,
        };
        std::iter::once(c.ret)
            .chain(this)
            .chain(c.args.iter().copied())
            .filter(|&n| n != NO_NODE)
            .map(|n| self.nodes[n as usize].frame)
            .find(|&f| f != 0)
            .unwrap_or(0)
    }

    /// The tokens of argument marker `b`'s family.
    pub(crate) fn family(&self, b: Tok) -> Vec<Tok> {
        let mut v = vec![b];
        v.extend(self.marker_reach.get(&b).copied());
        v.extend(
            self.keyed_reach
                .get(&b)
                .into_iter()
                .flatten()
                .map(|&(_, r)| r),
        );
        v
    }

    fn marker_template(&mut self, display: String) -> TmplId {
        let tm = self.tmpls.len() as TmplId;
        self.tmpls.push(Template {
            kind: TmplKind::BObj {
                module: 0,
                site: 0,
                what: "parameter marker",
            },
            display,
            birth_keys: Vec::new(),
            definite: Vec::new(),
            declared: Vec::new(),
            parent: None,
            ctor: None,
            native_parent: None,
            accessor_keys: Vec::new(),
            has_computed_members: false,
            field_init: None,
            proto_members: Vec::new(),
            parent_value: None,
            parent_links: Vec::new(),
        });
        tm
    }

    /// The marker of parameter `i` of contract function `f`.
    pub(crate) fn param_marker_tok(&mut self, f: FnIdx, i: u32) -> Tok {
        if let Some(&t) = self.marker_of.get(&(f, i)) {
            return t;
        }
        let tm = self.tmpls.len() as TmplId;
        let display = format!("{}:param{i}", self.funcs[f as usize].name);
        self.tmpls.push(Template {
            kind: TmplKind::BObj {
                module: 0,
                site: 0,
                what: "parameter marker",
            },
            display,
            birth_keys: Vec::new(),
            definite: Vec::new(),
            declared: Vec::new(),
            parent: None,
            ctor: None,
            native_parent: None,
            accessor_keys: Vec::new(),
            has_computed_members: false,
            field_init: None,
            proto_members: Vec::new(),
            parent_value: None,
            parent_links: Vec::new(),
        });
        let t = self.tok(TokKind::Obj(tm, NO_CTX));
        // What the argument reaches has its own marker (fields of fields
        // reach it again), so a result can tell the argument itself from
        // values reachable from it (§11.2 "the return").
        let rm = self.tmpls.len() as TmplId;
        let mut reach = self.tmpls[tm as usize].clone();
        reach.display = format!("{}:param{i}:reach", self.funcs[f as usize].name);
        self.tmpls.push(reach);
        let r = self.tok(TokKind::Obj(rm, NO_CTX));
        self.param_marker.insert(t, (f, i));
        self.param_marker.insert(r, (f, i));
        self.reach_marker.insert(r);
        self.marker_reach.insert(t, r);
        self.marker_of.insert((f, i), t);
        // Markers before their field nodes: those nodes are of the marker.
        let any = self.field_any(t);
        self.add_tok(any, r);
        let rany = self.field_any(r);
        self.add_tok(rany, r);
        t
    }
}
