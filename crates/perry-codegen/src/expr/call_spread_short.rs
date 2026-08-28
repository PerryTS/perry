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
use crate::rooting::Repr;
use crate::types::{DOUBLE, I1, I32, I64, PTR};

use super::FnCtx;

const MAX_SPREAD_ARITY: usize = 4;
const MAX_METHOD_ARMS: usize = 8;

#[derive(Clone)]
struct DirectCandidate {
    class_id: u32,
    target: String,
    declared_count: usize,
    shape: CandidateShape,
    needs_declare: bool,
}

#[derive(Clone)]
enum CandidateShape {
    Local {
        class_name: String,
        keys_global: String,
    },
    Foreign {
        cache_key: String,
        shape_id_global: String,
    },
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
    let mut seen_class_ids = std::collections::HashSet::new();
    for (class_name, class_id) in roots {
        let Some(keys_global) = ctx.class_keys_globals.get(class_name) else {
            continue;
        };
        let mut current = Some(class_name.clone());
        while let Some(owner) = current {
            let key = (owner.clone(), property.to_string());
            if let Some(public_target) = ctx.methods.get(&key) {
                let unsupported_abi = public_target.starts_with("perry_static_")
                    || matches!(ctx.method_has_rest.get(&key), Some(true))
                    || matches!(ctx.method_has_synthetic_arguments.get(&key), Some(true));
                if !unsupported_abi && seen_class_ids.insert(class_id) {
                    let target = if owner == *class_name
                        && ctx
                            .pshape_methods
                            .contains_key(&(owner.clone(), property.to_string()))
                    {
                        crate::collectors::pshape_method_name(public_target)
                    } else {
                        public_target.clone()
                    };
                    // Imported class stubs mint caller-local layout metadata;
                    // that ShapeId cannot match instances allocated by the
                    // producer. Prefer the producer's published slot whenever
                    // an own-method capability identifies this class id.
                    let shape = ctx
                        .short_spread_method_candidates
                        .get(property)
                        .and_then(|candidates| {
                            candidates
                                .iter()
                                .find(|candidate| candidate.class_id == class_id)
                        })
                        .map(|candidate| CandidateShape::Foreign {
                            cache_key: format!("#short-spread:{}", candidate.target),
                            shape_id_global: candidate.shape_id_global.clone(),
                        })
                        .unwrap_or_else(|| CandidateShape::Local {
                            class_name: class_name.clone(),
                            keys_global: keys_global.clone(),
                        });
                    out.push(DirectCandidate {
                        class_id,
                        target,
                        declared_count: ctx.method_param_counts.get(&key).copied().unwrap_or(0),
                        shape,
                        needs_declare: false,
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
    if let Some(reverse_candidates) = ctx.short_spread_method_candidates.get(property) {
        for candidate in reverse_candidates {
            if !seen_class_ids.insert(candidate.class_id) {
                continue;
            }
            out.push(DirectCandidate {
                class_id: candidate.class_id,
                target: candidate.target.clone(),
                declared_count: candidate.declared_count,
                shape: CandidateShape::Foreign {
                    cache_key: format!("#short-spread:{}", candidate.target),
                    shape_id_global: candidate.shape_id_global.clone(),
                },
                needs_declare: true,
            });
            if out.len() == MAX_METHOD_ARMS {
                break;
            }
        }
    }
    out
}

fn load_candidate_shape(ctx: &mut FnCtx<'_>, candidate: &DirectCandidate) -> String {
    match &candidate.shape {
        CandidateShape::Local {
            class_name,
            keys_global,
        } => crate::typed_shape::load_class_shape_id(ctx, class_name, keys_global),
        CandidateShape::Foreign {
            cache_key,
            shape_id_global,
        } => {
            let slot = if let Some(slot) = ctx.class_shape_slots.get(cache_key) {
                slot.clone()
            } else {
                let slot = ctx
                    .func
                    .entry_init_load_global(shape_id_global, crate::types::I32);
                ctx.class_shape_slots
                    .insert(cache_key.clone(), slot.clone());
                slot
            };
            ctx.block().load(I32, &slot)
        }
    }
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
    for candidate in &candidates {
        if candidate.needs_declare {
            ctx.pending_declares.push((
                candidate.target.clone(),
                DOUBLE,
                vec![DOUBLE; candidate.declared_count + 1],
            ));
        }
    }

    // Receiver, fixed arguments, then the final spread expression: exactly the
    // ECMAScript evaluation order and exactly once each. Root an evaluated
    // operand only across a *later operand evaluation* that can collect. Both
    // guards below are non-allocating and a successful direct call consumes
    // the values, so eagerly retaining every operand through the dispatch
    // diamond put three root barriers in perform-ecs's reset loop for no
    // safety gain. A miss installs its own cold, branch-local roots below.
    let mut operand_exprs = Vec::with_capacity(args.len() + 1);
    operand_exprs.push(object);
    for arg in &args[..args.len() - 1] {
        let CallArg::Expr(expr) = arg else {
            unreachable!()
        };
        operand_exprs.push(expr);
    }
    operand_exprs.push(spread_expr);
    let mut roots = crate::rooting::open_rooted_group(operand_exprs.len());
    let mut operand_roots = Vec::with_capacity(operand_exprs.len());
    for (index, expr) in operand_exprs.iter().enumerate() {
        let collects = crate::rooting::any_operand_may_collect(
            ctx,
            operand_exprs[index + 1..].iter().copied(),
        );
        operand_roots.push(roots.lower(ctx, expr, collects)?);
    }
    let recv_root = operand_roots[0];
    let fixed_roots = &operand_roots[1..operand_roots.len() - 1];
    let spread_root = operand_roots[operand_roots.len() - 1];

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
        .map(|candidate| load_candidate_shape(ctx, candidate))
        .collect();
    let key_idx = ctx.strings.intern(property);
    let entry = ctx.strings.entry(key_idx);
    let method_guard_slot = (entry.dispatch_hash & 0xffff).to_string();
    let (live_class, live_shape) =
        crate::lower_call::method_override::emit_inline_direct_method_shape_probe(
            ctx,
            &fast_recv,
            &method_guard_slot,
        );

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
    // No allocation has run since the fast operands were re-read. Publish all
    // of them only on this cold branch, immediately before the materializer's
    // first collection point. This group nests above `roots` and is released
    // before the merge, preserving temp-root stack order on both branches.
    let mut fallback_roots = crate::rooting::open_rooted_group(args.len() + 1);
    let fallback_recv_root = fallback_roots.adopt_emitted(ctx, Repr::Boxed, &fast_recv, true);
    let fallback_fixed_roots: Vec<_> = fast_fixed
        .iter()
        .map(|value| fallback_roots.adopt_emitted(ctx, Repr::Boxed, value, true))
        .collect();
    let fallback_spread_root = fallback_roots.adopt_emitted(ctx, Repr::Boxed, &fast_spread, true);
    let (fixed_ptr, fixed_len) = if fallback_fixed_roots.is_empty() {
        ("null".to_string(), "0".to_string())
    } else {
        let fixed_alloca = ctx
            .func
            .alloca_entry_array(DOUBLE, fallback_fixed_roots.len());
        for (index, &root) in fallback_fixed_roots.iter().enumerate() {
            let value = fallback_roots.reread_emitted(ctx, root);
            let slot = ctx
                .block()
                .gep(DOUBLE, &fixed_alloca, &[(I64, &index.to_string())]);
            ctx.block().store(DOUBLE, &value, &slot);
        }
        (
            first_element_ptr(ctx, &fixed_alloca, fallback_fixed_roots.len()),
            fallback_fixed_roots.len().to_string(),
        )
    };
    let fallback_spread = fallback_roots.reread_emitted(ctx, fallback_spread_root);
    let args_array = ctx.block().call(
        I64,
        "js_spread_tail_fallback_args",
        &[
            (PTR, &fixed_ptr),
            (I64, &fixed_len),
            (DOUBLE, &fallback_spread),
        ],
    );
    let fallback_recv = fallback_roots.reread_emitted(ctx, fallback_recv_root);
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
    fallback_roots.release(ctx);
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
            "candidate_scope=whole_program_producer_capabilities".to_string(),
            "operand_roots=collecting_evaluation_suffix_only;fallback_roots=guard_miss_only"
                .to_string(),
            "generic_fallback=js_spread_tail_fallback_args+js_native_call_method_apply_by_id"
                .to_string(),
        ],
    );
    Ok(Some(result))
}
