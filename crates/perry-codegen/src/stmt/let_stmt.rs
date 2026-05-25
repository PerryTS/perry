//! `Stmt::Let` lowering — large arm extracted from the dispatcher.

use super::*;

use crate::expr::lower_expr_with_expected_type;
use crate::native_value::{
    AliasState, BufferElem, BufferViewSlot, LengthSource, MaterializationReason,
};
use crate::types::{I32, I64, I8, PTR};

pub(crate) fn lower_let(
    ctx: &mut FnCtx<'_>,
    id: u32,
    name: &str,
    init: Option<&perry_hir::Expr>,
    ty: &perry_types::Type,
    mutable: bool,
) -> Result<()> {
    // `let C = SomeClass` aliases the local `C` to the class
    // `SomeClass` for `new C()` site rerouting. The HIR lowers
    // class identifiers referenced as values to `Expr::ClassRef`,
    // so we just check whether the init is a ClassRef and stash
    // the (let_name → class_name) mapping in `ctx.local_class_aliases`.
    // The map is consulted by `lower_new` when its
    // `ctx.classes.get(class_name)` lookup misses — without
    // this, `new C()` falls back to the empty-object placeholder.
    // Record the (id → name) mapping unconditionally so the
    // class-alias chain resolution below (and any other site
    // that needs id → name) can use it.
    ctx.local_id_to_name.insert(id, name.to_string());
    if let Some(init_expr) = init {
        if let Some(source_id) = native_i32_alias_source(init_expr) {
            ctx.native_i32_aliases.insert(id, source_id);
        }
        if let Some(buffer_ids) = math_min_length_buffer_ids(init_expr) {
            ctx.min_length_bounds.insert(id, buffer_ids);
        }
    }
    crate::expr::record_int_facts_for_let(ctx, id, init, mutable);
    // Class alias detection. Two shapes:
    //
    //   (a) `let C = SomeClass` — init is `Expr::ClassRef("SomeClass")`
    //       (the HIR's `lower.rs::ast::Expr::Ident` lifts class
    //       names referenced as values to ClassRef). We register
    //       `local_class_aliases["C"] = "SomeClass"`.
    //
    //   (b) `let B = A` where A is itself a class alias —
    //       init is `Expr::LocalGet(other_id)`. We look up
    //       other_id's name via `local_id_to_name`, then check
    //       if that name is in `local_class_aliases`, and
    //       propagate the resolved class name. This handles
    //       chains like `let A = X; let B = A; let C = B; new C()`.
    //
    // Both cases let `lower_new("C", args)` reroute through
    // `lower_new("X", args)` instead of falling back to the
    // empty-object placeholder when the class name turns out to
    // be a local-bound alias rather than a real class identifier.
    match init {
        Some(perry_hir::Expr::ClassRef(class_name)) => {
            ctx.local_class_aliases
                .insert(name.to_string(), class_name.clone());
        }
        Some(perry_hir::Expr::LocalGet(other_id)) => {
            if let Some(other_name) = ctx.local_id_to_name.get(other_id).cloned() {
                if let Some(resolved) = ctx.local_class_aliases.get(&other_name).cloned() {
                    ctx.local_class_aliases.insert(name.to_string(), resolved);
                }
            }
            // Also propagate the per-object field-class map: `let
            // O2 = O` should carry `O`'s known field→class
            // bindings forward (otherwise `new O2.Inner(...)`
            // can't resolve back to the class). Refs #740.
            if let Some(fields) = ctx.local_class_field_aliases.get(other_id).cloned() {
                ctx.local_class_field_aliases.insert(id, fields);
            }
        }
        // Refs #740: `let X = O.Inner` where `O` is an object
        // literal that holds a class ref under "Inner" — promote
        // X to a class alias so `new X(args)` dispatches to the
        // real class instead of the empty-object placeholder.
        Some(perry_hir::Expr::PropertyGet { object, property }) => {
            if let perry_hir::Expr::LocalGet(other_id) = object.as_ref() {
                if let Some(fields) = ctx.local_class_field_aliases.get(other_id) {
                    if let Some(class_name) = fields.get(property) {
                        ctx.local_class_aliases
                            .insert(name.to_string(), class_name.clone());
                    }
                }
            }
        }
        _ => {}
    }

    // Refs #740: object literal embeds class refs. When `init` is
    // `Expr::New { class_name (an __AnonShape), args }`, walk the
    // class's fields and the args in parallel — any `ClassRef`
    // arg becomes a `(local_id, field_name) → class_name` entry
    // in `local_class_field_aliases`. This lets later reads
    // (`O.Inner` / `let C = O.Inner`) recover the underlying
    // class. Mirrors the shape-fields ordering produced by
    // `synthesize_anon_shape_class` in the HIR lowering.
    if let Some(perry_hir::Expr::New {
        class_name: shape_name,
        args,
        ..
    }) = init
    {
        if let Some(class) = ctx.classes.get(shape_name).copied() {
            let mut field_map: std::collections::HashMap<String, String> =
                std::collections::HashMap::new();
            for (field, arg) in class.fields.iter().zip(args.iter()) {
                if let perry_hir::Expr::ClassRef(class_name_ref) = arg {
                    field_map.insert(field.name.clone(), class_name_ref.clone());
                }
            }
            if !field_map.is_empty() {
                ctx.local_class_field_aliases.insert(id, field_map);
            }
        }
    }

    // Issue #50: row-alias detection. When `let krow = X[i]` where
    // `X` is a folded flat-const 2D int array, record
    // `krow_id → (X_id, i)` so a later `krow[j]` can lower through
    // the same flat `[N x i32]` load path as an inline `X[i][j]`.
    // Only fires for non-mutable lets (reassignment would invalidate
    // the alias relationship).
    if !mutable {
        if let Some(perry_hir::Expr::IndexGet { object, index }) = init {
            if let perry_hir::Expr::LocalGet(const_id) = object.as_ref() {
                if ctx.flat_const_arrays.contains_key(const_id) {
                    ctx.array_row_aliases
                        .insert(id, (*const_id, Box::new((**index).clone())));
                }
            }
        }
    }
    // Refine the declared type from the initializer when the
    // declared type is Any. The HIR's destructuring lowering
    // declares synthetic `__destruct_*` lets as `ty: Any` even
    // when the init is obviously an Array literal — that breaks
    // is_array_expr at later use sites that depend on
    // `local_types[id]` to dispatch to the array fast path.
    //
    // We only refine Any → something more specific; we don't
    // override declared types because the user may have written
    // `let x: Object = ...` deliberately.
    let refined_ty = if matches!(ty, perry_types::Type::Any) {
        init.and_then(|e| crate::type_analysis::refine_type_from_init(ctx, e))
            .unwrap_or_else(|| ty.clone())
    } else if matches!(ty, perry_types::Type::Array(ref elem) if matches!(**elem, perry_types::Type::Any))
    {
        // Also refine Array<Any> when the init provides more
        // specific element type info. Object.keys() returns
        // Array<string> but the HIR often declares Array<Any>.
        init.and_then(|e| crate::type_analysis::refine_type_from_init(ctx, e))
            .unwrap_or_else(|| ty.clone())
    } else {
        ty.clone()
    };

    // Track closure func_id → local_id mapping so the closure
    // call site in lower_call can look up rest param info.
    if let Some(perry_hir::Expr::Closure {
        func_id: cfid,
        params,
        ..
    }) = init
    {
        ctx.local_closure_func_ids.insert(id, *cfid);
        ctx.local_closure_param_counts.insert(id, params.len());
    }

    // Scalar replacement: if this Let binds a non-escaping array
    // literal, skip the heap allocation entirely. Each element gets
    // its own stack alloca; constant-index reads in the Let's scope
    // load directly from the corresponding slot. See the
    // `collect_non_escaping_arrays` pass in collectors.rs for the
    // escape criteria.
    if let Some(perry_hir::Expr::Array(elements)) = init {
        if ctx.non_escaping_arrays.contains_key(&id) {
            let n = elements.len();
            let mut slots: Vec<String> = Vec::with_capacity(n);
            for _ in 0..n {
                slots.push(ctx.func.alloca_entry(DOUBLE));
            }
            // Evaluate each element expression first; store the
            // result into its slot. Order matches source, so any
            // side effects stay observable in the same sequence the
            // heap-allocating path would have produced.
            for (i, elem) in elements.iter().enumerate() {
                let v = lower_expr(ctx, elem)?;
                ctx.block().store(DOUBLE, &v, &slots[i]);
            }
            ctx.scalar_replaced_arrays.insert(id, slots);

            // Register the local's type + a dummy slot so any surviving
            // LocalGet (e.g. debug instrumentation, unrecognized
            // expression shapes the collector conservatively rejected)
            // still resolves; the actual scalar-replaced reads short-
            // circuit before hitting this slot.
            ctx.local_types.insert(id, refined_ty);
            let dummy_slot = ctx.func.alloca_entry(DOUBLE);
            ctx.locals.insert(id, dummy_slot);
            return Ok(());
        }
    }

    // Scalar replacement: if this Let binds a non-escaping object
    // literal, skip the heap allocation entirely. One alloca per
    // unique field; PropertyGet/Set already resolve through
    // `ctx.scalar_replaced`, so no additional read path is needed.
    // See `collect_non_escaping_object_literals` in collectors.rs.
    if let Some(perry_hir::Expr::Object(props)) = init {
        if let Some(field_order) = ctx.non_escaping_object_literals.get(&id).cloned() {
            let undef = crate::nanbox::double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED));
            let mut field_slots: std::collections::HashMap<String, String> =
                std::collections::HashMap::with_capacity(field_order.len());
            for fname in &field_order {
                let slot = ctx.func.alloca_entry(DOUBLE);
                ctx.func.entry_allocas_push_store(DOUBLE, &undef, &slot);
                field_slots.insert(fname.clone(), slot);
            }

            // Evaluate and store each property expression in source
            // order — duplicate keys naturally do last-write-wins
            // because they share a slot. Side effects of each value
            // expression stay observable in declaration order.
            for (key, value_expr) in props {
                let v = lower_expr(ctx, value_expr)?;
                if let Some(slot) = field_slots.get(key).cloned() {
                    ctx.block().store(DOUBLE, &v, &slot);
                }
            }

            ctx.scalar_replaced.insert(id, field_slots);

            // Register type + dummy slot so any surviving LocalGet
            // (conservative collector rejects are possible) resolves
            // — the scalar-replaced PropertyGet/Set paths short-
            // circuit before loading this slot.
            ctx.local_types.insert(id, refined_ty);
            let dummy_slot = ctx.func.alloca_entry(DOUBLE);
            ctx.locals.insert(id, dummy_slot);
            return Ok(());
        }
    }

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
                ctx.local_types.insert(id, refined_ty);
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
                        }
                    }
                    return Ok(());
                }

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
                    for (param, arg_val) in ctor.params.iter().zip(lowered_args.iter()) {
                        let slot = ctx.func.alloca_entry(DOUBLE);
                        ctx.block().store(DOUBLE, arg_val, &slot);
                        ctx.locals.insert(param.id, slot);
                        ctx.local_types.insert(param.id, param.ty.clone());
                    }
                    crate::stmt::lower_stmts(ctx, &ctor.body)?;
                    ctx.locals = saved_locals;
                    ctx.local_types = saved_local_types;
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
                                }
                                ctx.class_stack.pop();
                                ctx.class_stack.push(pname.clone());
                                crate::stmt::lower_stmts(ctx, &parent_ctor.body)?;
                                ctx.class_stack.pop();
                                ctx.class_stack.push(class_name.clone());
                                ctx.locals = saved_locals;
                                ctx.local_types = saved_local_types;
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

                ctx.this_stack.pop();
                ctx.class_stack.pop();
                ctx.scalar_ctor_target.pop();

                return Ok(());
            }
        }
    }

    // CRITICAL: register the local's storage BEFORE lowering
    // the init expression. Self-recursive closures (`let f = (n)
    // => f(n-1) ...`) reference the let-bound name from inside
    // their own body, and the closure's auto-capture pass needs
    // to find the slot or global. Lowering the init first means
    // the body sees `LocalGet(7)` with no entry in ctx.locals.
    //
    // For module globals we register first, then lower init,
    // then store. Same for stack-local lets.
    if let Some(global_name) = ctx.module_globals.get(&id).cloned() {
        ctx.local_types.insert(id, refined_ty.clone());
        if let Some(init_expr) = init {
            let v = lower_expr_with_expected_type(ctx, init_expr, Some(&refined_ty))?;
            let g_ref = format!("@{}", global_name);
            ctx.block().store(DOUBLE, &v, &g_ref);

            // Buffer data-pointer slot: when the HIR facts identify a fresh
            // immutable u8 buffer, pre-compute the data base pointer (handle +
            // 8, past BufferHeader) and store it in a ptr alloca.
            // Uint8ArrayGet/Set then uses `getelementptr inbounds` from this
            // pointer instead of the inttoptr chain.
            if ctx.known_noalias_buffer_locals.contains(&id) {
                let blk = ctx.block();
                let handle = crate::expr::unbox_to_i64(blk, &v);
                let handle_ptr = blk.inttoptr(I64, &handle);
                let data_ptr = blk.gep(I8, &handle_ptr, &[(I32, "8")]);
                let slot = ctx.func.alloca_entry(PTR);
                ctx.block().store(PTR, &data_ptr, &slot);
                let scope_idx = ctx.buffer_alias_base + ctx.buffer_data_slots.len() as u32;
                ctx.buffer_data_slots.insert(id, (slot.clone(), scope_idx));
                ctx.buffer_view_slots.insert(
                    id,
                    BufferViewSlot {
                        data_slot: slot,
                        scope_idx: Some(scope_idx),
                        elem: BufferElem::U8,
                        alias: AliasState::NoAliasProven,
                        length_source: Some(buffer_alloc_length_source(init_expr)),
                    },
                );
            }
            if let Some(source_id) = buffer_local_alias_source(init_expr) {
                crate::expr::alias_buffer_view_slot(
                    ctx,
                    id,
                    source_id,
                    MaterializationReason::UnknownAlias,
                );
            }
        }
        return Ok(());
    }
    // Boxed local: allocate a heap box and store its pointer
    // in the slot. `LocalGet` / `LocalSet` / `Update` on this
    // id all dereference through the box. See `boxed_vars` on
    // FnCtx for why this exists.
    //
    // CRITICAL: register the local's slot BEFORE lowering the
    // init expression — same as the non-boxed path. Self-
    // recursive closures (`let fib = (n) => fib(n-1)`) need
    // to find the slot during their capture pass. Without
    // this, the capture reads 0.0 from the soft fallback
    // instead of the box pointer.
    if ctx.boxed_vars.contains(&id) {
        // Issue #569: if `Stmt::PreallocateBoxes` already alloca'd
        // a slot+box for this id at function-body entry, skip the
        // fresh alloc and just `js_box_set` the init value into
        // the existing box. The slot is already registered in
        // `ctx.locals` from the prealloc pass.
        if ctx.prealloc_boxes.contains(&id) {
            ctx.local_types.insert(id, refined_ty.clone());
            if let Some(init_expr) = init {
                let init_val = lower_expr_with_expected_type(ctx, init_expr, Some(&refined_ty))?;
                let slot_clone = ctx.locals[&id].clone();
                let blk = ctx.block();
                let box_dbl = blk.load(DOUBLE, &slot_clone);
                let bptr = blk.bitcast_double_to_i64(&box_dbl);
                blk.call_void(
                    "js_box_set",
                    &[(crate::types::I64, &bptr), (DOUBLE, &init_val)],
                );
            }
            return Ok(());
        }
        // Step 1: allocate box with undefined sentinel.
        let undef = crate::nanbox::double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED));
        let blk = ctx.block();
        let box_ptr = blk.call(crate::types::I64, "js_box_alloc", &[(DOUBLE, &undef)]);
        // Slot must live in the entry block — closures from sibling
        // branches may capture this id later, and an alloca placed
        // here would not dominate those branches' loads.
        let slot = ctx.func.alloca_entry(DOUBLE);
        let box_as_double = ctx.block().bitcast_i64_to_double(&box_ptr);
        ctx.block().store(DOUBLE, &box_as_double, &slot);
        // Step 2: register BEFORE lowering init.
        ctx.locals.insert(id, slot);
        ctx.local_types.insert(id, refined_ty.clone());
        crate::expr::emit_shadow_slot_bind_for_local(ctx, id);
        // Step 3: lower init and store into the box.
        if let Some(init_expr) = init {
            let init_val = lower_expr_with_expected_type(ctx, init_expr, Some(&refined_ty))?;
            // Read the box pointer back from the slot and
            // js_box_set the real init value.
            let slot_clone = ctx.locals[&id].clone();
            let blk = ctx.block();
            let box_dbl = blk.load(DOUBLE, &slot_clone);
            let bptr = blk.bitcast_double_to_i64(&box_dbl);
            blk.call_void(
                "js_box_set",
                &[(crate::types::I64, &bptr), (DOUBLE, &init_val)],
            );
        }
        return Ok(());
    }
    // Slot must live in the entry block — see the boxed-var case
    // above. Putting allocas inside an `if` arm causes verifier
    // failures the moment a closure in another branch captures
    // this local, because the alloca block doesn't dominate the
    // closure-capture site.
    let slot = ctx.func.alloca_entry(DOUBLE);
    // Initialize to TAG_UNDEFINED so that if a try/catch path
    // skips the real init, reads from this slot produce undefined
    // (which runtime functions handle safely) rather than 0.0
    // (which looks like a null pointer when NaN-unboxed).
    {
        let undef = crate::nanbox::double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED));
        ctx.func.entry_allocas_push_store(DOUBLE, &undef, &slot);
    }
    ctx.locals.insert(id, slot.clone());
    ctx.local_types.insert(id, refined_ty.clone());
    // Int32 specialization (issue #48): if this local qualifies as
    // integer-valued (all writes are `| 0` / `>>> 0` / bitwise / int
    // literal / ++/--), allocate a parallel i32 slot. Update/LocalSet
    // mirror writes to it; IndexGet and hot-loop consumers prefer it
    // over the double slot — skipping the `fadd → fcvtzs → scvtf`
    // round-trip per iteration of `sum = (sum + i) | 0`.
    //
    // Only fire on `mutable` locals: an immutable `const SEED = 0xDEAD_BEEF`
    // never benefits from i32 specialization (no per-iteration cost), and
    // its initializer may legitimately exceed i32 range (e.g. 0x9E3779B9
    // = 2654435769 > INT32_MAX) — fptosi'ing it saturates to INT32_MAX
    // and silently corrupts every read of the i32 slot. Mutable locals
    // are always written through paths we control (Update, `(expr) | 0`)
    // which produce in-range int32 values per JS ToInt32 semantics.
    let init_in_i32_range = match init {
        Some(perry_hir::Expr::Integer(n)) => i32::try_from(*n).is_ok(),
        _ => true, // non-Integer init: writes will always go via i32-coercing paths
    };
    // Issue #140 follow-up + #435 fix: gate the Let-site i32
    // shadow on `index_used_locals` (with transitive closure —
    // see `collect_index_used_locals` in collectors.rs).  The
    // original v0.5.164 gate dropped the shadow for image-
    // convolution's transitively-index-used locals (`xx → idx
    // → array[idx]`) because the analysis was direct-only; the
    // comment said dropping the gate was "fine" because
    // `is_int32_producing_expr` would keep the right locals
    // off the shadow path.  That claim was wrong:
    // `is_int32_producing_expr` accepts `Add | Sub | Mul`
    // over int-stable operands, so pure accumulators like
    // `let sum = 0; for (...) sum = sum + compute(i)` (the
    // canonical 14_closure shape) ended up with an i32 shadow
    // whose reads truncated 64-bit sums to 32-bit signed
    // integers — silent-correctness bug, exit 0, no
    // diagnostics.  The gate-with-transitive-closure restores
    // both invariants: image_conv's chain stays on the i32
    // path (xx is transitively index-used through idx), and
    // accumulators that never reach an array index stay off
    // it.
    //
    // Drop the `*mutable` gate: immutable integer-stable Lets
    // also benefit from an i32 shadow when they participate in
    // an integer-arithmetic chain (`const row = yy * W;` then
    // `idx = (row + xx) * 3` in a hot inner loop). The
    // saturation concern in the original v0.5.164 comment was
    // about `const SEED = 0x9E3779B9 >>> 0` whose value
    // exceeds INT32_MAX — but that's a u32 (`>>> 0`), and
    // `>>> 0` is intentionally not seeded into signed integer_locals
    // (see collect_integer_let_ids). Mutable u32 recurrences are handled
    // separately through unsigned_i32_locals so ordinary JS reads use
    // `uitofp` instead of signed `sitofp`.
    // (Issue #436) Allow the i32 fast path when the local is
    // either index-used (existing #435 path) OR
    // strictly-i32-bounded by every write (new path that
    // recovers the FNV-1a `h` accumulator and similar
    // explicit-i32-coerce shapes without reintroducing #435's
    // accumulator overflow).
    let is_unsigned_i32_local = ctx.unsigned_i32_locals.contains(&id);
    let i32_safe_local = ctx.index_used_locals.contains(&id)
        || ctx.strictly_i32_bounded_locals.contains(&id)
        || is_unsigned_i32_local;
    let needs_i32_slot = (ctx.integer_locals.contains(&id) || is_unsigned_i32_local)
        && i32_safe_local
        && init_in_i32_range
        && !ctx.boxed_vars.contains(&id)
        && !ctx.module_globals.contains_key(&id)
        && !ctx.i32_counter_slots.contains_key(&id);
    if needs_i32_slot {
        let i32_slot = ctx.func.alloca_entry(I32);
        ctx.func.entry_allocas_push_store(I32, "0", &i32_slot);
        ctx.i32_counter_slots.insert(id, i32_slot);
    }
    // Issue #50 follow-up: when this local is a row alias of a
    // flat-const 2D int array, `try_lower_flat_const_index_get` will
    // intercept every `LocalGet(this).at(j)` access at lowering time
    // and emit a direct GEP into the `[N x i32]` global — the slot
    // value is never read. Skip lowering the init expression
    // (`let krow = KERNEL[ky+2]` would otherwise emit a generic
    // IndexGet with the v0.5.357 lazy/forwarded cond_br guard,
    // serializing the inner conv loop through `js_array_get_f64`
    // and blocking SIMD on `image_convolution`'s 5×5 blur kernel).
    // Park TAG_UNDEFINED in the slot so any pathological non-alias
    // read (`console.log(krow)`) gets `undefined` rather than
    // garbage; DCE removes the dummy store when no such reader
    // exists.
    if init.is_some() && ctx.array_row_aliases.contains_key(&id) {
        let undef = crate::nanbox::double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED));
        ctx.block().store(DOUBLE, &undef, &slot);
    } else if let Some(init_expr) = init {
        // Issue #49 follow-up: i32-native init path. If this local
        // has an i32 shadow slot AND the init expression can be
        // lowered straight to i32 (Add/Sub/Mul/bitwise on i32
        // operands, clamp call, MathImul, Integer literal,
        // Buffer/Uint8ArrayGet, …), compute the init in i32
        // directly and `sitofp` to seed the double slot. This
        // avoids the `fadd → fmul → fptosi` round-trip that
        // image_convolution's `let row = yy * W` would otherwise
        // emit when both operands have i32 slots.
        let used_i32_init = if let Some(i32_slot) = ctx.i32_counter_slots.get(&id).cloned() {
            let i32_slots = ctx.i32_counter_slots.clone();
            let flat_ca = ctx.flat_const_arrays.clone();
            let ara = ctx.array_row_aliases.clone();
            let int_locals = ctx.integer_locals.clone();
            if crate::expr::can_lower_expr_as_i32(
                init_expr,
                &i32_slots,
                &flat_ca,
                &ara,
                &int_locals,
                ctx.clamp3_functions,
                ctx.clamp_u8_functions,
                ctx.integer_returning_functions,
                ctx.i32_identity_functions,
            ) {
                let i32_v = crate::expr::lower_expr_as_i32(ctx, init_expr)?;
                let unsigned_i32 = ctx.unsigned_i32_locals.contains(&id);
                let blk = ctx.block();
                blk.store(I32, &i32_v, &i32_slot);
                let v = if unsigned_i32 {
                    blk.uitofp(I32, &i32_v, DOUBLE)
                } else {
                    blk.sitofp(I32, &i32_v, DOUBLE)
                };
                blk.store(DOUBLE, &v, &slot);
                true
            } else {
                false
            }
        } else {
            false
        };
        let v = if !used_i32_init {
            let v = lower_expr_with_expected_type(ctx, init_expr, Some(&refined_ty))?;
            // String aliasing fix: `let y = x` (init is `LocalGet`
            // of a string-typed local) shares the same heap
            // pointer between `y` and `x`. A later
            // `x = x + suffix` would otherwise see refcount==1
            // and mutate the string in-place via
            // `js_string_append`'s fast path, also corrupting
            // `y`. Mark the underlying string as shared so the
            // next append allocates fresh. Pre-fix this didn't
            // surface in practice; the v0.5.667 finally-inline
            // pass (issue #536) introduced exactly this aliasing
            // shape via its `let __finally_ret_<id> = X` hoist
            // and `test_edge_error_handling`'s `finallyReturn`
            // started returning `start-try-finally` instead of
            // `start-try`.
            if let perry_hir::Expr::LocalGet(src_id) = init_expr {
                if matches!(ctx.local_types.get(src_id), Some(perry_types::Type::String)) {
                    let blk = ctx.block();
                    let s_ptr = blk.call(
                        crate::types::I64,
                        "js_get_string_pointer_unified",
                        &[(DOUBLE, &v)],
                    );
                    blk.call_void("js_string_addref", &[(crate::types::I64, &s_ptr)]);
                }
            }
            ctx.block().store(DOUBLE, &v, &slot);
            v
        } else {
            String::new() // unused below; cleanup blocks check used_i32_init
        };
        // Gen-GC Phase A sub-phase 3b: if this local has a
        // shadow-frame slot, mirror the store into the
        // frame. Bitcast double → i64 (NaN-box bits) then
        // call js_shadow_slot_set. LLVM will fold the
        // redundant double-alloca and i64-pass through
        // mem2reg/SROA in many cases; when it can't, the
        // cost is one bitcast + one call per pointer-typed
        // Let — measured noise on bench_json_roundtrip.
        // Only fires when PERRY_SHADOW_STACK=1 is set at
        // compile time, since the map is empty otherwise.
        if !used_i32_init {
            if ctx.shadow_slot_map.contains_key(&id)
                && !crate::expr::expr_is_known_non_pointer_shadow_value(ctx, init_expr)
            {
                crate::expr::emit_shadow_slot_update_for_expr(ctx, id, &v, init_expr);
            }
            // Seed the i32 slot from the init value when the local has one.
            // Use fptosi→i64 + trunc→i32 instead of direct fptosi→i32
            // to handle unsigned values (e.g. `let s = 0x9E3779B9 >>> 0`
            // where the double exceeds INT32_MAX). Direct fptosi→i32 is
            // UB for such values; going through i64 then truncating gives
            // the correct bit pattern.
            if let Some(i32_slot) = ctx.i32_counter_slots.get(&id).cloned() {
                let v_i64 = ctx.block().fptosi(DOUBLE, &v, crate::types::I64);
                let v_i32 = ctx.block().trunc(crate::types::I64, &v_i64, I32);
                ctx.block().store(I32, &v_i32, &i32_slot);
            }
        }
        // Buffer data-pointer slot for local (non-global) const buffers. The
        // HIR fact layer owns the source-shape decision; lowering only consumes
        // the stable local-id fact and emits the ptr slot used by
        // Uint8ArrayGet/Set.
        //
        // Only relevant on the f64-init path (BufferAlloc isn't
        // i32-able, so used_i32_init is always false here, but
        // gate explicitly to keep the invariant readable).
        if !used_i32_init && ctx.known_noalias_buffer_locals.contains(&id) {
            let blk = ctx.block();
            let handle = crate::expr::unbox_to_i64(blk, &v);
            let handle_ptr = blk.inttoptr(I64, &handle);
            let data_ptr = blk.gep(I8, &handle_ptr, &[(I32, "8")]);
            let buf_slot = ctx.func.alloca_entry(PTR);
            ctx.block().store(PTR, &data_ptr, &buf_slot);
            let scope_idx = ctx.buffer_alias_base + ctx.buffer_data_slots.len() as u32;
            ctx.buffer_data_slots
                .insert(id, (buf_slot.clone(), scope_idx));
            ctx.buffer_view_slots.insert(
                id,
                BufferViewSlot {
                    data_slot: buf_slot,
                    scope_idx: Some(scope_idx),
                    elem: BufferElem::U8,
                    alias: AliasState::NoAliasProven,
                    length_source: Some(buffer_alloc_length_source(init_expr)),
                },
            );
        }
        if let Some(source_id) = buffer_local_alias_source(init_expr) {
            crate::expr::alias_buffer_view_slot(
                ctx,
                id,
                source_id,
                MaterializationReason::UnknownAlias,
            );
        }
    } else if let Some(cv) = ctx.compile_time_constants.get(&id) {
        // Compile-time constants (e.g. `declare const __platform__: number`)
        // have no init expression but their value is known. Store the
        // constant value so runtime reads get the correct number instead
        // of TAG_UNDEFINED (a NaN that fails all numeric comparisons).
        let lit = crate::nanbox::double_literal(*cv);
        ctx.block().store(DOUBLE, &lit, &slot);
    }
    Ok(())
}

