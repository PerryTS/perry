//! Small self-contained codegen helpers (arg-array marshalling, NaN-box
//! unboxing, globalThis builtin-name tables) extracted from `expr.rs`,
//! issue #1098. Pure move — no logic changes.

use anyhow::Result;
use perry_hir::types::Type as HirType;
use perry_hir::{BinaryOp, Expr, UnaryOp};

use super::{lower_expr, FnCtx};
use crate::block::LlBlock;
use crate::nanbox::POINTER_MASK_I64;
use crate::types::{DOUBLE, I64};

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

/// The expression's shape IS an allocation site: an object / array / closure
/// literal, or a `new`.
///
/// Read the claim precisely, because the `Expr::New` arm makes it weaker than
/// [`expr_produces_non_pointer_bits_by_construction`]'s mirror image. What a
/// `new C()` *evaluates to* is a runtime question — a constructor return
/// override (`js_ctor_return_override`) can hand back anything, including a
/// heap string. So this is **not** a proof that the stored bits carry
/// `POINTER_TAG`; it is a proof that the expression's purpose is to allocate
/// one.
///
/// That is exactly, and only, what its two consumers need:
///
/// - **Picking arrays worth declaring all-pointer**
///   (`collectors/all_pointer_arrays.rs`). A wrong guess costs scan time —
///   `GC_LAYOUT_ALL_POINTERS` visits the same slots `GC_LAYOUT_UNKNOWN` does,
///   and a non-pointer word is re-validated and rejected — never a stranded
///   child.
/// - **Eliding the per-push layout / numeric-write notes**
///   (`expr/array_push.rs`). Neither elision rests on this predicate at all:
///   both are proven by the header test emitted at the store.
///
/// It must NEVER gate `js_string_addref_if_heap_string`. That demote is dead
/// only when the value provably is not a heap string — a claim the `New` arm
/// does not make, and whose absence is silent corruption rather than a crash
/// (a refcount==1 string aliased from an array slot, rewritten in place by a
/// later `+=` on the source local). `array_push.rs` gates it separately on
/// [`store_needs_string_addref`].
///
/// The `New` arm is load-bearing rather than a widening for its own sake: HIR
/// rewrites a closed-shape object literal into
/// `New { class_name: "__AnonShape_<hash>", … }` before codegen ever sees it,
/// so without it the predicate misses the object-literal push loop this whole
/// analysis exists for.
///
/// Takes no `FnCtx`: the test is purely syntactic, so the per-region fact
/// collector (which runs before any lowering context exists) and the lowering
/// site consult the same function and can never disagree about a store.
pub(crate) fn expr_produces_fresh_heap_allocation(expr: &Expr) -> bool {
    match expr {
        Expr::Object(_) | Expr::Array(_) | Expr::Closure { .. } | Expr::New { .. } => true,
        Expr::Conditional {
            then_expr,
            else_expr,
            ..
        } => {
            expr_produces_fresh_heap_allocation(then_expr)
                && expr_produces_fresh_heap_allocation(else_expr)
        }
        Expr::Sequence(exprs) => exprs
            .last()
            .is_some_and(expr_produces_fresh_heap_allocation),
        _ => false,
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
/// A pointer-valued store into a pointer-masked slot is handled separately, by
/// [`class_field_store_layout_note_is_conforming`] — see there.
pub(crate) fn class_field_store_needs_layout_note(ctx: &FnCtx<'_>, value: &Expr) -> bool {
    !expr_produces_non_pointer_bits_by_construction(ctx, value)
}

/// Phase 4b.2 (#5094, refs #7510): is this class-field store's layout note a
/// *provable no-op whenever the receiver carries an intact side-mask
/// descriptor*?
///
/// [`class_field_store_needs_layout_note`] above elides the note for a value
/// that is a non-pointer by construction. The complementary case — a **pointer
/// stored into a slot the class's own compile-time pointer mask declares** —
/// was deliberately left un-elided, because "the receiver has a descriptor" was
/// not total: `lower_new_impl`'s standalone-ctor-symbol branch could return a
/// freshly allocated instance with none, sitting at `GC_LAYOUT_POINTER_FREE`,
/// where the note is the only thing that ever sets the pointer-mask bit the
/// collector reads. **#6921 closed that exit** (`lower_call/new.rs` now emits
/// the init there too), so the elision is available — but this returns only
/// *elidable under a header test*, not *elidable outright*, and the emitter
/// pairs it with that test. A descriptor-less receiver from any path this
/// reasoning did not enumerate still takes the full note.
///
/// The test the emitter pairs this with is
/// `_reserved & (STATE_MASK | TYPED_LAYOUT_INTACT) == SIDE_MASK | INTACT`, and
/// together the two prove `layout_note_slot` would return `Conforms` without
/// touching anything:
///
/// * The `#5093` inline precheck has already proven, on this path, that the
///   receiver's `keys_array` equals **this class's** keys global and its
///   `field_count` exceeds the slot index. So the descriptor reachable for it
///   was installed from this class's mask globals — shared by shape under that
///   same key (`SHAPE_LAYOUTS`), or per-object if that key was poisoned
///   ambiguous, and in both cases from these same words.
/// * `slot` is in that mask's `pointer_mask` and (checked in
///   [`crate::typed_shape::layout_declares_pointer_slot`]) not in its
///   `raw_f64_mask`, so neither `layout_note_slot` downgrade arm can fire: the
///   raw-f64 arm is not this slot's, and the pointer arm's condition is
///   `!pointer_mask.contains(slot)`.
/// * `INTACT` set is exactly the runtime's own invariant that *some* descriptor
///   is reachable; `SIDE_MASK` is the state a non-empty pointer mask installs.
///   A cleared bit or any other state routes to the real note, which is the
///   pre-change behaviour.
///
/// Deliberately keyed on the DECLARED field type, not on the value: the value
/// is already known pointer-bearing at this point (the emitter's live
/// `may_carry_heap_pointer` test gates the whole bookkeeping block), and the
/// mask is what the collector will read.
pub(crate) fn class_field_store_layout_note_is_conforming(
    ctx: &FnCtx<'_>,
    class_name: &str,
    field_index: u32,
) -> bool {
    // No keys global ⟹ no mask globals are emitted for this class and no
    // descriptor is ever installed, so the header test could never pass. Skip
    // the extra IR rather than emit a branch that is always taken.
    if !ctx.class_keys_globals.contains_key(class_name) {
        return false;
    }
    let layout = ctx
        .class_init_chains
        .get(class_name)
        .map(|chain| crate::typed_shape::class_typed_layout_from_chain(chain))
        .unwrap_or_else(|| crate::typed_shape::class_typed_layout(ctx.classes, class_name));
    crate::typed_shape::layout_declares_pointer_slot(&layout, field_index)
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
    store_needs_string_addref(ctx, value)
}

/// Receiver-independent form of [`class_field_store_needs_string_addref`]: the
/// demote is keyed on the stored value alone, so an array element store asks
/// exactly the same question. Used by the #7469 elided array push, which drops
/// the layout note but must keep this call whenever the pushed value could be
/// a heap string (a `new C()` with a constructor return override can be one).
pub(crate) fn store_needs_string_addref(ctx: &FnCtx<'_>, value: &Expr) -> bool {
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

/// #7469 — emit the at-allocation all-pointer element-layout declaration for
/// `id`, when this region's fact graph admits it and `init_expr` really is the
/// array literal the fact was proven against.
///
/// Called from the `Stmt::Let` tail with the lowered (NaN-boxed) initializer,
/// which for an array literal is the fresh array's pointer. Re-testing
/// `Expr::Array` here rather than trusting the id alone keeps the emission
/// pinned to the single binding the collector proved: a fact that somehow named
/// an id bound by something else emits nothing, instead of declaring an element
/// layout for the wrong object.
pub(crate) fn emit_all_pointer_array_declaration(
    ctx: &mut FnCtx<'_>,
    id: u32,
    init_expr: &Expr,
    init_value: &str,
) {
    if init_value.is_empty()
        || !matches!(init_expr, Expr::Array(_))
        || !ctx.native_facts.declares_all_pointer_elements(id)
    {
        return;
    }
    let blk = ctx.block();
    let handle = super::unbox_to_i64(blk, init_value);
    blk.call_void("js_array_declare_all_pointer_elements", &[(I64, &handle)]);
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

// `proxy_build_args_array` lived here and was deleted by #7615 slice 7. It
// threaded the array's raw `*mut ArrayHeader` through its push loop in a bare
// SSA register while each element — arbitrary user code — was lowered, which is
// #7154's accumulator bug, and it had no way to root the CALLER's receiver
// across the same loop. Its four call sites now build the array inside a
// `crate::rooting::RootedGroup` that holds the receiver too
// (`expr::proxy_reflect::build_args_array`).

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
