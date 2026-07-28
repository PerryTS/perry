//! Small self-contained codegen helpers (arg-array marshalling, NaN-box
//! unboxing, globalThis builtin-name tables) extracted from `expr.rs`,
//! issue #1098. Pure move — no logic changes.

use anyhow::Result;
use perry_hir::types::Type as HirType;
use perry_hir::{BinaryOp, Expr, UnaryOp};

use super::{lower_expr, FnCtx};
use crate::block::LlBlock;
use crate::nanbox::POINTER_MASK_I64;
use crate::types::{DOUBLE, I32, I64};

/// Static-type predicate: the type's runtime array layout has no pointer
/// payloads, so a pointer-mask layout note isn't necessary for stores.
/// Folded into the codegen for `IndexSet` / `ArrayPush` to skip the
/// note-emission path on statically-numeric arrays.
pub(crate) fn type_has_numeric_pointer_free_array_layout(ty: &HirType) -> bool {
    match ty {
        HirType::Array(elem) => matches!(elem.as_ref(), HirType::Number | HirType::Int32),
        // #6011: `new Array<number>(n)` declarations carry the generic
        // spelling; treat it exactly like `Array(Number)` so number-context
        // element reads keep their coerced fallback (a raw arithmetic op on
        // an uncoerced boxed `undefined` would propagate the NaN-box payload
        // verbatim) and stores take the guarded numeric-layout path.
        HirType::Generic { base, type_args } if base == "Array" && type_args.len() == 1 => {
            matches!(type_args[0], HirType::Number | HirType::Int32)
        }
        HirType::Tuple(elems) => elems
            .iter()
            .all(|elem| matches!(elem, HirType::Number | HirType::Int32)),
        HirType::Union(variants) => variants.iter().all(|variant| {
            matches!(variant, HirType::Null | HirType::Void | HirType::Never)
                || type_has_numeric_pointer_free_array_layout(variant)
        }),
        _ => false,
    }
}

pub(crate) fn expr_has_numeric_pointer_free_array_layout(ctx: &FnCtx<'_>, expr: &Expr) -> bool {
    crate::type_analysis::static_type_of(ctx, expr)
        .as_ref()
        .is_some_and(type_has_numeric_pointer_free_array_layout)
}

fn local_get_produces_non_pointer_bits_by_dataflow(ctx: &FnCtx<'_>, id: u32) -> bool {
    (ctx.i32_counter_slots.contains_key(&id) || ctx.integer_locals.contains(&id))
        // Repsel Phase 1: a canonical-i32 local has no `ctx.locals` entry —
        // its rep-map membership proves plain (non-boxed, non-captured)
        // numeric storage just as well.
        && (ctx.locals.contains_key(&id) || ctx.local_slot_reps.contains_key(&id))
        && !ctx.boxed_vars.contains(&id)
        && !ctx.closure_captures.contains_key(&id)
        && !ctx.module_globals.contains_key(&id)
}

fn expr_produces_numeric_bits_by_construction(ctx: &FnCtx<'_>, expr: &Expr) -> bool {
    match expr {
        Expr::Integer(_)
        | Expr::Number(_)
        | Expr::PodLayoutSizeOf { .. }
        | Expr::PodLayoutAlignOf { .. }
        | Expr::PodLayoutOffsetOf { .. }
        | Expr::DateNow
        | Expr::NumberCoerce(_) => true,
        Expr::LocalGet(id) => local_get_produces_non_pointer_bits_by_dataflow(ctx, *id),
        Expr::Unary { op, operand } => match op {
            UnaryOp::Neg | UnaryOp::Pos | UnaryOp::BitNot => {
                expr_produces_numeric_bits_by_construction(ctx, operand)
            }
            UnaryOp::Not => false,
        },
        Expr::Binary { op, left, right } => {
            !matches!(op, BinaryOp::Add)
                && expr_produces_numeric_bits_by_construction(ctx, left)
                && expr_produces_numeric_bits_by_construction(ctx, right)
        }
        Expr::Conditional {
            then_expr,
            else_expr,
            ..
        } => {
            expr_produces_numeric_bits_by_construction(ctx, then_expr)
                && expr_produces_numeric_bits_by_construction(ctx, else_expr)
        }
        Expr::Sequence(exprs) => exprs
            .last()
            .is_some_and(|last| expr_produces_numeric_bits_by_construction(ctx, last)),
        _ => false,
    }
}

