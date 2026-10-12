//! Shape-authorized direct calls of compiled class accessors.
//! The shared guard consumes immutable receiver/holder candidates; misses use
//! ordinary property semantics. There is no site cache or accessor-pair load.
use super::{lower_expr, FnCtx};
use crate::types::{DOUBLE, I32, I64};
use anyhow::Result;
use perry_hir::Expr;

pub(super) fn candidate(
    ctx: &FnCtx<'_>,
    object: &Expr,
    property: &str,
    setter: bool,
) -> Option<(String, String, bool)> {
    if crate::target_layout::target_is_ilp32(ctx.target_triple)
        || property.starts_with('#')
        || ctx.is_static_class_this(object)
    {
        return None;
    }
    let class = crate::type_analysis::receiver_class_name(ctx, object)
        .or_else(|| super::property_get::guarded_declared_class_get_candidate(ctx, object))?;
    let (function, chain) =
        crate::codegen::static_prototype::accessor_chain(ctx, &class, property, setter)?;
    let keys = ctx.class_keys_globals.get(&class)?.clone();
    let birth = crate::codegen::static_shape_ids::static_birth_content(&keys)?;
    if birth
        .keys
        .split(|&b| b == 0)
        .take(birth.key_count as usize)
        .any(|k| k == property.as_bytes())
    {
        return None;
    }
    // The immutable candidate holds only ShapeIds, not heap references.
    // Emission re-derives the same chain from this receiver class.
    let mut contained = ctx.ptr_shape_receiver_fact(object).is_some_and(|fact| {
        fact.class_name == class
            && matches!(object, Expr::LocalGet(id)
            if ctx.native_facts.shape_proven_ptr_local(*id).is_some())
    });
    let key = (
        class.clone(),
        format!("__{}_{property}", if setter { "set" } else { "get" }),
    );
    let owner_id = chain.last()?.0;
    let owner = ctx.class_ids.iter().find(|(_, id)| **id == owner_id)?.0;
    let declaration = ctx.classes.get(owner)?;
    let method = (if setter {
        &declaration.setters
    } else {
        &declaration.getters
    })
    .iter()
    .find(|(p, _)| p == property)?
    .1
    .clone();
    // A closed layout proof need not prove its slots stay numeric. The pure
    // numeric body consumes F64 lanes, so its guard-free entry must meet the
    // existing exhaustive reachable-store proof as well.
    let numeric = crate::codegen::accessor_numeric::fields(
        ctx.classes.get(&class)?,
        &method,
        ctx.classes,
        &keys,
    );
    contained &= ctx
        .ptr_shape_receiver_fact(object)
        .is_some_and(|fact| numeric.is_subset(&fact.numeric_fields));
    let safe_body = contained
        || crate::codegen::accessor_numeric::preserves_layout(
            ctx.classes.get(&class)?,
            &method,
            ctx.classes,
            &keys,
        );
    let function = if safe_body && ctx.pshape_methods.contains_key(&key) {
        let public = ctx.methods.get(&key)?;
        let public = if chain.len() == 1 {
            public.clone()
        } else {
            crate::codegen::inherited_accessors::receiver_body_name(public, ctx.class_ids[&class])
        };
        crate::collectors::pshape_method_name(&public)
    } else {
        function
    };
    Some((function, keys, contained))
}

/// A guard-free pure numeric body is a result contract, not a return-type
/// annotation. Consumers may carry it through ordinary arithmetic lowering.
pub(crate) fn contained_number_get(ctx: &FnCtx<'_>, object: &Expr, property: &str) -> bool {
    let Some((_, keys, true)) = candidate(ctx, object, property, false) else {
        return false;
    };
    let Some(fact) = ctx.ptr_shape_receiver_fact(object) else {
        return false;
    };
    let Some((owner, _)) = crate::codegen::static_prototype::accessor_owner(
        ctx.classes,
        ctx.class_ids,
        &fact.class_name,
        property,
        false,
    ) else {
        return false;
    };
    let method = &ctx.classes[&owner]
        .getters
        .iter()
        .find(|(p, _)| p == property)
        .unwrap()
        .1;
    !crate::codegen::accessor_numeric::fields(
        ctx.classes[&fact.class_name],
        method,
        ctx.classes,
        &keys,
    )
    .is_empty()
}

pub(super) fn get(
    ctx: &mut FnCtx<'_>,
    object: &Expr,
    property: &str,
    function: &str,
    keys: &str,
    contained: bool,
) -> Result<String> {
    let receiver = lower_expr(ctx, object)?;
    emit(
        ctx, &receiver, property, function, keys, None, false, contained, false,
    )
}

