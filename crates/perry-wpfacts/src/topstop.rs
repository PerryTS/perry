//! `PERRY_FACTS_STOP_ON_TOP=1`: stop the process the moment the solver
//! CREATES ⊤ (an unknown value) in a node, and print what that value is.
//!
//! Every arrival of ⊤ in a node passes `Model::add_tokens_s` or
//! `Model::adopt_delivery`; both call `Model::top_created`, which decides
//! whether the arrival is a propagation (⊤ copied along an edge from a node
//! that already holds it: nothing printed) or a creation:
//! - (S) seed: a rule puts ⊤ into the node with no ⊤ input (an unmodeled
//!   builtin result, an extern value, a direct ⊤ token, a rule applied to a
//!   node without ⊤);
//! - (A) amplifier: a rule puts ⊤ into the node because some ⊤ already
//!   exists (a rule applied to a node that holds ⊤, an escape into unknown
//!   code, a throw by unknown code that runs because of ⊤).
//!
//! - (M) marker: the second representation of "unknown": a marker that
//!   reads as ⊤ (`Model::set_marker_top`, DECISIONS 42/119) is made, or a
//!   token is marked escaped so that beside ⊤ it is that ⊤
//!   (`escaped_beside_top`, `mark_escaped`).
//!
//! Every block names a program line: every node carries, from its
//! creation, the program position of the expression it stands for
//! (`StopTop::node_pos`). Generation records the expression it walks; a rule
//! records the position of the expression that attached it
//! (`StopTop::cons_pos`), and a node the solver makes while it applies the
//! rule carries that position. The rule applications that made a node are
//! listed as `via:` steps (`StopTop::node_from`). A node made with no
//! position is a bug of its creation site: the block names that site.
//!
//! `PERRY_FACTS_STOP_ON_TOP_SKIP=<n>`: pass the first n creations, stop at
//! creation n+1. `PERRY_FACTS_STOP_ON_TOP_CLASS=<letters of S, A, M>`
//! (default all).
//! The process exits with code 86 after printing one block to stderr.
//! Off: `Model::stop_top` is `None`, and nothing of this runs.

use crate::model::{Cons, Model, NodeId, Tok, NO_NODE, TOP};
use crate::pts::FxHashMap;
use std::panic::Location;
use std::sync::Mutex;

pub const NOPOS: (u32, u32) = (u32::MAX, u32::MAX);

#[derive(Default)]
pub struct StopTop {
    skip: u64,
    seen: u64,
    /// bit 1: stop on S, bit 2: stop on A, bit 4: stop on M.
    class: u8,
    capture: bool,
    done: bool,
    pub(crate) paths: Vec<String>,
    /// The (module, byte offset) generation is at: the innermost expression
    /// that carries a source offset.
    pub(crate) gen_pos: (u32, u32),
    pub(crate) node_pos: Vec<(u32, u32)>,
    /// A node made without a program position (by the solver, after
    /// generation): the node whose rule was running, and that rule.
    pub(crate) node_from: Vec<(NodeId, Option<Cons>)>,
    pub(crate) fn_pos: FxHashMap<u32, (u32, u32)>,
    /// Named ⊤ holders (`Model::top_val` under the switch): why, the solver
    /// line that made it, the program position.
    pub(crate) topvals: FxHashMap<NodeId, (String, &'static Location<'static>, (u32, u32))>,
    /// The rule being applied (`Model::apply_tokens` loops): its node.
    pub(crate) rule: Option<(NodeId, Cons)>,
    /// The program position of the rule being applied (`cons_pos`).
    pub(crate) rule_pos: (u32, u32),
    /// The program position of each attached rule, by (node, index in its
    /// `cons`): the expression generation walks when it attaches it, else
    /// the position of the rule whose application attached it.
    pub(crate) cons_pos: FxHashMap<(NodeId, u32), (u32, u32)>,
    /// A node made outside a program expression: the solver line that made it.
    pub(crate) node_at: FxHashMap<NodeId, &'static Location<'static>>,
    /// Nodes made with no program position, by solver line (a bug each).
    pub(crate) nopos: FxHashMap<&'static Location<'static>, u64>,
    /// Escapes in progress: (token, reason id, node the rule ran on).
    pub(crate) esc: Vec<(Tok, u32, NodeId)>,
    /// The program position of the protocol request `Model::implicit` runs
    /// (the console.* call that asked for the inspection): what a node or
    /// rule made inside it carries when no rule position is known.
    pub(crate) req_pos: (u32, u32),
    /// The request position of each `implicit-throw` node (the first
    /// request of a shared touch), which the touches it runs inherit.
    pub(crate) throw_pos: FxHashMap<NodeId, (u32, u32)>,
    /// The origin `Model::add_top` names while it adds ⊤.
    pub(crate) why: Option<String>,
}

