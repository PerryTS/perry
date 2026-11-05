//! The one read-before-write predicate of the per-body "typed by
//! construction" facts (#12658).
//!
//! A hoisted `var` (and a `let x;`) lowers to a `Let` with an `undefined`
//! seed ahead of its real writes. A fact that proves a local's kind from its
//! real writes alone and skips that seed (`typed_locals`, and the optimistic
//! `undefined` seed `integer_locals` exempts) holds only when no read of the
//! local can run before a real write: such a read sees `undefined`, which is
//! neither a String, a Number nor an int32. Every such fact asks this walk;
//! none keeps its own copy.
//!
//! The walk is a definite-assignment pass over the body. A read is safe only
//! when every path reaching it writes the local first. A write inside a
//! conditional part (`&&`, `||`, `??`, `?:`, one `if` arm, a loop body, a
//! `try` block, a `switch` case) does not count after it unless every path
//! that continues past the construct writes it.
//!
//! Paths that cannot complete normally do not reach the merge point after a
//! construct: `return` and `throw` leave the body; `break` and `continue`
//! leave for their target. A `break` carries its state to the end of the
//! loop, `switch` or labeled statement it leaves, where it joins the paths
//! that complete normally; a `continue` goes back to its loop's head and
//! reaches no merge point the walk uses (the code after a loop assumes only
//! the state before it, since the body may never run; a labeled block, which
//! lowers to a run-once `do .. while (false)`, runs its body once). A
//! `switch` without a
//! `default` also completes when no case is taken. A `try` completes
//! normally only when its block or a catch does and then its `finally` does;
//! a `finally` that cannot complete makes the whole statement terminate.
//!
//! A closure reads what its body reads when it runs, which for a closure
//! expression may be any time after its creation, so creating one counts as
//! a read of every local it captures or reads in its body. A function
//! binding (`function f() {}`, hoisted to the top of the body, or any `Let`
//! bound once to a closure and never written again) runs only where its
//! value is used: a direct call `f(..)` runs its body after the arguments,
//! and every other use of `f` (an argument, a property or array store, a
//! return, an alias, `new`, a class capture) hands the value on, so the body
//! may run from that point on. The walk judges the body's reads at each of
//! those points instead of at the creation. A use inside another closure is
//! a use where that closure runs (transitively through other function
//! bindings). Creating the binding still copies every capture that does not
//! live in a cell the body preallocates (`PreallocateBoxes`), so those are
//! read at the creation. A body with a direct `eval` or a `with` (which can
//! reach any binding by name) keeps every closure read at its creation.
//!
//! Whether a local is flagged depends only on the body and that local, never
//! on which other locals are candidates, so a caller may exclude the result
//! from its candidates up front and run its fixed point once.

use std::collections::{HashMap, HashSet};

use perry_hir::types::FuncId;
use perry_hir::{Expr, Stmt};

/// Locals some read of which may run before any write (so it may see the
/// `undefined` a `var` starts as). A read is safe only when every path that
/// reaches it writes the local first.
pub(crate) fn reads_before_write(stmts: &[Stmt], candidates: &HashSet<u32>) -> HashSet<u32> {
    let fns = Fns::collect(stmts, candidates);
    let mut w = Walk {
        c: candidates,
        fns: &fns,
        cells: HashSet::new(),
        bad: HashSet::new(),
        targets: Vec::new(),
    };
    let mut st = State {
        da: HashSet::new(),
        live: true,
    };
    for s in stmts {
        // A cell the body preallocates exists before anything after it runs.
        if let Stmt::PreallocateBoxes(ids) | Stmt::PreallocateTdzBoxes(ids) = s {
            w.cells.extend(ids.iter().copied());
        }
        w.stmt(s, &mut st);
    }
    w.bad
}

/// The function bindings of a body: locals bound by exactly one `Let` to a
/// closure and never written otherwise, so every use of the function is a
/// use of the local (or of the closure's `FuncRef`).
#[derive(Default)]
struct Fns {
    local: HashMap<u32, usize>,
    func: HashMap<FuncId, usize>,
    /// Per binding: the candidates its body reads when it runs, including
    /// through the bindings its body uses.
    reads: Vec<HashSet<u32>>,
}

