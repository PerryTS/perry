//! `PERRY_FACTS_LEDGER=<path>` (diagnostic, off by default): the blocker
//! ledger. The realm is never held (`hold_realm` records and returns), so
//! the solve continues past every hold; each hold's own effects (the values
//! it escapes, the ⊤ it makes) stay and carry the hold they come from (its
//! root reason). After the final derive every refused site is blamed on the
//! roots of its receiver's ⊤ and escaped / HARD tokens, and one TSV row per
//! root gives its site mass. The compile then stops (`program_facts`): no
//! fact of a ledger solve reaches codegen. Unset: `Model::ledger` is `None`.
//!
//! LEDG2: a site's full root set also holds every later root that escaped
//! one of its receiver tokens again (an escape of an escaped token changes
//! nothing in the solve; the ledger records its root) and the roots of every
//! escaped token of a receiver's template. Per root, `mass_only` counts the
//! sites whose full set is exactly that root: the sites its removal alone
//! would free of every recorded hold.
//!
//! LEDG3: an escape stops at a token that already escaped, so a cause that
//! reaches a token only through another escaped token is masked in those
//! sets. The ledger therefore also keeps the escape-predecessor graph:
//! every escape, sink and ⊤ seed adds one edge from its immediate cause (the
//! root, the escaping token, the sink node or the ⊤ origin of the cascade)
//! to what it touches, also when that is already escaped. A refused site is
//! freed by removing a set of roots when none of its causes (its receiver's
//! escaped tokens, their templates' escaped tokens, its ⊤ origin) is still
//! reachable from another root (`<path>.freed`).

use crate::model::{Model, NodeId, Tok};
use crate::pts::FxHashMap;
use std::panic::Location;

const NONE: u32 = u32::MAX;

/// One root: a reason id that started a cascade of unknown values.
pub(crate) struct Root {
    seq: u32,
    reason: u32,
    kind: &'static str,
    loc: &'static Location<'static>,
    /// The first node (`n<id>`) or token (`t<id>`) it touched.
    at: String,
}

#[derive(Default)]
pub(crate) struct HoldLedger {
    roots: Vec<Root>,
    /// (reason, first node / token id, 0 node / 1 token) -> root id.
    by_key: FxHashMap<(u32, u32, u8), u32>,
    /// The root of the cascade being applied (`NONE` outside one).
    pub(crate) cur: u32,
    tok_root: FxHashMap<Tok, u32>,
    sink_root: FxHashMap<NodeId, u32>,
    top_root: FxHashMap<(NodeId, u32), u32>,
    /// Later roots that escaped an escaped token again (beside `tok_root`).
    tok_again: FxHashMap<Tok, Vec<u32>>,
    again_seen: crate::pts::FxHashSet<(Tok, u32)>,
    /// LEDG3: the immediate cause of the cascade being applied (a vertex,
    /// `NOV` outside one) and the escape-predecessor edges.
    cause: u64,
    edges: crate::pts::FxHashSet<(u64, u64)>,
    /// BLAST: the rule (code place, reason) that made each edge first.
    pub(crate) lab: FxHashMap<(u64, u64), (&'static Location<'static>, u32)>,
    /// ⊤ origin (`top_origin_id`) -> its vertex number.
    top_ids: FxHashMap<(NodeId, u32), u32>,
    /// `PERRY_FACTS_LEDGER_DROP`: roots the solve leaves out (a re-solve
    /// that checks a `freed` prediction): `file:line` (every root recorded
    /// there) or `file:line@t<id>` / `@n<id>` (one root).
    drop: Vec<(String, u32, Option<String>)>,
}

/// No vertex.
const NOV: u64 = u64::MAX;

/// The graph's vertices: a root, a token, a sink node, a ⊤ origin.
pub(crate) fn v_root(r: u32) -> u64 {
    r as u64
}
pub(crate) fn v_tok(t: Tok) -> u64 {
    1 << 32 | t as u64
}
pub(crate) fn v_node(n: NodeId) -> u64 {
    2 << 32 | n as u64
}
fn v_top(i: u32) -> u64 {
    3 << 32 | i as u64
}

/// The cascade context an `enter_*` replaced (restored by `leave`).
#[derive(Clone, Copy)]
pub(crate) struct Ctx {
    cur: u32,
    cause: u64,
}

/// Read where a model is made and where the ledger is written (a test sets
/// it for one solve).
pub(crate) fn path() -> Option<String> {
    crate::provenance::env("PERRY_FACTS_LEDGER").filter(|v| !v.is_empty())
}

