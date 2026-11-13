//! Contexts for immutable HotKey receiver facts. Unknown receivers retain the
//! ordinary erased-target union; a proven receiver follows its own initializer.
use crate::{
    graph::{Graph, Node},
    instructions::Format,
};
use object::{Object, ObjectSection, ObjectSymbol, RelocationTarget};
use std::{
    collections::{BTreeSet, HashMap},
    sync::Arc,
};

type Keys = HashMap<(Arc<str>, String), String>;
pub(crate) fn method(raw: &str) -> bool {
    let raw = raw.strip_prefix(".text.").unwrap_or(raw);
    let name = format!("{:#}", rustc_demangle::demangle(raw));
    name.contains("perry_runtime::tls_hot::HotKey<")
        && (name.contains("::resolve_and_cache") || name.contains("::get_slow"))
}

pub(crate) fn data_keys(
    file: &object::File<'_>,
    fmt: Format,
) -> anyhow::Result<HashMap<String, String>> {
    let mut keys = HashMap::new();
    let mut by_section = HashMap::<_, Vec<_>>::new();
    for symbol in file
        .symbols()
        .filter(|s| s.kind() == object::SymbolKind::Data)
    {
        if let Some(section) = symbol.section_index() {
            by_section.entry(section).or_default().push(symbol);
        }
    }
    for section in file
        .sections()
        .filter(|s| s.kind() == object::SectionKind::Data)
    {
        let Some(labels) = by_section.get(&section.index()) else {
            continue;
        };
        for (offset, relocation) in section.relocations() {
            let RelocationTarget::Symbol(index) = relocation.target() else {
                continue;
            };
            let symbol = file.symbol_by_index(index)?;
            let mut target = symbol.name()?.to_owned();
            if symbol.kind() == object::SymbolKind::Section {
                target = file
                    .section_by_index(symbol.section_index().unwrap())?
                    .name()?
                    .to_owned();
                target = target.strip_prefix(".text.").unwrap_or(&target).to_owned();
            }
            let target = fmt.norm(&target);
            let dem = format!("{:#}", rustc_demangle::demangle(&target));
            let Some(owner) = dem.strip_suffix("::resolve") else {
                continue;
            };
            for label in labels {
                let name = fmt.norm(label.name()?);
                if format!("{:#}", rustc_demangle::demangle(&name)) != owner
                    || offset < label.address() - section.address()
                    || offset >= label.address() - section.address() + label.size()
                {
                    continue;
                }
                keys.insert(name, target.clone());
                if label.address() == section.address() {
                    keys.insert(fmt.norm(section.name()?), target.clone());
                }
            }
        }
    }
    Ok(keys)
}

pub(crate) fn specialize(g: &mut Graph, keys: &Keys) {
    let original = g.nodes.len();
    let mut contexts = HashMap::new();
    for caller in 0..original {
        rewrite(g, keys, caller, None, &mut contexts);
    }
    g.pred = vec![Vec::new(); g.nodes.len()];
    for (i, targets) in g.succ.iter().enumerate() {
        for &target in targets {
            g.pred[target].push(i);
        }
    }
    g.edge_count = g.succ.iter().map(Vec::len).sum();
}

fn rewrite(
    g: &mut Graph,
    keys: &Keys,
    caller: usize,
    receiver: Option<&str>,
    contexts: &mut HashMap<(usize, String), usize>,
) {
    let facts = g.nodes[caller].receiver_calls.clone();
    let targets = g.succ[caller].clone();
    for target in targets {
        if !g.nodes[target].names.iter().any(|n| method(n)) {
            continue;
        }
        let mut selected = BTreeSet::new();
        let mut unknown = false;
        let mut found = false;
        for (name, values) in &facts {
            if !g.nodes[target].names.contains(name) && g.nodes[target].sec.as_ref() != name {
                continue;
            }
            found = true;
            let Some(values) = values else {
                unknown = true;
                continue;
            };
            for key in values {
                let key = if key == "$self" {
                    receiver
                } else {
                    Some(key.as_str())
                };
                match key {
                    Some(key)
                        if keys.contains_key(&(g.nodes[caller].obj.clone(), key.to_owned())) =>
                    {
                        selected.insert(key.to_owned());
                    }
                    _ => unknown = true,
                }
            }
        }
        if !found {
            continue;
        }
        if !unknown {
            g.succ[caller].retain(|&t| t != target);
        }
        for key in selected {
            let specialized = context(g, keys, target, &key, contexts);
            if !g.succ[caller].contains(&specialized) {
                g.succ[caller].push(specialized);
            }
        }
    }
}

