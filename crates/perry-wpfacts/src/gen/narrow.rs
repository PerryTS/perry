//! Loop-test narrowing of a counter's interval (`model::range`).
//!
//! In `for (let i = a; i < e; u) body`, every read of `i` in `body` before
//! the body writes `i` sees a value that passed the test: it reads the
//! narrowed node `start`, which holds `i`'s values with the interval the test
//! leaves (`RangeOp::Clip`). From the body's first write on, and in the
//! update, `i` holds the start value or a value the body wrote: those reads
//! see `end`, the join of `start` and every body write. The counter's own
//! node is the join of the init and the update's writes, so the cycle
//! `i -> start -> end -> update -> i` stops at the test bound instead of
//! widening away.
//!
//! ## Guards
//!
//! The same split narrows a local by any comparison that guards code
//! (`narrow_guard`): an `if`'s branches, a `while` body, the right operand of
//! `&&`/`||`, the arms of `?:`, and the statements after an `if` whose branch
//! always leaves the block (`return`, `throw`, `break`, `continue`). A
//! condition implies the clips of its ordered comparisons of a local with a
//! bound through `&&` (true), `||` (false) and `!`. A negated comparison is
//! sound for the interval: a Number in a proper interval is an integer, never
//! NaN, so `!(x < b)` is `x >= b` for it; a ⊤ host or bound clips nothing.
//! Only a local of the body being walked that no closure captures mutably
//! (and no hoisted box holds) is narrowed (`scope_locals`).
//!
//! The same conditions narrow a local by presence (`Implied::Present`): a
//! truthy test of it (`x`, `!x` false), `x != null` true or `x == null`
//! false leave its values but `null` and `undefined` (`Cons::NotNullish`),
//! and `x !== undefined` true or `x === undefined` false leave its values
//! but `undefined` (`Cons::NotUndefined`). It is one predicate with the
//! clips: what an `if` implies, `&&`, `||`, `?:`, a `while` and an early
//! exit imply alike, and a write in the guarded code joins the written
//! value back (the end node). `PERRY_FACTS_GUARD_PRESENCE=0` turns it off.
//! Rule G2: a truthy test of a
//! local's property (`x.f`, `!x.f` false) leaves its values but
//! `null`/`undefined` and the objects whose read of `f` holds only
//! `undefined`/`null` (`Cons::FieldTruthy`, solved per token).
//! Rule G3: where `x.f === lit` is true (a String, Number, `null` or
//! `undefined` literal), and in a `switch (x.f)` clause that only `case`
//! tests of such literals of one type select (`Gen::case_narrow`), `x`
//! holds only the values whose read of `f` may yield a value of the
//! literal's type (ECMA-262 7.2.16 step 1, 14.12.2 CaseClauseIsSelected).
//! Rule G4: where `typeof x.f === s` is true (`!==` false), `s` the String
//! "number", "string" or "undefined", `x` holds only the values whose read
//! of `f` may yield a value of that type: the G3 test of a literal of the
//! type (ECMA-262 13.5.3 typeof table, 7.2.15 IsStrictlyEqual of Strings).
//! The other `typeof` results have no exact test; the false side implies
//! nothing.
//!
//! The bound is a literal, a local, or a local's `length` (an Array's
//! carries its length interval, `solve::arr_len`).
//!
//! ## Writes on a leaving path
//!
//! A write ends the start reads for the code it can reach, not for the
//! program text after it. A branch of an `if` that always leaves
//! (`always_exits`) reaches the code after the `if` only through its jumps,
//! so after the branch each narrowing's `written` is what it was before it
//! when every way out of the branch lands outside the narrowing's code
//! (`jumps_leave`):
//! - an unlabeled `break`/`continue` out of the branch, with no loop or
//!   `switch` between the `if` and the narrowing (`Gen::jumps`): it lands at
//!   the narrowed loop's update (which reads the end node) or past the
//!   guarded code;
//! - a `return` or `throw`, and any exception, with no `try` between them
//!   (`Gen::tries`): no `catch` or `finally` of the narrowing's code runs
//!   after it;
//! - no labeled jump (where it lands is not followed).
//!
//! The write still reaches the end node and the local, so the update and
//! every read the jump reaches see it. `PERRY_FACTS_LEAVING_WRITES=0` ends
//! the start reads at the first write in the text again.
//!
//! The rule only applies where no other code can write `i` during the loop:
//! `i` is declared by the loop's `init` and no closure appears in the loop.
//! A nested loop that writes `i` ends the start reads before it (its body
//! reads may run after its own write). Reads in another function body
//! (a closure walked meanwhile) are never substituted.

use super::*;
use perry_hir::{CompareOp, Expr, Stmt};

#[derive(Clone, Copy, Debug)]
pub(crate) struct Narrow {
    mi: u32,
    id: u32,
    f: FnIdx,
    start: NodeId,
    end: NodeId,
    written: bool,
    /// The update is walked: it reads the end node, and its write (which
    /// runs after the body) reaches only the counter.
    in_update: bool,
    /// `Gen::jumps` and `Gen::tries` where the narrowed code starts (for a
    /// loop, inside it): "Writes on a leaving path".
    jumps: u32,
    tries: u32,
    /// The test implies `local < a.length` for this local `a`
    /// (`gen/in_bounds.rs`).
    below_length: Option<u32>,
}

impl Narrow {
    /// (module, local, function body).
    pub(crate) fn key(&self) -> (u32, u32, FnIdx) {
        (self.mi, self.id, self.f)
    }

    /// The local still holds the value the test saw.
    pub(crate) fn unwritten(&self) -> bool {
        !self.written
    }

    pub(crate) fn below_length_of(&self) -> Option<u32> {
        self.below_length
    }
}

/// The local `a` of a clip `< a.length`.
fn below_length(kind: u8, bound: &Bound) -> Option<u32> {
    match bound {
        Bound::Length(a) if kind == crate::model::range::CLIP_LT => Some(*a),
        _ => None,
    }
}

fn expr_has(e: &Expr, p: &mut impl FnMut(&Expr) -> bool) -> bool {
    if p(e) {
        return true;
    }
    let mut hit = false;
    perry_hir::walker::walk_expr_children(e, &mut |c| {
        if !hit && expr_has(c, p) {
            hit = true;
        }
    });
    hit
}

