//! Charter step 4b (#10884): LOOP regions and per-iteration (loop-body)
//! regions. Design: `/root/linktime/DESIGN.md` §3.0–§3.2 (approved
//! 2026-09-28).
//!
//! # What this does
//!
//! A loop whose body reads or writes static keys of a receiver that cannot
//! change inside the loop (`this`, or a parameter / local no closure mutates
//! and the loop never assigns) is entered through ONE guard per receiver, in
//! the preheader:
//!
//! ```text
//! preheader:  receiver test + ONE ShapeId compare against the region word
//!             (+ the two per-object store facts, when the region stores)
//!             -> valid (an i1 slot) and the region word's slot vector
//! body top:   !valid                 -> G-body (today's lowering)
//!             [body can re-enter JS] valid := ([p+4] == S) && no-proof
//!             F-body                  (facts: bare loads / stores)
//! ```
//!
//! Only the BODY is lowered twice; the loop's own mechanics (counter
//! representation, condition, update, polls) are lowered once by whichever
//! tier takes the loop, so no tier needs a hand-off protocol (DESIGN §3.0).
//! F-body and G-body rejoin at the latch. That is sound because a fact is
//! used only inside F-body, and F-body is entered only when `valid` was
//! established by the guard or the re-check with no JS in between: the
//! re-check sits at the TOP of every iteration.
//!
//! A per-iteration receiver (`const o = objs[k & 7]; o.d = k; ...`) gets the
//! same split for the TAIL of the body after its binding, with the full guard
//! at the tail's top every iteration (no `valid` slot).
//!
//! # Which accesses are bare
//!
//! [`plan`] is the planner and the census in one pass (DESIGN §3.4): it walks
//! the body in JS evaluation order with the facts FRESH at the split point, and
//! anything that can run JS — a call, `new`, a coercion of an unproven operand,
//! another receiver's access (a getter), an index access, a non-bare store —
//! makes them STALE for the rest of the iteration. An access reached FRESH is
//! bare; every other access is today's tower. After lowering, [`verify`]
//! recomputes the same fact state over the EMITTED blocks from the calls that
//! were actually emitted; a bare access reachable after a call that may run JS
//! discards F-body (the split then branches to G-body unconditionally). A plan
//! that is wrong can cost speed, never a value.
//!
//! # GC
//!
//! No pointer is held across anything: every bare access re-derives the
//! receiver handle from the binding's root (`load; bitcast; and POINTER_MASK`
//! — a derivation `root_reload.rs` and the gc-root-dominance checker both
//! track), and LLVM merges re-derivations inside collection-free stretches. S
//! and the slots are plain integers.

use std::collections::{BTreeMap, HashMap, HashSet};

use anyhow::Result;
use perry_hir::{BinaryOp, CompareOp, Expr, Stmt, UnaryOp};

use crate::expr::region_guard::{self, Sites, MAX_KEYS};
use crate::expr::{lower_expr, FnCtx};
use crate::types::{DOUBLE, I1, I16, I32, I64, I8};

const SLOT_BITS: u32 = 6;
const PRIME_ATTEMPTS: &str = "8";
/// `REGION_GUARD_WORD_EMPTY`.
const EMPTY_WORD: &str = "4294967295";
/// `OBJ_FLAG_PACKED_NUMERIC_PROOF` (0x80) — DESIGN §6.5a fact F-B.
const PROOF_FLAG_I16: &str = "128";
/// `OBJ_FLAG_PLAIN_ORDINARY | OBJ_FLAG_TYPED_ARRAY_PROTO` and its admitted
/// value — DESIGN §6.5a fact F-A (class-less receivers).
const CLASSLESS_ADMIT_MASK_I16: &str = "768";
const CLASSLESS_ADMIT_I16: &str = "512";

/// `PERRY_REGIONS=0` switches loop regions off in ONE compiler (A/B arm);
/// `PERRY_REGION_READS=0` (slices 1/2) switches them off too.
fn disabled() -> bool {
    matches!(
        std::env::var("PERRY_REGIONS").as_deref(),
        Ok("0") | Ok("off") | Ok("false")
    ) || region_guard::disabled()
}

#[derive(Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Debug)]
pub(crate) enum Recv {
    Local(u32),
    This,
}

impl Recv {
    fn of(e: &Expr) -> Option<Recv> {
        match e {
            Expr::LocalGet(id) => Some(Recv::Local(*id)),
            Expr::This => Some(Recv::This),
            _ => None,
        }
    }
    fn expr(self) -> Expr {
        match self {
            Recv::Local(id) => Expr::LocalGet(id),
            Recv::This => Expr::This,
        }
    }
}

/// One receiver of an admitted region, as lowering needs it.
#[derive(Clone)]
pub(crate) struct Receiver {
    pub(crate) recv: Recv,
    pub(crate) keys: Vec<String>,
    pub(crate) has_store: bool,
    sites: Option<(String, String)>,
    /// The region word, an SSA value of the preheader (loop regions) or of
    /// the tail's guard block (body regions).
    word: String,
    /// Each key's slot (`i64`), decoded once from `word`.
    slots: Vec<String>,
}

/// A region whose body has not been lowered yet: `lower_stmts` recognises the
/// body slice by address and splits it.
pub(crate) struct Pending {
    body_ptr: usize,
    body_len: usize,
    /// First statement of the split tail (0 for a loop region).
    split_at: usize,
    /// Loop region: the i1 slot the preheader guard and the re-check write.
    valid_slot: Option<String>,
    /// Loop region: set by a fact tree's generic arm.
    dirty_slot: Option<String>,
    recheck: Recheck,
    receivers: Vec<Receiver>,
    bare: HashSet<usize>,
    trees: HashSet<usize>,
    token: u64,
}

/// The facts active while F-body is lowered.
pub(crate) struct Active {
    receivers: Vec<Receiver>,
    bare: HashSet<usize>,
    trees: HashSet<usize>,
    dirty_slot: Option<String>,
    /// Emitted bare accesses: (block index, instruction index) — what
    /// [`verify`] checks.
    emitted: Vec<(usize, usize)>,
}

thread_local! {
    static NEXT_TOKEN: std::cell::Cell<u64> = const { std::cell::Cell::new(1) };
    static STATS: std::cell::RefCell<[u64; 6]> = const { std::cell::RefCell::new([0; 6]) };
}

/// `PERRY_REGION_DIAG=1` counters: loop regions formed, body regions formed,
/// bare reads, bare stores, F-bodies discarded by the verifier, loops refused.
fn stat(i: usize, n: u64) {
    STATS.with(|s| s.borrow_mut()[i] += n);
}

pub(crate) fn take_stats() -> [u64; 6] {
    STATS.with(|s| std::mem::take(&mut *s.borrow_mut()))
}

// ---------------------------------------------------------------- planning

/// Facts hold on every path.
const FRESH: u8 = 2;
/// Facts hold on every path except the generic arm of a fact tree, which
/// sets the region's dirty flag: the next iteration re-checks only if it was
/// taken. Not enough for a bare access.
const DIRTY: u8 = 1;

/// `None` = unreachable. Absent receiver = stale.
type St = Option<BTreeMap<Recv, u8>>;

fn meet(a: St, b: St) -> St {
    match (a, b) {
        (None, x) | (x, None) => x,
        (Some(a), Some(b)) => Some(
            a.into_iter()
                .filter_map(|(k, v)| b.get(&k).map(|w| (k, v.min(*w))))
                .collect(),
        ),
    }
}

fn dirty(st: &mut St) {
    if let Some(m) = st {
        for v in m.values_mut() {
            *v = (*v).min(DIRTY);
        }
    }
}

