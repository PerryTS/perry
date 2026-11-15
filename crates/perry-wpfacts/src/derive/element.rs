use super::*;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum ElementOp {
    Read,
    Write,
    Call,
}

/// A coarse key summary, never a set of string constants.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ElementKey {
    /// Only Number/BigInt primitives, with no object or unknown alternative.
    NumberOnly,
    /// An atom already named by generation or by a for-in run.
    Named(String),
    /// No single atom or numeric-only proof; conservatively any property
    /// name, including a symbol or an object's ToPropertyKey result.
    AnyString,
}

/// A solved token with its typed identities and diagnostic spelling. Ids
/// are local to this whole-program solve, never runtime identities.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ReceiverValue {
    pub token: Tok,
    pub kind: TokKind,
    /// The token's resolved runtime shape, local to this solve.
    pub shape: Option<usize>,
    /// Birth kind, including flow-only arrays absent from ProgramFacts.templates.
    pub template_kind: Option<TmplKind>,
    pub display: String,
    /// An unresolved marker stands for arbitrary values of another unit.
    pub marker: bool,
    /// A closed marker records uses; its callers' values are beside it.
    pub closed_marker: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ElementFacts {
    pub module: u32,
    /// SWC 1-based source offset; zero means unlocated/synthetic HIR.
    pub byte_offset: u32,
    pub op: ElementOp,
    /// Explicit TOP or an unresolved receiver marker.
    pub top: bool,
    pub receivers: Vec<ReceiverValue>,
    /// Primitive kinds (`model::prim`), alongside the object/function tokens.
    pub prims: u16,
    pub key: ElementKey,
}

/// The same receiver union feeds named-site verdicts and element facts.
/// Guarded verdicts may refine a claim, but never this unguarded value set.
pub(crate) struct ReceiverSet {
    pub toks: Vec<Tok>,
    pub prims: u16,
    pub top_recv: NodeId,
}

pub(crate) fn receiver_set(m: &Model, recvs: &[NodeId]) -> ReceiverSet {
    let mut toks = Vec::new();
    let mut prims = 0;
    let mut top_recv = recvs[0];
    let mut saw_top = false;
    for &r in recvs {
        let node = &m.nodes[r as usize];
        let values = node.pts.to_sorted();
        if !saw_top && values.contains(&TOP) {
            top_recv = r;
            saw_top = true;
        }
        toks.extend(values);
        prims |= node.prim;
    }
    toks.sort_unstable();
    toks.dedup();
    ReceiverSet {
        toks,
        prims,
        top_recv,
    }
}

impl ReceiverSet {
    pub fn top(&self, m: &Model) -> bool {
        self.toks
            .iter()
            .any(|&t| t == TOP || (m.is_marker(t) && !m.closed_marker(t)))
    }

    pub fn values(&self, m: &Model) -> Vec<ReceiverValue> {
        self.toks
            .iter()
            .map(|&t| ReceiverValue {
                token: t,
                kind: m.toks[t as usize].clone(),
                shape: m.toks.shape(t),
                template_kind: m.tmpl_of(t).map(|tm| m.tmpls[tm as usize].kind.clone()),
                display: match m.toks[t as usize] {
                    TokKind::Obj(tm, _) if !m.is_marker(t) => m.tmpls[tm as usize].display.clone(),
                    _ => m.tok_name(t),
                },
                marker: m.is_marker(t),
                closed_marker: m.closed_marker(t),
            })
            .collect()
    }
}

fn key_fact(m: &Model, site: &crate::gen::ElementSite) -> ElementKey {
    if let Key::Named(a) = site.key {
        return ElementKey::Named(m.atoms[a as usize].clone());
    }
    if let Some(&a) = m.key_nodes.get(&site.idx) {
        return ElementKey::Named(m.atoms[a as usize].clone());
    }
    let node = &m.nodes[site.idx as usize];
    if node.prim != 0
        && node.prim & !(prim::NUM | prim::BIGINT) == 0
        && node.pts.to_sorted().is_empty()
    {
        ElementKey::NumberOnly
    } else {
        ElementKey::AnyString
    }
}

pub(crate) fn element_facts(g: &Gen) -> Vec<ElementFacts> {
    let m = &g.m;
    let mut first: FxHashMap<(u32, u32, ElementOp), usize> = FxHashMap::default();
    let mut groups: Vec<Vec<&crate::gen::ElementSite>> = Vec::new();
    for site in &g.element_sites {
        let k = (site.module, site.byte_offset, site.op);
        // Zero has no source identity: keep unlocated operations separate.
        if site.byte_offset != 0 {
            if let Some(&i) = first.get(&k) {
                groups[i].push(site);
                continue;
            }
            first.insert(k, groups.len());
        }
        groups.push(vec![site]);
    }
    groups
        .into_iter()
        .map(|sites| {
            let site = sites[0];
            let recvs: Vec<NodeId> = sites.iter().map(|s| s.recv).collect();
            let set = receiver_set(m, &recvs);
            let mut key = key_fact(m, site);
            for s in &sites[1..] {
                if key != key_fact(m, s) {
                    key = ElementKey::AnyString;
                    break;
                }
            }
            ElementFacts {
                module: site.module,
                byte_offset: site.byte_offset,
                op: site.op,
                top: set.top(m),
                receivers: set.values(m),
                prims: set.prims,
                key,
            }
        })
        .collect()
}
