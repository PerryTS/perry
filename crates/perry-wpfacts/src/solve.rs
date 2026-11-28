//! The solver: inclusion constraints with difference propagation over a
//! worklist. Copy edges are plain successor lists; every other rule is a
//! complex constraint (`Cons`) re-applied to each node's new tokens.
//!
//! Soundness rests on two things: every rule over-approximates the concrete
//! transfer function of its construct, and every unknown value or operation
//! goes to ⊤ (`TOP` token for values coming from unmodeled code, `TopSink`
//! for values leaving into it). The rules here are the executable form of
//! design section 3.2; `scripts/wpfacts_recompute.py` re-implements them.

mod accessor_lookup;
mod copy;
mod define;
mod lookup;
pub(crate) mod map_vals;
mod not_error;
mod own_read;
mod range_inputs;
mod record_entry;
pub(crate) use range_inputs::{grow_crange, grow_range, grow_rule_range};

use crate::builtins;

mod call_and_construct;
mod native_value;
mod reflect_construct;
mod shared;
use crate::delta::Tokens;
use crate::model::*;

#[path = "solve/apply.rs"]
mod apply;
mod arr_len;
mod builtin_fields;
pub(crate) mod proxy;

#[path = "solve/store.rs"]
mod store;

mod escape;
mod fields;
mod json_pos;
pub(crate) mod json_rt;
mod shaped_keys;
pub(crate) mod strs;
mod virt;

// Property-key ranges come from value deltas, not source annotations. Keep
// computed reads and calls on the same partition of ToPropertyKey's inputs.
/// An escaped value that code no contract describes holds (it may write
/// any key of it, `Model::is_escaped_writer`) and that is no parameter or
/// argument marker: beside ⊤ it is that ⊤, since every rule does to ⊤ at
/// least what it does to the value. A value handed only by contract is not:
/// a program store through ⊤ beside it must still write it. The one predicate of the arrival rule
/// (`add_tokens_s`, with the markers that read as ⊤, DECISIONS 42), of ⊤'s
/// arrival at a node that lists the value, and of a ⊤ node's pop.
pub(crate) fn escaped_beside_top<V>(
    esc_bits: &[u64],
    param_marker: &crate::pts::FxHashMap<Tok, V>,
    arg_bits: &[u64],
    t: Tok,
) -> bool {
    t != TOP
        && marker_top_bit(esc_bits, t)
        && !(param_marker.contains_key(&t) || marker_top_bit(arg_bits, t))
}

fn index_needs_general_key(toks: Tokens<'_>, prim: u16) -> bool {
    prim & (prim::STR | prim::SYMBOL) != 0 || !toks.is_empty()
}

fn index_has_numeric_key(prim: u16) -> bool {
    prim & (prim::NUM | prim::BIGINT) != 0
}

/// How many times the work bound a widened solve may use before it stops.
const WIDENED_WORK: u64 = 1000;

mod fixpoint;
mod implicit;
pub(crate) use implicit::implsrc_on;
mod lines;
mod markers;
mod profile;

impl Model {
    // ---------------------------------------------------------------- nodes

    /// Add tokens/prims to a node; schedules it when anything is new.
    #[track_caller]
    pub fn add(&mut self, n: NodeId, toks: &[Tok], prim: u16) {
        #[cfg(test)]
        if self.packet_emit(crate::summary::packet::Emission::Seed(
            n,
            toks.to_vec(),
            prim,
        )) {
            return;
        }
        self.add_tokens(n, Tokens::Slice(toks), prim);
    }

    /// `add` with the string constants `strs` for a string in `prim` (a
    /// copy of another node's value: `Node::strs`).
    ///
    /// `#[track_caller]` and always inlined: a seed through it is booked to
    /// its caller, and the location is a constant at each call site (no
    /// extra argument at run time).
    #[track_caller]
    #[inline(always)]
    pub(crate) fn add_s(&mut self, n: NodeId, toks: &[Tok], prim: u16, strs: StrSet) {
        #[cfg(test)]
        if self.packet_emit(crate::summary::packet::Emission::Seed(
            n,
            toks.to_vec(),
            prim,
        )) {
            return;
        }
        self.add_tokens_s(n, Tokens::Slice(toks), prim, strs);
    }
    /// The work bound (`FactsConfig::max_work`, 0 = none). Past it a
    /// whole-program solve (`Model::widen`) is widened (`bind_layout`: no
    /// new calling contexts) and goes on to its fixpoint; past `WIDENED_WORK` times it, or past it
    /// at all per unit, the solve stops and claims nothing.
    pub(crate) fn past_work_bound(&mut self) -> bool {
        if self.max_work == 0 || self.work.get() <= self.max_work {
            return false;
        }
        self.widened = self.widen;
        if !self.widen {
            self.aborted = true;
        } else if self.work.get() > self.max_work.saturating_mul(WIDENED_WORK) {
            self.aborted = true;
            self.widened_stop = true;
        }
        self.aborted
    }

    /// A string that is no copy of another node's is any string
    /// (`Node::strs`).
    #[track_caller]
    fn add_tokens(&mut self, n: NodeId, toks: Tokens<'_>, prim: u16) {
        self.add_tokens_s(n, toks, prim, STRS_TOP);
    }

