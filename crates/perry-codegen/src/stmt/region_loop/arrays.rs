//! Step 4b regions, array slice S3: array facts inside a LOOP region.
//!
//! An array binding the loop cannot reassign, read as `xs[<index>]` where
//! the index has a static range `[0, c]` (`e & c`, or a literal), is a region
//! receiver of its own. The preheader checks, once:
//!
//! ```text
//! heap band -> guard word (ordinary array, not forwarded, no element
//! descriptors) -> prototype facts (protector byte + custom-proto bit)
//! -> c <u capacity
//! ```
//!
//! and derives the element base from the same header into a slot. An element
//! read in F-body is then `load base; load [base + 8 idx]; hole ? undefined`:
//! the bounds hold because `c < capacity`, a hole reads `undefined` because
//! the prototype facts hold, and a slot in `[length, capacity)` is a hole
//! (the S0 invariant). The read runs no JavaScript, so it is not a staleness
//! point for the region's other facts.
//!
//! # What keeps the facts true
//!
//! Anything that can run JavaScript can grow, reshape or re-prototype the
//! array: the planner stales every fact there, exactly as for object
//! receivers, and [`verify`](super::verify) checks it on the emitted IR.
//!
//! # GC
//!
//! The base is an address, and a collection can move the array. The base
//! slot is re-derived from the binding's root on the loop's poll arm
//! ([`emit_poll_refresh`]), at every re-check and in the preheader; nothing
//! else inside F-body may collect before a read ([`verify_arrays`]), and an
//! F-body path that collects must leave through a re-check (the dirty flag or
//! an every-iteration re-check), so the next iteration never reads through a
//! stale base.

use super::*;
use crate::inst::LlInst;

/// A static index bound above this is not a region fact (the guard compares
/// it against an `i32` capacity).
const MAX_STATIC_INDEX: i64 = 1 << 24;

/// One array receiver of a loop region.
#[derive(Clone)]
pub(crate) struct ArrayRecv {
    pub(super) recv: Recv,
    /// Every planned read's index lies in `[0, max_index]`.
    pub(super) max_index: u32,
    /// `i64` alloca: the element base, valid while the region's `valid` flag is.
    pub(super) base_slot: String,
}

/// `[0, c]` when `e` is `x & c` / `c & x` (`c >= 0`, the result of ToInt32
/// masking) or the literal `c`.
pub(super) fn static_index_max(e: &Expr) -> Option<u32> {
    fn lit(e: &Expr) -> Option<i64> {
        match e {
            Expr::Integer(c) => Some(*c),
            Expr::Number(n) if n.fract() == 0.0 && n.abs() <= MAX_STATIC_INDEX as f64 => {
                Some(*n as i64)
            }
            _ => None,
        }
    }
    let c = match e {
        Expr::Binary {
            op: BinaryOp::BitAnd,
            left,
            right,
        } => lit(right).or_else(|| lit(left))?,
        _ => lit(e)?,
    };
    (0..=MAX_STATIC_INDEX).contains(&c).then_some(c as u32)
}

/// Loop control that cannot collect: numeric compares and arithmetic over
/// locals and literals. A collection there would move an array between the
/// poll (which refreshes the base) and F-body.
fn quiet(ctx: &FnCtx<'_>, e: &Expr) -> bool {
    let num = |x: &Expr| crate::type_analysis::is_numeric_expr(ctx, x);
    match e {
        Expr::LocalGet(_) | Expr::Integer(_) | Expr::Number(_) | Expr::Bool(_) => true,
        Expr::Compare { left, right, .. } | Expr::Binary { left, right, .. } => {
            num(left) && num(right) && quiet(ctx, left) && quiet(ctx, right)
        }
        Expr::Update { id, .. } => num(&Expr::LocalGet(*id)),
        Expr::LocalSet(_, v) => num(v) && quiet(ctx, v),
        _ => false,
    }
}

