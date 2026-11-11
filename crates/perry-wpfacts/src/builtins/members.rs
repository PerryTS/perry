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
use perry_abi::lock::LockHolder;
use perry_hir::realm_manifest::{
    RealmEffect, RealmEntry, RealmKind, RealmOperand, RealmOwn, RealmOwnValue, RealmSlotRoot,
};

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
/// The methods `Object.prototype` holds (`OBJECT`'s functions): what a
/// computed key may read there when its shape can spell one of them.
pub(super) fn object_proto_methods() -> impl Iterator<Item = &'static str> {
    OBJECT
        .iter()
        .filter(|(_, mem)| matches!(mem, Mem::Fn))
        .map(|(key, _)| *key)
}

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
    use perry_abi::lock::{final_on, LockKey};
    use perry_abi::BuiltinProto as P;
    // The locked prototypes the lookup passes before `Object.prototype`
    // (which follows on every chain), each with whether `name` is one of its
    // image members.
    let own_member = || own(fam, name).is_some();
    let chain: &[(LockHolder, bool)] = match fam {
        Family::Object => &[],
        Family::Function if what == Some(FN_PROTO_ONLY) => {
            &[(LockHolder::Proto(P::Function), own_member())]
        }
        Family::Array => &[(LockHolder::Proto(P::Array), own_member())],
        Family::Str => &[(LockHolder::Proto(P::String), own_member())],
        Family::Num => &[(LockHolder::Proto(P::Number), own_member())],
        Family::Bool => &[(LockHolder::Proto(P::Boolean), own_member())],
        Family::Sym => &[(LockHolder::Proto(P::Symbol), own_member())],
        Family::BigInt => &[(LockHolder::Proto(P::BigInt), own_member())],
        Family::Promise => &[(LockHolder::Proto(P::Promise), own_member())],
        Family::Collection => {
            let (proto, keys) = match what {
                Some("Map") => (P::Map, MAP_KEYS),
                Some("Set") => (P::Set, SET_KEYS),
                Some("WeakMap") => (P::WeakMap, WEAKMAP_KEYS),
                Some("WeakSet") => (P::WeakSet, WEAKSET_KEYS),
                _ => return false,
            };
            let image = keys.contains(&name) && find(COLLECTION, name).is_some();
            &[(LockHolder::Proto(proto), image)]
        }
        // An array iterator's chain: `%ArrayIteratorPrototype%`, then
        // `%IteratorPrototype%`. The iterator family's member list is the
        // union of every iterator kind, so it names neither holder's image;
        // both holders are add-locked, where an image member and an absent
        // key are final alike, so the answer does not depend on it. The
        // Map/Set/String iterator prototypes and the iterator helpers' take
        // adds: no other iterator kind is final.
        // A container the model fills: the chain its creator gave it
        // (`box_chain`). An ordinary object passes `Object.prototype` only,
        // an array `Array.prototype` first; a null-prototype object has no
        // chain, so nothing the program writes reaches a read past its own
        // keys.
        Family::Box => match what.map_or(BoxChain::Unknown, box_chain) {
            BoxChain::Object => &[],
            BoxChain::Array => &[(
                LockHolder::Proto(P::Array),
                own(Family::Array, name).is_some(),
            )],
            BoxChain::Null => return true,
            BoxChain::Unknown => return false,
        },
        Family::Iterator if what == Some(ARRAY_ITERATOR) => &[
            (LockHolder::ArrayIterator, false),
            (LockHolder::Iterator, false),
        ],
        Family::Function | Family::Iterator | Family::Error | Family::Wrapper | Family::Opaque => {
            return false
        }
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
    for &(holder, image) in chain {
        let key = class(image);
        if !final_on(holder, key) {
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

/// Builtin iterator template kinds (`TmplKind::BObj::what`, family
/// `Iterator`): the iterator's intrinsic prototype, when the model knows it.
pub(crate) const ANY_ITERATOR: &str = "Iterator";
/// `%ArrayIteratorPrototype%` (`extends %IteratorPrototype%`): the runtime
/// seals and add-locks both (`builtin_proto.rs` `add_locked`:
/// `ITERATOR_FAMILY`, `ARRAY_ITERATOR_FAMILY`), so past them the chain is
/// `Object.prototype`, add-locked too: every read but `constructor` is
/// final (`locked_read`).
pub(crate) const ARRAY_ITERATOR: &str = "ArrayIterator";
pub(crate) const MAP_ITERATOR: &str = "MapIterator";
pub(crate) const SET_ITERATOR: &str = "SetIterator";

/// The intrinsic prototype of builtin object kind `what` whose image members
/// the runtime locks while it takes adds: `%MapIteratorPrototype%`,
/// `%SetIteratorPrototype%` (`builtin_proto.rs` `MAP_ITERATOR_FAMILY`,
/// `SET_ITERATOR_FAMILY`), `RegExp.prototype` and `Date.prototype`
/// (`seal_realm` seals every `BuiltinProto`). Its realm-table name and its
/// lock holder (`perry_abi::lock`), for `builtins::locked_chain_read` and
/// `builtins::native_locked`: a read of an image member there is final, an
/// absent key is not.
pub(crate) fn image_locked_prototype(what: &str) -> Option<(&'static str, LockHolder)> {
    use perry_abi::BuiltinProto as P;
    match what {
        MAP_ITERATOR => Some(("%MapIteratorPrototype%", LockHolder::MapIterator)),
        SET_ITERATOR => Some(("%SetIteratorPrototype%", LockHolder::SetIterator)),
        "RegExp" => Some(("RegExp.prototype", LockHolder::Proto(P::RegExp))),
        "Date" => Some(("Date.prototype", LockHolder::Proto(P::Date))),
        _ => None,
    }
}

/// The iterator kind a collection of kind `what` (`Map`/`Set`) gives.
pub(super) fn collection_iterator(what: &str) -> &'static str {
    match what {
        "Map" => MAP_ITERATOR,
        "Set" => SET_ITERATOR,
        _ => ANY_ITERATOR,
    }
}

/// The family of a builtin object template kind (`TmplKind::BObj::what`).
pub(super) fn bobj_family(what: &str) -> Family {
    match what {
        "Map" | "Set" | "WeakMap" | "WeakSet" => Family::Collection,
        ANY_ITERATOR | ARRAY_ITERATOR | MAP_ITERATOR | SET_ITERATOR => Family::Iterator,
        "Promise" => Family::Promise,
        "Error" => Family::Error,
        "IterResult" | "settledRecord" | "CjsParent" | "require.cache" | "require.extensions" => {
            Family::Object
        }
        "primbox" | "arraybox" | "keys" | "json" | "descriptor" | "descriptors" | "groups"
        | "fromEntries" | "mapGroups" | "hostrecord" => Family::Box,
        "Object()" | "Object(prim)" | "BoxedPrimitive" => Family::Wrapper,
        _ => Family::Opaque,
    }
}

/// The [[Prototype]] a `Family::Box` value of kind `what` is born with:
/// the one its creator allocates it with. `JSON.parse` objects (its arrays
/// are `TmplKind::Arr`), property descriptors and `Object.fromEntries`
/// results are ordinary objects (`OrdinaryObjectCreate(%Object.prototype%)`);
/// own-keys results are arrays (`CreateArrayFromList`); a match's `groups`
/// and an `Object.groupBy` result have a null prototype (`perex_results.rs`
/// `null_groups`, `groupby.rs`). The result of an undescribed method
/// (`primbox`) and a `Map.groupBy` Map (`mapGroups`, whose entries the model
/// keeps as a container's) have no chain the model describes.
pub(crate) fn box_chain(what: &str) -> BoxChain {
    match what {
        "json" | "descriptor" | "descriptors" | "fromEntries" | "hostrecord" => BoxChain::Object,
        "keys" | "arraybox" => BoxChain::Array,
        "groups" => BoxChain::Null,
        _ => BoxChain::Unknown,
    }
}

/// [`box_chain`]'s answer.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum BoxChain {
    Object,
    Array,
    Null,
    Unknown,
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
    /// Another builtin constructor (`TypeError`'s is `Error`): its statics
    /// are read through it, and it is the namespace's chain value
    /// (`Model::proto_node`).
    Constructor(BuiltinNs),
    /// An undescribed chain; the family only preserves generic call summaries.
    Unknown(Family),
}

#[derive(Clone, Copy)]
pub(super) struct NsStatics {
    pub(super) members: Option<Own>,
    pub(super) prototype: NsProto,
}

/// A described namespace's own statics: listed here, or the realm's
/// declaration of the constructor (`RealmEntry::own`), the one source of
/// what the runtime defines on it.
#[derive(Clone, Copy)]
pub(super) enum Own {
    Listed(&'static [(&'static str, Mem)]),
    Realm(&'static [RealmOwn]),
}

