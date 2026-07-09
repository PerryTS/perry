//! `SH` impls for HIR statements. Split out of `stable_hash.rs` (no
//! behavior change).

use super::primitives::{tag, SH};
use super::StableHasher;
use crate::ir::*;

// --- Statements ------------------------------------------------------------

impl SH for Stmt {
    fn hash<H: StableHasher>(&self, h: &mut H) {
        match self {
            Stmt::Let {
                id,
                name,
                ty,
                mutable,
                init,
            } => {
                tag(h, 0);
                id.hash(h);
                name.hash(h);
                ty.hash(h);
                mutable.hash(h);
                init.hash(h);
            }
            Stmt::Expr(e) => {
                tag(h, 1);
                e.hash(h);
            }
            Stmt::Return(e) => {
                tag(h, 2);
                e.hash(h);
            }
            Stmt::If {
                condition,
                then_branch,
                else_branch,
            } => {
                tag(h, 3);
                condition.hash(h);
                then_branch.hash(h);
                else_branch.hash(h);
            }
            Stmt::While { condition, body } => {
                tag(h, 4);
                condition.hash(h);
                body.hash(h);
            }
            Stmt::DoWhile { body, condition } => {
                tag(h, 5);
                body.hash(h);
                condition.hash(h);
            }
            Stmt::For {
                init,
                condition,
                update,
                body,
            } => {
                tag(h, 6);
                init.hash(h);
                condition.hash(h);
                update.hash(h);
                body.hash(h);
            }
            Stmt::Labeled { label, body } => {
                tag(h, 7);
                label.hash(h);
                body.as_ref().hash(h);
            }
            Stmt::Break => tag(h, 8),
            Stmt::Continue => tag(h, 9),
            Stmt::LabeledBreak(s) => {
                tag(h, 10);
                s.hash(h);
            }
            Stmt::LabeledContinue(s) => {
                tag(h, 11);
                s.hash(h);
            }
            Stmt::Throw(e) => {
                tag(h, 12);
                e.hash(h);
            }
            Stmt::Try {
                body,
                catch,
                finally,
            } => {
                tag(h, 13);
                body.hash(h);
                catch.hash(h);
                finally.hash(h);
            }
            Stmt::Switch {
                discriminant,
                cases,
            } => {
                tag(h, 14);
                discriminant.hash(h);
                cases.hash(h);
            }
            Stmt::PreallocateBoxes(ids) => {
                tag(h, 15);
                ids.hash(h);
            }
            Stmt::PreallocateTdzBoxes(ids) => {
                tag(h, 16);
                ids.hash(h);
            }
        }
    }
}

impl SH for SwitchCase {
    fn hash<H: StableHasher>(&self, h: &mut H) {
        let SwitchCase { test, body } = self;
        test.hash(h);
        body.hash(h);
    }
}

impl SH for CatchClause {
    fn hash<H: StableHasher>(&self, h: &mut H) {
        let CatchClause { param, body } = self;
        param.hash(h);
        body.hash(h);
    }
}
