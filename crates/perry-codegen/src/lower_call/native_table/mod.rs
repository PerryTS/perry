//! Native stdlib module dispatch table (`NATIVE_MODULE_TABLE`) + the
//! arg/return kind types and manifest-introspection helpers.
//!
//! Originally extracted from `lower_call.rs` (#1099, part of #1097) into
//! a single 5,875-line file; split into row-family sub-modules
//! (v0.5.1019) to satisfy the file-size CI gate. Each `*_ROWS` slice
//! defines one family's entries; mod.rs assembles them, in declaration
//! order, into a single `LazyLock<Vec<NativeModSig>>` that consumers
//! still call `.iter()` on. The dispatch *consumers*
//! (`native_module_lookup`, `lower_native_module_dispatch`) stay in
//! `lower_call/mod.rs` and import the `pub(super)` items below.

use std::sync::LazyLock;

mod async_decimal;
mod bun;
mod databases;
mod dates;
mod extras;
mod fastify;
mod http_client;
mod http_http2;
mod http_server;
mod media;
mod net_classes_state;
mod net_events;
mod node_core;
mod node_core_process;
mod node_core_util;
mod node_dns;
mod node_domain;
mod node_misc;
mod thread_lodash;
mod tls_events;
mod tui;
mod utils_crypto;
mod yoga;

// ============================================================================
// Native stdlib module dispatch (fastify, mysql2, ws, pg, ioredis, mongodb,
// better-sqlite3, etc.). Ported from the old Cranelift codegen's dispatch
// table that was lost in the v0.5.0 LLVM cutover.
// ============================================================================

/// How each argument should be coerced before passing to the runtime fn.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub(super) enum NativeArgKind {
    /// NaN-boxed f64 — pass as-is (objects, generic JSValues).
    F64,
    /// NaN-boxed value → extract/stringify to a raw i64 StringHeader pointer.
    /// Use for Rust signatures like `*const StringHeader`.
    StrPtr,
    /// NaN-boxed closure/pointer → unbox to i64 via the standard mask.
    PtrI64,
    /// Pass the NaN-boxed JSValue bits as-is (bitcast f64 → i64, no
    /// unboxing). Use for Rust signatures where the function receives
    /// `name: i64` and internally calls `string_from_nanboxed(name)` or
    /// similar — the callee expects the full NaN-boxed value, not an
    /// unboxed raw pointer. Common pattern in fastify context methods.
    JsvalI64,
    /// Pack all remaining user-supplied args (from this position onward)
    /// into a freshly allocated JS array and pass a single i64
    /// `*const ArrayHeader` to the runtime. Must be the last entry in
    /// `sig.args`. When the user supplies no args at this position, an
    /// empty array is passed (a real allocated header, not a null
    /// pointer — callees that walk `*arr_ptr` unconditionally are safe).
    /// Used for variadic JS-side call shapes like
    /// `stmt.all(...params)` / `stmt.run(...)` / `stmt.get(...)` that
    /// the runtime consumes as a single `*const ArrayHeader`.
    VarArgsAsArray,
}

/// What the runtime function returns.
#[derive(Copy, Clone, Debug)]
pub(super) enum NativeRetKind {
    /// Returns i64 handle → NaN-box as POINTER.
    Ptr,
    /// Returns i64 promise handle → NaN-box as POINTER, but record the async
    /// boundary separately from generic native handles.
    Promise,
    /// Returns `*mut StringHeader` → NaN-box as STRING. Use for runtime
    /// functions whose Rust signature returns a raw string pointer; the
    /// caller (and `JSON.stringify`, string-comparison, etc.) needs the
    /// STRING_TAG to recognize it as a string rather than a heap object.
    Str,
    /// Returns `*mut StringHeader` containing JSON → automatically pipe
    /// through `js_json_parse` so the user-visible value is a parsed
    /// object/array, not the JSON-encoded string. Symmetric to `NA_JSON`
    /// on the argument side (#915). Null pointer → TAG_NULL so a failed
    /// verify (`jwt.verify` on bad signature) still reads as `null`
    /// rather than dereferencing a dangling pointer. Issue #927.
    ObjFromJsonStr,
    /// Returns `*mut BigIntHeader` → NaN-box as BIGINT (0x7FFA tag). Use
    /// for functions like `parseEther`/`parseUnits` that return bigint values.
    BigInt,
    /// Returns f64 → pass through (NaN-boxed JSValue).
    F64,
    /// Returns f64 `1.0`/`0.0` → box as a real JS boolean
    /// (`TAG_TRUE`/`TAG_FALSE`) so `console.log` prints `true`/`false`
    /// rather than `1`/`0`. Use for predicates whose runtime signature
    /// returns the FFI bool-as-f64 convention (e.g. `uuid.validate`).
    Bool,
    /// Returns i32 → ignored, return TAG_UNDEFINED.
    I32Void,
    /// Returns void → return TAG_UNDEFINED.
    Void,
}