pub(crate) fn blast() -> bool {
    static B: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *B.get_or_init(|| std::env::var("PERRY_BLAST").is_ok_and(|v| v == "1"))
}

/// BLAST: the labelled graph, the roots, the ⊤ origins and every refused
/// site with its causes (`<path>.blast`).
fn write_blast(m: &Model, l: &HoldLedger, refused: &[Refused], path: &str) {
    use std::fmt::Write as _;
    let rs = |w: u32| {
        m.reasons
            .get(w as usize)
            .map_or("-".to_string(), |s| s.replace(['\t', '\n', '\r'], " "))
    };
    let mut o = String::new();
    for (i, x) in l.roots.iter().enumerate() {
        let _ = writeln!(
            o,
            "R\t{i}\t{}\t{}:{}\t{}\t{}",
            x.kind,
            short(x.loc.file()),
            x.loc.line(),
            x.at,
            rs(x.reason)
        );
    }
    for (&(n, why), &i) in &l.top_ids {
        let _ = writeln!(o, "T\t{i}\t{n}\t{}", rs(why));
    }
    let mut toks: crate::pts::FxHashSet<u64> = Default::default();
    for (&(a, b), &(loc, why)) in &l.lab {
        let _ = writeln!(
            o,
            "E\t{a}\t{b}\t{}:{}\t{}",
            short(loc.file()),
            loc.line(),
            rs(why)
        );
        for v in [a, b] {
            if v >> 32 == 1 {
                toks.insert(v);
            }
        }
    }
    for v in toks {
        let t = (v & 0xffff_ffff) as Tok;
        let _ = writeln!(o, "K\t{v}\t{}", m.tok_name(t).replace(['\t', '\n'], " "));
    }
    for s in refused {
        let c: Vec<String> = s.causes.iter().map(|c| c.to_string()).collect();
        let _ = writeln!(o, "S\t{}\t{}", s.site, c.join(","));
    }
    let _ = std::fs::write(format!("{path}.blast"), o);
}

pub(crate) fn on() -> bool {
    path().is_some()
}

impl HoldLedger {
    pub(crate) fn new() -> Box<Self> {
        Box::new(HoldLedger {
            cur: NONE,
            cause: NOV,
            drop: std::env::var("PERRY_FACTS_LEDGER_DROP")
                .unwrap_or_default()
                .split(',')
                .filter_map(|d| {
                    let (place, at) = match d.split_once('@') {
                        Some((p, a)) => (p, Some(a.to_string())),
                        None => (d, None),
                    };
                    let (file, line) = place.rsplit_once(':')?;
                    Some((file.to_string(), line.parse().ok()?, at))
                })
                .collect(),
            ..Default::default()
        })
    }

    /// A new root (outside a cascade) for the node or token `id` recorded
    /// at `loc` is one `PERRY_FACTS_LEDGER_DROP` leaves out.
    pub(crate) fn dropped(&self, loc: &Location, id: u32, tok: bool) -> bool {
        if self.drop.is_empty() || self.cur != NONE {
            return false;
        }
        let at = || format!("{}{id}", if tok { "t" } else { "n" });
        self.drop.iter().any(|(file, line, a)| {
            *line == loc.line()
                && short(loc.file()) == file
                && a.as_ref().is_none_or(|a| *a == at())
        })
    }

    /// The root a new effect belongs to: the cascade's, else a new one for
    /// (`reason`, the node or token `id` it first touched).
    fn root_of(
        &mut self,
        reason: u32,
        id: u32,
        tok: bool,
        kind: &'static str,
        loc: &'static Location<'static>,
    ) -> u32 {
        if self.cur != NONE {
            return self.cur;
        }
        let n = self.roots.len() as u32;
        let r = *self.by_key.entry((reason, id, tok as u8)).or_insert(n);
        if r == n {
            let at = if tok {
                format!("t{id}")
            } else {
                format!("n{id}")
            };
            self.roots.push(Root {
                seq: n,
                reason,
                kind,
                loc,
                at,
            });
        }
        r
    }

    /// The immediate cause of an effect whose root is `r` (`root_of`):
    /// the cascade's cause, else the new root itself.
    fn parent(&self, r: u32) -> u64 {
        match self.cur != NONE && self.cause != NOV {
            true => self.cause,
            false => v_root(r),
        }
    }

    /// One predecessor edge (LEDG3), once per pair.
    fn edge(&mut self, from: u64, to: u64, loc: &'static Location<'static>, why: u32) {
        if from != to {
            self.edges.insert((from, to));
            self.lab.entry((from, to)).or_insert((loc, why));
        }
    }

