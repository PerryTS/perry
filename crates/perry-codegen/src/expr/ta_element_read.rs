//! Kind-specialized checked reads of numeric typed arrays. Immutable loop
//! parameters validate their receiver at entry; other locals use the existing
//! kind cache. Length and storage remain live, including after `.buffer`,
//! resize, detach and a traced backing rewrite. Native arenas keep their
//! disposal-aware runtime path.
use super::FnCtx;
use crate::nanbox::{double_literal, TAG_UNDEFINED};
use crate::types::{DOUBLE, F32, I1, I16, I32, I64, I8, PTR};
use anyhow::Result;
use perry_hir::Expr;

pub(crate) fn receiver_kind(ctx: &FnCtx<'_>, object: &Expr) -> Option<u8> {
    if ctx.disable_buffer_fast_path || !super::ta_param_f64_read::ta_param_f64_read_enabled() {
        return None;
    }
    let Expr::LocalGet(id) = object else {
        return None;
    };
    if ctx.reassigned_locals.contains(id) {
        return None;
    }
    // Tracked views own both their native accesses and their proof-aware
    // fallback. Taking a demoted view here bypasses its invalidation evidence
    // (and emits a managed-storage guard that can never admit a native arena).
    // Keep those reads on the existing view/fallback path after an escape,
    // disposal or backing-buffer exposure, too.
    if ctx.receiver_descriptors.contains_buffer_view(id) {
        return None;
    }
    let class = crate::type_analysis::receiver_class_name(ctx, object)
        .or_else(|| match ctx.module_global_proven_types.get(id) {
            Some(perry_hir::types::Type::Named(n)) => Some(n.clone()),
            _ => None,
        })
        .or_else(|| match ctx.local_type_hint(id) {
            Some(perry_hir::types::Type::Named(n)) => Some(n.clone()),
            _ => None,
        })?;
    match class.as_str() {
        "Int8Array" => Some(0),
        "Int16Array" => Some(2),
        "Uint16Array" => Some(3),
        "Int32Array" => Some(4),
        "Uint32Array" => Some(5),
        "Float32Array" => Some(6),
        "Float64Array" => Some(7),
        "Uint8ClampedArray" => Some(8),
        "Float16Array" => Some(11),
        _ => None,
    }
}

pub(crate) fn materialize_param(ctx: &mut FnCtx<'_>, id: u32, boxed: &str, kind: u8) {
    let result = ctx.block().call(
        I32,
        "js_ta_read_receiver_is_kind",
        &[(DOUBLE, boxed), (I32, &kind.to_string())],
    );
    let valid_i1 = ctx.block().icmp_ne(I32, &result, "0");
    ctx.receiver_descriptors
        .materialize_typed_read_param(id, valid_i1);
}

pub(crate) fn try_lower(
    ctx: &mut FnCtx<'_>,
    object: &Expr,
    index: &Expr,
    number_context: bool,
) -> Result<Option<String>> {
    let Some(kind) = receiver_kind(ctx, object) else {
        return Ok(None);
    };
    // Symbol keys have a separate property resolver. Other keys retain full
    // ToPropertyKey on the cold arm, including object keys that mutate storage.
    if super::compare::is_proven_symbol_expr(ctx, index) {
        return Ok(None);
    }
    let proof = match object {
        Expr::LocalGet(id) => ctx.receiver_descriptors.typed_read_param(*id).cloned(),
        _ => None,
    };
    let integer_index = super::index_get::numeric_index_has_integer_array_index_proof(ctx, index);
    crate::rooting::with_operands_rooted(ctx, &[object, index], |ctx, values| {
        Ok(Some(emit_get(
            ctx,
            &values[0],
            &values[1],
            kind,
            proof.as_deref(),
            number_context,
            integer_index,
        )))
    })
}

