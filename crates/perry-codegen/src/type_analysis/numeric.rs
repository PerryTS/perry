//! Numeric / bigint / boolean static-type predicates.
//!
//! Split out of `type_analysis.rs` (file-size gate). Pure code move.

use super::*;

use perry_hir::types::Type as HirType;
use perry_hir::{BinaryOp, Expr, UnaryOp};

use crate::expr::FnCtx;

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
        Expr::LocalGet(id) => matches!(ctx.stable_local_type_proof(id), Some(HirType::BigInt)),
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

/// `String.prototype` methods whose lowering in `lower_string_method.rs`
/// produces a **real f64** on every input — either `sitofp` of an `i32`
/// helper result, or a helper documented to return a plain double.
///
/// Deliberately excluded, and why each one would be wrong here:
/// * `codePointAt` — returns `undefined` (a NaN-BOX tag, not a number) for an
///   out-of-range index; `js_string_code_point_at` is documented as
///   "NaN-boxed number **or undefined**".
/// * `at` / `charAt` — string results.
/// * `startsWith` / `endsWith` / `includes` — booleans (NaN-boxed tags).
///
/// `charCodeAt` IS admitted: its out-of-range result is `f64::NAN`
/// (`0x7FF8_0000_0000_0000`), a hardware quiet NaN outside the NaN-box tag
/// band `0x7FF9..=0x7FFF`, so it is a genuine Number in exactly the sense
/// `is_numeric_expr` promises.
fn string_method_returns_number(name: &str) -> bool {
    matches!(
        name,
        "charCodeAt" | "indexOf" | "lastIndexOf" | "search" | "localeCompare"
    )
}

/// True when `<object>.<property>(…)` is one of the Number-returning
/// `String.prototype` methods AND the receiver takes codegen's proven-string
/// lowering path.
///
/// The receiver test mirrors `lower_call/property_get.rs`'s static-String
/// dispatch condition exactly (`is_string_expr && !is_array_only_method_name
/// && is_known_string_method_name`), so the predicate can only answer `true`
/// for a call that really is lowered by `lower_string_method` — the
/// Any-typed-receiver fallback, which routes to `js_native_call_method` and
/// can return `undefined` for a non-string runtime value, is not claimed.
///
/// #7592: without this, `h ^ s.charCodeAt(i)` fails the "both operands are
/// statically primitive" test in `expr/binary.rs` and every iteration of an
/// FNV-style hash loop pays a `js_dynamic_bitxor` FFI call — 31% of the leaf
/// profile of `json_pipeline`'s hash phase, for an integer xor.
fn string_method_call_returns_number(ctx: &FnCtx<'_>, object: &Expr, property: &str) -> bool {
    string_method_returns_number(property)
        && crate::type_analysis::is_string_expr(ctx, object)
        && !crate::lower_call::property_get::is_array_only_method_name(property)
        && crate::lower_string_method::is_known_string_method_name(property)
}

