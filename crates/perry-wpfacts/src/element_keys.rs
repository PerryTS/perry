//! PR-C2 (DESIGN-p4l1 §4.2): the integer interval of an element read's key,
//! for the rep-keyed element tier.
//!
//! An element read `a[k]` is in the floor of its receiver's `Rep::Arr` when
//! `k` is an integer in `[0, floor)`. The receiver's rep is the binding's
//! (`BindingFacts::rep`); this is the other half, the key at the site: the
//! hull of the key's interval over every copy of the read the program holds
//! (inlining and an enclosing for-of rewrite duplicate a read; a summarized
//! body has one copy per instance), in the form `call_arg_ranges` has for a
//! call's arguments. A copy whose key may be anything but an integer Number
//! drops the site. Reads that are not copies of one another may share an
//! offset (in `a[i][0]` both member expressions start at `a`): the hull
//! then covers both keys, which says less.
//!
//! Exported with `Model::array_reps` on only (`PERRY_FACTS_ARRAY_REPS=0`
//! exports nothing).

use crate::bindings::{number_range, ModuleBindings};
use crate::gen::Gen;
use crate::model::{Key, NodeId};

/// The key interval of one copy: a canonical index literal, or the key
/// node's integer interval.
fn copy_key(g: &Gen<'_>, key: Key, idx: NodeId) -> Option<(i64, i64)> {
    match key {
        Key::Named(a) => {
            let k = &g.m.atoms[a as usize];
            let v = k.parse::<u32>().ok().filter(|v| v.to_string() == *k)?;
            Some((i64::from(v), i64::from(v)))
        }
        Key::Any(_) => number_range(&g.m, idx),
    }
}

/// Fill `ModuleBindings::element_keys` (module docs).
pub(crate) fn collect(g: &Gen<'_>, bindings: &mut [ModuleBindings]) {
    if g.m.aborted || !g.m.array_reps {
        return;
    }
    // A summarized body's key node has one copy per instance (`Inst::fresh`).
    let mut copies: std::collections::HashMap<NodeId, Vec<NodeId>> = Default::default();
    for inst in 0..g.m.sum.insts.len() as u32 {
        g.m.inst_copies(inst, |orig, copy| {
            copies.entry(orig).or_default().push(copy)
        });
    }
    // (module, offset) -> Some(hull), or None once a copy has no interval.
    let mut sites: std::collections::BTreeMap<(u32, u32), Option<(i64, i64)>> = Default::default();
    let hull = |a: Option<(i64, i64)>, b: Option<(i64, i64)>| match (a, b) {
        (Some((a, b)), Some((c, d))) => Some((a.min(c), b.max(d))),
        _ => None,
    };
    for site in &g.element_sites {
        if site.op != crate::derive::ElementOp::Read || site.byte_offset == 0 {
            continue;
        }
        let mut k = copy_key(g, site.key, site.idx);
        if matches!(site.key, Key::Any(_)) {
            for &c in copies.get(&site.idx).into_iter().flatten() {
                k = hull(k, number_range(&g.m, c));
            }
        }
        sites
            .entry((site.module, site.byte_offset))
            .and_modify(|e| *e = hull(*e, k))
            .or_insert(k);
    }
    for ((mi, offset), k) in sites {
        if let (Some(k), Some(mb)) = (k, bindings.get_mut(mi as usize)) {
            mb.element_keys.push((offset, k));
        }
    }
}
