use super::LocalWrite;
use perry_hir::{Expr, Stmt};
use std::collections::{HashMap, HashSet};
use std::sync::OnceLock;

type Writes = HashMap<u32, Vec<LocalWrite>>;

/// Only the borrowed source occurrence owns a memo entry. Lowering also
/// analyzes temporary HIR copies, whose allocations can be reused.
#[derive(Default)]
pub(crate) struct ClosureWrites<'hir> {
    sources: HashMap<u32, (&'hir Expr, OnceLock<Writes>)>,
}

impl<'hir> ClosureWrites<'hir> {
    pub(crate) fn pointer_locals(
        &self,
        params: &[perry_hir::Param],
        stmts: &[Stmt],
        flat_const_ids: &HashSet<u32>,
    ) -> HashMap<u32, u32> {
        super::pointer_locals(self, params, stmts, flat_const_ids)
    }

    pub(crate) fn new(closures: &[(u32, &'hir Expr)]) -> Self {
        Self {
            sources: closures
                .iter()
                .map(|&(id, e)| (id, (e, OnceLock::new())))
                .collect(),
        }
    }
    fn collect_expr_writes_in_closure_stmts(
        &self,
        stmts: &[Stmt],
        writes: &mut HashMap<u32, Vec<LocalWrite>>,
    ) {
        for stmt in stmts {
            match stmt {
                Stmt::Let { init, .. } => {
                    if let Some(init) = init {
                        self.collect_expr_writes(init, writes);
                    }
                }
                Stmt::Expr(expr) | Stmt::Return(Some(expr), _) | Stmt::Throw(expr) => {
                    self.collect_expr_writes(expr, writes);
                }
                Stmt::Return(None, _)
                | Stmt::Break
                | Stmt::Continue
                | Stmt::LabeledBreak(_)
                | Stmt::LabeledContinue(_)
                | Stmt::PreallocateBoxes(_)
                | Stmt::PreallocateTdzBoxes(_)
                | Stmt::ReleaseBoxes(_) => {}
                Stmt::If {
                    condition,
                    then_branch,
                    else_branch,
                } => {
                    self.collect_expr_writes(condition, writes);
                    self.collect_expr_writes_in_closure_stmts(then_branch, writes);
                    if let Some(else_branch) = else_branch {
                        self.collect_expr_writes_in_closure_stmts(else_branch, writes);
                    }
                }
                Stmt::While { condition, body } => {
                    self.collect_expr_writes(condition, writes);
                    self.collect_expr_writes_in_closure_stmts(body, writes);
                }
                Stmt::DoWhile { body, condition } => {
                    self.collect_expr_writes_in_closure_stmts(body, writes);
                    self.collect_expr_writes(condition, writes);
                }
                Stmt::For {
                    init,
                    condition,
                    update,
                    body,
                } => {
                    if let Some(init) = init {
                        self.collect_expr_writes_in_closure_stmts(
                            std::slice::from_ref(init.as_ref()),
                            writes,
                        );
                    }
                    if let Some(condition) = condition {
                        self.collect_expr_writes(condition, writes);
                    }
                    if let Some(update) = update {
                        self.collect_expr_writes(update, writes);
                    }
                    self.collect_expr_writes_in_closure_stmts(body, writes);
                }
                Stmt::Labeled { body, .. } => {
                    self.collect_expr_writes_in_closure_stmts(
                        std::slice::from_ref(body.as_ref()),
                        writes,
                    );
                }
                Stmt::Try {
                    body,
                    catch,
                    finally,
                    ..
                } => {
                    self.collect_expr_writes_in_closure_stmts(body, writes);
                    if let Some(catch) = catch {
                        self.collect_expr_writes_in_closure_stmts(&catch.body, writes);
                    }
                    if let Some(finally) = finally {
                        self.collect_expr_writes_in_closure_stmts(finally, writes);
                    }
                }
                Stmt::Switch {
                    discriminant,
                    cases, ..
                } => {
                    self.collect_expr_writes(discriminant, writes);
                    for case in cases {
                        if let Some(test) = &case.test {
                            self.collect_expr_writes(test, writes);
                        }
                        self.collect_expr_writes_in_closure_stmts(&case.body, writes);
                    }
                }
            }
        }
    }

    /// An enclosing frame needs only writes to its own bindings. Each nested
    /// closure filters its declarations before its free writes propagate.
    fn collect_direct_let_ids(stmts: &[Stmt], out: &mut HashSet<u32>) {
        for stmt in stmts {
            match stmt {
                Stmt::Let { id, .. } => {
                    out.insert(*id);
                }
                Stmt::If {
                    then_branch,
                    else_branch,
                    ..
                } => {
                    Self::collect_direct_let_ids(then_branch, out);
                    if let Some(else_branch) = else_branch {
                        Self::collect_direct_let_ids(else_branch, out);
                    }
                }
                Stmt::While { body, .. } | Stmt::DoWhile { body, .. } => {
                    Self::collect_direct_let_ids(body, out);
                }
                Stmt::For { init, body, .. } => {
                    if let Some(init) = init {
                        Self::collect_direct_let_ids(std::slice::from_ref(init.as_ref()), out);
                    }
                    Self::collect_direct_let_ids(body, out);
                }
                Stmt::Labeled { body, .. } => {
                    Self::collect_direct_let_ids(std::slice::from_ref(body.as_ref()), out);
                }
                Stmt::Try {
                    body,
                    catch,
                    finally,
                    ..
                } => {
                    Self::collect_direct_let_ids(body, out);
                    if let Some(catch) = catch {
                        Self::collect_direct_let_ids(&catch.body, out);
                    }
                    if let Some(finally) = finally {
                        Self::collect_direct_let_ids(finally, out);
                    }
                }
                Stmt::Switch { cases, .. } => {
                    for case in cases {
                        Self::collect_direct_let_ids(&case.body, out);
                    }
                }
                _ => {}
            }
        }
    }

    pub(super) fn collect_expr_writes(
        &self,
        expr: &Expr,
        writes: &mut HashMap<u32, Vec<LocalWrite>>,
    ) {
        match expr {
            Expr::LocalSet(id, rhs) => {
                writes
                    .entry(*id)
                    .or_default()
                    .push(LocalWrite::Expr((**rhs).clone()));
                self.collect_expr_writes(rhs, writes);
            }
            Expr::Update { id, .. } => {
                writes.entry(*id).or_default().push(LocalWrite::NonPointer);
            }
            Expr::Closure {
                func_id,
                params,
                body,
                ..
            } => {
                for param in params {
                    if let Some(default) = &param.default {
                        self.collect_expr_writes(default, writes);
                    }
                }
                let compute = || {
                    let mut sub = HashMap::new();
                    self.collect_expr_writes_in_closure_stmts(body, &mut sub);
                    let mut own: HashSet<u32> = params.iter().map(|p| p.id).collect();
                    Self::collect_direct_let_ids(body, &mut own);
                    sub.retain(|id, _| !own.contains(id));
                    sub
                };
                let uncached;
                let sub = if let Some((_, cell)) = self
                    .sources
                    .get(func_id)
                    .filter(|(source, _)| std::ptr::eq(*source, expr))
                {
                    cell.get_or_init(compute)
                } else {
                    uncached = compute();
                    &uncached
                };
                for (id, ws) in sub.iter() {
                    writes.entry(*id).or_default().extend(ws.iter().cloned());
                }
            }
            _ => {
                perry_hir::walker::walk_expr_children(expr, &mut |child| {
                    self.collect_expr_writes(child, writes)
                });
            }
        }
    }
}