impl Own {
    fn find(self, name: &str) -> Option<Mem> {
        match self {
            Own::Listed(set) => find(set, name),
            Own::Realm(own) => own
                .iter()
                .find(|p| realm_key_is(p, name))
                .and_then(realm_mem),
        }
    }

    /// Its keys, spelled as atoms are (`@@__perry_wk_species`).
    fn keys(self) -> Vec<String> {
        match self {
            Own::Listed(set) => set.iter().map(|(key, _)| key.to_string()).collect(),
            Own::Realm(own) => own
                .iter()
                .map(|p| match p.key.strip_prefix("@@") {
                    Some(wk) if p.symbol => format!("{}{wk}", super::WK_PREFIX),
                    _ => p.key.to_string(),
                })
                .collect(),
        }
    }
}

/// Is the realm's own property `p` the key atoms spell `name`?
fn realm_key_is(p: &RealmOwn, name: &str) -> bool {
    match name.strip_prefix(super::WK_PREFIX) {
        Some(wk) => p.symbol && p.key.strip_prefix("@@") == Some(wk),
        None => !p.symbol && p.key == name,
    }
}

/// The realm constructor a realm constructor's declared [[Prototype]] names
/// (`node:stream.Readable` -> `node:stream.Stream`); `None` for the default
/// `Function.prototype` or a non-constructor.
pub(crate) fn realm_parent_constructor(identity: &str) -> Option<&'static str> {
    let entry = realm_entry(identity)?;
    if entry.kind != RealmKind::Constructor {
        return None;
    }
    let parent = realm_entry(entry.proto?)?;
    (parent.kind == RealmKind::Constructor).then_some(parent.name)
}


