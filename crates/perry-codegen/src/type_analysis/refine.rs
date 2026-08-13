//! Init-expression type refinement + capture analysis.
//!
//! Split out of `type_analysis.rs` (file-size gate). Pure code move.

use super::*;

use perry_hir::types::Type as HirType;
use perry_hir::{BinaryOp, Expr, UnaryOp};

use crate::expr::FnCtx;
use crate::type_analysis_facts::hir_inferred_refinable_type;
use crate::type_analysis_net::net_result_type;

pub(crate) fn is_global_constructor_expr(e: &Expr, name: &str) -> bool {
    matches!(e, Expr::GlobalGet(_))
        || matches!(
            e,
            Expr::PropertyGet { object, property, .. }
                if property == name && matches!(object.as_ref(), Expr::GlobalGet(_))
        )
}

fn is_process_module_ref_name(module: &str) -> bool {
    let module = module.strip_prefix("node:").unwrap_or(module);
    matches!(module, "process" | "process.namespace" | "process.default")
}

pub(crate) fn is_process_namespace_version_property(object: &Expr, property: &str) -> bool {
    property == "version"
        && matches!(object, Expr::NativeModuleRef(module) if is_process_module_ref_name(module))
}

/// Drop `null` / `undefined` / `void` from a union and return the single
/// surviving member, if there is exactly one.
///
/// `type Env = { … }` + `let e: Env | null` is the ordinary way to write a
/// linked structure in TypeScript, and it is the *only* thing standing between
/// `const names = e.names` and a usable type: a read that returns at all had a
/// non-nullish receiver, because reading a property off `null`/`undefined`
/// throws. So the nullish arms contribute nothing to the RESULT type and can be
/// dropped without weakening anything.
fn strip_nullish_union(ty: &HirType) -> Option<&HirType> {
    let HirType::Union(members) = ty else {
        return Some(ty);
    };
    let mut live = members
        .iter()
        .filter(|m| !matches!(m, HirType::Null | HirType::Void));
    let first = live.next()?;
    if live.next().is_some() {
        return None;
    }
    Some(first)
}

/// Resolve `<receiver>.<property>`'s type from the receiver's DECLARED
/// annotation, through the same class / interface / object-type-alias tables
/// [`crate::type_analysis::static_type_of`] already consults.
///
/// # Why this exists
///
/// `refine_type_from_init`'s class walk resolves the receiver with
/// `receiver_class_name`, which answers `None` for two shapes that dominate
/// real TypeScript:
///
///   * a **reassigned** local (`predicates.rs`'s first arm) — e.g. the cursor
///     of a chain walk, `let e: Env | null = env; … e = e.parent;`
///   * a **union** local — `receiver_class_name`'s `LocalGet` arm matches only
///     `Named`/`Generic`, so `Env | null` resolves to nothing.
///
/// and its table walk then looks only in `ctx.classes`, so a `type X = { … }`
/// alias or an `interface` resolves to nothing either — the `type`-vs-
/// `interface` asymmetry #655 already fixed for `static_type_of` but not here.
///
/// Measured on `gc-handoff/apps/interp.ts`, whose `lookup` is
/// `const names = e.names; for (i = 0; i < names.length; i++) names[i]`: with
/// `names` left `Any`, `names[i]` lowers to `js_dyn_index_get` (4.3% of the
/// program) and `names.length` misses the property IC into `js_array_length`
/// (2.9%). Writing the annotation by hand — `const names: string[] = e.names` —
/// removes both and is **-14.5%** on the whole benchmark. This infers exactly
/// the type that hand annotation would have written.
///
/// # It is a CLAIM, and one consumer could not take one
///
/// This produces a *declared* type, so it is a claim, not a proof — the same
/// claim the author's own `const names: string[] = e.names` would install,
/// through the same `local_types` entry, but Perry enforces no annotation at
/// runtime. Element READS and STORES tolerate that: both re-check
/// `GC_TYPE_ARRAY` on the receiver and fall back, so a violated claim costs a
/// branch and nothing else.
///
/// `.length` used to be the exception: #7854 refused these ids there because
/// its FALLBACK (`js_value_length_f64`) answered **0** for every value that
/// carries no length where JS answers `undefined`, and continued instead of
/// throwing for a nullish receiver (#7853). #7862 replaced that fallback with
/// `js_value_length_property_f64` — ordinary property semantics — so `.length`
/// now takes a claim on the same terms as an element read, and the refusal and
/// its bookkeeping set are gone. `test_gap_declared_field_type_refine_guarded.ts`
/// and `test_gap_7853_declared_array_length_runtime_value.ts` still pin it: the
/// same declaration is handed strings, plain objects, numbers, `null` and
/// `undefined`, and every row must match node.
///
/// Deliberately conservative: only a NON-generic receiver name whose entry is a
/// class, an interface, or an alias to a closed object type answers, and only
/// the property's own declared type is returned — no inheritance walk beyond
/// what the class table already does, and no index-signature fallback.
/// Is `expr` a property READ whose declared type on the receiver's annotation is
/// an array (`e.vals` on `type Env = { vals: Value[] }`)?
///
/// #7854 taught `refine_type_from_init` to recover that type for a LOCAL
/// (`const names = e.names`), which is why `names[i]` is an inline element read
/// today. It did nothing for the read used DIRECTLY as a receiver — `e.vals[i]`,
/// `p.toks[p.pos]` — because the HIR types a `PropertyGet` off a UNION receiver
/// as `Any` (`perry-hir/src/analysis/value_types.rs`, the `Union` arm), so
/// `static_type_of` answers `Any` and `expr/index_get.rs` routes the read to the
/// `js_dyn_index_get` unknown-receiver dispatcher. In `gc-handoff/apps/interp.ts`
/// — where the lexer, the parser cursor and the environment chain are all
/// `type` aliases over arrays — that dispatcher plus the `js_array_length` its
/// miss path calls is 9.6% of the program.
///
/// **This is a claim, not a proof**, and its ONLY admissible consumer is a
/// guarded element read: `lower_guarded_array_index_get` re-checks
/// `GC_TYPE_ARRAY`, the forwarding flag, per-array descriptors, the prototype
/// latch and the bounds on the receiver itself, and routes every failure to
/// `js_typed_feedback_array_index_get_fallback_boxed`. So a violated claim costs
/// a predicted branch and the same answer, which is the exact deal #7854
/// records for element reads. Do not hand it to a consumer that has no guarded
/// fallback.
pub(crate) fn declared_array_property_claim(ctx: &FnCtx<'_>, expr: &Expr) -> bool {
    let Expr::PropertyGet {
        object, property, ..
    } = expr
    else {
        return false;
    };
    matches!(
        declared_property_type_from_annotation(ctx, object, property),
        Some(HirType::Array(_)) | Some(HirType::Tuple(_))
    )
}

