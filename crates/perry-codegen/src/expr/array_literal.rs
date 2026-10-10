//! Array-literal lowering (extracted from `expr.rs`, issue #1098).
//!
//! # Layer 1 migrated module (#7615, slice 3)
//!
//! Nothing here names `expr::temp_root`. The element group goes through
//! [`crate::rooting::with_rooted_group`], and
//! `crate::rooting::migration_ledger` fails the build if this module reaches
//! back into the raw API.
//!
//! The group protects elements across later evaluations and the allocation,
//! then re-reads them below the collecting slow arm. It owns release across
//! outlined, inline and large-array exits, including lowering errors.

use anyhow::Result;
use perry_hir::Expr;

use super::{
    emit_jsvalue_slot_store_on_block, expr_produces_non_pointer_bits_by_construction,
    load_inline_arena_state, nanbox_pointer_inline, FnCtx,
};
use crate::rooting;
use crate::type_analysis::is_numeric_expr;
use crate::types::{DOUBLE, I1, I32, I64, I8, PTR};

/// Lower an array literal `[a, b, c, …]`.
///
/// Fast path: element expressions are lowered first (any allocations
/// inside elements complete before we claim the arena bump slot for the
/// outer array), then for small literals (≤ 16 elements) we emit inline
/// bump-allocator IR — the same pattern `new ClassName()` uses when
/// `class_keys_globals` is populated. No extern call on the hot path:
/// a load of the per-function arena state, a bump-pointer check, one i64
/// store for the packed GcHeader, one i64 store for the packed ArrayHeader
/// (length and capacity share the same 8 bytes), and N `store double, ptr`
/// for the elements. The slow path (block overflow) calls
/// `js_inline_arena_slow_alloc`.
///
/// For N > 16 we fall back to the extern `js_array_alloc_literal` — the
/// inline path emits per-literal IR that's cheap at small N but grows with
/// each element store, so large literals benefit more from a compact call.
///
/// GC safety: the array header is written after the bump commits, so any
/// GC observing the partially-written arena block sees either a not-yet-
/// allocated slot (offset hasn't advanced past the `fits` check) or a
/// header with `length == capacity` and uninitialized elements. No
/// allocator call runs between the header write and the element stores,
/// so GC can't run in that window.
///
/// #6951: element values themselves are a different matter. They are lowered
/// before the allocation and each one then sits in an SSA register across
/// every later element's evaluation — which is not a root, and was only ever
/// covered by conservative native-stack scanning. `[freshString(), f()]` lost
/// its first element as soon as `f` collected. The rooted group now also
/// protects the last element across the array's own allocation, then reads
/// every element again before initialization.
pub(crate) fn lower_array_literal(ctx: &mut FnCtx<'_>, elements: &[Expr]) -> Result<String> {
    let n = elements.len();
    let all_numeric_elements = elements.iter().all(|e| is_numeric_expr(ctx, e));

    // #8583 follow-up: a LARGE, fully-CONSTANT array literal (the minified
    // data-table shape — a giant nested array of number/bool/null literals)
    // becomes a static rodata descriptor + ONE bulk-materialization runtime
    // call, instead of the N per-subarray `js_array_from_values` allocations and
    // the giant procedural body that made `__33499` fan out under RS4GC (245k
    // instrs / 11,104 allocations → one call over a compact blob). Small const
    // arrays fall through to the fast inline bump-alloc path below.
    if const_array_descriptor_enabled() {
        if let Some(v) = try_lower_const_array_descriptor(ctx, elements) {
            return Ok(v);
        }
    }

    // Evaluate all element expressions *before* allocating, so nested
    // allocations inside element expressions don't see a half-initialized
    // outer array. Pointer-bearing values remain rooted through the array's
    // allocation and are re-read before initialization (#6951).
    let canonical_raw_f64: Vec<bool> = elements
        .iter()
        .map(|e| crate::type_analysis::expr_produces_canonical_raw_f64(ctx, e))
        .collect();
    let mut layout_notes_needed = Vec::with_capacity(n);
    for value_expr in elements {
        layout_notes_needed.push(!expr_produces_non_pointer_bits_by_construction(
            ctx, value_expr,
        ));
    }
    rooting::with_rooted_group(ctx, n, |ctx, group| {
        for element in elements {
            group.lower(ctx, element, true)?;
        }
        let arr = emit_array_from_lowered_values(
            ctx,
            n,
            &canonical_raw_f64,
            &layout_notes_needed,
            all_numeric_elements,
            |ctx| group.reread_all(ctx),
        )?;
        Ok(nanbox_pointer_inline(ctx.block(), &arr))
    })
}

