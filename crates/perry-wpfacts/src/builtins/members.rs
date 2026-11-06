//! The members of the intrinsic prototypes (ECMA-262, Annex B included):
//! what a builtin object or a primitive reads under a named key before the
//! program writes anything. A name no intrinsic of the family's chain
//! defines is absent (`undefined`; a TypeError when called); a function
//! member is an opaque builtin function value; a data member has its kind.
//! The chain of every family ends in `Object.prototype`.
//!
//! The description decides the lookup only while the key may not have been
//! written onto a builtin prototype (`builtins::intrinsic_written`): a
//! program-written key reads the opaque member, as a native prototype's does
//! (`native_skips`).

use crate::model::*;

/// A member of an intrinsic prototype.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Mem {
    /// A builtin function value.
    Fn,
    /// A data property or accessor whose value is a primitive of these kinds.
    Prim(u16),
    /// The family's constructor (`constructor`).
    Ctor,
    /// `__proto__`: the prototype object itself.
    Proto,
}

/// A family of builtin objects: which intrinsic prototype chain it has.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Family {
    /// A plain object (`Object.prototype` only).
    Object,
    Function,
    Array,
    Str,
    Num,
    Bool,
    Sym,
    BigInt,
    /// Map, Set, WeakMap, WeakSet (one template kind, `Collection`).
    Collection,
    /// A builtin iterator (array/Map/Set/string/regexp iterators, iterator
    /// helpers, generators' `return`/`throw` included).
    Iterator,
    Promise,
    Error,
    /// A container the model fills with primitives or JSON values: the
    /// array (and iterator) protocol over it, as `bobj_method` routes it.
    Box,
    /// `Object(v)`: a wrapper object of any primitive, or `v` itself.
    Wrapper,
    /// A family the model does not describe (Date, RegExp, typed arrays,
    /// URL, timers …): every name is a member, as before.
    Opaque,
}

const OBJECT: &[(&str, Mem)] = &[
    ("constructor", Mem::Ctor),
    ("hasOwnProperty", Mem::Fn),
    ("isPrototypeOf", Mem::Fn),
    ("propertyIsEnumerable", Mem::Fn),
    ("toLocaleString", Mem::Fn),
    ("toString", Mem::Fn),
    ("valueOf", Mem::Fn),
    ("__proto__", Mem::Proto),
    ("__defineGetter__", Mem::Fn),
    ("__defineSetter__", Mem::Fn),
    ("__lookupGetter__", Mem::Fn),
    ("__lookupSetter__", Mem::Fn),
];

const FUNCTION: &[(&str, Mem)] = &[
    ("apply", Mem::Fn),
    ("bind", Mem::Fn),
    ("call", Mem::Fn),
    ("constructor", Mem::Ctor),
    ("length", Mem::Prim(prim::NUM)),
    ("name", Mem::Prim(prim::STR)),
    ("toString", Mem::Fn),
    (WK_HAS_INSTANCE, Mem::Fn),
    // Annex B: throwing accessors on %Function.prototype%.
    ("arguments", Mem::Prim(prim::NULL)),
    ("caller", Mem::Prim(prim::NULL)),
];

const ARRAY: &[(&str, Mem)] = &[
    ("at", Mem::Fn),
    ("concat", Mem::Fn),
    ("constructor", Mem::Ctor),
    ("copyWithin", Mem::Fn),
    ("entries", Mem::Fn),
    ("every", Mem::Fn),
    ("fill", Mem::Fn),
    ("filter", Mem::Fn),
    ("find", Mem::Fn),
    ("findIndex", Mem::Fn),
    ("findLast", Mem::Fn),
    ("findLastIndex", Mem::Fn),
    ("flat", Mem::Fn),
    ("flatMap", Mem::Fn),
    ("forEach", Mem::Fn),
    ("includes", Mem::Fn),
    ("indexOf", Mem::Fn),
    ("join", Mem::Fn),
    ("keys", Mem::Fn),
    ("lastIndexOf", Mem::Fn),
    ("length", Mem::Prim(prim::NUM)),
    ("map", Mem::Fn),
    ("pop", Mem::Fn),
    ("push", Mem::Fn),
    ("reduce", Mem::Fn),
    ("reduceRight", Mem::Fn),
    ("reverse", Mem::Fn),
    ("shift", Mem::Fn),
    ("slice", Mem::Fn),
    ("some", Mem::Fn),
    ("sort", Mem::Fn),
    ("splice", Mem::Fn),
    ("toLocaleString", Mem::Fn),
    ("toReversed", Mem::Fn),
    ("toSorted", Mem::Fn),
    ("toSpliced", Mem::Fn),
    ("toString", Mem::Fn),
    ("unshift", Mem::Fn),
    ("values", Mem::Fn),
    ("with", Mem::Fn),
    (super::WK_ITERATOR, Mem::Fn),
    ("@@__perry_wk_unscopables", Mem::Prim(prim::UNDEF)),
];