    #[track_caller]
    fn add_tokens_s(&mut self, n: NodeId, toks: Tokens<'_>, prim: u16, strs: StrSet) {
        if self.ft_check.is_some() {
            self.filter_check(n, toks);
        }
        if self.relations.is_some() {
            self.rel_generic_fn_arrival(n, &toks);
        }
        if let Some(c) = self.diagnostic.as_mut().and_then(|d| d.census.as_mut()) {
            if !c.injecting
                && self.cur_src != NO_NODE
                && self.cur_src != n
                && toks.iter().any(|&t| t == TOP)
            {
                c.top_edges.insert((self.cur_src, n));
            }
        }
        let mut later: Vec<Tok> = Vec::new();
        let mut ledger_gone: Vec<Tok> = Vec::new();
        // A marker crossing into the node arrives as a member of the node's
        // own marker (MRK, `Model::crosses`; DECISIONS 44 for parameters):
        // the node no longer holds every value of its sources.
        let crossed: Option<Vec<Tok>> =
            match !self.param_marker.is_empty() && toks.iter().any(|&t| self.crosses(n, t)) {
                true => {
                    let r = self.arg_marker(n);
                    let mut v = Vec::with_capacity(toks.len());
                    for &t in toks {
                        if self.crosses(n, t) {
                            self.link_marker(t, r);
                            v.push(r);
                        } else {
                            v.push(t);
                        }
                    }
                    self.nodes[n as usize].dropped = true;
                    Some(v)
                }
                false => None,
            };
        let boundary = self.nodes[n as usize].succ.len() as u32;
        // Delta tokens are distinct; a slice may repeat one.
        let distinct =
            crossed.is_none() && matches!(toks, Tokens::Pending(_) | Tokens::Delivery(..));
        let mut fresh = std::mem::take(&mut self.fresh);
        let mut seen = std::mem::take(&mut self.fresh_seen);
        fresh.clear();
        seen.clear();
        // Is `t` already among the new members?
        let is_fresh = |fresh: &[Tok], seen: &mut crate::pts::FxHashSet<Tok>, t: Tok| {
            if distinct {
                return false;
            }
            if fresh.len() <= 16 {
                return fresh.contains(&t);
            }
            if seen.len() < fresh.len() {
                seen.extend(fresh[seen.len()..].iter().copied());
            }
            seen.contains(&t)
        };
        let node = &mut self.nodes[n as usize];
        let mut changed = false;
        let mut top_new = false;
        let toks = match &crossed {
            Some(v) => Tokens::Slice(v),
            None => toks,
        };
        for &t in toks {
            // A marker that reads as ⊤ (DECISIONS 42) is decided after the
            // other tokens, once it is known whether ⊤ arrived with them. So
            // is an escaped value (`Model::top_beside`): unknown code holds
            // it, so beside ⊤ it is that ⊤.
            if marker_top_bit(&self.marker_top, t)
                || escaped_beside_top(&self.esc_writer_bits, &self.param_marker, &self.arg_bits, t)
            {
                later.push(t);
                continue;
            }
            if !node.pts.contains_tok(t) && !is_fresh(&fresh, &mut seen, t) {
                fresh.push(t);
                self.pending.push_to(&mut node.delta, t, boundary);
                changed = true;
                self.pts_entries += 1;
                top_new |= t == TOP;
            }
        }
        // Beside ⊤ it is that ⊤: every rule does to ⊤ at least what it does
        // to the marker, and the readers after the solve read it as ⊤. Left
        // out, it never floods the unit. Without ⊤ it is delivered as before.
        if !later.is_empty() {
            if top_new || node.pts.contains_tok(TOP) {
                node.dropped = true;
                if self.ledger.is_some() || self.k3_held.is_some() {
                    ledger_gone = std::mem::take(&mut later);
                }
            } else {
                for t in later {
                    if !node.pts.contains_tok(t) && !is_fresh(&fresh, &mut seen, t) {
                        fresh.push(t);
                        self.pending.push_to(&mut node.delta, t, boundary);
                        changed = true;
                        self.pts_entries += 1;
                    }
                }
            }
        }
        // One step from the old set to the new one: a shared set is never
        // copied in place (`SetTable::extend`).
        if !fresh.is_empty() {
            let old = std::mem::take(&mut node.pts);
            node.pts = self.sets.extend(old, &fresh);
        }
        // ⊤ arrived: a member listed before it and escaped since is that ⊤
        // now, and leaves the set (rebuilt) and the node's delta.
        if top_new {
            if self.ledger.is_some() || self.k3_held.is_some() {
                let gone: Vec<Tok> = self.nodes[n as usize]
                    .pts
                    .to_sorted()
                    .into_iter()
                    .filter(|&t| {
                        escaped_beside_top(&self.esc_writer_bits, &self.param_marker, &self.arg_bits, t)
                    })
                    .collect();
                self.ledger_again_beside_top(n, &gone);
            }
            let node = &mut self.nodes[n as usize];
            let (esc, pm, ab) = (&self.esc_writer_bits, &self.param_marker, &self.arg_bits);
            let old = std::mem::take(&mut node.pts);
            let before = old.len();
            node.pts = self
                .sets
                .retain(old, |t| !escaped_beside_top(esc, pm, ab, t));
            self.pending
                .retain(&mut node.delta, |t| !escaped_beside_top(esc, pm, ab, t));
            if node.pts.len() != before {
                node.dropped = true;
                self.charge_work(before as u64);
            }
        }
        if !ledger_gone.is_empty() && (self.ledger.is_some() || self.k3_held.is_some()) {
            self.ledger_again_beside_top(n, &ledger_gone);
        }
        if self.prov.is_some() && !fresh.is_empty() {
            let at = std::panic::Location::caller();
            self.prov_arrivals(n, &mut fresh.iter().copied(), false, at);
        }
        self.fresh = fresh;
        self.fresh_seen = seen;
        if top_new && self.diagnostic.is_some() {
            self.diagnostic_predecessor(n, self.cur_src, self.cur_why);
        }
        if top_new && self.explain {
            self.top_pred
                .entry(n)
                .or_insert((self.cur_src, self.cur_why));
            if let Some(l) = self.ledger.as_mut() {
                l.top_seed(n, self.cur_why, std::panic::Location::caller());
            }
        }
        if top_new && self.stop_top.is_some() {
            self.top_created(n, std::panic::Location::caller());
        }
        if self.pts_entries & 0xfffff == 0 && self.pts_entries != 0 {
            if self
                .budget
                .is_some_and(|(start, limit)| start.elapsed() > limit)
                || (self.max_pts != 0 && self.pts_entries > self.max_pts)
            {
                self.aborted = true;
            }
        }
        if self.prov.is_some() {
            let newp = prim & !self.nodes[n as usize].prim;
            if newp != 0 {
                self.why_prim(n, newp, std::panic::Location::caller());
            }
        }
        let iv = self.cur_range.unwrap_or(crate::model::Rg::TOP);
        let node = &mut self.nodes[n as usize];
        let newp = prim & !node.prim;
        if newp != 0 {
            node.prim |= newp;
            node.dprim |= newp;
            changed = true;
        }
        // A Number brings its interval: the delivering edge's, else ⊤. A
        // rule's result takes its interval from the rule only.
        if prim & prim::NUM != 0 && !node.ruled {
            changed |= grow_range(node, iv.num);
            if iv.frac && !node.frac {
                node.frac = true;
                node.drange = true;
                changed = true;
            }
        }
        // A String brings its code interval the same way.
        if prim & prim::STR != 0 && !node.cruled {
            changed |= grow_crange(node, iv.chr);
        }
        if prim & prim::STR != 0 {
            let strs = if strs == STRS_NONE { STRS_TOP } else { strs };
            if self.prov.is_some() {
                let old = self.nodes[n as usize].strs;
                if self.join_strs(n, strs) {
                    changed = true;
                    self.why_strs(n, old, std::panic::Location::caller());
                }
            } else {
                changed |= self.join_strs(n, strs);
            }
        }
        let node = &mut self.nodes[n as usize];
        if changed && !node.in_wl {
            node.in_wl = true;
            self.wl.push_back(n);
        }
    }