#[cfg(test)]
mod empty_birth_tests {
    use perry_hir::{types::Type, Expr, Function, Module, Stmt};

    #[test]
    fn empty_literal_uses_the_bump_birth_with_initialized_append_reserve() {
        let mut hir = Module::new("empty_birth");
        hir.functions.push(Function {
            id: 0,
            name: "empty".into(),
            type_params: Vec::new(),
            params: Vec::new(),
            return_type: Type::Any,
            body: vec![Stmt::Return(Some(Expr::Array(Vec::new())))],
            is_async: false,
            is_generator: false,
            is_strict: true,
            is_exported: false,
            captures: Vec::new(),
            decorators: Vec::new(),
            was_plain_async: false,
            was_unrolled: false,
        });
        let ir = String::from_utf8(
            crate::compile_module(
                &hir,
                crate::CompileOptions {
                    emit_ir_only: true,
                    ..Default::default()
                },
            )
            .unwrap(),
        )
        .unwrap();
        assert!(
            ir.contains("arrlit.fast"),
            "the birth must actually be inline: {ir}"
        );
        assert!(
            !ir.contains("call i64 @js_array_alloc("),
            "outlined birth: {ir}"
        );
        assert!(ir.contains("call ptr @js_inline_arena_slow_alloc("));
        assert!(
            ir.contains("store i64 17179869184,"),
            "length 0, capacity 4: {ir}"
        );
        assert_eq!(
            ir.matches(&format!("store i64 {},", crate::nanbox::TAG_HOLE))
                .count(),
            4,
            "every unused slot must be initialized: {ir}"
        );
        assert!(
            ir.contains("load volatile i8"),
            "birth color must stay live: {ir}"
        );
    }
}

/// Element count up to which an array is built inline (bump allocation plus N
/// stores) rather than through `js_array_alloc`. Matches `MAX_SCALAR_ARRAY_LEN`
/// in collectors.rs so every candidate the escape pass rejects still benefits.
pub(crate) const INLINE_ARRAY_MAX_ELEMENTS: usize = 16;

