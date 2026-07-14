//! NaN-boxing tag constants and shared type aliases.
//!
//! Every tag value here is part of the wire-level contract between the
//! Rust runtime, the LLVM IR emitter in `perry-codegen`, and any external
//! tooling that inspects compiled binaries. Renumbering any constant in
//! this file is an ABI break — keep them verbatim.

use std::sync::atomic::AtomicPtr;

/// Tag-marker for the singleton specials (undefined / null / true / false /
/// hole). 0x7FFC chosen so the first two mantissa bits are `11`: that keeps
/// it inside the qNaN encoding space (mantissa bit 51 set) while staying
/// distinct from the canonical qNaN 0x7FF8 the FPU produces from arithmetic
/// like `0/0` — code that wants to tell "Perry tagged" from "real NaN" can
/// gate on `top16 >= 0x7FFC` (see `JSValue::is_number` below).
///
/// #854: part of the NaN-boxing tag contract documented in CLAUDE.md.
/// Kept as a named constant even when no Rust code consults it directly —
/// codegen, doc references, and external tooling all match against the
/// numeric value.
#[allow(dead_code)]
pub(crate) const TAG_MARKER: u64 = 0x7FFC_0000_0000_0000;

/// Special singleton values
pub(crate) const TAG_UNDEFINED: u64 = 0x7FFC_0000_0000_0001;
pub(crate) const TAG_NULL: u64 = 0x7FFC_0000_0000_0002;
pub(crate) const TAG_FALSE: u64 = 0x7FFC_0000_0000_0003;
pub(crate) const TAG_TRUE: u64 = 0x7FFC_0000_0000_0004;

/// Issue #323: hole sentinel for sparse arrays. Slots in `new Array(n)` are
/// initialized to this value; reads through `js_array_get_f64` translate it
/// back to TAG_UNDEFINED so user code never observes the raw bits, while
/// `Object.keys` and the `in` operator inspect slots directly to distinguish a
/// hole from an explicit `undefined` write. Bits chosen in the same 0x7FFC
/// singleton namespace, distinct from UNDEFINED/NULL/FALSE/TRUE so a NaN-box
/// payload can never be mistaken for a hole.
pub(crate) const TAG_HOLE: u64 = 0x7FFC_0000_0000_0010;

/// TDZ (Temporal Dead Zone) sentinel. A lexical `let`/`const`/`class` binding
/// created at scope entry but not yet initialized holds this value in its box
/// cell. Reading it (via `js_box_get_bits`) throws a spec `ReferenceError`
/// ("Cannot access ... before initialization"). Chosen in the same 0x7FFC
/// singleton namespace, distinct from UNDEFINED/NULL/FALSE/TRUE/HOLE so no
/// legitimate NaN-box payload can ever be mistaken for a TDZ hole. Unlike a
/// hole, code never silently coerces it to `undefined` — the whole point is to
/// throw on any observation before the declaration is evaluated.
pub(crate) const TAG_TDZ: u64 = 0x7FFC_0000_0000_0011;

/// Pointer tag: 0x7FFD_XXXX_XXXX_XXXX (48 bits for pointer) - objects/arrays
pub(crate) const POINTER_TAG: u64 = 0x7FFD_0000_0000_0000;
pub(crate) const POINTER_MASK: u64 = 0x0000_FFFF_FFFF_FFFF;

/// Int32 tag: 0x7FFE_0000_XXXX_XXXX (32 bits for i32)
pub(crate) const INT32_TAG: u64 = 0x7FFE_0000_0000_0000;
pub(crate) const INT32_MASK: u64 = 0x0000_0000_FFFF_FFFF;

/// String pointer tag: 0x7FFF_XXXX_XXXX_XXXX (48 bits for string pointer)
pub(crate) const STRING_TAG: u64 = 0x7FFF_0000_0000_0000;

/// Small String Optimization (SSO) — tier 1 #2 per
/// `docs/memory-perf-roadmap.md`. A string of length 0..=5 bytes
/// encodes inline in the 48-bit NaN-box payload instead of
/// allocating a `StringHeader`. Layout:
///
/// ```text
/// bits  63........48  47.....40  39...32 31..24 23..16 15..8  7..0
///       0x7FF9 tag    length     byte0   byte1  byte2  byte3  byte4
/// ```
///
/// Length in bits 40..=47 (0..=5 — 6 valid values, 3 bits would
/// suffice but we use a full byte for alignment). Data in bits
/// 0..=39 (5 bytes, little-endian by byte index — `byte0` is the
/// first character).
///
/// Why 5 bytes not 6: 6 bytes × 8 bits = 48 bits would fill the
/// entire payload leaving no room for length, forcing us to use 3
/// different tag values for length buckets or a null-terminator
/// convention (which breaks strings containing U+0000). Staying at
/// 5 bytes with one tag keeps decode simple: tag check + 40-bit
/// extract. Covers "id", "name", "age", "true", "false", "null",
/// single-byte ASCII, etc. — a large fraction of real-world JSON
/// keys and short values.
///
/// Strings with length > 5 fall through to the standard heap
/// `StringHeader` path; callers read-side use `is_string()` (which
/// accepts BOTH tags) + `string_bytes()` (which decodes either
/// form to a (ptr, len) slice view).
pub(crate) const SHORT_STRING_TAG: u64 = 0x7FF9_0000_0000_0000;
pub(crate) const SHORT_STRING_LEN_SHIFT: u64 = 40;
// Length byte at bits 40..=47 (byte index 5 from LSB). Not
// 0x00FF_0000_0000_0000 — that would be byte 6, overlapping the
// tag.
pub(crate) const SHORT_STRING_LEN_MASK: u64 = 0x0000_FF00_0000_0000;
// Data bytes at bits 0..=39 (5 bytes, byte indices 0..=4 from LSB).
pub(crate) const SHORT_STRING_DATA_MASK: u64 = 0x0000_00FF_FFFF_FFFF;
pub const SHORT_STRING_MAX_LEN: usize = 5;