    fn ctx(&self) -> Ctx {
        Ctx {
            cur: self.cur,
            cause: self.cause,
        }
    }

    /// Restore the cascade an `enter_*` replaced.
    pub(crate) fn leave(&mut self, c: Ctx) {
        self.cur = c.cur;
        self.cause = c.cause;
    }

    /// `escape(t, reason)` begins (also when `t` already escaped): returns
    /// the cascade to restore after it.
    pub(crate) fn enter_escape(
        &mut self,
        t: Tok,
        reason: u32,
        loc: &'static Location<'static>,
    ) -> Ctx {
        let r = self.root_of(reason, t, true, "escape", loc);
        let saved = self.ctx();
        let p = self.parent(r);
        self.edge(p, v_tok(t), loc, reason);
        self.cur = r;
        self.cause = v_tok(t);
        saved
    }

    /// `t`, already escaped, is escaped again for `reason`.
    pub(crate) fn again(&mut self, t: Tok, reason: u32, loc: &'static Location<'static>) {
        let r = self.root_of(reason, t, true, "escape", loc);
        let p = self.parent(r);
        self.edge(p, v_tok(t), loc, reason);
        self.again_root(t, r);
    }

    /// Root `r` escapes the escaped token `t` again.
    pub(crate) fn again_root(&mut self, t: Tok, r: u32) {
        if r == NONE || self.tok_root.get(&t) == Some(&r) || !self.again_seen.insert((t, r)) {
            return;
        }
        self.tok_again.entry(t).or_default().push(r);
    }

    #[track_caller]
    pub(crate) fn mark_escaped(&mut self, t: Tok, reason: u32) {
        let r = self.root_of(reason, t, true, "escape", Location::caller());
        let p = self.parent(r);
        self.edge(p, v_tok(t), Location::caller(), reason);
        self.tok_root.entry(t).or_insert(r);
    }

    pub(crate) fn sink(&mut self, n: NodeId, reason: u32, loc: &'static Location<'static>) {
        let r = self.root_of(reason, n, false, "sink", loc);
        let p = self.parent(r);
        self.edge(p, v_node(n), loc, reason);
        self.sink_root.entry(n).or_insert(r);
    }

    /// A TopSink rule is attached to `n` inside a cascade: what it escapes
    /// later (as values reach `n`) belongs to that cascade, `n` its cause.
    #[track_caller]
    pub(crate) fn sink_rule(&mut self, n: NodeId) {
        if self.cur == NONE {
            return;
        }
        let p = self.parent(self.cur);
        self.edge(p, v_node(n), Location::caller(), NONE);
        self.sink_root.entry(n).or_insert(self.cur);
    }

    /// The TopSink rule of `n` runs: its escapes belong to `n`'s root, and
    /// their immediate cause is `n`.
    pub(crate) fn enter_sink(&mut self, n: NodeId) -> Ctx {
        let saved = self.ctx();
        if let Some(&r) = self.sink_root.get(&n) {
            self.cur = r;
            self.cause = v_node(n);
        }
        saved
    }

    /// A store into ⊤ sinks its value: the sink joins the root of the
    /// receiver's ⊤ (`origin`, its `top_origin_id`) when one is recorded.
    /// Returns the root to restore after it.
    pub(crate) fn enter_top(&mut self, origin: Option<(NodeId, u32)>) -> Ctx {
        let saved = self.ctx();
        if let Some(k) = origin {
            if let (Some(&r), Some(&i)) = (self.top_root.get(&k), self.top_ids.get(&k)) {
                self.cur = r;
                self.cause = v_top(i);
            }
        }
        saved
    }

    /// ⊤ first entered `n` for `why` (a seed, not a copy).
    pub(crate) fn top_seed(&mut self, n: NodeId, why: u32, loc: &'static Location<'static>) {
        if why == NONE {
            return;
        }
        let r = self.root_of(why, n, false, "top", loc);
        self.top_root.entry((n, why)).or_insert(r);
        let k = self.top_ids.len() as u32;
        let i = *self.top_ids.entry((n, why)).or_insert(k);
        let p = self.parent(r);
        self.edge(p, v_top(i), loc, why);
    }

    /// The realm would be held for `reason` (`hold_realm`): only recorded.
    pub(crate) fn realm(
        &mut self,
        kind: &'static str,
        reason: u32,
        loc: &'static Location<'static>,
    ) {
        self.root_of(reason, NONE, false, kind, loc);
    }
}

