// `%Object.prototype%`, `%Function.prototype%` and `%Array.prototype%`'s
// named holder, plus the generator intrinsic holders, as static layouts
// (object model §3.6: builtin prototypes are
// families with runtime-defined static layouts; step 6 slice 3b-i). Each
// prototype is born on its layout and the realm's population fills every
// declared slot, so its own keys are exactly its key list (plus whatever the
// program adds later, which sets its shadow bit): no builtin of these families
// is answered outside the list, and a list miss proves absence.
//
// Keys are static atoms; the key order is the own-key order; the attribute
// bytes are the builtin descriptors.

/// The attribute byte of a builtin method: writable, not enumerable,
/// configurable.
const BUILTIN_METHOD: u8 = crate::object::key_attrs::ENTRY_NON_ENUMERABLE;
/// A function's `length` and `name`: read-only, not enumerable, configurable.
const BUILTIN_READ_ONLY: u8 =
    crate::object::key_attrs::ENTRY_NON_ENUMERABLE | crate::object::key_attrs::ENTRY_NON_WRITABLE;
/// `Object.prototype.__proto__`, and `Function.prototype.caller` /
/// `arguments`: an accessor with a getter and a setter, not enumerable,
/// configurable.
const BUILTIN_ACCESSOR: u8 = crate::object::key_attrs::ENTRY_NON_ENUMERABLE
    | crate::object::key_attrs::ENTRY_ACCESSOR
    | crate::object::key_attrs::ENTRY_HAS_GET
    | crate::object::key_attrs::ENTRY_HAS_SET;

/// `ShapeObjectKind::OrdinaryUnmarked` / `CallableOrdinary` codes: a store is
/// never admitted on a builtin prototype by its layout alone, so every write
/// reaches the funnel that sets its shadow bit.
pub(crate) const LAYOUT_KIND_UNMARKED: u8 = 4;
pub(crate) const LAYOUT_KIND_CALLABLE: u8 = 11;

