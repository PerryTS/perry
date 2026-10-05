#![no_std]
//! Runtime layout facts that generated code bakes in, in ONE file both sides
//! depend on: `perry-runtime` (as `crate::codegen_abi`) and `perry-codegen`
//! (as `crate::runtime_abi`). A layout change edits a number here, and the
//! runtime's `offset_of!`/`size_of` assertions next to each struct refuse to
//! compile until the number is right — so emitted code can never disagree
//! with the struct it indexes. No dependencies.

/// `object::shapes::SHAPE_ID_BASE`: the first ShapeId.
pub const SHAPE_ID_BASE: u32 = 0x8000_0000;
/// The compiler-assigned ("static") ShapeId band is
/// `[SHAPE_ID_BASE, SHAPE_ID_BASE + STATIC_SHAPE_ID_COUNT)`: the driver
/// assigns ids there by content and generated code embeds them as immediates;
/// the runtime's counter never draws from it (design step 4).
pub const STATIC_SHAPE_ID_COUNT: u32 = 1 << 20;

/// `array::ArrayHeader` size: element 0 follows it.
pub const ARRAY_HEADER_SIZE: usize = 8;

/// `agent_ptrs::PERRY_AGENT_PTRS`: the number of per-agent pointer slots.
pub const AGENT_PTR_SLOTS: usize = 4;
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
pub const RECEIVER_HANDLE_FLOOR: usize = 0x10_0000;
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
/// `gc::GC_TYPE_BUFFER` and `gc::GC_TYPE_BUFFER_UINT8ARRAY`: the GcHeader type
/// bytes of the two BYTE-VIEW buffer brands, a Node `Buffer` and a
/// `BufferHeader`-backed `Uint8Array` (#10694: a buffer's flavor is its type
/// byte). Emitted byte-access guards accept exactly these two.
pub const GC_TYPE_BUFFER: u8 = 10;
pub const GC_TYPE_BUFFER_UINT8ARRAY: u8 = 26;
/// A buffer view stores its resolved native byte pointer at payload +8.
/// The backing GC edge remains traced by the buffer descriptor. This bit
/// never admits a view to the owning-buffer inline-storage cache.
pub const GC_BUFFER_VIEW_DATA: u16 = 0x100;
pub const BUFFER_VIEW_DATA_OFFSET: usize = 8;
/// Header-less symbols must be screened before trusting a buffer layout.
pub const SYMBOL_HEADER_MAGIC: u32 = 0x5359_4D42;
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
    /// Padding (zero). Private, so an info can only be built through
    /// [`JsFunctionInfo::of`] (typed) or the `unsafe`
    /// [`JsFunctionInfo::from_code`].
    reserved: u16,
    /// The versioned-loop clone's boxed-capture mask. When
    /// [`FN_HAS_SOURCE`] is set and `versioned_code` is null, this otherwise
    /// idle word instead carries the signed 64-bit source displacement.
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
/// Any rest kind.
pub const FN_REST_MASK: u32 =
    FN_REST_USER | FN_REST_SYNTHETIC_ARGUMENTS | FN_REST_USER_AND_ARGUMENTS;
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
/// by two `i32`s. Runtime/native infos stay [`JS_FUNCTION_INFO_SIZE`] bytes.
pub const FN_HAS_SOURCE: u32 = 1 << 14;
/// The retained source describes an ordinary non-strict function. Methods,
/// arrows and strict ordinary functions leave this clear.
pub const FN_NON_STRICT_ORDINARY: u32 = 1 << 15;

/// Byte offsets of the fields codegen emits and emitted code reads.
pub const JS_FUNCTION_INFO_CODE_OFFSET: usize = 0;
pub const JS_FUNCTION_INFO_PARAMS_OFFSET: usize = 8;
pub const JS_FUNCTION_INFO_FLAGS_OFFSET: usize = 12;
pub const JS_FUNCTION_INFO_SIZE: usize = 64;