    /// Whether operation `c` on a ⊤ operand may run unknown code (DESIGN
    /// SEEDE section 8): every kind but tests, escapes and call plumbing.
    fn may_run_code(c: &Cons) -> bool {
        !matches!(
            c,
            Cons::Raise { .. }
                | Cons::HandRaise { .. }
                | Cons::Identity { .. }
                | Cons::Guard { .. }
                | Cons::Defined { .. }
                | Cons::NotNullish { .. }
                | Cons::FieldTruthy { .. }
                | Cons::Fulfil { .. }
                | Cons::Falsy { .. }
                | Cons::NotBuffer { .. }
                | Cons::NotObjectProto { .. }
                | Cons::NotError { .. }
                | Cons::NotRegExp { .. }
                | Cons::ProtoObjects { .. }
                | Cons::NotNull { .. }
                | Cons::RecordEntry { .. }
                | Cons::ArrayRecordEntry { .. }
                | Cons::TypeIs { .. }
                | Cons::Objects { .. }
                | Cons::NotUndefined { .. }
                | Cons::TopSink { .. }
                | Cons::RuntimeStore { .. }
                | Cons::BirthField { .. }
                | Cons::ThisTo { .. }
                | Cons::CallArgs { .. }
                | Cons::RetInto { .. }
                | Cons::ArgInto { .. }
                | Cons::ThisInst { .. }
                | Cons::NativeThis { .. }
                | Cons::HolderStep { .. }
                | Cons::VisitHolders { .. }
                | Cons::VisitStop { .. }
        )
    }

    /// The throw region of function `f`'s body (DESIGN SEEDE section 8),
    /// made on first use. Per unit any function may be called by another
    /// unit's code, whose `catch` reads the shared cell: its region feeds
    /// `thrown` (the cell the unit boundary publishes).
    pub fn raise_of(&mut self, f: FnIdx) -> NodeId {
        let r = self.funcs[f as usize].raise;
        if r != u32::MAX {
            return r;
        }
        let r = self.node_l("RAISE");
        self.funcs[f as usize].raise = r;
        if self.unit_mode {
            let t = self.thrown;
            self.edge(r, t);
        }
        r
    }

    /// Unknown code may run in `region` (a ⊤ or opaque callee, an operation
    /// on ⊤, a native handing a value to unknown code): it may throw anything
    /// it holds there.
    #[track_caller]
    pub fn raise_top(&mut self, region: NodeId, from: NodeId, why: &'static str) {
        if region == u32::MAX || self.nodes[region as usize].pts.contains_tok(TOP) {
            return;
        }
        let saved = (self.cur_src, self.cur_why);
        self.cur_src = from;
        self.cur_why = self.reason(why);
        self.add_tokens(region, Tokens::Slice(&[TOP]), prim::ALL);
        (self.cur_src, self.cur_why) = saved;
    }

    /// Seed a token (an input fact outside the solver rules: logged).
    #[track_caller]
    /// `hold_ledger` (LEDG3): escaped tokens `toks` leave `n` beside its ⊤
    /// (`escaped_beside_top`), so `n`'s TopSink rules never see them. Each
    /// such rule would escape them again (`apply`): the predecessor graph
    /// records that, so it does not depend on whether ⊤ or the token reached
    /// `n` first.
    fn ledger_again_beside_top(&mut self, n: NodeId, toks: &[Tok]) {
        if let Some(h) = self.k3_held.as_mut() {
            let fs = toks.iter().copied().filter(|&t| matches!(self.toks[t as usize], TokKind::Func(..)));
            h.entry(n).or_default().extend(fs);
        }
        let sinks: Vec<(u32, Option<Tok>)> = self.nodes[n as usize]
            .cons
            .iter()
            .filter_map(|c| match *c {
                Cons::TopSink { reason } => Some((reason, None)),
                Cons::RuntimeStore { reason, retained } => Some((reason, Some(retained))),
                _ => None,
            })
            .collect();
        let Some(l) = self.ledger.as_mut() else {
            return;
        };
        for (reason, retained) in sinks {
            let root = l.enter_sink(n);
            for &t in toks {
                if retained != Some(t)
                    && escaped_beside_top(&self.esc_writer_bits, &self.param_marker, &self.arg_bits, t)
                {
                    l.again(t, reason, std::panic::Location::caller());
                }
            }
            l.leave(root);
        }
    }

    #[track_caller]
    pub fn add_tok(&mut self, n: NodeId, t: Tok) {
        self.add_tok_arm(n, t, "")
    }

    /// `add_tok` naming the code arm: under `PERRY_FACTS_RELATIONS`, a
    /// solver-phase add (which writes no `addr` relation) writes the
    /// provenance line `A addtok <node> <tok> <arm>@<file>:<line>` (the
    /// caller's location; diagnostic only).
    #[track_caller]
    pub fn add_tok_arm(&mut self, n: NodeId, t: Tok, arm: &'static str) {
        if t == TOP {
            if let Some(s) = self.stop_top.as_ref() {
                if !s.quiet_top && s.cur_pos() == crate::topstop::NOPOS {
                    self.note_nopos(std::panic::Location::caller(), "top");
                }
            }
        }
        if t == TOP
            && self.diagnostic.as_ref().is_some_and(|d| d.census.is_some())
            && !self
                .nodes
                .get(self.cur_src as usize)
                .is_some_and(|v| v.pts.contains_tok(TOP))
        {
            if let Some(d) = &mut self.diagnostic {
                if let Some(c) = &mut d.census {
                    if !c.injecting {
                        let at = std::panic::Location::caller();
                        let gate = c.node_gate(n);
                        c.inject_at = Some((n, gate));
                        c.injections.insert((
                            n,
                            at.file().into(),
                            at.line(),
                            "direct TOP token".into(),
                            d.cause_node.unwrap_or(NO_NODE),
                            d.cur_call.unwrap_or(u32::MAX),
                            d.escaping.last().copied().unwrap_or(u32::MAX),
                            self.cur_fn,
                            gate,
                        ));
                        if c.suppress.as_deref()
                            == Some(format!("{}:{}:{n}", at.file(), at.line()).as_str())
                        {
                            c.suppressed += 1;
                            return;
                        }
                    }
                }
            }
        }
        if self.rel_phase != b'S' {
            self.rel(|| format!("addr {n} {t}"));
        } else if let Some(r) = self.relations.as_mut() {
            let at = std::panic::Location::caller();
            let f = at.file();
            let f = f.rsplit_once("src/").map_or(f, |(_, x)| x);
            r.push(format!("A addtok {n} {t} {arm}@{f}:{}", at.line()));
        }
        self.add(n, &[t], 0);
    }

