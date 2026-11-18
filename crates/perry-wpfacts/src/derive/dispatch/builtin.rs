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

/// The fast arm may inline part of an argument the generic arm preserves. A
/// full source call span still identifies the actual fast operand across copies.
fn numeric_results(g: &Gen<'_>) -> std::collections::HashSet<(u32, u32, u32)> {
    let copies = element::run_copies(&g.m);
    let mut results = HashMap::new();
    for &(mi, at, end, n) in &g.call_kinds_at {
        let numeric = end > at
            && at != 0
            && element::with_copies(&copies, &[n]).iter().all(|&id| {
                let node = &g.m.nodes[id as usize];
                !node.dropped && node.pts.is_empty() && node.prim == prim::NUM
            });
        *results.entry((mi, at, end)).or_insert(true) &= numeric;
    }
    results
        .into_iter()
        .filter_map(|(span, yes)| yes.then_some(span))
        .collect()
}

/// Each read's receiver must pass the indexed-access proof at every instance;
/// a builtin member by itself only proves the lookup under its own name.
fn quiet_arrays(g: &Gen<'_>) -> std::collections::HashSet<(At, String)> {
    let m = &g.m;
    if !m.native_writes.is_empty()
        || m.objproto_accessor
        || m.reshaped_any
        || !m.reshaped_keys.is_empty()
    {
        return Default::default();
    }
    let copies = element::run_copies(m);
    let mut sites = HashMap::new();
    for s in &g.read_sites {
        let key = &m.atoms[s.key as usize];
        if !s.is_call || !matches!(key.as_str(), "fill" | "pop" | "includes") {
            continue;
        }
        let quiet = !s.value_call
            && element::with_copies(&copies, &[s.recv]).iter().all(|&n| {
                let node = &m.nodes[n as usize];
                !node.dropped
                    && node.prim == 0
                    && crate::quiet_sites::not_plain_arrays(m, n).is_none()
                    && node.pts.to_sorted().iter().all(|t| {
                        !m.hard.contains_key(t)
                            && !m.super_relink.contains(t)
                            && m.tmpl_of(*t)
                                .is_some_and(|tm| !m.record_arrays.contains_key(&tm))
                    })
            });
        *sites
            .entry(((s.module, s.byte_offset), key.to_string()))
            .or_insert(true) &= quiet;
    }
    sites
        .into_iter()
        .filter_map(|(at, quiet)| quiet.then_some(at))
        .collect()
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
    if let Expr::Closure { body, params, .. } = e {
        for p in params {
            decorators(&p.decorators, f);
        }
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
fn decorators(decorators: &[perry_hir::Decorator], f: &mut impl FnMut(&Expr)) {
    for decorator in decorators {
        for arg in &decorator.args {
            visit(arg, f);
        }
    }
}
fn function(fun: &Function, f: &mut impl FnMut(&Expr)) {
    decorators(&fun.decorators, f);
    for p in &fun.params {
        decorators(&p.decorators, f);
        if let Some(e) = &p.default {
            visit(e, f);
        }
    }
    stmts(&fun.body, f);
}
pub(super) fn module(hir: &Module, f: &mut impl FnMut(&Expr)) {
    stmts(&hir.init, f);
    for g in &hir.globals {
        if let Some(e) = &g.init {
            visit(e, f);
        }
    }
    for fun in &hir.functions {
        function(fun, f);
    }
    for c in &hir.classes {
        for fun in c
            .constructor
            .iter()
            .chain(&c.methods)
            .chain(&c.static_methods)
            .chain(c.getters.iter().map(|(_, f)| f))
            .chain(c.setters.iter().map(|(_, f)| f))
            .chain(c.computed_members.iter().map(|m| &m.function))
        {
            function(fun, f);
        }
    }
    for c in &hir.classes {
        decorators(&c.decorators, f);
        for member in &c.computed_members {
            visit(&member.key_expr, f);
        }
        if let Some(e) = &c.extends_expr {
            visit(e, f);
        }
        for field in c.fields.iter().chain(&c.static_fields) {
            decorators(&field.decorators, f);
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
    use perry_hir::folded_call::{direct_map_call, Site, SITE_DIRECT_MAP, SITE_MAP};
    let args = arguments(g);
    let numeric_results = numeric_results(g);
    let mut strings = std::collections::BTreeSet::new();
    let quiet_arrays = quiet_arrays(g);
    let mut arrays = std::collections::BTreeSet::new();
    let mut maps = std::collections::HashSet::new();
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
            let map = namespace.is_none()
                && !spread
                && direct_map_call(callee, arity)
                && pf.bindings[mi].method_sites.iter().any(|(site, k, flags)| {
                    *site == Site::Callee(*byte_offset) && k == property && flags & SITE_MAP != 0
                });
            if map {
                maps.insert((at, property.clone()));
            }
            let array = namespace.is_none()
                && !spread
                && perry_hir::folded_call::direct_array_call(callee, arity)
                && quiet_arrays.contains(&(at, property.clone()))
                && pf.bindings[mi].method_sites.iter().any(|(site, k, flags)| {
                    *site == Site::Callee(*byte_offset)
                        && k == property
                        && flags & perry_hir::folded_call::SITE_ARRAY_MEMBER != 0
                });
            if array {
                arrays.insert((at, property.clone()));
            }
            let fill = namespace.is_none()
                && pf.bindings[mi].fill_births.iter().any(|&(o, len)| {
                    o == *byte_offset
                        && perry_hir::folded_call::fill_birth_value(call, len)
                            .is_ok_and(|v| matches!(v, Expr::Integer(_) | Expr::Number(_)))
                });
            let string = namespace.is_none()
                && !spread
                && perry_hir::folded_call::direct_string_call(callee, arity)
                && (arity == 0
                    || args.get(&at).and_then(Option::as_ref).is_some_and(|p| {
                        p.as_slice()
                            == [if property == "indexOf" {
                                prim::STR
                            } else {
                                prim::NUM
                            }]
                    }));
            if string {
                strings.insert((at, property.clone()));
            }
            let safe = (map || fill || string || array) && selected
                || selected && !spread && args.get(&at).is_some_and(|a| {
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
                            && matches!(call, Expr::Call { args, .. } if fast_args.iter().zip(args)
                            .all(|(a, b)| perry_hir::stable_hash::same_expr_structure(a, b)))
                        {
                            return a.is_empty() || (no_subscribers && a.as_slice() == [prim::STR]);
                        }
                    }
                    namespace.is_some_and(|ns| {
                        if ns == "Math" && property == "abs" && arity == 1 {
                            if let Expr::MathAbs(x) = fast {
                                if matches!(x.as_ref(), Expr::Call {byte_offset, byte_end, ..}
                                    if numeric_results.contains(&(mi as u32, *byte_offset, *byte_end))) {
                                    return true;
                                }
                            }
                        }
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
    for ((mi, offset), key) in strings {
        let at = ((mi, offset), key.clone());
        let mut twins = pf
            .sites
            .iter()
            .filter(|s| s.is_call && (s.module, s.byte_offset) == (mi, offset) && s.key == key)
            .peekable();
        if certified.get(&at) == Some(&true)
            && twins.peek().is_some()
            && twins.all(|s| {
                s.verdict == Verdict::NativeMember
                    && s.receiver_prims == prim::STR
                    && s.receiver_values.is_empty()
            })
        {
            let sites = &mut pf.bindings[mi as usize].method_sites;
            use perry_hir::folded_call::SITE_DIRECT_STRING;
            if let Some((_, _, flags)) = sites
                .iter_mut()
                .find(|(s, k, _)| *s == Site::Callee(offset) && *k == key)
            {
                *flags |= SITE_DIRECT_STRING;
            } else {
                sites.push((Site::Callee(offset), key, SITE_DIRECT_STRING));
            }
        } else {
            certified.insert(at, false);
        }
    }
    for ((mi, offset), key) in arrays {
        if certified.get(&((mi, offset), key.clone())) == Some(&true) {
            if let Some((_, _, flags)) = pf.bindings[mi as usize]
                .method_sites
                .iter_mut()
                .find(|(site, k, _)| *site == Site::Callee(offset) && *k == key)
            {
                *flags |= perry_hir::folded_call::SITE_DIRECT_ARRAY;
            }
        }
    }
    for ((mi, offset), key) in maps {
        let at = ((mi, offset), key.clone());
        if certified.get(&at) == Some(&true)
            && pf
                .sites
                .iter()
                .filter(|s| s.is_call && (s.module, s.byte_offset) == (mi, offset) && s.key == key)
                .all(|s| s.verdict == Verdict::NativeMember && s.receiver_prims == 0)
        {
            if let Some((_, _, flags)) = pf.bindings[mi as usize]
                .method_sites
                .iter_mut()
                .find(|(site, k, _)| *site == Site::Callee(offset) && k == &key)
            {
                *flags |= SITE_DIRECT_MAP;
            }
        } else {
            certified.insert(at, false);
        }
    }
    for s in &mut pf.sites {
        s.builtin_dispatch = s.is_call
            && certified
                .get(&((s.module, s.byte_offset), s.key.clone()))
                .copied()
                .unwrap_or(false);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn native_dispatch_visits_computed_members_and_decorator_arguments() {
        let ast = perry_parser::parse_typescript(
            "class P { value = 0; m(x: any) { console.log('body'); } } console.log('init');",
            "t.ts",
        )
        .unwrap();
        let mut hir = perry_hir::lower_module(&ast, "t", "t.ts").unwrap();
        let call = hir
            .init
            .iter()
            .find_map(|s| match s {
                Stmt::Expr(e @ Expr::Call { .. }) => Some(e.clone()),
                _ => None,
            })
            .expect("live init call");
        let decorator = perry_hir::Decorator {
            name: "decorator".into(),
            args: vec![call.clone()],
            is_factory: true,
            is_reflect_metadata: false,
        };
        let c = &mut hir.classes[0];
        c.decorators.push(decorator.clone());
        c.fields[0].decorators.push(decorator.clone());
        let mut method = c.methods.remove(0);
        method.decorators.push(decorator.clone());
        method.params[0].default = Some(call.clone());
        method.params[0].decorators.push(decorator);
        c.computed_members.push(perry_hir::ClassComputedMember {
            key_expr: call,
            function: method,
            is_static: false,
            kind: perry_hir::ClassComputedMemberKind::Method,
            source_order: 1,
        });
        let mut calls = 0;
        module(&hir, &mut |e| {
            if matches!(e, Expr::Call { callee, .. }
                if matches!(callee.as_ref(), Expr::PropertyGet { property, .. } if property == "log"))
            {
                calls += 1;
            }
        });
        assert_eq!(
            calls, 8,
            "every generated call location participates in twin agreement"
        );
    }
}