pub(crate) fn is_numeric_expr(ctx: &FnCtx<'_>, e: &Expr) -> bool {
    match e {
        Expr::Integer(_)
        | Expr::Number(_)
        | Expr::PodLayoutSizeOf { .. }
        | Expr::PodLayoutAlignOf { .. }
        | Expr::PodLayoutOffsetOf { .. } => true,
        // #7700: a `Uint8ArrayGet` is a BYTE read only when its key is numeric.
        // This is the very test `arrays_finds::lower_uint8array_get_i32` applies
        // to choose between the byte accessor and
        // `js_object_get_index_polymorphic`, so the two cannot disagree about
        // whether `u8[Symbol.iterator]` is a number — which matters wherever a
        // `true` here means "a raw double": `fcmp`-based truthiness, `fadd`
        // operands, the non-BigInt bitwise fast path.
        Expr::Uint8ArrayGet { index, .. } => is_numeric_expr(ctx, index),
        Expr::BufferIndexGet { .. } | Expr::Uint8ArrayLength(_) | Expr::BufferLength(_) => true,
        Expr::LocalGet(id) => {
            ctx.element_shape_loop_facts
                .iter()
                .rev()
                .any(|fact| fact.numeric_accumulator == *id)
                || ctx.integer_locals.contains(id)
                || ctx.unsigned_i32_locals.contains(id)
                || ctx.int_valued_i64_locals.contains_key(id)
                || matches!(
                    ctx.stable_local_type_proof(id),
                    Some(HirType::Number) | Some(HirType::Int32)
                )
                // #8105: the reassignment-tolerant proof. Every arm above
                // either needs the local to be write-once
                // (`stable_local_type_proof` answers `None` the moment it is
                // reassigned) or is an integer-range fact, so a plain
                // fractional accumulator — `let x = 0.0; … x = x * x - y * y
                // + cx` — had NO numeric proof and every `x * x` bailed to
                // the BigInt-aware `js_dynamic_mul`. This set proves the
                // value is a Number from the WRITES, so reassignment is fine.
                || ctx.number_by_construction_locals.contains(id)
        }
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
        // `x++`/`x--`/`++x`/`--x` evaluates to `ToNumeric(x) ± 1`, a Number —
        // EXCEPT when `x` is a BigInt, where it stays a BigInt (`5n++` → `6n`).
        // So this is NOT unconditionally numeric: mirror the `LocalGet` arm's
        // type check, and additionally honor the integer-range collectors so an
        // `Any`-typed loop counter proven to hold an i32 stays on the numeric
        // fast path. An `Any` local NOT proven integer could hold a BigInt and
        // is excluded — otherwise `(bigintLocal++) & x` would take the
        // all-numeric operand path (which `toint32`s the BigInt to 0) instead
        // of throwing the spec-mandated TypeError, and `(bigintLocal++) * 2`
        // would silently produce NaN.
        Expr::Update { id, .. } => {
            matches!(
                ctx.stable_local_type_proof(id),
                Some(HirType::Number) | Some(HirType::Int32)
            ) || ctx.integer_locals.contains(id)
                || ctx.unsigned_i32_locals.contains(id)
        }
        Expr::DateNow => true,
        // Math.* builtins always evaluate to a real numeric double: every
        // lowering coerces its operands internally (ToNumber via
        // `lower_math_operand` / `js_math_to_number` — BigInt and Symbol
        // throw) and emits a raw-f64-returning intrinsic or runtime helper,
        // never a NaN-tagged value. Without these arms, `Math.sqrt(x) *
        // Math.sin(y)` fails the "statically numeric" test and the multiply
        // is routed through the BigInt-aware dynamic helper — two non-leaf
        // re-coercion calls per operation instead of an inline `fmul`
        // (#6511, a ~1.45x regression introduced by the #5970 routing).
        Expr::MathFloor(..)
        | Expr::MathCeil(..)
        | Expr::MathRound(..)
        | Expr::MathTrunc(..)
        | Expr::MathSign(..)
        | Expr::MathAbs(..)
        | Expr::MathSqrt(..)
        | Expr::MathLog(..)
        | Expr::MathLog2(..)
        | Expr::MathLog10(..)
        | Expr::MathPow(..)
        | Expr::MathMin(..)
        | Expr::MathMax(..)
        | Expr::MathMinSpread(..)
        | Expr::MathMaxSpread(..)
        | Expr::MathImul(..)
        | Expr::MathRandom
        | Expr::MathSin(..)
        | Expr::MathCos(..)
        | Expr::MathTan(..)
        | Expr::MathAsin(..)
        | Expr::MathAcos(..)
        | Expr::MathAtan(..)
        | Expr::MathAtan2(..)
        | Expr::MathCbrt(..)
        | Expr::MathHypot(..)
        | Expr::MathFround(..)
        | Expr::MathF16round(..)
        | Expr::MathClz32(..)
        | Expr::MathExpm1(..)
        | Expr::MathLog1p(..)
        | Expr::MathSinh(..)
        | Expr::MathCosh(..)
        | Expr::MathTanh(..)
        | Expr::MathAsinh(..)
        | Expr::MathAcosh(..)
        | Expr::MathAtanh(..)
        | Expr::MathExp(..) => true,
        // Unary `-x` / `+x` / `~x` always evaluate to a JS number by
        // ToNumber/ToInt32 semantics, so the result feeds the native f64
        // path (#5497, Lever E). The unary lowering coerces the operand
        // internally (its own `numeric` flag already factors in the
        // raw-f64 boxed-fallback hazard), so the produced value is a clean
        // f64 regardless of the operand's runtime shape — no downstream
        // coerce is needed. BigInt is the sole exception: `-1n` / `~1n`
        // stay BigInt (their lowering routes through `js_dynamic_neg` /
        // `js_dynamic_bitnot`, which preserve the BigInt tag), so a bigint
        // operand must not be treated as numeric. (`!x` is a boolean, not
        // a number — handled by `is_bool_expr`.)
        Expr::Unary { op, operand } => {
            matches!(op, UnaryOp::Neg | UnaryOp::Pos | UnaryOp::BitNot)
                && !is_bigint_expr(ctx, operand)
        }
        // Explicit numeric-coercion node — lowers to `js_number_coerce`,
        // which always yields a clean f64.
        Expr::NumberCoerce(_) => true,
        // `a || b` / `a && b` / `a ?? b` select ONE OF THE OPERAND VALUES —
        // never a synthesized value — so the result is numeric when BOTH
        // operands are (repsel Phase 4a.0, #6904: `(counts[v] || 0) + 1`
        // previously routed the whole Add through
        // `js_dynamic_string_or_number_add`).
        //
        // Soundness notes:
        // * A possibly-string / union / bool operand fails `is_numeric_expr`
        //   on that side, so `x || "fallback"` and `s && n` stay non-numeric
        //   (the `1 + "foo"` concat hazard keeps the dynamic-add bail).
        // * A proven-numeric operand can still surface a BOXED value at
        //   runtime through a raw-f64 array/field read's cold fallback (a
        //   hole reads `undefined`). That hazard is tracked separately by
        //   `expr_may_return_boxed_value_from_raw_f64_fallback`, which gained
        //   the matching Logical arm — every consumer of `is_numeric_expr`
        //   that needs a REAL double (truthiness fcmp, fadd operands, raw
        //   stores) already consults it and inserts `js_number_coerce` /
        //   `js_is_truthy` on that path, and the runtime numeric-array SET
        //   guard independently rejects non-numeric VALUES, so a passed-
        //   through `undefined` still stores as `undefined` via the boxed
        //   fallback (hole-vs-undefined observability is preserved).
        Expr::Logical { left, right, .. } => {
            is_numeric_expr(ctx, left) && is_numeric_expr(ctx, right)
        }
        // `obj.field` where the field is declared as `number` on the
        // owning class. Without this, `this.value + 1` in a hot loop
        // wraps the field load in `js_number_coerce` which prevents
        // LLVM from doing GVN/LICM on the load. The class field
        // walker matches `class_field_global_index`'s inheritance
        // traversal so the type of any inherited field is also seen.
        Expr::PropertyGet {
            object, property, ..
        } => {
            if matches!(
                crate::lower_call::guarded_path_type(ctx, e),
                Some(HirType::Number | HirType::Int32)
            ) {
                return true;
            }
            if property == "length" && expression_has_numeric_length(ctx, object) {
                return true;
            }
            // repsel #7480 step 3: inside an element-shape fast clone a tracked
            // `arr[i].field` read is a GUARD-PROVEN raw double — the preheader
            // pinned the element class and the per-element residual check
            // requires `GC_OBJ_TYPED_LAYOUT_INTACT`, so the slot cannot hold a
            // NaN-boxed value. This is a stronger proof than the declared-type
            // answer below, and it is the ONLY one available for an
            // object-literal element type, whose owner class
            // `receiver_class_name` deliberately does not resolve.
            //
            // It is also load-bearing rather than a bonus: without it
            // `sum += keep[j].v` lowers through
            // `js_dynamic_string_or_number_add`, that call fails the clone's
            // call-free admission test, and the clone is never entered at all.
            // Scoped to the clone — outside one the fact vector is empty.
            if crate::expr::element_shape_loop_fact_for_property_get(ctx, object, property)
                .is_some()
            {
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
        Expr::IndexGet { object, index } => {
            // #6750 follow-up: a masked-index read covered by an ACTIVE
            // masked-window fact (dense range-loop / straight-line-region
            // fast copy) is a guard-proven numeric element load, even when
            // the receiver's STATIC type is erased (`any` parameter).
            // Without this, `n ^= S[x & 0xff]` inside a fast copy still
            // routed through the BigInt-aware dynamic helpers. Facts are
            // scope-managed by the versioned lowerings, so the answer is
            // only `true` while a fast copy that proved the window is being
            // lowered. The fact itself proves the index is an integer, so it
            // stands ahead of the index check below.
            if let Expr::LocalGet(arr_id) = object.as_ref() {
                if crate::expr::masked_window_fact_for_index(ctx, *arr_id, index).is_some() {
                    return true;
                }
            }
            // #7796: an element type only describes reads at a NUMERIC index.
            // `a[sym]` on a `number[]` is not an element read at all — it is a
            // property read on the array OBJECT, and `a[Symbol.iterator]`
            // answers with a function.
            //
            // Getting this wrong is not merely a missed optimization. The
            // caller acts on "this is a number" by testing the raw double with
            // `fcmp one %v, 0.0`, and every NaN-boxed pointer IS a NaN, so that
            // comparison is false for every object, string and function alive.
            // `if (a[Symbol.iterator])` therefore took the FALSE branch on a
            // value that `Boolean()` and `typeof` both agreed was a function.
            //
            // Proof is required rather than merely the absence of counter-
            // evidence: an index we cannot type may hold anything at runtime,
            // and answering `false` here costs a fast path while answering
            // `true` costs a wrong branch.
            if !is_numeric_expr(ctx, index) {
                return false;
            }
            if receiver_class_name(ctx, object)
                .as_deref()
                .is_some_and(is_numeric_typed_array_class)
            {
                return true;
            }
            let Expr::LocalGet(arr_id) = object.as_ref() else {
                return false;
            };
            if ctx.native_facts.num_array_local(*arr_id).is_some() {
                return true;
            }
            // #8225: tracked BufferViewSlots come from compiler-owned
            // Buffer/typed-array constructors (including NativeArena views),
            // and every representable element kind is numeric. This runtime
            // fact is stronger than the erasable declaration consulted below.
            if ctx
                .buffer_view_slots
                .get(arr_id)
                .is_some_and(|view| view.elem.is_number_valued())
            {
                return true;
            }
            match ctx.stable_local_type_proof(arr_id) {
                Some(HirType::Array(elem)) => {
                    matches!(**elem, HirType::Number | HirType::Int32)
                }
                // #6011: `new Array<number>(n)` locals carry the generic
                // spelling `Generic { base: "Array", type_args: [Number] }`;
                // element reads are numeric exactly like `Array(Number)`.
                Some(HirType::Generic { base, type_args })
                    if base == "Array" && type_args.len() == 1 =>
                {
                    matches!(type_args[0], HirType::Number | HirType::Int32)
                }
                Some(HirType::Named(name)) => is_numeric_typed_array_class(name),
                _ => false,
            }
        }
        // User function calls returning Number: skip js_number_coerce.
        // Without this, `fib(n-1) + fib(n-2)` wraps both results in
        // js_number_coerce — ~4 billion wasted runtime calls on fib(40).
        Expr::Call { callee, .. } => {
            if let Expr::PropertyGet {
                object, property, ..
            } = callee.as_ref()
            {
                if string_method_call_returns_number(ctx, object, property) {
                    return true;
                }
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

/// Repsel Phase 4a.0 (#6904): statically prove that an expression's LOWERED
/// value is a **canonical raw f64** — a real machine double whose bit pattern
/// is never a NaN-box tag (`0x7FF9..=0x7FFF` upper 16 with a set quiet-NaN
/// payload in tag space). Such a value may be stored into a raw-f64 numeric
/// array slot verbatim, skipping the `js_array_numeric_value_to_raw_f64`
/// canonicalization call (whose INT32-unbox + `is_class_id_registered`
/// registry probe cannot fire for a value this predicate admits).
///
/// SOUNDNESS CONTRACT — a `true` return authorizes a raw slot store with no
/// runtime value check, so the value must be a real number for EVERY input:
/// * never NaN-boxed (INT32/STRING/POINTER/BIGINT/UNDEFINED/HOLE tags), and
/// * any NaN it produces must carry a non-tag payload. Arithmetic on
///   canonical inputs propagates canonical NaNs (hardware qNaN `0x7FF8…`, or
///   a sign-flipped `0xFFF8…` through `fneg` — both outside tag space), and
///   every operand feeding these lowerings is itself coerced/canonical, so
///   the property holds inductively.
///
/// Structurally admitted:
/// * numeric literals;
/// * `Binary` arithmetic/bitwise when the whole node is `is_numeric_expr`, is
///   `is_provably_not_bigint`, and does not inherit a declared-only numeric
///   proof (a possibly-BigInt chain routes through BIGINT-boxed dynamic
///   helpers, while a declared-only `+` may return a boxed string);
/// * `Unary` Neg/Pos/BitNot over a non-BigInt operand;
/// * `Update` (++/--) when numeric per `is_numeric_expr`;
/// * the `Math.*` family / `Date.now` (Rust-computed f64s);
/// * explicit `NumberCoerce`;
/// * `Logical` selections whose BOTH operands are themselves canonical.
///
/// Deliberately NOT admitted: `LocalGet` (a Number-typed local can hold an
/// INT32-boxed value assigned from a boxed read fallback), reads
/// (`IndexGet`/`PropertyGet` — cold fallbacks return boxed bits), and calls.
pub(crate) fn expr_produces_canonical_raw_f64(ctx: &FnCtx<'_>, e: &Expr) -> bool {
    match e {
        Expr::Integer(_) | Expr::Number(_) => true,
        Expr::Binary { .. } => {
            is_numeric_expr(ctx, e)
                && is_provably_not_bigint(ctx, e)
                && !crate::type_analysis::numeric_proof_is_declared_only(ctx, e)
        }
        Expr::Unary { op, operand } => {
            matches!(op, UnaryOp::Neg | UnaryOp::Pos | UnaryOp::BitNot)
                && is_provably_not_bigint(ctx, operand)
        }
        Expr::Update { .. } => is_numeric_expr(ctx, e),
        Expr::NumberCoerce(_) => true,
        Expr::Logical { left, right, .. } => {
            expr_produces_canonical_raw_f64(ctx, left)
                && expr_produces_canonical_raw_f64(ctx, right)
        }
        Expr::MathFloor(..)
        | Expr::MathCeil(..)
        | Expr::MathRound(..)
        | Expr::MathTrunc(..)
        | Expr::MathSign(..)
        | Expr::MathAbs(..)
        | Expr::MathSqrt(..)
        | Expr::MathLog(..)
        | Expr::MathLog2(..)
        | Expr::MathLog10(..)
        | Expr::MathPow(..)
        | Expr::MathMin(..)
        | Expr::MathMax(..)
        | Expr::MathMinSpread(..)
        | Expr::MathMaxSpread(..)
        | Expr::MathImul(..)
        | Expr::MathRandom
        | Expr::MathSin(..)
        | Expr::MathCos(..)
        | Expr::MathTan(..)
        | Expr::MathAsin(..)
        | Expr::MathAcos(..)
        | Expr::MathAtan(..)
        | Expr::MathAtan2(..)
        | Expr::MathCbrt(..)
        | Expr::MathHypot(..)
        | Expr::MathFround(..)
        | Expr::MathF16round(..)
        | Expr::MathClz32(..)
        | Expr::MathExpm1(..)
        | Expr::MathLog1p(..)
        | Expr::MathSinh(..)
        | Expr::MathCosh(..)
        | Expr::MathTanh(..)
        | Expr::MathAsinh(..)
        | Expr::MathAcosh(..)
        | Expr::MathAtanh(..)
        | Expr::MathExp(..)
        | Expr::DateNow => true,
        _ => false,
    }
}

/// Statically prove that an expression's runtime value can **never** be a
/// BigInt.
///
/// Consumed by the bitwise-op lowering (`expr/binary.rs`): a bitwise op on two
/// operands neither of which can `ToNumeric` to a BigInt is safe to lower
/// inline (`ToInt32 <op> ToInt32 + sitofp`) instead of bailing to the
/// BigInt-aware `js_dynamic_bit*` runtime helper.
///
/// SOUNDNESS CONTRACT — a `true` return authorizes skipping the dynamic
/// helper. That helper is exactly what makes `bigint <op> number` throw the
/// spec-mandated `TypeError` and `bigint <op> bigint` compute a BigInt. So
/// this MUST only return `true` when the operand's `ToNumeric` provably cannot
/// yield a BigInt; when unsure, return `false` (keep the safe bail). The
/// bitwise lowering additionally requires BOTH operands to pass before
/// inlining, so a possibly-BigInt operand on EITHER side still preserves the
/// throw / bigint result.
pub(crate) fn is_provably_not_bigint(ctx: &FnCtx<'_>, e: &Expr) -> bool {
    // Check statically-BigInt FIRST — a defensive short-circuit for anything
    // `is_bigint_expr` can prove is (or involves) a BigInt: BigInt literals,
    // BigInt-typed locals, and BigInt arith/bitwise chains (`5n ^ 3n`,
    // `bigintLocal & mask`). Keeps the answer unambiguously `false` for those
    // before the structural rules below get a chance to reason about operands.
    if is_bigint_expr(ctx, e) {
        return false;
    }
    // Handle arithmetic/bitwise/unary nodes STRUCTURALLY, before the
    // `is_numeric_expr` shortcut below. `is_numeric_expr` blanket-treats every
    // non-`Add` binary and every `-x`/`+x`/`~x` unary as numeric — fine for its
    // own callers (they guard BigInt upstream), but it would over-approximate
    // here: `anyA ^ anyB` could be `bigint ^ bigint` (a BigInt result), yet
    // `is_bigint_expr` can't see it when both operands are `Any`. The
    // structural rules recurse into the operands instead.
    match e {
        // Every arithmetic / bitwise binary op yields a BigInt ONLY when BOTH
        // operands `ToNumeric` to BigInt (a mixed operand throws; a string
        // operand of `+` concatenates). So the result is provably non-BigInt as
        // soon as EITHER operand is. (`BinaryOp` has no non-arithmetic variants
        // — comparisons / logicals are separate `Expr`s.)
        Expr::Binary { left, right, .. } => {
            return is_provably_not_bigint(ctx, left) || is_provably_not_bigint(ctx, right);
        }
        // `!x` → boolean and `+x` → Number-or-throw (unary plus on a BigInt is a
        // `TypeError`, so it never yields a BigInt VALUE). `-x` / `~x` PRESERVE
        // BigInt, so they are non-BigInt only when the operand is.
        Expr::Unary { op, operand } => {
            return match op {
                UnaryOp::Not | UnaryOp::Pos => true,
                UnaryOp::Neg | UnaryOp::BitNot => is_provably_not_bigint(ctx, operand),
            };
        }
        // `x++` / `x--` / `++x` / `--x` PRESERVE the target's numeric kind
        // (`bigint++` stays BigInt, `number++` stays Number), so the update's
        // not-BigInt-ness IS the target local's — mirror the `LocalGet(id)` arm
        // below. This MUST run before the `is_numeric_expr` shortcut, which
        // treats every `Update` as numeric unconditionally and would otherwise
        // misclassify a BigInt local's `x++` as non-BigInt (`is_bigint_expr`
        // does not see through `Update`, so the guard above misses it too).
        Expr::Update { id, .. } => {
            return ctx.not_bigint_locals.contains(id)
                || ctx.integer_locals.contains(id)
                || ctx.unsigned_i32_locals.contains(id);
        }
        _ => {}
    }
    // Already-proven primitives: a real Number/Int32, a Boolean, or a String
    // are all definitionally non-BigInt.
    if is_numeric_expr(ctx, e) || is_bool_expr(ctx, e) || string_value_is_runtime_guaranteed(ctx, e)
    {
        return true;
    }
    match e {
        // Non-BigInt literals. `BigInt`/`BigIntCoerce` are deliberately absent.
        Expr::Undefined
        | Expr::Null
        | Expr::Number(_)
        | Expr::Integer(_)
        | Expr::Bool(_)
        | Expr::String(_)
        | Expr::WtfString(_) => true,

        // Definitionally non-BigInt value producers: `typeof` → string,
        // `void` → undefined, comparisons / `instanceof` / `in` / brand-checks
        // and the `Number.is*` / `isNaN` / `isFinite` family → boolean, and the
        // explicit numeric / string / boolean coercions.
        Expr::TypeOf(_)
        | Expr::Void(_)
        | Expr::Compare { .. }
        | Expr::InstanceOf { .. }
        | Expr::In { .. }
        | Expr::PrivateBrandCheck { .. }
        | Expr::NumberCoerce(_)
        | Expr::StringCoerce(_)
        | Expr::BooleanCoerce(_)
        | Expr::IsNaN(_)
        | Expr::IsFinite(_)
        | Expr::NumberIsNaN(_)
        | Expr::NumberIsFinite(_)
        | Expr::NumberIsInteger(_)
        | Expr::IsUndefinedOrBareNan(_)
        | Expr::DateNow => true,

        // Typed-array element reads yield Number | undefined (OOB), never a
        // BigInt. `Uint8ArrayGet` / `BufferIndexGet` are byte reads; a general
        // `IndexGet` qualifies only when the receiver is a numeric typed array
        // (a plain object / array element could hold a BigInt).
        // #7700: a byte read only with a numeric key. With any other key this
        // is a property read, and an expando holds anything — `u8.n = 1n;
        // const k: any = "n"; u8[k]` IS a BigInt.
        Expr::Uint8ArrayGet { index, .. } => is_numeric_expr(ctx, index),
        Expr::BufferIndexGet { .. } => true,
        Expr::IndexGet { object, .. } => receiver_class_name(ctx, object)
            .as_deref()
            .is_some_and(is_numeric_typed_array_class),

        // (`Expr::Binary` / `Expr::Unary` are handled structurally above.)

        // Ternary / logical selection: non-BigInt when every branch that can
        // become the result value is non-BigInt.
        Expr::Conditional {
            then_expr,
            else_expr,
            ..
        } => is_provably_not_bigint(ctx, then_expr) && is_provably_not_bigint(ctx, else_expr),
        Expr::Logical { left, right, .. } => {
            is_provably_not_bigint(ctx, left) && is_provably_not_bigint(ctx, right)
        }

        // A local proven never to hold a BigInt — even when its declared type
        // is erased to `Any`. This is what lets bcryptjs's Feistel accumulators
        // (`l` / `r`: init from an `Int32Array` element, then only ever `^=` /
        // `+=` updated) reach the inline path. `not_bigint_locals` is the
        // dedicated flow set (`collect_not_bigint_locals`); `integer_locals` /
        // `unsigned_i32_locals` prove an integer VALUE, which is strictly a JS
        // Number, so they imply non-BigInt too.
        Expr::LocalGet(id) => {
            ctx.not_bigint_locals.contains(id)
                || ctx.integer_locals.contains(id)
                || ctx.unsigned_i32_locals.contains(id)
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
/// - `Expr::LocalGet(id)` for locals pre-analyzed as integer-valued, either by
///   `collectors::collect_integer_locals` (i32-range: for-loop counters etc.)
///   or by `collectors::int_valued_i64_locals` (i64-range: literal-initialised
///   locals whose every write is a bounded constant translation)
/// - `Expr::Update { .. }` — `i++`/`i--`, whose value is always integer
///   if the underlying local is integer-valued
/// - `Expr::Binary { Add/Sub/Mul/Mod }` recursively when both operands are
///   integer-valued (closed under integer arithmetic; Div is excluded
///   because `1 / 2` is 0.5 in JS, not 0)
/// - bitwise ops: always integer by JS ToInt32 semantics
///
/// The result additionally guarantees the value fits `fptosi double -> i64`
/// — see `integer_magnitude_bits`, which this delegates to. `fptosi` is
/// **poison** when the operand is out of the target's range, so proving
/// integrality alone is not enough for the `%` lowering.
pub(crate) fn is_integer_valued_expr(ctx: &FnCtx<'_>, e: &Expr) -> bool {
    integer_magnitude_bits_inner(ctx, e, true).is_some_and(|bits| bits <= MAX_FPTOSI_I64_BITS)
}

/// The same judgment for the **divisor** of `%`, with the i64-range local set
/// deliberately withheld.
///
/// `srem(x, 0)` is UB in LLVM while JS requires `NaN`, and the caller's
/// `right_is_known_zero` guard only recognises a literal `0` — a *local* that
/// happens to hold `0` slips past it. The i64-range set admits exactly the
/// counters that walk through zero (`for (let d = 10; d >= 0; d--) … x % d`),
/// so letting it widen the divisor would introduce a UB window that
/// `integer_locals` alone does not open. The dividend has no such constraint.
pub(crate) fn is_integer_valued_divisor(ctx: &FnCtx<'_>, e: &Expr) -> bool {
    if matches!(
        e,
        Expr::LocalGet(id) | Expr::Update { id, .. }
            if ctx.int_valued_i64_locals.contains_key(id)
    ) {
        return false;
    }
    integer_magnitude_bits_inner(ctx, e, false).is_some_and(|bits| bits <= MAX_FPTOSI_I64_BITS)
}

/// Largest magnitude (as `log2`) an expression may have and still convert
/// exactly and in-range through `fptosi double -> i64`. `i64::MAX` is
/// `2^63 - 1`, so `2^62` leaves a full bit of headroom.
const MAX_FPTOSI_I64_BITS: u32 = 62;

/// Conservative upper bound on `log2(|value|)` for a provably integer-valued
/// expression; `None` when the expression is not provably integer-valued.
///
/// This is a magnitude *lattice*, not just an integrality predicate, because
/// the `%` fast path emits `fptosi double -> i64` and LLVM makes that **poison**
/// for an operand outside i64 range. A plain "is it an integer?" answer lets
/// `(a * b * c) % n` through even when the product needs 93 bits.
///
/// Leaf bounds:
/// - `integer_locals` — proven i32-range, so `|v| <= 2^31` → 31 bits.
/// - `int_valued_i64_locals` — per-local bound recorded by the collector
///   (56 bits for the common `+-1` step chain; see that module for the
///   IEEE-754 saturation proof that makes it a hard ceiling).
/// - bitwise results — `ToInt32` gives `|v| <= 2^31`; `>>>` is `ToUint32`,
///   so `|v| <= 2^32`.
/// - `Uint8ArrayGet` / `BufferIndexGet` — a byte, `|v| <= 2^8`.
///
/// Composition mirrors ordinary magnitude arithmetic: `Add`/`Sub` add a bit,
/// `Mul` adds the exponents, and `%` is bounded by the smaller operand
/// (`|a % b| <= min(|a|, |b|)`).
fn integer_magnitude_bits_inner(ctx: &FnCtx<'_>, e: &Expr, allow_i64_locals: bool) -> Option<u32> {
    let recurse = |sub: &Expr| integer_magnitude_bits_inner(ctx, sub, allow_i64_locals);
    match e {
        Expr::Integer(v) => Some(crate::collectors::ceil_log2_abs(*v)),
        // A byte value — #7700: only with a numeric key. A property read has no
        // magnitude bound at all.
        Expr::Uint8ArrayGet { index, .. } if is_numeric_expr(ctx, index) => Some(8),
        Expr::Uint8ArrayGet { .. } => None,
        Expr::BufferIndexGet { .. } => Some(8),
        Expr::LocalGet(id) | Expr::Update { id, .. } => {
            if ctx.integer_locals.contains(id) {
                Some(31)
            } else if allow_i64_locals {
                ctx.int_valued_i64_locals.get(id).copied()
            } else {
                None
            }
        }
        Expr::Binary { op, left, right } => match op {
            BinaryOp::Add | BinaryOp::Sub => {
                let l = recurse(left)?;
                let r = recurse(right)?;
                Some(l.max(r).saturating_add(1))
            }
            BinaryOp::Mul => {
                let l = recurse(left)?;
                let r = recurse(right)?;
                Some(l.saturating_add(r))
            }
            // `|a % b| <= min(|a|, |b|)`. Admitted only against a NON-ZERO
            // integer literal divisor: `x % 0` is NaN, and `fptosi(NaN)` is
            // poison, so a nested `%` by a possibly-zero divisor must not be
            // treated as an integer.
            BinaryOp::Mod => match right.as_ref() {
                Expr::Integer(d) if *d != 0 => {
                    let l = recurse(left)?;
                    Some(l.min(crate::collectors::ceil_log2_abs(*d)))
                }
                _ => None,
            },
            // ToInt32 → |v| <= 2^31.
            BinaryOp::BitAnd
            | BinaryOp::BitOr
            | BinaryOp::BitXor
            | BinaryOp::Shl
            | BinaryOp::Shr => Some(31),
            // ToUint32 → |v| <= 2^32.
            BinaryOp::UShr => Some(32),
            _ => None,
        },
        _ => None,
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
        Expr::LocalGet(id) => matches!(ctx.stable_local_type_proof(id), Some(HirType::Boolean)),
        _ => false,
    }
}

#[cfg(test)]
mod tests;
