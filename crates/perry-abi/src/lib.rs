#![no_std]
//! Runtime layout facts that generated code bakes in, in ONE file both sides
//! depend on: `perry-runtime` (as `crate::codegen_abi`) and `perry-codegen`
//! (as `crate::runtime_abi`). A layout change edits a number here, and the
//! runtime's `offset_of!`/`size_of` assertions next to each struct refuse to
//! compile until the number is right — so emitted code can never disagree
//! with the struct it indexes. No dependencies.

pub mod accessor_guards;
pub mod layouts;
pub use layouts::*;

/// `object::shapes::SHAPE_ID_BASE`: the first ShapeId.
pub const SHAPE_ID_BASE: u32 = 0x8000_0000;
/// The compiler-assigned ("static") ShapeId band is
/// `[SHAPE_ID_BASE, SHAPE_ID_BASE + STATIC_SHAPE_ID_COUNT)`: the driver
/// assigns ids there by content and generated code embeds them as immediates;
/// the runtime's counter never draws from it (design step 4).
pub const STATIC_SHAPE_ID_COUNT: u32 = 1 << 20;
/// Object model step 3 (§3.6): a layout word (`ObjectHeader` +4) below this
/// is an unshadowed ordinary layout (or a word no write stamped); shadowed,
/// dictionary and exotic words lie at or above it. The site test of a
/// prototype constant or a known-holder read is one unsigned compare.
pub const UNSHADOWED_WORD_END: u32 = 0xB000_0000;
/// `object::shapes::SHAPE_ID_KIND_MASK`: a ShapeId's identity-kind bits.
/// All set on the mixed kind (`SHAPE_ID_KIND_MIXED`, a class instance linked
/// to a recorded prototype), the only kind a class evaluation's copy of a
/// family layout carries, whose instances a constructor store run asks the
/// runtime about (`perry_family_run_admits`).
pub const SHAPE_ID_KIND_MASK: u32 = 3 << 20;
/// `object::shapes::WEAKMAP_BIRTH_LAYOUT` / `WEAKSET_BIRTH_LAYOUT`: the
/// declared layout every `new WeakMap()` / `new WeakSet()` is born on (the
/// brand, no key, the family's intrinsic as `[[Prototype]]`). An own key, a
/// `[[Prototype]]` change or an exotic mark moves the instance off it, so a
/// layout word (`ObjectHeader` +4) equal to one of them proves a plain weak
/// collection in one compare: the method site's receiver test (Z2).
pub const WEAKMAP_BIRTH_LAYOUT: u32 = SHAPE_ID_BASE + (4 << 20);
pub const WEAKSET_BIRTH_LAYOUT: u32 = WEAKMAP_BIRTH_LAYOUT + 1;

/// `array::ArrayHeader` size: element 0 follows it.
pub const ARRAY_HEADER_SIZE: usize = 24;

/// `PerryLayoutKey.attrs` bit (the runtime's key entry byte, `ENTRY_OPTIONAL`):
/// the key is a layout's optional or late member. Its slot may hold the hole
/// marker, and while it does the key is absent (OBJECT_MODEL_2026-10-13.md
/// §3.2). A descriptor emits it only for such keys; the layout then carries
/// the "may have holes" summary bit and every other layout keeps its cost.
pub const LAYOUT_KEY_OPTIONAL: u8 = 0x80;
/// `PerryLayoutKey.attrs` attribute bits (the runtime's `key_attrs` entry
/// bits, pinned there): `[[Writable]]`, `[[Enumerable]]`, `[[Configurable]]`
/// false, an accessor, and which halves it has.
pub const LAYOUT_KEY_NON_WRITABLE: u8 = 0x01;
pub const LAYOUT_KEY_NON_ENUMERABLE: u8 = 0x02;
pub const LAYOUT_KEY_NON_CONFIGURABLE: u8 = 0x04;
pub const LAYOUT_KEY_ACCESSOR: u8 = 0x08;
pub const LAYOUT_KEY_HAS_GET: u8 = 0x10;
pub const LAYOUT_KEY_HAS_SET: u8 = 0x20;
/// `PerryLayoutDesc.integrity` of a layout born non-extensible (the runtime's
/// `gc::OBJ_FLAG_NO_EXTEND`, pinned there): a module namespace object
/// (ECMA-262 10.4.6) is born on such a layout, so no later
/// `preventExtensions` transition mints a further shape.
pub const LAYOUT_INTEGRITY_NON_EXTENSIBLE: u8 = 0x04;
/// `PerryLayoutKey.attrs` of a private field's element (the runtime's
/// `key_attrs::PRIVATE_FIELD_ENTRY`: `ENTRY_PRIVATE | ENTRY_NON_ENUMERABLE`):
/// the key is the field's qualified private name, never a property. A class
/// family emits it with [`LAYOUT_KEY_OPTIONAL`]: the element is absent until
/// the field's definition fills its slot.
pub const LAYOUT_KEY_PRIVATE_FIELD: u8 = 0x42;

/// `StaticAtom.kind` (OBJECT_MODEL_2026-10-13.md §6a): a string key.
pub const ATOM_KIND_STRING: u8 = 0;
/// `StaticAtom.kind`: a well-known symbol; `string` is null and `index` is
/// its number in [`WELL_KNOWN_SYMBOLS`].
pub const ATOM_KIND_WELL_KNOWN_SYMBOL: u8 = 1;
/// `StaticAtom.kind`: a program symbol (a module-level `const s =
/// Symbol(desc)`); `string` is the description and `hash` the creation
/// sequence number the runtime assigns when it creates the symbol.
pub const ATOM_KIND_PROGRAM_SYMBOL: u8 = 2;
/// `StaticAtom.kind`: a registered symbol (`Symbol.for(desc)`); `string` is
/// its registry key, `hash` the compiler's hash of it (a key index position
/// only: the runtime resolves the symbol through the registry).
pub const ATOM_KIND_REGISTERED_SYMBOL: u8 = 3;
/// `StaticAtom.kind`: a module-local symbol (a top-level `const k =
/// Symbol(desc)` of the program) as a key of a carried prototype layout;
/// `string` is null, `hash` the compiler's hash of the binding (a key index
/// position only). The symbol exists only once its binding is initialized:
/// the prototype's bind (`perry_bind_family_prototype`) hands the layout's
/// such keys in key order, and the layout registers there.
pub const ATOM_KIND_BINDING_SYMBOL: u8 = 4;
/// `PerryLayoutDesc.variant` of a carried prototype layout (object model
/// §3.6): a marked prototype's, registered under its own semantic generation.
pub const LAYOUT_VARIANT_PROTOTYPE_IMAGE: u16 = 1;
/// Ordinary function constructor family, with a live per-object prototype.
pub const LAYOUT_VARIANT_FUNCTION_CONSTRUCTOR: u16 = 2;
/// A module namespace object's layout. Startup only reserves its id; the
/// namespace's birth installs it, so a module whose namespace is never built
/// costs no key list and no shape.
pub const LAYOUT_VARIANT_NAMESPACE: u16 = 3;
/// The well-known symbols by number (`StaticAtom.index` of a kind-1 atom),
/// spelled as `Symbol.<name>`.
pub const WELL_KNOWN_SYMBOLS: [&str; 15] = [
    "iterator",
    "asyncIterator",
    "hasInstance",
    "toPrimitive",
    "toStringTag",
    "species",
    "isConcatSpreadable",
    "match",
    "matchAll",
    "replace",
    "search",
    "split",
    "unscopables",
    "dispose",
    "asyncDispose",
];
/// `WELL_KNOWN_SYMBOLS` number of `Symbol.iterator`.
pub const WELL_KNOWN_SYMBOL_ITERATOR: u32 = 0;
const _: () = assert!(WELL_KNOWN_SYMBOLS[WELL_KNOWN_SYMBOL_ITERATOR as usize].len() == 8);

