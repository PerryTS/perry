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
/// The key an element store (`push`, `set.add`) writes on a value that may
/// be a builtin prototype: an index, or no property at all (a collection's
/// entries). A write under it is a write of every key that may be ToString
/// of a Number (`builtins::native_written`, `solve::numeric_key`).
pub const NUM_KEY: &str = "#num";
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
    /// A method of `Object.prototype` read as a value under a key that may
    /// name any of them (`o[k]` on an object whose chain ends there): one of
    /// `hasOwnProperty`, `valueOf`, `__defineGetter__` ... A call runs each
    /// one's effect on its receiver (`proto_method`); none is a constructor.
    ObjectProtoMethod,
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

/// An object every unit shares (decision 151): the runtime's own objects
/// (`Object.prototype` and the builtin prototypes, the builtin namespaces and
/// the global object, builtin function values) are one object for the whole
/// program, and ⊤ may be any of them. A value contract names them
/// (`ValueContract::shared`); what one unit does to them is every unit's
/// ([`SharedWrite`]).
#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Shared {
    Top,
    ObjectProto,
    Builtin(BuiltinNs),
    BuiltinFn,
    NativeProto(String),
}

/// What a unit's solve can observe of the [`Shared`] objects: the ones its
/// code reaches (their fields, members, prototype lookups), and the keys of
/// its lookups that still pass a native prototype or read a program-owned
/// global (`Model::native_missing`), which a write of ⊤ under the key would
/// turn opaque. The driver solves a unit again only for a write it can see
/// (`units::run`).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct SharedReads {
    /// The shared objects it reads fields of, by key (`None`: under a
    /// computed key, so every key).
    pub objects: std::collections::BTreeMap<Shared, Option<std::collections::BTreeSet<String>>>,
    pub keys: std::collections::BTreeSet<String>,
}

impl SharedReads {
    /// It reads `s` under key `k` (`None`: a computed write, any key).
    fn reads(&self, s: &Shared, k: Option<&String>) -> bool {
        match (self.objects.get(s), k) {
            (None, _) => false,
            (Some(None), _) | (Some(Some(_)), None) => true,
            (Some(Some(ks)), Some(k)) => ks.contains(k),
        }
    }

    pub fn sees(&self, w: &SharedWrite) -> bool {
        match w {
            SharedWrite::Handed(_) => false,
            // E23: every lookup.
            SharedWrite::Accessor(Shared::ObjectProto, _) => true,
            SharedWrite::Store(Shared::Top, k) => match k {
                Some(k) => self.keys.contains(k),
                None => !self.keys.is_empty(),
            },
            SharedWrite::Accessor(Shared::Top, k) => self.keys.contains(k),
            SharedWrite::Accessor(s, k) => self.reads(s, Some(k)) || self.keys.contains(k),
            SharedWrite::Store(s, k) => {
                self.reads(s, k.as_ref())
                    || match k {
                        Some(k) => self.keys.contains(k),
                        None => !self.keys.is_empty(),
                    }
            }
            SharedWrite::Unmodeled(s) => self.objects.contains_key(s) || !self.keys.is_empty(),
        }
    }
}

