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

/// The test's clip kind and bound when it compares `id` with a literal or a
/// local: `id < e` (or `e > id`, ...).
fn test_bound(c: &Expr, id: u32) -> Option<(u8, &Expr)> {
    use crate::model::range::{CLIP_GE, CLIP_GT, CLIP_LE, CLIP_LT};
    let Expr::Compare { op, left, right } = c else {
        return None;
    };
    let is_id = |e: &Expr| matches!(e, Expr::LocalGet(l) if *l == id);
    let (kind, bound) = if is_id(left) {
        let k = match op {
            CompareOp::Lt => CLIP_LT,
            CompareOp::Le => CLIP_LE,
            CompareOp::Gt => CLIP_GT,
            CompareOp::Ge => CLIP_GE,
            _ => return None,
        };
        (k, right.as_ref())
    } else if is_id(right) {
        let k = match op {
            CompareOp::Gt => CLIP_LT,
            CompareOp::Ge => CLIP_LE,
            CompareOp::Lt => CLIP_GT,
            CompareOp::Le => CLIP_GE,
            _ => return None,
        };
        (k, left.as_ref())
    } else {
        return None;
    };
    matches!(
        bound,
        Expr::Integer(_) | Expr::Number(_) | Expr::LocalGet(_)
    )
    .then_some((kind, bound))
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
        let Some((kind, bound)) = condition.and_then(|c| test_bound(c, id)) else {
            return false;
        };
        if let Expr::LocalGet(b) = bound {
            if *b == id {
                return false;
            }
        }
        let closure = &mut |e: &Expr| matches!(e, Expr::Closure { .. });
        if body.iter().any(|s| stmt_has(s, closure))
            || update.is_some_and(|u| expr_has(u, closure))
            || condition.is_some_and(|c| expr_has(c, closure))
        {
            return false;
        }
        let mi = fx.module;
        let i = self.local(mi, id);
        let b = match bound {
            Expr::Integer(v) => self.num_literal(*v as f64),
            Expr::Number(v) => self.num_literal(*v),
            Expr::LocalGet(l) => self.local(mi, *l),
            _ => return false,
        };
        let start = self.tmp("narrow");
        self.m.edge(i, start);
        self.range_rule(crate::model::RangeOp::Clip(kind), i, Some(b), start);
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
