//! Typed-array element access selected by the DECLARED element kind.
//!
//! A receiver whose declared type is a typed array (today: a class-field read
//! `o.f` with `f: Int32Array`) selects this lowering. The declaration only
//! selects it; the value's own header proves it, in O(1), at every access:
//!
//! 1. the value is a heap pointer (the shared receiver range test of
//!    `receiver_range`, as the class-field guard uses);
//! 2. its header's type byte is exactly the declared kind's OWNER brand (a view
//!    carries `BYTES_TYPE_VIEW` and fails; a detached owner has length 0 and
//!    fails the bounds test). A store needs no integrity test: freezing an
//!    owner with elements throws (`js_object_freeze`), and an empty one fails
//!    the bounds test;
//! 3. the key is an exact integer below the live length (one unsigned compare,
//!    which also rejects a negative key);
//! 4. the element is read or written at the kind's width. Elements are raw
//!    bytes: no barrier, no width dispatch.
//!
//! Every other case leaves through ONE shared runtime exit, the complete
//! `[[Get]]` (`js_dyn_index_get`) or `[[Set]]` (`js_dyn_index_set_strict`):
//! out of bounds, a non-integral or non-Number key, a non-Number store value,
//! views, detached or resizable buffers, another kind, a lying annotation, and
//! the BigInt kinds (which emit only the exit). The inline part is emitted by
//! this one emitter for every kind; there are no per-site runtime bodies.
//!
//! An element read's inline arm yields a raw double, so a relational compare
//! against a Number tests the ordered case inline (`ucomisd`) and gives the
//! unordered case (NaN, `undefined`, anything a lying annotation produced) to
//! the runtime's relational helper. That is exact for every value.
use super::FnCtx;
use crate::types::{DOUBLE, F32, I1, I16, I32, I64, I8};
use anyhow::Result;
use perry_hir::types::Type as HirType;
use perry_hir::{CompareOp, Expr};

/// `BYTES_OUT_OF_LINE` within the low byte of the header's `_reserved` flags.
const OUT_OF_LINE_FLAG: i8 = crate::runtime_abi::BYTES_OUT_OF_LINE as u8 as i8;

const KIND_UINT8: u8 = 1;
const KIND_UINT8_CLAMPED: u8 = 8;
const KIND_BIGINT64: u8 = 9;
const KIND_BIGUINT64: u8 = 10;
const KIND_FLOAT16: u8 = 11;

fn name_kind(name: &str) -> Option<u8> {
    Some(match name {
        "Int8Array" => 0,
        "Uint8Array" | "Buffer" => KIND_UINT8,
        "Int16Array" => 2,
        "Uint16Array" => 3,
        "Int32Array" => 4,
        "Uint32Array" => 5,
        "Float32Array" => 6,
        "Float64Array" => 7,
        "Uint8ClampedArray" => KIND_UINT8_CLAMPED,
        "BigInt64Array" => KIND_BIGINT64,
        "BigUint64Array" => KIND_BIGUINT64,
        "Float16Array" => KIND_FLOAT16,
        _ => return None,
    })
}

pub(crate) fn type_kind(ty: &HirType) -> Option<u8> {
    match ty {
        HirType::Named(name) => name_kind(name),
        HirType::Generic { base, .. } => name_kind(base),
        _ => None,
    }
}

fn is_bigint(kind: u8) -> bool {
    matches!(kind, KIND_BIGINT64 | KIND_BIGUINT64)
}

fn width_shift(kind: u8) -> u32 {
    match kind {
        0 | KIND_UINT8 | KIND_UINT8_CLAMPED => 0,
        2 | 3 | KIND_FLOAT16 => 1,
        4..=6 => 2,
        _ => 3,
    }
}

