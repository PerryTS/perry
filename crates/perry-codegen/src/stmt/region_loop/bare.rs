//! Step 4b regions: bare accesses and fact trees inside an F-body.

use super::*;

// ------------------------------------------------------- bare accesses

/// The slot of `key`, decoded from the region word AT THE USE. Every bare
/// access addresses `fields + ((word >> (32 + 6i)) & 63) * 8` from the one
/// word, the uniform form that lets LLVM gather a run of loads (§L7.6.1), and
/// one word is one register where five decoded slots are five.
pub(super) fn active_slot(ctx: &mut FnCtx<'_>, e: &Expr, r: Recv, key: &str) -> Option<String> {
    let a = ctx.region_loop_facts.last()?;
    if !a.bare.contains(&(e as *const Expr as usize)) {
        return None;
    }
    let rv = a.receivers.iter().find(|x| x.recv == r)?;
    let i = rv.keys.iter().position(|k| k == key)?;
    let word = rv.word.clone();
    let shift = (32 + SLOT_BITS * i as u32).to_string();
    let s = ctx.block().lshr(I64, &word, &shift);
    Some(ctx.block().and(I64, &s, "63"))
}

/// The receiver's handle for a bare access. Every derivation reads the
/// binding's root afresh (the object may have moved), so two accesses with
/// nothing collecting between them would each pay a root reload and a mask
/// that name the same address. When the previous derivation for this
/// receiver DOMINATES the current block and no instruction on any path from
/// it to here can collect, the same handle is still the object's address and
/// is reused; otherwise a fresh one is derived. The question is answered on
/// the emitted IR, not on the HIR, so a lowering that adds a call is seen.
pub(super) fn region_handle(ctx: &mut FnCtx<'_>, r: Recv) -> Result<String> {
    let cached = ctx
        .region_loop_facts
        .last()
        .and_then(|a| a.handles.iter().find(|h| h.0 == r).cloned());
    if let Some((_, h, b0, i0)) = cached {
        if nothing_collects_since(ctx, b0, i0) {
            return Ok(h);
        }
    }
    let recv_box = lower_recv(ctx, r)?;
    let h = handle_of(ctx, &recv_box);
    let b = ctx.current_block;
    let i = ctx.func.blocks()[b].insts().len();
    if let Some(a) = ctx.region_loop_facts.last_mut() {
        a.handles.retain(|x| x.0 != r);
        a.handles.push((r, h.clone(), b, i));
    }
    Ok(h)
}

/// Does (block `b0`, instruction `i0`) dominate the current insertion point
/// with no possibly-collecting instruction on any path between? Walks the
/// current block's predecessors backwards and gives up (answers no) at any
/// block created before `b0` — conservative, never wrong.
pub(super) fn nothing_collects_since(ctx: &FnCtx<'_>, b0: usize, i0: usize) -> bool {
    let blocks = ctx.func.blocks();
    let cur = ctx.current_block;
    let tail = |b: usize, from: usize| {
        let insts = blocks[b].insts();
        insts[from.min(insts.len())..].iter().any(inst_may_collect)
    };
    if cur == b0 {
        return !tail(b0, i0);
    }
    if cur < b0 || tail(b0, i0) || tail(cur, 0) {
        return false;
    }
    let mut by_label: HashMap<&str, usize> = HashMap::new();
    for (i, b) in blocks.iter().enumerate() {
        by_label.insert(b.label.as_str(), i);
    }
    let mut preds: HashMap<usize, Vec<usize>> = HashMap::new();
    for (i, b) in blocks.iter().enumerate() {
        if i == cur {
            continue;
        }
        for s in successors(b) {
            if let Some(&t) = by_label.get(s.as_str()) {
                preds.entry(t).or_default().push(i);
            }
        }
    }
    let mut seen: HashSet<usize> = HashSet::new();
    let mut work = vec![cur];
    while let Some(b) = work.pop() {
        let ps = match preds.get(&b) {
            Some(ps) if !ps.is_empty() => ps,
            // Reached a block with no predecessor without passing `b0`.
            _ => return false,
        };
        for &p in ps {
            if p == b0 {
                continue;
            }
            if p < b0 || p == cur {
                return false;
            }
            if seen.insert(p) {
                if tail(p, 0) {
                    return false;
                }
                work.push(p);
            }
        }
    }
    true
}

