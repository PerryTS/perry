//! `Expr::GlobalGet` receiver dispatch extracted from `property_get.rs`.
//!
//! Pure mechanical move — body is the verbatim contents of the
//! `if matches!(object.as_ref(), Expr::GlobalGet(_)) { ... }` block from the
//! general catch-all arm, lifted into its own function.
//!
//! Global values use the ordinary property-read lowering, including its
//! rooting and shape guards. Legacy HIR that collapsed a constructor's
//! static-value receiver is reconstructed before entering that lowering.

use super::*;

use anyhow::Result;

use crate::nanbox::double_literal;
use crate::types::{DOUBLE, I64, PTR};

/// Lower a `PropertyGet` whose receiver is the `GlobalGet(0)` builtin-global
/// sentinel, read by the `property` string alone (the receiver name has been
/// collapsed during HIR lowering).
/// Emit the runtime installer a global VALUE read needs (binary size).
///
/// The `process` / `console` namespaces exist from realm bootstrap, but their
/// dynamic-dispatch buckets (and `process`'s stdio stream objects) are
/// installed only on demand, so a program that uses them purely through
/// member intrinsics (`console.log(…)`, `process.argv`) links none of that
/// surface. Every site that can hand one of those objects to user code as a
/// value — the bare global, a method read as a value, or the global object
/// itself (`globalThis` / `global` / `self` / `globalThis[k]`) — runs the
/// matching installer first. Idempotent and cheap (a few relaxed stores).
pub(crate) fn emit_global_value_installs(ctx: &mut FnCtx<'_>, name: &str) {
    let sym = match name {
        "process" => "js_nm_install_process",
        "console" => "js_nm_install_console",
        "globalThis" | "global" | "self" | "window" | "frames" => {
            "js_install_global_value_surfaces"
        }
        _ => return,
    };
    ctx.block().call_void(sym, &[]);
}

/// A builtin global identifier read as a VALUE (`Object`, `Array`, `Date`
/// ...): a static read site of the global object for the name's pooled key.
///
/// The global object is an ordinary object whose own keys are the builtins,
/// so the identifier is `globalThis[name]` with a compile-time key: the
/// generic read site's per-site cache compares the global object's ShapeId
/// and loads the slot, with the key's atom (its pool handle) for the miss.
/// It used to call `js_get_global_this_builtin_value(bytes, len)`, which
/// validated the name's UTF-8, interned it and ran a by-name `[[Get]]` on
/// every evaluation. Reassigning the global (`globalThis.Object = X`) is a
/// store to that object, so the next read sees it: the slot is loaded on
/// every hit, and a key added or deleted changes the ShapeId.
pub(crate) fn lower_global_builtin_read(ctx: &mut FnCtx<'_>, name: &str) -> Result<String> {
    let global = Expr::Call {
        callee: Box::new(Expr::ExternFuncRef {
            name: "js_get_global_this".to_string(),
            param_types: Vec::new(),
            return_type: perry_hir::types::Type::Any,
        }),
        args: Vec::new(),
        type_args: Vec::new(),
        byte_offset: 0,
    };
    super::lower_generic_property_get(ctx, &global, name, 0)
}