    /// `#[track_caller]` and always inlined (see `add_s`); the relation
    /// line is out of line, so each call site stays a test and a call.
    #[track_caller]
    #[inline(always)]
    pub fn add_prim(&mut self, n: NodeId, p: u16) {
        if self.rel_phase != b'S' {
            self.rel_prim(n, p);
        }
        self.add(n, &[], p);
    }

    #[inline(never)]
    fn rel_prim(&mut self, n: NodeId, p: u16) {
        self.rel(|| format!("prim {n} {p}"));
    }

    /// Seed ⊤ into `n` as an ORIGIN (not a propagation): `why` names the
    /// unmodeled thing; only interned in explain mode (diagnostics).
    #[track_caller]
    pub fn add_top(&mut self, n: NodeId, why: impl FnOnce(&Self) -> String) {
        if self.stop_top.is_some() && !self.explain && self.diagnostic.is_none() {
            // `add_top_inner` without diagnostics, the origin's name kept
            // for `top_created`.
            let w = why(self);
            if self.stop_top.as_ref().unwrap().cur_pos() == crate::topstop::NOPOS {
                self.note_nopos(std::panic::Location::caller(), "top");
            }
            let s = self.stop_top.as_mut().unwrap();
            let old = s.why.replace(w);
            let q = std::mem::replace(&mut s.quiet_top, true);
            self.add_tok(n, TOP);
            let s = self.stop_top.as_mut().unwrap();
            s.why = old;
            s.quiet_top = q;
            return;
        }
        if self.diagnostic.as_ref().is_some_and(|d| d.census.is_some()) {
            let w = why(self);
            let at = std::panic::Location::caller();
            let d = self.diagnostic.as_mut().unwrap();
            let c = d.census.as_mut().unwrap();
            let gate = c.node_gate(n);
            c.inject_at = Some((n, gate));
            c.injections.insert((
                n,
                at.file().into(),
                at.line(),
                w.clone(),
                d.cause_node.unwrap_or(NO_NODE),
                d.cur_call.unwrap_or(u32::MAX),
                d.escaping.last().copied().unwrap_or(u32::MAX),
                self.cur_fn,
                gate,
            ));
            let key = format!("{}:{}:{n}", at.file(), at.line());
            if c.suppress.as_deref() == Some(key.as_str()) {
                c.suppressed += 1;
                return;
            }
            let saved = std::mem::replace(&mut c.injecting, true);
            self.add_top_inner(n, |_| w);
            self.diagnostic
                .as_mut()
                .unwrap()
                .census
                .as_mut()
                .unwrap()
                .injecting = saved;
            return;
        }
        self.add_top_inner(n, why);
    }
    fn add_top_inner(&mut self, n: NodeId, why: impl FnOnce(&Self) -> String) {
        if self.explain || self.diagnostic.is_some() {
            let w = why(self);
            if !self.nodes[n as usize].pts.contains_tok(TOP) {
                if let Some(d) = &mut self.diagnostic {
                    d.seeds.entry(n).or_insert(w.clone());
                    if let Some(&b) = d
                        .seed_holder
                        .last()
                        .or(d.seed_by.last())
                        .or(d.escaping.last())
                    {
                        d.top_by.entry(n).or_insert(b);
                    }
                }
            }
            if self.explain {
                let r = self.reason(&w);
                let saved = (self.cur_src, self.cur_why);
                self.cur_src = NO_NODE;
                self.cur_why = r;
                self.add_tok(n, TOP);
                (self.cur_src, self.cur_why) = saved;
            } else {
                self.add_tok(n, TOP);
            }
        } else {
            self.add_tok(n, TOP);
        }
    }

    /// The shared ⊤ holder (`top_tok_node`); under `PERRY_FACTS_STOP_ON_TOP`
    /// a named one (`top_val`), so the creation is reported with its origin.
    #[track_caller]
    pub fn top_src(&mut self, why: &str) -> NodeId {
        if self.stop_top.is_some() {
            return self.top_val(why);
        }
        self.top_tok_node
    }

    /// A node holding only ⊤ (all kinds) for one named unmodeled origin.
    #[track_caller]
    pub fn top_val(&mut self, why: &str) -> NodeId {
        if crate::unksrc::on() {
            crate::unksrc::note("topval", why, u32::MAX, self.cur_fn as u32);
        }
        // Explain and the dump name each origin: its own node.
        if let Some(s) = self.stop_top.as_ref() {
            // One holder per creation point, named (`topstop`).
            let p = s.cur_pos();
            let n = self.node_l("TOPVAL");
            let at = std::panic::Location::caller();
            if p == crate::topstop::NOPOS {
                self.note_nopos(at, "topval");
            }
            let s = self.stop_top.as_mut().unwrap();
            s.topvals.insert(n, (why.to_string(), at, p));
            let q = std::mem::replace(&mut s.quiet_top, true);
            self.add_tok(n, TOP);
            self.stop_top.as_mut().unwrap().quiet_top = q;
            self.add_prim(n, prim::ALL);
            return n;
        }
        if !self.explain && self.diagnostic.is_none() {
            return self.top_tok_node;
        }
        if let Some(&n) = self.top_vals.get(why) {
            return n;
        }
        let n = self.node_l("TOPVAL");
        #[cfg(test)]
        {
            self.nodes[n as usize].intrinsic_top = Some(why.to_owned());
        }
        self.add_top(n, |_| why.to_string());
        self.add_prim(n, prim::ALL);
        self.top_vals.insert(why.to_string(), n);
        n
    }

