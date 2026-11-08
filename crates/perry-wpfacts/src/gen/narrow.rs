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
//! The bound is a literal, a local, or a local's `length` (an Array's
//! carries its length interval, `solve::arr_len`).
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
        for (kind, bound) in clips {
            let b = self.bound_node(mi, bound);
            let dst = self.tmp("narrow");
            self.m.edge(start, dst);
            self.range_rule(crate::model::RangeOp::Clip(kind), start, Some(b), dst);
            start = dst;
        }
        let end = self.tmp("narrow-end");
        self.m.edge(start, end);
        self.narrow.push(Narrow {
            mi,
            id,
            f: self.m.cur_fn,
            start,
            end,
            written: false,
            in_update: false,
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

/// The clips condition `c` implies when it is `positive`ly true (false):
/// `(local, clip kind, bound)` for each ordered comparison of a local.
/// `pred` resolves a called function to its predicate body (`subst` maps
/// its parameters to the caller's locals and literals).
fn implied<'p>(
    c: &Expr,
    positive: bool,
    subst: &dyn Fn(u32) -> Option<Bound>,
    pred: &dyn Fn(u32) -> Option<Predicate<'p>>,
    depth: u8,
    out: &mut Vec<(u32, u8, Bound)>,
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
            // Only a clip of a parameter that the call binds to a local.
            out.extend(found);
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
                        out.push((*id, kind, b.clone()));
                    }
                }
            }
            if let (Some(Bound::Local(id)), Some(b)) = (&r, &l) {
                if !b.reads(*id) {
                    if let Some(kind) = k(&mirror(op), positive) {
                        out.push((*id, kind, b.clone()));
                    }
                }
            }
        }
        _ => {}
    }
}

/// `e` as a bound: a literal, a local (through `subst` inside a
/// predicate's body), or a local's `length`.
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
        if !self.guard_ranges {
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
        if facts.is_empty() {
            return 0;
        }
        // Its nodes belong to the body (`stmts` calls this outside `stmt`).
        let prev = std::mem::replace(&mut self.m.cur_fn, f);
        let pushed = self.narrow_guard_facts(fx, facts);
        self.m.cur_fn = prev;
        pushed
    }

    fn narrow_guard_facts(&mut self, fx: &Fx, facts: Vec<(u32, u8, Bound)>) -> usize {
        let f = fx.f;
        let mi = fx.module;
        let mut pushed = 0;
        for (id, kind, bound) in facts {
            if !self
                .narrow_scope
                .as_ref()
                .is_some_and(|(_, ls)| ls.contains(&id))
            {
                continue;
            }
            let is_char = matches!(bound, Bound::Char(_));
            let b = self.bound_node(mi, bound);
            let op = match is_char {
                true => crate::model::RangeOp::SClip(kind),
                false => crate::model::RangeOp::Clip(kind),
            };
            let src = self
                .narrowed_read(mi, id)
                .unwrap_or_else(|| self.local(mi, id));
            let start = self.tmp("guard");
            self.m.edge(src, start);
            self.range_rule(op, src, Some(b), start);
            let end = self.tmp("guard-end");
            self.m.edge(start, end);
            self.narrow.push(Narrow {
                mi,
                id,
                f,
                start,
                end,
                written: false,
                in_update: false,
            });
            pushed += 1;
        }
        pushed
    }

    pub(crate) fn narrow_pop(&mut self, k: usize) {
        for _ in 0..k {
            self.narrow.pop();
        }
    }
}