/// What a unit does to a [`Shared`] object that other units' lookups can
/// see: published by every unit (`ProgramFacts::shared_out`) and replayed in
/// every unit's solve (`Model::replay_shared`), as whole-program facts see
/// every module's writes in one solve.
#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum SharedWrite {
    /// A store under a key (`None`: computed) (`Model::store_into`).
    Store(Shared, Option<String>),
    /// An accessor installed under a key (`Cons::AccessorInstall`).
    Accessor(Shared, String),
    /// The object reached unmodeled code (`Model::escape`): any key of it
    /// may be written.
    Unmodeled(Shared),
    /// The object escaped (`Model::escape`): unknown code holds it, and per
    /// unit that includes the other units' code. It is ⊤ there, so every
    /// unit's store into ⊤ may be a store into it (`units::shared_closure`).
    Handed(Shared),
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
    /// A declared-keys record born with a null prototype
    /// (`Class::declared_keys_null_proto`: `require.cache`): its prototype
    /// stand-in has no members and no prototype, so a lookup through it ends
    /// there and never reaches `Object.prototype`.
    pub null_proto: bool,
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
    /// A class constructor's hidden trailing parameters, one per value the
    /// class captures (`__perry_cap_*`, perry-hir `synthesize_class_captures`):
    /// the class environment slot each one is filled from. A construction
    /// from another module passes the class's own snapshot there, whatever
    /// arguments it was given (`construct_registered_class_resolved` fills
    /// only the user slots from them), so another unit never chooses these
    /// values (DECISIONS 117).
    pub captures: Vec<NodeId>,
    /// The function whose body creates this closure (`NO_FN` for
    /// declarations, methods and builtins).
    pub parent: FnIdx,
    /// Another unit's function seen through a contract that lets any unit
    /// use it (a stand-in's method, a function contract's stand-in; not a
    /// closed one, whose users its owner knows): published onward, it
    /// leaves with that contract (DECISIONS 120, `Model::escape`).
    pub forwarded: bool,
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