/// The element kind the receiver's DECLARATION names, for a receiver this
/// lowering owns: a field read `o.f` whose holder's declared class gives `f` a
/// typed-array type. Locals, parameters and module bindings keep
/// `ta_element_read`, which carries their hoisted loop proofs.
pub(crate) fn declared_kind(ctx: &FnCtx<'_>, object: &Expr) -> Option<u8> {
    if ctx.disable_buffer_fast_path {
        return None;
    }
    let Expr::PropertyGet {
        object: holder,
        property,
        ..
    } = object
    else {
        return None;
    };
    let class = crate::type_analysis::receiver_class_name(ctx, holder)
        .or_else(|| super::property_get::guarded_declared_class_get_candidate(ctx, holder))?;
    let ty = crate::type_analysis_class_fields::class_declared_field_type(ctx, &class, property)?;
    type_kind(&ty)
}

fn key_is_element_candidate(ctx: &FnCtx<'_>, index: &Expr) -> bool {
    !matches!(
        index,
        Expr::String(_) | Expr::WtfString(_) | Expr::SymbolFor(_)
    ) && !crate::type_analysis::is_string_expr(ctx, index)
        && !super::compare::is_proven_symbol_expr(ctx, index)
}

/// `o.f[i]` with a declared typed-array `f`.
pub(crate) fn try_lower_read(
    ctx: &mut FnCtx<'_>,
    object: &Expr,
    index: &Expr,
    number_context: bool,
) -> Result<Option<String>> {
    let Some(kind) = declared_kind(ctx, object) else {
        return Ok(None);
    };
    if !key_is_element_candidate(ctx, index) {
        return Ok(None);
    }
    let integer_index = super::index_get::numeric_index_has_integer_array_index_proof(ctx, index);
    let proven = super::wpfacts_access::ta_field_kind(ctx, object) == Some(kind);
    crate::rooting::with_operands_rooted(ctx, &[object, index], |ctx, values| {
        let value = emit_read(
            ctx,
            &values[0],
            &values[1],
            kind,
            integer_index,
            number_context,
            proven,
        );
        ctx.record_lowered_value(
            "TypedArrayGet",
            None,
            "TypedArrayGet.declared_kind_inline",
            &crate::native_value::LoweredValue::js_value(value.clone()),
            None,
            None,
            None,
            false,
            false,
            vec![
                format!("kind={kind}"),
                "proof=owner_brand_header".to_string(),
            ],
        );
        Ok(Some(value))
    })
}

/// `o.f[i] = v` with a declared typed-array `f`. Returns the assignment value.
pub(crate) fn try_lower_store(
    ctx: &mut FnCtx<'_>,
    object: &Expr,
    index: &Expr,
    value: &Expr,
    strict: bool,
) -> Result<Option<String>> {
    let Some(kind) = declared_kind(ctx, object) else {
        return Ok(None);
    };
    if !key_is_element_candidate(ctx, index) {
        return Ok(None);
    }
    let integer_index = super::index_get::numeric_index_has_integer_array_index_proof(ctx, index);
    let proven = super::wpfacts_access::ta_field_kind(ctx, object) == Some(kind);
    crate::rooting::with_operands_rooted_across(
        ctx,
        &[object, index],
        &[value],
        |ctx| super::lower_expr(ctx, value),
        |ctx, values, stored| {
            if ctx.block().is_terminated() {
                return Ok(Some(stored));
            }
            emit_store(
                ctx,
                &values[0],
                &values[1],
                &stored,
                kind,
                integer_index,
                strict,
                proven,
            );
            ctx.record_lowered_value(
                "TypedArraySet",
                None,
                "TypedArraySet.declared_kind_inline",
                &crate::native_value::LoweredValue::js_value(stored.clone()),
                None,
                None,
                None,
                false,
                false,
                vec![
                    format!("kind={kind}"),
                    "proof=owner_brand_header".to_string(),
                ],
            );
            Ok(Some(stored))
        },
    )
}

