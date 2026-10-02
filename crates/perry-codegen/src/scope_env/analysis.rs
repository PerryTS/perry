//! Dominance facts for one function-like body, shared by the grouping pass
//! and by codegen's [`super::ScopeMap`] validation.
//!
//! Statements are numbered in pre-order (closure bodies are not entered; a
//! reference inside a closure counts at the statement that creates the
//! closure). A statement list's subtree occupies a contiguous number range, so
//! "every reference sits in the home statement or in a later sibling of it" is
//! the interval test `home.num <= ref < end(home list)`, where `end` is one
//! past the list's last statement number. That bound is the NEXT statement's
//! number: after a switch case it is the first statement of the following
//! case, which shares the binding's scope but not the home's activation, so an
//! inclusive test grouped a binding that case reads before its `let` ever ran
//! (#11771).

use std::collections::{BTreeSet, HashMap, HashSet};

use perry_hir::{Expr, Module as HirModule, Stmt};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum DeclKind {
    Let,
    Prealloc,
    PreallocTdz,
    /// A `for` head, catch parameter or labeled statement: not in a list.
    Unlisted,
}

#[derive(Clone, Copy, Debug)]
pub(super) struct ListPos {
    /// Address of the list's buffer. Stable between the analysis and the
    /// rewrite, which do not reallocate any list before it is looked up.
    pub ptr: usize,
    pub index: usize,
    pub token: u32,
}

#[derive(Clone, Copy, Debug)]
pub(super) struct Decl {
    pub num: u32,
    pub pos: Option<ListPos>,
    pub kind: DeclKind,
}

pub(super) struct PreallocStmt {
    pub num: u32,
    pub ids: Vec<u32>,
    pub tdz: bool,
}

#[derive(Default)]
pub(super) struct BodyFacts {
    pub decls: HashMap<u32, Vec<Decl>>,
    /// Lowest and highest statement number referencing the id.
    pub refs: HashMap<u32, (u32, u32)>,
    /// One past the highest statement number in each list (exclusive).
    pub list_end: HashMap<u32, u32>,
    pub preallocs: Vec<PreallocStmt>,
}

impl BodyFacts {
    /// The id's home declaration when it dominates every declaration and
    /// reference of the id in this body.
    pub fn dominating_home(&self, id: u32) -> Option<Decl> {
        let decls = self.decls.get(&id)?;
        let home = *decls.first()?;
        if home.kind == DeclKind::Unlisted {
            return None;
        }
        let pos = home.pos?;
        let end = *self.list_end.get(&pos.token)?;
        if decls.iter().any(|d| d.num < home.num || d.num >= end) {
            return None;
        }
        if let Some(&(lo, hi)) = self.refs.get(&id) {
            if lo < home.num || hi >= end {
                return None;
            }
        }
        Some(home)
    }
}

pub(super) fn analyze_body(stmts: &[Stmt], interest: &HashSet<u32>) -> BodyFacts {
    let mut an = Analyzer {
        interest,
        counter: 0,
        next_token: 0,
        facts: BodyFacts::default(),
        scratch: HashSet::new(),
    };
    an.list(stmts);
    an.facts
}

struct Analyzer<'a> {
    interest: &'a HashSet<u32>,
    counter: u32,
    next_token: u32,
    facts: BodyFacts,
    scratch: HashSet<u32>,
}

