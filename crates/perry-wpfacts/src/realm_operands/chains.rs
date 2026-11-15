use crate::model::*;
use crate::pts::FxHashSet;
use perry_hir::realm_reach_scope::Layer;
use std::collections::BTreeSet;

pub(super) fn realm_layer(name: impl Into<String>) -> Layer {
    Layer {
        realm: Some(name.into()),
        own: BTreeSet::new(),
        token: None,
    }
}

pub(super) fn namespace(ns: BuiltinNs) -> Option<String> {
    match ns {
        BuiltinNs::Module(name) => {
            let name = name.to_string();
            crate::builtins::members::realm_entry(&name).map(|e| e.name.into())
        }
        _ => {
            let names: BTreeSet<_> = crate::native_effects::realm()
                .iter()
                .filter(|e| crate::builtins::ns_by_name(e.name) == Some(ns))
                .map(|e| e.name)
                .collect();
            (names.len() == 1).then(|| names.into_iter().next().unwrap().into())
        }
    }
}

/// A collapsed builtin-prototype token is never a proof of Object.prototype.
/// Static seeds may be decoded only at their creating token, before any relink.
pub(super) fn chains(
    m: &Model,
    token: Tok,
    seen: &mut FxHashSet<Tok>,
    opaque: Tok,
) -> Option<Vec<Vec<Layer>>> {
    if token == TOP || token == opaque || m.is_marker(token) || !seen.insert(token) {
        return None;
    }
    let kind = &m.toks[token as usize];
    let mut own = BTreeSet::new();
    let realm = match kind {
        TokKind::Builtin(ns) => Some(namespace(*ns)?),
        TokKind::BuiltinFn(Native::Member { identity, .. }) => Some((*identity).into()),
        TokKind::NativeProto(name) => {
            let names: BTreeSet<_> = crate::native_effects::realm()
                .iter()
                .filter(|e| {
                    e.name == name
                        || e.name.strip_suffix(".prototype").is_some_and(|n| {
                            n == name || n.rsplit('.').next() == Some(name.as_str())
                        })
                })
                .filter(|e| e.kind == perry_hir::realm_manifest::RealmKind::Prototype)
                .map(|e| e.name)
                .collect();
            if names.len() != 1 {
                return None;
            }
            Some(names.into_iter().next().unwrap().into())
        }
        TokKind::ObjectProto | TokKind::Top | TokKind::BuiltinFn(Native::Any) => return None,
        TokKind::Obj(tm, _) => {
            if !m.hard.contains_key(&token) && !m.escaped.contains_key(&token) {
                own.extend(
                    m.tmpls[*tm as usize]
                        .definite
                        .iter()
                        .map(|&a| m.atoms[a as usize].clone()),
                );
            }
            None
        }
        TokKind::Proto(tm) => {
            if !m.hard.contains_key(&token) && !m.escaped.contains_key(&token) {
                own.extend(
                    m.tmpls[*tm as usize]
                        .proto_members
                        .iter()
                        .map(|&a| m.atoms[a as usize].clone()),
                );
            }
            None
        }
        TokKind::ClassObj(tm) => {
            if !m.hard.contains_key(&token) && !m.escaped.contains_key(&token) {
                own.extend(
                    m.tmpls[*tm as usize]
                        .static_methods
                        .iter()
                        .map(|&a| m.atoms[a as usize].clone()),
                );
                own.insert("prototype".into());
                own.insert("name".into());
                own.insert("length".into());
            }
            None
        }
        TokKind::Func(_, _) | TokKind::Bound(_) => None,
        TokKind::ModNs(_) => return None,
    };
    if let Some(name) = realm {
        // A runtime object's changed chain cannot be reconstructed from declarations.
        if m.proto_node
            .get(&token)
            .is_some_and(|pn| m.given_proto.iter().any(|(_, p)| p == pn))
            || m.native_given.iter().any(|(t, _)| *t == token || *t == TOP)
        {
            return None;
        }
        seen.remove(&token);
        return Some(vec![vec![realm_layer(name)]]);
    }
    let layer = Layer {
        realm: None,
        own,
        token: Some(token),
    };
    let pn = m.proto_node.get(&token).copied();
    if pn.is_some_and(|node| {
        let node = &m.nodes[node as usize];
        node.pts.contains_tok(TOP) || node.prim & !prim::NULL != 0
    }) {
        return None;
    }

    let custom = pn.is_some_and(|pn| m.given_proto.iter().any(|(_, p)| *p == pn));
    let default = match kind {
        TokKind::Obj(tm, _) => {
            let template = &m.tmpls[*tm as usize];
            match &template.kind {
                TmplKind::Class { .. } => {
                    let proto = *m
                        .tok_index
                        .get(&TokKind::Proto(m.constructor_origin(*tm)))?;
                    chains(m, proto, seen, opaque)?
                }
                TmplKind::Arr { .. } => vec![vec![realm_layer("Array.prototype")]],
                TmplKind::Lit { .. } | TmplKind::Args { .. } if !custom => {
                    vec![vec![realm_layer("Object.prototype")]]
                }
                TmplKind::BObj { what, .. } if !custom => {
                    use crate::builtins::members::{box_chain, BoxChain};
                    let proto = format!("{what}.prototype");
                    if crate::builtins::members::realm_entry(what)
                        .is_some_and(|e| e.kind == perry_hir::realm_manifest::RealmKind::Prototype)
                    {
                        vec![vec![realm_layer(*what)]]
                    } else if crate::builtins::members::realm_entry(&proto).is_some() {
                        vec![vec![realm_layer(proto)]]
                    } else {
                        match box_chain(what) {
                            BoxChain::Object => vec![vec![realm_layer("Object.prototype")]],
                            BoxChain::Array => vec![vec![realm_layer("Array.prototype")]],
                            BoxChain::Null => vec![vec![]],
                            BoxChain::Unknown => return None,
                        }
                    }
                }
                _ => node_chains(m, pn?, seen, opaque)?,
            }
        }
        TokKind::Proto(tm) => {
            let template = &m.tmpls[*tm as usize];
            if custom || template.parent_value.is_some() {
                node_chains(m, pn?, seen, opaque)?
            } else if let Some(parent) = template.parent {
                let proto = *m
                    .tok_index
                    .get(&TokKind::Proto(m.constructor_origin(parent)))?;
                chains(m, proto, seen, opaque)?
            } else if let Some((_, name)) = &template.native_parent {
                let proto = *m.tok_index.get(&TokKind::NativeProto(name.clone()))?;
                chains(m, proto, seen, opaque)?
            } else if template.null_proto {
                vec![vec![]]
            } else {
                vec![vec![realm_layer("Object.prototype")]]
            }
        }
        TokKind::ClassObj(tm)
            if !custom
                && m.tmpls[*tm as usize].parent.is_none()
                && m.tmpls[*tm as usize].parent_value.is_none()
                && m.tmpls[*tm as usize].native_parent.is_none() =>
        {
            vec![vec![realm_layer("Function.prototype")]]
        }
        TokKind::Func(f, _) if !custom && m.funcs[*f as usize].gen_proto.is_none() => {
            use perry_hir::fn_proto::FnProtoKind;
            let name = match m.funcs[*f as usize].proto? {
                FnProtoKind::Function => "Function.prototype",
                FnProtoKind::Async => "%AsyncFunctionPrototype%",
                FnProtoKind::Generator => "%GeneratorFunctionPrototype%",
                FnProtoKind::AsyncGenerator => "%AsyncGeneratorFunctionPrototype%",
            };
            vec![vec![realm_layer(name)]]
        }
        _ => node_chains(m, pn?, seen, opaque)?,
    };
    // A changed prototype may be a collapsed builtin token: fail closed.
    let mut result = if custom {
        node_chains(m, pn?, seen, opaque)?
    } else {
        default
    };
    for chain in &mut result {
        chain.insert(0, layer.clone());
    }
    seen.remove(&token);
    Some(result)
}

pub(super) fn node_chains(
    m: &Model,
    node: NodeId,
    seen: &mut FxHashSet<Tok>,
    opaque: Tok,
) -> Option<Vec<Vec<Layer>>> {
    let node = &m.nodes[node as usize];
    let tokens = node.pts.to_sorted();
    let mut out = Vec::new();
    for token in tokens {
        out.extend(chains(m, token, seen, opaque)?);
    }
    if node.prim & !prim::NULL != 0 {
        return None;
    }
    if node.prim & prim::NULL != 0 {
        out.push(Vec::new());
    }
    (!out.is_empty()).then_some(out)
}