/// The array candidates of a loop region: eligible bindings the loop never
/// assigns and never reads by static key, with the largest static index of
/// their reads.
pub(super) fn candidates(
    ctx: &FnCtx<'_>,
    cond: Option<&Expr>,
    body: &[Stmt],
    update: Option<&Expr>,
) -> HashMap<Recv, u32> {
    let mut out: HashMap<Recv, u32> = HashMap::new();
    if cond.is_some_and(|c| !quiet(ctx, c)) || update.is_some_and(|u| !quiet(ctx, u)) {
        return out;
    }
    let mut reads: Vec<(u32, u32)> = Vec::new();
    fn e_walk(e: &Expr, out: &mut Vec<(u32, u32)>) {
        if let Expr::IndexGet { object, index } = e {
            if let (Expr::LocalGet(id), Some(c)) = (object.as_ref(), static_index_max(index)) {
                out.push((*id, c));
            }
        }
        perry_hir::walker::walk_expr_children(e, &mut |c| e_walk(c, out));
    }
    fn s_walk(s: &Stmt, out: &mut Vec<(u32, u32)>) {
        match s {
            Stmt::Let { init: Some(e), .. }
            | Stmt::Expr(e)
            | Stmt::Throw(e)
            | Stmt::Return(Some(e)) => e_walk(e, out),
            Stmt::If {
                condition,
                then_branch,
                else_branch,
            } => {
                e_walk(condition, out);
                then_branch.iter().for_each(|s| s_walk(s, out));
                if let Some(b) = else_branch {
                    b.iter().for_each(|s| s_walk(s, out));
                }
            }
            Stmt::While { condition, body } | Stmt::DoWhile { body, condition } => {
                e_walk(condition, out);
                body.iter().for_each(|s| s_walk(s, out));
            }
            Stmt::For {
                init,
                condition,
                update,
                body,
            } => {
                if let Some(i) = init {
                    s_walk(i, out);
                }
                condition.iter().for_each(|c| e_walk(c, out));
                update.iter().for_each(|u| e_walk(u, out));
                body.iter().for_each(|s| s_walk(s, out));
            }
            Stmt::Switch {
                discriminant,
                cases,
            } => {
                e_walk(discriminant, out);
                for c in cases {
                    c.test.iter().for_each(|t| e_walk(t, out));
                    c.body.iter().for_each(|s| s_walk(s, out));
                }
            }
            _ => {}
        }
    }
    body.iter().for_each(|s| s_walk(s, &mut reads));
    if reads.is_empty() {
        return out;
    }
    let mut extra: Vec<&Expr> = Vec::new();
    extra.extend(cond);
    extra.extend(update);
    let written = assigned(body, &extra);
    let keyed: HashSet<Recv> = accesses(body)
        .into_iter()
        .map(|(r, _, _, _, _)| r)
        .collect();
    for (id, c) in reads {
        let r = Recv::Local(id);
        if written.contains(&id) || keyed.contains(&r) || !receiver_eligible(ctx, r) {
            continue;
        }
        let m = out.entry(r).or_insert(0);
        *m = (*m).max(c);
    }
    out
}

/// The preheader / re-check guard of one array receiver; stores the base.
pub(super) fn emit_guard(ctx: &mut FnCtx<'_>, a: &ArrayRecv) -> Result<String> {
    let recv_box = lower_recv(ctx, a.recv)?;
    Ok(crate::expr::emit_array_region_guard(
        ctx,
        &recv_box,
        a.max_index,
        &a.base_slot,
    ))
}

/// The `IndexGet` hook: a planned-bare element read in F-body.
pub(crate) fn try_lower_bare_index_get(ctx: &mut FnCtx<'_>, e: &Expr) -> Result<Option<String>> {
    let Some(a) = ctx.region_loop_facts.last() else {
        return Ok(None);
    };
    if a.arrays.is_empty() || !a.bare.contains(&(e as *const Expr as usize)) {
        return Ok(None);
    }
    let Expr::IndexGet { object, index } = e else {
        return Ok(None);
    };
    let Some(r) = Recv::of(object) else {
        return Ok(None);
    };
    let Some(ar) = a.arrays.iter().find(|x| x.recv == r).cloned() else {
        return Ok(None);
    };
    // The planner proved `index` in `[0, max_index]` and JS-free.
    let idx = lower_expr(ctx, index)?;
    let b = ctx.current_block;
    let i = ctx.func.blocks()[b].insts().len();
    if let Some(a) = ctx.region_loop_facts.last_mut() {
        a.emitted_arr.push((b, i));
    }
    let blk = ctx.block();
    let idx = blk.fptosi(DOUBLE, &idx, I64);
    let base = blk.load(I64, &ar.base_slot);
    let off = blk.shl(I64, &idx, "3");
    let addr = blk.add(I64, &base, &off);
    let ptr = blk.inttoptr(I64, &addr);
    let raw = blk.load(I64, &ptr);
    let hole = blk.icmp_eq(I64, &raw, crate::nanbox::TAG_HOLE_I64);
    let v = blk.select(I1, &hole, I64, crate::nanbox::TAG_UNDEFINED_I64, &raw);
    let v = blk.bitcast_i64_to_double(&v);
    stat(2, 1);
    Ok(Some(v))
}

