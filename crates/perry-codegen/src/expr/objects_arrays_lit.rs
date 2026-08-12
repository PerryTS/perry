//! Object/Array literals + spread.
//!
//! Extracted from `expr/mod.rs` to keep that file under the 2000-line cap.
//! Pure mechanical move — match arm bodies are verbatim copies, called from
//! `lower_expr`'s outer dispatch.
//!
//! # Layer 1 migrated module (#7615, slice 3)
//!
//! Nothing here names `expr::temp_root`. The spread-literal accumulator — the
//! canonical shape [`crate::rooting::with_rooted_accumulator`] exists for —
//! goes through that combinator; `crate::rooting::migration_ledger` fails the
//! build if this module reaches back into the raw API.
//!
//! The other three arms delegate ([`lower_object_literal`],
//! [`lower_array_literal`]) or lower a single operand that the very next
//! instruction consumes, which is the template module's rule (`expr/url_main.rs`,
//! #7617): with nothing lowered after the operand there is no window, and
//! `operand_protection` would answer `Reuse`.
//!
//! ## What the migration found here
//!
//! Nothing new. #7280 rooted this accumulator by hand and did it correctly —
//! push before the first element, re-read before every append, **republish the
//! append's return value** (the address legitimately changes), read once below
//! the last append and release above nothing. The migration is therefore a
//! *translation*, and its IR is byte-identical; claiming it as a closed hazard
//! would be the overclaim slices 1a/1b warned about. What it buys is that the
//! republish can no longer be forgotten (`advance` fuses it to the call) and
//! that the release cannot become branch-conditional — #7462's shape, which
//! shipped in a sibling arm.

use anyhow::Result;
use perry_hir::Expr;

use crate::rooting::{self, Arg, Repr};
use crate::types::{DOUBLE, I32, I64};

use super::{lower_array_literal, lower_expr, lower_object_literal, nanbox_pointer_inline, FnCtx};

pub(crate) fn lower(ctx: &mut FnCtx<'_>, expr: &Expr) -> Result<String> {
    match expr {
        Expr::Object(props) => lower_object_literal(ctx, props, None),

        // -------- Arrays (Phase B.3) --------
        // `[a, b, c]` literal: allocate via js_array_alloc(N), then
        // sequentially push each element. js_array_push_f64 may return a
        // new pointer if it had to realloc, so we thread the pointer
        // through each push. Final pointer is NaN-boxed via js_nanbox_pointer
        // (POINTER_TAG, not STRING_TAG).
        Expr::Array(elements) => lower_array_literal(ctx, elements),

        // `[a, ...b, c]` literal with spread elements. Spread operands go
        // through the runtime iterator materializer so `GetIterator` errors
        // and iterator value/getter order match JavaScript semantics.
        Expr::ArraySpread(elements) => {
            use perry_hir::ArrayElement;
            if let [ArrayElement::Spread(e)] = elements.as_slice() {
                let src_box = lower_expr(ctx, e)?;
                let cloned =
                    ctx.block()
                        .call(I64, "js_array_clone_for_spread", &[(DOUBLE, &src_box)]);
                return Ok(nanbox_pointer_inline(ctx.block(), &cloned));
            }
            // #7280: unlike `lower_array_literal` — which lowers every element
            // FIRST (each into a temp root) and only then allocates — this path
            // allocates the accumulator UP FRONT and lowers the elements into
            // it. The half-built array is therefore live across every element
            // expression, and for a spread literal those are arbitrary user
            // code: `[a, ...gen(), b]` runs an iterator protocol between two
            // pushes. It is live across the lowering's OWN calls too —
            // `js_array_push_f64`, `js_array_push_hole` and
            // `js_array_spread_append` all allocate.
            //
            // Threading `current_arr` through each call's RETURN value already
            // handles REALLOCATION (`js_array_push_f64` hands back a new
            // pointer when it grows). It does nothing for RELOCATION: nothing
            // rooted the accumulator, so a minor between two elements finds an
            // array reachable from no root at all — it is reclaimed, not merely
            // moved, and the remaining appends write into recycled memory.
            // 29 of the 77 fatal moving stale uses on the #7280 reproducer are
            // this shape.
            //
            // `RootedAcc::advance` rather than a fixed slot that is only
            // re-read: the accumulator's address legitimately CHANGES on every
            // append, so the slot must be rewritten, not just re-read. Same
            // contract as the string-concat accumulator (#6971), and the reason
            // `advance` exists — it fuses "call the helper" to "publish what it
            // handed back", so the republish is not a statement a later edit can
            // drop.
            let cap_str = (elements.len() as u32).to_string();
            let current_arr = ctx.block().call(I64, "js_array_alloc", &[(I32, &cap_str)]);
            // `protect` is STATED, not derived from the elements. Two of the
            // three appends can re-enter user code on their own —
            // `js_array_spread_append` runs the iterator protocol, and a
            // spread literal always has at least one — so deriving the window
            // from the element expressions would answer *false* for
            // `[...a, ...b]` over two plain locals and drop the root at the one
            // site that provably needs it. `any_operand_may_collect` is the
            // right predicate for an operand group, not for a window whose
            // collection points are the lowering's own emitted calls
            // (`with_operands_rooted_across_call` makes the same argument).
            rooting::with_rooted_accumulator(
                ctx,
                Repr::Ptr,
                &current_arr,
                true,
                |ctx, acc| {
                    for elem in elements {
                        match elem {
                            ArrayElement::Expr(e) => {
                                let v = lower_expr(ctx, e)?;
                                acc.advance(ctx, "js_array_push_f64", &[Arg::Plain(DOUBLE, &v)]);
                            }
                            ArrayElement::Hole => {
                                acc.advance(ctx, "js_array_push_hole", &[]);
                            }
                            ArrayElement::Spread(e) => {
                                let src_box = lower_expr(ctx, e)?;
                                acc.advance(
                                    ctx,
                                    "js_array_spread_append",
                                    &[Arg::Plain(DOUBLE, &src_box)],
                                );
                            }
                        }
                    }
                    Ok(())
                },
                |ctx, current_arr| Ok(nanbox_pointer_inline(ctx.block(), current_arr)),
            )
        }

        // `arr[i]` index access. INLINE FAST PATH for typed-Number arrays:
        // skip the runtime function call, do the address arithmetic
        // directly. The ArrayHeader layout is `{ length: u32, capacity:
        // u32, elements: [f64; N] }` — elements start at offset 8.
        //
        // Equivalent to:
        //   element_ptr = arr_ptr + 8 + idx*8
        //   load double, ptr element_ptr
        //
        // Saves a function call (~5-10 ns) per access. For
        // bench_array_ops with ~400K reads per iteration this is a
        // major performance win.
        _ => unreachable!("expr/mod.rs dispatched a variant not handled by this submodule"),
    }
}