/// Can this instruction run a collection (and so move the receiver)?
pub(super) fn inst_may_collect(inst: &crate::inst::LlInst) -> bool {
    use crate::gc_call_effects::{classify_direct_callee, GcCallEffect};
    use crate::inst::LlInst;
    let named = |callee: &str| -> bool {
        !(callee.starts_with("llvm.")
            || callee == "js_recv_route_note"
            || matches!(classify_direct_callee(callee), GcCallEffect::CannotCollect)
            || crate::root_reload::is_non_collecting(callee))
    };
    match inst {
        LlInst::Call { callee, .. } => named(callee),
        LlInst::CallIndirect { .. } => true,
        LlInst::Raw(s) => {
            let t = s.trim_start();
            if !(t.contains("call ") || t.starts_with("invoke") || t.contains(" invoke ")) {
                return false;
            }
            if t.contains(" asm ") {
                return false;
            }
            match t.find('@') {
                Some(at) => {
                    let name: String = t[at + 1..]
                        .chars()
                        .take_while(|c| c.is_alphanumeric() || *c == '_' || *c == '.' || *c == '$')
                        .collect();
                    named(&name)
                }
                None => true,
            }
        }
        _ => false,
    }
}

/// A route-census note (`PERRY_RECV_ROUTE_COUNT=1` builds only; nothing is
/// emitted otherwise).
pub(super) fn note(ctx: &mut FnCtx<'_>, route: Route) {
    crate::expr::receiver_range::emit_route_note(ctx.block(), route);
}

pub(super) fn note_emitted(ctx: &mut FnCtx<'_>) {
    note(ctx, Route::RloopBare);
    let b = ctx.current_block;
    let i = ctx.func.blocks()[b].insts().len();
    if let Some(a) = ctx.region_loop_facts.last_mut() {
        a.emitted.push((b, i));
    }
}

/// The address a bare READ loads. In an all-inline copy (and for every store,
/// whose key the runtime publishes only when inline) it is the inline slot.
/// In a spill copy the key's field says where the value lives: `< 32` is an
/// inline slot, `32 + i` is spill index `i`, reached exactly as the S5 hit
/// path reaches it — `ObjectHeader.meta`, `ObjectMeta.spill`, the element —
/// on the ShapeId's word alone (every carrier of the shape has that storage).
pub(super) fn bare_read_ptr(ctx: &mut FnCtx<'_>, handle: &str, slot: &str) -> String {
    let spill = ctx.region_loop_facts.last().is_some_and(|a| a.spill);
    if !spill {
        return slot_ptr(ctx, handle, slot);
    }
    let is_spill = ctx.block().icmp_uge(I64, slot, "32");
    let in_b = ctx.new_block("rloop.slot.inline");
    let sp_b = ctx.new_block("rloop.slot.spill");
    let jn_b = ctx.new_block("rloop.slot.join");
    let (in_l, sp_l, jn_l) = (
        ctx.block_label(in_b),
        ctx.block_label(sp_b),
        ctx.block_label(jn_b),
    );
    ctx.block().cond_br(&is_spill, &sp_l, &in_l);
    ctx.current_block = in_b;
    let p_in = slot_ptr(ctx, handle, slot);
    let in_end = ctx.block().label.clone();
    ctx.block().br(&jn_l);
    ctx.current_block = sp_b;
    let ilp32 = crate::target_layout::target_is_ilp32(ctx.target_triple);
    let meta_off = crate::target_layout::object_meta_slot_offset_bytes(ctx.target_triple);
    let meta_addr = ctx.block().add(I64, handle, &meta_off.to_string());
    let meta_slot = ctx.block().inttoptr(I64, &meta_addr);
    let meta = if ilp32 {
        let narrow = ctx.block().load(I32, &meta_slot);
        ctx.block().zext(I32, &narrow, I64)
    } else {
        ctx.block().load(I64, &meta_slot)
    };
    let meta_ptr = ctx.block().inttoptr(I64, &meta);
    let spill_slot = ctx.block().gep(
        I8,
        &meta_ptr,
        &[(
            I64,
            &crate::target_layout::OBJECT_META_SPILL_OFFSET_BYTES.to_string(),
        )],
    );
    let buf = ctx.block().load(I64, &spill_slot);
    let buf_ptr = ctx.block().inttoptr(I64, &buf);
    let elems = ctx.block().gep(
        I8,
        &buf_ptr,
        &[(
            I64,
            &crate::target_layout::ARRAY_HEADER_SIZE_BYTES.to_string(),
        )],
    );
    let index = ctx.block().sub(I64, slot, "32");
    let p_sp = ctx.block().gep(DOUBLE, &elems, &[(I64, &index)]);
    let sp_end = ctx.block().label.clone();
    ctx.block().br(&jn_l);
    ctx.current_block = jn_b;
    ctx.block().phi(
        crate::types::PTR,
        &[
            (p_in.as_str(), in_end.as_str()),
            (p_sp.as_str(), sp_end.as_str()),
        ],
    )
}