    /// Run `f` as a builtin summary: its relations are axioms for the
    /// independent checker.
    pub fn with_b<R>(&mut self, f: impl FnOnce(&mut Self) -> R) -> R {
        let p = self.rel_phase;
        self.rel_phase = b'B';
        let r = f(self);
        self.rel_phase = p;
        r
    }

    fn with_s<R>(&mut self, f: impl FnOnce(&mut Self) -> R) -> R {
        let p = self.rel_phase;
        self.rel_phase = b'S';
        let r = f(self);
        self.rel_phase = p;
        r
    }

    /// Attach a complex constraint and apply it to what the node holds now.
    /// Inside a clause of a switch on a parameter the rules it adds to `n`
    /// record the clause's reach mask (rule M1, `Model::cons_reach`).
    #[track_caller]
    pub fn cons(&mut self, n: NodeId, c: Cons) {
        let tag = match self.clause {
            Some((f, m)) if self.cur_fn == f => Some(m),
            _ => None,
        };
        let len0 = self.nodes[n as usize].cons.len();
        self.cons_untagged(n, c);
        if let Some(m) = tag {
            for i in len0..self.nodes[n as usize].cons.len() {
                self.cons_reach.insert((n, i as u32), m);
            }
        }
    }

    #[track_caller]
    fn cons_untagged(&mut self, n: NodeId, c: Cons) {
        let defer = std::mem::take(&mut self.chain_defer);
        #[cfg(test)]
        if self.packet_emit(crate::summary::packet::Emission::Rule(n, c)) {
            return;
        }
        self.stats_cons += 1;
        if let Cons::FieldTruthy { dst, .. } = c {
            if self.ft_check.is_none() && std::env::var_os("PERRY_WPF_FILTER_CHECK").is_some() {
                self.ft_check = Some(Default::default());
            }
            if let Some(m) = self.ft_check.as_mut() {
                m.entry(dst).or_default().push(n);
            }
        }
        if matches!(c, Cons::IdxOwnLoad { .. }) {
            self.own_read_census[0] += 1;
        }
        if self.diagnostic.is_some() {
            if let Cons::TopSink { reason } | Cons::RuntimeStore { reason, .. } = c {
                self.diagnostic_sink(n, reason);
            }
        }
        // `hold_ledger`: what the rule later escapes belongs to the cascade
        // that attached it.
        if let (Cons::TopSink { .. } | Cons::RuntimeStore { .. }, Some(l)) =
            (c, self.ledger.as_mut())
        {
            l.sink_rule(n);
        }
        if self.relations.is_some()
            && !matches!(
                c,
                // No flow: the recomputation has nothing to check.
                Cons::Identity { .. }
                    | Cons::HasProp { .. }
                    | Cons::OwnProp { .. }
                    | Cons::DeleteIdx { .. }
                    | Cons::Meta { .. }
                    | Cons::ThisTo { .. }
                    | Cons::CallArgs { .. }
                    | Cons::RetInto { .. }
                    | Cons::ArgInto { .. }
                    | Cons::ThisInst { .. }
                    | Cons::NativeThis { .. }
                    | Cons::BuiltinCallee
                    | Cons::ProxyCallee { .. }
                    | Cons::HolderStep { .. }
                    | Cons::VisitHolders { .. }
                    | Cons::VisitStop { .. }
            )
        {
            let line = self.cons_line(n, &c);
            self.rel(|| line);
        }
        if let Cons::Range { dst, op, .. } = c {
            // The rule's result is every Number (String, for a code rule)
            // the node can hold: what reached it before (an instance's
            // seeds) is subsumed.
            let d = &mut self.nodes[dst as usize];
            if matches!(op, RangeOp::SClip(_)) {
                if !d.cruled {
                    d.cruled = true;
                    d.crange = Iv::EMPTY;
                    d.csteps = 0;
                }
            } else if !d.ruled {
                d.ruled = true;
                d.range = Iv::EMPTY;
                d.rsteps = 0;
            }
        }
        if let Cons::HandRaise { region } = c {
            *self.hand_raise.entry((n, region)).or_default() += 1;
        }
        self.nodes[n as usize].cons.push(c);
        let ci = self.nodes[n as usize].cons.len() as u32 - 1;
        if let Some(s) = self.stop_top.as_mut() {
            let p = s.cur_pos();
            s.cons_pos.insert((n, ci), p);
            if p == crate::topstop::NOPOS {
                let at = std::panic::Location::caller();
                s.cons_at.insert((n, ci), at);
                self.note_nopos(at, "cons");
            }
        }
        if let Some(region) = self.gen_region.filter(|_| Self::may_run_code(&c)) {
            let cs = &self.nodes[n as usize].cons;
            let seen = cs
                .iter()
                .rev()
                .take(4)
                .any(|x| matches!(x, Cons::Raise { region: r } if *r == region));
            if !seen {
                self.cons(n, Cons::Raise { region });
            }
        }
        if !self.cell_watch.is_empty() && self.cell_watch.remove(&n) {
            self.cell_woken.push(n);
        }
        if self.aborted {
            return;
        }
        if self.foreign_rule(n) {
            let f = self.cur_fn;
            let a = self.act.as_mut().unwrap();
            a.foreign.entry(f).or_default().push(crate::model::Foreign::Cons(n, ci, c, false));
            return;
        }
        self.own_refuted_cons(n, c);
        if defer {
            // SKINROOT27: replayed by the draining EnumChain (`Model::chain_q`).
            self.chain_q.push((n, c, ci));
            return;
        }
        self.replay_cons(n, c, ci);
    }

