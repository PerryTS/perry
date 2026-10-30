//! Observational dump data. Never add constraints, nodes, tokens or proof inputs here.
use crate::gen::{Fx, Gen};
use crate::ledger::Unknown;
use crate::model::*;
use crate::pts::FxHashMap;
use perry_hir::Expr;
use std::collections::{BTreeMap, BTreeSet};

#[derive(Default)]
pub(crate) struct Trace {
    pub pred: BTreeMap<NodeId, (NodeId, String)>,
    pub contract_inputs: BTreeMap<NodeId, (String, String)>,
    pub seeds: BTreeMap<NodeId, String>,
    pub bound_origins: BTreeMap<u32, CallId>,
    pub source_calls: BTreeSet<CallId>,
    pub raw: BTreeMap<CallId, BTreeSet<Tok>>,
    pub builtins: BTreeMap<CallId, BTreeSet<String>>,
    /// The tokens whose `Model::escape` is running, innermost last.
    pub escaping: Vec<Tok>,
    /// The call whose transfer is running (`Model::diagnostic_callee`).
    pub cur_call: Option<CallId>,
    /// The node whose rule is being applied (`Model::apply_tokens`).
    pub cause_node: Option<NodeId>,
    /// Why each `TopSink` node sinks: the token whose escape made it, the
    /// call, and the function generation was walking.
    pub sink_by: BTreeMap<NodeId, SinkBy>,
    /// Each token's first escape (where it first became escaped).
    pub esc_first: BTreeMap<Tok, EscFirst>,
    /// Every (token, cause) of an escape attempt, taking effect or not.
    pub esc_causes: BTreeSet<(Tok, Cause)>,
    /// Every (function, transfer kind, cause) that reached a body, with the
    /// first call it was seen at.
    pub reach: BTreeMap<(FnIdx, &'static str, Cause), Option<CallId>>,
    /// A summary instance's fresh slot node -> the summary's own node of
    /// that slot: what an instance holds without a recorded predecessor it
    /// holds as its summary does.
    pub inst_orig: BTreeMap<NodeId, NodeId>,
    /// UEB census: the operation each parameter marker first escaped by
    /// (`Model::mark_escape_why`), and the stack of those running.
    pub esc_why: BTreeMap<Tok, String>,
    pub esc_ctx: Vec<String>,
    /// UEB census: a stand-in function's parameter node its contract sinks
    /// (node, callee, index, reach key: `Some("")` the whole reach).
    pub stand_in_params: Vec<(NodeId, String, u32, Option<String>)>,
    /// `Model::top_predecessors`, built when a chain needs it.
    pub top_in: std::cell::OnceCell<FxHashMap<NodeId, Vec<NodeId>>>,
}

/// The operation an event belongs to: a function body's (`Fn`), a token's
/// escape (`Tok`), or none known (`Unknown`).
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub(crate) enum Cause {
    Fn(FnIdx),
    Tok(Tok),
    Unknown,
}

#[derive(Clone, Copy)]
pub(crate) struct SinkBy {
    pub by: Option<Tok>,
    pub call: Option<CallId>,
    pub gen_fn: FnIdx,
}

pub(crate) struct EscFirst {
    pub reason: u32,
    pub cause: Cause,
    pub call: Option<CallId>,
    pub node: Option<NodeId>,
}

#[derive(Default)]
pub(crate) struct Capture {
    pub values: FxHashMap<usize, NodeId>,
    pub calls: FxHashMap<NodeId, CallId>,
    pub namespace_receivers: BTreeSet<NodeId>,
    sites: Vec<Site>,
}
struct Site {
    module: u32,
    function: FnIdx,
    start: u32,
    end: u32,
    kind: &'static str,
    key: String,
    origin: &'static str,
    receiver: Option<NodeId>,
    index: Option<NodeId>,
    callee: Option<NodeId>,
    result: NodeId,
    call: Option<CallId>,
    /// The HIR expression's address (`ledger::census` compares the
    /// recorded sites with the HIR).
    addr: usize,
}

impl Model {
    pub(crate) fn diagnostic_predecessor(&mut self, dst: NodeId, src: NodeId, reason: u32) {
        if let Some(d) = &mut self.diagnostic {
            d.pred.entry(dst).or_insert_with(|| {
                (
                    src,
                    self.reasons
                        .get(reason as usize)
                        .cloned()
                        .unwrap_or_default(),
                )
            });
        }
    }
    /// The origin text of a ⊤ whose chain has no recorded seed: the node it
    /// ended at (its label) and how the chain broke. `ledger::classify`
    /// reads it as `untraced-top`; the ledger reports the labels.
    fn untraced(&self, n: NodeId, what: &str) -> String {
        let label = self.labels.get(n as usize).copied().unwrap_or("");
        format!("untraced: {what} at {label}")
    }

