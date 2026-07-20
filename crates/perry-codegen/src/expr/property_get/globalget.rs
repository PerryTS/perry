//! `Expr::GlobalGet` receiver dispatch extracted from `property_get.rs`.
//!
//! Pure mechanical move — body is the verbatim contents of the
//! `if matches!(object.as_ref(), Expr::GlobalGet(_)) { ... }` block from the
//! general catch-all arm, lifted into its own function.

use super::*;

use anyhow::Result;
#[allow(unused_imports)]
use perry_hir::{BinaryOp, CompareOp, Expr, UnaryOp, UpdateOp};
#[allow(unused_imports)]
use perry_types::Type as HirType;

#[allow(unused_imports)]
use crate::lower_call::{lower_call, lower_native_method_call, lower_new};
#[allow(unused_imports)]
use crate::lower_conditional::{lower_conditional, lower_logical, lower_truthy};
#[allow(unused_imports)]
use crate::lower_string_method::{
    flatten_string_add_chain, lower_string_coerce_concat, lower_string_concat,
    lower_string_concat_chain, lower_string_self_append,
};
#[allow(unused_imports)]
use crate::nanbox::{double_literal, POINTER_MASK_I64};
use crate::native_value::{
    BoundsState, BufferAccessMode, LoweredValue, MaterializationReason, NativeRep, SemanticKind,
};
#[allow(unused_imports)]
use crate::type_analysis::{
    compute_auto_captures, is_array_expr, is_bigint_expr, is_bool_expr, is_map_expr,
    is_numeric_expr, is_numeric_typed_array_class, is_set_expr, is_string_expr,
    is_url_search_params_expr, receiver_class_name,
};
#[allow(unused_imports)]
use crate::types::{DOUBLE, I1, I32, I64, I8, PTR};

