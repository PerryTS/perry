#![no_std]
//! Runtime layout facts that generated code bakes in, in ONE file both sides
//! depend on: `perry-runtime` (as `crate::codegen_abi`) and `perry-codegen`
//! (as `crate::runtime_abi`). A layout change edits a number here, and the
//! runtime's `offset_of!`/`size_of` assertions next to each struct refuse to
//! compile until the number is right — so emitted code can never disagree
//! with the struct it indexes. No dependencies.

/// `array::ArrayHeader` size: element 0 follows it.
pub const ARRAY_HEADER_SIZE: usize = 8;

/// `agent_ptrs::PERRY_AGENT_PTRS`: the number of per-agent pointer slots.
/// Slot 0 is reserved (the megamorphic follow-up's shape-record directory);
/// slot 1 held the implicit-`this` cell's address until this-as-a-parameter
/// deleted the cell, and is free; slot 2 is the stack limit.
pub const AGENT_PTR_SLOTS: usize = 4;
/// Slot 2: this agent's stack limit (#10812) — not a pointer to anything, the
/// lowest frame address a compiled prologue accepts before it throws
/// `RangeError: Maximum call stack size exceeded`. Null means unchecked.
pub const AGENT_PTR_STACK_LIMIT: usize = 2;
/// `tls_hot::HotTls::agent_ptrs` (Apple aarch64 TSD path; LP64): the first
/// inline value, behind fixed-size fields only.
pub const HOT_TLS_AGENT_PTRS_OFFSET: usize = 128;

/// `closure::ClosureHeader` (LP64): the u32 capture count at 0, the ShapeId
/// at 4 (the same word as `ObjectHeader`), the code pointer at 8, the shaped
/// own-property record at 16, captures from 24. ILP32 targets shrink the two
/// pointers: code pointer at 8, props at 12, captures from 16 (derived in
/// `perry-codegen/src/target_layout.rs`).
pub const CLOSURE_SHAPE_OFFSET: usize = 4;
pub const CLOSURE_FUNC_PTR_OFFSET: usize = 8;
pub const CLOSURE_PROPS_OFFSET: usize = 16;
pub const CLOSURE_HEADER_SIZE: usize = 24;

/// `gc::GC_TYPE_CLOSURE`: the GcHeader type byte (at payload - 8) that makes a
/// cell a function object. The kind is this byte, never a payload magic.
pub const GC_TYPE_CLOSURE: u8 = 4;
/// `gc::GC_FLAG_FORWARDED` (GcHeader byte 1): an evacuated from-space stub.
pub const GC_FLAG_FORWARDED: u8 = 0x80;
/// `gc::GC_HEADER_SIZE`.
pub const GC_HEADER_SIZE: usize = 8;

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
/// [`js_body_fn_ty!`] pointer types (`JsBody0<C>` .. `JsBody16<C>`), so an API
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
/// `object::method_site::MethodEntry` — the words the emitted method-call site
/// reads (`perry-codegen/src/expr/method_site.rs`).
pub const METHOD_SITE_WORD_OFFSET: usize = 0;
pub const METHOD_SITE_SLOT_OFFSET: usize = 8;
pub const METHOD_SITE_FUNC_OFFSET: usize = 16;
pub const METHOD_SITE_CLOSURE_OFFSET: usize = 24;
pub const METHOD_SITE_GEN_OFFSET: usize = 32;
/// Entries per method site, and one entry's size.
pub const METHOD_SITE_WAYS: usize = 2;
pub const METHOD_SITE_ENTRY_SIZE: usize = 40;
/// A method site calls a body with its argument count padded by `undefined`
/// up to this many extra arguments (never past 16), and admits bodies that
/// declare up to that many parameters.
pub const METHOD_SITE_ARG_PAD: usize = 3;
pub const fn method_site_padded_argc(argc: usize) -> usize {
    let padded = argc + METHOD_SITE_ARG_PAD;
    if padded > 16 {
        if argc > 16 {
            argc
        } else {
            16
        }
    } else {
        padded
    }
}
/// The entry `slot` bit for an own key in the receiver's spill buffer.
pub const METHOD_SITE_SPILL: u64 = 1 << 62;
/// The entry `slot` bit for an own key of a function-object receiver: an
/// inline slot of the object at `ClosureHeader::props`.
pub const METHOD_SITE_FUNCTION_BAG: u64 = 1 << 61;
/// The index bits of an entry's `slot` word (bit 60 is reserved for the
/// accessor entry kind).
pub const METHOD_SITE_INDEX_MASK: u64 = (1 << 60) - 1;
/// `object::ObjectMeta::spill` (the object-owned overflow buffer).
pub const OBJECT_META_SPILL_OFFSET: usize = 32;
