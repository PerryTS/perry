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
        other: node.widened,
        ..Default::default()
    };
    for t in node.pts.to_sorted() {
        if t == TOP {
            out.other = true;
            continue;
        }
        match &m.toks[t as usize] {
            TokKind::Obj(tm) => match m.tmpls[*tm as usize].kind {
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

fn merge(a: &mut BindingFacts, b: BindingFacts) {
    a.class_ids.extend(b.class_ids);
    a.class_ids.sort_unstable();
    a.class_ids.dedup();
    a.prims |= b.prims;
    a.other |= b.other;
}

pub(crate) fn collect(g: &Gen) -> Vec<ModuleBindings> {
    let mut out: Vec<ModuleBindings> = (0..g.mods.len()).map(|_| Default::default()).collect();
    for (mi, info) in g.mods.iter().enumerate() {
        let mut v: Vec<(u32, BindingFacts)> = info
            .locals
            .iter()
            .map(|(&id, &n)| {
                let mut facts = facts_of(g, n);
                if let Some(copies) = g.context_locals.get(&(mi as u32, id)) {
                    for &n in copies {
                        merge(&mut facts, facts_of(g, n));
                    }
                }
                (id, facts)
            })
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
        let mut merged: Vec<(u32, BindingFacts)> = Vec::new();
        for (id, facts) in std::mem::take(&mut mb.this_of) {
            if let Some((last, value)) = merged.last_mut() {
                if *last == id {
                    merge(value, facts);
                    continue;
                }
            }
            merged.push((id, facts));
        }
        mb.this_of = merged;
    }
    out
}
