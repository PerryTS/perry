//! Step 4b regions: the IR check that discards an F-body a JS-capable call reaches.

use super::*;

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
