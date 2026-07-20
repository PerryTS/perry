//! Detects unsafe uses of a producer's accumulator local. See
//! [`OutUsageAnalyzer`] for the supported "safe" patterns.

use super::*;

/// Walks an HIR subtree looking for unsafe uses of a target local.
/// "Safe" uses are limited to:
/// - `out.push(value)` — both as `Expr::ArrayPush { array: LocalGet(out), value }`
///   and as a generic `Expr::Call { callee: PropertyGet { LocalGet(out), "push" } }`.
/// - `out[index]` reads (Expr::IndexGet) — they don't escape the
///   array, and the rewrite doesn't change their semantics.
/// - `out.length` reads (PropertyGet `.length`) — same.
/// - The consumer-pattern shape (a parent `for` loop reading
///   `child.length` / `child[j]` and calling `outer.push`) — checked
///   at call-site time.
pub struct OutUsageAnalyzer {
    pub out_id: LocalId,
    pub unsafe_use: bool,
}

impl OutUsageAnalyzer {
    pub fn visit_stmt(&mut self, stmt: &Stmt) {
        if self.unsafe_use {
            return;
        }
        match stmt {
            Stmt::Let { init, .. } => {
                if let Some(e) = init {
                    self.visit_expr(e);
                }
            }
            Stmt::Expr(e) | Stmt::Throw(e) => self.visit_expr(e),
            Stmt::Return(opt) => {
                if let Some(e) = opt {
                    self.visit_expr(e);
                }
            }
            Stmt::If {
                condition,
                then_branch,
                else_branch,
            } => {
                self.visit_expr(condition);
                for s in then_branch {
                    self.visit_stmt(s);
                }
                if let Some(eb) = else_branch {
                    for s in eb {
                        self.visit_stmt(s);
                    }
                }
            }
            Stmt::While { condition, body } | Stmt::DoWhile { body, condition } => {
                self.visit_expr(condition);
                for s in body {
                    self.visit_stmt(s);
                }
            }
            Stmt::For {
                init,
                condition,
                update,
                body,
            } => {
                if let Some(i) = init {
                    self.visit_stmt(i);
                }
                if let Some(c) = condition {
                    self.visit_expr(c);
                }
                if let Some(u) = update {
                    self.visit_expr(u);
                }
                for s in body {
                    self.visit_stmt(s);
                }
            }
            Stmt::Try {
                body,
                catch,
                finally,
            } => {
                for s in body {
                    self.visit_stmt(s);
                }
                if let Some(c) = catch {
                    for s in &c.body {
                        self.visit_stmt(s);
                    }
                }
                if let Some(f) = finally {
                    for s in f {
                        self.visit_stmt(s);
                    }
                }
            }
            Stmt::Switch {
                discriminant,
                cases,
            } => {
                self.visit_expr(discriminant);
                for c in cases {
                    if let Some(e) = &c.test {
                        self.visit_expr(e);
                    }
                    for s in &c.body {
                        self.visit_stmt(s);
                    }
                }
            }
            Stmt::Labeled { body, .. } => self.visit_stmt(body),
            // Other stmt kinds: no expressions to visit at this level.
            _ => {}
        }
    }

    pub fn visit_expr(&mut self, e: &Expr) {
        if self.unsafe_use {
            return;
        }
        // Check for SAFE patterns first; if matched, dive only into
        // their non-`out` subexpressions and skip the catch-all walk
        // below (which would otherwise flag the LocalGet(out) inside
        // them as unsafe).
        match e {
            Expr::ArrayPush { array_id, value } if *array_id == self.out_id => {
                // Safe: out.push(v). Visit only `value`.
                self.visit_expr(value);
                return;
            }
            Expr::ArrayPushSpread { array_id, source } if *array_id == self.out_id => {
                self.visit_expr(source);
                return;
            }
            Expr::PropertyGet {
                object, property, ..
            } if matches!(object.as_ref(), Expr::LocalGet(id) if *id == self.out_id)
                && property == "length" =>
            {
                // Safe: out.length read.
                return;
            }
            Expr::IndexGet { object, index } => {
                if matches!(object.as_ref(), Expr::LocalGet(id) if *id == self.out_id) {
                    // Safe: out[idx] read. Still need to visit index
                    // because it might contain its own out-references.
                    self.visit_expr(index);
                    return;
                }
            }
            Expr::LocalSet(id, _value) if *id == self.out_id => {
                // out = X — disallowed except for the initial Let
                // (which the caller filters out). Any post-init
                // reassignment breaks the rewrite.
                self.unsafe_use = true;
                return;
            }
            Expr::LocalGet(id) if *id == self.out_id => {
                // Bare LocalGet(out) outside of a safe parent pattern.
                self.unsafe_use = true;
                return;
            }
            _ => {}
        }
        // Catch-all: walk children.
        walk_expr_children(e, &mut |child| self.visit_expr(child));
    }
}