impl Analyzer<'_> {
    fn list(&mut self, stmts: &[Stmt]) {
        let token = self.next_token;
        self.next_token += 1;
        let ptr = stmts.as_ptr() as usize;
        for (index, s) in stmts.iter().enumerate() {
            self.stmt(s, Some(ListPos { ptr, index, token }));
        }
        self.facts.list_end.insert(token, self.counter);
    }

    fn decl(&mut self, id: u32, num: u32, pos: Option<ListPos>, kind: DeclKind) {
        if !self.interest.contains(&id) {
            return;
        }
        let kind = if pos.is_none() {
            DeclKind::Unlisted
        } else {
            kind
        };
        self.facts
            .decls
            .entry(id)
            .or_default()
            .push(Decl { num, pos, kind });
    }

    fn expr(&mut self, e: &Expr, num: u32) {
        self.scratch.clear();
        crate::collectors::collect_ref_ids_in_expr(e, &mut self.scratch);
        for id in self.scratch.iter() {
            if self.interest.contains(id) {
                let r = self.facts.refs.entry(*id).or_insert((num, num));
                r.0 = r.0.min(num);
                r.1 = r.1.max(num);
            }
        }
    }

    fn stmt(&mut self, s: &Stmt, pos: Option<ListPos>) {
        let num = self.counter;
        self.counter += 1;
        match s {
            Stmt::Let { id, init, .. } => {
                self.decl(*id, num, pos, DeclKind::Let);
                if let Some(e) = init {
                    self.expr(e, num);
                }
            }
            Stmt::Expr(e) | Stmt::Throw(e) => self.expr(e, num),
            Stmt::Return(e) => {
                if let Some(e) = e {
                    self.expr(e, num);
                }
            }
            Stmt::If {
                condition,
                then_branch,
                else_branch,
            } => {
                self.expr(condition, num);
                self.list(then_branch);
                if let Some(eb) = else_branch {
                    self.list(eb);
                }
            }
            Stmt::While { condition, body } => {
                self.expr(condition, num);
                self.list(body);
            }
            Stmt::DoWhile { body, condition } => {
                self.expr(condition, num);
                self.list(body);
            }
            Stmt::For {
                init,
                condition,
                update,
                body,
            } => {
                if let Some(c) = condition {
                    self.expr(c, num);
                }
                if let Some(u) = update {
                    self.expr(u, num);
                }
                if let Some(i) = init {
                    self.stmt(i, None);
                }
                self.list(body);
            }
            Stmt::Labeled { body, .. } => self.stmt(body, None),
            Stmt::Try {
                body,
                catch,
                finally,
            } => {
                self.list(body);
                if let Some(c) = catch {
                    if let Some((id, _)) = &c.param {
                        self.decl(*id, num, None, DeclKind::Unlisted);
                    }
                    self.list(&c.body);
                }
                if let Some(f) = finally {
                    self.list(f);
                }
            }
            Stmt::Switch {
                discriminant,
                cases,
            } => {
                self.expr(discriminant, num);
                for case in cases {
                    if let Some(t) = &case.test {
                        self.expr(t, num);
                    }
                }
                for case in cases {
                    self.list(&case.body);
                }
            }
            Stmt::PreallocateBoxes(ids) | Stmt::PreallocateTdzBoxes(ids) => {
                let kind = if matches!(s, Stmt::PreallocateTdzBoxes(_)) {
                    DeclKind::PreallocTdz
                } else {
                    DeclKind::Prealloc
                };
                for id in ids {
                    self.decl(*id, num, pos, kind);
                }
                self.facts.preallocs.push(PreallocStmt {
                    num,
                    ids: ids.clone(),
                    tdz: kind == DeclKind::PreallocTdz,
                });
            }
            Stmt::ReleaseBoxes(_)
            | Stmt::Break
            | Stmt::Continue
            | Stmt::LabeledBreak(_)
            | Stmt::LabeledContinue(_) => {}
        }
    }
}

/// Ids named by a preallocation statement of this body (closures excluded).
pub(super) fn collect_shallow_prealloc_ids(stmts: &[Stmt], out: &mut HashSet<u32>) {
    for_each_stmt_shallow(stmts, &mut |s| {
        if let Stmt::PreallocateBoxes(ids) | Stmt::PreallocateTdzBoxes(ids) = s {
            out.extend(ids.iter().copied());
        }
    });
}

/// Visit every statement of this body, entering nested statement lists but
/// not closure bodies.
pub(super) fn for_each_stmt_shallow(stmts: &[Stmt], f: &mut dyn FnMut(&Stmt)) {
    for s in stmts {
        f(s);
        match s {
            Stmt::If {
                then_branch,
                else_branch,
                ..
            } => {
                for_each_stmt_shallow(then_branch, f);
                if let Some(eb) = else_branch {
                    for_each_stmt_shallow(eb, f);
                }
            }
            Stmt::While { body, .. } | Stmt::DoWhile { body, .. } => for_each_stmt_shallow(body, f),
            Stmt::For { init, body, .. } => {
                if let Some(i) = init {
                    for_each_stmt_shallow(std::slice::from_ref(i.as_ref()), f);
                }
                for_each_stmt_shallow(body, f);
            }
            Stmt::Labeled { body, .. } => {
                for_each_stmt_shallow(std::slice::from_ref(body.as_ref()), f)
            }
            Stmt::Try {
                body,
                catch,
                finally,
            } => {
                for_each_stmt_shallow(body, f);
                if let Some(c) = catch {
                    for_each_stmt_shallow(&c.body, f);
                }
                if let Some(fin) = finally {
                    for_each_stmt_shallow(fin, f);
                }
            }
            Stmt::Switch { cases, .. } => {
                for case in cases {
                    for_each_stmt_shallow(&case.body, f);
                }
            }
            _ => {}
        }
    }
}

