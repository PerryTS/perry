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
//! `PERRY_FACTS_STOP_ON_TOP_SKIP=<n>`: pass the first n creations, stop at
//! creation n+1. `PERRY_FACTS_STOP_ON_TOP_CLASS=S|A|SA` (default SA).
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
    /// bit 1: stop on S, bit 2: stop on A.
    class: u8,
    capture: bool,
    done: bool,
    pub(crate) paths: Vec<String>,
    /// The (module, byte offset) generation is at: the innermost expression
    /// that carries a source offset.
    pub(crate) gen_pos: (u32, u32),
    pub(crate) node_pos: Vec<(u32, u32)>,
    pub(crate) fn_pos: FxHashMap<u32, (u32, u32)>,
    /// Named ⊤ holders (`Model::top_val` under the switch): why, the solver
    /// line that made it, the program position.
    pub(crate) topvals: FxHashMap<NodeId, (String, &'static Location<'static>, (u32, u32))>,
    /// The rule being applied (`Model::apply_tokens` loops): its node.
    pub(crate) rule: Option<(NodeId, Cons)>,
    /// Escapes in progress: (token, reason id, node the rule ran on).
    pub(crate) esc: Vec<(Tok, u32, NodeId)>,
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
        let class = match std::env::var("PERRY_FACTS_STOP_ON_TOP_CLASS").ok().as_deref() {
            Some("S") => 1,
            Some("A") => 2,
            _ => 3,
        };
        Some(Box::new(StopTop {
            skip,
            class,
            capture,
            paths,
            gen_pos: NOPOS,
            ..Default::default()
        }))
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
        if s.done || n == self.top_tok_node || self.labels[n as usize] == "TOPVAL" || s.topvals.contains_key(&n) {
            return;
        }
        let src = self.cur_src;
        let why = self.cur_why;
        let src_ok = src != NO_NODE && (src as usize) < self.nodes.len();
        let src_top = src_ok && self.nodes[src as usize].pts.contains_tok(TOP);
        let rule = s.rule.filter(|&(rn, _)| rn == src);
        let esc = s.esc.last().copied();
        let wtext = if why != u32::MAX { Some(self.reason_text(why)) } else { s.why.clone() };
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
                    (2, w, Some(format!("already unknown: {}", self.node_words(src))))
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
        let cl = if class == 1 { "S (seed)" } else { "A (amplifier)" };
        b.push_str(&format!(
            "TOP CREATED #{} class={cl}\n  rule:    {rule_name}\n  solver:  {}:{}\n",
            self.stop_top.as_ref().unwrap().seen,
            at.file(),
            at.line()
        ));
        b.push_str(&format!("  value:   {}\n", self.node_words(n)));
        let p = self.node_program_pos(n);
        b.push_str(&format!("  where:   {}\n", self.pos_text(p)));
        if let Some(l) = self.source_line(p) {
            b.push_str(&format!("  source:  {l}\n"));
        }
        if let Some(t) = trigger {
            b.push_str(&format!("  cause:   {t}\n"));
        }
        if let Some((rn, c)) = rule {
            b.push_str(&format!("  context: constraint {:?} on node {rn}\n", c));
        }
        if capture {
            if let Some(v) = CAPTURE.lock().unwrap_or_else(|e| e.into_inner()).as_mut() {
                v.push(b);
            }
            return;
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
        let f = self.node_fn.get(n as usize).copied().unwrap_or(crate::model::NO_FN);
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

    fn pos_text(&self, p: (u32, u32)) -> String {
        let Some(s) = self.stop_top.as_ref() else {
            return "?".into();
        };
        if p == NOPOS {
            return "(no program position: made by the solver outside generation)".into();
        }
        let path = s.paths.get(p.0 as usize).cloned().unwrap_or_else(|| format!("module#{}", p.0));
        match std::fs::read(&path) {
            Ok(src) => {
                let o = (p.1 as usize).min(src.len());
                let line = 1 + src[..o].iter().filter(|&&c| c == b'\n').count();
                let ls = src[..o].iter().rposition(|&c| c == b'\n').map_or(0, |i| i + 1);
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
        let ls = src[..o].iter().rposition(|&c| c == b'\n').map_or(0, |i| i + 1);
        let le = src[o..].iter().position(|&c| c == b'\n').map_or(src.len(), |i| o + i);
        let t = String::from_utf8_lossy(&src[ls..le]).trim().to_string();
        Some(t.chars().take(160).collect())
    }

    /// What a node is for, in words.
    pub(crate) fn node_words(&self, n: NodeId) -> String {
        if n == NO_NODE || n as usize >= self.nodes.len() {
            return "(none)".into();
        }
        let label = self.labels[n as usize];
        let f = self.node_fn.get(n as usize).copied().unwrap_or(crate::model::NO_FN);
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
                if f != crate::model::NO_FN { format!(" in function {}", fname(f)) } else { String::new() }
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
            if f != crate::model::NO_FN { format!(" in function {}", fname(f)) } else { String::new() },
            pts.join(", ")
        )
    }
}