/// What a method lookup of a named key finds on the [[Prototype]] chain of
/// an object a realm declaration made (`fresh(P)`, builtin object kind `P`):
/// the own property of the first declaration on `P`'s declared chain that
/// has the key (with that declaration, its holder), or that no declaration
/// on it does.
#[derive(Clone, Copy, Debug)]
pub(super) enum RealmFind {
    Own(&'static RealmOwn, &'static RealmEntry),
    Absent,
}

/// The realm declaration `P` of a builtin object of kind `what` that a
/// declaration's `fresh(P)` made (`effects::Run::fresh` keeps the name):
/// a prototype declaration of a family this description does not model
/// otherwise, every link of whose [[Prototype]] chain the realm declares.
/// `None`: not such an object, or a link the realm does not declare (the
/// lookup stays undescribed).
pub(super) fn realm_born(what: &str) -> Option<&'static RealmEntry> {
    if bobj_family(what) != Family::Opaque {
        return None;
    }
    let born = realm_entry(what).filter(|e| e.kind == RealmKind::Prototype)?;
    let mut at = born;
    // The realm's chains are short; a cycle is no chain.
    for _ in 0..32 {
        match at.proto {
            None => return Some(born),
            Some(p) => at = realm_entry(p)?,
        }
    }
    None
}

/// The lookup of `name` (spelled as atoms are) on the chain of `born`
/// (`realm_born`): its realm-declared own properties, then those of its
/// declared [[Prototype]]s, to `null`.
pub(super) fn realm_chain_find(born: &'static perry_hir::realm_manifest::RealmEntry, name: &str) -> RealmFind {
    let mut at = born;
    loop {
        if let Some(p) = at.own.iter().find(|p| realm_key_is(p, name)) {
            return RealmFind::Own(p, at);
        }
        match at.proto.and_then(realm_entry) {
            Some(next) => at = next,
            None => return RealmFind::Absent,
        }
    }
}

