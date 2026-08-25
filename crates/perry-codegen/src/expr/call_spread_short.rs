//! Guarded direct calls for `receiver.method(fixed..., ...shortArray)` (#8772).
//!
//! The spread expression is still evaluated exactly once and in source order.
//! A non-allocating runtime proof then admits only an exact ordinary packed
//! Array with 0..=4 present elements. The method side is independently guarded
//! by the same `(class id, ShapeId, method invalidation slot)` proof used by
//! ordinary shape-directed method calls. Either miss joins the existing apply
//! path, which drives the full iterator protocol.

use anyhow::Result;
use perry_hir::{CallArg, Expr};

use crate::nanbox::double_literal;
use crate::native_value::LoweredValue;
use crate::types::{DOUBLE, I1, I32, I64, PTR};

use super::FnCtx;

const MAX_SPREAD_ARITY: usize = 4;
const MAX_METHOD_ARMS: usize = 8;

#[derive(Clone)]
struct DirectCandidate {
    class_id: u32,
    class_name: String,
    target: String,
    declared_count: usize,
}

/// Collect concrete class implementations in deterministic class-id order.
///
/// Rest/`arguments` bodies are intentionally left to the generic dispatcher:
/// their direct ABI allocates one or two argument arrays and would erase the
/// small-tail win. An omitted class is harmless because a guard miss always
/// reaches apply.
fn direct_candidates(ctx: &FnCtx<'_>, property: &str) -> Vec<DirectCandidate> {
    let mut roots: Vec<(&String, u32)> =
        ctx.class_ids.iter().map(|(name, &id)| (name, id)).collect();
    roots.sort_unstable_by(|a, b| a.1.cmp(&b.1).then_with(|| a.0.cmp(b.0)));

    let mut out = Vec::new();
    let mut seen = std::collections::HashSet::new();
    for (class_name, class_id) in roots {
        let Some(_keys_global) = ctx.class_keys_globals.get(class_name) else {
            continue;
        };
        let mut current = Some(class_name.clone());
        while let Some(owner) = current {
            let key = (owner.clone(), property.to_string());
            if let Some(public_target) = ctx.methods.get(&key) {
                let unsupported_abi = public_target.starts_with("perry_static_")
                    || matches!(ctx.method_has_rest.get(&key), Some(true))
                    || matches!(ctx.method_has_synthetic_arguments.get(&key), Some(true));
                if !unsupported_abi && seen.insert((class_id, public_target.clone())) {
                    let target = if owner == *class_name
                        && ctx
                            .pshape_methods
                            .contains_key(&(owner.clone(), property.to_string()))
                    {
                        crate::collectors::pshape_method_name(public_target)
                    } else {
                        public_target.clone()
                    };
                    out.push(DirectCandidate {
                        class_id,
                        class_name: class_name.clone(),
                        target,
                        declared_count: ctx.method_param_counts.get(&key).copied().unwrap_or(0),
                    });
                    if out.len() == MAX_METHOD_ARMS {
                        return out;
                    }
                }
                break;
            }
            current = ctx
                .classes
                .get(&owner)
                .and_then(|class| class.extends_name.clone());
        }
    }
    out
}

fn first_element_ptr(ctx: &mut FnCtx<'_>, alloca: &str, count: usize) -> String {
    let ptr = ctx.block().next_reg();
    ctx.block().emit_raw(format!(
        "{ptr} = getelementptr [{count} x double], ptr {alloca}, i64 0, i64 0"
    ));
    ptr
}

