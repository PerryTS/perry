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
//! - **an Array member** ([`ARRAY_MEMBER`]): at every copy of the site,
//!   and every summary instance's copy of it, the solver's `NativeMember`
//!   verdict over the Array family holds (`derive::native_member`, the one
//!   predicate; here over the site's receiver nodes): every receiver value
//!   is an Array exotic object (no primitive, no ⊤, no marker, no other
//!   object), `Array.prototype` describes the member as a function, no
//!   receiver holds the key as an own property or had its prototype
//!   relinked, and no builtin prototype is written under the key. The call
//!   then runs the realm's `Array.prototype` built-in and no other code.
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
//! then trusts the annotation, as before); `PERRY_FACTS_ARRAY_METHOD_SITES=0`
//! exports no Array-member fact (codegen then guards every site at run
//! time, as before). The negative controls `PERRY_FACTS_SABOTAGE=tasite` and
//! `=arraysite` claim every site a typed-array receiver / an Array member.
//! The dump (`PERRY_FACTS_DUMP`) prints one `# method_site` line per site:
//! `module= site=callee:N|recv:N|local:N key= typed_array=1|0(<why>)
//! array_member=1|0(<why>)` (a fact that is switched off prints `-`).

use crate::bindings::ModuleBindings;
use crate::gen::Gen;
use crate::model::{Model, NodeId, TmplKind, TokKind, TOP};
use perry_hir::folded_call::Site;
use std::collections::BTreeMap;

/// The site's receiver holds only typed arrays the realm bore.
pub const TYPED_ARRAY: u8 = perry_hir::folded_call::SITE_TYPED_ARRAY;

/// The site's member is the realm's `Array.prototype` built-in on every
/// receiver value (the `NativeMember` verdict over the Array family).
pub const ARRAY_MEMBER: u8 = perry_hir::folded_call::SITE_ARRAY_MEMBER;

/// `PERRY_FACTS_ARRAY_METHOD_SITES=0` turns the Array-member fact off.
pub fn array_member_enabled() -> bool {
    !matches!(
        std::env::var("PERRY_FACTS_ARRAY_METHOD_SITES").as_deref(),
        Ok("0") | Ok("off") | Ok("false")
    )
}

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
    sabotage_array_member: bool,
    mut probe: Option<&mut Vec<String>>,
) {
    let m = &g.m;
    let (typed, array) = (typed_array_enabled(), array_member_enabled());
    if !(typed || array) || m.aborted || m.unit_mode || interpreted {
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
    let mut sites: BTreeMap<(u32, Site, crate::model::Atom), Vec<NodeId>> = BTreeMap::new();
    let mut add = |module: u32, site: Site, key: crate::model::Atom, recv: NodeId| {
        let nodes = sites.entry((module, site, key)).or_default();
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
    let mut out: BTreeMap<(u32, Site, String), (u8, String)> = BTreeMap::new();
    for ((mi, site, key), nodes) in sites {
        let mut flags = 0;
        let mut text = String::new();
        let mut fact = |name: &str, on: bool, bit: u8, why: Option<String>| {
            text.push_str(&match (on, why) {
                (false, _) => format!(" {name}=-"),
                (true, None) => {
                    flags |= bit;
                    format!(" {name}=1")
                }
                (true, Some(why)) => format!(" {name}=0({why})"),
            });
        };
        fact(
            "typed_array",
            typed,
            TYPED_ARRAY,
            nodes
                .iter()
                .find_map(|&n| not_typed_arrays(m, n))
                .filter(|_| !sabotage_typed_array)
                .map(str::to_string),
        );
        fact(
            "array_member",
            array,
            ARRAY_MEMBER,
            crate::derive::native_array_member_call(m, key, &nodes)
                .err()
                .filter(|_| !sabotage_array_member),
        );
        // Two atoms of one spelling (a private name and a public key) are
        // one key here: the site holds a fact only when both do.
        let slot = out
            .entry((mi, site, m.atoms[key as usize].to_string()))
            .or_insert((u8::MAX, String::new()));
        slot.0 &= flags;
        slot.1 = text;
    }
    // The map is ordered by (module, site, key): each list comes out sorted.
    for ((mi, site, key), (flags, text)) in out {
        if let Some(p) = probe.as_deref_mut() {
            let site = site_text(site);
            p.push(format!(
                "# method_site module={mi} site={site} key={key}{text}"
            ));
        }
        if let (true, Some(mb)) = (flags != 0, bindings.get_mut(mi as usize)) {
            mb.method_sites.push((site, key, flags));
        }
    }
}
