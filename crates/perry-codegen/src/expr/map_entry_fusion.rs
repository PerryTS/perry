//! One Map lookup for the get-then-set idiom.
//!
//! `m.set(k, f(m.get(k)))` and `const cur = m.get(k); ... m.set(k, g(cur))`
//! probe the Map twice for one entry. This lowers the `get` through
//! `js_map_get_entry`, which also reports the entry's value slot, and lets the
//! matching `set` store straight into that slot. Nothing about it is a cache:
//!
//! * the slot lives in a stack cell of ONE activation, zeroed when the region
//!   opens and consumed only by the region's own `set` sites;
//! * every call the region emits zeroes the cell before entering the callee
//!   (the call-emission choke point, `LlBlock::dirty_revalidations_before_call`),
//!   so a collection, a user `valueOf`/getter, `delete`, `clear` or growth that
//!   could move or retire the entry always leaves the cell null on that path;
//! * the `set` stores only when the cell is non-null AND its key bits equal the
//!   bits the `get` looked up (bit identity is SameValueZero for a stored key),
//!   AND the value is not a heap reference (a scalar store owes no barrier);
//!   anything else is the ordinary `js_map_set`;
//! * the receiver is the same never-reassigned local at both sites, and an
//!   absent key leaves the cell null, so an insertion is always the ordinary
//!   `js_map_set`.
//!
//! A region is either one `MapSet` expression whose value computation reads
//! the same receiver and key, or a `Let` whose initializer reads `m.get(k)`
//! up to the last later statement of the same list that sets `m` at key `k`.
use super::FnCtx;
use perry_hir::{Expr, Stmt};
use std::collections::{HashMap, HashSet};

#[derive(Default)]
pub(crate) struct State {
    gets: HashSet<usize>,
    sets: HashSet<usize>,
    /// receiver local -> (slot cell, key-bits cell), both `i64` allocas.
    cells: HashMap<u32, (String, String)>,
    /// Open statement regions: (list address, last statement index, slot cell).
    open_lists: Vec<(usize, usize, String)>,
}

/// The cells a fused `set` site consults.
pub(crate) struct SetCells {
    pub(crate) slot: String,
    pub(crate) key: String,
}

fn stable_receiver(ctx: &FnCtx<'_>, e: &Expr) -> Option<u32> {
    match e {
        Expr::LocalGet(id)
            if !ctx.reassigned_locals.contains(id) && !ctx.boxed_vars.contains(id) =>
        {
            Some(*id)
        }
        _ => None,
    }
}

/// Whether two key expressions read the same place. Soundness never depends
/// on this (the set compares the key bits at run time); it only keeps the
/// fusion to the idiom.
fn same_key(a: &Expr, b: &Expr) -> bool {
    match (a, b) {
        (Expr::LocalGet(x), Expr::LocalGet(y)) => x == y,
        (Expr::Number(x), Expr::Number(y)) => x.to_bits() == y.to_bits(),
        (Expr::Integer(x), Expr::Integer(y)) => x == y,
        (Expr::String(x), Expr::String(y)) => x == y,
        (
            Expr::PropertyGet {
                object: oa,
                property: pa,
                ..
            },
            Expr::PropertyGet {
                object: ob,
                property: pb,
                ..
            },
        ) => pa == pb && same_key(oa, ob),
        _ => false,
    }
}

fn node(e: &Expr) -> usize {
    e as *const Expr as usize
}

fn cells_for(ctx: &mut FnCtx<'_>, local: u32) -> (String, String) {
    if let Some(c) = ctx.map_entry_fusion.cells.get(&local) {
        return c.clone();
    }
    let slot = ctx.func.alloca_entry(crate::types::I64);
    ctx.func
        .entry_allocas_push_store(crate::types::I64, "0", &slot);
    let key = ctx.func.alloca_entry(crate::types::I64);
    ctx.func
        .entry_allocas_push_store(crate::types::I64, "0", &key);
    ctx.map_entry_fusion
        .cells
        .insert(local, (slot.clone(), key.clone()));
    (slot, key)
}

/// Zero the cell and register it with the call-emission choke point.
fn open_cell(ctx: &mut FnCtx<'_>, local: u32) -> String {
    let (slot, _) = cells_for(ctx, local);
    ctx.block().store(crate::types::I64, "0", &slot);
    ctx.func.reg_counter().push_map_entry_slot(slot.clone());
    slot
}

fn close_cell(ctx: &mut FnCtx<'_>, slot: &str) {
    ctx.func.reg_counter().pop_map_entry_slot(slot);
}