const STRING: &[(&str, Mem)] = &[
    ("at", Mem::Fn),
    ("charAt", Mem::Fn),
    ("charCodeAt", Mem::Fn),
    ("codePointAt", Mem::Fn),
    ("concat", Mem::Fn),
    ("constructor", Mem::Ctor),
    ("endsWith", Mem::Fn),
    ("includes", Mem::Fn),
    ("indexOf", Mem::Fn),
    ("isWellFormed", Mem::Fn),
    ("lastIndexOf", Mem::Fn),
    ("length", Mem::Prim(prim::NUM)),
    ("localeCompare", Mem::Fn),
    ("match", Mem::Fn),
    ("matchAll", Mem::Fn),
    ("normalize", Mem::Fn),
    ("padEnd", Mem::Fn),
    ("padStart", Mem::Fn),
    ("repeat", Mem::Fn),
    ("replace", Mem::Fn),
    ("replaceAll", Mem::Fn),
    ("search", Mem::Fn),
    ("slice", Mem::Fn),
    ("split", Mem::Fn),
    ("startsWith", Mem::Fn),
    ("substring", Mem::Fn),
    ("toLocaleLowerCase", Mem::Fn),
    ("toLocaleUpperCase", Mem::Fn),
    ("toLowerCase", Mem::Fn),
    ("toString", Mem::Fn),
    ("toUpperCase", Mem::Fn),
    ("toWellFormed", Mem::Fn),
    ("trim", Mem::Fn),
    ("trimEnd", Mem::Fn),
    ("trimStart", Mem::Fn),
    ("valueOf", Mem::Fn),
    (super::WK_ITERATOR, Mem::Fn),
    // Annex B.
    ("substr", Mem::Fn),
    ("anchor", Mem::Fn),
    ("big", Mem::Fn),
    ("blink", Mem::Fn),
    ("bold", Mem::Fn),
    ("fixed", Mem::Fn),
    ("fontcolor", Mem::Fn),
    ("fontsize", Mem::Fn),
    ("italics", Mem::Fn),
    ("link", Mem::Fn),
    ("small", Mem::Fn),
    ("strike", Mem::Fn),
    ("sub", Mem::Fn),
    ("sup", Mem::Fn),
    ("trimLeft", Mem::Fn),
    ("trimRight", Mem::Fn),
];

const NUMBER: &[(&str, Mem)] = &[
    ("constructor", Mem::Ctor),
    ("toExponential", Mem::Fn),
    ("toFixed", Mem::Fn),
    ("toLocaleString", Mem::Fn),
    ("toPrecision", Mem::Fn),
    ("toString", Mem::Fn),
    ("valueOf", Mem::Fn),
];

const BOOLEAN: &[(&str, Mem)] = &[
    ("constructor", Mem::Ctor),
    ("toString", Mem::Fn),
    ("valueOf", Mem::Fn),
];

const SYMBOL: &[(&str, Mem)] = &[
    ("constructor", Mem::Ctor),
    ("description", Mem::Prim(prim::STR | prim::UNDEF)),
    ("toString", Mem::Fn),
    ("valueOf", Mem::Fn),
    (WK_TO_PRIMITIVE, Mem::Fn),
    ("@@__perry_wk_toStringTag", Mem::Prim(prim::STR)),
];

const BIGINT: &[(&str, Mem)] = &[
    ("constructor", Mem::Ctor),
    ("toLocaleString", Mem::Fn),
    ("toString", Mem::Fn),
    ("valueOf", Mem::Fn),
    ("@@__perry_wk_toStringTag", Mem::Prim(prim::STR)),
];

/// Map, Set, WeakMap and WeakSet together (`Collection` is one kind).
const COLLECTION: &[(&str, Mem)] = &[
    ("add", Mem::Fn),
    ("clear", Mem::Fn),
    ("constructor", Mem::Ctor),
    ("delete", Mem::Fn),
    ("difference", Mem::Fn),
    ("entries", Mem::Fn),
    ("forEach", Mem::Fn),
    ("get", Mem::Fn),
    ("has", Mem::Fn),
    ("intersection", Mem::Fn),
    ("isDisjointFrom", Mem::Fn),
    ("isSubsetOf", Mem::Fn),
    ("isSupersetOf", Mem::Fn),
    ("keys", Mem::Fn),
    ("set", Mem::Fn),
    ("size", Mem::Prim(prim::NUM)),
    ("symmetricDifference", Mem::Fn),
    ("union", Mem::Fn),
    ("values", Mem::Fn),
    (super::WK_ITERATOR, Mem::Fn),
    ("@@__perry_wk_toStringTag", Mem::Prim(prim::STR)),
];