/// Try the #8772 lowering. `None` means the caller must retain its existing
/// source-ordered argument bundling path.
pub(crate) fn try_lower<'f, 'e>(
    ctx: &mut FnCtx<'f>,
    object: &'e Expr,
    property: &str,
    args: &'e [CallArg],
) -> Result<Option<String>> {
    let Some(CallArg::Spread(spread_expr)) = args.last() else {
        return Ok(None);
    };
    if args[..args.len() - 1]
        .iter()
        .any(|arg| !matches!(arg, CallArg::Expr(_)))
    {
        return Ok(None);
    }
    let candidates = direct_candidates(ctx, property);
    if candidates.is_empty() {
        return Ok(None);
    }

    // Receiver, fixed arguments, then the final spread expression: exactly the
    // ECMAScript evaluation order and exactly once each. One open group keeps
    // every pointer-bearing value current through both CFG diamonds and the
    // allocating generic fallback.
    let mut roots = crate::rooting::open_rooted_group(args.len() + 1);
    let recv_root = roots.lower(ctx, object, true)?;
    let mut fixed_roots = Vec::with_capacity(args.len().saturating_sub(1));
    for arg in &args[..args.len() - 1] {
        let CallArg::Expr(expr) = arg else {
            unreachable!()
        };
        fixed_roots.push(roots.lower(ctx, expr, true)?);
    }
    let spread_root = roots.lower(ctx, spread_expr, true)?;

    // Re-read once below all operand evaluation. The two guards from here to a
    // direct call are non-allocating; fallback re-reads again after its
    // materializer allocates.
    let fast_recv = roots.reread(ctx, recv_root)?;
    let fast_fixed: Vec<String> = fixed_roots
        .iter()
        .map(|&root| roots.reread(ctx, root))
        .collect::<Result<_>>()?;
    let fast_spread = roots.reread(ctx, spread_root)?;

    let values_alloca = ctx.func.alloca_entry_array(DOUBLE, MAX_SPREAD_ARITY);
    let values_ptr = first_element_ptr(ctx, &values_alloca, MAX_SPREAD_ARITY);
    let arity = ctx.block().call(
        I32,
        "js_short_packed_spread_values",
        &[(DOUBLE, &fast_spread), (PTR, &values_ptr)],
    );

    let method_probe_idx = ctx.new_block("short_spread.method_probe");
    let fallback_idx = ctx.new_block("short_spread.fallback");
    let merge_idx = ctx.new_block("short_spread.merge");
    let method_probe_label = ctx.block_label(method_probe_idx);
    let fallback_label = ctx.block_label(fallback_idx);
    let merge_label = ctx.block_label(merge_idx);
    let packed = ctx.block().icmp_sge(I32, &arity, "0");
    ctx.block()
        .cond_br(&packed, &method_probe_label, &fallback_label);

    // Load compiler-published ShapeIds once. Entry-init slots dominate this
    // whole diamond; these ordinary loads do not allocate.
    ctx.current_block = method_probe_idx;
    let expected_shapes: Vec<String> = candidates
        .iter()
        .map(|candidate| {
            let keys = ctx
                .class_keys_globals
                .get(&candidate.class_name)
                .expect("candidate required a keys global")
                .clone();
            crate::typed_shape::load_class_shape_id(ctx, &candidate.class_name, &keys)
        })
        .collect();
    let key_idx = ctx.strings.intern(property);
    let entry = ctx.strings.entry(key_idx);
    let method_guard_slot = (entry.dispatch_hash & 0xffff).to_string();
    let shape_out = ctx.func.alloca_entry(I32);
    let live_class = ctx.block().call(
        I32,
        "js_method_direct_shape_class",
        &[
            (DOUBLE, &fast_recv),
            (PTR, &shape_out),
            (I32, &method_guard_slot),
        ],
    );
    let live_shape = ctx.block().load(I32, &shape_out);

    let candidate_test_idxs: Vec<usize> = (0..candidates.len())
        .map(|index| ctx.new_block(&format!("short_spread.target_test{index}")))
        .collect();
    let candidate_select_idxs: Vec<usize> = (0..candidates.len())
        .map(|index| ctx.new_block(&format!("short_spread.target{index}")))
        .collect();
    let first_candidate_label = ctx.block_label(candidate_test_idxs[0]);
    ctx.block().br(&first_candidate_label);

    let mut phi_inputs: Vec<(String, String)> = Vec::new();
    let undefined = double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED));
    for (candidate_no, candidate) in candidates.iter().enumerate() {
        ctx.current_block = candidate_test_idxs[candidate_no];
        let target_label = ctx.block_label(candidate_select_idxs[candidate_no]);
        let miss_label = candidate_test_idxs
            .get(candidate_no + 1)
            .map(|&index| ctx.block_label(index))
            .unwrap_or_else(|| fallback_label.clone());
        let cid_ok = ctx
            .block()
            .icmp_eq(I32, &live_class, &candidate.class_id.to_string());
        let shape_ok = ctx
            .block()
            .icmp_eq(I32, &live_shape, &expected_shapes[candidate_no]);
        let target_ok = ctx.block().and(I1, &cid_ok, &shape_ok);
        ctx.block().cond_br(&target_ok, &target_label, &miss_label);

        ctx.current_block = candidate_select_idxs[candidate_no];
        let arity_blocks: Vec<usize> = (0..=MAX_SPREAD_ARITY)
            .map(|spread_arity| {
                ctx.new_block(&format!(
                    "short_spread.target{candidate_no}.arity{spread_arity}"
                ))
            })
            .collect();
        let arity_tests: Vec<usize> = (1..=MAX_SPREAD_ARITY)
            .map(|spread_arity| {
                ctx.new_block(&format!(
                    "short_spread.target{candidate_no}.arity_test{spread_arity}"
                ))
            })
            .collect();
        for spread_arity in 0..=MAX_SPREAD_ARITY {
            if spread_arity > 0 {
                ctx.current_block = arity_tests[spread_arity - 1];
            }
            let hit = ctx.block_label(arity_blocks[spread_arity]);
            let miss = arity_tests
                .get(spread_arity)
                .map(|&index| ctx.block_label(index))
                .unwrap_or_else(|| fallback_label.clone());
            let matches = ctx.block().icmp_eq(I32, &arity, &spread_arity.to_string());
            ctx.block().cond_br(&matches, &hit, &miss);
        }

        for (spread_arity, &block_idx) in arity_blocks.iter().enumerate() {
            ctx.current_block = block_idx;
            let mut user_args = fast_fixed.clone();
            for index in 0..spread_arity {
                let slot = ctx
                    .block()
                    .gep(DOUBLE, &values_alloca, &[(I64, &index.to_string())]);
                user_args.push(ctx.block().load(DOUBLE, &slot));
            }
            let mut direct_args = Vec::with_capacity(candidate.declared_count + 1);
            direct_args.push(fast_recv.clone());
            direct_args.extend(user_args.into_iter().take(candidate.declared_count));
            while direct_args.len() < candidate.declared_count + 1 {
                direct_args.push(undefined.clone());
            }
            let direct_slices: Vec<(crate::types::LlvmType, &str)> = direct_args
                .iter()
                .map(|value| (DOUBLE, value.as_str()))
                .collect();
            let value = ctx.block().call(DOUBLE, &candidate.target, &direct_slices);
            let after = ctx.block().label.clone();
            ctx.block().br(&merge_label);
            phi_inputs.push((value, after));
        }
    }

    // Every rejection shares the original apply dispatch. The helper only
    // constructs its source-ordered argument array; method lookup remains in
    // js_native_call_method_apply_by_id so overrides and wrong receivers retain
    // the generic semantics.
    ctx.current_block = fallback_idx;
    let (fixed_ptr, fixed_len) = if fixed_roots.is_empty() {
        ("null".to_string(), "0".to_string())
    } else {
        let fixed_alloca = ctx.func.alloca_entry_array(DOUBLE, fixed_roots.len());
        for (index, &root) in fixed_roots.iter().enumerate() {
            let value = roots.reread(ctx, root)?;
            let slot = ctx
                .block()
                .gep(DOUBLE, &fixed_alloca, &[(I64, &index.to_string())]);
            ctx.block().store(DOUBLE, &value, &slot);
        }
        (
            first_element_ptr(ctx, &fixed_alloca, fixed_roots.len()),
            fixed_roots.len().to_string(),
        )
    };
    let fallback_spread = roots.reread(ctx, spread_root)?;
    let args_array = ctx.block().call(
        I64,
        "js_spread_tail_fallback_args",
        &[
            (PTR, &fixed_ptr),
            (I64, &fixed_len),
            (DOUBLE, &fallback_spread),
        ],
    );
    let fallback_recv = roots.reread(ctx, recv_root)?;
    let dispatch_global = ctx.strings.static_dispatch_global(key_idx);
    let method_id = crate::strings::emit_static_dispatch_id(ctx.block(), &dispatch_global);
    let fallback_value = ctx.block().call(
        DOUBLE,
        "js_native_call_method_apply_by_id",
        &[
            (DOUBLE, &fallback_recv),
            (I64, &method_id),
            (I64, &args_array),
        ],
    );
    let fallback_after = ctx.block().label.clone();
    ctx.block().br(&merge_label);
    phi_inputs.push((fallback_value, fallback_after));

    ctx.current_block = merge_idx;
    let incoming: Vec<(&str, &str)> = phi_inputs
        .iter()
        .map(|(value, label)| (value.as_str(), label.as_str()))
        .collect();
    let result = ctx.block().phi(DOUBLE, &incoming);
    roots.release(ctx);

    let targets = candidates
        .iter()
        .map(|candidate| candidate.target.as_str())
        .collect::<Vec<_>>()
        .join(",");
    ctx.record_lowered_value(
        "MethodSpreadCall",
        None,
        "short_packed_spread_direct_call",
        &LoweredValue::js_value(result.clone()),
        None,
        None,
        None,
        false,
        false,
        vec![
            "packed_spread_arities=0,1,2,3,4".to_string(),
            format!("method={property}"),
            format!("direct_targets={targets}"),
            "spread_guard=exact_ordinary_packed_array,no_holes,max_length_4".to_string(),
            "iterator_guard=builtin_array_iterator,no_own_iterator,no_custom_prototype"
                .to_string(),
            "method_identity_guard=js_method_direct_shape_class(class_id,shape_id,invalidation_slot)"
                .to_string(),
            "generic_fallback=js_spread_tail_fallback_args+js_native_call_method_apply_by_id"
                .to_string(),
        ],
    );
    Ok(Some(result))
}
