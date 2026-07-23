//! Numeric / bigint / boolean static-type predicates.
//!
//! Split out of `type_analysis.rs` (file-size gate). Pure code move.

use super::*;

use perry_hir::{BinaryOp, Expr, UnaryOp};
use perry_types::Type as HirType;

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
        // `obj.field` where the field is declared as `number` on the
        // owning class. Without this, `this.value + 1` in a hot loop
        // wraps the field load in `js_number_coerce` which prevents
        // LLVM from doing GVN/LICM on the load. The class field
        // walker matches `class_field_global_index`'s inheritance
        // traversal so the type of any inherited field is also seen.
        Expr::PropertyGet {
            object, property, ..
        } => {
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
        Expr::IndexGet { object, index } => {
            if receiver_class_name(ctx, object)
                .as_deref()
                .is_some_and(is_numeric_typed_array_class)
            {
                return true;
            }
            let Expr::LocalGet(arr_id) = object.as_ref() else {
                return false;
            };
            // #6750 follow-up: a masked-index read covered by an ACTIVE
            // masked-window fact (dense range-loop / straight-line-region
            // fast copy) is a guard-proven numeric element load, even when
            // the receiver's STATIC type is erased (`any` parameter).
            // Without this, `n ^= S[x & 0xff]` inside a fast copy still
            // routed through the BigInt-aware dynamic helpers. Facts are
            // scope-managed by the versioned lowerings, so the answer is
            // only `true` while a fast copy that proved the window is being
            // lowered.
            if crate::expr::masked_window_fact_for_index(ctx, *arr_id, index).is_some() {
                return true;
            }
            match ctx.local_types.get(arr_id) {
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

#[cfg(test)]
mod tests;