/// %IteratorPrototype%, the iterator helpers, and the concrete builtin
/// iterators' `next` (generators add `return`/`throw`).
const ITERATOR: &[(&str, Mem)] = &[
    ("constructor", Mem::Ctor),
    ("drop", Mem::Fn),
    ("every", Mem::Fn),
    ("filter", Mem::Fn),
    ("find", Mem::Fn),
    ("flatMap", Mem::Fn),
    ("forEach", Mem::Fn),
    ("map", Mem::Fn),
    ("next", Mem::Fn),
    ("reduce", Mem::Fn),
    ("return", Mem::Fn),
    ("some", Mem::Fn),
    ("take", Mem::Fn),
    ("throw", Mem::Fn),
    ("toArray", Mem::Fn),
    (super::WK_ITERATOR, Mem::Fn),
    ("@@__perry_wk_dispose", Mem::Fn),
    ("@@__perry_wk_toStringTag", Mem::Prim(prim::STR)),
];

const PROMISE: &[(&str, Mem)] = &[
    ("catch", Mem::Fn),
    ("constructor", Mem::Ctor),
    ("finally", Mem::Fn),
    ("then", Mem::Fn),
    ("@@__perry_wk_toStringTag", Mem::Prim(prim::STR)),
];

const ERROR: &[(&str, Mem)] = &[
    ("constructor", Mem::Ctor),
    ("message", Mem::Prim(prim::STR)),
    ("name", Mem::Prim(prim::STR)),
    ("toString", Mem::Fn),
];

fn find(set: &[(&str, Mem)], name: &str) -> Option<Mem> {
    set.iter().find(|(n, _)| *n == name).map(|&(_, m)| m)
}

/// The member `name` of the family's own intrinsic prototype (before
/// `Object.prototype`).
pub(super) fn own(fam: Family, name: &str) -> Option<Mem> {
    match fam {
        Family::Object => None,
        Family::Function => find(FUNCTION, name),
        Family::Array => find(ARRAY, name),
        Family::Str => find(STRING, name),
        Family::Num => find(NUMBER, name),
        Family::Bool => find(BOOLEAN, name),
        Family::Sym => find(SYMBOL, name),
        Family::BigInt => find(BIGINT, name),
        Family::Collection => find(COLLECTION, name),
        Family::Iterator => find(ITERATOR, name),
        Family::Promise => find(PROMISE, name),
        Family::Error => find(ERROR, name),
        Family::Box => find(ARRAY, name).or_else(|| find(ITERATOR, name)),
        Family::Wrapper => [STRING, NUMBER, BOOLEAN, SYMBOL, BIGINT]
            .iter()
            .find_map(|s| find(s, name)),
        Family::Opaque => Some(Mem::Fn),
    }
}

/// The member `name` on the family's chain: its own intrinsic prototype,
/// then `Object.prototype`. `None`: no intrinsic of the chain defines it.
pub(super) fn member(fam: Family, name: &str) -> Option<Mem> {
    own(fam, name).or_else(|| find(OBJECT, name))
}

/// The families of a primitive mask (`null`/`undefined` have none).
pub(super) fn prim_families(p: u16) -> impl Iterator<Item = Family> {
    [
        (prim::STR, Family::Str),
        (prim::NUM, Family::Num),
        (prim::BOOL, Family::Bool),
        (prim::SYMBOL, Family::Sym),
        (prim::BIGINT, Family::BigInt),
    ]
    .into_iter()
    .filter(move |(b, _)| p & b != 0)
    .map(|(_, f)| f)
}

/// The member `name` on the chain of any kind in `p`: a function on one
/// kind's chain decides (a call of it may run), else a data member's kinds.
pub(super) fn prim_member(p: u16, name: &str) -> Option<Mem> {
    let mut out = None;
    for fam in prim_families(p) {
        match (member(fam, name), out) {
            (Some(Mem::Fn), _) => return Some(Mem::Fn),
            (Some(m), None) => out = Some(m),
            (Some(Mem::Prim(a)), Some(Mem::Prim(b))) => out = Some(Mem::Prim(a | b)),
            _ => {}
        }
    }
    out
}