fn native_i32_alias_source(expr: &perry_hir::Expr) -> Option<u32> {
    match expr {
        perry_hir::Expr::Binary {
            op: perry_hir::BinaryOp::BitOr,
            left,
            right,
        } if matches!(right.as_ref(), perry_hir::Expr::Integer(0)) => match left.as_ref() {
            perry_hir::Expr::LocalGet(id) => Some(*id),
            _ => native_i32_alias_source(left),
        },
        perry_hir::Expr::LocalGet(id) => Some(*id),
        _ => None,
    }
}

fn buffer_local_alias_source(expr: &perry_hir::Expr) -> Option<u32> {
    match expr {
        perry_hir::Expr::LocalGet(id) => Some(*id),
        _ => None,
    }
}

fn math_min_length_buffer_ids(expr: &perry_hir::Expr) -> Option<Vec<u32>> {
    let perry_hir::Expr::MathMin(args) = expr else {
        return None;
    };
    if args.len() < 2 {
        return None;
    }
    let mut out = Vec::new();
    for arg in args {
        if let Some(id) = length_of_local_buffer_id(arg) {
            out.push(id);
        } else {
            return None;
        }
    }
    out.sort_unstable();
    out.dedup();
    (!out.is_empty()).then_some(out)
}

fn length_of_local_buffer_id(expr: &perry_hir::Expr) -> Option<u32> {
    match expr {
        perry_hir::Expr::Uint8ArrayLength(inner) | perry_hir::Expr::BufferLength(inner) => {
            match inner.as_ref() {
                perry_hir::Expr::LocalGet(id) => Some(*id),
                _ => None,
            }
        }
        perry_hir::Expr::PropertyGet { object, property } if property == "length" => {
            match object.as_ref() {
                perry_hir::Expr::LocalGet(id) => Some(*id),
                _ => None,
            }
        }
        _ => None,
    }
}

