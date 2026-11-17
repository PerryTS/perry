//! `PERRY_FACTS_LEDGER=<path>` (diagnostic, off by default): the blocker
//! ledger. The realm is never held (`hold_realm` records and returns), so
//! the solve continues past every hold; each hold's own effects (the values
//! it escapes, the ⊤ it makes) stay and carry the hold they come from (its
//! root reason). After the final derive every refused site is blamed on the
//! roots of its receiver's ⊤ and escaped / HARD tokens, and one TSV row per
//! root gives its site mass. The compile then stops (`program_facts`): no
//! fact of a ledger solve reaches codegen. Unset: `Model::ledger` is `None`.

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
}

/// Read where a model is made and where the ledger is written (a test sets
/// it for one solve).
pub(crate) fn path() -> Option<String> {
    std::env::var("PERRY_FACTS_LEDGER")
        .ok()
        .filter(|v| !v.is_empty())
}

pub(crate) fn on() -> bool {
    path().is_some()
}

impl HoldLedger {
    pub(crate) fn new() -> Box<Self> {
        Box::new(HoldLedger {
            cur: NONE,
            ..Default::default()
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

    /// `escape(t, reason)` begins: returns the root to restore after it.
    pub(crate) fn enter_escape(
        &mut self,
        t: Tok,
        reason: u32,
        loc: &'static Location<'static>,
    ) -> u32 {
        let r = self.root_of(reason, t, true, "escape", loc);
        std::mem::replace(&mut self.cur, r)
    }

    #[track_caller]
    pub(crate) fn mark_escaped(&mut self, t: Tok, reason: u32) {
        let r = self.root_of(reason, t, true, "escape", Location::caller());
        self.tok_root.entry(t).or_insert(r);
    }

    pub(crate) fn sink(&mut self, n: NodeId, reason: u32, loc: &'static Location<'static>) {
        let r = self.root_of(reason, n, false, "sink", loc);
        self.sink_root.entry(n).or_insert(r);
    }

    /// The TopSink rule of `n` runs: its escapes belong to `n`'s root.
    pub(crate) fn enter_sink(&mut self, n: NodeId) -> u32 {
        let r = self.sink_root.get(&n).copied().unwrap_or(self.cur);
        std::mem::replace(&mut self.cur, r)
    }

    /// A store into ⊤ sinks its value: the sink joins the root of the
    /// receiver's ⊤ (`origin`, its `top_origin_id`) when one is recorded.
    /// Returns the root to restore after it.
    pub(crate) fn enter_top(&mut self, origin: Option<(NodeId, u32)>) -> u32 {
        let r = origin
            .and_then(|k| self.top_root.get(&k).copied())
            .unwrap_or(self.cur);
        std::mem::replace(&mut self.cur, r)
    }

    /// ⊤ first entered `n` for `why` (a seed, not a copy).
    pub(crate) fn top_seed(&mut self, n: NodeId, why: u32, loc: &'static Location<'static>) {
        if why == NONE {
            return;
        }
        let r = self.root_of(why, n, false, "top", loc);
        self.top_root.entry((n, why)).or_insert(r);
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

/// The roots one refused site is blamed on, in no order.
pub(crate) fn blame(m: &Model, toks: &[Tok], top_recv: NodeId) -> Vec<u32> {
    let Some(l) = m.ledger.as_deref() else {
        return Vec::new();
    };
    let mut out = Vec::new();
    if toks.contains(&crate::model::TOP) {
        if let Some(&r) = m.top_origin_id(top_recv).and_then(|k| l.top_root.get(&k)) {
            out.push(r);
        }
    }
    for &t in toks {
        if let Some(&r) = l.tok_root.get(&t) {
            out.push(r);
        } else if let crate::model::TokKind::Obj(tm, _) = m.toks[t as usize] {
            // The layout is one per template: a site is refused when any
            // token of its template escaped (`template_facts`), so it is
            // blamed on that token's root.
            let first = m
                .tmpl_toks(tm)
                .into_iter()
                .find(|u| m.escaped.contains_key(u));
            if let Some(&r) = first.and_then(|u| l.tok_root.get(&u)) {
                out.push(r);
            }
        }
    }
    out.sort_unstable();
    out.dedup();
    out
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
    pub(crate) part: usize,
    pub(crate) store: bool,
    /// No root: the site's own refusal (`None` when a root is blamed).
    pub(crate) why: Option<Unattributed>,
}

/// A refused site no root explains: its verdict and where it is.
pub(crate) struct Unattributed {
    pub(crate) why: String,
    /// Module, offset, key, read/store, receiver tokens, ⊤ origin (TSV).
    pub(crate) detail: String,
}

/// Write the ledger TSV (sorted by first mass) and its summary.
pub(crate) fn write(m: &Model, refused: &[Refused], totals: [[usize; 3]; 2]) {
    let (Some(path), Some(l)) = (path(), m.ledger.as_deref()) else {
        return;
    };
    // Per root: [first; 3 parts], any, first stores.
    let mut mass: FxHashMap<u32, ([usize; 3], usize, usize)> = FxHashMap::default();
    let mut unattributed = [0usize; 3];
    // Per refusal of the unattributed sites: [3 parts], stores.
    let mut by_why: FxHashMap<&str, ([usize; 3], usize)> = FxHashMap::default();
    let mut details = String::from("why\tmodule\toffset\tkey\taccess\treceivers\ttop_origin\n");
    let seq_of = |r: u32| r;
    for s in refused {
        let Some(&first) = s.roots.iter().min_by_key(|&&r| (seq_of(r), r)) else {
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
            continue;
        };
        let e = mass.entry(first).or_default();
        e.0[s.part] += 1;
        if s.store {
            e.2 += 1;
        }
        for &r in &s.roots {
            mass.entry(r).or_default().1 += 1;
        }
    }
    let mut rows: Vec<(u32, [usize; 3], usize, usize)> = mass
        .into_iter()
        .map(|(r, (f, a, st))| (r, f, a, st))
        .collect();
    let sum = |f: &[usize; 3]| f.iter().sum::<usize>();
    rows.sort_by(|a, b| {
        sum(&b.1)
            .cmp(&sum(&a.1))
            .then(b.2.cmp(&a.2))
            .then(seq_of(a.0).cmp(&seq_of(b.0)))
    });
    let clean = |s: &str| s.replace(['\t', '\n', '\r'], " ");
    let mut tsv = String::from(
        "seq\tkind\treason\twhere\tmass_first\tmass_any\tcore\taddons\trest\tstores_first\n",
    );
    for (r, f, a, st) in &rows {
        let x = &l.roots[*r as usize];
        let reason = m.reasons.get(x.reason as usize).map_or("-", |s| s.as_str());
        let (seq, kind) = (x.seq, x.kind);
        let at = format!("{}:{} {}", short(x.loc.file()), x.loc.line(), x.at);
        tsv.push_str(&format!(
            "{seq}\t{kind}\t{}\t{at}\t{}\t{a}\t{}\t{}\t{}\t{st}\n",
            clean(reason),
            sum(f),
            f[0],
            f[1],
            f[2]
        ));
    }
    tsv.push_str(&format!(
        "-\tnone\tunattributed (refused, no hold on the receiver)\t-\t{}\t-\t{}\t{}\t{}\t-\n",
        sum(&unattributed),
        unattributed[0],
        unattributed[1],
        unattributed[2]
    ));
    let mut whys: Vec<(&str, ([usize; 3], usize))> = by_why.into_iter().collect();
    whys.sort_by(|a, b| sum(&b.1 .0).cmp(&sum(&a.1 .0)).then(a.0.cmp(b.0)));
    for (why, (f, st)) in &whys {
        tsv.push_str(&format!(
            "-\tunattributed\t{}\t-\t{}\t-\t{}\t{}\t{}\t{st}\n",
            clean(why),
            sum(f),
            f[0],
            f[1],
            f[2]
        ));
    }
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
    let _ = std::fs::write(&path, tsv);
    let _ = std::fs::write(format!("{path}.summary"), &summary);
    eprint!("{summary}");
}

fn short(f: &str) -> &str {
    f.rsplit_once("src/").map_or(f, |(_, x)| x)
}
