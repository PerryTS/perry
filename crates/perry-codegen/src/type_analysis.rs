//! Type analysis helpers for expression codegen.
//!
//! Pure predicates and type refinement that don't emit IR themselves.
//! Used by `expr.rs`, `lower_call.rs`, `lower_string_method.rs`,
//! `lower_conditional.rs`, and `stmt.rs`.

use perry_hir::{BinaryOp, Expr, UnaryOp};
use perry_types::Type as HirType;

use crate::expr::FnCtx;

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
        Expr::Number(_) | Expr::Integer(_) => Some(HirType::Number),
        Expr::Binary { op, left, right } => {
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
        Expr::Array(_) | Expr::ArraySpread(_) => {
            Some(HirType::Array(Box::new(HirType::Any)))
        }
        Expr::ArraySlice { .. }
        | Expr::ArrayMap { .. }
        | Expr::ArrayFilter { .. }
        | Expr::ArrayFlat { .. }
        | Expr::ArrayFlatMap { .. }
        | Expr::ArrayFrom(_)
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
        | Expr::StringMatch { .. }
        | Expr::StringMatchAll { .. } => Some(HirType::Array(Box::new(HirType::Any))),
        // TextEncoder.encode(str) — runtime returns an ArrayHeader whose
        // f64 elements hold UTF-8 byte values. Refining the local type to
        // Array(Number) lets `encoded.length` / `encoded[i]` hit the
        // inline array fast paths instead of the dynamic-field fallback.
        Expr::TextEncoderEncode(_) => Some(HirType::Array(Box::new(HirType::Number))),
        // TextDecoder.decode(buf) always produces a string.
        Expr::TextDecoderDecode(_) => Some(HirType::String),
        // string.split(sep) → Array<string>
        Expr::StringSplit { .. } => Some(HirType::Array(Box::new(HirType::String))),
        // Set.values() / Set.keys() → iterable, but Array.from wraps it
        // into an Array. Without an Array.from wrap, it's still iterable.
        // Set/Map constructors refine to `Generic { base, type_args: [] }` —
        // `is_set_expr` / `is_map_expr` check `base == "Set" / "Map"` on the
        // Generic variant, so `Named("Set")` here used to silently miss the
        // fast path and `s.has(v)` returned undefined.
        Expr::SetNewFromArray(_) | Expr::SetNew => Some(HirType::Generic {
            base: "Set".into(),
            type_args: Vec::new(),
        }),
        Expr::MapNewFromArray(_) | Expr::MapNew => Some(HirType::Generic {
            base: "Map".into(),
            type_args: Vec::new(),
        }),
        // Object.keys() always returns string handles.
        Expr::ObjectKeys(_) => Some(HirType::Array(Box::new(HirType::String))),
        Expr::ObjectGetOwnPropertyNames(_) => Some(HirType::Array(Box::new(HirType::String))),
        Expr::ObjectGetOwnPropertySymbols(_) => Some(HirType::Array(Box::new(HirType::Any))),
        Expr::String(_)
        | Expr::ArrayJoin { .. }
        | Expr::StringCoerce(_)
        | Expr::StringFromCodePoint(_)
        | Expr::StringFromCharCode(_)
        | Expr::StringAt { .. }
        | Expr::RegExpSource(_)
        | Expr::RegExpFlags(_)
        // process/os string accessors — lower to runtime calls that
        // return NaN-boxed strings in expr.rs. Refining the local type
        // to String lets `const v = process.version; v.startsWith('v')`
        // hit the string method fast path.
        | Expr::ProcessVersion
        | Expr::ProcessCwd
        | Expr::OsArch
        | Expr::OsType
        | Expr::OsPlatform
        | Expr::OsRelease
        | Expr::OsHostname
        | Expr::OsEOL
        // Date string-returning methods all produce real string handles
        // via js_date_to_*_string. Refining the local lets `dateStr.includes("2024")`
        // hit the string .includes fast path.
        | Expr::DateToDateString(_)
        | Expr::DateToTimeString(_)
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
        // fs.readFileSync returns a NaN-boxed string. Without this,
        // `const text = readFileSync(path); text.split('\n')` routes
        // .split() through the generic method dispatcher instead of
        // the string fast path.
        Expr::FsReadFileSync(_) | Expr::FsReadFileBinary(_) => Some(HirType::String),
        // `process.hrtime.bigint()` returns a BigInt value. Refining the
        // local type lets `hr2 >= hr1` route through the BigInt compare
        // fast path (`js_bigint_cmp`) instead of fcmp-on-NaN.
        Expr::ProcessHrtimeBigint => Some(HirType::BigInt),
        // `BigInt(x)` / `0n` literal via StringCoerce paths.
        // `BigInt('123')` lowers to BigIntCoerce; refine so `const x = BigInt(str)`
        // gets local type BigInt and `x === y` routes through js_bigint_cmp.
        Expr::BigInt(_) | Expr::BigIntCoerce(_) => Some(HirType::BigInt),
        // `let l = new ClassName<...>()` — refine to Named(ClassName)
        // so subsequent `l.method()` dispatch goes through the class
        // method registry instead of the universal fallback. This is
        // the difference between `l.size()` returning the real size
        // and returning undefined for generic class instances.
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
        | Expr::BufferAlloc { .. }
        | Expr::BufferAllocUnsafe(_)
        | Expr::BufferConcat(_)
        | Expr::CryptoRandomBytes(_) => Some(HirType::Named("Uint8Array".into())),
        // Compare results are now NaN-boxed booleans (TAG_TRUE/FALSE).
        // Type-refining the local as Boolean lets is_numeric_expr
        // skip the fast path (which would emit fcmp/sitofp on a NaN
        // bit pattern, giving wrong results) and routes printing
        // through js_console_log_dynamic which dispatches on the
        // NaN tag to print "true"/"false" instead of "1"/"0".
        Expr::Compare { .. } | Expr::Bool(_) | Expr::Logical { .. } => {
            Some(HirType::Boolean)
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
            // Error instance `e.message` / `e.stack` / `e.name` — all
            // return string handles via the runtime's GC_TYPE_ERROR
            // dispatch in js_object_get_field_by_name_f64. Refining to
            // String lets `const m = e.message; m.length` hit the
            // string fast path instead of returning undefined.
            if matches!(property.as_str(), "message" | "stack" | "name") {
                let _ = object;
                return Some(HirType::String);
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
        Expr::Call { callee, .. } => {
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
            }
            // `crypto.createHash(alg).update(data).digest(enc)` chain.
            // The expr.rs handler collapses this into a runtime call that
            // returns a NaN-boxed string. Refine the local to String so
            // `hmac === hmac2` routes through `js_string_equals` instead of
            // bit-comparing two distinct allocations.
            if is_crypto_digest_chain(callee) {
                return Some(HirType::String);
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
            // Cross-module function calls: refine from the imported return
            // type table. Without this, `const name = getFileName(path)`
            // stays typed as Any even though `getFileName` declares
            // `return_type: String` in the source module. This causes
            // `name.charCodeAt(i)` to fall through to the generic method
            // dispatcher (which returns [object Object] for strings).
            if let Expr::ExternFuncRef { name, .. } = callee.as_ref() {
                if let Some(ret_ty) = ctx.imported_func_return_types.get(name) {
                    if !matches!(ret_ty, HirType::Any | HirType::Void) {
                        return Some(ret_ty.clone());
                    }
                }
            }
            None
        }
        _ => None,
    }
}

/// Detects the `crypto.createHash(alg).update(data).digest(enc)` /
/// `crypto.createHmac(alg, key).update(data).digest(enc)` chain shape.
/// Walks the nested PropertyGet→Call structure looking for the
/// `NativeModuleRef("crypto")` root.
fn is_crypto_digest_chain(callee: &Expr) -> bool {
    let Expr::PropertyGet { property: p1, object: o1 } = callee else { return false };
    if p1 != "digest" { return false; }
    let Expr::Call { callee: c2, .. } = o1.as_ref() else { return false };
    let Expr::PropertyGet { property: p2, object: o2 } = c2.as_ref() else { return false };
    if p2 != "update" { return false; }
    let Expr::Call { callee: c3, .. } = o2.as_ref() else { return false };
    let Expr::PropertyGet { property: p3, object: o3 } = c3.as_ref() else { return false };
    if p3 != "createHash" && p3 != "createHmac" { return false; }
    matches!(o3.as_ref(), Expr::NativeModuleRef(n) if n == "crypto")
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
    let mut out: Vec<u32> = explicit.to_vec();
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
        Expr::LocalGet(id) => matches!(
            ctx.local_types.get(id),
            Some(HirType::BigInt)
        ),
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
        _ => false,
    }
}

