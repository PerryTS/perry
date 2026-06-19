//! Type analysis helpers for expression codegen.
//!
//! Pure predicates and type refinement that don't emit IR themselves.
//! Used by `expr.rs`, `lower_call.rs`, `lower_string_method.rs`,
//! `lower_conditional.rs`, and `stmt.rs`.

use perry_hir::{BinaryOp, Expr, UnaryOp};
use perry_types::Type as HirType;

use crate::expr::FnCtx;
use crate::type_analysis_facts::{
    function_type_from_decl, hir_inferred_refinable_type, hir_inferred_static_type,
};
use crate::type_analysis_net::{net_result_class, net_result_type};

#[cfg(test)]
pub(crate) use crate::type_analysis_facts::{
    hir_inferred_refinable_type_from_facts, hir_inferred_refinable_type_from_locals,
    hir_inferred_static_type_from_locals, CodegenTypeFacts,
};

// Class-field layout / declared-type resolution lives in a sibling module
// (file-size gate). Re-exported here so existing `type_analysis::*` call
// sites keep resolving, and brought into scope for local callers.
pub(crate) use crate::type_analysis_class_fields::{
    class_field_declared_type, class_field_global_index, declared_field_type,
};

pub(crate) fn is_global_constructor_expr(e: &Expr, name: &str) -> bool {
    matches!(e, Expr::GlobalGet(_))
        || matches!(
            e,
            Expr::PropertyGet { object, property }
                if property == name && matches!(object.as_ref(), Expr::GlobalGet(_))
        )
}

fn is_process_module_ref_name(module: &str) -> bool {
    let module = module.strip_prefix("node:").unwrap_or(module);
    matches!(module, "process" | "process.namespace" | "process.default")
}

fn is_process_namespace_version_property(object: &Expr, property: &str) -> bool {
    property == "version"
        && matches!(object, Expr::NativeModuleRef(module) if is_process_module_ref_name(module))
}