    /// Apply the attached constraint `c` of `n` to what `n` holds now.
    pub(crate) fn replay_cons(&mut self, n: NodeId, c: Cons, ci: u32) {
        if matches!(c, Cons::Detached) || self.inert_node(n) {
            return;
        }
        let prim = self.nodes[n as usize].prim;
        if self.nodes[n as usize].pts.is_empty() && prim == 0 {
            return;
        }
        let mut toks = self.bufs.pop().unwrap_or_default();
        toks.clear();
        self.nodes[n as usize].pts.sorted_into(&mut toks);
        // The replay is solve work: the bound covers it.
        self.charge_work(toks.len() as u64 + 1);
        if self.past_work_bound() {
            self.bufs.push(toks);
            return;
        }
        if let Some(p) = self.prof.as_mut() {
            let e = p.kind.entry(c.kind_name()).or_default();
            e.2 += 1;
            e.3 += toks.len() as u64 + 1;
        }
        let saved = (self.cur_src, self.cur_why);
        self.cur_src = n;
        self.cur_why = u32::MAX;
        let pv = (self.prov_mode, self.prov_rule);
        let on = self.prov.is_some();
        let pin = if on {
            (self.prov_mode, self.prov_rule) = (crate::provenance::PM_RULE, Some((n, c)));
            self.prov_in_take()
        } else {
            0
        };
        let rule0 = self.stop_top.as_mut().map(|s| s.enter_rule(n, c, ci));
        self.with_s(|m| m.apply(n, &c, &toks, prim));
        if let (Some(r0), Some(s)) = (rule0, self.stop_top.as_mut()) {
            s.leave_rule(r0);
        }
        (self.prov_mode, self.prov_rule) = pv;
        if on {
            self.prov_in_set(pin);
        }
        (self.cur_src, self.cur_why) = saved;
        self.bufs.push(toks);
    }

    // --------------------------------------------------------------- fields

    /// The node holding the prototype objects of `t`.
    pub fn proto_node(&mut self, t: Tok) -> NodeId {
        #[cfg(test)]
        if self.packet_scope.is_some() && self.packet_protocol_depth == 0 {
            return self.packet_field_protocol(|m| m.proto_node(t));
        }
        if let Some(&n) = self.proto_node.get(&t) {
            return n;
        }
        let n = self.node_named("protos", || format!("protos({t})"));
        #[cfg(test)]
        {
            self.nodes[n as usize].field_origin = Some(crate::summary::packet::FieldOrigin {
                object: Some(t),
                key: crate::summary::packet::FieldPort::Prototype,
            });
        }
        self.nodes[n as usize].obs = self.obs_toks.contains(&t);
        self.proto_node.insert(t, n);
        self.rel(|| format!("protonode {n} {t}"));
        // Rule PXNEWT: an object made with a Proxy new target has the
        // prototype its `prototype` [[Get]] gives (`proxy_nt_inst`) only.
        if self.proxy_nt_toks.contains(&t) {
            return n;
        }
        // Seed the static part of the chain.
        let objproto = self.tok(TokKind::ObjectProto);
        match self.toks[t as usize].clone() {
            TokKind::Obj(tm, _) => match self.tmpls[tm as usize].kind.clone() {
                // A specialization's instances have its origin's prototype
                // object (`js_register_class_generic_origin`: one object;
                // `class_id_holders` holds its class-id members).
                TmplKind::Class { .. } => {
                    let p = self.tok(TokKind::Proto(self.constructor_origin(tm)));
                    self.add_tok(n, p);
                }
                TmplKind::FnInst { func } => {
                    // `new f()` takes its prototype from `f.prototype`,
                    // which holds `f`'s own prototype object from `f`'s
                    // creation, read or not (as a `f.prototype` read mints
                    // it, `builtins::loads`), unless a store replaced it.
                    let ftok = self.funcs[func as usize].token;
                    let a = self.atom("prototype");
                    let fp = self.field(ftok, a);
                    let p = self.tok(TokKind::Proto(tm));
                    self.add_tok(fp, p);
                    self.edge(fp, n);
                }
                // Object.create / literal prototypes are added by gen. A
                // receiver token or marker has its classes' (DECISIONS 59).
                _ => match self.virt_protos.get(&t).cloned() {
                    Some(ps) => {
                        for p in ps {
                            let pt = self.tok(TokKind::Proto(self.constructor_origin(p)));
                            self.add_tok(n, pt);
                        }
                    }
                    None => self.add_tok(n, objproto),
                },
            },
            TokKind::Proto(tm) => match (
                self.tmpls[tm as usize].parent,
                self.tmpls[tm as usize].native_parent.clone(),
            ) {
                (Some(p), _) => {
                    let pp = self.tok(TokKind::Proto(self.constructor_origin(p)));
                    self.add_tok(n, pp);
                }
                (None, Some((_, name))) => {
                    let np = crate::builtins::native_proto(self, &name);
                    self.add_tok(n, np);
                }
                (None, None) => match self.tmpls[tm as usize].parent_value {
                    // `S.prototype.__proto__` is the parent's `prototype`.
                    Some((_, pp)) => self.edge(pp, n),
                    // A null-prototype record: the chain ends here.
                    None if self.tmpls[tm as usize].null_proto => {}
                    None => self.add_tok(n, objproto),
                },
            },
            TokKind::NativeProto(..) => self.add_tok(n, objproto),
            // A builtin namespace's prototype is `Function.prototype` (a
            // constructor's) or `Object.prototype` (`Math`, `JSON`, the
            // global object's chain): `ObjectProto` stands for both.
            // A generator object (its step closure) inherits from what its
            // generator function's `prototype` holds (`FuncInfo::gen_proto`:
            // the original object or a replacement), whose chain the
            // runtime's generator prototypes continue.
            TokKind::Func(f, _) if self.funcs[f as usize].gen_step() => {
                if let Some(p) = self.funcs[f as usize].gen_proto {
                    self.edge(p, n);
                }
                self.add_tok(n, objproto);
            }
            // A builtin constructor whose [[Prototype]] is another one
            // (`TypeError`'s is `Error`): that namespace.
            TokKind::Builtin(ns) if crate::builtins::ns_parent(ns).is_some() => {
                let p = self.tok(TokKind::Builtin(crate::builtins::ns_parent(ns).unwrap()));
                self.add_tok(n, p);
            }
            // A realm constructor whose declared [[Prototype]] is another
            // realm constructor (`Readable` -> `Stream` -> `EventEmitter`,
            // node's legacy stream chain): that constructor's value, so a
            // lookup on it reaches what the program gave or stored up the
            // chain (`Object.setPrototypeOf(EventEmitter, p)` then
            // `Readable.apply`).
            TokKind::BuiltinFn(Native::Member {
                identity,
                holder: None,
            }) if crate::builtins::realm_parent_constructor(identity).is_some() => {
                let parent = crate::builtins::realm_parent_constructor(identity).unwrap();
                let p = self.tok(TokKind::BuiltinFn(Native::Member {
                    identity: parent,
                    holder: None,
                }));
                self.add_tok(n, p);
            }
            TokKind::Func(_, _)
            | TokKind::Bound(_)
            | TokKind::BuiltinFn(_)
            | TokKind::Builtin(_) => self.add_tok(n, objproto),
            TokKind::ClassObj(tm) => match self.tmpls[tm as usize].parent {
                Some(p) => {
                    let pc = self.tok(TokKind::ClassObj(p));
                    self.add_tok(n, pc);
                }
                None => match self.tmpls[tm as usize].parent_value {
                    // `S.__proto__` is the value of the `extends` expression.
                    Some((pv, _)) => self.edge(pv, n),
                    None => match self.tmpls[tm as usize].native_parent.clone() {
                        // `class C extends EventEmitter` over a named import
                        // of a node module: the heritage reads the import's
                        // binding, which holds what key `EventEmitter` of the
                        // module object reads (the named ESM export value,
                        // `js_native_module_named_esm_export_value`).
                        Some((module, name)) if !self.unit_mode => {
                            match crate::builtins::node_module_ns(&module) {
                                Some(ns) => {
                                    let mt = self.tok(TokKind::Builtin(ns));
                                    let mn = self.node_l("native-parent-module");
                                    self.add_tok(mn, mt);
                                    let key = Key::Named(self.atom(&name));
                                    self.cons(mn, Cons::Load { key, dst: n });
                                }
                                None => self.add_tok(n, objproto),
                            }
                        }
                        _ => self.add_tok(n, objproto),
                    },
                },
            },
            _ => {}
        }
        n
    }

