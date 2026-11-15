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

use crate::model::{Cons, Model, NodeId, Tok, NO_NODE, TOP};
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
    out: Option<std::io::BufWriter<std::fs::File>>,
    why: Option<WhyRec>,
    edges: crate::pts::FxHashMap<(NodeId, NodeId), EdgeOrigin>,
    pub(crate) lines: u64,
}

impl Prov {
    /// The provenance file next to the dump, when `PERRY_FACTS_PROVENANCE=1`
    /// (with relations); the in-memory record when `PERRY_FACTS_WHY=1` or
    /// `PERRY_FACTS_STOP=hold` (read once per solve, here).
    pub(crate) fn from_env(relations: bool, mods: impl FnOnce() -> Vec<String>) -> Option<Box<Prov>> {
        let out = match std::env::var_os("PERRY_FACTS_DUMP") {
            Some(mut path)
                if relations
                    && std::env::var("PERRY_FACTS_PROVENANCE").ok().as_deref() == Some("1") =>
            {
                path.push(".prov");
                match std::fs::File::create(&path) {
                    Ok(f) => Some(std::io::BufWriter::with_capacity(1 << 20, f)),
                    Err(e) => {
                        eprintln!("perry: wpfacts: cannot write provenance: {e}");
                        None
                    }
                }
            }
            _ => None,
        };
        let stop = std::env::var("PERRY_FACTS_STOP").ok();
        let stop_hold = stop.as_deref() == Some("hold");
        let why = (stop_hold || std::env::var("PERRY_FACTS_WHY").ok().as_deref() == Some("1")).then(|| {
            WhyRec {
                solve: SOLVES.fetch_add(1, std::sync::atomic::Ordering::Relaxed) + 1,
                stop_hold,
                held: false,
                first: Default::default(),
                by_node: Default::default(),
                mods: mods(),
            }
        });
        if out.is_none() && why.is_none() {
            return None;
        }
        Some(Box::new(Prov { out, why, edges: Default::default(), lines: 0 }))
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
        let toks: Vec<Tok> = fresh.collect();
        if self.prov.as_ref().is_some_and(|p| p.why.is_some()) {
            for &t in &toks {
                self.why_note(n, WVal::Tok(t), src, adopted, loc);
            }
        }
        if self.prov.as_ref().is_some_and(|p| p.out.is_none()) {
            return;
        }
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
        let Some(out) = p.out.as_mut() else {
            return;
        };
        for t in toks {
            let _ = writeln!(out, "P {n} {t} {from} {how} r{r}");
            p.lines += 1;
        }
    }

    /// The node and instance tables, then flush (end of the solve).
    pub(crate) fn prov_finish(&mut self) {
        let Some(mut p) = self.prov.take() else {
            return;
        };
        let Some(mut out) = p.out.take() else {
            return;
        };
        let mut orig: crate::pts::FxHashMap<NodeId, NodeId> = Default::default();
        for i in 0..self.sum.insts.len() as u32 {
            self.inst_copies(i, |o, c| {
                orig.insert(c, o);
            });
        }
        // The last field is the run's value (`Inst::val`): `-` for the
        // shared run.
        for (&(k, l, t, v), &i) in self.sum.inst_key.iter() {
            let f = self.sum.insts[i as usize].f;
            let _ = match v {
                crate::summary::NO_VAL => writeln!(out, "I {i} {k} {f} {t} {l} -"),
                v => writeln!(out, "I {i} {k} {f} {t} {l} {v}"),
            };
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
            let _ = writeln!(out, "N {n} {f} {l} {fr} {o}");
        }
        let _ = out.flush();
        eprintln!("perry: wpfacts provenance: {} first arrivals, {} edges", p.lines, p.edges.len());
    }
}

// WHYHOLD1: `PERRY_FACTS_WHY=1` keeps, for the first arrival of each
// distinct value at a node (a token, ⊤ among them; one primitive-kind bit;
// one string entry; the any-string entry), ONE parent: the source node, how
// it was delivered (copy edge, adoption, a constraint of the source, a
// summary seed, a direct add), the call being bound, the code location and
// the work counter. `PERRY_FACTS_STOP=hold` (implies it) prints the chain
// from the first realm hold's callee value back to parentless arrivals and
// exits with 86. Never read by the facts; unset, `Model::prov` is `None`.