/// Refine an `Any`-typed local's static type based on its initializer
/// expression. Returns Some(Type) when we can statically prove the
/// initializer produces a more specific type, so the `Stmt::Let`
/// lowerer can store the more specific type into `local_types` and
/// downstream code (`is_array_expr`, `is_string_expr`) can dispatch
/// to fast paths.
///
/// Recognizes:
/// - Array literals / spread / slice / map / filter / Object.keys → Array
/// - String literals / coerce / join → String
/// - **IndexGet on a known Array<T>** → element type T (so destructuring
///   nested arrays gets the right type for `__item_63 = arr[i]` patterns)
/// - **PropertyGet on a known class field** → the field's declared type
pub(crate) fn refine_type_from_init(ctx: &FnCtx<'_>, init: &Expr) -> Option<HirType> {
    match init {
        // Numeric literals + arithmetic results: refine to Number so the
        // for-loop counter `let i = 0` (and any other untyped numeric
        // local) gets recognized by `is_numeric_expr`. Without this,
        // `i + 1` wraps the `i` load in `js_number_coerce` per iteration
        // because the local stays at type Any. Critical for hot loops
        // in object_create / binary_trees / fibonacci where the counter
        // is a "let i = 0" with no explicit annotation.
        Expr::Number(_)
        | Expr::Integer(_)
        | Expr::PodLayoutSizeOf { .. }
        | Expr::PodLayoutAlignOf { .. }
        | Expr::PodLayoutOffsetOf { .. } => Some(HirType::Number),
        Expr::Binary { op, left, right } => {
            if is_bigint_expr(ctx, init)
                && matches!(
                    op,
                    BinaryOp::Add
                        | BinaryOp::Sub
                        | BinaryOp::Mul
                        | BinaryOp::Div
                        | BinaryOp::Mod
                        | BinaryOp::Pow
                        | BinaryOp::BitAnd
                        | BinaryOp::BitOr
                        | BinaryOp::BitXor
                        | BinaryOp::Shl
                        | BinaryOp::Shr
                )
            {
                return Some(HirType::BigInt);
            }
            // Numeric arithmetic produces Number when both operands are
            // statically numeric (matches `is_numeric_expr`'s rule).
            // Sub/Mul/Div/etc. always produce Number; Add only does so
            // when neither operand is a string.
            if is_numeric_expr(ctx, left) && is_numeric_expr(ctx, right) {
                let _ = op;
                Some(HirType::Number)
            } else {
                None
            }
        }
        Expr::Unary { op, operand } => {
            if matches!(op, UnaryOp::Neg | UnaryOp::BitNot) && is_bigint_expr(ctx, operand) {
                Some(HirType::BigInt)
            } else {
                None
            }
        }
        Expr::Array(_) | Expr::ArraySpread(_) => {
            Some(HirType::Array(Box::new(HirType::Any)))
        }
        // `new Array(n)` / `new Array(a, b, ...)` — the shared HIR inference
        // already maps this to Array<Any>, so the let-binding refinement
        // must agree. Without it, `const xs = new Array(4); xs[i]` falls
        // through to the generic Object index path which doesn't translate
        // the issue #323 HOLE sentinel back to undefined.
        Expr::New { class_name, .. } if class_name == "Array" => {
            Some(HirType::Array(Box::new(HirType::Any)))
        }
        Expr::ArraySlice { .. }
        | Expr::ArrayMap { .. }
        | Expr::ArrayFilter { .. }
        | Expr::ArrayFlat { .. }
        | Expr::ArrayFlatMap { .. }
        | Expr::ArrayFrom(_)
        | Expr::ArrayFromArrayLikeHoley(_)
        | Expr::ArrayFromMapped { .. }
        | Expr::ArraySort { .. }
        | Expr::ArrayToReversed { .. }
        | Expr::ArrayToSorted { .. }
        | Expr::ArrayToSpliced { .. }
        | Expr::ArrayWith { .. }
        | Expr::ObjectValues(_)
        | Expr::ObjectEntries(_)
        | Expr::ArrayEntries { .. }
        | Expr::ArrayKeys { .. }
        | Expr::ArrayValues { .. }
        | Expr::StringMatch { .. } => hir_inferred_refinable_type(ctx, init)
            .or_else(|| Some(HirType::Array(Box::new(HirType::Any)))),
        Expr::StringMatchAll { .. } => Some(HirType::Any),
        // TextEncoder.encode(str) — runtime returns a BufferHeader with
        // packed u8 bytes (same shape as `new Uint8Array([...])`). Refining
        // the local type to Uint8Array lets `encoded[i]` route through the
        // `Uint8ArrayGet` u8-load fast path. Pre-fix this was Array(Number)
        // and the generic f64-stride indexing read 8 bytes-as-f64 instead
        // of one byte (issue #584).
        Expr::TextEncoderEncode(_) => Some(HirType::Named("Uint8Array".into())),
        Expr::TextEncoderEncodeInto { .. } => Some(HirType::Object(Default::default())),
        // TextDecoder.decode(buf) / .encoding always produce a string.
        Expr::TextDecoderDecode { .. } => Some(HirType::String),
        Expr::TextDecoderEncoding(_) => Some(HirType::String),
        Expr::TextDecoderFatal(_) | Expr::TextDecoderIgnoreBom(_) => Some(HirType::Boolean),
        // string.split(sep) → Array<string>
        Expr::StringSplit { .. } => Some(HirType::Array(Box::new(HirType::String))),
        // Set.values() / Set.keys() → iterable, but Array.from wraps it
        // into an Array. Without an Array.from wrap, it's still iterable.
        // Set/Map constructors refine to `Generic { base, type_args }` —
        // `is_set_expr` / `is_map_expr` check `base == "Set" / "Map"` on the
        // Generic variant, so `Named("Set")` here used to silently miss the
        // fast path and `s.has(v)` returned undefined. Delegate to shared HIR
        // inference so constructor inputs can preserve key/value element facts.
        Expr::SetNewFromArray(_) | Expr::SetNew | Expr::MapNewFromArray(_) | Expr::MapNew => {
            hir_inferred_refinable_type(ctx, init)
        }
        // Object.keys() / for-in keys always return string handles.
        Expr::ObjectKeys(_) | Expr::ForInKeys(_) => {
            Some(HirType::Array(Box::new(HirType::String)))
        }
        Expr::ObjectGetOwnPropertyNames(_) => Some(HirType::Array(Box::new(HirType::String))),
        Expr::ObjectGetOwnPropertySymbols(_) => Some(HirType::Array(Box::new(HirType::Any))),
        Expr::String(_)
        | Expr::WtfString(_)
        | Expr::ArrayJoin { .. }
        | Expr::StringCoerce(_)
        | Expr::StringFromCodePoint(_)
        | Expr::StringFromCharCode(_)
        | Expr::StringFromCharCodeSpread(_)
        | Expr::StringRaw { .. }
        | Expr::StringAt { .. }
        | Expr::RegExpSource(_)
        | Expr::RegExpFlags(_)
        // process/os string accessors — lower to runtime calls that
        // return NaN-boxed strings in expr.rs. Refining the local type
        // to String lets `const v = process.version; v.startsWith('v')`
        // hit the string method fast path.
        | Expr::ProcessVersion
        | Expr::ProcessCwd
        | Expr::ProcessTitle
        | Expr::OsArch
        | Expr::OsType
        | Expr::OsPlatform
        | Expr::OsRelease
        | Expr::OsHostname
        | Expr::OsEOL
        | Expr::OsDevNull
        | Expr::OsEndianness
        | Expr::OsMachine
        | Expr::OsVersion
        // Date string-returning methods all produce real string handles
        // via js_date_to_*_string. Refining the local lets `dateStr.includes("2024")`
        // hit the string .includes fast path.
        | Expr::DateToString(_)
        | Expr::DateToDateString(_)
        | Expr::DateToTimeString(_)
        | Expr::DateToUTCString(_)
        | Expr::DateToLocaleString(_)
        | Expr::DateToLocaleDateString(_)
        | Expr::DateToLocaleTimeString(_)
        | Expr::DateToISOString(_)
        | Expr::DateToJSON(_)
        // node:path constants
        | Expr::PathSep
        | Expr::PathDelimiter
        // JSON.stringify returns a string (Union<String,Void> for toJSON
        // interop, but always a string in practice for the common case —
        // explicitly refining to String makes `s.includes(...)` /
        // `s.split(...)` etc. hit the string method fast path).
        | Expr::JsonStringify(_)
        | Expr::JsonStringifyPretty { .. }
        | Expr::JsonStringifyFull(..) => Some(HirType::String),
        // `atob(b64)` / `btoa(s)` return raw binary strings. Without
        // this refinement, `const dec = atob(...)` is typed as Any, so
        // chained `dec.charCodeAt(i)` routes through the universal
        // method dispatcher (which doesn't know how to handle string
        // pointers — `js_native_call_method` returns a NULL_OBJECT
        // sentinel that prints as `[object Object]`). With the local
        // refined to String, charCodeAt hits the inline string fast
        // path that calls `js_string_char_code_at`.
        Expr::Atob(_) | Expr::Btoa(_) => Some(HirType::String),
        // fs.readFileSync(path, 'utf8') returns a NaN-boxed string;
        // fs.readFileSync(path) (no encoding, lowered to FsReadFileBinary)
        // returns a Buffer. Refining the string variant lets `.split()`
        // / `.length` / etc. take the string fast path. The Buffer variant
        // dispatches through the POINTER_TAG path with BUFFER_REGISTRY.
        Expr::FsReadFileSync(_) => Some(HirType::String),
        // `process.hrtime.bigint()` returns a BigInt value. Refining the
        // local type lets `hr2 >= hr1` route through the BigInt compare
        // fast path (`js_bigint_cmp`) instead of fcmp-on-NaN.
        Expr::ProcessHrtimeBigint => Some(HirType::BigInt),
        Expr::StaticMethodCall {
            class_name,
            method_name,
            ..
        } => ctx
            .classes
            .get(class_name)
            .and_then(|class| {
                class
                    .static_methods
                    .iter()
                    .find(|method| method.name == *method_name)
            })
            .map(|method| method.return_type.clone()),
        // `BigInt(x)` / `0n` literal via StringCoerce paths.
        // `BigInt('123')` lowers to BigIntCoerce; refine so `const x = BigInt(str)`
        // gets local type BigInt and `x === y` routes through js_bigint_cmp.
        Expr::BigInt(_) | Expr::BigIntCoerce(_) => Some(HirType::BigInt),
        // `let l = new ClassName<...>()` — refine to Named(ClassName)
        // so subsequent `l.method()` dispatch goes through the class
        // method registry instead of the universal fallback. This is
        // the difference between `l.size()` returning the real size
        // and returning undefined for generic class instances.
        // WHATWG URL constructors — both routes (`new URL(...)` /
        // `new URL(rel, base)`) go through the dedicated HIR variant
        // `Expr::UrlNew`, which bypasses the generic `Expr::New` arm
        // below. Refining to `Named("URL")` lets `u.searchParams.get(k)` and
        // friends hit the `is_url_search_params_expr` fast paths.
        Expr::UrlNew { .. } => Some(HirType::Named("URL".to_string())),
        Expr::UrlPatternNew { .. } => Some(HirType::Named("URLPattern".to_string())),
        Expr::UrlSearchParamsNew(_) => Some(HirType::Named("URLSearchParams".to_string())),
        // `url.searchParams` getter on a typed URL: refining lets a chained
        // `const sp = url.searchParams; sp.append(...)` keep the typed
        // dispatch instead of falling through to generic property access.
        Expr::UrlGetSearchParams(_) => Some(HirType::Named("URLSearchParams".to_string())),
        Expr::New { class_name, .. } => {
            // Resolve through `local_class_aliases` so `let b: any = new Y()`
            // (where `let Y = SomeClass` aliased Y → SomeClass) refines `b`
            // to `Named("SomeClass")` instead of `Named("Y")`. Without this,
            // the PropertyGet fast path looks up "Y" in `ctx.classes`, finds
            // nothing, and falls back to the slow path —
            // `js_object_get_field_by_name_f64`. The slow path is broken
            // for fast-path-allocated objects, so the read returns undefined
            // even though the field is correctly initialized in memory.
            // Resolving the alias here keeps `b` on the fast field-access
            // path that matches how `lower_new` actually built the object.
            let resolved = ctx
                .local_class_aliases
                .get(class_name.as_str())
                .cloned()
                .unwrap_or_else(|| class_name.clone());
            Some(HirType::Named(resolved))
        }
        // Buffer / Uint8Array constructors all produce a Buffer instance.
        // Refining the local lets `buf[i]`/`buf.length` use the byte-indexed
        // fast path (`js_buffer_get`/`js_buffer_length`) and `buf.method(...)`
        // route through the runtime buffer dispatch — without this they
        // fall through to the dynamic-array codegen which reads f64 elements
        // from the underlying storage as if they were JS values.
        Expr::BufferFrom { .. }
        | Expr::BufferFromArrayBuffer { .. }
        | Expr::BufferAlloc { .. }
        | Expr::BufferAllocUnsafe(_)
        | Expr::BufferConcat(_)
        | Expr::BufferConcatWithLength { .. }
        | Expr::CryptoRandomBytes(_) => Some(HirType::Named("Uint8Array".into())),
        e if net_result_type(e).is_some() => net_result_type(e),
        Expr::NativeMethodCall {
            module,
            method,
            object: None,
            ..
        } if module == "buffer" && method == "copyBytesFrom" => {
            Some(HirType::Named("Uint8Array".into()))
        }
        Expr::NativeMethodCall {
            module,
            method,
            object: None,
            ..
        } if matches!(module.as_str(), "http" | "https")
            && matches!(method.as_str(), "request" | "get") =>
        {
            Some(HirType::Named("ClientRequest".into()))
        }
        // Compare results are now NaN-boxed booleans (TAG_TRUE/FALSE).
        // Type-refining the local as Boolean lets is_numeric_expr
        // skip the fast path (which would emit fcmp/sitofp on a NaN
        // bit pattern, giving wrong results) and routes printing
        // through js_console_log_dynamic which dispatches on the
        // NaN tag to print "true"/"false" instead of "1"/"0".
        Expr::Compare { .. } | Expr::Bool(_) => Some(HirType::Boolean),
        // Issue #637: `a || b` / `a && b` produce the operand's value
        // per JS spec, NOT a boolean. Only refine as Boolean when BOTH
        // operands are statically known to be bool — otherwise the
        // result inherits whatever truthy operand wins. Pre-fix,
        // `let c = objA || objB` had `c` typed as Boolean, and
        // subsequent `if (c)` / `!c` went through the bool fast-path
        // `bits == TAG_TRUE_I64` which returned false for the
        // NaN-boxed pointer (whose bits don't equal TAG_TRUE), so the
        // `if (c)` branch was treated as falsy even though `c` was a
        // real object reference. Repro: `const a = {x:1}; const b =
        // {y:2}; const c = a || b; if (c) ...` — pre-fix took the
        // else branch.
        Expr::Logical { left, right, .. } => {
            if is_bool_expr(ctx, left) && is_bool_expr(ctx, right) {
                Some(HirType::Boolean)
            } else {
                None
            }
        }
        Expr::IndexGet { object, .. } => {
            // arr[i] where arr is Array<T> → element type T.
            // Handles both LocalGet(arr) and PropertyGet(this, "field")
            // — the latter lets `this.parts[i]` get the right type
            // when `parts: string[]`.
            if let Expr::LocalGet(arr_id) = object.as_ref() {
                if let Some(HirType::Array(elem_ty)) = ctx.local_types.get(arr_id) {
                    return Some((**elem_ty).clone());
                }
                // str[i] — single-char string from string indexing.
                if let Some(HirType::String) = ctx.local_types.get(arr_id) {
                    return Some(HirType::String);
                }
            }
            if let Some(ty) = static_type_of(ctx, object) {
                if let HirType::Array(elem_ty) = ty {
                    return Some(*elem_ty);
                }
                if let HirType::String = ty {
                    return Some(HirType::String);
                }
            }
            None
        }
        Expr::PropertyGet { object, property } => {
            if is_process_namespace_version_property(object, property) {
                return Some(HirType::String);
            }
            // Error instance `e.message` / `e.stack` / `e.name` — all
            // return string handles via the runtime's GC_TYPE_ERROR
            // dispatch in js_object_get_field_by_name_f64. Refining to
            // String lets `const m = e.message; m.length` hit the
            // string fast path instead of returning undefined.
            // NOTE: `.stack` is deliberately excluded — `Error.prepareStackTrace`
            // can make `.stack` an ARRAY of CallSites (depd / source-map-support),
            // and a plain object may carry any `.stack` value. Typing it String
            // unconditionally corrupted those array values on store (the array
            // pointer got reinterpreted as a string). `.stack` stays `Any`.
            if matches!(property.as_str(), "message" | "name") {
                // A user class's DECLARED field type wins over the Error String assumption.
                let declared = receiver_class_name(ctx, object).and_then(|c| {
                    let class = ctx.classes.get(&c)?;
                    class.fields.iter().find(|f| f.name == *property).map(|f| f.ty.clone())
                });
                return Some(declared.unwrap_or(HirType::String));
            }
            // obj.field where obj is a known class instance → field's
            // declared type. Reuses the same walk static_type_of uses.
            let receiver_class = receiver_class_name(ctx, object)?;
            let class = ctx.classes.get(&receiver_class)?;
            class
                .fields
                .iter()
                .find(|f| f.name == *property)
                .map(|f| f.ty.clone())
        }
        // Promise-returning expressions: `Promise.resolve(x)`,
        // `p.then(cb)`, `p.catch(cb)`, etc. Refine the local to
        // `Promise(Any)` so `is_promise_expr` can detect subsequent
        // `.then()` / `.catch()` chains.
        Expr::Call { callee, args, .. } => {
            if is_promise_expr(ctx, init) {
                return Some(HirType::Promise(Box::new(HirType::Any)));
            }
            // fs.readdirSync(path) → Array<String>. HIR lowers this as
            // `Call { callee: PropertyGet { object: NativeModuleRef("fs"),
            // property: "readdirSync" } }` — refine so `entries.includes(...)`
            // hits the array fast path via is_array_expr.
            // Same for realpathSync/mkdtempSync (string-returning).
            if let Expr::PropertyGet { object, property } = callee.as_ref() {
                if matches!(object.as_ref(), Expr::NativeModuleRef(m) if m == "fs") {
                    match property.as_str() {
                        "readdirSync" => {
                            return Some(HirType::Array(Box::new(HirType::String)));
                        }
                        "realpathSync" | "mkdtempSync" | "readlinkSync"
                        | "readFileSync" => {
                            return Some(HirType::String);
                        }
                        _ => {}
                    }
                }
                if matches!(object.as_ref(), Expr::NativeModuleRef(m) if m == "crypto") {
                    match property.as_str() {
                        // #1432: crypto factories / KDFs that return a
                        // NaN-boxed BufferHeader. Without this refinement
                        // they're typed `Any`, so the HMAC fast-path's
                        // `key_is_buffer` check can't identify a
                        // `SecretKey` / `pbkdf2Sync` result as a Buffer —
                        // the call falls through to handle-dispatch
                        // (~3 mutex locks) instead of the inline-FFI
                        // literal-key fast path.
                        "createSecretKey"
                        | "generateKeySync"
                        | "scryptSync"
                        | "pbkdf2Sync"
                        | "argon2Sync"
                        | "decapsulate"
                        | "hkdfSync"
                        | "randomBytes" => {
                            return Some(HirType::Named("Buffer".into()));
                        }
                        // Inventory helpers expose a `string[]` to JS.
                        "getHashes" | "getCiphers" | "getCurves" => {
                            return Some(HirType::Array(Box::new(HirType::String)));
                        }
                        // `generateKeyPairSync` returns a `{ publicKey,
                        // privateKey }` object; tagging it lets callers
                        // refine the field types downstream.
                        "generateKeyPairSync" => {
                            return Some(HirType::Named("CryptoKeyPair".into()));
                        }
                        _ => {}
                    }
                }
            }
            // `crypto.createHash(alg).update(data).digest(enc)` chain.
            // The expr.rs handler collapses this into a runtime call. With an
            // encoding arg (`'hex'`/`'base64'`/…) it returns a NaN-boxed
            // string — refine to String so `hmac === hmac2` routes through
            // `js_string_equals` instead of bit-comparing two distinct
            // allocations. With no arg (or `undefined`), `digest()` returns a
            // Buffer; refining to Uint8Array lets `buf.toString('hex')` and
            // `buf[i]` take the buffer dispatch instead of mis-reading the
            // raw bytes as a Latin-1 string (#1353).
            if is_crypto_digest_chain(callee) {
                let no_encoding = match args.first() {
                    None => true,
                    Some(Expr::Undefined) => true,
                    _ => false,
                };
                return Some(if no_encoding {
                    HirType::Named("Uint8Array".into())
                } else {
                    HirType::String
                });
            }
            // String prototype methods that return strings — when called
            // on a known-string receiver, the result is also a string.
            // Without this refinement, `const fixed = s.toWellFormed()`
            // gets typed as Any and chained `fixed.isWellFormed()` routes
            // through dynamic dispatch (which prints `[object Object]`).
            // Mirrors the `is_string_expr` logic just below.
            if let Expr::PropertyGet { property, object } = callee.as_ref() {
                let returns_string = matches!(
                    property.as_str(),
                    "toString" | "toLowerCase" | "toUpperCase" | "trim"
                        | "trimStart" | "trimEnd" | "slice" | "substring"
                        | "substr" | "charAt" | "repeat" | "replace"
                        | "replaceAll" | "padStart" | "padEnd" | "concat"
                        | "normalize" | "at" | "toWellFormed"
                );
                if returns_string && is_string_expr(ctx, object) {
                    return Some(HirType::String);
                }
            }
            if let Some(ret_ty) = static_type_of(ctx, init) {
                if !matches!(ret_ty, HirType::Any | HirType::Void | HirType::Function(_)) {
                    return Some(ret_ty);
                }
            }
            None
        }
        _ => hir_inferred_refinable_type(ctx, init),
    }
}

