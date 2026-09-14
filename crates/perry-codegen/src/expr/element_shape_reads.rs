//! #10185: the two NON-numeric reads the element-shape fast clone admits —
//! `arr[i].prop.length` on a string field and `arr[i].prop ? A : B` on a
//! boolean one.
//!
//! ## Why they need their own lowering at all
//!
//! Both are ordinary JavaScript that the generic lowering handles perfectly
//! well — with a call. `.length` on an untyped receiver reaches the property
//! diamond and, on a miss, `js_object_get_field_by_name_f64`; a ternary on an
//! untyped condition reaches `js_is_truthy`. Inside this clone a call is not a
//! slow path, it is a DELETED clone (`contains_gc_unsafe_call`, #7690), so the
//! benchmark's `fields` shape —
//!
//! ```text
//! sum += rows[index].id;
//! sum += rows[index].name.length;
//! sum += rows[index].active ? 1 : 0;
//! ```
//!
//! got no clone at all: one unadmitted read in the body costs the whole loop.
//!
//! ## What each one proves before it reads
//!
//! The element-shape preheader proves WHICH inline slot holds `name` and
//! `active`; it proves nothing about what is in them. So each read tag-tests
//! the loaded word and side-exits to the slow clone when the answer is not the
//! representation it is about to assume — exactly the discipline #10123
//! established for the Number case. A `name` that is a number, a `null` or an
//! object, and an `active` that is anything but `true`/`false`, are re-run in
//! the slow clone, where full JS semantics apply.
//!
//! **`.length`.** JS `.length` is UTF-16 code units. A heap string keeps that
//! count in `StringHeader::utf16_len`, the leading `u32` — the identical load
//! the inline `.length` fast path in `property_get/generic_dispatch.rs` emits.
//! An SSO immediate (`SHORT_STRING_TAG`, up to five bytes packed into the
//! NaN-box) keeps its byte length in bits 40..=47. The shared SSO length
//! lowering returns that count for ASCII and counts UTF-16 units inline for
//! non-ASCII payloads, matching heap strings without adding a call (#10191).
//!
//! **The ternary.** JS truthiness of an arbitrary value is a runtime question
//! (`""`, `0`, `NaN`, `null`, every object). The clone does not guess it: only
//! the two boolean singletons are admitted, by exact NaN-box bit pattern, and
//! everything else side-exits. The result is one `select` between two
//! compile-time constants.

use anyhow::Result;
use perry_hir::Expr;

use super::FnCtx;
use crate::types::{DOUBLE, I1, I32, I64};

/// `STRING_TAG >> 48` — a heap `StringHeader` pointer in the low 48 bits.
const STRING_TAG_TOP16: &str = crate::nanbox::STRING_TAG_TOP16_I64;
/// `SHORT_STRING_TAG >> 48` — an SSO immediate.
const SHORT_STRING_TAG_TOP16: &str = crate::nanbox::SHORT_STRING_TAG_TOP16_I64;

/// `TAG_TRUE` (`0x7FFC_0000_0000_0004`) as a decimal i64 literal.
const TAG_TRUE_I64: &str = "9222246136947933188";
/// `TAG_FALSE` (`0x7FFC_0000_0000_0003`) as a decimal i64 literal.
const TAG_FALSE_I64: &str = "9222246136947933187";

/// Is `expr` a tracked element read the fast clone has a fact for?
///
/// Only meaningful INSIDE the clone — outside one the fact vector is empty and
/// every answer is `None`.
fn tracked_element_read(ctx: &FnCtx<'_>, expr: &Expr) -> bool {
    let Expr::PropertyGet {
        object, property, ..
    } = expr
    else {
        return false;
    };
    super::element_shape_loop_fact_for_property_get(ctx, object, property).is_some()
}

/// `<tracked element read>.length` — returns the inner read.
pub(crate) fn cloned_string_length_read<'e>(ctx: &FnCtx<'_>, expr: &'e Expr) -> Option<&'e Expr> {
    let Expr::PropertyGet {
        object, property, ..
    } = expr
    else {
        return None;
    };
    if property != "length" {
        return None;
    }
    tracked_element_read(ctx, object.as_ref()).then(|| object.as_ref())
}

/// `<tracked element read> ? <int literal> : <int literal>` — returns the
/// condition's inner read and the two constants.
pub(crate) fn cloned_bool_select_read<'e>(
    ctx: &FnCtx<'_>,
    expr: &'e Expr,
) -> Option<(&'e Expr, i64, i64)> {
    let Expr::Conditional {
        condition,
        then_expr,
        else_expr,
    } = expr
    else {
        return None;
    };
    if !tracked_element_read(ctx, condition.as_ref()) {
        return None;
    }
    Some((
        condition.as_ref(),
        integer_arm(then_expr)?,
        integer_arm(else_expr)?,
    ))
}

