//! A packed clone's accumulator scope: which locals the fast clone proves
//! Number (accumulators and element bindings), their unboxed F64 storage, and
//! the write-back on every exit. Split out of `stmt/loops.rs`.

use super::*;

/// The live state of a packed clone's accumulator admission: the admitted
/// ids and the element bindings
/// (`stable_packed_accumulator::collect_numeric_element_lets`), which open
/// the clone's Number scope (see `local_is_number`), the
/// unboxed subset (id, F64 alloca, real slot) whose reads/writes redirect
/// through `ctx.numeric_accumulator_f64_slots`, and the side-exit trampoline
/// that writes the live values back before entering the slow clone.
pub(super) struct PackedAccumulatorScope {
    pub(super) accumulators: Vec<u32>,
    pub(super) unboxed: Vec<(u32, String, String)>,
    /// Update-only integer accumulators deferred to the i32 slot for the
    /// clone: (id, i32 slot, double slot) — exits re-sync the double.
    pub(super) deferred_integer: Vec<(u32, String, String)>,
    pub(super) side_exit_override: Option<String>,
    /// Receiver ids this scope materialised in the shared descriptor table;
    /// cleared by `finish`. An inner scope reusing an outer descriptor does
    /// not record that receiver here.
    pub(super) hoisted_receivers: Vec<u32>,
    /// Element bindings this scope redirected to a plain F64 alloca
    /// (`numeric_accumulator_f64_slots`); cleared by `finish`.
    pub(super) element_slots: Vec<u32>,
}

impl PackedAccumulatorScope {
    pub(super) fn empty() -> Self {
        Self {
            accumulators: Vec::new(),
            unboxed: Vec::new(),
            deferred_integer: Vec::new(),
            side_exit_override: None,
            hoisted_receivers: Vec::new(),
            element_slots: Vec::new(),
        }
    }

    /// Bind each element binding (`collect_numeric_element_lets`) to a plain
    /// F64 alloca for the clone: its `Let` stores there and its reads load
    /// from there (`stmt::let_stmt`, `expr::literals_vars`). Inside the clone
    /// the value is a Number, so it needs no GC root, and a root slot's
    /// escaped address would otherwise keep LLVM from hoisting the clone's
    /// receiver loads past the per-element store.
    pub(super) fn with_element_slots(mut self, ctx: &mut FnCtx<'_>, element_lets: &[u32]) -> Self {
        for id in element_lets {
            if ctx.numeric_accumulator_f64_slots.contains_key(id) {
                continue;
            }
            let alloca = ctx.func.alloca_entry(DOUBLE);
            ctx.numeric_accumulator_f64_slots.insert(*id, alloca);
            self.element_slots.push(*id);
        }
        self
    }
}