/// Marks the `MapGet` nodes of `root` that read receiver `m` at `key`.
fn find_gets<'a>(
    ctx: &FnCtx<'_>,
    root: &'a Expr,
    m: u32,
    key: Option<&Expr>,
    out: &mut Vec<(&'a Expr, &'a Expr)>,
) {
    let mut visit = |e: &'a Expr| {
        if let Expr::MapGet { map, key: k } = e {
            if stable_receiver(ctx, map) == Some(m) && key.map_or(true, |key| same_key(key, k)) {
                out.push((e, k));
            }
        }
    };
    walk(root, &mut visit);
}

fn walk<'a>(e: &'a Expr, f: &mut dyn FnMut(&'a Expr)) {
    f(e);
    perry_hir::walker::walk_expr_children(e, &mut |c| walk(c, f));
}

/// Open the region of one `MapSet` expression. Returns the cell to close once
/// the whole expression is lowered.
pub(crate) fn open_set(ctx: &mut FnCtx<'_>, expr: &Expr) -> Option<String> {
    let Expr::MapSet { map, key, value } = expr else {
        return None;
    };
    let m = stable_receiver(ctx, map)?;
    let mut gets = Vec::new();
    find_gets(ctx, value, m, Some(key), &mut gets);
    if gets.is_empty() {
        return None;
    }
    for (g, _) in &gets {
        ctx.map_entry_fusion.gets.insert(node(g));
    }
    ctx.map_entry_fusion.sets.insert(node(expr));
    Some(open_cell(ctx, m))
}

pub(crate) fn close_set(ctx: &mut FnCtx<'_>, token: Option<String>) {
    if let Some(slot) = token {
        close_cell(ctx, &slot);
    }
}

/// Statement hook: a `Let` whose initializer reads `m.get(k)` opens a region
/// that runs to the last later statement of this list setting `m` at `k`.
pub(crate) fn before_stmt(ctx: &mut FnCtx<'_>, stmts: &[Stmt], i: usize) {
    let Stmt::Let {
        init: Some(init), ..
    } = &stmts[i]
    else {
        return;
    };
    // The first receiver/key read of the initializer that has a later set.
    let mut candidates: Vec<(u32, &Expr, &Expr)> = Vec::new();
    walk(init, &mut |e| {
        if let Expr::MapGet { map, key } = e {
            if let Some(m) = stable_receiver(ctx, map) {
                candidates.push((m, e, key));
            }
        }
    });
    for (m, get_node, get_key) in candidates {
        let mut sets: Vec<(usize, usize)> = Vec::new();
        let mut last = None;
        for (j, stmt) in stmts.iter().enumerate().skip(i + 1) {
            perry_hir::tdz_check::for_each_stmts_expr_shallow(
                std::slice::from_ref(stmt),
                &mut |e| {
                    if let Expr::MapSet { map, key, .. } = e {
                        if stable_receiver(ctx, map) == Some(m) && same_key(get_key, key) {
                            sets.push((node(e), j));
                            last = Some(j);
                        }
                    }
                },
            );
        }
        let Some(last) = last else { continue };
        ctx.map_entry_fusion.gets.insert(node(get_node));
        for (s, _) in sets {
            ctx.map_entry_fusion.sets.insert(s);
        }
        let slot = open_cell(ctx, m);
        ctx.map_entry_fusion
            .open_lists
            .push((stmts.as_ptr() as usize, last, slot));
        return;
    }
}

/// Statement hook after statement `i` of the list has been lowered.
pub(crate) fn after_stmt(ctx: &mut FnCtx<'_>, stmts: &[Stmt], i: usize) {
    let list = stmts.as_ptr() as usize;
    let mut k = 0;
    while k < ctx.map_entry_fusion.open_lists.len() {
        let (l, end, _) = &ctx.map_entry_fusion.open_lists[k];
        if *l == list && *end == i {
            let (_, _, slot) = ctx.map_entry_fusion.open_lists.remove(k);
            close_cell(ctx, &slot);
        } else {
            k += 1;
        }
    }
}

/// The list is done (normally or early): no region of it stays registered.
pub(crate) fn close_list(ctx: &mut FnCtx<'_>, stmts: &[Stmt]) {
    let list = stmts.as_ptr() as usize;
    let mut k = 0;
    while k < ctx.map_entry_fusion.open_lists.len() {
        if ctx.map_entry_fusion.open_lists[k].0 == list {
            let (_, _, slot) = ctx.map_entry_fusion.open_lists.remove(k);
            close_cell(ctx, &slot);
        } else {
            k += 1;
        }
    }
}

