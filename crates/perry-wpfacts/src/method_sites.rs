//! Method sites: what the solve knows about the receiver of a by-name
//! method call `recv.m(..)` (`ModuleBindings::method_sites`).
//!
//! `perry-codegen` lowers a method call to a built-in's body when it names
//! one (`a.fill(0)`, `a.at(i)`, a folded `Expr::ArrayIndexOf`). The static
//! type of the receiver expression is an annotation: the value behind
//! `a: Float64Array` may be an ordinary object with its own `fill`, a class
//! instance, a subclass instance, `null`. Which built-in a site may run is a
//! fact about the VALUES the receiver holds, answered here per site:
//!
//! - **a typed-array receiver** ([`TYPED_ARRAY`]): at every copy of the
//!   site, and every summary instance's copy of it, the receiver node holds
//!   typed arrays the realm bore (`new Float64Array(n)`, a built-in's typed
//!   array result) and nothing else: no primitive (so no `null` and no
//!   `undefined`), no ⊤, no marker, no object of another birth, no instance
//!   of a program class (a subclass of a typed-array constructor is one).
//!   It says nothing about the MEMBER: an own property or a prototype write
//!   may still shadow the method (no typed-array member has a
//!   `NativeMember` verdict; the prototypes carry no shadow bit).
//!
//! A site is keyed as codegen keys it (`perry_hir::folded_call::Site`): an
//! unfolded call by its callee read's offset, a folded node by its
//! receiver, each with the method name. Two sites under one key (a chain
//! `x.m().m()`, a body walked twice) hold a fact only when both do.
//!
//! No fact for a site means its receiver may be anything, and the site is
//! the ordinary method call (a real `Get` and `Call`). There is no fact at
//! all in an aborted or per-unit solve or when interpreted code can run.
//!
//! `PERRY_FACTS_TYPED_ARRAY_SITES=0` exports no typed-array fact (codegen
//! then trusts the annotation, as before). The negative control
//! `PERRY_FACTS_SABOTAGE=tasite` claims every site a typed-array receiver.
//! The dump (`PERRY_FACTS_DUMP`) prints one `# method_site` line per site:
//! `module= site=callee:N|recv:N|local:N key= typed_array=1`, or
//! `typed_array=0 refused=<why>`.

use crate::bindings::ModuleBindings;
use crate::gen::Gen;
use crate::model::{Model, NodeId, TmplKind, TokKind, TOP};
use perry_hir::folded_call::Site;
use std::collections::BTreeMap;

/// The site's receiver holds only typed arrays the realm bore.
pub const TYPED_ARRAY: u8 = perry_hir::folded_call::SITE_TYPED_ARRAY;

/// `PERRY_FACTS_TYPED_ARRAY_SITES=0` turns the typed-array receiver fact
/// off.
pub fn typed_array_enabled() -> bool {
    !matches!(
        std::env::var("PERRY_FACTS_TYPED_ARRAY_SITES").as_deref(),
        Ok("0") | Ok("off") | Ok("false")
    )
}

/// Why node `n` may hold something that is no realm-born typed array
/// (`None`: it holds such typed arrays and nothing else).
fn not_typed_arrays(m: &Model, n: NodeId) -> Option<&'static str> {
    let node = &m.nodes[n as usize];
    if node.dropped {
        return Some("receiver:dropped");
    }
    if node.prim != 0 {
        return Some("receiver:primitive");
    }
    if node.pts.is_empty() {
        return Some("receiver:empty");
    }
    if node.pts.contains_tok(TOP) {
        return Some("receiver:top");
    }
    for t in node.pts.to_sorted() {
        if m.is_marker(t) {
            return Some("receiver:marker");
        }
        let TokKind::Obj(tm, _) = m.toks[t as usize] else {
            return Some("receiver:not-typed-array");
        };
        if !matches!(
            m.tmpls[tm as usize].kind,
            TmplKind::BObj {
                what: "TypedArray",
                ..
            }
        ) {
            return Some("receiver:not-typed-array");
        }
    }
    None
}

fn site_text(site: Site) -> String {
    match site {
        Site::Callee(o) => format!("callee:{o}"),
        Site::Recv(o) => format!("recv:{o}"),
        Site::Local(id) => format!("local:{id}"),
    }
}

/// Fill the method-site list of every module (module docs). `interpreted`:
/// interpreted code can run (`ProgramFacts::interp_guard`).
pub(crate) fn collect(
    g: &Gen<'_>,
    bindings: &mut [ModuleBindings],
    interpreted: bool,
    sabotage_typed_array: bool,
    mut probe: Option<&mut Vec<String>>,
) {
    let m = &g.m;
    if !typed_array_enabled() || m.aborted || m.unit_mode || interpreted {
        return;
    }
    // A summarized body's node has one copy per instance (`Inst::fresh`):
    // the fact must hold for each of them too.
    let mut copies: std::collections::HashMap<NodeId, Vec<NodeId>> = Default::default();
    for inst in 0..m.sum.insts.len() as u32 {
        m.inst_copies(inst, |orig, copy| {
            copies.entry(orig).or_default().push(copy)
        });
    }
    // (module, site, key) -> the receiver nodes of every record of the site.
    let mut sites: BTreeMap<(u32, Site, String), Vec<NodeId>> = BTreeMap::new();
    let mut add = |module: u32, site: Site, key: crate::model::Atom, recv: NodeId| {
        let nodes = sites
            .entry((module, site, m.atoms[key as usize].to_string()))
            .or_default();
        nodes.push(recv);
        nodes.extend(copies.get(&recv).into_iter().flatten().copied());
    };
    for s in &g.read_sites {
        if s.is_call && !s.value_call && s.byte_offset != 0 {
            add(s.module, Site::Callee(s.byte_offset), s.key, s.recv);
        }
    }
    for s in &g.folded_sites {
        add(s.module, s.site, s.key, s.recv);
    }
    // The map is ordered by (module, site, key): each list comes out sorted.
    for ((mi, site, key), nodes) in sites {
        let why = nodes
            .iter()
            .find_map(|&n| not_typed_arrays(m, n))
            .filter(|_| !sabotage_typed_array);
        if let Some(p) = probe.as_deref_mut() {
            let site = site_text(site);
            p.push(match why {
                None => format!("# method_site module={mi} site={site} key={key} typed_array=1"),
                Some(why) => format!(
                    "# method_site module={mi} site={site} key={key} typed_array=0 refused={why}"
                ),
            });
        }
        if let (None, Some(mb)) = (why, bindings.get_mut(mi as usize)) {
            mb.method_sites.push((site, key, TYPED_ARRAY));
        }
    }
}
