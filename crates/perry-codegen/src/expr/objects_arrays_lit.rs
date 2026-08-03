//! Object/Array literals + spread.
//!
//! Extracted from `expr/mod.rs` to keep that file under the 2000-line cap.
//! Pure mechanical move — match arm bodies are verbatim copies, called from
//! `lower_expr`'s outer dispatch.

use anyhow::Result;
use perry_hir::Expr;

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
            // `temp_root_set_i64` rather than a fixed `RootedHandle`: the
            // accumulator's address legitimately CHANGES on every append, so
            // the slot must be rewritten, not just re-read. Same contract as
            // the string-concat accumulator (#6971).
            let cap_str = (elements.len() as u32).to_string();
            let mut current_arr = ctx.block().call(I64, "js_array_alloc", &[(I32, &cap_str)]);
            let root = super::temp_root::temp_root_push_i64(ctx, &current_arr);
            for elem in elements {
                match elem {
                    ArrayElement::Expr(e) => {
                        let v = lower_expr(ctx, e)?;
                        let arr = super::temp_root::temp_root_get_i64(ctx, &root);
                        current_arr = ctx.block().call(
                            I64,
                            "js_array_push_f64",
                            &[(I64, &arr), (DOUBLE, &v)],
                        );
                    }
                    ArrayElement::Hole => {
                        let arr = super::temp_root::temp_root_get_i64(ctx, &root);
                        current_arr = ctx.block().call(I64, "js_array_push_hole", &[(I64, &arr)]);
                    }
                    ArrayElement::Spread(e) => {
                        let src_box = lower_expr(ctx, e)?;
                        let arr = super::temp_root::temp_root_get_i64(ctx, &root);
                        current_arr = ctx.block().call(
                            I64,
                            "js_array_spread_append",
                            &[(I64, &arr), (DOUBLE, &src_box)],
                        );
                    }
                }
                // The append may have grown the array (a new address) and may
                // have run a collection that moved it again. The returned
                // pointer is the live one; republish it before the next
                // element's lowering can collect.
                super::temp_root::temp_root_set_i64(ctx, &root, &current_arr);
            }
            let current_arr = super::temp_root::temp_root_get_i64(ctx, &root);
            let boxed = nanbox_pointer_inline(ctx.block(), &current_arr);
            super::temp_root::temp_root_truncate(ctx, &root);
            Ok(boxed)
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