fn buffer_alloc_length_source(expr: &perry_hir::Expr) -> LengthSource {
    let len = match expr {
        perry_hir::Expr::BufferAlloc { size, .. } => Some(size.as_ref()),
        perry_hir::Expr::BufferAllocUnsafe(size) => Some(size.as_ref()),
        perry_hir::Expr::Uint8ArrayNew(Some(size)) => Some(size.as_ref()),
        _ => None,
    };
    len.and_then(length_source_from_expr)
        .unwrap_or(LengthSource::Unknown)
}

fn length_source_from_expr(expr: &perry_hir::Expr) -> Option<LengthSource> {
    match expr {
        perry_hir::Expr::Integer(n) => Some(LengthSource::Constant(*n)),
        perry_hir::Expr::LocalGet(id) => Some(LengthSource::Local { id: *id, addend: 0 }),
        perry_hir::Expr::Binary {
            op: perry_hir::BinaryOp::Add,
            left,
            right,
        } => match (left.as_ref(), right.as_ref()) {
            (perry_hir::Expr::LocalGet(id), perry_hir::Expr::Integer(addend))
            | (perry_hir::Expr::Integer(addend), perry_hir::Expr::LocalGet(id)) => {
                Some(LengthSource::Local {
                    id: *id,
                    addend: *addend,
                })
            }
            _ => None,
        },
        _ => None,
    }
}