pub(super) fn emit(
    ctx: &mut FnCtx<'_>,
    receiver: &str,
    property: &str,
    function: &str,
    keys: &str,
    value: Option<&str>,
    strict: bool,
    contained: bool,
    value_i32: bool,
) -> Result<String> {
    let fast = ctx.new_block("named_accessor.hit");
    let slow = ctx.new_block("named_accessor.miss");
    let merge = ctx.new_block("named_accessor.merge");
    let fast_label = ctx.block_label(fast);
    let slow_label = ctx.block_label(slow);
    let merge_label = ctx.block_label(merge);
    let class = ctx
        .class_keys_globals
        .iter()
        .find(|(_, k)| k.as_str() == keys)
        .map(|(name, _)| name.clone())
        .expect("accessor receiver class");
    let (_, chain) =
        crate::codegen::static_prototype::accessor_chain(ctx, &class, property, value.is_some())
            .expect("accessor holder candidate");
    let slot = ctx.ic_site_counter;
    ctx.ic_site_counter += 1;
    let global = format!(
        "perry_accessor_candidate_{}__{slot}",
        ctx.strings.module_prefix()
    );
    let words: Vec<_> = chain
        .iter()
        .map(|&(cid, shape)| format!("i64 {}", (u64::from(shape) << 32) | u64::from(cid)))
        .collect();
    ctx.typed_parse_rodata.push(format!(
        "@{global} = private constant [{} x i64] [{}], align 8",
        words.len(),
        words.join(", ")
    ));
    let expected = crate::typed_shape::class_shape_id_operand_on_block(ctx.block(), keys, true);
    if contained {
        // The existing closed-receiver proof already establishes this class's
        // lifetime layout and unmodified prototype. Consume that authority
        // exactly as ordinary method and field sites do.
        ctx.block().br(&fast_label);
    } else {
        emit_general_guard(
            ctx,
            receiver,
            &expected,
            &chain,
            &global,
            &fast_label,
            &slow_label,
        );
    }
    ctx.current_block = fast;
    let mut args = vec![(DOUBLE, receiver)];
    if let Some(v) = value {
        if value_i32 {
            // Carry the caller's existing canonical i32 representation through
            // normal inlining. This is a semantic fact, never an annotation.
            let in_range = super::binary::emit_is_int64_exact_number(ctx, v);
            ctx.block()
                .emit_raw(format!("call void @llvm.assume(i1 {in_range})"));
        }
        args.push((DOUBLE, v));
    }
    let result = ctx.block().call(DOUBLE, function, &args);
    ctx.block().br(&merge_label);
    ctx.current_block = slow;
    let index = ctx.strings.intern(property);
    let global = format!("@{}", ctx.strings.entry(index).handle_global);
    let key = ctx.block().load(DOUBLE, &global);
    let cold = if let Some(v) = value {
        let strict = if strict { "1" } else { "0" };
        ctx.block().call(
            DOUBLE,
            "js_put_value_set",
            &[
                (DOUBLE, receiver),
                (DOUBLE, &key),
                (DOUBLE, v),
                (DOUBLE, receiver),
                (I32, strict),
            ],
        )
    } else {
        let bytes = format!("@{}", ctx.strings.entry(index).bytes_global);
        let name = ctx.block().ptrtoint(&bytes, I64);
        ctx.block().call(
            DOUBLE,
            "js_get_property",
            &[
                (DOUBLE, receiver),
                (I64, &name),
                (I64, &property.len().to_string()),
            ],
        )
    };
    ctx.block().br(&merge_label);
    ctx.current_block = merge;
    Ok(ctx
        .block()
        .phi(DOUBLE, &[(&result, &fast_label), (&cold, &slow_label)]))
}

/// One shared proof keeps arbitrary receivers out of per-site dispatch
/// lattices. Only the call and its result branch remain at the site.
fn emit_general_guard(
    ctx: &mut FnCtx<'_>,
    receiver: &str,
    expected: &str,
    chain: &[(u32, u32)],
    candidate: &str,
    fast: &str,
    slow: &str,
) {
    use crate::types::PTR;
    let pass = ctx.block().call_gc_leaf(
        I32,
        "js_class_accessor_shape_guard",
        &[
            (DOUBLE, receiver),
            (I32, expected),
            (PTR, &format!("@{candidate}")),
            (I64, &chain.len().to_string()),
        ],
    );
    let valid = ctx.block().icmp_ne(I32, &pass, "0");
    ctx.block().cond_br(&valid, fast, slow);
}
