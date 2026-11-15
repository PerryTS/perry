use super::chains::{chains, namespace};
use crate::derive::{element::receiver_set, ReceiverValue};
use crate::model::*;
use crate::pts::FxHashSet;
use perry_hir::realm_reach_scope::{Layer, Values};
use std::collections::BTreeSet;

pub(super) fn ordinary_class(m: &Model, mut template: TmplId) -> bool {
    let mut seen = FxHashSet::default();
    while seen.insert(template) {
        let row = &m.tmpls[template as usize];
        if row.native_parent.is_some()
            || row.parent_value.is_some()
            || m.tok_index
                .get(&TokKind::ClassObj(template))
                .is_some_and(|t| m.super_relink.contains(t))
        {
            return false;
        }
        match row.parent {
            Some(parent) => template = parent,
            None => return true,
        }
    }
    false
}

pub(super) fn program_bodies(
    m: &Model,
    tokens: &[Tok],
    prims: u16,
    seen: &mut FxHashSet<Tok>,
) -> bool {
    prims == 0
        && !tokens.is_empty()
        && tokens.iter().all(|&t| {
            if !seen.insert(t) {
                return false;
            }
            let result = match m.toks[t as usize] {
                TokKind::Func(f, _) => {
                    let body = &m.funcs[f as usize];
                    body.func_id != u32::MAX
                        && !m
                            .resolvers
                            .values()
                            .chain(m.executors.values())
                            .any(|t| m.tok_fn(*t) == f)
                }
                TokKind::ClassObj(tm) => ordinary_class(m, tm),
                TokKind::Bound(b) => {
                    let node = &m.nodes[m.bound[b as usize].0 as usize];
                    program_bodies(m, &node.pts.to_sorted(), node.prim, seen)
                }
                _ => false,
            };
            seen.remove(&t);
            result
        })
}

pub(super) fn value_set(
    m: &Model,
    nodes: &[NodeId],
    opaque: Tok,
) -> (Vec<ReceiverValue>, u16, Values) {
    let set = receiver_set(m, nodes);
    let values = set.values(m);
    // Noncallable alternatives can throw; they cannot run unknown code.
    let mut scope = Values {
        top: (set.toks.is_empty() && set.prims == 0)
            || values.iter().any(|v| v.marker)
            || set.top(m),
        program_bodies: program_bodies(m, &set.toks, 0, &mut FxHashSet::default()),
        ..Values::default()
    };
    for (bit, prototype) in [
        (prim::STR, "String.prototype"),
        (prim::NUM, "Number.prototype"),
        (prim::BOOL, "Boolean.prototype"),
        (prim::BIGINT, "BigInt.prototype"),
        (prim::SYMBOL, "Symbol.prototype"),
    ] {
        if set.prims & bit != 0 {
            scope.primitive_prototypes.insert(prototype.into());
        }
    }
    for token in &set.toks {
        match chains(m, *token, &mut FxHashSet::default(), opaque) {
            Some(chains) => scope.chains.extend(chains),
            None => scope.top = true,
        }
    }
    scope.known_callees = (!set.toks.is_empty() || set.prims != 0)
        && set.toks.iter().all(|token| {
            if *token == TOP || m.is_marker(*token) {
                return false;
            }
            match m.toks[*token as usize] {
                TokKind::Func(_, _) => program_bodies(m, &[*token], 0, &mut FxHashSet::default()),
                TokKind::ClassObj(tm) => ordinary_class(m, tm),
                TokKind::Builtin(ns) => namespace(ns)
                    .and_then(|name| crate::builtins::members::realm_entry(&name))
                    .is_some_and(|row| {
                        row.kind == perry_hir::realm_manifest::RealmKind::Constructor
                    }),
                TokKind::Bound(_) => program_bodies(m, &[*token], 0, &mut FxHashSet::default()),
                TokKind::BuiltinFn(Native::Member { .. }) => true,
                TokKind::Obj(tm, _) => match &m.tmpls[tm as usize].kind {
                    TmplKind::Lit { .. }
                    | TmplKind::Arr { .. }
                    | TmplKind::Args { .. }
                    | TmplKind::FnInst { .. }
                    | TmplKind::ObjCreate { .. } => true,
                    TmplKind::Class { .. } => ordinary_class(m, tm),
                    TmplKind::BObj { what, .. } => !matches!(
                        crate::builtins::members::box_chain(what),
                        crate::builtins::members::BoxChain::Unknown
                    ),
                },
                _ => false,
            }
        });
    (values, set.prims, scope)
}

pub(super) fn known_own_callees(m: &Model, token: Tok, key: Atom, opaque: Tok) -> bool {
    if !m.own_props_modelled(token)
        || m.hard.contains_key(&token)
        || m.escaped.contains_key(&token)
        || m.accessor_keys
            .get(&token)
            .is_some_and(|keys| !keys.is_empty())
    {
        return false;
    }
    let mut nodes = Vec::new();
    nodes.extend(m.fields.get(&(token, key)).copied());
    if m.is_elem_array(token) && !crate::solve::numeric_key(&m.atoms[key as usize]) {
        nodes.extend(m.named_any.get(&token).copied());
    } else {
        nodes.extend(m.field_any.get(&token).copied());
        nodes.extend(m.num_fields.get(&token).copied());
    }
    nodes.into_iter().all(|node| {
        let values = &m.nodes[node as usize];
        (values.prim == 0 && values.pts.is_empty()) || value_set(m, &[node], opaque).2.known_callees
    })
}

pub(super) fn known_chain_callee(chain: &[Layer], key: &str, program: bool) -> bool {
    use perry_hir::realm_manifest::{RealmKind, RealmOwnValue};
    for layer in chain {
        if let Some(name) = &layer.realm {
            let mut next = Some(name.as_str());
            let mut seen = BTreeSet::new();
            while let Some(name) = next.filter(|n| seen.insert((*n).to_string())) {
                let Some(row) = crate::builtins::members::realm_entry(name) else {
                    return false;
                };
                if let Some(property) = row.own.iter().find(|p| p.key == key) {
                    return match property.value {
                        RealmOwnValue::Reference(target) => {
                            crate::builtins::members::realm_entry(target).is_some_and(|r| {
                                matches!(
                                    r.kind,
                                    RealmKind::Function
                                        | RealmKind::Method
                                        | RealmKind::Constructor
                                )
                            })
                        }
                        RealmOwnValue::Accessor { .. } => false,
                        _ => true,
                    };
                }
                next = row.proto;
            }
            return true;
        }
        if layer.own.contains(key) {
            return program;
        }
    }
    true
}