/// The keys of each collection kind's own prototype (ECMA-262 24.1-24.4,
/// the Set methods of ES2025): `COLLECTION` is their union, which the
/// solve's lookups take (a template knows its kind, a lookup through
/// `ObjectProto` does not).
const MAP_KEYS: &[&str] = &[
    "clear",
    "constructor",
    "delete",
    "entries",
    "forEach",
    "get",
    "has",
    "keys",
    "set",
    "size",
    "values",
    super::WK_ITERATOR,
    "@@__perry_wk_toStringTag",
];
const SET_KEYS: &[&str] = &[
    "add",
    "clear",
    "constructor",
    "delete",
    "difference",
    "entries",
    "forEach",
    "has",
    "intersection",
    "isDisjointFrom",
    "isSubsetOf",
    "isSupersetOf",
    "keys",
    "size",
    "symmetricDifference",
    "union",
    "values",
    super::WK_ITERATOR,
    "@@__perry_wk_toStringTag",
];
const WEAKMAP_KEYS: &[&str] = &[
    "constructor",
    "delete",
    "get",
    "has",
    "set",
    "@@__perry_wk_toStringTag",
];
const WEAKSET_KEYS: &[&str] = &[
    "add",
    "constructor",
    "delete",
    "has",
    "@@__perry_wk_toStringTag",
];

/// The member `name` on the chain of a collection of kind `what` (`Map`,
/// `Set`, `WeakMap`, `WeakSet`): its own prototype's, then
/// `Object.prototype`'s. `None`: absent, or no collection kind.
pub(super) fn collection_member(what: &str, name: &str) -> Option<Mem> {
    let keys = match what {
        "Map" => MAP_KEYS,
        "Set" => SET_KEYS,
        "WeakMap" => WEAKMAP_KEYS,
        "WeakSet" => WEAKSET_KEYS,
        _ => return None,
    };
    if keys.contains(&name) {
        return find(COLLECTION, name);
    }
    // Another kind's member is absent here; `Object.prototype` is on every chain.
    match find(COLLECTION, name) {
        Some(_) => None,
        None => find(OBJECT, name),
    }
}

/// [`locked_read`]'s `what` for a `Function`-family value whose chain is
/// `Function.prototype` only: a parentless class constructor (a class is
/// never an async or generator function), or a function the prototype-kind
/// predicate (`perry_hir::fn_proto::FnProtoKind::Function`) says is born
/// with `%Function.prototype%` (a plain function, arrow, method, accessor).
pub(super) const FN_PROTO_ONLY: &str = "Function.prototype chain";

/// DEC 250: is a read of `name` on the intrinsic chain of a value of
/// family `fam` (a collection of kind `what`) final, so that no write the
/// program can make (compiled, interpreted or unknown code alike) changes
/// what it finds? The runtime refuses (TypeError at the store) to replace,
/// delete or redefine a member the image gives an intrinsic prototype
/// (`builtin_proto::locked_family`, except `constructor`) and to add a key
/// to `Object.prototype` (`add_locked`). So the read is final when the key
/// is not `constructor` and either an own member of the family's own
/// prototype, that prototype being one of the locked families, or the
/// lookup passes no prototype but `Object.prototype` (a plain object: its
/// members and its absent keys alike). Not final where a family stands for
/// prototypes the runtime leaves open: a function's chain may pass the
/// async/generator function prototypes or a bound target's, the iterator
/// family the Map/Set/String/RegExp iterators' and generators', the error
/// family the `TypeError.prototype`s, `Object(v)` may be `v`, and the
/// undescribed families have no member list here. A key the lookup finds
/// past an add-open prototype (`hasOwnProperty` of an array) may be added
/// there and is not final either.
///
/// The add lock (`add_locked`: `Object.prototype`, `Function.prototype`)
/// makes every key final on a chain that passes no other prototype: a
/// plain object's under any key, a number key (`NUM_KEY`) and a computed
/// one (`STAR_KEY`, the `constructor` it may name aside) included; a
/// function's whose chain is `Function.prototype` only (`FN_PROTO_ONLY`:
/// a parentless class constructor, a plain function, arrow or method; not
/// an async or generator function, whose own intrinsic prototype takes
/// adds). An array's chain refuses index adds on both of its prototypes
/// (`refuse_index_add`): a number key is final there, a named absent key
/// is not (`Array.prototype` takes named adds).
pub(super) fn locked_read(fam: Family, what: Option<&str>, name: &str) -> bool {
    use perry_abi::lock::{final_on, LockHolder, LockKey};
    use perry_abi::BuiltinProto as P;
    // The family's own prototype (if the lock holds it) and whether `name`
    // is one of its image members; `Object.prototype` follows on every chain.
    let own_proto = match fam {
        Family::Object => None,
        Family::Function if what == Some(FN_PROTO_ONLY) => {
            Some((P::Function, own(fam, name).is_some()))
        }
        Family::Array => Some((P::Array, own(fam, name).is_some())),
        Family::Str => Some((P::String, own(fam, name).is_some())),
        Family::Num => Some((P::Number, own(fam, name).is_some())),
        Family::Bool => Some((P::Boolean, own(fam, name).is_some())),
        Family::Sym => Some((P::Symbol, own(fam, name).is_some())),
        Family::BigInt => Some((P::BigInt, own(fam, name).is_some())),
        Family::Promise => Some((P::Promise, own(fam, name).is_some())),
        Family::Collection => {
            let (proto, keys) = match what {
                Some("Map") => (P::Map, MAP_KEYS),
                Some("Set") => (P::Set, SET_KEYS),
                Some("WeakMap") => (P::WeakMap, WEAKMAP_KEYS),
                Some("WeakSet") => (P::WeakSet, WEAKSET_KEYS),
                _ => return false,
            };
            Some((
                proto,
                keys.contains(&name) && find(COLLECTION, name).is_some(),
            ))
        }
        Family::Function
        | Family::Iterator
        | Family::Error
        | Family::Box
        | Family::Wrapper
        | Family::Opaque => return false,
    };
    let class = |image: bool| {
        if name == "constructor" {
            LockKey::Constructor
        } else if image {
            LockKey::Image
        } else if name == crate::model::NUM_KEY {
            LockKey::Index
        } else {
            LockKey::Absent
        }
    };
    // The lock's one decision (`perry_abi::lock`, the runtime's too) on each
    // prototype the lookup passes: an image member ends it, an absent key
    // goes on to the next prototype.
    if let Some((proto, image)) = own_proto {
        let key = class(image);
        if !final_on(LockHolder::Proto(proto), key) {
            return false;
        }
        if key == LockKey::Image {
            return true;
        }
    }
    final_on(
        LockHolder::Proto(P::Object),
        class(find(OBJECT, name).is_some()),
    )
}