static CAPTURE: Mutex<Option<Vec<String>>> = Mutex::new(None);

/// Tests: run the switch without exiting; `take_captured` returns the block.
pub fn capture_on() {
    *CAPTURE.lock().unwrap_or_else(|e| e.into_inner()) = Some(Vec::new());
}

pub fn take_captured() -> Vec<String> {
    CAPTURE
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .take()
        .unwrap_or_default()
}

impl StopTop {
    pub(crate) fn from_env(paths: Vec<String>) -> Option<Box<StopTop>> {
        let capture = CAPTURE.lock().unwrap_or_else(|e| e.into_inner()).is_some();
        if !capture && std::env::var("PERRY_FACTS_STOP_ON_TOP").ok().as_deref() != Some("1") {
            return None;
        }
        let skip = std::env::var("PERRY_FACTS_STOP_ON_TOP_SKIP")
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(0);
        let class = match std::env::var("PERRY_FACTS_STOP_ON_TOP_CLASS") {
            Ok(v) if !v.is_empty() => {
                let mut c = 0u8;
                for ch in v.chars() {
                    c |= match ch {
                        'S' => 1,
                        'A' => 2,
                        'M' => 4,
                        _ => {
                            eprintln!(
                                "PERRY_FACTS_STOP_ON_TOP_CLASS: letters S, A, M only (got {v:?})"
                            );
                            std::process::exit(2);
                        }
                    };
                }
                c
            }
            _ => 7,
        };
        Some(Box::new(StopTop {
            skip,
            class,
            capture,
            paths,
            gen_pos: NOPOS,
            rule_pos: NOPOS,
            req_pos: NOPOS,
            ..Default::default()
        }))
    }

    /// Rule `c` (index `ci` of node `n`'s rules) starts to apply: the rule
    /// and its position before it, to restore after.
    pub(crate) fn enter_rule(
        &mut self,
        n: NodeId,
        c: Cons,
        ci: u32,
    ) -> (Option<(NodeId, Cons)>, (u32, u32)) {
        let p = self.cons_pos.get(&(n, ci)).copied().unwrap_or(NOPOS);
        (
            self.rule.replace((n, c)),
            std::mem::replace(&mut self.rule_pos, p),
        )
    }

    /// The program position of what runs now: the expression generation
    /// walks, else the expression that attached the rule being applied.
    pub(crate) fn cur_pos(&self) -> (u32, u32) {
        if self.gen_pos != NOPOS {
            self.gen_pos
        } else if self.rule.is_some() && self.rule_pos != NOPOS {
            self.rule_pos
        } else {
            self.req_pos
        }
    }
}

/// The byte offset an expression carries (perry-hir records one on calls,
/// `new`, property and index accesses).
pub(crate) fn expr_offset(e: &perry_hir::Expr) -> Option<u32> {
    use perry_hir::Expr as E;
    let o = match e {
        E::Call { byte_offset, .. }
        | E::CallSpread { byte_offset, .. }
        | E::New { byte_offset, .. }
        | E::PropertyGet { byte_offset, .. }
        | E::PropertySet { byte_offset, .. }
        | E::PropertyUpdate { byte_offset, .. }
        | E::IndexGet { byte_offset, .. }
        | E::IndexSet { byte_offset, .. }
        | E::IndexUpdate { byte_offset, .. } => *byte_offset,
        _ => return None,
    };
    (o != 0).then_some(o)
}