/// The number of well-known symbol `name` (`"iterator"`), if it is one.
pub fn well_known_symbol_number(name: &str) -> Option<u32> {
    WELL_KNOWN_SYMBOLS
        .iter()
        .position(|n| *n == name)
        .map(|i| i as u32)
}

/// The fixed `StaticAtom.hash` of well-known symbol `number`.
pub const fn well_known_symbol_atom_hash(number: u32) -> u64 {
    0x9e37_79b9_7f4a_7c15u64.wrapping_mul(number as u64 + 1)
}

/// `agent_ptrs::PERRY_AGENT_PTRS`: the number of per-agent pointer slots.
pub const AGENT_PTR_SLOTS: usize = 4 + INTRINSIC_FAMILIES;
/// Slots `AGENT_PTR_INTRINSIC_PROTO + k`: this agent's intrinsic prototype of
/// builtin family `k` (`object::builtin_proto::BuiltinProto`; for Array the
/// sealed named-property bag), whose layout word's shadow bit
/// (`UNSHADOWED_WORD_END`) is `Pristine(k)`. Never null: until the realm is
/// sealed every slot names `PERRY_PRISTINE_PROTO`, whose word is 0.
pub const AGENT_PTR_INTRINSIC_PROTO: usize = 4;
/// Number of builtin prototype families.
pub const BUILTIN_PROTO_FAMILIES: usize = 16;
/// The builtin namespace objects whose direct member calls lowering binds to
/// intrinsics (`Math.max(..)`, `Object.keys(..)`, `console.log(..)`). Namespace
/// `i` is intrinsic family `BUILTIN_PROTO_FAMILIES + i`: its agent slot
/// (`AGENT_PTR_INTRINSIC_PROTO + BUILTIN_PROTO_FAMILIES + i`) names the object
/// that holds the namespace's own members (a constructor's property bag), and
/// that object's shadow bit (`UNSHADOWED_WORD_END`) is `Pristine(namespace)`:
/// clear while every member still holds its builtin and `globalThis[name]`
/// still names the namespace.
pub const BUILTIN_NAMESPACES: [&str; 15] = [
    "console", "Math", "JSON", "Reflect", "Object", "Number", "String", "Array", "Promise", "Date",
    "Symbol", "BigInt", "Atomics", "Intl", "Boolean",
];
/// Builtin prototype families plus builtin namespaces: the intrinsic slots.
pub const INTRINSIC_FAMILIES: usize = BUILTIN_PROTO_FAMILIES + BUILTIN_NAMESPACES.len();

/// The intrinsic family of builtin namespace `name`, if it is one.
pub fn builtin_namespace_family(name: &str) -> Option<usize> {
    let mut i = 0;
    while i < BUILTIN_NAMESPACES.len() {
        if BUILTIN_NAMESPACES[i].as_bytes() == name.as_bytes() {
            return Some(BUILTIN_PROTO_FAMILIES + i);
        }
        i += 1;
    }
    None
}
/// Slot 0: the address of this agent's ordinary shape-directory mirror
/// (`shapes_store::AGENT_SHAPE_DIR[0]`), which a generic read site passes to its
/// GC-leaf miss front (`js_object_get_field_ic_front`) so the front reads no
/// thread-local. Slot 1 held the implicit-`this` cell's address until
/// this-as-a-parameter deleted the cell, and is free; slot 2 is the stack limit.
pub const AGENT_PTR_SHAPE_DIR: usize = 0;
/// Payloads below this are native-registry handles, never heap cells
/// (`addr_class::HANDLE_BAND_MAX`). A generic read site's fused receiver test
/// computes `payload - RECEIVER_HANDLE_FLOOR` on its pointer edge, and its
/// miss front (`js_object_get_field_ic_front`) takes the receiver in exactly
/// that form: the front adds the floor back inside its load displacements,
/// and the site passes the value its test already holds.
pub const RECEIVER_HANDLE_FLOOR: usize = 0x20_0000;

/// Object model step 5 (case 2c): the size of one ordinary-band shape record
/// (`perry-runtime` `object/shapes_store.rs` `ShapeRecord`). Record `id` of
/// an agent is at `flat_base + (id - SHAPE_ID_BASE) * SHAPE_RECORD_SIZE`,
/// `flat_base` and the committed id bound at `SHAPE_DIR_FLAT_BASE` and
/// `SHAPE_DIR_FLAT_BASE + 8` of the agent's shape directory
/// (`AGENT_PTR_SHAPE_DIR`).
pub const SHAPE_RECORD_SIZE: usize = 128;
/// Offset of a record's two-word coloured-key mask (read: the key is inline
/// data at its colour slot).
pub const SHAPE_RECORD_COLOUR_MASK: usize = 88;
/// Offset of a record's two-word coloured-key store mask (the key is also a
/// plain writable `Any`- or `F64`-lane slot of a store-admitted, non-frozen
/// layout; an `F64` lane takes only a plain double, so a site tests the lane
/// when its value may be anything else).
pub const SHAPE_RECORD_COLOUR_STORE_MASK: usize = 104;
/// Offset of a record's per-slot representation word (two bits per inline
/// slot 0..32, `Any` = 0; a slot at or past 32 is `Any`).
pub const SHAPE_RECORD_REP: usize = 48;
/// The number of inline slots a record's `rep` word describes.
pub const SHAPE_REP_SLOTS: u32 = 32;
/// Offset of the flat record base in the agent's shape directory.
pub const SHAPE_DIR_FLAT_BASE: usize = 24;
/// The layout word's shadow bit (`perry-runtime` `layout_desc.rs`).
pub const LAYOUT_SHADOW_BIT: u32 = 1 << 30;
/// Slot 2: this agent's stack limit (#10812) — not a pointer to anything, the
/// lowest frame address a compiled prologue accepts before it throws
/// `RangeError: Maximum call stack size exceeded`. Null means unchecked.
pub const AGENT_PTR_STACK_LIMIT: usize = 2;
/// `tls_hot::HotTls::agent_ptrs` (Apple aarch64 TSD path; LP64): the first
/// inline value, behind fixed-size fields only.
pub const HOT_TLS_AGENT_PTRS_OFFSET: usize = 104;

/// `closure::ClosureHeader` (LP64): the u32 capture count at 0, the ShapeId
/// at 4 (the same word as `ObjectHeader`), the function's
/// [`JsFunctionInfo`] pointer at 8, the shaped own-property record at 16,
/// captures from 24. ILP32 targets shrink the two pointers: info at 8, props
/// at 12, captures from 16 (derived in `perry-codegen/src/target_layout.rs`).
pub const CLOSURE_SHAPE_OFFSET: usize = 4;
pub const CLOSURE_INFO_OFFSET: usize = 8;
pub const CLOSURE_PROPS_OFFSET: usize = 16;
pub const CLOSURE_HEADER_SIZE: usize = 24;

/// Reserved intrinsic function ShapeIds, seeded in each agent's authoritative
/// shape table. No body/shape cache is needed when a closure is born.
pub const FUNCTION_SHAPE_BASE: u32 = 0xB810_0000;
/// Per-object dictionary prototype identity uses the plain kind band.
pub const FUNCTION_DICTIONARY_SHAPE: u32 = 0xB800_0004;
pub const FUNCTION_SHAPE_COUNT: u32 = 9;
pub const FUNCTION_SHAPE: u32 = FUNCTION_SHAPE_BASE;
pub const ASYNC_FUNCTION_SHAPE: u32 = FUNCTION_SHAPE_BASE + 1;

/// `object::ObjectHeader::parent_class_id`: the object's ShapeId word (LP64
/// and ILP32 alike; `CLOSURE_SHAPE_OFFSET` is the same word of a closure).
pub const OBJECT_SHAPE_OFFSET: usize = 4;

