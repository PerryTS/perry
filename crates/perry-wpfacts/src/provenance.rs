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

use crate::model::{Cons, Key, Model, NodeId, Tok, NO_NODE, TOP};
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
    /// The constraint's input token when it made the edge, plus one (a
    /// load's or store's receiver; 0: none).
    tok: u32,
}

pub struct Prov {
    out: Option<std::io::BufWriter<std::fs::File>>,
    why: Option<WhyRec>,
    edges: crate::pts::FxHashMap<(NodeId, NodeId), EdgeOrigin>,
    pub(crate) lines: u64,
    /// The input token of the constraint being applied, plus one (0: none):
    /// the recorded cause of a rule's output. In `Prov`, so nothing of it
    /// runs or is stored while provenance is off.
    pub(crate) in_tok: u32,
}

impl Prov {
    /// The provenance file next to the dump, when `PERRY_FACTS_PROVENANCE=1`
    /// (with relations); the in-memory record when `PERRY_FACTS_WHY=1` or
    /// `PERRY_FACTS_STOP=hold` (read once per solve, here).
    pub(crate) fn from_env(
        relations: bool,
        mods: impl FnOnce() -> Vec<String>,
    ) -> Option<Box<Prov>> {
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
        let stop = match std::env::var("PERRY_FACTS_STOP").ok().as_deref() {
            Some("hold") => STOP_HOLD,
            Some("realm") => STOP_REALM,
            Some("realm-all") => STOP_REALM_ALL,
            Some("wide") => STOP_WIDE,
            _ => STOP_NONE,
        };
        let why = (stop != STOP_NONE
            || std::env::var("PERRY_FACTS_WHY").ok().as_deref() == Some("1"))
        .then(|| {
            let solve = SOLVES.fetch_add(1, std::sync::atomic::Ordering::Relaxed) + 1;
            let sel = parse_sel();
            let mut sel_nodes: crate::pts::FxHashMap<NodeId, usize> = Default::default();
            for (i, x) in sel.iter().enumerate() {
                if let Some(n) = x.node {
                    sel_nodes.insert(n, i);
                    eprintln!("WHY select n{n} solve={solve} sel={} by=id", x.raw);
                }
            }
            WhyRec {
                solve,
                stop,
                held: false,
                realm_done: false,
                wide_done: false,
                pending: None,
                first: Default::default(),
                by_node: Default::default(),
                mods: mods(),
                sel,
                sel_nodes,
                sel_checked: Default::default(),
                mints: Default::default(),
                implicit_by: Default::default(),
                arrive: std::env::var("PERRY_FACTS_WHY_ARRIVE").ok().as_deref() == Some("1"),
            }
        });
        if out.is_none() && why.is_none() {
            return None;
        }
        Some(Box::new(Prov {
            out,
            why,
            edges: Default::default(),
            lines: 0,
            in_tok: 0,
        }))
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
            let tok = p.in_tok;
            p.edges.entry((a, b)).or_insert(EdgeOrigin {
                loc,
                call,
                rule,
                tok,
            });
        }
    }

    /// The constraint being applied now works on input token `t` (the
    /// recorded cause of what it delivers).
    /// `on` is `self.prov.is_some()`, read once per constraint application
    /// (`apply_inner`): while provenance is off this is one test of a
    /// register and no store.
    #[inline(always)]
    pub(crate) fn prov_tok(&mut self, on: bool, t: Tok) {
        if on {
            self.prov_tok_on(t);
        }
    }

    #[cold]
    #[inline(never)]
    fn prov_tok_on(&mut self, t: Tok) {
        if let Some(p) = self.prov.as_mut() {
            p.in_tok = t + 1;
        }
    }

    /// Clear the recorded input token (a new constraint starts); returns
    /// the previous one. Called only while provenance is on.
    #[cold]
    #[inline(never)]
    pub(crate) fn prov_in_take(&mut self) -> u32 {
        self.prov
            .as_mut()
            .map_or(0, |p| std::mem::take(&mut p.in_tok))
    }

    /// Set the recorded input token. Called only while provenance is on.
    #[cold]
    #[inline(never)]
    pub(crate) fn prov_in_set(&mut self, v: u32) {
        if let Some(p) = self.prov.as_mut() {
            p.in_tok = v;
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
        let Some(o) = self
            .prov
            .as_ref()
            .and_then(|p| p.edges.get(&(a, b)).copied())
        else {
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
        let from = if src == NO_NODE {
            "-".to_string()
        } else {
            src.to_string()
        };
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
            let f = if f == crate::model::NO_FN {
                "-".to_string()
            } else {
                f.to_string()
            };
            let l = self.labels.get(n).copied().unwrap_or("?");
            let fr = self.nodes[n].frame;
            let fr = if fr == 0 {
                "-".to_string()
            } else {
                (fr - 1).to_string()
            };
            let o = orig
                .get(&(n as NodeId))
                .map_or("-".to_string(), |o| o.to_string());
            let _ = writeln!(out, "N {n} {f} {l} {fr} {o}");
        }
        let _ = out.flush();
        eprintln!(
            "perry: wpfacts provenance: {} first arrivals, {} edges",
            p.lines,
            p.edges.len()
        );
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
    /// A rule's input token at delivery, plus one (0: none recorded).
    cause: u32,
}

pub(crate) const STOP_NONE: u8 = 0;
pub(crate) const STOP_HOLD: u8 = 1;
pub(crate) const STOP_REALM: u8 = 2;
pub(crate) const STOP_WIDE: u8 = 3;
/// `PERRY_FACTS_STOP=realm-all` (diagnostic): one `WHY ... HOLDSRC` line per
/// hold source (kind, site, function, node, value, reason) and one
/// `REALM-ALL` line at the flip; the solve goes on. Beside
/// `PERRY_FACTS_LEDGER` (the realm is never held) it lists every source,
/// not only those before the flip.
pub(crate) const STOP_REALM_ALL: u8 = 4;

/// A `PERRY_FACTS_WHY_AT` selector: `n<id>`, or `<fn-substring>#<label |
/// parameter name | p<index>>`; `:prim` also stops at a non-string
/// primitive.
pub(crate) struct Sel {
    raw: String,
    node: Option<NodeId>,
    fnsub: String,
    what: String,
    prim: bool,
    /// `:undef`: stop at `undefined` only.
    undef: bool,
}

fn parse_sel() -> Vec<Sel> {
    let Ok(v) = std::env::var("PERRY_FACTS_WHY_AT") else {
        return Vec::new();
    };
    v.split(',')
        .map(str::trim)
        .filter(|x| !x.is_empty())
        .map(|x| {
            let (x, undef) = match x.strip_suffix(":undef") {
                Some(b) => (b, true),
                None => (x, false),
            };
            let (body, prim) = match x.strip_suffix(":prim") {
                Some(b) => (b, true),
                None => (x, false),
            };
            let node = body
                .strip_prefix('n')
                .and_then(|d| d.parse::<NodeId>().ok());
            let (fnsub, what) = match body.split_once('#') {
                Some((f, w)) => (f.to_string(), w.to_string()),
                None => (String::new(), body.to_string()),
            };
            Sel {
                raw: x.to_string(),
                node,
                fnsub,
                what,
                prim,
                undef,
            }
        })
        .collect()
}

/// A hold seen under `STOP=realm`: what the realm flip that follows prints.
#[derive(Clone, Copy)]
struct Pending {
    kind: &'static str,
    call: u32,
    t: Option<Tok>,
    reason: u32,
    start: NodeId,
    rule: Option<(NodeId, Cons)>,
    work: u64,
}

pub(crate) struct WhyRec {
    solve: u32,
    stop: u8,
    held: bool,
    realm_done: bool,
    wide_done: bool,
    pending: Option<Pending>,
    first: crate::pts::FxHashMap<(NodeId, WVal), Parent>,
    /// Per node, its first arrivals in solve order (work, value).
    by_node: crate::pts::FxHashMap<NodeId, Vec<(u64, WVal)>>,
    mods: Vec<String>,
    sel: Vec<Sel>,
    /// Selected node -> its selector (`sel`).
    sel_nodes: crate::pts::FxHashMap<NodeId, usize>,
    /// Nodes already matched against the label selectors.
    sel_checked: crate::pts::FxHashSet<NodeId>,
    /// Calls minted outside generation (`Model::why_mint`).
    mints: crate::pts::FxHashMap<u32, Mint>,
    /// (node, reason) -> the model call whose `implicit_args` touched it.
    implicit_by: crate::pts::FxHashMap<(NodeId, u32), u32>,
    /// `PERRY_FACTS_WHY_ARRIVE=1`: print every first arrival of a value at
    /// a node an `n<id>` selector names (its value set as it grows, with
    /// the source position of the rule or edge that delivered it).
    arrive: bool,
}

/// Where a call without a source site was minted (the solver or a builtin
/// model), what it was made for and what was being applied then.
#[derive(Clone, Copy)]
pub(crate) struct Mint {
    loc: &'static Location<'static>,
    src: NodeId,
    rule: Option<(NodeId, Cons)>,
    tok: Option<Tok>,
    reason: u32,
    /// The call it runs for (a builtin's callback call), u32::MAX: none.
    by: u32,
}

/// The call a call-carrying constraint names, and its kind.
fn cons_call(c: &Cons) -> Option<u32> {
    match *c {
        Cons::Call { call }
        | Cons::CallThis { call, .. }
        | Cons::New { call }
        | Cons::NewProgram { call }
        | Cons::ReflectNew { call, .. }
        | Cons::ReflectNewTarget { call, .. }
        | Cons::NewNamed { call, .. }
        | Cons::MCall { call, .. }
        | Cons::ProtoMethods { call, .. }
        | Cons::SuperCtor { call, .. } => Some(call),
        _ => None,
    }
}

/// The constraint reads or writes a key no constant names.
fn cons_key_any(c: &Cons) -> bool {
    match c {
        Cons::Load { key, .. }
        | Cons::Store { key, .. }
        | Cons::MCall { key, .. }
        | Cons::Chain { key, .. }
        | Cons::ChainUnion { key, .. } => matches!(key, Key::Any(_)),
        _ => false,
    }
}

impl Prov {
    /// A summary instance's slot `copy` of `orig`: selected when `orig` is.
    pub(crate) fn why_copy_sel(&mut self, copy: NodeId, orig: NodeId) {
        let Some(w) = self.why.as_mut() else {
            return;
        };
        if let Some(&i) = w.sel_nodes.get(&orig) {
            w.sel_nodes.insert(copy, i);
            w.sel_checked.insert(copy);
            eprintln!(
                "WHY select n{copy} solve={} copy-of=n{orig} sel={} by=instance",
                w.solve, w.sel[i].raw
            );
        }
    }
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
    /// The hop's call (u32::MAX: none), the rule's node and the receiver
    /// token its edge was made for: what `why_calls` explains.
    pub call: u32,
    pub rule_node: NodeId,
    pub recv: Option<Tok>,
}

impl std::fmt::Display for WhyHop {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "WHY solve={} {} n{} label={} fn={} value={} via={} site={} work={} at={}",
            self.solve,
            self.depth,
            self.node,
            self.label,
            self.func,
            self.value,
            self.via,
            self.site,
            self.work,
            self.at
        )?;
        if !self.end.is_empty() {
            write!(f, " END={}", self.end)?;
        }
        Ok(())
    }
}

