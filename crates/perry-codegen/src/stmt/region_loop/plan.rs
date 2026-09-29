//! Step 4b regions: the planner (which accesses are bare, the re-check a loop needs).

use super::*;

// ---------------------------------------------------------------- planning

/// Facts hold on every path.
pub(super) const FRESH: u8 = 2;
/// Facts hold on every path except the generic arm of a fact tree, which
/// sets the region's dirty flag: the next iteration re-checks only if it was
/// taken. Not enough for a bare access.
pub(super) const DIRTY: u8 = 1;

/// `None` = unreachable. Absent receiver = stale.
pub(super) type St = Option<BTreeMap<Recv, u8>>;

pub(super) fn meet(a: St, b: St) -> St {
    match (a, b) {
        (None, x) | (x, None) => x,
        (Some(a), Some(b)) => Some(
            a.into_iter()
                .filter_map(|(k, v)| b.get(&k).map(|w| (k, v.min(*w))))
                .collect(),
        ),
    }
}

pub(super) fn dirty(st: &mut St) {
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
pub(super) fn fact_tree_leaves<'e>(
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

pub(super) fn kill(st: &mut St) {
    if let Some(m) = st {
        m.clear();
    }
}

/// A leaf that is a primitive by construction: an operator over it cannot
/// reach ToPrimitive.
pub(super) fn prim(e: &Expr) -> bool {
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

pub(super) struct Planner<'p, 'a> {
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
pub(super) fn body_refused(ss: &[Stmt]) -> bool {
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

/// The HIR size of `ss` (statements plus expressions): what a region copies
/// when it versions a loop or splits a body.
pub(super) fn body_nodes(ss: &[Stmt]) -> usize {
    fn e_n(e: &Expr) -> usize {
        let mut n = 1;
        perry_hir::walker::walk_expr_children(e, &mut |c| n += e_n(c));
        n
    }
    fn s_n(s: &Stmt) -> usize {
        1 + match s {
            Stmt::Let { init: Some(e), .. } | Stmt::Expr(e) | Stmt::Throw(e) => e_n(e),
            Stmt::Return(Some(e)) => e_n(e),
            Stmt::If {
                condition,
                then_branch,
                else_branch,
            } => {
                e_n(condition)
                    + then_branch.iter().map(s_n).sum::<usize>()
                    + else_branch
                        .as_ref()
                        .map_or(0, |b| b.iter().map(s_n).sum::<usize>())
            }
            Stmt::While { condition, body } | Stmt::DoWhile { body, condition } => {
                e_n(condition) + body.iter().map(s_n).sum::<usize>()
            }
            Stmt::For {
                init,
                condition,
                update,
                body,
            } => {
                init.as_ref().map_or(0, |i| s_n(i))
                    + condition.as_ref().map_or(0, e_n)
                    + update.as_ref().map_or(0, e_n)
                    + body.iter().map(s_n).sum::<usize>()
            }
            Stmt::Switch {
                discriminant,
                cases,
            } => {
                e_n(discriminant)
                    + cases
                        .iter()
                        .map(|c| {
                            c.test.as_ref().map_or(0, e_n) + c.body.iter().map(s_n).sum::<usize>()
                        })
                        .sum::<usize>()
            }
            _ => 0,
        }
    }
    ss.iter().map(s_n).sum()
}

/// Locals assigned (or declared) anywhere in `ss`, and in `extra`.
pub(super) fn assigned(ss: &[Stmt], extra: &[&Expr]) -> HashSet<u32> {
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
pub(super) fn accesses(ss: &[Stmt]) -> Vec<(Recv, String, bool)> {
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
pub(super) fn receiver_eligible(ctx: &FnCtx<'_>, r: Recv) -> bool {
    match r {
        // A derived constructor's `this` may still be in its TDZ (before
        // `super()`), and reading it throws: the preheader must not hoist
        // that read, so such a body's `this` is not a region receiver.
        // A `this` whose shape is already PROVEN (a proven-shape method clone)
        // reads its slots with no guard at all; a region would only add one.
        Recv::This => {
            !ctx.this_stack.is_empty()
                && !ctx.in_static_member
                && ctx.super_called_stack.is_empty()
                && ctx.ptr_shape_receiver_fact(&Expr::This).is_none()
        }
        Recv::Local(id) => {
            !ctx.boxed_vars.contains(&id)
                && !ctx.prealloc_boxes.contains(&id)
                && !ctx.tdz_boxes.contains(&id)
                // A module-level binding can be assigned by any call, unless
                // it is `const`.
                && (!ctx.module_globals.contains_key(&id) || const_module_global(id))
                && !ctx.pod_records.contains_key(&id)
                && !ctx.scalar_replaced.contains_key(&id)
                && !ctx.spec_ta_bindings.contains_key(&id)
                && !ctx.local_slot_reps.contains_key(&id)
                && !ctx.integer_locals.contains(&id)
                && !ctx.receiver_descriptors.contains_buffer_view(id)
                && ctx.ptr_shape_receiver_fact(&Expr::LocalGet(id)).is_none()
                && (ctx.locals.contains_key(&id)
                    || ctx.closure_captures.contains_key(&id)
                    || ctx.module_globals.contains_key(&id))
        }
    }
}

pub(super) struct Plan {
    pub(super) receivers: Vec<(Recv, Vec<String>, bool, u32)>,
    pub(super) bare: HashSet<usize>,
    pub(super) trees: HashSet<usize>,
    pub(super) recheck: Recheck,
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
pub(super) fn plan(
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
    // Which of each receiver's keys the body ever STORES (bare or not): the
    // runtime serves a spill-located key to reads only, so a stored key must
    // be inline for the word to be published.
    let mut stored: HashMap<Recv, u32> = HashMap::new();
    for (r, k, is_store) in accesses(tail) {
        if is_store && cands.contains(&r) {
            if let Some(i) = keys[&r].iter().position(|x| *x == k) {
                *stored.entry(r).or_default() |= 1 << i;
            }
        }
    }
    let mut receivers: Vec<(Recv, Vec<String>, bool, u32)> = used
        .into_iter()
        .map(|r| {
            (
                r,
                keys[&r].clone(),
                bare_stores.contains(&r),
                stored.get(&r).copied().unwrap_or(0),
            )
        })
        .collect();
    receivers.sort_by_key(|(r, _, _, _)| *r);
    Some(Plan {
        receivers,
        bare,
        trees,
        recheck,
    })
}