pub(crate) fn declared_property_type_from_annotation(
    ctx: &FnCtx<'_>,
    object: &Expr,
    property: &str,
) -> Option<HirType> {
    let declared = match object {
        // This function produces metadata only. Representation consumers must
        // either re-check the current value or use `proven_local_types`.
        Expr::LocalGet(id) => ctx.local_type_hint(id)?,
        _ => return None,
    };
    match strip_nullish_union(declared)? {
        // `let e: Env | null` where `type Env = { … }` / `interface Env` /
        // `class Env`.
        HirType::Named(name) => {
            if let Some(class) = ctx.classes.get(name) {
                if let Some(f) = class.fields.iter().find(|f| f.name == property) {
                    return Some(f.ty.clone());
                }
            }
            if let Some(iface) = ctx.interfaces.get(name) {
                if let Some(p) = iface.properties.iter().find(|p| p.name == property) {
                    return Some(p.ty.clone());
                }
            }
            if let Some(HirType::Object(obj)) = ctx.type_aliases.get(name) {
                return obj.properties.get(property).map(|p| p.ty.clone());
            }
            None
        }
        // The alias was already expanded in place (`let e: { … } | null`).
        HirType::Object(obj) => obj.properties.get(property).map(|p| p.ty.clone()),
        _ => None,
    }
}

