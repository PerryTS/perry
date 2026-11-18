//! Certificates for emitted builtin calls with no remaining tower edge.
use super::*;
use crate::derive::{element, native_calls::native_member_calls_of};
use crate::model::prim;
use perry_hir::{Module, Stmt};

type At = (u32, u32);

/// Every generated copy must have the same arity and primitive-only arguments.
/// Missing or compressed-away nodes refuse; an empty set is not a value proof.
fn arguments(g: &Gen<'_>) -> HashMap<At, Option<Vec<u16>>> {
    let copies = element::run_copies(&g.m);
    let mut out = HashMap::new();
    let mut ends = HashMap::new();
    for c in &g.call_sites_at {
        let end = ends.entry((c.module, c.offset)).or_insert(c.end);
        let current = (!c.spread && c.callee.is_none() && *end == c.end)
            .then(|| {
                c.args
                    .iter()
                    .map(|&n| {
                        let mut mask = 0;
                        for id in element::with_copies(&copies, &[n]) {
                            let node = g.m.nodes.get(id as usize)?;
                            if node.dropped || !node.pts.is_empty() {
                                return None;
                            }
                            mask |= node.prim;
                        }
                        (mask != 0).then_some(mask)
                    })
                    .collect::<Option<Vec<_>>>()
            })
            .flatten();
        let previous = out.entry((c.module, c.offset)).or_insert(current.clone());
        match (previous.as_mut(), current) {
            (Some(a), Some(b)) if a.len() == b.len() => {
                for (a, b) in a.iter_mut().zip(b) {
                    *a |= b;
                }
            }
            _ => *previous = None,
        }
    }
    out
}

/// Operand coercions are part of the proof: the audited Math helpers take
/// Numbers only. Number predicates perform no coercion at all.
fn namespace_fast(ns: &str, key: &str, fast: &Expr, args: &[u16], operands: &[Expr]) -> bool {
    let mut children = Vec::new();
    walker::walk_expr_children(fast, &mut |e| children.push(e));
    if children.len() != operands.len()
        || !children
            .iter()
            .zip(operands)
            .all(|(a, b)| perry_hir::stable_hash::same_expr_structure(a, b))
    {
        return false;
    }
    match (ns, key, fast) {
        ("Math", "abs", Expr::MathAbs(_)) | ("Math", "round", Expr::MathRound(_)) => {
            args == [prim::NUM]
        }
        ("Math", "min", Expr::MathMin(_)) | ("Math", "max", Expr::MathMax(_)) => {
            args.iter().all(|&p| p == prim::NUM)
        }
        ("Number", "isInteger", Expr::NumberIsInteger(_))
        | ("Number", "isSafeInteger", Expr::NumberIsSafeInteger(_)) => args.len() == 1,
        _ => false,
    }
}

fn visit(e: &Expr, f: &mut impl FnMut(&Expr)) {
    f(e);
    // A namespace pair is one call. Its arms share an offset; visit only
    // their children after inspecting the selected arm as a single route.
    if let Expr::Conditional {
        condition,
        then_expr,
        else_expr,
    } = e
    {
        if matches!(condition.as_ref(), Expr::NamespacePatched(_)) {
            walker::walk_expr_children(then_expr, &mut |c| visit(c, f));
            walker::walk_expr_children(else_expr, &mut |c| visit(c, f));
            return;
        }
    }
    if let Expr::Closure { body, .. } = e {
        stmts(body, f);
    }
    walker::walk_expr_children(e, &mut |child| visit(child, f));
}
fn stmts(body: &[Stmt], f: &mut impl FnMut(&Expr)) {
    for s in body {
        walker::stmt_any_expr(s, &mut |e| {
            visit(e, f);
            false
        });
    }
}
fn module(hir: &Module, f: &mut impl FnMut(&Expr)) {
    stmts(&hir.init, f);
    for g in &hir.globals {
        if let Some(e) = &g.init {
            visit(e, f);
        }
    }
    let mut function = |fun: &Function| {
        for p in &fun.params {
            if let Some(e) = &p.default {
                visit(e, f);
            }
        }
        stmts(&fun.body, f);
    };
    for fun in &hir.functions {
        function(fun);
    }
    for c in &hir.classes {
        for fun in c
            .constructor
            .iter()
            .chain(&c.methods)
            .chain(&c.static_methods)
            .chain(c.getters.iter().map(|(_, f)| f))
            .chain(c.setters.iter().map(|(_, f)| f))
        {
            function(fun);
        }
    }
    for c in &hir.classes {
        if let Some(e) = &c.extends_expr {
            visit(e, f);
        }
        for field in c.fields.iter().chain(&c.static_fields) {
            if let Some(e) = &field.key_expr {
                visit(e, f);
            }
            if let Some(e) = &field.init {
                visit(e, f);
            }
        }
    }
}

