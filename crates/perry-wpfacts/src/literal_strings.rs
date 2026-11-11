//! Literal strings: a binding or a field whose every String is the value
//! of a program string literal (`BindingFacts::literal_strings`,
//! `FieldFacts::literal_strings`).
//!
//! It is read off the one string value lattice the solver already keeps
//! (`Node::strs`, `solve::strs`): a string literal is a source node holding
//! its one constant, a copy edge and the nullish filters hand a set on
//! unchanged, and every other String birth (a concatenation, a method
//! result, a coercion, a value of ⊤ or of another unit) is any string
//! (`STRS_TOP`). So a node whose set is finite, and which holds no ⊤ and no
//! bare marker, holds a String only by an unbroken chain of copies from a
//! literal: at run time that String is the literal's own value, which is
//! born shared (`StringHeader::refcount == 0`: an image record or a pool
//! atom) and which nothing appends to in place. A store or an alias of
//! such a value therefore has no string to demote
//! (`js_string_addref_if_heap_string`), which is what codegen consumes
//! (`perry-codegen` `expr::literal_strings`).
//!
//! The fact is about the Strings only: the node may hold values of other
//! kinds beside them.
//!
//! `PERRY_FACTS_LITERAL_STRINGS=0` exports none. The negative control
//! `PERRY_FACTS_SABOTAGE=litstr` claims it for every node that may hold a
//! String, a wrong claim the check-mode witness at the consuming store or
//! alias must catch. The dump (`PERRY_FACTS_DUMP`) prints one
//! `# lit_strings` line per local and per field the fact holds for, with
//! its constants.

use crate::gen::Gen;
use crate::model::{prim, Atom, Model, NodeId, STRS_TOP, TOP};

/// `PERRY_FACTS_LITERAL_STRINGS=0` turns the fact off.
pub fn enabled() -> bool {
    !matches!(
        std::env::var("PERRY_FACTS_LITERAL_STRINGS").as_deref(),
        Ok("0") | Ok("off") | Ok("false")
    )
}

/// The string constants `nodes` hold together when every String any of
/// them holds is a literal's (module docs): some node holds a String, none
/// holds any string, ⊤ or a bare marker. Sorted, distinct.
pub(crate) fn constants(m: &Model, nodes: &[NodeId]) -> Option<Vec<Atom>> {
    if !m.literal_strings {
        return None;
    }
    let mut atoms: Vec<Atom> = Vec::new();
    let mut string = false;
    for &n in nodes {
        let node = &m.nodes[n as usize];
        let unknown = node
            .pts
            .to_sorted()
            .into_iter()
            .any(|t| t == TOP || m.bare_marker(t));
        if node.prim & prim::STR == 0 && !unknown {
            continue;
        }
        string = true;
        if m.literal_strings_sabotage {
            continue;
        }
        if unknown || node.strs == STRS_TOP {
            return None;
        }
        atoms.extend_from_slice(m.str_atoms(node.strs));
    }
    if !string {
        return None;
    }
    atoms.sort_unstable();
    atoms.dedup();
    Some(atoms)
}

/// `constants` holds for `nodes`.
pub(crate) fn holds(m: &Model, nodes: &[NodeId]) -> bool {
    constants(m, nodes).is_some()
}

/// The probe (module docs): the locals, then the fields of `templates`
/// that carry the fact.
pub(crate) fn probe(g: &Gen, templates: &[crate::derive::TemplateFacts]) -> Vec<String> {
    let m = &g.m;
    if !m.literal_strings {
        return Vec::new();
    }
    let show = |atoms: &[Atom]| {
        atoms
            .iter()
            .map(|&a| format!("{:?}", m.atoms[a as usize]))
            .collect::<Vec<_>>()
            .join(",")
    };
    let mut out = Vec::new();
    for (mi, info) in g.mods.iter().enumerate() {
        let mut ls: Vec<(u32, NodeId)> = info.locals.iter().map(|(&i, &n)| (i, n)).collect();
        ls.sort_unstable();
        for (id, n) in ls {
            if let Some(atoms) = constants(m, &[n]) {
                let name = m.node_names.get(n as usize).map_or("?", |s| s.as_str());
                out.push(format!(
                    "# lit_strings module={mi} local={id} name={} set=[{}]",
                    name.replace(' ', "_"),
                    show(&atoms)
                ));
            }
        }
    }
    for tf in templates {
        let ts = m.tmpl_toks(tf.id);
        for f in tf.fields.iter().filter(|f| f.literal_strings) {
            let key = if f.private {
                m.private_atoms.get(&f.key).copied()
            } else {
                m.atom_index.get(&f.key).copied()
            };
            let nodes: Vec<NodeId> = key
                .map(|a| {
                    ts.iter()
                        .filter_map(|t| m.fields.get(&(*t, a)).copied())
                        .collect()
                })
                .unwrap_or_default();
            out.push(format!(
                "# lit_strings field {}.{} set=[{}]",
                tf.display,
                f.key,
                show(&constants(m, &nodes).unwrap_or_default())
            ));
        }
    }
    out
}
