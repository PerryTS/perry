//! P3 B1 (DESIGN-p3-canonical-numbers rev 3.1 §2): element reads whose value
//! the facts prove a Number, or a Number or `undefined`.
//!
//! The fact is the NUM declaration itself (REVIEW-p3 R2): a node holds only
//! the Number primitive when every value reaching it is a program Number
//! (literals and arithmetic, which codegen lowers to plain doubles) or comes
//! from a builtin whose NUM seed is audited against its runtime producer
//! (`NUM_SEEDS.md`, checked by `tests/num_seeds_audit.rs`). There is no
//! separate encoding bit: a value the solver cannot see is ⊤, never NUM-only.
//!
//! An element read qualifies when, at every copy of the read, its value node
//! holds Number (and possibly `undefined`, for a hole or an index past the
//! end) and nothing else: no other primitive and no token. A copy that does
//! not qualify drops the site.

use crate::bindings::ModuleBindings;
use crate::gen::Gen;
use crate::model::prim;

/// `PERRY_FACTS_CANONICAL_NUMBERS=0` exports nothing.
pub fn enabled() -> bool {
    !matches!(
        std::env::var("PERRY_FACTS_CANONICAL_NUMBERS").as_deref(),
        Ok("0") | Ok("off") | Ok("false")
    )
}

/// Fill `ModuleBindings::number_element_reads` (module docs).
pub(crate) fn collect(g: &Gen<'_>, bindings: &mut [ModuleBindings]) {
    if g.m.aborted {
        return;
    }
    // (module, offset) -> Some(may be undefined), or None once a copy fails.
    let mut sites: std::collections::BTreeMap<(u32, u32), Option<bool>> = Default::default();
    // A summarized body's value node has one copy per instance
    // (`Inst::fresh`); the fact must hold for each of them too.
    let mut copies: std::collections::HashMap<u32, Vec<u32>> = Default::default();
    for inst in 0..g.m.sum.insts.len() as u32 {
        g.m.inst_copies(inst, |orig, copy| {
            copies.entry(orig).or_default().push(copy)
        });
    }
    let nodes = g.element_values.iter().flat_map(|&(mi, offset, n)| {
        std::iter::once(n)
            .chain(copies.get(&n).into_iter().flatten().copied())
            .map(move |c| (mi, offset, c))
    });
    for (mi, offset, n) in nodes {
        let node = &g.m.nodes[n as usize];
        let number = node.pts.is_empty()
            && node.prim & prim::NUM != 0
            && node.prim & !(prim::NUM | prim::UNDEF) == 0;
        let undef = node.prim & prim::UNDEF != 0;
        let entry = sites.entry((mi, offset)).or_insert(Some(false));
        *entry = match (*entry, number) {
            (Some(prev), true) => Some(prev || undef),
            _ => None,
        };
    }
    for ((mi, offset), fact) in sites {
        if let (Some(undef), Some(mb)) = (fact, bindings.get_mut(mi as usize)) {
            mb.number_element_reads.push((offset, undef));
        }
    }
}