/// Extract all field names (parent chain + own) and the constructor for
/// a class, cloning everything out of `ctx.classes` so the immutable
/// borrow is released before the caller mutates `ctx`.
///
/// Returns `None` if the class is not found in `ctx.classes`.
pub(crate) fn collect_scalar_class_data(
    ctx: &FnCtx<'_>,
    class_name: &str,
) -> Option<(Vec<String>, Option<perry_hir::Function>)> {
    let class = ctx.classes.get(class_name)?;
    let mut all_fields: Vec<String> = Vec::new();
    let mut chain: Vec<String> = Vec::new();
    let mut p = class.extends_name.clone();
    while let Some(pname) = p {
        chain.push(pname.clone());
        if let Some(pc) = ctx.classes.get(pname.as_str()) {
            p = pc.extends_name.clone();
        } else {
            break;
        }
    }
    chain.reverse();
    for pname in &chain {
        if let Some(pc) = ctx.classes.get(pname.as_str()) {
            for f in &pc.fields {
                all_fields.push(f.name.clone());
            }
        }
    }
    for f in &class.fields {
        all_fields.push(f.name.clone());
    }
    let ctor = class.constructor.clone();
    Some((all_fields, ctor))
}

fn lower_unused_expr(ctx: &mut FnCtx<'_>, expr: &perry_hir::Expr) -> Result<bool> {
    match expr {
        perry_hir::Expr::New {
            class_name, args, ..
        } if class_name.starts_with("__AnonShape_") => {
            // Anonymous-shape `new` is how object literals lower. When the
            // constructed value is immediately discarded by scalar replacement
            // we still must preserve evaluation order of every property value,
            // but we can skip the synthetic object allocation/field stores.
            for arg in args {
                if !lower_unused_expr(ctx, arg)? {
                    let _ = lower_expr(ctx, arg)?;
                }
            }
            Ok(true)
        }
        perry_hir::Expr::ArrayMap { array, callback } => {
            if array_map_callback_is_discard_pure(callback) {
                // The map result is unused and the callback only builds an
                // anonymous object from its parameter. Evaluate the receiver to
                // preserve source-order effects, but skip closure allocation,
                // callback dispatch, and all discarded object construction.
                let _ = lower_expr(ctx, array)?;
                return Ok(true);
            }
            let arr_box = lower_expr(ctx, array)?;
            let cb_box = lower_expr(ctx, callback)?;
            let blk = ctx.block();
            let arr_handle = crate::expr::unbox_to_i64(blk, &arr_box);
            let cb_handle = crate::expr::unbox_to_i64(blk, &cb_box);
            blk.call_void(
                "js_array_map_discard",
                &[(I64, &arr_handle), (I64, &cb_handle)],
            );
            Ok(true)
        }
        _ => Ok(false),
    }
}