fn stmt_has(s: &Stmt, p: &mut impl FnMut(&Expr) -> bool) -> bool {
    perry_hir::walker::stmt_any_expr(s, &mut |e| expr_has(e, p))
}

fn writes(e: &Expr, id: u32) -> bool {
    matches!(e, Expr::LocalSet(l, _) | Expr::Update { id: l, .. } if *l == id)
}

impl<'a> Gen<'a> {
    /// Start narrowing the counter of a `for` loop (see the module docs);
    /// whether it did. Called after the test is walked, before the update
    /// and the body.
    pub(crate) fn narrow_for(
        &mut self,
        fx: &Fx,
        init: Option<&Stmt>,
        condition: Option<&Expr>,
        update: Option<&Expr>,
        body: &[Stmt],
    ) -> bool {
        if !self.int_ranges {
            return false;
        }
        let Some(Stmt::Let { id, .. }) = init else {
            return false;
        };
        let id = *id;
        // Every comparison of the counter the test implies (`implied`:
        // through `&&`, predicates, pure bound expressions).
        let Some(cond) = condition else {
            return false;
        };
        let module: &'a perry_hir::Module = self.modules[fx.module as usize].1;
        let pred = |fid: u32| -> Option<Predicate<'a>> {
            module
                .functions
                .iter()
                .find(|f| f.id == fid)
                .and_then(predicate_of)
        };
        let mut facts = Vec::new();
        implied(cond, true, &|l| Some(Bound::Local(l)), &pred, 0, &mut facts);
        // Without guard ranges (`FactsConfig::no_guard_ranges`) only a test
        // that is one comparison with a literal, a local or a `length`.
        let simple = self.guard_ranges || matches!(cond, Expr::Compare { .. });
        let clips: Vec<(u8, Bound)> = facts
            .into_iter()
            .filter_map(|f| match f {
                Implied::Clip(l, k, b) => Some((l, k, b)),
                Implied::Present(..) => None,
            })
            .filter(|(l, _, b)| {
                *l == id
                    && !matches!(b, Bound::Char(_))
                    && (self.guard_ranges || !matches!(b, Bound::Op(..)))
            })
            .filter(|_| simple)
            .map(|(_, k, b)| (k, b))
            .collect();
        if clips.is_empty() {
            return false;
        }
        let closure = &mut |e: &Expr| matches!(e, Expr::Closure { .. });
        if body.iter().any(|s| stmt_has(s, closure))
            || update.is_some_and(|u| expr_has(u, closure))
            || expr_has(cond, closure)
        {
            return false;
        }
        let mi = fx.module;
        let i = self.local(mi, id);
        // One clip per comparison, chained: the reads see their meet.
        let mut start = i;
        let mut below = None;
        for (kind, bound) in clips {
            below = below.or(below_length(kind, &bound));
            let b = self.bound_node(mi, bound);
            let dst = self.tmp("narrow");
            self.zs_carrier(dst);
            self.m.edge(start, dst);
            self.range_rule(crate::model::RangeOp::Clip(kind), start, Some(b), dst);
            start = dst;
        }
        let end = self.tmp("narrow-end");
        self.zs_carrier(end);
        self.m.edge(start, end);
        self.narrow.push(Narrow {
            mi,
            id,
            f: self.m.cur_fn,
            start,
            end,
            written: false,
            in_update: false,
            jumps: self.jumps,
            tries: self.tries,
            below_length: below,
        });
        true
    }

    /// Walking the update (`true`) or the body (`false`) of the innermost
    /// narrowing.
    pub(crate) fn narrow_in_update(&mut self, on: bool) {
        if let Some(n) = self.narrow.last_mut() {
            n.in_update = on;
            n.written = on;
        }
    }

    pub(crate) fn narrow_end(&mut self) {
        self.narrow.pop();
    }

    /// The node a read of local `id` sees under a narrowing, if any.
    pub(crate) fn narrowed_read(&self, mi: u32, id: u32) -> Option<NodeId> {
        let f = self.m.cur_fn;
        self.narrow
            .iter()
            .rev()
            .find(|n| n.mi == mi && n.id == id && n.f == f)
            .map(|n| if n.written { n.end } else { n.start })
    }

    /// A write of `vn` into local `id`: it reaches every narrowing's end
    /// node, and later reads see the end.
    pub(crate) fn narrowed_write(&mut self, mi: u32, id: u32, vn: NodeId) {
        let f = self.m.cur_fn;
        let ends: Vec<NodeId> = self
            .narrow
            .iter_mut()
            .filter(|n| n.mi == mi && n.id == id && n.f == f && !n.in_update)
            .map(|n| {
                n.written = true;
                n.end
            })
            .collect();
        for e in ends {
            self.m.edge(vn, e);
        }
    }

    /// Before walking a branch of an `if`: each narrowing's `written`.
    pub(crate) fn narrow_snapshot(&self) -> Vec<bool> {
        if !self.leaving_writes {
            return Vec::new();
        }
        self.narrow.iter().map(|n| n.written).collect()
    }

    /// After walking `branch` (`pre` from `narrow_snapshot` before it): a
    /// write in a branch that always leaves past a narrowing's code does
    /// not reach the reads after the `if` (module docs, "Writes on a
    /// leaving path").
    pub(crate) fn narrow_leave(&mut self, branch: &[Stmt], pre: Vec<bool>) {
        if pre.is_empty() {
            return;
        }
        let sabotage = self.leaving_write_sabotage;
        if !sabotage && !(always_exits(branch) && jumps_leave(branch)) {
            return;
        }
        let (f, jumps, tries) = (self.m.cur_fn, self.jumps, self.tries);
        for (n, w) in self.narrow.iter_mut().zip(pre) {
            if n.f == f && (sabotage || (n.jumps == jumps && n.tries == tries)) {
                n.written = w;
            }
        }
    }

    /// Before walking a nested loop: a narrowed counter it writes is read
    /// through the end node inside it (a read there may follow the write).
    pub(crate) fn narrow_enter_loop(&mut self, s: &Stmt) {
        if self.narrow.is_empty() {
            return;
        }
        let f = self.m.cur_fn;
        for i in 0..self.narrow.len() {
            let n = self.narrow[i];
            if n.f == f && !n.written && stmt_has(s, &mut |e| writes(e, n.id)) {
                self.narrow[i].written = true;
            }
        }
    }
}