/// `object::class_value::StaticCallMemo` (LP64) — the words the emitted
/// static-call guard reads (`perry-codegen/src/expr/static_method.rs`).
pub const STATIC_CALL_MEMO_KEY_OFFSET: usize = 0;
pub const STATIC_CALL_MEMO_C_OFFSET: usize = 8;
pub const STATIC_CALL_MEMO_OWNER_OFFSET: usize = 16;
pub const STATIC_CALL_MEMO_VALUE_OFFSET: usize = 24;

/// #11759 (c′): the capture slot of a class function object (an INT32) that
/// says whether the object is its declaration's first evaluation, and the
/// value generated code stores there when the first evaluation hands the
/// shared class out (`INT32_TAG | 1`; the slot is born `INT32_TAG | 0`).
pub const CLASS_EVALUATION_STATE_CAPTURE: usize = 1;
pub const CLASS_FIRST_EVALUATION_STATE: u64 = 0x7FFE_0000_0000_0001;

/// `gc::GC_TYPE_CLOSURE`: the GcHeader type byte (at payload - 8) that makes a
/// cell a function object. The kind is this byte, never a payload magic.
pub const GC_TYPE_CLOSURE: u8 = 4;
/// `gc::GC_TYPE_OBJECT`: an ordinary object, e.g. a byte cell's property bag.
pub const GC_TYPE_OBJECT: u8 = 2;
/// `gc::GC_TYPE_BUFFER` and `gc::GC_TYPE_BUFFER_UINT8ARRAY`: the GcHeader type
/// bytes of the two BYTE-VIEW buffer brands, a Node `Buffer` and a
/// `BufferHeader`-backed `Uint8Array` (#10694: a buffer's flavor is its type
/// byte). Emitted byte-access guards accept exactly these two.
pub const GC_TYPE_BUFFER: u8 = 0x4c;
pub const GC_TYPE_BUFFER_UINT8ARRAY: u8 = 0x40;
/// Header-less symbols must be screened before trusting a buffer layout.
pub const SYMBOL_HEADER_MAGIC: u32 = 0x5359_4D42;
/// `gc::GC_FLAG_FORWARDED` (GcHeader byte 1): an evacuated from-space stub.
pub const GC_FLAG_FORWARDED: u8 = 0x80;
/// `gc::GC_HEADER_SIZE`.
pub const GC_HEADER_SIZE: usize = 8;

/// `GcHeader::_reserved` flags any of which refuses a family constructor's
/// run of in-place stores (`perry_family_run_admits`, and the site's proof
/// in `perry-codegen` `proto_constants::emit_family_run_proof`): frozen,
/// sealed, non-extensible, stable tombstones.
pub const OBJ_FLAGS_REFUSE_FAMILY_RUN: u16 = 0x0407;

/// Creation-time [[Call]] on OBJECT and PROXY only. This bit has no other
/// meaning; closures are callable by their type byte.
pub const GC_OBJECT_CALLABLE: u16 = 0x0020;
/// Heap-birth facts shared by runtime allocation and realm-cell emission.
/// These are existing meanings in GcHeader::_reserved, never image markers.
pub const OBJ_FLAG_PLAIN_ORDINARY: u16 = 0x0200;
pub const GC_LAYOUT_POINTER_FREE: u16 = 0x4000;
pub const GC_LAYOUT_SIDE_MASK: u16 = 0x8000;
pub const GC_LAYOUT_ALL_POINTERS: u16 = 0x2000;
pub const GC_TYPE_PROXY: u8 = 37;
pub const GC_TYPE_SYMBOL: u8 = 32;
pub const GC_TYPE_STRING: u8 = 3;
/// `gc::GC_TYPE_BOX`: a JSValue cell (one 8-byte payload word) holding a
/// boxed binding. A declaring function's inline box read accepts exactly this
/// kind, unforwarded, of exactly that size.
pub const GC_TYPE_BOX: u8 = 22;

/// Classify the first four GC-header bytes, loaded as a native-endian u32.
/// No shape or target read is needed after the creation-time publication.
#[inline(always)]
pub const fn gc_header_word_is_callable(word: u32) -> bool {
    let bytes = word.to_ne_bytes();
    let kind = bytes[0];
    let facts = u16::from_ne_bytes([bytes[2], bytes[3]]);
    kind == GC_TYPE_CLOSURE
        || ((kind == GC_TYPE_OBJECT || kind == GC_TYPE_PROXY) && facts & GC_OBJECT_CALLABLE != 0)
}

/// The JS BODY calling convention. Every native body a function object runs —
/// a compiled closure body, a value wrapper, a native builtin installed as a
/// function object, a body an addon registers through perry-ffi — is
///
/// ```text
/// double body(i64 callee, i64 this, double a0, double a1, ...)
/// ```
///
/// where `callee` is the function object (its captures follow the header) and
/// `this` is the NaN-boxed receiver bits ([`JsThis`]), passed in an INTEGER
/// register so every floating-point argument register stays free for JS
/// arguments (SysV x86-64 / AAPCS64; Win64 assigns positionally, which is
/// equally correct). Passing more JS arguments than a body declares is safe
/// (the caller owns the stack argument area); fewer is padded with
/// `undefined` by the caller. Its Rust type is [`js_body_fn_ty!`], defined
/// here and nowhere else. The runtime calls bodies only through
/// `closure/body_call.rs`; emitted code only through
/// `expr::body_call::emit_js_body_call`.
///
/// The `this` parameter is the only way a body learns its receiver: a
/// method-style caller passes the receiver, a plain call `undefined`.
pub const JS_BODY_CALLEE_PARAM: usize = 0;
/// Native parameter index of the receiver (`this`) bits.
pub const JS_BODY_THIS_PARAM: usize = 1;
/// Native parameter index of the first JS argument.
pub const JS_BODY_FIRST_ARG_PARAM: usize = 2;
/// Native parameters every JS body declares before its JS arguments.
pub const JS_BODY_FIXED_PARAMS: usize = 2;

/// NaN-boxed `undefined` (`value::TAG_UNDEFINED` in the runtime, which
/// asserts it equals this).
/// Runtime registry ids have their own quiet-NaN tag. 0x7FFB belongs to
/// perry-jsruntime's V8 handles; the negative qNaN word is distinct from
/// every positive tag and the canonical numeric NaN (0x7FF8).
pub const HANDLE_TAG: u64 = 0xFFF9_0000_0000_0000;

pub const TAG_UNDEFINED: u64 = 0x7FFC_0000_0000_0001;

/// The receiver a JS body takes as its second native parameter
/// ([`JS_BODY_THIS_PARAM`]): the NaN-boxed `this` bits, in an integer
/// register (`repr(transparent)` over `u64`, so its ABI is exactly a `u64`'s).
#[repr(transparent)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct JsThis(pub u64);

impl JsThis {
    /// `undefined`: the receiver of a plain (non-method) call.
    pub const UNDEFINED: JsThis = JsThis(TAG_UNDEFINED);

    /// The receiver bits.
    #[inline(always)]
    pub const fn bits(self) -> u64 {
        self.0
    }

    /// The receiver as a NaN-boxed value.
    #[inline(always)]
    pub fn as_f64(self) -> f64 {
        f64::from_bits(self.0)
    }

    /// A NaN-boxed value as a receiver.
    #[inline(always)]
    pub fn from_f64(value: f64) -> Self {
        JsThis(value.to_bits())
    }
}

/// THE Rust type of a JS body: `js_body_fn_ty!(Callee; a, b)` is
/// `unsafe extern "C" fn(*const Callee, JsThis, f64, f64) -> f64` — the
/// callee header type, then one `f64` per token. A safe `extern "C" fn` body
/// coerces to it.
#[macro_export]
macro_rules! js_body_fn_ty {
    (@f64 $x:tt) => { f64 };
    ($callee:ty; $($x:tt),* $(,)?) => {
        unsafe extern "C" fn(
            *const $callee,
            $crate::JsThis
            $(, $crate::js_body_fn_ty!(@f64 $x))*
        ) -> f64
    };
}

