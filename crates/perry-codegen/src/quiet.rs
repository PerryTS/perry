//! Quiet sites: does evaluating this expression run program code?
//!
//! A loop "without calls" is not a loop that runs only its own function's
//! code. A store can run a setter or a Proxy `set` trap, a read a getter
//! or a `get` trap, an operator the `valueOf` / `toString` /
//! `@@toPrimitive` of an operand, a computed key its `toString`, a spread
//! an iterator. Every consumer of "this loop body runs no program code"
//! that reads the HIR (the length hoist's walker, `stmt::loops`) asks
//! [`site`] for each such expression; the answer comes from the
//! whole-program facts (wpfacts `quiet_sites`), never from the text or a
//! declared type:
//!
//! - a named read: its site is a quiet read (an own data slot of every
//!   receiver, or the realm's intrinsic member), or `x.length` of a local
//!   that holds only Strings and ordinary Arrays;
//! - an element read, store or update: its site is a quiet element site
//!   (plain program Arrays under a Number key, nothing defined on them);
//! - a store `o.k = v`: such an element site, or the facts' store rule on
//!   the receiver binding (`wpfacts_access::quiet_store`: `k` is a data
//!   birth field of every class `o` may hold; an accessor key is none);
//! - an operator or conversion: every operand it converts is a primitive
//!   ([`primitive`]): a literal, an operator's own result, a local, named
//!   read or element read the facts give only primitives, or a value
//!   codegen proved a non-pointer primitive from the function's own writes
//!   (`rooting::expr_is_inert_primitive`);
//! - a byte access of a native view: the access codegen lowers with no
//!   call (`can_lower_buffer_access_without_calls`).
//!
//! No fact means the site may run code. `PERRY_FACTS_QUIET_SITES=0` turns
//! the question off ([`enabled`]): every site answers as before the fact
//! existed.

use crate::expr::FnCtx;
use perry_hir::{ArrayElement, CompareOp, Expr, UnaryOp};

/// `PERRY_FACTS_QUIET_SITES=0` turns the question off.
pub(crate) fn enabled() -> bool {
    static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ON.get_or_init(|| {
        !matches!(
            std::env::var("PERRY_FACTS_QUIET_SITES").as_deref(),
            Ok("0") | Ok("off") | Ok("false")
        )
    })
}

/// The module's quiet-site facts, when they may be read.
fn facts<'a>(ctx: &'a FnCtx<'_>) -> Option<&'a crate::codegen::ElisionFacts> {
    (!ctx.wpfacts_off
        && !ctx.wpfacts.interp_guard
        && !crate::expr::typed_feedback_emission_enabled())
    .then_some(&*ctx.wpfacts)
}

/// The value of `e` is a primitive: converting it (ToPrimitive, ToNumber,
/// ToString, ToPropertyKey) runs no program code.
pub(crate) fn primitive(ctx: &FnCtx<'_>, e: &Expr) -> bool {
    match e {
        Expr::Undefined
        | Expr::Null
        | Expr::Bool(_)
        | Expr::Number(_)
        | Expr::Integer(_)
        | Expr::String(_)
        | Expr::WtfString(_) => true,
        // An operator's result is a primitive, whatever it ran to make it
        // (that is its own site's question).
        Expr::Binary { .. }
        | Expr::Compare { .. }
        | Expr::Unary { .. }
        | Expr::Update { .. }
        | Expr::TypeOf(_)
        | Expr::Void(_)
        | Expr::NumberCoerce(_)
        | Expr::StringCoerce(_)
        | Expr::TemplateStringCoerce(_)
        | Expr::BooleanCoerce(_)
        | Expr::MathImul(..)
        | Expr::MathPow(..)
        | Expr::MathMin(_)
        | Expr::MathMax(_)
        | Expr::MathAbs(_)
        | Expr::MathSqrt(_)
        | Expr::MathFloor(_)
        | Expr::MathCeil(_)
        | Expr::MathRound(_)
        | Expr::MathTrunc(_)
        | Expr::MathSign(_)
        | Expr::MathF16round(_) => true,
        Expr::Logical { left, right, .. } => primitive(ctx, left) && primitive(ctx, right),
        Expr::Conditional {
            then_expr,
            else_expr,
            ..
        } => primitive(ctx, then_expr) && primitive(ctx, else_expr),
        Expr::LocalSet(_, value) => primitive(ctx, value),
        Expr::LocalGet(id) => {
            facts(ctx).is_some_and(|f| f.primitive_locals.contains(id))
                || crate::rooting::expr_is_inert_primitive(ctx, e)
        }
        Expr::PropertyGet {
            object,
            property,
            byte_offset,
            ..
        } => {
            facts(ctx).is_some_and(|f| {
                f.primitive_reads.contains(&(*byte_offset, property.clone()))
                    || property == "length"
                        && matches!(object.as_ref(), Expr::LocalGet(id) if f.length_number_inputs.contains(id))
            }) || crate::rooting::expr_is_inert_primitive(ctx, e)
        }
        Expr::IndexGet { element_offset, .. } => facts(ctx).is_some_and(|f| {
            f.primitive_elements.contains(element_offset)
                || f.number_element_reads.contains_key(element_offset)
        }),
        Expr::Uint8ArrayGet { .. } | Expr::BufferIndexGet { .. } => {
            crate::rooting::expr_is_inert_primitive(ctx, e)
        }
        _ => false,
    }
}