/// The locals of a function body (its parameters and every `let` outside
/// nested function bodies) that no code but the body itself can write: not
/// captured mutably by a closure in it, not in a hoisted box.
pub(crate) fn scope_locals(
    params: &[perry_hir::Param],
    body: &[Stmt],
) -> crate::pts::FxHashSet<u32> {
    let mut decl = crate::pts::FxHashSet::default();
    let mut excl = crate::pts::FxHashSet::default();
    for p in params {
        decl.insert(p.id);
    }
    scan_stmts(body, &mut decl, &mut excl);
    decl.retain(|id| !excl.contains(id));
    decl
}

fn scan_stmts(
    ss: &[Stmt],
    decl: &mut crate::pts::FxHashSet<u32>,
    excl: &mut crate::pts::FxHashSet<u32>,
) {
    for s in ss {
        scan_stmt(s, decl, excl);
    }
}

fn scan_stmt(
    s: &Stmt,
    decl: &mut crate::pts::FxHashSet<u32>,
    excl: &mut crate::pts::FxHashSet<u32>,
) {
    match s {
        Stmt::Let { id, .. } => {
            decl.insert(*id);
        }
        Stmt::If {
            then_branch,
            else_branch,
            ..
        } => {
            scan_stmts(then_branch, decl, excl);
            if let Some(e) = else_branch {
                scan_stmts(e, decl, excl);
            }
        }
        Stmt::While { body, .. } | Stmt::DoWhile { body, .. } => scan_stmts(body, decl, excl),
        Stmt::For { init, body, .. } => {
            if let Some(i) = init {
                scan_stmt(i, decl, excl);
            }
            scan_stmts(body, decl, excl);
        }
        Stmt::Labeled { body, .. } => scan_stmt(body, decl, excl),
        Stmt::Try {
            body,
            catch,
            finally,
        } => {
            scan_stmts(body, decl, excl);
            if let Some(c) = catch {
                scan_stmts(&c.body, decl, excl);
            }
            if let Some(f) = finally {
                scan_stmts(f, decl, excl);
            }
        }
        Stmt::Switch { cases, .. } => {
            for c in cases {
                scan_stmts(&c.body, decl, excl);
            }
        }
        Stmt::PreallocateBoxes(ids) | Stmt::PreallocateTdzBoxes(ids) => {
            excl.extend(ids.iter().copied());
        }
        _ => {}
    }
    perry_hir::walker::stmt_any_expr(s, &mut |e| {
        closure_writes(e, excl);
        false
    });
}

/// Every local a closure in `e` (or nested in one) may write.
fn closure_writes(e: &Expr, excl: &mut crate::pts::FxHashSet<u32>) {
    let _ = expr_has(e, &mut |x| {
        if let Expr::Closure {
            mutable_captures,
            body,
            ..
        } = x
        {
            excl.extend(mutable_captures.iter().copied());
            let mut inner = crate::pts::FxHashSet::default();
            scan_stmts(body, &mut inner, excl);
        }
        false
    });
}

/// The `const` bindings a body declares itself (`Stmt::Let` with an
/// initializer and no `mutable`), outside its nested function bodies (each
/// adds its own when walked). A const is written once, by its
/// initialization, and a read before it throws (TDZ): every read sees the
/// one value, in this body and in every closure that captures it.
fn own_consts(ss: &[Stmt], out: &mut Vec<u32>) {
    for s in ss {
        match s {
            Stmt::Let {
                id,
                mutable: false,
                init: Some(_),
                ..
            } => out.push(*id),
            Stmt::If {
                then_branch,
                else_branch,
                ..
            } => {
                own_consts(then_branch, out);
                if let Some(e) = else_branch {
                    own_consts(e, out);
                }
            }
            Stmt::While { body, .. } | Stmt::DoWhile { body, .. } => own_consts(body, out),
            Stmt::For { init, body, .. } => {
                if let Some(i) = init {
                    own_consts(std::slice::from_ref(i), out);
                }
                own_consts(body, out);
            }
            Stmt::Labeled { body, .. } => own_consts(std::slice::from_ref(body), out),
            Stmt::Try {
                body,
                catch,
                finally,
            } => {
                own_consts(body, out);
                if let Some(c) = catch {
                    own_consts(&c.body, out);
                }
                if let Some(f) = finally {
                    own_consts(f, out);
                }
            }
            Stmt::Switch { cases, .. } => {
                for c in cases {
                    own_consts(&c.body, out);
                }
            }
            _ => {}
        }
    }
}

/// What a guard may narrow in the body walked next: its own locals
/// (`scope_locals`) and the consts of the bodies around it that it
/// captures (`Gen::narrow_consts`). `exit` restores the walk's state.
pub(crate) struct Scope {
    saved: Option<(FnIdx, crate::pts::FxHashSet<u32>)>,
    mi: u32,
    own: Vec<u32>,
    /// The enclosing body's in-bounds state (`gen/in_bounds.rs`).
    in_bounds: (
        Option<super::in_bounds::BodyLocals>,
        Vec<super::in_bounds::Derived>,
    ),
}

impl<'a> Gen<'a> {
    pub(crate) fn narrow_enter_body(
        &mut self,
        mi: u32,
        f: FnIdx,
        params: &[perry_hir::Param],
        body: &[Stmt],
        captures: &[u32],
    ) -> Scope {
        let mut own = Vec::new();
        let scope = (self.int_ranges || self.guard_presence).then(|| {
            let mut ls = scope_locals(params, body);
            // `this` of a body with its own binding (not an arrow's, which
            // is its enclosing body's).
            // PERRY_WPF_NO_THIS_HOST: diagnostic switch (default off) that
            // leaves `this` out of the narrowing hosts.
            if !self.m.funcs[f as usize].lexical_this
                && std::env::var_os("PERRY_WPF_NO_THIS_HOST").is_none()
            {
                ls.insert(THIS_LOCAL);
            }
            ls.extend(
                captures
                    .iter()
                    .filter(|c| self.narrow_consts.contains(&(mi, **c)))
                    .copied(),
            );
            own_consts(body, &mut own);
            self.narrow_consts.extend(own.iter().map(|&c| (mi, c)));
            (f, ls)
        });
        let in_bounds = self.in_bounds_enter_body(f, params, body);
        Scope {
            saved: std::mem::replace(&mut self.narrow_scope, scope),
            mi,
            own,
            in_bounds,
        }
    }

