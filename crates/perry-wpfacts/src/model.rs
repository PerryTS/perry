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
/// The atom of the protocol value key (`Model::yield_key`) and of the
/// builtin `IterResult` field it reads.
pub const YIELD_VALUE: &str = "#yield";

/// `Model::idx_done` rules of DECISIONS 217 (`IdxKey` uses 0-18): a
/// prototype read as a value, a number key's chain (read, call), a
/// computed call's full lookup and numeric own fields, and (plus the primitive bit) a call under
/// ToPropertyKey of `true`/`null`/`undefined`.
pub const IDX_PROTO_VALUE: u16 = 32;
pub const IDX_PLAIN_LOAD: u16 = 33;
pub const IDX_PLAIN_CALL: u16 = 34;
pub const IDX_NUM_OWN: u16 = 35;
pub const IDX_CALL_ANY: u16 = 36;
pub const IDX_CALL_NUM_OWN: u16 = 37;
pub const IDX_CALL_NAMED: u16 = 64;
/// The `IdxMCall` guards: a token receiver's key rule attached
/// (`Cons::IdxKeyCall` on the key node), and (plus the primitive mask) a
/// primitive receiver's. Each guarded action has its own tag: the
/// `IdxKeyCall` run of a primitive receiver is `128 | mask` on the same
/// (key node, ⊤, call), and `IdxLoad` holds tag 0 with a load's dst in the
/// call's slot.
pub const IDX_MCALL_TOKEN: u16 = 38;
pub const IDX_MCALL_PRIM: u16 = 256;
/// `idx_done` tags of the own-only read (`Cons::IdxOwnLoad`): its key rule
/// attached, a token key's full lookup, a string/symbol key, a number key,
/// the gate fired; 46, 47, 48: a `true`/`false`, `null`, `undefined` key.
pub const IDX_OWN_KEY: u16 = 41;
pub const IDX_OWN_ANY: u16 = 42;
pub const IDX_OWN_STR: u16 = 43;
pub const IDX_OWN_NUM: u16 = 44;
pub const IDX_OWN_GATE: u16 = 45;
pub const IDX_OWN_PRIM: u16 = 46;

/// Token 0 is ⊤: a value produced or touched by code the analysis does not
/// model. Its presence in a node means "any escaped object or anything else".
pub const TOP: Tok = 0;

/// The context of a template's own token (`TokKind::Obj`).
pub const NO_CTX: CallId = u32::MAX;

/// Primitive kind bits carried next to the object tokens of a node.
/// What a test value must be able to hold for a [`Cons::Guard`] to let its
/// branch through. ⊤ and parameter or argument markers (another unit's or
/// an unseen caller's value) can hold anything; every other token is an
/// object or a function: truthy, neither `undefined` nor `null`, and no
/// String (its ToPrimitive may return one or a number).
pub mod guard {
    pub const UNDEF: u8 = 0;
    pub const NOT_UNDEF: u8 = 1;
    pub const NULLISH: u8 = 2;
    pub const NOT_NULLISH: u8 = 3;
    pub const TRUTHY: u8 = 4;
    pub const FALSY: u8 = 5;
    /// A value that is no String after ToPrimitive: an operand of `+` that
    /// lets the other operand's numeric kinds reach the result.
    pub const NOT_STR: u8 = 6;
    /// A primitive but `undefined`/`null`, no object: what a sloppy
    /// function boxes as its `this`.
    pub const BOXED: u8 = 7;

    /// The primitive kinds (`prim` bits) a test admits.
    pub fn prims(test: u8) -> u16 {
        use super::prim as p;
        match test {
            UNDEF => p::UNDEF,
            NOT_UNDEF => p::ALL & !p::UNDEF,
            NULLISH => p::UNDEF | p::NULL,
            NOT_NULLISH => p::ALL & !(p::UNDEF | p::NULL),
            // Every primitive kind but `undefined`/`null` has a truthy value.
            TRUTHY => p::NUM | p::STR | p::BOOL | p::BIGINT | p::SYMBOL,
            NOT_STR => p::ALL & !p::STR,
            BOXED => p::ALL & !(p::UNDEF | p::NULL),
            // `0`, `""`, `false`, `0n`, `null`, `undefined`.
            _ => p::NUM | p::STR | p::BOOL | p::BIGINT | p::NULL | p::UNDEF,
        }
    }

    /// An object or function token admits the test.
    pub fn objects(test: u8) -> bool {
        matches!(test, NOT_UNDEF | NOT_NULLISH | TRUTHY | NOT_STR)
    }
}

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
    /// A NativeError constructor (ECMA-262 20.5.6: `EvalError`, `RangeError`,
    /// `ReferenceError`, `SyntaxError`, `TypeError`, `URIError`), by its
    /// global name: its own prototype object, its [[Prototype]] `Error`.
    NativeError(&'static str),
    AggregateError,
    Date,
    RegExp,
    /// `%TypedArray%` (ECMA-262 23.2.1), the [[Prototype]] of every
    /// concrete typed-array constructor.
    TypedArray,
    /// A concrete typed-array constructor (`Uint8Array`, ECMA-262 23.2.6),
    /// by its global name: its own prototype object, its [[Prototype]]
    /// `%TypedArray%`.
    TypedArrayKind(&'static str),
    DataView,
    ArrayBuffer,
    SharedArrayBuffer,
    Symbol,
    Number,
    BigInt,
    String,
    Boolean,
    Function,
    /// `Map`: `new` builds a collection of the entries' contents.
    Map,
    /// `Set`: `new` builds a collection of the values.
    Set,
    /// `WeakMap`: as `Map`, with the members of `%WeakMap.prototype%`.
    WeakMap,
    /// `WeakSet`: as `Set`, with the members of `%WeakSet.prototype%`.
    WeakSet,
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
    /// `Object.prototype`'s own method under its own key, read as a value
    /// (`Object.prototype.hasOwnProperty`): one no builtin prototype
    /// overrides ([`ProtoOwn`]), so every builtin prototype
    /// reads it there. A call is that method's call on its receiver
    /// (`object_proto_own_call`); `call`/`apply`/`bind` are
    /// `Function.prototype`'s. Not a constructor.
    ///
    /// The method is named by [`ProtoOwn`], not by an atom: builtin tokens
    /// are shared between the units' models, whose atom tables differ.
    ObjectProtoOwn(ProtoOwn),
    /// `process.stdout`, `process.stderr`, `process.stdin`: host objects
    /// perry's runtime creates once (`js_process_stdout` ...), each one
    /// token. Their described members (`members::ns_statics`) are the
    /// runtime's natives until a store reaches the key (`builtins::host_written`).
    Stdout,
    Stderr,
    Stdin,
    /// A node module object (`require("fs")`, `import * as fs from "fs"`):
    /// the realm's namespace declaration of that name (`node:fs`), one object
    /// per program (node: `require("fs") === require("node:fs")`). Its
    /// members are the realm's declared natives until a store reaches the
    /// key; it describes no other key (an undeclared key reads ⊤).
    Module(&'static str),
    Other,
}

/// The `Object.prototype` methods no builtin prototype overrides
/// (`BuiltinNs::ObjectProtoOwn`): under their key every builtin prototype
/// chain reads `Object.prototype`'s own. Each only reads: `hasOwnProperty` /
/// `propertyIsEnumerable` convert their key (ToPropertyKey) and test an own
/// property of ToObject(this); `isPrototypeOf` walks its argument's chain.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum ProtoOwn {
    HasOwnProperty,
    PropertyIsEnumerable,
    IsPrototypeOf,
}

impl ProtoOwn {
    pub fn of_name(name: &str) -> Option<ProtoOwn> {
        Some(match name {
            "hasOwnProperty" => ProtoOwn::HasOwnProperty,
            "propertyIsEnumerable" => ProtoOwn::PropertyIsEnumerable,
            "isPrototypeOf" => ProtoOwn::IsPrototypeOf,
            _ => return None,
        })
    }
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
    /// A runtime function value (a builtin method read as a value): the
    /// native the read found, or `Native::Any`.
    BuiltinFn(Native),
    /// A module's namespace / `module.exports` object.
    ModNs(u32),
    /// The prototype of a native parent class (`extends EventEmitter`),
    /// whose methods have summaries in `builtins.rs`.
    NativeProto(String),
}

pub use crate::builtins::Family;

/// A native function value, named by the realm's canonical object reference.
/// Reads retain their own keys; only the value uses this identity. `Any`
/// denotes every native function where a computed or written key cannot
/// identify the value.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Native {
    Any,
    /// `holder`: for a closure native whose declaration reads its own
    /// internal slots (`islot(callee, self)`), the object whose
    /// declaration installed this value (one value per holder, as the
    /// runtime makes one closure per object); `None`: the identity's one
    /// value.
    Member {
        identity: &'static str,
        holder: Option<Tok>,
    },
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
            // Every chain reaches Object.prototype.
            SharedWrite::Hard(Shared::ObjectProto, _) => true,
            // DECISIONS 234: a key may be gone from any escaped object.
            SharedWrite::Hard(Shared::Top, Hard::E5DefineProperty) => true,
            SharedWrite::Removed(_) => true,
            SharedWrite::Hard(s, _) => self.objects.contains_key(s),
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
    /// A HARD mark (`Model::mark_hard`: `defineProperty`, `delete`,
    /// `setPrototypeOf` ...) on the object: on `Object.prototype` every
    /// chain through it is HARD, as the whole program derives it.
    Hard(Shared, Hard),
    /// DECISIONS 234: a delete, defineProperty or `with` under a key
    /// (`None`: computed) on an object ⊤ denotes: it may remove or redefine
    /// that key of any escaped object (`top_reshape`). A delete or define on
    /// a parameter marker travels as `Hard` (the callers pass the objects).
    Removed(Option<String>),
}

