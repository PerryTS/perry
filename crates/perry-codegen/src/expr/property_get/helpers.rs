//! Free helper functions extracted from `property_get.rs`.
//!
//! Pure mechanical move — bodies are verbatim. Visibility widened to
//! `pub(crate)` so both the trunk's guarded arms and the sibling general
//! dispatch can reach them.
//!
//! # Rooting (Layer 1, slice 4)
//!
//! Listed in `crate::rooting`'s `MIGRATED_MODULES`, and the listing is
//! **vacuous on the committed source**: this module has never named an
//! `expr::temp_root` symbol, so only the sabotage arm makes the line an
//! assertion. The audit that earned it: these helpers receive the receiver
//! already lowered and lower no user expression, so no operand window opens
//! inside them. The class-field guard diamond does hold a derived
//! `obj_bits`/`obj_handle` across `js_typed_feedback_class_field_get_guard`;
//! that shape is a *derived raw pointer*, which no temp root can name and which
//! `crate::rooting` therefore cannot express — it is recorded in #7640, not
//! papered over here.

use super::*;

use anyhow::Result;
use perry_hir::Expr;

use crate::native_value::{
    BoundsState, BufferAccessMode, LoweredValue, MaterializationReason, NativeRep, SemanticKind,
};
use crate::type_analysis::receiver_class_name;
use crate::types::{DOUBLE, I1, I32, I64, I8, PTR};

pub(crate) fn class_has_computed_runtime_members(ctx: &FnCtx<'_>, class_name: &str) -> bool {
    ctx.classes
        .get(class_name)
        .is_some_and(|class| !class.computed_members.is_empty())
}

pub(crate) fn lower_runtime_property_get_by_name(
    ctx: &mut FnCtx<'_>,
    object: &Expr,
    property: &str,
) -> Result<String> {
    let recv_box = lower_expr(ctx, object)?;
    let key_idx = ctx.strings.intern(property);
    let dispatch_global = ctx.strings.static_dispatch_global(key_idx);
    let blk = ctx.block();
    let obj_bits = blk.bitcast_double_to_i64(&recv_box);
    // The helper takes a raw `*const ObjectHeader`, so strip the NaN-box
    // POINTER_TAG to a canonical pointer.
    let obj_handle = blk.and(I64, &obj_bits, crate::nanbox::POINTER_MASK_I64);
    let property_id = crate::strings::emit_static_dispatch_id(blk, &dispatch_global);
    Ok(blk.call(
        DOUBLE,
        "js_object_get_field_by_property_id_f64",
        &[(I64, &obj_handle), (I64, &property_id)],
    ))
}

pub(crate) fn lower_class_method_bind(
    ctx: &mut FnCtx<'_>,
    object: &Expr,
    method_name: &str,
) -> Result<String> {
    let recv_box = lower_expr(ctx, object)?;
    let key_idx = ctx.strings.intern(method_name);
    let dispatch_global = ctx.strings.static_dispatch_global(key_idx);
    let blk = ctx.block();
    let method_id = crate::strings::emit_static_dispatch_id(blk, &dispatch_global);
    Ok(blk.call(
        DOUBLE,
        "js_class_method_bind_by_id",
        &[(DOUBLE, &recv_box), (I64, &method_id)],
    ))
}

pub(crate) fn is_primitive_builtin_proto_method(builtin_name: &str, method_name: &str) -> bool {
    match builtin_name {
        "Number" => matches!(
            method_name,
            "toExponential" | "toFixed" | "toLocaleString" | "toPrecision" | "toString" | "valueOf"
        ),
        "Boolean" | "Symbol" => matches!(method_name, "toString" | "valueOf"),
        "BigInt" => matches!(method_name, "toString" | "valueOf"),
        _ => false,
    }
}

pub(crate) fn builtin_prototype_method_read<'a>(
    object: &'a Expr,
    property: &'a str,
) -> Option<(&'a str, &'a str)> {
    let Expr::PropertyGet {
        object: ctor_object,
        property: proto_property,
        ..
    } = object
    else {
        return None;
    };
    if proto_property != "prototype" {
        return None;
    }
    let Expr::PropertyGet {
        object: global_object,
        property: builtin_name,
        ..
    } = ctor_object.as_ref()
    else {
        return None;
    };
    if !matches!(global_object.as_ref(), Expr::GlobalGet(_)) {
        return None;
    }
    is_primitive_builtin_proto_method(builtin_name, property)
        .then_some((builtin_name.as_str(), property))
}