/// `derive`'s NativeMember lookup: the member `name` on the chain of every
/// value of family `fam` (an exact Array or a primitive String).
pub(super) fn intrinsic_member(fam: Family, name: &str) -> Option<Mem> {
    match fam {
        Family::Array | Family::Str => member(fam, name),
        _ => None,
    }
}

/// The family of a builtin object template kind (`TmplKind::BObj::what`).
pub(super) fn bobj_family(what: &str) -> Family {
    match what {
        "Map" | "Set" | "WeakMap" | "WeakSet" => Family::Collection,
        "Iterator" => Family::Iterator,
        "Promise" => Family::Promise,
        "Error" => Family::Error,
        "IterResult" | "settledRecord" | "CjsParent" | "require.cache" | "require.extensions" => {
            Family::Object
        }
        "primbox" | "arraybox" | "keys" | "json" | "descriptor" | "descriptors" | "groups"
        | "fromEntries" => Family::Box,
        "Object()" | "Object(prim)" | "BoxedPrimitive" => Family::Wrapper,
        _ => Family::Opaque,
    }
}

/// A canonical array index (`"0"`, `"17"`): an element, not a member.
pub(super) fn is_index_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 10
        && name.bytes().all(|b| b.is_ascii_digit())
        && (name == "0" || !name.starts_with('0'))
}

const FN: Mem = Mem::Fn;
const N: Mem = Mem::Prim(prim::NUM);
const SY: Mem = Mem::Prim(prim::SYMBOL);

/// A namespace's described [[Prototype]], distinct from the broader
/// function family (which also includes async, generator and bound chains).
#[derive(Clone, Copy)]
pub(super) enum NsProto {
    Function,
    Object,
    Constructor(&'static NsStatics),
    /// An undescribed chain; the family only preserves generic call summaries.
    Unknown(Family),
}

#[derive(Clone, Copy)]
pub(super) struct NsStatics {
    pub(super) members: Option<&'static [(&'static str, Mem)]>,
    pub(super) prototype: NsProto,
}

// Error tokens include native subclasses as well as Error itself. Their
// possible parent constructor is the mutable Error namespace, whose own
// [[Prototype]] is exactly Function.prototype.
const ERROR_NAMESPACE: NsStatics = NsStatics {
    members: Some(&[
        ("prototype", Mem::Proto),
        ("captureStackTrace", FN),
        ("isError", FN),
        ("prepareStackTrace", FN),
        ("stackTraceLimit", N),
    ]),
    prototype: NsProto::Function,
};

// The concrete typed-array constructors inherit from %TypedArray%, whose
// own members are undescribed here. That constructor is not a locked prototype.
const TYPED_ARRAY_NAMESPACE: NsStatics = NsStatics {
    members: None,
    prototype: NsProto::Function,
};