impl Fns {
    fn collect(stmts: &[Stmt], c: &HashSet<u32>) -> Fns {
        let mut lets: HashMap<u32, Vec<Option<&Expr>>> = HashMap::new();
        each_stmt(stmts, &mut |s| {
            if let Stmt::Let { id, init, .. } = s {
                lets.entry(*id).or_default().push(init.as_ref());
            }
        });
        let mut written: HashSet<u32> = HashSet::new();
        let mut dynamic = false;
        each_expr_deep(stmts, &mut |e| match e {
            Expr::LocalSet(id, _)
            | Expr::Update { id, .. }
            | Expr::ScopedTemp { id, .. }
            | Expr::ArrayPop(id)
            | Expr::ArrayShift(id)
            | Expr::ArrayPush { array_id: id, .. }
            | Expr::ArrayPushSpread { array_id: id, .. }
            | Expr::ArrayUnshift { array_id: id, .. }
            | Expr::ArraySplice { array_id: id, .. }
            | Expr::ArrayCopyWithin { array_id: id, .. }
            | Expr::SetAdd { set_id: id, .. } => {
                written.insert(*id);
            }
            Expr::WithGet { .. } | Expr::WithSet { .. } => dynamic = true,
            Expr::ExternFuncRef { name, .. } if name == "js_eval_in_scope" => dynamic = true,
            _ => {}
        });
        let mut fns = Fns::default();
        if dynamic {
            return fns;
        }
        let mut closures: Vec<&Expr> = Vec::new();
        let mut ids: Vec<u32> = lets.keys().copied().collect();
        ids.sort_unstable();
        for id in ids {
            if let [Some(e @ Expr::Closure { func_id, .. })] = lets[&id][..] {
                if !written.contains(&id) {
                    fns.local.insert(id, closures.len());
                    fns.func.insert(*func_id, closures.len());
                    closures.push(e);
                }
            }
        }
        let mut uses: Vec<Vec<usize>> = Vec::with_capacity(closures.len());
        for e in &closures {
            let mut reads = HashSet::new();
            let mut used = Vec::new();
            closure_reads(e, &mut |x| match x {
                Read::Local(id) => {
                    if c.contains(&id) {
                        reads.insert(id);
                    }
                }
                Read::Expr(x) => {
                    if let Some(j) = fns.reached(x) {
                        used.push(j);
                    }
                }
            });
            fns.reads.push(reads);
            uses.push(used);
        }
        loop {
            let mut changed = false;
            for (i, used) in uses.iter().enumerate() {
                for &j in used {
                    if i == j {
                        continue;
                    }
                    let add: Vec<u32> = fns.reads[j].difference(&fns.reads[i]).copied().collect();
                    if !add.is_empty() {
                        fns.reads[i].extend(add);
                        changed = true;
                    }
                }
            }
            if !changed {
                break;
            }
        }
        fns
    }

    /// The binding a use inside another closure's body reaches.
    fn reached(&self, e: &Expr) -> Option<usize> {
        match e {
            Expr::LocalGet(id) => self.local.get(id).copied(),
            Expr::FuncRef(f) => self.func.get(f).copied(),
            _ => None,
        }
    }

    /// Binding `i` may run here: its body's reads happen now.
    fn run(&self, i: usize, c: &HashSet<u32>, da: &HashSet<u32>, bad: &mut HashSet<u32>) {
        for id in &self.reads[i] {
            if c.contains(id) && !da.contains(id) {
                bad.insert(*id);
            }
        }
    }
}

enum Read<'a> {
    Local(u32),
    Expr(&'a Expr),
}

/// Everything a closure reads when it runs: its captures, its parameter
/// defaults and body, and the closures nested in them; `Read::Expr` hands
/// over every expression so a caller can see the function bindings used.
fn closure_reads(e: &Expr, f: &mut dyn FnMut(Read<'_>)) {
    let Expr::Closure {
        params,
        body,
        captures,
        mutable_captures,
        ..
    } = e
    else {
        return;
    };
    for id in captures.iter().chain(mutable_captures.iter()) {
        f(Read::Local(*id));
    }
    let mut visit = |x: &Expr| {
        if let Expr::LocalGet(id) | Expr::Update { id, .. } = x {
            f(Read::Local(*id));
        }
        f(Read::Expr(x));
    };
    for p in params {
        if let Some(d) = &p.default {
            each_expr_deep_in(d, &mut visit);
        }
    }
    each_expr_deep(body, &mut visit);
}

/// Every statement of `stmts`, nested statements included, closures not.
fn each_stmt<'a>(stmts: &'a [Stmt], f: &mut dyn FnMut(&'a Stmt)) {
    for s in stmts {
        f(s);
        match s {
            Stmt::If {
                then_branch,
                else_branch,
                ..
            } => {
                each_stmt(then_branch, f);
                if let Some(b) = else_branch {
                    each_stmt(b, f);
                }
            }
            Stmt::While { body, .. } | Stmt::DoWhile { body, .. } => each_stmt(body, f),
            Stmt::For { init, body, .. } => {
                if let Some(i) = init {
                    each_stmt(std::slice::from_ref(i.as_ref()), f);
                }
                each_stmt(body, f);
            }
            Stmt::Labeled { body, .. } => each_stmt(std::slice::from_ref(body.as_ref()), f),
            Stmt::Try {
                body,
                catch,
                finally,
            } => {
                each_stmt(body, f);
                if let Some(cc) = catch {
                    each_stmt(&cc.body, f);
                }
                if let Some(b) = finally {
                    each_stmt(b, f);
                }
            }
            Stmt::Switch { cases, .. } => {
                for case in cases {
                    each_stmt(&case.body, f);
                }
            }
            _ => {}
        }
    }
}

/// Every expression of `stmts`, inside nested statements and closures too.
fn each_expr_deep(stmts: &[Stmt], f: &mut dyn FnMut(&Expr)) {
    for s in stmts {
        perry_hir::walker::stmt_any_expr(s, &mut |x| {
            each_expr_deep_in(x, f);
            false
        });
    }
}

fn each_expr_deep_in(e: &Expr, f: &mut dyn FnMut(&Expr)) {
    f(e);
    if let Expr::Closure { body, .. } = e {
        each_expr_deep(body, f);
    }
    perry_hir::walker::walk_expr_children(e, &mut |x| each_expr_deep_in(x, f));
}

/// The definite-assignment state at one program point. `live == false`
/// means no path reaches the point normally (it follows a `return`,
/// `throw`, `break` or `continue` on every path); `da` then still holds the
/// state at the jump, so reads in such dead code are judged as if they ran
/// there.
#[derive(Clone)]
struct State {
    da: HashSet<u32>,
    live: bool,
}

impl State {
    /// The join of two paths: a path that cannot reach the point adds
    /// nothing.
    fn join(self, o: State) -> State {
        match (self.live, o.live) {
            (true, false) => self,
            (false, true) => o,
            _ => State {
                da: self.da.intersection(&o.da).copied().collect(),
                live: self.live,
            },
        }
    }
}

/// What a `break` / `continue` may leave.
enum Kind {
    Loop,
    Switch,
    Label(String),
    /// The run-once `do { .. } while (false)` a labeled non-loop statement
    /// (`a: { .. }`) lowers to. JS sends an unlabeled `break` / `continue`
    /// inside it to the enclosing loop or `switch`; the lowered loop catches
    /// them itself. The walk holds for both: such a jump reaches the block's
    /// end and the enclosing target's.
    Block,
}

/// An enclosing construct a jump may target, with the join of the states of
/// the jumps that leave it for its end.
struct Target {
    kind: Kind,
    exits: Option<State>,
}

impl Target {
    fn exit(&mut self, st: &State) {
        if st.live {
            self.exits = Some(match self.exits.take() {
                Some(b) => b.join(st.clone()),
                None => st.clone(),
            });
        }
    }
}

struct Walk<'a> {
    c: &'a HashSet<u32>,
    fns: &'a Fns,
    /// Locals whose cell the body preallocated before the current point.
    cells: HashSet<u32>,
    bad: HashSet<u32>,
    targets: Vec<Target>,
}