/// Build an array from elements the caller has already lowered and rooted,
/// returning the raw `i64` user pointer. `reread` must recover their current
/// values using GC-leaf root reads; it runs after the collecting allocation
/// and before initialization. The outlined helper consumes them in its call.
///
/// Split out of [`lower_array_literal`] so the rest/`arguments` bundle at a
/// call site builds its array the same way a literal does — one inline bump
/// allocation and N stores — instead of `js_array_alloc` plus one
/// `js_array_push_f64` per element, where every push re-classifies the
/// receiver, re-notes the slot layout and re-checks the barrier (#7154's
/// accumulator shape keeps the rooting, only the construction changes).
///
/// `canonical_raw_f64[i]` says element `i` is a plain double by construction,
/// `layout_notes_needed[i]` that it may carry a heap pointer, and
/// `all_numeric_elements` that every element is statically a number. The
/// caller owns rooting: the slow arm of the bump allocator collects, so every
/// pointer value must already live in a root the group re-reads.
pub(crate) fn emit_array_from_lowered_values<'f>(
    ctx: &mut FnCtx<'f>,
    n: usize,
    canonical_raw_f64: &[bool],
    layout_notes_needed: &[bool],
    all_numeric_elements: bool,
    reread: impl FnOnce(&mut FnCtx<'f>) -> Result<Vec<String>>,
) -> Result<String> {
    // #5391: oversized modules outline array-literal construction. The inline
    // bump-alloc + N×(store + layout-note + barrier) sequence makes minified
    // data-table builders huge (single 18MB functions are impractical to
    // optimize). Instead spill the already-evaluated element values to
    // a per-literal stack buffer and build the array in ONE runtime call. The
    // buffer is hoisted to the entry block (fixed size per site; bounded total
    // stack) and consumed immediately by the call, so no GC-visible window.
    if crate::codegen::full_outline_ic_enabled() {
        let vals = reread(ctx)?;
        let buf = ctx.func.alloca_entry_array(DOUBLE, n);
        for (i, v) in vals.iter().enumerate() {
            let slot = ctx.block().gep(DOUBLE, &buf, &[(I64, &i.to_string())]);
            ctx.block().store(DOUBLE, v, &slot);
        }
        let n_str = n.to_string();
        let arr = ctx
            .block()
            .call(I64, "js_array_from_values", &[(PTR, &buf), (I32, &n_str)]);
        return Ok(arr);
    }

    // Inline bump-allocator path for small literals. Size threshold matches
    // `MAX_SCALAR_ARRAY_LEN` in collectors.rs so every candidate the escape
    // pass rejects can still benefit from the inline alloc.
    // ILP32 (wasm32 WASI, #11378): the inline bump reads `InlineArenaState`
    // at LP64 offsets; take the runtime call below instead.
    if n <= INLINE_ARRAY_MAX_ELEMENTS && !crate::codegen::helpers::ilp32_target() {
        if n == 0 {
            return Ok(super::inline_birth::empty_array(ctx));
        }
        // Layout constants — must match `ArrayHeader` in array.rs and
        // `GcHeader` in gc.rs. Duplicated here because codegen emits raw
        // byte offsets; the runtime declarations are authoritative.
        const GC_HEADER_SIZE: u64 = 8;
        const ARRAY_HEADER_SIZE: u64 = 8;
        const ELEMENT_SIZE: u64 = 8;
        const GC_TYPE_ARRAY: u64 = 1;
        const GC_FLAG_ARENA: u64 = 0x02;
        // PR #1146: pointer-free hint for slot-layout tracking. The
        // element-store loop below only suppresses per-slot notes for
        // values whose non-pointer bits are proven by expression shape.
        const GC_LAYOUT_POINTER_FREE: u64 = 0x4000;

        // Empty arrays use the same birth protocol, retaining the runtime's
        // four-slot append reserve. Nonempty literals keep their exact size.
        let capacity = n;
        let total_size = GC_HEADER_SIZE + ARRAY_HEADER_SIZE + (capacity as u64) * ELEMENT_SIZE;
        let total_size_str = total_size.to_string();

        // Load state + compute bump check. `total_size` is always a
        // multiple of 8, every prior alloc rounds offset to 8, and blocks
        // start 8-aligned, so no align-up step is needed.
        let state_ptr = load_inline_arena_state(ctx);
        let blk = ctx.block();
        let offset_field_ptr = blk.gep(I8, &state_ptr, &[(I64, "8")]);
        let offset_val = blk.load(I64, &offset_field_ptr);
        let aligned_off = offset_val.clone();
        let new_offset = blk.add(I64, &aligned_off, &total_size_str);
        let size_field_ptr = blk.gep(I8, &state_ptr, &[(I64, "16")]);
        let size_val = blk.load(I64, &size_field_ptr);
        let fits = blk.icmp_ule(I64, &new_offset, &size_val);

        let fast_idx = ctx.new_block("arrlit.fast");
        let slow_idx = ctx.new_block("arrlit.slow");
        let merge_idx = ctx.new_block("arrlit.merge");
        let fast_label = ctx.block_label(fast_idx);
        let slow_label = ctx.block_label(slow_idx);
        let merge_label = ctx.block_label(merge_idx);

        ctx.block().cond_br(&fits, &fast_label, &slow_label);

        // Fast path: commit the bump, compute `data + offset`.
        ctx.current_block = fast_idx;
        let blk = ctx.block();
        // GC_STORE_AUDIT(INIT): arena bump offset is allocator metadata, not a JS heap edge.
        blk.store(I64, &new_offset, &offset_field_ptr);
        let data_ptr = blk.load(PTR, &state_ptr);
        let raw_fast = blk.gep(I8, &data_ptr, &[(I64, &aligned_off)]);
        let fast_pred_label = blk.label.clone();
        blk.br(&merge_label);

        // Slow path: call the runtime slow-alloc (same one used by the
        // inline `new` path). Returns a fresh raw pointer (inclusive of
        // GcHeader space).
        ctx.current_block = slow_idx;
        let raw_slow = ctx.block().call(
            PTR,
            "js_inline_arena_slow_alloc",
            &[(PTR, &state_ptr), (I64, &total_size_str), (I64, "8")],
        );
        let slow_pred_label = ctx.block().label.clone();
        ctx.block().br(&merge_label);

        // Merge: phi the raw pointer and write everything.
        ctx.current_block = merge_idx;
        let blk = ctx.block();
        let raw = blk.phi(
            PTR,
            &[(&raw_fast, &fast_pred_label), (&raw_slow, &slow_pred_label)],
        );
        // The slow allocation can move every element. Root homes own the
        // values until this point; their reads are GC-leaf, so initialization
        // still contains no collecting call after the raw pointer is born.
        let vals = reread(ctx)?;
        let birth_flags = super::inline_birth::flags(ctx, &state_ptr);
        let blk = ctx.block();

        // Packed GcHeader (bits 0..7 obj_type, 8..15 gc_flags, 16..31
        // _reserved, 32..63 size). PR #1146 packs the layout-tag in the
        // reserved bits so the GC sees the array as pointer-free until
        // the element-store loop overrides per-slot via
        // `js_gc_note_slot_layout` below.
        let gc_packed: u64 = GC_TYPE_ARRAY
            | (GC_FLAG_ARENA << 8)
            | (GC_LAYOUT_POINTER_FREE << 16)
            | (total_size << 32);
        // A literal whose elements are statically numbers is usually all
        // plain doubles at runtime. Then the array is born exactly as
        // `js_array_mark_numeric_f64_layout` would leave it — pointer-free
        // with the dense raw-f64 flag — so decide that with one signed
        // compare per element and skip every per-slot note and the
        // marking walk. Any NaN-boxed element (an int32 box, or a value
        // whose annotation lied) takes the unchanged noted path.
        let all_plain_numbers = if all_numeric_elements {
            let mut all_plain: Option<String> = None;
            for (i, v) in vals.iter().enumerate() {
                if canonical_raw_f64[i] {
                    continue;
                }
                let bits = blk.bitcast_double_to_i64(v);
                // 0x7FF9 << 48: the lowest NaN-box tag.
                let plain = blk.icmp_slt(I64, &bits, "9221401712017801216");
                all_plain = Some(match all_plain {
                    None => plain,
                    Some(acc) => blk.and(I1, &acc, &plain),
                });
            }
            Some(all_plain.unwrap_or_else(|| "true".to_string()))
        } else {
            None
        };
        let header_word = match &all_plain_numbers {
            Some(all_plain) => {
                // GC_ARRAY_RAW_F64_LAYOUT (0x80) in `_reserved`.
                let flagged = gc_packed | (0x80u64 << 16);
                blk.select(
                    I1,
                    all_plain,
                    I64,
                    &flagged.to_string(),
                    &gc_packed.to_string(),
                )
            }
            None => gc_packed.to_string(),
        };
        let header_word = super::inline_birth::header(ctx, &header_word, &birth_flags);
        let blk = ctx.block();
        // GC_STORE_AUDIT(INIT): live runtime birth flags are part of the fresh array header.
        blk.store(I64, &header_word, &raw);

        // Packed ArrayHeader at raw+8 (length low 32 / capacity high 32).
        let arr_header_addr = blk.gep(I8, &raw, &[(I64, "8")]);
        let arr_header_packed = (n as u64) | ((capacity as u64) << 32);
        // GC_STORE_AUDIT(INIT): freshly allocated ArrayHeader length/capacity, no child pointer.
        blk.store(I64, &arr_header_packed.to_string(), &arr_header_addr);
        for i in n..capacity {
            let slot = blk.gep_inbounds(I8, &raw, &[(I64, &(16 + i * 8).to_string())]);
            // GC_STORE_AUDIT(INIT): fresh unused array capacity holds holes.
            blk.store(I64, &crate::nanbox::TAG_HOLE.to_string(), &slot);
        }

        // User pointer = raw + GC_HEADER_SIZE. Computed before the
        // element loop so the per-slot layout notes target the correct
        // user-visible address.
        let user_ptr = blk.gep(I8, &raw, &[(I64, "8")]);
        let user_ptr_as_i64 = blk.ptrtoint(&user_ptr, I64);

        if let Some(all_plain) = all_plain_numbers {
            let plain_idx = ctx.new_block("arrlit.plain_numbers");
            let noted_idx = ctx.new_block("arrlit.noted");
            let done_idx = ctx.new_block("arrlit.done");
            let plain_label = ctx.block_label(plain_idx);
            let noted_label = ctx.block_label(noted_idx);
            let done_label = ctx.block_label(done_idx);
            ctx.block().cond_br(&all_plain, &plain_label, &noted_label);

            ctx.current_block = plain_idx;
            {
                let blk = ctx.block();
                for (i, v) in vals.iter().enumerate() {
                    let offset = (16 + i * 8).to_string();
                    let elem_ptr = blk.gep_inbounds(I8, &raw, &[(I64, &offset)]);
                    // GC_STORE_AUDIT(POINTER_FREE): every element was just
                    // tested to be a plain double; the header already says
                    // pointer-free raw-f64.
                    blk.store(DOUBLE, v, &elem_ptr);
                }
                blk.br(&done_label);
            }

            ctx.current_block = noted_idx;
            {
                let blk = ctx.block();
                for (i, v) in vals.iter().enumerate() {
                    let offset = (16 + i * 8).to_string();
                    let elem_ptr = blk.gep_inbounds(I8, &raw, &[(I64, &offset)]);
                    let slot_index = i.to_string();
                    emit_jsvalue_slot_store_on_block(
                        blk,
                        &elem_ptr,
                        v,
                        &user_ptr_as_i64,
                        &slot_index,
                        layout_notes_needed[i],
                        &user_ptr_as_i64,
                        "0",
                        false,
                    );
                }
                blk.br(&done_label);
            }
            ctx.current_block = done_idx;
            super::inline_birth::finish(ctx, &raw, &birth_flags, &state_ptr);
            if all_plain == "true" {
                // Statically canonical doubles cannot reach normalization.
                // Do not reserve a phantom temporary root in their function.
                return Ok(user_ptr_as_i64);
            }
            // The generic normalizer's receiver resolution is Reenters.
            // Keep it OUTSIDE the no-safepoint initialization window, and
            // protect/re-read the initialized, seeded array across the call.
            // Plain doubles retain their no-call/no-temporary-root hot path.
            let normalize_idx = ctx.new_block("arrlit.normalize");
            let return_idx = ctx.new_block("arrlit.return");
            let normalize_label = ctx.block_label(normalize_idx);
            let return_label = ctx.block_label(return_idx);
            let plain_pred = ctx.block().label.clone();
            ctx.block()
                .cond_br(&all_plain, &return_label, &normalize_label);
            ctx.current_block = normalize_idx;
            let normalized = rooting::with_rooted_group(ctx, 1, |ctx, group| {
                let root = group.adopt_emitted(ctx, rooting::Repr::Ptr, &user_ptr_as_i64, true);
                let array = group.reread_emitted(ctx, root);
                ctx.block()
                    .call(I32, "js_array_mark_numeric_f64_layout", &[(I64, &array)]);
                Ok(group.reread_emitted(ctx, root))
            })?;
            let normalized_pred = ctx.block().label.clone();
            ctx.block().br(&return_label);
            ctx.current_block = return_idx;
            return Ok(ctx.block().phi(
                I64,
                &[
                    (&user_ptr_as_i64, &plain_pred),
                    (&normalized, &normalized_pred),
                ],
            ));
        }

        // Elements at raw+16 + i*8.
        let blk = ctx.block();
        for (i, v) in vals.iter().enumerate() {
            let offset = (16 + i * 8).to_string();
            let elem_ptr = blk.gep_inbounds(I8, &raw, &[(I64, &offset)]);
            let slot_index = i.to_string();
            emit_jsvalue_slot_store_on_block(
                blk,
                &elem_ptr,
                v,
                &user_ptr_as_i64,
                &slot_index,
                layout_notes_needed[i],
                &user_ptr_as_i64,
                "0",
                false,
            );
        }

        super::inline_birth::finish(ctx, &raw, &birth_flags, &state_ptr);
        return Ok(user_ptr_as_i64);
    }

    // Fallback for N > INLINE_MAX_ELEMENTS: keep the extern call + N inline
    // stores. Thin-LTO already inlines this call into user IR, so the cost
    // is ~1 inlined arena bump plus some LLVM churn around the arg pack.
    let cap_str = n.to_string();
    let arr = ctx
        .block()
        .call(I64, "js_array_alloc_literal", &[(I32, &cap_str)]);
    let vals = reread(ctx)?;

    let arr_ptr = ctx.block().inttoptr(I64, &arr);
    for (i, v) in vals.iter().enumerate() {
        let offset = (8 + i * 8).to_string();
        let elem_ptr = ctx.block().gep_inbounds(I8, &arr_ptr, &[(I64, &offset)]);
        let elem_addr = if layout_notes_needed[i] {
            ctx.block().ptrtoint(&elem_ptr, I64)
        } else {
            "0".to_string()
        };
        let slot_index = i.to_string();
        emit_jsvalue_slot_store_on_block(
            ctx.block(),
            &elem_ptr,
            v,
            &arr,
            &slot_index,
            layout_notes_needed[i],
            &arr,
            &elem_addr,
            layout_notes_needed[i],
        );
    }

    if all_numeric_elements {
        ctx.block()
            .call(I32, "js_array_mark_numeric_f64_layout", &[(I64, &arr)]);
    }

    Ok(arr)
}