    fn fns_of(&self, n: NodeId) -> Vec<Tok> {
        self.nodes[n as usize].pts.to_sorted()
    }

    // ---------------------------------------------------------------- rules

    pub fn apply(&mut self, n: NodeId, c: &Cons, toks: &[Tok], prim: u16) {
        self.apply_tokens(n, c, Tokens::Slice(toks), prim);
    }
    /// R9READ11: a builtin's prototype object taken as a value
    /// (`builtins::builtin_proto_value`: a `NativeProto` token) is also the
    /// collapsed builtin prototype (`ObjectProto` stands for each of them)
    /// for every operation but a read of a key: a store, a definition, a
    /// delete, a reshape, an escape, a flow or a call through the value is
    /// the operation on `ObjectProto` too, as when the value held that
    /// token. A read (`Load`, `MCall`) reads the builtin's members only:
    /// the family read a receiver of the builtin's kind makes, which sees
    /// every write of a builtin prototype (`intrinsic_miss`).
    fn native_proto_alias(&mut self, c: &Cons, toks: Tokens<'_>) -> Option<Vec<Tok>> {
        if matches!(c, Cons::Load { .. } | Cons::MCall { .. }) {
            return None;
        }
        let mut native = false;
        for t in toks.iter() {
            match self.toks[*t as usize] {
                TokKind::ObjectProto => return None,
                // A realm prototype taken as a value
                // (`builtins::realm_proto_value`) is one object of its own:
                // an operation through it is on it alone, and the lookups
                // that pass it ask it under its identity
                // (`builtins::intrinsic_miss_on`).
                TokKind::NativeProto(..) if builtins::realm_proto_of(self, *t).is_some() => {}
                TokKind::NativeProto(..) => native = true,
                _ => {}
            }
        }
        if !native {
            return None;
        }
        let mut v: Vec<Tok> = toks.iter().copied().collect();
        v.push(self.tok(TokKind::ObjectProto));
        Some(v)
    }
    fn apply_tokens(&mut self, n: NodeId, c: &Cons, toks: Tokens<'_>, prim: u16) {
        // PROXY2: a Proxy token reaches `proxy_apply` only.
        if toks
            .iter()
            .any(|&t| matches!(self.toks[t as usize], TokKind::Proxy(_)))
        {
            let mut rest: Vec<Tok> = Vec::new();
            for &t in toks.iter() {
                match self.toks[t as usize] {
                    TokKind::Proxy(p) => self.proxy_apply(n, c, p),
                    _ => rest.push(t),
                }
            }
            if rest.is_empty() && prim == 0 {
                return;
            }
            return self.apply_tokens(n, c, Tokens::Slice(&rest), prim);
        }
        // GUARDS1: a receiver-only operation (a store, a definition, a
        // delete) through the alias changes the builtin prototype it holds,
        // not `%Object.prototype%` (`Model::objproto_direct`); every other
        // operation keeps counting as one on `%Object.prototype%`.
        let saved = self.objproto_alias;
        if let Some(v) = self.native_proto_alias(c, toks) {
            self.objproto_alias = matches!(
                c,
                Cons::Store { .. }
                    | Cons::DeleteKey { .. }
                    | Cons::DeleteIdx { .. }
                    | Cons::DefineKey { .. }
                    | Cons::DefineAll { .. }
                    | Cons::DefineDesc { .. }
            );
            self.apply_tokens_on(n, c, Tokens::Slice(&v), prim);
            self.objproto_alias = saved;
            return;
        }
        self.objproto_alias = false;
        self.apply_tokens_on(n, c, toks, prim);
        self.objproto_alias = saved;
    }
    fn apply_tokens_on(&mut self, n: NodeId, c: &Cons, toks: Tokens<'_>, prim: u16) {
        if crate::hold_ledger::blast()
            && self.ledger.as_ref().is_some_and(|l| l.cur == u32::MAX)
            && toks.iter().any(|&t| t == crate::model::TOP)
        {
            let k = self.top_origin_id(n);
            let rest: Vec<Tok> = toks
                .iter()
                .copied()
                .filter(|&t| t != crate::model::TOP)
                .collect();
            if !rest.is_empty() || prim != 0 {
                self.apply_tokens_on_b(n, c, Tokens::Slice(&rest), prim);
            }
            let saved = self.ledger.as_mut().unwrap().enter_top(k);
            self.apply_tokens_on_b(n, c, Tokens::Slice(&[crate::model::TOP]), 0);
            if let Some(l) = self.ledger.as_mut() {
                l.leave(saved);
            }
            return;
        }
        self.apply_tokens_on_b(n, c, toks, prim);
    }
    fn apply_tokens_on_b(&mut self, n: NodeId, c: &Cons, toks: Tokens<'_>, prim: u16) {
        if let Some(d) = self.diagnostic.as_mut() {
            let saved = (d.cur_call.take(), d.cause_node.replace(n));
            self.apply_tokens_in(n, c, toks, prim);
            if let Some(d) = self.diagnostic.as_mut() {
                (d.cur_call, d.cause_node) = saved;
            }
            return;
        }
        self.apply_tokens_in(n, c, toks, prim);
    }
    fn apply_tokens_in(&mut self, n: NodeId, c: &Cons, toks: Tokens<'_>, prim: u16) {
        if self.sum_skip(n, c) {
            return;
        }
        if self.profile.is_some() {
            let started = std::time::Instant::now();
            let w0 = self.work.get();
            self.apply_inner(n, c, toks, prim);
            let name = c.kind_name();
            if let Some(p) = self.prof.as_mut() {
                let e = p.kind.entry(name).or_default();
                e.0 += 1;
                e.1 += self.work.get() - w0;
            }
            let e = self.profile.as_mut().unwrap().entry(name).or_default();
            e.0 += 1;
            e.1 += started.elapsed();
            return;
        }
        self.apply_inner(n, c, toks, prim);
    }