/// The roots one refused site is blamed on, in no order: the recorded
/// ones (the first root of each token) and the full set (LEDG2: with every
/// root that escaped a token again, and every escaped token of a template).
///
/// LEDG3: also the site's causes (graph vertices): its ⊤ origin, its
/// escaped receiver tokens and the escaped tokens of their templates.
pub(crate) fn blame(m: &Model, toks: &[Tok], top_recv: NodeId) -> Blame {
    let Some(l) = m.ledger.as_deref() else {
        return Blame::default();
    };
    let mut out = Vec::new();
    let mut full = Vec::new();
    let mut causes = Vec::new();
    if toks.contains(&crate::model::TOP) {
        let k = m.top_origin_id(top_recv);
        if let Some(&r) = k.and_then(|k| l.top_root.get(&k)) {
            out.push(r);
        }
        match k.and_then(|k| l.top_ids.get(&k)) {
            Some(&i) => causes.push(v_top(i)),
            // ⊤ of no recorded origin: a cause no root removal frees.
            None => causes.push(NOV),
        }
    }
    let all_of = |u: Tok, full: &mut Vec<u32>| {
        if let Some(&r) = l.tok_root.get(&u) {
            full.push(r);
        }
        if let Some(v) = l.tok_again.get(&u) {
            full.extend_from_slice(v);
        }
    };
    for &t in toks {
        if m.escaped.contains_key(&t) {
            causes.push(v_tok(t));
        }
        if let crate::model::TokKind::Obj(tm, _) = m.toks[t as usize] {
            for u in m.tmpl_toks(tm) {
                if m.escaped.contains_key(&u) {
                    causes.push(v_tok(u));
                }
            }
        }
        if let Some(&r) = l.tok_root.get(&t) {
            out.push(r);
            all_of(t, &mut full);
        } else if let crate::model::TokKind::Obj(tm, _) = m.toks[t as usize] {
            // The layout is one per template: a site is refused when any
            // token of its template escaped (`template_facts`), so it is
            // blamed on that token's root.
            let escaped: Vec<Tok> = m
                .tmpl_toks(tm)
                .into_iter()
                .filter(|u| m.escaped.contains_key(u))
                .collect();
            if let Some(&r) = escaped.first().and_then(|u| l.tok_root.get(u)) {
                out.push(r);
            }
            for u in escaped {
                all_of(u, &mut full);
            }
        }
    }
    out.sort_unstable();
    out.dedup();
    full.extend_from_slice(&out);
    full.sort_unstable();
    full.dedup();
    causes.sort_unstable();
    causes.dedup();
    Blame {
        roots: out,
        full,
        causes,
    }
}

/// What `blame` gives one refused site.
#[derive(Default)]
pub(crate) struct Blame {
    pub(crate) roots: Vec<u32>,
    pub(crate) full: Vec<u32>,
    pub(crate) causes: Vec<u64>,
}

/// Which app part a module path is in (the app counts' split).
pub(crate) fn part(path: &str) -> usize {
    if path.contains("three/examples/") || path.contains("three/addons/") {
        1
    } else if path.contains("/three/build/") || path.contains("/three/src/") {
        0
    } else {
        2
    }
}

/// One refused site: its blame roots and its part.
pub(crate) struct Refused {
    pub(crate) roots: Vec<u32>,
    /// The full root set (LEDG2), a superset of `roots`.
    pub(crate) full: Vec<u32>,
    /// Its causes in the predecessor graph (LEDG3; `NOV`: one no root
    /// removal frees).
    pub(crate) causes: Vec<u64>,
    pub(crate) part: usize,
    pub(crate) store: bool,
    /// No root: the site's own refusal (`None` when a root is blamed).
    pub(crate) why: Option<Unattributed>,
    /// BLAST: module path, offset, key, store.
    pub(crate) site: String,
}

/// A refused site no root explains: its verdict and where it is.
pub(crate) struct Unattributed {
    pub(crate) why: String,
    /// Module, offset, key, read/store, receiver tokens, ⊤ origin (TSV).
    pub(crate) detail: String,
}

/// One root's site mass.
#[derive(Default, Clone, Copy)]
pub(crate) struct RootMass {
    /// Sites it is the first root of, per app part.
    pub(crate) first: [usize; 3],
    /// Sites whose recorded roots hold it.
    pub(crate) any: usize,
    pub(crate) stores_first: usize,
    /// Sites whose full root set is exactly this root.
    pub(crate) only: usize,
    pub(crate) stores_only: usize,
}