pub(crate) fn is_numeric_expr(ctx: &FnCtx<'_>, e: &Expr) -> bool {
    match e {
        Expr::Integer(_) | Expr::Number(_) => true,
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
        Expr::Binary { op: BinaryOp::Add, left, right } => {
            is_numeric_expr(ctx, left) && is_numeric_expr(ctx, right)
        }
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
            let Expr::LocalGet(arr_id) = object.as_ref() else {
                return false;
            };
            match ctx.local_types.get(arr_id) {
                Some(HirType::Array(elem)) => {
                    matches!(**elem, HirType::Number | HirType::Int32)
                }
                _ => false,
            }
        }
        // User function calls returning Number: skip js_number_coerce.
        // Without this, `fib(n-1) + fib(n-2)` wraps both results in
        // js_number_coerce — ~4 billion wasted runtime calls on fib(40).
        Expr::Call { callee, .. } => {
            if let Expr::FuncRef(fid) = callee.as_ref() {
                ctx.func_signatures
                    .get(fid)
                    .map(|(_, _, returns_number)| *returns_number)
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
        Expr::Logical { left, right, .. } => {
            is_bool_expr(ctx, left) && is_bool_expr(ctx, right)
        }
        Expr::Unary { op: UnaryOp::Not, .. } => true,
        Expr::BooleanCoerce(_) => true,
        Expr::IsFinite(_) | Expr::IsNaN(_) | Expr::NumberIsNaN(_) | Expr::NumberIsFinite(_)
        | Expr::NumberIsInteger(_) | Expr::IsUndefinedOrBareNan(_) => true,
        Expr::SetHas { .. } | Expr::SetDelete { .. } | Expr::MapHas { .. }
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
        Expr::String(_) => true,
        Expr::LocalGet(id) => {
            matches!(ctx.local_types.get(id), Some(HirType::String))
        }
        Expr::StringCoerce(_)
        | Expr::TypeOf(_)
        | Expr::ArrayJoin { .. }
        | Expr::JsonStringify(_)
        | Expr::JsonStringifyPretty { .. }
        | Expr::JsonStringifyFull(..)
        | Expr::StringFromCodePoint(_)
        | Expr::StringFromCharCode(_)
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
        | Expr::ProcessVersion
        | Expr::ProcessCwd
        | Expr::OsArch
        | Expr::OsType
        | Expr::OsPlatform
        | Expr::OsRelease
        | Expr::OsHostname
        | Expr::OsEOL => true,
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
        Expr::Binary { op: BinaryOp::Add, left, right } => {
            is_definitely_string_expr(ctx, left) || is_definitely_string_expr(ctx, right)
        }
        // Ternary `cond ? a : b` is definitely a string when BOTH
        // branches are definitely strings. Without this, code like
        //   (d ? "D" : "") + (v ? "V" : "")
        // misses the string-concat fast path because each ternary is
        // typed as Any, the `+` falls through to numeric Add, both
        // operands get js_number_coerce'd (string → NaN), and the
        // result prints as "NaN" instead of the concatenation.
        Expr::Conditional { then_expr, else_expr, .. } => {
            is_definitely_string_expr(ctx, then_expr)
                && is_definitely_string_expr(ctx, else_expr)
        }
        _ => false,
    }
}

pub(crate) fn is_string_expr(ctx: &FnCtx<'_>, e: &Expr) -> bool {
    match e {
        Expr::String(_) => true,
        Expr::LocalGet(id) => {
            match ctx.local_types.get(id) {
                Some(HirType::String) => true,
                // Union(String, Null/Void) — nullable strings are still
                // strings at runtime when non-null. The ?. and != null
                // guard paths lower the non-null case through the string
                // method dispatch. Without this, `(s: string | null).
                // toUpperCase()` fell through to the generic path and
                // returned undefined.
                Some(HirType::Union(members)) => {
                    members.iter().any(|m| matches!(m, HirType::String))
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
        | Expr::PathNormalize(_) => true,
        // String.fromCodePoint(...) / String.fromCharCode(...) / str.at(i)
        // / RegExp.source|flags — all produce string handles.
        Expr::StringFromCodePoint(_)
        | Expr::StringFromCharCode(_)
        | Expr::StringAt { .. }
        | Expr::RegExpSource(_)
        | Expr::RegExpFlags(_)
        // Date.prototype.to*String() → string
        | Expr::DateToDateString(_)
        | Expr::DateToTimeString(_)
        | Expr::DateToLocaleString(_)
        | Expr::DateToLocaleDateString(_)
        | Expr::DateToLocaleTimeString(_)
        | Expr::DateToISOString(_)
        | Expr::DateToJSON(_)
        // node:path constants
        | Expr::PathSep
        | Expr::PathDelimiter
        // JSON.stringify returns a string
        | Expr::JsonStringify(_)
        | Expr::JsonStringifyPretty { .. }
        | Expr::JsonStringifyFull(..) => true,
        // process.* / os.* string-returning accessors. These lower to runtime
        // calls that return raw StringHeader* pointers, NaN-boxed with STRING_TAG
        // in expr.rs. Without this, `process.version.startsWith('v')` falls
        // through to the generic native method dispatch and returns undefined.
        Expr::ProcessVersion
        | Expr::ProcessCwd
        | Expr::OsArch
        | Expr::OsType
        | Expr::OsPlatform
        | Expr::OsRelease
        | Expr::OsHostname
        | Expr::OsEOL => true,
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
        Expr::PropertyGet { property, .. }
            if matches!(property.as_str(), "message" | "stack" | "name") =>
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
        // `crypto.createHash(alg).update(data).digest(enc)` chain.
        // Recognized so chained `.length`/`.includes`/`===` on the
        // resulting hex string hit the string fast paths.
        Expr::Call { callee, .. } if is_crypto_digest_chain(callee) => true,
        // atob/btoa always return strings.
        Expr::Atob(_) | Expr::Btoa(_) => true,
        _ => false,
    }
}

/// Statically determine whether an expression evaluates to a Promise.
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
        Expr::LocalGet(id) => matches!(
            ctx.local_types.get(id),
            Some(HirType::Promise(_))
        ),
        // Promise.resolve / reject / all / race / allSettled
        Expr::Call { callee, .. } => match callee.as_ref() {
            Expr::PropertyGet { object, property } => {
                // `Promise.resolve(...)` etc. — GlobalGet receiver with
                // a promise-shaped static method name.
                if matches!(object.as_ref(), Expr::GlobalGet(_))
                    && matches!(
                        property.as_str(),
                        "resolve" | "reject" | "all" | "race" | "allSettled"
                    )
                {
                    return true;
                }
                // `Array.fromAsync(...)` returns a Promise<Array>.
                if matches!(object.as_ref(), Expr::GlobalGet(_))
                    && property == "fromAsync"
                {
                    return true;
                }
                // `.then(cb)` / `.catch(cb)` / `.finally(cb)` on a promise
                // receiver — the result is itself a promise.
                if matches!(property.as_str(), "then" | "catch" | "finally")
                    && is_promise_expr(ctx, object)
                {
                    return true;
                }
                false
            }
            // Direct call to a locally-defined async function — its
            // return value is a `Promise<T>`. The HIR's
            // `Function::is_async` flag is collected into
            // `cross_module.local_async_funcs` at module compile time.
            Expr::FuncRef(fid) => ctx.local_async_funcs.contains(fid),
            _ => false,
        },
        _ => false,
    }
}

/// Look up a field's global index in the object's slot layout, walking
/// the inheritance chain. Returns `Some(index)` only if the field is a
/// plain instance field (no getter/setter shadowing) and the entire
/// parent chain is resolvable from `ctx.classes`.
///
/// Layout convention: parent class fields come first (in declaration
/// order), then the child's own fields. So `Child` with parent `Base`
/// and `Base.fields = [a, b]`, `Child.fields = [c]` produces slot order
/// `[a, b, c]` — `Base.b` is index 1, `Child.c` is index 2.
///
/// This mirrors how `js_object_alloc_with_parent` lays out the inline
/// field array (parent first, then child) and how the constructor
/// codegen at `lower_call.rs::compile_new` walks parent constructors
/// before the child's own initializers.
///
/// Returns `None` when:
/// - The class has a getter or setter for this property (the dispatch
///   path needs to call the synthesized accessor instead).
/// - The field name doesn't exist anywhere in the chain.
/// - A parent class isn't in `ctx.classes` (imported class with no HIR).
pub(crate) fn class_field_global_index(
    ctx: &FnCtx<'_>,
    class_name: &str,
    property: &str,
) -> Option<u32> {
    // Walk parent chain to find the field. Parent fields come first in
    // the slot layout, so we sum parent counts as we descend.
    fn walk(
        ctx: &FnCtx<'_>,
        class_name: &str,
        property: &str,
        offset: u32,
    ) -> Option<u32> {
        let class = ctx.classes.get(class_name)?;
        // Bail if a getter/setter shadows the field — those need real
        // method dispatch, not a direct memory access.
        if class.getters.iter().any(|(n, _)| n == property)
            || class.setters.iter().any(|(n, _)| n == property)
        {
            return None;
        }
        // Compute the byte-offset contribution from this class's parent.
        let parent_count = if let Some(parent_name) = class.extends_name.as_deref() {
            let mut p_count = 0u32;
            let mut p = Some(parent_name.to_string());
            while let Some(name) = p {
                if let Some(parent) = ctx.classes.get(&name) {
                    p_count += parent.fields.len() as u32;
                    p = parent.extends_name.clone();
                } else {
                    return None; // unresolvable parent — no inline path
                }
            }
            p_count
        } else {
            0
        };
        // Look for the field on this class first (the most-derived
        // declaration shadows parents in TypeScript).
        if let Some(idx) = class.fields.iter().position(|f| f.name == property) {
            return Some(offset + parent_count + idx as u32);
        }
        // Otherwise walk into the parent chain looking for the field.
        if let Some(parent_name) = class.extends_name.as_deref() {
            return walk(ctx, parent_name, property, offset);
        }
        None
    }
    walk(ctx, class_name, property, 0)
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
            HirType::Generic { base, .. } if ctx.classes.contains_key(base) => {
                Some(base.clone())
            }
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
        // `this` inside a constructor or method body — the class name is
        // at the top of class_stack (for inlined constructors) or comes
        // from the enclosing method's owning class.
        Expr::This => ctx.class_stack.last().cloned(),
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
        // `T | null`, `T | undefined`, `T[] | null` — when an `if (x)`
        // guard narrows away the null/undefined, the truthy branch
        // still has the same union type in the HIR, so recognize
        // unions whose non-nullish variant is an array. Without this
        // `maybeArr.length` falls through to object-field access and
        // prints `undefined`.
        Some(HirType::Union(variants)) => variants.iter().any(|v| {
            matches!(v, HirType::Array(_) | HirType::Tuple(_))
        }),
        _ => false,
    }
}

/// Best-effort static type lookup for an expression. Returns the HIR
/// type when it's cheap to determine (literals, locals, field accesses
/// on known classes). Returns `None` when computing the type would
/// require a fuller type-checker pass.
pub(crate) fn static_type_of(ctx: &FnCtx<'_>, e: &Expr) -> Option<HirType> {
    match e {
        Expr::Array(_) => Some(HirType::Array(Box::new(HirType::Any))),
        // Built-in `new Array(...)` produces a real array, not a generic
        // class instance. Without this, the receiver of any chained
        // `.fill()` / `.push()` / `.length` would not be recognized by
        // `is_array_expr`, falling out of the array method dispatch
        // and crashing.
        Expr::New { class_name, .. } if class_name == "Array" => {
            Some(HirType::Array(Box::new(HirType::Any)))
        }
        Expr::String(_) => Some(HirType::String),
        Expr::Number(_) | Expr::Integer(_) => Some(HirType::Number),
        Expr::Bool(_) => Some(HirType::Boolean),
        Expr::LocalGet(id) => ctx.local_types.get(id).cloned(),
        Expr::PropertyGet { object, property } => {
            // If the object is a known class instance, look up the field
            // type from the class definition.
            let receiver_class = receiver_class_name(ctx, object)?;
            let class = ctx.classes.get(&receiver_class)?;
            class
                .fields
                .iter()
                .find(|f| f.name == *property)
                .map(|f| f.ty.clone())
                .or_else(|| {
                    // Walk up the inheritance chain.
                    let mut parent = class.extends_name.as_deref();
                    while let Some(p) = parent {
                        if let Some(pc) = ctx.classes.get(p) {
                            if let Some(field) = pc.fields.iter().find(|f| f.name == *property) {
                                return Some(field.ty.clone());
                            }
                            parent = pc.extends_name.as_deref();
                        } else {
                            break;
                        }
                    }
                    None
                })
        }
        Expr::This => {
            let cls = ctx.class_stack.last()?.clone();
            Some(HirType::Named(cls))
        }
        Expr::ArrayMap { .. }
        | Expr::ArrayFilter { .. }
        | Expr::ArraySpread(_)
        | Expr::ArraySlice { .. }
        | Expr::ArrayToReversed { .. }
        | Expr::ArrayToSorted { .. }
        | Expr::ArrayToSpliced { .. }
        | Expr::ArrayWith { .. }
        | Expr::ArrayFlat { .. }
        | Expr::ArrayFlatMap { .. }
        | Expr::ArrayFromMapped { .. }
        | Expr::ArrayFrom(_)
        | Expr::ArrayEntries(_)
        | Expr::ArrayKeys(_)
        | Expr::ArrayValues(_)
        | Expr::ObjectKeys(_)
        | Expr::ObjectValues(_)
        | Expr::ObjectEntries(_) => {
            Some(HirType::Array(Box::new(HirType::Any)))
        }
        // `str.split(delim)` returns Array<String>. Catches the generic
        // Call form that bypasses the `Expr::StringSplit` variant — e.g.
        // `"a,b,c".split(",")` in an expression position where we need
        // `.length` / `[i]` to follow the array fast path.
        // Also: `str.match(regex)` / `str.matchAll(regex)` produce arrays.
        Expr::Call { callee, .. }
            if matches!(
                callee.as_ref(),
                Expr::PropertyGet { property, object } if matches!(
                    property.as_str(), "split" | "match" | "matchAll"
                ) && is_string_expr(ctx, object)
            ) =>
        {
            Some(HirType::Array(Box::new(HirType::String)))
        }
        // `arr[i]` where `arr: Array<T>` has static type `T`. This lets
        // nested access like `grid[i][j]` and `grid[i].length` reach
        // the array fast paths (via is_array_expr) when `grid` is
        // statically known to be `Array<Array<T>>` / `Array<Tuple<...>>`.
        // Also handles `Record<K, V>[key]` → V so `groups["a"].length`
        // on `Record<string, number[]>` finds the array fast path.
        Expr::IndexGet { object, .. } => match static_type_of(ctx, object)? {
            HirType::Array(inner) => Some(*inner),
            HirType::Tuple(elems) if !elems.is_empty() => {
                Some(elems[0].clone())
            }
            HirType::Generic { base, type_args } if base == "Record" && type_args.len() == 2 => {
                Some(type_args[1].clone())
            }
            _ => None,
        },
        _ => None,
    }
}