/// Admit Update-only INTEGER accumulators (`c++`) for i32-slot deferral:
/// writes all Updates, an integer local with a live i32 slot (the
/// authoritative in-clone storage — reads already prefer it), not the
/// loop counter, plain-local storage. An entry range test
/// (|value| < 2^30) branches to the slow clone so `bound <= 16M`
/// iterations of `add i32` cannot wrap; the i32 and double slots are in
/// sync at entry by the Update/LocalSet mirror invariant, so the i32
/// value IS the number.
fn admit_integer_update_accumulators(
    ctx: &mut FnCtx<'_>,
    body: &[Stmt],
    counter_id: u32,
    slow_pre_label: &str,
    block_prefix: &str,
) -> Vec<(u32, String, String)> {
    let mut writes = std::collections::BTreeMap::new();
    super::stable_packed_accumulator::collect_local_writes(body, &mut writes);
    let mut in_range: Option<String> = None;
    let mut admitted: Vec<(u32, String, String)> = Vec::new();
    for (id, ws) in &writes {
        if *id == counter_id
            || ws.is_empty()
            || !ws.iter().all(|w| w.is_none())
            || !ctx.integer_locals.contains(id)
            || ctx.boxed_vars.contains(id)
            || ctx.closure_captures.contains_key(id)
            || ctx.module_globals.contains_key(id)
        {
            continue;
        }
        let Some(dbl_slot) = ctx.locals.get(id).cloned() else {
            continue;
        };
        // Count accumulators are rarely index-used, so most have no i32
        // slot yet — create a scope-local one here: range-test the (integer
        // by `integer_locals` invariant) double value, seed the slot with
        // its exact `fptosi`, and REGISTER it so every in-clone read takes
        // the i32-first `LocalGet` arm. `finish`/the trampoline unregister
        // it and sync the double back.
        let created_slot = if ctx.i32_counter_slots.contains_key(id) {
            None
        } else {
            Some(ctx.func.alloca_entry(I32))
        };
        let existing_slot = ctx.i32_counter_slots.get(id).cloned();
        let blk = ctx.block();
        let (value_i32, ok) = if let Some(new_slot) = &created_slot {
            let dbl = blk.load(DOUBLE, &dbl_slot);
            let below = blk.fcmp("olt", &dbl, "1073741824.0");
            let above = blk.fcmp("ogt", &dbl, "-1073741824.0");
            let ok = blk.and(I1, &below, &above);
            let as_i32 = blk.fptosi(DOUBLE, &dbl, I32);
            blk.store(I32, &as_i32, new_slot);
            (new_slot.clone(), ok)
        } else {
            let slot = existing_slot.expect("checked contains_key above");
            let value = blk.load(I32, &slot);
            let below = blk.icmp_slt(I32, &value, "1073741824");
            let above = blk.icmp_sgt(I32, &value, "-1073741824");
            (slot, blk.and(I1, &below, &above))
        };
        in_range = Some(match in_range {
            Some(prev) => ctx.block().and(I1, &prev, &ok),
            None => ok,
        });
        if let Some(new_slot) = created_slot {
            ctx.i32_counter_slots.insert(*id, new_slot);
        }
        admitted.push((*id, value_i32, dbl_slot));
    }
    if admitted.is_empty() {
        return admitted;
    }
    let ok_idx = ctx.new_block(&format!("{block_prefix}.intacc.ok"));
    let ok_label = ctx.block_label(ok_idx);
    let in_range = in_range.expect("at least one admitted");
    ctx.block().cond_br(&in_range, &ok_label, slow_pre_label);
    ctx.current_block = ok_idx;
    for (id, _, _) in &admitted {
        ctx.deferred_integer_update_accumulators.insert(*id);
    }
    admitted
}

impl PackedAccumulatorScope {
    /// Cache each receiver's box in a promotable precise-root alloca for this
    /// fast clone. Receivers here are matcher-validated plain locals or module
    /// globals (never captures/boxes), and the clone body is call-free, so the
    /// only collection point is the loop poll — whose armed arm asks the
    /// shared receiver-descriptor table for every admitted refresh recipe.
    pub(super) fn hoist_receivers(&mut self, ctx: &mut FnCtx<'_>, array_ids: &[u32]) {
        for arr_id in array_ids {
            if ctx.receiver_descriptors.contains(*arr_id) {
                continue;
            }
            let Some((rooted_box_slot, base_handle_slot, source_ref)) =
                create_poll_refreshed_receiver_cache(ctx, *arr_id)
            else {
                continue;
            };
            let installed = ctx.receiver_descriptors.materialize_poll_refreshed_address(
                *arr_id,
                rooted_box_slot,
                base_handle_slot,
                source_ref,
                // Packed loop admission rejects Stmt::Try. A throw may
                // leave the clone, but no descriptor is live in the
                // handler reached outside this dynamic extent.
                true,
            );
            debug_assert!(installed, "duplicate receiver checked above");
            self.hoisted_receivers.push(*arr_id);
        }
    }

