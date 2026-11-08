//! b2 (DESIGN-b2-cold-arm-refresh rev 2): a loop's element reads of one
//! array binding through a receiver descriptor that its own cold arm
//! refreshes.
//!
//! The loop's preheader installs, for a receiver binding the body never
//! assigns, a rooted box slot, a handle slot and an `i8` valid slot
//! (`install`). A read `a[e]` the shared predicate admits (`read_shape`:
//! the receiver is that binding and `e` is inert and reaches no ender) lowers
//! as
//!
//! - **fast**: the valid slot holds; `e` is an i32-exact non-negative
//!   Number below the array's live header length; the element is own data
//!   (not a hole; a plain Number in a number context): one raw load;
//! - **revalidate**: the valid slot is clear: the box is tested inline (heap
//!   band, an ordinary unforwarded array with no element descriptors), the
//!   handle re-derived, and on success the fast form runs;
//! - **slow**: everything else lowers by the ordinary route (the read with
//!   the descriptor bypassed), and the cold-arm refresh follows its normal
//!   return: every active descriptor's box is reloaded from its binding.
//!
//! Every call that may run code or collect clears every valid slot at the
//! `LlBlock` call chokepoint (`push_byte_access_dirty_slot`): the slow arm's
//! own calls, the back-edge poll, and anything unforeseen. So the fast form
//! never runs on a handle or a kind older than the last such call: it first
//! revalidates the current box. The reload restores the binding a getter may
//! have reassigned (a module global), which `LocalGet` of the receiver reads
//! through the rooted slot. Nothing else is believed across the slow arm:
//! admission requires that every other active receiver fact and loop table
//! refuses or survives a cold arm (`ReceiverDescriptorTable::cold_arm_refreshes`).

use anyhow::Result;
use perry_hir::{Expr, Stmt};

use super::FnCtx;
use crate::collectors::{ReceiverArrayValidationKind, RegionEnder};
use crate::nanbox::POINTER_MASK_I64;
use crate::types::{DOUBLE, I1, I32, I64, I8};

/// `PERRY_RECEIVER_COLD_ARM_REFRESH=0` restores the loops' admission
/// without b2.
fn enabled() -> bool {
    !matches!(
        std::env::var("PERRY_RECEIVER_COLD_ARM_REFRESH").as_deref(),
        Ok("0") | Ok("off") | Ok("false")
    )
}

/// Negative controls (`PERRY_RECEIVER_COLD_ARM_SABOTAGE`): drop one step of
/// the recipe. `reload`: no box reload after the slow arm; `guard`: the
/// valid slot is not cleared by calls; `handle`: revalidation keeps the old
/// handle. Each must make a fixture fail.
fn sabotage(step: &str) -> bool {
    std::env::var("PERRY_RECEIVER_COLD_ARM_SABOTAGE").as_deref() == Ok(step)
}

/// THE predicate (DESIGN-b2 §1), shared by the loop's classification and the
/// lowering: `e` is an element read of `receiver` whose index is inert and
/// reaches no ender (so the base it reads is the one the descriptor holds).
pub(crate) fn read_shape(ctx: &FnCtx<'_>, e: &Expr, receiver: u32) -> bool {
    let Expr::IndexGet { object, index, .. } = e else {
        return false;
    };
    if !matches!(object.as_ref(), Expr::LocalGet(id) if *id == receiver)
        || super::typed_feedback_emission_enabled()
    {
        return false;
    }
    let is_inert = |x: &Expr| crate::rooting::expr_is_inert_primitive(ctx, x);
    is_inert(index) && crate::collectors::region_enders_in_expr(index, &is_inert).is_empty()
}

/// The descriptor read route for `e`, when its receiver's cold-arm
/// descriptor is active.
fn route(ctx: &FnCtx<'_>, e: &Expr) -> Option<u32> {
    let Expr::IndexGet { object, .. } = e else {
        return None;
    };
    let Expr::LocalGet(id) = object.as_ref() else {
        return None;
    };
    let addr = e as *const Expr as usize;
    (!ctx.receiver_descriptors.cold_arm_bypassed(addr)
        && ctx.receiver_descriptors.cold_arm_access(*id).is_some()
        && read_shape(ctx, e, *id))
    .then_some(*id)
}