/// A `+` tree over reads of ONE receiver's region keys, locals and numeric
/// literals (at least one read): lowered as a FACT TREE — the reads are bare
/// loads, every leaf is verified a Number, the tree folds to `fadd`s, and a
/// failed check runs the tree through today's lowering in source order (with
/// the facts masked) and sets the dirty flag. Slice 1's rule (§L7.3) with the
/// region's facts in place of its own guard.
fn fact_tree_leaves<'e>(
    e: &'e Expr,
    covered: &dyn Fn(Recv, &str) -> bool,
) -> Option<(Recv, Vec<&'e Expr>)> {
    fn leaves<'e>(e: &'e Expr, out: &mut Vec<&'e Expr>) {
        if let Expr::Binary {
            op: BinaryOp::Add,
            left,
            right,
        } = e
        {
            leaves(left, out);
            leaves(right, out);
        } else {
            out.push(e);
        }
    }
    if !matches!(
        e,
        Expr::Binary {
            op: BinaryOp::Add,
            ..
        }
    ) {
        return None;
    }
    let mut all = Vec::new();
    leaves(e, &mut all);
    let mut recv: Option<Recv> = None;
    let mut reads = Vec::new();
    for l in all {
        match l {
            Expr::PropertyGet {
                object, property, ..
            } => {
                let r = Recv::of(object)?;
                if recv.is_some_and(|x| x != r) || !covered(r, property) {
                    return None;
                }
                recv = Some(r);
                reads.push(l);
            }
            Expr::LocalGet(_) | Expr::Number(_) | Expr::Integer(_) => {}
            _ => return None,
        }
    }
    Some((recv?, reads))
}

fn kill(st: &mut St) {
    if let Some(m) = st {
        m.clear();
    }
}

/// A leaf that is a primitive by construction: an operator over it cannot
/// reach ToPrimitive.
fn prim(e: &Expr) -> bool {
    match e {
        Expr::Number(_)
        | Expr::Integer(_)
        | Expr::String(_)
        | Expr::Bool(_)
        | Expr::Undefined
        | Expr::Null
        | Expr::TypeOf(_) => true,
        Expr::Binary { op, left, right } => {
            !matches!(op, BinaryOp::Add) || (prim(left) && prim(right))
        }
        Expr::Compare { .. } => true,
        Expr::Unary { op, operand } => matches!(op, UnaryOp::Not) || prim(operand),
        _ => false,
    }
}

struct Planner<'p, 'a> {
    ctx: &'p FnCtx<'a>,
    cands: &'p HashSet<Recv>,
    keys: &'p HashMap<Recv, Vec<String>>,
    bare: HashSet<usize>,
    trees: HashSet<usize>,
    bare_stores: HashSet<Recv>,
    continues: Vec<St>,
    record: bool,
}

impl Planner<'_, '_> {
    /// A primitive by construction, or a value the compiler proves a raw
    /// double (`expr_produces_canonical_raw_f64` is the predicate that already
    /// licenses an unguarded `fadd`).
    fn prim(&self, e: &Expr) -> bool {
        prim(e) || crate::type_analysis::is_numeric_expr(self.ctx, e)
    }

    fn covered(&self, r: Recv, key: &str) -> bool {
        self.keys
            .get(&r)
            .is_some_and(|k| k.iter().any(|x| x == key))
    }

    fn access(&mut self, e: &Expr, r: Recv, key: &str, store: bool, st: &mut St) {
        let fresh = st.as_ref().is_some_and(|m| m.get(&r) == Some(&FRESH));
        if fresh && self.cands.contains(&r) && self.covered(r, key) {
            if self.record {
                self.bare.insert(e as *const Expr as usize);
                if store {
                    self.bare_stores.insert(r);
                }
            }
            return;
        }
        // Today's tower: its miss path can reach a getter/setter or reshape
        // the receiver.
        kill(st);
    }

    fn exprs(&mut self, es: &[Expr], mut st: St) -> St {
        for e in es {
            st = self.expr(e, st);
        }
        st
    }

    fn expr(&mut self, e: &Expr, mut st: St) -> St {
        st.as_ref()?;
        match e {
            Expr::PropertyGet {
                object, property, ..
            } => {
                st = self.expr(object, st);
                match Recv::of(object) {
                    Some(r) => self.access(e, r, property, false, &mut st),
                    None => kill(&mut st),
                }
                st
            }
            Expr::PutValueSet {
                target,
                key,
                value,
                receiver,
                ..
            } => {
                st = self.expr(target, st);
                st = self.expr(key, st);
                st = self.expr(value, st);
                match (Recv::of(target), key.as_ref()) {
                    (Some(r), Expr::String(k)) if Recv::of(receiver) == Some(r) => {
                        self.access(e, r, k, true, &mut st)
                    }
                    _ => kill(&mut st),
                }
                st
            }
            Expr::Call { callee, args, .. } => {
                if let Expr::PropertyGet { object, .. } = callee.as_ref() {
                    st = self.expr(object, st);
                } else {
                    st = self.expr(callee, st);
                }
                st = self.exprs(args, st);
                kill(&mut st);
                st
            }
            Expr::LocalSet(_, v) => self.expr(v, st),
            Expr::Binary { left, right, .. } => {
                let keys = self.keys;
                let cands = self.cands;
                let covered = |r: Recv, k: &str| {
                    cands.contains(&r) && keys.get(&r).is_some_and(|l| l.iter().any(|x| x == k))
                };
                if let Some((r, reads)) = fact_tree_leaves(e, &covered) {
                    if st.as_ref().is_some_and(|m| m.get(&r) == Some(&FRESH)) {
                        if self.record {
                            self.trees.insert(e as *const Expr as usize);
                            for l in reads {
                                self.bare.insert(l as *const Expr as usize);
                            }
                        }
                        dirty(&mut st);
                        return st;
                    }
                }
                st = self.expr(left, st);
                st = self.expr(right, st);
                if !(self.prim(left) && self.prim(right)) {
                    kill(&mut st);
                }
                st
            }
            Expr::Unary { op, operand } => {
                st = self.expr(operand, st);
                if !matches!(op, UnaryOp::Not) && !self.prim(operand) {
                    kill(&mut st);
                }
                st
            }
            Expr::Compare { op, left, right } => {
                st = self.expr(left, st);
                st = self.expr(right, st);
                if !matches!(op, CompareOp::Eq | CompareOp::Ne)
                    && !(self.prim(left) && self.prim(right))
                {
                    kill(&mut st);
                }
                st
            }
            Expr::Logical { left, right, .. } => {
                let a = self.expr(left, st);
                let b = self.expr(right, a.clone());
                meet(a, b)
            }
            Expr::Conditional {
                condition,
                then_expr,
                else_expr,
            } => {
                let c = self.expr(condition, st);
                let t = self.expr(then_expr, c.clone());
                let f = self.expr(else_expr, c);
                meet(t, f)
            }
            Expr::Sequence(v) => self.exprs(v, st),
            Expr::Undefined
            | Expr::Null
            | Expr::Bool(_)
            | Expr::Number(_)
            | Expr::Integer(_)
            | Expr::BigInt(_)
            | Expr::String(_)
            | Expr::WtfString(_)
            | Expr::LocalGet(_)
            | Expr::GlobalGet(_)
            | Expr::FuncRef(_)
            | Expr::This => st,
            // A local ++/-- ToNumerics its operand (valueOf on an object).
            Expr::Update { id, .. } => {
                if !self.ctx.integer_locals.contains(id) {
                    kill(&mut st);
                }
                st
            }
            Expr::TypeOf(x) | Expr::Void(x) => self.expr(x, st),
            _ => {
                // Everything else is treated as able to run JS: the planner is
                // conservative, and `verify` is the check.
                let mut kids: Vec<&Expr> = Vec::new();
                perry_hir::walker::walk_expr_children(e, &mut |c| kids.push(c));
                for k in kids {
                    st = self.expr(k, st);
                }
                kill(&mut st);
                st
            }
        }
    }

    fn stmts(&mut self, ss: &[Stmt], mut st: St) -> St {
        for s in ss {
            st = self.stmt(s, st);
        }
        st
    }