    /// Integer-only scopes still need the side-exit write-back trampoline
    /// (the double slot is stale mid-clone); build it and set the override.
    pub(super) fn with_deferred_trampoline(
        mut self,
        ctx: &mut FnCtx<'_>,
        slow_pre_label: &str,
        block_prefix: &str,
    ) -> Self {
        if self.deferred_integer.is_empty() {
            return self;
        }
        let tramp_idx = ctx.new_block(&format!("{block_prefix}.intacc.writeback_exit"));
        let saved = ctx.current_block;
        ctx.current_block = tramp_idx;
        for (_, i32_slot, dbl_slot) in &self.deferred_integer {
            let blk = ctx.block();
            let value = blk.load(I32, i32_slot);
            let as_double = blk.sitofp(I32, &value, DOUBLE);
            blk.store(DOUBLE, &as_double, dbl_slot);
        }
        ctx.block().br(slow_pre_label);
        ctx.current_block = saved;
        self.side_exit_override = Some(ctx.block_label(tramp_idx));
        self
    }

    /// The label packed facts should carry as their side exit: the
    /// write-back trampoline when unboxed accumulators exist, else the slow
    /// preheader itself.
    pub(super) fn fact_side_exit(&self, slow_pre_label: &str) -> String {
        self.side_exit_override
            .clone()
            .unwrap_or_else(|| slow_pre_label.to_string())
    }

    /// Fall-through exit: write the live values back to the real slots and
    /// end the redirect scope. Must run right after the fast clone's
    /// lowering, BEFORE the slow clone is lowered (the slow clone reads and
    /// writes the real slots).
    pub(super) fn finish(self, ctx: &mut FnCtx<'_>) {
        let emit_writeback = !ctx.block().is_terminated();
        for (id, alloca, real_slot) in &self.unboxed {
            if emit_writeback {
                let value = ctx.block().load(DOUBLE, alloca);
                // A genuine double's bits are its nanbox; numbers carry no
                // heap edge, so no barrier. Leaving the shadow state
                // conservative is always safe — shadow slots only license
                // SKIPPING a root scan, and scanning a number nanbox is
                // harmless.
                ctx.block().store(DOUBLE, &value, real_slot);
            }
            ctx.numeric_accumulator_f64_slots.remove(id);
        }
        for (id, i32_slot, dbl_slot) in &self.deferred_integer {
            if emit_writeback {
                let blk = ctx.block();
                let value = blk.load(I32, i32_slot);
                let as_double = blk.sitofp(I32, &value, DOUBLE);
                blk.store(DOUBLE, &as_double, dbl_slot);
            }
            ctx.deferred_integer_update_accumulators.remove(id);
            // A slot this scope created is scope-local: unregister it so
            // post-loop reads go back to the (now re-synced) double slot.
            if ctx.i32_counter_slots.get(id) == Some(i32_slot) {
                ctx.i32_counter_slots.remove(id);
            }
        }
        for id in &self.element_slots {
            ctx.numeric_accumulator_f64_slots.remove(id);
        }
        for arr_id in &self.hoisted_receivers {
            let removed = ctx.receiver_descriptors.dematerialize(*arr_id);
            debug_assert!(removed, "scope owns every receiver it materialised");
        }
    }
}