/// Lower `e` through its descriptor read when one is active. `numeric`: the
/// consumer is a number context.
pub(crate) fn try_lower(ctx: &mut FnCtx<'_>, e: &Expr, numeric: bool) -> Result<Option<String>> {
    let addr = e as *const Expr as usize;
    let Some(receiver) = route(ctx, e) else {
        // The lowering-side assertion (DESIGN-b2 §1): a read the region
        // classified as trusted is lowered by this form, or by its own slow
        // arm, and never by another route.
        assert!(
            !ctx.receiver_descriptors.is_trusted_cold_arm_read(addr)
                || ctx.receiver_descriptors.cold_arm_bypassed(addr),
            "a trusted descriptor read reached another lowering route"
        );
        return Ok(None);
    };
    lower_read(ctx, e, receiver, numeric).map(Some)
}

fn lower_read(ctx: &mut FnCtx<'_>, e: &Expr, receiver: u32, numeric: bool) -> Result<String> {
    let Expr::IndexGet { index, .. } = e else {
        unreachable!("route admits element reads only")
    };
    let access = ctx
        .receiver_descriptors
        .cold_arm_access(receiver)
        .expect("route admits an active descriptor");
    let rooted = ctx
        .receiver_descriptors
        .rooted_box_slot(receiver)
        .expect("a cold-arm descriptor is an address")
        .to_owned();
    crate::expr::store_census::bump(ctx, crate::expr::store_census::ELEM_READ_OTHER_TIER);
    // The index first: inert and ender-free, so the base is unchanged by it.
    let idx = crate::expr::lower_expr(ctx, index)?;
    let check = ctx.new_block("dread.revalidate");
    let check_deref = ctx.new_block("dread.revalidate.deref");
    let fast = ctx.new_block("dread.fast");
    let load = ctx.new_block("dread.load");
    let slow = ctx.new_block("dread.slow");
    let merge = ctx.new_block("dread.merge");
    let l = |ctx: &FnCtx<'_>, i| ctx.block_label(i);
    let (check_l, deref_l, fast_l, load_l, slow_l, merge_l) = (
        l(ctx, check),
        l(ctx, check_deref),
        l(ctx, fast),
        l(ctx, load),
        l(ctx, slow),
        l(ctx, merge),
    );
    {
        let blk = ctx.block();
        let v = blk.load(I8, &access.valid_slot);
        let valid = blk.icmp_ne(I8, &v, "0");
        blk.cond_br(&valid, &fast_l, &check_l);
    }
    // Revalidate the current box: heap band, then an ordinary unforwarded
    // array without element descriptors (the guarded read's own word test).
    ctx.current_block = check;
    {
        let blk = ctx.block();
        let bx = blk.load(DOUBLE, &rooted);
        let bits = blk.bitcast_double_to_i64(&bx);
        let off = blk.sub(I64, &bits, super::guarded_array::HEAP_POINTER_BAND_BASE_I64);
        let in_band = blk.icmp_ult(I64, &off, super::guarded_array::HEAP_POINTER_BAND_SPAN_I64);
        blk.cond_br(&in_band, &deref_l, &slow_l);
    }
    ctx.current_block = check_deref;
    {
        let blk = ctx.block();
        let bx = blk.load(DOUBLE, &rooted);
        let bits = blk.bitcast_double_to_i64(&bx);
        let handle = blk.and(I64, &bits, POINTER_MASK_I64);
        let word = super::guarded_array::emit_array_guard_word(blk, &handle);
        let ok = super::guarded_array::emit_array_guard_word_ok(blk, &word, &handle);
        if !sabotage("handle") {
            blk.store(I64, &handle, &access.base_handle_slot);
        }
        let byte = blk.zext(I1, &ok, I8);
        blk.store(I8, &byte, &access.valid_slot);
        blk.cond_br(&ok, &fast_l, &slow_l);
    }
    // Fast: an i32-exact non-negative index below the live length.
    ctx.current_block = fast;
    let idx_i32 = {
        let blk = ctx.block();
        let nonneg = blk.fcmp("oge", &idx, "0.0");
        let below = blk.fcmp("olt", &idx, "2147483648.0");
        let in_range = blk.and(I1, &nonneg, &below);
        let safe = blk.select(I1, &in_range, DOUBLE, &idx, "0.0");
        let i = blk.fptosi(DOUBLE, &safe, I32);
        let back = blk.sitofp(I32, &i, DOUBLE);
        let exact = blk.fcmp("oeq", &back, &idx);
        let integral = blk.and(I1, &in_range, &exact);
        let handle = blk.load(I64, &access.base_handle_slot);
        let ptr = blk.inttoptr(I64, &handle);
        let length = blk.load(I32, &ptr);
        let inside = blk.icmp_ult(I32, &i, &length);
        let go = blk.and(I1, &integral, &inside);
        blk.cond_br(&go, &load_l, &slow_l);
        i
    };
    ctx.current_block = load;
    let (fast_value, load_end) = {
        let blk = ctx.block();
        let handle = blk.load(I64, &access.base_handle_slot);
        let idx64 = blk.zext(I32, &idx_i32, I64);
        let off = blk.shl(I64, &idx64, "3");
        let elements = blk.array_elements_addr(&handle);
        let addr = blk.add(I64, &elements, &off);
        let slot = blk.inttoptr(I64, &addr);
        let raw = blk.load(I64, &slot);
        let hit = if numeric {
            crate::codegen::emit_plain_number_bits_test(blk, &raw)
        } else {
            blk.icmp_ne(I64, &raw, crate::nanbox::TAG_HOLE_I64)
        };
        let value = blk.bitcast_i64_to_double(&raw);
        let end = blk.label.clone();
        blk.cond_br(&hit, &merge_l, &slow_l);
        (value, end)
    };
    // Slow: the ordinary route with this descriptor bypassed, then the
    // cold-arm refresh.
    ctx.current_block = slow;
    let addr = e as *const Expr as usize;
    ctx.receiver_descriptors.push_cold_arm_bypass(addr);
    let slow_value = if numeric {
        match super::lower_numeric_index_get_for_number_context(ctx, e)? {
            Some(v) => v,
            None => {
                let v = super::lower(ctx, e)?;
                ctx.block().call(DOUBLE, "js_number_coerce", &[(DOUBLE, &v)])
            }
        }
    } else {
        super::lower(ctx, e)?
    };
    ctx.receiver_descriptors.pop_cold_arm_bypass(addr);
    emit_cold_arm_refresh(ctx);
    let slow_end = ctx.block().label.clone();
    ctx.block().br(&merge_l);
    ctx.current_block = merge;
    Ok(ctx.block().phi(
        DOUBLE,
        &[(&fast_value, &load_end), (&slow_value, &slow_end)],
    ))
}