/// #8583 follow-up gate. Default ON; `PERRY_CONST_ARRAY_DESCRIPTOR=0/off/false`
/// reverts every large constant literal to the procedural construction path
/// (A/B bisection, and an escape hatch if a descriptor ever proves wrong).
fn const_array_descriptor_enabled() -> bool {
    !matches!(
        std::env::var("PERRY_CONST_ARRAY_DESCRIPTOR").as_deref(),
        Ok("0") | Ok("off") | Ok("false")
    )
}

/// A value the const-descriptor path can materialize with no JS evaluation:
/// number/int/bool/null/undefined, or an array recursively of the same. Strings
/// and objects decline (v2) — the whole literal then falls back to the
/// procedural path, so a mixed table is never half-materialized.
fn is_const_materializable(e: &Expr) -> bool {
    match e {
        Expr::Number(_) | Expr::Integer(_) | Expr::Bool(_) | Expr::Null | Expr::Undefined => true,
        Expr::Array(elems) => elems.iter().all(is_const_materializable),
        _ => false,
    }
}

/// Total materializable nodes (every scalar + every array), the size gate below.
fn count_const_nodes(e: &Expr) -> usize {
    match e {
        Expr::Array(elems) => 1 + elems.iter().map(count_const_nodes).sum::<usize>(),
        _ => 1,
    }
}

