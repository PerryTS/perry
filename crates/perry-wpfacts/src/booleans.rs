//! Booleans: a binding or a field whose every value is `true` or `false`
//! (`BindingFacts::booleans`, `FieldFacts::booleans`).
//!
//! It is read off the value lattice as it stands after the solve: the
//! nodes together hold the Boolean primitive and nothing else, no other
//! primitive (so no `undefined` of a missing argument, an uninitialized
//! `let` or an absent field), no object or function token, no ⊤ and no
//! marker of another unit. The value in such a slot is one of the two
//! words `TAG_FALSE` / `TAG_TRUE`, so its truthiness is one compare with
//! `TAG_TRUE`: no number test, no tag range test and no runtime call,
//! which is what codegen consumes (`perry-codegen` `expr::booleans`). A
//! TypeScript `boolean` annotation proves nothing of that (a caller may
//! pass anything, an optional parameter may be missing): the fact is about
//! the values the program stores.
//!
//! `PERRY_FACTS_BOOLEANS=0` exports none. The negative control
//! `PERRY_FACTS_SABOTAGE=bools` claims it for every node that may hold a
//! Boolean beside anything else, a wrong claim the check-mode witness at
//! the consuming test must catch. The dump (`PERRY_FACTS_DUMP`) prints one
//! `# booleans` line per local and per field the fact holds for.

use crate::gen::Gen;
use crate::model::{prim, Model, NodeId};

/// `PERRY_FACTS_BOOLEANS=0` turns the fact off.
pub fn enabled() -> bool {
    !matches!(
        std::env::var("PERRY_FACTS_BOOLEANS").as_deref(),
        Ok("0") | Ok("off") | Ok("false")
    )
}

/// The switch and the negative control (module docs).
pub(crate) fn configure(m: &mut Model, sabotage: Option<&str>) {
    m.booleans = enabled();
    m.booleans_sabotage = sabotage == Some("bools");
}

/// Every value `nodes` hold together is a Boolean (module docs): some node
/// holds one, and none holds any other primitive or any token.
pub(crate) fn holds(m: &Model, nodes: &[NodeId]) -> bool {
    if !m.booleans {
        return false;
    }
    let mut prims = 0;
    let mut tokens = false;
    for &n in nodes {
        let node = &m.nodes[n as usize];
        prims |= node.prim;
        tokens |= !node.pts.is_empty();
    }
    if m.booleans_sabotage {
        return prims & prim::BOOL != 0;
    }
    prims == prim::BOOL && !tokens
}

/// The probe (module docs): the locals, then the fields of `templates`
/// that carry the fact.
pub(crate) fn probe(g: &Gen, templates: &[crate::derive::TemplateFacts]) -> Vec<String> {
    let m = &g.m;
    if !m.booleans {
        return Vec::new();
    }
    let mut out = Vec::new();
    let nodes = crate::bindings::BindingNodes::new(m);
    for (mi, info) in g.mods.iter().enumerate() {
        let mut ls: Vec<(u32, NodeId)> = info.locals.iter().map(|(&i, &n)| (i, n)).collect();
        ls.sort_unstable();
        for (id, n) in ls {
            if holds(m, &nodes.of(n)) {
                let name = m.node_names.get(n as usize).map_or("?", |s| s.as_str());
                out.push(format!(
                    "# booleans module={mi} local={id} name={}",
                    name.replace(' ', "_")
                ));
            }
        }
    }
    for tf in templates {
        for f in tf.fields.iter().filter(|f| f.booleans) {
            out.push(format!("# booleans field {}.{}", tf.display, f.key));
        }
    }
    out
}