/// Derive only the runtime kind established by the initializer expression
/// itself. This is deliberately narrower than [`refine_type_from_init`],
/// which is also allowed to propagate declared property/return metadata for
/// consumers that carry their own runtime guard.
///
/// Array/object/function details are erased to their outer runtime kind. A
/// specialized method HIR node is intentionally not enough: it may have been
/// selected from source metadata and may retain an override-aware fallback
/// whose result has another kind. New expression variants are unproven by
/// default until their full lowering contract is explicitly reviewed here.
pub(crate) fn proven_type_from_init(ctx: &FnCtx<'_>, init: &Expr) -> Option<HirType> {
    match init {
        Expr::LocalGet(id) => ctx.stable_local_type_proof(id).cloned(),
        Expr::Undefined | Expr::Void(_) => Some(HirType::Void),
        Expr::Null => Some(HirType::Null),
        Expr::Bool(_) | Expr::Compare { .. } => Some(HirType::Boolean),
        Expr::Number(_)
        | Expr::Integer(_)
        | Expr::PodLayoutSizeOf { .. }
        | Expr::PodLayoutAlignOf { .. }
        | Expr::PodLayoutOffsetOf { .. } => Some(HirType::Number),
        Expr::Unary { op, operand } => match op {
            UnaryOp::Not => Some(HirType::Boolean),
            UnaryOp::Neg | UnaryOp::BitNot if is_bigint_expr(ctx, operand) => Some(HirType::BigInt),
            UnaryOp::Neg | UnaryOp::Pos | UnaryOp::BitNot if is_numeric_expr(ctx, operand) => {
                Some(HirType::Number)
            }
            _ => None,
        },
        Expr::Binary { op, left, right }
            if is_bigint_expr(ctx, left) && is_bigint_expr(ctx, right) =>
        {
            matches!(
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
            .then_some(HirType::BigInt)
        }
        Expr::Binary { left, right, .. }
            if is_numeric_expr(ctx, left) && is_numeric_expr(ctx, right) =>
        {
            Some(HirType::Number)
        }
        Expr::String(_) | Expr::WtfString(_) | Expr::I18nString { .. } | Expr::TypeOf(_) => {
            Some(HirType::String)
        }
        Expr::Array(_) | Expr::ArraySpread(_) => Some(HirType::Array(Box::new(HirType::Any))),
        Expr::MapNew | Expr::MapNewFromArray(_) => Some(HirType::Generic {
            base: "Map".to_string(),
            type_args: vec![HirType::Any, HirType::Any],
        }),
        Expr::SetNew | Expr::SetNewFromArray(_) => Some(HirType::Generic {
            base: "Set".to_string(),
            type_args: vec![HirType::Any],
        }),
        // These HIR constructors always allocate a Uint8Array representation;
        // unlike metadata-selected access nodes, they have no override-aware
        // result fallback.
        Expr::Uint8ArrayNew(_) | Expr::Uint8ArrayFrom(_) => {
            Some(HirType::Named("Uint8Array".to_string()))
        }
        Expr::Object(_) | Expr::ObjectSpread { .. } => Some(HirType::Object(Default::default())),
        Expr::Closure {
            is_async,
            is_generator,
            ..
        } => Some(HirType::Function(perry_hir::types::FunctionType {
            params: Vec::new(),
            return_type: Box::new(HirType::Any),
            is_async: *is_async,
            is_generator: *is_generator,
        })),
        // A constructor can explicitly return a different object, so `new C`
        // proves only Object, never C's class-specific layout.
        Expr::New { .. } => Some(HirType::Object(Default::default())),
        Expr::BigInt(_) => Some(HirType::BigInt),
        _ => None,
    }
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
        Expr::IndexGet { object, index } => {
            // arr[i] where arr is Array<T> → element type T.
            // Handles both LocalGet(arr) and PropertyGet(this, "field")
            // — the latter lets `this.parts[i]` get the right type
            // when `parts: string[]`.
            //
            // #7796: only at a NUMERIC index. `a[Symbol.iterator]` on a
            // `number[]` reads a property of the array OBJECT and answers with
            // a function, so typing the local as `number` is simply false — and
            // it is a costly kind of false, because a local believed numeric is
            // tested for truthiness with `fcmp one %v, 0.0`. Every NaN-boxed
            // pointer is a NaN, so that comparison says false for every
            // function, object and string, and `if (f)` took the wrong branch
            // on a value `typeof` and `Boolean()` both called a function.
            if !is_numeric_expr(ctx, index) {
                return None;
            }
            if let Expr::LocalGet(arr_id) = object.as_ref() {
                if let Some(HirType::Array(elem_ty)) = ctx.stable_local_type_proof(arr_id) {
                    return Some((**elem_ty).clone());
                }
                // str[i] — single-char string from string indexing.
                if let Some(HirType::String) = ctx.stable_local_type_proof(arr_id) {
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
        Expr::PropertyGet { object, property, .. } => {
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
            if let Some(ty) = receiver_class_name(ctx, object)
                .and_then(|receiver_class| ctx.classes.get(&receiver_class))
                .and_then(|class| class.fields.iter().find(|f| f.name == *property))
                .map(|f| f.ty.clone())
            {
                return Some(ty);
            }
            declared_property_type_from_annotation(ctx, object, property)
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
            if let Expr::PropertyGet { object, property, .. } = callee.as_ref() {
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
            if let Expr::PropertyGet { property, object, .. } = callee.as_ref() {
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
pub(crate) fn is_crypto_digest_chain(callee: &Expr) -> bool {
    crypto_digest_chain_has_string_encoding(callee).is_some()
}

#[allow(dead_code)]
fn crypto_digest_chain_has_string_encoding(callee: &Expr) -> Option<bool> {
    let Expr::PropertyGet {
        property: p1,
        object: o1,
        ..
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
        ..
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
        ..
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