impl Walk<'_> {
    fn stmts(&mut self, stmts: &[Stmt], st: &mut State) {
        for s in stmts {
            self.stmt(s, st);
        }
    }

    fn expr(&mut self, e: &Expr, st: &mut State) {
        da_expr(e, self.c, self.fns, &mut st.da, &mut self.bad);
    }

    /// Walks `body` with `kind` as the innermost jump target; returns the
    /// join of the jumps that left it for its end.
    fn within(&mut self, kind: Kind, body: impl FnOnce(&mut Self)) -> Option<State> {
        self.targets.push(Target { kind, exits: None });
        body(self);
        self.targets.pop().and_then(|t| t.exits)
    }

    /// A `break` (unlabeled: the innermost loop or `switch`; labeled: the
    /// statement with that label). Its state joins the target's end. A
    /// target the walk does not see (none: labels never cross a function)
    /// leaves the path as it is.
    fn jump_break(&mut self, label: Option<&str>, st: &mut State) {
        let mut found = false;
        for t in self.targets.iter_mut().rev() {
            match (&t.kind, label) {
                (Kind::Block, None) => {
                    t.exit(st);
                    found = true;
                }
                (Kind::Loop | Kind::Switch, None) => {
                    t.exit(st);
                    found = true;
                    break;
                }
                (Kind::Label(l), Some(want)) if l == want => {
                    t.exit(st);
                    found = true;
                    break;
                }
                _ => {}
            }
        }
        if found {
            st.live = false;
        }
    }

    /// A `continue` goes back to its loop's head, which the walk joins with
    /// nothing (the code after a loop assumes only the state before it). In
    /// a labeled block it may also leave the block for its end.
    fn jump_continue(&mut self, label: Option<&str>, st: &mut State) {
        let mut found = false;
        for t in self.targets.iter_mut().rev() {
            match (&t.kind, label) {
                (Kind::Block, None) => {
                    t.exit(st);
                    found = true;
                }
                (Kind::Loop, None) => {
                    found = true;
                    break;
                }
                (Kind::Label(l), Some(want)) if l == want => {
                    found = true;
                    break;
                }
                _ => {}
            }
        }
        if found {
            st.live = false;
        }
    }

    /// The end of a construct: its normal completion joined with its
    /// `break`s.
    fn end(st: State, breaks: Option<State>) -> State {
        match breaks {
            Some(b) => st.join(b),
            None => st,
        }
    }

    fn stmt(&mut self, s: &Stmt, st: &mut State) {
        match s {
            Stmt::Let { id, init, .. } => match init {
                // The `undefined` seed of a hoisted `var` is not an assignment.
                None | Some(Expr::Undefined) => {
                    st.da.remove(id);
                }
                // Creating a function binding runs nothing; it copies the
                // captures that do not live in a preallocated cell.
                Some(Expr::Closure {
                    captures,
                    mutable_captures,
                    ..
                }) if self.fns.local.contains_key(id) => {
                    for cap in captures.iter().chain(mutable_captures.iter()) {
                        if self.c.contains(cap) && !self.cells.contains(cap) && !st.da.contains(cap)
                        {
                            self.bad.insert(*cap);
                        }
                    }
                    st.da.insert(*id);
                }
                Some(e) => {
                    self.expr(e, st);
                    st.da.insert(*id);
                }
            },
            Stmt::Expr(e) => self.expr(e, st),
            Stmt::Throw(e) => {
                self.expr(e, st);
                st.live = false;
            }
            Stmt::Return(e) => {
                if let Some(e) = e {
                    self.expr(e, st);
                }
                st.live = false;
            }
            Stmt::Break => self.jump_break(None, st),
            Stmt::LabeledBreak(l) => self.jump_break(Some(l), st),
            Stmt::Continue => self.jump_continue(None, st),
            Stmt::LabeledContinue(l) => self.jump_continue(Some(l), st),
            Stmt::If {
                condition,
                then_branch,
                else_branch,
            } => {
                self.expr(condition, st);
                let mut t = st.clone();
                self.stmts(then_branch, &mut t);
                let mut f = st.clone();
                if let Some(b) = else_branch {
                    self.stmts(b, &mut f);
                }
                *st = t.join(f);
            }
            // A loop body may never run: the code after a loop assumes the
            // state before the body, joined with the `break`s that leave it.
            Stmt::While { condition, body } => {
                self.expr(condition, st);
                let mut b = st.clone();
                let breaks = self.within(Kind::Loop, |w| w.stmts(body, &mut b));
                *st = Self::end(st.clone(), breaks);
            }
            Stmt::DoWhile { body, condition } => {
                // The body runs once before the condition, but a `continue`
                // can reach the condition from anywhere in it.
                let mut b = st.clone();
                let breaks = self.within(Kind::Loop, |w| w.stmts(body, &mut b));
                let mut cnd = st.clone();
                self.expr(condition, &mut cnd);
                *st = Self::end(st.clone(), breaks);
            }
            Stmt::For {
                init,
                condition,
                update,
                body,
            } => {
                if let Some(i) = init {
                    self.stmt(i, st);
                }
                if let Some(cnd) = condition {
                    self.expr(cnd, st);
                }
                let mut b = st.clone();
                let breaks = self.within(Kind::Loop, |w| w.stmts(body, &mut b));
                // `continue` reaches the update from anywhere in the body.
                if let Some(u) = update {
                    let mut u_st = st.clone();
                    self.expr(u, &mut u_st);
                }
                *st = Self::end(st.clone(), breaks);
            }
            Stmt::Labeled { label, body } => {
                let mut b = st.clone();
                let breaks = self.within(Kind::Label(label.clone()), |w| match body.as_ref() {
                    // A labeled block runs its body once.
                    Stmt::DoWhile {
                        body: inner,
                        condition: Expr::Bool(false),
                    } => {
                        let exits = w.within(Kind::Block, |w| w.stmts(inner, &mut b));
                        b = Self::end(b.clone(), exits);
                    }
                    other => w.stmt(other, &mut b),
                });
                *st = Self::end(b, breaks);
            }
            Stmt::Try {
                body,
                catch,
                finally,
            } => {
                // A throw can leave the block before any of its writes, so
                // the catch and the finally start from the state before it.
                let entry = st.clone();
                let mut done = st.clone();
                self.stmts(body, &mut done);
                if let Some(cc) = catch {
                    let mut k = entry.clone();
                    self.stmts(&cc.body, &mut k);
                    done = done.join(k);
                }
                if let Some(f) = finally {
                    // Reads in the finally are judged from the weakest entry
                    // (an exception thrown at the start of the block).
                    let mut fe = entry.clone();
                    self.stmts(f, &mut fe);
                    // The statement completes normally only through a block
                    // or catch that completed, then a finally that completes.
                    if done.live && fe.live {
                        self.stmts(f, &mut done);
                    } else {
                        done.live = false;
                    }
                }
                *st = done;
            }
            Stmt::Switch {
                discriminant,
                cases,
            } => {
                self.expr(discriminant, st);
                // Tests run in order until one matches: a case body is
                // entered with the state after its own test (the `default`
                // with the state after every test), or by falling through
                // from the case above.
                let mut tested = st.clone();
                let mut entered: Vec<State> = Vec::with_capacity(cases.len());
                for case in cases {
                    if let Some(t) = &case.test {
                        self.expr(t, &mut tested);
                    }
                    entered.push(tested.clone());
                }
                let has_default = cases.iter().any(|case| case.test.is_none());
                for (case, e) in cases.iter().zip(entered.iter_mut()) {
                    if case.test.is_none() {
                        *e = tested.clone();
                    }
                }
                let mut fall = State {
                    da: st.da.clone(),
                    live: false,
                };
                let breaks = self.within(Kind::Switch, |w| {
                    for (case, e) in cases.iter().zip(entered) {
                        let mut k = fall.clone().join(e);
                        w.stmts(&case.body, &mut k);
                        fall = k;
                    }
                });
                let mut out = Self::end(fall, breaks);
                // No case taken: the switch completes after the tests.
                if !has_default {
                    out = out.join(tested);
                }
                *st = out;
            }
            _ => {}
        }
    }
}