pub(super) fn slot_ptr(ctx: &mut FnCtx<'_>, handle: &str, slot: &str) -> String {
    let header = crate::target_layout::object_header_size_bytes(ctx.target_triple) as i64;
    let base = ctx.block().inttoptr(I64, handle);
    let fields = ctx.block().gep(I8, &base, &[(I64, &header.to_string())]);
    ctx.block().gep(DOUBLE, &fields, &[(I64, slot)])
}

/// `property_get::lower`'s hook: a planned-bare read in F-body.
pub(crate) fn try_lower_bare_get(ctx: &mut FnCtx<'_>, e: &Expr) -> Result<Option<String>> {
    if ctx.region_loop_facts.is_empty() {
        return Ok(None);
    }
    let Expr::PropertyGet {
        object, property, ..
    } = e
    else {
        return Ok(None);
    };
    let Some(r) = Recv::of(object) else {
        return Ok(None);
    };
    let Some(slot) = active_slot(ctx, e, r, property) else {
        return Ok(None);
    };
    let h = region_handle(ctx, r)?;
    let p = bare_read_ptr(ctx, &h, &slot);
    note_emitted(ctx);
    let v = ctx.block().load(DOUBLE, &p);
    stat(2, 1);
    Ok(Some(v))
}

/// The `PutValueSet` hook: a planned-bare store in F-body. The value is
/// evaluated first (the target is a binding read, so evaluating it after the
/// RHS is unobservable), then the receiver's CURRENT address is read from its
/// root, then the store and exactly the store IC's GC obligations.
pub(crate) fn try_lower_bare_put(
    ctx: &mut FnCtx<'_>,
    e: &Expr,
    target: &Expr,
    key: &Expr,
    value: &Expr,
) -> Result<Option<String>> {
    if ctx.region_loop_facts.is_empty() {
        return Ok(None);
    }
    let (Some(r), Expr::String(k)) = (Recv::of(target), key) else {
        return Ok(None);
    };
    let Some(slot) = active_slot(ctx, e, r, k) else {
        return Ok(None);
    };
    // A value the compiler proves a canonical raw double is pointer-free and
    // raw-f64-compatible with every slot: the store owes the GC nothing (the
    // store IC's own "plain double: nothing" case, decided statically).
    let raw_double = crate::type_analysis::expr_produces_canonical_raw_f64(ctx, value);
    let val_double = lower_expr(ctx, value)?;
    let val_bits = ctx.block().bitcast_double_to_i64(&val_double);
    let h = region_handle(ctx, r)?;
    let p = slot_ptr(ctx, &h, &slot);
    note_emitted(ctx);
    if raw_double {
        // GC_STORE_AUDIT(POINTER_FREE): a proven canonical raw double carries no pointer.
        ctx.block().store(DOUBLE, &val_double, &p);
        stat(3, 1);
        return Ok(Some(val_double));
    }
    let reserved = field_i16(ctx, &h, -6);
    // GC_STORE_AUDIT(BARRIERED): the obligations follow, from the stored bits.
    ctx.block().store(DOUBLE, &val_double, &p);
    crate::expr::put_value_store_ic::emit_static_store_ic_bookkeeping(
        ctx,
        &h,
        &slot,
        &p,
        &reserved,
        &val_double,
        &val_bits,
        "put.pic",
    );
    stat(3, 1);
    Ok(Some(val_double))
}