    fn stmt(&mut self, s: &Stmt, mut st: St) -> St {
        match s {
            Stmt::Let { init, .. } => {
                if let Some(e) = init {
                    st = self.expr(e, st);
                }
                st
            }
            Stmt::Expr(e) => self.expr(e, st),
            Stmt::Throw(e) => {
                let _ = self.expr(e, st);
                None
            }
            Stmt::Return(e) => {
                if let Some(e) = e {
                    let _ = self.expr(e, st);
                }
                None
            }
            Stmt::If {
                condition,
                then_branch,
                else_branch,
            } => {
                let c = self.expr(condition, st);
                let t = self.stmts(then_branch, c.clone());
                let f = match else_branch {
                    Some(b) => self.stmts(b, c),
                    None => c,
                };
                meet(t, f)
            }
            Stmt::Break => None,
            Stmt::Continue => {
                self.continues.push(st);
                None
            }
            // Nested loops: two passes (the second records) from the meet of
            // the entry and the back edge; `break`/`continue` inside them
            // belong to them.
            Stmt::While { condition, body } => self.inner_loop(Some(condition), body, None, st),
            Stmt::DoWhile { body, condition } => self.inner_loop(Some(condition), body, None, st),
            Stmt::For {
                init,
                condition,
                update,
                body,
            } => {
                if let Some(i) = init {
                    st = self.stmt(i, st);
                }
                self.inner_loop(condition.as_ref(), body, update.as_ref(), st)
            }
            Stmt::Switch {
                discriminant,
                cases,
            } => {
                // `break` inside a switch leaves the switch; conservatively
                // everything after a switch is stale.
                let d = self.expr(discriminant, st);
                let saved = std::mem::take(&mut self.continues);
                let mut prev = d.clone();
                for c in cases {
                    if let Some(t) = &c.test {
                        prev = self.expr(t, prev);
                    }
                    prev = self.stmts(&c.body, prev);
                }
                let inner = std::mem::replace(&mut self.continues, saved);
                self.continues.extend(inner);
                d.map(|_| BTreeMap::new())
            }
            _ => {
                kill(&mut st);
                st
            }
        }
    }

    fn inner_loop(
        &mut self,
        cond: Option<&Expr>,
        body: &[Stmt],
        update: Option<&Expr>,
        st: St,
    ) -> St {
        let rec = self.record;
        let saved = std::mem::take(&mut self.continues);
        self.record = false;
        let mut s = st.clone();
        if let Some(c) = cond {
            s = self.expr(c, s);
        }
        let end = self.stmts(body, s);
        let conts = std::mem::take(&mut self.continues);
        let mut back = end;
        for c in conts {
            back = meet(back, c);
        }
        if let Some(u) = update {
            back = self.expr(u, back);
        }
        self.record = rec;
        let head = meet(st.clone(), back);
        let mut s = head.clone();
        if let Some(c) = cond {
            s = self.expr(c, s);
        }
        let _ = self.stmts(body, s.clone());
        self.continues = saved;
        // `break` can leave from anywhere: the exit is stale.
        s.map(|_| BTreeMap::new())
    }
}

/// Anything that makes a body unsuitable for a split: a closure (a second
/// lowering would emit it twice), `try` (handlers), labels, generators.
fn body_refused(ss: &[Stmt]) -> bool {
    fn e_bad(e: &Expr) -> bool {
        if matches!(
            e,
            Expr::Closure { .. } | Expr::Yield { .. } | Expr::Await(_)
        ) {
            return true;
        }
        let mut bad = false;
        perry_hir::walker::walk_expr_children(e, &mut |c| bad |= e_bad(c));
        bad
    }
    fn s_bad(s: &Stmt) -> bool {
        match s {
            Stmt::Let { init: Some(e), .. } | Stmt::Expr(e) | Stmt::Throw(e) => e_bad(e),
            Stmt::Return(Some(e)) => e_bad(e),
            Stmt::Let { .. } | Stmt::Return(None) | Stmt::Break | Stmt::Continue => false,
            Stmt::If {
                condition,
                then_branch,
                else_branch,
            } => {
                e_bad(condition)
                    || then_branch.iter().any(s_bad)
                    || else_branch.as_ref().is_some_and(|b| b.iter().any(s_bad))
            }
            Stmt::While { condition, body } | Stmt::DoWhile { body, condition } => {
                e_bad(condition) || body.iter().any(s_bad)
            }
            Stmt::For {
                init,
                condition,
                update,
                body,
            } => {
                init.as_ref().is_some_and(|i| s_bad(i))
                    || condition.as_ref().is_some_and(e_bad)
                    || update.as_ref().is_some_and(e_bad)
                    || body.iter().any(s_bad)
            }
            Stmt::Switch {
                discriminant,
                cases,
            } => {
                e_bad(discriminant)
                    || cases
                        .iter()
                        .any(|c| c.test.as_ref().is_some_and(e_bad) || c.body.iter().any(s_bad))
            }
            _ => true,
        }
    }
    ss.iter().any(s_bad)
}

/// Locals assigned (or declared) anywhere in `ss`, and in `extra`.
fn assigned(ss: &[Stmt], extra: &[&Expr]) -> HashSet<u32> {
    fn e_walk(e: &Expr, out: &mut HashSet<u32>) {
        match e {
            Expr::LocalSet(id, _) | Expr::Update { id, .. } => {
                out.insert(*id);
            }
            _ => {}
        }
        perry_hir::walker::walk_expr_children(e, &mut |c| e_walk(c, out));
    }
    fn s_walk(s: &Stmt, out: &mut HashSet<u32>) {
        match s {
            Stmt::Let { id, init, .. } => {
                out.insert(*id);
                if let Some(e) = init {
                    e_walk(e, out);
                }
            }
            Stmt::Expr(e) | Stmt::Throw(e) => e_walk(e, out),
            Stmt::Return(Some(e)) => e_walk(e, out),
            Stmt::If {
                condition,
                then_branch,
                else_branch,
            } => {
                e_walk(condition, out);
                then_branch.iter().for_each(|s| s_walk(s, out));
                if let Some(b) = else_branch {
                    b.iter().for_each(|s| s_walk(s, out));
                }
            }
            Stmt::While { condition, body } | Stmt::DoWhile { body, condition } => {
                e_walk(condition, out);
                body.iter().for_each(|s| s_walk(s, out));
            }
            Stmt::For {
                init,
                condition,
                update,
                body,
            } => {
                if let Some(i) = init {
                    s_walk(i, out);
                }
                if let Some(c) = condition {
                    e_walk(c, out);
                }
                if let Some(u) = update {
                    e_walk(u, out);
                }
                body.iter().for_each(|s| s_walk(s, out));
            }
            Stmt::Switch {
                discriminant,
                cases,
            } => {
                e_walk(discriminant, out);
                for c in cases {
                    if let Some(t) = &c.test {
                        e_walk(t, out);
                    }
                    c.body.iter().for_each(|s| s_walk(s, out));
                }
            }
            _ => {}
        }
    }
    let mut out = HashSet::new();
    ss.iter().for_each(|s| s_walk(s, &mut out));
    extra.iter().for_each(|e| e_walk(e, &mut out));
    out
}

