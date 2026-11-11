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
//! is T0 with `target_fns == [F]`), the call does not spread, and its offset
//! is not 0.
//!
//! E3 (CWN 11-09): the HIR calls gen walks under one key are copies of one
//! source call when they share its span (`offset..end`): the body walked
//! twice (a for…of's counted and IteratorRecord arms), or a callee the
//! inliner copied into several callers. The site is joined over them: every
//! copy must resolve to F, and every copy's arguments must hold F's
//! signature (`admits`). Calls of different spans under one key are distinct
//! calls and refuse.

use crate::bindings::{Enc, ModuleBindings, Rep};
use crate::derive::{SiteFacts, Verdict};
use crate::gen::Gen;
use crate::model::{NodeId, TokKind, TOP};

/// One call site as gen walks it (`Gen::call_sites_at`).
#[derive(Clone, Debug)]
pub(crate) struct CallSiteRec {
    pub module: u32,
    pub offset: u32,
    /// The end of the span the key names (a method call's member, else the
    /// call): with `offset`, the source call this HIR call is a copy of.
    pub end: u32,
    /// The argument nodes of this copy.
    pub args: Vec<NodeId>,
    pub spread: bool,
    /// A direct call's callee node; `None` for a method call.
    pub callee: Option<NodeId>,
    /// The solver's call (`Model::calls`) this copy made.
    pub call: crate::model::CallId,
}

/// A function's typed signature (module docs).
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Signature {
    pub params: Vec<Rep>,
    /// Each parameter's int interval, from the same entry node (T1).
    pub ranges: Vec<Option<(i64, i64)>>,
    pub ret: Rep,
    /// The class id of the one class `this` holds (E2).
    pub this_class: Option<u32>,
}

impl Signature {
    /// A signature that changes nothing: every slot's encoding is Boxed
    /// (an `Arr` slot holds the Boxed word, DESIGN-p4l1 §4.4) and there is
    /// no layout.
    pub fn is_boxed(&self) -> bool {
        self.ret.encoding() == Enc::Boxed
            && self.this_class.is_none()
            && self.params.iter().all(|r| r.encoding() == Enc::Boxed)
    }

    /// A parameter or the result is a value record (DESIGN-p4l4 §5.2).
    pub fn has_record(&self) -> bool {
        self.params
            .iter()
            .chain(std::iter::once(&self.ret))
            .any(|r| matches!(r, Rep::Record(_)))
    }
}

/// `PERRY_FACTS_TYPED_SIGNATURES=0` exports no signature and no typed call.
pub fn enabled() -> bool {
    !matches!(
        std::env::var("PERRY_FACTS_TYPED_SIGNATURES").as_deref(),
        Ok("0") | Ok("off") | Ok("false")
    )
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
    // A generator the transform lowered no longer says `is_generator`; its
    // body returns the generator object it links to the function's
    // `prototype`.
    let lowered_generator = f.body.iter().any(|s| {
        matches!(
            s,
            perry_hir::Stmt::Return(Some(perry_hir::Expr::LinkGeneratorPrototype { .. }))
        )
    });
    f.is_async
        || f.is_generator
        || lowered_generator
        || f.params
            .iter()
            .any(|p| p.is_rest || p.arguments_object.is_some())
}