/// The top-level expressions of one statement (not its nested lists).
pub(super) fn stmt_exprs<'a>(s: &'a Stmt, f: &mut dyn FnMut(&'a Expr)) {
    match s {
        Stmt::Let { init: Some(e), .. }
        | Stmt::Expr(e)
        | Stmt::Throw(e)
        | Stmt::Return(Some(e)) => f(e),
        Stmt::If { condition, .. }
        | Stmt::While { condition, .. }
        | Stmt::DoWhile { condition, .. } => f(condition),
        Stmt::For {
            condition, update, ..
        } => {
            if let Some(c) = condition {
                f(c);
            }
            if let Some(u) = update {
                f(u);
            }
        }
        Stmt::Switch {
            discriminant,
            cases,
        } => {
            f(discriminant);
            for case in cases {
                if let Some(t) = &case.test {
                    f(t);
                }
            }
        }
        _ => {}
    }
}

/// Call `f` with the body of every closure literal directly in this body's
/// statements (not inside another closure's body — the caller recurses).
pub(super) fn for_each_closure_in_stmts(stmts: &[Stmt], f: &mut dyn FnMut(&[Stmt])) {
    for_each_stmt_shallow(stmts, &mut |s| {
        stmt_exprs(s, &mut |e| for_each_closure_in_expr(e, f));
    });
}

/// Call `f` with the body of every closure literal in `e`, not descending into
/// the bodies themselves (param defaults of a closure are searched).
pub(super) fn for_each_closure_in_expr(e: &Expr, f: &mut dyn FnMut(&[Stmt])) {
    if let Expr::Closure { body, .. } = e {
        f(body);
    }
    perry_hir::walker::walk_expr_children(e, &mut |child| for_each_closure_in_expr(child, f));
}

/// How many preallocation statements in the whole module name each id.
pub(super) fn prealloc_counts(hir: &HirModule) -> HashMap<u32, u32> {
    let mut counts: HashMap<u32, u32> = HashMap::new();
    super::for_each_body(hir, &mut |stmts: &[Stmt]| {
        for_each_stmt_shallow(stmts, &mut |s| {
            if let Stmt::PreallocateBoxes(ids) | Stmt::PreallocateTdzBoxes(ids) = s {
                for id in ids {
                    *counts.entry(*id).or_default() += 1;
                }
            }
        });
    });
    counts
}

/// The retention class every hoisted function declaration of one body shares.
pub(super) const HOISTED_DECL_CLASS: u32 = u32::MAX;

/// `func_id`s of this body's hoisted function declarations: a non-arrow
/// closure bound by a `Stmt::Let` whose binding the body preallocates (HIR
/// hoists a declaration's binding to the body's `PreallocateBoxes`). They are
/// all created at scope entry and form the scope's long-lived API (the module
/// factory shape), so they share one retention class. Arrows and function
/// expressions are values with lifetimes of their own and each keep a class.
pub(super) fn hoisted_decl_closures(stmts: &[Stmt]) -> HashSet<u32> {
    let mut hoisted_ids = HashSet::new();
    collect_shallow_prealloc_ids(stmts, &mut hoisted_ids);
    let mut out = HashSet::new();
    for_each_stmt_shallow(stmts, &mut |s| {
        if let Stmt::Let {
            id,
            init:
                Some(Expr::Closure {
                    func_id,
                    is_arrow: false,
                    ..
                }),
            ..
        } = s
        {
            if hoisted_ids.contains(id) {
                out.insert(*func_id);
            }
        }
    });
    out
}

/// For every id of `interest`, the set of closures (by `func_id`, at any
/// depth below this body) whose body or explicit capture list names it.
pub(super) fn closure_capture_sets(
    stmts: &[Stmt],
    interest: &HashSet<u32>,
) -> HashMap<u32, BTreeSet<u32>> {
    let mut out: HashMap<u32, BTreeSet<u32>> = HashMap::new();
    fn visit_closures(
        stmts: &[Stmt],
        interest: &HashSet<u32>,
        out: &mut HashMap<u32, BTreeSet<u32>>,
    ) {
        for_each_stmt_shallow(stmts, &mut |s| {
            stmt_exprs(s, &mut |e| visit_expr(e, interest, out));
        });
    }
    fn visit_expr(e: &Expr, interest: &HashSet<u32>, out: &mut HashMap<u32, BTreeSet<u32>>) {
        if let Expr::Closure {
            func_id,
            body,
            captures,
            ..
        } = e
        {
            let mut refs = HashSet::new();
            crate::collectors::collect_ref_ids_in_expr(e, &mut refs);
            refs.extend(captures.iter().copied());
            for id in refs {
                if interest.contains(&id) {
                    out.entry(id).or_default().insert(*func_id);
                }
            }
            visit_closures(body, interest, out);
        }
        perry_hir::walker::walk_expr_children(e, &mut |child| visit_expr(child, interest, out));
    }
    visit_closures(stmts, interest, &mut out);
    out
}