/// A JS body with a statically known JS arity: implemented for exactly the
/// [`js_body_fn_ty!`] pointer types (`JsBody0<C>` .. `JsBody32<C>`), so an API
/// taking `impl JsBody<C>` refuses any other signature — a bare `*const u8`,
/// a body without the receiver, a wrong argument type — at compile time.
///
/// # Safety
/// Implemented only here, for the body pointer types; `code` is the body's
/// entry address.
pub unsafe trait JsBody<C>: Copy {
    /// The JS parameters the body declares.
    const ARITY: u32;
    /// The body's code address, for the runtime's registries.
    fn code(self) -> *const u8;
}

/// [`js_body_fn_ty!`] with the `C-unwind` calling convention, for the few
/// runtime bodies a JS exception must be able to unwind through (the same
/// register ABI).
#[macro_export]
macro_rules! js_body_unwind_fn_ty {
    (@f64 $x:tt) => { f64 };
    ($callee:ty; $($x:tt),* $(,)?) => {
        unsafe extern "C-unwind" fn(
            *const $callee,
            $crate::JsThis
            $(, $crate::js_body_unwind_fn_ty!(@f64 $x))*
        ) -> f64
    };
}

macro_rules! js_body_types {
    ($($alias:ident = $n:literal [$($x:tt),*];)*) => {$(
        #[doc = concat!("A JS body declaring ", stringify!($n), " JS parameters.")]
        pub type $alias<C> = js_body_fn_ty!(C; $($x),*);
        // SAFETY: the pointer type is a JS body type by construction.
        unsafe impl<C> JsBody<C> for $alias<C> {
            const ARITY: u32 = $n;
            #[inline(always)]
            fn code(self) -> *const u8 {
                self as *const u8
            }
        }
    )*};
}

macro_rules! js_body_unwind_types {
    ($($alias:ident = $n:literal [$($x:tt),*];)*) => {$(
        #[doc = concat!("A `C-unwind` JS body declaring ", stringify!($n), " JS parameters.")]
        pub type $alias<C> = js_body_unwind_fn_ty!(C; $($x),*);
        // SAFETY: the pointer type is a JS body type by construction.
        unsafe impl<C> JsBody<C> for $alias<C> {
            const ARITY: u32 = $n;
            #[inline(always)]
            fn code(self) -> *const u8 {
                self as *const u8
            }
        }
    )*};
}

js_body_unwind_types! {
    JsBodyUnwind0 = 0 [];
    JsBodyUnwind1 = 1 [a];
    JsBodyUnwind2 = 2 [a, a];
    JsBodyUnwind3 = 3 [a, a, a];
    JsBodyUnwind4 = 4 [a, a, a, a];
    JsBodyUnwind5 = 5 [a, a, a, a, a];
    JsBodyUnwind6 = 6 [a, a, a, a, a, a];
    JsBodyUnwind7 = 7 [a, a, a, a, a, a, a];
    JsBodyUnwind8 = 8 [a, a, a, a, a, a, a, a];
    JsBodyUnwind9 = 9 [a, a, a, a, a, a, a, a, a];
    JsBodyUnwind10 = 10 [a, a, a, a, a, a, a, a, a, a];
    JsBodyUnwind11 = 11 [a, a, a, a, a, a, a, a, a, a, a];
    JsBodyUnwind12 = 12 [a, a, a, a, a, a, a, a, a, a, a, a];
    JsBodyUnwind13 = 13 [a, a, a, a, a, a, a, a, a, a, a, a, a];
    JsBodyUnwind14 = 14 [a, a, a, a, a, a, a, a, a, a, a, a, a, a];
    JsBodyUnwind15 = 15 [a, a, a, a, a, a, a, a, a, a, a, a, a, a, a];
    JsBodyUnwind16 = 16 [a, a, a, a, a, a, a, a, a, a, a, a, a, a, a, a];
}

js_body_types! {
    JsBody0 = 0 [];
    JsBody1 = 1 [a];
    JsBody2 = 2 [a, a];
    JsBody3 = 3 [a, a, a];
    JsBody4 = 4 [a, a, a, a];
    JsBody5 = 5 [a, a, a, a, a];
    JsBody6 = 6 [a, a, a, a, a, a];
    JsBody7 = 7 [a, a, a, a, a, a, a];
    JsBody8 = 8 [a, a, a, a, a, a, a, a];
    JsBody9 = 9 [a, a, a, a, a, a, a, a, a];
    JsBody10 = 10 [a, a, a, a, a, a, a, a, a, a];
    JsBody11 = 11 [a, a, a, a, a, a, a, a, a, a, a];
    JsBody12 = 12 [a, a, a, a, a, a, a, a, a, a, a, a];
    JsBody13 = 13 [a, a, a, a, a, a, a, a, a, a, a, a, a];
    JsBody14 = 14 [a, a, a, a, a, a, a, a, a, a, a, a, a, a];
    JsBody15 = 15 [a, a, a, a, a, a, a, a, a, a, a, a, a, a, a];
    JsBody16 = 16 [a, a, a, a, a, a, a, a, a, a, a, a, a, a, a, a];
    JsBody17 = 17 [a, a, a, a, a, a, a, a, a, a, a, a, a, a, a, a, a];
    JsBody18 = 18 [a, a, a, a, a, a, a, a, a, a, a, a, a, a, a, a, a, a];
    JsBody19 = 19 [a, a, a, a, a, a, a, a, a, a, a, a, a, a, a, a, a, a, a];
    JsBody20 = 20 [a, a, a, a, a, a, a, a, a, a, a, a, a, a, a, a, a, a, a, a];
    JsBody21 = 21 [a, a, a, a, a, a, a, a, a, a, a, a, a, a, a, a, a, a, a, a, a];
    JsBody22 = 22 [a, a, a, a, a, a, a, a, a, a, a, a, a, a, a, a, a, a, a, a, a, a];
    JsBody23 = 23 [a, a, a, a, a, a, a, a, a, a, a, a, a, a, a, a, a, a, a, a, a, a, a];
    JsBody24 = 24 [a, a, a, a, a, a, a, a, a, a, a, a, a, a, a, a, a, a, a, a, a, a, a, a];
    JsBody25 = 25 [a, a, a, a, a, a, a, a, a, a, a, a, a, a, a, a, a, a, a, a, a, a, a, a, a];
    JsBody26 = 26 [a, a, a, a, a, a, a, a, a, a, a, a, a, a, a, a, a, a, a, a, a, a, a, a, a, a];
    JsBody27 = 27 [a, a, a, a, a, a, a, a, a, a, a, a, a, a, a, a, a, a, a, a, a, a, a, a, a, a, a];
    JsBody28 = 28 [a, a, a, a, a, a, a, a, a, a, a, a, a, a, a, a, a, a, a, a, a, a, a, a, a, a, a, a];
    JsBody29 = 29 [a, a, a, a, a, a, a, a, a, a, a, a, a, a, a, a, a, a, a, a, a, a, a, a, a, a, a, a, a];
    JsBody30 = 30 [a, a, a, a, a, a, a, a, a, a, a, a, a, a, a, a, a, a, a, a, a, a, a, a, a, a, a, a, a, a];
    JsBody31 = 31 [a, a, a, a, a, a, a, a, a, a, a, a, a, a, a, a, a, a, a, a, a, a, a, a, a, a, a, a, a, a, a];
    JsBody32 = 32 [a, a, a, a, a, a, a, a, a, a, a, a, a, a, a, a, a, a, a, a, a, a, a, a, a, a, a, a, a, a, a, a];
}

