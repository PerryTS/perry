//! Kind-specialized checked reads of numeric typed arrays. Immutable loop
//! parameters and constructed module bindings use B4's access proof.
//! Other receivers use the same common owner header. Length and storage remain live, including after `.buffer`,
//! resize, detach and a traced backing rewrite. Native arenas keep their
//! disposal-aware runtime path.
use super::FnCtx;
use crate::nanbox::{double_literal, TAG_UNDEFINED};
use crate::types::{DOUBLE, F32, I1, I16, I32, I64, I8};
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
        "Uint8Array" | "Buffer" if super::u8_buffer_read::u8_inline_read_enabled() => Some(1),
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

fn byte_type(ty: &perry_hir::types::Type) -> bool {
    use perry_hir::types::Type;
    match ty {
        Type::Named(name) => matches!(name.as_str(), "Uint8Array" | "Buffer"),
        Type::Union(types) => !types.is_empty() && types.iter().all(byte_type),
        _ => false,
    }
}

/// An annotation selects a checked access, never the value's numeric type.
/// A stable construction or a dominating byte-brand guard proves that a
/// numeric key can only read a byte or undefined.
pub(crate) fn byte_receiver_is_proven(ctx: &FnCtx<'_>, object: &Expr) -> bool {
    let Expr::LocalGet(id) = object else {
        return false;
    };
    if ctx.reassigned_locals.contains(id) {
        return false;
    }
    ctx.stable_local_type_proof(id).is_some_and(byte_type)
        || ctx
            .module_global_proven_types
            .get(id)
            .is_some_and(byte_type)
}

pub(crate) fn byte_read_is_numeric(ctx: &FnCtx<'_>, expr: &Expr) -> bool {
    let (object, index) = match expr {
        Expr::Uint8ArrayGet { array, index } => (array, index),
        Expr::BufferIndexGet { buffer, index } => (buffer, index),
        _ => return false,
    };
    byte_receiver_is_proven(ctx, object)
        && crate::type_analysis::is_numeric_expr(ctx, index)
        && !crate::type_analysis::expr_may_return_boxed_value_from_raw_f64_fallback(ctx, index)
}

/// Register B4's loop proofs before any body call is emitted. In particular,
/// a callback before the first global-table read must dirty that proof on
/// every iteration, including iterations after its initial admission.
pub(crate) fn prepare_loop_accesses(ctx: &mut FnCtx<'_>, body: &[perry_hir::Stmt]) {
    if ctx.is_async_fn || ctx.disable_buffer_fast_path
        || !super::ta_param_f64_read::ta_param_f64_read_enabled()
    {
        return;
    }
    // Declared types only select a checked brand guard; they are not admission
    // evidence. Register before initializers too, since a loop-local initializer
    // can call back while the same receiver survives from an earlier iteration.
    let mut declared = std::collections::HashMap::new();
    crate::boxed_vars::collect_let_types_in_stmts(body, &mut declared);
    let mut referenced = std::collections::HashSet::new();
    crate::collectors::collect_ref_ids_in_stmts(body, &mut referenced);
    let ids: std::collections::BTreeSet<u32> = referenced.into_iter().collect();
    for id in ids {
        if ctx.boxed_vars.contains(&id)
            || (!ctx.module_global_proven_types.contains_key(&id)
                && !ctx.locals.contains_key(&id)
                && !declared.contains_key(&id))
            || !super::u8_buffer_read::loop_param_is_accessed(body, id)
        {
            continue;
        }
        let Some(kind) = receiver_kind(ctx, &Expr::LocalGet(id)).or_else(|| {
            (!ctx.reassigned_locals.contains(&id)
                && !ctx.receiver_descriptors.contains_buffer_view(&id)
                && super::u8_buffer_read::u8_inline_read_enabled()
                && declared.get(&id).is_some_and(byte_type))
                .then_some(1)
        }) else {
            continue;
        };
        let brand = [super::byte_cell::brand_for_kind(kind)];
        let brands = if kind == 1 {
            &super::u8_buffer_read::U8_BRANDS[..]
        } else {
            &brand[..]
        };
        let mut access = super::byte_cell::install_loop_access(ctx, id, brands);
        // Construction plus the existing single-definition/no-reassignment
        // proof makes this binding invariant. Calls still dirty its storage
        // proof, including detach, resize and moving collection.
        if ctx.module_globals.contains_key(&id) && ctx.module_global_proven_types.contains_key(&id)
        {
            access.fixed_receiver = true;
            ctx.receiver_descriptors
                .materialize_byte_view_param(id, access);
        }
    }
}