/// Static-key accesses in `ss` per candidate receiver, in first-seen key
/// order: `(receiver, key, is_store)`.
fn accesses(ss: &[Stmt]) -> Vec<(Recv, String, bool)> {
    fn e_walk(e: &Expr, out: &mut Vec<(Recv, String, bool)>) {
        match e {
            Expr::PropertyGet {
                object, property, ..
            } => {
                if let Some(r) = Recv::of(object) {
                    out.push((r, property.clone(), false));
                }
            }
            Expr::PutValueSet {
                target,
                key,
                receiver,
                ..
            } => {
                if let (Some(r), Expr::String(k)) = (Recv::of(target), key.as_ref()) {
                    if Recv::of(receiver) == Some(r) {
                        out.push((r, k.clone(), true));
                    }
                }
            }
            _ => {}
        }
        // A method callee `o.m(...)` is not an own-slot read.
        if let Expr::Call { callee, args, .. } = e {
            if let Expr::PropertyGet { object, .. } = callee.as_ref() {
                e_walk(object, out);
            } else {
                e_walk(callee, out);
            }
            for a in args {
                e_walk(a, out);
            }
            return;
        }
        perry_hir::walker::walk_expr_children(e, &mut |c| e_walk(c, out));
    }
    fn s_walk(s: &Stmt, out: &mut Vec<(Recv, String, bool)>) {
        match s {
            Stmt::Let { init: Some(e), .. } | Stmt::Expr(e) | Stmt::Throw(e) => e_walk(e, out),
            Stmt::Return(Some(e)) => e_walk(e, out),
            Stmt::If {
                condition,
                then_branch,
                else_branch,
            } => {
                e_walk(condition, out);
                then_branch.iter().for_each(|s| s_walk(s, out));
                if let Some(b) = else_branch {
                    b.iter().for_each(|s| s_walk(s, out));
                }
            }
            Stmt::While { condition, body } | Stmt::DoWhile { body, condition } => {
                e_walk(condition, out);
                body.iter().for_each(|s| s_walk(s, out));
            }
            Stmt::For {
                init,
                condition,
                update,
                body,
            } => {
                if let Some(i) = init {
                    s_walk(i, out);
                }
                if let Some(c) = condition {
                    e_walk(c, out);
                }
                if let Some(u) = update {
                    e_walk(u, out);
                }
                body.iter().for_each(|s| s_walk(s, out));
            }
            Stmt::Switch {
                discriminant,
                cases,
            } => {
                e_walk(discriminant, out);
                for c in cases {
                    if let Some(t) = &c.test {
                        e_walk(t, out);
                    }
                    c.body.iter().for_each(|s| s_walk(s, out));
                }
            }
            _ => {}
        }
    }
    let mut out = Vec::new();
    ss.iter().for_each(|s| s_walk(s, &mut out));
    out
}

/// Is `r` a binding whose value only this function's visible writes can
/// change, stored the plain way (a NaN-boxed root slot or a capture)?
fn receiver_eligible(ctx: &FnCtx<'_>, r: Recv) -> bool {
    match r {
        Recv::This => !ctx.this_stack.is_empty() && !ctx.in_static_member,
        Recv::Local(id) => {
            !ctx.boxed_vars.contains(&id)
                && !ctx.prealloc_boxes.contains(&id)
                && !ctx.tdz_boxes.contains(&id)
                // A module-level binding can be assigned by any call.
                && !ctx.module_globals.contains_key(&id)
                && !ctx.pod_records.contains_key(&id)
                && !ctx.scalar_replaced.contains_key(&id)
                && !ctx.spec_ta_bindings.contains_key(&id)
                && !ctx.local_slot_reps.contains_key(&id)
                && !ctx.integer_locals.contains(&id)
                && !ctx.receiver_descriptors.contains_buffer_view(id)
                && ctx.ptr_shape_receiver_fact(&Expr::LocalGet(id)).is_none()
                && (ctx.locals.contains_key(&id) || ctx.closure_captures.contains_key(&id))
        }
    }
}

struct Plan {
    receivers: Vec<(Recv, Vec<String>, bool)>,
    bare: HashSet<usize>,
    trees: HashSet<usize>,
    recheck: Recheck,
}

/// What the top of an iteration must do before F-body.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum Recheck {
    /// Nothing between iterations can run JS.
    None,
    /// Only fact trees' generic arms can: re-check when the flag is set.
    Dirty,
    /// Every iteration.
    Always,
}

/// Plan the split of `tail` (the whole body for a loop region). `cands` are
/// the eligible receivers; the facts are FRESH at the tail's top. `loop_ctl`
/// carries the loop's condition and update for the re-check decision (loop
/// regions only).
fn plan(
    ctx: &FnCtx<'_>,
    tail: &[Stmt],
    cands: HashSet<Recv>,
    loop_ctl: Option<(Option<&Expr>, Option<&Expr>)>,
) -> Option<Plan> {
    if cands.is_empty() {
        return None;
    }
    let mut keys: HashMap<Recv, Vec<String>> = HashMap::new();
    let mut overflow: HashSet<Recv> = HashSet::new();
    for (r, k, _) in accesses(tail) {
        if !cands.contains(&r) {
            continue;
        }
        let list = keys.entry(r).or_default();
        if !list.contains(&k) {
            if list.len() == MAX_KEYS {
                overflow.insert(r);
            } else {
                list.push(k);
            }
        }
    }
    let cands: HashSet<Recv> = cands
        .into_iter()
        .filter(|r| keys.contains_key(r) && !overflow.contains(r))
        .collect();
    if cands.is_empty() {
        return None;
    }
    let fresh: BTreeMap<Recv, u8> = cands.iter().map(|r| (*r, FRESH)).collect();
    let mut p = Planner {
        ctx,
        cands: &cands,
        keys: &keys,
        bare: HashSet::new(),
        trees: HashSet::new(),
        bare_stores: HashSet::new(),
        continues: Vec::new(),
        record: true,
    };
    let end = p.stmts(tail, Some(fresh.clone()));
    let conts = std::mem::take(&mut p.continues);
    let mut recheck = Recheck::None;
    if let Some((cond, update)) = loop_ctl {
        p.record = false;
        let mut back = end;
        for c in conts {
            back = meet(back, c);
        }
        if let Some(u) = update {
            back = p.expr(u, back);
        }
        // Entry from the preheader and from the back edge both evaluate the
        // condition before the body.
        let mut head = meet(Some(fresh.clone()), back);
        if let Some(c) = cond {
            head = p.expr(c, head);
        }
        recheck = match &head {
            None => Recheck::None,
            Some(m) => {
                let worst = cands
                    .iter()
                    .map(|r| m.get(r).copied().unwrap_or(0))
                    .min()
                    .unwrap_or(FRESH);
                if worst == FRESH {
                    Recheck::None
                } else if worst == DIRTY {
                    Recheck::Dirty
                } else {
                    Recheck::Always
                }
            }
        };
    }
    let bare = std::mem::take(&mut p.bare);
    let trees = std::mem::take(&mut p.trees);
    let bare_stores = std::mem::take(&mut p.bare_stores);
    if bare.is_empty() {
        return None;
    }
    // Only receivers with a bare access need a guard.
    let used: HashSet<Recv> = {
        let mut u = HashSet::new();
        for (r, k, _) in accesses(tail) {
            if cands.contains(&r) && keys[&r].contains(&k) {
                u.insert(r);
            }
        }
        u
    };
    let mut receivers: Vec<(Recv, Vec<String>, bool)> = used
        .into_iter()
        .map(|r| (r, keys[&r].clone(), bare_stores.contains(&r)))
        .collect();
    receivers.sort_by_key(|(r, _, _)| *r);
    Some(Plan {
        receivers,
        bare,
        trees,
        recheck,
    })
}

// ---------------------------------------------------------------- lowering

/// The receiver's value in the current block. A fresh lowering of the binding
/// reads its root now, so the result names the object's CURRENT address.
fn lower_recv(ctx: &mut FnCtx<'_>, r: Recv) -> Result<String> {
    lower_expr(ctx, &r.expr())
}

/// `bits & POINTER_MASK` — the handle, derived transparently.
fn handle_of(ctx: &mut FnCtx<'_>, recv_box: &str) -> String {
    let bits = ctx.block().bitcast_double_to_i64(recv_box);
    ctx.block().and(I64, &bits, crate::nanbox::POINTER_MASK_I64)
}

fn field_i16(ctx: &mut FnCtx<'_>, handle: &str, offset: i64) -> String {
    let addr = ctx.block().add(I64, handle, &offset.to_string());
    let ptr = ctx.block().inttoptr(I64, &addr);
    ctx.block().load(I16, &ptr)
}