static SOLVES: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
static LAST: std::sync::Mutex<Vec<WhyHop>> = std::sync::Mutex::new(Vec::new());

/// A value a node holds, at the record's grain.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub(crate) enum WVal {
    Tok(Tok),
    Prim(u16),
    Str(u32),
    AnyStr,
}

/// The one parent of a first arrival.
#[derive(Clone, Copy)]
pub(crate) struct Parent {
    src: NodeId,
    mode: u8,
    adopted: bool,
    rule: Option<(NodeId, Cons)>,
    edge: Option<EdgeOrigin>,
    call: u32,
    work: u64,
    loc: &'static Location<'static>,
}

pub(crate) struct WhyRec {
    solve: u32,
    stop_hold: bool,
    held: bool,
    first: crate::pts::FxHashMap<(NodeId, WVal), Parent>,
    /// Per node, its first arrivals in solve order (work, value).
    by_node: crate::pts::FxHashMap<NodeId, Vec<(u64, WVal)>>,
    mods: Vec<String>,
}

/// One hop of a printed chain (the record API).
#[derive(Clone, Debug)]
pub struct WhyHop {
    pub solve: u32,
    pub depth: u32,
    pub node: NodeId,
    pub label: String,
    pub func: String,
    pub value: String,
    pub via: String,
    pub site: String,
    pub work: u64,
    pub at: String,
    /// "" (the walk goes on), "seed" (no parent: a maker, a builtin model,
    /// a literal), "seen" (printed already), "norec" (no record).
    pub end: &'static str,
}

impl std::fmt::Display for WhyHop {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "WHY solve={} {} n{} label={} fn={} value={} via={} site={} work={} at={}",
            self.solve, self.depth, self.node, self.label, self.func, self.value, self.via, self.site, self.work, self.at
        )?;
        if !self.end.is_empty() {
            write!(f, " END={}", self.end)?;
        }
        Ok(())
    }
}

/// The chain the last hold of this process printed (tests).
pub fn take_chain() -> Vec<WhyHop> {
    std::mem::take(&mut *LAST.lock().unwrap_or_else(|e| e.into_inner()))
}

impl Model {
    pub(crate) fn why_note(
        &mut self,
        n: NodeId,
        v: WVal,
        src: NodeId,
        adopted: bool,
        loc: &'static Location<'static>,
    ) {
        let (mode, rule, call, work) = (self.prov_mode, self.prov_rule, self.prov_call, self.work.get());
        let Some(p) = self.prov.as_mut() else {
            return;
        };
        let edge = if src != NO_NODE { p.edges.get(&(src, n)).copied() } else { None };
        let Some(w) = p.why.as_mut() else {
            return;
        };
        if let std::collections::hash_map::Entry::Vacant(e) = w.first.entry((n, v)) {
            e.insert(Parent { src, mode, adopted, rule, edge, call, work, loc });
            w.by_node.entry(n).or_default().push((work, v));
        }
    }

    pub(crate) fn why_prim(&mut self, n: NodeId, newp: u16, loc: &'static Location<'static>) {
        if !self.prov.as_ref().is_some_and(|p| p.why.is_some()) {
            return;
        }
        let src = self.cur_src;
        for b in 0..16 {
            if newp & (1 << b) != 0 {
                self.why_note(n, WVal::Prim(1 << b), src, false, loc);
            }
        }
    }

    /// `n`'s strings grew from `old`: one record per new entry.
    pub(crate) fn why_strs(&mut self, n: NodeId, old: crate::model::StrSet, loc: &'static Location<'static>) {
        if !self.prov.as_ref().is_some_and(|p| p.why.is_some()) {
            return;
        }
        let src = self.cur_src;
        let new = self.nodes[n as usize].strs;
        if new == crate::model::STRS_TOP {
            return self.why_note(n, WVal::AnyStr, src, false, loc);
        }
        let had: Vec<_> = if old == crate::model::STRS_NONE { Vec::new() } else { self.str_atoms(old).to_vec() };
        let add: Vec<_> = self.str_atoms(new).iter().copied().filter(|a| !had.contains(a)).collect();
        for a in add {
            self.why_note(n, WVal::Str(a as u32), src, false, loc);
        }
    }