#[derive(Copy, Clone, Debug)]
pub(super) struct NativeModSig {
    pub(super) module: &'static str,
    pub(super) has_receiver: bool,
    pub(super) method: &'static str,
    /// Optional class_name filter. When Some, only matches if the HIR's
    /// class_name equals this value (e.g. "Pool" vs "Connection" for mysql2).
    /// When None, matches regardless of class_name.
    pub(super) class_filter: Option<&'static str>,
    pub(super) runtime: &'static str,
    pub(super) args: &'static [NativeArgKind],
    pub(super) ret: NativeRetKind,
}

// Short aliases to keep the row tables compact without wildcard imports
// (wildcard would clash with crate::types::* names like I64, DOUBLE).
// Visibility note: `pub(super)` (not file-private) so the row tables in
// each sibling sub-module can reach them via `use super::*;`.
pub(super) const NA_F64: NativeArgKind = NativeArgKind::F64;
pub(super) const NA_STR: NativeArgKind = NativeArgKind::StrPtr;
pub(super) const NA_PTR: NativeArgKind = NativeArgKind::PtrI64;
pub(super) const NA_JSV: NativeArgKind = NativeArgKind::JsvalI64;
pub(super) const NA_VARARGS: NativeArgKind = NativeArgKind::VarArgsAsArray;
pub(super) const NR_PTR: NativeRetKind = NativeRetKind::Ptr;
pub(super) const NR_PROMISE: NativeRetKind = NativeRetKind::Promise;
pub(super) const NR_STR: NativeRetKind = NativeRetKind::Str;
pub(super) const NR_OBJ_FROM_JSON_STR: NativeRetKind = NativeRetKind::ObjFromJsonStr;
pub(super) const NR_BIGINT: NativeRetKind = NativeRetKind::BigInt;
pub(super) const NR_F64: NativeRetKind = NativeRetKind::F64;
pub(super) const NR_BOOL: NativeRetKind = NativeRetKind::Bool;
pub(super) const NR_I32: NativeRetKind = NativeRetKind::I32Void;
pub(super) const NR_VOID: NativeRetKind = NativeRetKind::Void;

/// Static dispatch table for native stdlib modules. Each entry maps
/// `(module, has_receiver, method)` → runtime function, with per-arg
/// coercion rules and return-value boxing.
///
/// The receiver (when `has_receiver = true`) is always NaN-unboxed to
/// an i64 pointer and passed as the first argument.
///
/// v0.5.1019: backed by `LazyLock<Vec<NativeModSig>>` that concatenates
/// the per-family `*_ROWS` slices from sub-modules. Iteration order is
/// stable and matches the pre-split declaration order in the original
/// single-file table — important for `iter_native_module_table` and the
/// downstream `perry-api-manifest` drift gate (#512). Consumers were
/// `.iter()`-only, so the `const` → `static` change is source-compatible.
pub(super) static NATIVE_MODULE_TABLE: LazyLock<Vec<NativeModSig>> = LazyLock::new(|| {
    let mut v: Vec<NativeModSig> = Vec::new();
    v.extend_from_slice(node_core::NODE_CORE_ROWS);
    v.extend_from_slice(bun::BUN_ROWS);
    v.extend_from_slice(node_core_process::NODE_CORE_PROCESS_ROWS);
    v.extend_from_slice(node_core_util::NODE_CORE_UTIL_ROWS);
    v.extend_from_slice(node_dns::NODE_DNS_ROWS);
    v.extend_from_slice(node_domain::NODE_DOMAIN_ROWS);
    v.extend_from_slice(fastify::FASTIFY_ROWS);
    v.extend_from_slice(databases::DATABASES_ROWS);
    v.extend_from_slice(net_events::NET_EVENTS_ROWS);
    v.extend_from_slice(net_classes_state::NET_CLASSES_STATE_ROWS);
    v.extend_from_slice(tls_events::TLS_EVENTS_ROWS);
    v.extend_from_slice(node_misc::NODE_MISC_ROWS);
    v.extend_from_slice(async_decimal::ASYNC_DECIMAL_ROWS);
    v.extend_from_slice(utils_crypto::UTILS_CRYPTO_ROWS);
    v.extend_from_slice(thread_lodash::THREAD_LODASH_ROWS);
    v.extend_from_slice(dates::DATES_ROWS);
    v.extend_from_slice(media::MEDIA_ROWS);
    v.extend_from_slice(tui::TUI_ROWS);
    v.extend_from_slice(yoga::YOGA_ROWS);
    v.extend_from_slice(extras::EXTRAS_ROWS);
    v.extend_from_slice(http_client::HTTP_CLIENT_ROWS);
    v.extend_from_slice(http_server::HTTP_SERVER_ROWS);
    v.extend_from_slice(http_http2::HTTP_HTTP2_ROWS);
    v
});