fn emit_get(
    ctx: &mut FnCtx<'_>,
    object: &str,
    key: &str,
    kind: u8,
    proof: Option<&str>,
    number_context: bool,
    integer_index: bool,
) -> String {
    let guard = ctx.new_block("ta.read.guard");
    let index = ctx.new_block("ta.read.index");
    let bounds = ctx.new_block("ta.read.bounds");
    let storage = ctx.new_block("ta.read.storage");
    let external = ctx.new_block("ta.read.external");
    let load = ctx.new_block("ta.read.load");
    let oob = ctx.new_block("ta.read.oob");
    let slow = ctx.new_block("ta.read.slow");
    let done = ctx.new_block("ta.read.done");
    let guard_l = ctx.block_label(guard);
    let index_l = ctx.block_label(index);
    let bounds_l = ctx.block_label(bounds);
    let storage_l = ctx.block_label(storage);
    let external_l = ctx.block_label(external);
    let load_l = ctx.block_label(load);
    let oob_l = ctx.block_label(oob);
    let slow_l = ctx.block_label(slow);
    let done_l = ctx.block_label(done);
    let bits = ctx.block().bitcast_double_to_i64(object);
    let raw = ctx.block().and(I64, &bits, crate::nanbox::POINTER_MASK_I64);
    let ready = if let Some(proof) = proof {
        proof.to_owned()
    } else {
        let tag = ctx.block().and(
            I64,
            &bits,
            &crate::nanbox::i64_literal(crate::nanbox::TAG_MASK),
        );
        let ptr = ctx
            .block()
            .icmp_eq(I64, &tag, crate::nanbox::POINTER_TAG_I64);
        let slot = ctx.block().lshr(I64, &raw, "3");
        let slot = ctx.block().and(I64, &slot, "63");
        let entry = ctx.block().gep(
            "[64 x i64]",
            "@PERRY_TA_KIND_CACHE",
            &[(I64, "0"), (I64, &slot)],
        );
        let entry = ctx.block().load(I64, &entry);
        let populated = ctx.block().icmp_ne(I64, &entry, "0");
        // Both inline and external cache tags prove kind; the storage guard
        // below distinguishes resolved ArrayBuffer slots from native arenas.
        let entry = ctx.block().and(I64, &entry, "-129");
        let expected = ctx.block().shl(I64, &raw, "8");
        let expected = ctx.block().or(I64, &expected, &kind.to_string());
        let hit = ctx.block().icmp_eq(I64, &entry, &expected);
        let hit = ctx.block().and(I1, &populated, &hit);
        ctx.block().and(I1, &ptr, &hit)
    };
    ctx.block().cond_br(&ready, &guard_l, &slow_l);
    ctx.current_block = guard;
    // The entry proof excludes native/foreign storage. A managed receiver
    // can only transition from inline to resolved on `.buffer` exposure.
    // Cache-only sites must still reject native storage before bounds, so a
    // disposed arena reaches its runtime even for an OOB key.
    let admitted_storage = if proof.is_some() {
        "true".to_owned()
    } else {
        let storage_addr = ctx.block().add(I64, &raw, "10");
        let storage_ptr = ctx.block().inttoptr(I64, &storage_addr);
        let storage = ctx.block().load(I8, &storage_ptr);
        let inline = ctx.block().icmp_eq(I8, &storage, "0");
        let resolved = ctx.block().icmp_eq(
            I8,
            &storage,
            &crate::runtime_abi::TA_STORAGE_RESOLVED.to_string(),
        );
        ctx.block().or(I1, &inline, &resolved)
    };
    let range = if integer_index {
        "true".to_owned()
    } else {
        let ge = ctx.block().fcmp("oge", key, "0.0");
        let lt = ctx.block().fcmp("olt", key, "4294967295.0");
        ctx.block().and(I1, &ge, &lt)
    };
    let range = ctx.block().and(I1, &range, &admitted_storage);
    ctx.block().cond_br(&range, &index_l, &slow_l);
    ctx.current_block = index;
    let idx = ctx.block().fptosi(DOUBLE, key, I64);
    let exact = if integer_index {
        "true".to_owned()
    } else {
        let back = ctx.block().sitofp(I64, &idx, DOUBLE);
        ctx.block().fcmp("oeq", key, &back)
    };
    ctx.block().cond_br(&exact, &bounds_l, &slow_l);
    ctx.current_block = bounds;
    let header = ctx.block().inttoptr(I64, &raw);
    let len = ctx.block().load(I32, &header);
    let len = ctx.block().zext(I32, &len, I64);
    let in_bounds = ctx.block().icmp_ult(I64, &idx, &len);
    ctx.block().cond_br(&in_bounds, &storage_l, &oob_l);
    ctx.current_block = storage;
    let slot = ctx
        .block()
        .add(I64, &raw, &crate::runtime_abi::TA_DATA_OFFSET.to_string());
    let slot_ptr = ctx.block().inttoptr(I64, &slot);
    let storage_addr = ctx.block().add(I64, &raw, "10");
    let storage_ptr = ctx.block().inttoptr(I64, &storage_addr);
    let storage_byte = ctx.block().load(I8, &storage_ptr);
    let inline = ctx.block().icmp_eq(I8, &storage_byte, "0");
    let inline_end = ctx.block().label.clone();
    ctx.block().cond_br(&inline, &load_l, &external_l);
    ctx.current_block = external;
    let resolved = if proof.is_some() {
        "true".to_owned()
    } else {
        ctx.block().icmp_eq(
            I8,
            &storage_byte,
            &crate::runtime_abi::TA_STORAGE_RESOLVED.to_string(),
        )
    };
    let pointer_block = ctx.new_block("ta.read.pointer");
    let pointer_l = ctx.block_label(pointer_block);
    ctx.block().cond_br(&resolved, &pointer_l, &slow_l);
    ctx.current_block = pointer_block;
    let data = ctx.block().load(PTR, &slot_ptr);
    let data = ctx.block().ptrtoint(&data, I64);
    let pointer_end = ctx.block().label.clone();
    ctx.block().br(&load_l);
    ctx.current_block = load;
    let data = ctx
        .block()
        .phi(I64, &[(&slot, &inline_end), (&data, &pointer_end)]);
    let width: u32 = match kind {
        0 | 1 | 8 => 1,
        2 | 3 | 11 => 2,
        4 | 5 | 6 => 4,
        _ => 8,
    };
    let off = ctx
        .block()
        .shl(I64, &idx, &width.trailing_zeros().to_string());
    let addr = ctx.block().add(I64, &data, &off);
    let ptr = ctx.block().inttoptr(I64, &addr);
    let target = ctx.target_triple.to_owned();
    let value = emit_element(ctx.block(), &target, &ptr, kind);
    let load_end = ctx.block().label.clone();
    ctx.block().br(&done_l);
    ctx.current_block = oob;
    let undefined = double_literal(if number_context {
        f64::NAN
    } else {
        f64::from_bits(TAG_UNDEFINED)
    });
    let oob_end = ctx.block().label.clone();
    ctx.block().br(&done_l);
    ctx.current_block = slow;
    // Preserve the boxed receiver on every miss. A lying annotation may
    // supply an SSO string or a primitive whose masked bits are no address.
    let fallback = ctx.block().call(
        DOUBLE,
        "js_dyn_index_get",
        &[(DOUBLE, object), (DOUBLE, key)],
    );
    let fallback = if number_context {
        ctx.block()
            .call(DOUBLE, "js_number_coerce", &[(DOUBLE, &fallback)])
    } else {
        fallback
    };
    let slow_end = ctx.block().label.clone();
    ctx.block().br(&done_l);
    ctx.current_block = done;
    ctx.block().phi(
        DOUBLE,
        &[
            (&value, &load_end),
            (&undefined, &oob_end),
            (&fallback, &slow_end),
        ],
    )
}