pub(crate) fn expr_produces_non_pointer_bits_by_construction(ctx: &FnCtx<'_>, expr: &Expr) -> bool {
    match expr {
        Expr::Undefined
        | Expr::Null
        | Expr::Bool(_)
        | Expr::Compare { .. }
        | Expr::Void(_)
        | Expr::BooleanCoerce(_)
        | Expr::IsNaN(_)
        | Expr::IsFinite(_)
        | Expr::NumberIsNaN(_)
        | Expr::NumberIsFinite(_)
        | Expr::NumberIsInteger(_)
        | Expr::NumberIsSafeInteger(_) => true,
        Expr::Unary {
            op: UnaryOp::Not, ..
        } => true,
        Expr::Conditional {
            then_expr,
            else_expr,
            ..
        } => {
            expr_produces_non_pointer_bits_by_construction(ctx, then_expr)
                && expr_produces_non_pointer_bits_by_construction(ctx, else_expr)
        }
        Expr::Sequence(exprs) => exprs
            .last()
            .is_some_and(|last| expr_produces_non_pointer_bits_by_construction(ctx, last)),
        _ => expr_produces_numeric_bits_by_construction(ctx, expr),
    }
}

/// Stores into statically numeric arrays may preserve the initial
/// pointer-free layout only when the stored value's bits are known from
/// expression construction, not from TypeScript's local type alone. Other
/// stores update the mask so pointer writes and pointer-clearing overwrites
/// on mixed arrays remain precise.
pub(crate) fn array_store_needs_layout_note(ctx: &FnCtx<'_>, array: &Expr, value: &Expr) -> bool {
    !(expr_has_numeric_pointer_free_array_layout(ctx, array)
        && expr_produces_non_pointer_bits_by_construction(ctx, value))
}

pub(crate) fn array_store_needs_write_barrier(ctx: &FnCtx<'_>, value: &Expr) -> bool {
    !expr_produces_non_pointer_bits_by_construction(ctx, value)
}

/// Object twin of [`array_store_needs_layout_note`] — Phase 4b.1.
///
/// `layout_note_slot` is a provable no-op for a class-field store whose value is
/// a **non-pointer by construction**: in that case the note can only ever
/// *clear* mask state, never set it, so it is never the difference between a
/// slot being scanned and a live child being stranded. That holds in every
/// layout state the receiver can be in:
///
/// - `GC_LAYOUT_UNKNOWN` — the note returns at its own state check.
/// - intact typed descriptor — a non-pointer value falls straight through the
///   `pointer_mask` arm without touching the descriptor. (The `raw_f64_mask`
///   arm is unreachable from the caller: it only uses this predicate when
///   `requires_raw_f64` is false, and that is the very same
///   `type_is_raw_f64_candidate` predicate the mask is built from.)
/// - `GC_LAYOUT_POINTER_FREE` — the note's `!pointer && POINTER_FREE` early
///   return.
/// - `GC_LAYOUT_SIDE_MASK` — the note would only *clear* this slot's bit.
///   Skipping that leaves a stale set bit over a non-pointer, which costs an
///   extra visit and nothing else: `mark_field_into_worklist` (`gc/trace.rs`)
///   re-validates every slot word — f64 bit patterns fall outside the 48-bit
///   user-address range and are rejected — and the evacuation rewrite path
///   routes through the same function.
///
/// **Deliberately NOT elided: a pointer-valued store into a pointer-masked
/// slot.** That would be a no-op under an intact descriptor, but the receiver
/// is not guaranteed to have one — `lower_new_impl` has an exit
/// (`lower_call/new.rs`, the `force_ctor_call` branch where
/// `call_local_constructor_symbol` yields `None`) that returns a freshly
/// allocated instance *without* emitting `js_gc_init_typed_shape_layout`. Such
/// an object sits at `GC_LAYOUT_POINTER_FREE`, where the note is the only thing
/// that ever sets the pointer-mask bit the collector reads. Closing that exit is
/// the prerequisite for the stronger elision.
pub(crate) fn class_field_store_needs_layout_note(ctx: &FnCtx<'_>, value: &Expr) -> bool {
    !expr_produces_non_pointer_bits_by_construction(ctx, value)
}

