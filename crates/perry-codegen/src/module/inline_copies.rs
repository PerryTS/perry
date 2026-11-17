//! Inlinable bodies for direct calls across codegen units.
//!
//! A split module compiles each unit as its own LLVM module, so a direct call
//! whose callee lives in another unit reaches only a `declare` and LLVM cannot
//! inline it, while the same call inside one unit is inlined. The caller unit
//! therefore also carries the callee's final text with `available_externally`
//! linkage: LLVM may inline it there, and drops the body before code emission
//! (it never reaches the object file). The one emitted definition is the
//! callee's home-unit definition, unchanged. The copy is the identical final
//! text (same instructions, same gc-leaf call annotations, same attributes);
//! only the linkage word differs.
//!
//! Which callees: only those LLVM can inline at all on this pipeline. RS4GC
//! runs before the size pipeline and only `always-inline` runs ahead of it, so
//! a call that becomes a statepoint is never inlined afterwards. A callee
//! qualifies when it is `alwaysinline` (pre-statepoint inline) or its call
//! sites stay plain calls (it is in the module's gc-leaf set, or the module
//! has no native roots), it is not `noinline` and has no personality. A
//! `perry_js_text` section does not matter: inlining moves the instructions
//! into the caller's section, as same-unit inlining of such a body does.
//! Its size bound is LLVM's own: see [`MAX_COPY_INSTRUCTIONS`]. A copy's own
//! direct callees get copies in the same unit too, so inlining a copy never
//! turns a call its home unit inlines into a real call.

use std::collections::{BTreeSet, HashSet};

use crate::function::LlFunction;

/// Largest instruction count of a copied non-`alwaysinline` callee: LLVM's
/// own highest call-site threshold on this pipeline, the `inlinehint`
/// threshold 325, read as an instruction count. Most instructions cost
/// InstrCost 5 and none costs less than 0, so a larger body is inlined only
/// when over four fifths of it is free; below it LLVM's cost model decides.
/// (A threshold/5 bound is too tight: free casts, GEPs and root asm make an
/// 89-instruction field copy inline at home.) An `alwaysinline` callee has no
/// cost bound and is copied regardless.
pub(crate) const MAX_COPY_INSTRUCTIONS: usize = 325;

/// For each function of `funcs`, the units other than its own that may
/// inline it (sorted): units that call it directly, and units holding a copy
/// of a function that calls it directly. `buckets` is the unit partition.
pub(crate) fn inline_copy_units(
    funcs: &[&LlFunction],
    buckets: &[Vec<usize>],
    gc_leaf_callees: &HashSet<String>,
    native_roots: bool,
) -> Vec<Vec<usize>> {
    let mut unit = vec![0usize; funcs.len()];
    for (bi, bucket) in buckets.iter().enumerate() {
        for &i in bucket {
            unit[i] = bi;
        }
    }
    let eligible: Vec<bool> = funcs
        .iter()
        .map(|f| copy_eligible(f, gc_leaf_callees, native_roots))
        .collect();
    let mut callees: Vec<Vec<usize>> = vec![Vec::new(); funcs.len()];
    for edge in super::unit_graph::reference_edges(funcs) {
        let (from, to) = (edge.from as usize, edge.to as usize);
        if edge.direct && eligible[to] {
            callees[from].push(to);
        }
    }
    let mut units: Vec<BTreeSet<usize>> = vec![BTreeSet::new(); funcs.len()];
    let mut work: Vec<(usize, usize)> = (0..funcs.len()).map(|f| (f, unit[f])).collect();
    while let Some((from, u)) = work.pop() {
        for &to in &callees[from] {
            if unit[to] != u && units[to].insert(u) {
                work.push((to, u));
            }
        }
    }
    units.into_iter().map(|s| s.into_iter().collect()).collect()
}

fn copy_eligible(f: &LlFunction, gc_leaf_callees: &HashSet<String>, native_roots: bool) -> bool {
    if f.personality.is_some() || f.no_inline {
        return false;
    }
    let header = f.define_header(true);
    if header.contains(" noinline") {
        return false;
    }
    if header.contains(" alwaysinline") {
        return true;
    }
    (!native_roots || gc_leaf_callees.contains(&f.name))
        && instruction_count(f) <= MAX_COPY_INSTRUCTIONS
}

/// The function's instructions: its final typed instructions and text lines
/// (labels and blanks excluded).
pub(crate) fn instruction_count(f: &LlFunction) -> usize {
    use crate::function::FinalItem;
    let mut n = 0usize;
    let _ = f.for_each_final_item::<std::convert::Infallible>(&mut |item| {
        if matches!(item, FinalItem::Inst(_) | FinalItem::Text(_)) {
            n += 1;
        }
        Ok(())
    });
    n
}

/// `text` (a function's final text) as an `available_externally` definition.
pub(crate) fn available_externally_text(f: &LlFunction, text: &str) -> Option<String> {
    let rest = text.strip_prefix("define ")?;
    let rest = if f.linkage.is_empty() {
        rest
    } else {
        rest.strip_prefix(f.linkage.as_str())?.strip_prefix(' ')?
    };
    let rest = rest.strip_prefix("external ").unwrap_or(rest);
    Some(format!("define available_externally {rest}"))
}