/// Relaxed integer lane loads also serve floats by bitcast. Each operation
/// reads exactly the element width, preserving shared backing semantics.
fn emit_element(blk: &mut crate::block::LlBlock, target: &str, ptr: &str, kind: u8) -> String {
    let (ty, width) = match kind {
        0 | 1 | 8 => (I8, 1),
        2 | 3 | 11 => (I16, 2),
        4 | 5 | 6 => (I32, 4),
        _ => (I64, 8),
    };
    let lane = if target.starts_with("x86_64") && width <= 2 {
        let reg = blk.next_reg();
        let instruction = if width == 1 { "movzbl" } else { "movzwl" };
        blk.emit_raw(format!("{reg} = call i32 asm sideeffect \"{instruction} ($1), $0\", \"=r,r,~{{memory}}\"(ptr {ptr}) \"gc-leaf-function\""));
        blk.trunc(I32, &reg, ty)
    } else {
        blk.load_atomic_monotonic(ty, ptr, width)
    };
    match kind {
        0 | 2 | 4 => blk.sitofp(ty, &lane, DOUBLE),
        1 | 3 | 5 | 8 => blk.uitofp(ty, &lane, DOUBLE),
        6 => {
            let f = blk.bitcast_i32_to_float(&lane);
            let f = blk.fpext(F32, &f, DOUBLE);
            super::nanbox_inline::canonicalize_lane_f64(blk, &f)
        }
        7 => {
            let f = blk.bitcast_i64_to_double(&lane);
            super::nanbox_inline::canonicalize_lane_f64(blk, &f)
        }
        11 => emit_half(blk, &lane),
        _ => unreachable!(),
    }
}