/// Is `e` a planned-bare element read of the active region? (The number
/// context's own tiers step aside for it.)
pub(crate) fn is_bare_index_get(ctx: &FnCtx<'_>, e: &Expr) -> bool {
    ctx.region_loop_facts
        .last()
        .is_some_and(|a| !a.arrays.is_empty() && a.bare.contains(&(e as *const Expr as usize)))
}

/// The loop poll's arm: a collection may have moved every region array, so
/// each base whose region is valid is re-derived from the binding's root.
pub(crate) fn emit_poll_refresh(ctx: &mut FnCtx<'_>) -> Result<()> {
    let recipes: Vec<(String, ArrayRecv)> = ctx
        .region_loops
        .iter()
        .filter_map(|p| p.valid_slot.clone().map(|v| (v, p.arrays.clone())))
        .flat_map(|(v, arrs)| arrs.into_iter().map(move |a| (v.clone(), a)))
        .collect();
    for (valid_slot, a) in recipes {
        let go = ctx.new_block("rloop.arr.refresh");
        let done = ctx.new_block("rloop.arr.refreshed");
        let go_l = ctx.block_label(go);
        let done_l = ctx.block_label(done);
        let v = ctx.block().load(I1, &valid_slot);
        ctx.block().cond_br(&v, &go_l, &done_l);
        ctx.current_block = go;
        let recv_box = lower_recv(ctx, a.recv)?;
        let h = handle_of(ctx, &recv_box);
        let blk = ctx.block();
        let base = blk.array_elements_addr(&h);
        blk.store(I64, &base, &a.base_slot);
        blk.br(&done_l);
        ctx.current_block = done;
    }
    Ok(())
}