pub(crate) fn realm_entry(name: &str) -> Option<&'static RealmEntry> {
    let realm = crate::native_effects::realm();
    // The runtime's realm source sorts its declarations by name.
    realm
        .binary_search_by_key(&name, |e| e.name)
        .ok()
        .map(|i| &realm[i])
}

/// What the realm's own property `p` of a constructor reads as a member:
/// a primitive of its kind, `prototype`, a native function, or
/// `@@species`, a getter that returns its receiver (the constructor).
/// `None`: a value this description has no member for.
fn realm_mem(p: &RealmOwn) -> Option<Mem> {
    Some(match p.value {
        RealmOwnValue::Undefined => Mem::Prim(prim::UNDEF),
        RealmOwnValue::Null => Mem::Prim(prim::NULL),
        RealmOwnValue::Boolean => Mem::Prim(prim::BOOL),
        RealmOwnValue::Number => Mem::Prim(prim::NUM),
        RealmOwnValue::String => Mem::Prim(prim::STR),
        RealmOwnValue::Symbol => Mem::Prim(prim::SYMBOL),
        RealmOwnValue::Reference(_) if !p.symbol && p.key == "prototype" => Mem::Proto,
        RealmOwnValue::Reference(r) => match realm_entry(r)?.kind {
            RealmKind::Function | RealmKind::Method => Mem::Fn,
            _ => return None,
        },
        RealmOwnValue::Accessor {
            get: Some(get),
            set: None,
        } if p.symbol && p.key == "@@species" && returns_receiver(realm_entry(get)?) => Mem::Ctor,
        _ => return None,
    })
}

fn returns_receiver(e: &RealmEntry) -> bool {
    matches!(e.effects, [RealmEffect::Returns(RealmOperand::Slot(s))]
        if s.root == RealmSlotRoot::This && s.steps.is_empty())
}

/// The statics of constructor `ns` exactly as the realm declares them, when
/// the realm declares it and this description has a member for each;
/// otherwise undescribed (every name may be a member).
fn realm_statics(ns: BuiltinNs, prototype: NsProto) -> NsStatics {
    let members = super::loads::proto_family(ns)
        .and_then(|(_, _, name)| realm_entry(name))
        .map(|e| e.own)
        .filter(|own| own.iter().all(|p| realm_mem(p).is_some()))
        .map(Own::Realm);
    NsStatics { prototype, members }
}

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
        // Error: V8's `captureStackTrace`, `prepareStackTrace` and
        // `stackTraceLimit` (node; the realm declares all three).
        BuiltinNs::Error => (
            NsProto::Function,
            Some(&[
                ("prototype", Mem::Proto),
                ("captureStackTrace", FN),
                ("isError", FN),
                ("prepareStackTrace", FN),
                ("stackTraceLimit", N),
            ]),
        ),
        // ECMA-262 20.5.6.2, 20.5.7.2: a NativeError and `AggregateError`
        // own `prototype`; the rest is `Error`'s, their [[Prototype]].
        BuiltinNs::NativeError(_) | BuiltinNs::AggregateError => (
            NsProto::Constructor(BuiltinNs::Error),
            Some(&[("prototype", Mem::Proto)]),
        ),
        // Undescribed: node's RegExp owns the legacy accessors (`$1`,
        // `lastMatch`, `input` ...), which the realm does not declare.
        BuiltinNs::RegExp => (NsProto::Function, None),
        // Their own statics are the realm's declaration of them (node:
        // `length`, `name`, `prototype`, `Date.now`/`parse`/`UTC`,
        // `ArrayBuffer.isView`, `%TypedArray%.from`/`of`, `@@species`, ...): a
        // key on neither the constructor nor its chain is absent.
        BuiltinNs::Date
        | BuiltinNs::ArrayBuffer
        | BuiltinNs::SharedArrayBuffer
        | BuiltinNs::DataView
        | BuiltinNs::TypedArray => return realm_statics(ns, NsProto::Function),
        // ECMA-262 23.2.6: a concrete typed-array constructor's
        // [[Prototype]] is `%TypedArray%`.
        BuiltinNs::TypedArrayKind(_) => {
            return realm_statics(ns, NsProto::Constructor(BuiltinNs::TypedArray))
        }
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
                ("isFinite", FN),
                ("isInteger", FN),
                ("isNaN", FN),
                ("isSafeInteger", FN),
                ("parseFloat", FN),
                ("parseInt", FN),
            ]),
        ),
        BuiltinNs::BigInt => (
            NsProto::Function,
            Some(&[("prototype", Mem::Proto), ("asIntN", FN), ("asUintN", FN)]),
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
        // `Other`, the HIR global sentinel (what a bare global name is read
        // off), is the global object: an ordinary object, not callable.
        BuiltinNs::GlobalThis
        | BuiltinNs::Other
        | BuiltinNs::Process
        | BuiltinNs::Module(_) => (NsProto::Object, None),
        BuiltinNs::Require => (NsProto::Unknown(Family::Function), None),
    };
    NsStatics {
        members: members.map(Own::Listed),
        prototype,
    }
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