mod builtin_prototype_atoms {
    use super::*;
    runtime_atom!(BP_ATOM_NEXT, "next", "6e657874");
    runtime_atom!(BP_ATOM_RETURN, "return", "72657475726e");
    runtime_atom!(BP_ATOM_THROW, "throw", "7468726f77");
    runtime_atom!(reuse BP_ATOM_CONSTRUCTOR, "constructor", "636f6e7374727563746f72");
    runtime_atom!(
        BP_ATOM_DEFINE_GETTER,
        "__defineGetter__",
        "5f5f646566696e654765747465725f5f"
    );
    runtime_atom!(
        BP_ATOM_DEFINE_SETTER,
        "__defineSetter__",
        "5f5f646566696e655365747465725f5f"
    );
    runtime_atom!(
        BP_ATOM_HAS_OWN_PROPERTY,
        "hasOwnProperty",
        "6861734f776e50726f7065727479"
    );
    runtime_atom!(
        BP_ATOM_LOOKUP_GETTER,
        "__lookupGetter__",
        "5f5f6c6f6f6b75704765747465725f5f"
    );
    runtime_atom!(
        BP_ATOM_LOOKUP_SETTER,
        "__lookupSetter__",
        "5f5f6c6f6f6b75705365747465725f5f"
    );
    runtime_atom!(
        BP_ATOM_IS_PROTOTYPE_OF,
        "isPrototypeOf",
        "697350726f746f747970654f66"
    );
    runtime_atom!(
        BP_ATOM_PROPERTY_IS_ENUMERABLE,
        "propertyIsEnumerable",
        "70726f70657274794973456e756d657261626c65"
    );
    runtime_atom!(BP_ATOM_TO_STRING, "toString", "746f537472696e67");
    runtime_atom!(BP_ATOM_VALUE_OF, "valueOf", "76616c75654f66");
    runtime_atom!(BP_ATOM_PROTO, "__proto__", "5f5f70726f746f5f5f");
    runtime_atom!(
        BP_ATOM_TO_LOCALE_STRING,
        "toLocaleString",
        "746f4c6f63616c65537472696e67"
    );
    runtime_atom!(reuse BP_ATOM_LENGTH, "length", "6c656e677468");
    runtime_atom!(reuse BP_ATOM_NAME, "name", "6e616d65");
    runtime_atom!(BP_ATOM_APPLY, "apply", "6170706c79");
    runtime_atom!(BP_ATOM_BIND, "bind", "62696e64");
    runtime_atom!(BP_ATOM_CALL, "call", "63616c6c");
    runtime_atom!(BP_ATOM_ARGUMENTS, "arguments", "617267756d656e7473");
    runtime_atom!(BP_ATOM_CALLER, "caller", "63616c6c6572");
    runtime_atom!(BP_ATOM_SLICE, "slice", "736c696365");
    runtime_atom!(BP_ATOM_ENTRIES, "entries", "656e7472696573");
    runtime_atom!(BP_ATOM_FLAT_MAP, "flatMap", "666c61744d6170");
    runtime_atom!(BP_ATOM_KEYS, "keys", "6b657973");
    runtime_atom!(BP_ATOM_TO_REVERSED, "toReversed", "746f5265766572736564");
    runtime_atom!(BP_ATOM_TO_SORTED, "toSorted", "746f536f72746564");
    runtime_atom!(BP_ATOM_TO_SPLICED, "toSpliced", "746f53706c69636564");
    runtime_atom!(BP_ATOM_WITH, "with", "77697468");
    runtime_atom!(BP_ATOM_FLAT, "flat", "666c6174");
    runtime_atom!(BP_ATOM_VALUES, "values", "76616c756573");
    runtime_atom!(BP_ATOM_POP, "pop", "706f70");
    runtime_atom!(reuse BP_ATOM_SHIFT, "shift", "7368696674");
    runtime_atom!(BP_ATOM_REVERSE, "reverse", "72657665727365");
    runtime_atom!(BP_ATOM_PUSH, "push", "70757368");
    runtime_atom!(BP_ATOM_UNSHIFT, "unshift", "756e7368696674");
    runtime_atom!(BP_ATOM_SPLICE, "splice", "73706c696365");
    runtime_atom!(BP_ATOM_FILL, "fill", "66696c6c");
    runtime_atom!(BP_ATOM_COPY_WITHIN, "copyWithin", "636f707957697468696e");
    runtime_atom!(BP_ATOM_SORT, "sort", "736f7274");
    runtime_atom!(BP_ATOM_FOR_EACH, "forEach", "666f7245616368");
    runtime_atom!(reuse BP_ATOM_MAP, "map", "6d6170");
    runtime_atom!(BP_ATOM_FILTER, "filter", "66696c746572");
    runtime_atom!(BP_ATOM_SOME, "some", "736f6d65");
    runtime_atom!(BP_ATOM_EVERY, "every", "6576657279");
    runtime_atom!(BP_ATOM_FIND, "find", "66696e64");
    runtime_atom!(BP_ATOM_FIND_INDEX, "findIndex", "66696e64496e646578");
    runtime_atom!(BP_ATOM_FIND_LAST, "findLast", "66696e644c617374");
    runtime_atom!(
        BP_ATOM_FIND_LAST_INDEX,
        "findLastIndex",
        "66696e644c617374496e646578"
    );
    runtime_atom!(BP_ATOM_REDUCE, "reduce", "726564756365");
    runtime_atom!(
        BP_ATOM_REDUCE_RIGHT,
        "reduceRight",
        "7265647563655269676874"
    );
    runtime_atom!(BP_ATOM_INDEX_OF, "indexOf", "696e6465784f66");
    runtime_atom!(
        BP_ATOM_LAST_INDEX_OF,
        "lastIndexOf",
        "6c617374496e6465784f66"
    );
    runtime_atom!(BP_ATOM_INCLUDES, "includes", "696e636c75646573");
    runtime_atom!(BP_ATOM_AT, "at", "6174");
    runtime_atom!(BP_ATOM_JOIN, "join", "6a6f696e");
    runtime_atom!(BP_ATOM_CONCAT, "concat", "636f6e636174");
}
pub(crate) use builtin_prototype_atoms::*;
pub(crate) use native_constructor_atoms::NC_ATOM_PROTOTYPE as BP_ATOM_PROTOTYPE;