pub(crate) fn materialize_param(ctx: &mut FnCtx<'_>, id: u32, boxed: &str, kind: u8) {
    let brand = [super::byte_cell::brand_for_kind(kind)];
    let brands = if kind == 1 {
        &super::u8_buffer_read::U8_BRANDS[..]
    } else {
        &brand[..]
    };
    super::byte_cell::materialize_param(ctx, id, boxed, brands);
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
    let integer_index = super::index_get::numeric_index_has_integer_array_index_proof(ctx, index);
    crate::rooting::with_operands_rooted(ctx, &[object, index], |ctx, values| {
        let brand = [super::byte_cell::brand_for_kind(kind)];
        let brands = if kind == 1 {
            &super::u8_buffer_read::U8_BRANDS[..]
        } else {
            &brand[..]
        };
        let proof = super::u8_buffer_read::byte_view_param_for(ctx, object, &values[0], brands);
        Ok(Some(emit_get(
            ctx,
            &values[0],
            &values[1],
            kind,
            proof.as_ref(),
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
    proof: Option<&crate::collectors::ByteViewParamAccess>,
    number_context: bool,
    integer_index: bool,
) -> String {
    let guard = ctx.new_block("ta.read.guard");
    let index = ctx.new_block("ta.read.index");
    let bounds = ctx.new_block("ta.read.bounds");
    let storage = ctx.new_block("ta.read.storage");
    let load = ctx.new_block("ta.read.load");
    let oob = ctx.new_block("ta.read.oob");
    let slow = ctx.new_block("ta.read.slow");
    let done = ctx.new_block("ta.read.done");
    let guard_l = ctx.block_label(guard);
    let index_l = ctx.block_label(index);
    let bounds_l = ctx.block_label(bounds);
    let storage_l = ctx.block_label(storage);
    let load_l = ctx.block_label(load);
    let oob_l = ctx.block_label(oob);
    let slow_l = ctx.block_label(slow);
    let done_l = ctx.block_label(done);
    let brand = [super::byte_cell::brand_for_kind(kind)];
    let brands = if kind == 1 {
        &super::u8_buffer_read::U8_BRANDS[..]
    } else {
        &brand[..]
    };
    let access = if let Some(param) = proof {
        let admitted = ctx.new_block("ta.read.hoisted");
        let admitted_l = ctx.block_label(admitted);
        ctx.block().cond_br(&param.valid_i1, &admitted_l, &slow_l);
        ctx.current_block = admitted;
        let len = ctx.block().load(I32, &param.length_slot);
        super::byte_cell::Access {
            word: String::new(),
            raw: String::new(),
            owner: String::new(),
            data: param.data_i64.clone(),
            len,
        }
    } else {
        super::byte_cell::resolve(ctx, object, brands, &slow_l)
    };
    let admitted_storage = "true".to_owned();
    ctx.block().br(&guard_l);
    ctx.current_block = guard;
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
    let len = ctx.block().zext(I32, &access.len, I64);
    let in_bounds = ctx.block().icmp_ult(I64, &idx, &len);
    ctx.block().cond_br(&in_bounds, &storage_l, &oob_l);
    ctx.current_block = storage;
    ctx.block().br(&load_l);
    ctx.current_block = load;
    let data = access.data;
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
    let value = emit_element(ctx.block(), &ptr, kind);
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

/// The header guard excludes shared owners. Read one ordinary lane of the
/// exact element width; shared storage uses the atomic runtime arm.
pub(super) fn emit_element(blk: &mut crate::block::LlBlock, ptr: &str, kind: u8) -> String {
    let ty = match kind {
        0 | 1 | 8 => I8,
        2 | 3 | 11 => I16,
        4 | 5 | 6 => I32,
        _ => I64,
    };
    let lane = blk.load(ty, ptr);
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
                assert!(
                    !body.contains("@PERRY_TA_KIND_CACHE")
                        && !body.contains("@PERRY_U8_INLINE_CACHE")
                );
                let marker = body
                    .lines()
                    .find(|l| l.contains("; bytes.hoist.roots "))
                    .expect("one owner/receiver hoist");
                let roots = crate::testing::root_slots::bound_slots(body);
                for field in ["receiver=", "owner="] {
                    let slot = marker
                        .split(field)
                        .nth(1)
                        .unwrap()
                        .split_whitespace()
                        .next()
                        .unwrap();
                    let slot = format!("%{slot}");
                    assert!(
                        roots.contains_key(&slot)
                            || body.contains(&format!("{slot} = alloca ptr addrspace(1)")),
                        "{name}: {field}{slot} must be a statepoint root"
                    );
                }
                assert!(
                    body.contains("ta.read.hoisted") && body.contains("ta.read.oob"),
                    "{name}: hoisted storage/bounds missing"
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
    fn dropping_the_hoisted_owner_root_turns_the_root_invariant_red() {
        let child = std::process::Command::new(std::env::current_exe().unwrap())
            .args(["--exact", "expr::ta_element_read::tests::typed_loop_reads_resolve_kind_once_and_keep_live_storage", "--nocapture"])
            .env("PERRY_B4_SABOTAGE", "hoist_owner").output().unwrap();
        assert!(String::from_utf8_lossy(&child.stdout).contains("running 1 test"));
        assert!(
            !child.status.success(),
            "dropping the owner statepoint root must turn the invariant red"
        );
    }

    #[test]
    fn element_loads_keep_width_sign_and_leaf_conversion() {
        for kind in [0, 1, 2, 3, 4, 5, 6, 7, 8, 11] {
            let mut blk = LlBlock::new("entry.0", Rc::new(RegCounter::new()));
            emit_element(&mut blk, "%data", kind);
            let ir = blk.to_ir();
            if matches!(kind, 0 | 2 | 4) {
                assert!(ir.contains("sitofp"));
            }
            if matches!(kind, 1 | 3 | 5 | 8) {
                assert!(ir.contains("uitofp"));
            }
            if kind == 11 {
                assert!(ir.contains("shl i64") && !ir.contains("call double"));
            }
            assert!(ir.contains("load ") && !ir.contains("load atomic") && !ir.contains("asm"));
        }
    }
}
