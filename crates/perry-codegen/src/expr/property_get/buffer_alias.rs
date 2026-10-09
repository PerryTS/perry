//! Invalidate inline typed-array storage proofs when a read exposes a buffer.

use perry_hir::Expr;

use super::FnCtx;

pub(super) fn invalidate_buffer_alias_for_read(ctx: &mut FnCtx<'_>, object: &Expr, property: &str) {
    // #7219: reading `.buffer` on a tracked typed-array view HANDS OUT ITS
    // STORAGE, so the local's inline-storage proof stops holding from here on.
    //
    // `js_typed_array_backing_buffer` materializes a backing `ArrayBuffer` for
    // a typed array that owned its bytes and rebinds the array to alias it —
    // element 0 no longer follows the header. The proven-view tiers
    // (`proven_view_access`, `buffer_access`, `range_facts`, `i32_fast_path`)
    // all read `header + 16 + idx*width` directly, so after
    //
    //     const words = new Uint32Array(1);      // storage_inline_proven
    //     const bytes = new Uint8Array(words.buffer);
    //     words[0] = 0x01020304;                 // <- wrote the ORPHANED bytes
    //
    // the write landed in the pre-materialization storage while `bytes` read
    // the buffer, and neither direction aliased: the repro summed 0 instead of
    // 10, and writing through `bytes` was equally invisible to `words`.
    //
    // The runtime side already guards its own inline reader with the typed
    // array's storage byte, which `register_view_meta` sets. These tiers are
    // the compile-time proof that skips that check entirely, so the hazard has
    // to be recorded where the alias is created rather than where it is used.
    // `MutableAlias` is exactly what this is.
    if property == "buffer" {
        if let Expr::LocalGet(id) = object {
            if ctx.receiver_descriptors.contains_buffer_view(id) {
                super::super::invalidate_buffer_view_pointer(
                    ctx,
                    *id,
                    crate::native_value::MaterializationReason::MutableAlias,
                );
            }
        }
    }
}