/// Everything a function object's BODY determines, in one static, immutable
/// record per body (the SharedFunctionInfo model): the code address and the
/// facts every caller of the body needs. A function object points to its
/// body's info from its header ([`CLOSURE_INFO_OFFSET`]); nothing is ever
/// looked up by code address. Codegen emits one as a constant next to each
/// body; runtime natives and perry-ffi addons declare one as a `static`
/// ([`JsFunctionInfo::of`]), so the parameter count comes from the body's
/// type.
///
/// Layout (LP64, pinned by `JS_FUNCTION_INFO_*` below; codegen emits it as
/// `{ ptr, i16, i16, i32, i32, i32, ptr, i64, ptr, i32, i16, i16, i64 }`).
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct JsFunctionInfo {
    /// The body's code address (a function-table index on wasm32).
    pub code: *const u8,
    /// JS `f64` parameters the body declares after the callee and receiver —
    /// a rest body's rest array included. A caller passing fewer pads with
    /// `undefined` to this count.
    pub params: u16,
    /// Fixed parameters before the rest parameter (valid with `FN_REST_*`).
    pub rest_fixed: u16,
    /// `FN_*` bits.
    pub flags: u32,
    /// ECMAScript `.length` (valid with [`FN_HAS_LENGTH`]).
    pub length: u32,
    /// Captures the trusted direct-call clone was compiled for.
    pub trusted_captures: u32,
    /// A compiler-private direct-call clone of an arrow body that may use
    /// compiler-proven capture invariants (null when none).
    pub trusted_code: *const u8,
    /// The trusted clone's boxed-capture mask.
    pub trusted_boxed_mask: u64,
    /// A compiler-private versioned-loop callback clone (null when none).
    pub versioned_code: *const u8,
    /// Captures the versioned-loop clone was compiled for. When
    /// [`FN_HAS_SOURCE`] is set and `versioned_code` is null, this otherwise
    /// idle word instead carries the source byte length.
    pub versioned_captures: u32,
    /// The JS-visible declared parameter count (valid with
    /// [`FN_HAS_DECLARED`]): what `.length` falls back to. It can differ
    /// from `params`, the ABI width a caller pads to.
    pub declared: u16,
    /// One plus the fewest JS arguments a call may hand straight to `code`: `params + 1`
    /// for a PLAIN body, one nothing stands between a call and (no bound
    /// value, no rest or `arguments` bundling, compiled from JS source), else
    /// zero, the legacy padding value, when no direct call is permitted. It is written with
    /// `code`, once, when the info is born: codegen renders it for each body
    /// it emits, and every info the runtime or an addon builds is
    /// zero. A call passing at least the decoded count jumps to
    /// `code` with its registers untouched (`js_closure_call{N}`). Other compiled
    /// calls enter `call_entry`; legacy/native records take the dispatcher. Private, so an info can only be built
    /// through [`JsFunctionInfo::of`] (typed) or the `unsafe`
    /// [`JsFunctionInfo::from_code`], and made plain only by
    /// [`JsFunctionInfo::plain`].
    plain_params: u16,
    /// The versioned-loop clone's boxed-capture mask. When
    /// [`FN_HAS_SOURCE`] is set and `versioned_code` is null, this otherwise
    /// idle word instead carries the signed 64-bit raw-source displacement,
    /// or the two 32-bit source-block parts licensed by FN_SOURCE_BLOCKS.
    pub versioned_boxed_mask: u64,
}

// SAFETY: immutable after construction; the pointers are code addresses.
unsafe impl Sync for JsFunctionInfo {}

/// `JsFunctionInfo::flags`: the body has a rest parameter bundling every JS
/// argument from `rest_fixed` on (`...rest`).
pub const FN_REST_USER: u32 = 1 << 0;
/// The body takes a synthetic `arguments` array of every argument.
pub const FN_REST_SYNTHETIC_ARGUMENTS: u32 = 1 << 1;
/// The body takes both a `...rest` array and a synthetic `arguments` array.
pub const FN_REST_USER_AND_ARGUMENTS: u32 = 1 << 2;
/// A runtime-native body that takes the call's arguments in place, as
/// [`JsNativeArgsBody`] `(callee, this, args, len)`: no array is built for
/// them. `rest_fixed` is its JS-visible declared count.
pub const FN_REST_NATIVE_ARGS: u32 = 1 << 16;
/// The builtin body resolves an Array `this` to its live forwarding head
/// before using it. A receiver-validated site may supply the head it already
/// proved, avoiding a second walk. This does not waive its descriptor,
/// prototype, frozen or extensibility checks, or change generic callers.
pub const FN_RESOLVES_ARRAY_THIS: u32 = 1 << 17;
/// Synthetic-arguments bodies that materialize an Arguments object accept
/// one born directly from the dispatcher's argument list instead of an Array.
pub const FN_ARGUMENTS_OBJECT: u32 = 1 << 18;
/// The directly born object's `callee` is restricted.
pub const FN_ARGUMENTS_RESTRICTED: u32 = 1 << 19;
/// The directly born object needs slots for mapped parameter aliases.
pub const FN_ARGUMENTS_MAPPED: u32 = 1 << 20;
/// Any rest kind.
pub const FN_REST_MASK: u32 =
    FN_REST_USER | FN_REST_SYNTHETIC_ARGUMENTS | FN_REST_USER_AND_ARGUMENTS | FN_REST_NATIVE_ARGS;

/// The native type of an [`FN_REST_NATIVE_ARGS`] body: the callee, the
/// receiver, and the call's `len` arguments at `args` (null when `len` is 0),
/// valid for the duration of the call.
///
/// The buffer is the caller's own argument storage (a stack buffer, a
/// register spill, a `Vec`), never GC-heap array storage, so the collector
/// neither scans nor rewrites it: the values in it are current only until the
/// body's first operation that can collect. A body that needs them after such
/// an operation roots them first (`RuntimeHandleScope::root_nanbox_f64_slice`).
/// The body must not keep the pointer, store it anywhere, or let it escape the
/// call.
pub type JsNativeArgsBody<C> = unsafe extern "C" fn(*const C, JsThis, *const f64, usize) -> f64;
/// The generic entry of a compiled function. Argument cells belong to the
/// caller and may be rewritten by moving GC; the callee roots these cells in
/// place before allocating and never retains their addresses after returning.
pub type JsCallEntry<C> =
    unsafe extern "C" fn(*const C, JsThis, *const core::cell::UnsafeCell<f64>, usize) -> f64;