/// After a slow arm returns: reload every active descriptor's box from its
/// binding (a getter may have reassigned it). The calls on the arm already
/// cleared every valid slot, so the next read revalidates the fresh box.
fn emit_cold_arm_refresh(ctx: &mut FnCtx<'_>) {
    let recipes = ctx
        .receiver_descriptors
        .cold_arm_refreshes()
        .expect("every active receiver fact must admit a cold arm");
    if sabotage("reload") {
        return;
    }
    let blk = ctx.block();
    for r in &recipes {
        let fresh = blk.load(DOUBLE, &r.refresh.source_root);
        blk.store(DOUBLE, &fresh, &r.refresh.rooted_box_slot);
        blk.store(I8, "0", &r.valid_slot);
    }
}

/// The loop tables a cold arm would stale and that do not live in the
/// descriptor table (REVIEW-b2 F1): the region is refused while any is set.
fn side_tables_clear(ctx: &FnCtx<'_>) -> bool {
    ctx.bounded_buffer_index_pairs.is_empty()
        && ctx.guarded_buffer_index_pairs.is_empty()
        && ctx.element_shape_loop_facts.is_empty()
        && ctx.stable_packed_loop_facts.is_empty()
        && ctx.versioned_indexed_loop_facts.is_empty()
        && ctx.string_window_array_facts.is_empty()
        && ctx.buffer_data_slots.is_empty()
        && ctx.class_keys_slots.is_empty()
}