/// What gen knows of the Array a `new Array(...)` call births
/// (`Model::arr_len_at_site`, `builtins::site_new_array`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct SiteBirth {
    /// The integer literal `k` of `new Array(k)` when the runtime backs it
    /// densely (`solve::arr_len`). Recorded only with array lengths on.
    pub len: Option<i64>,
    /// The birth is the receiver of a one-argument `fill`
    /// (`new Array(k).fill(v)`, DESIGN-p4l1 §2.6): it holds no hole.
    pub hole_free: bool,
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
    /// An array literal / array-producing site (flow only). `hole_free`:
    /// the birth fills every index below its length (a program literal
    /// without an elision, a spread literal, an array record); every other
    /// birth may hold a hole (`Model::hole`).
    ///
    /// `plain` (DESIGN-p4l1 §2.3, PR-B): the birth is a fresh
    /// `GC_TYPE_ARRAY` (never a lazy JSON header), its element storage starts
    /// at the allocation's front (front offset 0), and its capacity is at
    /// least its recorded birth length. Set only at the audited program
    /// births: an array literal, a spread literal and `new Array`/`Array()`
    /// (`bindings::arr_rep` admits only such tokens). Every runtime-born
    /// array is not plain.
    Arr {
        module: u32,
        site: SiteId,
        hole_free: bool,
        plain: bool,
    },
    /// A function's `arguments` object (ECMA-262 10.4.4, flow only). It is
    /// no Array: its prototype is `%Object.prototype%`, and `length` and
    /// `callee` are ordinary own data properties (writable, configurable:
    /// a store or `defineProperty` gives them any value). Its indices are
    /// elements, as an array's are, and it iterates them.
    Args { module: u32, site: SiteId },
    /// A builtin object created at a site (Map, Set, Promise, Date, Error,
    /// RegExp, typed array, ArrayBuffer, iterator …). Flow only.
    BObj {
        module: u32,
        site: SiteId,
        what: &'static str,
    },
}

impl TmplKind {
    /// Buffer storage (the B1 buffer rules, sealed buffer fields): a
    /// `%TypedArray%` instance, a host `Buffer` (`fs.readFileSync(path)`, a
    /// `Uint8Array` whose prototype is `Buffer.prototype`), a `DataView`, an
    /// `ArrayBuffer` or a `SharedArrayBuffer`.
    pub fn typed_storage(&self) -> bool {
        matches!(
            self,
            TmplKind::BObj {
                what: "TypedArray" | "Buffer" | "DataView" | "ArrayBuffer" | "SharedArrayBuffer",
                ..
            }
        )
    }

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
    /// Birth keys reserved for a constructor store
    /// (`ClassFieldOrigin::ConstructorStore`) that are not `definite`: code
    /// can observe the instance before the constructor stores them (a method
    /// call on `this`, ...). Known slots, holding the hole (absent) until
    /// stored; each read pairs its load with the hole compare.
    pub reserved: Vec<Atom>,
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
    /// DECISIONS 242: a lookup on an object of this template may pass its
    /// own keys and reach the prototype chain: a [[Get]] or call of a key
    /// that is not a definite own data key (a computed key included), or a
    /// [[Set]] of one (which may add it). Set by the solve
    /// (`Model::note_chain_miss`). Without one, an accessor under any key on
    /// `Object.prototype` reaches the template only if its keys are made by
    /// [[Set]] (a constructor's `this.k =`), never a literal's birth keys.
    pub chain_miss: bool,
    /// DEC 250 (Q2) on DECISIONS 242(1): a miss whose key a program
    /// accessor on `Object.prototype` may stand under: a computed key, or
    /// `constructor` (the only key the runtime lets a program define there,
    /// `builtins::object_final_read`). On a chain where the collapsed
    /// prototype token is `Object.prototype` only (`derive::plain_chain`),
    /// an accessor under any key there reaches the template only through
    /// one of these.
    pub chain_miss_open: bool,
    /// An array a builtin made with ArraySpeciesCreate (ECMA-262 10.4.2.3:
    /// `slice`, `splice`, `concat`, `map`, `filter`, `flat`, `flatMap`): the
    /// node of the constructors the algorithm may take, each receiver's
    /// `constructor` and that value's `@@species`
    /// (`builtins::array_species`). It is an Array exotic only while no
    /// object but `%Array%` is among them (`derive::exact_array`). `None`:
    /// born by ArrayCreate, or no array.
    pub species: Option<NodeId>,
    /// DECISIONS 251 rule C: this is the template of a class expression the
    /// program may evaluate more than once (the HIR's `Expr::ClassExprFresh`,
    /// the node codegen gives per-evaluation class objects and private
    /// brands: `fresh_class_templates`). Each evaluation is its own class
    /// object, prototype and private brand, so no fact is keyed on them: a
    /// site on its class object (static, `prototype`, static method) and a
    /// private-key site on its instances are No("per class evaluation").
    /// Set where gen meets the node.
    pub per_evaluation: bool,
    /// The named static methods the class defines on its class object at class
    /// evaluation (own data properties, CreateMethodProperty). Read by `shadows`.
    pub static_methods: Vec<Atom>,
}

/// The function body's own-test evidence has an explicit walk lifecycle.
/// Pending evidence is invalidated before summaries run, including bodies
/// skipped by activation or abandoned by a generation bound.
#[derive(Clone, Copy, Debug)]
pub enum OwnTestBody {
    Unknown,
    Pending(NodeId),
    Checked(NodeId),
}

#[derive(Clone, Debug)]
pub struct FuncInfo {
    /// The function body's throw region (DESIGN SEEDE section 8): what a
    /// call of it may throw; `u32::MAX` until first used (`Model::raise_of`).
    pub raise: NodeId,
    #[cfg(test)]
    pub(crate) body_packet: std::rc::Rc<crate::summary::packet::BodyPacket>,
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
    /// Body-derived own-test predicate: a marker refutes it. Synthetic
    /// functions have no body evidence. Shared across closure contexts, so
    /// an invalidating context conservatively invalidates every use.
    pub own_test: OwnTestBody,
    pub token: Tok,
    /// This function is the constructor of that class template.
    pub ctor_of: Option<TmplId>,
    /// Arrow function / `captures_this` closure: `this` is the enclosing one.
    pub lexical_this: bool,
    /// Something outside the call protocol writes the parameters (an object
    /// literal IIFE, a computed accessor): never summarized.
    pub no_summary: bool,
    /// The step closure of a transformed generator is the generator object
    /// (`LinkGeneratorPrototype`); `next`/`return`/`throw` and the
    /// iteration protocol on it run the closure. Its [[Prototype]] is what
    /// this node holds: the generator function's `prototype` field (its
    /// original object, what the program stored there instead, ⊤ once the
    /// function escaped), read by `proto_node`; the program's members sit
    /// there before the protocol.
    pub gen_proto: Option<NodeId>,
    /// A generator function: the node of every `prototype` value of its
    /// tokens (each closure context's own field, `Model::closure_tok`),
    /// which its generator objects inherit from (`gen_proto`).
    pub gen_fn_protos: Option<NodeId>,
    /// An async generator's step closure (its `next` settles a Promise).
    pub gen_async: bool,
    /// A sync step closure with a `yield*`: a step may hand back its
    /// delegate's inner result object as-is (step kind 2).
    pub gen_reyields: bool,
    /// The intrinsic prototype the function object is born with
    /// (`perry_hir::fn_proto`, the predicate codegen births it by): `None`
    /// where no lowered function says it (a constructor, which is its class
    /// value's; a stand-in, a virtual or builtin summary).
    pub proto: Option<perry_hir::fn_proto::FnProtoKind>,
    /// A class constructor of the shape `constructor(...a) { super(...a) }`
    /// (the spec default a derived class gets): the `new` call's own
    /// arguments construct the parent, position by position, not through
    /// one rest array whose elements reach every parameter of the parent
    /// (DECISIONS 49). Its body is not walked: `run_ctor_chain` runs the
    /// parent with the `new` call itself.
    pub forwards_super: bool,
    /// An environment-publishing constructor's hidden trailing parameters:
    /// the class environment slot each one is filled from. Instance-capture
    /// constructors keep their forwarded arguments instead. A construction
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
    /// The throw region the call runs in (DESIGN SEEDE section 8): what the
    /// callee throws reaches this node. A call made outside generation runs
    /// in `Model::thrown`, the shared cell every catch reads.
    pub region: NodeId,
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

/// A property key: a named atom, or a computed key (`Any`). A computed
/// key built from literal parts (`o[type + "s"]`, a template with literal
/// text) carries its shape (`KeyShapeId`, `Model::key_shapes`): it names
/// only members whose name matches (`Model::key_may_name`). Shape 0 is no
/// shape: any key.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Key {
    Named(Atom),
    Any(KeyShapeId),
}