    pub(crate) fn narrow_exit_body(&mut self, s: Scope) {
        self.in_bounds_exit_body(s.in_bounds);
        for &id in &s.own {
            self.narrow_consts.remove(&(s.mi, id));
        }
        self.narrow_scope = s.saved;
    }
}

/// Whether `ss` always leaves the enclosing statement list.
pub(crate) fn always_exits(ss: &[Stmt]) -> bool {
    matches!(
        ss.last(),
        Some(
            Stmt::Return(_)
                | Stmt::Throw(_)
                | Stmt::Break
                | Stmt::Continue
                | Stmt::LabeledBreak(_)
                | Stmt::LabeledContinue(_)
        )
    )
}

/// Whether every jump out of `ss` lands where an unlabeled jump at the
/// statement around `ss` lands, or leaves the function: no labeled `break`
/// or `continue` (where it lands is not followed). A `break` inside a loop
/// or `switch` of `ss`, and a `continue` inside a loop of it, stay in `ss`.
/// Nested function bodies are expressions: not walked.
pub(crate) fn jumps_leave(ss: &[Stmt]) -> bool {
    ss.iter().all(|s| match s {
        Stmt::LabeledBreak(_) | Stmt::LabeledContinue(_) => false,
        Stmt::If {
            then_branch,
            else_branch,
            ..
        } => jumps_leave(then_branch) && else_branch.as_deref().is_none_or(jumps_leave),
        Stmt::While { body, .. } | Stmt::DoWhile { body, .. } | Stmt::For { body, .. } => {
            jumps_leave(body)
        }
        Stmt::Switch { cases, .. } => cases.iter().all(|c| jumps_leave(&c.body)),
        Stmt::Try {
            body,
            catch,
            finally,
        } => {
            jumps_leave(body)
                && catch.as_ref().is_none_or(|c| jumps_leave(&c.body))
                && finally.as_deref().is_none_or(jumps_leave)
        }
        Stmt::Labeled { body, .. } => jumps_leave(std::slice::from_ref(body)),
        _ => true,
    })
}

/// Whether running `ss` may reach its end: a function body that does
/// returns `undefined` there. Conservative: a `return` or `throw` on every
/// path (through `if`/`else`, `try`, a `switch` with a `default` and an
/// infinite loop, none of them broken out of) ends it abruptly; anything
/// else may complete.
pub(crate) fn may_complete(ss: &[Stmt]) -> bool {
    !ss.iter().any(|s| match s {
        Stmt::Switch { cases, .. } => {
            cases.iter().any(|c| c.test.is_none())
                && !cases.iter().any(|c| breaks_out(&c.body))
                && cases.last().is_some_and(|c| !may_complete(&c.body))
        }
        Stmt::While {
            condition: Expr::Bool(true),
            body,
        }
        | Stmt::DoWhile {
            condition: Expr::Bool(true),
            body,
        }
        | Stmt::For {
            condition: None,
            body,
            ..
        } => !breaks_out(body),
        Stmt::Return(_) | Stmt::Throw(_) => true,
        Stmt::If {
            then_branch,
            else_branch: Some(else_branch),
            ..
        } => !may_complete(then_branch) && !may_complete(else_branch),
        Stmt::Try {
            body,
            catch,
            finally,
        } => {
            finally.as_deref().is_some_and(|f| !may_complete(f))
                || (!may_complete(body) && catch.as_ref().is_none_or(|c| !may_complete(&c.body)))
        }
        _ => false,
    })
}

/// Whether `ss` may leave the loop or `switch` it is the body of by a
/// `break` (any labeled `break` counts, wherever it is).
fn breaks_out(ss: &[Stmt]) -> bool {
    fn labeled(ss: &[Stmt]) -> bool {
        ss.iter().any(|s| match s {
            Stmt::LabeledBreak(_) => true,
            Stmt::If {
                then_branch,
                else_branch,
                ..
            } => labeled(then_branch) || else_branch.as_deref().is_some_and(labeled),
            Stmt::While { body, .. } | Stmt::DoWhile { body, .. } | Stmt::For { body, .. } => {
                labeled(body)
            }
            Stmt::Switch { cases, .. } => cases.iter().any(|c| labeled(&c.body)),
            Stmt::Try {
                body,
                catch,
                finally,
            } => {
                labeled(body)
                    || catch.as_ref().is_some_and(|c| labeled(&c.body))
                    || finally.as_deref().is_some_and(labeled)
            }
            Stmt::Labeled { body, .. } => labeled(std::slice::from_ref(body)),
            _ => false,
        })
    }
    ss.iter().any(|s| match s {
        Stmt::Break | Stmt::LabeledBreak(_) => true,
        Stmt::If {
            then_branch,
            else_branch,
            ..
        } => breaks_out(then_branch) || else_branch.as_deref().is_some_and(breaks_out),
        Stmt::Try {
            body,
            catch,
            finally,
        } => {
            breaks_out(body)
                || catch.as_ref().is_some_and(|c| breaks_out(&c.body))
                || finally.as_deref().is_some_and(breaks_out)
        }
        Stmt::Labeled { body, .. } => breaks_out(std::slice::from_ref(body)),
        // An inner loop's or switch's own `break` stays inside it.
        Stmt::While { body, .. } | Stmt::DoWhile { body, .. } | Stmt::For { body, .. } => {
            labeled(body)
        }
        Stmt::Switch { cases, .. } => cases.iter().any(|c| labeled(&c.body)),
        _ => false,
    })
}

/// A clip's bound: a literal, a local, a local's `length`, or a pure
/// integer expression of those.
#[derive(Clone, Debug)]
pub(crate) enum Bound {
    Lit(f64),
    Local(u32),
    Length(u32),
    /// A one-unit String literal (its code; `""` is -1): the clip is of
    /// the local's Strings (`RangeOp::SClip`).
    Char(i64),
    /// `op` over bounds (`Math.min(7, file + 1)`): its node is the rules'
    /// result, so an operand that may be anything but a Number makes it ⊤
    /// (no clip).
    Op(crate::model::RangeOp, std::rc::Rc<(Bound, Option<Bound>)>),
}

impl Bound {
    /// Whether the bound reads local `id`.
    fn reads(&self, id: u32) -> bool {
        match self {
            Bound::Local(l) | Bound::Length(l) => *l == id,
            Bound::Lit(_) | Bound::Char(_) => false,
            Bound::Op(_, ab) => ab.0.reads(id) || ab.1.as_ref().is_some_and(|b| b.reads(id)),
        }
    }
}