pub(crate) fn is_global_builtin_value_expr(expr: &Expr, name: &str) -> bool {
    matches!(
        expr,
        Expr::PropertyGet { object, property, .. }
            if property == name && matches!(object.as_ref(), Expr::GlobalGet(_))
    )
}

pub(crate) fn promise_static_function_length_expr(expr: &Expr) -> Option<u32> {
    let Expr::PropertyGet {
        object, property, ..
    } = expr
    else {
        return None;
    };
    let is_promise_receiver = matches!(object.as_ref(), Expr::GlobalGet(_))
        || is_global_builtin_value_expr(object, "Promise");
    if !is_promise_receiver {
        return None;
    }
    match property.as_str() {
        "withResolvers" => Some(0),
        "resolve" | "reject" | "all" | "race" | "allSettled" | "any" | "try" => Some(1),
        _ => None,
    }
}

pub(crate) fn lower_global_builtin_static_value(
    ctx: &mut FnCtx<'_>,
    builtin: &str,
    property: &str,
) -> String {
    if builtin == "Promise" {
        let key_idx = ctx.strings.intern(property);
        let key_bytes_global = format!("@{}", ctx.strings.entry(key_idx).bytes_global);
        let key_len = property.len().to_string();
        return ctx.block().call(
            DOUBLE,
            "js_promise_static_function_value",
            &[(PTR, &key_bytes_global), (I64, &key_len)],
        );
    }

    let builtin_idx = ctx.strings.intern(builtin);
    let builtin_bytes_global = format!("@{}", ctx.strings.entry(builtin_idx).bytes_global);
    let builtin_len = builtin.len().to_string();
    let builtin_value = ctx.block().call(
        DOUBLE,
        "js_get_global_this_builtin_value",
        &[(PTR, &builtin_bytes_global), (I64, &builtin_len)],
    );
    let key_idx = ctx.strings.intern(property);
    let key_handle_global = format!("@{}", ctx.strings.entry(key_idx).handle_global);
    let blk = ctx.block();
    let builtin_handle = unbox_to_i64(blk, &builtin_value);
    let key_box = blk.load(DOUBLE, &key_handle_global);
    let key_bits = blk.bitcast_double_to_i64(&key_box);
    let key_raw = blk.and(I64, &key_bits, POINTER_MASK_I64);
    blk.call(
        DOUBLE,
        "js_object_get_field_by_name_f64",
        &[(I64, &builtin_handle), (I64, &key_raw)],
    )
}