    /// The ⊤-holding nodes with a copy edge into each node (built once per
    /// model, only for a chain that has no recorded arrival).
    fn top_predecessors(&self) -> &FxHashMap<NodeId, Vec<NodeId>> {
        let d = self.diagnostic.as_ref().expect("a diagnostic model");
        d.top_in.get_or_init(|| {
            let mut m: FxHashMap<NodeId, Vec<NodeId>> = FxHashMap::default();
            for (s, v) in self.nodes.iter().enumerate() {
                if v.pts.contains_tok(TOP) {
                    for &b in &v.succ {
                        m.entry(b).or_default().push(s as NodeId);
                    }
                }
            }
            for v in m.values_mut() {
                v.sort_unstable();
                v.dedup();
            }
            m
        })
    }

    fn diagnostic_root(&self, start: NodeId) -> (NodeId, String) {
        let Some(d) = &self.diagnostic else {
            return (start, String::new());
        };
        let mut n = start;
        let mut seen = BTreeSet::new();
        // The node that led the walk to `n`.
        let mut prev: Option<NodeId> = None;
        // An instance slot with no recorded arrival holds what its
        // summary's node holds (a slot without one, `NO_NODE`, ends here).
        let summary = |n: NodeId| {
            d.inst_orig.get(&n).copied().filter(|&o| {
                o != n
                    && self
                        .nodes
                        .get(o as usize)
                        .is_some_and(|v| v.pts.contains_tok(TOP))
            })
        };
        // Where the first arrival is not recorded (a set adopted whole, a
        // seed copied from a summary, an edge added before the value): the
        // lowest ⊤-holding node with a copy edge into `at` that the walk has
        // not been through holds it as well.
        let other = |at: NodeId, seen: &BTreeSet<NodeId>| {
            self.top_predecessors()
                .get(&at)
                .and_then(|v| v.iter().copied().find(|s| !seen.contains(s)))
        };
        loop {
            if !seen.insert(n) {
                // A chain that came back to a node: leave through another
                // predecessor of the node that led here.
                match prev.and_then(|p| other(p, &seen).map(|o| (p, o))) {
                    Some((p, o)) => {
                        n = o;
                        prev = Some(p);
                        continue;
                    }
                    None => return (n, "cycle".into()),
                }
            }
            if let Some(why) = d.seeds.get(&n) {
                return (n, why.clone());
            }
            let Some((src, why)) = d.pred.get(&n) else {
                if let Some(o) = summary(n) {
                    prev = Some(n);
                    n = o;
                    continue;
                }
                match other(n, &seen) {
                    Some(o) => {
                        prev = Some(n);
                        n = o;
                        continue;
                    }
                    None => return (n, self.untraced(n, "no arrival recorded")),
                }
            };
            if !why.is_empty() {
                return (n, why.clone());
            }
            if *src == NO_NODE || !self.nodes[*src as usize].pts.contains_tok(TOP) {
                if let Some(o) = summary(n) {
                    prev = Some(n);
                    n = o;
                    continue;
                }
                match other(n, &seen) {
                    Some(o) => {
                        prev = Some(n);
                        n = o;
                        continue;
                    }
                    None => {
                        let what = if *src == NO_NODE {
                            "arrival outside a rule"
                        } else {
                            "source no longer holds it"
                        };
                        return (n, self.untraced(n, what));
                    }
                }
            }
            prev = Some(n);
            n = *src;
        }
    }

