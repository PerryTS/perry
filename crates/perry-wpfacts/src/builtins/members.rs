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
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Family {
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

/// The family of a builtin object template kind (`TmplKind::BObj::what`).
pub(super) fn bobj_family(what: &str) -> Family {
    match what {
        "Collection" => Family::Collection,
        "Iterator" => Family::Iterator,
        "Promise" => Family::Promise,
        "Error" => Family::Error,
        "IterResult" | "settledRecord" | "CjsParent" | "require.cache" | "require.extensions" => {
            Family::Object
        }
        "primbox" | "keys" | "json" | "descriptor" | "descriptors" | "groups" | "fromEntries" => {
            Family::Box
        }
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

/// The own properties of a builtin namespace object (the constructor or
/// namespace itself, not its `prototype`), ECMA-262 and node's `Error`
/// extras. `None` for the namespaces the model does not describe
/// (`console`, the global object, `process`, `require`).
fn ns_statics(ns: BuiltinNs) -> Option<&'static [(&'static str, Mem)]> {
    Some(match ns {
        BuiltinNs::Object => &[
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
        ],
        BuiltinNs::Reflect => &[
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
        ],
        BuiltinNs::Array => &[
            ("from", FN),
            ("fromAsync", FN),
            ("isArray", FN),
            ("of", FN),
            ("@@__perry_wk_species", Mem::Ctor),
        ],
        BuiltinNs::Json => &[
            ("isRawJSON", FN),
            ("parse", FN),
            ("rawJSON", FN),
            ("stringify", FN),
            ("@@__perry_wk_toStringTag", Mem::Prim(prim::STR)),
        ],
        BuiltinNs::Math => &[
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
        ],
        BuiltinNs::Promise => &[
            ("all", FN),
            ("allSettled", FN),
            ("any", FN),
            ("race", FN),
            ("reject", FN),
            ("resolve", FN),
            ("try", FN),
            ("withResolvers", FN),
            ("@@__perry_wk_species", Mem::Ctor),
        ],
        // Error and its native subclasses; V8's `captureStackTrace` and
        // `stackTraceLimit` (node), `prepareStackTrace` (absent until set:
        // a program write is a store, `intrinsic_written`).
        BuiltinNs::Error => &[
            ("captureStackTrace", FN),
            ("isError", FN),
            ("stackTraceLimit", N),
        ],
        BuiltinNs::Symbol => &[
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
        ],
        // Number and BigInt (one namespace kind).
        BuiltinNs::Number => &[
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
        ],
        BuiltinNs::String => &[("fromCharCode", FN), ("fromCodePoint", FN), ("raw", FN)],
        BuiltinNs::Boolean | BuiltinNs::Function => &[],
        // Map/WeakMap and Set/WeakSet.
        BuiltinNs::Map => &[("groupBy", FN), ("@@__perry_wk_species", Mem::Ctor)],
        BuiltinNs::Set => &[("@@__perry_wk_species", Mem::Ctor)],
        BuiltinNs::Console
        | BuiltinNs::GlobalThis
        | BuiltinNs::Process
        | BuiltinNs::Require
        | BuiltinNs::Other => return None,
    })
}

/// The member `name` of a builtin namespace object: its own statics, then
/// its chain: `Function.prototype` for a constructor, `Object.prototype`
/// for a namespace object (`Math`, `JSON`, `Reflect`). Outer `None`: the
/// namespace is not described (every name may be a member).
pub(super) fn ns_member(ns: BuiltinNs, name: &str) -> Option<Option<Mem>> {
    let statics = ns_statics(ns)?;
    let chain = match ns {
        BuiltinNs::Math | BuiltinNs::Json | BuiltinNs::Reflect => Family::Object,
        _ => Family::Function,
    };
    Some(find(statics, name).or_else(|| member(chain, name)))
}