fn field_i32(ctx: &mut FnCtx<'_>, handle: &str, offset: i64) -> String {
    let addr = ctx.block().add(I64, handle, &offset.to_string());
    let ptr = ctx.block().inttoptr(I64, &addr);
    ctx.block().load(I32, &ptr)
}

/// DESIGN §6.5a: F-A (receiver kind) and F-B (no Array-subclass numeric
/// proof), as an i1, for a receiver whose handle is proven an object.
fn store_admission(ctx: &mut FnCtx<'_>, handle: &str, with_kind: bool) -> String {
    let reserved = field_i16(ctx, handle, -6);
    let proof = ctx.block().and(I16, &reserved, PROOF_FLAG_I16);
    let no_proof = ctx.block().icmp_eq(I16, &proof, "0");
    if !with_kind {
        return no_proof;
    }
    let class_id = field_i32(ctx, handle, 0);
    let biased = ctx.block().add(I32, &class_id, "2");
    let has_class = ctx.block().icmp_ugt(I32, &biased, "2");
    let admit_bits = ctx.block().and(I16, &reserved, CLASSLESS_ADMIT_MASK_I16);
    let admitted = ctx.block().icmp_eq(I16, &admit_bits, CLASSLESS_ADMIT_I16);
    let classless = ctx.block().icmp_eq(I32, &class_id, "0");
    let plain = ctx.block().and(I1, &admitted, &classless);
    let kind_ok = ctx.block().or(I1, &has_class, &plain);
    ctx.block().and(I1, &kind_ok, &no_proof)
}

/// Emit the full guard for one receiver from the CURRENT block. Returns the
/// word (EMPTY on every failing edge) and the pass flag, both valid in the
/// block the function leaves current.
fn emit_guard(ctx: &mut FnCtx<'_>, rv: &mut Receiver) -> Result<(String, String)> {
    let sites: Sites = region_guard::state_globals(ctx);
    rv.sites = Some((sites.word_g.clone(), sites.tries_g.clone()));
    let recv_box = lower_recv(ctx, rv.recv)?;
    let bits = ctx.block().bitcast_double_to_i64(&recv_box);
    let test = crate::expr::receiver_range::emit_fused_receiver_test(ctx.block(), &bits);
    let chk = ctx.new_block("rloop.guard.chk");
    let miss = ctx.new_block("rloop.guard.miss");
    let prime = ctx.new_block("rloop.guard.prime");
    let join = ctx.new_block("rloop.guard.join");
    let chk_l = ctx.block_label(chk);
    let miss_l = ctx.block_label(miss);
    let prime_l = ctx.block_label(prime);
    let join_l = ctx.block_label(join);
    let entry_l = ctx.block().label.clone();
    ctx.block()
        .cond_br(&test.is_object_pointer, &chk_l, &join_l);

    ctx.current_block = chk;
    let handle = crate::expr::receiver_range::emit_handle(ctx.block(), &test.biased);
    let word = ctx.block().load_atomic_monotonic(I64, &sites.word_g, 8);
    let expected = ctx.block().trunc(I64, &word, I32);
    let sid = field_i32(ctx, &handle, 4);
    let eq = ctx.block().icmp_eq(I32, &sid, &expected);
    let admit = if rv.has_store {
        store_admission(ctx, &handle, true)
    } else {
        "true".to_string()
    };
    let pass = ctx.block().and(I1, &eq, &admit);
    let chk_end = ctx.block().label.clone();
    ctx.block().cond_br(&eq, &join_l, &miss_l);

    // The shape did not match: prime (bounded for the process), then compare
    // again against what the runtime published.
    ctx.current_block = miss;
    let tries = ctx.block().load(I32, &sites.tries_g);
    let may = ctx.block().icmp_ult(I32, &tries, PRIME_ATTEMPTS);
    ctx.block().cond_br(&may, &prime_l, &join_l);

    ctx.current_block = prime;
    let next = ctx.block().add(I32, &tries, "1");
    ctx.block().store(I32, &next, &sites.tries_g);
    let mut key_bits: Vec<String> = Vec::with_capacity(MAX_KEYS);
    for i in 0..MAX_KEYS {
        if let Some(key) = rv.keys.get(i) {
            let idx = ctx.strings.intern(key);
            let g = format!("@{}", ctx.strings.entry(idx).handle_global);
            let boxed = ctx.block().load(DOUBLE, &g);
            key_bits.push(ctx.block().bitcast_double_to_i64(&boxed));
        } else {
            key_bits.push("0".to_string());
        }
    }
    let n = rv.keys.len().to_string();
    let word_g = sites.word_g.clone();
    let primed = ctx.block().call(
        I64,
        "js_region_loop_prime",
        &[
            (crate::types::PTR, &word_g),
            (I32, &sid),
            (I32, &n),
            (I64, &key_bits[0]),
            (I64, &key_bits[1]),
            (I64, &key_bits[2]),
            (I64, &key_bits[3]),
            (I64, &key_bits[4]),
        ],
    );
    // The prime is not a collection point for the receiver's facts: it reads
    // shapes and key bytes only. The object may still have moved in theory
    // (unclassified callee), so the handle is re-derived for the admission.
    let recv_box2 = lower_recv(ctx, rv.recv)?;
    let handle2 = handle_of(ctx, &recv_box2);
    let sid2 = field_i32(ctx, &handle2, 4);
    let exp2 = ctx.block().trunc(I64, &primed, I32);
    let eq2 = ctx.block().icmp_eq(I32, &sid2, &exp2);
    let admit2 = if rv.has_store {
        store_admission(ctx, &handle2, true)
    } else {
        "true".to_string()
    };
    let pass2 = ctx.block().and(I1, &eq2, &admit2);
    let prime_end = ctx.block().label.clone();
    ctx.block().br(&join_l);

    ctx.current_block = join;
    let word_out = ctx.block().phi(
        I64,
        &[
            (EMPTY_WORD, entry_l.as_str()),
            (word.as_str(), chk_end.as_str()),
            (EMPTY_WORD, miss_l.as_str()),
            (primed.as_str(), prime_end.as_str()),
        ],
    );
    let pass_out = ctx.block().phi(
        I1,
        &[
            ("false", entry_l.as_str()),
            (pass.as_str(), chk_end.as_str()),
            ("false", miss_l.as_str()),
            (pass2.as_str(), prime_end.as_str()),
        ],
    );
    Ok((word_out, pass_out))
}

fn decode_slots(ctx: &mut FnCtx<'_>, rv: &mut Receiver, word: &str) {
    rv.word = word.to_string();
    rv.slots = (0..rv.keys.len())
        .map(|i| {
            let shift = (32 + SLOT_BITS * i as u32).to_string();
            let s = ctx.block().lshr(I64, word, &shift);
            ctx.block().and(I64, &s, "63")
        })
        .collect();
}

fn candidates_for_loop(
    ctx: &FnCtx<'_>,
    cond: Option<&Expr>,
    body: &[Stmt],
    update: Option<&Expr>,
) -> HashSet<Recv> {
    let mut extra: Vec<&Expr> = Vec::new();
    if let Some(c) = cond {
        extra.push(c);
    }
    if let Some(u) = update {
        extra.push(u);
    }
    let written = assigned(body, &extra);
    accesses(body)
        .into_iter()
        .map(|(r, _, _)| r)
        .filter(|r| match r {
            Recv::Local(id) => !written.contains(id),
            Recv::This => true,
        })
        .filter(|r| receiver_eligible(ctx, *r))
        .collect()
}