    /// A summary instance keeps its template call identity. `CallSite::site`
    /// is an allocation context, and synthetic protocol calls may use zero:
    /// it must never be used to attribute their callees to source call zero.
    pub(crate) fn diagnostic_source_call(&self, call: CallId) -> Option<CallId> {
        let d = self.diagnostic.as_ref()?;
        if d.source_calls.contains(&call) {
            return Some(call);
        }
        let source = *self.sum.tmpl_of.get(&call)?;
        d.source_calls.contains(&source).then_some(source)
    }
    /// The function a node's rules run for: its body's owner, or the
    /// function of the run (instance or shared run) it belongs to.
    pub(crate) fn diagnostic_owner(&self, n: NodeId) -> Option<FnIdx> {
        if n == NO_NODE || n as usize >= self.nodes.len() {
            return None;
        }
        let f = self.node_fn[n as usize];
        if f != NO_FN {
            return Some(f);
        }
        let fr = self.nodes[n as usize].frame;
        if fr == 0 {
            None
        } else if fr & FN_RUN != 0 {
            Some(fr & !FN_RUN)
        } else {
            self.sum.insts.get((fr - 1) as usize).map(|i| i.f)
        }
    }
    fn diagnostic_call_owner(&self, call: Option<CallId>) -> Option<FnIdx> {
        let c = call?;
        let cs = self.calls.get(c as usize)?;
        self.diagnostic_owner(cs.ret).or_else(|| {
            let src = self.diagnostic_source_call(c)?;
            self.diagnostic_owner(self.calls.get(src as usize)?.ret)
        })
    }
    fn diagnostic_sink_cause(&self, n: Option<NodeId>) -> Option<Cause> {
        let d = self.diagnostic.as_ref()?;
        let s = d.sink_by.get(&n?)?;
        if let Some(t) = s.by {
            return Some(Cause::Tok(t));
        }
        if let Some(f) = self.diagnostic_call_owner(s.call) {
            return Some(Cause::Fn(f));
        }
        (s.gen_fn != NO_FN).then_some(Cause::Fn(s.gen_fn))
    }
    /// The operation an escape attempt now belongs to.
    fn diagnostic_escape_cause(&self) -> Cause {
        let Some(d) = self.diagnostic.as_ref() else {
            return Cause::Unknown;
        };
        if let Some(f) = d.cause_node.and_then(|n| self.diagnostic_owner(n)) {
            return Cause::Fn(f);
        }
        if let Some(&t) = d.escaping.last() {
            return Cause::Tok(t);
        }
        if let Some(c) = self.diagnostic_sink_cause(d.cause_node) {
            return c;
        }
        if let Some(f) = self.diagnostic_call_owner(d.cur_call) {
            return Cause::Fn(f);
        }
        if self.cur_fn != NO_FN {
            return Cause::Fn(self.cur_fn);
        }
        Cause::Unknown
    }
    /// The operation a transfer into a body (by `call`) belongs to.
    fn diagnostic_reach_cause(&self, call: Option<CallId>) -> Cause {
        let Some(d) = self.diagnostic.as_ref() else {
            return Cause::Unknown;
        };
        if let Some(f) = self.diagnostic_call_owner(call) {
            return Cause::Fn(f);
        }
        if let Some(&t) = d.escaping.last() {
            return Cause::Tok(t);
        }
        if let Some(f) = d.cause_node.and_then(|n| self.diagnostic_owner(n)) {
            return Cause::Fn(f);
        }
        if let Some(c) = self.diagnostic_sink_cause(d.cause_node) {
            return c;
        }
        if self.cur_fn != NO_FN {
            return Cause::Fn(self.cur_fn);
        }
        Cause::Unknown
    }
    /// Escape of `t` begins: the cause of this attempt, and of its first
    /// escape when it takes effect.
    pub(crate) fn diagnostic_escape_begin(&mut self, t: Tok, reason: u32) -> Option<EscFirst> {
        self.diagnostic.as_ref()?;
        let was = self.is_escaped(t);
        let cause = self.diagnostic_escape_cause();
        let d = self.diagnostic.as_mut()?;
        d.esc_causes.insert((t, cause));
        let cand = (!was).then(|| EscFirst {
            reason,
            cause,
            call: d.cur_call,
            node: d.cause_node,
        });
        d.escaping.push(t);
        cand
    }
    pub(crate) fn diagnostic_escape_end(&mut self, t: Tok, cand: Option<EscFirst>) {
        let now = self.is_escaped(t);
        let Some(d) = self.diagnostic.as_mut() else {
            return;
        };
        d.escaping.pop();
        if let Some(c) = cand.filter(|_| now) {
            d.esc_first.entry(t).or_insert(c);
        }
    }
    /// Marks `t` escaped (`mark_use`) by operation `why`; the dump names
    /// the first one per parameter marker (UEB census, dump only).
    pub(crate) fn mark_escape_why(&mut self, t: Tok, why: &dyn Fn(&Self) -> String) {
        if self.diagnostic.is_none() {
            return self.mark_use(t, crate::model::MARK_ESCAPE);
        }
        let w = why(self);
        self.diagnostic.as_mut().unwrap().esc_ctx.push(w);
        self.mark_use(t, crate::model::MARK_ESCAPE);
        self.diagnostic.as_mut().unwrap().esc_ctx.pop();
    }
    pub(crate) fn diagnostic_sink(&mut self, n: NodeId) {
        let gen_fn = self.cur_fn;
        if let Some(d) = self.diagnostic.as_mut() {
            let s = SinkBy {
                by: d.escaping.last().copied(),
                call: d.cur_call,
                gen_fn,
            };
            d.sink_by.entry(n).or_insert(s);
        }
    }
    /// A transfer reached the body of `f` (`call`: `u32::MAX` for none).
    pub(crate) fn diagnostic_reach(&mut self, f: FnIdx, kind: &'static str, call: CallId) {
        if self.diagnostic.is_none() {
            return;
        }
        let call = (call != u32::MAX).then_some(call);
        let cause = self.diagnostic_reach_cause(call);
        if let Some(d) = self.diagnostic.as_mut() {
            d.reach.entry((f, kind, cause)).or_insert(call);
        }
    }
    pub(crate) fn diagnostic_callee(&mut self, call: CallId, tok: Tok) {
        if let Some(d) = self.diagnostic.as_mut() {
            d.cur_call = Some(call);
        }
        let Some(source) = self.diagnostic_source_call(call) else {
            return;
        };
        self.diagnostic
            .as_mut()
            .unwrap()
            .raw
            .entry(source)
            .or_default()
            .insert(tok);
    }
    pub(crate) fn diagnostic_builtin(&mut self, call: CallId, name: impl FnOnce(&Self) -> String) {
        let Some(source) = self.diagnostic_source_call(call) else {
            return;
        };
        let name = name(self);
        self.diagnostic
            .as_mut()
            .unwrap()
            .builtins
            .entry(source)
            .or_default()
            .insert(name);
    }
}

impl Gen<'_> {
    pub(crate) fn diagnostic_expr(&mut self, fx: &Fx, e: &Expr, result: NodeId) {
        let Some(d) = &mut self.diagnostic else {
            return;
        };
        let get = |e: &Expr| d.values.get(&(e as *const Expr as usize)).copied();
        let (start, end, kind, key, receiver, index, callee) = match e {
            Expr::PropertyGet {
                object,
                property,
                byte_offset,
                byte_end,
            } => (
                *byte_offset,
                *byte_end,
                "property",
                property.clone(),
                get(object),
                None,
                None,
            ),
            Expr::IndexGet {
                object,
                index,
                byte_offset,
                byte_end,
            } => (
                *byte_offset,
                *byte_end,
                "element",
                "[]".into(),
                get(object),
                get(index),
                None,
            ),
            Expr::Call {
                callee,
                byte_offset,
                byte_end,
                ..
            }
            | Expr::CallSpread {
                callee,
                byte_offset,
                byte_end,
                ..
            } => {
                let (recv, idx, key) = match callee.as_ref() {
                    Expr::PropertyGet {
                        object, property, ..
                    } => (get(object), None, property.clone()),
                    Expr::IndexGet { object, index, .. } => (get(object), get(index), "[]".into()),
                    _ => (None, None, "()".into()),
                };
                (
                    *byte_offset,
                    *byte_end,
                    "value-call",
                    key,
                    recv,
                    idx,
                    get(callee),
                )
            }
            Expr::PropertySet {
                object,
                property,
                byte_offset,
                byte_end,
                ..
            } => (
                *byte_offset,
                *byte_end,
                "write",
                property.clone(),
                get(object),
                None,
                None,
            ),
            Expr::PutValueSet {
                target,
                key,
                byte_offset,
                byte_end,
                ..
            } => match key.as_ref() {
                Expr::String(k) => (
                    *byte_offset,
                    *byte_end,
                    "write",
                    k.clone(),
                    get(target),
                    None,
                    None,
                ),
                _ => (
                    *byte_offset,
                    *byte_end,
                    "element-write",
                    "[]".into(),
                    get(target),
                    get(key),
                    None,
                ),
            },
            Expr::PropertyUpdate {
                object,
                property,
                byte_offset,
                byte_end,
                ..
            } => (
                *byte_offset,
                *byte_end,
                "update",
                property.clone(),
                get(object),
                None,
                None,
            ),
            Expr::IndexSet {
                object,
                index,
                byte_offset,
                byte_end,
                ..
            } => (
                *byte_offset,
                *byte_end,
                "element-write",
                "[]".into(),
                get(object),
                get(index),
                None,
            ),
            Expr::IndexUpdate {
                object,
                index,
                byte_offset,
                byte_end,
                ..
            } => (
                *byte_offset,
                *byte_end,
                "element-update",
                "[]".into(),
                get(object),
                get(index),
                None,
            ),
            Expr::New {
                class_name,
                byte_offset,
                byte_end,
                ..
            } => (
                *byte_offset,
                *byte_end,
                "new",
                class_name.clone(),
                None,
                None,
                None,
            ),
            Expr::NewDynamic {
                callee,
                byte_offset,
                byte_end,
                ..
            }
            | Expr::NewDynamicSpread {
                callee,
                byte_offset,
                byte_end,
                ..
            } => (
                *byte_offset,
                *byte_end,
                "new",
                "new".into(),
                None,
                None,
                get(callee),
            ),
            _ => {
                d.values.insert(e as *const Expr as usize, result);
                return;
            }
        };
        // Namespace lowering may not walk the syntactic receiver as an expression.
        let receiver = receiver.or_else(|| {
            self.read_sites
                .iter()
                .rev()
                .find(|s| s.module == fx.module && s.byte_offset == start && s.func == fx.f)
                .map(|s| s.recv)
        });
        let namespace = receiver.is_some_and(|n| d.namespace_receivers.contains(&n));
        let origin = if namespace {
            if end == 0 {
                "synthesized-namespace"
            } else {
                "namespace"
            }
        } else if start == 0 {
            "synthesized"
        } else if end == 0 {
            "transformed"
        } else {
            "source"
        };
        d.sites.push(Site {
            module: fx.module,
            function: fx.f,
            start,
            end,
            kind,
            key,
            origin,
            receiver,
            index,
            callee,
            result,
            call: d.calls.get(&result).copied(),
            addr: e as *const Expr as usize,
        });
        d.values.insert(e as *const Expr as usize, result);
    }
}