/// `Symbol.hasInstance` as a layout key (a well-known symbol atom).
#[used(linker)]
#[cfg_attr(target_vendor = "apple", link_section = "__DATA,__perry_atoms")]
#[cfg_attr(target_os = "windows", link_section = ".perryatm$m")]
#[cfg_attr(
    not(any(target_vendor = "apple", target_os = "windows", target_family = "wasm")),
    link_section = "perry_atoms"
)]
#[linkage = "weak_odr"]
#[export_name = "perry_atom_well_known_hasInstance"]
static ATOM_SYMBOL_HAS_INSTANCE: StaticAtom = StaticAtom {
    hash: crate::codegen_abi::well_known_symbol_atom_hash(WELL_KNOWN_SYMBOL_HAS_INSTANCE),
    string: std::ptr::null(),
    kind: crate::codegen_abi::ATOM_KIND_WELL_KNOWN_SYMBOL,
    _pad: [0; 3],
    index: WELL_KNOWN_SYMBOL_HAS_INSTANCE,
};
wasm_atom!(ATOM_SYMBOL_HAS_INSTANCE);
const WELL_KNOWN_SYMBOL_HAS_INSTANCE: u32 = 2;
const _: () = assert!(
    crate::codegen_abi::WELL_KNOWN_SYMBOLS[WELL_KNOWN_SYMBOL_HAS_INSTANCE as usize].len() == 11,
    "`hasInstance` is well-known symbol 2"
);

// `%Object.prototype%`: ECMA-262 §20.1.3 and Annex B §B.2.2, in V8's own-key
// order. Its [[Prototype]] is null.
runtime_layout!(OBJECT_PROTOTYPE, kind = LAYOUT_KIND_UNMARKED, 0x5254_0010, 0,
    crate::object::layout_desc::LAYOUT_PROTO_NULL, [
    BP_ATOM_CONSTRUCTOR => BUILTIN_METHOD,
    BP_ATOM_DEFINE_GETTER => BUILTIN_METHOD,
    BP_ATOM_DEFINE_SETTER => BUILTIN_METHOD,
    BP_ATOM_HAS_OWN_PROPERTY => BUILTIN_METHOD,
    BP_ATOM_LOOKUP_GETTER => BUILTIN_METHOD,
    BP_ATOM_LOOKUP_SETTER => BUILTIN_METHOD,
    BP_ATOM_IS_PROTOTYPE_OF => BUILTIN_METHOD,
    BP_ATOM_PROPERTY_IS_ENUMERABLE => BUILTIN_METHOD,
    BP_ATOM_TO_STRING => BUILTIN_METHOD,
    BP_ATOM_VALUE_OF => BUILTIN_METHOD,
    BP_ATOM_PROTO => BUILTIN_ACCESSOR,
    BP_ATOM_TO_LOCALE_STRING => BUILTIN_METHOD,
], realm = ["Object.prototype"]);

// `%Function.prototype%` (a callable ordinary object): ECMA-262 §20.2.3 and
// the restricted `caller` / `arguments` accessors of §10.2.4
// (AddRestrictedFunctionProperties), in V8's own-key order. `valueOf` and
// `toLocaleString` are not its own: it inherits them from
// `%Object.prototype%`.
runtime_layout!(FUNCTION_PROTOTYPE, kind = LAYOUT_KIND_CALLABLE, 0x5254_0011, 0,
    crate::object::layout_desc::LAYOUT_PROTO_OBJECT, [
    BP_ATOM_LENGTH => BUILTIN_READ_ONLY,
    BP_ATOM_NAME => BUILTIN_READ_ONLY,
    BP_ATOM_ARGUMENTS => BUILTIN_ACCESSOR,
    BP_ATOM_CALLER => BUILTIN_ACCESSOR,
    BP_ATOM_CONSTRUCTOR => BUILTIN_METHOD,
    BP_ATOM_APPLY => BUILTIN_METHOD,
    BP_ATOM_BIND => BUILTIN_METHOD,
    BP_ATOM_CALL => BUILTIN_METHOD,
    BP_ATOM_TO_STRING => BUILTIN_METHOD,
    ATOM_SYMBOL_HAS_INSTANCE,
], realm = ["Function.prototype"]);