/// BigInt pointer tag: 0x7FFA_XXXX_XXXX_XXXX (48 bits for bigint pointer)
pub(crate) const BIGINT_TAG: u64 = 0x7FFA_0000_0000_0000;

/// JS Handle tag: 0x7FFB_XXXX_XXXX_XXXX (48 bits for handle ID)
/// This is used by perry-jsruntime to reference V8 objects
pub(crate) const JS_HANDLE_TAG: u64 = 0x7FFB_0000_0000_0000;
pub(crate) const TAG_MASK: u64 = 0xFFFF_0000_0000_0000;

// ----- JS handle function-pointer types (used by handle.rs FFI setters) -----

pub(crate) type JsHandleArrayGetFn = extern "C" fn(f64, i32) -> f64;
pub(crate) type JsHandleArrayLengthFn = extern "C" fn(f64) -> i32;
pub(crate) type JsHandleObjectGetPropertyFn = extern "C" fn(f64, *const i8, usize) -> f64;
pub(crate) type JsHandleToStringFn = extern "C" fn(f64) -> *mut crate::string::StringHeader;
pub(crate) type JsHandleCallMethodFn =
    unsafe extern "C" fn(f64, *const i8, usize, *const f64, usize) -> f64;
pub(crate) type JsNativeModuleJsLoaderFn =
    unsafe extern "C" fn(*const u8, usize, *const u8, usize) -> f64;
pub(crate) type JsNewFromHandleV8Fn = unsafe extern "C" fn(f64, *const f64, usize) -> f64;
/// Returns the JS spec `typeof` string discriminator for a V8 handle:
/// 1 = "function" (V8 callable), 0 = "object" (everything else — including arrays).
/// Negative values reserved for future use ("symbol" = 2 if V8 ever exposes it that way).
pub(crate) type JsHandleTypeofFn = unsafe extern "C" fn(f64) -> i32;
/// node:crypto module-method dispatcher (registered by perry-stdlib). Takes
/// `(method_name_ptr, method_name_len, args_ptr, args_len)` and returns the
/// NaN-boxed result. Lets a captured-then-called crypto method reach the
/// stdlib crypto impls, which this crate can't call directly. (#1577)
pub(crate) type JsNativeCryptoDispatchFn =
    unsafe extern "C" fn(*const u8, usize, *const f64, usize) -> f64;
/// WebCrypto `crypto.subtle` namespace-method dispatcher (registered by
/// perry-stdlib). Kept separate from node:crypto because method names such as
/// `generateKey` overlap with top-level crypto APIs.
pub(crate) type JsNativeWebCryptoDispatchFn =
    unsafe extern "C" fn(*const u8, usize, *const f64, usize) -> f64;
/// node:zlib module-method dispatcher (registered by perry-stdlib). Same
/// shape and rationale as the crypto dispatcher above — lets a captured /
/// promisified zlib method (`const f = zlib.gzip; await f(buf)`) reach the
/// stdlib zlib FFIs since this crate cannot call them directly.
pub(crate) type JsNativeZlibDispatchFn =
    unsafe extern "C" fn(*const u8, usize, *const f64, usize) -> f64;
/// node:querystring module-method dispatcher (registered by perry-stdlib).
/// Same dependency-boundary pattern as crypto/zlib: captured callable exports
/// can reach the stdlib implementation without perry-runtime depending on it.
pub(crate) type JsNativeQuerystringDispatchFn =
    unsafe extern "C" fn(*const u8, usize, *const f64, usize) -> f64;
/// node:sqlite module-method/constructor dispatcher. Same dependency-boundary
/// pattern as crypto/zlib, with an extra construct flag so dynamic `new
/// DatabaseSync(...)` can reach the real stdlib constructor.
pub(crate) type JsNativeSqliteDispatchFn =
    unsafe extern "C" fn(*const u8, usize, *const f64, usize, i32) -> f64;
pub(crate) type JsNativeDomainDispatchFn =
    unsafe extern "C" fn(*const u8, usize, *const f64, usize) -> f64;