/// A ternary arm the clone may bake in: an integer-valued literal.
///
/// Deliberately narrow. A non-constant arm would have to be lowered on both
/// sides of a branch inside a clone whose whole admission argument is that its
/// body is one straight line of pure reads.
pub(crate) fn integer_arm(expr: &Expr) -> Option<i64> {
    match expr {
        Expr::Integer(k) => Some(*k),
        Expr::Number(n) if n.is_finite() && n.fract() == 0.0 && *n >= -1e15 && *n <= 1e15 => {
            Some(*n as i64)
        }
        _ => None,
    }
}

/// Resolve one tracked read to (fact, slot, index) and emit the raw slot word.
fn emit_tracked_slot_word(ctx: &mut FnCtx<'_>, read: &Expr) -> Option<String> {
    let Expr::PropertyGet {
        object, property, ..
    } = read
    else {
        return None;
    };
    let (fact, slot) = super::element_shape_loop_fact_for_property_get(ctx, object, property)
        .map(|(fact, slot)| (fact.clone(), slot.clone()))?;
    // A class-keyed clone's slot holds a RAW double, not a NaN-boxed value —
    // there is no tag to test and these two readers would be reading a
    // `double`'s bit pattern as a box. Both forms are shape-keyed-only.
    if !fact.shape_keyed {
        return None;
    }
    let idx_i32 = super::element_shape_guard::emit_element_shape_index(ctx, &fact)?;
    Some(super::element_shape_guard::emit_element_shape_slot_load(
        ctx, &fact, &idx_i32, &slot,
    ))
}

/// The side exit the tracked read's own fact names.
fn side_exit_label(ctx: &FnCtx<'_>, read: &Expr) -> Option<String> {
    let Expr::PropertyGet {
        object, property, ..
    } = read
    else {
        return None;
    };
    super::element_shape_loop_fact_for_property_get(ctx, object, property)
        .map(|(fact, _)| fact.side_exit_label.clone())
}

/// Emit `<tracked read>.length` as an f64.
pub(crate) fn lower_cloned_string_length(
    ctx: &mut FnCtx<'_>,
    read: &Expr,
) -> Result<Option<String>> {
    let Some(exit) = side_exit_label(ctx, read) else {
        return Ok(None);
    };
    let Some(value) = emit_tracked_slot_word(ctx, read) else {
        return Ok(None);
    };

    let bits = ctx.block().bitcast_double_to_i64(&value);
    let top16 = ctx.block().lshr(I64, &bits, "48");
    let is_heap = ctx.block().icmp_eq(I64, &top16, STRING_TAG_TOP16);
    let is_sso = ctx.block().icmp_eq(I64, &top16, SHORT_STRING_TAG_TOP16);
    let is_string = ctx.block().or(I1, &is_heap, &is_sso);

    let dispatch_idx = ctx.new_block("element_shape.strlen");
    let heap_idx = ctx.new_block("element_shape.strlen.heap");
    let sso_idx = ctx.new_block("element_shape.strlen.sso");
    let done_idx = ctx.new_block("element_shape.strlen.done");
    let dispatch_label = ctx.block_label(dispatch_idx);
    let heap_label = ctx.block_label(heap_idx);
    let sso_label = ctx.block_label(sso_idx);
    let done_label = ctx.block_label(done_idx);

    // A non-string `name` re-runs the whole iteration in the slow clone rather
    // than being decoded as one.
    ctx.block().cond_br(&is_string, &dispatch_label, &exit);

    ctx.current_block = dispatch_idx;
    ctx.block().cond_br(&is_heap, &heap_label, &sso_label);

    // `StringHeader::utf16_len` is the leading `u32`; `safe_load_i32_from_ptr`
    // keeps a sub-page handle off the load, and is itself call-free (an `icmp`,
    // a `select` onto a zero-valued global, and the load).
    ctx.current_block = heap_idx;
    let heap_handle = ctx.block().and(I64, &bits, crate::nanbox::POINTER_MASK_I64);
    let heap_len_i32 = ctx.block().safe_load_i32_from_ptr(&heap_handle);
    let heap_len = ctx.block().uitofp(I32, &heap_len_i32, DOUBLE);
    let heap_end = ctx.block().label.clone();
    ctx.block().br(&done_label);

    ctx.current_block = sso_idx;
    let sso_len = super::string_length::lower_sso_length(ctx, &bits);
    let sso_end = ctx.block().label.clone();
    ctx.block().br(&done_label);

    ctx.current_block = done_idx;
    let length = ctx
        .block()
        .phi(DOUBLE, &[(&heap_len, &heap_end), (&sso_len, &sso_end)]);
    Ok(Some(length))
}