/// Half-to-double conversion is exact: normal exponents/mantissas are rebased
/// as bits; subnormals are integer multiples of 2^-24. Preserve signed zero,
/// infinities and canonical numeric NaNs without a runtime call or double
/// rounding. Stores retain the existing direct f64-to-half rounding helper.
fn emit_half(blk: &mut crate::block::LlBlock, lane: &str) -> String {
    let bits = blk.zext(I16, lane, I64);
    let magnitude = blk.and(I64, &bits, "32767");
    let exp = blk.and(I64, &bits, "31744");
    let mant = blk.and(I64, &bits, "1023");
    let normal = blk.add(I64, &magnitude, "1032192"); // (1023-15) << 10
    let normal = blk.shl(I64, &normal, "42");
    let special = blk.icmp_eq(I64, &exp, "31744");
    let nan = blk.icmp_ne(I64, &mant, "0");
    let special_bits = blk.select(I1, &nan, I64, "9221120237041090560", "9218868437227405312");
    let encoded = blk.select(I1, &special, I64, &special_bits, &normal);
    let sign = blk.and(I64, &bits, "32768");
    let sign = blk.shl(I64, &sign, "48");
    let encoded = blk.or(I64, &encoded, &sign);
    let normal = blk.bitcast_i64_to_double(&encoded);
    let sub = blk.uitofp(I64, &mant, DOUBLE);
    let sub = blk.fmul(&sub, &double_literal(2f64.powi(-24)));
    let sub_bits = blk.bitcast_double_to_i64(&sub);
    let sub_bits = blk.or(I64, &sub_bits, &sign);
    let sub = blk.bitcast_i64_to_double(&sub_bits);
    let zero_exp = blk.icmp_eq(I64, &exp, "0");
    let value = blk.select(I1, &zero_exp, DOUBLE, &sub, &normal);
    super::nanbox_inline::canonicalize_lane_f64(blk, &value)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::block::{LlBlock, RegCounter};
    use perry_hir::{types::Type, Function, Module, Param, Stmt};
    use std::rc::Rc;

    #[test]
    fn typed_loop_reads_resolve_kind_once_and_keep_live_storage() {
        for name in [
            "Int8Array",
            "Uint8ClampedArray",
            "Int16Array",
            "Uint16Array",
            "Int32Array",
            "Uint32Array",
            "Float16Array",
            "Float32Array",
            "Float64Array",
        ] {
            let param = |id, name: &str, ty| Param {
                id,
                name: name.to_owned(),
                ty,
                default: None,
                decorators: vec![],
                is_rest: false,
                arguments_object: None,
            };
            let mut module = Module::new("typed_loop_read.ts");
            module.functions.push(Function {
                id: 10,
                name: "scan".to_owned(),
                type_params: vec![],
                params: vec![
                    param(1, "a", Type::Named(name.to_owned())),
                    param(2, "i", Type::Number),
                ],
                return_type: Type::Any,
                body: vec![Stmt::While {
                    condition: Expr::LocalGet(2),
                    body: vec![Stmt::Return(Some(Expr::IndexGet {
                        object: Box::new(Expr::LocalGet(1)),
                        index: Box::new(Expr::LocalGet(2)),
                    }))],
                }],
                is_async: false,
                is_generator: false,
                is_strict: true,
                is_exported: false,
                captures: vec![],
                decorators: vec![],
                was_plain_async: false,
                was_unrolled: false,
            });
            let ir = String::from_utf8(
                crate::compile_module(
                    &module,
                    crate::CompileOptions {
                        emit_ir_only: true,
                        ..Default::default()
                    },
                )
                .unwrap(),
            )
            .unwrap();
            let bodies: Vec<_> = ir
                .split("\ndefine")
                .filter(|body| {
                    body.lines().next().is_some_and(|head| {
                        head.contains("double @perry_fn")
                            && head.contains("scan")
                            && body.contains("ta.read.load")
                    })
                })
                .collect();
            assert!(!bodies.is_empty());
            for body in bodies {
                assert_eq!(
                    body.matches("call i32 @js_ta_read_receiver_is_kind(")
                        .count(),
                    1,
                    "{name}: exactly one proof per normal/specialized body"
                );
                assert!(
                    body.contains("ta.read.pointer") && body.contains("ta.read.oob"),
                    "{name}: live storage/bounds missing"
                );
                assert!(
                    !body.contains("tav.width")
                        && !body.contains("call double @js_typed_array_get("),
                    "{name}: per-element dispatch returned"
                );
            }
        }
    }

    #[test]
    fn element_loads_keep_width_sign_and_leaf_conversion() {
        for target in ["x86_64-unknown-linux-gnu", "aarch64-apple-darwin"] {
            for kind in [0, 2, 3, 4, 5, 6, 7, 8, 11] {
                let mut blk = LlBlock::new("entry.0", Rc::new(RegCounter::new()));
                emit_element(&mut blk, target, "%data", kind);
                let ir = blk.to_ir();
                if matches!(kind, 0 | 2 | 4) {
                    assert!(ir.contains("sitofp"));
                }
                if matches!(kind, 3 | 5 | 8) {
                    assert!(ir.contains("uitofp"));
                }
                if kind == 11 {
                    assert!(ir.contains("shl i64") && !ir.contains("call double"));
                }
                if target.starts_with("x86_64") && matches!(kind, 0 | 2 | 3 | 8 | 11) {
                    assert!(ir.contains(if matches!(kind, 0 | 8) {
                        "movzbl"
                    } else {
                        "movzwl"
                    }));
                    assert!(ir.contains("gc-leaf-function") && ir.contains("~{memory}"));
                } else {
                    assert!(ir.contains("load atomic"));
                }
            }
        }
    }
}