/// The `let`s of `s` and its nested statements.
fn lets(s: &Stmt, out: &mut std::collections::HashSet<u32>) {
    match s {
        Stmt::Let { id, .. } => {
            out.insert(*id);
        }
        Stmt::If {
            then_branch,
            else_branch,
            ..
        } => {
            then_branch.iter().for_each(|x| lets(x, out));
            else_branch.iter().flatten().for_each(|x| lets(x, out));
        }
        Stmt::While { body, .. } | Stmt::DoWhile { body, .. } => body.iter().for_each(|x| lets(x, out)),
        Stmt::For { init, body, .. } => {
            init.iter().for_each(|x| lets(x, out));
            body.iter().for_each(|x| lets(x, out));
        }
        Stmt::Labeled { body, .. } => lets(body, out),
        Stmt::Try {
            body,
            catch,
            finally,
        } => {
            body.iter().for_each(|x| lets(x, out));
            if let Some(c) = catch {
                c.body.iter().for_each(|x| lets(x, out));
            }
            finally.iter().flatten().for_each(|x| lets(x, out));
        }
        Stmt::Switch { cases, .. } => cases
            .iter()
            .flat_map(|c| c.body.iter())
            .for_each(|x| lets(x, out)),
        _ => {}
    }
}

/// Every local the loop writes (body, condition, update).
fn loop_writes(stmts: &[Stmt], controls: &[&Expr], out: &mut std::collections::HashSet<u32>) {
    fn expr(e: &Expr, out: &mut std::collections::HashSet<u32>) {
        if let Expr::LocalSet(id, _) | Expr::Update { id, .. } = e {
            out.insert(*id);
        }
        perry_hir::walker::walk_expr_children(e, &mut |c| expr(c, out));
    }
    for s in stmts {
        lets(s, out);
        perry_hir::walker::stmt_any_expr(s, &mut |e| {
            expr(e, out);
            false
        });
    }
    for c in controls {
        expr(c, out);
    }
}

/// Every element read in the loop, with its receiver binding.
fn candidate_reads(stmts: &[Stmt], controls: &[&Expr]) -> Vec<(u32, usize)> {
    let mut out = Vec::new();
    let mut visit = |e: &Expr| {
        if let Expr::IndexGet { object, .. } = e {
            if let Expr::LocalGet(id) = object.as_ref() {
                out.push((*id, e as *const Expr as usize));
            }
        }
    };
    fn walk(e: &Expr, f: &mut dyn FnMut(&Expr)) {
        f(e);
        perry_hir::walker::walk_expr_children(e, &mut |c| walk(c, f));
    }
    for s in stmts {
        perry_hir::walker::stmt_any_expr(s, &mut |e| {
            walk(e, &mut visit);
            false
        });
    }
    for c in controls {
        walk(c, &mut visit);
    }
    out
}

/// The receivers whose descriptors this loop installed (`uninstall`).
pub(crate) struct Installed(Vec<u32>);

impl Installed {
    pub(crate) fn none() -> Self {
        Installed(Vec::new())
    }
}