/// What another unit's method does to its receiver (DECISIONS 57): its use
/// and reach marks (`MARK_*`) and the keys it stores (`None`: any key).
#[derive(Clone, Debug)]
pub struct ThisContract {
    pub uses: u8,
    pub reach: u8,
    pub stores: Option<Vec<String>>,
    /// What the method stores under each key of `stores`, in order
    /// (DECISIONS 179): the values its flow describes, ⊤ where none does.
    pub values: Vec<NodeId>,
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
    /// A method call on a primitive receiver of these kinds.
    PrimCall(u16, CallId),
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
    /// The node is the value a contract function `f` stores under `key`
    /// into its parameter `dst`'s argument (`reach`: into what it reaches)
    /// (DECISIONS 118; `dst` `u32::MAX`: into what its receiver reaches,
    /// and `Key::Any` a computed key, DECISIONS 149): a marker of one of
    /// `f`'s parameters is that argument, which the reader stores; any
    /// other value escapes.
    StoreOut {
        f: FnIdx,
        dst: u32,
        reach: bool,
        /// A keyed reach (design §18.4): what the argument holds under it.
        via: Option<Atom>,
        key: Key,
        reason: u32,
        /// The receiver's reach marker stored into (`dst` `u32::MAX`).
        at: Tok,
        /// What the store met: `at`, or (a receiver-rooted store, DECISIONS
        /// 149b) another function's marker or ⊤.
        to: Tok,
    },
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
    /// unit's census covers what the constructor does with it; an array
    /// leaves by its element cell (DECISIONS 94, 149e: the parameter is
    /// checked as a published value, `Gen::check_value`, and the argument
    /// contract names it); any other token escapes, with the reason.
    /// DECISIONS 152: a parameter marker of this unit's function (its
    /// callers' value) is handed over by that function's contract
    /// (`FnContract::param_handovers`): the callers' units pass what they
    /// hold to argument `arg` of the constructor `ctor` themselves.
    HandOver { reason: u32, ctor: Tok, arg: u32 },
    /// DECISIONS 149d: what a receiver-rooted store of function `f` puts
    /// into an object of this unit: every value but `f`'s parameter and
    /// argument markers, which are `f`'s receiver flow (`Cons::StoreOut`).
    OwnOnly { f: FnIdx, dst: NodeId },
    /// The `this` of a contract stand-in of another unit's class `tmpl`
    /// (design §11.2): its instances are inside the contract; any other
    /// receiver escapes (that unit runs the method on it unseen).
    /// `this` of another unit's method: only that class's instances (and,
    /// for a static method, its class object); any other receiver escapes.
    /// `this` (`Model::this_contracts`, `u32::MAX`: none) is the method's
    /// receiver contract (DECISIONS 57): a token of a subclass of `tmpl` is
    /// admitted with the published effects on it.
    ContractThis {
        tmpl: TmplId,
        reason: u32,
        statics: bool,
        this: u32,
    },
    /// `this` of the override token of (`tmpl`, `key`) (DECISIONS 97): a
    /// method's receiver marker records the call (`Model::this_calls`);
    /// `tmpl`'s receiver token passes (it stands for the class's own
    /// instances, which no subclass key reaches); any other receiver
    /// escapes with `reason`, as for an unknown callee.
    OverrideThis {
        tmpl: TmplId,
        key: Atom,
        reason: u32,
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
    /// `delete o.key`: E7 HARD on every object token; on ⊤ (which may be a
    /// builtin prototype) a write of `key` only.
    DeleteKey { key: Atom, reason: u32 },
    /// `Object.defineProperty(o, key, desc)` with a constant key (FVA 4.3a).
    /// An accessor descriptor (`data` false) on a literal layout that does
    /// not hold `key` as data: the key becomes an accessor key (the getter
    /// and setter are installed by `Accessor`) and the layout gains it. On a
    /// prototype that has no method `key`, either descriptor: `key` is a key
    /// of the prototype that is no plain data slot (an accessor, or a value
    /// whose attributes are the descriptor's), as a class getter is; the
    /// value is stored by the caller. Any other token (a key it holds as
    /// data, a class instance, ⊤, a data descriptor on a literal): E5 HARD.
    DefineKey { key: Atom, data: bool, reason: u32 },
    /// `Object.defineProperties(o, map)`: the node holds the maps. A literal
    /// with exactly its birth keys defines each of them by the descriptor
    /// under it (`DefineDesc`); any other map defines its own values
    /// (`descs`) under a computed key: E5 HARD on `o`, the descriptors'
    /// values (`value`, a load of `descs`) stored under any key at `site`.
    DefineAll {
        o: NodeId,
        descs: NodeId,
        value: NodeId,
        site: SiteId,
        reason: u32,
    },
    /// Internal: the node holds the descriptors `defineProperties` defines
    /// `key` of `o` by. An accessor descriptor literal is a `DefineKey`
    /// accessor; any other descriptor is a data definition (its `value`
    /// stored under `key` at `site`); either installs its get/set.
    DefineDesc {
        o: NodeId,
        key: Atom,
        site: SiteId,
        reason: u32,
    },
    /// Every token gains keys after birth (computed store …).
    AddsKeys,
    /// Builtin protocol touch: what `proto` reads on every token.
    Implicit { reason: u32, proto: Protocol },
    /// The node holds `Object.create` prototype arguments (E10).
    CreateProto { reason: u32 },
    /// The node is `ProtoNode(of)`: dst ⊇ the values of `key` on each
    /// prototype (and further up), shared by every receiver of `of`.
    ChainUnion { key: Key, dst: NodeId, of: Tok },
    /// The node holds a value given as the prototype of an object (its
    /// `ProtoNode` is `protos`): a builtin prototype there may be any of
    /// them (`ObjectProto` collapses them), whose members the chain cannot
    /// name: the chain also holds an unknown prototype.
    ProtoGiven { protos: NodeId },
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
    /// a run's one key defines it as a constant key does (`DefineKey`);
    /// any other key is E5 HARD.
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
            Cons::OwnOnly { .. } => "ownonly",
            Cons::StoreOut { .. } => "storeout",
            Cons::ContractThis { .. } => "contractthis",
            Cons::OverrideThis { .. } => "overridethis",
            Cons::TopIfAny { .. } => "topifany",
            Cons::Hard { .. } => "hard",
            Cons::DeleteKey { .. } => "deletekey",
            Cons::AddsKeys => "addskeys",
            Cons::DefineKey { .. } => "definekey",
            Cons::DefineAll { .. } => "defineall",
            Cons::DefineDesc { .. } => "definedesc",
            Cons::Implicit { .. } => "implicit",
            Cons::CreateProto { .. } => "createproto",
            Cons::ChainUnion { .. } => "chainunion",
            Cons::ProtoGiven { .. } => "protogiven",
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

/// DECISIONS 118: (function, parameter, into its reach, via, key).
pub type StoreFlowKey = (FnIdx, u32, bool, Option<Atom>, Key);

/// DECISIONS 118: one stored key of a contract function's parameter.
#[derive(Clone, Debug, Default)]
pub struct StoreFlow {
    pub from: Vec<u32>,
    /// DECISIONS 136: what parameter `j`'s argument holds under `via`
    /// (its keyed reach), or reaches (`None`), stored here.
    pub from_reach: Vec<(u32, Option<Atom>)>,
    pub top: bool,
    pub prim: u16,
    /// DECISIONS 176: this unit's values stored here (its own instances,
    /// other units' stand-ins), handed to the caller's unit as a result is
    /// (`Gen::publish_result`); `vals_top` once no contract describes them.
    pub vals: Option<NodeId>,
    pub vals_top: bool,
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
    /// DECISIONS 149b: the object stored into is reached from `func`'s
    /// receiver by a chain of loads (`rooted.rs`).
    pub rooted: bool,
    /// DECISIONS 149d: rooted, and the object is a load (not `this`): it
    /// holds a reach marker of the receiver.
    pub deep: bool,
}

#[derive(Default, Clone, Debug)]
pub struct FieldWriters {
    pub ctor: Vec<SiteId>,
    pub post: Vec<SiteId>,
}

/// Where the solve work went (PERRY_FACTS_PROFILE only): by the popped node
/// it was spent for (or the phase outside the worklist), by rule kind
/// (inclusive of what the rule triggers), by propagation step, and a growth
/// line every `WorkProfile::STEP` work.
#[derive(Default)]
pub struct WorkProfile {
    /// The work since `at.1` is charged to node `at.0` (`NO_NODE`: phase `at.2`).
    pub at: (NodeId, u64, &'static str),
    pub node: FxHashMap<NodeId, u64>,
    pub phase: FxHashMap<&'static str, u64>,
    /// Rule kind: (applications, work, attach replays, attach replay work).
    pub kind: FxHashMap<&'static str, (u64, u64, u64, u64)>,
    /// Pop charge (delta x constraints), fast copy deliveries, copy visits and
    /// checks, `copy_to` extra.
    pub step: [u64; 4],
    pub next_curve: u64,
}

impl WorkProfile {
    pub const STEP: u64 = 5_000_000;
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
    /// An array's values under a key no store named (`Model::named_any`):
    /// what its named non-index keys read. Its elements are not among them.
    /// A module namespace's values every name reads (DECISIONS 215).
    pub named_any: FxHashMap<Tok, NodeId>,
    /// DECISIONS 215: a module namespace -> the namespaces of its `export *`
    /// sources (`TOP`: one outside the program), and the names the module
    /// exports itself (those shadow every `export *`).
    pub star_from: FxHashMap<Tok, Vec<Tok>>,
    pub ns_own: crate::pts::FxHashSet<(Tok, Atom)>,
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
    /// (value node, `ProtoNode`) pairs a `ProtoGiven` watches.
    pub given_proto: crate::pts::FxHashSet<(NodeId, NodeId)>,
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
    /// The function standing for another unit's accessors on the shared
    /// objects (`builtins::other_unit_accessor`).
    pub other_accessor: Option<Tok>,
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
    /// No interpreted code can run in the program (no dynamic code, no
    /// dynamic code reach, no direct eval scope): what this unit publishes
    /// reaches compiled units only, which use another unit's stand-in
    /// through its contract (DECISIONS 120, `Model::escape`).
    pub interp_free: bool,
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
    /// Per unit: what this unit's code does to the shared objects
    /// ([`SharedWrite`]), not counting what it replays of other units'.
    pub shared_out: std::collections::BTreeSet<SharedWrite>,
    /// Writing for other units' code: replaying their shared writes
    /// (`Model::replay_shared`), or the unit boundary standing in for what
    /// they do with this unit's values (`Gen::unit_boundary`). Each unit
    /// publishes its own writes; these are not this unit's.
    pub shared_replay: bool,
    /// The boundary's store standing in for other units' stores into what
    /// this unit gives them (`Gen::unit_boundary`): not this unit's write.
    pub other_units_store: Option<NodeId>,
    /// Prototype nodes of escaped values (`Model::escape_inner`): what
    /// escapes there is reached along a chain, not handed over as a value.
    pub chain_sinks: crate::pts::FxHashSet<NodeId>,
    /// Escaping through a node of `chain_sinks`.
    pub chain_escape: bool,
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
    /// `defineProperties` maps defined key by key: (map, o, descs, value,
    /// site, reason) of the `DefineAll`, checked again once the worklist is
    /// empty (`settle_define`).
    pub define_runs: Vec<(Tok, NodeId, NodeId, NodeId, SiteId, u32)>,
    /// (descs, map) defined under any key; descs whose E5 and store under
    /// any key are attached.
    pub define_any: crate::pts::FxHashSet<(NodeId, Tok)>,
    pub define_any_on: crate::pts::FxHashSet<NodeId>,
    /// Accessor descriptors a `DefineDesc` defined by: (descriptor, o, key,
    /// site, reason), checked again once the worklist is empty.
    pub desc_runs: Vec<(Tok, NodeId, Atom, SiteId, u32)>,
    /// (o, key, descriptor) defined as data.
    pub desc_data: crate::pts::FxHashSet<(NodeId, Atom, Tok)>,
    /// (o, key, data) whose `DefineKey` is attached.
    pub define_on: crate::pts::FxHashSet<(NodeId, Atom, bool)>,
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
    /// DECISIONS 171: cell nodes nothing in the unit consumed when it was
    /// seeded (`Gen::seed_cell`); the first successor or constraint of one
    /// moves it to `cell_woken`, and the unit seeds it then.
    pub cell_watch: crate::pts::FxHashSet<NodeId>,
    pub cell_woken: Vec<NodeId>,
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
    /// Explain: builtin function values minted, by the arm that minted them.
    pub bf_mints: FxHashMap<&'static str, u32>,
    /// Per unit (design §17.16 (b)): the global object's program-owned
    /// fields are cells (`global:<key>`), made at the unit boundary. A field
    /// first named later holds ⊤ and sets `global_late`: the unit then
    /// publishes `global:*` unknown, as for a global unmodeled code writes.
    pub global_cells: bool,
    pub global_late: bool,
    /// This unit's own code (not a replayed write of another unit's,
    /// `shared_replay`) put the globals in reach of unmodeled code: the
    /// global object reached ⊤, a computed key of it was written, a global
    /// was named after the boundary. Its `global:*` write says so; a
    /// replayed one is its writer's, published there already.
    pub global_written_own: bool,
    /// DECISIONS 94: arrays published through an element cell
    /// (`arr:<name>`, `Gen::arr_leaves`) and other units' arrays this unit
    /// holds, each with the seed of cell `arr:<name>#named`: a named field
    /// of one holds what that seed holds (⊤ once another unit stored a
    /// named key) and what is stored there escapes (no contract describes
    /// it). `arr_named`: the ones this unit stored a named key into.
    pub cell_arrs: FxHashMap<Tok, NodeId>,
    pub arr_named: crate::pts::FxHashSet<Tok>,
    /// What was stored under a named key into an array, per array (in unit
    /// mode; sunk if the array turns out to be shared, `Gen::arr_leaves`).
    pub arr_named_srcs: FxHashMap<Tok, Vec<NodeId>>,
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
    /// With the profile: where the solve work went (`WorkProfile`).
    pub prof: Option<Box<WorkProfile>>,
    pub fn_inst: FxHashMap<FnIdx, TmplId>,
    /// Per-unit contracts (design §11.2): the methods of a published class
    /// run on its receiver token (`Model::publish_class`), one per published
    /// class whose chain inside the unit holds the method.
    pub published_this: FxHashMap<FnIdx, Vec<Tok>>,
    /// A published class's receiver token -> its class.
    pub recv_class: FxHashMap<Tok, TmplId>,
    /// Receiver tokens and receiver markers (DECISIONS 59): their prototype
    /// chain (the published classes they stand for), and the method keys a
    /// named load resolves through it instead of the own field.
    pub virt_protos: FxHashMap<Tok, Vec<TmplId>>,
    pub virt_by_class: FxHashMap<TmplId, Vec<Tok>>,
    /// Named-load nodes on those tokens, and their keys per token.
    pub virt_nodes: FxHashMap<(Tok, Atom), NodeId>,
    pub virt_tok_keys: FxHashMap<Tok, Vec<Atom>>,
    /// Keys an instance of a published class may hold as own keys (stored
    /// by the class code), and classes whose instances may hold any key (a
    /// computed store, a hard operation, an escape).
    pub virt_revoked: crate::pts::FxHashSet<(TmplId, Atom)>,
    pub virt_open: crate::pts::FxHashSet<TmplId>,
    /// Census: keys a subclass of another unit defines, per published
    /// class (`override <k> of <display>`; `*`: any key).
    pub override_keys: FxHashMap<TmplId, Vec<Atom>>,
    /// The callee node of a call of an override-only key on a receiver of
    /// a published class (`Model::override_node`, DECISIONS 97): it holds
    /// one function token per (class, key), the method another unit's
    /// subclass defines under the key.
    pub override_fns: FxHashMap<(TmplId, Atom), NodeId>,
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
    /// DECISIONS 125: the call by which other units construct a published
    /// class (`Model::publish_class`): its instance is the class's own
    /// token `Obj(tm, NO_CTX)`, the one its receiver token and the
    /// published receivers (DECISIONS 107) stand for (`Model::site_tok`).
    pub contract_new: crate::pts::FxHashSet<CallId>,
    /// DECISIONS 116: published classes no other unit names (given out
    /// through a contract result or value): their methods run on the
    /// instances given out only.
    pub sealed: crate::pts::FxHashSet<TmplId>,
    /// The `this` nodes of the methods that run on the instances given out
    /// (`Model::escape_this`: a sealed class's methods, DECISIONS 116, and
    /// the methods run on published receivers, DECISIONS 125), which every
    /// later `give_out` reaches.
    pub given_this: FxHashMap<FnIdx, Vec<NodeId>>,
    /// The receiver contracts of other units' methods this unit calls
    /// (`Cons::ContractThis`'s `this`, DECISIONS 57).
    pub this_contracts: Vec<ThisContract>,
    /// The keys a published method stores into its receiver marker
    /// (DECISIONS 57); in `this_open` when a key is computed.
    pub this_stores: FxHashMap<FnIdx, Vec<Atom>>,
    /// The override-only keys a published method calls on its receiver
    /// (DECISIONS 97): only another unit's subclass defines them, so the
    /// readers run the call (`FnContract::this_calls`).
    pub this_calls: FxHashMap<FnIdx, Vec<Atom>>,
    pub this_open: crate::pts::FxHashSet<FnIdx>,
    /// The same for a contract function's parameter (DECISIONS 84): the
    /// keys it stores into the argument, by (function, parameter index).
    pub param_stores: FxHashMap<(FnIdx, u32), Vec<Atom>>,
    pub param_open: crate::pts::FxHashSet<(FnIdx, u32)>,
    /// What a contract function stores under a named key into its argument
    /// or into what that reaches (DECISIONS 118), under any key into what
    /// an argument or its receiver reaches (DECISIONS 149), by (function,
    /// parameter, into its reach, key): the parameters whose arguments are
    /// stored, and whether another value (⊤ to the reader) or primitives
    /// are.
    pub store_flows: FxHashMap<StoreFlowKey, StoreFlow>,
    pub store_out_done: crate::pts::FxHashSet<(NodeId, FnIdx, u32, bool, Option<Atom>, Key, Tok)>,
    /// DECISIONS 149d: per (stored node, function) the node of its values
    /// that are not the function's markers (`Cons::OwnOnly`).
    pub rooted_own: FxHashMap<(NodeId, FnIdx), NodeId>,
    /// DECISIONS 118, argument markers: the named-key stores into one (each
    /// replayed on every marker that links in later), the `StoreOut`s it is
    /// the source of (function, parameter, reason: a member other than that
    /// parameter's marker escapes), the parameter node it is the marker of,
    /// and the links not replayed yet (the solver loop replays them).
    pub arg_stores: FxHashMap<Tok, Vec<(Key, NodeId, SiteId)>>,
    pub arg_store_done: crate::pts::FxHashSet<(Tok, Key, NodeId)>,
    pub arg_src_watch: FxHashMap<Tok, Vec<(FnIdx, u32, u32)>>,
    /// DECISIONS 152: the markers handed to a closed class's constructor
    /// (marker, constructor token, argument), published per parameter by
    /// the function the marker is of; per argument marker the reasons a
    /// member that is no parameter marker escapes with, as members link in.
    pub handed: crate::pts::FxHashSet<(Tok, Tok, u32)>,
    pub handover_watch: FxHashMap<Tok, Vec<u32>>,
    /// DECISIONS 136: a store flow whose stored value is (what) an argument
    /// marker (reaches): (stored marker, flow, reason) per argument marker
    /// whose members it stands for, replayed as members link in; the
    /// receiver reach marker stored into when the store met it
    /// (DECISIONS 151).
    pub arg_src_flows: FxHashMap<Tok, Vec<(Tok, StoreFlowKey, u32, Option<Tok>)>>,
    /// `arg_src_flows` entries by (argument marker, entry): each once.
    pub arg_src_flow_done: crate::pts::FxHashSet<(Tok, Tok, StoreFlowKey, u32, Option<Tok>)>,
    /// An argument marker's reach marker -> the argument marker.
    pub arg_reach_base: FxHashMap<Tok, Tok>,
    pub arg_marker_node: FxHashMap<Tok, NodeId>,
    /// The function and parameter index of each parameter node: the
    /// function's own and each summary instance's (a method run through an
    /// instance holds its argument marker on the instance's node).
    pub param_at: FxHashMap<NodeId, (FnIdx, u32)>,
    pub marker_links: Vec<(Tok, Tok)>,
    /// Published classes whose constructor is a contract (DECISIONS 57).
    pub ctor_published: crate::pts::FxHashSet<TmplId>,
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
    /// DECISIONS 194: the closed functions another unit's code calls (an
    /// argument contract names them): only their parameters carry store
    /// flows across the unit edge.
    pub closed_called_out: crate::pts::FxHashSet<FnIdx>,
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
    /// The reason of a publication to another unit (`unit_boundary`): a
    /// published method's receiver is its contract's then (DECISIONS 85).
    pub pub_reason: Option<u32>,
    /// The reason thrown values reach interpreted code (`compute_in`), and
    /// what the seed of cell `thrown` holds: the values the other units
    /// threw (DECISIONS 174), as their contracts name them. Another unit's
    /// value thrown there escaped in its thrower's solve (`escape`).
    pub thrown_reason: Option<u32>,
    pub thrown_in: crate::pts::FxHashSet<Tok>,
    /// The function contracts the census broke (`fn:<name>`): published as
    /// escapes, not contracts.
    pub broken_fns: crate::pts::FxHashSet<String>,
    /// The stand-in function of each `this_contracts` entry: a receiver
    /// its contract does not admit is a use beyond it (DECISIONS 85).
    pub this_contract_fns: Vec<Tok>,
    /// DECISIONS 107: (`this_contracts` index, receiver) for every receiver
    /// this unit runs another unit's method on.
    pub this_admitted: Vec<(u32, Tok)>,
    /// DECISIONS 107: a published method's receivers as the units running
    /// it published them (`Gen::published_receivers`).
    pub this_args: FxHashMap<FnIdx, NodeId>,
    /// DECISIONS 165: the published methods no unit runs
    /// (`Gen::published_args`): per parameter, its marker stands for no
    /// value of another unit.
    pub method_described: FxHashMap<FnIdx, Vec<bool>>,
    /// DECISIONS 148: a published method some unit runs: per parameter the
    /// node holding what the running units pass (`Gen::published_args`),
    /// `NO_NODE` where some call passes an undescribed value.
    pub method_params: FxHashMap<FnIdx, Vec<NodeId>>,
    /// DECISIONS 165: the functions a call bound (`bind_args`), in any
    /// context.
    pub called: crate::pts::FxHashSet<FnIdx>,
    /// DECISIONS 210: the stand-ins of other units' contract functions, each
    /// with the node holding the stand-in instance of its owner's `new <f>`
    /// layout and that layout's template, `None` until the owner publishes
    /// it. A `new` of one constructs that layout (`construct`), never a
    /// template of this unit; the construction is published (`constructed`,
    /// `ProgramFacts::ctor_runs_out`).
    pub foreign_ctor: FxHashMap<FnIdx, Option<(NodeId, TmplId)>>,
    pub constructed: crate::pts::FxHashSet<FnIdx>,
    /// DECISIONS 210: this unit's constructions of its own contract
    /// functions for the units that construct them (`Gen::published_ctors`):
    /// function and result node. The instance's layout is published to them
    /// (`Gen::contract_check`, `fn_inst_published`).
    pub own_ctor_runs: Vec<(FnIdx, NodeId)>,
    pub fn_inst_published: crate::pts::FxHashSet<TmplId>,
    /// DECISIONS 210: stand-ins of other units' functions this unit used as
    /// objects beyond their contract (a store into one, its `prototype`
    /// read while its owner published no layout): census reports.
    pub fn_touched: crate::pts::FxHashSet<Tok>,
    /// DECISIONS 210: this unit's contract functions published to other
    /// units (`Model::escape_props`): every own property but `prototype`
    /// left with the function (unknown code reads and writes them); the
    /// reason it left.
    pub props_escaped: FxHashMap<Tok, u32>,
    /// DECISIONS 210/211: the functions unknown code constructs
    /// (`Model::construct_unknown`), once each.
    pub unknown_new: crate::pts::FxHashSet<FnIdx>,
    /// DECISIONS 210: the `new` calls of other units' functions: no call
    /// of the function (`called`; their arguments go by `ctor_runs_out`).
    pub ctor_calls: crate::pts::FxHashSet<CallId>,
    /// The reason a published literal's field function escapes
    /// (`Gen::note_field_fns`): a contract publication, as `pub_reason`.
    pub field_fn_reason: Option<u32>,
    /// DECISIONS 165: the stand-in methods of other units' classes with no
    /// function contract (`Gen::stub_fn`): named as the method.
    pub stub_methods: crate::pts::FxHashSet<FnIdx>,
    pub closed_pub: crate::pts::FxHashSet<Tok>,
    pub contract_rets: Vec<(FnIdx, NodeId)>,
    /// The calls through a contract function's parameter marker
    /// (function, parameter, method key, call): `p(..)` (no key, DECISIONS
    /// 140) or `p.k(..)` (DECISIONS 144). Their arguments go to the
    /// caller's unit, described as the result is.
    pub marker_calls: Vec<(FnIdx, u32, Option<Atom>, CallId)>,
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
    /// DECISIONS 158: a parameter's keyed reach marker, and each step below
    /// it -> (the marker it was read from, the key; `None`: a computed
    /// key). A path marker stands for the values at its path in the
    /// argument (the shape's path), not for everything read from them.
    pub path_parent: FxHashMap<Tok, (Tok, Option<Atom>)>,
    /// A path marker's step under a key: a marker of its own, or the
    /// earlier marker on its path read under the same key (the fold).
    pub path_step: FxHashMap<(Tok, Option<Atom>), Tok>,
    /// The steps made below each path marker (not the folds).
    pub path_steps: FxHashMap<Tok, Vec<Tok>>,
    /// The keys a fold repeats at a path marker: the path goes on through
    /// them any number of times there.
    pub path_loops: FxHashMap<Tok, Vec<Option<Atom>>>,
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
    /// DECISIONS 193: per argument marker, the readers it stands for: each
    /// non-argument marker below it (`None`), or what one reaches under a
    /// key (an argument marker's keyed reach below it, DECISIONS 121); kept
    /// once each (`arg_reader_seen`). The argument markers a marker is a
    /// member of (`arg_parents`; a keyed reach's base by key:
    /// `arg_keyed_parents`) take the readers it gains.
    pub arg_readers: FxHashMap<Tok, Vec<(Tok, Option<Atom>)>>,
    pub arg_reader_seen: crate::pts::FxHashSet<(Tok, Tok, Option<Atom>)>,
    pub arg_parents: FxHashMap<Tok, Vec<Tok>>,
    pub arg_keyed_parents: FxHashMap<Tok, Vec<(Atom, Tok)>>,
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

#[path = "model/operations.rs"]
mod operations;

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
