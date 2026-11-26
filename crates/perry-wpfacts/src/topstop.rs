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
//!
//! `PERRY_FACTS_STOP_ON_TOP_LIST=<n>` (with `PERRY_FACTS_STOP_ON_TOP=1`):
//! the seed census, for ordering work only (`Census`). No creation stops
//! the solve. The first creation of each seed key (solver line, program
//! position) prints one `CENSUS-SEED` line; every first ⊤ arrival in a node
//! is booked to the seed its ⊤ came from (an S creation books itself, a
//! propagation or an A creation books the seed of its ⊤ input). At `n`
//! distinct seeds, or past `PERRY_FACTS_STOP_ON_TOP_WORK=<w>` work when that
//! is set (keys past `n` are then booked together as `later`), or at the end
//! of the solve, one `CENSUS-REACH` line per seed gives the nodes booked to
//! it, and the process exits with code 86. A seed whose rule input already
//! held ⊤ is `down=#k`: a consequence of seed k, not an independent root.

use crate::model::{CallId, Cons, Model, NodeId, Tok, NO_NODE, TOP};
use crate::pts::FxHashMap;
use std::panic::Location;
use std::sync::Mutex;

pub const NOPOS: (u32, u32) = (u32::MAX, u32::MAX);
/// Module tag of a shared operation's position: `(SHARED, id)` names
/// `StopTop::shared[id]`, an operation made once for every caller (an
/// implicit-protocol touch per (value, protocol)); its callers carry the
/// program positions.
pub const SHARED: u32 = u32::MAX - 1;

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
    /// Operations attached with no program position, by (solver line, kind
    /// of operation, solver context): each one a bug of that line.
    pub(crate) nopos: FxHashMap<(&'static Location<'static>, &'static str, &'static str), u64>,
    /// The solver line that attached a rule with no position, by (node, index).
    pub(crate) cons_at: FxHashMap<(NodeId, u32), &'static Location<'static>>,
    /// The program position of each copy edge, keyed `a << 32 | b`.
    pub(crate) edge_pos: FxHashMap<u64, (u32, u32)>,
    /// A copy edge made with no program position, keyed as `edge_pos`: the
    /// solver line that made it, the rule being applied and the phase.
    pub(crate) edge_at: FxHashMap<u64, (&'static Location<'static>, Option<(NodeId, Cons)>, &'static str)>,
    /// Shared operations: what each is, and its callers' positions (program
    /// positions or `(SHARED, id)` of the shared operation that asked).
    pub(crate) shared: Vec<(String, Vec<(u32, u32)>)>,
    pub(crate) shared_key: FxHashMap<(Tok, crate::model::Protocol), u32>,
    /// The shared operation being built (`Model::implicit`), else NOPOS.
    pub(crate) shared_pos: (u32, u32),
    /// The rule's index in its node's rules.
    pub(crate) rule_ci: u32,
    /// `add_top` / `top_val` count their own operation.
    pub(crate) quiet_top: bool,
    /// The HIR expression variant generation walks (`variant`).
    pub(crate) gen_kind: &'static str,
    /// Generation operations without a position, by HIR expression variant
    /// (the variant that carries no source offset).
    pub(crate) gen_gap: FxHashMap<&'static str, u64>,
    /// HIR operations of the site-identity kinds whose own offset is 0
    /// (`unlocated_kind`), by kind: the lowering gaps left to close.
    pub(crate) self_gap: FxHashMap<&'static str, u64>,
    /// The solver phase running (`Model::prof_mark`).
    pub(crate) phase: &'static str,
    /// Escapes in progress: (token, reason id, node the rule ran on).
    pub(crate) esc: Vec<(Tok, u32, NodeId)>,
    /// Native calls in progress: (call, callee token), innermost last.
    pub(crate) natives: Vec<(CallId, Tok)>,
    /// The escape a native hands to unknown code while it raises the
    /// native's throw region (`escape_at`): (token, reason id).
    pub(crate) hand: Option<(Tok, u32)>,
    /// The origin `Model::add_top` names while it adds ⊤.
    pub(crate) why: Option<String>,
    /// `PERRY_TOPSTOP_TOPFROM=1`: the ⊤ chain of an input that holds ⊤.
    topfrom: bool,
    /// Under `topfrom`: the first arrival of ⊤ in each node: its sender
    /// (`cur_src`), the reason named, the rule running, the solver line,
    /// the origin `add_top` names and the escape that runs (if any).
    top_from: FxHashMap<NodeId, (NodeId, u32, Option<(NodeId, Cons)>, &'static Location<'static>, String)>,
    /// `PERRY_FACTS_STOP_ON_TOP_LIST`: the seed census.
    pub(crate) census: Option<Box<Census>>,
    /// Tests (`tests::amp_flow`): every ⊤ creation is recorded here and the
    /// solve goes on (nothing printed, no exit).
    #[cfg(test)]
    pub(crate) flow: Option<Vec<FlowArrival>>,
}