/// Write the fast clone's promoted accumulators back to their real slots
/// WITHOUT ending the redirect scope.
///
/// `PackedAccumulatorScope::finish` covers the fall-through exit and the
/// side-exit trampolines cover a mid-iteration deopt, but both are blocks the
/// clone BRANCHES to, and `break`/`continue` reach them the same way. An
/// unwind edge reaches neither: under invoke-EH `js_throw` leaves for the
/// landing pad from inside the call itself, so a `catch` observes whatever is
/// in the real slot at that moment. Emitting the stores at the throw site is
/// the only point that works, which is what #9185 was missing.
///
/// Unlike `finish` this does NOT unregister the redirects: lowering continues
/// inside the clone afterwards, and later statements must keep reading the
/// promoted values.
pub(crate) fn flush_packed_accumulator_locals(ctx: &mut FnCtx<'_>) {
    if ctx.numeric_accumulator_f64_slots.is_empty()
        && ctx.deferred_integer_update_accumulators.is_empty()
    {
        return;
    }
    if ctx.block().is_terminated() {
        return;
    }

    // The side tables are hash-keyed, so collect and sort before emitting —
    // IR order must not depend on hash iteration order.
    let mut unboxed: Vec<(u32, String, String)> = ctx
        .numeric_accumulator_f64_slots
        .iter()
        .filter_map(|(id, alloca)| {
            ctx.locals
                .get(id)
                .map(|real_slot| (*id, alloca.clone(), real_slot.clone()))
        })
        .collect();
    unboxed.sort_by_key(|(id, _, _)| *id);
    for (_, alloca, real_slot) in &unboxed {
        // Same argument as `finish`: a genuine double's bits are its nanbox,
        // numbers carry no heap edge so no barrier, and leaving the shadow
        // state conservative only ever costs an extra root scan.
        let value = ctx.block().load(DOUBLE, alloca);
        ctx.block().store(DOUBLE, &value, real_slot);
    }

    let mut deferred: Vec<(u32, String, String)> = ctx
        .deferred_integer_update_accumulators
        .iter()
        .filter_map(|id| {
            let i32_slot = ctx.i32_counter_slots.get(id)?;
            let dbl_slot = ctx.locals.get(id)?;
            Some((*id, i32_slot.clone(), dbl_slot.clone()))
        })
        .collect();
    deferred.sort_by_key(|(id, _, _)| *id);
    for (_, i32_slot, dbl_slot) in &deferred {
        let blk = ctx.block();
        let value = blk.load(I32, i32_slot);
        let as_double = blk.sitofp(I32, &value, DOUBLE);
        blk.store(DOUBLE, &as_double, dbl_slot);
    }
}