/// A predicate function: one `return <expr>` over its plain parameters.
pub(crate) type Predicate<'e> = (&'e [perry_hir::Param], &'e Expr);

/// What a guard's outcome implies of one local.
#[derive(Clone, Debug)]
pub(crate) enum Implied {
    /// `(local, clip kind, bound)`: an ordered comparison of the local.
    Clip(u32, u8, Bound),
    /// The local holds no value the presence excludes: a truthy test of
    /// it, or a failed (passed) comparison of it with `null`/`undefined`.
    Present(u32, Presence),
}

/// The values a guard proves a local is not.
#[derive(Clone, Debug)]
pub(crate) enum Presence {
    /// Neither `null` nor `undefined` (`Cons::NotNullish`): a truthy value,
    /// or one that is not `== null`.
    NotNullish,
    /// Not `undefined` (`Cons::NotUndefined`): one that is not
    /// `=== undefined`.
    NotUndefined,
    /// `local.key` decided by a test of the read (`Cons::FieldTruthy`):
    /// rule G2 `guard::TRUTHY` (truthy), rule G1 `guard::IS_FALSE`/
    /// `NOT_FALSE`/`IS_TRUE`/`NOT_TRUE` (`=== false`/`true` decided). Neither
    /// `null`/`undefined` (the read would throw) nor an object whose read
    /// of `key` cannot pass the test.
    FieldTruthy(String, u8),
    /// `local instanceof ArrayBuffer` (`true`: `SharedArrayBuffer`) failed,
    /// the bare global name (`Cons::NotBuffer`): no built-in buffer object
    /// of that kind, while the global holds the intrinsic constructor.
    NotBuffer(bool),
}

/// What condition `c` implies when it is `positive`ly true (false): a clip
/// for each ordered comparison of a local, and a presence for each truthy
/// test of a local or nullish comparison it decides. Both go through `&&`
/// (true), `||` (false), `!` and predicates alike, the one rule of every
/// guard (module docs, "Guards"). `pred` resolves a called function to its
/// predicate body (`subst` maps its parameters to the caller's locals and
/// literals).
fn implied<'p>(
    c: &Expr,
    positive: bool,
    subst: &dyn Fn(u32) -> Option<Bound>,
    pred: &dyn Fn(u32) -> Option<Predicate<'p>>,
    depth: u8,
    out: &mut Vec<Implied>,
) {
    use crate::model::range::{CLIP_GE, CLIP_GT, CLIP_LE, CLIP_LT};
    use perry_hir::{LogicalOp, UnaryOp};
    match c {
        Expr::Logical {
            op: LogicalOp::And,
            left,
            right,
        } if positive => {
            implied(left, true, subst, pred, depth, out);
            implied(right, true, subst, pred, depth, out);
        }
        Expr::Logical {
            op: LogicalOp::Or,
            left,
            right,
        } if !positive => {
            implied(left, false, subst, pred, depth, out);
            implied(right, false, subst, pred, depth, out);
        }
        Expr::Unary {
            op: UnaryOp::Not,
            operand,
        } => implied(operand, !positive, subst, pred, depth, out),
        // A predicate's result is its body's: what the body implies of its
        // parameters, it implies of the arguments that are locals.
        Expr::Call { callee, args, .. } if depth < 2 => {
            let Expr::FuncRef(fid) = callee.as_ref() else {
                return;
            };
            let Some((params, body)) = pred(*fid) else {
                return;
            };
            if params.len() != args.len() {
                return;
            }
            let actual: Vec<Option<Bound>> = args.iter().map(|a| bound_of(a, subst)).collect();
            let inner = |id: u32| {
                params
                    .iter()
                    .position(|p| p.id == id)
                    .and_then(|i| actual[i].clone())
            };
            let mut found = Vec::new();
            implied(body, positive, &inner, pred, depth + 1, &mut found);
            // Only a fact of a parameter that the call binds to a local.
            out.extend(found);
        }
        // Rule G2: a truthy `local.key` (module docs, "Guards").
        Expr::PropertyGet {
            object, property, ..
        } if positive => {
            if let Some(id) = host_of(object, subst, depth) {
                out.push(Implied::Present(
                    id,
                    Presence::FieldTruthy(property.clone(), crate::model::guard::TRUTHY),
                ));
            }
        }
        // Rule G1: `local.key === b` (`!==`) with `b` a Boolean constant,
        // decided (ECMA-262 7.2.16 IsStrictlyEqual: only `b` is `b`).
        Expr::Compare {
            op: op @ (CompareOp::Eq | CompareOp::Ne),
            left,
            right,
        } if matches!(
            (left.as_ref(), right.as_ref()),
            (Expr::Bool(_), Expr::PropertyGet { .. }) | (Expr::PropertyGet { .. }, Expr::Bool(_))
        ) =>
        {
            use crate::model::guard;
            let ((Expr::Bool(b), Expr::PropertyGet { object, property, .. })
            | (Expr::PropertyGet { object, property, .. }, Expr::Bool(b))) =
                (left.as_ref(), right.as_ref())
            else {
                return;
            };
            // The read is `b` where `===` is true.
            let is = matches!(op, CompareOp::Eq) == positive;
            let test = match (is, *b) {
                (true, false) => guard::IS_FALSE,
                (false, false) => guard::NOT_FALSE,
                (true, true) => guard::IS_TRUE,
                (false, true) => guard::NOT_TRUE,
            };
            if let Some(id) = host_of(object, subst, depth) {
                out.push(Implied::Present(id, Presence::FieldTruthy(property.clone(), test)));
            }
        }
        // A failed `local instanceof ArrayBuffer` (`SharedArrayBuffer`),
        // the right-hand side the global name's live binding: the runtime
        // answers the intrinsic by the buffer brand, so no built-in buffer
        // of that kind is the local. A local, a program class or any other
        // right-hand side decides nothing here.
        Expr::InstanceOf {
            expr,
            ty_expr: Some(rhs),
            ..
        } if !positive => {
            let shared = match rhs.as_ref() {
                Expr::PropertyGet {
                    object, property, ..
                } if matches!(object.as_ref(), Expr::GlobalGet(0)) => match property.as_str() {
                    "ArrayBuffer" => Some(false),
                    "SharedArrayBuffer" => Some(true),
                    _ => None,
                },
                _ => None,
            };
            if let (Some(shared), Some(Bound::Local(id))) = (shared, bound_of(expr, subst)) {
                out.push(Implied::Present(id, Presence::NotBuffer(shared)));
            }
        }
        // A truthy local is neither `null` nor `undefined`.
        Expr::LocalGet(_) if positive => {
            if let Some(Bound::Local(id)) = bound_of(c, subst) {
                out.push(Implied::Present(id, Presence::NotNullish));
            }
        }
        // Rule G3: `local.key === lit` true (`!==` false), `lit` a String,
        // Number, `null` or `undefined` literal: the read is of its type.
        Expr::Compare {
            op: op @ (CompareOp::Eq | CompareOp::Ne),
            left,
            right,
        } if (matches!(op, CompareOp::Eq) == positive)
            && match (left.as_ref(), right.as_ref()) {
                (Expr::PropertyGet { .. }, l) | (l, Expr::PropertyGet { .. }) => {
                    literal_type_test(l).is_some()
                }
                _ => false,
            } =>
        {
            let ((Expr::PropertyGet { object, property, .. }, l)
            | (l, Expr::PropertyGet { object, property, .. })) = (left.as_ref(), right.as_ref())
            else {
                return;
            };
            if let (Some(test), Some(id)) = (literal_type_test(l), host_of(object, subst, depth)) {
                out.push(Implied::Present(id, Presence::FieldTruthy(property.clone(), test)));
            }
        }
        // Rule G4: `typeof local.key === s` true (`!==` false), `s` a String
        // whose type has a G3 test: the read is of that type.
        Expr::Compare {
            op: op @ (CompareOp::Eq | CompareOp::Ne),
            left,
            right,
        } if (matches!(op, CompareOp::Eq) == positive)
            && typeof_member_test(left, right).is_some() =>
        {
            let Some((object, property, test)) = typeof_member_test(left, right) else {
                return;
            };
            if let Some(id) = host_of(object, subst, depth) {
                out.push(Implied::Present(id, Presence::FieldTruthy(property.clone(), test)));
            }
        }
        // `local == null` (`undefined`) false, `local != null` true: not
        // nullish. `local === undefined` false, `!==` true: not
        // `undefined`. (`=== null` decides no presence a filter keeps.)
        Expr::Compare { op, left, right }
            if matches!(left.as_ref(), Expr::Undefined | Expr::Null)
                || matches!(right.as_ref(), Expr::Undefined | Expr::Null) =>
        {
            let (x, nil) = match (left.as_ref(), right.as_ref()) {
                (n @ (Expr::Undefined | Expr::Null), x) => (x, n),
                (x, n) => (x, n),
            };
            let presence = match (op, positive) {
                (CompareOp::LooseEq, false) | (CompareOp::LooseNe, true) => {
                    Some(Presence::NotNullish)
                }
                (CompareOp::Eq, false) | (CompareOp::Ne, true)
                    if matches!(nil, Expr::Undefined) =>
                {
                    Some(Presence::NotUndefined)
                }
                _ => None,
            };
            if let (Some(p), Some(Bound::Local(id))) = (presence, bound_of(x, subst)) {
                out.push(Implied::Present(id, p));
            }
        }
        Expr::Compare { op, left, right } => {
            // The clip of `local OP bound`, `OP` read for a true comparison.
            let k = |op: &CompareOp, positive: bool| -> Option<u8> {
                Some(match (op, positive) {
                    (CompareOp::Lt, true) | (CompareOp::Ge, false) => CLIP_LT,
                    (CompareOp::Le, true) | (CompareOp::Gt, false) => CLIP_LE,
                    (CompareOp::Gt, true) | (CompareOp::Le, false) => CLIP_GT,
                    (CompareOp::Ge, true) | (CompareOp::Lt, false) => CLIP_GE,
                    _ => return None,
                })
            };
            let mirror = |op: &CompareOp| match op {
                CompareOp::Lt => CompareOp::Gt,
                CompareOp::Le => CompareOp::Ge,
                CompareOp::Gt => CompareOp::Lt,
                CompareOp::Ge => CompareOp::Le,
                o => o.clone(),
            };
            let (l, r) = (bound_of(left, subst), bound_of(right, subst));
            if let (Some(Bound::Local(id)), Some(b)) = (&l, &r) {
                if !b.reads(*id) {
                    if let Some(kind) = k(op, positive) {
                        out.push(Implied::Clip(*id, kind, b.clone()));
                    }
                }
            }
            if let (Some(Bound::Local(id)), Some(b)) = (&r, &l) {
                if !b.reads(*id) {
                    if let Some(kind) = k(&mirror(op), positive) {
                        out.push(Implied::Clip(*id, kind, b.clone()));
                    }
                }
            }
        }
        _ => {}
    }
}

