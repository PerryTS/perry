//! EH scopes that carry unwind actions (ehfree design §4.1).
//!
//! A landing pad restores the runtime's managed stacks by frame marks
//! (`js_eh_pad_enter`, perry-runtime `exception/marks.rs`): it pops every
//! entry a frame BELOW the catching frame pushed. Entries the catching frame
//! pushed itself carry its own mark and survive its pad — exactly right for a
//! bracket the frame opened before the `try`, which must stay open. A bracket
//! the frame opens INSIDE one of its own protected regions must instead be
//! closed when that region's handler runs, because the code that would close
//! it normally is skipped by the throw.
//!
//! Such a bracket pushes an *action scope* for its extent: every call it emits
//! unwinds to the scope's own landing pad, which restores the managed stacks
//! (`js_eh_pad_enter`), closes the bracket (its `Close` action, typically the
//! very truncate/pop its normal exit runs, keyed by the SSA value its push
//! returned), and then continues where the enclosing scope's pad would have
//! gone (`EhScope::after_pad`: the catch entry, or the next outer action
//! block). A pad never `resume`s.
//!
//! An action scope is pushed only when a handler of the same function
//! encloses the bracket. Without one, the unwind leaves the frame and the
//! catching frame's pad pops the bracket's entry by its mark.
//!
//! WASI keeps setjmp transport: its handler has no pad to chain to, and the
//! runtime restores the stacks compiled code pushes in its own frame by the
//! depths the handler recorded (perry-runtime `exception.rs`), so no action
//! scope is pushed there.

use crate::block::EhScope;
use crate::expr::FnCtx;

/// An open action scope. Must be closed with [`EhActionScope::pop`] at the end
/// of the bracket's extent, in LIFO order with every other EH scope.
#[must_use = "an action scope must be popped at the end of its bracket"]
#[derive(Debug)]
pub(crate) struct EhActionScope {
    depth: Option<usize>,
}

impl EhActionScope {
    pub(crate) fn none() -> Self {
        Self { depth: None }
    }

    pub(crate) fn pop(self, ctx: &mut FnCtx<'_>) {
        let Some(depth) = self.depth else {
            return;
        };
        assert_eq!(
            ctx.func.eh_scope_depth(),
            depth,
            "EH action scope popped out of order"
        );
        let scope = ctx
            .func
            .pop_eh_scope()
            .expect("action scope is on the stack");
        debug_assert!(scope.is_action, "popped a handler scope as an action scope");
    }
}

/// Open an action scope whose pad runs `close` (emitted into the action
/// block) and then continues at the enclosing scope's `after_pad`. A no-op
/// (nothing pushed) when no chainable handler of this function encloses the
/// point. `close` may use any SSA value defined before this call: every edge
/// into the action block comes from a call emitted after it.
pub(crate) fn push_close(
    ctx: &mut FnCtx<'_>,
    what: &'static str,
    close: impl FnOnce(&mut crate::block::LlBlock),
) -> EhActionScope {
    let Some(outer) = ctx.func.innermost_eh_scope() else {
        return EhActionScope::none();
    };
    let Some(after) = outer.after_pad.clone() else {
        return EhActionScope::none();
    };
    // A chainable outer scope is a native handler (or an action scope inside
    // one), so it carries the activation's handle height.
    let handle_height = outer
        .handle_height
        .clone()
        .expect("a chainable EH scope carries the handle height");
    let pad_idx = ctx.new_block(&format!("{what}.eh.pad"));
    let act_idx = ctx.new_block(&format!("{what}.eh.close"));
    let pad_label = ctx.block_label(pad_idx);
    let act_label = ctx.block_label(act_idx);
    let saved = ctx.current_block;
    ctx.current_block = pad_idx;
    {
        let blk = ctx.block();
        let lp = blk.next_reg();
        blk.emit_raw(format!("{lp} = landingpad {{ ptr, i32 }} catch ptr null"));
        crate::stmt::try_stmt::emit_pad_enter(blk, &handle_height);
        blk.br(&act_label);
    }
    ctx.current_block = act_idx;
    {
        let blk = ctx.block();
        close(blk);
        blk.br(&after);
    }
    ctx.current_block = saved;
    ctx.func.push_eh_scope_entry(EhScope {
        pad: pad_label,
        after_pad: Some(act_label),
        is_action: true,
        handle_height: Some(handle_height),
    });
    EhActionScope {
        depth: Some(ctx.func.eh_scope_depth()),
    }
}