/// Detects the `crypto.createHash(alg).update(data).digest(enc)` /
/// `crypto.createHmac(alg, key).update(data).digest(enc)` chain shape.
/// Walks the nested PropertyGet→Call structure looking for the
/// `NativeModuleRef("crypto")` root.
/// Wrapper used by the call-site refinement: returns `true` when the
/// callee is the `crypto.create(Hash|Hmac)(...).update(...).digest(...)`
/// shape, regardless of whether the encoding arg is present.
fn is_crypto_digest_chain(callee: &Expr) -> bool {
    crypto_digest_chain_has_string_encoding(callee).is_some()
}

#[allow(dead_code)]
fn crypto_digest_chain_has_string_encoding(callee: &Expr) -> Option<bool> {
    let Expr::PropertyGet {
        property: p1,
        object: o1,
    } = callee
    else {
        return None;
    };
    if p1 != "digest" {
        return None;
    }
    let Expr::Call {
        callee: c2,
        args: digest_args,
        ..
    } = o1.as_ref()
    else {
        return None;
    };
    let Expr::PropertyGet {
        property: p2,
        object: o2,
    } = c2.as_ref()
    else {
        return None;
    };
    if p2 != "update" {
        return None;
    }
    let Expr::Call { callee: c3, .. } = o2.as_ref() else {
        return None;
    };
    let Expr::PropertyGet {
        property: p3,
        object: o3,
    } = c3.as_ref()
    else {
        return None;
    };
    if p3 != "createHash" && p3 != "createHmac" {
        return None;
    }
    if !matches!(o3.as_ref(), Expr::NativeModuleRef(n) if n == "crypto") {
        return None;
    }
    // Node returns a Buffer for `.digest()` with no encoding and a string
    // when an encoding is supplied. Preserve that distinction so
    // `.digest().toString("hex")` dispatches through Buffer, not String.
    if digest_args.is_empty() || matches!(digest_args.first(), Some(Expr::Undefined)) {
        return Some(false);
    }
    if matches!(digest_args.first(), Some(Expr::String(s)) if s.eq_ignore_ascii_case("buffer")) {
        return Some(false);
    }
    Some(true)
}

/// Compute the effective list of capture LocalIds for a closure. Starts
/// with the HIR's `captures` list (which may be empty if the closure
/// conversion pass missed it), then walks the body to find any LocalGet/
/// LocalSet/Update on ids that aren't params, inner-lets, or module
/// globals — those are the auto-detected captures.
///
/// Both the closure creation site (`Expr::Closure` lowering in
/// `lower_expr`) and the closure body site (`compile_closure` in
/// `codegen.rs`) call this so they agree on the slot indices.
pub(crate) fn compute_auto_captures(
    ctx: &FnCtx<'_>,
    params: &[perry_hir::Param],
    body: &[perry_hir::Stmt],
    explicit: &[u32],
) -> Vec<u32> {
    // Exclude module globals from the explicit captures list. perry-hir
    // sometimes lists block-scoped top-level lets (those whose
    // `inside_block_scope > 0`) in `Closure.captures` — the HIR-side
    // `module_level_ids` filter only catches the strict module-top
    // case. If such a var was later globalized (referenced from any
    // function/closure body, see codegen.rs:1029), capturing it would
    // store the global's f64 VALUE in the capture slot — not a box
    // pointer. The closure body, which sees `boxed_vars.contains(id)`,
    // would then deref that f64 as a box pointer (0x0 → "invalid box
    // pointer 0x0" warning, count stays 0). Symmetric with the
    // auto-detected branch below: closures auto-load module globals
    // directly through `@perry_global_*`, no capture slot needed.
    let mut out: Vec<u32> = explicit
        .iter()
        .copied()
        .filter(|id| !ctx.module_globals.contains_key(id))
        .collect();
    let mut referenced: std::collections::HashSet<u32> = std::collections::HashSet::new();
    crate::collectors::collect_ref_ids_in_stmts(body, &mut referenced);
    let mut inner_lets: std::collections::HashSet<u32> = std::collections::HashSet::new();
    crate::collectors::collect_let_ids(body, &mut inner_lets);
    let param_ids: std::collections::HashSet<u32> = params.iter().map(|p| p.id).collect();
    let already: std::collections::HashSet<u32> = out.iter().copied().collect();
    // Sort for determinism (HashSet iteration order is unspecified).
    let mut sorted: Vec<u32> = referenced.into_iter().collect();
    sorted.sort();
    for id in sorted {
        if !param_ids.contains(&id)
            && !inner_lets.contains(&id)
            && !already.contains(&id)
            && !ctx.module_globals.contains_key(&id)
        {
            out.push(id);
        }
    }
    out
}

/// Statically determine whether an expression evaluates to a real numeric
/// `double` (NOT a NaN-boxed value). Used by `lower_truthy` to decide
/// between the fast `fcmp one cond, 0.0` test and the runtime
/// `js_is_truthy` dispatch.
///
/// Recognizes:
/// - integer/number literals
/// - LocalGet of `Number`/`Int32`-typed locals
/// - arithmetic Binary / Compare results (always raw doubles in our model)
/// - the value of an Update (++/--) — also a raw double
///
/// CRUCIALLY excludes Bool, String, Array, Object — those produce
/// NaN-tagged doubles where `fcmp` is unsafe (NaN is unordered).
/// Statically determine whether an expression is a BigInt value. Used by
/// the Compare path to route `a > b` / `a >= b` / `a < b` / `a <= b` through
/// `js_bigint_cmp` instead of the fcmp default (which sees NaN-tagged bits
/// and always reports unordered).
pub(crate) fn is_bigint_expr(ctx: &FnCtx<'_>, e: &Expr) -> bool {
    match e {
        Expr::BigInt(_) => true,
        // `BigInt(x)` always returns a bigint.
        Expr::BigIntCoerce(_) => true,
        Expr::LocalGet(id) => matches!(ctx.local_types.get(id), Some(HirType::BigInt)),
        Expr::StaticMethodCall {
            class_name,
            method_name,
            ..
        } => ctx
            .classes
            .get(class_name)
            .and_then(|class| {
                class
                    .static_methods
                    .iter()
                    .find(|method| method.name == *method_name)
            })
            .is_some_and(|method| matches!(method.return_type, HirType::BigInt)),
        Expr::PropertyGet { .. } | Expr::Call { .. } => {
            matches!(static_type_of(ctx, e), Some(HirType::BigInt))
        }
        // Nested bigint arithmetic — `(n * 10n) + d` must see the
        // inner `n * 10n` as bigint so the outer `+` routes through
        // the bigint dispatch instead of the float fallback.
        Expr::Binary { op, left, right } => {
            matches!(
                op,
                BinaryOp::Add
                    | BinaryOp::Sub
                    | BinaryOp::Mul
                    | BinaryOp::Div
                    | BinaryOp::Mod
                    | BinaryOp::Pow
                    // Bitwise ops on bigints produce bigints — include
                    // them so `(a * prime) & mask64` where both operands
                    // are bigint stays bigint-typed all the way up the
                    // chain. Without this the outer `&` falls through to
                    // the i32 ToInt32 path and returns 0 (closes #39).
                    | BinaryOp::BitAnd
                    | BinaryOp::BitOr
                    | BinaryOp::BitXor
                    | BinaryOp::Shl
                    | BinaryOp::Shr
            ) && (is_bigint_expr(ctx, left) || is_bigint_expr(ctx, right))
        }
        Expr::Unary { op, operand } => {
            matches!(op, UnaryOp::Neg | UnaryOp::BitNot) && is_bigint_expr(ctx, operand)
        }
        _ => false,
    }
}

pub(crate) fn is_numeric_typed_array_class(name: &str) -> bool {
    matches!(
        name,
        "Int8Array"
            | "Uint8Array"
            | "Uint8ClampedArray"
            | "Int16Array"
            | "Uint16Array"
            | "Int32Array"
            | "Uint32Array"
            | "Float16Array"
            | "Float32Array"
            | "Float64Array"
    )
}

fn expression_has_numeric_length(ctx: &FnCtx<'_>, object: &Expr) -> bool {
    match static_type_of(ctx, object) {
        Some(HirType::Array(_)) | Some(HirType::Tuple(_)) | Some(HirType::String) => true,
        Some(HirType::Named(name)) => name == "Buffer" || is_numeric_typed_array_class(&name),
        _ => false,
    }
}

fn native_rep_materializes_to_js_number(rep: &crate::native_value::NativeRep) -> bool {
    matches!(
        rep,
        crate::native_value::NativeRep::I32
            | crate::native_value::NativeRep::I64
            | crate::native_value::NativeRep::U32
            | crate::native_value::NativeRep::U64
            | crate::native_value::NativeRep::USize
            | crate::native_value::NativeRep::F64
            | crate::native_value::NativeRep::F32
            | crate::native_value::NativeRep::U8
            | crate::native_value::NativeRep::BufferLen
            | crate::native_value::NativeRep::HandleId
    )
}

fn pod_record_local_has_materialized_object(ctx: &FnCtx<'_>, local_id: u32) -> bool {
    // Once a POD local has a materialized JS object path, later property
    // reads may observe mutable boxed object state instead of native bytes.
    ctx.native_rep_records.iter().any(|record| {
        record.local_id == Some(local_id) && record.consumer == "pod_record_materialize_object"
    })
}

pub(crate) fn pod_record_field_is_numeric(ctx: &FnCtx<'_>, object: &Expr, field: &str) -> bool {
    let Expr::LocalGet(id) = object else {
        return false;
    };
    if pod_record_local_has_materialized_object(ctx, *id) {
        return false;
    }
    ctx.pod_records
        .get(id)
        .and_then(|local| {
            local
                .layout
                .fields
                .iter()
                .find(|candidate| candidate.name == field)
        })
        .is_some_and(|field| native_rep_materializes_to_js_number(&field.native_rep))
}

fn collect_pod_numeric_field_read_locals(ctx: &FnCtx<'_>, expr: &Expr, out: &mut Vec<u32>) {
    match expr {
        Expr::PropertyGet { object, property }
            if matches!(object.as_ref(), Expr::LocalGet(_))
                && pod_record_field_is_numeric(ctx, object, property) =>
        {
            if let Expr::LocalGet(id) = object.as_ref() {
                out.push(*id);
            }
        }
        Expr::PropertyGet { object, .. } => collect_pod_numeric_field_read_locals(ctx, object, out),
        Expr::PropertySet { object, value, .. } => {
            collect_pod_numeric_field_read_locals(ctx, object, out);
            collect_pod_numeric_field_read_locals(ctx, value, out);
        }
        Expr::IndexGet { object, index } => {
            collect_pod_numeric_field_read_locals(ctx, object, out);
            collect_pod_numeric_field_read_locals(ctx, index, out);
        }
        Expr::IndexSet {
            object,
            index,
            value,
        } => {
            collect_pod_numeric_field_read_locals(ctx, object, out);
            collect_pod_numeric_field_read_locals(ctx, index, out);
            collect_pod_numeric_field_read_locals(ctx, value, out);
        }
        Expr::Binary { left, right, .. } | Expr::Compare { left, right, .. } => {
            collect_pod_numeric_field_read_locals(ctx, left, out);
            collect_pod_numeric_field_read_locals(ctx, right, out);
        }
        Expr::Unary { operand, .. } | Expr::TypeOf(operand) | Expr::Void(operand) => {
            collect_pod_numeric_field_read_locals(ctx, operand, out);
        }
        Expr::Logical { left, right, .. } => {
            collect_pod_numeric_field_read_locals(ctx, left, out);
            collect_pod_numeric_field_read_locals(ctx, right, out);
        }
        Expr::Conditional {
            condition,
            then_expr,
            else_expr,
        } => {
            collect_pod_numeric_field_read_locals(ctx, condition, out);
            collect_pod_numeric_field_read_locals(ctx, then_expr, out);
            collect_pod_numeric_field_read_locals(ctx, else_expr, out);
        }
        Expr::Call { callee, args, .. } => {
            collect_pod_numeric_field_read_locals(ctx, callee, out);
            for arg in args {
                collect_pod_numeric_field_read_locals(ctx, arg, out);
            }
        }
        Expr::NativeMethodCall { object, args, .. } => {
            if let Some(object) = object {
                collect_pod_numeric_field_read_locals(ctx, object, out);
            }
            for arg in args {
                collect_pod_numeric_field_read_locals(ctx, arg, out);
            }
        }
        Expr::New { args, .. } | Expr::NewDynamic { args, .. } => {
            for arg in args {
                collect_pod_numeric_field_read_locals(ctx, arg, out);
            }
        }
        Expr::Array(items) => {
            for item in items {
                collect_pod_numeric_field_read_locals(ctx, item, out);
            }
        }
        Expr::Object(items) => {
            for (_, item) in items {
                collect_pod_numeric_field_read_locals(ctx, item, out);
            }
        }
        _ => {}
    }
}

