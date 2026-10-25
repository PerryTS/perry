//! Receiver facts per binding (slice 2 consumer input): what a local or a
//! function's `this` can hold, read off the solved graph.
//!
//! The analysis is flow-insensitive per binding, so one answer holds at
//! every use of the binding. Codegen asks it at a property access whose
//! receiver is `LocalGet(id)` or `This`; HIR property sets carry no source
//! offset, so the binding (not the site) is the key.

use crate::gen::Gen;
use crate::model::{TmplKind, TokKind, TOP};

/// What one binding can hold.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct BindingFacts {
    /// Diagnostic-only TOP provenance; never consumed by emitted-code decisions.
    pub top: bool,
    pub top_cause: Option<String>,
    /// Class ids of the class templates (fact units) it can hold, sorted.
    pub class_ids: Vec<u32>,
    /// Primitive kind bits (`model::prim`).
    pub prims: u16,
    /// It can hold ⊤ or any non-class object (literal, builtin, function).
    pub other: bool,
}

/// Per module: `(local id, facts)` and `(HIR function id, facts of this)`,
/// both sorted by id.
#[derive(Clone, Debug, Default)]
pub struct ModuleBindings {
    pub locals: Vec<(u32, BindingFacts)>,
    pub this_of: Vec<(u32, BindingFacts)>,
}

fn facts_of(g: &Gen, n: u32) -> BindingFacts {
    let m = &g.m;
    let node = &m.nodes[n as usize];
    let mut out = BindingFacts {
        prims: node.prim,
        top: m.explain && node.pts.contains_tok(TOP),
        top_cause: (m.explain && node.pts.contains_tok(TOP)).then(|| m.top_origin(n)),
        ..Default::default()
    };
    for t in node.pts.to_sorted() {
        if t == TOP {
            out.other = true;
            continue;
        }
        // A marker beside its values (a closed function's described
        // parameter, what it reaches, an argument marker of only such
        // markers; DECISIONS 74, 75) stands for no value of its own: the
        // callers' values are the contract's, beside it in this node. It
        // adds no class and is not `other` (DECISIONS 76).
        if m.beside_values(t) {
            continue;
        }
        match &m.toks[t as usize] {
            TokKind::Obj(tm, _) => match m.tmpls[*tm as usize].kind {
                TmplKind::Class { class_id, .. } => out.class_ids.push(class_id),
                _ => out.other = true,
            },
            _ => out.other = true,
        }
    }
    out.class_ids.sort_unstable();
    out.class_ids.dedup();
    out
}

pub(crate) fn collect(g: &Gen) -> Vec<ModuleBindings> {
    let mut out: Vec<ModuleBindings> = (0..g.mods.len()).map(|_| Default::default()).collect();
    for (mi, info) in g.mods.iter().enumerate() {
        let mut v: Vec<(u32, BindingFacts)> = info
            .locals
            .iter()
            .map(|(&id, &n)| (id, facts_of(g, n)))
            .collect();
        v.sort_by_key(|(id, _)| *id);
        out[mi].locals = v;
    }
    for f in &g.m.funcs {
        if f.func_id == u32::MAX {
            continue;
        }
        let b = facts_of(g, f.this);
        out[f.module as usize].this_of.push((f.func_id, b));
    }
    for mb in &mut out {
        mb.this_of.sort_by_key(|(id, _)| *id);
        // A function id registered twice (should not happen) gets no answer.
        let mut dup = Vec::new();
        for w in mb.this_of.windows(2) {
            if w[0].0 == w[1].0 {
                dup.push(w[0].0);
            }
        }
        mb.this_of.retain(|(id, _)| !dup.contains(id));
    }
    out
}