/// Called by the loop lowerings right before the loop is lowered (after a
/// `for`'s init, after the specialised tiers declined). Plans the region and,
/// when admitted, emits the preheader guard and registers the body for the
/// split. Returns a token for [`end`].
pub(crate) fn begin(
    ctx: &mut FnCtx<'_>,
    cond: Option<&Expr>,
    body: &[Stmt],
    update: Option<&Expr>,
) -> Result<Option<u64>> {
    if disabled()
        || crate::codegen::full_outline_ic_enabled()
        || crate::expr::typed_feedback_emission_enabled()
        || !ctx.pending_labels.is_empty()
        || in_call_free_clone(ctx)
        || !ctx.region_loop_facts.is_empty()
        || body_refused(body)
        || cond.is_some_and(expr_refused)
        || update.is_some_and(expr_refused)
    {
        return Ok(None);
    }
    // A loop region; failing that, a body region (per-iteration receiver).
    let cands = candidates_for_loop(ctx, cond, body, update);
    if let Some(p) = plan(ctx, body, cands, Some((cond, update))) {
        let token = NEXT_TOKEN.with(|t| {
            let v = t.get();
            t.set(v + 1);
            v
        });
        let valid_slot = ctx.func.alloca_entry(I1);
        let mut receivers: Vec<Receiver> = p
            .receivers
            .iter()
            .map(|(r, k, st)| Receiver {
                recv: *r,
                keys: k.clone(),
                has_store: *st,
                sites: None,
                word: String::new(),
                slots: Vec::new(),
            })
            .collect();
        let mut all = "true".to_string();
        for rv in receivers.iter_mut() {
            let (word, pass) = emit_guard(ctx, rv)?;
            all = ctx.block().and(I1, &all, &pass);
            decode_slots(ctx, rv, &word);
        }
        ctx.block().store(I1, &all, &valid_slot);
        let dirty_slot = ctx.func.alloca_entry(I1);
        ctx.block().store(I1, "false", &dirty_slot);
        stat(0, 1);
        ctx.region_loops.push(Pending {
            body_ptr: body.as_ptr() as usize,
            body_len: body.len(),
            split_at: 0,
            valid_slot: Some(valid_slot),
            dirty_slot: Some(dirty_slot),
            recheck: p.recheck,
            receivers,
            bare: p.bare,
            trees: p.trees,
            token,
        });
        return Ok(Some(token));
    }
    // Body region: the first `const o = <expr>` whose binding the rest of the
    // body reads or writes by static key.
    for (i, s) in body.iter().enumerate() {
        let Stmt::Let {
            id, init: Some(_), ..
        } = s
        else {
            continue;
        };
        let tail = &body[i + 1..];
        // Declared once, never assigned in the tail: a per-iteration constant.
        let decls = body
            .iter()
            .filter(|s| matches!(s, Stmt::Let { id: x, .. } if x == id))
            .count();
        if decls != 1 || assigned(tail, &[]).contains(id) || body_refused(tail) {
            continue;
        }
        // Eligibility of a body-declared binding is re-checked at the split,
        // after its declaration has been lowered.
        if ctx.boxed_vars.contains(id)
            || ctx.prealloc_boxes.contains(id)
            || ctx.tdz_boxes.contains(id)
            || ctx.local_slot_reps.contains_key(id)
            || ctx.integer_locals.contains(id)
        {
            continue;
        }
        let mut cands = HashSet::new();
        cands.insert(Recv::Local(*id));
        let Some(p) = plan(ctx, tail, cands, None) else {
            continue;
        };
        let token = NEXT_TOKEN.with(|t| {
            let v = t.get();
            t.set(v + 1);
            v
        });
        let receivers: Vec<Receiver> = p
            .receivers
            .iter()
            .map(|(r, k, st)| Receiver {
                recv: *r,
                keys: k.clone(),
                has_store: *st,
                sites: None,
                word: String::new(),
                slots: Vec::new(),
            })
            .collect();
        ctx.region_loops.push(Pending {
            body_ptr: body.as_ptr() as usize,
            body_len: body.len(),
            split_at: i + 1,
            valid_slot: None,
            dirty_slot: None,
            recheck: Recheck::None,
            receivers,
            bare: p.bare,
            trees: p.trees,
            token,
        });
        return Ok(Some(token));
    }
    stat(5, 1);
    Ok(None)
}

fn expr_refused(e: &Expr) -> bool {
    body_refused(&[Stmt::Expr(e.clone())])
}

pub(crate) fn end(ctx: &mut FnCtx<'_>, token: Option<u64>) {
    if let Some(t) = token {
        ctx.region_loops.retain(|p| p.token != t);
    }
}

/// Inside a call-free-by-construction fast clone of another tier the body is
/// not split: G-body contains calls, and those tiers discard a clone that
/// does.
fn in_call_free_clone(ctx: &FnCtx<'_>) -> bool {
    !ctx.class_field_loop_facts.is_empty()
        || !ctx.element_shape_loop_facts.is_empty()
        || !ctx.stable_packed_loop_facts.is_empty()
}

/// `lower_stmts`' hook: is `stmts` a registered region body?
pub(crate) fn pending_for(ctx: &FnCtx<'_>, stmts: &[Stmt]) -> Option<usize> {
    if ctx.region_loops.is_empty() || in_call_free_clone(ctx) {
        return None;
    }
    let ptr = stmts.as_ptr() as usize;
    ctx.region_loops
        .iter()
        .position(|p| p.body_ptr == ptr && p.body_len == stmts.len())
}