/// Lower a `PropertyGet` whose receiver is the `GlobalGet(0)` builtin-global
/// sentinel, read by the `property` string alone (the receiver name has been
/// collapsed during HIR lowering).
pub(crate) fn lower_globalget_property(ctx: &mut FnCtx<'_>, property: &str) -> Result<String> {
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
        return Ok(lower_global_builtin_static_value(ctx, "Promise", property));
    }
    // `Proxy.revocable` read as a VALUE (not in a call) — the receiver is
    // collapsed to GlobalGet(0) so we route by property name. Resolves the
    // closure installed by `install_builtin_constructor_statics("Proxy", …)`.
    if property == "revocable" {
        return Ok(lower_global_builtin_static_value(ctx, "Proxy", property));
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
        let error_idx = ctx.strings.intern("Error");
        let error_bytes_global = format!("@{}", ctx.strings.entry(error_idx).bytes_global);
        let error_len = "Error".len().to_string();
        let error_ctor = ctx.block().call(
            DOUBLE,
            "js_get_global_this_builtin_value",
            &[(PTR, &error_bytes_global), (I64, &error_len)],
        );
        let key_idx = ctx.strings.intern(property);
        let key_handle_global = format!("@{}", ctx.strings.entry(key_idx).handle_global);
        let blk = ctx.block();
        let ctor_handle = unbox_to_i64(blk, &error_ctor);
        let key_box = blk.load(DOUBLE, &key_handle_global);
        let key_bits = blk.bitcast_double_to_i64(&key_box);
        let key_raw = blk.and(I64, &key_bits, POINTER_MASK_I64);
        return Ok(blk.call(
            DOUBLE,
            "js_object_get_field_by_name_f64",
            &[(I64, &ctor_handle), (I64, &key_raw)],
        ));
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
        return Ok(lower_global_builtin_static_value(ctx, "Object", property));
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
        let object_idx = ctx.strings.intern("Object");
        let object_bytes_global = format!("@{}", ctx.strings.entry(object_idx).bytes_global);
        let object_len = "Object".len().to_string();
        let object_ctor = ctx.block().call(
            DOUBLE,
            "js_get_global_this_builtin_value",
            &[(PTR, &object_bytes_global), (I64, &object_len)],
        );
        let key_idx = ctx.strings.intern(property);
        let key_handle_global = format!("@{}", ctx.strings.entry(key_idx).handle_global);
        let blk = ctx.block();
        let ctor_handle = unbox_to_i64(blk, &object_ctor);
        let key_box = blk.load(DOUBLE, &key_handle_global);
        let key_bits = blk.bitcast_double_to_i64(&key_box);
        let key_raw = blk.and(I64, &key_bits, POINTER_MASK_I64);
        return Ok(blk.call(
            DOUBLE,
            "js_object_get_field_by_name_f64",
            &[(I64, &ctor_handle), (I64, &key_raw)],
        ));
    }
    // #4033: `ArrayBuffer.isView` must also work as a value
    // (`const isView = ArrayBuffer.isView; isView(view)`). Bare
    // builtin receivers are collapsed to `GlobalGet(0)`, so recover
    // the populated constructor closure and read the reified static.
    if property == "isView" {
        let ctor_idx = ctx.strings.intern("ArrayBuffer");
        let ctor_bytes_global = format!("@{}", ctx.strings.entry(ctor_idx).bytes_global);
        let ctor_len = "ArrayBuffer".len().to_string();
        let ctor = ctx.block().call(
            DOUBLE,
            "js_get_global_this_builtin_value",
            &[(PTR, &ctor_bytes_global), (I64, &ctor_len)],
        );
        let key_idx = ctx.strings.intern(property);
        let key_handle_global = format!("@{}", ctx.strings.entry(key_idx).handle_global);
        let blk = ctx.block();
        let ctor_handle = unbox_to_i64(blk, &ctor);
        let key_box = blk.load(DOUBLE, &key_handle_global);
        let key_bits = blk.bitcast_double_to_i64(&key_box);
        let key_raw = blk.and(I64, &key_bits, POINTER_MASK_I64);
        return Ok(blk.call(
            DOUBLE,
            "js_object_get_field_by_name_f64",
            &[(I64, &ctor_handle), (I64, &key_raw)],
        ));
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
        let ctor_idx = ctx.strings.intern("Uint8Array");
        let ctor_bytes_global = format!("@{}", ctx.strings.entry(ctor_idx).bytes_global);
        let ctor_len = "Uint8Array".len().to_string();
        let ctor = ctx.block().call(
            DOUBLE,
            "js_get_global_this_builtin_value",
            &[(PTR, &ctor_bytes_global), (I64, &ctor_len)],
        );
        let key_idx = ctx.strings.intern(property);
        let key_handle_global = format!("@{}", ctx.strings.entry(key_idx).handle_global);
        let blk = ctx.block();
        let ctor_handle = unbox_to_i64(blk, &ctor);
        let key_box = blk.load(DOUBLE, &key_handle_global);
        let key_bits = blk.bitcast_double_to_i64(&key_box);
        let key_raw = blk.and(I64, &key_bits, POINTER_MASK_I64);
        return Ok(blk.call(
            DOUBLE,
            "js_object_get_field_by_name_f64",
            &[(I64, &ctor_handle), (I64, &key_raw)],
        ));
    }
    if property == "supports" {
        let ctor_idx = ctx.strings.intern("SubtleCrypto");
        let ctor_bytes_global = format!("@{}", ctx.strings.entry(ctor_idx).bytes_global);
        let ctor_len = "SubtleCrypto".len().to_string();
        let ctor = ctx.block().call(
            DOUBLE,
            "js_get_global_this_builtin_value",
            &[(PTR, &ctor_bytes_global), (I64, &ctor_len)],
        );
        let key_idx = ctx.strings.intern(property);
        let key_handle_global = format!("@{}", ctx.strings.entry(key_idx).handle_global);
        let blk = ctx.block();
        let ctor_handle = unbox_to_i64(blk, &ctor);
        let key_box = blk.load(DOUBLE, &key_handle_global);
        let key_bits = blk.bitcast_double_to_i64(&key_box);
        let key_raw = blk.and(I64, &key_bits, POINTER_MASK_I64);
        return Ok(blk.call(
            DOUBLE,
            "js_object_get_field_by_name_f64",
            &[(I64, &ctor_handle), (I64, &key_raw)],
        ));
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
        let math_idx = ctx.strings.intern("Math");
        let math_bytes_global = format!("@{}", ctx.strings.entry(math_idx).bytes_global);
        let math_len = "Math".len().to_string();
        let math_obj = ctx.block().call(
            DOUBLE,
            "js_get_global_this_builtin_value",
            &[(PTR, &math_bytes_global), (I64, &math_len)],
        );
        let key_idx = ctx.strings.intern(property);
        let key_handle_global = format!("@{}", ctx.strings.entry(key_idx).handle_global);
        let blk = ctx.block();
        let math_handle = unbox_to_i64(blk, &math_obj);
        let key_box = blk.load(DOUBLE, &key_handle_global);
        let key_bits = blk.bitcast_double_to_i64(&key_box);
        let key_raw = blk.and(I64, &key_bits, POINTER_MASK_I64);
        return Ok(blk.call(
            DOUBLE,
            "js_object_get_field_by_name_f64",
            &[(I64, &math_handle), (I64, &key_raw)],
        ));
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
        let key_idx = ctx.strings.intern(property);
        let key_bytes_global = format!("@{}", ctx.strings.entry(key_idx).bytes_global);
        let key_len = property.len().to_string();
        return Ok(ctx.block().call(
            DOUBLE,
            "js_get_global_this_builtin_value",
            &[(PTR, &key_bytes_global), (I64, &key_len)],
        ));
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