impl Model {
    /// ⊤ just arrived in `n` (`cur_src`/`cur_why`: what sent it; `at`: the
    /// solver line). Decides propagation / S / A; stops at a counted
    /// creation.
    pub(crate) fn top_created(&mut self, n: NodeId, at: &'static Location<'static>) {
        let Some(s) = self.stop_top.as_ref() else {
            return;
        };
        // A ⊤ holder (`TOPVAL`, `top_val`) is no program value.
        if s.done
            || n == self.top_tok_node
            || self.labels[n as usize] == "TOPVAL"
            || s.topvals.contains_key(&n)
        {
            return;
        }
        let src = self.cur_src;
        let why = self.cur_why;
        let src_ok = src != NO_NODE && (src as usize) < self.nodes.len();
        let src_top = src_ok && self.nodes[src as usize].pts.contains_tok(TOP);
        let rule = s.rule.filter(|&(rn, _)| rn == src);
        // The rule that runs, also when its body sends from another node.
        let rule_any = s.rule;
        let esc = s.esc.last().copied();
        let wtext = if why != u32::MAX {
            Some(self.reason_text(why))
        } else {
            s.why.clone()
        };
        // (class, rule name, trigger)
        let (class, rule_name, trigger): (u8, String, Option<String>) =
            if src_ok && (src == self.top_tok_node || s.topvals.contains_key(&src)) {
                let w = match s.topvals.get(&src) {
                    Some((w, l, p)) => format!(
                        "named unknown value \"{w}\" (made at {}:{}; program position {})",
                        l.file(),
                        l.line(),
                        self.pos_text(*p)
                    ),
                    None => "the shared unknown value (no named origin)".into(),
                };
                (1, w, None)
            } else if let Some((t, r, sink)) = esc {
                let tr = format!(
                    "escape of {} into unknown code: {}; the rule ran on node {}",
                    self.tok_name(t),
                    self.reason_text(r),
                    self.node_words(sink)
                );
                let rn = match rule {
                    Some((_, c)) => format!("escape ({})", c.kind_name()),
                    None => "escape".into(),
                };
                let rn = match &wtext {
                    Some(w) => format!("{rn}: {w}"),
                    None => rn,
                };
                (2, rn, Some(tr))
            } else if let Some(w) = wtext {
                let w = match rule {
                    Some((_, c)) => format!("{w} [rule {}]", c.kind_name()),
                    None => w,
                };
                if src_top {
                    (
                        2,
                        w,
                        Some(format!("already unknown: {}", self.node_words(src))),
                    )
                } else {
                    (1, w, None)
                }
            } else if let Some((_, c)) = rule {
                if src_top {
                    (
                        2,
                        format!("rule {} applied to a node that holds ⊤", c.kind_name()),
                        Some(format!("already unknown: {}", self.node_words(src))),
                    )
                } else {
                    (
                        1,
                        format!("rule {} (input holds no ⊤)", c.kind_name()),
                        Some(format!("rule input: {}", self.node_words(src))),
                    )
                }
            } else if src_top {
                // An edge copy from a node that holds ⊤: propagation.
                return;
            } else if src == NO_NODE {
                (1, "direct ⊤ token".into(), None)
            } else {
                (
                    1,
                    "⊤ sent from a node without ⊤ (no rule active)".into(),
                    Some(format!("sender: {}", self.node_words(src))),
                )
            };
        let s = self.stop_top.as_mut().unwrap();
        if s.class & class == 0 {
            return;
        }
        s.seen += 1;
        if s.seen <= s.skip {
            return;
        }
        s.done = true;
        let capture = s.capture;
        let mut b = String::new();
        let cl = if class == 1 {
            "S (seed)"
        } else {
            "A (amplifier)"
        };
        b.push_str(&format!(
            "TOP CREATED #{} class={cl}\n  rule:    {rule_name}\n  solver:  {}:{}\n",
            self.stop_top.as_ref().unwrap().seen,
            at.file(),
            at.line()
        ));
        b.push_str(&format!("  value:   {}\n", self.node_words(n)));
        self.push_where(&mut b, n);
        if let Some(t) = trigger {
            b.push_str(&format!("  cause:   {t}\n"));
        }
        // The node(s) the rule was applied to, each with its own origin.
        let mut inputs: Vec<NodeId> = Vec::new();
        if src_ok {
            inputs.push(src);
        }
        if let Some((_, _, sink)) = esc {
            if sink != NO_NODE && !inputs.contains(&sink) {
                inputs.push(sink);
            }
        }
        if let Some((rn, _)) = rule_any {
            if rn != NO_NODE && (rn as usize) < self.nodes.len() && !inputs.contains(&rn) {
                inputs.insert(0, rn);
            }
        }
        if inputs.is_empty() {
            b.push_str("  input:   none (the rule adds ⊤ directly, no node is read)\n");
        }
        for i in inputs {
            b.push_str(&format!("  input:   {}\n", self.input_words(i)));
        }
        if let Some((rn, c)) = rule_any {
            b.push_str(&format!("  context: constraint {:?} on node {rn}\n", c));
        }
        self.finish_block(b, capture);
    }

