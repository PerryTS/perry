//! Alias-edge collection for the `Ptr<Shape>` containment proof.

use super::*;

/// Collect `Let { mutable: false, init: Some(LocalGet(src)) }` edges — the
/// alias shape the exact-receiver inliner emits for compound assigns.
pub(in crate::collectors) fn collect_alias_edges(stmts: &[Stmt], out: &mut Vec<(u32, u32)>) {
    for s in stmts {
        match s {
            Stmt::Let {
                id,
                mutable: false,
                init: Some(Expr::LocalGet(src)),
                ..
            } => out.push((*id, *src)),
            Stmt::If {
                then_branch,
                else_branch,
                ..
            } => {
                collect_alias_edges(then_branch, out);
                if let Some(eb) = else_branch {
                    collect_alias_edges(eb, out);
                }
            }
            Stmt::While { body, .. } | Stmt::DoWhile { body, .. } => {
                collect_alias_edges(body, out);
            }
            Stmt::For { init, body, .. } => {
                if let Some(init) = init {
                    collect_alias_edges(std::slice::from_ref(init.as_ref()), out);
                }
                collect_alias_edges(body, out);
            }
            Stmt::Try {
                body,
                catch,
                finally,
            } => {
                collect_alias_edges(body, out);
                if let Some(c) = catch {
                    collect_alias_edges(&c.body, out);
                }
                if let Some(f) = finally {
                    collect_alias_edges(f, out);
                }
            }
            Stmt::Switch { cases, .. } => {
                for case in cases {
                    collect_alias_edges(&case.body, out);
                }
            }
            Stmt::Labeled { body, .. } => {
                collect_alias_edges(std::slice::from_ref(body.as_ref()), out);
            }
            _ => {}
        }
    }
}