/// `e` as a bound: a literal, a local (through `subst` inside a
/// predicate's body), or a local's `length`.
/// The narrowing id of `this` (`Gen::narrow_enter_body` puts it in scope
/// for a body with its own `this` binding; no local has this id).
pub(crate) const THIS_LOCAL: u32 = u32::MAX - 1;

/// `this` is a narrowing host of rule G1 only (`this.f === b`), not of
/// rule G2 (`if (this.f)`); `PERRY_WPF_THIS_HOST=all` makes it a host of
/// both.
fn this_host_bool() -> bool {
    !std::env::var("PERRY_WPF_THIS_HOST").is_ok_and(|v| v == "all")
}

/// The host of a field test `host.key`: a local, or `this` of the body the
/// guard is in (`depth` 0: a predicate's `this` is its own call's). `this`
/// is one value in one invocation of a function with its own `this` binding
/// (ECMA-262 9.4.4 ResolveThisBinding; nothing assigns it).
/// Rule G3: the guard test `=== e` true implies of the other operand: the
/// type of the literal `e` (ECMA-262 7.2.16 step 1).
pub(crate) fn literal_type_test(e: &Expr) -> Option<u8> {
    use crate::model::guard;
    match e {
        Expr::String(_) => Some(guard::IS_STR),
        Expr::Number(_) | Expr::Integer(_) => Some(guard::IS_NUM),
        Expr::Null => Some(guard::IS_NULL),
        Expr::Undefined => Some(guard::UNDEF),
        _ => None,
    }
}