// `%Array.prototype%`'s named layout (`ArrayHeader.named`, object model step
// 7): `Symbol.iterator` is its fixed slot 0, so array iteration reads one
// slot; every builtin method follows it.
runtime_layout!(ARRAY_PROTOTYPE_NAMED, kind = LAYOUT_KIND_UNMARKED, 0x5254_000C, 0,
    crate::object::layout_desc::LAYOUT_PROTO_OBJECT, [
    ATOM_SYMBOL_ITERATOR => BUILTIN_METHOD,
    BP_ATOM_CONSTRUCTOR => BUILTIN_METHOD,
    BP_ATOM_SLICE => BUILTIN_METHOD,
    BP_ATOM_TO_STRING => BUILTIN_METHOD,
    BP_ATOM_ENTRIES => BUILTIN_METHOD,
    BP_ATOM_FLAT_MAP => BUILTIN_METHOD,
    BP_ATOM_KEYS => BUILTIN_METHOD,
    BP_ATOM_TO_LOCALE_STRING => BUILTIN_METHOD,
    BP_ATOM_TO_REVERSED => BUILTIN_METHOD,
    BP_ATOM_TO_SORTED => BUILTIN_METHOD,
    BP_ATOM_TO_SPLICED => BUILTIN_METHOD,
    BP_ATOM_WITH => BUILTIN_METHOD,
    BP_ATOM_FLAT => BUILTIN_METHOD,
    BP_ATOM_VALUES => BUILTIN_METHOD,
    BP_ATOM_POP => BUILTIN_METHOD,
    BP_ATOM_SHIFT => BUILTIN_METHOD,
    BP_ATOM_REVERSE => BUILTIN_METHOD,
    BP_ATOM_PUSH => BUILTIN_METHOD,
    BP_ATOM_UNSHIFT => BUILTIN_METHOD,
    BP_ATOM_SPLICE => BUILTIN_METHOD,
    BP_ATOM_FILL => BUILTIN_METHOD,
    BP_ATOM_COPY_WITHIN => BUILTIN_METHOD,
    BP_ATOM_SORT => BUILTIN_METHOD,
    BP_ATOM_FOR_EACH => BUILTIN_METHOD,
    BP_ATOM_MAP => BUILTIN_METHOD,
    BP_ATOM_FILTER => BUILTIN_METHOD,
    BP_ATOM_SOME => BUILTIN_METHOD,
    BP_ATOM_EVERY => BUILTIN_METHOD,
    BP_ATOM_FIND => BUILTIN_METHOD,
    BP_ATOM_FIND_INDEX => BUILTIN_METHOD,
    BP_ATOM_FIND_LAST => BUILTIN_METHOD,
    BP_ATOM_FIND_LAST_INDEX => BUILTIN_METHOD,
    BP_ATOM_REDUCE => BUILTIN_METHOD,
    BP_ATOM_REDUCE_RIGHT => BUILTIN_METHOD,
    BP_ATOM_INDEX_OF => BUILTIN_METHOD,
    BP_ATOM_LAST_INDEX_OF => BUILTIN_METHOD,
    BP_ATOM_INCLUDES => BUILTIN_METHOD,
    BP_ATOM_AT => BUILTIN_METHOD,
    BP_ATOM_JOIN => BUILTIN_METHOD,
    BP_ATOM_CONCAT => BUILTIN_METHOD,
    BP_ATOM_VALUE_OF => BUILTIN_METHOD,
], realm = ["Array.prototype"]);
/// The fixed slot of `Symbol.iterator` in [`ARRAY_PROTOTYPE_NAMED`].
pub(crate) const ARRAY_PROTOTYPE_ITERATOR_SLOT: u32 = 0;

/// Is `obj` on a builtin prototype's declared layout? Such a
/// prototype's layout is its authority for life (only a program write moves
/// it, and sets its shadow bit): nothing the runtime learns about it is
/// stamped as another layout id. A compare against a fixed set of layouts.
///
/// # Safety
/// `obj` is a live `ObjectHeader`.
pub(crate) unsafe fn on_declared_prototype_layout(obj: *const crate::object::ObjectHeader) -> bool {
    let id = crate::object::layout_desc::layout_id_of_word((*obj).parent_class_id);
    [
        std::ptr::addr_of!(OBJECT_PROTOTYPE),
        std::ptr::addr_of!(FUNCTION_PROTOTYPE),
        std::ptr::addr_of!(ARRAY_PROTOTYPE_NAMED),
    ]
    .iter()
    .any(|d| (**d).layout_id == id)
}