/// The statics of a built-in constructor the realm reads by name itself,
/// in operations the model does not run as loads or calls on the
/// constructor: V8's stack hooks (`Error.prepareStackTrace`,
/// `Error.stackTraceLimit`) and the Promise combinators' `C.resolve`
/// (GetPromiseResolve). `@@species` is no such static: SpeciesConstructor
/// and ArraySpeciesCreate read `O.constructor` and then its `@@species` as
/// loads (`collections::species_link`, `typed_array_methods::species`, the
/// regexp's), and a constructor that owns `@@species` owns a getter without
/// a setter, which an ordinary store does not change. Operand protocols, a
/// pattern's symbol methods and the formatter's hook are loads and calls on
/// the value, which read what the program stored there.
pub(super) fn realm_read_statics(ns: BuiltinNs) -> &'static [&'static str] {
    match ns {
        BuiltinNs::Error | BuiltinNs::AggregateError => &["prepareStackTrace", "stackTraceLimit"],
        BuiltinNs::Promise => &["resolve"],
        _ => &[],
    }
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
    if let Some(mem) = own.find(name) {
        return Some(Some(mem));
    }
    match description.prototype {
        NsProto::Function => Some(member(Family::Function, name)),
        NsProto::Object => Some(member(Family::Object, name)),
        NsProto::Constructor(parent) => described_member(ns_statics(parent), name),
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
            NsProto::Constructor(parent) => prototype = ns_statics(parent).prototype,
            NsProto::Unknown(family) => return family,
        }
    }
}

/// What a key that may name any member reads on the family's chain
/// (`Object.prototype` included): the kinds of its data members, and
/// whether its own prototype has a function member (`Object.prototype`'s
/// methods, its `constructor` and `__proto__` are on every chain).
/// The own member table of family `fam`'s intrinsic prototype, when the
/// family is described member by member (`None`: Box, Wrapper, Opaque).
fn own_table(fam: Family) -> Option<&'static [(&'static str, Mem)]> {
    Some(match fam {
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
        Family::Box | Family::Wrapper | Family::Opaque => return None,
    })
}

/// The names on family `fam`'s intrinsic chain: its own prototype's, then
/// the `Object.prototype` ones it does not shadow (`member`'s order).
/// `None`: the family has no member table.
pub(super) fn chain_names(fam: Family) -> Option<impl Iterator<Item = &'static str>> {
    let own = own_table(fam)?;
    let object = OBJECT.iter().filter(move |(n, _)| find(own, n).is_none());
    Some(own.iter().chain(object).map(|(n, _)| *n))
}