/// The chain the last hold of this process printed (tests).
#[cfg(test)]
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
        let (mode, rule, call, work) = (
            self.prov_mode,
            self.prov_rule,
            self.prov_call,
            self.work.get(),
        );
        let Some(p) = self.prov.as_mut() else {
            return;
        };
        let cause = if mode == PM_RULE && !adopted {
            p.in_tok
        } else {
            0
        };
        let edge = if src != NO_NODE {
            p.edges.get(&(src, n)).copied()
        } else {
            None
        };
        let Some(w) = p.why.as_mut() else {
            return;
        };
        let mut check = false;
        let mut arrive = false;
        if let std::collections::hash_map::Entry::Vacant(e) = w.first.entry((n, v)) {
            e.insert(Parent {
                src,
                mode,
                adopted,
                rule,
                edge,
                call,
                work,
                loc,
                cause,
            });
            w.by_node.entry(n).or_default().push((work, v));
            check = w.stop == STOP_WIDE && !w.wide_done;
            arrive = w.arrive && w.sel_nodes.contains_key(&n);
        }
        if arrive {
            let solve = w.solve;
            eprintln!(
                "WHY ARRIVE solve={solve} n{n} value={} src=n{src} work={work} at={loc}",
                self.why_val(v)
            );
        }
        if check {
            self.why_wide_check(n, v);
        }
    }

    /// A parameter `pname` (index `idx`) of `fname`: selected when a
    /// `<fn>#<name>` or `<fn>#p<idx>` selector names it.
    pub(crate) fn why_param(&mut self, n: NodeId, fname: &str, pname: &str, idx: usize) {
        let Some(w) = self.prov.as_mut().and_then(|p| p.why.as_mut()) else {
            return;
        };
        if w.stop != STOP_WIDE {
            return;
        }
        let pi = format!("p{idx}");
        let Some(i) = w.sel.iter().position(|s| {
            s.node.is_none()
                && fname.contains(s.fnsub.as_str())
                && (s.what == pname || s.what == pi)
        }) else {
            return;
        };
        w.sel_nodes.insert(n, i);
        w.sel_checked.insert(n);
        eprintln!(
            "WHY select n{n} solve={} fn={fname} label=param:{pname} sel={} by=param",
            w.solve, w.sel[i].raw
        );
    }

    /// `STOP=wide`: `v` is the first arrival of `v` at `n`. A selected node
    /// that gets the any-string entry, ⊤ or (`:prim`) a non-string
    /// primitive prints the chain and exits 86.
    fn why_wide_check(&mut self, n: NodeId, v: WVal) {
        let Some(w) = self.prov.as_ref().and_then(|p| p.why.as_ref()) else {
            return;
        };
        if !w.sel_checked.contains(&n) && w.sel.iter().any(|s| s.node.is_none()) {
            let label = self.labels.get(n as usize).copied().unwrap_or("?");
            let f = self
                .node_fn
                .get(n as usize)
                .copied()
                .unwrap_or(crate::model::NO_FN);
            let fname = (f != crate::model::NO_FN)
                .then(|| self.funcs.get(f as usize).map(|fi| fi.name.clone()))
                .flatten();
            let shown = self.why_fn(n);
            let w = self.prov.as_mut().and_then(|p| p.why.as_mut()).unwrap();
            w.sel_checked.insert(n);
            if let Some(fname) = fname {
                if let Some(i) = w.sel.iter().position(|s| {
                    s.node.is_none() && fname.contains(s.fnsub.as_str()) && s.what == label
                }) {
                    w.sel_nodes.insert(n, i);
                    eprintln!(
                        "WHY select n{n} solve={} fn={shown} label={label} sel={} by=label",
                        w.solve, w.sel[i].raw
                    );
                }
            }
        }
        let w = self.prov.as_mut().and_then(|p| p.why.as_mut()).unwrap();
        let Some(&i) = w.sel_nodes.get(&n) else {
            return;
        };
        let wide = match v {
            WVal::AnyStr => true,
            WVal::Tok(t) => t == TOP,
            WVal::Prim(b) => {
                (w.sel[i].prim && b != crate::model::prim::STR)
                    || (w.sel[i].undef && b == crate::model::prim::UNDEF)
            }
            WVal::Str(_) => false,
        };
        if !wide {
            return;
        }
        w.wide_done = true;
        let (solve, raw) = (w.solve, w.sel[i].raw.clone());
        let header = format!(
            "WHY solve={} WIDE n{n} sel={} fn={} label={} value={} work={}",
            solve,
            raw,
            self.why_fn(n),
            self.labels.get(n as usize).copied().unwrap_or("?"),
            self.why_val(v),
            self.work.get()
        );
        self.why_emit(header, n, vec![v], true);
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
    pub(crate) fn why_strs(
        &mut self,
        n: NodeId,
        old: crate::model::StrSet,
        loc: &'static Location<'static>,
    ) {
        if !self.prov.as_ref().is_some_and(|p| p.why.is_some()) {
            return;
        }
        let src = self.cur_src;
        let new = self.nodes[n as usize].strs;
        if new == crate::model::STRS_TOP {
            return self.why_note(n, WVal::AnyStr, src, false, loc);
        }
        let had: Vec<_> = if old == crate::model::STRS_NONE {
            Vec::new()
        } else {
            self.str_atoms(old).to_vec()
        };
        let add: Vec<_> = self
            .str_atoms(new)
            .iter()
            .copied()
            .filter(|a| !had.contains(a))
            .collect();
        for a in add {
            self.why_note(n, WVal::Str(a as u32), src, false, loc);
        }
    }

    fn why_fn(&self, n: NodeId) -> String {
        let Some(w) = self.prov.as_ref().and_then(|p| p.why.as_ref()) else {
            return "-".into();
        };
        let f = self
            .node_fn
            .get(n as usize)
            .copied()
            .unwrap_or(crate::model::NO_FN);
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
            WVal::Str(a) => format!(
                "str:{:?}",
                self.atoms.get(a as usize).map_or("?", |s| s.as_str())
            ),
            WVal::AnyStr => "any-string".into(),
        };
        let s: String = s
            .chars()
            .take(120)
            .map(|c| if c.is_whitespace() { '_' } else { c })
            .collect();
        s
    }

    fn why_site(&self, call: u32) -> String {
        match self.calls.get(call as usize) {
            Some(c) => format!("c{call}/s{:?}", c.site),
            None => "-".into(),
        }
    }

    fn why_hop(
        &self,
        solve: u32,
        depth: u32,
        n: NodeId,
        v: WVal,
        p: Option<&Parent>,
        end: &'static str,
    ) -> WhyHop {
        let (mut hcall, mut hrule, mut hrecv) = (u32::MAX, NO_NODE, None);
        if let Some(p) = p {
            hcall = if p.call != 0 { p.call - 1 } else { u32::MAX };
            if let Some((rn, _)) = p.rule {
                hrule = rn;
            }
            if let Some(e) = p.edge {
                if hcall == u32::MAX && e.call != 0 {
                    hcall = e.call - 1;
                }
                if let Some((rn, _)) = e.rule {
                    hrule = rn;
                }
                if e.tok != 0 {
                    hrecv = Some(e.tok - 1);
                }
            }
        }
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
                if p.mode == PM_RULE && !p.adopted {
                    if p.cause != 0 {
                        via.push_str(&format!("|in=t{}", p.cause - 1));
                    }
                    if p.rule.is_some_and(|(_, c)| cons_key_any(&c)) {
                        via.push_str("|key=any");
                    }
                }
                let mut call = if p.call != 0 { p.call - 1 } else { u32::MAX };
                if let Some(e) = p.edge {
                    via.push_str(&format!("|edge@{}:{}", short(e.loc.file()), e.loc.line()));
                    if call == u32::MAX && e.call != 0 {
                        call = e.call - 1;
                    }
                    if let Some((rn, c)) = e.rule {
                        via.push_str(&format!("|{}@n{rn}", c.kind_name()));
                        if cons_key_any(&c) {
                            via.push_str("|key=any");
                        }
                    }
                    if e.tok != 0 {
                        via.push_str(&format!("|recv=t{}", e.tok - 1));
                    }
                }
                (
                    via,
                    self.why_site(call),
                    p.work,
                    format!("{}:{}", short(p.loc.file()), p.loc.line()),
                )
            }
        };
        WhyHop {
            solve,
            depth,
            node: n,
            label: self
                .labels
                .get(n as usize)
                .copied()
                .unwrap_or("?")
                .replace(char::is_whitespace, "_"),
            func: self.why_fn(n),
            value: self.why_val(v),
            via,
            site,
            work,
            at,
            end,
            call: hcall,
            rule_node: hrule,
            recv: hrecv,
        }
    }

    /// `PERRY_FACTS_STOP` / `PERRY_FACTS_WHY` only: note where `call`, a
    /// call without a source site, was minted. Off: one `Option` test.
    #[track_caller]
    pub(crate) fn why_mint(&mut self, call: u32, tok: Option<Tok>, reason: u32) {
        let (src, rule) = (self.cur_src, self.prov_rule);
        if let Some(w) = self.prov.as_mut().and_then(|p| p.why.as_mut()) {
            w.mints.insert(
                call,
                Mint {
                    loc: Location::caller(),
                    src,
                    rule,
                    tok,
                    reason,
                    by: u32::MAX,
                },
            );
        }
    }

    /// `why_mint` of a call a builtin model makes while it runs for `by`.
    #[track_caller]
    pub(crate) fn why_mint_by(&mut self, call: u32, by: u32) {
        let (src, rule) = (self.cur_src, self.prov_rule);
        if let Some(w) = self.prov.as_mut().and_then(|p| p.why.as_mut()) {
            w.mints.insert(
                call,
                Mint {
                    loc: Location::caller(),
                    src,
                    rule,
                    tok: None,
                    reason: u32::MAX,
                    by,
                },
            );
        }
    }

    /// Diagnostic only: model call `call` puts a protocol touch with
    /// `reason` on argument node `n` (`builtins::implicit_args`).
    pub(crate) fn why_implicit(&mut self, n: NodeId, reason: u32, call: u32) {
        if let Some(w) = self.prov.as_mut().and_then(|p| p.why.as_mut()) {
            w.implicit_by.entry((n, reason)).or_insert(call);
        }
    }

    /// A node: label, owner function, first values.
    fn why_node(&self, n: NodeId) -> String {
        if n == NO_NODE || n as usize >= self.nodes.len() {
            return "-".into();
        }
        let vals = self.why_vals(n, None);
        let vs: Vec<String> = vals.iter().take(3).map(|&v| self.why_val(v)).collect();
        let more = if vals.len() > 3 {
            format!(",+{}", vals.len() - 3)
        } else {
            String::new()
        };
        let label = self
            .labels
            .get(n as usize)
            .copied()
            .unwrap_or("?")
            .replace(char::is_whitespace, "_");
        format!("n{n}[{label}|{}|{}{more}]", self.why_fn(n), vs.join(","))
    }

    /// For each call of the chain (and the calls their minting traces back
    /// to): owner, callee, receiver, arguments, and where it was minted.
    fn why_calls(&self, solve: u32, hops: &[WhyHop]) {
        let Some(w) = self.prov.as_ref().and_then(|p| p.why.as_ref()) else {
            return;
        };
        // Node -> (call, role), and call -> callee constraints: one scan.
        let mut role: crate::pts::FxHashMap<NodeId, Vec<(u32, &'static str)>> = Default::default();
        for (i, c) in self.calls.iter().enumerate() {
            let i = i as u32;
            if let crate::model::ThisArg::Node(n) = c.this {
                role.entry(n).or_default().push((i, "this"));
            }
            for &a in &c.args {
                role.entry(a).or_default().push((i, "arg"));
            }
            for &a in &c.spread {
                role.entry(a).or_default().push((i, "spread"));
            }
            role.entry(c.ret).or_default().push((i, "ret"));
        }
        let mut callee: crate::pts::FxHashMap<u32, Vec<(NodeId, &'static str)>> =
            Default::default();
        for (n, node) in self.nodes.iter().enumerate() {
            for c in &node.cons {
                if let Some(k) = cons_call(c) {
                    callee
                        .entry(k)
                        .or_default()
                        .push((n as NodeId, c.kind_name()));
                }
            }
        }
        let in_calls = |n: NodeId| -> String {
            role.get(&n).map_or("-".into(), |v| {
                v.iter()
                    .take(4)
                    .map(|(c, r)| format!("c{c}:{r}"))
                    .collect::<Vec<_>>()
                    .join(",")
            })
        };
        let mut queue: Vec<u32> = Vec::new();
        for h in hops {
            if h.call != u32::MAX {
                queue.push(h.call);
            }
            if h.rule_node != NO_NODE {
                let recv = h
                    .recv
                    .map_or("-".to_string(), |t| self.why_val(WVal::Tok(t)));
                let back = h
                    .recv
                    .and_then(|t| w.first.get(&(h.rule_node, WVal::Tok(t))))
                    .map_or("-".to_string(), |p| {
                        format!(
                            "n{}<-{} work={} at={}:{}",
                            h.rule_node,
                            self.why_node(p.src),
                            p.work,
                            short(p.loc.file()),
                            p.loc.line()
                        )
                    });
                eprintln!(
                    "WHY solve={solve} RULE hop={} node={} in={} recv={recv} recv-from={back}",
                    h.depth,
                    self.why_node(h.rule_node),
                    in_calls(h.rule_node)
                );
                for &(c, _) in role.get(&h.rule_node).into_iter().flatten().take(2) {
                    queue.push(c);
                }
            }
        }
        let mut done = crate::pts::FxHashSet::default();
        let mut touch_from: Vec<u32> = Vec::new();
        let mut k = 0;
        while k < queue.len() && done.len() < 16 {
            let id = queue[k];
            k += 1;
            if !done.insert(id) {
                continue;
            }
            let Some(c) = self.calls.get(id as usize) else {
                continue;
            };
            let this = match c.this {
                crate::model::ThisArg::Undef => "undef".to_string(),
                crate::model::ThisArg::Node(n) => self.why_node(n),
                crate::model::ThisArg::Tok(t) => format!("tok:{}", self.why_val(WVal::Tok(t))),
            };
            let args: Vec<String> = c.args.iter().take(3).map(|&a| self.why_node(a)).collect();
            let spread: Vec<String> = c.spread.iter().take(2).map(|&a| self.why_node(a)).collect();
            let ce: Vec<String> = callee
                .get(&id)
                .map(|v| {
                    v.iter()
                        .take(3)
                        .map(|&(n, kn)| format!("{kn}@{}", self.why_node(n)))
                        .collect()
                })
                .unwrap_or_default();
            let kind = match w.mints.get(&id) {
                Some(m) => format!("synth@{}:{}", short(m.loc.file()), m.loc.line()),
                None if c.site == id && self.call_place.contains_key(&id) => "gen".to_string(),
                None if c.site == id => "model-or-instance(site=self,no-place)".to_string(),
                None => format!("copy-of-site-c{}", c.site),
            };
            let owner = match self.call_place.get(&id) {
                Some(&(f, _)) => self
                    .funcs
                    .get(f as usize)
                    .map_or("-".to_string(), |fi| fi.name.clone()),
                None => self.why_fn(c.ret),
            };
            eprintln!(
                "WHY solve={solve} CALL c{id} site=s{} kind={kind} owner={owner} callee=[{}] this={this} args=[{}] spread=[{}] ret={}",
                c.site,
                ce.join(";"),
                args.join(";"),
                spread.join(";"),
                self.why_node(c.ret)
            );
            if let Some(m) = w.mints.get(&id) {
                let tok = m
                    .tok
                    .map_or("-".to_string(), |t| self.why_val(WVal::Tok(t)));
                let reason = self
                    .reasons
                    .get(m.reason as usize)
                    .map_or("-", |s| s.as_str())
                    .replace(char::is_whitespace, "_");
                let rule = m.rule.map_or("-".to_string(), |(rn, rc)| {
                    format!(
                        "{}@{} in={}",
                        rc.kind_name(),
                        self.why_node(rn),
                        in_calls(rn)
                    )
                });
                let from = match m.rule {
                    Some((rn, Cons::Implicit { reason: r, .. })) => {
                        w.implicit_by.get(&(rn, r)).copied()
                    }
                    _ => None,
                };
                if let Some(x) = from {
                    queue.push(x);
                    touch_from.push(x);
                }
                if m.by != u32::MAX {
                    queue.push(m.by);
                }
                eprintln!(
                    "WHY solve={solve} MINT c{id} for={tok} reason={reason} while={rule} touch-from-model-call={} by={} src={} src-in={}",
                    from.map_or("-".to_string(), |x| format!("c{x}")),
                    if m.by == u32::MAX { "-".to_string() } else { format!("c{}", m.by) },
                    self.why_node(m.src),
                    in_calls(m.src)
                );
                for n in [m.rule.map_or(NO_NODE, |(rn, _)| rn), m.src] {
                    for &(c2, _) in role.get(&n).into_iter().flatten().take(2) {
                        queue.push(c2);
                    }
                }
            }
        }
        // The model call that put the touch: every value of its callee
        // nodes (a method call's receiver), each with its chain.
        let mut walked = crate::pts::FxHashSet::default();
        for x in touch_from {
            for &(n, kn) in callee.get(&x).into_iter().flatten().take(3) {
                let vals = self.why_vals(n, None);
                let vs: Vec<String> = vals.iter().take(16).map(|&v| self.why_val(v)).collect();
                eprintln!(
                    "WHY solve={solve} FROM c{x} callee={kn}@n{n} values={} [{}]",
                    vals.len(),
                    vs.join(";")
                );
                for &v in vals.iter().take(16) {
                    let mut h = Vec::new();
                    self.why_walk(solve, n, v, &mut h, &mut walked);
                    for hp in &h {
                        eprintln!("WHY solve={solve} FROM c{x} {hp}");
                    }
                }
            }
        }
    }

    /// Walk the parents of `v` at `n` back to a parentless arrival. A hop
    /// that keeps the value goes on with it at the source; one that changes
    /// it (a constraint's output, a builtin model's new token) goes on with
    /// the source's latest first arrival at or before it (the delta being
    /// applied then).
    fn why_walk(
        &self,
        solve: u32,
        n: NodeId,
        v: WVal,
        hops: &mut Vec<WhyHop>,
        seen: &mut crate::pts::FxHashSet<(NodeId, WVal)>,
    ) {
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
            let recorded = (p.cause != 0)
                .then(|| WVal::Tok(p.cause - 1))
                .filter(|c| w.first.contains_key(&(p.src, *c)));
            let mut heur = false;
            let next = if let Some(c) = recorded {
                Some(c)
            } else if same {
                Some(v)
            } else {
                heur = true;
                w.by_node.get(&p.src).and_then(|l| {
                    let i = l.partition_point(|&(wk, _)| wk <= p.work);
                    l[..i].last().map(|&(_, x)| x)
                })
            };
            let Some(nv) = next else {
                hops.push(self.why_hop(solve, d, n, v, Some(&p), "seed"));
                return;
            };
            let mut h = self.why_hop(solve, d, n, v, Some(&p), "");
            if heur {
                h.via.push_str("|heur");
            }
            hops.push(h);
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
    pub(crate) fn why_hold_r(
        &mut self,
        kind: &'static str,
        call: u32,
        t: Option<Tok>,
        reason: u32,
    ) {
        let start = self.prov_rule.map(|(n, _)| n).unwrap_or(self.cur_src);
        let (rule0, work) = (self.prov_rule, self.work.get());
        if self
            .prov
            .as_ref()
            .and_then(|p| p.why.as_ref())
            .is_some_and(|w| w.stop == STOP_REALM_ALL)
        {
            return self.why_hold_src(kind, call, t, reason, start, work);
        }
        let Some(w) = self.prov.as_mut().and_then(|p| p.why.as_mut()) else {
            return;
        };
        if w.held {
            return;
        }
        match w.stop {
            // The realm flip that follows (`why_realm`) prints this cause;
            // its own `hold_realm` / `interp` record never replaces one.
            STOP_REALM => {
                if w.pending.is_none() || !matches!(kind, "hold_realm" | "interp") {
                    w.pending = Some(Pending {
                        kind,
                        call,
                        t,
                        reason,
                        start,
                        rule: rule0,
                        work,
                    });
                }
                return;
            }
            STOP_WIDE => return,
            _ => {}
        }
        w.held = true;
        let (solve, stop) = (w.solve, w.stop == STOP_HOLD);
        let vals = self.why_vals(start, t);
        let rule = rule0.map_or("-".to_string(), |(rn, c)| {
            format!("{}@n{rn}", c.kind_name())
        });
        let f = crate::unksrc::call_fn(self, call);
        let fname = self
            .funcs
            .get(f as usize)
            .map_or("-".to_string(), |fi| fi.name.clone());
        let header = format!(
            "WHY solve={solve} HOLD kind={kind} site={} fn={fname} start=n{start} rule={rule} work={work} values={} reason={}",
            self.why_site(call),
            vals.len(),
            self.reasons.get(reason as usize).map_or("-", |s| s.as_str()).replace(char::is_whitespace, "_")
        );
        self.why_emit(header, start, vals, stop);
    }

    /// `STOP=realm-all`: one line for this hold source; the solve goes on.
    fn why_hold_src(
        &self,
        kind: &'static str,
        call: u32,
        t: Option<Tok>,
        reason: u32,
        start: NodeId,
        work: u64,
    ) {
        let solve = self
            .prov
            .as_ref()
            .and_then(|p| p.why.as_ref())
            .map_or(0, |w| w.solve);
        let f = crate::unksrc::call_fn(self, call);
        let fname = self
            .funcs
            .get(f as usize)
            .map_or("-".to_string(), |fi| fi.name.clone());
        let tok = t.map_or("-".to_string(), |t| self.why_val(WVal::Tok(t)));
        eprintln!(
            "WHY solve={solve} HOLDSRC kind={kind} site={} fn={fname} start=n{start} tok={tok} work={work} reason={}",
            self.why_site(call),
            self.reasons.get(reason as usize).map_or("-", |s| s.as_str()).replace(char::is_whitespace, "_")
        );
    }

    /// The values a hold's walk starts with at `start`: the value given
    /// (the callee token; an escaping token) and ⊤ when the node holds it
    /// beside it; with none given, ⊤ when the node holds it, else every
    /// token it holds.
    fn why_vals(&self, start: NodeId, t: Option<Tok>) -> Vec<WVal> {
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
        vals
    }

    /// `STOP=realm`: the realm becomes held now (`hold_realm`,
    /// `builtin_fields_interp`). Print the chain from the hold that caused
    /// it (the last one recorded before; else the current constraint's
    /// node) and exit 86.
    pub(crate) fn why_realm(&mut self, flip: &'static str) {
        let here = self.prov_rule.map(|(n, _)| n).unwrap_or(self.cur_src);
        let (rule0, work) = (self.prov_rule, self.work.get());
        let Some(w) = self.prov.as_mut().and_then(|p| p.why.as_mut()) else {
            return;
        };
        if w.stop == STOP_REALM_ALL && !w.realm_done {
            w.realm_done = true;
            eprintln!("WHY solve={} REALM-ALL flip={flip} work={work}", w.solve);
            return;
        }
        if w.stop != STOP_REALM || w.realm_done {
            return;
        }
        w.realm_done = true;
        let solve = w.solve;
        let p = w.pending.take().unwrap_or(Pending {
            kind: flip,
            call: u32::MAX,
            t: None,
            reason: u32::MAX,
            start: here,
            rule: rule0,
            work,
        });
        let vals = self.why_vals(p.start, p.t);
        let rule = p.rule.map_or("-".to_string(), |(rn, c)| {
            format!("{}@n{rn}", c.kind_name())
        });
        let f = crate::unksrc::call_fn(self, p.call);
        let fname = self
            .funcs
            .get(f as usize)
            .map_or("-".to_string(), |fi| fi.name.clone());
        let tok = p.t.map_or("-".to_string(), |t| self.why_val(WVal::Tok(t)));
        let header = format!(
            "WHY solve={solve} REALM flip={flip} work={work} cause={} cause_work={} site={} fn={fname} start=n{} rule={rule} tok={tok} values={} reason={}",
            p.kind,
            p.work,
            self.why_site(p.call),
            p.start,
            vals.len(),
            self.reasons.get(p.reason as usize).map_or("-", |s| s.as_str()).replace(char::is_whitespace, "_")
        );
        self.why_emit(header, p.start, vals, true);
    }

    /// Print `header`, the chain of the first value and a count line for
    /// the rest; keep the chain for `take_chain`; exit 86 when `stop`
    /// (never in unit tests: they read the record).
    fn why_emit(&mut self, header: String, start: NodeId, vals: Vec<WVal>, stop: bool) {
        let solve = self
            .prov
            .as_ref()
            .and_then(|p| p.why.as_ref())
            .map_or(0, |w| w.solve);
        eprintln!("{header}");
        let mut hops = Vec::new();
        let mut seen = crate::pts::FxHashSet::default();
        let (mut more, mut more_hops) = (0usize, 0usize);
        if start != NO_NODE {
            for (i, v) in vals.into_iter().enumerate() {
                if i == 0 {
                    self.why_walk(solve, start, v, &mut hops, &mut seen);
                } else {
                    let mut h = Vec::new();
                    self.why_walk(solve, start, v, &mut h, &mut seen);
                    more += 1;
                    more_hops += h.len();
                }
            }
        }
        for h in &hops {
            eprintln!("{h}");
        }
        self.why_calls(solve, &hops);
        if more > 0 {
            eprintln!("WHY solve={solve} MORE values={more} hops={more_hops} (chains of the other values not printed)");
        }
        let (recs, nodes) = self
            .prov
            .as_ref()
            .and_then(|p| p.why.as_ref())
            .map_or((0, 0), |w| (w.first.len(), w.by_node.len()));
        eprintln!(
            "WHY solve={solve} END hops={} records={recs} nodes={nodes}",
            hops.len()
        );
        *LAST.lock().unwrap_or_else(|e| e.into_inner()) = hops;
        if stop && !cfg!(test) {
            use std::io::Write as _;
            let _ = std::io::stderr().flush();
            std::process::exit(86);
        }
    }
}