/// Emit `<tracked read> ? A : B` as an f64 select over the two constants.
pub(crate) fn lower_cloned_bool_select(
    ctx: &mut FnCtx<'_>,
    read: &Expr,
    on_true: i64,
    on_false: i64,
) -> Result<Option<String>> {
    let Some(exit) = side_exit_label(ctx, read) else {
        return Ok(None);
    };
    let Some(value) = emit_tracked_slot_word(ctx, read) else {
        return Ok(None);
    };

    let bits = ctx.block().bitcast_double_to_i64(&value);
    let is_true = ctx.block().icmp_eq(I64, &bits, TAG_TRUE_I64);
    let is_false = ctx.block().icmp_eq(I64, &bits, TAG_FALSE_I64);
    let is_bool = ctx.block().or(I1, &is_true, &is_false);

    let ok_idx = ctx.new_block("element_shape.bool");
    let ok_label = ctx.block_label(ok_idx);
    // Everything that is not one of the two boolean singletons — `0`, `""`,
    // `null`, an object — has a JS truthiness the clone is not allowed to
    // guess, so it takes the side exit.
    ctx.block().cond_br(&is_bool, &ok_label, &exit);

    ctx.current_block = ok_idx;
    let then_literal = format!("{:.1}", on_true as f64);
    let else_literal = format!("{:.1}", on_false as f64);
    Ok(Some(ctx.block().select(
        I1,
        &is_true,
        DOUBLE,
        &then_literal,
        &else_literal,
    )))
}

/// The arithmetic-operand entry point (`expr::binary::lower_arithmetic_operand`).
///
/// Returns the lowered f64, already representation-proven, so the caller must
/// NOT append a `js_number_coerce` — that call would delete the clone.
pub(crate) fn try_lower_cloned_read_operand(
    ctx: &mut FnCtx<'_>,
    expr: &Expr,
) -> Result<Option<String>> {
    if let Some(read) = cloned_string_length_read(ctx, expr) {
        let read = read.clone();
        return lower_cloned_string_length(ctx, &read);
    }
    if let Some((read, on_true, on_false)) = cloned_bool_select_read(ctx, expr) {
        let read = read.clone();
        return lower_cloned_bool_select(ctx, &read, on_true, on_false);
    }
    Ok(None)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Anti-drift gate, the same one `element_shape_guard`'s constants carry:
    /// every literal above reproduces a `perry-runtime::value::tags` constant,
    /// and `perry-codegen` cannot share the definition.
    #[test]
    fn boolean_singleton_literals_match_the_runtime() {
        assert_eq!(TAG_TRUE_I64, (0x7FFC_0000_0000_0004u64 as i64).to_string());
        assert_eq!(TAG_FALSE_I64, (0x7FFC_0000_0000_0003u64 as i64).to_string());
        // Neither may collide with any other singleton in the 0x7FFC namespace
        // — a clone that read `undefined`/`null`/a hole as `false` would be a
        // miscompile, not a slow path.
        for other in [
            0x7FFC_0000_0000_0001u64, // TAG_UNDEFINED
            0x7FFC_0000_0000_0002,    // TAG_NULL
            0x7FFC_0000_0000_0010,    // TAG_HOLE
            0x7FFC_0000_0000_0011,    // TAG_TDZ
        ] {
            assert_ne!(TAG_TRUE_I64, (other as i64).to_string());
            assert_ne!(TAG_FALSE_I64, (other as i64).to_string());
        }
    }

    #[test]
    fn string_tag_literals_match_the_runtime() {
        assert_eq!(STRING_TAG_TOP16, (0x7FFFu64).to_string());
        assert_eq!(SHORT_STRING_TAG_TOP16, (0x7FF9u64).to_string());
    }

    #[test]
    fn only_integral_ternary_arms_are_admitted() {
        assert_eq!(integer_arm(&Expr::Integer(1)), Some(1));
        assert_eq!(integer_arm(&Expr::Number(0.0)), Some(0));
        assert_eq!(integer_arm(&Expr::Number(1.5)), None);
        assert_eq!(integer_arm(&Expr::Number(f64::NAN)), None);
        assert_eq!(integer_arm(&Expr::Number(f64::INFINITY)), None);
        assert_eq!(integer_arm(&Expr::LocalGet(3)), None);
    }
}