/// Steps 1 and 2 of the module docs. Leaves the current block on the passing
/// edge and returns the receiver's biased address (`receiver_range`): every
/// cell field is addressed from it with the handle floor folded into the
/// displacement.
///
/// `proven` (wpfacts F6, `wpfacts_access::ta_field_kind`): the value is a
/// stable owner of exactly this kind, so neither test is emitted; the bounds
/// test that follows still runs.
fn emit_owner_proof(
    ctx: &mut FnCtx<'_>,
    boxed: &str,
    kind: u8,
    miss: &str,
    proven: bool,
) -> String {
    use super::receiver_range::{emit_field_ptr, emit_fused_receiver_test};
    if proven {
        let blk = ctx.block();
        let bits = blk.bitcast_double_to_i64(boxed);
        return emit_fused_receiver_test(blk, &bits).biased;
    }
    let header = ctx.new_block("tael.header");
    let owner = ctx.new_block("tael.owner");
    let header_l = ctx.block_label(header);
    let owner_l = ctx.block_label(owner);
    let blk = ctx.block();
    let bits = blk.bitcast_double_to_i64(boxed);
    let receiver = emit_fused_receiver_test(blk, &bits);
    blk.cond_br(&receiver.is_object_pointer, &header_l, miss);
    ctx.current_block = header;
    let blk = ctx.block();
    let biased = receiver.biased;
    // The type byte alone: an owner of exactly this kind. A view sets
    // `BYTES_TYPE_VIEW`; a detached owner has length 0 (`clear_owner_extent`),
    // so the bounds test sends it to the exit; owners are never resizable
    // (only an ArrayBuffer owner is, and its typed arrays are views).
    let type_ptr = emit_field_ptr(blk, &biased, -(crate::runtime_abi::GC_HEADER_SIZE as i64));
    let type_byte = blk.load(I8, &type_ptr);
    let brands: &[u8] = if kind == KIND_UINT8 {
        &super::u8_buffer_read::U8_BRANDS
    } else {
        &[]
    };
    let own = if brands.is_empty() {
        let brand = super::byte_cell::brand_for_kind(kind);
        #[cfg(test)]
        let brand = sabotaged_brand(brand);
        blk.icmp_eq(I8, &type_byte, &(brand as i8).to_string())
    } else {
        let mut any = "false".to_string();
        for brand in brands {
            let hit = blk.icmp_eq(I8, &type_byte, &(*brand as i8).to_string());
            any = blk.or(I1, &any, &hit);
        }
        any
    };
    blk.cond_br(&own, &owner_l, miss);
    ctx.current_block = owner;
    biased
}

/// The low byte of the header's `_reserved` flags (`OUT_OF_LINE`).
fn emit_flags(blk: &mut crate::block::LlBlock, biased: &str) -> String {
    let ptr = super::receiver_range::emit_field_ptr(
        blk,
        biased,
        2 - crate::runtime_abi::GC_HEADER_SIZE as i64,
    );
    blk.load(I8, &ptr)
}

/// Negative control: `PERRY_TYPED_ELEMENT_SABOTAGE=brand` makes the proof
/// admit a brand off by one kind, so the IR tests must go red.
#[cfg(test)]
fn sabotaged_brand(brand: u8) -> u8 {
    if std::env::var("PERRY_TYPED_ELEMENT_SABOTAGE")
        .ok()
        .as_deref()
        == Some("brand")
    {
        brand ^ 2
    } else {
        brand
    }
}

/// Step 3: the key as an exact integer below the live length. Returns the
/// index as i64 and the condition (to be combined by the caller).
fn emit_index(
    ctx: &mut FnCtx<'_>,
    biased: &str,
    key: &str,
    integer_index: bool,
) -> (String, String) {
    let blk = ctx.block();
    let converted = blk.fptosi(DOUBLE, key, I32);
    let (index, exact) = if integer_index {
        (converted, "true".to_string())
    } else {
        // Out of range or NaN, `fptosi` is poison; frozen, it is some i32 that
        // cannot convert back to the key, so the round trip rejects it.
        let index = blk.freeze(I32, &converted);
        let back = blk.sitofp(I32, &index, DOUBLE);
        let exact = blk.fcmp("oeq", &back, key);
        (index, exact)
    };
    let len_ptr =
        super::receiver_range::emit_field_ptr(blk, biased, crate::runtime_abi::BYTES_LEN as i64);
    let len = blk.load(I32, &len_ptr);
    let in_bounds = blk.icmp_ult(I32, &index, &len);
    let ok = blk.and(I1, &exact, &in_bounds);
    let wide = blk.zext(I32, &index, I64);
    (wide, ok)
}