/// The own properties and [[Prototype]] of a builtin namespace. Missing
/// own descriptions remain opaque even when a structural parent is known.
pub(super) fn ns_statics(ns: BuiltinNs) -> NsStatics {
    let (prototype, members): (NsProto, Option<&'static [(&'static str, Mem)]>) = match ns {
        BuiltinNs::Object => (
            NsProto::Function,
            Some(&[
                ("prototype", Mem::Proto),
                ("assign", FN),
                ("create", FN),
                ("defineProperties", FN),
                ("defineProperty", FN),
                ("entries", FN),
                ("freeze", FN),
                ("fromEntries", FN),
                ("getOwnPropertyDescriptor", FN),
                ("getOwnPropertyDescriptors", FN),
                ("getOwnPropertyNames", FN),
                ("getOwnPropertySymbols", FN),
                ("getPrototypeOf", FN),
                ("groupBy", FN),
                ("hasOwn", FN),
                ("is", FN),
                ("isExtensible", FN),
                ("isFrozen", FN),
                ("isSealed", FN),
                ("keys", FN),
                ("preventExtensions", FN),
                ("seal", FN),
                ("setPrototypeOf", FN),
                ("values", FN),
            ]),
        ),
        BuiltinNs::Reflect => (
            NsProto::Object,
            Some(&[
                ("apply", FN),
                ("construct", FN),
                ("defineProperty", FN),
                ("deleteProperty", FN),
                ("get", FN),
                ("getOwnPropertyDescriptor", FN),
                ("getPrototypeOf", FN),
                ("has", FN),
                ("isExtensible", FN),
                ("ownKeys", FN),
                ("preventExtensions", FN),
                ("set", FN),
                ("setPrototypeOf", FN),
                ("@@__perry_wk_toStringTag", Mem::Prim(prim::STR)),
            ]),
        ),
        BuiltinNs::Array => (
            NsProto::Function,
            Some(&[
                ("prototype", Mem::Proto),
                ("from", FN),
                ("fromAsync", FN),
                ("isArray", FN),
                ("of", FN),
                ("@@__perry_wk_species", Mem::Ctor),
            ]),
        ),
        BuiltinNs::Json => (
            NsProto::Object,
            Some(&[
                ("isRawJSON", FN),
                ("parse", FN),
                ("rawJSON", FN),
                ("stringify", FN),
                ("@@__perry_wk_toStringTag", Mem::Prim(prim::STR)),
            ]),
        ),
        BuiltinNs::Math => (
            NsProto::Object,
            Some(&[
                ("E", N),
                ("LN10", N),
                ("LN2", N),
                ("LOG10E", N),
                ("LOG2E", N),
                ("PI", N),
                ("SQRT1_2", N),
                ("SQRT2", N),
                ("abs", FN),
                ("acos", FN),
                ("acosh", FN),
                ("asin", FN),
                ("asinh", FN),
                ("atan", FN),
                ("atan2", FN),
                ("atanh", FN),
                ("cbrt", FN),
                ("ceil", FN),
                ("clz32", FN),
                ("cos", FN),
                ("cosh", FN),
                ("exp", FN),
                ("expm1", FN),
                ("f16round", FN),
                ("floor", FN),
                ("fround", FN),
                ("hypot", FN),
                ("imul", FN),
                ("log", FN),
                ("log10", FN),
                ("log1p", FN),
                ("log2", FN),
                ("max", FN),
                ("min", FN),
                ("pow", FN),
                ("random", FN),
                ("round", FN),
                ("sign", FN),
                ("sin", FN),
                ("sinh", FN),
                ("sqrt", FN),
                ("sumPrecise", FN),
                ("tan", FN),
                ("tanh", FN),
                ("trunc", FN),
                ("@@__perry_wk_toStringTag", Mem::Prim(prim::STR)),
            ]),
        ),
        BuiltinNs::Promise => (
            NsProto::Function,
            Some(&[
                ("prototype", Mem::Proto),
                ("all", FN),
                ("allSettled", FN),
                ("any", FN),
                ("race", FN),
                ("reject", FN),
                ("resolve", FN),
                ("try", FN),
                ("withResolvers", FN),
                ("@@__perry_wk_species", Mem::Ctor),
            ]),
        ),
        // Error and its native subclasses; V8's `captureStackTrace` and
        // `stackTraceLimit` (node), `prepareStackTrace` (absent until set:
        // a program write is a store, `intrinsic_written`).
        // `AggregateError` is an `Error` subclass: the same statics apply.
        BuiltinNs::Error | BuiltinNs::AggregateError => (
            NsProto::Constructor(&ERROR_NAMESPACE),
            Some(&[
                ("prototype", Mem::Proto),
                ("captureStackTrace", FN),
                ("isError", FN),
                ("stackTraceLimit", N),
            ]),
        ),
        // The model does not describe these namespaces' own members. They are
        // constructor tokens only, for the `new`/call summaries; naming them
        // here would claim a precision the model does not have.
        BuiltinNs::Date | BuiltinNs::RegExp | BuiltinNs::ArrayBuffer => (NsProto::Function, None),
        BuiltinNs::TypedArray => (NsProto::Constructor(&TYPED_ARRAY_NAMESPACE), None),
        BuiltinNs::Symbol => (
            NsProto::Function,
            Some(&[
                ("prototype", Mem::Proto),
                ("asyncDispose", SY),
                ("asyncIterator", SY),
                ("dispose", SY),
                ("for", FN),
                ("hasInstance", SY),
                ("isConcatSpreadable", SY),
                ("iterator", SY),
                ("keyFor", FN),
                ("match", SY),
                ("matchAll", SY),
                ("replace", SY),
                ("search", SY),
                ("species", SY),
                ("split", SY),
                ("toPrimitive", SY),
                ("toStringTag", SY),
                ("unscopables", SY),
            ]),
        ),
        // Number and BigInt (one namespace kind).
        BuiltinNs::Number => (
            NsProto::Function,
            Some(&[
                ("prototype", Mem::Proto),
                ("EPSILON", N),
                ("MAX_SAFE_INTEGER", N),
                ("MAX_VALUE", N),
                ("MIN_SAFE_INTEGER", N),
                ("MIN_VALUE", N),
                ("NEGATIVE_INFINITY", N),
                ("NaN", N),
                ("POSITIVE_INFINITY", N),
                ("asIntN", FN),
                ("asUintN", FN),
                ("isFinite", FN),
                ("isInteger", FN),
                ("isNaN", FN),
                ("isSafeInteger", FN),
                ("parseFloat", FN),
                ("parseInt", FN),
            ]),
        ),
        BuiltinNs::String => (
            NsProto::Function,
            Some(&[
                ("prototype", Mem::Proto),
                ("fromCharCode", FN),
                ("fromCodePoint", FN),
                ("raw", FN),
            ]),
        ),
        BuiltinNs::Boolean | BuiltinNs::Function => {
            (NsProto::Function, Some(&[("prototype", Mem::Proto)]))
        }
        BuiltinNs::ObjectProtoMethod | BuiltinNs::ObjectProtoOwn(_) => {
            (NsProto::Function, Some(&[]))
        }
        BuiltinNs::Map => (
            NsProto::Function,
            Some(&[
                ("prototype", Mem::Proto),
                ("groupBy", FN),
                ("@@__perry_wk_species", Mem::Ctor),
            ]),
        ),
        BuiltinNs::Set => (
            NsProto::Function,
            Some(&[
                ("prototype", Mem::Proto),
                ("@@__perry_wk_species", Mem::Ctor),
            ]),
        ),
        // No statics, no `@@species`.
        BuiltinNs::WeakMap | BuiltinNs::WeakSet => {
            (NsProto::Function, Some(&[("prototype", Mem::Proto)]))
        }
        BuiltinNs::Console => (NsProto::Unknown(Family::Object), Some(CONSOLE)),
        BuiltinNs::Stdout | BuiltinNs::Stderr => (NsProto::Unknown(Family::Object), Some(STDOUT)),
        BuiltinNs::Stdin => (NsProto::Unknown(Family::Object), Some(STDIN)),
        BuiltinNs::GlobalThis | BuiltinNs::Process => (NsProto::Object, None),
        BuiltinNs::Require | BuiltinNs::Other => (NsProto::Unknown(Family::Function), None),
    };
    NsStatics { members, prototype }
}