fn expr_may_materialize_pod_local(ctx: &FnCtx<'_>, expr: &Expr, target_id: u32) -> bool {
    match expr {
        Expr::LocalGet(id) => *id == target_id && ctx.pod_records.contains_key(id),
        Expr::PropertyGet { object, property }
            if matches!(object.as_ref(), Expr::LocalGet(id) if *id == target_id)
                && ctx.pod_records.get(&target_id).is_some_and(|local| {
                    local
                        .layout
                        .fields
                        .iter()
                        .any(|field| field.name == *property)
                }) =>
        {
            false
        }
        Expr::PropertyGet { object, .. } => expr_may_materialize_pod_local(ctx, object, target_id),
        Expr::PropertySet {
            object,
            property,
            value,
        } => {
            let pod_field_set = matches!(object.as_ref(), Expr::LocalGet(id) if *id == target_id)
                && ctx.pod_records.get(&target_id).is_some_and(|local| {
                    local
                        .layout
                        .fields
                        .iter()
                        .any(|field| field.name == *property)
                });
            pod_field_set
                || expr_may_materialize_pod_local(ctx, object, target_id)
                || expr_may_materialize_pod_local(ctx, value, target_id)
        }
        Expr::Call { callee, args, .. } => {
            expr_may_materialize_pod_local(ctx, callee, target_id)
                || args
                    .iter()
                    .any(|arg| expr_may_materialize_pod_local(ctx, arg, target_id))
        }
        Expr::NativeMethodCall { object, args, .. } => {
            object
                .as_ref()
                .is_some_and(|object| expr_may_materialize_pod_local(ctx, object, target_id))
                || args
                    .iter()
                    .any(|arg| expr_may_materialize_pod_local(ctx, arg, target_id))
        }
        Expr::IndexGet { object, index } => {
            expr_may_materialize_pod_local(ctx, object, target_id)
                || expr_may_materialize_pod_local(ctx, index, target_id)
        }
        Expr::IndexSet {
            object,
            index,
            value,
        } => {
            expr_may_materialize_pod_local(ctx, object, target_id)
                || expr_may_materialize_pod_local(ctx, index, target_id)
                || expr_may_materialize_pod_local(ctx, value, target_id)
        }
        Expr::Binary { left, right, .. }
        | Expr::Compare { left, right, .. }
        | Expr::Logical { left, right, .. } => {
            expr_may_materialize_pod_local(ctx, left, target_id)
                || expr_may_materialize_pod_local(ctx, right, target_id)
        }
        Expr::Unary { operand, .. } | Expr::TypeOf(operand) | Expr::Void(operand) => {
            expr_may_materialize_pod_local(ctx, operand, target_id)
        }
        Expr::Conditional {
            condition,
            then_expr,
            else_expr,
        } => {
            expr_may_materialize_pod_local(ctx, condition, target_id)
                || expr_may_materialize_pod_local(ctx, then_expr, target_id)
                || expr_may_materialize_pod_local(ctx, else_expr, target_id)
        }
        Expr::New { args, .. } | Expr::NewDynamic { args, .. } => args
            .iter()
            .any(|arg| expr_may_materialize_pod_local(ctx, arg, target_id)),
        Expr::Array(items) => items
            .iter()
            .any(|item| expr_may_materialize_pod_local(ctx, item, target_id)),
        Expr::Object(items) => items
            .iter()
            .any(|(_, item)| expr_may_materialize_pod_local(ctx, item, target_id)),
        _ => false,
    }
}

pub(crate) fn add_operands_have_pod_materialization_hazard(
    ctx: &FnCtx<'_>,
    left: &Expr,
    right: &Expr,
) -> bool {
    let mut right_pod_reads = Vec::new();
    collect_pod_numeric_field_read_locals(ctx, right, &mut right_pod_reads);
    right_pod_reads
        .into_iter()
        .any(|id| expr_may_materialize_pod_local(ctx, left, id))
}

fn static_object_property_type(ctx: &FnCtx<'_>, object: &Expr, field: &str) -> Option<HirType> {
    match static_type_of(ctx, object)? {
        HirType::Object(object_ty) => object_ty
            .properties
            .get(field)
            .map(|property| property.ty.clone()),
        _ => None,
    }
}

fn scalar_replaced_field_static_type(
    ctx: &FnCtx<'_>,
    object: &Expr,
    field: &str,
) -> Option<HirType> {
    match object {
        Expr::LocalGet(id)
            if ctx
                .scalar_replaced
                .get(id)
                .is_some_and(|fields| fields.contains_key(field)) =>
        {
            declared_field_type(ctx, object, field)
                .or_else(|| static_object_property_type(ctx, object, field))
        }
        Expr::This => {
            let target_id = ctx.scalar_ctor_target.last()?;
            if !ctx
                .scalar_replaced
                .get(target_id)
                .is_some_and(|fields| fields.contains_key(field))
            {
                return None;
            }
            ctx.class_stack
                .last()
                .and_then(|class_name| class_field_declared_type(ctx, class_name, field))
        }
        _ => None,
    }
}

pub(crate) fn scalar_replaced_field_is_raw_f64(
    ctx: &FnCtx<'_>,
    object: &Expr,
    field: &str,
) -> bool {
    scalar_replaced_field_static_type(ctx, object, field)
        .as_ref()
        .is_some_and(crate::typed_shape::type_is_raw_f64_candidate)
}

pub(crate) fn scalar_replaced_field_raw_f64_store_state(
    ctx: &FnCtx<'_>,
    local_id: Option<u32>,
    field: &str,
    declared_raw_f64: bool,
) -> bool {
    if !declared_raw_f64 {
        return false;
    }

    let field_note = format!("field={}", field);
    let mut proven_raw = false;
    for record in &ctx.native_rep_records {
        if record.local_id != local_id || !record.notes.iter().any(|note| note == &field_note) {
            continue;
        }
        match record.consumer.as_str() {
            "scalar_object_field_store.raw_f64" => {
                proven_raw = true;
            }
            "scalar_object_field_store"
                if record.notes.iter().any(|note| note == "raw_f64_field=1") =>
            {
                proven_raw = false;
            }
            _ => {}
        }
    }
    proven_raw
}

fn constant_array_index(index: &Expr) -> Option<usize> {
    match index {
        Expr::Integer(k) if *k >= 0 => Some(*k as usize),
        Expr::Number(f) if f.is_finite() && *f >= 0.0 && f.fract() == 0.0 => Some(*f as usize),
        _ => None,
    }
}

pub(crate) fn scalar_replaced_array_element_is_raw_f64(
    ctx: &FnCtx<'_>,
    object: &Expr,
    index: &Expr,
) -> bool {
    let Expr::LocalGet(id) = object else {
        return false;
    };
    let Some(k) = constant_array_index(index) else {
        return false;
    };
    if !ctx
        .scalar_replaced_arrays
        .get(id)
        .is_some_and(|slots| k < slots.len())
    {
        return false;
    }
    match static_type_of(ctx, object) {
        Some(HirType::Array(elem)) => crate::typed_shape::type_is_raw_f64_candidate(elem.as_ref()),
        Some(HirType::Tuple(elems)) => elems
            .get(k)
            .is_some_and(crate::typed_shape::type_is_raw_f64_candidate),
        _ => false,
    }
}

fn type_has_numeric_pointer_free_array_layout_for_fallback(ty: &HirType) -> bool {
    match ty {
        HirType::Array(elem) => matches!(elem.as_ref(), HirType::Number | HirType::Int32),
        HirType::Tuple(elems) => elems
            .iter()
            .all(|elem| matches!(elem, HirType::Number | HirType::Int32)),
        HirType::Union(variants) => variants.iter().all(|variant| {
            matches!(variant, HirType::Null | HirType::Void | HirType::Never)
                || type_has_numeric_pointer_free_array_layout_for_fallback(variant)
        }),
        _ => false,
    }
}

pub(crate) fn expr_may_return_boxed_value_from_raw_f64_fallback(
    ctx: &FnCtx<'_>,
    expr: &Expr,
) -> bool {
    match expr {
        Expr::PropertyGet { object, property } => receiver_class_name(ctx, object)
            .and_then(|class_name| class_field_declared_type(ctx, &class_name, property))
            .as_ref()
            .is_some_and(crate::typed_shape::type_is_raw_f64_candidate),
        Expr::IndexGet { object, .. } => static_type_of(ctx, object)
            .as_ref()
            .is_some_and(type_has_numeric_pointer_free_array_layout_for_fallback),
        _ => false,
    }
}

fn is_fixed_width_buffer_numeric_read(method: &str) -> bool {
    matches!(
        method,
        "readUInt8"
            | "readUint8"
            | "readInt8"
            | "readUInt16BE"
            | "readUint16BE"
            | "readUInt16LE"
            | "readUint16LE"
            | "readInt16BE"
            | "readInt16LE"
            | "readUInt32BE"
            | "readUint32BE"
            | "readUInt32LE"
            | "readUint32LE"
            | "readInt32BE"
            | "readInt32LE"
            | "readFloatBE"
            | "readFloatLE"
            | "readDoubleBE"
            | "readDoubleLE"
    )
}

