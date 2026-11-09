//! P2 (DESIGN-p2-typed-signatures rev 2.1): one representation-typed
//! signature per function, and the call sites that use it.
//!
//! `Signature` is a per-function view of the one representation fact
//! (`bindings::rep_of` over the audited NUM declarations, REVIEW-p3 R2):
//!
//! - `params[i]`: `rep_of` of parameter i's entry node (its local: for a
//!   defaulted parameter the post-default value, `params_inner`), the very
//!   `BindingFacts::rep` P1 gives that local (instance copies flow into it);
//! - `ret`: `rep_of` of the result node joined over the copies (implicit
//!   `undefined` completion is in it, so such a function is Boxed);
//! - `this_class`: the one class every copy's `this` holds, when it holds
//!   nothing else (E2): only then may a typed body assume that layout.
//!
//! `typed_calls` are the call sites a typed entry serves (§2, E3): at every
//! copy of the caller the callee is exactly one body F (a direct call whose
//! callee node holds only F's function tokens, or a method call whose verdict
//! is T0 with `target_fns == [F]`), the call does not spread, its offset is
//! not 0 and names exactly one HIR call.

use crate::bindings::{ModuleBindings, Rep};
use crate::derive::{SiteFacts, Verdict};
use crate::gen::Gen;
use crate::model::{NodeId, TokKind, TOP};

/// One call site as gen walks it (`Gen::call_sites_at`).
#[derive(Clone, Debug)]
pub(crate) struct CallSiteRec {
    pub module: u32,
    pub offset: u32,
    /// The HIR identity of the call (its callee expression's address).
    pub hir: usize,
    pub spread: bool,
    /// A direct call's callee node; `None` for a method call.
    pub callee: Option<NodeId>,
}

/// A function's typed signature (module docs).
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Signature {
    pub params: Vec<Rep>,
    pub ret: Rep,
    /// The class id of the one class `this` holds (E2).
    pub this_class: Option<u32>,
}

impl Signature {
    /// A signature that changes nothing (every slot Boxed, no layout).
    pub fn is_boxed(&self) -> bool {
        self.ret == Rep::Boxed
            && self.this_class.is_none()
            && self.params.iter().all(|r| *r == Rep::Boxed)
    }
}

/// `PERRY_FACTS_TYPED_SIGNATURES=0` exports no signature and no typed call.
pub fn enabled() -> bool {
    !matches!(
        std::env::var("PERRY_FACTS_TYPED_SIGNATURES").as_deref(),
        Ok("0") | Ok("off") | Ok("false")
    )
}

/// The join of two representations: I32 ⊑ F64 ⊑ Boxed.
fn join(a: Rep, b: Rep) -> Rep {
    match (a, b) {
        (Rep::Boxed, _) | (_, Rep::Boxed) => Rep::Boxed,
        (Rep::F64, _) | (_, Rep::F64) => Rep::F64,
        (Rep::I32, Rep::I32) => Rep::I32,
    }
}

fn is_literal(e: &perry_hir::Expr) -> bool {
    use perry_hir::Expr;
    matches!(
        e,
        Expr::Number(_) | Expr::Integer(_) | Expr::Bool(_) | Expr::Null | Expr::String(_)
    )
}

/// The HIR body of `(module, func_id)`.
fn hir_fn<'a>(g: &'a Gen<'_>, mi: u32, id: u32) -> Option<&'a perry_hir::Function> {
    let (_, m) = g.modules.get(mi as usize)?;
    m.classes
        .iter()
        .flat_map(|c| c.methods.iter().chain(c.static_methods.iter()))
        .chain(m.functions.iter())
        .find(|f| f.id == id)
}

/// Whether `f` may get a typed signature at all (§1 exclusions). Direct
/// `eval` needs no rule here: it writes the caller's locals as ⊤, so their
/// parameters are Boxed (REVIEW-p2, checked).
fn excluded(f: &perry_hir::Function) -> bool {
    f.is_async
        || f.is_generator
        || f.params
            .iter()
            .any(|p| p.is_rest || p.arguments_object.is_some())
}