    /// Is `site` a construction-phase writer for instances of `tm`?
    fn is_ctor_phase(&self, site: SiteId, tm: TmplId) -> bool {
        if let TmplKind::FnInst { func } = self.tmpls[tm as usize].kind {
            return self.stores[site as usize].ctor_fn == Some(func);
        }
        let Some(owner) = self.stores[site as usize].ctor_phase_of else {
            return false;
        };
        // The store sits in the constructor of `owner`; instances of `tm`
        // run it during construction when `owner` is `tm` or an ancestor.
        self.has_ancestor(tm, owner)
    }

    /// After the solve (§17.2 item 3): a parent value that holds exactly one
    /// class and nothing else is the class's parent (one chain for derive,
    /// the contract export and `ancestors`); anything else — ⊤, two classes,
    /// a non-class — leaves no single layout chain: hard E11.
    pub fn settle_parent_values(&mut self) {
        for tm in 0..self.tmpls.len() as TmplId {
            let Some((pv, _)) = self.tmpls[tm as usize].parent_value else {
                continue;
            };
            let toks = self.nodes[pv as usize].pts.to_sorted();
            // `extends v` with v a primitive other than null throws a
            // TypeError when the class is defined: no instance of it exists
            // with that parent. `extends null` is a parent of its own.
            let null = self.nodes[pv as usize].prim & prim::NULL != 0;
            let one = match toks.as_slice() {
                [t] if !null => match self.toks[*t as usize] {
                    TokKind::ClassObj(p) if !self.has_ancestor(p, tm) => Some(p),
                    _ => None,
                },
                _ => None,
            };
            // One realm constructor (ECMA-262 13.3.7.1): the chain is the
            // class's prototype, then the realm prototype; `super()` runs
            // the constructor's declared construct effects on the instance
            // (`run_super_construct`), else the instance is E21.
            let realm = match toks.as_slice() {
                [t] if !null && one.is_none() => match &self.toks[*t as usize] {
                    TokKind::BuiltinFn(crate::model::Native::Member { identity, .. }) => {
                        Some(identity.to_string())
                    }
                    TokKind::Builtin(_) => Some(self.tok_name(*t)),
                    _ => None,
                },
                _ => None,
            };
            // A class linked to a realm parent by name (`Model::realm_parent`):
            // the evaluated `extends` is exactly that constructor, else the
            // layout the solve used is not the instances' (E11, as for a
            // program parent linked by name).
            if let Some(&want) = self.realm_parent.get(&tm) {
                if realm.as_deref() != Some(want) {
                    let inst = self.tok(TokKind::Obj(tm, NO_CTX));
                    let what = format!(
                        "E11 class whose evaluated `extends` is not its linked realm constructor {want} ({} tokens{}{})",
                        toks.len(),
                        if toks.contains(&TOP) { ", ⊤" } else { "" },
                        if null { ", null" } else { "" }
                    );
                    let r = self.reason(&what);
                    self.mark_hard(inst, Hard::E11ReflectConstruct, r);
                    self.parent_unsettled.insert(tm);
                    continue;
                }
            }
            if let Some(identity) = realm {
                self.parent_unsettled.insert(tm);
                if self.unit_mode || crate::builtins::declared_super_construct(&identity).is_none()
                {
                    let inst = self.tok(TokKind::Obj(tm, NO_CTX));
                    let r = self.reason(&format!(
                        "E21 realm parent constructor {identity} declares no construct effects"
                    ));
                    self.mark_hard(inst, Hard::E21NativeParent, r);
                }
                continue;
            }
            match one {
                Some(p) => self.tmpls[tm as usize].parent = Some(p),
                None => {
                    let inst = self.tok(TokKind::Obj(tm, NO_CTX));
                    let what = format!(
                        "E11 class whose `extends` value is not one class ({} tokens{}{})",
                        toks.len(),
                        if toks.contains(&TOP) { ", ⊤" } else { "" },
                        if null { ", null" } else { "" }
                    );
                    let r = self.reason(&what);
                    self.mark_hard(inst, Hard::E11ReflectConstruct, r);
                    self.parent_unsettled.insert(tm);
                }
            }
        }
        // A parent linked by name: the evaluated `extends` value is that
        // class (the same primitive rule as above), else the chain the
        // solve modeled is not the chain of the instances.
        for (tm, chk) in std::mem::take(&mut self.parent_checks) {
            let Some(p) = self.tmpls[tm as usize].parent else {
                continue;
            };
            let toks = self.nodes[chk as usize].pts.to_sorted();
            let null = self.nodes[chk as usize].prim & prim::NULL != 0;
            let linked = !null
                && matches!(toks.as_slice(), [t] if matches!(self.toks[*t as usize], TokKind::ClassObj(q) if q == p));
            if !linked {
                let inst = self.tok(TokKind::Obj(tm, NO_CTX));
                let what = format!(
                    "E11 class whose evaluated `extends` is not its linked parent ({} tokens{}{})",
                    toks.len(),
                    if toks.contains(&TOP) { ", ⊤" } else { "" },
                    if null { ", null" } else { "" }
                );
                let r = self.reason(&what);
                self.mark_hard(inst, Hard::E11ReflectConstruct, r);
                self.parent_unsettled.insert(tm);
            }
        }
    }
}

/// May key `name` be ToString of a Number (an index, `1.5`, `-1`, `1e+21`,
/// `NaN`, `Infinity`)? A store under a Number key lands there. A superset
/// is safe: such a key reads the elements as well.
pub(crate) fn numeric_key(name: &str) -> bool {
    name.parse::<f64>().is_ok() || matches!(name, "NaN" | "Infinity" | "-Infinity")
}