/// Reads in evaluation order; a write inside a conditional part
/// (`&&`, `||`, `??`, `?:`) does not count after it.
fn da_expr(e: &Expr, c: &HashSet<u32>, fns: &Fns, da: &mut HashSet<u32>, bad: &mut HashSet<u32>) {
    match e {
        Expr::LocalGet(id) => {
            if c.contains(id) && !da.contains(id) {
                bad.insert(*id);
            }
            // Any use of a function binding but a direct call hands its
            // value on: the body may run from here on.
            if let Some(&i) = fns.local.get(id) {
                fns.run(i, c, da, bad);
            }
        }
        Expr::FuncRef(f) => {
            if let Some(&i) = fns.func.get(f) {
                fns.run(i, c, da, bad);
            }
        }
        // A direct call runs the body after the arguments.
        Expr::Call { callee, args, .. } if matches!(callee.as_ref(), Expr::LocalGet(id) if fns.local.contains_key(id)) =>
        {
            for a in args {
                da_expr(a, c, fns, da, bad);
            }
            if let Expr::LocalGet(id) = callee.as_ref() {
                fns.run(fns.local[id], c, da, bad);
            }
        }
        Expr::Update { id, .. } => {
            if c.contains(id) && !da.contains(id) {
                bad.insert(*id);
            }
        }
        Expr::LocalSet(id, v) => {
            da_expr(v, c, fns, da, bad);
            da.insert(*id);
        }
        Expr::Logical { left, right, .. } => {
            da_expr(left, c, fns, da, bad);
            let mut r = da.clone();
            da_expr(right, c, fns, &mut r, bad);
        }
        Expr::Conditional {
            condition,
            then_expr,
            else_expr,
        } => {
            da_expr(condition, c, fns, da, bad);
            let mut t = da.clone();
            da_expr(then_expr, c, fns, &mut t, bad);
            let mut f = da.clone();
            da_expr(else_expr, c, fns, &mut f, bad);
            *da = t.intersection(&f).copied().collect();
        }
        Expr::Closure { .. } => {
            // Creating the closure reads what it captures (by value unless
            // boxed) and, since it may run any time after, what its body
            // reads, including through the function bindings it uses.
            let mut read = |id: &u32| {
                if c.contains(id) && !da.contains(id) {
                    bad.insert(*id);
                }
            };
            closure_reads(e, &mut |x| match x {
                Read::Local(id) => read(&id),
                Read::Expr(x) => {
                    if let Some(j) = fns.reached(x) {
                        fns.reads[j].iter().for_each(&mut read);
                    }
                }
            });
        }
        _ => perry_hir::walker::walk_expr_children(e, &mut |x| da_expr(x, c, fns, da, bad)),
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use perry_hir::types::Type as HirType;
    use perry_hir::{BinaryOp, CatchClause, Expr, Stmt, SwitchCase};

    use super::reads_before_write;

    const K: u32 = 1;
    const R: u32 = 2;
    const N: u32 = 3;
    const X: u32 = 4;

    fn let_(id: u32, init: Option<Expr>) -> Stmt {
        Stmt::Let {
            id,
            name: format!("v{id}"),
            ty: HirType::Any,
            mutable: true,
            init,
        }
    }

    fn set(id: u32, v: Expr) -> Stmt {
        Stmt::Expr(Expr::LocalSet(id, Box::new(v)))
    }

    fn get(id: u32) -> Expr {
        Expr::LocalGet(id)
    }

    fn bin(op: BinaryOp, l: Expr, r: Expr) -> Expr {
        Expr::Binary {
            op,
            left: Box::new(l),
            right: Box::new(r),
        }
    }

    /// #12658: `var r = 0; if (k) { r = n + 1; } var n = 5; n = n * 2;
    /// return r + n;` with `var n` hoisted as its `undefined` seed.
    fn issue_body() -> Vec<Stmt> {
        vec![
            let_(R, Some(Expr::Integer(0))),
            let_(N, Some(Expr::Undefined)),
            Stmt::If {
                condition: get(K),
                then_branch: vec![set(R, bin(BinaryOp::Add, get(N), Expr::Integer(1)))],
                else_branch: None,
            },
            let_(N, Some(Expr::Integer(5))),
            set(N, bin(BinaryOp::Mul, get(N), Expr::Integer(2))),
            Stmt::Return(Some(bin(BinaryOp::Add, get(R), get(N)))),
        ]
    }

    /// The control: the same seed, every read after a real write.
    fn control_body() -> Vec<Stmt> {
        vec![
            let_(X, Some(Expr::Undefined)),
            set(X, Expr::Integer(4)),
            set(X, Expr::Integer(6)),
            Stmt::Return(Some(get(X))),
        ]
    }

    fn integer_locals(body: &[Stmt]) -> HashSet<u32> {
        crate::collectors::integer_locals::collect_integer_locals(
            body,
            &HashSet::new(),
            &HashSet::new(),
            &HashSet::new(),
            &HashSet::new(),
        )
    }

    #[test]
    fn a_read_in_a_branch_above_the_declaration_sees_the_seed() {
        let all = HashSet::from([R, N]);
        assert_eq!(reads_before_write(&issue_body(), &all), HashSet::from([N]));
        assert!(reads_before_write(&control_body(), &HashSet::from([X])).is_empty());
    }

    #[test]
    fn an_int32_local_read_before_its_first_write_is_not_an_int32_local() {
        let ints = integer_locals(&issue_body());
        assert!(!ints.contains(&N), "{ints:?}");
        // `r = n + 1` relied on `n`, so it goes too.
        assert!(!ints.contains(&R), "{ints:?}");
        let ints = integer_locals(&control_body());
        assert!(ints.contains(&X), "{ints:?}");
    }

    #[test]
    fn the_shared_predicate_is_what_drops_the_seeded_int32_local() {
        // The same body with the early read moved below the declaration is
        // proven: only the read's position differs.
        let mut late = issue_body();
        let early = late.remove(2);
        late.insert(4, early);
        let ints = integer_locals(&late);
        assert!(ints.contains(&N), "{ints:?}");
    }

    // ---- paths that cannot complete normally ----

    fn case(test: Option<Expr>, body: Vec<Stmt>) -> SwitchCase {
        SwitchCase { test, body }
    }

    fn switch(cases: Vec<SwitchCase>) -> Stmt {
        Stmt::Switch {
            discriminant: get(K),
            cases,
        }
    }

    fn throw() -> Stmt {
        Stmt::Throw(Expr::Integer(0))
    }

    /// `let x; <stmts>; return x;`, flagged or not.
    fn flags_x(stmts: Vec<Stmt>) -> bool {
        let mut body = vec![let_(X, None)];
        body.extend(stmts);
        body.push(Stmt::Return(Some(get(X))));
        reads_before_write(&body, &HashSet::from([X])).contains(&X)
    }

    #[test]
    fn a_switch_whose_default_throws_assigns_on_every_continuing_path() {
        // parsePseudoBigInt's `log2Base`.
        assert!(!flags_x(vec![switch(vec![
            case(
                Some(Expr::Integer(2)),
                vec![set(X, Expr::Integer(1)), Stmt::Break]
            ),
            case(
                Some(Expr::Integer(8)),
                vec![set(X, Expr::Integer(3)), Stmt::Break]
            ),
            case(None, vec![throw()]),
        ])]));
        // A default that returns is the same.
        assert!(!flags_x(vec![switch(vec![
            case(
                Some(Expr::Integer(2)),
                vec![set(X, Expr::Integer(1)), Stmt::Break]
            ),
            case(None, vec![Stmt::Return(None)]),
        ])]));
    }

    #[test]
    fn a_switch_without_default_may_take_no_case() {
        assert!(flags_x(vec![switch(vec![
            case(
                Some(Expr::Integer(2)),
                vec![set(X, Expr::Integer(1)), Stmt::Break]
            ),
            case(
                Some(Expr::Integer(8)),
                vec![set(X, Expr::Integer(3)), Stmt::Break]
            ),
        ])]));
    }

    #[test]
    fn a_break_completes_the_switch_with_its_own_state() {
        assert!(flags_x(vec![switch(vec![
            case(Some(Expr::Integer(2)), vec![Stmt::Break]),
            case(None, vec![set(X, Expr::Integer(1))]),
        ])]));
    }

    #[test]
    fn fallthrough_carries_the_state_into_the_next_case() {
        // `case 1:` falls into `case 2:`, which writes.
        assert!(!flags_x(vec![switch(vec![
            case(Some(Expr::Integer(1)), vec![]),
            case(
                Some(Expr::Integer(2)),
                vec![set(X, Expr::Integer(1)), Stmt::Break]
            ),
            case(None, vec![set(X, Expr::Integer(2))]),
        ])]));
        // A default above the last case: the last case completes the switch.
        assert!(!flags_x(vec![switch(vec![
            case(None, vec![throw()]),
            case(Some(Expr::Integer(1)), vec![set(X, Expr::Integer(1))]),
        ])]));
        // The default entered from no match does not see the case above's
        // write: a read in it is flagged.
        let body = vec![
            let_(X, None),
            switch(vec![
                case(Some(Expr::Integer(1)), vec![set(X, Expr::Integer(1))]),
                case(None, vec![Stmt::Expr(get(X))]),
            ]),
        ];
        assert!(reads_before_write(&body, &HashSet::from([X])).contains(&X));
    }

    #[test]
    fn an_if_with_one_returning_arm_continues_only_through_the_other() {
        assert!(!flags_x(vec![Stmt::If {
            condition: get(K),
            then_branch: vec![Stmt::Return(None)],
            else_branch: Some(vec![set(X, Expr::Integer(1))]),
        }]));
        // Without the return the then-path reaches the read unwritten.
        assert!(flags_x(vec![Stmt::If {
            condition: get(K),
            then_branch: vec![],
            else_branch: Some(vec![set(X, Expr::Integer(1))]),
        }]));
    }

    #[test]
    fn a_try_whose_catch_rethrows_continues_only_from_the_block() {
        let try_ = |catch_body: Vec<Stmt>| Stmt::Try {
            body: vec![set(X, Expr::Integer(1))],
            catch: Some(CatchClause {
                param: None,
                body: catch_body,
            }),
            finally: None,
        };
        assert!(!flags_x(vec![try_(vec![throw()])]));
        assert!(flags_x(vec![try_(vec![])]));
    }

    #[test]
    fn finally_completion_decides_the_try() {
        // A finally that returns: nothing continues past the try.
        assert!(!flags_x(vec![Stmt::If {
            condition: get(K),
            then_branch: vec![Stmt::Try {
                body: vec![],
                catch: None,
                finally: Some(vec![Stmt::Return(None)]),
            }],
            else_branch: Some(vec![set(X, Expr::Integer(1))]),
        }]));
        // A finally that completes does not make an unwritten catch path
        // terminate.
        assert!(flags_x(vec![Stmt::Try {
            body: vec![set(X, Expr::Integer(1))],
            catch: Some(CatchClause {
                param: None,
                body: vec![],
            }),
            finally: Some(vec![]),
        }]));
        // A finally's write counts after the try ...
        assert!(!flags_x(vec![Stmt::Try {
            body: vec![],
            catch: None,
            finally: Some(vec![set(X, Expr::Integer(1))]),
        }]));
        // ... but a read in it still sees the state before the block.
        let body = vec![
            let_(X, None),
            Stmt::Try {
                body: vec![set(X, Expr::Integer(1))],
                catch: None,
                finally: Some(vec![Stmt::Expr(get(X))]),
            },
        ];
        assert!(reads_before_write(&body, &HashSet::from([X])).contains(&X));
    }

    #[test]
    fn a_loop_body_is_never_assumed_to_run() {
        assert!(flags_x(vec![Stmt::While {
            condition: Expr::Bool(true),
            body: vec![set(X, Expr::Integer(1)), Stmt::Break],
        }]));
        assert!(flags_x(vec![Stmt::DoWhile {
            body: vec![set(X, Expr::Integer(1))],
            condition: Expr::Bool(false),
        }]));
    }

    #[test]
    fn a_break_inside_a_switch_inside_a_loop_leaves_only_the_switch() {
        let in_loop = |cases: Vec<SwitchCase>| {
            vec![
                let_(X, None),
                Stmt::While {
                    condition: get(K),
                    body: vec![switch(cases), Stmt::Expr(get(X))],
                },
            ]
        };
        let ok = in_loop(vec![
            case(
                Some(Expr::Integer(1)),
                vec![set(X, Expr::Integer(1)), Stmt::Break],
            ),
            case(None, vec![Stmt::Continue]),
        ]);
        assert!(reads_before_write(&ok, &HashSet::from([X])).is_empty());
        let bad = in_loop(vec![
            case(Some(Expr::Integer(1)), vec![Stmt::Break]),
            case(None, vec![set(X, Expr::Integer(1))]),
        ]);
        assert!(reads_before_write(&bad, &HashSet::from([X])).contains(&X));
    }

    #[test]
    fn labeled_jumps_leave_the_construct_they_name() {
        // outer: while (k) { switch (k) { case 1: x = 1; break;
        //   default: continue outer; } read x }
        let labeled = |default: Vec<Stmt>| {
            vec![
                let_(X, None),
                Stmt::Labeled {
                    label: "outer".into(),
                    body: Box::new(Stmt::While {
                        condition: get(K),
                        body: vec![
                            switch(vec![
                                case(
                                    Some(Expr::Integer(1)),
                                    vec![set(X, Expr::Integer(1)), Stmt::Break],
                                ),
                                case(None, default),
                            ]),
                            Stmt::Expr(get(X)),
                        ],
                    }),
                },
            ]
        };
        let c = HashSet::from([X]);
        assert!(
            reads_before_write(&labeled(vec![Stmt::LabeledContinue("outer".into())]), &c)
                .is_empty()
        );
        assert!(
            reads_before_write(&labeled(vec![Stmt::LabeledBreak("outer".into())]), &c).is_empty()
        );
        // A labeled break leaves the labeled loop, not just the switch: the
        // read after the loop is reached by it unwritten.
        let mut after = labeled(vec![Stmt::LabeledBreak("outer".into())]);
        after.push(Stmt::Return(Some(get(X))));
        assert!(reads_before_write(&after, &c).contains(&X));
        // An unlabeled break here leaves only the switch.
        assert!(reads_before_write(&labeled(vec![Stmt::Break]), &c).contains(&X));
    }

    #[test]
    fn an_int32_local_written_in_every_continuing_case_is_an_int32_local() {
        let body = vec![
            let_(X, Some(Expr::Undefined)),
            switch(vec![
                case(
                    Some(Expr::Integer(2)),
                    vec![set(X, Expr::Integer(1)), Stmt::Break],
                ),
                case(
                    Some(Expr::Integer(8)),
                    vec![set(X, Expr::Integer(3)), Stmt::Break],
                ),
                case(None, vec![throw()]),
            ]),
            Stmt::Return(Some(bin(BinaryOp::BitOr, get(X), Expr::Integer(0)))),
        ];
        let ints = integer_locals(&body);
        assert!(ints.contains(&X), "{ints:?}");
        // Without the default the no-match path returns the `undefined` seed.
        let mut no_default = body.clone();
        if let Stmt::Switch { cases, .. } = &mut no_default[1] {
            cases.pop();
        }
        let ints = integer_locals(&no_default);
        assert!(!ints.contains(&X), "{ints:?}");
    }

    #[test]
    fn a_labeled_block_runs_once_and_its_jumps_reach_its_end() {
        // blk: { if (k) break blk; x = 1; }  (lowered: blk: do { .. } while (false))
        let block = |inner: Vec<Stmt>| Stmt::Labeled {
            label: "blk".into(),
            body: Box::new(Stmt::DoWhile {
                body: inner,
                condition: Expr::Bool(false),
            }),
        };
        assert!(!flags_x(vec![block(vec![set(X, Expr::Integer(1))])]));
        assert!(flags_x(vec![block(vec![
            Stmt::If {
                condition: get(K),
                then_branch: vec![Stmt::LabeledBreak("blk".into())],
                else_branch: None,
            },
            set(X, Expr::Integer(1)),
        ])]));
        // while (k) { blk: { if (k) break; x = 1; } read x }: the unlabeled
        // break may leave the loop (JS) or just the block (the lowering);
        // either way the read is not proven.
        let c = HashSet::from([X]);
        let body = vec![
            let_(X, None),
            Stmt::While {
                condition: get(K),
                body: vec![
                    block(vec![
                        Stmt::If {
                            condition: get(K),
                            then_branch: vec![Stmt::Break],
                            else_branch: None,
                        },
                        set(X, Expr::Integer(1)),
                    ]),
                    Stmt::Expr(get(X)),
                ],
            },
        ];
        assert!(reads_before_write(&body, &c).contains(&X));
        // A `do .. while (false)` that is not a labeled block is a loop.
        assert!(flags_x(vec![Stmt::DoWhile {
            body: vec![set(X, Expr::Integer(1))],
            condition: Expr::Bool(false),
        }]));
    }

    // ---- function bindings run where they are called or handed on ----

    const F: u32 = 10;
    const G: u32 = 11;

    fn closure(func_id: u32, captures: Vec<u32>, body: Vec<Stmt>) -> Expr {
        Expr::Closure {
            func_id,
            params: Vec::new(),
            return_type: HirType::Any,
            body,
            captures,
            mutable_captures: Vec::new(),
            captures_this: false,
            captures_new_target: false,
            enclosing_class: None,
            is_arrow: false,
            is_async: false,
            is_generator: false,
            is_strict: true,
        }
    }

    fn call(callee: Expr, args: Vec<Expr>) -> Expr {
        Expr::Call {
            byte_end: 0,
            callee: Box::new(callee),
            args,
            type_args: Vec::new(),
            byte_offset: 0,
        }
    }

    /// `function f() { return x; }` hoisted (its cell preallocated), then
    /// `<before>; let x = 0; <after>`.
    fn hoisted(before: Vec<Stmt>, after: Vec<Stmt>) -> Vec<Stmt> {
        let mut body = vec![
            Stmt::PreallocateBoxes(vec![X]),
            let_(
                F,
                Some(closure(100, vec![X], vec![Stmt::Return(Some(get(X)))])),
            ),
        ];
        body.extend(before);
        body.push(let_(X, Some(Expr::Integer(0))));
        body.extend(after);
        body
    }

    fn flagged(body: &[Stmt]) -> bool {
        reads_before_write(body, &HashSet::from([X])).contains(&X)
    }

    #[test]
    fn a_function_binding_reads_its_captures_where_it_is_called() {
        // Called only after the counter is written.
        assert!(!flagged(&hoisted(
            vec![],
            vec![Stmt::Expr(call(get(F), vec![]))]
        )));
        // Called before.
        assert!(flagged(&hoisted(
            vec![Stmt::Expr(call(get(F), vec![]))],
            vec![]
        )));
        // A direct call runs the body after its arguments: `f(x = 1)`.
        let body = vec![
            Stmt::PreallocateBoxes(vec![X]),
            let_(
                F,
                Some(closure(100, vec![X], vec![Stmt::Return(Some(get(X)))])),
            ),
            let_(X, None),
            Stmt::Expr(call(
                get(F),
                vec![Expr::LocalSet(X, Box::new(Expr::Integer(1)))],
            )),
        ];
        assert!(!flagged(&body));
    }

    #[test]
    fn every_other_use_of_a_function_binding_is_where_it_may_run() {
        // Passed as an argument, stored, returned before the write.
        let escapes = [
            Stmt::Expr(call(Expr::Undefined, vec![get(F)])),
            Stmt::Expr(Expr::LocalSet(N, Box::new(get(F)))),
            Stmt::If {
                condition: get(K),
                then_branch: vec![Stmt::Return(Some(get(F)))],
                else_branch: None,
            },
        ];
        for e in escapes {
            assert!(flagged(&hoisted(vec![e.clone()], vec![])), "{e:?}");
            assert!(!flagged(&hoisted(vec![], vec![e.clone()])), "{e:?}");
        }
    }

    #[test]
    fn a_function_binding_runs_where_the_bindings_using_it_run() {
        // function g() { return f(); } hoisted too; g() before the write.
        let g = let_(
            G,
            Some(closure(
                101,
                vec![F],
                vec![Stmt::Return(Some(call(get(F), vec![])))],
            )),
        );
        let mut body = hoisted(vec![Stmt::Expr(call(get(G), vec![]))], vec![]);
        body.insert(2, g.clone());
        assert!(flagged(&body));
        let mut body = hoisted(vec![], vec![Stmt::Expr(call(get(G), vec![]))]);
        body.insert(2, g);
        assert!(!flagged(&body));
        // A recursive pair called after the write.
        let f = let_(
            F,
            Some(closure(
                100,
                vec![X, G],
                vec![Stmt::Return(Some(bin(
                    BinaryOp::Add,
                    get(X),
                    call(get(G), vec![]),
                )))],
            )),
        );
        let g = let_(
            G,
            Some(closure(
                101,
                vec![F],
                vec![Stmt::Return(Some(call(get(F), vec![])))],
            )),
        );
        let pair = |at_end: bool| {
            let mut body = vec![Stmt::PreallocateBoxes(vec![X]), f.clone(), g.clone()];
            let c = Stmt::Expr(call(get(G), vec![]));
            if !at_end {
                body.push(c.clone());
            }
            body.push(let_(X, Some(Expr::Integer(0))));
            if at_end {
                body.push(c);
            }
            body
        };
        assert!(!flagged(&pair(true)));
        assert!(flagged(&pair(false)));
        // An arrow created before the write that calls f reads x there.
        let arrow = Stmt::Expr(call(
            Expr::Undefined,
            vec![closure(
                102,
                vec![F],
                vec![Stmt::Return(Some(call(get(F), vec![])))],
            )],
        ));
        assert!(flagged(&hoisted(vec![arrow.clone()], vec![])));
        assert!(!flagged(&hoisted(vec![], vec![arrow])));
    }

    #[test]
    fn unknown_function_bindings_read_at_creation() {
        let after = vec![Stmt::Expr(call(get(F), vec![]))];
        // The name is written again.
        let mut body = hoisted(vec![], after.clone());
        body.push(set(F, Expr::Undefined));
        assert!(flagged(&body));
        // A capture without a preallocated cell is copied at creation.
        let mut body = hoisted(vec![], after.clone());
        body.remove(0);
        assert!(flagged(&body));
        // A direct eval can reach the binding by name.
        let mut body = hoisted(vec![], after.clone());
        body.push(Stmt::Expr(call(
            Expr::ExternFuncRef {
                name: "js_eval_in_scope".into(),
                param_types: Vec::new(),
                return_type: HirType::Any,
            },
            vec![],
        )));
        assert!(flagged(&body));
        // Control: the same body without those.
        assert!(!flagged(&hoisted(vec![], after)));
    }
}
