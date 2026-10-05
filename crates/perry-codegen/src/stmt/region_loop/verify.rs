//! Step 4b regions: the IR check that discards an F-body a JS-capable call reaches.

use super::*;

/// Judge the emitted clone, after representation-specific lowering, using
/// the same collection effects as bare-address verification. Unknown calls
/// refuse the proof, including calls in conservatively unreachable blocks.
pub(super) fn cannot_collect(
    ctx: &FnCtx<'_>,
    entry: usize,
    scan_start: usize,
    scan_end: usize,
) -> bool {
    std::iter::once(entry).chain(scan_start..scan_end).all(|b| {
        ctx.func.blocks()[b]
            .insts()
            .iter()
            .all(|inst| !bare::inst_may_collect(inst))
    })
}

// ---------------------------------------------------------------- verify

/// Callees that cannot run JavaScript (they may allocate or collect — the
/// receiver is re-derived at every use — but they cannot reshape anything).
pub(super) fn cannot_run_js(callee: &str) -> bool {
    use crate::gc_call_effects::{classify_direct_callee, GcCallEffect};
    if callee.starts_with("llvm.") {
        return true;
    }
    if matches!(
        classify_direct_callee(callee),
        GcCallEffect::CannotCollect | GcCallEffect::AllocNoReentry
    ) {
        return true;
    }
    crate::root_reload::is_non_collecting(callee)
        || matches!(
            callee,
            "js_gc_loop_safepoint"
                | "js_gc_note_slot_layout"
                | "js_gc_note_slot_layout_aware"
                | "js_write_barrier_slot_validated_parent"
                | "js_write_barrier_slot"
                | "js_write_barrier_root_nanbox"
                | "js_write_barrier_root_heap_word"
                | "js_string_addref_if_heap_string"
                | "js_region_loop_prime"
                | "js_recv_route_note"
        )
}

pub(super) fn inst_may_run_js(inst: &crate::inst::LlInst) -> bool {
    use crate::inst::LlInst;
    match inst {
        LlInst::Call { callee, .. } => !cannot_run_js(callee),
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
                    !cannot_run_js(&name)
                }
                None => true,
            }
        }
        _ => false,
    }
}

pub(super) fn successors(block: &crate::block::LlBlock) -> Vec<String> {
    use crate::inst::LlInst;
    match block.insts().last() {
        Some(LlInst::Br { label }) => vec![label.clone()],
        Some(LlInst::CondBr { t, f, .. }) => vec![t.clone(), f.clone()],
        Some(LlInst::Raw(s)) => s
            .split("label %")
            .skip(1)
            .map(|x| {
                x.chars()
                    .take_while(|c| c.is_alphanumeric() || *c == '_' || *c == '.' || *c == '$')
                    .collect()
            })
            .collect(),
        _ => Vec::new(),
    }
}

/// Recompute fresh/stale over the EMITTED F-body blocks and require every
/// bare access to be reached only fresh.
pub(super) fn verify(
    ctx: &FnCtx<'_>,
    entry: usize,
    scan_start: usize,
    scan_end: usize,
    emitted: &[(usize, usize)],
) -> bool {
    let blocks = ctx.func.blocks();
    let in_f = |b: usize| b == entry || (scan_start..scan_end).contains(&b);
    let mut by_label: HashMap<&str, usize> = HashMap::new();
    for b in (scan_start..scan_end).chain(std::iter::once(entry)) {
        by_label.insert(blocks[b].label.as_str(), b);
    }
    // stale_in[b]: some path from the F entry reaches b after a JS-capable call.
    let mut stale_in: HashMap<usize, bool> = HashMap::new();
    let mut reached: HashSet<usize> = HashSet::new();
    let mut work = vec![(entry, false)];
    while let Some((b, stale)) = work.pop() {
        let seen = reached.contains(&b);
        let prev = stale_in.get(&b).copied().unwrap_or(false);
        if seen && (prev || !stale) {
            continue;
        }
        reached.insert(b);
        stale_in.insert(b, prev || stale);
        let mut s = prev || stale;
        for inst in blocks[b].insts() {
            if inst_may_run_js(inst) {
                s = true;
            }
        }
        for succ in successors(&blocks[b]) {
            if let Some(&nb) = by_label.get(succ.as_str()) {
                if in_f(nb) {
                    work.push((nb, s));
                }
            }
        }
    }
    for &(b, i) in emitted {
        let mut s = match stale_in.get(&b) {
            Some(v) => *v,
            None => continue,
        };
        for inst in &blocks[b].insts()[..i.min(blocks[b].insts().len())] {
            if inst_may_run_js(inst) {
                s = true;
            }
        }
        if s {
            return false;
        }
    }
    true
}