/// The own statics of described namespace `ns` and of the constructors
/// its chain passes (`NsProto::Constructor`), spelled as atoms are; `None`
/// where the namespace or a constructor on its chain is undescribed.
pub(super) fn ns_static_names(ns: BuiltinNs) -> Option<Vec<String>> {
    let mut out = Vec::new();
    let mut d = ns_statics(ns);
    loop {
        out.extend(d.members?.keys());
        match d.prototype {
            NsProto::Function | NsProto::Object => return Some(out),
            NsProto::Constructor(parent) => d = ns_statics(parent),
            NsProto::Unknown(_) => return None,
        }
    }
}

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
    fn keys(d: NsStatics, out: &mut Vec<String>) -> bool {
        let Some(own) = d.members else { return false };
        out.extend(own.keys());
        match d.prototype {
            NsProto::Function => {
                out.extend(FUNCTION.iter().map(|(key, _)| key.to_string()));
                out.extend(OBJECT.iter().map(|(key, _)| key.to_string()));
            }
            NsProto::Object => out.extend(OBJECT.iter().map(|(key, _)| key.to_string())),
            NsProto::Constructor(parent) => return keys(ns_statics(parent), out),
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

/// The constructors whose statics are the realm's declaration: the keys a
/// named read finds as their own are exactly the keys the realm declares on
/// them, and node's (v26.8.1, `Reflect.ownKeys`).
#[test]
fn realm_described_statics_are_the_realm_declaration() {
    let realm = crate::native_effects::realm();
    assert!(
        realm.windows(2).all(|w| w[0].name < w[1].name),
        "the realm table is sorted by name"
    );
    let node: &[(BuiltinNs, &[&str])] = &[
        (
            BuiltinNs::Date,
            &["UTC", "length", "name", "now", "parse", "prototype"],
        ),
        (
            BuiltinNs::ArrayBuffer,
            &[
                "@@__perry_wk_species",
                "isView",
                "length",
                "name",
                "prototype",
            ],
        ),
        (
            BuiltinNs::SharedArrayBuffer,
            &["@@__perry_wk_species", "length", "name", "prototype"],
        ),
        (BuiltinNs::DataView, &["length", "name", "prototype"]),
        (
            BuiltinNs::TypedArray,
            &[
                "@@__perry_wk_species",
                "from",
                "length",
                "name",
                "of",
                "prototype",
            ],
        ),
        (
            BuiltinNs::TypedArrayKind("Uint8Array"),
            &[
                "BYTES_PER_ELEMENT",
                "fromBase64",
                "fromHex",
                "length",
                "name",
                "prototype",
            ],
        ),
    ];
    let mut all: Vec<BuiltinNs> = vec![
        BuiltinNs::Date,
        BuiltinNs::ArrayBuffer,
        BuiltinNs::SharedArrayBuffer,
        BuiltinNs::DataView,
        BuiltinNs::TypedArray,
    ];
    all.extend(
        super::TYPED_ARRAYS
            .iter()
            .map(|&n| BuiltinNs::TypedArrayKind(n)),
    );
    for ns in all {
        let name = super::loads::proto_family(ns).unwrap().2;
        let entry = realm_entry(name).unwrap_or_else(|| panic!("the realm declares {name}"));
        let Some(own @ Own::Realm(_)) = ns_statics(ns).members else {
            panic!("{name}: statics described by the realm");
        };
        let mut declared = own.keys();
        declared.sort();
        assert_eq!(declared.len(), entry.own.len(), "{name}");
        // The read set: every declared key reads its own member, nothing
        // else does.
        let probes = [
            "replace",
            "@@__perry_wk_toPrimitive",
            "@@__perry_wk_iterator",
            "fromBase64",
            "isView",
            "now",
            "from",
            "BYTES_PER_ELEMENT",
            "@@__perry_wk_species",
            "caller",
            "constructor",
        ];
        let mut read: Vec<String> = declared
            .iter()
            .map(String::as_str)
            .chain(probes)
            .filter(|k| own.find(k).is_some())
            .map(str::to_string)
            .collect();
        read.sort();
        read.dedup();
        assert_eq!(read, declared, "{name}");
        for k in &declared {
            assert!(matches!(ns_member(ns, k), Some(Some(_))), "{name}.{k}");
        }
        if let Some((_, keys)) = node.iter().find(|(n, _)| *n == ns) {
            assert_eq!(declared, keys.to_vec(), "{name}: node's own keys");
        }
        if let BuiltinNs::TypedArrayKind(n) = ns {
            if n != "Uint8Array" {
                assert_eq!(
                    declared,
                    ["BYTES_PER_ELEMENT", "length", "name", "prototype"],
                    "{n}"
                );
            }
        }
        // Absent on the constructor and its chain: undefined.
        assert_eq!(ns_member(ns, "replace"), Some(None), "{name}.replace");
        assert_eq!(
            ns_member(ns, "@@__perry_wk_toPrimitive"),
            Some(None),
            "{name}[@@toPrimitive]"
        );
        // The chain's: `Function.prototype.call`.
        assert_eq!(ns_member(ns, "call"), Some(Some(Mem::Fn)), "{name}.call");
    }
    assert_eq!(
        ns_member(BuiltinNs::TypedArrayKind("Int16Array"), "from"),
        Some(Some(Mem::Fn))
    );
    assert_eq!(
        ns_member(BuiltinNs::TypedArrayKind("Int16Array"), "BYTES_PER_ELEMENT"),
        Some(Some(Mem::Prim(prim::NUM)))
    );
    assert_eq!(
        ns_member(BuiltinNs::Date, "prototype"),
        Some(Some(Mem::Proto))
    );
    assert_eq!(
        ns_member(BuiltinNs::ArrayBuffer, "@@__perry_wk_species"),
        Some(Some(Mem::Ctor))
    );
    // node's RegExp owns legacy accessors the realm does not declare.
    assert!(ns_statics(BuiltinNs::RegExp).members.is_none());
    assert_eq!(ns_member(BuiltinNs::RegExp, "replace"), None);
}

#[test]
fn a_realm_born_object_reads_exactly_its_declared_chain() {
    // W2a: an object `fresh(P)` makes has `P` as its [[Prototype]]. The keys
    // the lookup finds are the realm's own keys of `P` and of every
    // declaration on its declared chain, to `null`; no other key is found.
    let atom = |p: &RealmOwn| match p.key.strip_prefix("@@") {
        Some(wk) if p.symbol => format!("{}{wk}", super::WK_PREFIX),
        _ => p.key.to_string(),
    };
    for (what, chain) in [
        ("Response.prototype", &["Response.prototype", "Object.prototype"][..]),
        (
            "TypeError.prototype",
            &["TypeError.prototype", "Error.prototype", "Object.prototype"][..],
        ),
        (
            "DOMException.prototype",
            &["DOMException.prototype", "Error.prototype", "Object.prototype"][..],
        ),
        (
            "AbortSignal.prototype",
            &["AbortSignal.prototype", "EventTarget.prototype", "Object.prototype"][..],
        ),
    ] {
        let born = realm_born(what).unwrap_or_else(|| panic!("{what} is realm-born"));
        let mut declared = std::collections::BTreeSet::new();
        let mut at = Some(born);
        let mut walked = Vec::new();
        while let Some(e) = at {
            walked.push(e.name);
            declared.extend(e.own.iter().map(atom));
            at = e.proto.map(|p| realm_entry(p).expect("declared link"));
        }
        assert_eq!(walked, chain, "{what}");
        for key in &declared {
            assert!(
                matches!(realm_chain_find(born, key), RealmFind::Own(..)),
                "{what}.{key} is found"
            );
        }
        for key in ["then", "zzNoSuchMember", "@@__perry_wk_iterator", "0"] {
            assert!(!declared.contains(key), "{what}.{key} is not declared");
            assert!(
                matches!(realm_chain_find(born, key), RealmFind::Absent),
                "{what}.{key} is absent"
            );
        }
    }
    // The first declaration on the chain that has the key decides:
    // `Object.prototype.hasOwnProperty` under `Response.prototype`.
    let found = |what: &str, key: &str| match realm_chain_find(realm_born(what).unwrap(), key) {
        RealmFind::Own(p, _) => p.value,
        RealmFind::Absent => panic!("{what}.{key} absent"),
    };
    assert_eq!(
        found("Response.prototype", "hasOwnProperty"),
        RealmOwnValue::Reference("Object.prototype.hasOwnProperty")
    );
    assert_eq!(found("Response.prototype", "@@__perry_wk_toStringTag"), RealmOwnValue::String);
    // A described family is not realm-born (its own description decides),
    // nor is a kind no declaration names.
    for what in ["Promise", "Error", "Map", "hostrecord", "Response", "builtin"] {
        assert!(realm_born(what).is_none(), "{what}");
    }
}