/// Serialize one constant value into the descriptor blob (must match the tag
/// bytes in `perry-runtime/src/array/alloc.rs::build_const_value`).
fn serialize_const_value(e: &Expr, out: &mut Vec<u8>) {
    match e {
        Expr::Number(n) => {
            out.push(0);
            out.extend_from_slice(&n.to_le_bytes());
        }
        Expr::Integer(i) => {
            out.push(0);
            out.extend_from_slice(&(*i as f64).to_le_bytes());
        }
        Expr::Bool(true) => out.push(2),
        Expr::Bool(false) => out.push(3),
        Expr::Null => out.push(4),
        Expr::Undefined => out.push(5),
        Expr::Array(elems) => {
            out.push(1);
            out.extend_from_slice(&(elems.len() as u32).to_le_bytes());
            for el in elems {
                serialize_const_value(el, out);
            }
        }
        // Guarded by `is_const_materializable`; unreachable in practice.
        _ => out.push(5),
    }
}

/// Only worth a rodata blob + runtime call for genuinely large tables; small
/// const arrays keep the fast inline bump-alloc path (no regression). `__33499`
/// has ~44k nodes; an ordinary `[1,2,3]` has 4 and never qualifies.
const CONST_DESCRIPTOR_MIN_NODES: usize = 256;