pub(crate) fn is_numeric_expr(ctx: &FnCtx<'_>, e: &Expr) -> bool {
    match e {
        Expr::Integer(_)
        | Expr::Number(_)
        | Expr::PodLayoutSizeOf { .. }
        | Expr::PodLayoutAlignOf { .. }
        | Expr::PodLayoutOffsetOf { .. } => true,
        Expr::Uint8ArrayGet { .. }
        | Expr::BufferIndexGet { .. }
        | Expr::Uint8ArrayLength(_)
        | Expr::BufferLength(_) => true,
        Expr::LocalGet(id) => matches!(
            ctx.local_types.get(id),
            Some(HirType::Number) | Some(HirType::Int32)
        ),
        // NOTE: Expr::Compare is NOT numeric — it produces a NaN-boxed
        // TAG_TRUE/TAG_FALSE which `fcmp one cond, 0.0` would handle
        // incorrectly (NaN compared with 0.0 is unordered → false).
        // Comparisons go through the slow path (js_is_truthy) which
        // dispatches on the NaN tag.
        //
        // For Add: only numeric when BOTH operands are statically
        // numeric (otherwise it could be string concatenation). The
        // recursive check is critical for nested arithmetic like
        // `sum + p.x + p.y` which parses as `((sum + p.x) + p.y)` —
        // the inner Add must be recognized as numeric for the outer
        // Add to also be numeric, otherwise the outer one wraps the
        // inner result in `js_number_coerce` and prevents LLVM from
        // doing GVN/LICM on the chain.
        Expr::Binary {
            op: BinaryOp::Add,
            left,
            right,
        } => is_numeric_expr(ctx, left) && is_numeric_expr(ctx, right),
        Expr::Binary { op, .. } => !matches!(op, BinaryOp::Add),
        Expr::Update { .. } => true,
        Expr::DateNow => true,
        // `obj.field` where the field is declared as `number` on the
        // owning class. Without this, `this.value + 1` in a hot loop
        // wraps the field load in `js_number_coerce` which prevents
        // LLVM from doing GVN/LICM on the load. The class field
        // walker matches `class_field_global_index`'s inheritance
        // traversal so the type of any inherited field is also seen.
        Expr::PropertyGet { object, property } => {
            if property == "length" && expression_has_numeric_length(ctx, object) {
                return true;
            }
            if let Expr::LocalGet(id) = object.as_ref() {
                if ctx
                    .scalar_replaced
                    .get(id)
                    .is_some_and(|fields| fields.contains_key(property))
                {
                    let declared_raw_f64 = scalar_replaced_field_is_raw_f64(ctx, object, property);
                    return scalar_replaced_field_raw_f64_store_state(
                        ctx,
                        Some(*id),
                        property,
                        declared_raw_f64,
                    );
                }
            }
            if matches!(object.as_ref(), Expr::This) {
                if let Some(target_id) = ctx.scalar_ctor_target.last().copied() {
                    if ctx
                        .scalar_replaced
                        .get(&target_id)
                        .is_some_and(|fields| fields.contains_key(property))
                    {
                        let declared_raw_f64 =
                            scalar_replaced_field_is_raw_f64(ctx, object, property);
                        return scalar_replaced_field_raw_f64_store_state(
                            ctx,
                            Some(target_id),
                            property,
                            declared_raw_f64,
                        );
                    }
                }
            }
            if pod_record_field_is_numeric(ctx, object, property) {
                return true;
            }
            let Some(owner_class_name) = receiver_class_name(ctx, object) else {
                return false;
            };
            let mut current = ctx.classes.get(owner_class_name.as_str()).copied();
            while let Some(cls) = current {
                if let Some(f) = cls.fields.iter().find(|f| f.name == *property) {
                    return matches!(f.ty, HirType::Number | HirType::Int32);
                }
                current = cls
                    .extends_name
                    .as_deref()
                    .and_then(|p| ctx.classes.get(p).copied());
            }
            false
        }
        // `arr[i]` where `arr` is statically `number[]` / `Int32[]`.
        // Without this, `sum + arr[i]` in a hot loop wraps the element
        // load in `js_number_coerce` which blocks LLVM's vectorizer
        // and adds a function call per iteration.
        Expr::IndexGet { object, .. } => {
            if receiver_class_name(ctx, object)
                .as_deref()
                .is_some_and(is_numeric_typed_array_class)
            {
                return true;
            }
            let Expr::LocalGet(arr_id) = object.as_ref() else {
                return false;
            };
            match ctx.local_types.get(arr_id) {
                Some(HirType::Array(elem)) => {
                    matches!(**elem, HirType::Number | HirType::Int32)
                }
                Some(HirType::Named(name)) => is_numeric_typed_array_class(name),
                _ => false,
            }
        }
        // User function calls returning Number: skip js_number_coerce.
        // Without this, `fib(n-1) + fib(n-2)` wraps both results in
        // js_number_coerce — ~4 billion wasted runtime calls on fib(40).
        Expr::Call { callee, .. } => {
            if let Expr::PropertyGet { object, property } = callee.as_ref() {
                if is_fixed_width_buffer_numeric_read(property)
                    && receiver_class_name(ctx, object)
                        .as_deref()
                        .is_some_and(|name| matches!(name, "Buffer" | "Uint8Array"))
                {
                    return true;
                }
            }
            if let Expr::FuncRef(fid) = callee.as_ref() {
                ctx.func_signatures
                    .get(fid)
                    .map(|(_, _, returns_number, _)| *returns_number)
                    .unwrap_or(false)
            } else {
                false
            }
        }
        _ => false,
    }
}

/// Statically determine whether an expression is provably an integer-valued
/// number — i.e., its result has no fractional part. Stricter than
/// `is_numeric_expr`, which accepts any numeric f64.
///
/// Used by `BinaryOp::Mod` lowering to decide whether to emit integer
/// modulo (`fptosi → srem → sitofp`) instead of `frem double`. A wrong
/// `true` here would truncate fraction bits from the operand and produce
/// an incorrect result — so we only return true when the HIR structure
/// proves the value is a whole number.
///
/// Recognizes:
/// - `Expr::Integer(_)` — integer literal
/// - `Expr::LocalGet(id)` for locals pre-analyzed as integer-valued by
///   `collectors::collect_integer_locals` (for-loop counters etc.)
/// - `Expr::Update { .. }` — `i++`/`i--`, whose value is always integer
///   if the underlying local is integer-valued
/// - `Expr::Binary { Add/Sub/Mul/Mod }` recursively when both operands are
///   integer-valued (closed under integer arithmetic; Div is excluded
///   because `1 / 2` is 0.5 in JS, not 0)
/// - bitwise ops: always integer by JS ToInt32 semantics
pub(crate) fn is_integer_valued_expr(ctx: &FnCtx<'_>, e: &Expr) -> bool {
    match e {
        Expr::Integer(_) => true,
        Expr::Uint8ArrayGet { .. } | Expr::BufferIndexGet { .. } => true,
        Expr::LocalGet(id) => ctx.integer_locals.contains(id),
        Expr::Update { id, .. } => ctx.integer_locals.contains(id),
        Expr::Binary { op, left, right } => match op {
            BinaryOp::Add | BinaryOp::Sub | BinaryOp::Mul | BinaryOp::Mod => {
                is_integer_valued_expr(ctx, left) && is_integer_valued_expr(ctx, right)
            }
            BinaryOp::BitAnd
            | BinaryOp::BitOr
            | BinaryOp::BitXor
            | BinaryOp::Shl
            | BinaryOp::Shr
            | BinaryOp::UShr => true,
            _ => false,
        },
        _ => false,
    }
}

/// Statically determine whether an expression is a string. Conservative —
/// returns `false` for anything that requires type information we don't
/// track (function-call returns, dynamic property access).
///
/// Recognizes:
/// - literal strings (`"foo"`)
/// - LocalGet of string-typed locals (params with `: string`, `let x = "a"`)
/// - recursive Add of strings (`"a" + "b" + s`)
pub(crate) fn is_bool_expr(ctx: &FnCtx<'_>, e: &Expr) -> bool {
    match e {
        Expr::Bool(_) => true,
        Expr::Compare { .. } => true,
        Expr::Logical { left, right, .. } => is_bool_expr(ctx, left) && is_bool_expr(ctx, right),
        Expr::Unary {
            op: UnaryOp::Not, ..
        } => true,
        Expr::BooleanCoerce(_) => true,
        Expr::IsFinite(_)
        | Expr::IsNaN(_)
        | Expr::NumberIsNaN(_)
        | Expr::NumberIsFinite(_)
        | Expr::NumberIsInteger(_)
        | Expr::IsUndefinedOrBareNan(_) => true,
        Expr::SetHas { .. }
        | Expr::SetDelete { .. }
        | Expr::MapHas { .. }
        | Expr::MapDelete { .. } => true,
        Expr::ArrayIncludes { .. } => true,
        Expr::LocalGet(id) => matches!(ctx.local_types.get(id), Some(HirType::Boolean)),
        _ => false,
    }
}

pub(crate) fn is_set_expr(ctx: &FnCtx<'_>, e: &Expr) -> bool {
    match e {
        Expr::SetNew | Expr::SetNewFromArray(_) => true,
        Expr::LocalGet(id) => matches!(
            ctx.local_types.get(id),
            Some(HirType::Generic { base, .. }) if base == "Set"
        ),
        // `this.field` where the field is declared as `Set<T>` on the
        // enclosing class. Same rationale as is_map_expr.
        Expr::PropertyGet { object, property } => {
            if let Some(cls_name) = receiver_class_name(ctx, object) {
                if let Some(cls) = ctx.classes.get(&cls_name) {
                    if let Some(field) = cls.fields.iter().find(|f| f.name == *property) {
                        return matches!(
                            field.ty,
                            HirType::Generic { ref base, .. } if base == "Set"
                        );
                    }
                }
            }
            false
        }
        _ => false,
    }
}

/// Issue #650: detect URLSearchParams receivers for `sp.size` property
/// access. URLSearchParams is allocated as a generic ObjectHeader; the
/// type system tracks it as `HirType::Named("URLSearchParams")`. Used by
/// the codegen `Expr::PropertyGet { property: "size" }` arm to route
/// through `js_url_search_params_size` instead of returning undefined.
pub(crate) fn is_url_search_params_expr(ctx: &FnCtx<'_>, e: &Expr) -> bool {
    match e {
        Expr::UrlSearchParamsNew(_) => true,
        Expr::LocalGet(id) => matches!(
            ctx.local_types.get(id),
            Some(HirType::Named(name)) if name == "URLSearchParams"
        ),
        Expr::UrlGetSearchParams(_) => true,
        // `urlInstance.searchParams` — the HIR keeps this as a generic
        // PropertyGet (the URL HIR variant only fires for direct typed
        // receivers in `lower_member`). Detect the chained access here
        // so `url.searchParams.size` works without an intermediate let.
        Expr::PropertyGet { object, property } if property == "searchParams" => {
            if let Expr::LocalGet(id) = object.as_ref() {
                return matches!(
                    ctx.local_types.get(id),
                    Some(HirType::Named(name)) if name == "URL"
                );
            }
            matches!(object.as_ref(), Expr::UrlNew { .. })
        }
        _ => false,
    }
}

pub(crate) fn is_map_expr(ctx: &FnCtx<'_>, e: &Expr) -> bool {
    match e {
        Expr::MapNew | Expr::MapNewFromArray(_) => true,
        Expr::LocalGet(id) => matches!(
            ctx.local_types.get(id),
            Some(HirType::Generic { base, .. }) if base == "Map"
        ),
        // `this.field` where the field is declared as `Map<K, V>` on
        // the enclosing class. Needed so `this.handlers.set(...)` /
        // `this.handlers.get(...)` inside class methods dispatch
        // through the Map fast path instead of the dynamic field-set
        // fallback.
        Expr::PropertyGet { object, property } => {
            if let Some(cls_name) = receiver_class_name(ctx, object) {
                if let Some(cls) = ctx.classes.get(&cls_name) {
                    if let Some(field) = cls.fields.iter().find(|f| f.name == *property) {
                        return matches!(
                            field.ty,
                            HirType::Generic { ref base, .. } if base == "Map"
                        );
                    }
                }
            }
            false
        }
        _ => false,
    }
}

/// Stricter variant of `is_string_expr` that requires the type to be
/// definitely `String` — unions are NOT treated as strings. Used in the
/// string-concat fast path where dispatching through the string-only
/// codegen on a non-string union value produces garbage (e.g. masking an
/// f64 number's bits with POINTER_MASK yields a null pointer).
///
/// For JS `+` semantics on a union of string and number, the correct
/// behavior depends on the runtime value: `1 + "foo"` concatenates,
/// `1 + 42` adds. The generic numeric-add path (with `js_number_coerce`
/// fallback) handles narrowed-numeric cases correctly and is safer than
/// the string path when the value might actually be a number.
pub(crate) fn is_definitely_string_expr(ctx: &FnCtx<'_>, e: &Expr) -> bool {
    match e {
        Expr::String(_) | Expr::WtfString(_) => true,
        Expr::LocalGet(id) => {
            matches!(ctx.local_types.get(id), Some(HirType::String))
        }
        Expr::PathToNamespacedPath(path) => is_definitely_string_expr(ctx, path),
        Expr::PathWin32 {
            method: perry_hir::PathWin32Method::ToNamespacedPath,
            args,
        } => args
            .first()
            .map_or(false, |arg| is_definitely_string_expr(ctx, arg)),
        Expr::StringCoerce(_)
        | Expr::TypeOf(_)
        | Expr::ArrayJoin { .. }
        | Expr::JsonStringify(_)
        | Expr::JsonStringifyPretty { .. }
        | Expr::JsonStringifyFull(..)
        | Expr::StringFromCodePoint(_)
        | Expr::StringFromCharCode(_)
        | Expr::StringFromCharCodeSpread(_)
        | Expr::StringRaw { .. }
        | Expr::FsReadFileSync(_)
        | Expr::FsReadFileBinary(_)
        | Expr::PathSep
        | Expr::PathDelimiter
        | Expr::PathJoin(..)
        | Expr::PathDirname(_)
        | Expr::PathBasename(_)
        | Expr::PathExtname(_)
        | Expr::PathResolve(_)
        | Expr::PathNormalize(_)
        | Expr::PathResolveJoin(..)
        | Expr::PathWin32Join(..)
        | Expr::PathWin32 {
            method:
                perry_hir::PathWin32Method::Dirname
                | perry_hir::PathWin32Method::Basename
                | perry_hir::PathWin32Method::BasenameExt
                | perry_hir::PathWin32Method::Extname
                | perry_hir::PathWin32Method::Normalize
                | perry_hir::PathWin32Method::Format
                | perry_hir::PathWin32Method::Relative
                | perry_hir::PathWin32Method::Resolve
                | perry_hir::PathWin32Method::ResolveJoin,
            ..
        }
        | Expr::ProcessVersion
        | Expr::ProcessCwd
        | Expr::ProcessTitle
        | Expr::OsArch
        | Expr::OsType
        | Expr::OsPlatform
        | Expr::OsRelease
        | Expr::OsHostname
        | Expr::OsEOL
        | Expr::OsDevNull
        | Expr::OsEndianness
        | Expr::OsMachine
        | Expr::OsVersion => true,
        // `.toString()` always returns a string regardless of receiver
        // type, so it's safe to count as definitely-string for concat.
        // Same for other unary string-returning string methods.
        Expr::Call { callee, .. }
            if matches!(
                callee.as_ref(),
                Expr::PropertyGet { property, .. } if matches!(
                    property.as_str(),
                    "toString" | "toLowerCase" | "toUpperCase" | "trim"
                        | "trimStart" | "trimEnd" | "slice" | "substring"
                        | "substr" | "charAt" | "repeat" | "replace"
                        | "replaceAll" | "padStart" | "padEnd" | "concat"
                        | "normalize" | "toFixed" | "toPrecision" | "toExponential"
                )
            ) =>
        {
            true
        }
        Expr::Binary {
            op: BinaryOp::Add,
            left,
            right,
        } => is_definitely_string_expr(ctx, left) || is_definitely_string_expr(ctx, right),
        // Ternary `cond ? a : b` is definitely a string when BOTH
        // branches are definitely strings. Without this, code like
        //   (d ? "D" : "") + (v ? "V" : "")
        // misses the string-concat fast path because each ternary is
        // typed as Any, the `+` falls through to numeric Add, both
        // operands get js_number_coerce'd (string → NaN), and the
        // result prints as "NaN" instead of the concatenation.
        Expr::Conditional {
            then_expr,
            else_expr,
            ..
        } => is_definitely_string_expr(ctx, then_expr) && is_definitely_string_expr(ctx, else_expr),
        Expr::PropertyGet { object, property }
            if is_process_namespace_version_property(object, property) =>
        {
            true
        }
        _ => false,
    }
}