/// The members of `console` the runtime gives every program (its methods).
const CONSOLE: &[(&str, Mem)] = &[
    ("assert", FN),
    ("count", FN),
    ("countReset", FN),
    ("debug", FN),
    ("dir", FN),
    ("dirxml", FN),
    ("error", FN),
    ("group", FN),
    ("groupCollapsed", FN),
    ("groupEnd", FN),
    ("info", FN),
    ("log", FN),
    ("table", FN),
    ("time", FN),
    ("timeEnd", FN),
    ("timeLog", FN),
    ("trace", FN),
    ("warn", FN),
];

/// The own members of `process.stdout` / `process.stderr` in both of the
/// runtime's layouts (`build_stream_object_with_write`: the TTY write stream
/// and the plain one). Its other keys depend on the layout or its chain: they
/// are not described.
const STDOUT: &[(&str, Mem)] = &[
    ("addListener", FN),
    ("emit", FN),
    ("fd", N),
    ("off", FN),
    ("on", FN),
    ("once", FN),
    ("removeAllListeners", FN),
    ("removeListener", FN),
    ("write", FN),
];

/// The own members of `process.stdin` in both of the runtime's layouts
/// (the TTY read stream and the plain one).
const STDIN: &[(&str, Mem)] = &[
    ("addListener", FN),
    ("destroy", FN),
    ("emit", FN),
    ("fd", N),
    ("isPaused", FN),
    ("listeners", FN),
    ("off", FN),
    ("on", FN),
    ("once", FN),
    ("pause", FN),
    ("read", FN),
    ("ref", FN),
    ("removeAllListeners", FN),
    ("removeListener", FN),
    ("resume", FN),
    ("setEncoding", FN),
    ("unref", FN),
    ("write", FN),
];