/// `js_string_addref_if_heap_string` demotes a uniquely-owned (refcount==1)
/// heap string to shared when it becomes aliased from a heap slot, and is a
/// no-op for every non-`STRING_TAG` value (`string/alloc.rs`). So it is dead
/// exactly when the stored value provably cannot be a heap string — a strictly
/// weaker condition than "cannot be a pointer", which is why this is gated
/// separately from the layout note above.
///
/// **This is keyed on the value expression, never on the declared field type.**
/// Perry does not validate declared types at runtime (see CLAUDE.md, "No
/// runtime type *validation*"): a field declared `boolean` legitimately
/// receives a string that arrived through an `any`, and skipping the demote
/// there would leave a refcount==1 string aliased from the heap for a later
/// in-place `+=` to rewrite underneath the stored slot — silent corruption
/// with no crash to trace it back from.
pub(crate) fn class_field_store_needs_string_addref(ctx: &FnCtx<'_>, value: &Expr) -> bool {
    !expr_cannot_produce_heap_string(ctx, value)
}

/// The stored value provably does not carry `STRING_TAG`.
///
/// Beyond the non-pointer set, the three *literal* constructor forms qualify:
/// each evaluates to a freshly allocated `POINTER_TAG` value with no path to a
/// primitive result. `Expr::New` is deliberately excluded — a constructor
/// return override (`js_ctor_return_override`) makes "what `new C()` evaluates
/// to" a runtime question, and this predicate must not depend on the answer.
fn expr_cannot_produce_heap_string(ctx: &FnCtx<'_>, expr: &Expr) -> bool {
    match expr {
        Expr::Object(_) | Expr::Array(_) | Expr::Closure { .. } => true,
        Expr::Conditional {
            then_expr,
            else_expr,
            ..
        } => {
            expr_cannot_produce_heap_string(ctx, then_expr)
                && expr_cannot_produce_heap_string(ctx, else_expr)
        }
        Expr::Sequence(exprs) => exprs
            .last()
            .is_some_and(|last| expr_cannot_produce_heap_string(ctx, last)),
        _ => expr_produces_non_pointer_bits_by_construction(ctx, expr),
    }
}

/// `lower_expr` variant that hands an expected-type hint down to the
/// object-literal lowerer (so it can pick raw f64 slots when the
/// destination has a typed shape). All other expression kinds ignore
/// the hint.
pub(crate) fn lower_expr_with_expected_type(
    ctx: &mut FnCtx<'_>,
    expr: &Expr,
    expected_ty: Option<&HirType>,
) -> Result<String> {
    match expr {
        Expr::Object(props) => super::lower_object_literal(ctx, props, expected_ty),
        Expr::NativePodView {
            owner,
            byte_offset,
            count,
            view_type,
        } => super::arrays_finds::lower_native_pod_view(
            ctx,
            owner,
            byte_offset,
            count,
            expected_ty,
            view_type.as_ref(),
        ),
        _ => lower_expr(ctx, expr),
    }
}