    fn why_fn(&self, n: NodeId) -> String {
        let Some(w) = self.prov.as_ref().and_then(|p| p.why.as_ref()) else {
            return "-".into();
        };
        let f = self.node_fn.get(n as usize).copied().unwrap_or(crate::model::NO_FN);
        if f == crate::model::NO_FN {
            return "-".into();
        }
        self.funcs.get(f as usize).map_or("-".into(), |fi| {
            let m = w.mods.get(fi.module as usize).map_or("?", |s| s.as_str());
            let m = m.rsplit('/').next().unwrap_or(m);
            match fi.name.contains(':') {
                true => fi.name.clone(),
                false => format!("{m}:{}", fi.name),
            }
        })
    }

    fn why_val(&self, v: WVal) -> String {
        let s = match v {
            WVal::Tok(t) => format!("t{t}:{:?}", self.toks[t as usize]),
            WVal::Prim(b) => format!("prim:{b:#x}"),
            WVal::Str(a) => format!("str:{:?}", self.atoms.get(a as usize).map_or("?", |s| s.as_str())),
            WVal::AnyStr => "any-string".into(),
        };
        let s: String = s.chars().take(120).map(|c| if c.is_whitespace() { '_' } else { c }).collect();
        s
    }

    fn why_site(&self, call: u32) -> String {
        match self.calls.get(call as usize) {
            Some(c) => format!("c{call}/s{:?}", c.site),
            None => "-".into(),
        }
    }

    fn why_hop(&self, solve: u32, depth: u32, n: NodeId, v: WVal, p: Option<&Parent>, end: &'static str) -> WhyHop {
        let (via, site, work, at) = match p {
            None => ("-".to_string(), "-".to_string(), 0, "-".to_string()),
            Some(p) => {
                let mut via = if p.adopted {
                    "adopt".to_string()
                } else {
                    match p.mode {
                        PM_COPY => "copy".to_string(),
                        PM_SEED => "seed".to_string(),
                        PM_RULE => match p.rule {
                            Some((rn, c)) => format!("rule:{}@n{rn}", c.kind_name()),
                            None => "rule".to_string(),
                        },
                        _ => "add".to_string(),
                    }
                };
                if p.src != NO_NODE {
                    via.push_str(&format!("<-n{}", p.src));
                }
                let mut call = if p.call != 0 { p.call - 1 } else { u32::MAX };
                if let Some(e) = p.edge {
                    via.push_str(&format!("|edge@{}:{}", short(e.loc.file()), e.loc.line()));
                    if call == u32::MAX && e.call != 0 {
                        call = e.call - 1;
                    }
                    if let Some((_, c)) = e.rule {
                        via.push_str(&format!("|{}", c.kind_name()));
                    }
                }
                (via, self.why_site(call), p.work, format!("{}:{}", short(p.loc.file()), p.loc.line()))
            }
        };
        WhyHop {
            solve,
            depth,
            node: n,
            label: self.labels.get(n as usize).copied().unwrap_or("?").replace(char::is_whitespace, "_"),
            func: self.why_fn(n),
            value: self.why_val(v),
            via,
            site,
            work,
            at,
            end,
        }
    }