pub(super) fn certify(pf: &mut ProgramFacts, g: &Gen<'_>, cfg: &crate::FactsConfig) {
    let args = arguments(g);
    // Console subscribers can rewrite the formatter's input array. The
    // manifest's thread family owns diagnostics_channel and its producers.
    let no_subscribers = pf.family_reach.as_ref().is_some_and(|f| !f.live("thread"));
    let mut certified: HashMap<(At, String), bool> = HashMap::new();
    for (mi, (_, hir)) in g.modules.iter().enumerate() {
        let proven = native_member_calls_of(pf, mi, cfg.namespace_pairs);
        module(hir, &mut |e| {
            let (call, fast, namespace) = match e {
                Expr::Conditional {
                    condition,
                    then_expr,
                    else_expr,
                } if matches!(condition.as_ref(), Expr::NamespacePatched(_)) => {
                    let Expr::NamespacePatched(ns) = condition.as_ref() else {
                        unreachable!()
                    };
                    (then_expr.as_ref(), else_expr.as_ref(), Some(ns.as_str()))
                }
                Expr::Call { .. } | Expr::CallSpread { .. } => (e, e, None),
                _ => return,
            };
            let (callee, arity, spread) = match call {
                Expr::Call { callee, args, .. } => (callee.as_ref(), args.len(), false),
                Expr::CallSpread { callee, args, .. } => (callee.as_ref(), args.len(), true),
                _ => return,
            };
            let Expr::PropertyGet {
                property,
                byte_offset,
                ..
            } = callee
            else {
                return;
            };
            let at = (mi as u32, *byte_offset);
            let selected = perry_hir::namespace_pairs::intrinsic_selected(*byte_offset, &proven)
                && namespace.is_none_or(|ns| {
                    cfg.namespace_pairs
                        && perry_hir::namespace_pairs::pair_offset(ns, call) == Some(*byte_offset)
                });
            let safe = selected
                && !spread
                && args.get(&at).is_some_and(|a| {
                    let Some(a) = a else {
                        return false;
                    };
                    if a.len() != arity {
                        return false;
                    }
                    if let Expr::Call {
                        callee: fast_callee,
                        args: fast_args,
                        ..
                    } = fast
                    {
                        if perry_hir::method_call::console_method(fast_callee) == Some("log")
                            && property == "log"
                            && fast_args.len() == arity
                        {
                            return a.is_empty() || (no_subscribers && a.as_slice() == [prim::STR]);
                        }
                    }
                    namespace.is_some_and(|ns| {
                        namespace_fast(
                            ns,
                            property,
                            fast,
                            a,
                            match call {
                                Expr::Call { args, .. } => args,
                                _ => unreachable!(),
                            },
                        )
                    })
                });
            *certified.entry((at, property.clone())).or_insert(true) &= safe;
        });
    }
    // One native verdict cannot override a refused twin at the same site.
    for s in pf.sites.iter().filter(|s| s.is_call) {
        let exact = matches!(&s.intrinsic_callee, Some(crate::derive::IntrinsicCallee::Exact(n))
            if n.rsplit('.').next() == Some(s.key.as_str()));
        *certified
            .entry(((s.module, s.byte_offset), s.key.clone()))
            .or_insert(false) &=
            s.access == Access::Read && !s.top && (s.verdict == Verdict::NativeMember || exact);
    }
    for s in &mut pf.sites {
        s.builtin_dispatch = s.is_call
            && certified
                .get(&((s.module, s.byte_offset), s.key.clone()))
                .copied()
                .unwrap_or(false);
    }
}