    fn finish_block(&self, b: String, capture: bool) {
        if capture {
            if let Some(v) = CAPTURE.lock().unwrap_or_else(|e| e.into_inner()).as_mut() {
                v.push(b);
            }
            return;
        }
        if let Some(s) = self.stop_top.as_ref() {
            let mut v: Vec<_> = s
                .nopos
                .iter()
                .map(|(l, &k)| (k, l.file(), l.line()))
                .collect();
            v.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.cmp(b.1)).then(a.2.cmp(&b.2)));
            for (k, f, l) in v {
                eprintln!("TOPSTOP nopos {f}:{l} {k}");
            }
        }
        eprint!("{b}");
        std::process::exit(86);
    }

    fn reason_text(&self, r: u32) -> String {
        self.reasons
            .get(r as usize)
            .map(|s| s.replace(['\n', '\r'], " "))
            .unwrap_or_else(|| format!("reason #{r}"))
    }

    /// The program position of a node: its own (generation), else its
    /// function's first position.
    fn node_program_pos(&self, n: NodeId) -> (u32, u32) {
        let Some(s) = self.stop_top.as_ref() else {
            return NOPOS;
        };
        if s.node_at.contains_key(&n) {
            return s.node_pos.get(n as usize).copied().unwrap_or(NOPOS);
        }
        let f = self
            .node_fn
            .get(n as usize)
            .copied()
            .unwrap_or(crate::model::NO_FN);
        if f != crate::model::NO_FN {
            let fi = &self.funcs[f as usize];
            if fi.params.contains(&n) || fi.this == n || fi.ret == n {
                if let Some(&p) = s.fn_pos.get(&(f as u32)) {
                    return p;
                }
            }
        }
        if let Some((t, _)) = self.fields.iter().find(|(_, &v)| v == n).map(|(k, _)| *k) {
            if let crate::model::TokKind::Func(f, _) = self.toks[t as usize] {
                if let Some(&p) = s.fn_pos.get(&(f as u32)) {
                    return p;
                }
            }
        }
        s.node_pos.get(n as usize).copied().unwrap_or(NOPOS)
    }

    /// `where:` and `source:` of node `n` (its program origin), and the
    /// steps that led there (`via:`) when the solver made it.
    fn push_where(&self, b: &mut String, n: NodeId) {
        let (p, steps) = self.program_origin(n);
        b.push_str(&format!("  where:   {}\n", self.pos_text(p)));
        if let Some(l) = self.source_line(p) {
            b.push_str(&format!("  source:  {l}\n"));
        }
        let k = steps.len();
        for (i, st) in steps.into_iter().enumerate() {
            let all = std::env::var_os("PERRY_TOPSTOP_VIAHOLD").is_some();
            if !all && i == 3 && k > 4 {
                b.push_str(&format!("  via:     ... {} more steps\n", k - 4));
            }
            if all || i < 3 || i + 1 == k {
                b.push_str(&format!("  via:     {st}\n"));
            }
        }
    }

    /// The program position of `n`, recorded when it was made, and the rule
    /// applications that made it (`via:`), back to a node of generation.
    pub(crate) fn program_origin(&self, n: NodeId) -> ((u32, u32), Vec<String>) {
        let mut steps = Vec::new();
        let Some(s) = self.stop_top.as_ref() else {
            return (NOPOS, steps);
        };
        let p = self.node_program_pos(n);
        let mut cur = n;
        for _ in 0..256 {
            let Some(&(from, c)) = s.node_from.get(cur as usize) else {
                break;
            };
            if from == NO_NODE || from == cur || from as usize >= self.nodes.len() {
                break;
            }
            let r = match c {
                Some(c) => format!("rule {}", c.kind_name()),
                None => "the sender".into(),
            };
            steps.push(format!(
                "node {cur} `{}` was made by the solver applying {r} on node {from} `{}`",
                self.labels[cur as usize], self.labels[from as usize]
            ));
            if std::env::var_os("PERRY_TOPSTOP_VIAHOLD").is_some()
                && cur > 0
                && self.labels[cur as usize - 1] == "implicit-recv"
            {
                let r = cur - 1;
                let ts = self.nodes[r as usize].pts.to_sorted();
                let w: Vec<String> = ts.iter().take(3).map(|&t| self.tok_words(t)).collect();
                let lst = steps.pop().unwrap();
                steps.push(format!(
                    "{lst} [recv {r} holds {} toks: {}]",
                    ts.len(),
                    w.join("; ")
                ));
            }
            cur = from;
        }
        if p == NOPOS {
            let at = s
                .node_at
                .get(&n)
                .map_or_else(|| "?".to_string(), |l| format!("{}:{}", l.file(), l.line()));
            steps.push(format!(
                "BUG: node {n} `{}` was made with no program position (at {at}): its creation site must record one",
                self.labels[n as usize]
            ));
        }
        (p, steps)
    }

    /// An input node: what it is, its program origin, what it holds.
    fn input_words(&self, n: NodeId) -> String {
        if n == NO_NODE || n as usize >= self.nodes.len() {
            return "(none)".into();
        }
        let label = self.labels[n as usize];
        let (p, steps) = self.program_origin(n);
        let toks = self.nodes[n as usize].pts.to_sorted();
        let mut held: Vec<String> = toks.iter().take(6).map(|&t| self.tok_words(t)).collect();
        if toks.len() > 6 {
            held.push(format!("{} more", toks.len() - 6));
        }
        let prim = self.nodes[n as usize].prim;
        format!(
            "node {n} `{label}` ({}), from {}{}; holds [{}]{}",
            self.node_words_short(n),
            self.pos_text(p),
            if steps.is_empty() {
                String::new()
            } else {
                format!(
                    " (made by the solver, {} steps from a program node)",
                    steps.len()
                )
            },
            held.join("; "),
            if prim != 0 {
                format!(" + primitives {prim:#x}")
            } else {
                String::new()
            }
        )
    }

    fn node_words_short(&self, n: NodeId) -> String {
        let w = self.node_words(n);
        match w.find(", holds [") {
            Some(i) => w[..i].to_string(),
            None => w,
        }
    }

    /// A token in words: class X, function Y at file:line, builtin Z.
    pub(crate) fn tok_words(&self, t: Tok) -> String {
        use crate::model::TokKind as K;
        match &self.toks[t as usize] {
            K::Top => "⊤ (unknown)".into(),
            K::Func(f, _) => {
                let p = self
                    .stop_top
                    .as_ref()
                    .and_then(|s| s.fn_pos.get(f).copied())
                    .unwrap_or(NOPOS);
                format!(
                    "function `{}` at {}",
                    self.funcs[*f as usize].name,
                    self.pos_text(p)
                )
            }
            K::ClassObj(tm) => format!("class {}", self.tmpls[*tm as usize].display),
            K::Proto(tm) => format!("prototype of class {}", self.tmpls[*tm as usize].display),
            K::Obj(tm, _) if self.param_marker.contains_key(&t) => {
                let (f, i) = self.param_marker[&t];
                format!(
                    "marker {} (parameter #{} of `{}`)",
                    self.tmpls[*tm as usize].display,
                    i + 1,
                    self.funcs[f as usize].name
                )
            }
            K::Obj(tm, _) => format!("object {}", self.tmpls[*tm as usize].display),
            K::Builtin(b) => format!("builtin {b:?}"),
            K::BuiltinFn(_) => format!(
                "builtin function {}",
                self.tok_name(t).trim_start_matches("builtin-fn:")
            ),
            K::ObjectProto => "Object.prototype".into(),
            _ => self.tok_name(t),
        }
    }

    /// Class M: a marker that reads as ⊤ is made (`what`: the rule), or a
    /// token is marked escaped so that beside ⊤ it is that ⊤.
    pub(crate) fn top_marker_created(
        &mut self,
        t: Tok,
        what: &str,
        at: &'static Location<'static>,
    ) {
        let Some(s) = self.stop_top.as_mut() else {
            return;
        };
        if s.done || s.class & 4 == 0 {
            return;
        }
        s.seen += 1;
        if s.seen <= s.skip {
            return;
        }
        s.done = true;
        let (seen, capture, rule, esc) = (s.seen, s.capture, s.rule, s.esc.last().copied());
        let mut b = format!(
            "TOP CREATED #{seen} class=M (marker that reads as ⊤)\n  rule:    {what}\n  solver:  {}:{}\n  value:   token {t}: {}\n",
            at.file(),
            at.line(),
            self.tok_words(t)
        );
        // Where: a parameter marker's function; else the node the running
        // rule (or the escape in progress) works on.
        let mut placed = false;
        if let Some(&(f, _)) = self.param_marker.get(&t) {
            if let Some(&p) = self.stop_top.as_ref().unwrap().fn_pos.get(&(f as u32)) {
                b.push_str(&format!(
                    "  where:   {} (first program position of the parameter's function)\n",
                    self.pos_text(p)
                ));
                if let Some(l) = self.source_line(p) {
                    b.push_str(&format!("  source:  {l}\n"));
                }
                placed = true;
            }
        }
        let mut inputs: Vec<NodeId> = Vec::new();
        if let Some((rn, _)) = rule {
            inputs.push(rn);
        }
        if let Some((et, r, sink)) = esc {
            b.push_str(&format!(
                "  cause:   escape of {} into unknown code: {}\n",
                self.tok_words(et),
                self.reason_text(r)
            ));
            if sink != NO_NODE && !inputs.contains(&sink) {
                inputs.push(sink);
            }
        }
        if inputs.is_empty()
            && self.cur_src != NO_NODE
            && (self.cur_src as usize) < self.nodes.len()
        {
            inputs.push(self.cur_src);
        }
        let rule_pos = self.stop_top.as_ref().unwrap().rule_pos;
        if !placed && rule.is_some() && rule_pos != NOPOS {
            b.push_str(&format!(
                "  where:   {} (the expression that attached the running rule)\n",
                self.pos_text(rule_pos)
            ));
            if let Some(l) = self.source_line(rule_pos) {
                b.push_str(&format!("  source:  {l}\n"));
            }
            placed = true;
        }
        if !placed {
            match inputs.first() {
                Some(&i) => {
                    b.push_str("  (where: the node the rule works on, the construct that made the solver mark it)\n");
                    self.push_where(&mut b, i);
                }
                None => b.push_str(
                    "  where:   (no program position: no rule, escape or sender is active)\n",
                ),
            }
        }
        for i in inputs {
            b.push_str(&format!("  input:   {}\n", self.input_words(i)));
        }
        if let Some((rn, c)) = rule {
            b.push_str(&format!("  context: constraint {:?} on node {rn}\n", c));
        }
        self.finish_block(b, capture);
    }

    pub(crate) fn pos_text(&self, p: (u32, u32)) -> String {
        let Some(s) = self.stop_top.as_ref() else {
            return "?".into();
        };
        if p == NOPOS {
            return "(no program position recorded)".into();
        }
        let path = s
            .paths
            .get(p.0 as usize)
            .cloned()
            .unwrap_or_else(|| format!("module#{}", p.0));
        match std::fs::read(&path) {
            Ok(src) => {
                let o = (p.1 as usize).min(src.len());
                let line = 1 + src[..o].iter().filter(|&&c| c == b'\n').count();
                let ls = src[..o]
                    .iter()
                    .rposition(|&c| c == b'\n')
                    .map_or(0, |i| i + 1);
                format!("{path}:{line}:{}", 1 + o - ls)
            }
            Err(_) => format!("{path} @byte {}", p.1),
        }
    }

    fn source_line(&self, p: (u32, u32)) -> Option<String> {
        let s = self.stop_top.as_ref()?;
        if p == NOPOS {
            return None;
        }
        let src = std::fs::read(s.paths.get(p.0 as usize)?).ok()?;
        let o = (p.1 as usize).min(src.len());
        let ls = src[..o]
            .iter()
            .rposition(|&c| c == b'\n')
            .map_or(0, |i| i + 1);
        let le = src[o..]
            .iter()
            .position(|&c| c == b'\n')
            .map_or(src.len(), |i| o + i);
        let t = String::from_utf8_lossy(&src[ls..le]).trim().to_string();
        Some(t.chars().take(160).collect())
    }

    /// What a node is for, in words.
    pub(crate) fn node_words(&self, n: NodeId) -> String {
        if n == NO_NODE || n as usize >= self.nodes.len() {
            return "(none)".into();
        }
        let label = self.labels[n as usize];
        let f = self
            .node_fn
            .get(n as usize)
            .copied()
            .unwrap_or(crate::model::NO_FN);
        let fname = |f: crate::model::FnIdx| {
            let fi = &self.funcs[f as usize];
            let m = self
                .stop_top
                .as_ref()
                .and_then(|s| s.paths.get(fi.module as usize).cloned())
                .unwrap_or_default();
            format!("`{}` ({})", fi.name, m)
        };
        for (fi_idx, fi) in self.funcs.iter().enumerate() {
            if let Some(i) = fi.params.iter().position(|&p| p == n) {
                return format!("parameter #{} of function {}", i + 1, fname(fi_idx as _));
            }
            if fi.this == n {
                return format!("`this` of function {}", fname(fi_idx as _));
            }
            if fi.ret == n {
                return format!("return value of function {}", fname(fi_idx as _));
            }
            if fi.args_all == n {
                return format!("`arguments` of function {}", fname(fi_idx as _));
            }
        }
        if let Some(c) = self.calls.iter().position(|c| c.ret == n) {
            let site = &self.calls[c];
            return format!(
                "result of call #{c} (args {}){}",
                site.args.len(),
                if f != crate::model::NO_FN {
                    format!(" in function {}", fname(f))
                } else {
                    String::new()
                }
            );
        }
        if let Some(c) = self.calls.iter().position(|c| c.region == n) {
            return format!("what call #{c} may throw (its throw region)");
        }
        if let Some((t, a)) = self.fields.iter().find(|(_, &v)| v == n).map(|(k, _)| *k) {
            return format!("field `{}` of {}", self.atoms[a as usize], self.tok_name(t));
        }
        let pts: Vec<String> = self.nodes[n as usize]
            .pts
            .to_sorted()
            .into_iter()
            .take(4)
            .map(|t| self.tok_name(t))
            .collect();
        format!(
            "node {n} label `{label}`{}, holds [{}]",
            if f != crate::model::NO_FN {
                format!(" in function {}", fname(f))
            } else {
                String::new()
            },
            pts.join(", ")
        )
    }
}