/// Rule G4: `typeof host.key` compared with the String "number", "string"
/// or "undefined" (either order): the host, the key and the G3 test of that
/// type (ECMA-262 13.5.3: "number" iff a Number, "string" iff a String,
/// "undefined" iff `undefined`). "object", "function", "boolean", "bigint"
/// and "symbol" have no test that admits exactly their values: `None`.
fn typeof_member_test<'e>(a: &'e Expr, b: &'e Expr) -> Option<(&'e Expr, &'e String, u8)> {
    use crate::model::guard;
    let ((Expr::TypeOf(o), Expr::String(s)) | (Expr::String(s), Expr::TypeOf(o))) = (a, b) else {
        return None;
    };
    let Expr::PropertyGet {
        object, property, ..
    } = o.as_ref()
    else {
        return None;
    };
    let test = match s.as_str() {
        "number" => guard::IS_NUM,
        "string" => guard::IS_STR,
        "undefined" => guard::UNDEF,
        _ => return None,
    };
    Some((object.as_ref(), property, test))
}

/// A clause body that never completes normally into the next clause: its
/// last statement leaves (the rule M1 test, `Stmt::Switch` in `walk`).
fn case_leaves(c: &perry_hir::SwitchCase) -> bool {
    matches!(
        c.body.last(),
        Some(
            Stmt::Break
                | Stmt::Continue
                | Stmt::LabeledBreak(_)
                | Stmt::LabeledContinue(_)
                | Stmt::Return(_)
                | Stmt::Throw(_)
        )
    )
}

fn host_of(e: &Expr, subst: &dyn Fn(u32) -> Option<Bound>, depth: u8) -> Option<u32> {
    match e {
        Expr::This if depth == 0 => Some(THIS_LOCAL),
        _ => match bound_of(e, subst)? {
            Bound::Local(id) => Some(id),
            _ => None,
        },
    }
}

fn bound_of(e: &Expr, subst: &dyn Fn(u32) -> Option<Bound>) -> Option<Bound> {
    use crate::model::RangeOp;
    use perry_hir::BinaryOp;
    let pair = |op: RangeOp, a: &Expr, b: &Expr| -> Option<Bound> {
        let x = bound_of(a, subst)?;
        let y = bound_of(b, subst)?;
        (!matches!(x, Bound::Char(_)) && !matches!(y, Bound::Char(_)))
            .then(|| Bound::Op(op, std::rc::Rc::new((x, Some(y)))))
    };
    match e {
        Expr::Integer(v) => Some(Bound::Lit(*v as f64)),
        Expr::Number(v) => Some(Bound::Lit(*v)),
        Expr::String(s) => {
            let c = crate::model::range::char_literal(s);
            (!c.is_top()).then_some(Bound::Char(c.lo))
        }
        Expr::LocalGet(l) => subst(*l),
        Expr::PropertyGet {
            object, property, ..
        } if property == "length" => match object.as_ref() {
            Expr::LocalGet(o) => match subst(*o)? {
                Bound::Local(x) => Some(Bound::Length(x)),
                _ => None,
            },
            _ => None,
        },
        Expr::Binary { op, left, right } => {
            let op = match op {
                BinaryOp::Add => RangeOp::Add,
                BinaryOp::Sub => RangeOp::Sub,
                BinaryOp::Mul => RangeOp::Mul,
                BinaryOp::BitAnd => RangeOp::BitAnd,
                BinaryOp::BitOr => RangeOp::BitOr,
                BinaryOp::Shr => RangeOp::Shr,
                _ => return None,
            };
            pair(op, left, right)
        }
        Expr::MathMin(xs) | Expr::MathMax(xs) if xs.len() == 2 => {
            let op = if matches!(e, Expr::MathMin(_)) {
                RangeOp::Min
            } else {
                RangeOp::Max
            };
            pair(op, &xs[0], &xs[1])
        }
        Expr::MathAbs(x) => {
            let b = bound_of(x, subst)?;
            (!matches!(b, Bound::Char(_)))
                .then(|| Bound::Op(RangeOp::Abs, std::rc::Rc::new((b, None))))
        }
        _ => None,
    }
}

/// A function whose body is one `return <expr>` and whose parameters are
/// plain (no default, no rest, no destructuring capture).
pub(crate) fn predicate_of(f: &perry_hir::Function) -> Option<Predicate<'_>> {
    if f.is_async || f.is_generator || f.params.iter().any(|p| p.is_rest || p.default.is_some()) {
        return None;
    }
    match f.body.as_slice() {
        // A write in the expression would make a later read of a parameter
        // no longer the argument.
        [Stmt::Return(Some(e))]
            if !expr_has(e, &mut |x| {
                matches!(
                    x,
                    Expr::LocalSet(..) | Expr::Update { .. } | Expr::Closure { .. }
                )
            }) =>
        {
            Some((&f.params, e))
        }
        _ => None,
    }
}

impl<'a> Gen<'a> {
    /// The node of bound `b` as read here.
    fn bound_node(&mut self, mi: u32, b: Bound) -> NodeId {
        match b {
            Bound::Op(op, ab) => {
                let (a, b2) = (*ab).clone();
                let an = self.bound_node(mi, a);
                let bn = b2.map(|b2| self.bound_node(mi, b2));
                let dst = self.tmp("narrow-bound");
                self.zs_carrier(dst);
                self.range_rule(op, an, bn, dst);
                self.m.add_prim(dst, prim::NUM);
                dst
            }
            Bound::Lit(v) => self.num_literal(v),
            Bound::Char(c) => {
                let s = if c < 0 {
                    String::new()
                } else {
                    String::from_utf16_lossy(&[c as u16])
                };
                self.str_literal(&s)
            }
            Bound::Local(l) => self
                .narrowed_read(mi, l)
                .unwrap_or_else(|| self.local(mi, l)),
            Bound::Length(o) => {
                let on = self
                    .narrowed_read(mi, o)
                    .unwrap_or_else(|| self.local(mi, o));
                let len = self.tmp("narrow-length");
                self.zs_carrier(len);
                let key = crate::model::Key::Named(self.m.atom("length"));
                self.m.cons(on, crate::model::Cons::Load { key, dst: len });
                len
            }
        }
    }

