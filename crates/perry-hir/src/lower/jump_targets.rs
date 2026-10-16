//! Jump targets of `break`/`continue` during lowering (#12663).
//!
//! A labeled non-loop statement (`L: { ... }`, `L: if ...`, `L: switch ...`)
//! is lowered to a labeled run-once `L: do { ... } while (false)` so that
//! `break L` has an exit block. That synthetic loop is a target for `break L`
//! only: per spec an unlabeled `break` targets the nearest enclosing
//! iteration or `switch` statement, and an unlabeled `continue` the nearest
//! iteration statement. Every consumer of the HIR (codegen, the generator and
//! finally transforms, the flow analyses) sees the run-once do-while as an
//! ordinary loop, so the lowering keeps one invariant instead of teaching each
//! of them about the synthetic loop:
//!
//!   an unlabeled `Break`/`Continue` never crosses a lowered labeled block.
//!
//! The lowering keeps a stack of jump frames, one per AST loop/switch and one
//! per labeled non-loop block. An unlabeled jump whose real target lies
//! outside a labeled block is emitted as a labeled jump to that target, and
//! the target gets a synthetic label (`Stmt::Labeled` around the loop or
//! switch) when the frame is popped. Jumps that do not cross a labeled block
//! keep their unlabeled form, so code without labeled blocks lowers exactly as
//! before.

use swc_ecma_ast as ast;

use crate::ir::Stmt;

use super::LoweringContext;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum JumpFrameKind {
    /// for / for-in / for-of / while / do-while: target of break and continue.
    Loop,
    /// switch: target of an unlabeled break only.
    Switch,
    /// A labeled non-loop statement: target of `break <its label>` only.
    LabeledBlock,
}

#[derive(Debug)]
pub(crate) struct JumpFrame {
    kind: JumpFrameKind,
    /// The label this loop/switch answers to when an unlabeled jump from
    /// inside a labeled block is rewritten into a labeled one.
    label: String,
    /// Some rewritten jump names `label`; the statement must carry it.
    used: bool,
}

/// The frame an AST statement opens when lowered, if it is a jump target for
/// unlabeled `break`/`continue`.
pub(crate) fn jump_frame_kind(stmt: &ast::Stmt) -> Option<JumpFrameKind> {
    match stmt {
        ast::Stmt::For(_)
        | ast::Stmt::ForIn(_)
        | ast::Stmt::ForOf(_)
        | ast::Stmt::While(_)
        | ast::Stmt::DoWhile(_) => Some(JumpFrameKind::Loop),
        ast::Stmt::Switch(_) => Some(JumpFrameKind::Switch),
        _ => None,
    }
}

impl LoweringContext {
    /// Open the frame of a loop/switch (`stmt`) or labeled block. Returns the
    /// depth to hand back to [`Self::pop_jump_frame`].
    pub(crate) fn push_jump_frame(
        &mut self,
        kind: JumpFrameKind,
        stmt: Option<&ast::Stmt>,
    ) -> usize {
        let label = format!("__perry_jump_{}", self.next_jump_label);
        self.next_jump_label += 1;
        // A rewritten `continue` to a for-of's synthetic label continues that
        // loop: the iterator-close rewrite must know the label names the loop
        // itself (it reads the for-of's labels by source offset).
        if let Some(ast::Stmt::ForOf(for_of)) = stmt {
            self.iterator_loop_labels
                .push((for_of.span.lo.0, label.clone()));
        }
        let depth = self.jump_frames.len();
        self.jump_frames.push(JumpFrame {
            kind,
            label,
            used: false,
        });
        depth
    }

    /// Close the frame opened at `depth`. Returns the synthetic label the
    /// lowered statement must be wrapped in, if any jump was rewritten to it.
    pub(crate) fn pop_jump_frame(&mut self, depth: usize) -> Option<String> {
        let frame = self.jump_frames.drain(depth..).next()?;
        frame.used.then_some(frame.label)
    }

    /// Lower an unlabeled `break`.
    pub(crate) fn lower_unlabeled_break(&mut self) -> Stmt {
        self.lower_unlabeled_jump(false)
    }

    /// Lower an unlabeled `continue`.
    pub(crate) fn lower_unlabeled_continue(&mut self) -> Stmt {
        self.lower_unlabeled_jump(true)
    }

    fn lower_unlabeled_jump(&mut self, is_continue: bool) -> Stmt {
        let mut crosses_labeled_block = false;
        for frame in self.jump_frames.iter_mut().rev() {
            match frame.kind {
                JumpFrameKind::LabeledBlock => crosses_labeled_block = true,
                // A switch never captures `continue`; codegen already scans
                // past switch frames for an unlabeled one.
                JumpFrameKind::Switch if is_continue => {}
                JumpFrameKind::Loop | JumpFrameKind::Switch => {
                    if !crosses_labeled_block {
                        break;
                    }
                    frame.used = true;
                    let label = frame.label.clone();
                    return if is_continue {
                        Stmt::LabeledContinue(label)
                    } else {
                        Stmt::LabeledBreak(label)
                    };
                }
            }
        }
        if is_continue {
            Stmt::Continue
        } else {
            Stmt::Break
        }
    }
}

/// Attach `label` to the loop/switch an AST loop or switch statement lowered
/// to. Shared by user labels and the synthetic labels above. The target is
/// the last statement the lowering produced; anything before it is a prelude
/// (hoisted bindings of a for-of desugar, a reflected `for (var ...)` init).
/// A for-of whose iterator must be closed lowers to `try { ...; loop } catch
/// .. finally { close }`: the label belongs on the loop inside the `try`, so
/// that `break label`/`continue label` stay inside it and the close runs
/// exactly when the loop is left (labeling the `try` made the finally-inline
/// pass treat `continue label` as leaving the `try`, which closed the
/// iterator early and then skipped the close on the real exit).
pub(crate) fn label_lowered_loop(stmts: &mut [Stmt], label: String) {
    let Some(last) = stmts.last_mut() else {
        return;
    };
    if let Stmt::Try { body, .. } = last {
        if matches!(
            body.last(),
            Some(
                Stmt::For { .. }
                    | Stmt::While { .. }
                    | Stmt::DoWhile { .. }
                    | Stmt::Labeled { .. }
                    | Stmt::Try { .. }
            )
        ) {
            label_lowered_loop(body, label);
            return;
        }
    }
    let body = std::mem::replace(last, Stmt::Break);
    *last = Stmt::Labeled {
        label,
        body: Box::new(body),
    };
}