fn array_map_callback_is_discard_pure(callback: &perry_hir::Expr) -> bool {
    let perry_hir::Expr::Closure {
        params,
        body,
        captures,
        mutable_captures,
        captures_this,
        is_async,
        ..
    } = callback
    else {
        return false;
    };
    if *is_async
        || *captures_this
        || !captures.is_empty()
        || !mutable_captures.is_empty()
        || params.is_empty()
    {
        return false;
    }
    let param_id = params[0].id;
    matches!(body.as_slice(), [perry_hir::Stmt::Return(Some(expr))] if discard_pure_expr(expr, param_id))
}

fn discard_pure_expr(expr: &perry_hir::Expr, param_id: perry_types::LocalId) -> bool {
    match expr {
        perry_hir::Expr::Undefined
        | perry_hir::Expr::Null
        | perry_hir::Expr::Bool(_)
        | perry_hir::Expr::Number(_)
        | perry_hir::Expr::Integer(_)
        | perry_hir::Expr::String(_)
        | perry_hir::Expr::WtfString(_) => true,
        perry_hir::Expr::LocalGet(id) => *id == param_id,
        // PropertyGet is deliberately *not* in the pure set: TypeScript
        // `get` accessors can run user code, so eliding the map body
        // would drop visible side effects. The intended target of this
        // optimization is the anonymous-shape `Expr::New` arm below.
        perry_hir::Expr::New {
            class_name, args, ..
        } if class_name.starts_with("__AnonShape_") => {
            args.iter().all(|arg| discard_pure_expr(arg, param_id))
        }
        _ => false,
    }
}