/// `length` is valid.
pub const FN_HAS_LENGTH: u32 = 1 << 3;
/// An arrow function: lexical `this`, not constructable.
pub const FN_ARROW: u32 = 1 << 4;
/// Strict-mode code (a primitive receiver is not boxed).
pub const FN_STRICT: u32 = 1 << 5;
/// An async function (async generators set it too).
pub const FN_ASYNC: u32 = 1 << 6;
/// A generator function (async generators set it too).
pub const FN_GENERATOR: u32 = 1 << 7;
/// An `async function*`.
pub const FN_ASYNC_GENERATOR: u32 = 1 << 8;
/// A built-in function kind without `[[Construct]]`.
pub const FN_NON_CONSTRUCTOR: u32 = 1 << 9;
/// A runtime-native built-in (its `[[Call]]` skips OrdinaryCallBindThis).
pub const FN_BUILTIN: u32 = 1 << 10;
/// `declared` is valid.
pub const FN_HAS_DECLARED: u32 = 1 << 11;
/// Body metadata and code are linked into a permanent executable image.
/// Dylib bodies omit this bit: a shape must not retain their info address
/// beyond `dlclose` or mistake a reused address for the same body.
pub const FN_PERMANENT_IMAGE: u32 = 1 << 12;
/// The compiler emitted this body from JavaScript source (every info
/// `perry-codegen` renders carries it; no runtime-native info does). A
/// function object on such a body is never a built-in, bound, native-module
/// or class constructor, so its `[[Construct]]` and `instanceof` are the
/// ordinary ones: the runtime decides that from this bit, once per body,
/// instead of probing the callee against every built-in on each use.
pub const FN_COMPILED_BODY: u32 = 1 << 13;
/// A compiler-emitted info carries retained source. When `versioned_code` is
/// null, its otherwise idle versioned-captures fields carry byte length and
/// signed displacement; an info that also has a versioned clone is followed
/// by two `i32`s (four with FN_SOURCE_BLOCKS). Runtime/native infos stay
/// [`JS_FUNCTION_INFO_SIZE`] bytes.
pub const FN_HAS_SOURCE: u32 = 1 << 14;
/// The retained source describes an ordinary non-strict function. Methods,
/// arrows and strict ordinary functions leave this clear.
pub const FN_NON_STRICT_ORDINARY: u32 = 1 << 15;
/// Compiled name bytes follow the info (and any retained-source tail) as
/// an i32 relative displacement and a u32 byte length. No GC edge or birth work.
pub const FN_HAS_NAME: u32 = 1 << 21;
/// With [`FN_HAS_NAME`]: the name bytes are the text of an immutable image
/// string, whose `StringHeader` immediately precedes them. That string is
/// the function's own `name` value, static data of its body.
pub const FN_NAME_IMAGE: u32 = 1 << 22;
/// An immutable constructor-layout record follows source/name tails.
pub const FN_HAS_CONSTRUCTOR_LAYOUT: u32 = 1 << 23;
/// With FN_HAS_SOURCE: the range names independent raw-DEFLATE frames.
/// The idle mask contains `{ i32 block displacement, u32 decoded offset }`;
/// versioned_captures remains the source length. A live versioned clone uses
/// an optional `{ i32 displacement, u32 offset, u32 length, u32 reserved }` tail.
/// This is an immutable representation discriminator, not a validity bit.
pub const FN_SOURCE_BLOCKS: u32 = 1 << 24;
/// Independent retained-source frames bound uncached small-range decoding.
pub const RETAINED_SOURCE_BLOCK_BYTES: usize = 4096;
/// A function whose own source is at most this long keeps a raw copy that its
/// info names directly; longer sources are read from the frames.
pub const RETAINED_SOURCE_RAW_MAX_BYTES: usize = 512;

/// Size of the optional source tail; the common source uses idle base fields.
#[inline(always)]
pub fn function_source_tail_size(info: &JsFunctionInfo) -> usize {
    if info.flags & FN_HAS_SOURCE == 0 || info.versioned_code.is_null() {
        0
    } else if info.flags & FN_SOURCE_BLOCKS != 0 {
        16
    } else {
        8
    }
}

/// The top nibble holds the generic-entry word's offset in eight-byte units.
/// Zero means absent. The record describes its own layout: admission and the
/// entry load need no source/name/constructor-tail decoding.
pub const FN_CALL_ENTRY_SHIFT: u32 = 28;

/// Byte offsets of the fields codegen emits and emitted code reads.
pub const JS_FUNCTION_INFO_CODE_OFFSET: usize = 0;
pub const JS_FUNCTION_INFO_PARAMS_OFFSET: usize = 8;
pub const JS_FUNCTION_INFO_FLAGS_OFFSET: usize = 12;
pub const JS_FUNCTION_INFO_SIZE: usize = 64;
/// Former zero padding; nonzero records opt in to plain closure dispatch.
pub const JS_FUNCTION_INFO_PLAIN_PARAMS_OFFSET: usize =
    core::mem::offset_of!(JsFunctionInfo, plain_params);

/// [`JsFunctionInfo::plain_params`] of a body no call reaches directly.
pub const NOT_PLAIN: u16 = u16::MAX;

impl JsFunctionInfo {
    /// The callee's argument-vector entry, directly in this immutable record.
    /// Runtime-native records omit it. Source, name and constructor operands
    /// retain their established offsets; the entry is the final optional word.
    #[inline(always)]
    pub fn call_entry(&self) -> Option<*const u8> {
        let words = self.flags >> FN_CALL_ENTRY_SHIFT;
        if words == 0 {
            return None;
        }
        // SAFETY: codegen publishes this offset with the immutable entry word.
        Some(unsafe {
            (self as *const Self)
                .cast::<u8>()
                .add(words as usize * 8)
                .cast::<*const u8>()
                .read()
        })
    }

    /// The info of the body at `code` declaring `params` JS parameters, with
    /// no other facts.
    ///
    /// # Safety
    /// `code` is a JS body (`js_body_fn_ty!`) declaring exactly `params` JS
    /// parameters — or one of the runtime's bound-value sentinels. Prefer
    /// [`JsFunctionInfo::of`], which takes both from the body's type.
    pub const unsafe fn from_code(code: *const u8, params: u16) -> Self {
        JsFunctionInfo {
            code,
            params,
            rest_fixed: 0,
            flags: 0,
            length: 0,
            trusted_captures: 0,
            trusted_code: core::ptr::null(),
            trusted_boxed_mask: 0,
            versioned_code: core::ptr::null(),
            versioned_captures: 0,
            declared: 0,
            plain_params: 0,
            versioned_boxed_mask: 0,
        }
    }

    /// The info of the typed body `body`: its code address and parameter
    /// count both come from `body` itself —
    /// `static INFO: JsFunctionInfo = JsFunctionInfo::of(body as JsBody2<C>);`
    /// A body of any other signature does not compile.
    pub const fn of<C, F: JsBody<C>>(body: F) -> Self {
        // A `JsBody` is a function pointer: reinterpret it as its address.
        union Code<F: Copy> {
            body: F,
            code: *const u8,
        }
        // SAFETY: `F` is a fn-pointer type (the trait is implemented for
        // nothing else), the same size and bits as a code pointer.
        let code = unsafe { Code { body }.code };
        // SAFETY: `code` is `body`, a JS body of `F::ARITY` parameters.
        unsafe { Self::from_code(code, F::ARITY as u16) }
    }

    /// The info of the native-arguments body `body` ([`FN_REST_NATIVE_ARGS`]),
    /// declaring `declared` JS-visible parameters.
    pub const fn of_native_args<C>(body: JsNativeArgsBody<C>, declared: u16) -> Self {
        // SAFETY: `body` is a fn pointer, the same size and bits as a code
        // pointer; the rest bit routes every call through the runtime's
        // native-arguments arm, never the `f64`-per-parameter body ABI.
        union Code<C> {
            body: JsNativeArgsBody<C>,
            code: *const u8,
        }
        let code = unsafe { Code { body }.code };
        let mut info = unsafe { Self::from_code(code, 0) };
        info.rest_fixed = declared;
        info.flags = FN_REST_NATIVE_ARGS;
        info
    }

    /// [`JsFunctionInfo::plain_params`]: the fewest arguments a call may hand
    /// straight to `code`, or [`NOT_PLAIN`].
    pub const fn plain_params(&self) -> u16 {
        self.plain_params.wrapping_sub(1)
    }

    /// A plain body: a call passing at least `params` arguments jumps to
    /// `code` with nothing in between. A body with a rest kind stays
    /// [`NOT_PLAIN`] (its arguments are bundled first). Adding a rest kind
    /// later also revokes direct-call eligibility.
    pub const fn plain(mut self) -> Self {
        self.plain_params = if self.flags & FN_REST_MASK == 0 {
            self.params.wrapping_add(1)
        } else {
            0
        };
        self
    }

    /// With `FN_*` bits set.
    pub const fn with_flags(mut self, flags: u32) -> Self {
        self.flags |= flags;
        if self.flags & FN_REST_MASK != 0 {
            self.plain_params = 0;
        }
        self
    }