/// Resolve the declared type of `<object>.<field>` when `object` is a
/// known user class or interface that declares (or inherits) a field
/// named `field`. Returns `None` when the receiver isn't a tracked
/// class/interface, or when no such field is declared on it.
///
/// Used to keep name-only field heuristics (the Error `.message` /
/// `.stack` / `.name` string assumption) from hijacking a user class
/// whose own field happens to share that name with a non-string type
/// (e.g. `effect`'s `RedBlackTreeIterator.stack: Array<...>` — #321).
pub(crate) fn is_string_expr(ctx: &FnCtx<'_>, e: &Expr) -> bool {
    match e {
        Expr::String(_) | Expr::WtfString(_) => true,
        Expr::LocalGet(id) => {
            match ctx.local_types.get(id) {
                Some(HirType::String | HirType::StringLiteral(_)) => true,
                // Union(String, Null/Void) — nullable strings are still
                // strings at runtime when non-null. The ?. and != null
                // guard paths lower the non-null case through the string
                // method dispatch. Without this, `(s: string | null).
                // toUpperCase()` fell through to the generic path and
                // returned undefined.
                Some(HirType::Union(members)) => {
                    members
                        .iter()
                        .any(|m| matches!(m, HirType::String | HirType::StringLiteral(_)))
                }
                _ => false,
            }
        }
        // arr[i] where arr is Array<string> → element is a string.
        // Lets `this.parts[i].length` use the string fast path inline
        // without needing an intermediate let binding. Also str[i] on
        // a string-typed receiver returns a single-character string,
        // so the tokenizer pattern `input[pos] >= "0"` routes through
        // string comparison.
        Expr::IndexGet { object, .. } => {
            match static_type_of(ctx, object) {
                Some(HirType::Array(elem)) if matches!(*elem, HirType::String) => true,
                Some(HirType::String) => true,
                _ => false,
            }
        }
        // Enum string members lower to string literals at the use
        // site, so a comparison like `c === Color.Red` should fire
        // the string equality fast path.
        Expr::EnumMember { enum_name, member_name } => {
            matches!(
                ctx.enums.get(&(enum_name.clone(), member_name.clone())),
                Some(perry_hir::EnumValue::String(_))
            )
        }
        Expr::Binary { op: BinaryOp::Add, left, right } => {
            is_string_expr(ctx, left) || is_string_expr(ctx, right)
        }
        Expr::PathToNamespacedPath(path) => is_definitely_string_expr(ctx, path),
        Expr::PathWin32 {
            method: perry_hir::PathWin32Method::ToNamespacedPath,
            args,
        } => args
            .first()
            .map_or(false, |arg| is_definitely_string_expr(ctx, arg)),
        // String coerce, JSON.stringify, ArrayJoin, etc. all return
        // strings.
        Expr::StringCoerce(_)
        | Expr::TypeOf(_)
        | Expr::ArrayJoin { .. }
        | Expr::JsonStringifyFull(..)
        | Expr::FsReadFileSync(_)
        | Expr::FsReadFileBinary(_)
        | Expr::PathJoin(..)
        | Expr::PathDirname(_)
        | Expr::PathBasename(_)
        | Expr::PathExtname(_)
        | Expr::PathResolve(_)
        | Expr::PathNormalize(_)
        | Expr::PathResolveJoin(..)
        | Expr::PathWin32Join(..)
        | Expr::PathWin32 {
            method:
                perry_hir::PathWin32Method::Dirname
                | perry_hir::PathWin32Method::Basename
                | perry_hir::PathWin32Method::BasenameExt
                | perry_hir::PathWin32Method::Extname
                | perry_hir::PathWin32Method::Normalize
                | perry_hir::PathWin32Method::Format
                | perry_hir::PathWin32Method::Relative
                | perry_hir::PathWin32Method::Resolve
                | perry_hir::PathWin32Method::ResolveJoin,
            ..
        } => true,
        // String.fromCodePoint(...) / String.fromCharCode(...) / str.at(i)
        // / RegExp.source|flags — all produce string handles.
        Expr::StringFromCodePoint(_)
        | Expr::StringFromCharCode(_)
        | Expr::StringFromCharCodeSpread(_)
        | Expr::StringRaw { .. }
        | Expr::StringAt { .. }
        | Expr::RegExpSource(_)
        | Expr::RegExpFlags(_)
        // Date.prototype.to*String() → string
        | Expr::DateToString(_)
        | Expr::DateToDateString(_)
        | Expr::DateToTimeString(_)
        | Expr::DateToUTCString(_)
        | Expr::DateToLocaleString(_)
        | Expr::DateToLocaleDateString(_)
        | Expr::DateToLocaleTimeString(_)
        | Expr::DateToISOString(_)
        | Expr::DateToJSON(_)
        // node:path constants
        | Expr::PathSep
        | Expr::PathDelimiter
        // JSON.stringify returns a string. #853: `JsonStringifyFull(..)`
        // is already enumerated in the earlier (line ~878) arm — listing
        // it again here was dead.
        | Expr::JsonStringify(_)
        | Expr::JsonStringifyPretty { .. } => true,
        // process.* / os.* string-returning accessors. These lower to runtime
        // calls that return raw StringHeader* pointers, NaN-boxed with STRING_TAG
        // in expr.rs. Without this, `process.version.startsWith('v')` falls
        // through to the generic native method dispatch and returns undefined.
        Expr::ProcessVersion
        | Expr::ProcessCwd
        | Expr::ProcessTitle
        | Expr::OsArch
        | Expr::OsType
        | Expr::OsPlatform
        | Expr::OsRelease
        | Expr::OsHostname
        | Expr::OsEOL
        | Expr::OsDevNull
        | Expr::OsEndianness
        | Expr::OsMachine
        | Expr::OsVersion => true,
        // `obj.toString()` always returns a string. Same for the
        // string-returning method family (trim, trimStart, trimEnd,
        // toLowerCase, toUpperCase, slice, substring, charAt, repeat,
        // replace, replaceAll, split's first elem, etc. — limited to
        // unary methods on a string receiver). Recognize these so
        // chained calls like `s.trimStart().trimEnd()` detect the
        // inner result as a string.
        Expr::Call { callee, .. }
            if matches!(
                callee.as_ref(),
                Expr::PropertyGet { property, object } if matches!(
                    property.as_str(),
                    "toString" | "toLowerCase" | "toUpperCase" | "trim"
                        | "trimStart" | "trimEnd" | "slice" | "substring"
                        | "substr" | "charAt" | "repeat" | "replace"
                        | "replaceAll" | "padStart" | "padEnd" | "concat"
                        | "normalize" | "at" | "toWellFormed"
                ) && (
                    is_string_expr(ctx, object)
                        || matches!(property.as_str(), "toString")
                )
            ) =>
        {
            true
        }
        // Error instance field access — e.message / e.stack / e.name
        // all route through the runtime's GC_TYPE_ERROR dispatch and
        // return string pointers. Recognize them so chained calls like
        // `e.stack!.includes("...")` hit the string method fast path.
        //
        // BUT this name-only heuristic must NOT hijack a user class /
        // interface whose own field happens to be called `stack` /
        // `name` / `message` with a non-string declared type. The
        // RedBlackTreeIterator in `effect` has `readonly stack:
        // Array<Node<K,V>>`; without this guard `this.stack[i]` was
        // mis-lowered as a string `char_at` (garbage element reads →
        // null SortedSet iteration, #321). When the receiver resolves
        // to a concrete declared field type, defer to it; only fall
        // back to the Error-string assumption when the receiver's type
        // is genuinely unknown (a real caught `Error`/`unknown`/`any`).
        Expr::PropertyGet { object, property }
            // `.stack` excluded — may be an array via `Error.prepareStackTrace`.
            if matches!(property.as_str(), "message" | "name") =>
        {
            // If the receiver is a known user class / interface that
            // *declares* a field with this name, that field's declared
            // type wins over the name-only Error heuristic.
            if let Some(declared) = declared_field_type(ctx, object, property) {
                return matches!(declared, HirType::String);
            }
            // Otherwise it's an Error-shaped property (caught `e`,
            // `unknown`/`any`, or an untracked receiver) → string.
            true
        }
        // Namespace `node:process` exports share the same runtime process
        // surface as bare `process`. Keep the string method dispatch
        // available for namespace imports:
        // `import * as process from "node:process"; process.version.startsWith("v")`.
        Expr::PropertyGet { object, property }
            if is_process_namespace_version_property(object, property) =>
        {
            true
        }
        // Perry's native crypto.generateKeyPairSync returns a plain object
        // with PEM string fields. Refining these fields keeps
        // `pair.publicKey.includes(...)` on the string fast path.
        Expr::PropertyGet { object, property }
            if matches!(property.as_str(), "publicKey" | "privateKey")
                && matches!(
                    static_type_of(ctx, object),
                    Some(HirType::Named(ref name)) if name == "CryptoKeyPair"
                ) =>
        {
            true
        }
        // PropertyGet on a known class field with declared type String.
        Expr::PropertyGet { object, property } => {
            let Some(class_name) = receiver_class_name(ctx, object) else {
                return false;
            };
            let Some(class) = ctx.classes.get(&class_name) else {
                return false;
            };
            class
                .fields
                .iter()
                .find(|f| f.name == *property)
                .map(|f| matches!(f.ty, HirType::String))
                .unwrap_or(false)
        }
        // `crypto.createHash(alg).update(data).digest(enc)` chain — only
        // when an encoding is given. Recognized so chained `.length` /
        // `.includes` / `===` on the resulting hex/base64 string hit the
        // string fast paths. The no-arg `digest()` returns a Buffer, not a
        // string, so it must NOT be classified here — otherwise
        // `digest().toString('hex')` skips the buffer encoding path and
        // mis-reads the bytes as Latin-1 (#1353).
        Expr::Call { callee, args, .. }
            if is_crypto_digest_chain(callee)
                && matches!(args.first(), Some(a) if !matches!(a, Expr::Undefined)) =>
        {
            true
        }
        // atob/btoa always return strings.
        Expr::Atob(_) | Expr::Btoa(_) => true,
        _ => false,
    }
}