/// `PERRY_CHECK_CONTRACTS=1` while compiling (the contract-check build, see
/// `perry-runtime` `entry_contracts`): every admitted access first asks the
/// runtime (`js_typed_element_contract`) whether it agrees, and aborts when it
/// does not. Reads only, never collects.
fn emit_contract_witness(ctx: &mut FnCtx<'_>, boxed: &str, kind: u8, index: &str) {
    if crate::codegen::helpers::check_contracts() {
        ctx.block().call_gc_leaf(
            I32,
            "js_typed_element_contract",
            &[(DOUBLE, boxed), (I32, &kind.to_string()), (I64, index)],
        );
    }
}

/// The element address. Bounds already hold (length >= 1), so the store word
/// at `BYTES_STORE` lies inside the allocation and is read unconditionally.
fn emit_element_address(ctx: &mut FnCtx<'_>, biased: &str, index: &str, kind: u8) -> String {
    let store = crate::runtime_abi::BYTES_STORE as i64;
    let blk = ctx.block();
    let slot = super::receiver_range::emit_field_ptr(blk, biased, store);
    let external = blk.load(I64, &slot);
    let base = blk.add(
        I64,
        biased,
        &(super::receiver_range::HANDLE_FLOOR as i64 + store).to_string(),
    );
    let flags = emit_flags(blk, biased);
    let flag = blk.and(I8, &flags, &OUT_OF_LINE_FLAG.to_string());
    let out_of_line = blk.icmp_ne(I8, &flag, "0");
    let data = blk.select(I1, &out_of_line, I64, &external, &base);
    let offset = blk.shl(I64, index, &width_shift(kind).to_string());
    let addr = blk.add(I64, &data, &offset);
    blk.inttoptr(I64, &addr)
}

fn emit_read(
    ctx: &mut FnCtx<'_>,
    boxed: &str,
    key: &str,
    kind: u8,
    integer_index: bool,
    number_context: bool,
    proven: bool,
) -> String {
    let slow = ctx.new_block("tael.get.slow");
    let done = ctx.new_block("tael.get.done");
    let slow_l = ctx.block_label(slow);
    let done_l = ctx.block_label(done);
    let mut incoming = Vec::new();
    if !is_bigint(kind) {
        let load = ctx.new_block("tael.get.load");
        let load_l = ctx.block_label(load);
        let biased = emit_owner_proof(ctx, boxed, kind, &slow_l, proven);
        let (index, ok) = emit_index(ctx, &biased, key, integer_index);
        ctx.block().cond_br(&ok, &load_l, &slow_l);
        ctx.current_block = load;
        emit_contract_witness(ctx, boxed, kind, &index);
        let ptr = emit_element_address(ctx, &biased, &index, kind);
        let value = super::ta_element_read::emit_element(ctx.block(), &ptr, kind);
        incoming.push((value, ctx.block().label.clone()));
        ctx.block().br(&done_l);
    } else {
        ctx.block().br(&slow_l);
    }
    ctx.current_block = slow;
    let _fallback_decision = crate::fallback_diag::decision(
        "typed element guard declined; complete dynamic indexed semantics required",
        "receiver brand/index/value admissibility",
    );
    let fallback = ctx.block().call(
        DOUBLE,
        "js_dyn_index_get",
        &[(DOUBLE, boxed), (DOUBLE, key)],
    );
    let fallback = if number_context {
        ctx.block()
            .call(DOUBLE, "js_number_coerce", &[(DOUBLE, &fallback)])
    } else {
        fallback
    };
    incoming.push((fallback, ctx.block().label.clone()));
    ctx.block().br(&done_l);
    ctx.current_block = done;
    let incoming: Vec<(&str, &str)> = incoming
        .iter()
        .map(|(v, l)| (v.as_str(), l.as_str()))
        .collect();
    ctx.block().phi(DOUBLE, &incoming)
}