/// `region_read_run`'s hook inside an F-body: a planned fact tree.
pub(crate) fn try_lower_fact_add_tree(ctx: &mut FnCtx<'_>, e: &Expr) -> Result<Option<String>> {
    let Some(a) = ctx.region_loop_facts.last() else {
        return Ok(None);
    };
    if !a.trees.contains(&(e as *const Expr as usize)) {
        return Ok(None);
    }
    let covered = |_: Recv, _: &str| true;
    let Some((r, _)) = fact_tree_leaves(e, &covered) else {
        return Ok(None);
    };
    let dirty_slot = a.dirty_slot.clone();
    fn leaves<'e>(e: &'e Expr, out: &mut Vec<&'e Expr>) {
        if let Expr::Binary {
            op: BinaryOp::Add,
            left,
            right,
        } = e
        {
            leaves(left, out);
            leaves(right, out);
        } else {
            out.push(e);
        }
    }
    let mut all = Vec::new();
    leaves(e, &mut all);
    // Effect-free leaves first, then the receiver's handle and the loads, so
    // nothing that could allocate sits between the handle and its loads.
    let mut values: Vec<Option<String>> = Vec::with_capacity(all.len());
    let mut needs: Vec<bool> = Vec::with_capacity(all.len());
    for l in &all {
        if matches!(l, Expr::PropertyGet { .. }) {
            values.push(None);
            needs.push(true);
        } else {
            values.push(Some(lower_expr(ctx, l)?));
            needs.push(!crate::type_analysis::expr_produces_canonical_raw_f64(
                ctx, l,
            ));
        }
    }
    let h = region_handle(ctx, r)?;
    for (i, l) in all.iter().enumerate() {
        if let Expr::PropertyGet { property, .. } = l {
            let slot = active_slot(ctx, l, r, property).expect("planned with its tree");
            let p = bare_read_ptr(ctx, &h, &slot);
            note_emitted(ctx);
            values[i] = Some(ctx.block().load(DOUBLE, &p));
            stat(2, 1);
        }
    }
    let values: Vec<String> = values.into_iter().map(|v| v.expect("lowered")).collect();
    let gen_i = ctx.new_block("rloop.tree.generic");
    let merge_i = ctx.new_block("rloop.tree.merge");
    let gen_l = ctx.block_label(gen_i);
    let merge_l = ctx.block_label(merge_i);
    fn fold(ctx: &mut FnCtx<'_>, e: &Expr, values: &[String], next: &mut usize) -> String {
        if let Expr::Binary {
            op: BinaryOp::Add,
            left,
            right,
        } = e
        {
            let l = fold(ctx, left, values, next);
            let r = fold(ctx, right, values, next);
            return ctx.block().fadd(&l, &r);
        }
        let v = values[*next].clone();
        *next += 1;
        v
    }
    // The fold runs first and is verified by its RESULT: every non-number a
    // leaf can hold is a NaN-box (a tag in the positive NaN suffix, an INT32
    // box included), and IEEE addition propagates a NaN from any operand, so a
    // non-NaN sum proves every leaf was a number and `+` was numeric addition
    // throughout. A NaN sum (a boxed leaf, or a genuine NaN operand) takes the
    // generic arm, which recomputes the tree from scratch: the reads are own
    // data slots, the other leaves are effect-free, so nothing is observed
    // twice. One ordered compare replaces a per-leaf tag test.
    let fast = fold(ctx, e, &values, &mut 0);
    let fast_end = ctx.block().label.clone();
    if needs.iter().any(|n| *n) {
        let ok = ctx.block().fcmp("ord", &fast, "0.0");
        ctx.block().cond_br(&ok, &merge_l, &gen_l);
    } else {
        ctx.block().br(&merge_l);
    }
    // The generic arm: the tree in source order through today's lowering,
    // with the facts masked (it may run JS between its reads), then the flag.
    ctx.current_block = gen_i;
    ctx.region_loop_facts.push(Active {
        receivers: Vec::new(),
        bare: HashSet::new(),
        trees: HashSet::new(),
        dirty_slot: None,
        emitted: Vec::new(),
        handles: Vec::new(),
        spill: false,
    });
    let slow = lower_expr(ctx, e);
    ctx.region_loop_facts.pop();
    let slow = slow?;
    if let Some(d) = &dirty_slot {
        ctx.block().store(I1, "true", d);
    }
    let slow_end = ctx.block().label.clone();
    ctx.block().br(&merge_l);
    ctx.current_block = merge_i;
    Ok(Some(
        ctx.block()
            .phi(DOUBLE, &[(&fast, &fast_end), (&slow, &slow_end)]),
    ))
}