/// Statically determine whether an expression evaluates to a Promise.
/// #1008: does `expr` refer to a built-in global (e.g. `Promise`,
/// `Array`)? Recognises both shapes that the HIR lowers bare global
/// idents into:
///
/// - Legacy: `Expr::GlobalGet(_)` directly. Pre-#973 codepath.
/// - Post-#973: `Expr::PropertyGet { object: GlobalGet(0), property:
///   <name> }`. After PR #973, bare built-in idents lower as a
///   property access on `globalThis` so they route through the
///   globalThis singleton closure path. Old call sites that only
///   matched the legacy shape silently lost specialization.
///
/// Pass `name = "Promise"` (etc.) to require the property-access form
/// to actually name that built-in; the legacy `GlobalGet(_)` arm
/// accepts any global because the original code never narrowed.
// `dead_code` allow: the function survived an unresolved merge in
// main (commit 9a9a233c's "fix: recognize global Promise static
// calls" left HEAD/incoming markers in this file). The
// `is_global_constructor_expr` helper added by the same commit
// supersedes this one, but ripping it out is outside #516's
// scope — leave the lingering definition with an allow so the
// dead-code lint doesn't fail the build.
#[allow(dead_code)]
pub(crate) fn is_global_builtin_named(expr: &Expr, name: &str) -> bool {
    if matches!(expr, Expr::GlobalGet(_)) {
        return true;
    }
    if let Expr::PropertyGet { object, property } = expr {
        if matches!(object.as_ref(), Expr::GlobalGet(_)) && property == name {
            return true;
        }
    }
    false
}

/// Used by `.then()` / `.catch()` / `.finally()` dispatch in lower_call
/// to intercept promise method calls and route them through the runtime
/// `js_promise_then` / `js_promise_catch` functions.
///
/// Recognizes:
/// - LocalGet of a `Promise(_)`-typed local
/// - `Promise.resolve(x)` / `Promise.reject(x)` / `Promise.all(x)` / etc.
///   (the GlobalGet + "resolve"/"reject"/"all"/"race"/"allSettled" pattern)
/// - Result of `.then(cb)` / `.catch(cb)` / `.finally(cb)` on a promise
///   (recursive: chains like `p.then(f).then(g)`)
/// - Async function calls (return type is Promise)
pub(crate) fn is_promise_expr(ctx: &FnCtx<'_>, e: &Expr) -> bool {
    match e {
        Expr::LocalGet(id) => match ctx.local_types.get(id) {
            Some(HirType::Promise(_)) => true,
            // `const p: Promise<T> = ...` is lowered as Generic { base: "Promise", ... }
            // by the HIR when the source annotation is `Promise<T>` rather than the
            // async-function return inference path (which produces HirType::Promise).
            Some(HirType::Generic { base, .. }) if base == "Promise" => true,
            _ => false,
        },
        // Promise.resolve / reject / all / race / allSettled / any
        Expr::Call { callee, .. } => match callee.as_ref() {
            Expr::PropertyGet { object, property } => {
                // `Promise.resolve(...)` etc. The receiver `Promise` can
                // appear in two shapes:
                //   - Legacy: bare ident → `Expr::GlobalGet(_)` directly.
                //   - Post-#973: bare built-in idents lower to
                //     `PropertyGet { GlobalGet(0), "Promise" }` so they
                //     route through the globalThis singleton closure
                //     path. Without the second arm, `is_promise_expr`
                //     returned false for `Promise.resolve()` and the
                //     `.then` codegen fell through to generic native
                //     dispatch — microtask-02..07 and edge-promises went
                //     silent (callbacks never enqueued). (#1008)
                //
                // Resolved-from-merge note: the HEAD side called
                // `is_global_builtin_named`, the incoming side called
                // `is_global_constructor_expr`. Post-#1030 the rest of
                // the codegen prefers the latter helper, so we keep the
                // richer HEAD comment but switch to the canonical call.
                if matches!(
                    property.as_str(),
                    "resolve" | "reject" | "all" | "race" | "allSettled" | "any"
                ) && is_global_builtin_named(object.as_ref(), "Promise")
                {
                    return true;
                }
                // `Array.fromAsync(...)` returns a Promise<Array>.
                if property == "fromAsync" && is_global_builtin_named(object.as_ref(), "Array") {
                    return true;
                }
                // `.then(cb)` / `.catch(cb)` / `.finally(cb)` on a promise
                // receiver — the result is itself a promise.
                if matches!(property.as_str(), "then" | "catch" | "finally")
                    && is_promise_expr(ctx, object)
                {
                    return true;
                }
                // Issue #489 followup: `obj.field(args)` where `field` is
                // typed as an async function or a function returning
                // `Promise<T>`. Drizzle's `mysql-proxy/session.js` calls
                // `this.client(...).then(({rows}) => rows)` where
                // `this.client` is a class field of type
                // `(sql, params, method) => Promise<{rows, …}>`. Without
                // this arm, perry's `.then` lowering doesn't recognize
                // the call result as a Promise and falls through to a
                // generic dispatch that silently drops the callback (the
                // await of `db.insert(...)` resolves to undefined / "").
                if let Some(HirType::Function(ft)) = static_type_of(ctx, callee.as_ref()) {
                    if ft.is_async {
                        return true;
                    }
                    if matches!(*ft.return_type, HirType::Promise(_)) {
                        return true;
                    }
                    if let HirType::Generic { ref base, .. } = *ft.return_type {
                        if base == "Promise" {
                            return true;
                        }
                    }
                }
                // Issue #489 followup: `obj.method(args)` where `method`
                // is a class instance method declared `async` or with a
                // return type of `Promise<T>`. Class methods live in
                // `class.methods` (not `class.fields`), so the
                // static_type_of branch above doesn't catch them. Walk
                // the parent chain for inherited async methods too —
                // drizzle's `MySqlInsertBase.execute` is a class-field
                // arrow defined on the subclass, but the override-vs-
                // inherited shape varies per query-builder, so handle
                // both. The fallback class_name comes from the receiver.
                if let Some(class_name) = receiver_class_name(ctx, object) {
                    let mut current = Some(class_name);
                    while let Some(cn) = current {
                        if let Some(class) = ctx.classes.get(&cn) {
                            if let Some(m) = class.methods.iter().find(|m| m.name == *property) {
                                if m.is_async {
                                    return true;
                                }
                                match &m.return_type {
                                    HirType::Promise(_) => return true,
                                    HirType::Generic { base, .. } if base == "Promise" => {
                                        return true
                                    }
                                    _ => {}
                                }
                            }
                            current = class.extends_name.clone();
                        } else {
                            break;
                        }
                    }
                }
                false
            }
            // Direct call to a locally-defined async function — its
            // return value is a `Promise<T>`. The HIR's
            // `Function::is_async` flag is collected into
            // `cross_module.local_async_funcs` at module compile time.
            Expr::FuncRef(fid) => ctx.local_async_funcs.contains(fid),
            // Issue #633 / #611 followup: call to a local LET-bound
            // async closure — `const fn = async (...) => ...; fn(...)`.
            // The let's type is `HirType::Function { is_async: true }`,
            // recorded in `local_types`. Without this arm, perry's
            // `.then()` lowering at `lower_call.rs:1188` doesn't
            // recognize `fn({}).then(cb)` as a Promise receiver and the
            // .then call falls through to a generic dispatch that
            // silently drops the callback.
            Expr::LocalGet(id) => match ctx.local_types.get(id) {
                Some(HirType::Function(ft)) if ft.is_async => true,
                Some(HirType::Function(ft)) => match ft.return_type.as_ref() {
                    HirType::Promise(_) => true,
                    HirType::Generic { base, .. } if base == "Promise" => true,
                    _ => false,
                },
                _ => false,
            },
            _ => false,
        },
        _ => false,
    }
}

/// If the expression is a known instance of a Named class type, return
/// the class name. Used by the class method dispatch in lower_call to
/// pick the right `perry_method_<class>_<name>` function.
pub(crate) fn receiver_class_name(ctx: &FnCtx<'_>, e: &Expr) -> Option<String> {
    match e {
        Expr::LocalGet(id) => match ctx.local_types.get(id)? {
            HirType::Named(name) => Some(name.clone()),
            // Generic instantiation `Box<number>` — strip the type
            // args and use the base class name. The codegen erases
            // type parameters anyway, so the dispatch is identical
            // to the non-generic Named form.
            HirType::Generic { base, .. } if ctx.classes.contains_key(base) => Some(base.clone()),
            _ => None,
        },
        // `new ClassName(...)` — the receiver class is the constructed class.
        // Lets `(new Config()).toString()` find Config's user toString.
        Expr::New { class_name, .. } => Some(class_name.clone()),
        // `ClassName.staticMethod(...)` chains often return an instance
        // of `ClassName` (factory pattern: `Color.red()`). Without type
        // info on the static method's return, assume it's the same class
        // so chained `.toString()` finds the user's toString.
        Expr::StaticMethodCall { class_name, .. } => Some(class_name.clone()),
        e if net_result_class(e).is_some() => net_result_class(e).map(str::to_string),
        // `this` inside a constructor or method body — the class name is
        // at the top of class_stack (for inlined constructors) or comes
        // from the enclosing method's owning class.
        Expr::This => ctx.class_stack.last().cloned(),
        // A private-access brand guard returns its receiver unchanged; see
        // through it so shadowed private-field slot resolution stays accurate.
        Expr::PrivateGuard { object, .. } => receiver_class_name(ctx, object),
        // `arr[i]` where `arr: ClassFoo[]` — the element type is the
        // array's parameter. Lets `items[2].display()` resolve the
        // method dispatch.
        Expr::IndexGet { object, .. } => {
            if let Expr::LocalGet(arr_id) = object.as_ref() {
                if let Some(HirType::Array(elem)) = ctx.local_types.get(arr_id) {
                    if let HirType::Named(name) = elem.as_ref() {
                        return Some(name.clone());
                    }
                }
            }
            None
        }
        // `this.field` or `obj.field` where the field's declared type
        // is a class. Walk the class definition to find the field's
        // type. Honors the parent inheritance chain.
        Expr::PropertyGet { object, property } => {
            let owner_class_name = receiver_class_name(ctx, object)?;
            let class = ctx.classes.get(&owner_class_name)?;
            // Look in own fields, then walk parent chain.
            let field_ty = class
                .fields
                .iter()
                .find(|f| f.name == *property)
                .map(|f| &f.ty)
                .or_else(|| {
                    let mut parent = class.extends_name.as_deref();
                    while let Some(p) = parent {
                        if let Some(pc) = ctx.classes.get(p) {
                            if let Some(f) = pc.fields.iter().find(|f| f.name == *property) {
                                return Some(&f.ty);
                            }
                            parent = pc.extends_name.as_deref();
                        } else {
                            break;
                        }
                    }
                    None
                })?;
            match field_ty {
                HirType::Named(name) => Some(name.clone()),
                _ => None,
            }
        }
        _ => None,
    }
}

/// Statically determine whether an expression is an array. Used for
/// dispatch on `arr.length` and `arr[i]`.
///
/// Recognizes:
/// - literal arrays `[a, b, c]` and `Expr::ArraySpread`
/// - LocalGet of an Array-typed local
/// - **PropertyGet on a class instance where the field is Array-typed**
///   (e.g. `this.items` when `Container.items: Item[]`)
/// - **NativeMethodCall results where the runtime returns an array**
///   (e.g. `arr.map(...)` — but those use the special Expr::ArrayMap
///   variant which is already handled)
pub(crate) fn is_array_expr(ctx: &FnCtx<'_>, e: &Expr) -> bool {
    match static_type_of(ctx, e) {
        Some(HirType::Array(_)) | Some(HirType::Tuple(_)) => true,
        Some(HirType::Generic { ref base, .. }) if base == "Array" => true,
        // #3148: %TypedArray% receivers route their not-already-folded methods
        // (fill / reverse / keys / values / entries / set / subarray) through
        // `lower_array_method`; the generic `js_array_*` helpers delegate to the
        // element-typed `js_typed_array_*` impls via `lookup_typed_array_kind`.
        // Uint8Array / Uint8ClampedArray are intentionally excluded — they are
        // buffer-backed and dispatched by `dispatch_buffer_method`.
        Some(HirType::Named(ref n))
            if matches!(
                n.as_str(),
                "Int8Array"
                    | "Int16Array"
                    | "Int32Array"
                    | "Uint16Array"
                    | "Uint32Array"
                    | "Float16Array"
                    | "Float32Array"
                    | "Float64Array"
                    | "BigInt64Array"
                    | "BigUint64Array"
            ) =>
        {
            true
        }
        // `T | null`, `T | undefined`, `T[] | null` — when an `if (x)`
        // guard narrows away the null/undefined, the truthy branch
        // still has the same union type in the HIR, so recognize
        // unions whose non-nullish variant is an array. Without this
        // `maybeArr.length` falls through to object-field access and
        // prints `undefined`.
        Some(HirType::Union(variants)) => variants
            .iter()
            .any(|v| matches!(v, HirType::Array(_) | HirType::Tuple(_))),
        _ => false,
    }
}