/// The string constants a node holds (`Node::strs`): `STRS_NONE`,
/// `STRS_TOP` or `STRS_SET0 + i` for the interned atom set
/// `Model::str_sets[i]` (sorted, at most `STRS_K` atoms).
pub type StrSet = u32;
pub const STRS_NONE: StrSet = 0;
pub const STRS_TOP: StrSet = 1;
pub const STRS_SET0: StrSet = 2;
/// Past this many constants a node holds any string. Constant sets are
/// exact (tsc-src holds ~200 option names in one key): interned sets keep
/// the join cheap, priced on tsc-src and three.js (SEED2).
pub const STRS_K: usize = usize::MAX;

/// An interned key shape (`Model::key_shapes`); 0: any string.
pub type KeyShapeId = u32;

impl Key {
    /// A computed key of no known shape.
    pub const fn any() -> Key {
        Key::Any(0)
    }

    pub fn is_any(self) -> bool {
        matches!(self, Key::Any(_))
    }

    /// The key without its shape: where a computed key is looked up as one
    /// key (accessor groups, contracts), every shape is that key.
    pub fn unshaped(self) -> Key {
        match self {
            Key::Any(_) => Key::any(),
            k => k,
        }
    }
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
    /// `new Name(...)` of a declared global constructor.
    New(CallId),
    /// Descriptor metadata, without reading or calling the property.
    Lookup(NodeId),
    /// A store of this node at this site that met a declared getter-only
    /// accessor (`builtins::getter_only`) and wrote nothing: once the key
    /// may be written natively (the accessor replaced), it is stored again.
    Store(NodeId, SiteId),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ChainAct {
    /// Return the installed accessor function, without invoking its body.
    Lookup {
        dst: NodeId,
        setter: bool,
    },
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
    /// The runtime's value formatter (`console.*`, perry-runtime
    /// builtins/formatting.rs and formatting/util_format.rs): the
    /// `[util.inspect.custom]` hook (a class's under
    /// `INSPECT_CUSTOM_METHOD`; an object's symbol-keyed one is a computed
    /// store, which every named key sees), ToPrimitive for `%s`/`%d`/`%i`/
    /// `%f`, labels and an Error's `name`/`message`, `toJSON` for `%j`.
    /// Every value it shows (own properties, elements, entries, what a hook
    /// or `toJSON` returns) is formatted in turn. Getters run only under
    /// `console.dir(x, {getters})`; the reads here run them always.
    Inspect,
    /// Node's `util.inspect(v, { depth: -1 })`, what an argument-type
    /// error's message runs on an object without a named constructor
    /// (lib/internal/errors.js `determineSpecificType`; node 26
    /// lib/internal/util/inspect.js `formatValue` / `formatRaw`): the
    /// `[util.inspect.custom]` hook (what it returns is formatted the same
    /// way), `@@toStringTag`, and for the types it prints before the depth
    /// check an Error's `stack` / `message` / `name` / `errors`, an
    /// ArrayBuffer's `byteLength`, a URL's `href`. Each value read is
    /// formatted the same way and ToString'd. No other key is read: own
    /// values are not shown at depth -1. To name the value it runs
    /// `v instanceof C` for the `constructor` data values on its chain
    /// (`getConstructorName`; `instance_of_reads`).
    ShallowInspect,
    /// StructuredSerialize (HTML 2.7.3), what `structuredClone` runs: a
    /// [[Get]] of every own value (own getters run), each serialized in
    /// turn; it calls nothing else (a function or a Proxy throws).
    Clone,
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
            Protocol::Inspect => &[
                INSPECT_CUSTOM_METHOD,
                WK_TO_PRIMITIVE,
                "valueOf",
                "toString",
                "toJSON",
            ],
            Protocol::ShallowInspect => &[INSPECT_CUSTOM_METHOD],
            Protocol::IterResult | Protocol::OwnKeys | Protocol::Clone => &[],
            Protocol::Any => return None,
        })
    }

    /// The keys whose values are read (and touched again for JSON).
    pub fn reads(self) -> &'static [&'static str] {
        match self {
            Protocol::IterResult => &["done", "value"],
            Protocol::ShallowInspect => &[
                WK_TO_STRING_TAG,
                "stack",
                "message",
                "name",
                "errors",
                "byteLength",
                "href",
            ],
            _ => &[],
        }
    }

    /// The keys whose values on the touched value's chain the touch runs
    /// InstanceofOperator on, the touched value on the left (a user
    /// `@@hasInstance` gets it as its argument). Node's `getConstructorName`
    /// (lib/internal/util/inspect.js) does so for each own data
    /// `constructor` of the value and its prototypes that is a named
    /// function, until one holds; the read here is the `[[Get]]`'s values
    /// on the whole chain, a superset.
    pub fn instance_of_reads(self) -> &'static [&'static str] {
        match self {
            Protocol::ShallowInspect => &["constructor"],
            _ => &[],
        }
    }

    /// The protocols run on each value the touch reads (`reads`, or every
    /// own value under `reads_all`).
    pub fn read_touches(self) -> &'static [Protocol] {
        match self {
            Protocol::Json => &[Protocol::Json],
            Protocol::Inspect => &[Protocol::Inspect],
            Protocol::Clone => &[Protocol::Clone],
            Protocol::ShallowInspect => &[Protocol::ShallowInspect, Protocol::ToPrimitive],
            _ => &[],
        }
    }

    /// Every value read off the object is touched again by the same
    /// protocol (JSON serializes it, the formatter shows it).
    pub fn reads_all(self) -> bool {
        matches!(self, Protocol::Json | Protocol::Inspect | Protocol::Clone)
    }

    /// The use bit of a touch by this protocol (`MARK_TOUCH*`).
    pub fn touch_bit(self) -> u8 {
        match self {
            Protocol::ToPrimitive => MARK_TOUCH_PRIMITIVE,
            Protocol::Iterate => MARK_TOUCH_ITERATE,
            Protocol::IterNext => MARK_TOUCH_ITER_NEXT,
            Protocol::IterResult => MARK_TOUCH_ITER_RESULT,
            Protocol::HasInstance => MARK_TOUCH_HAS_INSTANCE,
            Protocol::Json
            | Protocol::Inspect
            | Protocol::ShallowInspect
            | Protocol::Clone
            | Protocol::OwnKeys
            | Protocol::Any => MARK_TOUCH,
        }
    }

    /// The protocols the touch bits of `uses` name, each once.
    pub fn of_touches(uses: u8) -> impl Iterator<Item = Protocol> {
        [
            Protocol::Any,
            Protocol::ToPrimitive,
            Protocol::Iterate,
            Protocol::IterNext,
            Protocol::IterResult,
            Protocol::HasInstance,
        ]
        .into_iter()
        .filter(move |p| uses & p.touch_bit() != 0)
    }

    /// The protocol run on what the called methods return.
    pub fn of_result(self) -> Option<Protocol> {
        match self {
            Protocol::Iterate => Some(Protocol::IterNext),
            Protocol::IterNext => Some(Protocol::IterResult),
            Protocol::Json => Some(Protocol::Json),
            Protocol::Inspect => Some(Protocol::Inspect),
            Protocol::ShallowInspect => Some(Protocol::ShallowInspect),
            Protocol::Any => Some(Protocol::Any),
            Protocol::ToPrimitive
            | Protocol::IterResult
            | Protocol::HasInstance
            | Protocol::OwnKeys
            | Protocol::Clone => None,
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
            Protocol::Inspect => "inspect",
            Protocol::ShallowInspect => "shallowinspect",
            Protocol::Clone => "clone",
            Protocol::Any => "any",
        }
    }
}

/// ToPropertyDescriptor (ECMA-262 6.2.6.5) reads each of these keys of the
/// descriptor with [[Get]], in this order: a getter on any of them runs,
/// with `this` = the descriptor, whichever keys the definition then uses.
pub const DESCRIPTOR_KEYS: [&str; 6] = [
    "enumerable",
    "configurable",
    "value",
    "writable",
    "get",
    "set",
];

/// `Symbol.toPrimitive` / `Symbol.hasInstance` as HIR folds them
/// (`Gen::read_key`).
pub const WK_TO_PRIMITIVE: &str = "@@__perry_wk_toPrimitive";
pub const WK_HAS_INSTANCE: &str = "@@__perry_wk_hasInstance";
pub const WK_TO_STRING_TAG: &str = "@@__perry_wk_toStringTag";
/// A class's `[util.inspect.custom]()` as HIR names it (perry-hir
/// lower_decl/class_members.rs); the formatter calls it by that name
/// (`class_method_slot_target`, perry-runtime builtins/formatting.rs).
pub const INSPECT_CUSTOM_METHOD: &str = "__perry_inspect_custom__";