/// The HIR sites this analysis recorded: address, kind, start.
pub(crate) fn recorded(g: &Gen) -> Vec<(usize, &'static str, u32)> {
    g.diagnostic.as_ref().map_or_else(Vec::new, |d| {
        d.sites.iter().map(|s| (s.addr, s.kind, s.start)).collect()
    })
}

impl Model {
    /// LEDGER: the first unknown value of operand `n` and why it is
    /// unknown. ⊤ names the seed of its first-arrival path; a contract
    /// function's parameter marker is the unit edge; no value at all is an
    /// unexecuted or dead operand. `None`: every value is known.
    fn diagnostic_unknown(&self, n: NodeId) -> Option<(Result<Unknown, String>, NodeId)> {
        let v = &self.nodes[n as usize];
        if v.pts.contains_tok(TOP) {
            let (root, origin) = self.diagnostic_root(n);
            return Some((crate::ledger::classify(&origin).ok_or(origin), root));
        }
        let toks = v.pts.to_sorted();
        if toks.is_empty() && v.prim == 0 {
            return Some((Ok(Unknown::NoValues), n));
        }
        // An argument marker stands for the parameter markers it reaches
        // (DECISIONS 44).
        let marker = toks.iter().find(|&&t| self.is_marker(t))?;
        let param = self
            .marker_members(*marker)
            .into_iter()
            .find_map(|t| self.param_marker.get(&t).copied());
        Some((
            Ok(match param {
                Some((_, u32::MAX)) => Unknown::UnitEdgeThis,
                Some(_) => Unknown::UnitEdgeParameter,
                None => Unknown::UnitEdgeMarker,
            }),
            n,
        ))
    }