    /// Walk the parents of `v` at `n` back to a parentless arrival. A hop
    /// that keeps the value goes on with it at the source; one that changes
    /// it (a constraint's output, a builtin model's new token) goes on with
    /// the source's latest first arrival at or before it (the delta being
    /// applied then).
    fn why_walk(&self, solve: u32, n: NodeId, v: WVal, hops: &mut Vec<WhyHop>, seen: &mut crate::pts::FxHashSet<(NodeId, WVal)>) {
        let Some(w) = self.prov.as_ref().and_then(|p| p.why.as_ref()) else {
            return;
        };
        let (mut n, mut v, mut d) = (n, v, 0u32);
        loop {
            if !seen.insert((n, v)) {
                hops.push(self.why_hop(solve, d, n, v, None, "seen"));
                return;
            }
            let Some(p) = w.first.get(&(n, v)).copied() else {
                hops.push(self.why_hop(solve, d, n, v, None, "norec"));
                return;
            };
            if p.src == NO_NODE || p.src == n {
                hops.push(self.why_hop(solve, d, n, v, Some(&p), "seed"));
                return;
            }
            let same = w.first.get(&(p.src, v)).is_some_and(|q| q.work <= p.work);
            let next = if same {
                Some(v)
            } else {
                w.by_node.get(&p.src).and_then(|l| {
                    let i = l.partition_point(|&(wk, _)| wk <= p.work);
                    l[..i].last().map(|&(_, x)| x)
                })
            };
            let Some(nv) = next else {
                hops.push(self.why_hop(solve, d, n, v, Some(&p), "seed"));
                return;
            };
            hops.push(self.why_hop(solve, d, n, v, Some(&p), ""));
            (n, v, d) = (p.src, nv, d + 1);
        }
    }

    /// The first realm hold of this solve: print the chain of the value
    /// that made the call unknown; under `PERRY_FACTS_STOP=hold` exit 86.
    pub(crate) fn why_hold(&mut self, kind: &'static str, call: u32, t: Option<Tok>) {
        self.why_hold_r(kind, call, t, u32::MAX)
    }

    /// `why_hold` with the reason the hold names (`Model::reasons`). The
    /// walk starts at the node whose constraint runs (else the delivering
    /// node) with the value given (the callee token; an escaping token) and,
    /// when the node holds ⊤ beside it, with ⊤ too; with no value given,
    /// with ⊤ when the node holds it, else with every token it holds.
    pub(crate) fn why_hold_r(&mut self, kind: &'static str, call: u32, t: Option<Tok>, reason: u32) {
        let Some(w) = self.prov.as_mut().and_then(|p| p.why.as_mut()) else {
            return;
        };
        if w.held {
            return;
        }
        w.held = true;
        let (solve, stop) = (w.solve, w.stop_hold);
        let start = self.prov_rule.map(|(n, _)| n).unwrap_or(self.cur_src);
        let mut vals: Vec<WVal> = Vec::new();
        let has_top = start != NO_NODE && self.nodes[start as usize].pts.contains_tok(TOP);
        match t {
            Some(t) => {
                vals.push(WVal::Tok(t));
                if t != TOP && has_top {
                    vals.push(WVal::Tok(TOP));
                }
            }
            None if has_top => vals.push(WVal::Tok(TOP)),
            None if start != NO_NODE => {
                let mut ts = Vec::new();
                self.nodes[start as usize].pts.sorted_into(&mut ts);
                vals.extend(ts.into_iter().map(WVal::Tok));
            }
            None => {}
        }
        let rule = self.prov_rule.map_or("-".to_string(), |(rn, c)| format!("{}@n{rn}", c.kind_name()));
        let f = crate::unksrc::call_fn(self, call);
        let fname = self.funcs.get(f as usize).map_or("-".to_string(), |fi| fi.name.clone());
        eprintln!(
            "WHY solve={solve} HOLD kind={kind} site={} fn={fname} start=n{start} rule={rule} work={} values={} reason={}",
            self.why_site(call),
            self.work.get(),
            vals.len(),
            self.reasons.get(reason as usize).map_or("-", |s| s.as_str()).replace(char::is_whitespace, "_")
        );
        let mut hops = Vec::new();
        let mut seen = crate::pts::FxHashSet::default();
        if start != NO_NODE {
            for v in vals {
                self.why_walk(solve, start, v, &mut hops, &mut seen);
            }
        }
        for h in &hops {
            eprintln!("{h}");
        }
        let (recs, nodes) = self
            .prov
            .as_ref()
            .and_then(|p| p.why.as_ref())
            .map_or((0, 0), |w| (w.first.len(), w.by_node.len()));
        eprintln!("WHY solve={solve} END hops={} records={recs} nodes={nodes}", hops.len());
        *LAST.lock().unwrap_or_else(|e| e.into_inner()) = hops;
        if stop {
            use std::io::Write as _;
            let _ = std::io::stderr().flush();
            std::process::exit(86);
        }
    }
}
