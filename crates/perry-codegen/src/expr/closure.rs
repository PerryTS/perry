//! Closure expressions.
//!
//! Extracted from `expr/mod.rs` to keep that file under the 2000-line cap.
//! Pure mechanical move — match arm bodies are verbatim copies, called from
//! `lower_expr`'s outer dispatch.

use anyhow::{anyhow, Result};
use perry_hir::Expr;

use crate::type_analysis::compute_auto_captures;
use crate::types::{DOUBLE, I32, I64, PTR};

use super::{lower_expr, nanbox_pointer_inline, FnCtx};

pub(crate) fn lower(ctx: &mut FnCtx<'_>, expr: &Expr) -> Result<String> {
    match expr {
        Expr::Closure {
            func_id,
            params,
            body,
            captures,
            mutable_captures,
            captures_this,
            captures_new_target,
            is_async,
            is_arrow,
            ..
        } => {
            // captures_this used to be a hard error here. Phase H.3
            // initializes the closure's `this_stack` with a sentinel
            // when enclosing_class is set, so the body lowering won't
            // crash on `this` references — they just produce garbage
            // until full this-capture support lands. The wrong-but-
            // doesn't-crash trade unblocks dozens of test files.
            //
            // Async-closure handling (post #1021 phase 2): async closures
            // whose body contains an `await` are pre-rewritten upstream
            // by `transform_async_to_generator` (via
            // `transform_plain_async_closure_body` in
            // `perry-transform/src/generator.rs`). By the time codegen sees
            // them, the rewrite has flipped `is_async` to false and the
            // body is a state machine returning a Promise. What still
            // arrives here with `is_async: true` is async closures
            // *without* awaits — for those the body just runs once and
            // returns its value, and the caller's `await` (if any) wraps
            // it in `Promise.resolve(value)` semantics via the surrounding
            // codegen. No state-machine wrapping needed here.
            let _ = is_async;
            // mutable_captures uses the same get/set runtime path —
            // they work as long as the outer scope doesn't also access
            // the captured variable after the closure is created.
            let _ = mutable_captures;

            // Auto-detect captures from the body. The HIR's captures
            // list is sometimes empty for closures passed as arguments
            // (the closure conversion pass doesn't visit every site).
            // We must detect the same set as `compile_closure` so the
            // creation site and the body lower with consistent slot
            // indices.
            let auto_captures = compute_auto_captures(ctx, params, body, captures);
            for cap_id in &auto_captures {
                super::downgrade_buffer_alias(
                    ctx,
                    *cap_id,
                    crate::native_value::MaterializationReason::ClosureCapture,
                );
            }

            // Lower each captured value from the OUTER scope (this is
            // an outer-scope access, NOT a closure capture access — at
            // closure creation we're still outside the closure body).
            //
            // Boxed captures are special: the CAPTURE VALUE is the
            // box pointer itself (not the value inside the box). We
            // store the box pointer bits in the closure's capture slot,
            // so reads/writes inside the
            // closure body can deref it via js_box_get/set. Without
            // this, each closure would get a snapshot of the box's
            // current value.
            let mut captured_value_bits: Vec<String> = Vec::with_capacity(auto_captures.len());
            for cap_id in &auto_captures {
                if ctx.boxed_vars.contains(cap_id) {
                    // If the enclosing function has this id boxed,
                    // we want to forward the BOX POINTER through
                    // the capture slot as raw bits, not the value inside
                    // the box. Read the slot directly without
                    // going through the normal LocalGet path (which
                    // would deref via js_box_get).
                    if let Some(&_capture_idx) = ctx.closure_captures.get(cap_id) {
                        // We're inside a closure and this id is a
                        // transitively-captured box. Read the
                        // capture slot RAW (it holds the box ptr
                        // bits) and propagate directly.
                        let closure_ptr = ctx.current_closure_ptr.clone().ok_or_else(|| {
                            anyhow!("nested boxed capture but no current_closure_ptr")
                        })?;
                        let idx_str = _capture_idx.to_string();
                        let v = ctx.block().call(
                            I64,
                            "js_closure_get_capture_bits",
                            &[(I64, &closure_ptr), (I32, &idx_str)],
                        );
                        captured_value_bits.push(v);
                    } else if let Some(slot) = ctx.locals.get(cap_id).cloned() {
                        // Enclosing function owns the box: slot holds
                        // the raw box pointer as i64.
                        let box_ptr = ctx.block().load(I64, &slot);
                        captured_value_bits.push(box_ptr);
                    } else if let Some(global_name) = ctx.module_globals.get(cap_id).cloned() {
                        // Global boxed var (rare).
                        let g_ref = format!("@{}", global_name);
                        let v = ctx.block().load(DOUBLE, &g_ref);
                        let v_bits = ctx.block().bitcast_double_to_i64(&v);
                        captured_value_bits.push(v_bits);
                    } else {
                        captured_value_bits.push("0".to_string());
                    }
                } else {
                    let v = lower_expr(ctx, &Expr::LocalGet(*cap_id))?;
                    let v_bits = ctx.block().bitcast_double_to_i64(&v);
                    captured_value_bits.push(v_bits);
                }
            }

            // Compute the closure function name BEFORE taking the
            // mutable block borrow.
            let func_name = format!("perry_closure_{}__{}", ctx.strings.module_prefix(), func_id);

            // Closures may reserve extra lexical slots after ordinary
            // captures. Keep `this` last because the runtime's
            // CAPTURES_THIS_FLAG helpers rebind/unbind the last slot.
            let new_target_capture_idx = auto_captures.len();
            let this_capture_idx = auto_captures.len() + usize::from(*captures_new_target);

            // Closures with `captures_this` reserve one extra capture
            // slot for the receiver.
            // `lower_object_literal` patches that slot with the
            // containing object pointer AFTER the closure is built.
            // Arrow-in-class closures leave it at 0.0, the existing
            // non-crashing fallback.
            let total_caps = auto_captures.len()
                + usize::from(*captures_new_target)
                + usize::from(*captures_this);

            let func_ref = format!("@{}", func_name);
            // Issue #450: when `captures_this`, OR in the runtime's
            // `CAPTURES_THIS_FLAG` (0x8000_0000) so the runtime can detect
            // closures whose last capture slot is the reserved `this` slot.
            // `js_closure_alloc` masks the flag off when computing allocation
            // size (real_capture_count) but preserves it in the stored
            // `capture_count` field. Used by `clone_closure_rebind_this` at
            // `Object.defineProperty(obj, k, { get(){}, set(){} })` time so
            // accessor invocation sees `this === obj` per spec, and by
            // `js_closure_unbind_this` for detached method references.
            let cap_count_val = if *captures_this {
                (total_caps as u32) | 0x8000_0000u32
            } else {
                total_caps as u32
            };
            let cap_count = cap_count_val.to_string();
            // Closures with NO captures (and no `this` to patch) are
            // observationally identical across every call site that
            // produces them, so route through `js_closure_alloc_singleton`
            // to share a single ClosureHeader cached by func_ptr.
            //
            // Closures WITH captures route through
            // `js_closure_alloc_with_captures_singleton` whenever none of
            // those captures are mutated by the body (the common case
            // for ECS callbacks like `(eid, arch, compId) => { ...
            // changeset ... }` capturing `this._changeset`). The cache
            // keys on (func_ptr, capture_bits…) so distinct capture
            // values still produce distinct closures; identical
            // (func, captures) at a hot call site re-uses the cached
            // ClosureHeader and skips gc_malloc + gc_check_trigger.
            //
            // We skip the captured-singleton path for closures whose
            // body mutates an unboxed capture: those want fresh
            // per-call identity because the captured slot itself holds
            // mutable state for that invocation.
            //
            // Closures that capture `this` are still routable through
            // the captured-singleton path — we include the `this` value
            // in the cache buffer (at slot `auto_captures.len()`,
            // matching the runtime layout), so distinct receivers
            // produce distinct cache keys. Hot ECS class methods like
            // `World.executeEntityCommands` benefit from this: their
            // inner arrow `(eid, arch, compId) => ... changeset ...`
            // is created per-call but always with the same `this` (the
            // World) and same captures (`this._changeset`).
            // Boxed captures still allow the cache path: the closure
            // stores the BOX POINTER (a stable per-allocation address),
            // and the box's contents are read dynamically inside the
            // body via `js_box_get`. Two closure-literal sites that
            // capture the same boxed local store identical box-pointer
            // bits, so the cache (keyed on bit-equality of capture
            // slots) still hits. The cache backing is a small LRU per
            // func_ptr, which tolerates the parallel-instance pattern
            // (50 concurrent unitOfWork calls each capturing a
            // different `__async_step` box) by holding multiple
            // captures rather than overwriting one slot per call.
            //
            // We previously bailed out when any captured local was
            // boxed (`mutable_captures` non-empty). That made the
            // async-to-generator transform's per-`await` `cb_v` /
            // `cb_e` closures (which capture the boxed `__async_step`
            // self-reference) miss the cache 100% of the time —
            // 2 fresh closure allocs per await ≈ 300 ns of `gc_malloc`
            // work even though the box pointers are stable across
            // call sites. The relaxed gate plus the multi-slot LRU
            // backing reclaims that overhead.
            //
            // IDENTITY CAVEAT (#4831 follow-up — Stripe `protoExtend`):
            // the singleton-sharing paths (`js_closure_alloc_singleton` /
            // `js_closure_alloc_with_captures_singleton`) return ONE cached
            // `ClosureHeader` for a given (func_ptr[, capture-bits]) key, so two
            // evaluations of the same closure literal observe the SAME function
            // object — and therefore the SAME `.prototype`. A non-arrow
            // `function` expression that is used as a CONSTRUCTOR must instead
            // follow JS identity semantics: every evaluation yields a fresh
            // function object with its own fresh `.prototype`. Stripe builds
            // each resource via
            //   `const Constructor = function (...a) { Super.apply(this, a); };
            //    Constructor.prototype = Object.create(Super.prototype);
            //    Object.assign(Constructor.prototype, sub); return Constructor;`
            // `Constructor` captures only the constant `Super`, so the
            // (func_ptr, Super-bits) cache key was identical for every resource
            // and they all shared ONE `Constructor`/`prototype`. Each
            // `Object.assign(Constructor.prototype, methods)` then clobbered the
            // previous resource's methods, so every `stripe.<resource>.<method>`
            // resolved to the last-registered resource (e.g. `products.create`
            // ran webhook_endpoints' method) — the `replace is not a function`
            // symptom.
            //
            // To preserve the hot-path optimizations while restoring identity,
            // a closure is singleton-eligible only when sharing one instance is
            // observationally safe:
            //   - arrow functions: no own `.prototype`, not constructable, the
            //     `.map`/ECS callbacks the cache targets; OR
            //   - non-arrow closures all of whose captures are BOXED (mutable)
            //     locals: the compiler-synthesized async-step `cb_v`/`cb_e`
            //     per-await callbacks capture the boxed `__async_step` self-ref
            //     and are never used as constructors — keeping them cached
            //     avoids 2 `gc_malloc`s per `await`.
            // A non-arrow closure capturing an UNBOXED value (Stripe's `Super`,
            // or no captures at all) is treated as a potential constructor and
            // always gets a fresh instance.
            let mut write_ids = std::collections::HashSet::new();
            crate::boxed_vars::collect_write_ids_in_stmts(body, &mut write_ids);
            let writes_unboxed_capture = auto_captures
                .iter()
                .any(|cap_id| !ctx.boxed_vars.contains(cap_id) && write_ids.contains(cap_id));
            // All captures boxed (and at least one), with no reserved `this` /
            // `new.target` slot: the compiler-synthesized async-callback shape.
            let captures_all_boxed = !*captures_this
                && !*captures_new_target
                && !auto_captures.is_empty()
                && auto_captures
                    .iter()
                    .all(|cap_id| ctx.boxed_vars.contains(cap_id));
            let singleton_identity_safe = *is_arrow || captures_all_boxed;
            let no_capture_singleton = *is_arrow && total_caps == 0;
            let captured_singleton =
                singleton_identity_safe && !no_capture_singleton && !writes_unboxed_capture;

            let new_target_value_for_cache = if captured_singleton && *captures_new_target {
                Some(if let Some(slot) = ctx.new_target_stack.last().cloned() {
                    ctx.block().load(DOUBLE, &slot)
                } else {
                    ctx.block().call(DOUBLE, "js_new_target_get", &[])
                })
            } else {
                None
            };

            // For captures_this, the cache buffer needs an extra slot for
            // the `this` value so the cache key distinguishes closures with
            // different receivers. We load `this` here (mirroring the
            // post-create patch site below) when we're taking the
            // captured-singleton path.
            let this_value_for_cache = if captured_singleton && *captures_this {
                let this_slot = ctx.this_stack.last().cloned();
                Some(if let Some(slot) = this_slot {
                    ctx.block().load(DOUBLE, &slot)
                } else {
                    // Issue #1845: empty `this_stack` => dynamically-bound
                    // `this` (computed-key method / function-expression
                    // receiver). Capture the runtime's `IMPLICIT_THIS`, not
                    // a 0.0 sentinel. See the matching comment on the
                    // post-create patch path below.
                    ctx.block().call(DOUBLE, "js_implicit_this_get", &[])
                })
            } else {
                None
            };

            let closure_handle = if no_capture_singleton {
                let blk = ctx.block();
                blk.call(I64, "js_closure_alloc_singleton", &[(PTR, &func_ref)])
            } else if captured_singleton {
                // Stack-allocate a `[u64; total_caps]` capture buffer
                // (auto captures, plus `this` at the reserved slot if
                // captures_this). The runtime helper copies these
                // verbatim into the cached closure's capture slots.
                let n_total = total_caps;
                let buf = ctx.func.alloca_entry_array(I64, n_total);
                {
                    let blk = ctx.block();
                    for (i, v_bits) in captured_value_bits.iter().enumerate() {
                        let slot = blk.gep(I64, &buf, &[(I64, &format!("{}", i))]);
                        blk.store(I64, v_bits, &slot);
                    }
                    if let Some(new_target_v) = &new_target_value_for_cache {
                        let slot =
                            blk.gep(I64, &buf, &[(I64, &format!("{}", new_target_capture_idx))]);
                        let v_bits = blk.bitcast_double_to_i64(new_target_v);
                        blk.store(I64, &v_bits, &slot);
                    }
                    if let Some(this_v) = &this_value_for_cache {
                        let slot = blk.gep(I64, &buf, &[(I64, &format!("{}", this_capture_idx))]);
                        let v_bits = blk.bitcast_double_to_i64(this_v);
                        blk.store(I64, &v_bits, &slot);
                    }
                }
                let blk = ctx.block();
                blk.call(
                    I64,
                    "js_closure_alloc_with_captures_singleton",
                    &[(PTR, &func_ref), (I32, &cap_count), (PTR, &buf)],
                )
            } else {
                let blk = ctx.block();
                blk.call(
                    I64,
                    "js_closure_alloc",
                    &[(PTR, &func_ref), (I32, &cap_count)],
                )
            };

            // Register an `async function(){}` *expression* closure (one with
            // no `await` — bodies that await are rewritten to a state machine
            // upstream and arrive with `is_async: false`) in the async-function
            // registry so `IsConstructor`/`util.types.isAsyncFunction` recognize
            // it. Generators are hoisted to top-level and registered elsewhere.
            // (Test262 subclass/superclass-async-function.)
            if *is_async {
                ctx.block()
                    .call_void("js_register_closure_async_function", &[(PTR, &func_ref)]);
            }

            // The captured-singleton helper writes captures internally
            // (so the cached layout matches a fresh allocation). The
            // other paths still need explicit per-slot writes.
            if !captured_singleton {
                let blk = ctx.block();
                for (idx, val_bits) in captured_value_bits.iter().enumerate() {
                    let idx_str = idx.to_string();
                    blk.call_void(
                        "js_closure_set_capture_bits",
                        &[(I64, &closure_handle), (I32, &idx_str), (I64, val_bits)],
                    );
                }
            }
            // Issue #291: when the closure is built inside a method
            // body (or constructor), the enclosing frame's `this` is the
            // topmost entry on `this_stack`; load and write that into
            // the reserved capture slot. Without this, the closure's
            // `Expr::This` reads back 0.0 and any `this.field` access in
            // the body crashes.
            //
            // Issue #1845: when `this_stack` is empty the enclosing
            // function still has a dynamically-bound `this` — this is the
            // case for a computed-key method (`[expr](){…}`) which lowers
            // to a function-expression closure field (see
            // `lower_computed_key_method_as_field`): its body reads `this`
            // via the runtime's `IMPLICIT_THIS` (set by `recv[k]()`
            // dispatch), NOT via `this_stack`. A direct `Expr::This` in
            // such a body already falls back to `js_implicit_this_get`
            // (see `this_super_call.rs`); a *nested* arrow that captures
            // `this` must capture that same dynamic receiver, otherwise it
            // snapshots a bogus 0.0 sentinel. effect's fiber-runtime
            // op-dispatch hit this: `[OP_WITH_RUNTIME](op){ internalCall(()
            // => op.i0(this, …)) }` passed `this = 0.0` (a raw number) into
            // the WithRuntime handler, so `fiber.currentContext` read
            // `undefined` and `Effect.all`/`Effect.forEach` died with a
            // `{}` FiberFailure. Falling back to `js_implicit_this_get`
            // here matches the direct-read path exactly: top-level arrows
            // that legitimately have no `this` see `undefined` (the
            // `IMPLICIT_THIS` default), not 0.0 — both are non-crashing.
            if *captures_this {
                let this_idx = this_capture_idx.to_string();
                let this_slot = ctx.this_stack.last().cloned();
                let this_value = if let Some(slot) = this_slot {
                    ctx.block().load(DOUBLE, &slot)
                } else {
                    ctx.block().call(DOUBLE, "js_implicit_this_get", &[])
                };
                let blk = ctx.block();
                let this_bits = blk.bitcast_double_to_i64(&this_value);
                blk.call_void(
                    "js_closure_set_capture_bits",
                    &[(I64, &closure_handle), (I32, &this_idx), (I64, &this_bits)],
                );
            }
            if *captures_new_target {
                let new_target_idx = new_target_capture_idx.to_string();
                let new_target_value = if let Some(slot) = ctx.new_target_stack.last().cloned() {
                    ctx.block().load(DOUBLE, &slot)
                } else {
                    ctx.block().call(DOUBLE, "js_new_target_get", &[])
                };
                let blk = ctx.block();
                let new_target_bits = blk.bitcast_double_to_i64(&new_target_value);
                blk.call_void(
                    "js_closure_set_capture_bits",
                    &[
                        (I64, &closure_handle),
                        (I32, &new_target_idx),
                        (I64, &new_target_bits),
                    ],
                );
            }
            Ok(nanbox_pointer_inline(ctx.block(), &closure_handle))
        }

        // -------- Classes (Phase C.1) --------
        // `new ClassName(args...)` — allocate an anonymous object,
        // inline-execute the constructor body with `this` bound to the
        // new object, return the NaN-boxed object. No method tables yet,
        // no inheritance — just data classes with constructor field
        // assignments.
        _ => unreachable!("expr/mod.rs dispatched a variant not handled by this submodule"),
    }
}