/// One ⊤ creation in flow record mode: its class (1 S, 2 A, 4 M), the node
/// (`NO_NODE` for a marker), the marked token, the rule that ran (its
/// node and constraint), and the solver line.
#[cfg(test)]
#[derive(Clone, Debug)]
pub(crate) struct FlowArrival {
    pub class: u8,
    pub node: NodeId,
    pub tok: Tok,
    pub rule: Option<(NodeId, Cons)>,
    pub what: String,
    pub at: &'static Location<'static>,
}

#[cfg(test)]
thread_local! {
    static FLOW: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

/// Tests: the next solves on this thread record every ⊤ creation
/// (`StopTop::flow`) instead of stopping.
#[cfg(test)]
pub(crate) fn flow_on(on: bool) {
    FLOW.with(|f| f.set(on));
}

thread_local! {
    /// Per thread: a test that captures sees only the blocks of its own solve.
    static CAPTURE: std::cell::RefCell<Option<Vec<String>>> = const { std::cell::RefCell::new(None) };
}

/// Tests: run the switch without exiting; `take_captured` returns the block.
pub fn capture_on() {
    CAPTURE.with_borrow_mut(|c| *c = Some(Vec::new()));
}

pub fn take_captured() -> Vec<String> {
    CENSUS.with(|c| c.set((0, 0)));
    CAPTURE.with_borrow_mut(|c| c.take()).unwrap_or_default()
}

thread_local! {
    /// Tests: the census bounds (distinct seeds, work) of the next captured
    /// solves on this thread; (0, 0) = no census.
    static CENSUS: std::cell::Cell<(u64, u64)> = const { std::cell::Cell::new((0, 0)) };
}

/// Tests: captured solves run the seed census (`Census`) with these bounds;
/// the report is one captured block.
pub fn census_capture_on(seeds: u64, work: u64) {
    CENSUS.with(|c| c.set((seeds, work)));
    capture_on();
}

/// A node's first ⊤ arrival booked to no listed seed: its ⊤ input had no
/// booked arrival (`LOST`), or its seed key came past the listed count
/// (`LATER`).
const C_NONE: u32 = u32::MAX;
const C_LATER: u32 = u32::MAX - 1;
const C_LOST: u32 = u32::MAX - 2;

/// The seed census (`PERRY_FACTS_STOP_ON_TOP_LIST`).
#[derive(Default)]
pub(crate) struct Census {
    /// Distinct seeds to list, and the work bound (0 = none).
    max: u64,
    work: u64,
    seeds: Vec<CensusSeed>,
    by_key: FxHashMap<(&'static str, u32, (u32, u32)), u32>,
    /// By node: the seed its first ⊤ arrival is booked to (`C_*` else).
    root: Vec<u32>,
    /// S creations in all, and those of keys past `max`.
    creations: u64,
    later: u64,
    /// A sink attached by a rule on a ⊤ node: that ⊤'s seed (`census_sink`).
    sink_root: FxHashMap<NodeId, u32>,
    /// `CENSUS-LOST` lines printed (at most 8).
    lost_seen: u32,
    /// The printed lines (kept for a captured report).
    lines: Vec<String>,
}

struct CensusSeed {
    /// Its first creation: work, ordinal among S creations, the seed it is
    /// downstream of (`C_NONE`: independent).
    work: u64,
    seq: u64,
    down: u32,
    creations: u64,
    /// Creations whose rule input held no ⊤.
    indep: u64,
}

pub(crate) type RuleSaved =(Option<(NodeId, Cons)>, (u32, u32), (u32, u32), u32);

impl StopTop {
    pub(crate) fn from_env(paths: Vec<String>) -> Option<Box<StopTop>> {
        #[cfg(test)]
        if FLOW.with(|f| f.get()) {
            return Some(Box::new(StopTop {
                class: 7,
                paths,
                gen_pos: NOPOS,
                rule_pos: NOPOS,
                flow: Some(Vec::new()),
                ..Default::default()
            }));
        }
        let capture = CAPTURE.with_borrow(|c| c.is_some());
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
        let num = |v: &str| std::env::var(v).ok().and_then(|s| s.parse::<f64>().ok());
        let (max, work) = if capture {
            CENSUS.with(|c| c.get())
        } else {
            (
                num("PERRY_FACTS_STOP_ON_TOP_LIST").map_or(0, |v| v as u64),
                num("PERRY_FACTS_STOP_ON_TOP_WORK").map_or(0, |v| v as u64),
            )
        };
        let census = (max != 0).then(|| {
            Box::new(Census {
                max,
                work,
                ..Default::default()
            })
        });
        Some(Box::new(StopTop {
            census,
            skip,
            class,
            capture,
            paths,
            gen_pos: NOPOS,
            rule_pos: NOPOS,
            topfrom: std::env::var_os("PERRY_TOPSTOP_TOPFROM").is_some(),
            shared_pos: NOPOS,
            ..Default::default()
        }))
    }

    /// Rule `c` (index `ci` of node `n`'s rules) starts to apply: the rule,
    /// its position and the shared operation before it, to restore after
    /// (`leave_rule`).
    pub(crate) fn enter_rule(&mut self, n: NodeId, c: Cons, ci: u32) -> RuleSaved {
        let p = self.cons_pos.get(&(n, ci)).copied().unwrap_or(NOPOS);
        (
            self.rule.replace((n, c)),
            std::mem::replace(&mut self.rule_pos, p),
            std::mem::replace(&mut self.shared_pos, NOPOS),
            std::mem::replace(&mut self.rule_ci, ci),
        )
    }

    /// The census is past its work bound.
    pub(crate) fn census_due(&self, work: u64) -> bool {
        self.census.as_ref().is_some_and(|c| c.work != 0 && work > c.work)
    }

    pub(crate) fn leave_rule(&mut self, r: RuleSaved) {
        (self.rule, self.rule_pos, self.shared_pos, self.rule_ci) = r;
    }

    /// A step recorded at program position `p` runs now (a generation step
    /// deferred until summaries exist, an instance's copy of its body's
    /// rule): what it attaches and makes stands at `p`, not at the rule or
    /// expression running when it is replayed. Restore with `replay_done`.
    pub(crate) fn replay_at(&mut self, p: (u32, u32)) -> ((u32, u32), (u32, u32)) {
        (
            std::mem::replace(&mut self.gen_pos, p),
            std::mem::replace(&mut self.rule_pos, p),
        )
    }

    pub(crate) fn replay_done(&mut self, saved: ((u32, u32), (u32, u32))) {
        (self.gen_pos, self.rule_pos) = saved;
    }

    /// The program position of node `n` (`node_pos`).
    pub(crate) fn pos_of(&self, n: NodeId) -> (u32, u32) {
        self.node_pos.get(n as usize).copied().unwrap_or(NOPOS)
    }

    /// The program position of what runs now: the shared operation being
    /// built, else the expression that attached the rule being applied
    /// (also when generation replays it), else the expression generation
    /// walks.
    pub(crate) fn cur_pos(&self) -> (u32, u32) {
        if self.shared_pos != NOPOS {
            self.shared_pos
        } else if self.rule.is_some() {
            self.rule_pos
        } else {
            self.gen_pos
        }
    }

    /// The shared operation of `key` (made at its first request), with the
    /// caller `p` added.
    pub(crate) fn shared_call(
        &mut self,
        key: (Tok, crate::model::Protocol),
        what: impl FnOnce() -> String,
        p: (u32, u32),
    ) -> u32 {
        let id = match self.shared_key.get(&key) {
            Some(&id) => id,
            None => {
                let id = self.shared.len() as u32;
                self.shared.push((what(), Vec::new()));
                self.shared_key.insert(key, id);
                id
            }
        };
        let cs = &mut self.shared[id as usize].1;
        if !cs.contains(&p) {
            cs.push(p);
        }
        id
    }
}

/// The variant name of an HIR expression (its Debug text up to the first
/// non-identifier byte), interned.
pub(crate) fn variant(e: &impl std::fmt::Debug, pre: &str) -> &'static str {
    use std::fmt::Write;
    struct W([u8; 40], usize);
    impl Write for W {
        fn write_str(&mut self, s: &str) -> std::fmt::Result {
            for &c in s.as_bytes() {
                if !(c.is_ascii_alphanumeric() || c == b'_') || self.1 == self.0.len() {
                    return Err(std::fmt::Error);
                }
                self.0[self.1] = c;
                self.1 += 1;
            }
            Ok(())
        }
    }
    let mut w = W([0; 40], 0);
    let _ = write!(w, "{e:?}");
    let name = format!("{pre}{}", std::str::from_utf8(&w.0[..w.1]).unwrap_or("?"));
    static NAMES: Mutex<Vec<&'static str>> = Mutex::new(Vec::new());
    let mut v = NAMES.lock().unwrap_or_else(|e| e.into_inner());
    if let Some(&n) = v.iter().find(|&&n| n == name) {
        return n;
    }
    let n: &'static str = Box::leak(name.into_boxed_str());
    v.push(n);
    n
}

/// The byte offset an expression carries (0: built by the compiler with
/// no source position).
/// The site-identity kind of `e` when its own source offset is 0 (the
/// census of `nopos-self`); `None` when located or of another kind.
pub(crate) fn unlocated_kind(e: &perry_hir::Expr) -> Option<&'static str> {
    use perry_hir::Expr;
    Some(match e {
        Expr::Call { byte_offset: 0, .. } if perry_hir::tdz_check::is_dead_zone_op(e) => "TdzCheck",
        Expr::Call { byte_offset: 0, .. } => "Call",
        Expr::CallSpread { byte_offset: 0, .. } => "CallSpread",
        Expr::PropertyGet { byte_offset: 0, .. } => "PropertyGet",
        Expr::PropertySet { byte_offset: 0, .. } => "PropertySet",
        Expr::PropertyUpdate { byte_offset: 0, .. } => "PropertyUpdate",
        Expr::IndexGet { byte_offset: 0, .. } => "IndexGet",
        Expr::IndexSet { byte_offset: 0, .. } => "IndexSet",
        Expr::IndexUpdate { byte_offset: 0, .. } => "IndexUpdate",
        Expr::IndexGet {
            element_offset: 0, ..
        } => "IndexGet.element",
        Expr::IndexSet {
            element_offset: 0, ..
        } => "IndexSet.element",
        Expr::NativeMethodCall { byte_offset: 0, .. } => "NativeMethodCall",
        Expr::New { byte_offset: 0, .. } => "New",
        Expr::StaticMethodCall { byte_offset: 0, .. } => "StaticMethodCall",
        Expr::ObjectSuperMethodCall { byte_offset: 0, .. } => "ObjectSuperMethodCall",
        _ => return None,
    })
}