/// node:tls module-method dispatcher (registered by perry-stdlib). Same
/// dependency-boundary pattern as crypto/zlib/querystring: captured callable
/// exports and object-valued properties can reach the rustls-backed stdlib
/// implementation without perry-runtime depending on perry-stdlib.
pub(crate) type JsNativeTlsDispatchFn =
    unsafe extern "C" fn(*const u8, usize, *const f64, usize) -> f64;
/// node:http / node:https / node:http2 server-factory dispatcher (registered
/// by perry-stdlib under the `external-http-server-pump` feature, which is
/// enabled whenever a program imports one of those modules). Lets a captured /
/// aliased `createServer` (`const cs = createServer; cs(handler)`, or
/// `@hono/node-server`'s `const createServer = options.createServer ||
/// createServerHTTP`) reach the perry-ext-http-server impls. Unlike crypto/zlib
/// it also takes the module name so one callback can route http vs https vs
/// http2. Stays null when the http ext crate isn't linked. (#2533)
pub(crate) type JsNativeHttpDispatchFn =
    unsafe extern "C" fn(*const u8, usize, *const u8, usize, *const f64, usize) -> f64;
/// node:events class-constructor dispatcher (registered by perry-stdlib under
/// `bundled-events`, or by perry-ext-events). Lets `new` on a bound
/// `events.EventEmitter` / `events.EventEmitterAsyncResource` export value —
/// reached via `require('events')`, a default import, or a namespace property
/// read — construct a real emitter instead of falling through to the generic
/// empty-object path. Takes (class_name_ptr, class_name_len, args_ptr,
/// args_len) and returns the NaN-boxed instance. Stays null when no events
/// impl is linked. (#4995)
pub(crate) type JsNativeEventsConstructFn =
    unsafe extern "C" fn(*const u8, usize, *const f64, usize) -> f64;

// ----- JS handle dispatch atomics (shared between handle.rs and consumers) -----

pub(crate) static JS_HANDLE_ARRAY_GET: AtomicPtr<()> = AtomicPtr::new(std::ptr::null_mut());
pub(crate) static JS_HANDLE_ARRAY_LENGTH: AtomicPtr<()> = AtomicPtr::new(std::ptr::null_mut());
pub(crate) static JS_HANDLE_OBJECT_GET_PROPERTY: AtomicPtr<()> =
    AtomicPtr::new(std::ptr::null_mut());
pub(crate) static JS_HANDLE_TO_STRING: AtomicPtr<()> = AtomicPtr::new(std::ptr::null_mut());
pub static JS_HANDLE_CALL_METHOD: AtomicPtr<()> = AtomicPtr::new(std::ptr::null_mut());
pub static JS_NATIVE_MODULE_JS_LOADER: AtomicPtr<()> = AtomicPtr::new(std::ptr::null_mut());
pub static JS_NEW_FROM_HANDLE_V8: AtomicPtr<()> = AtomicPtr::new(std::ptr::null_mut());
pub static JS_HANDLE_TYPEOF: AtomicPtr<()> = AtomicPtr::new(std::ptr::null_mut());
pub static JS_NATIVE_CRYPTO_DISPATCH: AtomicPtr<()> = AtomicPtr::new(std::ptr::null_mut());
pub static JS_NATIVE_WEBCRYPTO_DISPATCH: AtomicPtr<()> = AtomicPtr::new(std::ptr::null_mut());
pub static JS_NATIVE_ZLIB_DISPATCH: AtomicPtr<()> = AtomicPtr::new(std::ptr::null_mut());
pub static JS_NATIVE_QUERYSTRING_DISPATCH: AtomicPtr<()> = AtomicPtr::new(std::ptr::null_mut());
pub static JS_NATIVE_SQLITE_DISPATCH: AtomicPtr<()> = AtomicPtr::new(std::ptr::null_mut());
pub static JS_NATIVE_DOMAIN_DISPATCH: AtomicPtr<()> = AtomicPtr::new(std::ptr::null_mut());
pub static JS_NATIVE_TLS_DISPATCH: AtomicPtr<()> = AtomicPtr::new(std::ptr::null_mut());
pub static JS_NATIVE_HTTP_DISPATCH: AtomicPtr<()> = AtomicPtr::new(std::ptr::null_mut());
pub static JS_NATIVE_EVENTS_CONSTRUCT: AtomicPtr<()> = AtomicPtr::new(std::ptr::null_mut());
// Dynamic `new <bound async_hooks ctor>()` (e.g. `new maybeGlobalAsyncLocalStorage()`
// where the value came from `globalThis.AsyncLocalStorage = AsyncLocalStorage`).
// Registered by perry-stdlib at startup so a bound `async_hooks.AsyncLocalStorage` /
// `AsyncResource` export value constructed dynamically reaches the real handle
// constructor instead of falling through to the class_id=0 empty object. Takes
// (method_name_ptr, method_name_len, args_ptr, args_len), returns the NaN-boxed
// instance. Next.js standalone server startup blocker.
pub static JS_NATIVE_ASYNC_HOOKS_CONSTRUCT: AtomicPtr<()> = AtomicPtr::new(std::ptr::null_mut());