/// Evaluating `e`'s own operation, its operands apart, runs no program
/// code (module docs). An expression kind that is no implicit-code site at
/// all (a local read, a literal, a branch) is quiet; a call is not this
/// question (its caller refuses it or asks its effects).
pub(crate) fn site(ctx: &FnCtx<'_>, e: &Expr) -> bool {
    if !enabled() {
        return true;
    }
    match e {
        Expr::PropertyGet {
            object,
            property,
            byte_offset,
            ..
        } => facts(ctx).is_some_and(|f| {
            f.quiet_reads.contains(&(*byte_offset, property.clone()))
                || property == "length"
                    && matches!(object.as_ref(), Expr::LocalGet(id) if f.length_number_inputs.contains(id))
        }),
        Expr::IndexGet { element_offset, .. }
        | Expr::IndexSet { element_offset, .. }
        | Expr::IndexUpdate { element_offset, .. } => {
            facts(ctx).is_some_and(|f| f.quiet_elements.contains(element_offset))
        }
        // `o.k = v`: an element site, or a store of a data field of every
        // class the receiver binding may hold.
        Expr::PutValueSet {
            target,
            key,
            receiver,
            element_offset,
            ..
        } => {
            facts(ctx).is_some_and(|f| f.quiet_elements.contains(element_offset))
                || match (target.as_ref(), key.as_ref(), receiver.as_ref()) {
                    (Expr::LocalGet(a), Expr::String(k), Expr::LocalGet(b)) if a == b => {
                        field_store(ctx, target, k)
                    }
                    (Expr::This, Expr::String(k), Expr::This) => field_store(ctx, target, k),
                    _ => false,
                }
        }
        Expr::Binary { left, right, .. } => primitive(ctx, left) && primitive(ctx, right),
        // Strict equality converts nothing.
        Expr::Compare { op, left, right } => {
            matches!(op, CompareOp::Eq | CompareOp::Ne)
                || primitive(ctx, left) && primitive(ctx, right)
        }
        Expr::Unary { op, operand } => matches!(op, UnaryOp::Not) || primitive(ctx, operand),
        Expr::Update { id, .. } => primitive(ctx, &Expr::LocalGet(*id)),
        Expr::NumberCoerce(operand)
        | Expr::StringCoerce(operand)
        | Expr::TemplateStringCoerce(operand)
        | Expr::MathAbs(operand)
        | Expr::MathSqrt(operand)
        | Expr::MathFloor(operand)
        | Expr::MathCeil(operand)
        | Expr::MathRound(operand)
        | Expr::MathTrunc(operand)
        | Expr::MathSign(operand)
        | Expr::MathF16round(operand) => primitive(ctx, operand),
        Expr::MathImul(a, b) | Expr::MathPow(a, b) => primitive(ctx, a) && primitive(ctx, b),
        Expr::MathMin(elems) | Expr::MathMax(elems) => elems.iter().all(|x| primitive(ctx, x)),
        // The message is converted with ToString.
        Expr::ErrorNew(message) => message.as_ref().is_none_or(|m| primitive(ctx, m)),
        Expr::TypeErrorNew(message)
        | Expr::RangeErrorNew(message)
        | Expr::ReferenceErrorNew(message)
        | Expr::SyntaxErrorNew(message) => primitive(ctx, message),
        Expr::ErrorNewWithCause { message, .. } => primitive(ctx, message),
        // The options object's `cause` is read: a getter.
        Expr::ErrorNewWithOptions { .. } => false,
        // A spread runs its operand's iterator.
        Expr::ArraySpread(elements) => elements
            .iter()
            .all(|el| !matches!(el, ArrayElement::Spread(_))),
        // `delete` of a Proxy's key runs its trap.
        Expr::Delete(_) => false,
        Expr::Uint8ArrayGet { array, index } => {
            crate::expr::can_lower_buffer_access_without_calls(
                ctx,
                array,
                index,
                crate::expr::BufferAccessSpec::uint8array_get(),
            )
        }
        Expr::BufferIndexGet { buffer, index } => {
            crate::expr::can_lower_buffer_access_without_calls(
                ctx,
                buffer,
                index,
                crate::expr::BufferAccessSpec::buffer_index_get(),
            )
        }
        // A byte store converts its value with ToNumber and its key with
        // ToPropertyKey; a receiver that is no native view takes the
        // ordinary store.
        Expr::Uint8ArraySet {
            array,
            index,
            value,
        } => native_view(ctx, array) && primitive(ctx, index) && primitive(ctx, value),
        Expr::BufferIndexSet {
            buffer,
            index,
            value,
        } => native_view(ctx, buffer) && primitive(ctx, index) && primitive(ctx, value),
        _ => true,
    }
}

/// `object` is a local codegen or the facts prove a native byte view: an
/// owned sealed buffer, a proven view, or a binding whose every value is
/// such an allocation (`indexed_owners`).
fn native_view(ctx: &FnCtx<'_>, object: &Expr) -> bool {
    let Expr::LocalGet(id) = object else {
        return false;
    };
    ctx.sealed_buffer_locals.contains(id)
        || crate::expr::proven_view_receiver(ctx, object).is_some()
        || facts(ctx).is_some_and(|f| f.indexed_owners.contains_key(id))
}

/// The facts' store rule for `object.key = v` (`wpfacts_access::quiet_store`),
/// when the facts may be read.
fn field_store(ctx: &FnCtx<'_>, object: &Expr, key: &str) -> bool {
    facts(ctx).is_some() && crate::expr::wpfacts_access::quiet_store(ctx, object, key)
}