mod cons;
pub use cons::*;
pub mod range;
pub use range::{Iv, RangeOp, Rg};

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
    #[cfg(test)]
    pub(crate) packet_owner: Option<crate::summary::packet::Identity>,
    #[cfg(test)]
    pub(crate) packet_interface: Option<(
        crate::summary::packet::Identity,
        crate::summary::packet::Binding,
    )>,
    #[cfg(test)]
    pub(crate) intrinsic_top: Option<String>,
    #[cfg(test)]
    pub(crate) field_origin: Option<crate::summary::packet::FieldOrigin>,
    #[cfg(test)]
    pub(crate) lexical_cell: Option<crate::summary::packet::Cell>,
    pub pts: Pts,
    pub prim: u16,
    /// Refuted own-test evidence, outside the value lattice. Only
    /// OwnTest(Refuted) propagates it; value-copy edges do not.
    pub own_refuted: bool,
    pub delta: crate::delta::Pending,
    pub dprim: u16,
    /// The string constants the node may hold (`StrSet`): `STRS_NONE`
    /// without a string, `STRS_TOP` for any string, else an interned set of
    /// at most `STRS_K` atoms (`Model::str_sets`). Nonzero exactly when
    /// `prim` has `prim::STR`. It only grows.
    pub strs: StrSet,
    /// `strs` grew since the node was last popped.
    pub dstrs: bool,
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
    /// The integer interval of the Numbers it holds (`range::Iv`).
    pub range: range::Iv,
    /// The interval grew since the node was last delivered.
    pub drange: bool,
    /// Growths of `range` so far (widening, `range::WIDEN_STEPS`).
    pub rsteps: u8,
    /// The result node of a `Cons::Range` rule: its interval is the rule's.
    pub ruled: bool,
    /// The code interval of the Strings it holds (`range::CHAR_ANY`: each
    /// is `""` or one code unit); ⊤ says nothing.
    pub crange: range::Iv,
    /// Growths of `crange` so far.
    pub csteps: u8,
    /// The result node of a code-interval rule (`RangeOp::SClip`).
    pub cruled: bool,
    /// The node is an int consumer's operand: an element key, a bitwise
    /// operand (`demand`).
    pub int_use: bool,
    /// One of its values can reach an int consumer (`demand::solve`).
    pub int_demand: bool,
    /// −0 tolerance (`zero_sign`): every use of the node's value is one the
    /// solve sees (an expression the generator classified, a copy, a rule, a
    /// field or element slot, a function's result). A node without it is
    /// used by code the solve does not see, so the sign of a zero it holds
    /// counts as observed.
    pub zs_known: bool,
    /// −0 tolerance: a use observes the sign of a zero the node holds (a
    /// division, `Object.is`, a native argument, any unclassified use).
    pub zs_use: bool,
    /// −0 tolerance: one of its values can reach a use that observes the
    /// sign of a zero (`zero_sign::solve`).
    pub zsign: bool,
    /// −0 tolerance: the node keeps its integer interval although it may
    /// hold `-0`, which no use observes (`zero_sign::solve`).
    pub zs_tol: bool,
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
    /// The evaluated key of an indexed source store. Its solved scalar kind
    /// distinguishes element effects from arbitrary named-property writes.
    pub key_value: Option<NodeId>,
    /// RPG1b: where the store runs in its function, when the walk made it
    /// at a statement of a function body: `Some(true)` inside a loop
    /// (body, condition or update), `Some(false)` not. `None`: a site no
    /// body walk ordered (a birth definition, a replay, a parameter
    /// default, a synthetic store); it is never an ordered writer.
    pub in_loop: Option<bool>,
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
    /// Per declared native (`builtins::effects`): the runs of its
    /// declaration and the source call sites they were for.
    pub natives: FxHashMap<&'static str, (u64, crate::pts::FxHashSet<CallId>)>,
}

impl WorkProfile {
    pub const STEP: u64 = 5_000_000;
}

/// An object literal with spreads: the keys it defines itself, and per
/// spread site the keys every source so far surely holds (`None`: no source
/// yet). A key is sure when the literal defines it or one spread copies it
/// from every source.
#[derive(Default, Clone, Debug)]
pub struct LitSpread {
    pub named: Vec<Atom>,
    pub sites: Vec<(SiteId, Option<Vec<Atom>>)>,
}

/// DECISIONS 241: the join's values of one function's own properties
/// (`Model::fn_joined`).
#[derive(Default, Clone, Debug)]
pub struct FnJoined {
    pub keys: Vec<(Atom, NodeId)>,
    pub any: Option<NodeId>,
}