/// Build a NaN-boxed Array JSValue from a slice of Expr arguments.
pub(crate) fn proxy_build_args_array(ctx: &mut FnCtx<'_>, args: &[Expr]) -> Result<String> {
    let cap = (args.len() as u32).to_string();
    let arr = ctx.block().call(I64, "js_array_alloc", &[(I32, &cap)]);
    let mut current = arr;
    for a in args {
        let v = lower_expr(ctx, a)?;
        current = ctx
            .block()
            .call(I64, "js_array_push_f64", &[(I64, &current), (DOUBLE, &v)]);
    }
    Ok(current)
}

/// Build the `, !alias.scope !N, !noalias !M` suffix attached to Buffer
/// load/store instructions on the GEP fast path. `scope_idx` is the per-
/// buffer identifier allocated by `Stmt::Let` when a `BufferAlloc` init
/// is detected. The metadata IDs map to nodes emitted at module level
/// by `emit_buffer_alias_metadata` (`codegen.rs`):
///
/// - `!(201 + idx)` is the alias-scope list containing this buffer's scope
/// - `!(301 + idx)` is the noalias set listing every *other* buffer's scope
///
/// LLVM's LoopVectorizer uses these to prove that loads from one buffer
/// don't alias stores to another buffer — the fix for the "unsafe
/// dependent memory operations" vectorization remark on the image_conv
/// blur kernel (src reads vs dst writes).
pub(crate) fn buffer_alias_metadata_suffix(scope_idx: u32) -> String {
    let scope_list = 201 + scope_idx;
    let noalias_list = 301 + scope_idx;
    format!(", !alias.scope !{}, !noalias !{}", scope_list, noalias_list)
}

/// Marshal a vector of already-lowered NaN-boxed args into a stack
/// alloca'd `[N x double]` and return `(args_ptr, args_len_str)` ready
/// for an FFI call expecting `(*const f64, usize)`. Empty input returns
/// `("null", "0")` so the FFI sees a null pointer + 0 length.
///
/// Issue #167: the alloca is hoisted to the function entry block via
/// `alloca_entry_array` so every per-iteration call inside a loop
/// reuses one stack slot instead of permanently shrinking the stack.
pub(crate) fn lower_js_args_array(
    ctx: &mut FnCtx<'_>,
    lowered_args: &[String],
) -> (String, String) {
    if lowered_args.is_empty() {
        return ("null".to_string(), "0".to_string());
    }
    let n = lowered_args.len();
    let buf = ctx.func.alloca_entry_array(DOUBLE, n);
    for (i, v) in lowered_args.iter().enumerate() {
        let slot = ctx.block().gep(DOUBLE, &buf, &[(I64, &format!("{}", i))]);
        ctx.block().store(DOUBLE, v, &slot);
    }
    let ptr_reg = ctx.block().next_reg();
    ctx.block().emit_raw(format!(
        "{} = getelementptr [{} x double], ptr {}, i64 0, i64 0",
        ptr_reg, n, buf
    ));
    (ptr_reg, n.to_string())
}

/// Unbox a NaN-boxed double into a raw i64 pointer via inline
/// `bitcast double → i64; and POINTER_MASK_I64`.
///
/// **⚠ Use [`unbox_str_handle`] instead when the value may be a JS string.**
/// The bitcast+mask returns the lower 48 bits, which is the correct
/// `*ObjectHeader` / `*ArrayHeader` / `*ClosureHeader` for heap pointers
/// (POINTER_TAG = 0x7FFD, ARRAY_TAG = 0x7FFB, etc.) and the correct
/// `*StringHeader` for **heap** strings (STRING_TAG = 0x7FFF), but is
/// **garbage** for short-string-optimization values (SHORT_STRING_TAG =
/// 0x7FF9), whose lower 48 bits encode the inline length + bytes. Any
/// runtime function that dereferences the resulting i64 as a
/// `*StringHeader` (reading `byte_len`, copying the UTF-8 bytes, …) will
/// segfault or return garbage on SSO inputs.
///
/// SSO-vulnerable callsites must route through [`unbox_str_handle`].
/// Issue #214 lineage: `Array.indexOf`, every `String.prototype.*` method,
/// `arr.join(sep)`, `obj[dynamicKey]`, `string.match(re)`, crypto digest
/// inputs, `process.env[name]` — all previously segfaulted on SSO operands
/// before being routed through the safe helper.
pub(crate) fn unbox_to_i64(blk: &mut LlBlock, boxed: &str) -> String {
    let bits = blk.bitcast_double_to_i64(boxed);
    blk.and(I64, &bits, POINTER_MASK_I64)
}