pub(super) fn emit_packed_numeric_accumulator_admission(
    ctx: &mut FnCtx<'_>,
    body: &[Stmt],
    array_ids: &std::collections::BTreeSet<u32>,
    counter_id: u32,
    slow_pre_label: &str,
    block_prefix: &str,
    offset_reads_inlined: bool,
    masked_reads_validated: bool,
    affine_reads: bool,
) -> PackedAccumulatorScope {
    let element_lets = super::stable_packed_accumulator::collect_numeric_element_lets(
        ctx, body, array_ids, counter_id,
    );
    let accumulators = super::stable_packed_accumulator::collect_numeric_accumulators_over(
        ctx,
        body,
        array_ids,
        counter_id,
        offset_reads_inlined,
        masked_reads_validated,
        affine_reads,
        &element_lets,
    );
    // Integer (`c++`) accumulators admit independently of the float set —
    // a pure count loop has no float accumulator at all.
    let scope_deferred_integer =
        admit_integer_update_accumulators(ctx, body, counter_id, slow_pre_label, block_prefix);
    // The clone's Number scope: the accumulators and the element bindings.
    let number_scope = || {
        let mut ids = accumulators.clone();
        ids.extend(
            element_lets
                .iter()
                .copied()
                .filter(|id| !accumulators.contains(id)),
        );
        ids
    };
    let element_lets: Vec<u32> = element_lets.iter().copied().collect();
    if accumulators.is_empty() && scope_deferred_integer.is_empty() {
        return PackedAccumulatorScope {
            accumulators: number_scope(),
            ..PackedAccumulatorScope::empty()
        }
        .with_element_slots(ctx, &element_lets);
    }
    if accumulators.is_empty() {
        return PackedAccumulatorScope {
            accumulators: number_scope(),
            unboxed: Vec::new(),
            deferred_integer: scope_deferred_integer,
            side_exit_override: None,
            hoisted_receivers: Vec::new(),
            element_slots: Vec::new(),
        }
        .with_deferred_trampoline(ctx, slow_pre_label, block_prefix)
        .with_element_slots(ctx, &element_lets);
    }
    let mut loaded: Vec<(u32, String, String)> = Vec::new();
    let mut all_numbers: Option<String> = None;
    for id in &accumulators {
        let Some(slot) = ctx.locals.get(id).cloned() else {
            return PackedAccumulatorScope::empty();
        };
        let value = ctx.block().load(DOUBLE, &slot);
        // `emit_js_value_is_number` IS the strict genuine-double window:
        // SHORT_STRING (0x7FF9) .. STRING (0x7FFF) is the ENTIRE boxed tag
        // band (INT32/POINTER/BIGINT/singletons included), so
        // `tag < SHORT_STRING || tag > STRING` accepts exactly non-boxed
        // doubles. That strictness is required here — the fact these ids
        // ride lets consumers use bare fadd/fcmp on the value, and an
        // INT32-boxed number's bits are not a valid double; it takes the
        // slow clone instead. Shared with the stable clone's admission so
        // the two cannot drift.
        let is_number = emit_js_value_is_number(ctx, &value);
        all_numbers = Some(match all_numbers {
            Some(prev) => ctx.block().and(I1, &prev, &is_number),
            None => is_number,
        });
        loaded.push((*id, slot, value));
    }
    let all_numbers = all_numbers.expect("at least one accumulator");
    let acc_ok_idx = ctx.new_block(&format!("{block_prefix}.acc.ok"));
    let acc_ok_label = ctx.block_label(acc_ok_idx);
    // A non-Number accumulator (a string total, a BigInt) takes the slow
    // clone before the first fast iteration; nothing has run yet, so the
    // slow clone sees pristine state.
    ctx.block()
        .cond_br(&all_numbers, &acc_ok_label, slow_pre_label);
    ctx.current_block = acc_ok_idx;

    // Unbox LocalSet-only accumulators into plain F64 allocas (mem2reg
    // promotes them to registers — the GC-root slot's per-iteration
    // store-to-load-forward chain was the reduce rows' latency floor).
    // Update-written accumulators (`c++`) keep the slot: the Update lowering
    // does not consult the redirect, and the int-slot machinery already
    // serves counters well.
    let mut writes = std::collections::BTreeMap::new();
    super::stable_packed_accumulator::collect_local_writes(body, &mut writes);
    let mut unboxed: Vec<(u32, String, String)> = Vec::new();
    for (id, slot, value) in loaded {
        let localset_only = writes
            .get(&id)
            .is_some_and(|ws| ws.iter().all(|w| w.is_some()));
        if !localset_only {
            continue;
        }
        let alloca = ctx.func.alloca_entry(DOUBLE);
        ctx.block().store(DOUBLE, &value, &alloca);
        ctx.numeric_accumulator_f64_slots.insert(id, alloca.clone());
        unboxed.push((id, alloca, slot));
    }
    let side_exit_override = if unboxed.is_empty() && scope_deferred_integer.is_empty() {
        None
    } else {
        // Side-exit trampoline: any mid-iteration exit (a hole-checked load,
        // a masked store's value check) lands here, writes the live values
        // back, and only then enters the slow clone — which re-executes the
        // current iteration against correct slot state.
        let tramp_idx = ctx.new_block(&format!("{block_prefix}.acc.writeback_exit"));
        let saved = ctx.current_block;
        ctx.current_block = tramp_idx;
        for (_, alloca, real_slot) in &unboxed {
            let value = ctx.block().load(DOUBLE, alloca);
            ctx.block().store(DOUBLE, &value, real_slot);
        }
        for (_, i32_slot, dbl_slot) in &scope_deferred_integer {
            let blk = ctx.block();
            let value = blk.load(I32, i32_slot);
            let as_double = blk.sitofp(I32, &value, DOUBLE);
            blk.store(DOUBLE, &as_double, dbl_slot);
        }
        ctx.block().br(slow_pre_label);
        ctx.current_block = saved;
        Some(ctx.block_label(tramp_idx))
    };
    PackedAccumulatorScope {
        accumulators: number_scope(),
        unboxed,
        deferred_integer: scope_deferred_integer,
        side_exit_override,
        hoisted_receivers: Vec::new(),
        element_slots: Vec::new(),
    }
    .with_element_slots(ctx, &element_lets)
}
