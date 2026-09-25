//! Scalar replacement of a non-escaping `let x = new C(...)` binding: one
//! stack alloca per field, with the constructor body inlined into those
//! allocas. Split out of `let_stmt.rs` to keep it under the 2,000-line cap
//! (#10750); the body is unchanged apart from reporting "handled" as
//! `Ok(true)` instead of returning from `lower_let` directly.

use super::let_stmt_facts::{collect_scalar_class_data, note_ptr_shape_scalar_replaced};
use super::unused_expr::lower_unused_expr;
use super::*;
use crate::native_value::{NativeRep, SemanticKind};

/// Returns `Ok(true)` when the binding was scalar-replaced and `lower_let`
/// must return; `Ok(false)` falls through to the ordinary lowering.
pub(super) fn try_lower_scalar_replaced_new(
    ctx: &mut FnCtx<'_>,
    id: u32,
    name: &str,
    init: Option<&perry_hir::Expr>,
    refined_ty: &perry_hir::types::Type,
) -> Result<bool> {
    // Scalar replacement: if this Let binds a non-escaping New,
    // skip the heap allocation entirely. Create a stack alloca
    // per field and inline the constructor stores into those allocas.
    //
    // Imported classes are excluded: their constructor bodies live
    // in the source module's .o and aren't available here, so
    // inlining produces a zero-initialized stub-shaped object with
    // no fields populated. The call must go through the standard
    // heap-allocation path so `lower_new` emits the cross-module
    // `<prefix>__<class>_constructor` call.
    if let Some(perry_hir::Expr::New {
        class_name, args, ..
    }) = init
    {
        let is_imported = ctx.imported_class_ctors.contains_key(class_name);
        if ctx.non_escaping_news.contains_key(&id) && !is_imported {
            // Extract all class data we need (field names + ctor) before
            // taking mutable borrows on ctx. Clone out of the shared
            // `classes` map so we release the immutable borrow early.
            let scalar_data = collect_scalar_class_data(ctx, class_name);

            if let Some((all_fields, ctor)) = scalar_data {
                // #7106 follow-up, mechanism 3: this binding is about to stop
                // being an object at all. If `Ptr<Shape>` also proved it, the
                // report already counted a promotion that cannot emit
                // anything — no property access will ever reach a
                // representation-selection lowering, because there is no
                // property access left. On `07_object_create` and
                // `12_binary_trees` that is literally the case: `--opt-report`
                // says `selected=1` while both arms of a
                // PERRY_PTR_SHAPE_LOCALS A/B emit byte-identical objects.
                //
                // Scalar replacement winning here is the BETTER outcome, not a
                // defect; the defect is that it was indistinguishable in the
                // report from a proof that was simply wasted.
                if crate::opt_report::enabled() {
                    note_ptr_shape_scalar_replaced(ctx, id, name);
                }
                // Create per-field allocas. For synthetic anonymous-shape
                // classes, scalar replacement may only need fields that are
                // observed after construction; unused constructor stores still
                // evaluate their RHS below but get discarded in property_set.
                let stored_fields: Vec<String> = if class_name.starts_with("__AnonShape_") {
                    if let Some(used_fields) = ctx.non_escaping_new_used_fields.get(&id) {
                        all_fields
                            .iter()
                            .filter(|fname| used_fields.contains(*fname))
                            .cloned()
                            .collect()
                    } else {
                        Vec::new()
                    }
                } else {
                    all_fields.clone()
                };
                let mut field_slots: std::collections::HashMap<String, String> =
                    std::collections::HashMap::new();
                for fname in &stored_fields {
                    let slot = ctx.func.alloca_entry(DOUBLE);
                    let undef =
                        crate::nanbox::double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED));
                    ctx.func.entry_allocas_push_store(DOUBLE, &undef, &slot);
                    field_slots.insert(fname.clone(), slot);
                }

                ctx.scalar_replaced.insert(id, field_slots);

                // Register type + dummy slot so LocalGet doesn't fail
                ctx.local_types.insert(id, refined_ty.clone());
                let dummy_slot = ctx.func.alloca_entry(DOUBLE);
                ctx.locals.insert(id, dummy_slot);

                // Anonymous-shape classes are synthesized for object
                // literals. Their constructor is a straight field-assigner,
                // so scalar replacement can bypass parameter allocas and the
                // inlined ctor body: evaluate args in order, store only the
                // observed fields, and discard the rest.
                if class_name.starts_with("__AnonShape_") {
                    for (idx, arg) in args.iter().enumerate() {
                        let slot = all_fields.get(idx).and_then(|fname| {
                            ctx.scalar_replaced
                                .get(&id)
                                .and_then(|fields| fields.get(fname))
                                .cloned()
                        });
                        if slot.is_none() && lower_unused_expr(ctx, arg)? {
                            continue;
                        }
                        let arg_val = lower_expr(ctx, arg)?;
                        if let Some(slot) = slot {
                            ctx.block().store(DOUBLE, &arg_val, &slot);
                            // #6968: anonymous-shape scalar replacement stores
                            // constructor arguments straight into per-field
                            // allocas — same unrooted-heap-value hole.
                            crate::expr::root_scalar_replaced_slot(ctx, &slot, arg);
                            let lowered = LoweredValue {
                                semantic: SemanticKind::JsValue,
                                rep: NativeRep::JsValue,
                                llvm_ty: DOUBLE,
                                value: arg_val,
                            };
                            let field_note = all_fields
                                .get(idx)
                                .map(|fname| format!("field={}", fname))
                                .unwrap_or_else(|| format!("field_index={}", idx));
                            ctx.record_lowered_value_with_access_mode(
                                "ScalarObjectLiteralInit",
                                Some(id),
                                "scalar_object_field_store",
                                &lowered,
                                None,
                                None,
                                None,
                                None,
                                false,
                                false,
                                vec![field_note],
                            );
                        }
                    }
                    return Ok(true);
                }

                // Preserve only initializer-derived argument evidence for
                // the inlined constructor parameters. Their declarations are
                // metadata, but the already-evaluated argument value can
                // legitimately establish a call-site-scoped runtime kind.
                let arg_proofs: Vec<Option<perry_hir::types::Type>> = args
                    .iter()
                    .map(|arg| crate::type_analysis::proven_type_from_init(ctx, arg))
                    .collect();

                // Lower args first
                let mut lowered_args: Vec<String> = Vec::new();
                for a in args {
                    lowered_args.push(lower_expr(ctx, a)?);
                }

                // Push scalar ctor target so PropertySet on `this` routes to allocas
                ctx.scalar_ctor_target.push(id);
                ctx.class_stack.push(class_name.clone());
                // A dummy this_stack entry — the ctor body references Expr::This
                // but scalar-replaced PropertySet intercepts it before loading
                let dummy_this = ctx.func.alloca_entry(DOUBLE);
                ctx.this_stack.push(dummy_this);

                // #2768/new.target: scalar replacement inlines the (own or
                // inherited) constructor here without going through
                // `lower_new`, so mirror its `new_target_stack` setup — bind
                // `new.target` in the inlined body to this leaf class's ref
                // (`INT32_TAG | class_id`). Without this a `new.target` read in
                // the ctor (notably `const t = new.target`) fell through to the
                // runtime cell, which this path never sets, yielding undefined.
                let new_target_bits = ctx
                    .class_ids
                    .get(class_name)
                    .map(|&cid| crate::nanbox::INT32_TAG | (cid as u64 & 0xFFFF_FFFF))
                    .unwrap_or(crate::nanbox::TAG_UNDEFINED);
                let new_target_slot = ctx.func.alloca_entry(DOUBLE);
                ctx.block().store(
                    DOUBLE,
                    &crate::nanbox::double_literal(f64::from_bits(new_target_bits)),
                    &new_target_slot,
                );
                ctx.new_target_stack.push(new_target_slot);

                // Stage field initializers around any parent body chain.
                // Refs #420: leaf field inits may reference state set by
                // parent body (e.g. drizzle's
                // `class PgText extends PgColumn { enumValues = this.config.enumValues }`),
                // so apply ancestors' fields first, then run the parent
                // body when the leaf has no own ctor, then leaf-self
                // fields. For own-ctor case, leaf-self runs at the
                // SuperCall site inside the body.
                let class_has_extends = ctx
                    .classes
                    .get(class_name)
                    .map(|c| c.extends_name.is_some())
                    .unwrap_or(false);
                // Issue #631-followup: for the no-own-ctor case,
                // only apply fields up to the inherited-ctor class
                // before the body inline. Intermediate classes
                // between the inherited-ctor and the leaf get
                // their fields after the body returns (their
                // initializers may depend on parent body state).
                let inherited_ctor_class: Option<String> = if ctor.is_none() && class_has_extends {
                    let mut walker = ctx
                        .classes
                        .get(class_name)
                        .and_then(|c| c.extends_name.clone());
                    let mut found: Option<String> = None;
                    while let Some(pname) = walker {
                        if let Some(parent_class) = ctx.classes.get(&pname).copied() {
                            if parent_class.constructor.is_some() {
                                found = Some(pname);
                                break;
                            }
                            walker = parent_class.extends_name.clone();
                        } else {
                            break;
                        }
                    }
                    found
                } else {
                    None
                };
                let init_mode = if let Some(stop_at) = inherited_ctor_class.clone() {
                    crate::lower_call::FieldInitMode::UpToInclusive(stop_at)
                } else if class_has_extends {
                    crate::lower_call::FieldInitMode::AncestorsOnly
                } else {
                    crate::lower_call::FieldInitMode::All
                };
                crate::lower_call::apply_field_initializers_recursive(ctx, class_name, init_mode)?;

                // Inline constructor body if present (own-ctor case).
                if let Some(ctor) = &ctor {
                    let saved_locals = ctx.locals.clone();
                    let saved_local_types = ctx.local_types.clone();
                    let saved_proven_local_types = ctx.proven_local_types.clone();
                    for (index, (param, arg_val)) in
                        ctor.params.iter().zip(lowered_args.iter()).enumerate()
                    {
                        let slot = ctx.func.alloca_entry(DOUBLE);
                        ctx.block().store(DOUBLE, arg_val, &slot);
                        ctx.locals.insert(param.id, slot);
                        ctx.local_types.insert(param.id, param.ty.clone());
                        ctx.proven_local_types.remove(&param.id);
                        if let Some(Some(proof)) = arg_proofs.get(index) {
                            ctx.proven_local_types.insert(param.id, proof.clone());
                        }
                    }
                    // #9081: the body is spliced into the enclosing frame,
                    // whose slot map never saw the ctor's locals.
                    crate::expr::root_inlined_ctor_pointer_locals(ctx, &ctor.params, &ctor.body);
                    crate::stmt::lower_stmts(ctx, &ctor.body)?;
                    ctx.locals = saved_locals;
                    ctx.local_types = saved_local_types;
                    ctx.proven_local_types = saved_proven_local_types;
                } else if class_has_extends {
                    // No own ctor — JS spec defaults to
                    // `constructor(...args) { super(...args); }`. Walk
                    // the parent chain to find the first ancestor with
                    // a body and inline it (forwarding args). Refs #420.
                    let mut parent_name = ctx
                        .classes
                        .get(class_name)
                        .and_then(|c| c.extends_name.clone());
                    while let Some(pname) = parent_name {
                        if let Some(parent_class) = ctx.classes.get(&pname).copied() {
                            if let Some(parent_ctor) = &parent_class.constructor {
                                let saved_locals = ctx.locals.clone();
                                let saved_local_types = ctx.local_types.clone();
                                let saved_proven_local_types = ctx.proven_local_types.clone();
                                for (i, param) in parent_ctor.params.iter().enumerate() {
                                    let slot = ctx.func.alloca_entry(DOUBLE);
                                    if i < lowered_args.len() {
                                        ctx.block().store(DOUBLE, &lowered_args[i], &slot);
                                    } else {
                                        let undef = crate::nanbox::double_literal(f64::from_bits(
                                            crate::nanbox::TAG_UNDEFINED,
                                        ));
                                        ctx.block().store(DOUBLE, &undef, &slot);
                                    }
                                    ctx.locals.insert(param.id, slot);
                                    ctx.local_types.insert(param.id, param.ty.clone());
                                    ctx.proven_local_types.remove(&param.id);
                                    if let Some(Some(proof)) = arg_proofs.get(i) {
                                        ctx.proven_local_types.insert(param.id, proof.clone());
                                    }
                                }
                                ctx.class_stack.pop();
                                ctx.class_stack.push(pname.clone());
                                // #9081: same frame-splice rooting as the
                                // own-ctor inline above.
                                crate::expr::root_inlined_ctor_pointer_locals(
                                    ctx,
                                    &parent_ctor.params,
                                    &parent_ctor.body,
                                );
                                crate::stmt::lower_stmts(ctx, &parent_ctor.body)?;
                                ctx.class_stack.pop();
                                ctx.class_stack.push(class_name.clone());
                                ctx.locals = saved_locals;
                                ctx.local_types = saved_local_types;
                                ctx.proven_local_types = saved_proven_local_types;
                                break;
                            }
                            parent_name = parent_class.extends_name.clone();
                        } else {
                            break;
                        }
                    }
                    // Apply leaf's own field initializers AFTER the
                    // parent body chain has run. Issue #631-followup:
                    // also include intermediate-class fields between
                    // the inherited-ctor and the leaf (per JS spec
                    // each default-ctor class's field inits run after
                    // its super() returns).
                    let post_mode = if let Some(stop_at) = inherited_ctor_class.clone() {
                        crate::lower_call::FieldInitMode::BetweenExclusiveTo(stop_at)
                    } else {
                        crate::lower_call::FieldInitMode::SelfOnly
                    };
                    crate::lower_call::apply_field_initializers_recursive(
                        ctx, class_name, post_mode,
                    )?;
                }

                ctx.new_target_stack.pop();
                ctx.this_stack.pop();
                ctx.class_stack.pop();
                ctx.scalar_ctor_target.pop();

                return Ok(true);
            }
        }
    }
    Ok(false)
}