/// The site-identity kind of an HIR expression for the census: its variant
/// name, except a dead-zone operation, which is its own kind (`TdzCheck`).
pub(crate) fn expr_kind(e: &perry_hir::Expr) -> &'static str {
    if perry_hir::tdz_check::is_dead_zone_op(e) {
        "TdzCheck"
    } else {
        variant(e, "")
    }
}

pub(crate) fn expr_offset(e: &perry_hir::Expr) -> Option<u32> {
    e.byte_offset().filter(|&o| o != 0)
}

impl Model {
    /// The solver context running now (census of position-less operations).
    pub(crate) fn pos_ctx(&self) -> &'static str {
        let Some(s) = self.stop_top.as_ref() else {
            return "";
        };
        if self.rel_phase == b'G' && s.rule.is_none() {
            "generation"
        } else if s.rule.is_some() {
            "rule"
        } else if !s.esc.is_empty() {
            "escape"
        } else if self.cur_src != NO_NODE {
            "sender"
        } else if s.phase.is_empty() {
            "worklist"
        } else {
            s.phase
        }
    }

    /// Run `f`; the shared operation it opens (`implicit_throw`) ends with it.
    pub(crate) fn shared_scope<R>(&mut self, f: impl FnOnce(&mut Self) -> R) -> R {
        let sp = self.stop_top.as_ref().map(|s| s.shared_pos);
        let r = f(self);
        if let (Some(p), Some(s)) = (sp, self.stop_top.as_mut()) {
            s.shared_pos = p;
        }
        r
    }

    /// An operation of `kind` made at `at` with no program position.
    /// Inside a rule that itself has no position the operation inherits
    /// that rule's bug (`-inherited`), counted apart from the roots.
    pub(crate) fn note_nopos(&mut self, at: &'static Location<'static>, kind: &'static str) {
        let ctx = self.pos_ctx();
        if let Some(s) = self.stop_top.as_mut() {
            let kind = if s.rule.is_some() && s.shared_pos == NOPOS {
                match kind {
                    "cons" => "cons-inherited",
                    "edge" => "edge-inherited",
                    "top" => "top-inherited",
                    "topval" => "topval-inherited",
                    _ => "other-inherited",
                }
            } else {
                if s.rule.is_none()
                    && s.gen_pos == NOPOS
                    && !s.gen_kind.is_empty()
                    && !kind.starts_with("edge")
                {
                    *s.gen_gap.entry(s.gen_kind).or_default() += 1;
                }
                kind
            };
            *s.nopos.entry((at, kind, ctx)).or_default() += 1;
        }
    }

    /// The program positions a shared operation's callers reach (through
    /// the shared operations that asked for it), deduplicated, in order.
    fn shared_program_callers(&self, id: u32) -> Vec<(u32, u32)> {
        let s = self.stop_top.as_ref().unwrap();
        let mut out = Vec::new();
        let mut seen = vec![false; s.shared.len()];
        let mut q = std::collections::VecDeque::from([id]);
        seen[id as usize] = true;
        while let Some(i) = q.pop_front() {
            for &p in &s.shared[i as usize].1 {
                if p.0 == SHARED {
                    if !seen[p.1 as usize] {
                        seen[p.1 as usize] = true;
                        q.push_back(p.1);
                    }
                } else if !out.contains(&p) {
                    out.push(p);
                }
            }
        }
        out
    }

    /// `where:` / `source:` of the operation running at the stop: its
    /// program position; a shared operation's callers; a position-less one
    /// is a bug named by the line that attached it.
    fn push_op_where(&self, b: &mut String, p: (u32, u32), what: &str) {
        let s = self.stop_top.as_ref().unwrap();
        if p == NOPOS {
            let at = s
                .rule
                .and_then(|(rn, _)| s.cons_at.get(&(rn, s.rule_ci)))
                .map_or_else(|| "?".to_string(), |l| format!("{}:{}", l.file(), l.line()));
            b.push_str(&format!(
                "  where:   BUG: {what} has no program position (attached at {at}): its creation site must record one\n"
            ));
            return;
        }
        if p.0 == SHARED {
            let (w, own) = &s.shared[p.1 as usize];
            let cs = self.shared_program_callers(p.1);
            b.push_str(&format!(
                "  where:   shared operation: {w} (made once; {} direct requests, {} program callers)\n",
                own.len(),
                cs.len()
            ));
            for (i, &c) in cs.iter().enumerate() {
                if i == 5 {
                    b.push_str(&format!("  caller:  ... {} more\n", cs.len() - 5));
                    break;
                }
                b.push_str(&format!(
                    "  caller:  {}  {}\n",
                    self.pos_text(c),
                    self.source_line(c).unwrap_or_default()
                ));
            }
            return;
        }
        b.push_str(&format!("  where:   {}\n", self.pos_text(p)));
        if let Some(l) = self.source_line(p) {
            b.push_str(&format!("  source:  {l}\n"));
        }
    }

    /// The copy edges into `n`, with their program positions.
    fn push_fed_by(&self, b: &mut String, n: NodeId) {
        let s = self.stop_top.as_ref().unwrap();
        let mut ps: Vec<(u32, u32)> = Vec::new();
        let mut nopos: Vec<NodeId> = Vec::new();
        let mut k = 0usize;
        for (a, nd) in self.nodes.iter().enumerate() {
            if nd.succ.contains(&n) {
                k += 1;
                let p = s
                    .edge_pos
                    .get(&((a as u64) << 32 | n as u64))
                    .copied()
                    .unwrap_or(NOPOS);
                if p == NOPOS {
                    nopos.push(a as NodeId);
                }
                if !ps.contains(&p) {
                    ps.push(p);
                }
            }
        }
        if k == 0 {
            return;
        }
        b.push_str(&format!(
            "  fed by:  {k} copy edges into node {n}, {} positions\n",
            ps.len()
        ));
        for &p in ps.iter().take(5) {
            b.push_str(&format!(
                "  edge:    {}  {}\n",
                self.pos_text(p),
                self.source_line(p).unwrap_or_default()
            ));
        }
        if ps.len() > 5 {
            b.push_str(&format!("  edge:    ... {} more\n", ps.len() - 5));
        }
        // An edge with no position: where it comes from and the solver
        // line that made it (each one a position bug of that line).
        for &a in nopos.iter().take(5) {
            let (at, rule, phase) = match s.edge_at.get(&((a as u64) << 32 | n as u64)) {
                Some(&(l, r, ph)) => (format!("{}:{}", l.file(), l.line()), r, ph),
                None => ("(not recorded)".to_string(), None, ""),
            };
            b.push_str(&format!(
                "  nopos:   from {} ({} values); made at {at} in phase {phase}{}\n",
                self.node_words(a),
                self.nodes[a as usize].pts.len(),
                rule.map_or(String::new(), |(rn, c)| format!(", rule {c:?} on node {rn}"))
            ));
        }
    }

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
        if s.topfrom {
            let (src, why) = (self.cur_src, self.cur_why);
            if !s.top_from.contains_key(&n) {
                let mut o = s.why.clone().map_or(String::new(), |w| format!(" origin \"{w}\""));
                if let Some((t, r, sink)) = s.esc.last().copied() {
                    o += &format!(
                        " during escape of {} ({}) on node {sink} `{}`",
                        self.tok_name(t),
                        self.reason_text(r),
                        self.labels[sink as usize]
                    );
                }
                let s = self.stop_top.as_mut().unwrap();
                let rule = s.rule;
                s.top_from.insert(n, (src, why, rule, at, o));
            }
        }
        let s = self.stop_top.as_ref().unwrap();
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
                if s.census.is_some() {
                    self.census_arrival(n, 0, src, None, None, at, String::new);
                }
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
        #[cfg(test)]
        if let Some(f) = s.flow.as_mut() {
            let what = match &trigger {
                Some(t) => format!("{rule_name} ({t})"),
                None => rule_name,
            };
            f.push(FlowArrival { class, node: n, tok: TOP, rule: rule_any, what, at });
            return;
        }
        if s.census.is_some() {
            let srcn = if src_ok { src } else { NO_NODE };
            let what = || match &trigger {
                Some(t) => format!("{rule_name} ({t})"),
                None => rule_name.clone(),
            };
            self.census_arrival(n, class, srcn, esc.map(|e| e.2), rule_any.map(|r| r.0), at, what);
            return;
        }
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
        // The operation that made the value: a named unknown value's own
        // position, else the running rule's, else what runs (generation).
        let st = self.stop_top.as_ref().unwrap();
        let op_pos = match st.topvals.get(&src).filter(|_| src_ok) {
            Some(&(_, _, p)) => p,
            None => st.cur_pos(),
        };
        let what = match rule_any {
            Some((rn, c)) => format!("rule {} on node {rn}", c.kind_name()),
            None => "the operation".into(),
        };
        self.push_op_where(&mut b, op_pos, &what);
        self.push_via(&mut b, n);
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
        let first = inputs.first().copied();
        for i in inputs {
            b.push_str(&format!("  input:   {}\n", self.input_words(i)));
        }
        if let Some(i) = first {
            self.push_fed_by(&mut b, i);
        }
        if let Some((rn, c)) = rule_any {
            b.push_str(&format!("  context: constraint {:?} on node {rn}\n", c));
        }
        let st = self.stop_top.as_ref().unwrap();
        for &(c, t) in st.natives.iter().rev() {
            b.push_str(&format!("  native:  call #{c} of {}\n", self.tok_name(t)));
        }
        if let Some((t, r)) = st.hand {
            b.push_str(&format!(
                "  hands:   {} ({})\n",
                self.tok_name(t),
                self.reason_text(r)
            ));
        }
        self.finish_block(b, capture);
    }

    /// The census: node `n` becomes a sink (what reaches it escapes into
    /// unknown code) while a rule runs on a node that holds ⊤ (a call of an
    /// unknown callee): what that escape makes is booked to that ⊤'s seed.
    pub(crate) fn census_sink(&mut self, n: NodeId) {
        let st = self.stop_top.as_ref().unwrap();
        let held = |x: NodeId| {
            x != NO_NODE
                && (x as usize) < self.nodes.len()
                && self.nodes[x as usize].pts.contains_tok(TOP)
        };
        let from = st.rule.map(|r| r.0).filter(|&x| held(x)).or_else(|| Some(self.cur_src).filter(|&x| held(x)));
        let Some(x) = from else {
            return;
        };
        let c = self.stop_top.as_mut().unwrap().census.as_mut().unwrap();
        if let Some(&r) = c.root.get(x as usize).filter(|&&r| r != C_NONE) {
            c.sink_root.entry(n).or_insert(r);
        }
    }

    /// The census books the first ⊤ arrival in `n` (class 0: propagation
    /// from `src`; 1 S; 2 A) and lists a new seed key.
    #[allow(clippy::too_many_arguments)]
    fn census_arrival(
        &mut self,
        n: NodeId,
        class: u8,
        src: NodeId,
        sink: Option<NodeId>,
        rule_node: Option<NodeId>,
        at: &'static Location<'static>,
        what: impl FnOnce() -> String,
    ) {
        let st = self.stop_top.as_ref().unwrap();
        let c = st.census.as_ref().unwrap();
        // The seed of the first input that holds ⊤ (the sender, the escape's
        // sink, the node the rule runs on).
        let up = [src, sink.unwrap_or(NO_NODE), rule_node.unwrap_or(NO_NODE)]
            .into_iter()
            .find(|&x| {
                x != NO_NODE
                    && (x as usize) < self.nodes.len()
                    && self.nodes[x as usize].pts.contains_tok(TOP)
            })
            .map(|x| match c.root.get(x as usize).copied().unwrap_or(C_NONE) {
                C_NONE => (C_LOST, x),
                r => (r, x),
            })
            // An escape with no ⊤ input: the seed of the ⊤ that made its
            // sink (`census_sink`), innermost escape first.
            .or_else(|| {
                st.esc
                    .iter()
                    .rev()
                    .find_map(|&(_, _, k)| c.sink_root.get(&k).map(|&r| (r, k)))
            });
        // The first arrivals booked to no seed: their ⊤ input and its label.
        if let Some((C_LOST, x)) = up {
            if c.lost_seen < 8 {
                let l = format!(
                    "CENSUS-LOST class={class} node {n} `{}` <- node {x} `{}` ({}:{}) work={}",
                    self.labels[n as usize],
                    self.labels[x as usize],
                    at.file(),
                    at.line(),
                    self.work.get()
                );
                let capture = st.capture;
                let c = self.stop_top.as_mut().unwrap().census.as_mut().unwrap();
                c.lost_seen += 1;
                if !capture {
                    eprintln!("{l}");
                }
                c.lines.push(l);
            }
        }
        let st = self.stop_top.as_ref().unwrap();
        let c = st.census.as_ref().unwrap();
        let up = up.map(|u| u.0);
        let book = |c: &mut Census, r: u32| {
            if c.root.len() <= n as usize {
                c.root.resize(n as usize + 1, C_NONE);
            }
            c.root[n as usize] = r;
        };
        if class != 1 && up.is_none() && c.lost_seen < 8 {
            let lb = |x: Option<NodeId>| match x {
                Some(x) if x != NO_NODE && (x as usize) < self.nodes.len() => {
                    format!("{x} `{}`", self.labels[x as usize])
                }
                _ => "-".into(),
            };
            let l = format!(
                "CENSUS-LOST class={class} node {n} `{}` no ⊤ input: src {} sink {} rule-node {} ({}:{}) {}",
                self.labels[n as usize],
                lb(Some(src)),
                lb(sink),
                lb(rule_node),
                at.file(),
                at.line(),
                what().replace(char::is_control, " ")
            );
            let capture = st.capture;
            let c = self.stop_top.as_mut().unwrap().census.as_mut().unwrap();
            c.lost_seen += 1;
            if !capture {
                eprintln!("{l}");
            }
            c.lines.push(l);
            book(c, C_LOST);
            return;
        }
        if class != 1 {
            let c = self.stop_top.as_mut().unwrap().census.as_mut().unwrap();
            book(c, up.unwrap_or(C_LOST));
            return;
        }
        let op_pos = match st.topvals.get(&src).filter(|_| src != NO_NODE) {
            Some(&(_, _, p)) => p,
            None => st.cur_pos(),
        };
        let key = (at.file(), at.line(), op_pos);
        let work = self.work.get();
        let line = if c.by_key.contains_key(&key) || c.seeds.len() as u64 >= c.max {
            None
        } else {
            let one = |t: String| t.replace(char::is_control, " ");
            let down = match up {
                Some(C_LOST) | Some(C_LATER) => " down=?".to_string(),
                Some(r) => format!(" down=#{r}"),
                None => " indep".to_string(),
            };
            let v: String = self.node_words_short(n).chars().take(200).collect();
            Some(format!(
                "CENSUS-SEED #{} seq={} work={work}{down} solver={}:{} site={} | rule: {} | src: {} | value: {}",
                c.seeds.len(),
                c.creations + 1,
                at.file(),
                at.line(),
                self.pos_text(op_pos),
                one(what()),
                one(self.source_line(op_pos).unwrap_or_default()),
                one(v)
            ))
        };
        let capture = self.stop_top.as_ref().unwrap().capture;
        let c = self.stop_top.as_mut().unwrap().census.as_mut().unwrap();
        c.creations += 1;
        let r = if let Some(&id) = c.by_key.get(&key) {
            let sd = &mut c.seeds[id as usize];
            sd.creations += 1;
            sd.indep += up.is_none() as u64;
            id
        } else if let Some(l) = line {
            let id = c.seeds.len() as u32;
            c.by_key.insert(key, id);
            c.seeds.push(CensusSeed {
                work,
                seq: c.creations,
                down: up.unwrap_or(C_NONE),
                creations: 1,
                indep: up.is_none() as u64,
            });
            if !capture {
                eprintln!("{l}");
            }
            c.lines.push(l);
            id
        } else {
            c.later += 1;
            C_LATER
        };
        let c = self.stop_top.as_mut().unwrap().census.as_mut().unwrap();
        book(c, r);
        if c.work == 0 && c.seeds.len() as u64 >= c.max {
            self.census_finish("seed count");
        }
    }

    /// The census report (`CENSUS-REACH` per seed, by reach), then exit 86
    /// (a captured test solve: one block, and the solve goes on unreported).
    pub(crate) fn census_finish(&mut self, why: &str) {
        let Some(s) = self.stop_top.as_mut() else {
            return;
        };
        let Some(c) = s.census.take() else {
            return;
        };
        s.done = true;
        let capture = s.capture;
        let k = c.seeds.len();
        // Per seed: nodes booked to it, of them nodes that run a read,
        // store or method call (`ops`); the same with every downstream seed
        // (`total`).
        let (mut own, mut ops) = (vec![0u64; k + 3], vec![0u64; k + 3]);
        let slot = |r: u32| match r {
            C_LATER => k,
            C_LOST => k + 1,
            r => r as usize,
        };
        for (n, &r) in c.root.iter().enumerate() {
            if r == C_NONE {
                continue;
            }
            let i = slot(r);
            own[i] += 1;
            if self.nodes[n]
                .cons
                .iter()
                .any(|c| matches!(c.kind_name(), "load" | "store" | "storeout" | "mcall"))
            {
                ops[i] += 1;
            }
        }
        let (mut total, mut total_ops) = (own.clone(), ops.clone());
        for j in (0..k).rev() {
            let d = c.seeds[j].down;
            if (d as usize) < k {
                total[d as usize] += total[j];
                total_ops[d as usize] += total_ops[j];
            }
        }
        let mut order: Vec<usize> = (0..k).collect();
        order.sort_by(|&a, &b| total[b].cmp(&total[a]).then(a.cmp(&b)));
        let mut b = String::new();
        for l in &c.lines {
            b.push_str(l);
            b.push('\n');
        }
        b.push_str(&format!(
            "CENSUS-END why={why} work={} seeds={k} creations={} later-creations={} booked={} later-nodes={} lost-nodes={}\n",
            self.work.get(),
            c.creations,
            c.later,
            own.iter().sum::<u64>(),
            own[k],
            own[k + 1]
        ));
        for i in order {
            let sd = &c.seeds[i];
            let down = if sd.down == C_NONE {
                "indep".to_string()
            } else if (sd.down as usize) < k {
                format!("down=#{}", sd.down)
            } else {
                "down=?".to_string()
            };
            b.push_str(&format!(
                "CENSUS-REACH #{i} {down} nodes={} ops={} total={} total-ops={} creations={} indep-creations={} work={} seq={}\n",
                own[i], ops[i], total[i], total_ops[i], sd.creations, sd.indep, sd.work, sd.seq
            ));
        }
        if capture {
            CAPTURE.with_borrow_mut(|v| {
                if let Some(v) = v.as_mut() {
                    v.push(b);
                }
            });
            return;
        }
        // The seed and lost lines were printed as they came.
        eprint!("{}", b.split_inclusive('\n').skip(c.lines.len()).collect::<String>());
        std::process::exit(86);
    }

    fn finish_block(&self, b: String, capture: bool) {
        if capture {
            CAPTURE.with_borrow_mut(|c| {
                if let Some(v) = c.as_mut() {
                    v.push(b);
                }
            });
            return;
        }
        if let Some(s) = self.stop_top.as_ref() {
            let mut v: Vec<_> = s
                .nopos
                .iter()
                .map(|(&(l, kd, c), &k)| (kd == "edge", k, l.file(), l.line(), kd, c))
                .collect();
            v.sort_by(|a, b| {
                a.0.cmp(&b.0)
                    .then(b.1.cmp(&a.1))
                    .then(a.2.cmp(b.2))
                    .then(a.3.cmp(&b.3))
                    .then(a.5.cmp(b.5))
            });
            let (mut ops, mut edges, mut inh) = (0u64, 0u64, 0u64);
            for (e, k, f, l, kd, c) in v {
                if kd.ends_with("-inherited") {
                    inh += k;
                    eprintln!("TOPSTOP nopos-inh {kd} {f}:{l} {c} {k}");
                } else if e {
                    edges += k;
                    eprintln!("TOPSTOP nopos-edge {f}:{l} {c} {k}");
                } else {
                    ops += k;
                    eprintln!("TOPSTOP nopos-op {kd} {f}:{l} {c} {k}");
                }
            }
            let mut g: Vec<_> = s.gen_gap.iter().map(|(&k, &n)| (n, k)).collect();
            g.sort_by(|a, b| b.cmp(a));
            for (n, k) in g {
                eprintln!("TOPSTOP nopos-hir {k} {n}");
            }
            let mut g: Vec<_> = s.self_gap.iter().map(|(&k, &n)| (n, k)).collect();
            g.sort_by(|a, b| b.cmp(a));
            for (n, k) in g {
                eprintln!("TOPSTOP nopos-self {k} {n}");
            }
            eprintln!(
                "TOPSTOP nopos-ops total {ops} (root operations that can stop); inherited {inh} (inside a rule without a position); nopos-edges total {edges}; edges with a position {}; shared operations {}",
                s.edge_pos.len(),
                s.shared.len()
            );
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
        let f = self
            .node_fn
            .get(n as usize)
            .copied()
            .unwrap_or(crate::model::NO_FN);
        if f != crate::model::NO_FN {
            let fi = &self.funcs[f as usize];
            if fi.this == n || fi.ret == n {
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

    /// The rule applications that made node `n` (`via:`).
    fn push_via(&self, b: &mut String, n: NodeId) {
        let (_, steps) = self.program_origin(n);
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
        let chain = self.top_chain(n);
        format!(
            "node {n} `{label}` ({}), from {}{}; holds [{}]{}{chain}",
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

    /// `PERRY_TOPSTOP_TOPFROM=1`: where the ⊤ node `n` holds came from, one
    /// line per first arrival back to the node it entered without a ⊤
    /// sender (the seed: its rule, reason and solver line).
    fn top_chain(&self, n: NodeId) -> String {
        let Some(s) = self.stop_top.as_ref().filter(|s| s.topfrom) else {
            return String::new();
        };
        let mut out = String::new();
        let mut cur = n;
        let mut seen = std::collections::HashSet::new();
        while seen.insert(cur) && seen.len() < 400 {
            let Some((src, why, rule, at, o)) = s.top_from.get(&cur) else {
                out += &format!("\n    top-from: node {cur}: no recorded arrival");
                break;
            };
            let (src, why, rule, at) = (*src, *why, *rule, *at);
            let p = self.node_program_pos(cur);
            let w = if why != u32::MAX { self.reason_text(why) } else { String::new() };
            let r = rule.map_or(String::new(), |(rn, c)| format!(" rule {} on {rn}", c.kind_name()));
            out += &format!(
                "\n    top-from: node {cur} `{}` at {} <- {}{r} [{w}]{o} ({}:{})",
                self.labels[cur as usize],
                self.pos_text(p),
                if src == NO_NODE { "NO_NODE".to_string() } else { format!("{src} `{}`", self.labels.get(src as usize).copied().unwrap_or("?")) },
                at.file(),
                at.line()
            );
            let src_top = src != NO_NODE
                && (src as usize) < self.nodes.len()
                && self.nodes[src as usize].pts.contains_tok(TOP);
            if !src_top {
                break;
            }
            cur = src;
        }
        out
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
        #[cfg(test)]
        if let Some(f) = s.flow.as_mut() {
            let rule = s.rule;
            f.push(FlowArrival { class: 4, node: NO_NODE, tok: t, rule, what: what.to_string(), at });
            return;
        }
        // The census lists seeds (S) only; a marker is no node arrival.
        if s.done || s.class & 4 == 0 || s.census.is_some() {
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
        if let Some(&(f, i)) = self.param_marker.get(&t) {
            let pn = self.funcs[f as usize]
                .params
                .get(i as usize)
                .copied()
                .unwrap_or(NO_NODE);
            let p = self
                .stop_top
                .as_ref()
                .unwrap()
                .node_pos
                .get(pn as usize)
                .copied()
                .unwrap_or(NOPOS);
            b.push_str(&format!(
                "  param:   {} (the parameter's declaration)\n",
                self.pos_text(p)
            ));
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
        let op_pos = self.stop_top.as_ref().unwrap().cur_pos();
        let what = match rule {
            Some((rn, c)) => format!("rule {} on node {rn}", c.kind_name()),
            None => "the operation".into(),
        };
        self.push_op_where(&mut b, op_pos, &what);
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
        if p.0 == SHARED {
            return format!("shared operation #{}: {}", p.1, s.shared[p.1 as usize].0);
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
        if p == NOPOS || p.0 == SHARED {
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