/// Built-in constructor / namespace names that the runtime pre-populates
/// on the globalThis singleton (`populate_global_this_builtins` in
/// crates/perry-runtime/src/object.rs). Used by codegen to decide whether
/// `globalThis.<Name>` should route through `js_get_global_this`
/// (returning the populated backing-object) or fall through to the `0.0`
/// no-value placeholder. Keep this list in sync with
/// `GLOBAL_THIS_BUILTIN_CONSTRUCTORS` + `GLOBAL_THIS_BUILTIN_FUNCTIONS`
/// + `GLOBAL_THIS_BUILTIN_NAMESPACES` + the singleton `globalThis`
/// self-reference in object.rs — the codegen check and the runtime
/// population together implement the lodash `runInContext` blocker fix.
pub(crate) fn is_global_this_builtin_name(name: &str) -> bool {
    matches!(
        name,
        // Constructors (typeof === "function" in spec).
        "Array"
            | "Object"
            | "String"
            | "Number"
            | "Boolean"
            | "Function"
            | "RegExp"
            | "Date"
            | "Error"
            | "TypeError"
            | "RangeError"
            | "SyntaxError"
            | "ReferenceError"
            | "EvalError"
            | "URIError"
            | "AggregateError"
            | "Symbol"
            | "Promise"
            | "Map"
            | "Set"
            | "WeakMap"
            | "WeakSet"
            | "WeakRef"
            | "Proxy"
            | "BigInt"
            | "Uint8Array"
            | "Int8Array"
            | "Uint16Array"
            | "Int16Array"
            | "Uint32Array"
            | "Int32Array"
            | "Float16Array"
            | "Float32Array"
            | "Float64Array"
            | "Uint8ClampedArray"
            | "BigInt64Array"
            | "BigUint64Array"
            | "ArrayBuffer"
            | "SharedArrayBuffer"
            | "DataView"
            | "TextEncoder"
            | "TextDecoder"
            | "TextEncoderStream"
            | "TextDecoderStream"
            | "CompressionStream"
            | "DecompressionStream"
            // The three core Web Streams constructors. Perry already implements
            // them (`new ReadableStream(…)` and `x instanceof ReadableStream` are
            // both lowered, and `ReadableStream` has a class id), but the NAMES
            // were never registered as globalThis builtins — so a bare
            // `ReadableStream` identifier did not resolve: `typeof
            // ReadableStream === "undefined"` and `"ReadableStream" in globalThis`
            // was false, while Node exposes all three as functions.
            //
            // Libraries feature-detect exactly this (`typeof ReadableStream !==
            // "undefined" ? … : …`) to decide how to consume a `fetch()` body.
            // Getting "undefined" sent a large esbuild-bundled CLI app down the
            // wrong branch, which then threw "The first argument must be a
            // Readable, a ReadableStream, or an async iterable" and aborted its
            // background tar-stream downloads entirely.
            | "ReadableStream"
            | "WritableStream"
            | "TransformStream"
            | "Navigator"
            | "URL"
            | "URLSearchParams"
            | "URLPattern"
            | "AbortController"
            | "AbortSignal"
            | "EventTarget"
            | "Crypto"
            | "CryptoKey"
            | "SubtleCrypto"
            | "Event"
            | "CustomEvent"
            | "DOMException"
            | "FormData"
            | "Blob"
            | "File"
            | "Headers"
            | "Request"
            | "Response"
            | "MessageChannel"
            | "MessagePort"
            | "BroadcastChannel"
            | "Storage"
            | "WebSocket"
            | "FinalizationRegistry"
            | "Performance"
            | "PerformanceEntry"
            | "PerformanceMark"
            | "PerformanceMeasure"
            | "PerformanceObserver"
            | "PerformanceObserverEntryList"
            | "PerformanceResourceTiming"
            // #2875: TC39 explicit-resource-management global constructors.
            | "DisposableStack"
            | "AsyncDisposableStack"
            | "SuppressedError"
            | "Buffer"
            // Global functions (typeof === "function" in spec).
            | "eval"
            | "fetch"
            | "structuredClone"
            | "atob"
            | "btoa"
            | "setTimeout"
            | "clearTimeout"
            | "setInterval"
            | "clearInterval"
            | "setImmediate"
            | "clearImmediate"
            | "queueMicrotask"
            // #2905: standard global helper functions (typeof === "function").
            | "parseInt"
            | "parseFloat"
            | "isNaN"
            | "isFinite"
            | "encodeURI"
            | "decodeURI"
            | "encodeURIComponent"
            | "decodeURIComponent"
            // #4511: legacy escape/unescape (ES Annex B).
            | "escape"
            | "unescape"
            // Test262 installs this dynamically on globalThis. Route reads
            // through the singleton so bare `print(...)` can call it once the
            // harness assigns the property.
            | "print"
            // Namespaces (typeof === "object" in spec).
            | "globalThis"
            | "global"
            | "console"
            | "Math"
            | "JSON"
            | "Reflect"
            | "Atomics"
            | "Intl"
            | "WebAssembly"
            | "Temporal"
            | "performance"
            | "process"
            | "navigator"
            | "crypto"
            | "localStorage"
            | "sessionStorage"
    )
}