/// True when `e` is a *dynamic* index into a native-module namespace —
/// the auditable `ns[dynamicKey]` sub-namespace-selection shape (#1740,
/// e.g. `(path as any)[k]` resolving to `path.win32` / `path.posix`).
///
/// Such a receiver evaluates to a native-module sub-object at runtime,
/// never a primitive, so a method call on it must route through the
/// generic `js_native_call_method` dispatch (which reaches
/// `dispatch_native_module_method`) rather than being mis-classified as
/// a `String`/`Number` prototype method by its name alone. Without this,
/// a prototype-colliding name like `normalize` is lowered as a string
/// method and the namespace pointer is handed to a string FFI → SIGSEGV
/// (#1760).
///
/// Gated on a *non-literal* index: `(path as any)["sep"]` (a literal
/// string property) can legitimately resolve to a string and must keep
/// its string-method lowering, whereas `(path as any)[k]` is the dynamic
/// sub-namespace form this targets.
pub(crate) fn is_native_module_dynamic_index(e: &Expr) -> bool {
    matches!(
        e,
        Expr::IndexGet { object, index }
            if matches!(object.as_ref(), Expr::NativeModuleRef(_))
                && !matches!(index.as_ref(), Expr::String(_) | Expr::WtfString(_))
    )
}

/// Best-effort static type lookup for an expression. Returns the HIR
/// type when it's cheap to determine (literals, locals, field accesses
/// on known classes). Returns `None` when computing the type would
/// require a fuller type-checker pass.
/// Extract a non-negative integer literal index from an index expression, if it
/// is one. Used to type tuple element accesses only for in-bounds literal
/// indices (dynamic indices into a heterogeneous tuple aren't statically known).
fn tuple_index_literal(index: &Expr) -> Option<usize> {
    match index {
        Expr::Integer(n) if *n >= 0 => Some(*n as usize),
        Expr::Number(f) if *f >= 0.0 && f.fract() == 0.0 => Some(*f as usize),
        _ => None,
    }
}

pub(crate) fn static_type_of(ctx: &FnCtx<'_>, e: &Expr) -> Option<HirType> {
    match e {
        Expr::Array(_) => Some(HirType::Array(Box::new(HirType::Any))),
        Expr::String(_) | Expr::WtfString(_) => Some(HirType::String),
        Expr::Number(_) | Expr::Integer(_) => Some(HirType::Number),
        Expr::Bool(_) => Some(HirType::Boolean),
        Expr::LocalGet(id) => ctx.local_types.get(id).cloned(),
        Expr::StaticMethodCall {
            class_name,
            method_name,
            ..
        } => ctx
            .classes
            .get(class_name)
            .and_then(|class| {
                class
                    .static_methods
                    .iter()
                    .find(|method| method.name == *method_name)
            })
            .map(|method| method.return_type.clone()),
        e if net_result_type(e).is_some() => net_result_type(e),
        Expr::PropertyGet { object, property } => {
            if property == "length" && expression_has_numeric_length(ctx, object) {
                return Some(HirType::Number);
            }
            if pod_record_field_is_numeric(ctx, object, property) {
                return Some(HirType::Number);
            }
            if is_process_namespace_version_property(object, property) {
                return Some(HirType::String);
            }
            if matches!(property.as_str(), "publicKey" | "privateKey")
                && matches!(
                    static_type_of(ctx, object),
                    Some(HirType::Named(ref name)) if name == "CryptoKeyPair"
                )
            {
                return Some(HirType::String);
            }
            if let Some(static_method_ty) = crate::expr::try_static_class_name(object, ctx)
                .and_then(|class_name| ctx.classes.get(class_name))
                .and_then(|class| {
                    class
                        .static_methods
                        .iter()
                        .find(|method| method.name == *property)
                        .map(function_type_from_decl)
                })
            {
                return Some(static_method_ty);
            }
            if let Some(receiver_class) = receiver_class_name(ctx, object) {
                // If the object is a known class instance, look up the field
                // type from the class definition.
                if let Some(class) = ctx.classes.get(&receiver_class) {
                    if let Some(field_ty) = class
                        .fields
                        .iter()
                        .find(|f| f.name == *property)
                        .map(|f| f.ty.clone())
                        .or_else(|| {
                            // Walk up the inheritance chain.
                            let mut parent = class.extends_name.as_deref();
                            while let Some(p) = parent {
                                if let Some(pc) = ctx.classes.get(p) {
                                    if let Some(field) =
                                        pc.fields.iter().find(|f| f.name == *property)
                                    {
                                        return Some(field.ty.clone());
                                    }
                                    parent = pc.extends_name.as_deref();
                                } else {
                                    break;
                                }
                            }
                            None
                        })
                    {
                        return Some(field_ty);
                    }
                    if let Some(method_ty) = class
                        .methods
                        .iter()
                        .find(|method| method.name == *property)
                        .map(function_type_from_decl)
                    {
                        return Some(method_ty);
                    }
                }
                // Issue #655: receiver may be typed against a TS `interface`
                // rather than a class. The runtime layout is identical to a
                // plain object literal, so the property's declared type is
                // the right answer for the array fast-path / `length=` setter
                // path. Walks the `extends` chain too so chained interfaces
                // (`interface Sub extends Base { ... }`) resolve.
                if let Some(iface) = ctx.interfaces.get(&receiver_class) {
                    if let Some(p) = iface.properties.iter().find(|p| p.name == *property) {
                        return Some(p.ty.clone());
                    }
                    if let Some(method) =
                        iface.methods.iter().find(|method| method.name == *property)
                    {
                        return Some(HirType::Function(perry_types::FunctionType {
                            params: method.params.clone(),
                            return_type: Box::new(method.return_type.clone()),
                            is_async: false,
                            is_generator: false,
                        }));
                    }
                    for ext in &iface.extends {
                        if let HirType::Named(parent_name) = ext {
                            if let Some(parent_iface) = ctx.interfaces.get(parent_name) {
                                if let Some(p) =
                                    parent_iface.properties.iter().find(|p| p.name == *property)
                                {
                                    return Some(p.ty.clone());
                                }
                            }
                        }
                    }
                }
            }
            hir_inferred_static_type(ctx, e)
        }
        Expr::This => {
            let cls = ctx.class_stack.last()?.clone();
            Some(HirType::Named(cls))
        }
        // `str.split(delim)` returns Array<String>. Catches the generic
        // Call form that bypasses the `Expr::StringSplit` variant — e.g.
        // `"a,b,c".split(",")` in an expression position where we need
        // `.length` / `[i]` to follow the array fast path.
        // Also: `str.match(regex)` produces an array. `matchAll` deliberately
        // stays dynamic because it returns a RegExp String Iterator object.
        Expr::Call { callee, .. }
            if matches!(
                callee.as_ref(),
                Expr::PropertyGet { property, object } if matches!(
                    property.as_str(), "split" | "match"
                ) && is_string_expr(ctx, object)
            ) =>
        {
            Some(HirType::Array(Box::new(HirType::String)))
        }
        // `crypto.createHash(alg).update(d).digest()` with no encoding arg
        // returns a Buffer. Recognizing the inline chain (not just a bound
        // local) lets `...digest().toString('hex')` / `...digest()[i]` take
        // the buffer dispatch instead of the Latin-1 string path (#1353).
        Expr::Call { callee, args, .. }
            if args.first().map_or(true, |a| matches!(a, Expr::Undefined))
                && is_crypto_digest_chain(callee) =>
        {
            Some(HirType::Named("Uint8Array".into()))
        }
        // crypto.getHashes()/getCiphers()/getCurves() all return
        // Array<string>. Recognize this even in expression position so
        // chained `.includes(...)` uses Array SameValueZero instead of
        // falling through to dynamic/string dispatch.
        Expr::Call { callee, .. }
            if matches!(
                callee.as_ref(),
                Expr::PropertyGet { property, object }
                    if matches!(object.as_ref(), Expr::NativeModuleRef(m) if m == "crypto")
                        && matches!(property.as_str(), "getHashes" | "getCiphers" | "getCurves")
            ) =>
        {
            Some(HirType::Array(Box::new(HirType::String)))
        }
        Expr::Call { callee, .. } => {
            if let Some(HirType::Function(ft)) = static_type_of(ctx, callee.as_ref()) {
                return Some((*ft.return_type).clone());
            }
            hir_inferred_static_type(ctx, e)
        }
        // `arr[i]` where `arr: Array<T>` has static type `T`. This lets
        // nested access like `grid[i][j]` and `grid[i].length` reach
        // the array fast paths (via is_array_expr) when `grid` is
        // statically known to be `Array<Array<T>>` / `Array<Tuple<...>>`.
        // Also handles `Record<K, V>[key]` → V so `groups["a"].length`
        // on `Record<string, number[]>` finds the array fast path.
        Expr::IndexGet { object, index } => match static_type_of(ctx, object) {
            Some(HirType::Array(inner)) => Some(*inner),
            // A literal, in-bounds index has the exact element type. A dynamic
            // index could hit any element, so it's only sound when the tuple is
            // homogeneous — otherwise stay conservative (e.g. `[string, number]`
            // must not type `t[i]` as `string`).
            Some(HirType::Tuple(elems)) if !elems.is_empty() => match tuple_index_literal(index) {
                Some(i) => elems.get(i).cloned(),
                None => {
                    let first = &elems[0];
                    elems.iter().all(|t| t == first).then(|| first.clone())
                }
            },
            Some(HirType::Generic { base, type_args })
                if base == "Record" && type_args.len() == 2 =>
            {
                Some(type_args[1].clone())
            }
            _ => hir_inferred_static_type(ctx, e),
        },
        // `a || b` and `a ?? b` lower to `Expr::Logical`. Recognize the
        // result as Array-typed when EITHER branch is Array — `is_array_expr`
        // already accepts the Union form, so this lets `(maybeArr || []).slice()`
        // route through the array fast path instead of falling through to
        // `js_native_call_method`, which has no `slice` arm for arrays and
        // returns a sentinel that downstream `.sort(cmp)` deref's to null
        // (issue #291). `&&` likewise — its truthy result is the right
        // operand which is an array literal in the common idiom.
        Expr::Logical { left, right, .. } => {
            let lt = static_type_of(ctx, left);
            let rt = static_type_of(ctx, right);
            match (lt, rt) {
                (Some(a), Some(b)) if a == b => Some(a),
                (Some(a), Some(b)) => Some(HirType::Union(vec![a, b])),
                (Some(t), None) | (None, Some(t)) => Some(t),
                _ => None,
            }
        }
        // `cond ? a : b` — same logic as Logical.
        Expr::Conditional {
            then_expr,
            else_expr,
            ..
        } => {
            let lt = static_type_of(ctx, then_expr);
            let rt = static_type_of(ctx, else_expr);
            match (lt, rt) {
                (Some(a), Some(b)) if a == b => Some(a),
                (Some(a), Some(b)) => Some(HirType::Union(vec![a, b])),
                (Some(t), None) | (None, Some(t)) => Some(t),
                _ => None,
            }
        }
        _ => hir_inferred_static_type(ctx, e),
    }
}

#[cfg(test)]
#[path = "type_analysis_tests.rs"]
mod tests;