/// A host object of perry's runtime (`console`, the stdio streams): only its
/// own members are described ([`ns_statics`]); a key it does not list may be
/// on a chain the model does not describe, so it is no absent key.
pub(super) fn is_host(ns: BuiltinNs) -> bool {
    matches!(
        ns,
        BuiltinNs::Console | BuiltinNs::Stdout | BuiltinNs::Stderr | BuiltinNs::Stdin
    )
}

/// The member `name` of a builtin namespace object: its own statics, then
/// its chain: `Function.prototype` for a constructor, `Object.prototype`
/// for a namespace object (`Math`, `JSON`, `Reflect`). Outer `None`: the
/// namespace is not described (every name may be a member).
pub(super) fn ns_member(ns: BuiltinNs, name: &str) -> Option<Option<Mem>> {
    described_member(ns_statics(ns), name)
}

fn described_member(description: NsStatics, name: &str) -> Option<Option<Mem>> {
    let own = description.members?;
    if let Some(mem) = find(own, name) {
        return Some(Some(mem));
    }
    match description.prototype {
        NsProto::Function => Some(member(Family::Function, name)),
        NsProto::Object => Some(member(Family::Object, name)),
        NsProto::Constructor(parent) => described_member(*parent, name),
        NsProto::Unknown(_) => None,
    }
}

/// The broad intrinsic family at the tail of the described namespace chain.
/// This summary does not assert that intervening constructors are locked.
pub(super) fn ns_chain(ns: BuiltinNs) -> Family {
    let mut prototype = ns_statics(ns).prototype;
    loop {
        match prototype {
            NsProto::Function => return Family::Function,
            NsProto::Object => return Family::Object,
            NsProto::Constructor(parent) => prototype = parent.prototype,
            NsProto::Unknown(family) => return family,
        }
    }
}

/// What a key that may name any member reads on the family's chain
/// (`Object.prototype` included): the kinds of its data members, and
/// whether its own prototype has a function member (`Object.prototype`'s
/// methods, its `constructor` and `__proto__` are on every chain).
pub(super) fn any_member(fam: Family) -> (bool, u16) {
    let own: &[(&str, Mem)] = match fam {
        Family::Object => &[],
        Family::Function => FUNCTION,
        Family::Array => ARRAY,
        Family::Str => STRING,
        Family::Num => NUMBER,
        Family::Bool => BOOLEAN,
        Family::Sym => SYMBOL,
        Family::BigInt => BIGINT,
        Family::Collection => COLLECTION,
        Family::Iterator => ITERATOR,
        Family::Promise => PROMISE,
        Family::Error => ERROR,
        Family::Box | Family::Wrapper | Family::Opaque => return (true, prim::ALL),
    };
    let prims = own.iter().fold(0, |p, (_, m)| match m {
        Mem::Prim(k) => p | k,
        _ => p,
    });
    (own.iter().any(|(_, m)| *m == Mem::Fn), prims)
}

/// Emit the description itself for the compiled-runtime behaviour check.
/// The harness compares observed own keys against these same member tables.
#[test]
fn runtime_member_description() {
    fn keys(d: NsStatics, out: &mut Vec<&'static str>) -> bool {
        let Some(own) = d.members else { return false };
        out.extend(own.iter().map(|(key, _)| *key));
        match d.prototype {
            NsProto::Function => {
                out.extend(FUNCTION.iter().map(|(key, _)| *key));
                out.extend(OBJECT.iter().map(|(key, _)| *key));
            }
            NsProto::Object => out.extend(OBJECT.iter().map(|(key, _)| *key)),
            NsProto::Constructor(parent) => return keys(*parent, out),
            NsProto::Unknown(_) => return false,
        }
        true
    }
    let mut rows = 0;
    for e in crate::native_effects::realm() {
        let Some(ns) = super::ns_by_name(e.name) else {
            continue;
        };
        let mut names = Vec::new();
        if !keys(ns_statics(ns), &mut names) {
            continue;
        }
        names.sort_unstable();
        names.dedup();
        println!("DESCRIPTION\t{}\t{}", e.name, names.join("\t"));
        rows += 1;
    }
    let mut names: Vec<_> = FUNCTION.iter().chain(OBJECT).map(|(key, _)| *key).collect();
    names.sort_unstable();
    names.dedup();
    for expr in ["Array.prototype.map", "Object.prototype.hasOwnProperty"] {
        println!("DESCRIPTION\t{expr}\t{}", names.join("\t"));
    }
    assert!(rows > 10);
}