/// Fill `ModuleBindings::signatures` and `ModuleBindings::typed_calls`.
/// `sabotage`: the negative controls `signature` / `signature-escape` claim
/// I32 for every F64 parameter of a function with a typed call.
pub(crate) fn collect(
    g: &Gen<'_>,
    sites: &[SiteFacts],
    bindings: &mut [ModuleBindings],
    interpreted: bool,
    sabotage: bool,
) {
    let m = &g.m;
    if m.aborted || interpreted {
        return;
    }
    // Instance copies of every summarized node.
    let mut copies: std::collections::HashMap<NodeId, Vec<NodeId>> = Default::default();
    for inst in 0..m.sum.insts.len() as u32 {
        m.inst_copies(inst, |orig, copy| {
            copies.entry(orig).or_default().push(copy)
        });
    }
    let all = |n: NodeId| -> Vec<NodeId> {
        std::iter::once(n)
            .chain(copies.get(&n).into_iter().flatten().copied())
            .collect()
    };
    let rep_at = |n: NodeId| -> Rep {
        all(n)
            .into_iter()
            .map(|c| crate::bindings::rep_of_node(g, c))
            .reduce(join)
            .unwrap_or(Rep::Boxed)
    };
    // The function token's body index of every function.
    let mut sigs: std::collections::BTreeMap<(u32, u32), Signature> = Default::default();
    for (fx, f) in m.funcs.iter().enumerate() {
        if f.func_id == u32::MAX {
            continue;
        }
        let Some(hir) = hir_fn(g, f.module, f.func_id) else {
            continue;
        };
        if excluded(hir) || f.has_arguments {
            continue;
        }
        let mut params = Vec::with_capacity(hir.params.len());
        // E1: a literal default is applied outside the body only when no
        // earlier parameter has a non-literal default.
        let mut earlier_nonliteral = false;
        for p in &hir.params {
            let rep = match &p.default {
                Some(d) if !is_literal(d) || earlier_nonliteral => {
                    earlier_nonliteral |= !is_literal(d);
                    Rep::Boxed
                }
                // The parameter local's own fact, exactly P1's
                // `BindingFacts::rep`: instance copies flow into the shared
                // node, so its values cover every copy.
                _ => match g.mods[f.module as usize].locals.get(&p.id) {
                    Some(&n) => crate::bindings::rep_of_node(g, n),
                    None => Rep::Boxed,
                },
            };
            params.push(rep);
        }
        let mut ret = rep_at(f.ret);
        let mut this_nodes = vec![f.this];
        for it in &m.sum.insts {
            if it.f as usize == fx {
                ret = join(ret, crate::bindings::rep_of_node(g, it.ret));
                this_nodes.push(it.this);
            }
        }
        let this_class = one_class(g, &this_nodes);
        let sig = Signature {
            params,
            ret,
            this_class,
        };
        if !sig.is_boxed() {
            sigs.insert((f.module, f.func_id), sig);
        }
    }
    // Typed call sites (§2, E3).
    let mut by_key: std::collections::BTreeMap<(u32, u32), Vec<&CallSiteRec>> = Default::default();
    for r in &g.call_sites_at {
        by_key.entry((r.module, r.offset)).or_default().push(r);
    }
    let method_target: std::collections::HashMap<(u32, u32), Option<(u32, u32)>> = {
        let mut out: std::collections::HashMap<(u32, u32), Option<(u32, u32)>> = Default::default();
        for s in sites.iter().filter(|s| s.is_call) {
            let one = (matches!(s.verdict, Verdict::T0Mono | Verdict::T0Set)
                && s.target_fns.len() == 1)
                .then(|| s.target_fns[0]);
            let e = out.entry((s.module, s.byte_offset)).or_insert(one);
            if *e != one {
                *e = None;
            }
        }
        out
    };
    let mut typed: Vec<((u32, u32), (u32, u32))> = Vec::new();
    for (&(mi, offset), recs) in &by_key {
        if offset == 0 || recs.iter().any(|r| r.spread) {
            continue;
        }
        // E3: one HIR call per key.
        let first = recs[0].hir;
        if recs.iter().any(|r| r.hir != first) {
            continue;
        }
        let target = match recs[0].callee {
            Some(_) => {
                let mut one: Option<(u32, u32)> = None;
                let mut ok = true;
                for r in recs {
                    for c in all(r.callee.unwrap()) {
                        match single_body(g, c) {
                            Some(b) if one.is_none_or(|o| o == b) => one = Some(b),
                            _ => ok = false,
                        }
                    }
                }
                if ok {
                    one
                } else {
                    None
                }
            }
            None => method_target.get(&(mi, offset)).copied().flatten(),
        };
        let Some(t) = target else { continue };
        if sigs.contains_key(&t) {
            typed.push(((mi, offset), t));
        }
    }
    if sabotage {
        let has_site: std::collections::HashSet<(u32, u32)> =
            typed.iter().map(|(_, t)| *t).collect();
        for (k, sig) in sigs.iter_mut() {
            if has_site.contains(k) {
                for p in &mut sig.params {
                    if *p == Rep::F64 {
                        *p = Rep::I32;
                    }
                }
            }
        }
    }
    for ((mi, id), sig) in sigs {
        if let Some(mb) = bindings.get_mut(mi as usize) {
            mb.signatures.push((id, sig));
        }
    }
    for ((mi, offset), t) in typed {
        if let Some(mb) = bindings.get_mut(mi as usize) {
            mb.typed_calls.push((offset, t));
        }
    }
    for mb in bindings.iter_mut() {
        mb.signatures.sort_by_key(|(id, _)| *id);
        mb.typed_calls.sort_by_key(|(o, _)| *o);
    }
}

/// The one body `(module, func_id)` node `n` holds when it holds only
/// function tokens of that body (any closure context) and no primitive.
fn single_body(g: &Gen<'_>, n: NodeId) -> Option<(u32, u32)> {
    let m = &g.m;
    let node = &m.nodes[n as usize];
    if node.prim != 0 || node.pts.is_empty() || node.pts.contains_tok(TOP) {
        return None;
    }
    let mut one: Option<(u32, u32)> = None;
    for t in node.pts.to_sorted() {
        let TokKind::Func(fx, _) = m.toks[t as usize] else {
            return None;
        };
        let f = &m.funcs[fx as usize];
        let b = (f.module, f.func_id);
        if one.replace(b).is_some_and(|o| o != b) {
            return None;
        }
    }
    one
}

/// The one class every node holds when they hold instances of it and
/// nothing else (E2).
fn one_class(g: &Gen<'_>, nodes: &[NodeId]) -> Option<u32> {
    let mut one: Option<u32> = None;
    for &n in nodes {
        let b = crate::bindings::facts_of_node(g, n);
        if b.other || b.prims != 0 || b.class_ids.len() != 1 {
            return None;
        }
        if one
            .replace(b.class_ids[0])
            .is_some_and(|c| c != b.class_ids[0])
        {
            return None;
        }
    }
    one
}
