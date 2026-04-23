//! Statement codegen — Phase 2.
//!
//! Supports: Expr, Return(Some|None), If (with/without else), Let. Enough
//! for a recursive fibonacci function plus `console.log(fibonacci(N))` at
//! top level. Loops and Date.now land in Phase 2.1.

use anyhow::{anyhow, bail, Result};
use perry_hir::Stmt;

use crate::expr::{lower_expr, FnCtx};
use crate::loop_purity::body_needs_asm_barrier;
use crate::lower_conditional::lower_truthy;
use crate::types::{DOUBLE, I8, I32, I64, PTR};

/// Lower a sequence of statements into the current block of `ctx`. If any
/// statement splits control flow, `ctx.current_block` is updated to the
/// "fall-through" block after the split.
pub(crate) fn lower_stmts(ctx: &mut FnCtx<'_>, stmts: &[Stmt]) -> Result<()> {
    for s in stmts {
        lower_stmt(ctx, s)?;
        // If an earlier statement already terminated the current block
        // (e.g. return in a straight-line sequence), any following statement
        // would emit dead code. Anvil silently drops these at the block
        // level; we do the same here to avoid tripping LLVM's verifier.
        if ctx.block().is_terminated() {
            break;
        }
    }
    Ok(())
}

pub(crate) fn lower_stmt(ctx: &mut FnCtx<'_>, stmt: &Stmt) -> Result<()> {
    match stmt {
        Stmt::Expr(e) => {
            let _ = lower_expr(ctx, e)?;
            Ok(())
        }

        Stmt::Return(Some(e)) => {
            let v = lower_expr(ctx, e)?;
            // Phase E: async functions wrap their return value in
            // js_promise_resolved so callers can await the result.
            // If the value is already a promise (e.g. `return
            // Promise.resolve(x)`), js_promise_resolved is a no-op
            // wrap that the caller's await loop unwraps anyway.
            let final_v = if ctx.is_async_fn {
                let blk = ctx.block();
                let handle = blk.call(crate::types::I64, "js_promise_resolved", &[(DOUBLE, &v)]);
                crate::expr::nanbox_pointer_inline_pub(blk, &handle)
            } else {
                v
            };
            ctx.block().ret(DOUBLE, &final_v);
            Ok(())
        }
        Stmt::Return(None) => {
            // Bare `return;` returns undefined (encoded as 0.0). For
            // async functions, wrap undefined in a resolved promise.
            if ctx.is_async_fn {
                let zero = "0.0".to_string();
                let blk = ctx.block();
                let handle = blk.call(crate::types::I64, "js_promise_resolved", &[(DOUBLE, &zero)]);
                let boxed = crate::expr::nanbox_pointer_inline_pub(blk, &handle);
                ctx.block().ret(DOUBLE, &boxed);
            } else {
                ctx.block().ret(DOUBLE, "0.0");
            }
            Ok(())
        }

        Stmt::Let { id, name, init, ty, mutable, .. } => {
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
            ctx.local_id_to_name.insert(*id, name.clone());
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
            match init.as_ref() {
                Some(perry_hir::Expr::ClassRef(class_name)) => {
                    ctx.local_class_aliases
                        .insert(name.clone(), class_name.clone());
                }
                Some(perry_hir::Expr::LocalGet(other_id)) => {
                    if let Some(other_name) = ctx.local_id_to_name.get(other_id).cloned() {
                        if let Some(resolved) = ctx.local_class_aliases.get(&other_name).cloned() {
                            ctx.local_class_aliases.insert(name.clone(), resolved);
                        }
                    }
                }
                _ => {}
            }

            // Issue #50: row-alias detection. When `let krow = X[i]` where
            // `X` is a folded flat-const 2D int array, record
            // `krow_id → (X_id, i)` so a later `krow[j]` can lower through
            // the same flat `[N x i32]` load path as an inline `X[i][j]`.
            // Only fires for non-mutable lets (reassignment would invalidate
            // the alias relationship).
            if !*mutable {
                if let Some(perry_hir::Expr::IndexGet { object, index }) = init.as_ref() {
                    if let perry_hir::Expr::LocalGet(const_id) = object.as_ref() {
                        if ctx.flat_const_arrays.contains_key(const_id) {
                            ctx.array_row_aliases.insert(
                                *id,
                                (*const_id, Box::new((**index).clone())),
                            );
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
                init.as_ref()
                    .and_then(|e| crate::type_analysis::refine_type_from_init(ctx, e))
                    .unwrap_or_else(|| ty.clone())
            } else if matches!(ty, perry_types::Type::Array(ref elem) if matches!(**elem, perry_types::Type::Any)) {
                // Also refine Array<Any> when the init provides more
                // specific element type info. Object.keys() returns
                // Array<string> but the HIR often declares Array<Any>.
                init.as_ref()
                    .and_then(|e| crate::type_analysis::refine_type_from_init(ctx, e))
                    .unwrap_or_else(|| ty.clone())
            } else {
                ty.clone()
            };

            // Track closure func_id → local_id mapping so the closure
            // call site in lower_call can look up rest param info.
            if let Some(perry_hir::Expr::Closure { func_id: cfid, .. }) = init.as_ref() {
                ctx.local_closure_func_ids.insert(*id, *cfid);
            }

            // Scalar replacement: if this Let binds a non-escaping array
            // literal, skip the heap allocation entirely. Each element gets
            // its own stack alloca; constant-index reads in the Let's scope
            // load directly from the corresponding slot. See the
            // `collect_non_escaping_arrays` pass in collectors.rs for the
            // escape criteria.
            if let Some(perry_hir::Expr::Array(elements)) = init.as_ref() {
                if ctx.non_escaping_arrays.contains_key(id) {
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
                    ctx.scalar_replaced_arrays.insert(*id, slots);

                    // Register the local's type + a dummy slot so any surviving
                    // LocalGet (e.g. debug instrumentation, unrecognized
                    // expression shapes the collector conservatively rejected)
                    // still resolves; the actual scalar-replaced reads short-
                    // circuit before hitting this slot.
                    ctx.local_types.insert(*id, refined_ty);
                    let dummy_slot = ctx.func.alloca_entry(DOUBLE);
                    ctx.locals.insert(*id, dummy_slot);
                    return Ok(());
                }
            }

            // Scalar replacement: if this Let binds a non-escaping object
            // literal, skip the heap allocation entirely. One alloca per
            // unique field; PropertyGet/Set already resolve through
            // `ctx.scalar_replaced`, so no additional read path is needed.
            // See `collect_non_escaping_object_literals` in collectors.rs.
            if let Some(perry_hir::Expr::Object(props)) = init.as_ref() {
                if let Some(field_order) = ctx.non_escaping_object_literals.get(id).cloned() {
                    let undef = crate::nanbox::double_literal(f64::from_bits(
                        crate::nanbox::TAG_UNDEFINED,
                    ));
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

                    ctx.scalar_replaced.insert(*id, field_slots);

                    // Register type + dummy slot so any surviving LocalGet
                    // (conservative collector rejects are possible) resolves
                    // — the scalar-replaced PropertyGet/Set paths short-
                    // circuit before loading this slot.
                    ctx.local_types.insert(*id, refined_ty);
                    let dummy_slot = ctx.func.alloca_entry(DOUBLE);
                    ctx.locals.insert(*id, dummy_slot);
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
            if let Some(perry_hir::Expr::New { class_name, args, .. }) = init.as_ref() {
                let is_imported = ctx.imported_class_ctors.contains_key(class_name);
                if ctx.non_escaping_news.contains_key(id) && !is_imported {
                    // Extract all class data we need (field names + ctor) before
                    // taking mutable borrows on ctx. Clone out of the shared
                    // `classes` map so we release the immutable borrow early.
                    let scalar_data = collect_scalar_class_data(ctx, class_name);

                    if let Some((all_fields, ctor)) = scalar_data {
                        // Create per-field allocas
                        let mut field_slots: std::collections::HashMap<String, String> =
                            std::collections::HashMap::new();
                        for fname in &all_fields {
                            let slot = ctx.func.alloca_entry(DOUBLE);
                            let undef = crate::nanbox::double_literal(f64::from_bits(
                                crate::nanbox::TAG_UNDEFINED,
                            ));
                            ctx.func.entry_allocas_push_store(DOUBLE, &undef, &slot);
                            field_slots.insert(fname.clone(), slot);
                        }

                        ctx.scalar_replaced.insert(*id, field_slots);

                        // Register type + dummy slot so LocalGet doesn't fail
                        ctx.local_types.insert(*id, refined_ty);
                        let dummy_slot = ctx.func.alloca_entry(DOUBLE);
                        ctx.locals.insert(*id, dummy_slot);

                        // Lower args first
                        let mut lowered_args: Vec<String> = Vec::new();
                        for a in args {
                            lowered_args.push(lower_expr(ctx, a)?);
                        }

                        // Push scalar ctor target so PropertySet on `this` routes to allocas
                        ctx.scalar_ctor_target.push(*id);
                        ctx.class_stack.push(class_name.clone());
                        // A dummy this_stack entry — the ctor body references Expr::This
                        // but scalar-replaced PropertySet intercepts it before loading
                        let dummy_this = ctx.func.alloca_entry(DOUBLE);
                        ctx.this_stack.push(dummy_this);

                        // Apply field initializers
                        crate::lower_call::apply_field_initializers_recursive_pub(ctx, class_name)?;

                        // Inline constructor body if present
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
            if let Some(global_name) = ctx.module_globals.get(id).cloned() {
                ctx.local_types.insert(*id, refined_ty);
                if let Some(init_expr) = init {
                    let v = lower_expr(ctx, init_expr)?;
                    let g_ref = format!("@{}", global_name);
                    ctx.block().store(DOUBLE, &v, &g_ref);

                    // Buffer data-pointer slot: when init is BufferAlloc on an
                    // immutable module global, pre-compute the data base pointer
                    // (handle + 8, past BufferHeader) and store it in a ptr
                    // alloca. Uint8ArrayGet/Set then uses `getelementptr inbounds`
                    // from this pointer instead of the inttoptr chain — giving
                    // LLVM proper pointer provenance for auto-vectorization.
                    if !*mutable {
                        if matches!(init_expr, perry_hir::Expr::BufferAlloc { .. }) {
                            let blk = ctx.block();
                            let handle = crate::expr::unbox_to_i64(blk, &v);
                            let handle_ptr = blk.inttoptr(I64, &handle);
                            let data_ptr = blk.gep(I8, &handle_ptr, &[(I32, "8")]);
                            let slot = ctx.func.alloca_entry(PTR);
                            ctx.block().store(PTR, &data_ptr, &slot);
                            let scope_idx = ctx.buffer_alias_base + ctx.buffer_data_slots.len() as u32;
                            ctx.buffer_data_slots.insert(*id, (slot, scope_idx));
                        }
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
            if ctx.boxed_vars.contains(id) {
                // Step 1: allocate box with undefined sentinel.
                let undef = crate::nanbox::double_literal(f64::from_bits(
                    crate::nanbox::TAG_UNDEFINED,
                ));
                let blk = ctx.block();
                let box_ptr =
                    blk.call(crate::types::I64, "js_box_alloc", &[(DOUBLE, &undef)]);
                // Slot must live in the entry block — closures from sibling
                // branches may capture this id later, and an alloca placed
                // here would not dominate those branches' loads.
                let slot = ctx.func.alloca_entry(DOUBLE);
                let box_as_double = ctx.block().bitcast_i64_to_double(&box_ptr);
                ctx.block().store(DOUBLE, &box_as_double, &slot);
                // Step 2: register BEFORE lowering init.
                ctx.locals.insert(*id, slot);
                ctx.local_types.insert(*id, refined_ty);
                // Step 3: lower init and store into the box.
                if let Some(init_expr) = init {
                    let init_val = lower_expr(ctx, init_expr)?;
                    // Read the box pointer back from the slot and
                    // js_box_set the real init value.
                    let slot_clone = ctx.locals[id].clone();
                    let blk = ctx.block();
                    let box_dbl = blk.load(DOUBLE, &slot_clone);
                    let bptr = blk.bitcast_double_to_i64(&box_dbl);
                    blk.call_void("js_box_set", &[(crate::types::I64, &bptr), (DOUBLE, &init_val)]);
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
                let undef = crate::nanbox::double_literal(f64::from_bits(
                    crate::nanbox::TAG_UNDEFINED,
                ));
                ctx.func.entry_allocas_push_store(DOUBLE, &undef, &slot);
            }
            ctx.locals.insert(*id, slot.clone());
            ctx.local_types.insert(*id, refined_ty);
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
            let init_in_i32_range = match init.as_ref() {
                Some(perry_hir::Expr::Integer(n)) => i32::try_from(*n).is_ok(),
                _ => true, // non-Integer init: writes will always go via i32-coercing paths
            };
            // Issue #140: only emit the parallel i32 shadow slot when this local
            // actually participates in an array/buffer index expression. Pure
            // accumulators (`sum = sum + 1` between `Date.now()` calls) would
            // otherwise get a dead i32/sitofp mirror that blocks LLVM's
            // vectorizer — combined with the #74 `asm sideeffect` loop barrier
            // the fadd reduction can't be SIMD-widened. Real loop counters that
            // match `classify_for_length_hoist` in `lower_for` still pick up
            // their i32 slot via the loop-specific allocation path a few hundred
            // lines below, so `for (let i=0; i<arr.length; i++) arr[i]=v` keeps
            // its fast-path even when this gate skips the Let site.
            let needs_i32_slot = ctx.integer_locals.contains(id)
                && ctx.index_used_locals.contains(id)
                && *mutable
                && init_in_i32_range
                && !ctx.boxed_vars.contains(id)
                && !ctx.module_globals.contains_key(id)
                && !ctx.i32_counter_slots.contains_key(id);
            if needs_i32_slot {
                let i32_slot = ctx.func.alloca_entry(I32);
                ctx.func.entry_allocas_push_store(I32, "0", &i32_slot);
                ctx.i32_counter_slots.insert(*id, i32_slot);
            }
            if let Some(init_expr) = init {
                let v = lower_expr(ctx, init_expr)?;
                ctx.block().store(DOUBLE, &v, &slot);
                // Seed the i32 slot from the init value when the local has one.
                // Use fptosi→i64 + trunc→i32 instead of direct fptosi→i32
                // to handle unsigned values (e.g. `let s = 0x9E3779B9 >>> 0`
                // where the double exceeds INT32_MAX). Direct fptosi→i32 is
                // UB for such values; going through i64 then truncating gives
                // the correct bit pattern.
                if let Some(i32_slot) = ctx.i32_counter_slots.get(id).cloned() {
                    let v_i64 = ctx.block().fptosi(DOUBLE, &v, crate::types::I64);
                    let v_i32 = ctx.block().trunc(crate::types::I64, &v_i64, I32);
                    ctx.block().store(I32, &v_i32, &i32_slot);
                }
                // Buffer data-pointer slot for local (non-global) const buffers.
                // `const src = Buffer.alloc(N)` at module-level lives here when
                // `src` doesn't escape to functions — same pattern as the
                // image_conv blur kernel. The pre-computed ptr slot lets
                // Uint8ArrayGet/Set emit `getelementptr inbounds` from a
                // proper `ptr` base instead of an `inttoptr` chain.
                if !*mutable {
                    if matches!(init_expr, perry_hir::Expr::BufferAlloc { .. }) {
                        let blk = ctx.block();
                        let handle = crate::expr::unbox_to_i64(blk, &v);
                        let handle_ptr = blk.inttoptr(I64, &handle);
                        let data_ptr = blk.gep(I8, &handle_ptr, &[(I32, "8")]);
                        let buf_slot = ctx.func.alloca_entry(PTR);
                        ctx.block().store(PTR, &data_ptr, &buf_slot);
                        let scope_idx = ctx.buffer_alias_base + ctx.buffer_data_slots.len() as u32;
                        ctx.buffer_data_slots.insert(*id, (buf_slot, scope_idx));
                    }
                }
            } else if let Some(cv) = ctx.compile_time_constants.get(id) {
                // Compile-time constants (e.g. `declare const __platform__: number`)
                // have no init expression but their value is known. Store the
                // constant value so runtime reads get the correct number instead
                // of TAG_UNDEFINED (a NaN that fails all numeric comparisons).
                let lit = crate::nanbox::double_literal(*cv);
                ctx.block().store(DOUBLE, &lit, &slot);
            }
            Ok(())
        }

        Stmt::If {
            condition,
            then_branch,
            else_branch,
        } => lower_if(ctx, condition, then_branch, else_branch.as_deref()),

        Stmt::For {
            init,
            condition,
            update,
            body,
        } => lower_for(ctx, init.as_deref(), condition.as_ref(), update.as_ref(), body),

        // `while (cond) { body }` — same CFG as for-loop without init/update.
        Stmt::While { condition, body } => lower_while(ctx, condition, body),

        // `do { body } while (cond)` — body runs at least once, then cond.
        Stmt::DoWhile { body, condition } => lower_do_while(ctx, body, condition),

        // `break;` — branch to the innermost loop's exit block. The
        // current block becomes terminated; subsequent statements in
        // the same scope are dead code and `lower_stmts` skips them.
        Stmt::Break => {
            let break_label = ctx
                .loop_targets
                .last()
                .map(|(_c, b)| b.clone())
                .ok_or_else(|| anyhow!("break statement outside any loop"))?;
            ctx.block().br(&break_label);
            Ok(())
        }

        // `continue;` — branch to the innermost loop's continue target
        // (which is the update block for `for`, the cond block for
        // `while`/`do-while`).
        Stmt::Continue => {
            let cont_label = ctx
                .loop_targets
                .last()
                .map(|(c, _b)| c.clone())
                .ok_or_else(|| anyhow!("continue statement outside any loop"))?;
            ctx.block().br(&cont_label);
            Ok(())
        }

        // `switch (disc) { case A: ... case B: ... default: ... }` —
        // lowered as a tower of test/body blocks with explicit fall-through
        // (each body block falls into the next body block, not the next
        // test). `break` inside a case branches to the exit block (we
        // push a (exit, exit) entry onto loop_targets so `break` works
        // even though there's no continue target).
        //
        // Layout for `switch (d) { case A: ...; break; case B: ...; default: ... }`:
        //
        //   <pre>:
        //     %dv = <discriminant>
        //     br test_A
        //   test_A:
        //     %cmp = fcmp oeq %dv, A
        //     br i1 %cmp, body_A, test_B
        //   body_A:
        //     ...
        //     br exit            ; from `break`
        //   test_B:
        //     %cmp = fcmp oeq %dv, B
        //     br i1 %cmp, body_B, body_default
        //   body_B:
        //     ...
        //     br body_default    ; fall-through
        //   body_default:
        //     ...
        //     br exit
        //   exit:
        //
        // Default position is preserved (it goes wherever it appears in
        // source order) — falling-through into the default case from the
        // preceding case is valid JS.
        Stmt::Switch { discriminant, cases } => lower_switch(ctx, discriminant, cases),

        // Labeled statement: set the pending label so the next loop
        // lowered (for/while/do-while) can register itself in
        // `label_targets` under this name.
        Stmt::Labeled { label, body } => {
            ctx.pending_label = Some(label.clone());
            lower_stmt(ctx, body)?;
            // If the body wasn't a loop that consumed the pending label,
            // clear it to avoid leaking into subsequent statements.
            ctx.pending_label = None;
            // Clean up the label target now that we've exited the labeled
            // statement's scope.
            ctx.label_targets.remove(label);
            Ok(())
        }
        Stmt::LabeledBreak(label) => {
            if let Some((_cont, brk)) = ctx.label_targets.get(label).cloned() {
                ctx.block().br(&brk);
            } else {
                // Fallback: use innermost loop (for unresolved labels).
                let target = ctx
                    .loop_targets
                    .last()
                    .map(|(_c, b)| b.clone())
                    .ok_or_else(|| anyhow!("labeled break '{}' outside any loop", label))?;
                ctx.block().br(&target);
            }
            Ok(())
        }
        Stmt::LabeledContinue(label) => {
            if let Some((cont, _brk)) = ctx.label_targets.get(label).cloned() {
                ctx.block().br(&cont);
            } else {
                // Fallback: use innermost loop.
                let target = ctx
                    .loop_targets
                    .last()
                    .map(|(c, _b)| c.clone())
                    .ok_or_else(|| anyhow!("labeled continue '{}' outside any loop", label))?;
                ctx.block().br(&target);
            }
            Ok(())
        }

        // Phase G: real setjmp/longjmp-based exception handling.
        //
        // `throw expr` evaluates the expression, calls js_throw(value)
        // which longjmps to the most recent try block, and emits an
        // LLVM `unreachable` terminator (js_throw never returns).
        Stmt::Throw(expr) => {
            let val = lower_expr(ctx, expr)?;
            ctx.block().call_void("js_throw", &[(DOUBLE, &val)]);
            ctx.block().unreachable();
            Ok(())
        }

        // Phase G: try/catch/finally via setjmp/longjmp.
        //
        // CFG shape:
        //   <current block>:
        //     %jmpbuf = call ptr @js_try_push()
        //     %sjr    = call i32 @setjmp(ptr %jmpbuf)
        //     %is_exc = icmp ne i32 %sjr, 0
        //     br i1 %is_exc, label %catch_entry, label %try_body
        //
        //   try_body:
        //     <lower try body stmts>
        //     call void @js_try_end()
        //     br label %finally_or_merge
        //
        //   catch_entry:
        //     call void @js_try_end()        ; pop try depth before catch body
        //     %exc = call double @js_get_exception()
        //     call void @js_clear_exception()
        //     <bind catch param to %exc if present>
        //     <lower catch body stmts>
        //     br label %finally_or_merge
        //
        //   finally_or_merge:
        //     <lower finally stmts if present>
        //     <continue>
        //
        // Local variable safety: all locals are alloca-backed (stack slots),
        // not SSA registers, so they survive longjmp without explicit
        // save/restore. This is the key advantage of the alloca+mem2reg
        // strategy used by our LLVM backend.
        Stmt::Try { body, catch, finally } => {
            lower_try(ctx, body, catch.as_ref(), finally.as_deref())
        }

        other => bail!(
            "perry-codegen Phase B.12: Stmt {} not yet supported",
            stmt_variant_name(other)
        ),
    }
}

/// Try/catch/finally via setjmp/longjmp.
///
/// The CFG pattern:
///   1. Call js_try_push() to get a jmp_buf pointer
///   2. Call setjmp(jmpbuf) — returns 0 on first call, non-0 after longjmp
///   3. Branch: 0 → try_body, non-0 → catch_entry
///   4. try_body runs, calls js_try_end(), branches to finally
///   5. catch_entry calls js_try_end(), reads exception, runs catch, branches to finally
///   6. finally runs (if present), then falls through to merge
fn lower_try(
    ctx: &mut FnCtx<'_>,
    body: &[perry_hir::Stmt],
    catch: Option<&perry_hir::CatchClause>,
    finally: Option<&[perry_hir::Stmt]>,
) -> Result<()> {
    use crate::types::{I32, PTR};

    // Mark the enclosing function so IR emission adds `#1`
    // (noinline optnone). At -O2 on aarch64, LLVM's mem2reg/SROA will
    // otherwise promote allocas to SSA registers across the setjmp
    // call — making mutations performed in the try body invisible in
    // the catch block after longjmp. `returns_twice` on the setjmp
    // call site alone is not sufficient.
    ctx.func.has_try = true;

    // Allocate blocks.
    let try_body_idx = ctx.new_block("try.body");
    let catch_idx = ctx.new_block("try.catch");
    let finally_idx = ctx.new_block("try.finally");

    let try_body_label = ctx.block_label(try_body_idx);
    let catch_label = ctx.block_label(catch_idx);
    let finally_label = ctx.block_label(finally_idx);

    // --- current block: setjmp dispatch ---
    let blk = ctx.block();
    let jmpbuf = blk.call(PTR, "js_try_push", &[]);
    // CRITICAL: setjmp must carry `returns_twice` on the call site
    // too (not just the declaration). Without it, LLVM -O2 promotes
    // alloca-backed locals to SSA registers and the longjmp return
    // path sees stale pre-setjmp values instead of the try-body's
    // assignments. The standard `blk.call()` doesn't support call
    // attributes, so we emit the instruction manually.
    let sjr_reg = blk.next_reg();
    // Windows MSVC uses _setjmp(buf, frame_ptr); Unix uses setjmp(buf).
    if cfg!(target_os = "windows") {
        blk.emit_raw(format!(
            "{} = call i32 @_setjmp(ptr {}, ptr null) #0",
            sjr_reg, jmpbuf
        ));
    } else {
        blk.emit_raw(format!(
            "{} = call i32 @setjmp(ptr {}) #0",
            sjr_reg, jmpbuf
        ));
    }
    let sjr = sjr_reg;
    let is_exc = blk.icmp_ne(I32, &sjr, "0");
    blk.cond_br(&is_exc, &catch_label, &try_body_label);

    // --- try body ---
    ctx.current_block = try_body_idx;
    lower_stmts(ctx, body)?;
    if !ctx.block().is_terminated() {
        ctx.block().call_void("js_try_end", &[]);
        ctx.block().br(&finally_label);
    }

    // --- catch ---
    ctx.current_block = catch_idx;
    ctx.block().call_void("js_try_end", &[]);
    if let Some(clause) = catch {
        let exc = ctx.block().call(DOUBLE, "js_get_exception", &[]);
        ctx.block().call_void("js_clear_exception", &[]);
        // Bind the catch param (if any) to the exception value.
        if let Some((id, _name)) = &clause.param {
            // Slot lives in the entry block — a closure inside the
            // catch body may capture the exception binding and get
            // called from a sibling branch that the catch block
            // doesn't dominate.
            let slot = ctx.func.alloca_entry(DOUBLE);
            ctx.locals.insert(*id, slot.clone());
            ctx.block().store(DOUBLE, &exc, &slot);
        }
        lower_stmts(ctx, &clause.body)?;
    }
    if !ctx.block().is_terminated() {
        ctx.block().br(&finally_label);
    }

    // --- finally / merge ---
    ctx.current_block = finally_idx;
    if let Some(f) = finally {
        lower_stmts(ctx, f)?;
    }
    Ok(())
}

/// For-loop lowering: classic init / cond / body / update / exit CFG.
///
/// ```text
///   <current>:
///     <init>
///     br cond
///   for.cond:
///     <condition>          ; if missing, treat as `true` (infinite loop)
///     fcmp one cond, 0.0
///     br i1, body, exit
///   for.body:
///     <body>
///     br update            ; if not already terminated
///   for.update:
///     <update>
///     br cond              ; if not already terminated
///   for.exit:
///     <continues here>
/// ```
///
/// Phase 2.1 does not support `break` / `continue`. The body must fall
/// through to update; otherwise codegen produces dead code that LLVM will
/// reject. We don't yet pass the loop's break/continue targets through
/// FnCtx — that lands when we need it.
fn lower_for(
    ctx: &mut FnCtx<'_>,
    init: Option<&Stmt>,
    condition: Option<&perry_hir::Expr>,
    update: Option<&perry_hir::Expr>,
    body: &[Stmt],
) -> Result<()> {
    // Init runs once in the current block. A `let i = 0` here adds `i` to
    // ctx.locals, which the body can then load via LocalGet.
    if let Some(init_stmt) = init {
        lower_stmt(ctx, init_stmt)?;
    }

    // Loop-invariant length hoisting peephole. Detect the very common
    // shape `for (...; i < arr.length; ...)` where `arr` is a local
    // that the body never mutates length-wise, and pre-load
    // `arr.length` into a stack slot before entering the cond block.
    // The length load inside the cond is then replaced with a load
    // from the slot — saves two instructions per iteration (the
    // `and` to unbox arr + the `ldr` of the length field) and lets
    // LLVM hoist a couple more downstream loads now that the slot
    // is the loop-invariant source of truth.
    //
    // Without this, LLVM's LICM declines to hoist the length load
    // because the loop body's `IndexSet` slow path (`js_array_set_f64
    // _extend`) is an external call that LLVM can't prove won't
    // modify the array's length field. We do the analysis ourselves
    // and only hoist when our (more domain-specific) walker can
    // prove the body won't change `arr.length`.
    //
    // Saves ~25-30% on `for (let i = 0; i < arr.length; i++) arr[i] = i`
    // and `for (let i = 0; i < arr.length; i++) for (let j = 0; j <
    // arr.length; j++) ...` patterns.
    let hoist_classification: Option<(u32, u32)> =
        condition.and_then(|cond| classify_for_length_hoist(cond, body));
    let hoisted_length_arr_id: Option<u32> = hoist_classification.map(|(arr, _)| arr);
    let hoisted_length_slot: Option<String> = if let Some((arr_id, counter_id)) =
        hoist_classification
    {
        let arr_box_loaded = lower_expr(
            ctx,
            &perry_hir::Expr::PropertyGet {
                object: Box::new(perry_hir::Expr::LocalGet(arr_id)),
                property: "length".to_string(),
            },
        )?;
        let slot = ctx.func.alloca_entry(DOUBLE);
        ctx.block().store(DOUBLE, &arr_box_loaded, &slot);
        ctx.cached_lengths.insert(arr_id, slot.clone());
        // Also tell `lower_index_set_fast` (and similar sites) that
        // `arr[counter_id]` is statically inbounds for this body, so
        // it can skip the runtime length-load + bound check.
        ctx.bounded_index_pairs.push((counter_id, arr_id));

        // If the counter is provably integer-valued (initialized from
        // an Integer literal, only mutated via Update ++/--), allocate
        // a parallel i32 slot. The Update lowering will keep it in sync,
        // and IndexGet/IndexSet will load the i32 directly instead of
        // emitting a `fptosi double → i32` on every iteration.
        if ctx.integer_locals.contains(&counter_id) {
            if let Some(counter_slot) = ctx.locals.get(&counter_id).cloned() {
                let i32_slot = ctx.func.alloca_entry(I32);
                // Initialize from the current double value.
                let cur_dbl = ctx.block().load(DOUBLE, &counter_slot);
                let cur_i32 = ctx.block().fptosi(DOUBLE, &cur_dbl, I32);
                ctx.block().store(I32, &cur_i32, &i32_slot);
                ctx.i32_counter_slots.insert(counter_id, i32_slot);
            }
        }

        Some(slot)
    } else {
        None
    };

    // If we have an i32 counter AND a hoisted length, pre-compute the
    // length as i32 so the loop condition can use `icmp slt i32` instead
    // of `fcmp olt double`. This eliminates the float counter fadd +
    // fcmp per iteration — saves ~2 instructions on the inner loop of
    // nested_loops and similar patterns.
    let i32_length_slot: Option<String> =
        if let Some((_, counter_id)) = hoist_classification {
            if let (Some(_), Some(len_dbl_slot)) =
                (ctx.i32_counter_slots.get(&counter_id).cloned(),
                 hoisted_length_slot.as_ref())
            {
                let len_dbl = ctx.block().load(DOUBLE, len_dbl_slot);
                let len_i32 = ctx.block().fptosi(DOUBLE, &len_dbl, I32);
                let slot = ctx.func.alloca_entry(I32);
                ctx.block().store(I32, &len_i32, &slot);
                Some(slot)
            } else {
                None
            }
        } else {
            None
        };

    // Issue #168: when the `i < arr.length` peephole didn't fire, also
    // detect the simpler `i < n` shape where `n` is a number-typed local
    // or function parameter. Emitting `fptosi(n)` once at the loop head
    // and using `icmp slt i32 %i, %n.i32` in the condition block
    // replaces `fcmp olt double`, letting LLVM's SCEV model `i` as a
    // clean integer induction variable — prerequisite for LoopVectorizer
    // to widen Buffer-read and similar intrinsic-heavy bodies.
    let local_bound_classification: Option<(u32, u32, perry_hir::CompareOp)> =
        if hoist_classification.is_none() {
            condition.and_then(|cond| classify_for_local_bound(cond, ctx))
        } else {
            None
        };
    // Track whether *we* allocated the counter's i32 slot (vs. the Let
    // site having done so already).  Only the site that inserted should
    // remove it at loop exit to avoid disturbing a pre-existing slot.
    let local_bound_counter_i32_was_fresh: bool;
    let i32_local_bound_slot: Option<String> =
        if let Some((counter_id, bound_id, _op)) = local_bound_classification {
            // Allocate a parallel i32 slot for the counter if not already
            // present.  The Let-site may have skipped it when the counter
            // wasn't in `index_used_locals`; providing it here enables both
            // `icmp slt i32` in the condition and `add i32 1` in Update.
            let fresh = if !ctx.i32_counter_slots.contains_key(&counter_id) {
                if let Some(counter_slot) = ctx.locals.get(&counter_id).cloned() {
                    let i32_slot = ctx.func.alloca_entry(I32);
                    let cur_dbl = ctx.block().load(DOUBLE, &counter_slot);
                    let cur_i32 = ctx.block().fptosi(DOUBLE, &cur_dbl, I32);
                    ctx.block().store(I32, &cur_i32, &i32_slot);
                    ctx.i32_counter_slots.insert(counter_id, i32_slot);
                    true
                } else {
                    false
                }
            } else {
                false
            };
            local_bound_counter_i32_was_fresh = fresh;
            // Hoist `fptosi(n)` to a fresh i32 alloca before the cond block
            // so LLVM sees a loop-invariant integer bound — critical for
            // SCEV / LoopVectorizer to recognize the induction variable.
            if let Some(bound_slot) = ctx.locals.get(&bound_id).cloned() {
                let bound_dbl = ctx.block().load(DOUBLE, &bound_slot);
                let bound_i32 = ctx.block().fptosi(DOUBLE, &bound_dbl, I32);
                let slot = ctx.func.alloca_entry(I32);
                ctx.block().store(I32, &bound_i32, &slot);
                Some(slot)
            } else {
                None
            }
        } else {
            local_bound_counter_i32_was_fresh = false;
            None
        };

    let cond_idx = ctx.new_block("for.cond");
    let body_idx = ctx.new_block("for.body");
    let update_idx = ctx.new_block("for.update");
    let exit_idx = ctx.new_block("for.exit");

    let cond_label = ctx.block_label(cond_idx);
    let body_label = ctx.block_label(body_idx);
    let update_label = ctx.block_label(update_idx);
    let exit_label = ctx.block_label(exit_idx);

    // Branch from the block holding the init into the cond block.
    ctx.block().br(&cond_label);

    // Cond block — fast i32 path when both counter and length are i32.
    ctx.current_block = cond_idx;
    let used_i32_cond = if let (Some((_, counter_id)), Some(ref len_i32_slot)) =
        (hoist_classification, &i32_length_slot)
    {
        // Existing path: `i < arr.length` with hoisted i32 length.
        if let Some(ctr_i32_slot) = ctx.i32_counter_slots.get(&counter_id).cloned() {
            let ctr = ctx.block().load(I32, &ctr_i32_slot);
            let len = ctx.block().load(I32, len_i32_slot);
            let cmp = ctx.block().icmp_slt(I32, &ctr, &len);
            ctx.block().cond_br(&cmp, &body_label, &exit_label);
            true
        } else {
            false
        }
    } else if let (Some((counter_id, _, op)), Some(ref bound_i32_slot)) =
        (local_bound_classification, &i32_local_bound_slot)
    {
        // Issue #168: `i < n` / `i <= n` where `n` is a number-typed local
        // or parameter.  The fptosi(n) was hoisted above; use icmp i32.
        if let Some(ctr_i32_slot) = ctx.i32_counter_slots.get(&counter_id).cloned() {
            let ctr = ctx.block().load(I32, &ctr_i32_slot);
            let bound = ctx.block().load(I32, bound_i32_slot);
            let cmp = match op {
                perry_hir::CompareOp::Le => ctx.block().icmp_sle(I32, &ctr, &bound),
                _ => ctx.block().icmp_slt(I32, &ctr, &bound),
            };
            ctx.block().cond_br(&cmp, &body_label, &exit_label);
            true
        } else {
            false
        }
    } else {
        false
    };
    if !used_i32_cond {
        if let Some(cond_expr) = condition {
            let cv = lower_expr(ctx, cond_expr)?;
            let i1 = lower_truthy(ctx, &cv, cond_expr);
            ctx.block().cond_br(&i1, &body_label, &exit_label);
        } else {
            // `for (;;)` — unconditional jump into the body. May be an
            // infinite loop unless the body contains a `break`.
            ctx.block().br(&body_label);
        }
    }

    // Push break/continue targets so nested `break`/`continue` know where
    // to jump. For for-loops, continue runs the update step.
    ctx.loop_targets.push((update_label.clone(), exit_label.clone()));

    // If this for-loop has a pending label (from an enclosing Stmt::Labeled),
    // register it so `break label;` / `continue label;` resolve here.
    let consumed_label = ctx.pending_label.take();
    if let Some(ref lbl) = consumed_label {
        ctx.label_targets.insert(lbl.clone(), (update_label.clone(), exit_label.clone()));
    }

    // Body block.
    ctx.current_block = body_idx;
    lower_stmts(ctx, body)?;
    // Issue #74: insert an empty `asm sideeffect` in bodies whose
    // statements are all LLVM-pure (local-only arithmetic, no calls,
    // no heap mutation). Without this, clang -O3's loop-deletion
    // pass folds patterns like `for (let i=0;i<N;i++) sum+=1;` to
    // `sum=N` and eliminates the loop entirely — so two `Date.now()`
    // calls bracketing the loop end up adjacent in the binary and
    // report 0ms wall-clock. The barrier emits zero machine
    // instructions but is opaque to IndVarSimplify.
    if !ctx.block().is_terminated() && body_needs_asm_barrier(body) {
        ctx.block().asm_sideeffect_barrier();
    }
    if !ctx.block().is_terminated() {
        ctx.block().br(&update_label);
    }

    // Update block.
    ctx.current_block = update_idx;
    if let Some(update_expr) = update {
        let _ = lower_expr(ctx, update_expr)?;
    }
    if !ctx.block().is_terminated() {
        ctx.block().br(&cond_label);
    }

    ctx.loop_targets.pop();

    // Pop the hoisted-length entry so nested loops or sibling loops
    // don't see a stale slot.
    if let Some((_, counter_id)) = hoist_classification {
        ctx.i32_counter_slots.remove(&counter_id);
    }
    if let Some(arr_id) = hoisted_length_arr_id {
        ctx.cached_lengths.remove(&arr_id);
        ctx.bounded_index_pairs.pop();
    }
    let _ = hoisted_length_slot;
    // Pop the i32 counter slot we inserted for the `i < n` number-bound
    // path, but only if *we* were the ones that inserted it (the Let site
    // may have already provided a slot, which should outlive the loop).
    if local_bound_counter_i32_was_fresh {
        if let Some((counter_id, _, _)) = local_bound_classification {
            ctx.i32_counter_slots.remove(&counter_id);
        }
    }
    let _ = i32_local_bound_slot;

    // Exit block — subsequent statements continue here.
    ctx.current_block = exit_idx;
    Ok(())
}

/// Inspect a `for` loop's condition expression and body, and return
/// `Some(arr_local_id)` if the loop is the well-known shape
/// `for (let i = ...; i < <arr>.length; ...) { body }` AND the body
/// is provably free of operations that can change `arr.length`.
///
/// The walker also accepts `arr[i] = expr` IndexSets where `i` is the
/// loop counter from the condition — those are guaranteed inbounds
/// (since `i < arr.length`) and therefore can't trigger the realloc
/// slow path that would extend `arr.length`. Without that exception
/// we'd reject the very common `for (let i = 0; i < arr.length; i++)
/// arr[i] = expr` shape, which is the canonical write-array pattern.
fn classify_for_length_hoist(
    cond: &perry_hir::Expr,
    body: &[perry_hir::Stmt],
) -> Option<(u32, u32)> {
    use perry_hir::{CompareOp, Expr};
    let (op, left, right) = match cond {
        Expr::Compare { op, left, right } => (op, left.as_ref(), right.as_ref()),
        _ => return None,
    };
    if !matches!(op, CompareOp::Lt | CompareOp::Le) {
        return None;
    }
    let arr_id = match right {
        Expr::PropertyGet { object, property } if property == "length" => {
            match object.as_ref() {
                Expr::LocalGet(id) => *id,
                _ => return None,
            }
        }
        _ => return None,
    };
    let bounded_idx_id = match left {
        Expr::LocalGet(id) => *id,
        _ => return None,
    };
    if !body
        .iter()
        .all(|s| stmt_preserves_array_length(s, arr_id, bounded_idx_id))
    {
        return None;
    }
    Some((arr_id, bounded_idx_id))
}

/// Inspect a `for` loop's condition and return `Some((counter_id, bound_id,
/// op))` if the condition is the shape `counter < bound` (or `<=`) where
/// both sides are `LocalGet` ids, the counter is in `integer_locals`, and
/// the bound is either (a) provably integer-valued (`integer_locals`) or
/// (b) a number-typed local / parameter whose slot is accessible directly
/// (i.e. not boxed and not a module global).
///
/// Case (b) relies on Perry's trust-types philosophy: a `number`-typed local
/// used as a for-loop bound is expected to hold a whole-number value at
/// runtime.  Callers that pass non-integer floats as loop bounds would
/// observe at most one iteration difference — a trade-off that is within
/// Perry's existing trust-types contract.
///
/// Used by `lower_for` to enable the same i32 counter specialization as
/// the `i < arr.length` peephole (`classify_for_length_hoist`) on the
/// common case where the loop bound comes from a function parameter or a
/// number-typed local variable.
fn classify_for_local_bound(
    cond: &perry_hir::Expr,
    ctx: &crate::expr::FnCtx<'_>,
) -> Option<(u32, u32, perry_hir::CompareOp)> {
    use perry_hir::{CompareOp, Expr};
    let (op, left, right) = match cond {
        Expr::Compare { op, left, right } => (*op, left.as_ref(), right.as_ref()),
        _ => return None,
    };
    if !matches!(op, CompareOp::Lt | CompareOp::Le) {
        return None;
    }
    let counter_id = match left {
        Expr::LocalGet(id) => *id,
        _ => return None,
    };
    let bound_id = match right {
        Expr::LocalGet(id) => *id,
        _ => return None,
    };
    // Counter must be provably integer-valued (initialized from integer
    // literal, only mutated by Update ++/--).
    if !ctx.integer_locals.contains(&counter_id) {
        return None;
    }
    // Bound is safe to fptosi when provably integer-valued, OR when it is a
    // number-typed slot that is accessible without boxing (params and simple
    // `let` locals).  Module globals and boxed (closure-captured) variables
    // go through different load paths so we skip those.
    let bound_is_integer_safe = ctx.integer_locals.contains(&bound_id)
        || (ctx.locals.contains_key(&bound_id)
            && !ctx.boxed_vars.contains(&bound_id)
            && !ctx.module_globals.contains_key(&bound_id)
            && matches!(
                ctx.local_types.get(&bound_id),
                Some(perry_types::Type::Number | perry_types::Type::Int32)
            ));
    if !bound_is_integer_safe {
        return None;
    }
    Some((counter_id, bound_id, op))
}

fn stmt_preserves_array_length(
    s: &perry_hir::Stmt,
    arr_id: u32,
    bounded_idx_id: u32,
) -> bool {
    use perry_hir::Stmt;
    match s {
        Stmt::Expr(e) | Stmt::Throw(e) => {
            expr_preserves_array_length(e, arr_id, bounded_idx_id)
        }
        Stmt::Return(opt) => opt
            .as_ref()
            .map_or(true, |e| expr_preserves_array_length(e, arr_id, bounded_idx_id)),
        Stmt::Let { init, .. } => init
            .as_ref()
            .map_or(true, |e| expr_preserves_array_length(e, arr_id, bounded_idx_id)),
        Stmt::If { condition, then_branch, else_branch } => {
            expr_preserves_array_length(condition, arr_id, bounded_idx_id)
                && then_branch
                    .iter()
                    .all(|s| stmt_preserves_array_length(s, arr_id, bounded_idx_id))
                && else_branch.as_ref().map_or(true, |b| {
                    b.iter()
                        .all(|s| stmt_preserves_array_length(s, arr_id, bounded_idx_id))
                })
        }
        Stmt::While { condition, body } | Stmt::DoWhile { body, condition } => {
            expr_preserves_array_length(condition, arr_id, bounded_idx_id)
                && body
                    .iter()
                    .all(|s| stmt_preserves_array_length(s, arr_id, bounded_idx_id))
        }
        Stmt::For { init, condition, update, body } => {
            init.as_ref()
                .map_or(true, |s| stmt_preserves_array_length(s, arr_id, bounded_idx_id))
                && condition.as_ref().map_or(true, |e| {
                    expr_preserves_array_length(e, arr_id, bounded_idx_id)
                })
                && update.as_ref().map_or(true, |e| {
                    expr_preserves_array_length(e, arr_id, bounded_idx_id)
                })
                && body
                    .iter()
                    .all(|s| stmt_preserves_array_length(s, arr_id, bounded_idx_id))
        }
        Stmt::Try { body, catch, finally } => {
            body.iter()
                .all(|s| stmt_preserves_array_length(s, arr_id, bounded_idx_id))
                && catch.as_ref().map_or(true, |c| {
                    c.body
                        .iter()
                        .all(|s| stmt_preserves_array_length(s, arr_id, bounded_idx_id))
                })
                && finally.as_ref().map_or(true, |b| {
                    b.iter()
                        .all(|s| stmt_preserves_array_length(s, arr_id, bounded_idx_id))
                })
        }
        Stmt::Switch { discriminant, cases } => {
            expr_preserves_array_length(discriminant, arr_id, bounded_idx_id)
                && cases.iter().all(|c| {
                    c.test.as_ref().map_or(true, |e| {
                        expr_preserves_array_length(e, arr_id, bounded_idx_id)
                    }) && c.body
                        .iter()
                        .all(|s| stmt_preserves_array_length(s, arr_id, bounded_idx_id))
                })
        }
        Stmt::Labeled { body, .. } => {
            stmt_preserves_array_length(body.as_ref(), arr_id, bounded_idx_id)
        }
        Stmt::Break
        | Stmt::Continue
        | Stmt::LabeledBreak(_)
        | Stmt::LabeledContinue(_) => true,
    }
}

fn expr_preserves_array_length(
    e: &perry_hir::Expr,
    arr_id: u32,
    bounded_idx_id: u32,
) -> bool {
    use perry_hir::{ArrayElement, CallArg, Expr};
    let walk = |sub: &Expr| expr_preserves_array_length(sub, arr_id, bounded_idx_id);
    match e {
        Expr::ArrayPush { array_id, value } => *array_id != arr_id && walk(value),
        Expr::ArrayPop(id) | Expr::ArrayShift(id) => *id != arr_id,
        Expr::ArraySplice {
            array_id,
            start,
            delete_count,
            items,
        } => {
            *array_id != arr_id
                && walk(start)
                && delete_count.as_ref().map_or(true, |e| walk(e))
                && items.iter().all(|e| walk(e))
        }
        Expr::IndexSet { object, index, value } => {
            // `arr[bounded_i] = expr` is the only IndexSet on `arr`
            // we accept — it's guaranteed inbounds because the loop
            // condition `i < arr.length` is invariant in this body,
            // and the IndexSet inbounds path doesn't extend the array.
            if let Expr::LocalGet(id) = object.as_ref() {
                if *id == arr_id {
                    if let Expr::LocalGet(idx_id) = index.as_ref() {
                        if *idx_id == bounded_idx_id {
                            return walk(value);
                        }
                    }
                    return false;
                }
            }
            walk(object) && walk(index) && walk(value)
        }
        // Reassigning the bounded index would invalidate the bound.
        // Reassigning the array variable would also invalidate (we'd
        // be tracking the wrong array).
        Expr::LocalSet(id, value) => {
            *id != arr_id && *id != bounded_idx_id && walk(value)
        }
        // `i++` / `i--` are fine — `i` stays integer-valued and the
        // for-loop's standard counter pattern depends on this. (We
        // don't try to verify the update direction; if `i--` ever
        // pushes `i` negative the IndexSet's runtime check still
        // catches it via the realloc fallback.)
        Expr::Update { id, .. } => *id != arr_id,
        Expr::NativeMethodCall { object, args, .. } => {
            if let Some(o) = object {
                if let Expr::LocalGet(id) = o.as_ref() {
                    if *id == arr_id {
                        return false;
                    }
                }
                if !walk(o) {
                    return false;
                }
            }
            args.iter().all(|a| walk(a))
        }
        Expr::Call { callee, args, .. } => {
            if !walk(callee) {
                return false;
            }
            for a in args {
                if let Expr::LocalGet(id) = a {
                    if *id == arr_id {
                        return false;
                    }
                }
                if !walk(a) {
                    return false;
                }
            }
            true
        }
        Expr::CallSpread { callee, args, .. } => {
            if !walk(callee) {
                return false;
            }
            for a in args {
                let inner = match a {
                    CallArg::Expr(e) | CallArg::Spread(e) => e,
                };
                if let Expr::LocalGet(id) = inner {
                    if *id == arr_id {
                        return false;
                    }
                }
                if !walk(inner) {
                    return false;
                }
            }
            true
        }
        Expr::Closure { .. } => false,
        Expr::Binary { left, right, .. }
        | Expr::Compare { left, right, .. }
        | Expr::Logical { left, right, .. } => walk(left) && walk(right),
        Expr::Unary { operand, .. }
        | Expr::Void(operand)
        | Expr::TypeOf(operand)
        | Expr::Await(operand)
        | Expr::Delete(operand)
        | Expr::StringCoerce(operand)
        | Expr::BooleanCoerce(operand)
        | Expr::NumberCoerce(operand) => walk(operand),
        Expr::Conditional {
            condition,
            then_expr,
            else_expr,
        } => walk(condition) && walk(then_expr) && walk(else_expr),
        Expr::PropertyGet { object, .. } => walk(object),
        Expr::PropertySet { object, value, .. } => walk(object) && walk(value),
        Expr::IndexGet { object, index } => walk(object) && walk(index),
        Expr::Array(elements) => elements.iter().all(|e| walk(e)),
        Expr::ArraySpread(elements) => elements.iter().all(|el| match el {
            ArrayElement::Expr(e) | ArrayElement::Spread(e) => walk(e),
        }),
        Expr::Object(fields) => fields.iter().all(|(_, v)| walk(v)),
        Expr::LocalGet(_)
        | Expr::GlobalGet(_)
        | Expr::Number(_)
        | Expr::Integer(_)
        | Expr::Bool(_)
        | Expr::Null
        | Expr::Undefined
        | Expr::String(_) => true,
        // Default: conservative reject for HIR variants we haven't
        // analyzed. Better to lose the optimization than to silently
        // hoist past a body that mutates the array.
        _ => false,
    }
}

/// `while (cond) { body }` — classic 3-block CFG (cond / body / exit).
///
/// ```text
///   <current>:
///     br cond
///   while.cond:
///     <condition>
///     truthy → body, falsey → exit
///   while.body:
///     <body>
///     br cond                 ; if not already terminated
///   while.exit:
///     <continues here>
/// ```
///
/// No break/continue support yet — body must fall through to the next
/// loop iteration. Same limitation as `for`.
fn lower_while(ctx: &mut FnCtx<'_>, condition: &perry_hir::Expr, body: &[Stmt]) -> Result<()> {
    let cond_idx = ctx.new_block("while.cond");
    let body_idx = ctx.new_block("while.body");
    let exit_idx = ctx.new_block("while.exit");

    let cond_label = ctx.block_label(cond_idx);
    let body_label = ctx.block_label(body_idx);
    let exit_label = ctx.block_label(exit_idx);

    ctx.block().br(&cond_label);

    ctx.current_block = cond_idx;
    let cv = lower_expr(ctx, condition)?;
    let i1 = lower_truthy(ctx, &cv, condition);
    ctx.block().cond_br(&i1, &body_label, &exit_label);

    // For while-loops, continue jumps back to the cond block.
    ctx.loop_targets.push((cond_label.clone(), exit_label.clone()));

    // Consume pending label (from enclosing Stmt::Labeled).
    let consumed_label = ctx.pending_label.take();
    if let Some(ref lbl) = consumed_label {
        ctx.label_targets.insert(lbl.clone(), (cond_label.clone(), exit_label.clone()));
    }

    ctx.current_block = body_idx;
    lower_stmts(ctx, body)?;
    // Issue #74: see lower_for for rationale.
    if !ctx.block().is_terminated() && body_needs_asm_barrier(body) {
        ctx.block().asm_sideeffect_barrier();
    }
    if !ctx.block().is_terminated() {
        ctx.block().br(&cond_label);
    }

    ctx.loop_targets.pop();

    ctx.current_block = exit_idx;
    Ok(())
}

/// `do { body } while (cond)` — body runs at least once. Same blocks as
/// `while`, but the initial branch goes to body, not cond.
fn lower_do_while(
    ctx: &mut FnCtx<'_>,
    body: &[Stmt],
    condition: &perry_hir::Expr,
) -> Result<()> {
    let body_idx = ctx.new_block("dowhile.body");
    let cond_idx = ctx.new_block("dowhile.cond");
    let exit_idx = ctx.new_block("dowhile.exit");

    let body_label = ctx.block_label(body_idx);
    let cond_label = ctx.block_label(cond_idx);
    let exit_label = ctx.block_label(exit_idx);

    ctx.block().br(&body_label);

    // Push break/continue targets BEFORE compiling the body so nested
    // break/continue see them.
    ctx.loop_targets.push((cond_label.clone(), exit_label.clone()));

    // Consume pending label (from enclosing Stmt::Labeled).
    let consumed_label = ctx.pending_label.take();
    if let Some(ref lbl) = consumed_label {
        ctx.label_targets.insert(lbl.clone(), (cond_label.clone(), exit_label.clone()));
    }

    ctx.current_block = body_idx;
    lower_stmts(ctx, body)?;
    // Issue #74: see lower_for for rationale.
    if !ctx.block().is_terminated() && body_needs_asm_barrier(body) {
        ctx.block().asm_sideeffect_barrier();
    }
    if !ctx.block().is_terminated() {
        ctx.block().br(&cond_label);
    }

    ctx.current_block = cond_idx;
    let cv = lower_expr(ctx, condition)?;
    let i1 = lower_truthy(ctx, &cv, condition);
    ctx.block().cond_br(&i1, &body_label, &exit_label);

    ctx.loop_targets.pop();

    ctx.current_block = exit_idx;
    Ok(())
}

/// `switch (disc) { case A: ...; break; case B: ...; default: ... }`
/// lowering. Each case gets a (test, body) block pair; bodies fall
/// through to the next body block (not the next test) to honor JS
/// fall-through. The default body is positioned wherever the default
/// case appears in source order. `break` inside a case branches to
/// the exit block via the `loop_targets` mechanism.
///
/// We don't use LLVM's `switch` instruction because the discriminant
/// is a NaN-boxed double whose equality semantics differ from i32
/// switch (NaN != NaN). The if-tower lowering uses fcmp oeq for each
/// test which yields the right semantics.
fn lower_switch(
    ctx: &mut FnCtx<'_>,
    discriminant: &perry_hir::Expr,
    cases: &[perry_hir::SwitchCase],
) -> Result<()> {
    let dv = lower_expr(ctx, discriminant)?;

    // Allocate test/body blocks for every case up front so we can wire
    // up the fall-through edges before each block is filled in.
    let mut test_blocks: Vec<usize> = Vec::with_capacity(cases.len());
    let mut body_blocks: Vec<usize> = Vec::with_capacity(cases.len());
    for (i, case) in cases.iter().enumerate() {
        let test_name = if case.test.is_some() {
            format!("switch.test{}", i)
        } else {
            format!("switch.default_test{}", i)
        };
        test_blocks.push(ctx.new_block(&test_name));
        body_blocks.push(ctx.new_block(&format!("switch.body{}", i)));
    }
    let exit_idx = ctx.new_block("switch.exit");
    let exit_label = ctx.block_label(exit_idx);

    // Branch from the discriminant block into the first test (or
    // straight into the body if there are zero cases — degenerate but
    // legal).
    if let Some(&first_test) = test_blocks.first() {
        let first_test_label = ctx.block_label(first_test);
        ctx.block().br(&first_test_label);
    } else {
        ctx.block().br(&exit_label);
        ctx.current_block = exit_idx;
        return Ok(());
    }

    // Find the default case index, if any. The "fall-through to default
    // when nothing matches" target is the default's body block; if
    // there's no default, we fall through to exit.
    let default_idx = cases.iter().position(|c| c.test.is_none());
    let no_match_target_label = match default_idx {
        Some(i) => ctx.block_label(body_blocks[i]),
        None => exit_label.clone(),
    };

    // Push break target. Switch has no continue, so we use exit for both.
    ctx.loop_targets.push((exit_label.clone(), exit_label.clone()));

    // Compile each test block. Each test compares dv against the case
    // expression with fcmp oeq, jumps to the body on match, otherwise
    // jumps to the next test (or to no_match_target if this is the last).
    for (i, case) in cases.iter().enumerate() {
        ctx.current_block = test_blocks[i];
        let body_label = ctx.block_label(body_blocks[i]);
        let next_label = if i + 1 < test_blocks.len() {
            ctx.block_label(test_blocks[i + 1])
        } else {
            no_match_target_label.clone()
        };

        if let Some(test_expr) = case.test.as_ref() {
            let cv = lower_expr(ctx, test_expr)?;
            // If either the discriminant or the case value is a static
            // string expression (e.g. `switch (typeof x) { case "foo": }`),
            // compare by string content via js_string_equals. Two allocations
            // of the same text have different pointers, so icmp on bits
            // would report them unequal. Dispatch through the unified
            // string-pointer getter which returns null for non-strings —
            // js_string_equals treats null as "not equal", matching the
            // expected fall-through behavior.
            let either_string = crate::type_analysis::is_string_expr(ctx, discriminant)
                || crate::type_analysis::is_string_expr(ctx, test_expr);
            if either_string {
                let blk = ctx.block();
                let l_handle = blk.call(
                    crate::types::I64,
                    "js_get_string_pointer_unified",
                    &[(crate::types::DOUBLE, &dv)],
                );
                let r_handle = blk.call(
                    crate::types::I64,
                    "js_get_string_pointer_unified",
                    &[(crate::types::DOUBLE, &cv)],
                );
                let i32_eq = blk.call(
                    crate::types::I32,
                    "js_string_equals",
                    &[(crate::types::I64, &l_handle), (crate::types::I64, &r_handle)],
                );
                let cmp = blk.icmp_ne(crate::types::I32, &i32_eq, "0");
                blk.cond_br(&cmp, &body_label, &next_label);
            } else {
                // fcmp on NaN-tagged string/pointer values is always
                // false (NaN comparisons are unordered). For switch on
                // strings or any value that might be NaN-tagged, compare
                // the i64 bit patterns instead. This works for numbers
                // too — equal doubles have equal bits except for ±0
                // which the JS spec treats as equal anyway and Number(0)
                // === Number(-0) is true.
                let blk = ctx.block();
                let dv_bits = blk.bitcast_double_to_i64(&dv);
                let cv_bits = blk.bitcast_double_to_i64(&cv);
                let cmp = blk.icmp_eq(crate::types::I64, &dv_bits, &cv_bits);
                blk.cond_br(&cmp, &body_label, &next_label);
            }
        } else {
            // Default case test block: unconditional jump to its body.
            ctx.block().br(&body_label);
        }
    }

    // Compile each body block. Bodies fall through to the next body
    // (NOT the next test) unless terminated by `break`/`return`/etc.
    for (i, case) in cases.iter().enumerate() {
        ctx.current_block = body_blocks[i];
        lower_stmts(ctx, &case.body)?;
        if !ctx.block().is_terminated() {
            let next_body_label = if i + 1 < body_blocks.len() {
                ctx.block_label(body_blocks[i + 1])
            } else {
                exit_label.clone()
            };
            ctx.block().br(&next_body_label);
        }
    }

    ctx.loop_targets.pop();
    ctx.current_block = exit_idx;
    Ok(())
}

/// If-else lowering using explicit then/else/merge blocks.
///
/// Truthiness uses `lower_truthy` which dispatches to either an inline
/// `fcmp one cond, 0.0` (statically-numeric conditions) or a runtime
/// `js_is_truthy` call (NaN-boxed booleans, strings, objects, unions).
/// Try to evaluate a condition at compile time using known constants.
/// Returns `Some(true)` or `Some(false)` if the condition can be folded,
/// `None` if it depends on runtime values.
fn try_const_fold_condition(
    ctx: &FnCtx<'_>,
    condition: &perry_hir::Expr,
) -> Option<bool> {
    use perry_hir::{Expr, CompareOp, LogicalOp};
    match condition {
        Expr::Compare { op, left, right } => {
            // Try to extract a known constant from one side and a literal
            // from the other.
            let (const_val, literal_val) = match (left.as_ref(), right.as_ref()) {
                (Expr::LocalGet(id), Expr::Integer(n)) => {
                    (ctx.compile_time_constants.get(id)?, *n as f64)
                }
                (Expr::Integer(n), Expr::LocalGet(id)) => {
                    (ctx.compile_time_constants.get(id)?, *n as f64)
                }
                (Expr::LocalGet(id), Expr::Number(n)) => {
                    (ctx.compile_time_constants.get(id)?, *n)
                }
                (Expr::Number(n), Expr::LocalGet(id)) => {
                    (ctx.compile_time_constants.get(id)?, *n)
                }
                _ => return None,
            };
            let c = *const_val;
            Some(match op {
                CompareOp::Eq | CompareOp::LooseEq => c == literal_val,
                CompareOp::Ne | CompareOp::LooseNe => c != literal_val,
                CompareOp::Lt => c < literal_val,
                CompareOp::Le => c <= literal_val,
                CompareOp::Gt => c > literal_val,
                CompareOp::Ge => c >= literal_val,
            })
        }
        Expr::Logical { op, left, right } => {
            let l = try_const_fold_condition(ctx, left)?;
            match op {
                LogicalOp::And => {
                    if !l { Some(false) } else { try_const_fold_condition(ctx, right) }
                }
                LogicalOp::Or => {
                    if l { Some(true) } else { try_const_fold_condition(ctx, right) }
                }
                LogicalOp::Coalesce => None,
            }
        }
        _ => None,
    }
}

fn lower_if(
    ctx: &mut FnCtx<'_>,
    condition: &perry_hir::Expr,
    then_branch: &[Stmt],
    else_branch: Option<&[Stmt]>,
) -> Result<()> {
    // Compile-time constant folding: when the condition involves only
    // known constants (e.g., `__platform__ === 1`), skip the dead branch
    // entirely. This prevents emitting `declare`/`call` instructions for
    // extern FFI functions that only exist on other platforms.
    if let Some(is_true) = try_const_fold_condition(ctx, condition) {
        if is_true {
            lower_stmts(ctx, then_branch)?;
        } else if let Some(else_stmts) = else_branch {
            lower_stmts(ctx, else_stmts)?;
        }
        return Ok(());
    }

    let cond_val = lower_expr(ctx, condition)?;
    let i1 = lower_truthy(ctx, &cond_val, condition);

    let then_idx = ctx.new_block("if.then");
    let else_idx = ctx.new_block("if.else");
    let merge_idx = ctx.new_block("if.merge");

    let then_label = ctx.block_label(then_idx);
    let else_label = ctx.block_label(else_idx);
    let merge_label = ctx.block_label(merge_idx);

    // Emit the branch in the incoming current block.
    ctx.block().cond_br(&i1, &then_label, &else_label);

    // Compile then branch.
    ctx.current_block = then_idx;
    lower_stmts(ctx, then_branch)?;
    if !ctx.block().is_terminated() {
        ctx.block().br(&merge_label);
    }

    // Compile else branch. If there's no explicit else, the else block is
    // still created so both sides of the condBr have a valid target — it
    // just branches immediately to merge.
    ctx.current_block = else_idx;
    if let Some(else_stmts) = else_branch {
        lower_stmts(ctx, else_stmts)?;
    }
    if !ctx.block().is_terminated() {
        ctx.block().br(&merge_label);
    }

    // Continue emitting subsequent statements into the merge block.
    ctx.current_block = merge_idx;
    Ok(())
}

fn stmt_variant_name(s: &Stmt) -> &'static str {
    match s {
        Stmt::Expr(_) => "Expr",
        Stmt::Let { .. } => "Let",
        Stmt::Return(_) => "Return",
        Stmt::If { .. } => "If",
        Stmt::While { .. } => "While",
        Stmt::DoWhile { .. } => "DoWhile",
        Stmt::For { .. } => "For",
        Stmt::Labeled { .. } => "Labeled",
        Stmt::Break => "Break",
        Stmt::Continue => "Continue",
        Stmt::LabeledBreak(_) => "LabeledBreak",
        Stmt::LabeledContinue(_) => "LabeledContinue",
        Stmt::Throw(_) => "Throw",
        Stmt::Try { .. } => "Try",
        Stmt::Switch { .. } => "Switch",
    }
}

// Silence the unused-import lint if lower_expr is not directly used here
// (it is used via the `use` above, but rustc's dead-code checker can be
// strict about helpers that only get called transitively).
#[allow(dead_code)]
fn _keep_anyhow_in_scope() -> anyhow::Error {
    anyhow!("")
}

/// Extract all field names (parent chain + own) and the constructor for
/// a class, cloning everything out of `ctx.classes` so the immutable
/// borrow is released before the caller mutates `ctx`.
///
/// Returns `None` if the class is not found in `ctx.classes`.
fn collect_scalar_class_data(
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