    /// The `causes` field of a site record: one entry per open operand.
    fn diagnostic_causes(&self, roles: &[(&str, Option<NodeId>)]) -> String {
        let mut out = Vec::new();
        for (role, n) in roles {
            let Some(n) = n else {
                continue;
            };
            let Some((cause, root)) = self.diagnostic_unknown(*n) else {
                continue;
            };
            let (cause, origin) = match cause {
                Ok(c) => (c.name(), self.diagnostic_root(root).1),
                Err(origin) => ("unclassified", origin),
            };
            out.push(format!(
                "{{\"role\":{},\"cause\":{},\"root\":{root},\"rootFunction\":{},\"origin\":{}}}",
                quote(role),
                quote(cause),
                opt(self.diagnostic_owner(root)),
                quote(&origin)
            ));
        }
        format!("[{}]", out.join(","))
    }
}

pub(crate) fn quote(s: &str) -> String {
    let mut out = String::from("\"");
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if c < ' ' => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}
fn nums<T: std::fmt::Display>(v: impl IntoIterator<Item = T>) -> String {
    format!(
        "[{}]",
        v.into_iter()
            .map(|x| x.to_string())
            .collect::<Vec<_>>()
            .join(",")
    )
}
fn opt(n: Option<u32>) -> String {
    n.map_or_else(|| "null".into(), |n| n.to_string())
}
fn strings(v: impl IntoIterator<Item = String>) -> String {
    format!(
        "[{}]",
        v.into_iter()
            .map(|x| quote(&x))
            .collect::<Vec<_>>()
            .join(",")
    )
}

