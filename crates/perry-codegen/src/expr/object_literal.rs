//! Object-literal lowering (extracted from `expr.rs`, issue #1098).
//! Pure move — no logic changes.

use anyhow::Result;
use perry_hir::Expr;

use super::{lower_expr, nanbox_pointer_inline, FnCtx};
use crate::nanbox::POINTER_MASK_I64;
use crate::type_analysis::compute_auto_captures;
use crate::types::{DOUBLE, I32, I64, PTR};

/// Lower an object literal `{ k1: v1, k2: v2, … }`.
///
/// Pattern:
/// ```llvm
/// %obj = call i64 @js_object_alloc(i32 0, i32 N)   ; class_id=0, field_count=N
/// ; for each (key, value):
/// %k_box = load double, ptr @.str.K.handle           ; interned key
/// %k_bits = bitcast double %k_box to i64
/// %k_handle = and i64 %k_bits, 281474976710655        ; POINTER_MASK_I64
/// %v = <lower value expression>                       ; double
/// call void @js_object_set_field_by_name(i64 %obj, i64 %k_handle, double %v)
/// %boxed = call double @js_nanbox_pointer(i64 %obj)
/// ```
///
/// Field names are interned via the StringPool, so the same key across
/// multiple object literals shares one global string allocation.
/// `class_id=0` is the anonymous-object class. The runtime allocates at
/// least 8 inline field slots regardless of `field_count` to prevent
/// buffer overflow on later set_field calls
/// (see `crates/perry-runtime/src/object.rs:500`).
pub(crate) fn lower_object_literal(
    ctx: &mut FnCtx<'_>,
    props: &[(String, Expr)],
) -> Result<String> {
    let field_count = props.len() as u32;
    let zero_str = "0".to_string();
    let n_str = field_count.to_string();

    // Fast path: no closure-with-`this` props. Use the shape-cache allocator
    // and write fields by INDEX — this skips the per-field linear key-search
    // done by `js_object_set_field_by_name`. Cuts ~10ns per field on the hot
    // path (and saves the keys_array realloc when `getDetailedIdType`-style
    // returns are evaluated 10k×/round). Closure-with-`this` props still
    // need the by-name path because `this_patches` populates them post-build
    // via `js_closure_set_capture_f64`, which assumes the key is already in
    // keys_array — fine here since the shape allocator pre-populates it.
    let any_method_closure = props.iter().any(|(_, v)| {
        matches!(
            v,
            Expr::Closure {
                captures_this: true,
                ..
            }
        )
    });

    if !any_method_closure && field_count > 0 {
        // Build packed keys "k1\0k2\0…" interned in the StringPool (shared
        // across all literals with the same key set + order).
        let mut packed_keys = String::new();
        for (k, _) in props {
            packed_keys.push_str(k);
            packed_keys.push('\0');
        }
        let keys_idx = ctx.strings.intern(&packed_keys);
        let keys_entry = ctx.strings.entry(keys_idx);
        let keys_global = format!("@{}", keys_entry.bytes_global);
        let keys_len_str = keys_entry.byte_len.to_string();

        // Stable shape_id derived from the packed-keys bytes. SHAPE_INLINE_CACHE
        // is a 256-slot direct-mapped array; collisions fall through to the
        // overflow HashMap, so any deterministic non-zero u32 works. FNV-1a
        // is fast, dependency-free, and well-distributed across small inputs.
        let mut shape_id: u32 = 0x811c9dc5;
        for b in packed_keys.as_bytes() {
            shape_id ^= *b as u32;
            shape_id = shape_id.wrapping_mul(0x01000193);
        }
        if shape_id == 0 {
            // shape_cache_get treats shape_id == 0 as "empty slot"; bump to 1.
            shape_id = 1;
        }
        let shape_id_str = shape_id.to_string();

        let obj_handle = ctx.block().call(
            I64,
            "js_object_alloc_with_shape",
            &[
                (I32, &shape_id_str),
                (I32, &n_str),
                (PTR, &keys_global),
                (I32, &keys_len_str),
            ],
        );

        for (i, (_, value_expr)) in props.iter().enumerate() {
            let v = lower_expr(ctx, value_expr)?;
            let idx_str = i.to_string();
            // Issue #448: the runtime `js_object_set_field` takes its
            // value as `JSValue` (`#[repr(transparent)] u64`), which the
            // System V / AArch64 / Win64 ABIs all pass in a *general*-
            // purpose register. The lowered NaN-box `v` is a `double`,
            // which the same ABIs pass in a *floating-point* register.
            // Without the bitcast the call sent the value in xmm0 / d0
            // while Rust read garbage from rdx / x2, so generator iter
            // objects (`{next, return, throw}` literals built via the
            // shape-cache fast path) read back closure-typed fields as
            // `0` — and the resulting `__iter.next()` dispatch never
            // returned a real iter-result, so `for…of` over a class
            // implementing `*[Symbol.iterator]()` hung forever
            // allocating empty results.
            let blk = ctx.block();
            let v_bits = blk.bitcast_double_to_i64(&v);
            blk.call_void(
                "js_object_set_field",
                &[(I64, &obj_handle), (I32, &idx_str), (I64, &v_bits)],
            );
        }
        return Ok(nanbox_pointer_inline(ctx.block(), &obj_handle));
    }

    let obj_handle = ctx
        .block()
        .call(I64, "js_object_alloc", &[(I32, &zero_str), (I32, &n_str)]);

    // Track `(closure_value_double, reserved_this_slot_idx)` for each
    // method closure that needs `this` patched after the object is
    // fully built. Enables `calc.add(n) { this.value = ... }`.
    let mut this_patches: Vec<(String, u32)> = Vec::new();

    for (key, value_expr) in props {
        let key_idx = ctx.strings.intern(key);
        let key_handle_global = format!("@{}", ctx.strings.entry(key_idx).handle_global);

        if let Expr::Closure {
            params: cparams,
            body: cbody,
            captures: ccaps,
            captures_this: true,
            ..
        } = value_expr
        {
            let auto_caps = compute_auto_captures(ctx, cparams, cbody, ccaps);
            let this_idx = auto_caps.len() as u32;

            let v = lower_expr(ctx, value_expr)?;
            this_patches.push((v.clone(), this_idx));

            let blk = ctx.block();
            let key_box = blk.load(DOUBLE, &key_handle_global);
            let key_bits = blk.bitcast_double_to_i64(&key_box);
            let key_raw = blk.and(I64, &key_bits, POINTER_MASK_I64);
            blk.call_void(
                "js_object_set_field_by_name",
                &[(I64, &obj_handle), (I64, &key_raw), (DOUBLE, &v)],
            );
            continue;
        }

        let v = lower_expr(ctx, value_expr)?;
        let blk = ctx.block();
        let key_box = blk.load(DOUBLE, &key_handle_global);
        let key_bits = blk.bitcast_double_to_i64(&key_box);
        let key_raw = blk.and(I64, &key_bits, POINTER_MASK_I64);
        blk.call_void(
            "js_object_set_field_by_name",
            &[(I64, &obj_handle), (I64, &key_raw), (DOUBLE, &v)],
        );
    }

    // Patch each method closure's reserved `this` slot with the object
    // pointer (NaN-boxed). Done AFTER all fields are set so every
    // method sees the fully-initialized object.
    if !this_patches.is_empty() {
        let blk = ctx.block();
        let obj_tagged = {
            let tagged = blk.or(I64, &obj_handle, crate::nanbox::POINTER_TAG_I64);
            blk.bitcast_i64_to_double(&tagged)
        };
        for (closure_val, this_idx) in &this_patches {
            let bits = blk.bitcast_double_to_i64(closure_val);
            let closure_handle = blk.and(I64, &bits, POINTER_MASK_I64);
            let idx_str = this_idx.to_string();
            blk.call_void(
                "js_closure_set_capture_f64",
                &[
                    (I64, &closure_handle),
                    (I32, &idx_str),
                    (DOUBLE, &obj_tagged),
                ],
            );
        }
    }

    Ok(nanbox_pointer_inline(ctx.block(), &obj_handle))
}