/// Iterate the dispatch table, projected to manifest-relevant fields.
/// Used by `perry-codegen`'s public `iter_native_method_signatures()`
/// — see `lib.rs`. Stable order = declaration order in
/// `NATIVE_MODULE_TABLE`. Returns args/ret as opaque tag strings so
/// downstream crates (perry-api-manifest's drift test) don't have to
/// know about `NativeArgKind` / `NativeRetKind` (#512).
#[allow(clippy::type_complexity)]
pub(crate) fn iter_native_module_table() -> impl Iterator<
    Item = (
        &'static str,
        bool,
        &'static str,
        Option<&'static str>,
        &'static [&'static str],
        &'static str,
    ),
> {
    NATIVE_MODULE_TABLE.iter().map(|sig| {
        (
            sig.module,
            sig.has_receiver,
            sig.method,
            sig.class_filter,
            arg_kinds_for(sig.args),
            ret_kind_tag(&sig.ret),
        )
    })
}

/// Map a `NativeArgKind` slice to its `NA_*` tag-name slice. The
/// returned slice is `&'static` — keeping each lookup costless on the
/// dispatch-table iteration path. Per-arity buckets keep the static
/// arrays addressable without alloc.
fn arg_kinds_for(args: &'static [NativeArgKind]) -> &'static [&'static str] {
    // Map each arg to its tag string. Up to 6 args covers every row
    // in NATIVE_MODULE_TABLE today (tls.connect = 4 args is the max).
    static TAGS_0: &[&str] = &[];
    let tags: Vec<&'static str> = args.iter().map(|a| arg_kind_tag(a)).collect();
    // Lookup against a small set of static fan-outs — but since we
    // can't easily memoize without `OnceLock`, just leak. The dispatch
    // table is < 400 rows; the resulting Vec leak is bounded and
    // happens once per process.
    if tags.is_empty() {
        return TAGS_0;
    }
    Box::leak(tags.into_boxed_slice())
}

fn arg_kind_tag(a: &NativeArgKind) -> &'static str {
    match a {
        NativeArgKind::F64 => "NA_F64",
        NativeArgKind::StrPtr => "NA_STR",
        NativeArgKind::PtrI64 => "NA_PTR",
        NativeArgKind::JsvalI64 => "NA_JSV",
        NativeArgKind::VarArgsAsArray => "NA_VARARGS",
    }
}

fn ret_kind_tag(r: &NativeRetKind) -> &'static str {
    match r {
        NativeRetKind::Ptr => "NR_PTR",
        NativeRetKind::Promise => "NR_PROMISE",
        NativeRetKind::Str => "NR_STR",
        NativeRetKind::ObjFromJsonStr => "NR_OBJ_FROM_JSON_STR",
        NativeRetKind::BigInt => "NR_BIGINT",
        NativeRetKind::F64 => "NR_F64",
        NativeRetKind::Bool => "NR_BOOL",
        NativeRetKind::I32Void => "NR_I32",
        NativeRetKind::Void => "NR_VOID",
    }
}