#[derive(Default)]
pub struct Model {
    #[cfg(test)]
    pub(crate) packet_protocol_depth: usize,
    #[cfg(test)]
    pub(crate) packets_enabled: bool,
    #[cfg(test)]
    pub(crate) packets_inert: bool,
    #[cfg(test)]
    pub(crate) packet_scope: Option<crate::summary::packet::Builder>,
    pub(crate) diagnostic: Option<crate::diagnostics::Trace>,
    pub toks: token_values::TokenValues,
    pub(crate) declarations: crate::builtins::realm_shapes::Declarations,
    pub tok_index: token_values::TokenIndex,
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
    pub work: std::cell::Cell<u64>,
    /// The interval a Number arriving through `add_tokens` carries: the
    /// source's, while a value-copy edge delivers; `None` (⊤) otherwise.
    pub cur_range: Option<range::Rg>,
    /// Test-only negative control (`FactsConfig::sabotage` = `range`): the
    /// `+` interval rule drops its right operand, a wrong transfer the
    /// int32 witnesses must catch.
    pub range_sabotage: bool,
    /// R1: a `Cons::Range` result takes the join of its rule's evaluations
    /// without widening (`solve::grow_rule_range`); only copy targets widen.
    /// `PERRY_FACTS_EXACT_RULES=0` widens a result like any node.
    pub exact_rules: bool,
    /// −0 tolerance (RFC §10.3 as amended, PR D; `zero_sign`): the interval
    /// rules give `0 * negative`, `-x` and `%` their integer hull with the
    /// `-0` bit, and a node may keep an integer interval while it may hold
    /// `-0` when no use observes the sign. `PERRY_FACTS_ZERO_SIGN=0`: the
    /// strict rules (such a result is ⊤).
    pub zero_sign: bool,
    /// −0 tolerance: the builtin running in `with_native` declared its
    /// arguments absorbing (ToString'd / ToIntegerOrInfinity'd only).
    pub zs_absorbing: bool,
    /// −0 tolerance: calls a builtin declared absorbing (`with_native`).
    pub zs_absorbed: crate::pts::FxHashSet<CallId>,
    /// PR D: `"lit".indexOf(..)` / `"lit".lastIndexOf(..)` calls on a String
    /// literal receiver, with the literal's length in code units: the
    /// native's result is in `[-1, len]` (`string_methods`).
    pub str_index_len: FxHashMap<CallId, i64>,
    /// −0 tolerance: spread argument arrays of calls whose callee observes
    /// its arguments: every element of every token they hold is observed.
    pub zs_spread: Vec<NodeId>,
    /// Negative control (`PERRY_FACTS_SABOTAGE=zerosign`): a division's
    /// operands are no sign-observing uses, a wrong classification the
    /// sign witness must catch.
    pub zero_sign_sabotage: bool,
    /// Negative control (`PERRY_FACTS_SABOTAGE=exactrule`): with the exact
    /// rule results, a sum is the hull of its operands instead of their
    /// sum, a claim too narrow that the witnesses must catch.
    pub exact_rule_sabotage: bool,
    /// Negative control (`PERRY_FACTS_SABOTAGE=arrlen`): every index store
    /// counts as one inside its array's birth length (`arr_len_index`).
    pub arr_len_sabotage: bool,
    /// PR-C1 (DESIGN-p4l1 §2.8): a computed-key read of a program Array
    /// adds its past-the-end `undefined` through `Cons::ElemUndef`, not
    /// unconditionally (`PERRY_FACTS_ELEM_UNDEF=0` turns it off).
    pub elem_undef: bool,
    /// Negative control (`PERRY_FACTS_SABOTAGE=arrundef`): `ElemUndef`
    /// takes a key that reaches the length's lower bound as below it.
    pub elem_undef_sabotage: bool,
    /// Negative control (`PERRY_FACTS_SABOTAGE=numbercall`): every method
    /// call is a Number call (`ModuleBindings::number_calls`), a wrong
    /// claim the call-result witness must catch.
    pub number_call_sabotage: bool,
    /// N6: bitwise operators over single int32 values and constant left
    /// shifts take their exact interval (`range::eval_exact`), and module
    /// bindings that hold one Number are exported (`const_numbers`;
    /// `PERRY_FACTS_CONST_NUMBERS=0` turns both off).
    pub const_numbers: bool,
    /// Negative control (`PERRY_FACTS_SABOTAGE=constnum`): a module binding
    /// with any bounded Number interval is exported as the constant at its
    /// lower bound (`const_numbers`), a wrong claim the read's witness must
    /// catch.
    pub const_number_sabotage: bool,
    /// Negative control (`PERRY_FACTS_SABOTAGE=arrcap`, also `densebirth`):
    /// `new Array(k)` records its birth length past
    /// `MAX_FRESH_DENSE_ARRAY_LENGTH`, over a store the runtime does not
    /// back densely, a wrong claim the element read's capacity witness must
    /// catch.
    pub arr_cap_sabotage: bool,
    /// The length floor is off (`solve::arr_len`;
    /// `PERRY_FACTS_ARRAY_FLOOR=0`): growth makes an array's length ⊤
    /// instead of raising only its upper bound.
    pub arr_floor_off: bool,
    /// Negative control (`PERRY_FACTS_SABOTAGE=arrfloor`): an event that
    /// may lower an array's length (`pop`, a `length` store, `splice`)
    /// keeps its interval (`arr_len_top`).
    pub arr_floor_sabotage: bool,
    /// Negative control (`PERRY_FACTS_SABOTAGE=arrappend`): an append
    /// raises the length's upper bound by one, once per token, not per
    /// runtime append (`arr_len_store`).
    pub arr_append_sabotage: bool,
    /// Negative control (`PERRY_FACTS_SABOTAGE=arrfwd`): an append keeps the
    /// length interval, so a token that is pushed to keeps `lo == hi`.
    pub arr_fwd_sabotage: bool,
    /// PR-B (DESIGN-p4l1 §2.2-2.6): plain Array births, the fused-fill
    /// birth and `Rep::Arr` (`PERRY_FACTS_ARRAY_REPS=0` turns them off).
    pub array_reps: bool,
    /// Negative control (`PERRY_FACTS_SABOTAGE=arrdense`): every
    /// `new Array(...)` is born hole-free, a wrong claim the `Arr` witness
    /// must catch at an in-floor hole.
    pub arr_dense_sabotage: bool,
    /// Negative control (`PERRY_FACTS_SABOTAGE=arrelem`): an `Arr` value's
    /// element representation counts only its Numbers (`bindings::arr_rep`).
    pub arr_elem_sabotage: bool,
    /// Negative control (`PERRY_FACTS_SABOTAGE=arrlazy`): `JSON.parse`
    /// (`Gen::json_value`) returns a plain Array of Numbers, a wrong claim the witness must
    /// catch at a lazy JSON array header.
    pub arr_lazy_sabotage: bool,
    /// Negative control (`PERRY_FACTS_SABOTAGE=arrfront`): `shift` keeps
    /// the receiver's length interval, so an Array whose first slot was
    /// removed (its elements no longer start at the front of its
    /// allocation) keeps its floor, a wrong claim the element tier's
    /// front-offset witness must catch.
    pub arr_front_sabotage: bool,
    /// Immutable Arrays and constant contents (`const_arrays`;
    /// `PERRY_FACTS_CONST_ARRAYS=0` turns them off).
    pub const_arrays: bool,
    /// Value records (DESIGN-p4l4; `PERRY_FACTS_VALUE_RECORDS=1`): identity
    /// observations are recorded (`Cons::Identity`). Off, the model is the
    /// one without the lever.
    pub value_records: bool,
    /// A record crosses a program call in a `Record` signature slot
    /// (DESIGN-p4l4 §5.2, VR-3b; `PERRY_FACTS_RECORD_SIGNATURES=0` turns it
    /// off: such a call slot then rejects the template, as in VR-3a).
    pub record_signatures: bool,
    /// The admitted value-record templates (`TemplateFacts::record` is
    /// `Ok`), set after the record derive; `bindings::rep_of` reads it.
    pub records: crate::pts::FxHashSet<TmplId>,
    /// Negative control (`PERRY_FACTS_SABOTAGE=arrconst`): a write to an
    /// Array is not recorded (`Model::arr_written`), so a literal that is
    /// stored into keeps its constant contents, a wrong claim the constant
    /// array witness must catch.
    pub arr_const_sabotage: bool,
    /// PR-C3 (DESIGN-p4l1 §3): a field's `Rep::Arr` (`FieldFacts::rep`) and
    /// a field read call's floor (`FieldReadCall::in_floor`);
    /// `PERRY_FACTS_ARRAY_FIELDS=0` turns them off.
    pub array_fields: bool,
    /// Negative control (`PERRY_FACTS_SABOTAGE=arrfield`): a field's
    /// representation ignores the `null` and `undefined` it also holds, a
    /// wrong claim the witness at the field read must catch.
    pub arr_field_sabotage: bool,
    /// Negative control (`PERRY_FACTS_SABOTAGE=restfield`): a Number
    /// field's representation is I32 whatever its interval (I32 at rest,
    /// PR A), a wrong claim the witness at the field read must catch.
    pub rest_field_sabotage: bool,
    /// I32 at rest, PR B (`rest_elems`; `PERRY_REST_ELEMS=0` turns it off).
    pub rest_elems: bool,
    /// The at-rest capacity reservation (`rest_elems::reserve`;
    /// `PERRY_REST_CAPACITY=0` turns it off).
    pub rest_capacity: bool,
    /// Fill births (`fill_births`; `PERRY_ARRAY_FILL_BIRTH=0` turns them off).
    pub fill_births: bool,
    /// I32 at rest, PR C (`rest_fields`; `PERRY_REST_FIELDS=0` turns it off).
    pub rest_fields: bool,
    /// The key signs (`element_keys::collect_signs`;
    /// `PERRY_FACTS_KEY_SIGN=0` turns them off).
    pub key_sign: bool,
    /// Negative control (`PERRY_FACTS_SABOTAGE=keysign`): every int32 key
    /// is claimed non-negative, a wrong claim the witness at the element
    /// site must catch.
    pub key_sign_sabotage: bool,
    /// The wide keys (`element_keys::collect_wide`;
    /// `PERRY_FACTS_WIDE_KEYS=0` turns them off).
    pub wide_keys: bool,
    /// Negative control (`PERRY_FACTS_SABOTAGE=widekey`): a key past int32
    /// that may be negative is claimed non-negative, a wrong claim the
    /// witness at the element site must catch.
    pub wide_key_sabotage: bool,
    /// Negative control (`PERRY_FACTS_SABOTAGE=restlane`): a field whose
    /// rep is F64 is admitted as an I32 lane too (`rest_fields`), a wrong
    /// claim the proven store's witness must catch.
    pub rest_lane_sabotage: bool,
    /// Object tokens a generic reader may touch (`Model::rest_obj_used`):
    /// their class gets no I32 lane.
    pub obj_used: crate::pts::FxHashSet<Tok>,
    /// Negative control (`PERRY_FACTS_SABOTAGE=restelem`): a builtin
    /// method's use of an Array is not recorded (`Model::rest_used`), so an
    /// Array a native writes may be admitted at rest, a wrong claim the
    /// at-rest load witness must catch.
    pub rest_elem_sabotage: bool,
    /// Array tokens a generic reader may touch (`Model::rest_used`): never
    /// at rest.
    pub arr_used: crate::pts::FxHashSet<Tok>,
    /// The fused fill births `new Array(k).fill(v)` (`Gen::call_with`).
    pub(crate) rest_fills: Vec<crate::rest_elems::RestFill>,
    /// The receiver relays `array_method` makes for a fused fill's call.
    pub rest_fill_relays: Vec<NodeId>,
    /// The templates admitted at rest (`rest_elems::admit`), and why each
    /// other candidate was not (`rest_elems::probe`).
    pub rest_tmpls: crate::pts::FxHashSet<TmplId>,
    pub rest_refused: FxHashMap<TmplId, String>,
    pub max_work: u64,
    pub max_nodes: usize,
    pub atoms: Vec<String>,
    pub atom_index: FxHashMap<String, Atom>,
    /// The atoms of class private names (`Model::private_atom`), by their
    /// storage key: a key apart from every public key of that spelling.
    pub private_atoms: FxHashMap<String, Atom>,
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
    /// `Model::hole`: what an Array's iteration reads at a hole.
    pub holes: FxHashMap<Tok, NodeId>,
    /// `Model::arr_len`: the node of an Array token's `length`.
    pub arr_len: FxHashMap<Tok, NodeId>,
    /// The birth length of an array literal template (`solve::arr_len`).
    pub arr_birth_len: FxHashMap<TmplId, i64>,
    /// Array tokens written after their birth (`Model::arr_written`).
    pub arr_stored: crate::pts::FxHashSet<Tok>,
    /// The elements of an array literal template whose elements may all be
    /// constant Numbers (`const_arrays::literal_elems`).
    pub arr_const: FxHashMap<TmplId, Vec<crate::const_arrays::Elem>>,
    /// `new Array(...)` by its call's source site: what gen knows of the
    /// Array its builtin births there (`builtins::site_new_array`).
    pub arr_len_at_site: FxHashMap<SiteId, SiteBirth>,
    /// Store sites of `a[i] = v` whose element stores `Cons::LenIndex`
    /// judges against the birth length (`Model::arr_len_store`).
    pub indexed_store_sites: crate::pts::FxHashSet<SiteId>,
    /// `(site, holder, lo, hi)` index intervals already judged.
    pub len_index_done: crate::pts::FxHashSet<(SiteId, NodeId, i64, i64)>,
    /// `builtins::arr_iterated`: what iterating Array `t` yields.
    pub arr_iterated: FxHashMap<Tok, NodeId>,
    /// `builtins::species_from`: the `constructor` ArraySpeciesCreate
    /// reads off receiver `t`.
    pub species_ctor: FxHashMap<Tok, NodeId>,
    /// `Model::private_field`: what a class private field holds on any
    /// object the class's code finds it on.
    pub private_fields: FxHashMap<Atom, NodeId>,
    /// `Model::num_fields`: the fields a number key may name.
    pub num_fields: FxHashMap<Tok, NodeId>,
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
    /// RPG1b: the functions whose body runs at most once per evaluation of
    /// module `.0`, and only while it evaluates (`gen/once.rs`); `.1`: once
    /// with reload (the runtime's path registry may evaluate the module
    /// again after its `require.cache` entry was deleted).
    pub once_fns: FxHashMap<FnIdx, (u32, bool)>,
    /// RPG1b: each module's static evaluation dependencies (imports that
    /// are not dynamic, type-only, erased or a deferred require): their
    /// evaluation completes before the module's starts, unless they reach
    /// it back (`derive::evaluated_before`).
    pub eval_deps: Vec<Vec<u32>>,
    /// The modules each module reaches through `eval_deps`, as a bit set,
    /// computed when an ordered-writer check first asks (`derive`).
    pub eval_reach: FxHashMap<u32, Vec<u64>>,
    pub adds_keys: FxHashMap<TmplId, u32>,
    /// The part of `adds_keys` that came from a named store of a key the
    /// template did not list as a birth key when the store was seen
    /// (`(template, key) -> stores`). A class whose parent is the value of
    /// another unit has only its own keys in `birth_keys` until the parent
    /// settles (`settle_parent_layouts`); the layout facts count these stores
    /// against the settled birth keys (`derive`), where an inherited birth key
    /// is no add.
    pub adds_named: FxHashMap<(TmplId, Atom), u32>,
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
    pub(crate) pending: crate::delta::EventQueue,
    // dedup of bindings / chain walks
    pub bind_done: crate::pts::FxHashSet<(CallId, Tok, Tok)>,
    pub chain_done: crate::pts::FxHashSet<(NodeId, Tok, Key, ChainAct)>,
    /// (value node, `ProtoNode`) pairs a `ProtoGiven` watches.
    pub given_proto: crate::pts::FxHashSet<(NodeId, NodeId)>,
    pub implicit_done: crate::pts::FxHashSet<(Tok, Protocol)>,
    /// The one argument node of the `@@hasInstance` touch on a token: every
    /// `instanceof` whose right-hand side holds the token adds its
    /// left-hand side (`Solver::has_instance`).
    pub has_instance_arg: crate::pts::FxHashMap<Tok, NodeId>,
    pub construct_done: crate::pts::FxHashSet<(CallId, Tok)>,
    /// (key or prototype node, object, dst or call, rule): `IdxLoad` /
    /// `IdxKey` use 0-18; the `IDX_*` rules of DECISIONS 217 the rest.
    pub idx_done: crate::pts::FxHashSet<(NodeId, Tok, NodeId, u16)>,
    /// OWN1 census (PERRY_FACTS_PROFILE): own-only reads generated, gates
    /// that made one ordinary, object/⊤ keys that ran the full lookup.
    pub own_read_census: [u64; 3],
    pub callthis_done: crate::pts::FxHashSet<(NodeId, CallId, Tok)>,
    /// `CallThis` split (see `Model::bind_via`): `ThisTo` per (method node,
    /// receiver), the callee relay per call, the edges into it, and the
    /// (call, receiver) pairs bound through each method node.
    pub this_done: crate::pts::FxHashSet<(NodeId, Tok)>,
    pub relays: FxHashMap<CallId, NodeId>,
    /// `Cons::ProtoKey`: per `dst`, the node holding the prototypes read
    /// through a receiver that names no native (it loads the key into `dst`).
    pub proto_key_relays: FxHashMap<NodeId, NodeId>,
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
    /// The solve's interned points-to sets (`crate::pts::SetTable`).
    pub sets: crate::pts::SetTable,
    /// Scratch of `add_tokens`: the members new to the node, in arrival
    /// order, and their lookup set once the list is long.
    pub(crate) fresh: Vec<Tok>,
    pub(crate) fresh_seen: crate::pts::FxHashSet<Tok>,
    pub sunk_nodes: crate::pts::FxHashSet<NodeId>,
    /// Classes linked to a parent at gen time whose `extends` expression is
    /// still evaluated (a class in a CommonJS factory, §17.4): the node the
    /// evaluated value reaches. After the solve it must be that parent.
    pub parent_checks: Vec<(TmplId, NodeId)>,
    /// DEC 251 B: classes whose evaluated `extends` value settle could not
    /// link to one class (`settle_parent_values`): no static construction
    /// through a parent is published for them (`super_unrelinked`).
    pub parent_unsettled: crate::pts::FxHashSet<TmplId>,
    pub any_walk_done: crate::pts::FxHashSet<(NodeId, Tok, ChainAct)>,
    pub any_walks: Vec<(NodeId, Tok, Tok, ChainAct)>,
    /// Computed-key walks by holder: every (receiver, act) whose walk
    /// reached that holder (the receiver itself or a prototype in its
    /// closure), so installing an accessor replays only those.
    pub any_walk_holders: FxHashMap<Tok, Vec<(Tok, ChainAct)>>,
    /// Published literals whose own accessors leave to other units
    /// (`leave_walk`): the node what they return loads into.
    pub leave_loads: FxHashMap<Tok, NodeId>,
    /// This unit's literals whose own getters became contract functions
    /// (`Gen::note_own_getters`): only theirs are contract getters.
    pub getter_owners: crate::pts::FxHashSet<Tok>,
    /// Source token of a spread -> its targets (and the spread site): a key
    /// the source gains later is copied too (`Model::field`).
    pub spread_to: FxHashMap<Tok, Vec<(Tok, SiteId)>>,
    pub spread_done: crate::pts::FxHashSet<(Tok, Tok)>,
    /// Object literals with spreads (`{ a, ...s }`): which keys they surely
    /// hold (`LitSpread`), by template, and their spread sites.
    pub lit_spread: FxHashMap<TmplId, LitSpread>,
    pub lit_spread_site: FxHashMap<SiteId, TmplId>,
    /// Reads of a key a fact unit surely holds (they read no `undefined`),
    /// by template: when the key stops being sure (a spread source that
    /// lacks it, a `delete`), each gets `undefined` after all.
    pub present_reads: FxHashMap<TmplId, Vec<(Atom, NodeId)>>,
    pub present_seen: crate::pts::FxHashSet<(TmplId, Atom, NodeId)>,
    /// The `Cons::Defined` node a present read's chain flows through, by
    /// the read's destination.
    pub defined_via: FxHashMap<NodeId, NodeId>,
    /// (chain node, read destination) pairs with their `Cons::Defined`.
    pub defined_done: crate::pts::FxHashSet<(NodeId, NodeId)>,
    /// Fact units whose keys can disappear (delete, defineProperty, with):
    /// no key of theirs is sure.
    pub keys_unsure: crate::pts::FxHashSet<TmplId>,
    /// DECISIONS 234: keys a `delete`, a `defineProperty` or an accessor
    /// through ⊤ or on a builtin prototype may have removed or redefined
    /// (`reshaped_any`: every key). A builtin prototype redefined under a
    /// key may intercept the store that makes an object's own key (a
    /// setter, a read-only value), so the own key no longer shadows it.
    pub reshaped_keys: crate::pts::FxHashSet<Atom>,
    pub reshaped_any: bool,
    /// DECISIONS 234: lookups of an own definite data key that skipped the
    /// builtin members of the chain (`Model::shadows`), by receiver: (key,
    /// the read, method call or passive descriptor query that looked it up,
    /// whether the receiver's own builtin members were skipped too).
    /// Once the key no longer shadows,
    /// the lookup takes them.
    pub shadowed: FxHashMap<Tok, Vec<(Atom, ChainAct, bool)>>,
    /// The receivers of `shadowed` by the key they skipped and by each class
    /// `Model::shadows` consults for them (the template of an object; the
    /// classes a receiver token or marker stands for, also those
    /// `register_virt` adds later). An event that can revoke a shadow names
    /// its key (`reshape`, `top_reshape`) or its classes (`note_inst`,
    /// `flush_present_reads`) and re-checks the lookups filed under it; an
    /// event of every key re-checks all (`flush_shadowed(None)`). Entries
    /// stay once filed (`shadowed_keyed`, `shadowed_classed`: filed once).
    pub shadowed_by_key: FxHashMap<Atom, Vec<Tok>>,
    pub shadowed_by_class: FxHashMap<TmplId, Vec<Tok>>,
    pub shadowed_keyed: crate::pts::FxHashSet<(Tok, Atom)>,
    pub shadowed_classed: crate::pts::FxHashSet<Tok>,
    /// DECISIONS 234: keys a delete or defineProperty through ⊤ may have
    /// removed or redefined (`top_reshaped_any`: every key), in any unit
    /// (`SharedWrite::Hard(Shared::Top, ..)`). ⊤ denotes escaped objects
    /// only (DECISIONS 150): it revokes no other object's key.
    pub top_reshaped_keys: crate::pts::FxHashSet<Atom>,
    pub top_reshaped_any: bool,
    /// Templates and classes some instance of which escaped (`inst_escaped`)
    /// or met a delete, defineProperty or `with` (`inst_lose`): for a
    /// class, an instance of it or of a subclass, or a receiver of it.
    pub inst_escaped: crate::pts::FxHashSet<TmplId>,
    pub inst_lose: crate::pts::FxHashSet<TmplId>,
    /// Calls the runtime makes for a protocol (ToPrimitive, iteration …).
    pub protocol_calls: crate::pts::FxHashSet<CallId>,
    /// Shared prototype-chain value nodes: (token, key) → node.
    /// `true`: with the builtin prototypes' members; `false`: the program
    /// prototypes only (an own key shadows the builtin ones, DECISIONS 234).
    pub cv_nodes: FxHashMap<(Tok, Key, bool), NodeId>,
    /// Prototype closure nodes: token → node.
    pub cp_nodes: FxHashMap<Tok, NodeId>,
    /// Shared chain unions that stopped at a declared member: resumed when
    /// that prototype is mutated.
    pub stopped_shared: FxHashMap<Tok, Vec<(Key, NodeId, bool)>>,
    /// Keys some accessor may be defined under (known at generation): only
    /// lookups of these keys walk the chain per receiver.
    pub accessor_atoms: crate::pts::FxHashSet<Atom>,
    /// One of `accessor_atoms` is a numeric string: a read under a number
    /// key walks accessors (`Model::plain_key`, DECISIONS 217).
    pub numeric_accessor: bool,