fn emit_store(
    ctx: &mut FnCtx<'_>,
    boxed: &str,
    key: &str,
    value: &str,
    kind: u8,
    integer_index: bool,
    strict: bool,
    proven: bool,
) {
    let slow = ctx.new_block("tael.set.slow");
    let done = ctx.new_block("tael.set.done");
    let slow_l = ctx.block_label(slow);
    let done_l = ctx.block_label(done);
    if !is_bigint(kind) && kind != KIND_FLOAT16 {
        let store = ctx.new_block("tael.set.store");
        let store_l = ctx.block_label(store);
        let biased = emit_owner_proof(ctx, boxed, kind, &slow_l, proven);
        let (index, ok) = emit_index(ctx, &biased, key, integer_index);
        let blk = ctx.block();
        // Only a plain double is stored inline: ToNumber of anything else may
        // run user code, which the runtime orders before the index check.
        let bits = blk.bitcast_double_to_i64(value);
        let integer_lane = !matches!(kind, 6 | 7 | KIND_UINT8_CLAMPED);
        let number = if integer_lane {
            // |v| < 2^63 (biased exponent below 1086): a finite Number whose
            // ToInt32 is the low half of `fptosi i64`. NaN, infinities, huge
            // values and every NaN-boxed value have a larger exponent field.
            let doubled = blk.shl(I64, &bits, "1");
            let exponent = blk.lshr(I64, &doubled, "53");
            blk.icmp_ult(I64, &exponent, "1086")
        } else {
            crate::codegen::emit_plain_number_bits_test(blk, &bits)
        };
        let mut ok = blk.and(I1, &ok, &number);
        let mut clamped = None;
        if kind == KIND_UINT8_CLAMPED {
            // ToUint8Clamp is the identity on integers 0..=255; rounding and
            // clamping stay on the runtime exit.
            let converted = blk.fptosi(DOUBLE, value, I32);
            let int = blk.freeze(I32, &converted);
            let back = blk.sitofp(I32, &int, DOUBLE);
            let exact = blk.fcmp("oeq", &back, value);
            let byte = blk.icmp_ult(I32, &int, "256");
            let fits = blk.and(I1, &exact, &byte);
            ok = blk.and(I1, &ok, &fits);
            clamped = Some(int);
        }
        blk.cond_br(&ok, &store_l, &slow_l);
        ctx.current_block = store;
        emit_contract_witness(ctx, boxed, kind, &index);
        let ptr = emit_element_address(ctx, &biased, &index, kind);
        match kind {
            6 => {
                let blk = ctx.block();
                let narrowed = blk.fptrunc(DOUBLE, value, F32);
                blk.store(F32, &narrowed, &ptr);
            }
            7 => ctx.block().store(DOUBLE, value, &ptr),
            _ => {
                let int = match clamped {
                    Some(int) => int,
                    None => {
                        // In range (checked above): truncation, then the low
                        // 32 bits, is ToInt32.
                        let blk = ctx.block();
                        let wide = blk.fptosi(DOUBLE, value, I64);
                        blk.trunc(I64, &wide, I32)
                    }
                };
                let ty = match width_shift(kind) {
                    0 => I8,
                    1 => I16,
                    _ => I32,
                };
                let blk = ctx.block();
                let narrowed = if ty == I32 {
                    int
                } else {
                    blk.trunc(I32, &int, ty)
                };
                blk.store(ty, &narrowed, &ptr);
            }
        }
        ctx.block().br(&done_l);
    } else {
        ctx.block().br(&slow_l);
    }
    ctx.current_block = slow;
    let _fallback_decision = crate::fallback_diag::decision(
        "typed element guard declined; complete dynamic indexed semantics required",
        "receiver brand/index/value admissibility",
    );
    ctx.block().call(
        DOUBLE,
        "js_dyn_index_set_strict",
        &[
            (DOUBLE, boxed),
            (DOUBLE, key),
            (DOUBLE, value),
            (I32, if strict { "1" } else { "0" }),
        ],
    );
    ctx.block().br(&done_l);
    ctx.current_block = done;
}