/// DESIGN-b2 §2: at a loop's preheader, install a cold-arm descriptor for
/// each array binding the loop reads through the shared predicate, when the
/// region's every ender is the back-edge poll or such a read's cold arm and
/// every other active fact survives a cold arm.
pub(crate) fn install(ctx: &mut FnCtx<'_>, body: &[Stmt], controls: &[&Expr]) -> Installed {
    if !enabled() || !side_tables_clear(ctx) {
        return Installed(Vec::new());
    }
    let mut written = std::collections::HashSet::new();
    loop_writes(body, controls, &mut written);
    let mut receivers: Vec<u32> = Vec::new();
    for (id, _) in candidate_reads(body, controls) {
        if !receivers.contains(&id) && receiver_ok(ctx, id, &written) {
            receivers.push(id);
        }
    }
    receivers.retain(|&id| !ctx.receiver_descriptors.contains(id));
    if receivers.is_empty() {
        return Installed(Vec::new());
    }
    let is_inert = |x: &Expr| crate::rooting::expr_is_inert_primitive(ctx, x);
    let trusted_of = |e: &Expr| receivers.iter().find(|&&r| read_shape(ctx, e, r)).copied();
    let enders = crate::collectors::region_enders_in_stmts_classified(body, controls, &is_inert, &|e| {
        trusted_of(e).map(|_| Some(RegionEnder::ColdArmCall))
    });
    if enders
        .iter()
        .any(|x| !matches!(x, RegionEnder::BackEdgePoll | RegionEnder::ColdArmCall))
        || !enders.contains(&RegionEnder::ColdArmCall)
    {
        return Installed(Vec::new());
    }
    let mut enders = enders;
    enders.push(RegionEnder::BackEdgePoll);
    let reads = candidate_reads(body, controls);
    let mut installed = Vec::new();
    for &id in &receivers {
        let trusted: Vec<usize> = reads
            .iter()
            .filter(|(r, addr)| {
                *r == id && read_shape(ctx, unsafe { &*(*addr as *const Expr) }, id)
            })
            .map(|(_, addr)| *addr)
            .collect();
        if trusted.is_empty() {
            continue;
        }
        let Some((rooted, handle, source)) =
            crate::stmt::loops::create_poll_refreshed_receiver_cache(ctx, id)
        else {
            continue;
        };
        let valid = ctx.func.alloca_entry(I8);
        ctx.func.entry_allocas_push_store(I8, "0", &valid);
        ctx.block().store(I8, "0", &valid);
        if !sabotage("guard") {
            ctx.func.reg_counter().push_byte_access_dirty_slot(valid.clone());
        }
        match ctx.receiver_descriptors.materialize_cold_arm_array(
            id,
            rooted,
            handle,
            source,
            ReceiverArrayValidationKind::Plain,
            valid,
            &enders,
            trusted,
        ) {
            Ok(true) => installed.push(id),
            Ok(false) => {}
            Err(_) => {}
        }
    }
    Installed(installed)
}

pub(crate) fn uninstall(ctx: &mut FnCtx<'_>, installed: Installed) {
    for id in installed.0 {
        let removed = ctx.receiver_descriptors.dematerialize(id);
        debug_assert!(removed, "loop owns the cold-arm descriptor it installed");
    }
}

/// A binding whose descriptor the loop may hold: an unboxed, uncaptured
/// stack local or a module global that is no transfer binding, never
/// written in the loop, never scalar-replaced, and statically an array.
fn receiver_ok(ctx: &FnCtx<'_>, id: u32, written: &std::collections::HashSet<u32>) -> bool {
    if written.contains(&id)
        || ctx.boxed_vars.contains(&id)
        || ctx.closure_captures.contains_key(&id)
        || ctx.scalar_replaced_arrays.contains_key(&id)
        || ctx.array_row_aliases.contains_key(&id)
        || ctx.scalar_replaced.contains_key(&id)
        || ctx.module_global_transfers.contains_key(&id)
    {
        return false;
    }
    let local = ctx.locals.contains_key(&id) || ctx.module_globals.contains_key(&id);
    local && crate::type_analysis::is_array_expr(ctx, &Expr::LocalGet(id))
}