    /// With a JS-visible declared parameter count (`.length`'s fallback).
    pub const fn with_declared(mut self, declared: u16) -> Self {
        self.declared = declared;
        self.flags |= FN_HAS_DECLARED;
        self
    }

    /// With an ECMAScript `.length`.
    pub const fn with_length(mut self, length: u32) -> Self {
        self.length = length;
        self.flags |= FN_HAS_LENGTH;
        self
    }

    /// A `...rest` body with `fixed` parameters before the rest array.
    pub const fn with_rest(mut self, fixed: u16) -> Self {
        self.rest_fixed = fixed;
        self.flags = (self.flags & !FN_REST_MASK) | FN_REST_USER;
        self.plain_params = 0;
        self
    }

    /// A body of rest `kind` (one `FN_REST_*` bit) with `fixed` parameters
    /// before its rest / `arguments` array.
    pub const fn with_rest_kind(mut self, fixed: u16, kind: u32) -> Self {
        self.rest_fixed = fixed;
        self.flags = (self.flags & !FN_REST_MASK) | kind;
        self.plain_params = 0;
        self
    }

    /// With a trusted direct-call clone of this arrow body, compiled for
    /// `captures` captures with `boxed_mask` boxed ones.
    pub const fn with_trusted_direct(
        mut self,
        code: *const u8,
        captures: u32,
        boxed_mask: u64,
    ) -> Self {
        self.trusted_code = code;
        self.trusted_captures = captures;
        self.trusted_boxed_mask = boxed_mask;
        self
    }

    /// With a versioned-loop callback clone of this arrow body.
    pub const fn with_versioned_loop(
        mut self,
        code: *const u8,
        captures: u32,
        boxed_mask: u64,
    ) -> Self {
        self.versioned_code = code;
        self.versioned_captures = captures;
        self.versioned_boxed_mask = boxed_mask;
        self
    }
}

/// `js_closure_call{N}(callee, this, a0..aN-1)` calls a function object with
/// receiver `this` ([`JsThis::UNDEFINED`] for a plain call); it exists for
/// `N <= JS_CLOSURE_CALL_MAX_ARGS`, and wider calls use
/// `js_closure_call_array(callee, this, args, len)`.
pub const JS_CLOSURE_CALL_MAX_ARGS: usize = 16;
/// The fixed-arity entries, indexed by JS argument count.
pub const JS_CLOSURE_CALL_ENTRIES: [&str; JS_CLOSURE_CALL_MAX_ARGS + 1] = [
    "js_closure_call0",
    "js_closure_call1",
    "js_closure_call2",
    "js_closure_call3",
    "js_closure_call4",
    "js_closure_call5",
    "js_closure_call6",
    "js_closure_call7",
    "js_closure_call8",
    "js_closure_call9",
    "js_closure_call10",
    "js_closure_call11",
    "js_closure_call12",
    "js_closure_call13",
    "js_closure_call14",
    "js_closure_call15",
    "js_closure_call16",
];
/// Every runtime entry point native code (emitted or Rust) calls to run a JS
/// function. Each takes the receiver after the function, can run arbitrary JS
/// and therefore collect: `scripts/gc_root_dominance_check.py` reads its
/// poll-capable set from THIS list.
pub const JS_CALL_ENTRIES: [&str; JS_CLOSURE_CALL_MAX_ARGS + 1 + 4] = [
    "js_closure_call0",
    "js_closure_call1",
    "js_closure_call2",
    "js_closure_call3",
    "js_closure_call4",
    "js_closure_call5",
    "js_closure_call6",
    "js_closure_call7",
    "js_closure_call8",
    "js_closure_call9",
    "js_closure_call10",
    "js_closure_call11",
    "js_closure_call12",
    "js_closure_call13",
    "js_closure_call14",
    "js_closure_call15",
    "js_closure_call16",
    "js_closure_call_array",
    "js_closure_call_apply_with_spread",
    "js_native_call_value",
    // V8's callback trampoline contract (`func(env, args, len)`, no
    // receiver): a plain call.
    "js_closure_v8_callback",
];

/// What `js_method_lookup` / `js_method_prepare` answer for a call whose
/// method read nothing can observe: dispatch by name after the arguments
/// (#11910). The array-hole marker, a bit pattern no JS value takes.
pub const METHOD_SITE_BY_NAME: u64 = 0x7FFC_0000_0000_0010;
/// The by-name answer for a receiver whose methods the universal dispatcher
/// answers (a primitive, a native cell). In the same never-a-value namespace;
/// `bits | 2 == METHOD_SITE_BY_NAME_DIRECT` tests for either by-name answer.
pub const METHOD_SITE_BY_NAME_DIRECT: u64 = 0x7FFC_0000_0000_0012;
const _: () = assert!(METHOD_SITE_BY_NAME | 2 == METHOD_SITE_BY_NAME_DIRECT);
/// `object::ObjectMeta::spill` (the object-owned overflow buffer).
pub const OBJECT_META_SPILL_OFFSET: usize = 32;

/// `closure::CAPTURES_THIS_FLAG` / `closure::NO_THIS_REBIND_FLAG`, the high
/// bits of `ClosureHeader::capture_count`. A closure with the first and not
/// the second is a rebindable `this` clone, which never satisfies a ConstFn
/// claim (`field_rep_store::constfn_store_info`).
pub const CLOSURE_CAPTURES_THIS_FLAG: u32 = 0x8000_0000;
pub const CLOSURE_NO_THIS_REBIND_FLAG: u32 = 0x4000_0000;
/// Immutable builtin [[Construct]] identity. No closure layout growth.
pub const CLOSURE_CONSTRUCT_SHIFT: u32 = 23;
pub const CLOSURE_CONSTRUCT_MASK: u32 = 0x3f80_0000;
pub const CLOSURE_CAPTURE_COUNT_MASK: u32 = 0x007f_ffff;
/// Encode an ordinary capture count before adding either `this` flag.
/// This check must survive release builds: truncation could forge an identity.
#[inline]
pub fn checked_closure_capture_count(count: usize) -> u32 {
    assert!(
        count <= CLOSURE_CAPTURE_COUNT_MASK as usize,
        "closure capture count exceeds the 23-bit header capacity"
    );
    count as u32
}

const _: () = assert!(
    CLOSURE_CONSTRUCT_MASK
        | CLOSURE_CAPTURE_COUNT_MASK
        | CLOSURE_CAPTURES_THIS_FLAG
        | CLOSURE_NO_THIS_REBIND_FLAG
        == u32::MAX
);

// Unified byte cell: the type byte carries the element brand and view role.
pub const BYTES_LEN: usize = 0;
pub const BYTES_AUX: usize = 4;
pub const BYTES_LINK: usize = 8;
pub const BYTES_STORE: usize = 16;
/// A view whose link is its property bag finds its owner, NaN-boxed, in the
/// bag's first inline slot: the bag is born holding `#<perry:view-owner>` as
/// its first key with one inline slot, so the owner sits right after the
/// 16-byte `ObjectHeader` on every target. Emitted view resolution and
/// `buffer::store::owner` read it with one load, never a key lookup.
pub const BYTES_VIEW_BAG_OWNER: usize = 16;
pub const BYTES_TYPE_BASE: u8 = 0x40;
pub const BYTES_TYPE_VIEW: u8 = 0x20;
pub const BYTES_TYPE_BRAND_MASK: u8 = 0x1f;
pub const BYTES_OUT_OF_LINE: u16 = 1 << 7;
pub const BYTES_LENGTH_TRACKING: u16 = 1 << 7;
pub const BYTES_RESIZABLE: u16 = 1 << 8;
pub const BYTES_DETACHED: u16 = 1 << 14;
pub const BYTES_ELEMENT_SHIFT: [u8; 19] = [0, 0, 0, 1, 1, 2, 2, 1, 2, 3, 3, 3, 0, 0, 0, 0, 0, 0, 0];

