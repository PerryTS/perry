//! Closure expressions.
//!
//! Extracted from `expr/mod.rs` to keep that file under the 2000-line cap.
//! Pure mechanical move — match arm bodies are verbatim copies, called from
//! `lower_expr`'s outer dispatch.

use anyhow::Result;
use perry_hir::{Expr, Stmt};

use crate::type_analysis::compute_auto_captures;
use crate::types::{DOUBLE, I32, I64, PTR};

use super::{lower_expr, nanbox_pointer_inline, FnCtx};

/// The activation cells of the compiler-private step closure for a lowered
/// plain async activation: every id its terminal `ReleaseBoxes` arms name, or
/// `None` for any other closure. `ReleaseBoxes` is emitted only in that
/// closure's terminal arms; user-authored closures can never contain it.
///
/// Queued and running instances of this closure are already covered by the
/// activation token's refcount. Counting those boxed capture slots as escaping
/// GC-closure edges would make every cell in the complete activation frame
/// wait for a full collection, even when no user closure can observe it. A
/// cell from an ENCLOSING scope is not covered by that token (#10464: its
/// owner frame now releases it at scope exit), so only these ids go uncounted.
pub(crate) fn plain_async_step_release_ids(
    stmts: &[Stmt],
) -> Option<std::collections::HashSet<u32>> {
    fn walk(stmts: &[Stmt], out: &mut Option<std::collections::HashSet<u32>>) {
        for stmt in stmts {
            match stmt {
                Stmt::ReleaseBoxes(ids) => out
                    .get_or_insert_with(Default::default)
                    .extend(ids.iter().copied()),
                Stmt::If {
                    then_branch,
                    else_branch,
                    ..
                } => {
                    walk(then_branch, out);
                    if let Some(else_branch) = else_branch {
                        walk(else_branch, out);
                    }
                }
                Stmt::While { body, .. } | Stmt::DoWhile { body, .. } => walk(body, out),
                Stmt::For { init, body, .. } => {
                    if let Some(init) = init {
                        walk(std::slice::from_ref(init.as_ref()), out);
                    }
                    walk(body, out);
                }
                Stmt::Try {
                    body,
                    catch,
                    finally,
                } => {
                    walk(body, out);
                    if let Some(catch) = catch {
                        walk(&catch.body, out);
                    }
                    if let Some(finally) = finally {
                        walk(finally, out);
                    }
                }
                Stmt::Switch { cases, .. } => {
                    for case in cases {
                        walk(&case.body, out);
                    }
                }
                Stmt::Labeled { body, .. } => walk(std::slice::from_ref(body.as_ref()), out),
                Stmt::Let { .. }
                | Stmt::Expr(_)
                | Stmt::Return(_)
                | Stmt::Break
                | Stmt::Continue
                | Stmt::LabeledBreak(_)
                | Stmt::LabeledContinue(_)
                | Stmt::Throw(_)
                | Stmt::PreallocateBoxes(_)
                | Stmt::PreallocateTdzBoxes(_) => {}
            }
        }
    }
    let mut out = None;
    walk(stmts, &mut out);
    out
}