/// Lower a registered body: the prefix (body regions) once, then the split.
pub(crate) fn lower_split(
    ctx: &mut FnCtx<'_>,
    stmts: &[Stmt],
    idx: usize,
    lower_list: fn(&mut FnCtx<'_>, &[Stmt]) -> Result<()>,
) -> Result<()> {
    let split_at = ctx.region_loops[idx].split_at;
    let token = ctx.region_loops[idx].token;
    if split_at > 0 {
        lower_list(ctx, &stmts[..split_at])?;
        if ctx.block().is_terminated() {
            return Ok(());
        }
    }
    let tail = &stmts[split_at..];
    let mut receivers = ctx.region_loops[idx].receivers.clone();
    // A body region's binding was declared by the prefix just lowered: if its
    // lowering gave it a special representation, the tail lowers plainly.
    if split_at > 0 && !receivers.iter().all(|rv| receiver_eligible(ctx, rv.recv)) {
        return lower_list(ctx, tail);
    }
    let valid_slot = ctx.region_loops[idx].valid_slot.clone();
    let recheck = ctx.region_loops[idx].recheck;
    let bare = ctx.region_loops[idx].bare.clone();
    let trees = ctx.region_loops[idx].trees.clone();
    let dirty_slot = ctx.region_loops[idx].dirty_slot.clone();

    let fast = ctx.new_block("rloop.fast");
    let slow = ctx.new_block("rloop.slow");
    let join = ctx.new_block("rloop.join");
    let fast_l = ctx.block_label(fast);
    let slow_l = ctx.block_label(slow);
    let join_l = ctx.block_label(join);

    // Where the entry decision is emitted; its terminator is written LAST,
    // once `verify` has judged F-body.
    let decide;
    let mut decide_top: Option<(usize, String, String)> = None;
    match &valid_slot {
        Some(slot) => {
            let v = ctx.block().load(I1, slot);
            if recheck == Recheck::None {
                decide = (ctx.current_block, v);
            } else {
                let rc = ctx.new_block("rloop.recheck");
                let rc_l = ctx.block_label(rc);
                let top = ctx.new_block("rloop.top");
                let top_l = ctx.block_label(top);
                ctx.block().cond_br(&v, &top_l, &slow_l);
                ctx.current_block = top;
                let d_slot = dirty_slot.clone().expect("loop regions carry a dirty slot");
                let need = if recheck == Recheck::Dirty {
                    ctx.block().load(I1, &d_slot)
                } else {
                    "true".to_string()
                };
                // Fresh without a re-check: straight into F-body. The
                // terminator of `top` is written at the end, like `decide`.
                let top_idx = ctx.current_block;
                ctx.current_block = rc;
                let mut ok = "true".to_string();
                for rv in &receivers {
                    let recv_box = lower_recv(ctx, rv.recv)?;
                    let h = handle_of(ctx, &recv_box);
                    let sid = field_i32(ctx, &h, 4);
                    let exp = ctx.block().trunc(I64, &rv.word, I32);
                    let eq = ctx.block().icmp_eq(I32, &sid, &exp);
                    let eq = if rv.has_store {
                        let adm = store_admission(ctx, &h, false);
                        ctx.block().and(I1, &eq, &adm)
                    } else {
                        eq
                    };
                    ok = ctx.block().and(I1, &ok, &eq);
                }
                ctx.block().store(I1, &ok, slot);
                ctx.block().store(I1, "false", &d_slot);
                let rc_idx = ctx.current_block;
                decide_top = Some((top_idx, need, rc_l));
                decide = (rc_idx, ok);
            }
        }
        None => {
            // Body region: the full guard, every iteration.
            let mut all = "true".to_string();
            for rv in receivers.iter_mut() {
                let (word, pass) = emit_guard(ctx, rv)?;
                all = ctx.block().and(I1, &all, &pass);
                decode_slots(ctx, rv, &word);
            }
            stat(1, 1);
            decide = (ctx.current_block, all);
        }
    }

    // F-body.
    ctx.current_block = fast;
    let fast_first_block = fast;
    let scan_start = ctx.func.num_blocks();
    ctx.region_loop_facts.push(Active {
        receivers: receivers.clone(),
        bare,
        trees,
        dirty_slot: dirty_slot.clone(),
        emitted: Vec::new(),
    });
    let r = lower_list(ctx, tail);
    let active = ctx.region_loop_facts.pop().expect("pushed above");
    r?;
    if !ctx.block().is_terminated() {
        ctx.block().br(&join_l);
    }
    let scan_end = ctx.func.num_blocks();
    let ok = verify(ctx, fast_first_block, scan_start, scan_end, &active.emitted);
    if !ok {
        stat(4, 1);
        if std::env::var("PERRY_REGION_DIAG").as_deref() == Ok("2") {
            eprintln!(
                "[perry region] F-body discarded by the verifier in {}",
                ctx.func.name
            );
        }
    }

    // G-body: today's lowering.
    ctx.current_block = slow;
    lower_list(ctx, tail)?;
    if !ctx.block().is_terminated() {
        ctx.block().br(&join_l);
    }

    if let Some((top_idx, need, rc_l)) = decide_top {
        ctx.current_block = top_idx;
        if ok {
            ctx.block().cond_br(&need, &rc_l, &fast_l);
        } else {
            ctx.block().br(&slow_l);
        }
    }
    ctx.current_block = decide.0;
    if ok {
        ctx.block().cond_br(&decide.1, &fast_l, &slow_l);
    } else {
        ctx.block().br(&slow_l);
    }
    ctx.current_block = join;
    let _ = token;
    Ok(())
}

// ------------------------------------------------------- bare accesses

fn active_slot(ctx: &FnCtx<'_>, e: &Expr, r: Recv, key: &str) -> Option<String> {
    let a = ctx.region_loop_facts.last()?;
    if !a.bare.contains(&(e as *const Expr as usize)) {
        return None;
    }
    let rv = a.receivers.iter().find(|x| x.recv == r)?;
    let i = rv.keys.iter().position(|k| k == key)?;
    Some(rv.slots[i].clone())
}

fn note_emitted(ctx: &mut FnCtx<'_>) {
    let b = ctx.current_block;
    let i = ctx.func.blocks()[b].insts().len();
    if let Some(a) = ctx.region_loop_facts.last_mut() {
        a.emitted.push((b, i));
    }
}

fn slot_ptr(ctx: &mut FnCtx<'_>, handle: &str, slot: &str) -> String {
    let header = crate::target_layout::object_header_size_bytes(ctx.target_triple) as i64;
    let base = ctx.block().inttoptr(I64, handle);
    let fields = ctx.block().gep(I8, &base, &[(I64, &header.to_string())]);
    ctx.block().gep(DOUBLE, &fields, &[(I64, slot)])
}

/// `property_get::lower`'s hook: a planned-bare read in F-body.
pub(crate) fn try_lower_bare_get(ctx: &mut FnCtx<'_>, e: &Expr) -> Result<Option<String>> {
    if ctx.region_loop_facts.is_empty() {
        return Ok(None);
    }
    let Expr::PropertyGet {
        object, property, ..
    } = e
    else {
        return Ok(None);
    };
    let Some(r) = Recv::of(object) else {
        return Ok(None);
    };
    let Some(slot) = active_slot(ctx, e, r, property) else {
        return Ok(None);
    };
    let recv_box = lower_recv(ctx, r)?;
    let h = handle_of(ctx, &recv_box);
    let p = slot_ptr(ctx, &h, &slot);
    note_emitted(ctx);
    let v = ctx.block().load(DOUBLE, &p);
    stat(2, 1);
    Ok(Some(v))
}

/// The `PutValueSet` hook: a planned-bare store in F-body. The value is
/// evaluated first (the target is a binding read, so evaluating it after the
/// RHS is unobservable), then the receiver's CURRENT address is read from its
/// root, then the store and exactly the store IC's GC obligations.
pub(crate) fn try_lower_bare_put(
    ctx: &mut FnCtx<'_>,
    e: &Expr,
    target: &Expr,
    key: &Expr,
    value: &Expr,
) -> Result<Option<String>> {
    if ctx.region_loop_facts.is_empty() {
        return Ok(None);
    }
    let (Some(r), Expr::String(k)) = (Recv::of(target), key) else {
        return Ok(None);
    };
    let Some(slot) = active_slot(ctx, e, r, k) else {
        return Ok(None);
    };
    // A value the compiler proves a canonical raw double is pointer-free and
    // raw-f64-compatible with every slot: the store owes the GC nothing (the
    // store IC's own "plain double: nothing" case, decided statically).
    let raw_double = crate::type_analysis::expr_produces_canonical_raw_f64(ctx, value);
    let val_double = lower_expr(ctx, value)?;
    let val_bits = ctx.block().bitcast_double_to_i64(&val_double);
    let recv_box = lower_recv(ctx, r)?;
    let h = handle_of(ctx, &recv_box);
    let p = slot_ptr(ctx, &h, &slot);
    note_emitted(ctx);
    if raw_double {
        // GC_STORE_AUDIT(BARRIERED): a proven raw double carries no pointer.
        ctx.block().store(DOUBLE, &val_double, &p);
        stat(3, 1);
        return Ok(Some(val_double));
    }
    let reserved = field_i16(ctx, &h, -6);
    // GC_STORE_AUDIT(BARRIERED): the obligations follow, from the stored bits.
    ctx.block().store(DOUBLE, &val_double, &p);
    crate::expr::put_value_store_ic::emit_static_store_ic_bookkeeping(
        ctx,
        &h,
        &slot,
        &p,
        &reserved,
        &val_double,
        &val_bits,
        "put.pic",
    );
    stat(3, 1);
    Ok(Some(val_double))
}

/// `region_read_run`'s hook inside an F-body: a planned fact tree.
pub(crate) fn try_lower_fact_add_tree(ctx: &mut FnCtx<'_>, e: &Expr) -> Result<Option<String>> {
    let Some(a) = ctx.region_loop_facts.last() else {
        return Ok(None);
    };
    if !a.trees.contains(&(e as *const Expr as usize)) {
        return Ok(None);
    }
    let covered = |_: Recv, _: &str| true;
    let Some((r, _)) = fact_tree_leaves(e, &covered) else {
        return Ok(None);
    };
    let dirty_slot = a.dirty_slot.clone();
    fn leaves<'e>(e: &'e Expr, out: &mut Vec<&'e Expr>) {
        if let Expr::Binary {
            op: BinaryOp::Add,
            left,
            right,
        } = e
        {
            leaves(left, out);
            leaves(right, out);
        } else {
            out.push(e);
        }
    }
    let mut all = Vec::new();
    leaves(e, &mut all);
    // Effect-free leaves first, then the receiver's handle and the loads, so
    // nothing that could allocate sits between the handle and its loads.
    let mut values: Vec<Option<String>> = Vec::with_capacity(all.len());
    let mut needs: Vec<bool> = Vec::with_capacity(all.len());
    for l in &all {
        if matches!(l, Expr::PropertyGet { .. }) {
            values.push(None);
            needs.push(true);
        } else {
            values.push(Some(lower_expr(ctx, l)?));
            needs.push(!crate::type_analysis::expr_produces_canonical_raw_f64(
                ctx, l,
            ));
        }
    }
    let recv_box = lower_recv(ctx, r)?;
    let h = handle_of(ctx, &recv_box);
    for (i, l) in all.iter().enumerate() {
        if let Expr::PropertyGet { property, .. } = l {
            let slot = active_slot(ctx, l, r, property).expect("planned with its tree");
            let p = slot_ptr(ctx, &h, &slot);
            note_emitted(ctx);
            values[i] = Some(ctx.block().load(DOUBLE, &p));
            stat(2, 1);
        }
    }
    let values: Vec<String> = values.into_iter().map(|v| v.expect("lowered")).collect();
    let fold_i = ctx.new_block("rloop.tree.fold");
    let gen_i = ctx.new_block("rloop.tree.generic");
    let merge_i = ctx.new_block("rloop.tree.merge");
    let fold_l = ctx.block_label(fold_i);
    let gen_l = ctx.block_label(gen_i);
    let merge_l = ctx.block_label(merge_i);
    let mut all_num: Option<String> = None;
    for (v, n) in values.iter().zip(needs.iter()) {
        if !n {
            continue;
        }
        let is_num = crate::stmt::emit_js_value_is_number(ctx, v);
        all_num = Some(match all_num {
            Some(prev) => ctx.block().and(I1, &prev, &is_num),
            None => is_num,
        });
    }
    match all_num {
        Some(c) => ctx.block().cond_br(&c, &fold_l, &gen_l),
        None => ctx.block().br(&fold_l),
    }
    ctx.current_block = fold_i;
    fn fold(ctx: &mut FnCtx<'_>, e: &Expr, values: &[String], next: &mut usize) -> String {
        if let Expr::Binary {
            op: BinaryOp::Add,
            left,
            right,
        } = e
        {
            let l = fold(ctx, left, values, next);
            let r = fold(ctx, right, values, next);
            return ctx.block().fadd(&l, &r);
        }
        let v = values[*next].clone();
        *next += 1;
        v
    }
    let fast = fold(ctx, e, &values, &mut 0);
    let fast_end = ctx.block().label.clone();
    ctx.block().br(&merge_l);
    // The generic arm: the tree in source order through today's lowering,
    // with the facts masked (it may run JS between its reads), then the flag.
    ctx.current_block = gen_i;
    ctx.region_loop_facts.push(Active {
        receivers: Vec::new(),
        bare: HashSet::new(),
        trees: HashSet::new(),
        dirty_slot: None,
        emitted: Vec::new(),
    });
    let slow = lower_expr(ctx, e);
    ctx.region_loop_facts.pop();
    let slow = slow?;
    if let Some(d) = &dirty_slot {
        ctx.block().store(I1, "true", d);
    }
    let slow_end = ctx.block().label.clone();
    ctx.block().br(&merge_l);
    ctx.current_block = merge_i;
    Ok(Some(
        ctx.block()
            .phi(DOUBLE, &[(&fast, &fast_end), (&slow, &slow_end)]),
    ))
}