pub mod component_guest;
pub mod component_host;
pub mod component_io;
pub mod native_class_ids;
pub mod wasm_host;

/// Slot 3: address of the existing compiled-class function directory.
pub const AGENT_PTR_CLASS_VALUES: usize = 3;

/// Declaration class function capture holding its immutable prototype link.
pub const CLASS_PROTOTYPE_LINK_CAPTURE: usize = 2;
/// A class function object's actual mutable [[Prototype]], a traced own edge.
pub const CLASS_CONSTRUCTOR_PARENT_CAPTURE: usize = 3;

/// `PERRY_NANBOX_OPERANDS` (`perry-runtime/src/value/tags.rs`): the read-only
/// table x86-64 generated code reads its most frequent 64-bit operands from
/// instead of encoding each use as a 10-byte `movabs`
/// (`perry-codegen/src/inprocess/nanbox_operands.rs`): NaN-box tags, masks
/// and their negations, and the method-site word masks. Every entry needs a
/// 64-bit immediate; the set is the head of the `movabs` census of compiled
/// programs (Claude Code, tsc). Codegen finds an operand's entry by value,
/// so order and length are free to change; the runtime asserts the NaN-box
/// entries against its own constants.
pub const NANBOX_OPERANDS: [u64; 26] = [
    0x7FFD_0000_0000_0000, // POINTER_TAG
    0x7FFC_0000_0000_0010, // TAG_HOLE (also METHOD_SITE_BY_NAME)
    0xFFFF_0000_0000_0000, // TAG_MASK
    0x7FF8_0000_0000_0000, // the canonical NaN
    TAG_UNDEFINED,
    TAG_UNDEFINED.wrapping_neg(),
    0x7FFD_0000_0000_0000_u64.wrapping_neg(), // -POINTER_TAG: payload = bits - POINTER_TAG
    0x7FFF_0000_0000_0000,                    // STRING_TAG
    (1 << 48) - RECEIVER_HANDLE_FLOOR as u64, // pointer payload span above the handle band
    (1 << 47) - RECEIVER_HANDLE_FLOOR as u64, // user address span above the handle band
    0x7FF9_0000_0000_0000,                    // SHORT_STRING_TAG
    0x7FFC_0000_0000_0004,                    // TAG_TRUE
    (0x7FFD_0000_0000_0000 + RECEIVER_HANDLE_FLOOR as u64).wrapping_neg(), // -(POINTER_TAG + floor)
    0xFFFF_FFFF_0000_0000,                    // the ShapeId half of an object's header word
    !(1 << 60),
    1 << 62,                     // bit 62, the token bit of a static ShapeId operand
    0xFFFD_0000_0000_0000,       // collapses POINTER_TAG and STRING_TAG
    0x7FFE_0000_0000_0000,       // INT32_TAG
    (1 << 48) - 4096,            // masks a 48-bit address down to its page
    (0xB800_0000_u64 << 32) - 1, // below the exotic ShapeId band
    1 << 61,
    0x7FFA_0000_0000_0000, // BIGINT_TAG
    0x7FFC_0000_0000_0011, // TAG_TDZ
    0x7FFC_0000_0000_0003, // TAG_FALSE
    0x7FFC_0000_0000_0002, // TAG_NULL
    0x7FFC_0000_0000_0000, // TAG_MARKER
];

pub mod wasi;

#[cfg(test)]
mod call_entry_layout_tests {
    use super::*;

    extern "C" fn operation(_: *const (), _: JsThis) -> f64 {
        0.0
    }
    extern "C" fn entry(
        _: *const (),
        _: JsThis,
        _: *const core::cell::UnsafeCell<f64>,
        _: usize,
    ) -> f64 {
        0.0
    }

    #[repr(C)]
    struct EntryOnly {
        info: JsFunctionInfo,
        entry: JsCallEntry<()>,
    }
    #[repr(C)]
    struct EntryWithTails {
        info: JsFunctionInfo,
        source: [u32; 2],
        name: [u32; 2],
        constructor: (*const u8, *const u8, u32, u32),
        entry: JsCallEntry<()>,
    }

    #[test]
    fn the_entry_word_follows_existing_optional_metadata() {
        let info = JsFunctionInfo::of(operation as JsBody0<()>);
        assert!(info.call_entry().is_none());
        let plain = EntryOnly {
            info: info.with_flags(
                ((core::mem::offset_of!(EntryOnly, entry) / 8) as u32) << FN_CALL_ENTRY_SHIFT,
            ),
            entry,
        };
        assert_eq!(plain.info.call_entry(), Some(entry as *const u8));
        // Keep a live clone so source uses the optional range, rather than
        // its packed representation in the otherwise idle clone fields.
        let with_tails = EntryWithTails {
            info: info
                .with_flags(
                    (((core::mem::offset_of!(EntryWithTails, entry) / 8) as u32)
                        << FN_CALL_ENTRY_SHIFT)
                        | FN_HAS_SOURCE
                        | FN_HAS_NAME
                        | FN_HAS_CONSTRUCTOR_LAYOUT,
                )
                .with_versioned_loop(operation as *const u8, 0, 0),
            source: [0; 2],
            name: [0; 2],
            constructor: (core::ptr::null(), core::ptr::null(), 0, 0),
            entry,
        };
        assert_eq!(with_tails.info.call_entry(), Some(entry as *const u8));
        assert_eq!(
            core::mem::size_of::<JsFunctionInfo>(),
            JS_FUNCTION_INFO_SIZE
        );
    }
}

/// Frame-owned wasm EH checkpoint. Fixed-width depths keep this POD layout
/// identical in the host compiler and the wasm32 runtime. No managed values.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct WasmEhCheckpoint {
    pub handles: u64,
    pub shadow_frame_top: u64,
    pub shadow_len: u64,
    pub async_context: u64,
    pub temp_roots: u64,
    pub call_method: u64,
    pub pump: u64,
    pub set_foreach: u64,
    pub map_foreach: u64,
    pub prototype_resolution: u64,
    pub static_private_owner: u64,
    pub private_lexical_brand: u64,
    pub derived_super_binding: u64,
    pub private_member_access_hints: u64,
    pub namespace_override: u64,
}

/// One canonical key's immutable attribute word, including cumulative prefix
/// summaries/Bloom filters and an optional descriptor storage slot. Shared by
/// heap writers and image-cell emitters; no key spelling is repeated here.
pub const fn canonical_key_attribute_word(
    previous: u64,
    entry: u8,
    bloom: u16,
    slot: Option<u32>,
) -> u64 {
    let mut summary = (previous >> 8) as u8;
    let mut entries = (previous >> 16) as u16;
    let mut accessors = (previous >> 32) as u16;
    if summary & 0x80 != 0 {
        summary &= !0x80;
        entries = u16::MAX;
        accessors = u16::MAX;
    }
    if entry & 8 != 0 {
        summary |= 1;
    } else if entry & 1 != 0 {
        summary |= 2;
    }
    if entry & 2 != 0 {
        summary |= 4;
    }
    if entry & 4 != 0 {
        summary |= 8;
    }
    if entry & 0x40 != 0 {
        summary |= 0x10;
    }
    if entry & 0x80 != 0 {
        summary |= 0x20;
    }
    if entry != 0 {
        entries |= bloom;
        if entry & 8 != 0 {
            accessors |= bloom;
        }
    }
    if let Some(slot) = slot {
        summary |= 0xc0;
        entries = slot as u16;
        accessors = (slot >> 16) as u16;
    }
    0x7ffe_0000_0000_0000
        | entry as u64
        | (summary as u64) << 8
        | (entries as u64) << 16
        | (accessors as u64) << 32
}