/// Push the word a fresh closure stores in the capture slot for `cap_id`:
/// for a cell binding the cell (or scope object) pointer, otherwise the
/// binding's current value. `closure_births` passes the same words for the
/// closures it bears from a table.
pub(crate) fn push_capture_word(
    ctx: &mut FnCtx<'_>,
    cap_id: u32,
    birth: &mut crate::stmt::binding_cell::BirthWords,
) -> Result<()> {
    if ctx.boxed_vars.contains(&cap_id) {
        // If the enclosing function has this id boxed,
        // we want to forward the BOX POINTER through
        // the capture slot as raw bits, not the value inside
        // the box. Read the slot directly without
        // going through the normal LocalGet path (which
        // would deref via js_box_get).
        if let Some(&_capture_idx) = ctx.closure_captures.get(&cap_id) {
            // We're inside a closure and this id is a
            // transitively-captured box. Read the
            // capture slot RAW (it holds the box ptr
            // bits) and propagate directly.
            let closure_ptr = super::current_closure_ptr_value(ctx, "nested boxed capture")?;
            let idx_str = _capture_idx.to_string();
            let v = ctx.block().call(
                I64,
                "js_closure_get_capture_bits",
                &[(I64, &closure_ptr), (I32, &idx_str)],
            );
            birth.push(v);
        } else if let Some(slot) = ctx.locals.get(&cap_id).cloned() {
            // Enclosing function owns the box: slot holds
            // the raw box pointer as i64.
            let box_ptr = ctx.block().load(I64, &slot);
            let bits = ctx
                .block()
                .or(I64, &box_ptr, &crate::nanbox::POINTER_TAG.to_string());
            birth.push(bits);
        } else if let Some(global_name) = ctx.module_globals.get(&cap_id).cloned() {
            // Global boxed var (rare).
            let v = crate::codegen::global_transfer::load_module_global(ctx, cap_id, &global_name);
            let v_bits = ctx.block().bitcast_double_to_i64(&v);
            birth.push(v_bits);
        } else {
            // No storage for the binding in this context: the
            // closure's own cell, minted above.
            birth.push_minted();
        }
    } else {
        let v = lower_expr(ctx, &Expr::LocalGet(cap_id))?;
        let v_bits = ctx.block().bitcast_double_to_i64(&v);
        birth.push(v_bits);
    }
    Ok(())
}

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
                crate::codegen::arguments::ensure_arguments_object(ctx, *cap_id);
            }
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
            let plain_async_step_cells = plain_async_step_release_ids(body);
            let uncounted_box_capture = |cap_id: &u32| {
                plain_async_step_cells
                    .as_ref()
                    .is_some_and(|cells| cells.contains(cap_id))
            };
            // The birth rule (`stmt::binding_cell`): every cell binding this
            // closure captures has its cell before any slot word is read.
            crate::stmt::binding_cell::ensure_capture_cells(ctx, &auto_captures);
            // A capture with no storage here gets a cell of its own, and a
            // mint allocates: `BirthWords` mints those cells before any other
            // word is read and keeps them rooted across each other.
            let mut birth = crate::stmt::binding_cell::BirthWords::new(ctx, &auto_captures);
            let mut cell_capture_slots: Vec<usize> = Vec::new();
            for cap_id in &auto_captures {
                push_capture_word(ctx, *cap_id, &mut birth)?;
                if ctx.boxed_vars.contains(cap_id) && !ctx.module_globals.contains_key(cap_id) {
                    cell_capture_slots.push(birth.len() - 1);
                }
            }
            let captured_value_bits = birth.read(ctx);

            if crate::stmt::binding_cell::capture_cell_asserts_enabled() {
                for &slot in &cell_capture_slots {
                    crate::stmt::binding_cell::emit_capture_cell_assert(
                        ctx,
                        &captured_value_bits[slot],
                    );
                }
            }

            // Compute the closure function name BEFORE taking the
            // mutable block borrow.
            let func_name =
                crate::fn_info::closure_body_symbol(ctx.strings.module_prefix(), *func_id);

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

            // The body's `JsFunctionInfo` (`crate::fn_info`): what every
            // allocation below names instead of the body.
            let info_ref = ctx.block().fn_info_ref(&func_name);
            // Issue #450: when `captures_this`, OR in the runtime's
            // `CAPTURES_THIS_FLAG` (0x8000_0000) so the runtime can detect
            // closures whose last capture slot is the reserved `this` slot.
            // `js_closure_alloc` masks the flag off when computing allocation
            // size (real_capture_count) but preserves it in the stored
            // `capture_count` field. Used by `clone_closure_rebind_this` at
            // `Object.defineProperty(obj, k, { get(){}, set(){} })` time so
            // accessor invocation sees `this === obj` per spec.
            // Validate before truncating to the packed header word.
            let total_caps_word = crate::runtime_abi::checked_closure_capture_count(total_caps);
            let cap_count_val = if *captures_this {
                total_caps_word | 0x8000_0000u32
            } else {
                total_caps_word
            };
            let cap_count = cap_count_val.to_string();
            // Closures with NO captures (and no `this` to patch) are
            // observationally identical across every call site that
            // produces them, so route through `js_closure_alloc_singleton`
            // to share a single ClosureHeader cached by body info.
            //
            // Closures WITH captures route through
            // `js_closure_alloc_with_captures_singleton` whenever none of
            // those captures are mutated by the body (the common case
            // for ECS callbacks like `(eid, arch, compId) => { ...
            // changeset ... }` capturing `this._changeset`). The cache
            // keys on (body info, capture_bits…) so distinct capture
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
            // Boxed captures may use the bulk cache helper, but codegen still
            // follows it with `js_closure_set_box_capture_ptr` for only those
            // slots. The idempotent write declares exact lifetime edges
            // without guessing from arbitrary pointer-shaped values.
            //
            // IDENTITY CAVEAT (#4831 follow-up — Stripe `protoExtend`):
            // the singleton-sharing paths (`js_closure_alloc_singleton` /
            // `js_closure_alloc_with_captures_singleton`) return ONE cached
            // `ClosureHeader` for a given (body info[, capture-bits]) key, so two
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
            // The Stripe fix restricted the caches to arrows (no own
            // `.prototype`, not constructable) plus non-arrow all-boxed
            // captures. pi's boot showed that is still not enough: identity
            // itself is observable (`===`, expandos, setPrototypeOf), not
            // just `.prototype`, so ANY user literal is off-limits — see the
            // identity gate below.
            let mut write_ids = std::collections::HashSet::new();
            crate::boxed_vars::collect_write_ids_in_stmts(body, &mut write_ids);
            let writes_unboxed_capture = auto_captures
                .iter()
                .any(|cap_id| !ctx.boxed_vars.contains(cap_id) && write_ids.contains(cap_id));
            let captures_all_boxed = !*captures_this
                && !*captures_new_target
                && !auto_captures.is_empty()
                && auto_captures
                    .iter()
                    .all(|cap_id| ctx.boxed_vars.contains(cap_id));
            // IDENTITY GATE (pi boot blocker): a closure literal evaluation
            // must produce a FRESH function object every time it runs
            // (ECMA-262 OrdinaryFunctionCreate). Sharing one ClosureHeader
            // across evaluations is observable through `===`, expando
            // properties, WeakMap/WeakSet keys, addEventListener identity
            // de-duplication — and through `Object.setPrototypeOf(a, b)`,
            // which for a conflated pair (a IS b) becomes a SELF-set and
            // throws "TypeError: Cyclic __proto__ value". pi's esbuild
            // bundle wires `setPrototypeOf(wrapped, original)` where both
            // came from the same arrow literal with bit-identical captures,
            // so its boot died on exactly that throw.
            //
            // The singleton caches therefore only serve closures the
            // COMPILER synthesized: the async-activation step closures
            // recognized by `plain_async_step_release_ids` (their terminal
            // `ReleaseBoxes` arms cannot appear in user code, and their
            // identity never escapes the runtime's promise machinery).
            // Those are the closures the caches were built for — re-created
            // per resume with the same per-activation box captures. User
            // arrows and function expressions always mint fresh objects.
            let is_plain_async_step = plain_async_step_cells.is_some();
            let singleton_identity_safe = is_plain_async_step && (*is_arrow || captures_all_boxed);
            let no_capture_singleton = is_plain_async_step && *is_arrow && total_caps == 0;
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
                    // Issue #1845: never a 0.0 sentinel. See the matching
                    // comment on the post-create patch path below.
                    crate::expr::body_call::unbound_this_value(ctx)
                })
            } else {
                None
            };

            // Fresh closures can install boxed and ordinary captures in bulk.
            // The runtime roots and refreshes every word on its collecting arm.
            let bulk_fresh_init = !no_capture_singleton && !captured_singleton && total_caps > 0;
            // An `async function(){}` *expression* closure (one with no
            // `await` — bodies that await are rewritten to a state machine
            // upstream and arrive with `is_async: false`) is async in its
            // info, so `IsConstructor`/`util.types.isAsyncFunction` recognize
            // it and its ShapeId names %AsyncFunction.prototype% at birth.
            // Generators are hoisted to top-level and described elsewhere.
            // (Test262 subclass/superclass-async-function.)
            if *is_async {
                ctx.block()
                    .note_fn_info(&func_name, |facts| facts.set_async());
            }
            // A small fresh birth passes the complete header image and its
            // capture words in registers. The shared helper stores directly in
            // the final payload; there is no per-site bump path or stack copy.
            let small_fresh_birth = bulk_fresh_init && total_caps <= 3;
            // Body-kind facts may be added after async/generator lowering.
            // Read the immutable local info constant, rather than the rewritten
            // expression flags. LLVM folds these operations into the birth
            // immediate: no body classification is emitted at a creation site.
            let shape = {
                // The info may carry range tails in an enclosing struct. A
                // byte GEP works for both layouts without asking the native IR
                // reader to resolve the base struct as a standalone type.
                let flags_offset = if crate::target_layout::target_is_ilp32(ctx.target_triple) {
                    "8"
                } else {
                    "12"
                };
                let blk = ctx.block();
                let flag_ptr = blk.gep(crate::types::I8, &info_ref, &[(I64, flags_offset)]);
                let flags = blk.load(I32, &flag_ptr);
                let shifted = blk.lshr(I32, &flags, "6");
                let ordinary = blk.and(I32, &shifted, "1");
                let generator = blk.and(I32, &flags, &crate::runtime_abi::FN_GENERATOR.to_string());
                let generator = blk.icmp_ne(I32, &generator, "0");
                let kind = blk.select(crate::types::I1, &generator, I32, "2", &ordinary);
                let async_generator = blk.and(
                    I32,
                    &flags,
                    &crate::runtime_abi::FN_ASYNC_GENERATOR.to_string(),
                );
                let async_generator = blk.icmp_ne(I32, &async_generator, "0");
                let kind = blk.select(crate::types::I1, &async_generator, I32, "3", &kind);
                blk.add(
                    I32,
                    &kind,
                    &crate::runtime_abi::FUNCTION_LAYOUT_WORD.to_string(),
                )
            };
            let birth_header = {
                let blk = ctx.block();
                let shape = blk.zext(I32, &shape, I64);
                let high = blk.shl(I64, &shape, "32");
                blk.or(I64, &high, &cap_count_val.to_string())
            };
            let closure_handle = if no_capture_singleton {
                let blk = ctx.block();
                blk.call(I64, "js_closure_alloc_singleton", &[(PTR, &info_ref)])
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
                    &[(PTR, &info_ref), (I32, &cap_count), (PTR, &buf)],
                )
            } else {
                let words: Vec<String> = (0..total_caps)
                    .map(|i| {
                        captured_value_bits
                            .get(i)
                            .cloned()
                            .unwrap_or_else(|| crate::nanbox::TAG_UNDEFINED_I64.to_string())
                    })
                    .collect();
                super::inline_birth::closure(ctx, &info_ref, &birth_header, &words, |ctx| {
                    if small_fresh_birth {
                        let words: Vec<&str> = (0..3)
                            .map(|i| {
                                captured_value_bits.get(i).map(String::as_str).unwrap_or(
                                    if i < total_caps {
                                        crate::nanbox::TAG_UNDEFINED_I64
                                    } else {
                                        "0"
                                    },
                                )
                            })
                            .collect();
                        ctx.block().call(
                            I64,
                            "js_closure_alloc_small",
                            &[
                                (PTR, &info_ref),
                                (I64, &birth_header),
                                (I64, words[0]),
                                (I64, words[1]),
                                (I64, words[2]),
                            ],
                        )
                    } else if bulk_fresh_init {
                        // Fresh (identity-carrying) closure with bulk captures:
                        // ONE runtime call does allocation + slots + layout instead of
                        // `js_closure_alloc` plus a `js_closure_set_capture_bits` per
                        // capture (each re-resolving the header, forwarding, kind
                        // dispatch and the barrier's page classification). The reserved
                        // `this` / `new.target` slots are pre-filled with the pointer-free
                        // sentinel the per-slot path relied on `js_closure_alloc` writing;
                        // the post-create patch below fills them exactly as before.
                        let buf = ctx.func.alloca_entry_array(I64, total_caps);
                        {
                            let blk = ctx.block();
                            for i in 0..total_caps {
                                let slot = blk.gep(I64, &buf, &[(I64, &format!("{}", i))]);
                                match captured_value_bits.get(i) {
                                    Some(v_bits) => blk.store(I64, v_bits, &slot),
                                    None => blk.store(I64, crate::nanbox::TAG_UNDEFINED_I64, &slot),
                                }
                            }
                        }
                        let blk = ctx.block();
                        blk.call(
                            I64,
                            "js_closure_alloc_fixed",
                            &[(PTR, &info_ref), (I64, &birth_header), (PTR, &buf)],
                        )
                    } else {
                        let blk = ctx.block();
                        blk.call(
                            I64,
                            "js_closure_alloc_empty",
                            &[(PTR, &info_ref), (I32, &shape)],
                        )
                    }
                })
            };

            // The captured-singleton helper writes captures internally. Boxed
            // slots still take the dedicated, idempotent setter afterward so
            // their lifetime edges are declared; fresh closures need every
            // slot initialized here. The compiler-private plain-async step
            // closure is different: its activation refcount already covers
            // every queued/running instance of its OWN cells, so declaring its
            // whole boxed frame as escaped would delay every terminal cell
            // until a full GC. User closures nested inside it still take the
            // dedicated setter and therefore preserve #8213's escaped-cell
            // lifetime, and so does a step closure's capture of an enclosing
            // scope's cell (#10464).
            let tracked_box_capture_slots = auto_captures
                .iter()
                .map(|cap_id| ctx.boxed_vars.contains(cap_id) && !uncounted_box_capture(cap_id))
                .collect::<Vec<_>>();
            // The allocation above collects: the setters below take the words
            // as of here.
            let captured_value_bits = if bulk_fresh_init {
                captured_value_bits
            } else {
                birth.read(ctx)
            };
            birth.release(ctx);
            let blk = ctx.block();
            for (idx, val_bits) in captured_value_bits.iter().enumerate() {
                let track_box_capture = tracked_box_capture_slots[idx];
                if bulk_fresh_init {
                    // Every slot was written by the bulk birth helper.
                    continue;
                }
                if !captured_singleton || track_box_capture {
                    let idx_str = idx.to_string();
                    let setter = if track_box_capture {
                        "js_closure_set_box_capture_ptr"
                    } else {
                        "js_closure_set_capture_bits"
                    };
                    blk.call_void(
                        setter,
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
            // Issue #1845: an arrow captures its enclosing body's `this`,
            // never a 0.0 sentinel. effect's fiber-runtime op-dispatch hit
            // the sentinel: `[OP_WITH_RUNTIME](op){ internalCall(() =>
            // op.i0(this, …)) }` passed `this = 0.0` (a raw number) into the
            // WithRuntime handler, so `fiber.currentContext` read `undefined`
            // and `Effect.all`/`Effect.forEach` died with a `{}`
            // FiberFailure. A body that reads `this` (a computed-key method
            // lowered to a function-expression field included) binds it from
            // its receiver parameter at entry, so the slot is the enclosing
            // `this`; code with no receiver (module top level) takes exactly
            // what a direct `Expr::This` there reads.
            if *captures_this {
                let this_idx = this_capture_idx.to_string();
                let this_slot = ctx.this_stack.last().cloned();
                let this_value = if let Some(slot) = this_slot {
                    ctx.block().load(DOUBLE, &slot)
                } else {
                    crate::expr::body_call::unbound_this_value(ctx)
                };
                let blk = ctx.block();
                let this_bits = blk.bitcast_double_to_i64(&this_value);
                blk.call_void(
                    "js_closure_set_capture_bits",
                    &[(I64, &closure_handle), (I32, &this_idx), (I64, &this_bits)],
                );
                if *is_arrow {
                    crate::expr::this_super_call::register_derived_ctor_arrow(
                        ctx,
                        &closure_handle,
                        &this_idx,
                        &this_value,
                    );
                }
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