/// The root-set bookkeeping over the attributed refused sites.
#[derive(Default)]
pub(crate) struct Tally {
    pub(crate) mass: FxHashMap<u32, RootMass>,
    /// Full root-set sizes: 1, 2, 3, 4-8, 9+.
    pub(crate) sizes: [usize; 5],
    /// Sites (and stores) whose full set is exactly the pair.
    pub(crate) pairs: FxHashMap<(u32, u32), (usize, usize)>,
    /// Sites whose full set is larger than their recorded roots.
    pub(crate) widened: usize,
}

/// Tally the refused sites a root is blamed on (sites with no recorded
/// root are the unattributed rows and are left out).
pub(crate) fn tally(refused: &[Refused]) -> Tally {
    let mut t = Tally::default();
    for s in refused {
        let Some(&first) = s.roots.iter().min() else {
            continue;
        };
        let e = t.mass.entry(first).or_default();
        e.first[s.part] += 1;
        e.stores_first += s.store as usize;
        for &r in &s.roots {
            t.mass.entry(r).or_default().any += 1;
        }
        let full: &[u32] = if s.full.is_empty() { &s.roots } else { &s.full };
        t.widened += (full.len() > s.roots.len()) as usize;
        t.sizes[match full.len() {
            1 => 0,
            2 => 1,
            3 => 2,
            4..=8 => 3,
            _ => 4,
        }] += 1;
        match *full {
            [r] => {
                let e = t.mass.entry(r).or_default();
                e.only += 1;
                e.stores_only += s.store as usize;
            }
            [a, b] => {
                let e = t.pairs.entry((a.min(b), a.max(b))).or_default();
                e.0 += 1;
                e.1 += s.store as usize;
            }
            _ => {}
        }
    }
    t
}

/// LEDG3: the escape-predecessor graph (CSR) and its roots in seq order.
pub(crate) struct Graph {
    ids: FxHashMap<u64, u32>,
    start: Vec<u32>,
    succ: Vec<u32>,
    roots: Vec<(u32, u32)>,
}

/// What removing a set of roots frees.
#[derive(Default, Debug, PartialEq)]
pub(crate) struct Freed {
    pub(crate) sites: usize,
    pub(crate) stores: usize,
    /// Sites whose first graph root (the smallest seq reaching a cause)
    /// is in the set.
    pub(crate) first: usize,
    /// The root that takes over most of the set's first sites that stay
    /// refused, and how many.
    pub(crate) reroot: Option<(u32, usize)>,
}

impl Graph {
    pub(crate) fn new<'a>(edges: impl Iterator<Item = &'a (u64, u64)>) -> Graph {
        let mut ids: FxHashMap<u64, u32> = FxHashMap::default();
        let mut roots = Vec::new();
        let mut id = |v: u64, roots: &mut Vec<(u32, u32)>| {
            let n = ids.len() as u32;
            let i = *ids.entry(v).or_insert(n);
            if i == n && v >> 32 == 0 {
                roots.push((v as u32, i));
            }
            i
        };
        let mut es: Vec<(u32, u32)> = edges
            .map(|&(a, b)| (id(a, &mut roots), id(b, &mut roots)))
            .collect();
        es.sort_unstable();
        roots.sort_unstable();
        let mut start = vec![0u32; ids.len() + 1];
        for &(a, _) in &es {
            start[a as usize + 1] += 1;
        }
        for i in 1..start.len() {
            start[i] += start[i - 1];
        }
        Graph {
            ids,
            start,
            succ: es.into_iter().map(|e| e.1).collect(),
            roots,
        }
    }

    pub(crate) fn edges(&self) -> usize {
        self.succ.len()
    }

    pub(crate) fn vertices(&self) -> usize {
        self.ids.len()
    }

    /// Each vertex's first root: the smallest seq not `skip`ped that
    /// reaches it (`NONE`: none does).
    pub(crate) fn labels(&self, skip: &dyn Fn(u32) -> bool) -> Vec<u32> {
        let mut lab = vec![NONE; self.ids.len()];
        let mut q = Vec::new();
        for &(r, v) in &self.roots {
            if skip(r) || lab[v as usize] != NONE {
                continue;
            }
            lab[v as usize] = r;
            q.push(v);
            while let Some(x) = q.pop() {
                let (a, b) = (self.start[x as usize], self.start[x as usize + 1]);
                for &y in &self.succ[a as usize..b as usize] {
                    if lab[y as usize] == NONE {
                        lab[y as usize] = r;
                        q.push(y);
                    }
                }
            }
        }
        lab
    }

    /// A site's first root under `lab`: `None` when a cause is no vertex
    /// (no root removal frees it), else the smallest label (`NONE` when no
    /// root reaches any cause).
    fn site(&self, lab: &[u32], causes: &[u64]) -> Option<u32> {
        let mut min = NONE;
        for c in causes {
            min = min.min(lab[*self.ids.get(c)? as usize]);
        }
        Some(min)
    }

    /// The refused sites the removal of the `skip`ped roots frees: every
    /// cause reached by a root before (`base`) and by none after.
    pub(crate) fn freed(
        &self,
        refused: &[Refused],
        base: &[u32],
        skip: &dyn Fn(u32) -> bool,
    ) -> Freed {
        let lab = self.labels(skip);
        let mut f = Freed::default();
        let mut to: FxHashMap<u32, usize> = FxHashMap::default();
        for s in refused {
            if s.causes.is_empty() {
                continue;
            }
            let Some(b) = self.site(base, &s.causes) else {
                continue;
            };
            let rooted = s.causes.iter().all(|c| base[self.ids[c] as usize] != NONE);
            if !rooted {
                continue;
            }
            let all_gone = s.causes.iter().all(|c| lab[self.ids[c] as usize] == NONE);
            if skip(b) {
                f.first += 1;
            }
            if all_gone {
                f.sites += 1;
                f.stores += s.store as usize;
            } else if skip(b) {
                *to.entry(self.site(&lab, &s.causes).unwrap_or(NONE))
                    .or_default() += 1;
            }
        }
        f.reroot = to
            .into_iter()
            .max_by(|a, b| a.1.cmp(&b.1).then(b.0.cmp(&a.0)));
        f
    }
}