// ---------------------------------------------------------------- verify

/// Callees that cannot run JavaScript (they may allocate or collect — the
/// receiver is re-derived at every use — but they cannot reshape anything).
fn cannot_run_js(callee: &str) -> bool {
    use crate::gc_call_effects::{classify_direct_callee, GcCallEffect};
    if callee.starts_with("llvm.") {
        return true;
    }
    if matches!(
        classify_direct_callee(callee),
        GcCallEffect::CannotCollect | GcCallEffect::AllocNoReentry
    ) {
        return true;
    }
    crate::root_reload::is_non_collecting(callee)
        || matches!(
            callee,
            "js_gc_loop_safepoint"
                | "js_gc_note_slot_layout"
                | "js_gc_note_slot_layout_aware"
                | "js_write_barrier_slot_validated_parent"
                | "js_write_barrier_slot"
                | "js_write_barrier_root_nanbox"
                | "js_write_barrier_root_heap_word"
                | "js_string_addref_if_heap_string"
                | "js_region_loop_prime"
        )
}

fn inst_may_run_js(inst: &crate::inst::LlInst) -> bool {
    use crate::inst::LlInst;
    match inst {
        LlInst::Call { callee, .. } => !cannot_run_js(callee),
        LlInst::CallIndirect { .. } => true,
        LlInst::Raw(s) => {
            let t = s.trim_start();
            if !(t.contains("call ") || t.starts_with("invoke") || t.contains(" invoke ")) {
                return false;
            }
            if t.contains(" asm ") {
                return false;
            }
            match t.find('@') {
                Some(at) => {
                    let name: String = t[at + 1..]
                        .chars()
                        .take_while(|c| c.is_alphanumeric() || *c == '_' || *c == '.' || *c == '$')
                        .collect();
                    !cannot_run_js(&name)
                }
                None => true,
            }
        }
        _ => false,
    }
}

fn successors(block: &crate::block::LlBlock) -> Vec<String> {
    use crate::inst::LlInst;
    match block.insts().last() {
        Some(LlInst::Br { label }) => vec![label.clone()],
        Some(LlInst::CondBr { t, f, .. }) => vec![t.clone(), f.clone()],
        Some(LlInst::Raw(s)) => s
            .split("label %")
            .skip(1)
            .map(|x| {
                x.chars()
                    .take_while(|c| c.is_alphanumeric() || *c == '_' || *c == '.' || *c == '$')
                    .collect()
            })
            .collect(),
        _ => Vec::new(),
    }
}

/// Recompute fresh/stale over the EMITTED F-body blocks and require every
/// bare access to be reached only fresh.
fn verify(
    ctx: &FnCtx<'_>,
    entry: usize,
    scan_start: usize,
    scan_end: usize,
    emitted: &[(usize, usize)],
) -> bool {
    let blocks = ctx.func.blocks();
    let in_f = |b: usize| b == entry || (scan_start..scan_end).contains(&b);
    let mut by_label: HashMap<&str, usize> = HashMap::new();
    for b in (scan_start..scan_end).chain(std::iter::once(entry)) {
        by_label.insert(blocks[b].label.as_str(), b);
    }
    // stale_in[b]: some path from the F entry reaches b after a JS-capable call.
    let mut stale_in: HashMap<usize, bool> = HashMap::new();
    let mut reached: HashSet<usize> = HashSet::new();
    let mut work = vec![(entry, false)];
    while let Some((b, stale)) = work.pop() {
        let seen = reached.contains(&b);
        let prev = stale_in.get(&b).copied().unwrap_or(false);
        if seen && (prev || !stale) {
            continue;
        }
        reached.insert(b);
        stale_in.insert(b, prev || stale);
        let mut s = prev || stale;
        for inst in blocks[b].insts() {
            if inst_may_run_js(inst) {
                s = true;
            }
        }
        for succ in successors(&blocks[b]) {
            if let Some(&nb) = by_label.get(succ.as_str()) {
                if in_f(nb) {
                    work.push((nb, s));
                }
            }
        }
    }
    for &(b, i) in emitted {
        let mut s = match stale_in.get(&b) {
            Some(v) => *v,
            None => continue,
        };
        for inst in &blocks[b].insts()[..i.min(blocks[b].insts().len())] {
            if inst_may_run_js(inst) {
                s = true;
            }
        }
        if s {
            return false;
        }
    }
    true
}