/// Check the complete F path, including instructions after its last bare
/// access. A JS-capable operation may reshape a receiver before the next
/// iteration; it therefore needs an unconditional recheck, a dirty-flag
/// store on that path, or an exit from the region. The earlier bare-access
/// check alone cannot establish this back-edge obligation.
pub(super) fn verify_exit_effects(
    ctx: &FnCtx<'_>,
    entry: usize,
    scan_start: usize,
    scan_end: usize,
    recheck: Recheck,
    dirty_slot: Option<&str>,
    valid_slot: Option<&str>,
) -> bool {
    let Some(valid_slot) = valid_slot else {
        return true; // A body region does not carry its facts to an iteration.
    };
    if recheck == Recheck::Always {
        return true;
    }
    use crate::inst::LlInst;
    const JS: u8 = 1;
    const DIRTY: u8 = 2;
    const LEFT: u8 = 4;
    let blocks = ctx.func.blocks();
    let in_f = |b: usize| b == entry || (scan_start..scan_end).contains(&b);
    let mut by_label: HashMap<&str, usize> = HashMap::new();
    for b in (scan_start..scan_end).chain(std::iter::once(entry)) {
        by_label.insert(blocks[b].label.as_str(), b);
    }
    let mut state: HashMap<usize, u8> = HashMap::new();
    let mut work = vec![(entry, 1u8)];
    let mut exits = 0u8;
    while let Some((b, incoming)) = work.pop() {
        let previous = state.get(&b).copied().unwrap_or(0);
        if previous | incoming == previous {
            continue;
        }
        let mut mask = previous | incoming;
        state.insert(b, mask);
        for inst in blocks[b].insts() {
            let mut next = 0u8;
            for bits in 0..8u8 {
                if mask & (1 << bits) == 0 {
                    continue;
                }
                let mut bits = bits;
                if inst_may_run_js(inst) {
                    bits |= JS;
                }
                if let LlInst::Store { val, ptr, .. } = inst {
                    if dirty_slot == Some(ptr.as_str()) {
                        if val == "true" {
                            bits |= DIRTY;
                        } else if val == "false" {
                            bits &= !DIRTY;
                        } else {
                            // An unknown write cannot prove coverage.
                            bits &= !DIRTY;
                        }
                    }
                    if ptr == valid_slot {
                        if val == "false" {
                            bits |= LEFT;
                        } else {
                            // Re-entering the region revokes the exit proof.
                            bits &= !LEFT;
                        }
                    }
                }
                next |= 1 << bits;
            }
            mask = next;
        }
        let successors = successors(&blocks[b]);
        if successors.is_empty() {
            exits |= mask;
        }
        for succ in successors {
            match by_label.get(succ.as_str()) {
                Some(&next) if in_f(next) => work.push((next, mask)),
                _ => exits |= mask,
            }
        }
    }
    (0..8u8).all(|bits| {
        exits & (1 << bits) == 0
            || bits & JS == 0
            || bits & LEFT != 0
            || (recheck == Recheck::Dirty && bits & DIRTY != 0)
    })
}