/// A root's mechanism: its reason with node / call / token ids made `#`,
/// and where it is recorded.
fn mechanism(reason: &str, loc: &Location) -> String {
    let mut out = String::with_capacity(reason.len());
    let mut digits = false;
    for ch in reason.chars() {
        match ch.is_ascii_digit() {
            true if digits => {}
            true => out.push('#'),
            false => out.push(ch),
        }
        digits = ch.is_ascii_digit();
    }
    format!(
        "{} @ {}:{}",
        out.replace(['\t', '\n', '\r'], " "),
        short(loc.file()),
        loc.line()
    )
}

/// Write the ledger TSV (sorted by first mass), the same rows sorted by
/// `mass_only` (`<path>.only`) and the summary.
pub(crate) fn write(m: &Model, refused: &[Refused], totals: [[usize; 3]; 2]) {
    let (Some(path), Some(l)) = (path(), m.ledger.as_deref()) else {
        return;
    };
    if blast() {
        write_blast(m, l, refused, &path);
    }
    let t = tally(refused);
    let mut unattributed = [0usize; 3];
    // Per refusal of the unattributed sites: [3 parts], stores.
    let mut by_why: FxHashMap<&str, ([usize; 3], usize)> = FxHashMap::default();
    let mut details = String::from("why\tmodule\toffset\tkey\taccess\treceivers\ttop_origin\n");
    let mut refused_stores = 0usize;
    for s in refused {
        refused_stores += s.store as usize;
        if !s.roots.is_empty() {
            continue;
        }
        unattributed[s.part] += 1;
        if let Some(u) = &s.why {
            let e = by_why.entry(u.why.as_str()).or_default();
            e.0[s.part] += 1;
            e.1 += s.store as usize;
            details.push_str(&format!(
                "{}\t{}\n",
                u.why.replace(['\t', '\n', '\r'], " "),
                u.detail.replace(['\n', '\r'], " ")
            ));
        }
    }
    let mut rows: Vec<(u32, RootMass)> = t.mass.iter().map(|(&r, &x)| (r, x)).collect();
    let sum = |f: &[usize; 3]| f.iter().sum::<usize>();
    rows.sort_by(|a, b| {
        sum(&b.1.first)
            .cmp(&sum(&a.1.first))
            .then(b.1.any.cmp(&a.1.any))
            .then(a.0.cmp(&b.0))
    });
    let clean = |s: &str| s.replace(['\t', '\n', '\r'], " ");
    let header = "seq\tkind\treason\twhere\tmass_first\tmass_any\tcore\taddons\trest\tstores_first\tmass_only\tstores_only\n";
    let named = |r: u32| {
        let x = &l.roots[r as usize];
        let reason = m.reasons.get(x.reason as usize).map_or("-", |s| s.as_str());
        let at = format!("{}:{} {}", short(x.loc.file()), x.loc.line(), x.at);
        format!("{}\t{}\t{}\t{at}", x.seq, x.kind, clean(reason))
    };
    let row = |(r, x): &(u32, RootMass)| {
        format!(
            "{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\n",
            named(*r),
            sum(&x.first),
            x.any,
            x.first[0],
            x.first[1],
            x.first[2],
            x.stores_first,
            x.only,
            x.stores_only
        )
    };
    let mut tsv = String::from(header);
    for x in &rows {
        tsv.push_str(&row(x));
    }
    tsv.push_str(&format!(
        "-\tnone\tunattributed (refused, no hold on the receiver)\t-\t{}\t-\t{}\t{}\t{}\t-\t-\t-\n",
        sum(&unattributed),
        unattributed[0],
        unattributed[1],
        unattributed[2]
    ));
    let mut whys: Vec<(&str, ([usize; 3], usize))> = by_why.into_iter().collect();
    whys.sort_by(|a, b| sum(&b.1 .0).cmp(&sum(&a.1 .0)).then(a.0.cmp(b.0)));
    for (why, (f, st)) in &whys {
        tsv.push_str(&format!(
            "-\tunattributed\t{}\t-\t{}\t-\t{}\t{}\t{}\t{st}\t-\t-\n",
            clean(why),
            sum(f),
            f[0],
            f[1],
            f[2]
        ));
    }
    // The second table: what removing one root alone frees.
    let mut by_only: Vec<&(u32, RootMass)> = rows.iter().filter(|x| x.1.only > 0).collect();
    by_only.sort_by(|a, b| {
        b.1.only
            .cmp(&a.1.only)
            .then(b.1.stores_only.cmp(&a.1.stores_only))
            .then(a.0.cmp(&b.0))
    });
    let mut only = String::from(header);
    for x in &by_only {
        only.push_str(&row(x));
    }
    let mut pairs: Vec<((u32, u32), (usize, usize))> = t.pairs.into_iter().collect();
    pairs.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
    let _ = std::fs::write(format!("{path}.unattributed"), &details);
    let attributed = refused.len() - sum(&unattributed);
    let mut summary = format!(
        "LEDGER sites reads {} (core {} addons {} rest {}), stores {}; refused {} (attributed {}, unattributed {}); roots recorded {}, roots with mass {}\n",
        sum(&totals[0]),
        totals[0][0],
        totals[0][1],
        totals[0][2],
        sum(&totals[1]),
        refused.len(),
        attributed,
        sum(&unattributed),
        l.roots.len(),
        rows.len()
    );
    // The normal compile's "N of M read/call sites proven, K of S stores
    // proven" line must give the same N and K.
    summary.push_str(&format!(
        "LEDGER proven reads {} of {}, stores {} of {} (sites - refused)\n",
        sum(&totals[0]) - (refused.len() - refused_stores),
        sum(&totals[0]),
        sum(&totals[1]) - refused_stores,
        sum(&totals[1]),
    ));
    if let Some(x) = l.roots.first() {
        summary.push_str(&format!(
            "LEDGER first root seq={} kind={} at={}:{} reason={}\n",
            x.seq,
            x.kind,
            short(x.loc.file()),
            x.loc.line(),
            clean(m.reasons.get(x.reason as usize).map_or("-", |s| s.as_str()))
        ));
    }
    for (i, line) in tsv.lines().skip(1).take(8).enumerate() {
        summary.push_str(&format!("LEDGER top{} {}\n", i + 1, line));
    }
    let z = t.sizes;
    summary.push_str(&format!(
        "LEDGER root-set sizes (attributed {attributed}): 1 {} | 2 {} | 3 {} | 4-8 {} | 9+ {}; {} sites have roots beyond their recorded first roots\n",
        z[0], z[1], z[2], z[3], z[4], t.widened
    ));
    for (i, line) in only.lines().skip(1).take(15).enumerate() {
        summary.push_str(&format!("LEDGER only{} {}\n", i + 1, line));
    }
    for (i, ((a, b), (n, st))) in pairs.iter().take(15).enumerate() {
        summary.push_str(&format!(
            "LEDGER pair{} sites {n} stores {st}\t{}\t|\t{}\n",
            i + 1,
            named(*a),
            named(*b)
        ));
    }
    // LEDG3: what removing a root, a pair, a mechanism frees for real.
    let g = Graph::new(l.edges.iter());
    let base = g.labels(&|_| false);
    summary.push_str(&format!(
        "LEDGER graph vertices {} edges {} roots {}\n",
        g.vertices(),
        g.edges(),
        g.roots.len()
    ));
    let mut freed = String::from(
        "set\tfreed\tfreed_stores\tgraph_first\treroot_to\treroot_sites\tmass_first\tmass_only\tname\n",
    );
    let reroot = |f: &Freed| match f.reroot {
        Some((r, n)) if r != NONE => (r.to_string(), n),
        Some((_, n)) => ("-".to_string(), n),
        None => ("-".to_string(), 0),
    };
    let mut one: Vec<(Freed, &(u32, RootMass))> = rows
        .iter()
        .take(30)
        .map(|x| (g.freed(refused, &base, &|r| r == x.0), x))
        .collect();
    one.sort_by(|a, b| b.0.sites.cmp(&a.0.sites).then(a.1 .0.cmp(&b.1 .0)));
    for (i, (f, (r, x))) in one.iter().enumerate() {
        let (to, n) = reroot(f);
        let line = format!(
            "root\t{}\t{}\t{}\t{to}\t{n}\t{}\t{}\t{}",
            f.sites,
            f.stores,
            f.first,
            sum(&x.first),
            x.only,
            named(*r)
        );
        if i < 15 {
            summary.push_str(&format!("LEDGER freed{} {line}\n", i + 1));
        }
        freed.push_str(&line);
        freed.push('\n');
    }
    for (i, ((a, b), (n_only, _))) in pairs.iter().take(10).enumerate() {
        let f = g.freed(refused, &base, &|r| r == *a || r == *b);
        let (to, n) = reroot(&f);
        let line = format!(
            "pair\t{}\t{}\t{}\t{to}\t{n}\t-\t{n_only}\t{}\t|\t{}",
            f.sites,
            f.stores,
            f.first,
            named(*a),
            named(*b)
        );
        summary.push_str(&format!("LEDGER pairfreed{} {line}\n", i + 1));
        freed.push_str(&line);
        freed.push('\n');
    }
    // Mechanisms: all roots of one reason shape and place.
    let mut mech_of = vec![NONE; l.roots.len()];
    let mut mech_ids: FxHashMap<String, u32> = FxHashMap::default();
    let mut mech_names = Vec::new();
    for (i, x) in l.roots.iter().enumerate() {
        let reason = m.reasons.get(x.reason as usize).map_or("-", |s| s.as_str());
        let k = mechanism(reason, x.loc);
        let n = mech_ids.len() as u32;
        let id = *mech_ids.entry(k.clone()).or_insert(n);
        if id == n {
            mech_names.push((k, x.kind));
        }
        mech_of[i] = id;
    }
    let mut mech_mass: FxHashMap<u32, (usize, usize, usize)> = FxHashMap::default();
    for (r, x) in &rows {
        let e = mech_mass.entry(mech_of[*r as usize]).or_default();
        e.0 += sum(&x.first);
        e.1 += x.only;
        e.2 += 1;
    }
    let mut mechs: Vec<(u32, (usize, usize, usize))> = mech_mass.into_iter().collect();
    mechs.sort_by(|a, b| b.1 .0.cmp(&a.1 .0).then(a.0.cmp(&b.0)));
    let mut mf: Vec<(Freed, u32, (usize, usize, usize))> = mechs
        .iter()
        .take(15)
        .map(|&(k, x)| (g.freed(refused, &base, &|r| mech_of[r as usize] == k), k, x))
        .collect();
    mf.sort_by(|a, b| b.0.sites.cmp(&a.0.sites).then(a.1.cmp(&b.1)));
    for (i, (f, k, (first, only_sum, nroots))) in mf.iter().enumerate() {
        let (to, n) = reroot(f);
        let (name, kind) = &mech_names[*k as usize];
        let line = format!(
            "mech\t{}\t{}\t{}\t{to}\t{n}\t{first}\t{only_sum}\t{kind} roots {nroots}\t{name}",
            f.sites, f.stores, f.first
        );
        summary.push_str(&format!("LEDGER mechfreed{} {line}\n", i + 1));
        freed.push_str(&line);
        freed.push('\n');
    }
    let _ = std::fs::write(format!("{path}.freed"), &freed);
    let _ = std::fs::write(&path, tsv);
    let _ = std::fs::write(format!("{path}.only"), &only);
    let _ = std::fs::write(format!("{path}.summary"), &summary);
    eprint!("{summary}");
}

fn short(f: &str) -> &str {
    f.rsplit_once("src/").map_or(f, |(_, x)| x)
}