impl JsFunctionInfo {
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
            reserved: 0,
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

    /// With `FN_*` bits set.
    pub const fn with_flags(mut self, flags: u32) -> Self {
        self.flags |= flags;
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
        self
    }

    /// A body of rest `kind` (one `FN_REST_*` bit) with `fixed` parameters
    /// before its rest / `arguments` array.
    pub const fn with_rest_kind(mut self, fixed: u16, kind: u32) -> Self {
        self.rest_fixed = fixed;
        self.flags = (self.flags & !FN_REST_MASK) | kind;
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
/// `object::method_site::MethodEntry` — the words the emitted method-call site
/// reads (`perry-codegen/src/expr/method_site.rs`).
pub const METHOD_SITE_WORD_OFFSET: usize = 0;
pub const METHOD_SITE_SLOT_OFFSET: usize = 8;
/// The memoized body's `JsFunctionInfo` (identity: a closure hits when its
/// info word equals this).
pub const METHOD_SITE_INFO_OFFSET: usize = 16;
pub const METHOD_SITE_CLOSURE_OFFSET: usize = 24;
pub const METHOD_SITE_GEN_OFFSET: usize = 32;
/// The memoized body's code address, the hit's call target.
pub const METHOD_SITE_CODE_OFFSET: usize = 40;
/// Entries per method site, and one entry's size.
pub const METHOD_SITE_WAYS: usize = 2;
pub const METHOD_SITE_ENTRY_SIZE: usize = 48;

/// `object::method_site::read_holder` — the property-read cache words
/// (`PicCache`) holding the read site's holder entry, which the emitted read
/// tower checks where the MRU word and the ways miss
/// (`perry-codegen/src/expr/property_get/generic_dispatch.rs`).
pub const PIC_HOLDER_RECV_WORD: usize = 12;
pub const PIC_HOLDER_OBJ_WORD: usize = 13;
pub const PIC_HOLDER_SHAPE_WORD: usize = 14;
pub const PIC_HOLDER_KIND_WORD: usize = 15;
/// A class-accessor entry (#10498): its kind word carries
/// [`PIC_HOLDER_ACCESSOR_BIT`] over the holder's inline slot (low 32 bits);
/// [`PIC_HOLDER_PAIR_WORD`] holds the raw address of the accessor pair that
/// slot held when the site primed (a strong root the collector rewrites), and
/// [`PIC_HOLDER_GETTER_WORD`] the code the hit calls for the getter that pair
/// names, as `double get(double this, i64 pair)` (0 when only the collecting
/// slow call answers the entry: a setter-only pair, or a lane in the holder's
/// spill storage, see [`PIC_HOLDER_SLOT_SPILL_BIT`]):
/// a compiled class getter, which declares `this` only (the pair is
/// over-applied), or the runtime's closure-getter entry, which calls the
/// function object in the pair's getter element through the closure ABI. A
/// hit is the receiver token, the holder's ShapeId and the slot's value equal
/// to the pair: then the getter is called with the receiver as `this`.
pub const PIC_HOLDER_ACCESSOR_BIT: i64 = 1 << 61;
pub const PIC_HOLDER_PAIR_WORD: usize = 16;
pub const PIC_HOLDER_GETTER_WORD: usize = 19;
/// A holder slot word (the low 32 bits of an entry's kind) with this bit set
/// names a position in the holder's SPILL storage, not an inline slot. The
/// emitted accessor arm loads inline lanes only: a spill lane's accessor entry
/// keeps getter word 0, and the collecting slow call answers it.
pub const PIC_HOLDER_SLOT_SPILL_BIT: i64 = 1 << 31;

/// `proxy::put_value::setter_site` (#10498): the word of a static-key store
/// site's ways cache that names the site's compiled-setter entry, as
/// [`SETTER_SITE_TAG`] over the entry's address ([`SETTER_SITE_ADDRESS_MASK`]).
/// The entry is a `#[repr(C)]` record the emitted store tower reads
/// (`perry-codegen/src/expr/put_value_store_ic/setter_arm.rs`): the receiver
/// ShapeId and the holder ShapeId (u32 each), the holder's raw address, the
/// holder's inline slot (u32), the raw address of the accessor pair that slot
/// held at prime time, and the compiled setter it names
/// (`double set(double this, double v)`).
pub const PACKED_SET_SETTER_WORD: usize = 9;
pub const SETTER_SITE_TAG: u64 = 0xA2C2_0000_0000_0000;
pub const SETTER_SITE_ADDRESS_MASK: u64 = 0x0000_FFFF_FFFF_FFFF;
pub const SETTER_SITE_RECV_SHAPE_OFFSET: usize = 0;
pub const SETTER_SITE_HOLDER_SHAPE_OFFSET: usize = 4;
pub const SETTER_SITE_HOLDER_OFFSET: usize = 8;

/// Byte offsets of the entry's pointer-sized fields and the u32 slot, for a
/// target whose pointers are `ptr_bytes` wide (8 on LP64, 4 on ILP32 such as
/// wasm32). The entry is `#[repr(C)]` with `usize` address fields, so the
/// offsets after the two u32 ShapeIds follow the target's pointer width; the
/// runtime asserts the `SETTER_SITE_*_OFFSET` constants (this crate's own
/// target width) against `offset_of!`, and codegen asks for the width of the
/// target it emits for.
pub const fn setter_site_layout(ptr_bytes: usize) -> SetterSiteLayout {
    let slot = SETTER_SITE_HOLDER_OFFSET + ptr_bytes;
    let pair = (slot + 4).next_multiple_of(ptr_bytes);
    SetterSiteLayout {
        holder: SETTER_SITE_HOLDER_OFFSET,
        slot,
        pair,
        code: pair + ptr_bytes,
    }
}

/// See [`setter_site_layout`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SetterSiteLayout {
    pub holder: usize,
    pub slot: usize,
    pub pair: usize,
    pub code: usize,
}

const SETTER_SITE_NATIVE: SetterSiteLayout = setter_site_layout(core::mem::size_of::<usize>());
pub const SETTER_SITE_SLOT_OFFSET: usize = SETTER_SITE_NATIVE.slot;
pub const SETTER_SITE_PAIR_OFFSET: usize = SETTER_SITE_NATIVE.pair;
pub const SETTER_SITE_CODE_OFFSET: usize = SETTER_SITE_NATIVE.code;
const _: () = assert!(
    setter_site_layout(8).slot == 16
        && setter_site_layout(8).pair == 24
        && setter_site_layout(8).code == 32
        && setter_site_layout(4).slot == 12
        && setter_site_layout(4).pair == 16
        && setter_site_layout(4).code == 20
);
/// The site's holder state word, and its bit for a LATCHED site: one that
/// refused, or whose non-own receivers took several shapes. Its misses ask the
/// inherited-read hook, as a never-primed site's do.
pub const PIC_HOLDER_STATE_WORD: usize = 20;
pub const PIC_HOLDER_STATE_LATCHED: i64 = 2;
/// The kind word of a depth-1 ABSENT entry: the answer is `undefined`.
pub const PIC_HOLDER_ABSENT_DEPTH1: i64 = 1 << 62;
/// Words in a property-read cache: MRU, way state, four ways, the holder entry.
pub const PIC_CACHE_WORDS: usize = 21;
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
/// The entry `slot` bit for an inherited entry (the direct holder's slot).
/// Combined with [`METHOD_SITE_CONSTFN`] the holder's shape fixes the body.
pub const METHOD_SITE_INHERITED: u64 = 1 << 63;
/// The entry `slot` bit for an own key in the receiver's spill buffer.
pub const METHOD_SITE_SPILL: u64 = 1 << 62;
/// The entry `slot` bit for an own key of a function-object receiver: an
/// inline slot of the object at `ClosureHeader::props`.
pub const METHOD_SITE_FUNCTION_BAG: u64 = 1 << 61;
/// An own inline method whose ShapeId fixes one static body. The hit loads
/// the receiver's current closure slot for captures, but needs no closure
/// kind or info load after the shape compare.
pub const METHOD_SITE_CONSTFN: u64 = 1 << 59;
/// The index bits of an entry's `slot` word (bit 60 remains reserved for the
/// accessor entry kind; bit 59 is ConstFn).
pub const METHOD_SITE_INDEX_MASK: u64 = (1 << 59) - 1;
/// What `js_method_site_prepare` answers for a call whose method read nothing
/// can observe: dispatch by name after the arguments (#11910). The array-hole
/// marker, a bit pattern no JS value takes.
pub const METHOD_SITE_BY_NAME: u64 = 0x7FFC_0000_0000_0010;
/// `js_method_site_prepare`'s by-name answer for a receiver no site entry
/// describes (a primitive, a native cell): the call goes straight to the
/// universal dispatcher. In the same never-a-value namespace;
/// `bits | 2 == METHOD_SITE_BY_NAME_DIRECT` tests for either by-name answer.
pub const METHOD_SITE_BY_NAME_DIRECT: u64 = 0x7FFC_0000_0000_0012;
const _: () = assert!(METHOD_SITE_BY_NAME | 2 == METHOD_SITE_BY_NAME_DIRECT);
/// `object::ObjectMeta::spill` (the object-owned overflow buffer).
pub const OBJECT_META_SPILL_OFFSET: usize = 32;

/// `proxy::put_value::packed_add::PackedSetSite` — the static-key store
/// site (`@perry_ic_N_packed_set`) the emitted `o.k = v` reads
/// (`perry-codegen/src/expr/put_value_store_ic.rs`): the existing-key word,
/// the primary key-add memo `{shapes, guard}`, the runtime's add-way block,
/// and the site's ConstFn body.
pub const PACKED_SET_SITE_WORDS: usize = 5;
/// The site's one ConstFn body: the `JsFunctionInfo` address every
/// ConstFn-flagged entry of the site (existing-key word, existing-key way,
/// add memo) names, 0 until the first is published. Written once, before
/// the first flagged entry, and never changed: a flagged entry hits only for
/// a closure of exactly this body.
pub const PACKED_SET_CONSTFN_INFO_WORD: usize = 4;
/// The existing-key word's (and way's) bit for a slot whose lane is ConstFn
/// in the word's ShapeId: the emitted hit stores only a closure of the site's
/// body. Bit 63 is the `F64` lane bit; the slot index is below bit 62.
pub const PACKED_SET_CONSTFN_SLOT: u64 = 1 << 62;
/// The key-add guard's bit for a successor whose lane at the slot is
/// ConstFn (the slot field is the guard's low 16 bits; bit 15 is the `F64`
/// lane bit, the index is below bit 14).
pub const PACKED_ADD_CONSTFN_SLOT: u64 = 1 << 14;
/// `closure::CAPTURES_THIS_FLAG` / `closure::NO_THIS_REBIND_FLAG`, the high
/// bits of `ClosureHeader::capture_count`. A closure with the first and not
/// the second is a rebindable `this` clone, which never satisfies a ConstFn
/// claim (`field_rep_store::constfn_store_info`).
pub const CLOSURE_CAPTURES_THIS_FLAG: u32 = 0x8000_0000;
pub const CLOSURE_NO_THIS_REBIND_FLAG: u32 = 0x4000_0000;

/// Numeric typed-array resolved ArrayBuffer storage contract.
pub const TA_STORAGE_RESOLVED: u8 = 2;
pub const TA_DATA_OFFSET: usize = 16;