/// If `elements` is a large, fully-constant array literal, emit a static rodata
/// descriptor and a single `js_value_from_const_descriptor` call and return the
/// nanboxed value; otherwise `None` (caller falls back to the procedural path).
fn try_lower_const_array_descriptor(ctx: &mut FnCtx<'_>, elements: &[Expr]) -> Option<String> {
    if !elements.iter().all(is_const_materializable) {
        return None;
    }
    // Only NESTED constant tables benefit: the fan-out cost is the per-subarray
    // allocation (a data table lowers to thousands of `js_array_from_values`).
    // A flat constant scalar array is already a single `js_array_alloc_literal`
    // + inline stores, so keep that path — it also preserves the precise
    // per-slot write barriers a later push/store relies on.
    if !elements.iter().any(|e| matches!(e, Expr::Array(_))) {
        return None;
    }
    let total_nodes: usize = 1 + elements.iter().map(count_const_nodes).sum::<usize>();
    if total_nodes < CONST_DESCRIPTOR_MIN_NODES {
        return None;
    }

    // Serialize the outer array: tag 1 (ARRAY) + u32 count + each element.
    let mut blob: Vec<u8> = Vec::new();
    blob.push(1);
    blob.extend_from_slice(&(elements.len() as u32).to_le_bytes());
    for el in elements {
        serialize_const_value(el, &mut blob);
    }

    // Emit the blob as a module-private rodata constant (mirrors
    // `expr/strings.rs::emit_string_literal_global`; `ic_site_counter` is the
    // module-wide site identity so re-emitted bodies don't collide).
    let idx = ctx.ic_site_counter;
    ctx.ic_site_counter += 1;
    let func_part: String = ctx
        .func
        .name
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '_' })
        .collect();
    let global_name = format!("perry_const_arr_{}_{}", func_part, idx);
    let mut lit = String::with_capacity(blob.len() + 4);
    lit.push('c');
    lit.push('"');
    for &b in &blob {
        if (32..127).contains(&b) && b != b'"' && b != b'\\' {
            lit.push(b as char);
        } else {
            lit.push('\\');
            lit.push_str(&format!("{:02X}", b));
        }
    }
    lit.push('"');
    ctx.typed_parse_rodata.push(format!(
        "@{} = private unnamed_addr constant [{} x i8] {}",
        global_name,
        blob.len(),
        lit
    ));

    // ONE runtime call materializes the whole nested structure and returns the
    // nanboxed (DOUBLE) JS value directly — no per-element IR, so no fan-out.
    let global_ref = format!("@{}", global_name);
    let len_str = blob.len().to_string();
    let v = ctx.block().call(
        DOUBLE,
        "js_value_from_const_descriptor",
        &[(PTR, &global_ref), (I32, &len_str)],
    );
    Some(v)
}