pub(crate) fn lower_globalget_property(ctx: &mut FnCtx<'_>, property: &str) -> Result<String> {
    emit_global_value_installs(ctx, property);
    // `process.env` read as a VALUE (not `process.env.X`) must
    // materialize the live env object, not the `undefined` sentinel.
    // Member reads `process.env.X` are special-cased elsewhere to
    // `EnvGet`, but passing `process.env` whole (e.g.
    // `EnvSchema.safeParse(process.env)` — the canonical config
    // pattern) reached the GlobalGet fall-through and lowered to
    // `undefined`, so the consumer iterated `undefined`. Only the
    // `process` global exposes a meaningful `.env`, so routing by the
    // property string alone is safe here.
    if property == "env" {
        return Ok(ctx.block().call(DOUBLE, "js_process_env", &[]));
    }
    if matches!(
        property,
        "resolve" | "reject" | "all" | "race" | "allSettled" | "any" | "withResolvers" | "try"
    ) {
        return lower_global_builtin_static_value(ctx, "Promise", property);
    }
    // `Proxy.revocable` read as a VALUE (not in a call) — the receiver is
    // collapsed to GlobalGet(0) so we route by property name. Resolves the
    // closure installed by `install_builtin_constructor_statics("Proxy", …)`.
    if property == "revocable" {
        return lower_global_builtin_static_value(ctx, "Proxy", property);
    }
    // #2904: V8/Node static Error members read as values
    // (`typeof Error.isError`, `Error.stackTraceLimit`, …). The
    // HIR collapses every builtin global receiver to
    // `GlobalGet(0)`, so route by property name alone: resolve the
    // real `Error` constructor closure and read the named field
    // off it (where `install_error_static_methods` stored them).
    if matches!(
        property,
        "captureStackTrace" | "isError" | "stackTraceLimit" | "prepareStackTrace"
    ) {
        return lower_global_builtin_static_value(ctx, "Error", property);
    }
    // Object statics read as VALUES (`var f = Object.seal`,
    // `typeof Object.defineProperties`, `Object.is.length`).
    // The receiver name is collapsed to GlobalGet(0), so route by
    // property name — but ONLY names unique to `Object` among the
    // builtin globals: the Reflect-overlapping ones
    // (defineProperty / getOwnPropertyDescriptor / getPrototypeOf /
    // setPrototypeOf / isExtensible / preventExtensions) and
    // Map-overlapping `groupBy` must keep their current behavior.
    // Resolves the reified ctor closure installed by
    // `install_builtin_constructor_statics`.
    if matches!(
        property,
        "keys"
            | "values"
            | "entries"
            | "fromEntries"
            | "assign"
            | "create"
            | "seal"
            | "freeze"
            | "isFrozen"
            | "isSealed"
            | "is"
            | "getOwnPropertyNames"
            | "getOwnPropertySymbols"
            | "getOwnPropertyDescriptors"
            | "defineProperties"
    ) {
        return lower_global_builtin_static_value(ctx, "Object", property);
    }
    // #3527: `Object.hasOwn` read as a VALUE (not a direct call) —
    // e.g. iconv-lite's merge-exports does
    // `var hasOwn = typeof Object.hasOwn === "undefined" ? … :
    // Object.hasOwn` then `hasOwn(obj, key)`. The ternary defeats
    // the const-alias call-fold, so the value must be a real
    // callable. Mirror the `Error.captureStackTrace` shape above:
    // resolve the reified `Object` constructor closure and read the
    // `hasOwn` static (installed by `install_builtin_constructor_statics`)
    // off it, instead of falling through to the `0.0` sentinel.
    if property == "hasOwn" {
        return lower_global_builtin_static_value(ctx, "Object", property);
    }
    // #4033: `ArrayBuffer.isView` must also work as a value
    // (`const isView = ArrayBuffer.isView; isView(view)`). Bare
    // builtin receivers are collapsed to `GlobalGet(0)`, so recover
    // the populated constructor closure and read the reified static.
    if property == "isView" {
        return lower_global_builtin_static_value(ctx, "ArrayBuffer", property);
    }
    // `Buffer.isBuffer` used as a callback (for example
    // `values.every(Buffer.isBuffer)`) needs the callable value, not only the
    // direct-call intrinsic. Bare builtin receivers are represented by the
    // shared `GlobalGet(0)` sentinel, and `isBuffer` is distinctive among the
    // builtin statics, so recover it from the populated Buffer constructor.
    if property == "isBuffer" {
        return lower_global_builtin_static_value(ctx, "Buffer", property);
    }
    // #6674: `Uint8Array.fromBase64` / `fromHex` read as a VALUE (not a direct
    // call) — jose/Auth.js feature-detect with `Uint8Array.fromBase64 ? native
    // : fallback`. The bare `Uint8Array` receiver collapses to `GlobalGet(0)`
    // here (HIR: `PropertyGet { object: GlobalGet(0), property: "fromBase64" }`),
    // so without this arm the read fell through to the `undefined` sentinel
    // below even though the runtime constructor closure now carries the static.
    // These names are distinctive to `Uint8Array` among the builtin globals
    // (Buffer inherits them via its constructor's prototype chain, matching
    // Node), so route by property name — resolve the reified ctor closure and
    // read the static installed by `install_builtin_constructor_statics`. The
    // direct call form is intercepted earlier in HIR (`module_static.rs`).
    if matches!(property, "fromBase64" | "fromHex") {
        return lower_global_builtin_static_value(ctx, "Uint8Array", property);
    }
    if property == "supports" {
        return lower_global_builtin_static_value(ctx, "SubtleCrypto", property);
    }
    if matches!(
        property,
        "abs"
            | "acos"
            | "acosh"
            | "asin"
            | "asinh"
            | "atan"
            | "atan2"
            | "atanh"
            | "cbrt"
            | "ceil"
            | "clz32"
            | "cos"
            | "cosh"
            | "exp"
            | "expm1"
            | "f16round"
            | "floor"
            | "fround"
            | "hypot"
            | "imul"
            | "log"
            | "log1p"
            | "log2"
            | "log10"
            | "max"
            | "min"
            | "pow"
            | "random"
            | "round"
            | "sign"
            | "sin"
            | "sinh"
            | "sqrt"
            | "tan"
            | "tanh"
            | "trunc"
    ) {
        return lower_global_builtin_static_value(ctx, "Math", property);
    }
    if matches!(
        property,
        "Console"
            | "log"
            | "info"
            | "debug"
            | "error"
            | "warn"
            | "assert"
            | "dir"
            | "dirxml"
            | "trace"
            | "table"
            | "clear"
            | "count"
            | "countReset"
            | "time"
            | "timeEnd"
            | "timeLog"
            | "group"
            | "groupCollapsed"
            | "groupEnd"
            | "profile"
            | "profileEnd"
            | "timeStamp"
    ) {
        emit_global_value_installs(ctx, "console");
        let mod_idx = ctx.strings.intern("console");
        let mod_bytes_global = format!("@{}", ctx.strings.entry(mod_idx).bytes_global);
        let mod_len_str = "console".len().to_string();
        let prop_idx = ctx.strings.intern(property);
        let prop_bytes_global = format!("@{}", ctx.strings.entry(prop_idx).bytes_global);
        let prop_len_str = property.len().to_string();
        return Ok(ctx.block().call(
            DOUBLE,
            "js_native_module_property_by_name",
            &[
                (PTR, &mod_bytes_global),
                (I64, &mod_len_str),
                (PTR, &prop_bytes_global),
                (I64, &prop_len_str),
            ],
        ));
    }
    // node:process — `process.abort` / `process.umask` etc. read
    // as VALUES (not called). Bare `process` lowers to the
    // GlobalGet(0) sentinel, so the receiver name is gone here;
    // route by the process-distinctive property name through the
    // native-module property helper, which returns a bound-method
    // closure (typeof "function"). The call forms lower separately
    // via dedicated HIR variants. (#1374, #1373)
    if matches!(
        property,
        "abort"
            | "cwd"
            | "uptime"
            | "memoryUsage"
            | "nextTick"
            | "chdir"
            | "kill"
            | "exit"
            | "umask"
            | "setSourceMapsEnabled"
            | "hasUncaughtExceptionCaptureCallback"
            | "setUncaughtExceptionCaptureCallback"
            | "addUncaughtExceptionCaptureCallback"
            | "threadCpuUsage"
            | "availableMemory"
            | "constrainedMemory"
            | "getuid"
            | "geteuid"
            | "getgid"
            | "getegid"
            | "getgroups"
            | "setuid"
            | "seteuid"
            | "setgid"
            | "setegid"
            | "setgroups"
            | "initgroups"
            | "emitWarning"
            | "on"
            | "addListener"
            | "once"
            | "prependListener"
            | "prependOnceListener"
            | "emit"
            | "listeners"
            | "rawListeners"
            | "eventNames"
            | "listenerCount"
            | "removeListener"
            | "off"
            | "removeAllListeners"
            | "setMaxListeners"
            | "getMaxListeners"
            | "cpuUsage"
            | "resourceUsage"
            | "getActiveResourcesInfo"
            | "hrtime"
    ) {
        emit_global_value_installs(ctx, "process");
        let mod_idx = ctx.strings.intern("process");
        let mod_bytes_global = format!("@{}", ctx.strings.entry(mod_idx).bytes_global);
        let mod_len_str = "process".len().to_string();
        let prop_idx = ctx.strings.intern(property);
        let prop_bytes_global = format!("@{}", ctx.strings.entry(prop_idx).bytes_global);
        let prop_len_str = property.len().to_string();
        return Ok(ctx.block().call(
            DOUBLE,
            "js_native_module_property_by_name",
            &[
                (PTR, &mod_bytes_global),
                (I64, &mod_len_str),
                (PTR, &prop_bytes_global),
                (I64, &prop_len_str),
            ],
        ));
    }
    // Built-in constructors / namespaces exposed on globalThis
    // (`Array`, `Object`, `Math`, `JSON`, ...): route the read
    // through the singleton so `globalThis.Array` (and the
    // identical `(globalThis as any).X` shape) returns the
    // pre-populated constructor backing-object instead of the
    // `0.0` no-value placeholder. Mirrors the IndexGet arm above
    // (Expr::IndexGet at ~2381) which already routes
    // `globalThis[<string>]` through `js_get_global_this`. The
    // runtime populates these on first init — see
    // `populate_global_this_builtins` in
    // crates/perry-runtime/src/object.rs. Unblocks lodash's
    // `runInContext` (`var Array = context.Array; var arrayProto
    // = Array.prototype`) — the prior `0.0` placeholder caused
    // the `.prototype` chained read on the locally-bound
    // alias to throw `Cannot read properties of undefined`.
    if is_global_this_builtin_name(property) {
        return lower_global_builtin_read(ctx, property);
    }
    // Unknown member on a builtin global namespace object
    // (`Reflect.enumerate`, `Math.bogus`, `JSON.bogus`, …): JS
    // semantics is a plain `undefined` property miss, not `0`. The
    // HIR collapsed the receiver to the `GlobalGet(0)` sentinel so we
    // can't tell which namespace it was, but an unrecognized member
    // read is `undefined` for every one of them. (The legacy `0.0`
    // here made `typeof Math.bogus === "number"` and broke
    // feature-detection like `Reflect.enumerate === undefined`.)
    Ok(double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED)))
}