fn context(
    g: &mut Graph,
    keys: &Keys,
    target: usize,
    key: &str,
    contexts: &mut HashMap<(usize, String), usize>,
) -> usize {
    let id = (target, key.to_owned());
    if let Some(&existing) = contexts.get(&id) {
        return existing;
    }
    let mut node: Node = g.nodes[target].clone();
    let index = g.nodes.len();
    let mut successors = g.succ[target].clone();
    // Both methods' self argument is the HotKey handle. Only the erased
    // resolver is refined; all ordinary callees and initializer edges stay.
    if node
        .names
        .iter()
        .any(|n| format!("{:#}", rustc_demangle::demangle(n)).contains("::resolve_and_cache"))
    {
        let Some(resolver) = keys.get(&(node.obj.clone(), key.to_owned())) else {
            return target;
        };
        let matches: Vec<_> = g.nodes[..index]
            .iter()
            .enumerate()
            .filter(|(_, n)| n.names.contains(resolver))
            .map(|(i, _)| i)
            .collect();
        if matches.is_empty() || node.indirect_calls + node.indirect_jmps != 1 {
            return target;
        }
        node.indirect_calls = 0;
        node.indirect_jmps = 0;
        successors.extend(matches);
    }
    contexts.insert(id, index);
    node.names.push(format!("receiver={key}"));
    g.nodes.push(node);
    g.succ.push(successors);
    rewrite(g, keys, index, Some(key), contexts);
    index
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        graph::{classify, Mode},
        table::Class,
    };
    use std::collections::HashSet;
    fn fixture(values: Option<BTreeSet<String>>) -> (Graph, Keys) {
        let method = "<perry_runtime::tls_hot::HotKey<T>>::resolve_and_cache";
        let mut nodes = Vec::new();
        for name in ["js_touch", method, "tls_leaf", "tls_alloc", "allocation"] {
            let mut n = Node::new(
                "fixture.o".into(),
                ".text".into(),
                nodes.len() as u64,
                Format::Elf,
            );
            n.names.push(name.into());
            nodes.push(n);
        }
        nodes[0].calls.insert(method.into());
        nodes[0].receiver_calls.push((method.into(), values));
        nodes[1].indirect_calls = 1;
        nodes[3].calls.insert("allocation".into());
        let globals = nodes
            .iter()
            .flat_map(|n| n.names.iter().map(|s| (n.obj.clone(), s.clone())))
            .collect::<HashSet<_>>();
        (
            Graph::new(nodes, globals).unwrap(),
            HashMap::from([
                (("fixture.o".into(), "good".into()), "tls_leaf".into()),
                (("fixture.o".into(), "bad".into()), "tls_alloc".into()),
            ]),
        )
    }
    fn class(g: &Graph) -> Class {
        let rules = crate::load_rules("js exact allocation -- fixture allocation\nindirect regex HotKey => regex ^tls_ -- conservative erased union\n").unwrap();
        let c = classify(g, &rules, None).unwrap();
        assert!(!c.witness_path(g, "js_touch", Mode::L2).is_empty());
        c.table["js_touch"].class
    }
    #[test]
    fn known_receiver_excludes_unrelated_initializer_and_widening_is_red() {
        let (mut g, keys) = fixture(Some(BTreeSet::from(["good".into()])));
        assert_eq!(class(&g), Class::Reenters);
        specialize(&mut g, &keys);
        assert_eq!(class(&g), Class::Leaf);
        let (mut g, keys) = fixture(Some(BTreeSet::from(["good".into(), "bad".into()])));
        specialize(&mut g, &keys);
        assert_eq!(class(&g), Class::Reenters);
    }
    #[test]
    fn unknown_receiver_and_real_initializer_allocation_stay_red() {
        let (mut g, keys) = fixture(None);
        specialize(&mut g, &keys);
        assert_eq!(class(&g), Class::Reenters);
        let (mut g, keys) = fixture(Some(BTreeSet::from(["good".into()])));
        g.nodes[2].calls.insert("allocation".into());
        g.succ[2].push(4);
        specialize(&mut g, &keys);
        assert_eq!(class(&g), Class::Reenters);
    }
}

#[cfg(test)]
#[path = "receiver_archive_tests.rs"]
mod archive_tests;