/// The array half of the F-body check, on the emitted IR: no instruction
/// that may collect lies on a path from the F entry to a bare element read,
/// and an F-body exit reached after one is covered by the next iteration's
/// re-check (every iteration, or the dirty flag set on that path).
pub(super) fn verify_arrays(
    ctx: &FnCtx<'_>,
    entry: usize,
    scan_start: usize,
    scan_end: usize,
    emitted: &[(usize, usize)],
    recheck: Recheck,
    dirty_slot: Option<&str>,
    valid_slot: Option<&str>,
) -> bool {
    if emitted.is_empty() {
        return true;
    }
    let blocks = ctx.func.blocks();
    let in_f = |b: usize| b == entry || (scan_start..scan_end).contains(&b);
    let mut by_label: HashMap<&str, usize> = HashMap::new();
    for b in (scan_start..scan_end).chain(std::iter::once(entry)) {
        by_label.insert(blocks[b].label.as_str(), b);
    }
    let sets_dirty = |inst: &LlInst| match (inst, dirty_slot) {
        (LlInst::Store { val, ptr, .. }, Some(d)) => ptr == d && val == "true",
        _ => false,
    };
    // A nested body region's G-tail leaves the loop region: no later
    // iteration runs F-body on these facts.
    let leaves = |inst: &LlInst| match (inst, valid_slot) {
        (LlInst::Store { val, ptr, .. }, Some(v)) => ptr == v && val == "false",
        _ => false,
    };
    // State per block entry: the set of (collected, dirty-set, left)
    // combinations some path reaches it with, as an 8-bit mask over
    // `c | d << 1 | l << 2`.
    const C: u8 = 1;
    const D: u8 = 2;
    const L: u8 = 4;
    let diag = std::env::var("PERRY_REGION_DIAG").as_deref() == Ok("4");
    let step = |mask: u8, inst: &LlInst| -> u8 {
        if diag && bare::inst_may_collect(inst) {
            let what = match inst {
                LlInst::Call { callee, .. } => callee.clone(),
                LlInst::Raw(s) => s.clone(),
                _ => "indirect".to_string(),
            };
            eprintln!("[perry region] array verify: may collect: {what}");
        }
        let mut out = 0u8;
        for combo in 0..8u8 {
            if mask & (1 << combo) == 0 {
                continue;
            }
            let mut c = combo;
            if bare::inst_may_collect(inst) {
                c |= C;
            }
            if sets_dirty(inst) {
                c |= D;
            }
            if leaves(inst) {
                c |= L;
            }
            out |= 1 << c;
        }
        out
    };
    // Any combination with C, for a read; for an exit, the combinations
    // with C that neither left the region nor (Dirty) set the flag.
    let collected = |mask: u8| (0..8u8).any(|c| c & C != 0 && mask & (1 << c) != 0);
    let collected_clean = |mask: u8| mask & (1 << C) != 0;
    let collected_staying =
        |mask: u8| (0..8u8).any(|c| c & C != 0 && c & L == 0 && mask & (1 << c) != 0);
    let mut state: HashMap<usize, u8> = HashMap::new();
    let mut work = vec![(entry, 1u8)];
    let mut exit_mask = 0u8;
    while let Some((b, m)) = work.pop() {
        let prev = state.get(&b).copied().unwrap_or(0);
        if prev | m == prev {
            continue;
        }
        let m = prev | m;
        state.insert(b, m);
        let mut cur = m;
        for inst in blocks[b].insts() {
            cur = step(cur, inst);
        }
        for succ in successors(&blocks[b]) {
            match by_label.get(succ.as_str()) {
                Some(&nb) if in_f(nb) => work.push((nb, cur)),
                _ => exit_mask |= cur,
            }
        }
    }
    for &(b, i) in emitted {
        let Some(&m) = state.get(&b) else { continue };
        let mut cur = m;
        for inst in &blocks[b].insts()[..i.min(blocks[b].insts().len())] {
            cur = step(cur, inst);
        }
        if collected(cur) {
            if diag {
                eprintln!("[perry region] array verify: read after a collection");
            }
            return false;
        }
    }
    if diag {
        eprintln!("[perry region] array verify: recheck={recheck:?} exit_mask={exit_mask:#x}");
    }
    match recheck {
        Recheck::Always => true,
        Recheck::Dirty => !collected_clean(exit_mask),
        Recheck::None => !collected_staying(exit_mask),
    }
}

/// A lowering that re-lowers a CLONE of planned HIR (a `let` redeclared
/// through `LocalSet`, #1803) keeps the plan: every clone node whose original
/// is a planned bare access or fact tree of the active region is planned too,
/// until [`unalias_clone`]. The verifier still judges the emitted IR.
pub(crate) fn alias_clone(ctx: &mut FnCtx<'_>, orig: &Expr, clone: &Expr) -> Vec<usize> {
    fn walk(orig: &Expr, clone: &Expr, a: &mut Active, out: &mut Vec<usize>) {
        let (o, c) = (orig as *const Expr as usize, clone as *const Expr as usize);
        if a.bare.contains(&o) && a.bare.insert(c) {
            out.push(c);
        }
        if a.trees.contains(&o) && a.trees.insert(c) {
            out.push(c);
        }
        let mut ok: Vec<&Expr> = Vec::new();
        let mut ck: Vec<&Expr> = Vec::new();
        perry_hir::walker::walk_expr_children(orig, &mut |x| ok.push(x));
        perry_hir::walker::walk_expr_children(clone, &mut |x| ck.push(x));
        if ok.len() == ck.len() {
            for (x, y) in ok.into_iter().zip(ck) {
                walk(x, y, a, out);
            }
        }
    }
    let mut out = Vec::new();
    if let Some(a) = ctx.region_loop_facts.last_mut() {
        walk(orig, clone, a, &mut out);
    }
    out
}

/// Drop the aliases [`alias_clone`] added (the clone is about to be freed,
/// and its addresses may be reused).
pub(crate) fn unalias_clone(ctx: &mut FnCtx<'_>, added: Vec<usize>) {
    if let Some(a) = ctx.region_loop_facts.last_mut() {
        for p in added {
            a.bare.remove(&p);
            a.trees.remove(&p);
        }
    }
}
