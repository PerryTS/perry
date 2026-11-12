//! First-arrival provenance (`PERRY_FACTS_PROVENANCE=1`, diagnostic only):
//! for the first arrival of each (node, token) pair one line
//! `P <node> <tok> <from|-> <how> r<relations line>` in solve order, and at
//! the end one `N` line per node and one `I` line per summary instance, so
//! `why.py` can walk a token back to its birth. Nothing here is read by the
//! facts; with the variable unset `Model::prov` is `None` and no arm runs.
//!
//! Every token reaches a node through `add_tokens_s` or `adopt_delivery`
//! (`solve.rs`, `solve/copy.rs`): those two write the `P` lines. `<how>`:
//! - `copy@<file:line>[|c<call>][|<rule>]`: delivery along the value-copy
//!   edge `<from> -> <node>`; the location is the code that made the edge,
//!   the call is the call being bound when it was made (`wire_args`,
//!   `bind_args`, `bind_via`), the rule the constraint being applied then.
//! - `adopt@...`: the same edge, delivered by set adoption.
//! - `rule:<constraint>@<file:line>`: a constraint of `<from>` added it
//!   directly (the location is the add in the rule's arm).
//! - `seed@<file:line>`: a summary instance's seed; `<from>` is the
//!   template node.
//! - `add@<file:line>`: a birth or direct add (generation, natives).

use crate::model::{Cons, Model, NodeId, Tok, NO_NODE};
use std::io::Write;
use std::panic::Location;

/// How the current arrival is delivered (`Model::prov_mode`).
pub(crate) const PM_ADD: u8 = 0;
pub(crate) const PM_COPY: u8 = 1;
pub(crate) const PM_RULE: u8 = 2;
pub(crate) const PM_SEED: u8 = 3;

/// Where an edge was made: the code location, the call being bound and the
/// constraint being applied at the time.
#[derive(Clone, Copy)]
pub(crate) struct EdgeOrigin {
    loc: &'static Location<'static>,
    call: u32,
    rule: Option<(NodeId, Cons)>,
}

pub struct Prov {
    out: std::io::BufWriter<std::fs::File>,
    edges: crate::pts::FxHashMap<(NodeId, NodeId), EdgeOrigin>,
    pub(crate) lines: u64,
}

impl Prov {
    /// The provenance file next to the dump, when `PERRY_FACTS_PROVENANCE=1`.
    pub(crate) fn from_env() -> Option<Box<Prov>> {
        if std::env::var("PERRY_FACTS_PROVENANCE").ok().as_deref() != Some("1") {
            return None;
        }
        let mut path = std::env::var_os("PERRY_FACTS_DUMP")?;
        path.push(".prov");
        match std::fs::File::create(&path) {
            Ok(f) => Some(Box::new(Prov {
                out: std::io::BufWriter::with_capacity(1 << 20, f),
                edges: Default::default(),
                lines: 0,
            })),
            Err(e) => {
                eprintln!("perry: wpfacts: cannot write provenance: {e}");
                None
            }
        }
    }
}

fn short(f: &str) -> &str {
    f.rsplit_once("src/").map_or(f, |(_, x)| x)
}

impl Model {
    /// Record where the new edge `a -> b` was made.
    pub(crate) fn prov_edge(&mut self, a: NodeId, b: NodeId, loc: &'static Location<'static>) {
        let (call, rule) = (self.prov_call, self.prov_rule);
        if let Some(p) = self.prov.as_mut() {
            p.edges.entry((a, b)).or_insert(EdgeOrigin { loc, call, rule });
        }
    }

    fn prov_rule_text(&self, r: Option<(NodeId, Cons)>) -> String {
        match r {
            Some((n, c)) => {
                let l = self.cons_line(n, &c);
                let mut s: String = l
                    .chars()
                    .take(80)
                    .map(|ch| if ch.is_whitespace() { '_' } else { ch })
                    .collect();
                if s.is_empty() {
                    s.push_str(c.kind_name());
                }
                s
            }
            None => String::new(),
        }
    }

    fn prov_edge_text(&self, kind: &str, a: NodeId, b: NodeId) -> String {
        let Some(o) = self.prov.as_ref().and_then(|p| p.edges.get(&(a, b)).copied()) else {
            return format!("{kind}@?");
        };
        let mut s = format!("{kind}@{}:{}", short(o.loc.file()), o.loc.line());
        if o.call != 0 {
            s.push_str(&format!("|c{}", o.call - 1));
        }
        if o.rule.is_some() {
            s.push('|');
            s.push_str(&self.prov_rule_text(o.rule));
        }
        s
    }

    /// `P` lines for the tokens `fresh` arriving at `n` for the first time.
    pub(crate) fn prov_arrivals(
        &mut self,
        n: NodeId,
        fresh: &mut dyn Iterator<Item = Tok>,
        adopted: bool,
        loc: &'static Location<'static>,
    ) {
        let src = self.cur_src;
        let how = if adopted {
            self.prov_edge_text("adopt", src, n)
        } else {
            match self.prov_mode {
                PM_COPY if src != NO_NODE => self.prov_edge_text("copy", src, n),
                PM_RULE => format!(
                    "rule:{}@{}:{}",
                    self.prov_rule_text(self.prov_rule),
                    short(loc.file()),
                    loc.line()
                ),
                PM_SEED => format!("seed@{}:{}", short(loc.file()), loc.line()),
                _ => format!("add@{}:{}", short(loc.file()), loc.line()),
            }
        };
        let from = if src == NO_NODE { "-".to_string() } else { src.to_string() };
        let r = self.relations.as_ref().map_or(0, |r| r.len());
        let Some(p) = self.prov.as_mut() else {
            return;
        };
        for t in fresh {
            let _ = writeln!(p.out, "P {n} {t} {from} {how} r{r}");
            p.lines += 1;
        }
    }

    /// The node and instance tables, then flush (end of the solve).
    pub(crate) fn prov_finish(&mut self) {
        let Some(mut p) = self.prov.take() else {
            return;
        };
        let mut orig: crate::pts::FxHashMap<NodeId, NodeId> = Default::default();
        for i in 0..self.sum.insts.len() as u32 {
            self.inst_copies(i, |o, c| {
                orig.insert(c, o);
            });
        }
        for (&(k, l, t, _), &i) in self.sum.inst_key.iter() {
            let f = self.sum.insts[i as usize].f;
            let _ = writeln!(p.out, "I {i} {k} {f} {t} {l}");
        }
        for n in 0..self.nodes.len() {
            let f = self.node_fn.get(n).copied().unwrap_or(crate::model::NO_FN);
            let f = if f == crate::model::NO_FN { "-".to_string() } else { f.to_string() };
            let l = self.labels.get(n).copied().unwrap_or("?");
            let fr = self.nodes[n].frame;
            let fr = if fr == 0 { "-".to_string() } else { (fr - 1).to_string() };
            let o = orig
                .get(&(n as NodeId))
                .map_or("-".to_string(), |o| o.to_string());
            let _ = writeln!(p.out, "N {n} {f} {l} {fr} {o}");
        }
        let _ = p.out.flush();
        eprintln!("perry: wpfacts provenance: {} first arrivals, {} edges", p.lines, p.edges.len());
    }
}