/// An element read of a declared typed array, or an immutable binding of one.
/// A selection hint only: the relational lowering below is exact for every
/// value, so a wrong hint costs speed, never an answer.
pub(crate) fn yields_element(ctx: &FnCtx<'_>, expr: &Expr) -> bool {
    match expr {
        Expr::IndexGet { object, .. } => {
            declared_kind(ctx, object).is_some_and(|kind| !is_bigint(kind))
        }
        Expr::LocalGet(id) => ctx.typed_element_locals.contains(id),
        _ => false,
    }
}

/// `x < n` (and `<=`, `>`, `>=`) where `x` is an element read and `n` a raw
/// double: the ordered case is one `fcmp`; the unordered case (NaN, undefined
/// or any NaN-boxed value) takes the runtime relational helper.
pub(crate) fn try_lower_relational(
    ctx: &mut FnCtx<'_>,
    op: CompareOp,
    left: &Expr,
    right: &Expr,
) -> Result<Option<String>> {
    let (pred, helper) = match op {
        CompareOp::Lt => ("olt", "js_rel_lt"),
        CompareOp::Le => ("ole", "js_rel_le"),
        CompareOp::Gt => ("ogt", "js_rel_gt"),
        CompareOp::Ge => ("oge", "js_rel_ge"),
        _ => return Ok(None),
    };
    let number = |ctx: &FnCtx<'_>, e: &Expr| {
        crate::type_analysis::is_numeric_expr(ctx, e)
            && !crate::type_analysis::expr_may_return_boxed_value_from_raw_f64_fallback(ctx, e)
            && crate::type_analysis::expr_produces_canonical_raw_f64(ctx, e)
    };
    let element_left = if yields_element(ctx, left) && number(ctx, right) {
        true
    } else if yields_element(ctx, right) && number(ctx, left) {
        false
    } else {
        return Ok(None);
    };
    crate::rooting::with_operands_rooted(ctx, &[left, right], |ctx, values| {
        let (l, r) = (values[0].clone(), values[1].clone());
        let element = if element_left { &l } else { &r };
        // Ordered and true decides at once; otherwise an unordered element
        // (NaN or any NaN-boxed value) takes the runtime helper.
        let unordered = ctx.new_block("tael.rel.unordered");
        let slow = ctx.new_block("tael.rel.slow");
        let done = ctx.new_block("tael.rel.done");
        let unordered_l = ctx.block_label(unordered);
        let slow_l = ctx.block_label(slow);
        let done_l = ctx.block_label(done);
        let fast_bit = ctx.block().fcmp(pred, &l, &r);
        let entry_end = ctx.block().label.clone();
        ctx.block().cond_br(&fast_bit, &done_l, &unordered_l);
        ctx.current_block = unordered;
        let nan = ctx.block().fcmp("uno", element, element);
        ctx.block().cond_br(&nan, &slow_l, &done_l);
        ctx.current_block = slow;
        let result = ctx
            .block()
            .call(DOUBLE, helper, &[(DOUBLE, &l), (DOUBLE, &r)]);
        let bits = ctx.block().bitcast_double_to_i64(&result);
        let slow_bit = ctx.block().icmp_eq(I64, &bits, crate::nanbox::TAG_TRUE_I64);
        let slow_end = ctx.block().label.clone();
        ctx.block().br(&done_l);
        ctx.current_block = done;
        Ok(Some(ctx.block().phi(
            I1,
            &[
                ("true", &entry_end),
                ("false", &unordered_l),
                (&slow_bit, &slow_end),
            ],
        )))
    })
}