/// Lower a marked `get` on the generic arm: `js_map_get_entry`, then publish
/// the slot and the key bits into the receiver's cells. `None` when this node
/// is not part of a region.
pub(crate) fn lower_get(
    ctx: &mut FnCtx<'_>,
    expr: &Expr,
    m_handle: &str,
    k_box: &str,
    constructed: bool,
) -> Option<String> {
    use crate::types::{DOUBLE, I64, PTR};
    let entry = if constructed {
        "js_map_get_entry__Map"
    } else {
        "js_map_get_entry"
    };
    if !ctx.map_entry_fusion.gets.contains(&node(expr)) {
        return None;
    }
    let Expr::MapGet { map, .. } = expr else {
        return None;
    };
    let local = stable_receiver(ctx, map)?;
    let (slot_cell, key_cell) = cells_for(ctx, local);
    let out = ctx.func.alloca_entry(I64);
    let blk = ctx.block();
    let value = blk.call(
        DOUBLE,
        entry,
        &[(I64, m_handle), (DOUBLE, k_box), (PTR, &out)],
    );
    let slot = blk.load(I64, &out);
    let bits = blk.bitcast_double_to_i64(k_box);
    blk.store(I64, &slot, &slot_cell);
    blk.store(I64, &bits, &key_cell);
    Some(value)
}

/// The cells of a marked generic-arm `set`.
pub(crate) fn set_cells(ctx: &FnCtx<'_>, expr: &Expr) -> Option<SetCells> {
    if !ctx.map_entry_fusion.sets.contains(&node(expr)) {
        return None;
    }
    let Expr::MapSet { map, .. } = expr else {
        return None;
    };
    let local = stable_receiver(ctx, map)?;
    let (slot, key) = ctx.map_entry_fusion.cells.get(&local)?.clone();
    Some(SetCells { slot, key })
}

/// `set` with the entry store inline: scalar value, live slot, same key bits;
/// else the ordinary `js_map_set`. Returns the receiver handle. The receiver
/// box is checked only on the `js_map_set` arm: a live slot was published by
/// this region's `get`, which checked the same never-reassigned binding.
pub(crate) fn emit_set(
    ctx: &mut FnCtx<'_>,
    cells: &SetCells,
    m_box: &str,
    k_box: &str,
    v_box: &str,
) -> String {
    use crate::types::{DOUBLE, I1, I64};
    let blk = ctx.block();
    let slot = blk.load(I64, &cells.slot);
    let held = blk.load(I64, &cells.key);
    let kbits = blk.bitcast_double_to_i64(k_box);
    let vbits = blk.bitcast_double_to_i64(v_box);
    let live = blk.icmp_ne(I64, &slot, "0");
    let same = blk.icmp_eq(I64, &kbits, &held);
    // A plain double owes no barrier: its high 16 bits are not zero (a zero
    // top could be a raw pointer) and it is outside the tag range
    // 0x7FF9..=0x7FFF (`vbits - 0x7FF9<<48` is not below 7<<48).
    let hi = blk.lshr(I64, &vbits, "48");
    let tagged = blk.icmp_ne(I64, &hi, "0");
    let rel = blk.sub(I64, &vbits, &format!("{}", 0x7FF9_0000_0000_0000u64 as i64));
    let plain = blk.icmp_uge(I64, &rel, &format!("{}", 0x0007_0000_0000_0000u64 as i64));
    let a = blk.and(I1, &live, &same);
    let b = blk.and(I1, &tagged, &plain);
    let ok = blk.and(I1, &a, &b);
    let fast_idx = ctx.new_block("map_entry.set.fast");
    let slow_idx = ctx.new_block("map_entry.set.slow");
    let merge_idx = ctx.new_block("map_entry.set.merge");
    let fast_label = ctx.block_label(fast_idx);
    let slow_label = ctx.block_label(slow_idx);
    let merge_label = ctx.block_label(merge_idx);
    ctx.block().cond_br(&ok, &fast_label, &slow_label);

    ctx.current_block = fast_idx;
    let blk = ctx.block();
    let p = blk.inttoptr(I64, &slot);
    blk.store(I64, &vbits, &p);
    let mbits = blk.bitcast_double_to_i64(m_box);
    let fast_handle = blk.and(I64, &mbits, crate::nanbox::POINTER_MASK_I64);
    let after_fast = blk.label.clone();
    blk.br(&merge_label);

    ctx.current_block = slow_idx;
    let m_handle = super::unbox_collection_receiver(ctx, m_box, "set");
    let blk = ctx.block();
    let slow = blk.call(
        I64,
        "js_map_set",
        &[(I64, &m_handle), (DOUBLE, k_box), (DOUBLE, v_box)],
    );
    let after_slow = blk.label.clone();
    blk.br(&merge_label);

    ctx.current_block = merge_idx;
    ctx.block().phi(
        I64,
        &[
            (fast_handle.as_str(), after_fast.as_str()),
            (slow.as_str(), after_slow.as_str()),
        ],
    )
}