/// A separate JSONL section: legacy S/T/F records and their digests stay untouched.
/// Node/token IDs are local to the enclosing emitting analysis unit.
pub(crate) fn collect(g: &Gen) -> Vec<String> {
    let Some(d) = &g.diagnostic else {
        return Vec::new();
    };
    let m = &g.m;
    let mut out = vec![format!(
        "{{\"record\":\"analysis\",\"aborted\":{},\"work\":{},\"maxWork\":{}}}",
        m.aborted, m.work, m.max_work
    )];
    let mut nodes = BTreeSet::new();
    let locations: BTreeMap<CallId, &Site> = d
        .sites
        .iter()
        .filter_map(|s| s.call.map(|c| (c, s)))
        .collect();
    for s in &d.sites {
        nodes.extend(
            [s.receiver, s.index, s.callee, Some(s.result)]
                .into_iter()
                .flatten(),
        );
        let call = s.call.map(|c| &m.calls[c as usize]);
        if let Some(c) = call {
            nodes.extend(c.args.iter().chain(&c.spread).copied());
        }
        let raw = s.call.and_then(|c| m.diagnostic.as_ref()?.raw.get(&c));
        let bi = s.call.and_then(|c| m.diagnostic.as_ref()?.builtins.get(&c));
        let causes = m.diagnostic_causes(&[
            ("receiver", s.receiver),
            ("index", s.index),
            ("callee", s.callee),
        ]);
        out.push(format!("{{\"record\":\"site\",\"file\":{},\"function\":{},\"start\":{},\"end\":{},\"kind\":{},\"key\":{},\"origin\":{},\"causes\":{causes},\"receiver\":{},\"index\":{},\"callee\":{},\"result\":{},\"call\":{},\"raw\":{},\"builtins\":{},\"args\":{},\"spread\":{}}}",
            quote(&g.modules[s.module as usize].0),s.function,s.start,s.end,quote(s.kind),quote(&s.key),quote(s.origin),
            opt(s.receiver),opt(s.index),opt(s.callee),s.result,opt(s.call),
            nums(raw.into_iter().flatten()),strings(bi.into_iter().flatten().cloned()),
            nums(call.into_iter().flat_map(|c|c.args.iter())),nums(call.into_iter().flat_map(|c|c.spread.iter()))));
    }
    for (i, f) in m.funcs.iter().enumerate() {
        nodes.extend(f.params.iter().chain([&f.this, &f.ret]));
        let module = g.modules.get(f.module as usize);
        let src = module.and_then(|(_, m)| m.closure_source_text.get(&f.func_id));
        let span = src.and_then(|s| s.source_span).unwrap_or((0, 0));
        let declaration_key = g
            .stand_in_fc
            .get(&f.token)
            .map(|c| c.name.clone())
            .unwrap_or_else(|| crate::fn_contract_name(g, i as FnIdx).0);
        out.push(format!("{{\"record\":\"function\",\"id\":{i},\"hirId\":{},\"declarationKey\":{},\"file\":{},\"name\":{},\"start\":{},\"end\":{},\"params\":{},\"this\":{},\"result\":{},\"rest\":{},\"parent\":{},\"escape\":{}}}",
            f.func_id,quote(&declaration_key),quote(module.map_or("",|(p,_)|*p)),quote(&f.name),span.0,span.1,nums(&f.params),f.this,f.ret,opt(f.rest),opt((f.parent!=NO_FN).then_some(f.parent)),
            quote(m.escaped.get(&f.token).map_or("",|r|&m.reasons[*r as usize]))));
    }
    for (i, t) in m.tmpls.iter().enumerate() {
        let (module, classid) = match &t.kind {
            TmplKind::Class {
                module, class_id, ..
            } => (Some(*module), Some(*class_id)),
            _ => (None, None),
        };
        let source = module
            .and_then(|mi| g.modules.get(mi as usize))
            .and_then(|(_, m)| m.class_source_text.get(&classid?));
        let span = source.map(|s| s.source_range()).unwrap_or(0..0);
        let mut ancestors = Vec::new();
        let mut next = t.parent;
        while let Some(a) = next {
            if ancestors.contains(&a) {
                break;
            }
            ancestors.push(a);
            next = m.tmpls[a as usize].parent;
        }
        out.push(format!("{{\"record\":\"layout\",\"id\":{i},\"name\":{},\"kind\":{},\"file\":{},\"start\":{},\"end\":{},\"parent\":{},\"parentLinks\":{},\"ancestors\":{},\"nativeParent\":{}}}",
            quote(&t.display),quote(&format!("{:?}",t.kind)),quote(module.map_or("",|mi|g.modules[mi as usize].0)),
            if source.is_some(){span.start+1}else{0},if source.is_some(){span.end+1}else{0},opt(t.parent),nums(&t.parent_links),nums(ancestors),quote(&format!("{:?}",t.native_parent))));
    }
    // Fields already reached by the solver, including independent any-key TOP.
    // Reading these maps must never call field()/load_from() and create flow.
    let mut fields: Vec<_> = m
        .fields
        .iter()
        .map(|(&(t, a), &n)| (t, m.atoms[a as usize].clone(), n))
        .collect();
    fields.extend(m.field_any.iter().map(|(&t, &n)| (t, "*".into(), n)));
    fields.sort();
    for (t, key, n) in fields {
        if m.nodes[n as usize].pts.contains_tok(TOP) {
            nodes.insert(n);
            out.push(format!(
                "{{\"record\":\"top-field\",\"token\":{t},\"key\":{},\"node\":{n}}}",
                quote(&key)
            ));
        }
    }
    for (i, t) in m.toks.iter().enumerate() {
        let (kind, layout, function, context) = match t {
            TokKind::Top => ("top", None, None, None),
            TokKind::Obj(l, c) => ("object", Some(*l), None, Some(*c)),
            TokKind::Func(f, c) => ("function", None, Some(*f), Some(*c)),
            TokKind::Bound(_) => ("bound", None, None, None),
            TokKind::ClassObj(l) => ("class", Some(*l), None, None),
            TokKind::Proto(l) => ("prototype", Some(*l), None, None),
            TokKind::Builtin(_) | TokKind::BuiltinFn => ("builtin", None, None, None),
            TokKind::ModNs(_) => ("namespace", None, None, None),
            TokKind::ObjectProto | TokKind::NativeProto(_) => {
                ("native-prototype", None, None, None)
            }
        };
        let bound = if let TokKind::Bound(b) = t {
            let (target, this, args) = &m.bound[*b as usize];
            nodes.extend([target, this]);
            nodes.extend(args);
            let origin = m
                .diagnostic
                .as_ref()
                .and_then(|d| d.bound_origins.get(b))
                .and_then(|c| locations.get(c))
                .map_or("null".into(), |s| {
                    format!(
                        "{{\"file\":{},\"start\":{},\"end\":{}}}",
                        quote(g.modules[s.module as usize].0),
                        s.start,
                        s.end
                    )
                });
            format!(
                "{{\"target\":{target},\"this\":{this},\"args\":{},\"origin\":{origin}}}",
                nums(args)
            )
        } else {
            "null".into()
        };
        out.push(format!("{{\"record\":\"token\",\"id\":{i},\"kind\":{},\"name\":{},\"layout\":{},\"function\":{},\"context\":{},\"bound\":{},\"escape\":{},\"marker\":{},\"closedMarker\":{}}}",
            quote(kind),quote(&m.tok_name(i as Tok)),opt(layout),opt(function),opt(context),bound,
            quote(m.escaped.get(&(i as Tok)).map_or("",|r|&m.reasons[*r as usize])), m.is_marker(i as Tok), m.closed_marker(i as Tok)));
    }

    // FCT slice 0: where each token first escaped, every cause of an
    // escape attempt, and every transfer into a body with its cause.
    let cause = |c: &Cause| -> String {
        match c {
            Cause::Fn(f) => format!("{{\"function\":{f}}}"),
            Cause::Tok(t) => format!("{{\"token\":{t}}}"),
            Cause::Unknown => "null".into(),
        }
    };
    let src_call = |c: Option<CallId>| c.map(|c| m.diagnostic_source_call(c).unwrap_or(c));
    if let Some(d) = m.diagnostic.as_ref() {
        for (t, e) in &d.esc_first {
            out.push(format!("{{\"record\":\"escape-first\",\"token\":{t},\"reason\":{},\"cause\":{},\"call\":{},\"node\":{}}}",
                quote(m.reasons.get(e.reason as usize).map_or("", |s| s)), cause(&e.cause), opt(src_call(e.call)), opt(e.node)));
        }
        for (t, w) in &d.esc_why {
            if let Some(&(f, i)) = m.param_marker.get(t) {
                out.push(format!("{{\"record\":\"param-escape\",\"token\":{t},\"function\":{f},\"index\":{i},\"marker\":{},\"why\":{}}}",
                    quote(&m.tok_name(*t)), quote(w)));
            }
        }
        for (n, name, i, k) in &d.stand_in_params {
            out.push(format!("{{\"record\":\"stand-in-param\",\"node\":{n},\"callee\":{},\"index\":{i},\"key\":{}}}",
                quote(name), k.as_deref().map_or("null".into(), quote)));
        }
        for (t, c) in &d.esc_causes {
            out.push(format!(
                "{{\"record\":\"escape-cause\",\"token\":{t},\"cause\":{}}}",
                cause(c)
            ));
        }
        for ((f, kind, c), call) in &d.reach {
            out.push(format!(
                "{{\"record\":\"reach\",\"function\":{f},\"kind\":{},\"cause\":{},\"call\":{}}}",
                quote(kind),
                cause(c),
                opt(src_call(*call))
            ));
        }
    }

    // PUB: read existing ownership/member maps, without settling or creating flow.
    for (&t, &(f, index)) in &m.param_marker {
        out.push(format!(
            "{{\"record\":\"parameter-marker\",\"token\":{t},\"function\":{f},\"index\":{index}}}"
        ));
    }
    for (t, _) in m
        .toks
        .iter()
        .enumerate()
        .filter(|(t, _)| m.is_marker(*t as Tok))
    {
        let t = t as Tok;
        let home = m.arg_marker_node.get(&t).copied();
        nodes.extend(home);
        let param = home
            .and_then(|n| m.param_at.get(&n))
            .map_or("null".into(), |(f, i)| format!("[{f},{i}]"));
        let parent = m.path_parent.get(&t).map_or("null".into(), |(p, k)| {
            format!(
                "[{p},{}]",
                k.map_or("null".into(), |a| quote(&m.atoms[a as usize]))
            )
        });
        let keyed = m.keyed_parent.get(&t).map_or("null".into(), |(p, k)| {
            format!("[{p},{}]", quote(&m.atoms[*k as usize]))
        });
        out.push(format!("{{\"record\":\"marker-route\",\"token\":{t},\"bare\":{},\"beside\":{},\"home\":{},\"parameter\":{param},\"members\":{},\"reach\":{},\"reachBase\":{},\"keyedParent\":{keyed},\"pathParent\":{parent}}}",
            m.bare_marker(t),m.beside_values(t),opt(home),nums(m.arg_members.get(&t).into_iter().flatten()),opt(m.marker_reach.get(&t).copied()),opt(m.arg_reach_base.get(&t).copied())));
    }
    for (f, fi) in m.funcs.iter().enumerate() {
        let f = f as FnIdx;
        out.push(format!("{{\"record\":\"parameter-contract\",\"function\":{f},\"closed\":{},\"described\":{},\"closedValues\":{},\"contract\":{},\"foreign\":{}}}",
            m.closed_params.contains_key(&f),m.closed_described.get(&f).map_or("null".into(), |v| format!("{:?}",v)),nums(m.closed_params.get(&f).into_iter().flatten()),m.contract_fns.contains(&f),g.stand_in_fc.contains_key(&fi.token)));
        nodes.extend(m.closed_params.get(&f).into_iter().flatten());
    }
    if let Some(trace) = &m.diagnostic {
        for (&n, (input, why)) in &trace.contract_inputs {
            nodes.insert(n);
            out.push(format!(
                "{{\"record\":\"contract-input\",\"node\":{n},\"description\":{},\"why\":{why}}}",
                quote(input)
            ));
        }
        for (&n, (src, why)) in &trace.pred {
            out.push(format!(
                "{{\"record\":\"top-predecessor\",\"node\":{n},\"source\":{src},\"reason\":{}}}",
                quote(why)
            ));
        }
    }
    for (src, v) in m.nodes.iter().enumerate() {
        if !v.succ.is_empty() {
            out.push(format!(
                "{{\"record\":\"copy-edges\",\"source\":{src},\"targets\":{}}}",
                nums(&v.succ)
            ));
        }
    }
    let roots: Vec<_> = nodes.iter().map(|&n| m.diagnostic_root(n).0).collect();
    nodes.extend(roots);
    for n in nodes {
        let v = &m.nodes[n as usize];
        let toks = v.pts.to_sorted();
        let top = toks.contains(&TOP);
        let marker_top = toks.iter().any(|&t| m.bare_marker(t));
        let mut mask = v.prim as u32;
        for &t in &toks {
            if m.is_marker(t) {
                continue;
            }
            mask |= match m.toks[t as usize] {
                TokKind::Top => 0,
                TokKind::Func(..)
                | TokKind::Bound(_)
                | TokKind::ClassObj(_)
                | TokKind::BuiltinFn => 256,
                TokKind::Builtin(ns)
                    if !matches!(
                        ns,
                        BuiltinNs::Json
                            | BuiltinNs::Math
                            | BuiltinNs::Console
                            | BuiltinNs::GlobalThis
                            | BuiltinNs::Process
                            | BuiltinNs::Other
                    ) =>
                {
                    256
                }
                _ => 128,
            };
        }
        let (root, origin) = m.diagnostic_root(n);
        out.push(format!("{{\"record\":\"node\",\"id\":{n},\"primitiveMask\":{},\"kindMask\":{mask},\"top\":{top},\"markerTop\":{marker_top},\"tokens\":{},\"label\":{},\"parameter\":{},\"observation\":{},\"topRoot\":{},\"topOrigin\":{}}}",
            v.prim,nums(toks),quote(m.labels.get(n as usize).copied().unwrap_or("")),v.param,v.obs,opt(top.then_some(root)),quote(&if top&&m.explain{m.top_origin(n)}else{origin})));
    }
    out
}

/// Existing contract summary, observed only after fact derivation.
pub(crate) fn value_brief(c: &crate::units::ValueContract) -> String {
    format!(
        "{{\"unknown\":{},\"why\":{},\"prim\":{},\"functions\":{},\"arrays\":{}}}",
        c.unknown,
        why_json(c),
        c.prim,
        strings(c.fns.iter().map(|f| f.name.clone())),
        strings(c.arrays.iter().cloned())
    )
}

/// The reason an unknown value contract is unknown and the unit that
/// published it, as JSON (`null` when it is known or has no recorded reason).
pub(crate) fn why_json(c: &crate::units::ValueContract) -> String {
    match &c.why.0 {
        Some((r, u)) if c.unknown => format!(
            "{{\"reason\":{},\"unit\":{}}}",
            quote(r),
            u.as_deref().map_or("null".into(), quote)
        ),
        _ => "null".into(),
    }
}