/// Fill `ModuleBindings::signatures` and `ModuleBindings::typed_calls`.
/// `sabotage`: the negative controls `signature` / `signature-escape` claim
/// I32 for every F64 parameter of a function with a typed call;
/// `signature-range` claims `[0, 127]` for a negative I32 interval.
pub(crate) fn collect(
    g: &Gen<'_>,
    sites: &[SiteFacts],
    bindings: &mut [ModuleBindings],
    interpreted: bool,
    sabotage: Option<&str>,
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
    // A node the solve did not create (`NO_NODE`, a unit's missing
    // interface term) is no fact: Boxed.
    let live = |n: NodeId| (n as usize) < m.nodes.len();
    let rep1 = |n: NodeId| -> Rep {
        if live(n) {
            crate::bindings::rep_of_node(g, n)
        } else {
            Rep::Boxed
        }
    };
    let rep_at = |n: NodeId| -> Rep {
        all(n)
            .into_iter()
            .map(rep1)
            .reduce(Rep::join)
            .unwrap_or(Rep::Boxed)
    };
    // The function token's body index of every function.
    let mut sigs: std::collections::BTreeMap<(u32, u32), Signature> = Default::default();
    // Each typed body's literal defaults (§4.1): the caller applies them.
    let mut defaults: std::collections::HashMap<(u32, u32), Vec<Option<f64>>> = Default::default();
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
        let mut ranges = Vec::with_capacity(hir.params.len());
        // E1: a literal default is applied outside the body only when no
        // earlier parameter has a non-literal default.
        let mut earlier_nonliteral = false;
        for p in &hir.params {
            let local = g.mods[f.module as usize].locals.get(&p.id).copied();
            let rep = match &p.default {
                Some(d) if !is_literal(d) || earlier_nonliteral => {
                    earlier_nonliteral |= !is_literal(d);
                    Rep::Boxed
                }
                // The parameter local's own fact, exactly P1's
                // `BindingFacts::rep`: instance copies flow into the shared
                // node, so its values cover every copy.
                _ => match g.mods[f.module as usize].locals.get(&p.id) {
                    Some(&n) => rep1(n),
                    None => Rep::Boxed,
                },
            };
            ranges.push(match (rep, local) {
                (Rep::I32, Some(n)) if live(n) => crate::bindings::facts_of_node(g, n).int_range,
                _ => None,
            });
            params.push(rep);
        }
        // DESIGN-p4l4 VR-3b: a Record slot exists only with record
        // signatures on.
        if !m.record_signatures {
            for p in &mut params {
                if matches!(p, Rep::Record(_)) {
                    *p = Rep::Boxed;
                }
            }
        }
        let mut ret = rep_at(f.ret);
        let mut this_nodes = vec![f.this];
        for it in &m.sum.insts {
            if it.f as usize == fx {
                ret = Rep::join(ret, rep1(it.ret));
                this_nodes.push(it.this);
            }
        }
        if !m.record_signatures && matches!(ret, Rep::Record(_)) {
            ret = Rep::Boxed;
        }
        let this_class = one_class(g, &this_nodes);
        let sig = Signature {
            params,
            ranges,
            ret,
            this_class,
        };
        if !sig.is_boxed() {
            defaults.insert((f.module, f.func_id), literal_defaults(hir));
            sigs.insert((f.module, f.func_id), sig);
        }
    }
    // Typed call sites (§2, E3).
    let targets = call_targets(g, sites);
    // DESIGN-p4l4 VR-3b: a Record slot is the callee's ABI, so every call
    // that enters the body by a source call site must be one of its typed
    // calls, in the body's own module (codegen's typed calls are
    // same-module). A function with a site that is not keeps its Record
    // slots Boxed: its callers pass a materialized copy (§4.2) and its body
    // reads the copy key by key.
    let mut demoted: std::collections::BTreeSet<(u32, u32)> = Default::default();
    for (key, (t, recs)) in &targets {
        let Some(sig) = sigs.get(t) else { continue };
        if sig.has_record()
            && (key.0 != t.0 || !recs.iter().all(|r| admits(g, &all, sig, &defaults[t], &r.args)))
        {
            demoted.insert(*t);
        }
    }
    for t in &demoted {
        let sig = sigs.get_mut(t).expect("a demoted signature");
        for p in sig.params.iter_mut().chain(std::iter::once(&mut sig.ret)) {
            if matches!(p, Rep::Record(_)) {
                *p = Rep::Boxed;
            }
        }
        if sig.is_boxed() {
            sigs.remove(t);
        }
    }
    let mut typed: Vec<((u32, u32), (u32, u32))> = Vec::new();
    for (key, (t, recs)) in targets {
        let Some(sig) = sigs.get(&t) else { continue };
        let lits = &defaults[&t];
        if recs.iter().all(|r| admits(g, &all, sig, lits, &r.args)) {
            typed.push((key, t));
        }
    }
    if matches!(sabotage, Some("signature") | Some("signature-escape")) {
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
    if sabotage == Some("signature-range") {
        // T1's negative control: claim `[0, 127]` for an I32 parameter some
        // caller passes a negative value.
        for sig in sigs.values_mut() {
            for r in sig.ranges.iter_mut().flatten() {
                if r.0 < 0 {
                    *r = (0, 127);
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

/// The call sites a typed entry can serve before any signature is known
/// (§2, E3): per source call `(module, offset)`, the one body every copy
/// resolves to, with the copies. A call at offset 0, a spread call, copies of
/// different spans or kinds, or a callee holding anything else is absent.
pub(crate) fn call_targets<'g>(
    g: &'g Gen<'_>,
    sites: &[SiteFacts],
) -> std::collections::BTreeMap<(u32, u32), ((u32, u32), Vec<&'g CallSiteRec>)> {
    let m = &g.m;
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
    let live = |n: NodeId| (n as usize) < m.nodes.len();
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
    let mut out = std::collections::BTreeMap::new();
    for (&(mi, offset), recs) in &by_key {
        if offset == 0 || recs.iter().any(|r| r.spread) {
            continue;
        }
        // E3: the key names one source call; its copies share its span.
        let end = recs[0].end;
        if recs.iter().any(|r| r.end != end) {
            continue;
        }
        // Every copy is a call of the same kind.
        let direct = recs[0].callee.is_some();
        if recs.iter().any(|r| r.callee.is_some() != direct) {
            continue;
        }
        let target = if direct {
            // Every copy (and every instance copy of its callee node)
            // holds exactly one body, the same one; a missing node refuses.
            let mut one: Option<(u32, u32)> = None;
            let mut ok = true;
            for r in recs {
                for c in all(r.callee.expect("a direct call")) {
                    match live(c).then(|| single_body(g, c)).flatten() {
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
        } else {
            method_target.get(&(mi, offset)).copied().flatten()
        };
        if let Some(t) = target {
            out.insert((mi, offset), (t, recs.clone()));
        }
    }
    out
}

/// Whether `(module, func_id)` may have a typed signature at all (§1): not a
/// generator or async function, no rest parameter, no `arguments`.
pub(crate) fn may_type(g: &Gen<'_>, f: &crate::model::FuncInfo) -> bool {
    match hir_fn(g, f.module, f.func_id) {
        Some(hir) => !excluded(hir) && !f.has_arguments,
        None => false,
    }
}

/// Each parameter's literal default when the caller applies it (§4.1, E1):
/// a literal default with no earlier non-literal one.
fn literal_defaults(f: &perry_hir::Function) -> Vec<Option<f64>> {
    let mut earlier_nonliteral = false;
    f.params
        .iter()
        .map(|p| match &p.default {
            Some(d) if !is_literal(d) || earlier_nonliteral => {
                earlier_nonliteral |= !is_literal(d);
                None
            }
            Some(perry_hir::Expr::Number(n)) => Some(*n),
            Some(perry_hir::Expr::Integer(i)) => Some(*i as f64),
            _ => None,
        })
        .collect()
}

/// E3: one copy's arguments hold `sig` (what the codegen site witness
/// checks): each argument the caller passes in an F64 slot holds only
/// Numbers, and in an I32 slot only integers inside the parameter's
/// interval, joined over the argument node's instance copies; an omitted or
/// `undefined` argument of a literal-defaulted parameter is that literal.
/// An omitted argument of any other non-Boxed parameter refuses.
fn admits(
    g: &Gen<'_>,
    all: &dyn Fn(NodeId) -> Vec<NodeId>,
    sig: &Signature,
    lits: &[Option<f64>],
    args: &[NodeId],
) -> bool {
    use crate::model::prim;
    // The int interval of the values joined so far: `None` before the
    // first value, `Some(None)` once one value is no int.
    fn join_range(acc: &mut Option<Option<(i64, i64)>>, r: Option<(i64, i64)>) {
        *acc = Some(match (*acc, r) {
            (None, r) => r,
            (Some(Some((a, b))), Some((c, d))) => Some((a.min(c), b.max(d))),
            _ => None,
        });
    }
    let lit_range = |v: f64| {
        (v.fract() == 0.0 && !(v == 0.0 && v.is_sign_negative())).then_some((v as i64, v as i64))
    };
    sig.params.iter().enumerate().all(|(i, &rep)| {
        // A Record slot's arguments are the parameter node's own values:
        // tokens of its one template (and `undefined`), DESIGN-p4l4 §1.1.
        if matches!(rep.encoding(), Enc::Boxed | Enc::Record) {
            return true;
        }
        let lit = lits.get(i).copied().flatten();
        let mut acc: Option<Option<(i64, i64)>> = None;
        match args.get(i) {
            None => match lit {
                Some(v) => join_range(&mut acc, lit_range(v)),
                None => return false,
            },
            Some(&a) => {
                for n in all(a) {
                    if (n as usize) >= g.m.nodes.len() {
                        return false;
                    }
                    let b = crate::bindings::facts_of_node(g, n);
                    let mut prims = b.prims;
                    if let Some(v) = lit.filter(|_| prims & prim::UNDEF != 0) {
                        prims &= !prim::UNDEF;
                        join_range(&mut acc, lit_range(v));
                    }
                    if b.other || !b.class_ids.is_empty() || prims & !prim::NUM != 0 {
                        return false;
                    }
                    if prims != 0 {
                        join_range(&mut acc, b.int_range);
                    }
                }
            }
        }
        match (rep, acc) {
            (Rep::I32, Some(r)) => {
                let (lo, hi) = sig.ranges[i].unwrap_or((i64::from(i32::MIN), i64::from(i32::MAX)));
                r.is_some_and(|(a, b)| a >= lo && b <= hi)
            }
            _ => true,
        }
    })
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
        if (n as usize) >= g.m.nodes.len() {
            return None;
        }
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