    /// Narrow the locals `cond` bounds while it is `positive`ly true
    /// (false) for the code walked next (module docs, "Guards"); how many
    /// narrowings to pop after it (`narrow_pop`).
    pub(crate) fn narrow_guard(&mut self, fx: &Fx, cond: &Expr, positive: bool) -> usize {
        if !self.guard_ranges && !self.guard_presence {
            return 0;
        }
        // The body being walked (`stmts` runs outside `stmt`'s `cur_fn`).
        let f = fx.f;
        if !self.narrow_scope.as_ref().is_some_and(|(sf, _)| *sf == f) {
            return 0;
        }
        let mut facts = Vec::new();
        let module: &'a perry_hir::Module = self.modules[fx.module as usize].1;
        let pred = |fid: u32| -> Option<Predicate<'a>> {
            module
                .functions
                .iter()
                .find(|f| f.id == fid)
                .and_then(predicate_of)
        };
        implied(
            cond,
            positive,
            &|l| Some(Bound::Local(l)),
            &pred,
            0,
            &mut facts,
        );
        // Without guard ranges (`FactsConfig::no_guard_ranges`) no clip;
        // without guard presence (`FactsConfig::no_guard_presence`) no
        // presence.
        facts.retain(|x| match x {
            Implied::Clip(..) => self.guard_ranges,
            Implied::Present(..) => self.guard_presence,
        });
        if facts.is_empty() {
            return 0;
        }
        // Its nodes belong to the body (`stmts` calls this outside `stmt`).
        let prev = std::mem::replace(&mut self.m.cur_fn, f);
        let pushed = self.narrow_guard_facts(fx, facts);
        self.m.cur_fn = prev;
        pushed
    }

    fn narrow_guard_facts(&mut self, fx: &Fx, facts: Vec<Implied>) -> usize {
        let f = fx.f;
        let mi = fx.module;
        let mut pushed = 0;
        for fact in facts {
            let id = match &fact {
                Implied::Clip(id, ..) | Implied::Present(id, _) => *id,
            };
            if !self
                .narrow_scope
                .as_ref()
                .is_some_and(|(_, ls)| ls.contains(&id))
            {
                continue;
            }
            // `this` is a host of rule G1 (`this.f === b`) only, not of
            // rule G2 (`if (this.f)`), unless `PERRY_WPF_THIS_HOST=all`.
            if id == THIS_LOCAL
                && matches!(&fact, Implied::Present(_, Presence::FieldTruthy(_, test))
                    if *test == crate::model::guard::TRUTHY)
                && this_host_bool()
            {
                continue;
            }
            let src = match self.narrowed_read(mi, id) {
                Some(n) => n,
                None if id == THIS_LOCAL => fx.this,
                None => self.local(mi, id),
            };
            let start = self.tmp("guard");
            self.zs_carrier(start);
            let mut below = None;
            match fact {
                Implied::Clip(_, kind, bound) => {
                    below = below_length(kind, &bound);
                    let is_char = matches!(bound, Bound::Char(_));
                    let b = self.bound_node(mi, bound);
                    let op = match is_char {
                        true => crate::model::RangeOp::SClip(kind),
                        false => crate::model::RangeOp::Clip(kind),
                    };
                    self.m.edge(src, start);
                    self.range_rule(op, src, Some(b), start);
                }
                // The local's values but the ones the guard excludes: a
                // filter of the program text, so a summary replays it.
                Implied::Present(_, Presence::NotNullish) => {
                    self.m.cons(src, Cons::NotNullish { dst: start })
                }
                Implied::Present(_, Presence::NotUndefined) => {
                    self.m.cons(src, Cons::NotUndefined { dst: start })
                }
                Implied::Present(_, Presence::NotBuffer(shared)) => {
                    self.m.cons(src, Cons::NotBuffer { shared, dst: start });
                    // The global's value (`globalThis.ArrayBuffer`, the
                    // read the right-hand side makes): any value but the
                    // intrinsic constructor lets every value through.
                    let name = match shared {
                        true => "SharedArrayBuffer",
                        false => "ArrayBuffer",
                    };
                    let global = self.expr(fx, &Expr::GlobalGet(0));
                    let ctor = self.tmp("instanceof-ctor");
                    let key = Key::Named(self.m.atom(name));
                    self.m.cons(global, Cons::Load { key, dst: ctor });
                    let test = match shared {
                        true => crate::model::guard::NOT_SHARED_BUFFER_CTOR,
                        false => crate::model::guard::NOT_BUFFER_CTOR,
                    };
                    self.m.cons(
                        ctor,
                        Cons::Guard {
                            test,
                            from: src,
                            dst: start,
                        },
                    );
                }
                Implied::Present(_, Presence::FieldTruthy(property, test)) => {
                    let key = Key::Named(self.m.atom(&property));
                    self.m.cons(src, Cons::FieldTruthy { key, test, dst: start })
                }
            }
            let end = self.tmp("guard-end");
            self.zs_carrier(end);
            self.m.edge(start, end);
            self.narrow.push(Narrow {
                mi,
                id,
                f,
                start,
                end,
                written: false,
                in_update: false,
                jumps: self.jumps,
                tries: self.tries,
                below_length: below,
            });
            pushed += 1;
        }
        pushed
    }

    /// Rule G3 for clause `i` of `switch (disc)`, `disc` a property read:
    /// the clauses `j..=i` whose bodies before `i` are empty, entered only
    /// by their tests (`j` first, or clause `j - 1` leaves), all `case`
    /// literals of one type (no `default`: 14.12.4 enters it when no test
    /// is equal), narrow as `disc === test_i` true; any other clause none.
    pub(crate) fn case_narrow(
        &mut self,
        fx: &Fx,
        disc: &Expr,
        cases: &[perry_hir::SwitchCase],
        i: usize,
    ) -> usize {
        if !matches!(disc, Expr::PropertyGet { .. }) {
            return 0;
        }
        let mut j = i;
        while j > 0 && cases[j - 1].body.is_empty() {
            j -= 1;
        }
        if j > 0 && !case_leaves(&cases[j - 1]) {
            return 0;
        }
        let mut test = None;
        for c in &cases[j..=i] {
            let Some(k) = c.test.as_ref().and_then(literal_type_test) else {
                return 0;
            };
            if test.is_some_and(|t| t != k) {
                return 0;
            }
            test = Some(k);
        }
        let Some(lit) = cases[i].test.as_ref() else {
            return 0;
        };
        let cond = Expr::Compare {
            op: CompareOp::Eq,
            left: Box::new(disc.clone()),
            right: Box::new(lit.clone()),
        };
        self.narrow_guard(fx, &cond, true)
    }

    pub(crate) fn narrow_pop(&mut self, k: usize) {
        for _ in 0..k {
            self.narrow.pop();
        }
    }
}
