//! #10464: hand a frame's variable-box cells back to the runtime when the
//! frame can no longer name them.
//!
//! A boxed local's entry alloca holds the cell this frame minted (or its
//! TAG_UNDEFINED entry sentinel). Two points end the frame's hold on that
//! cell: every `ret` (registered here, emitted by the `LlFunction` return-site
//! rewrite so no return path can be missed), and a declaration inside a loop
//! minting the next iteration's cell into the same slot. The runtime
//! (`perry_runtime::r#box::scope_release`) publishes a cell nothing else
//! captured and leaves a closure-captured cell to its closures' GC death.
//!
//! Only holders the runtime counts may keep a released cell alive. The two
//! compiler-emitted holders it does not count withdraw the slot instead:
//! a mapped sloppy-mode `arguments` object (`codegen/arguments.rs`) and a
//! plain-async step closure's own activation cells (`expr/closure.rs`).

use crate::expr::FnCtx;
use crate::types::{LlvmType, I64};

pub(crate) const JS_BOX_SCOPE_RELEASE: &str = "js_box_scope_release";
pub(crate) const I32_BOX_SCOPE_RELEASE: &str = "js_i32_box_scope_release";
pub(crate) const BOOL_BOX_SCOPE_RELEASE: &str = "js_bool_box_scope_release";

/// Release `slot`'s cell before every `ret` of the current function.
pub(crate) fn release_at_frame_exit(ctx: &mut FnCtx<'_>, slot: &str, release_fn: &'static str) {
    ctx.func.add_pre_return_box_release(slot, release_fn);
}

/// A declaration about to mint a fresh cell into `slot` re-executes only in a
/// loop; the previous iteration's cell is then unnameable by this frame.
/// Outside a loop the slot still holds its entry sentinel, so nothing is
/// emitted. `switch` frames push an empty continue label and do not count.
pub(crate) fn release_previous_iteration_cell(
    ctx: &mut FnCtx<'_>,
    slot: &str,
    release_fn: &'static str,
) {
    let in_loop = ctx
        .loop_targets
        .iter()
        .any(|(continue_label, _, _)| !continue_label.is_empty());
    if !in_loop {
        return;
    }
    let previous = ctx.block().load(I64, slot);
    ctx.block().call_void(release_fn, &[(I64, &previous)]);
}

/// Store a freshly minted cell (`alloc_fn(args)`) into this frame's entry
/// `slot`: release the previous iteration's cell first (so an uncaptured one
/// is reused by this very allocation), and release the slot at frame exit.
/// Returns the new cell pointer.
pub(crate) fn mint_frame_cell(
    ctx: &mut FnCtx<'_>,
    slot: &str,
    alloc_fn: &str,
    args: &[(LlvmType, &str)],
    release_fn: &'static str,
) -> String {
    release_previous_iteration_cell(ctx, slot, release_fn);
    let cell = ctx.block().call(I64, alloc_fn, args);
    ctx.block().store(I64, &cell, slot);
    release_at_frame_exit(ctx, slot, release_fn);
    cell
}