pub(crate) fn lower_raw_f64_class_field_get_for_number_context(
    ctx: &mut FnCtx<'_>,
    expr: &Expr,
) -> Result<Option<String>> {
    let Expr::PropertyGet {
        object, property, ..
    } = expr
    else {
        return Ok(None);
    };

    // Scalar-replaced objects do not have a valid heap receiver. The general
    // property-get lowering handles this, but native-f64 numeric contexts query
    // raw class-field lowering first. Keep allocation-elided objects on their
    // scalar slots rather than feeding a dummy/uninitialized receiver into the
    // class-field guard path.
    if let Expr::LocalGet(id) = object.as_ref() {
        if let Some(slot) = ctx
            .scalar_replaced
            .get(id)
            .and_then(|fs| fs.get(property.as_str()))
            .cloned()
        {
            let declared_raw_f64 = crate::type_analysis::scalar_replaced_field_is_raw_f64(
                ctx,
                object.as_ref(),
                property,
            );
            let raw_f64_field = crate::type_analysis::scalar_replaced_field_raw_f64_store_state(
                ctx,
                Some(*id),
                property,
                declared_raw_f64,
            );
            if !raw_f64_field {
                return Ok(None);
            }
            let value = ctx.block().load(DOUBLE, &slot);
            let lowered_js = LoweredValue {
                semantic: SemanticKind::JsValue,
                rep: NativeRep::JsValue,
                llvm_ty: DOUBLE,
                value: value.clone(),
            };
            ctx.record_lowered_value_with_access_mode(
                "ScalarObjectFieldGet",
                Some(*id),
                "scalar_object_field_load",
                &lowered_js,
                None,
                None,
                None,
                None,
                false,
                false,
                vec![
                    format!("field={}", property),
                    format!("raw_f64_field={}", raw_f64_field as u8),
                    "number_context=true".to_string(),
                ],
            );
            let lowered_f64 = LoweredValue::f64(value.clone());
            ctx.record_lowered_value_with_access_mode(
                "ScalarObjectFieldGet",
                Some(*id),
                "scalar_object_field_load.raw_f64",
                &lowered_f64,
                None,
                None,
                None,
                None,
                false,
                false,
                vec![
                    format!("field={}", property),
                    "raw_f64_field=1".to_string(),
                    "number_context=true".to_string(),
                ],
            );
            return Ok(Some(value));
        }
    }

    if let Expr::This = object.as_ref() {
        if let Some(target_id) = ctx.scalar_ctor_target.last().copied() {
            if let Some(slot) = ctx
                .scalar_replaced
                .get(&target_id)
                .and_then(|fs| fs.get(property.as_str()))
                .cloned()
            {
                let declared_raw_f64 = crate::type_analysis::scalar_replaced_field_is_raw_f64(
                    ctx,
                    object.as_ref(),
                    property,
                );
                let raw_f64_field = crate::type_analysis::scalar_replaced_field_raw_f64_store_state(
                    ctx,
                    Some(target_id),
                    property,
                    declared_raw_f64,
                );
                if !raw_f64_field {
                    return Ok(None);
                }
                let value = ctx.block().load(DOUBLE, &slot);
                let lowered_js = LoweredValue {
                    semantic: SemanticKind::JsValue,
                    rep: NativeRep::JsValue,
                    llvm_ty: DOUBLE,
                    value: value.clone(),
                };
                ctx.record_lowered_value_with_access_mode(
                    "ScalarThisFieldGet",
                    Some(target_id),
                    "scalar_object_field_load",
                    &lowered_js,
                    None,
                    None,
                    None,
                    None,
                    false,
                    false,
                    vec![
                        format!("field={}", property),
                        format!("raw_f64_field={}", raw_f64_field as u8),
                        "number_context=true".to_string(),
                    ],
                );
                let lowered_f64 = LoweredValue::f64(value.clone());
                ctx.record_lowered_value_with_access_mode(
                    "ScalarThisFieldGet",
                    Some(target_id),
                    "scalar_object_field_load.raw_f64",
                    &lowered_f64,
                    None,
                    None,
                    None,
                    None,
                    false,
                    false,
                    vec![
                        format!("field={}", property),
                        "raw_f64_field=1".to_string(),
                        "number_context=true".to_string(),
                    ],
                );
                return Ok(Some(value));
            }
        }
    }

    // repsel #7480 / #5093: inside the fast clone of an ELEMENT-shape
    // versioned loop, `arr[i].field` in number context lowers to a bare
    // element load plus the residual per-element check, with no element-read
    // tier and no guard call (see stmt/element_shape_loop.rs).
    //
    // #7480 step 3: this sits ABOVE the `receiver_class_name` gate on purpose.
    // The clone's element class can be one that resolver does not answer for —
    // an object-literal element type (`keep: {v: number}[]`) resolves to its
    // `__AnonShape_<hash>` only inside the matcher, which is where that
    // resolution is kept so it cannot un-gate anything else (#6377). Every
    // fact this consults was validated by the matcher when the fact was built:
    // the class has no computed members and no base, the property is not an
    // accessor and is not denylisted, and its declared type is a raw-f64
    // candidate at the packed slot index carried here. So the lowering needs
    // nothing from the receiver's static type, and asking for it would have
    // made the whole clone dead IR.
    if let Some((fact, field_index)) =
        crate::expr::element_shape_loop_fact_for_property_get(ctx, object, property)
            .map(|(fact, idx)| (fact.clone(), idx))
    {
        // Both receiver spellings — `arr[j].field` and #7771's `r.field`
        // through the clone's element binding — resolve to the fact's own
        // array; the report below must not re-derive it from the expression
        // shape, which the binding form does not carry.
        let arr_id = fact.array_local_id;
        // The counter's canonical i32 slot is what the matcher required;
        // without it there is nothing to index with.
        if let Some(slot) = ctx.i32_counter_slots.get(&fact.index_local_id).cloned() {
            let idx_i32 = ctx.block().load(I32, &slot);
            let value = crate::expr::element_shape_guard::emit_element_shape_field_load(
                ctx,
                &fact,
                &idx_i32,
                field_index,
            );
            let lowered = LoweredValue {
                semantic: SemanticKind::JsNumber,
                rep: NativeRep::F64,
                llvm_ty: DOUBLE,
                value: value.clone(),
            };
            ctx.record_lowered_value_with_access_mode_and_facts(
                "ElementShapeFieldGet",
                Some(arr_id),
                "element_shape_loop.raw_f64_load",
                &lowered,
                Some(BoundsState::Guarded {
                    guard_id: "element_shape_loop_preheader_check".to_string(),
                }),
                None,
                Some(BufferAccessMode::CheckedNative),
                None,
                None,
                None,
                vec![raw_f64_layout_fact(
                    Some(arr_id),
                    "consumed",
                    "element_shape_loop_preheader_check",
                    None,
                )],
                Vec::new(),
                false,
                false,
                vec![
                    format!("field={property}"),
                    format!("class={}", fact.class_name),
                    "loop_versioning=element_shape".to_string(),
                    "index_range=nonnegative_i32".to_string(),
                    "length_range=guarded_i32".to_string(),
                    "element_shape=homogeneous_class".to_string(),
                ],
            );
            // `--opt-report` consumption (#7766): the selection recorded at
            // clone emission was APPLIED here. Without this row a build under
            // the report would print "selected, consumed 0" — the wasted-proof
            // outcome — for a proof that is in fact doing the work.
            if crate::opt_report::enabled() {
                let (name, local_id) = match fact.element_binding {
                    Some(id) => (
                        ctx.local_id_to_name
                            .get(&id)
                            .cloned()
                            .unwrap_or_else(|| format!("<local {id}>")),
                        Some(id),
                    ),
                    None => (
                        ctx.local_id_to_name
                            .get(&arr_id)
                            .map(|n| format!("elements of `{n}`"))
                            .unwrap_or_else(|| format!("elements of <local {arr_id}>")),
                        None,
                    ),
                };
                crate::opt_report::consume(
                    crate::opt_report::Position::Local,
                    &name,
                    local_id,
                    crate::opt_report::Analysis::PtrShape,
                    "Ptr<Shape>",
                    "element_shape_loop.raw_f64_load",
                );
            }
            return Ok(Some(value));
        }
    }

    let Some(class_name) = receiver_class_name(ctx, object) else {
        return Ok(None);
    };
    if class_has_computed_runtime_members(ctx, &class_name) {
        return Ok(None);
    }

    let is_static_accessor = ctx
        .classes
        .get(&class_name)
        .map(|c| c.static_accessor_names.iter().any(|n| n == property))
        .unwrap_or(false);
    let getter_key = (class_name.clone(), format!("__get_{}", property));
    if is_static_accessor || ctx.methods.contains_key(&getter_key) {
        return Ok(None);
    }

    let Some(declared_type) =
        crate::type_analysis::class_field_declared_type(ctx, &class_name, property)
    else {
        return Ok(None);
    };
    if !crate::typed_shape::type_is_raw_f64_candidate(&declared_type) {
        return Ok(None);
    }
    let Some(field_index) =
        crate::type_analysis::class_field_global_index(ctx, &class_name, property)
    else {
        return Ok(None);
    };
    let (Some(&expected_class_id), Some(keys_global_name)) = (
        ctx.class_ids.get(&class_name),
        ctx.class_keys_globals.get(&class_name).cloned(),
    ) else {
        return Ok(None);
    };

    // #5093 loop versioning: inside the fast clone of a class-field versioned
    // loop, a tracked number-context field read on the proven receiver lowers
    // to a bare slot load on the preheader-cached object pointer — no shape
    // check, no guard call, no fallback (see stmt/loops.rs). Mirrors the hook
    // in the generic class-field GET diamond (property_get.rs).
    let loop_fact_ptr = match object.as_ref() {
        Expr::LocalGet(recv_id) => crate::expr::class_field_loop_fact_lookup(
            &ctx.class_field_loop_facts,
            *recv_id,
            &class_name,
            property,
        )
        .filter(|(_, loop_idx)| *loop_idx == field_index)
        .map(|(fact, _)| fact.obj_ptr.clone()),
        _ => None,
    };
    if let Some(obj_ptr) = loop_fact_ptr {
        let field_idx_str = field_index.to_string();
        let header_skip =
            crate::target_layout::object_header_size_bytes(ctx.target_triple).to_string();
        let blk = ctx.block();
        let fields_base = blk.gep(I8, &obj_ptr, &[(I64, &header_skip)]);
        let field_ptr = blk.gep(DOUBLE, &fields_base, &[(I64, &field_idx_str)]);
        let val = blk.load(DOUBLE, &field_ptr);
        let fast = LoweredValue {
            semantic: SemanticKind::JsNumber,
            rep: NativeRep::F64,
            llvm_ty: DOUBLE,
            value: val.clone(),
        };
        ctx.record_lowered_value_with_access_mode_and_facts(
            "ClassFieldGet",
            None,
            "class_field_get_number.loop_raw_f64_load",
            &fast,
            Some(BoundsState::Guarded {
                guard_id: "class_field_loop_preheader_check".to_string(),
            }),
            None,
            Some(BufferAccessMode::CheckedNative),
            None,
            None,
            None,
            vec![raw_f64_layout_fact(
                None,
                "consumed",
                "class_field_loop_preheader_check",
                None,
            )],
            Vec::new(),
            false,
            false,
            vec![
                format!("class={}", class_name),
                format!("field={}", property),
                format!("field_index={}", field_idx_str),
                "receiver_proof=loop_preheader_shape_check".to_string(),
                "field_layout=raw_f64_slot_array".to_string(),
                "loop_versioning=class_field_fast_clone".to_string(),
            ],
        );
        return Ok(Some(val));
    }

    // Representation-selection Phase 3b: shape-proven Ptr<Shape> receiver
    // whose field is numeric-proven (every reachable store is a number) —
    // bare fixed-offset load, no guard diamond. The numeric proof is what
    // licenses handing the raw load to a number context without the
    // fallback's `js_number_coerce`; non-numeric-proven fields fall through
    // to the guarded path below.
    // Phase 5a: a proven `this` never claims `numeric_fields` (see
    // collectors/proven_this.rs), so this site stays Phase-3b-local-only in
    // practice — the shared accessor keeps the two phases in one place.
    let ptr_shape_numeric = ctx
        .ptr_shape_receiver_fact(object.as_ref())
        .map(|fact| fact.class_name == class_name && fact.numeric_fields.contains(property))
        .unwrap_or(false);
    if ptr_shape_numeric {
        ctx.note_ptr_shape_consumed(object.as_ref(), "class_field_get_number.shape_proven_load");
        let recv_box = lower_expr(ctx, object)?;
        let field_idx_str = field_index.to_string();
        let header_skip =
            crate::target_layout::object_header_size_bytes(ctx.target_triple).to_string();
        let blk = ctx.block();
        let obj_bits = blk.bitcast_double_to_i64(&recv_box);
        let obj_handle = blk.and(I64, &obj_bits, POINTER_MASK_I64);
        let obj_ptr = blk.inttoptr(I64, &obj_handle);
        let fields_base = blk.gep(I8, &obj_ptr, &[(I64, &header_skip)]);
        let field_ptr = blk.gep(DOUBLE, &fields_base, &[(I64, &field_idx_str)]);
        let val = blk.load(DOUBLE, &field_ptr);
        let fast = LoweredValue {
            semantic: SemanticKind::JsNumber,
            rep: NativeRep::F64,
            llvm_ty: DOUBLE,
            value: val.clone(),
        };
        ctx.record_lowered_value_with_access_mode_and_facts(
            "ClassFieldGet",
            None,
            "class_field_get_number.shape_proven_load",
            &fast,
            Some(BoundsState::Guarded {
                guard_id: "ptr_shape_static_proof".to_string(),
            }),
            None,
            Some(BufferAccessMode::CheckedNative),
            None,
            None,
            None,
            vec![raw_f64_layout_fact(
                None,
                "consumed",
                "ptr_shape_static_proof",
                None,
            )],
            Vec::new(),
            false,
            false,
            vec![
                format!("class={}", class_name),
                format!("field={}", property),
                format!("field_index={}", field_idx_str),
                "receiver_proof=ptr_shape_local".to_string(),
                "numeric_proven=true".to_string(),
            ],
        );
        return Ok(Some(val));
    }

    // Representation-selection Phase 5a: the receiver's SHAPE is proven but
    // the field is not numeric-proven (always the case for a proven `this` —
    // see `collectors/proven_this.rs`: the receiver is caller-owned and
    // therefore aliased, so no exhaustive-store proof is available).
    //
    // The shape proof alone already retires the entire guard diamond: no
    // volatile gate, no 7-header-load shape check, no
    // `js_typed_feedback_class_field_get_guard` call, and no by-name
    // `js_object_get_field_by_name_f64` fallback — the proof says this slot IS
    // the field, so a bare fixed-offset load always yields the right VALUE.
    // What is NOT proven is the value's TYPE: an aliased `obj.f = "s"` store
    // elsewhere downgrades the slot's raw-f64 layout, and a guard-free read
    // cannot consult that layout. So the number context keeps a 2-instruction
    // inline plain-finite check on the LOADED BITS (`and` + `icmp`, no call,
    // no header load) with a cold `js_number_coerce` arm — exactly the
    // ToNumber the guarded fallback performs, minus the lookup.
    let ptr_shape_proven_shape = ctx
        .ptr_shape_receiver_fact(object.as_ref())
        .map(|fact| fact.class_name == class_name)
        .unwrap_or(false);
    if ptr_shape_proven_shape {
        ctx.note_ptr_shape_consumed(object.as_ref(), "ptr_shape_get_number");
        let recv_box = lower_expr(ctx, object)?;
        let field_idx_str = field_index.to_string();
        let header_skip =
            crate::target_layout::object_header_size_bytes(ctx.target_triple).to_string();
        let (val_raw, is_plain) = {
            let blk = ctx.block();
            let obj_bits = blk.bitcast_double_to_i64(&recv_box);
            let obj_handle = blk.and(I64, &obj_bits, POINTER_MASK_I64);
            let obj_ptr = blk.inttoptr(I64, &obj_handle);
            let fields_base = blk.gep(I8, &obj_ptr, &[(I64, &header_skip)]);
            let field_ptr = blk.gep(DOUBLE, &fields_base, &[(I64, &field_idx_str)]);
            let val_raw = blk.load(DOUBLE, &field_ptr);
            let val_bits = blk.bitcast_double_to_i64(&val_raw);
            let is_plain = crate::expr::class_field_inline_guard::emit_plain_finite_number_check(
                blk, &val_bits,
            );
            (val_raw, is_plain)
        };
        let fast_idx = ctx.new_block("ptr_shape_get_number.plain");
        let coerce_idx = ctx.new_block("ptr_shape_get_number.coerce");
        let merge_idx = ctx.new_block("ptr_shape_get_number.merge");
        let fast_label = ctx.block_label(fast_idx);
        let coerce_label = ctx.block_label(coerce_idx);
        let merge_label = ctx.block_label(merge_idx);
        ctx.block().cond_br(&is_plain, &fast_label, &coerce_label);

        ctx.current_block = fast_idx;
        let fast_end = ctx.block().label.clone();
        ctx.block().br(&merge_label);

        ctx.current_block = coerce_idx;
        let coerced = ctx
            .block()
            .call(DOUBLE, "js_number_coerce", &[(DOUBLE, &val_raw)]);
        let coerce_end = ctx.block().label.clone();
        ctx.block().br(&merge_label);

        ctx.current_block = merge_idx;
        let merged = ctx
            .block()
            .phi(DOUBLE, &[(&val_raw, &fast_end), (&coerced, &coerce_end)]);
        let lowered = LoweredValue {
            semantic: SemanticKind::JsNumber,
            rep: NativeRep::F64,
            llvm_ty: DOUBLE,
            value: merged.clone(),
        };
        ctx.record_lowered_value_with_access_mode_and_facts(
            "ClassFieldGet",
            None,
            "class_field_get_number.shape_proven_checked_load",
            &lowered,
            Some(BoundsState::Guarded {
                guard_id: "ptr_shape_static_proof".to_string(),
            }),
            None,
            Some(BufferAccessMode::CheckedNative),
            None,
            None,
            None,
            vec![raw_f64_layout_fact(
                None,
                "consumed",
                "ptr_shape_static_proof",
                None,
            )],
            Vec::new(),
            false,
            false,
            vec![
                format!("class={}", class_name),
                format!("field={}", property),
                format!("field_index={}", field_idx_str),
                "receiver_proof=ptr_shape_static_proof".to_string(),
                "numeric_proven=false".to_string(),
                "value_check=inline_plain_finite".to_string(),
                "number_context=true".to_string(),
            ],
        );
        return Ok(Some(merged));
    }

    let recv_box = lower_expr(ctx, object)?;
    let key_idx = ctx.strings.intern(property);
    let key_handle_global = format!("@{}", ctx.strings.entry(key_idx).handle_global);
    let site_id = emit_typed_feedback_register_site(
        ctx,
        TypedFeedbackKind::PropertyGet,
        property,
        TypedFeedbackContract::class_field_get(),
    );
    let field_idx_str = field_index.to_string();
    let expected_class_id_str = expected_class_id.to_string();
    let expected_shape_id =
        crate::lower_call::new_alloc::load_class_shape_id(ctx, &class_name, &keys_global_name);
    let (obj_bits, obj_handle, key_raw) = {
        let blk = ctx.block();
        let obj_bits = blk.bitcast_double_to_i64(&recv_box);
        let obj_handle = blk.and(I64, &obj_bits, POINTER_MASK_I64);
        let key_box = blk.load(DOUBLE, &key_handle_global);
        let key_bits = blk.bitcast_double_to_i64(&key_box);
        let key_raw = blk.and(I64, &key_bits, POINTER_MASK_I64);
        (obj_bits, obj_handle, key_raw)
    };

    let fast_idx = ctx.new_block("class_field_get_number.fast");
    let fallback_idx = ctx.new_block("class_field_get_number.fallback");
    let merge_idx = ctx.new_block("class_field_get_number.merge");
    let fast_label = ctx.block_label(fast_idx);
    let fallback_label = ctx.block_label(fallback_idx);
    let merge_label = ctx.block_label(merge_idx);

    let subclass_arms = crate::expr::class_field_inline_guard::class_field_subclass_arms(
        ctx,
        &class_name,
        property,
        field_index,
        true,
    );
    let _guardcall_label = crate::expr::class_field_inline_guard::emit_class_field_inline_precheck(
        ctx,
        &obj_bits,
        &obj_handle,
        &expected_class_id_str,
        &expected_shape_id,
        field_index,
        true,
        None,
        &fast_label,
        &subclass_arms,
    );
    let guard_ok = ctx.block().call(
        I32,
        "js_typed_feedback_class_field_get_guard",
        &[
            (I64, &site_id),
            (DOUBLE, &recv_box),
            (I32, &expected_class_id_str),
            (I32, &expected_shape_id),
            (I64, &key_raw),
            (I32, &field_idx_str),
            (I32, "1"),
        ],
    );
    let guard_pass = ctx.block().icmp_ne(I32, &guard_ok, "0");
    ctx.block()
        .cond_br(&guard_pass, &fast_label, &fallback_label);

    ctx.current_block = fast_idx;
    let header_skip = crate::target_layout::object_header_size_bytes(ctx.target_triple).to_string();
    let blk = ctx.block();
    let obj_ptr = blk.inttoptr(I64, &obj_handle);
    let fields_base = blk.gep(I8, &obj_ptr, &[(I64, &header_skip)]);
    let field_ptr = blk.gep(DOUBLE, &fields_base, &[(I64, &field_idx_str)]);
    let val_fast = blk.load(DOUBLE, &field_ptr);
    let fast_end_label = blk.label.clone();
    blk.br(&merge_label);
    let fast = LoweredValue {
        semantic: SemanticKind::JsNumber,
        rep: NativeRep::F64,
        llvm_ty: DOUBLE,
        value: val_fast.clone(),
    };
    ctx.record_lowered_value_with_access_mode_and_facts(
        "ClassFieldGet",
        None,
        "class_field_get.raw_f64_number_context",
        &fast,
        Some(BoundsState::Guarded {
            guard_id: "class_field_get_guard".to_string(),
        }),
        None,
        Some(BufferAccessMode::CheckedNative),
        None,
        None,
        None,
        vec![raw_f64_layout_fact(
            None,
            "consumed",
            "class_field_get_guard",
            None,
        )],
        Vec::new(),
        false,
        false,
        vec![
            format!("class={}", class_name),
            format!("class_id={}", expected_class_id_str),
            format!("field={}", property),
            format!("field_index={}", field_idx_str),
            "receiver_proof=declared_named_receiver_guarded_exact_class".to_string(),
            "field_layout=raw_f64_slot_array".to_string(),
            "pointer_bitmap=non_pointer".to_string(),
            "number_context=true".to_string(),
        ],
    );

    ctx.current_block = fallback_idx;
    // #7153: same nullish-receiver check as the value-context diamond in
    // property_get.rs — a nullish field read must throw TypeError, not coerce
    // `undefined` to NaN and keep running.
    let (is_null, is_nullish) = {
        let blk = ctx.block();
        let is_undef = blk.icmp_eq(I64, &obj_bits, crate::nanbox::TAG_UNDEFINED_I64);
        let is_null = blk.icmp_eq(I64, &obj_bits, crate::nanbox::TAG_NULL_I64);
        let is_nullish = blk.or(I1, &is_undef, &is_null);
        (is_null, is_nullish)
    };
    let throw_idx = ctx.new_block("class_field_get_number.throw_nullish");
    let lookup_idx = ctx.new_block("class_field_get_number.fallback_lookup");
    let throw_label = ctx.block_label(throw_idx);
    let lookup_label = ctx.block_label(lookup_idx);
    ctx.block()
        .cond_br(&is_nullish, &throw_label, &lookup_label);

    ctx.current_block = throw_idx;
    let prop_entry = ctx.strings.entry(key_idx);
    let prop_bytes_global = format!("@{}", prop_entry.bytes_global);
    let prop_len_str = prop_entry.byte_len.to_string();
    let is_null_i32 = ctx.block().zext(I1, &is_null, I32);
    ctx.block().call_void(
        "js_throw_type_error_property_access",
        &[
            (I32, &is_null_i32),
            (PTR, &prop_bytes_global),
            (I64, &prop_len_str),
        ],
    );
    ctx.block().unreachable();

    ctx.current_block = lookup_idx;
    let blk = ctx.block();
    crate::expr::emit_typed_feedback_record_call(
        blk,
        "js_typed_feedback_record_fallback_call",
        &[(I64, &site_id)],
    );
    let val_fallback_js = blk.call(
        DOUBLE,
        "js_object_get_field_by_name_f64",
        &[(I64, &obj_bits), (I64, &key_raw)],
    );
    let val_fallback = blk.call(DOUBLE, "js_number_coerce", &[(DOUBLE, &val_fallback_js)]);
    let fallback_end_label = blk.label.clone();
    blk.br(&merge_label);
    let fallback = LoweredValue {
        semantic: SemanticKind::JsValue,
        rep: NativeRep::JsValue,
        llvm_ty: DOUBLE,
        value: val_fallback_js.clone(),
    };
    ctx.record_lowered_value_with_access_mode_and_facts(
        "ClassFieldGet",
        None,
        "js_object_get_field_by_name_f64.number_context_fallback",
        &fallback,
        Some(BoundsState::Unknown),
        None,
        Some(BufferAccessMode::DynamicFallback),
        Some(MaterializationReason::RuntimeApi),
        None,
        None,
        Vec::new(),
        vec![
            raw_f64_layout_fact(
                None,
                "rejected",
                "class_field_get_guard",
                Some(MaterializationReason::RuntimeApi),
            ),
            raw_f64_layout_fact(
                None,
                "invalidated",
                "runtime_api",
                Some(MaterializationReason::RuntimeApi),
            ),
        ],
        false,
        false,
        vec![
            format!("class={}", class_name),
            format!("field={}", property),
            format!("field_index={}", field_idx_str),
            "number_context=true".to_string(),
        ],
    );

    ctx.current_block = merge_idx;
    Ok(Some(ctx.block().phi(
        DOUBLE,
        &[
            (&val_fast, &fast_end_label),
            (&val_fallback, &fallback_end_label),
        ],
    )))
}
