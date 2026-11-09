//! PR-C1 (DESIGN-p4l1 §2.8): the reads whose past-the-end `undefined` the
//! solve withheld (`Cons::ElemUndef`), for the check-mode witness.
//!
//! A read qualifies when, at every copy of it the program holds (inlining
//! and an enclosing for-of rewrite duplicate a read; a summarized body has
//! one copy per instance), its receiver holds only program Arrays and each
//! one's `ElemUndef` verdict is "in the floor": the key is an integer in
//! `[0, lo)` of that Array's length interval `[lo, hi]`. The export is the
//! smallest such `lo`. Codegen re-tests `0 <= key < floor` at the read
//! (`expr::index_get::elem_floor`): a key the facts placed below the length
//! but that reaches it is a false proof (exit 86).
//!
//! Exported in check mode only (`FactsConfig::verify_facts`): nothing else
//! reads it, so a normal build's facts do not change.

use crate::bindings::ModuleBindings;
use crate::gen::Gen;
use crate::model::{NodeId, TokKind, TOP};

/// The floor of one copy of a read: receiver `recv`, key `idx`.
fn copy_floor(g: &Gen<'_>, recv: NodeId, idx: NodeId) -> Option<u32> {
    let m = &g.m;
    let node = &m.nodes[recv as usize];
    if node.prim != 0 || node.pts.is_empty() || node.pts.contains_tok(TOP) {
        return None;
    }
    let mut floor = u32::MAX;
    for t in node.pts.to_sorted() {
        if !matches!(m.toks[t as usize], TokKind::Obj(..)) || !m.is_arr(t) || m.is_marker(t) {
            return None;
        }
        let &ln = m.arr_len.get(&t)?;
        if m.elem_in_floor(t, idx) != Some(true) {
            return None;
        }
        let lo = u32::try_from(m.nodes[ln as usize].range.lo).ok()?;
        floor = floor.min(lo);
    }
    Some(floor)
}

/// Fill `ModuleBindings::element_floors` (module docs).
pub(crate) fn collect(g: &Gen<'_>, bindings: &mut [ModuleBindings]) {
    if g.m.aborted || !g.m.elem_undef {
        return;
    }
    // Per summarized-body instance: original node -> its copy.
    let mut insts: Vec<std::collections::HashMap<NodeId, NodeId>> = Vec::new();
    for inst in 0..g.m.sum.insts.len() as u32 {
        let mut map = std::collections::HashMap::new();
        g.m.inst_copies(inst, |orig, copy| {
            map.insert(orig, copy);
        });
        if !map.is_empty() {
            insts.push(map);
        }
    }
    // (module, offset) -> Some(floor), or None once a copy fails.
    let mut sites: std::collections::BTreeMap<(u32, u32), Option<u32>> = Default::default();
    for site in &g.element_sites {
        if site.op != crate::derive::ElementOp::Read || site.byte_offset == 0 {
            continue;
        }
        let mut f = copy_floor(g, site.recv, site.idx);
        for map in &insts {
            if !map.contains_key(&site.recv) && !map.contains_key(&site.idx) {
                continue;
            }
            let recv = map.get(&site.recv).copied().unwrap_or(site.recv);
            let idx = map.get(&site.idx).copied().unwrap_or(site.idx);
            f = match (f, copy_floor(g, recv, idx)) {
                (Some(a), Some(b)) => Some(a.min(b)),
                _ => None,
            };
        }
        let entry = sites
            .entry((site.module, site.byte_offset))
            .or_insert(Some(u32::MAX));
        *entry = match (*entry, f) {
            (Some(a), Some(b)) => Some(a.min(b)),
            _ => None,
        };
    }
    for ((mi, offset), floor) in sites {
        if let (Some(floor), Some(mb)) = (floor, bindings.get_mut(mi as usize)) {
            if floor > 0 && floor != u32::MAX {
                mb.element_floors.push((offset, floor));
            }
        }
    }
}