/// Subset of `is_global_this_builtin_name` whose `typeof` is `"function"`
/// in spec (constructors). Used by the `Expr::TypeOf` short-circuit so
/// `typeof globalThis.Array === "function"`. Math/JSON/Reflect are
/// namespaces — they keep `typeof === "object"` via the existing match
/// arms.
pub(crate) fn is_global_this_builtin_function_name(name: &str) -> bool {
    is_global_this_builtin_name(name)
        && !matches!(
            name,
            "globalThis"
                | "global"
                | "console"
                | "Math"
                | "JSON"
                | "Reflect"
                | "Intl"
                | "WebAssembly"
                | "Temporal"
                | "performance"
                | "process"
                | "navigator"
                | "print"
                | "crypto"
                | "localStorage"
                | "sessionStorage"
        )
}

/// SSO-safe variant of `unbox_to_i64` for NaN-boxed string operands.
///
/// The plain `unbox_to_i64(bitcast double → i64; and POINTER_MASK_I64)`
/// pattern returns the lower 48 bits, which is the correct
/// `*StringHeader` for heap strings (STRING_TAG = 0x7FFF) but is
/// **garbage** for short-string-optimization (SSO) values
/// (SHORT_STRING_TAG = 0x7FF9), whose lower 48 bits encode the inline
/// length + bytes. Any consumer that dereferences the result —
/// `js_string_concat`, `js_string_equals`, `js_string_to_lower_case`,
/// the on-the-wire StringHeader length field, etc. — segfaults at a
/// pseudo-random address built from the inline payload bytes.
///
/// Issue #214: `string[]` element loads (e.g. `JSON.parse('["hello"]')[0]`)
/// returned SSO bits, then `arr[0] + "x"` / `arr[0] === "hello"` /
/// `arr[0].toUpperCase()` segfaulted on the inline mask. This helper
/// routes through `js_get_string_pointer_unified`, which materializes
/// SSO values to a real heap StringHeader (one allocation per SSO unbox)
/// while preserving the heap-string fast path internally.
pub(crate) fn unbox_str_handle(blk: &mut LlBlock, boxed: &str) -> String {
    blk.call(I64, "js_get_string_pointer_unified", &[(DOUBLE, boxed)])
}