    /// The relations dump, if requested: one line per primitive relation.
    pub relations: Option<Vec<String>>,
    /// Relation phase tag: b'G' generation (inputs), b'S' solver rules
    /// (re-derived by the checker), b'B' builtin summaries (axioms).
    pub rel_phase: u8,
    pub stats_cons: u64,
    /// Per-call-site templates (`builtins::site_tmpl_born`) by source site,
    /// `what` and whether the birth is plain (`TmplKind::Arr::plain`): a
    /// plain birth never shares its template with a runtime-born one at the
    /// same site (REVIEW-p4l1 R1).
    pub site_tmpls: FxHashMap<(CallId, &'static str, bool), Tok>,
    /// Generation-time birth templates (literals, arrays, builtin objects)
    /// -> the function whose body makes them.
    pub tmpl_fn: FxHashMap<TmplId, FnIdx>,
    /// Template tokens of literals a summary births per instance: they live
    /// on observation nodes only, so their own nodes observe too.
    pub obs_toks: crate::pts::FxHashSet<Tok>,
    /// Typed-array owner templates born by `TypedArrayNew` with a known
    /// element kind (`perry_hir::TYPED_ARRAY_KIND_*`).
    pub ta_elem: FxHashMap<TmplId, u8>,
    pub ta_min_len: FxHashMap<TmplId, u32>,
    pub ta_exact_len: FxHashMap<TmplId, u32>,
    pub ta_birth_input: FxHashMap<TmplId, NodeId>,
    pub resolvers: FxHashMap<NodeId, Tok>,
    /// The executors of promise capabilities (`builtins::capability_executor`),
    /// by the node of the value their capability is resolved with.
    pub executors: FxHashMap<NodeId, Tok>,
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
    /// The throw region generation is in (the walked body's `FuncInfo::raise`,
    /// a try body's node); `None` outside generation of a body.
    pub gen_region: Option<NodeId>,
    /// The region of calls unknown code makes to escaped functions: what they
    /// throw goes to unknown code (sunk; no catch reads it).
    pub esc_raise: NodeId,
    /// The accessor functions of escaped values (unknown code reads them
    /// with `Object.getOwnPropertyDescriptor`): sunk, no one reads it.
    pub esc_accessors: NodeId,
    /// What a module body run lazily may throw (a deferred CommonJS target,
    /// a factory the path registry re-runs): the region of a call that can
    /// run one (a require through the path registry) holds it.
    pub lazy_raise: NodeId,
    /// The native call being dispatched: a value it hands to unknown code
    /// may run there, throwing into the call's region.
    pub cur_native_call: Option<CallId>,
    /// The class objects unknown code can reach (escaped): a `Reflect.construct`
    /// whose target may be unknown constructs each of them (`Cons::ReflectNew` here).
    pub escaped_classes: NodeId,
    /// E14: interpreted code can run (and patch `Object.prototype`).
    pub dyn_eval: Option<String>,
    /// An async_hooks hook may be enabled (why): its `init` is unknown code
    /// (`createHook` is undeclared) that gets every resource the runtime
    /// makes, as the declarations say (`async_resource`).
    pub async_hooks: Option<String>,
    /// The values declared `async_resource` while no hook may be enabled:
    /// they escape once one may.
    pub async_resources: Vec<(NodeId, &'static str)>,
    /// The dyn-eval interpreter is linked (the driver's link decision): a
    /// solved call or construct that may be the Function constructor then
    /// runs interpreted code (DECISIONS 235).
    pub interp_linked: bool,
    /// Interpreted code may run: its global writes and its catch of thrown
    /// values are in the solve (`builtins::interpreted_code_runs`).
    pub interp_runs: bool,
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
    /// (receiver, member, getter) → the one call a builtin method makes of
    /// that member of the receiver (`collections::recv_call`), or, with
    /// `true`, the one run of the member's declared getter on the receiver
    /// that every load of the key reads (`collections::getter_call`).
    pub recv_calls: FxHashMap<(Tok, Atom, bool), CallId>,
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
    /// This run is one unit of a per-unit solve (`Config::unit`).
    pub unit_mode: bool,
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
    pub fnproto_borrow_write: Option<u32>,
    /// #13645: why iterating an array may not read the builtin
    /// `%Array.prototype%[@@iterator]`: the first write under
    /// `Symbol.iterator`, `__proto__` or a computed key, definition or
    /// prototype change on a value that may be an array (or a prototype on
    /// an Array subclass's chain), or a `Reflect.construct` with a new
    /// target ([`Model::note_array_member_write`]). Writes that may reach a
    /// builtin prototype are in [`Model::native_writes`]
    /// ([`Model::array_iteration_writes`]).
    pub array_member_write: Option<u32>,
    /// #13645: computed-key stores (site, reason) on a value that may be an
    /// array: a write of `@@iterator` or `__proto__` only if the site's key
    /// can hold a string, a symbol or an object once the solve is done
    /// ([`Model::array_iteration_writes`]); a number key is an element.
    pub array_keyed_stores: Vec<(SiteId, u32)>,
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
    /// Rule R1 (`solve/builtin_fields.rs`): the built-in objects holding
    /// fields under keys they do not declare.
    pub r1_holders: crate::pts::FxHashSet<Tok>,
    /// Rule R1 sinks (`builtin_fields_sink`): [namespace holders, of them
    /// holding fields; native function values, of them holding fields].
    pub r1_sinks: [u64; 4],
    /// FVA 4.3b: a for-in body run's copy of its key variable -> the run's one
    /// key (the variable holds nothing else).
    pub key_nodes: FxHashMap<NodeId, Atom>,
    /// Interned computed-key shapes (`model/key_shape.rs`; 0 is none).
    pub key_shapes: Vec<KeyShape>,
    pub key_shape_ids: FxHashMap<KeyShape, KeyShapeId>,
    /// Computed-key sites generated, those with a shape, and those whose
    /// shape excludes `constructor` (the profile's `key shapes` line).
    pub key_shape_census: [u64; 3],
    /// A shaped key's own-field read of an object: the fields whose name the
    /// shape admits and every write under a computed key
    /// (`solve/shaped_keys.rs`).
    pub shaped_reads: FxHashMap<(Tok, KeyShapeId), NodeId>,
    /// A shaped key's store into an object: it reaches only the fields
    /// whose name the shape admits, and computed-key reads.
    pub shaped_stores: FxHashMap<(Tok, KeyShapeId), NodeId>,
    /// The shaped reads and stores of each object, for fields made later.
    pub shaped_of: FxHashMap<Tok, Vec<(KeyShapeId, NodeId, bool)>>,
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
    /// (key node, store site, index) a `StoreIndexed` store has been added
    /// for: `true` the index store, `false` the computed-key one. A shared
    /// body's replay has its own key node.
    pub index_store_done: crate::pts::FxHashSet<(NodeId, SiteId, u8)>,
    /// (key node, store site, atom) a `StoreIndexed` named store has been
    /// added for, by a string constant of the key (`Node::strs`).
    pub str_store_done: crate::pts::FxHashSet<(NodeId, SiteId, Atom)>,
    /// The interned string-constant sets (`StrSet`) and their index.
    pub str_sets: Vec<Box<[Atom]>>,
    pub str_set_index: crate::pts::FxHashMap<Box<[Atom]>, StrSet>,
    /// One source node per string literal atom (`Gen::str_literal`).
    pub str_literals: crate::pts::FxHashMap<Atom, NodeId>,
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
    /// (runtime object or ⊤, prototype node) of each `setPrototypeOf` that
    /// may give a runtime object a new prototype: its lookups may now reach
    /// the keys of that prototype's chain (`settle_native_given`).
    pub native_given: Vec<(Tok, NodeId)>,
    /// (descs, map) defined under any key; descs whose E5 and store under
    /// any key are attached.
    pub define_any: crate::pts::FxHashSet<(NodeId, Tok)>,
    pub define_any_on: crate::pts::FxHashSet<NodeId>,
    /// Accessor descriptors a `DefineDesc` defined by: (descriptor, o, key,
    /// site, reason), checked again once the worklist is empty.
    pub desc_runs: Vec<(Tok, NodeId, Atom, SiteId, u32)>,
    /// (o, key, descriptor) defined as data.
    pub desc_data: crate::pts::FxHashSet<(NodeId, Atom, Tok)>,
    /// (o, key, data): a key defined on `o` (`STAR_KEY`: under a computed
    /// key); a named key has its `DefineKey` attached.
    pub define_on: crate::pts::FxHashSet<(NodeId, Atom, bool)>,
    /// The node a `Cons::Store` runs on, while it runs.
    pub store_at: Option<NodeId>,
    /// A call's constant first argument as a property-key spelling. Value
    /// callees record strings only (`BuiltinNs::Require`). Compiler data.
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
    /// The realm's `navigator` object (namespace `navigator`, its
    /// [[Prototype]] `Navigator.prototype`), born once per model: what
    /// Node's `get navigator` on the global object returns.
    pub navigator: Option<Tok>,
    /// What `require.cache` / `require.extensions` / `require.main` are at
    /// run time: the support module's cache and the extensions the runtime
    /// returned to it (`__perry_runtime.cjsInstall`), the runtime's own
    /// objects otherwise, and the main record or `undefined`.
    pub cjs_cache: NodeId,
    pub cjs_ext: NodeId,
    pub cjs_main: NodeId,
    /// The keys of `process` the realm declares as host arrays of strings
    /// (`argv`, `execArgv`): per key, what the property holds (the one array
    /// the runtime makes before the program runs, an "arraybox", or what the
    /// program stores there) and that array.
    pub process_arrays: Vec<(Atom, NodeId, Tok)>,
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
    /// Another unit's classes this unit gives as a class object's parent:
    /// their constructor runs on a receiver their unit does not see. The
    /// census reports each (`Gen::reports`, `extends <class>`).
    pub relinked_parents: crate::pts::FxHashSet<TmplId>,
    /// Source constructor of a type-specialized instance layout (compiler data).
    pub constructor_origins: FxHashMap<TmplId, TmplId>,
    pub constructor_this_done: crate::pts::FxHashSet<(CallId, TmplId, Tok)>,
    /// Wall-clock budget on generation and solve; when exceeded the pass
    /// stops and the program gets no facts at all (every fact off: the
    /// sound fallback).
    pub budget: Option<(std::time::Instant, std::time::Duration)>,
    pub aborted: bool,
    /// Past the work bound (`Model::past_work_bound`): a call that would
    /// make a new calling context runs the function's shared instance
    /// (`Model::bind_layout`), so the solve reaches a coarser fixpoint.
    pub widened: bool,
    /// The bound widens instead of aborting (whole program only: a unit
    /// past its bound publishes nothing, `compute_act`).
    pub widen: bool,
    /// The widened solve passed `WIDENED_WORK` times the bound: it
    /// stopped and claims nothing (`Model::past_work_bound`).
    pub widened_stop: bool,
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
    /// DECISIONS 231(c): keys the code of this unit stores, with known
    /// values, into an instance or receiver of each published class or a
    /// subclass (`virt_store`), and the node holding those values: the own
    /// field a receiver of the class reads.
    pub virt_stored: FxHashMap<(TmplId, Atom), NodeId>,
    /// Published classes an instance or receiver of which escaped: a store
    /// into ⊤ under a key (any unit's, `top_stored`) may be a store into it.
    pub virt_escaped: crate::pts::FxHashSet<TmplId>,
    /// The keys stored into ⊤ (this unit's stores and the replayed ones);
    /// `None`: under a computed key.
    pub top_stored: crate::pts::FxHashSet<Option<Atom>>,
    /// DECISIONS 231(b): argument markers that stood for receiver markers
    /// only and now stand for another value too: every key may be an own
    /// key of what they denote, as for an opened class (`virt_own`).
    pub virt_tok_open: crate::pts::FxHashSet<Tok>,
    /// The argument markers (no reach) each marker is a member of, for the
    /// receiver classes an argument marker stands for (`virt_member`).
    pub virt_member_of: FxHashMap<Tok, Vec<Tok>>,
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
    /// Per stored node, its value as this unit's own instances hold it
    /// (`Cons::CallerValue`).
    pub caller_value: FxHashMap<NodeId, NodeId>,
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
    /// Foreign classes whose constructor chain may return an object other
    /// than `this` (`ClassContract::ctor_returns`): their `new` is unknown.
    pub foreign_ctor_returns: crate::pts::FxHashSet<TmplId>,
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
    /// DECISIONS 224: contract functions of this unit published to a known
    /// closed callee (DECISIONS 223, `Gen::closed_arg_functions`). Their
    /// contract asks every unit that runs them to publish what it passes
    /// (`FnContract::callable`), as a published method's does (DECISIONS
    /// 148/165): their parameter markers stand beside that join.
    pub callable_pub: crate::pts::FxHashSet<FnIdx>,
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
    /// Per beside argument marker: the guards (`from`, `dst`) it did not
    /// admit (a marker beside its values is no value, DECISIONS 74); their
    /// edges follow if it leaves (75).
    pub beside_guard: FxHashMap<Tok, Vec<(NodeId, NodeId)>>,
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
    /// DECISIONS 241: what this unit's code stored on a function token's
    /// own property (`store_into`), apart from what its fields hold from
    /// the join (`fn_joined`): the unit publishes these
    /// (`ProgramFacts::fn_stores_out`). Not `prototype` (DECISIONS 210(1)).
    pub fn_stored: FxHashMap<(Tok, Atom), NodeId>,
    pub fn_stored_any: FxHashMap<Tok, NodeId>,
    /// DECISIONS 241: per function (this unit's or a stand-in), the values
    /// every unit stored on its own properties (`Published::fn_stores`),
    /// each a node holding their stand-ins: each token of the function
    /// reads them under its key (`field`), its computed key (`field_any`)
    /// and every key (`all_fields`).
    pub fn_joined: FxHashMap<FnIdx, FnJoined>,
    /// DECISIONS 241: function tokens that reached unknown code (not a
    /// publication to another unit, `UNDESCRIBED`): any key of theirs may
    /// hold anything, published as an unknown computed-key store.
    pub fn_props_top: crate::pts::FxHashSet<Tok>,
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
    /// caller's unit, described as the result is. DECISIONS 236: with a
    /// path (parameter `u32::MAX`, path `Some(f)`), `this.f.k(..)` through
    /// the receiver marker's keyed reach: each unit holding a receiver runs
    /// it on what its object holds under `f`.
    pub marker_calls: Vec<(FnIdx, u32, Option<Atom>, Option<Atom>, CallId)>,
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
    /// DECISIONS 242 (contract half): the miss sites (`note_chain_miss`)
    /// met on a marker token during the solve, and, settled after it over
    /// `marker_members` (`settle_marker_ops`), per parameter marker: the
    /// contract publishes them (`reach_keys`), the caller runs them on its
    /// argument (`Cons::Miss`).
    pub marker_reads: FxHashMap<Tok, Vec<Key>>,
    pub param_reads: FxHashMap<Tok, Vec<Key>>,
    /// DECISIONS 242 (contract half): the defineProperty calls (key, `None`
    /// computed; data) met on a marker token, and per parameter marker
    /// after the solve (`settle_marker_ops`): `FnContract::param_defines`.
    pub marker_defines: FxHashMap<Tok, Vec<(Option<Atom>, bool)>>,
    pub param_defines: FxHashMap<Tok, Vec<(Option<Atom>, bool)>>,
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
    /// DECISIONS 225: the function bodies that can run, when the analysis
    /// gates bodies on it (`None`: every body runs).
    pub act: Option<Act>,
}

/// DECISIONS 225: which function bodies can run. A body is active once its
/// module runs it, a transfer of an active body solves to it, or it escapes
/// (unknown code may call anything escaped, DECISIONS 150). The set only
/// grows; generation walks a body once it is active (`Gen::walk_activated`).
#[derive(Default, Clone, Debug)]
pub struct Act {
    /// The active functions, by their stable identity (`Model::act_key`).
    pub on: crate::pts::FxHashSet<(u32, u32, String)>,
    /// Activated since generation last walked.
    pub queue: Vec<FnIdx>,
    /// The set is final (the analysis after the activation pass): nothing
    /// activates, a body outside it is never walked.
    pub fixed: bool,
    /// The bodies the analysis after the activation pass walked (by
    /// `Model::act_key`): what can run, module bodies and static
    /// initializers included (`ProgramFacts::body_reach`).
    pub walked: crate::pts::FxHashSet<(u32, u32, String)>,
    /// Every function the activation pass generated (by `Model::act_key`).
    /// A body outside it is one a later pass minted: it is walked (sound),
    /// the pass never asked whether it can run.
    pub seen: crate::pts::FxHashSet<(u32, u32, String)>,
    /// The negative control (`PERRY_FACTS_SABOTAGE=activation:<path>[+<path>..]`):
    /// the named activation paths left out, so a refusal fixture reached
    /// only through them proves wrongly (two paths that both carry one
    /// transfer, such as a call into a summarized callee and its instance,
    /// are left out together).
    pub sabotage: Option<String>,
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

#[path = "model/key_shape.rs"]
mod key_shape;
#[path = "model/operations.rs"]
mod operations;
pub use key_shape::KeyShape;

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
/// A touch by a protocol whose keys ECMA-262 fixes (`Protocol::keys`):
/// the reader runs that protocol on its value, not `Protocol::Any`
/// (`MARK_TOUCH`: a touch whose keys are any, `Any` and JSON).
pub const MARK_TOUCH_PRIMITIVE: u8 = 8;
pub const MARK_TOUCH_ITERATE: u8 = 16;
pub const MARK_TOUCH_ITER_NEXT: u8 = 32;
pub const MARK_TOUCH_ITER_RESULT: u8 = 64;
pub const MARK_TOUCH_HAS_INSTANCE: u8 = 128;
/// Every touch bit.
pub const MARK_TOUCHES: u8 = MARK_TOUCH
    | MARK_TOUCH_PRIMITIVE
    | MARK_TOUCH_ITERATE
    | MARK_TOUCH_ITER_NEXT
    | MARK_TOUCH_ITER_RESULT
    | MARK_TOUCH_HAS_INSTANCE;

mod token_values;

impl Model {
    pub(crate) fn charge_work(&self, amount: u64) {
        self.work.set(self.work.get() + amount);
    }

    /// The program observes the identity of what `n` holds (DESIGN-p4l4
    /// §2.2). Recorded only with value records on, so the model without the
    /// lever is unchanged.
    pub(crate) fn identity(&mut self, n: NodeId, route: IdentityRoute) {
        if self.value_records && n != NO_NODE {
            self.cons(n, Cons::Identity { route });
        }
    }
}

impl FuncInfo {
    /// The step closure of a transformed generator (`gen_proto`).
    pub fn gen_step(&self) -> bool {
        self.gen_proto.is_some()
    }
}
