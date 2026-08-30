//! Object-literal lowering (extracted from `expr.rs`, issue #1098).
//! Pure move — no logic changes.
//!
//! # Layer 1 migrated module (#7615, slice 3)
//!
//! Nothing here names `expr::temp_root`. Both build paths root the half-built
//! object through [`crate::rooting::with_rooted_accumulator`], and so does the
//! *other* GC value this lowering holds across user code — a `this`-capturing
//! method closure's value, which the patch loop consumes below every remaining
//! initializer. `crate::rooting::migration_ledger` fails the build if this
//! module reaches back into the raw API.
//!
//! ## Why the closure values nest rather than sitting in a list
//!
//! #6951 rooted them as a flat `Vec` of raw slots and released none of them:
//! the object handle's own `temp_root_truncate` is a stack **cut**, so cutting
//! at the handle dropped every closure slot above it. That is correct, and it
//! is correct only because of an invariant stated in a comment — "keep
//! `rooted_handle_begin` ahead of this loop and the release after the patch
//! loop". Reorder those two statements and the leak is silent.
//!
//! Nesting one `with_rooted_accumulator` per such property expresses the same
//! lifetime as a scope: each value's root spans its own installing
//! `js_object_set_field_by_name` and the whole suffix of the literal that
//! follows it, the release is owned on every path out (including a `?` from a
//! later initializer, which the flat form leaked), and the value can only be
//! read in `finish` — below the last initializer, above the release.
//!
//! The install is INSIDE the scope, not above it (#8809). Rooting after it is
//! the #7192 shape: the setter can run a user setter or a Proxy trap, and the
//! closure it just installed is reachable from the object, so an evacuating
//! minor moves it and the register queued above names from-space.
//!
//! No fourth combinator: the three existing `with_operands_rooted*` forms all
//! lower their own operand list up front, which would evaluate every property
//! before storing any of them and reorder observable side effects, and
//! `with_rooted_accumulator` is exactly "a GC value held while more user code
//! is lowered".
//!
//! ## What the migration found here
//!
//! Nothing new. #6951's rooting was already complete and correctly ordered —
//! the handle is rooted immediately after its allocation, every field store
//! re-reads it, the interned key is loaded *below* the value's lowering (the
//! #7627 finding, already right in this arm), and the deferred closure values
//! are re-read before the patch loop. This is a translation, and the IR differs
//! only by instruction-order permutations with the identical opcode multiset:
//! the re-read is now fused to the emission that consumes it, so it lands after
//! the value's `bitcast` / the key's `and` instead of before.

use anyhow::Result;
use perry_hir::types::Type as HirType;
use perry_hir::Expr;

use super::{lower_expr, nanbox_pointer_inline, FnCtx};
use crate::nanbox::POINTER_MASK_I64;
use crate::rooting::{self, Arg, Repr, RootedAcc};
use crate::type_analysis::compute_auto_captures;
use crate::types::{DOUBLE, I32, I64, PTR};

fn expected_interface_property_type(
    ctx: &FnCtx<'_>,
    iface: &perry_hir::Interface,
    key: &str,
    depth: usize,
) -> Option<HirType> {
    if let Some(prop) = iface.properties.iter().find(|prop| prop.name == key) {
        return Some(prop.ty.clone());
    }
    if iface.methods.iter().any(|method| method.name == key) {
        return Some(HirType::Any);
    }
    for ext in &iface.extends {
        if let Some(ty) = expected_object_property_type(ctx, ext, key, depth + 1) {
            return Some(ty);
        }
    }
    None
}

fn expected_class_property_type(
    ctx: &FnCtx<'_>,
    class_name: &str,
    key: &str,
    depth: usize,
) -> Option<HirType> {
    if depth > 32 {
        return None;
    }
    let class = ctx.classes.get(class_name).copied()?;
    if let Some(field) = class
        .fields
        .iter()
        .find(|field| field.key_expr.is_none() && field.name == key)
    {
        return Some(field.ty.clone());
    }
    class
        .extends_name
        .as_deref()
        .and_then(|parent| expected_class_property_type(ctx, parent, key, depth + 1))
}

fn expected_object_property_type(
    ctx: &FnCtx<'_>,
    ty: &HirType,
    key: &str,
    depth: usize,
) -> Option<HirType> {
    if depth > 32 {
        return None;
    }
    match ty {
        HirType::Object(obj) => {
            if obj.index_signature.is_some() {
                return None;
            }
            obj.properties.get(key).map(|prop| prop.ty.clone())
        }
        HirType::Named(name) => {
            if let Some(alias) = ctx.type_aliases.get(name) {
                if let Some(ty) = expected_object_property_type(ctx, alias, key, depth + 1) {
                    return Some(ty);
                }
            }
            if let Some(iface) = ctx.interfaces.get(name) {
                return expected_interface_property_type(ctx, iface, key, depth + 1);
            }
            expected_class_property_type(ctx, name, key, depth + 1)
        }
        _ => None,
    }
}

fn typed_object_literal_layout(
    ctx: &FnCtx<'_>,
    props: &[(String, Expr)],
    expected_ty: Option<&HirType>,
) -> Option<crate::typed_shape::TypedShapeLayout> {
    let expected_ty = expected_ty?;
    let mut raw_f64_mask_words = Vec::new();
    let mut pointer_mask_words = Vec::new();
    for (slot, (key, _)) in props.iter().enumerate() {
        let prop_ty = expected_object_property_type(ctx, expected_ty, key, 0)?;
        if crate::typed_shape::type_is_raw_f64_candidate(&prop_ty) {
            let word = slot / 64;
            if raw_f64_mask_words.len() <= word {
                raw_f64_mask_words.resize(word + 1, 0);
            }
            raw_f64_mask_words[word] |= 1u64 << (slot % 64);
        }
        if crate::typed_shape::type_is_pointer_bearing(&prop_ty) {
            let word = slot / 64;
            if pointer_mask_words.len() <= word {
                pointer_mask_words.resize(word + 1, 0);
            }
            pointer_mask_words[word] |= 1u64 << (slot % 64);
        }
    }
    Some(crate::typed_shape::TypedShapeLayout {
        slot_count: props.len() as u32,
        raw_f64_mask_words: crate::typed_shape::trim_mask_words(raw_f64_mask_words),
        pointer_mask_words: crate::typed_shape::trim_mask_words(pointer_mask_words),
    })
}

fn emit_object_mask_global(ctx: &mut FnCtx<'_>, kind: &str, mask_words: &[u64]) -> String {
    if mask_words.is_empty() {
        return "null".to_string();
    }
    let site_id = ctx.ic_site_counter;
    ctx.ic_site_counter += 1;
    let prefix = ctx.strings.module_prefix().to_string();
    let global_name = if prefix.is_empty() {
        format!("perry_typed_obj_shape_{}_mask_{}", kind, site_id)
    } else {
        format!(
            "perry_typed_obj_shape_{}_mask_{}__{}",
            kind, prefix, site_id
        )
    };
    let words = mask_words
        .iter()
        .map(|word| format!("i64 {}", word))
        .collect::<Vec<_>>()
        .join(", ");
    ctx.typed_parse_rodata.push(format!(
        "@{} = private unnamed_addr constant [{} x i64] [{}]",
        global_name,
        mask_words.len(),
        words
    ));
    format!("@{}", global_name)
}

fn emit_object_typed_shape_init(
    ctx: &mut FnCtx<'_>,
    obj_handle: &str,
    layout: &crate::typed_shape::TypedShapeLayout,
) {
    let slot_count_str = layout.slot_count.to_string();
    let raw_mask_word_count_str = layout.raw_f64_mask_words.len().to_string();
    let pointer_mask_word_count_str = layout.pointer_mask_words.len().to_string();
    let raw_mask_ref = emit_object_mask_global(ctx, "raw_f64", &layout.raw_f64_mask_words);
    let pointer_mask_ref = emit_object_mask_global(ctx, "ptr", &layout.pointer_mask_words);
    ctx.block().call_void(
        "js_gc_init_typed_shape_layout",
        &[
            (I64, obj_handle),
            (I32, &slot_count_str),
            (PTR, &raw_mask_ref),
            (I32, &raw_mask_word_count_str),
            (PTR, &pointer_mask_ref),
            (I32, &pointer_mask_word_count_str),
        ],
    );
}

/// Materialize an interned key's raw `StringHeader` pointer.
///
/// Emitted **below** the property value's lowering, deliberately: the handle
/// global is a registered root that an evacuating cycle *rewrites*, so a load
/// taken above the value would name the pre-move address while the global did
/// not — the shape #7627 found in `with (o) { x = f() }`. Interning is
/// compile-time and stays where it was, so the string pool's numbering is
/// untouched.
fn emit_interned_key_raw(ctx: &mut FnCtx<'_>, key_handle_global: &str) -> String {
    let blk = ctx.block();
    let key_box = blk.load(DOUBLE, key_handle_global);
    let key_bits = blk.bitcast_double_to_i64(&key_box);
    blk.and(I64, &key_bits, POINTER_MASK_I64)
}

/// Lower the by-name path's properties from index `from` onward into the rooted
/// object accumulator, returning the `(closure value, reserved `this` slot)`
/// pairs the patch loop needs — innermost first, i.e. reverse source order.
///
/// Recursive because a `this`-capturing method closure's value has to stay
/// rooted for the whole *suffix* of the literal that follows it, which is one
/// `with_rooted_accumulator` scope per such property. See the module header for
/// why that is the shape rather than a list of slots.
fn lower_by_name_props<'f>(
    ctx: &mut FnCtx<'f>,
    obj: &mut RootedAcc,
    props: &[(String, Expr)],
    from: usize,
    protect: bool,
) -> Result<Vec<(String, u32)>> {
    for (i, (key, value_expr)) in props.iter().enumerate().skip(from) {
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

            // The closure value is deferred: the patch loop reads it after every
            // remaining property has been lowered, so it must survive their
            // allocations AND be re-read afterwards (an evacuating cycle rewrote
            // the slot; the register queued above is stale). `build` owns the
            // rest of the literal, `finish` is the one place the value escapes,
            // and the release happens on both paths out.
            //
            // #8809: the root is pushed BEFORE the installing
            // `js_object_set_field_by_name`, and that call re-reads it like any
            // other rooted argument. It used to be pushed after, which is #7192
            // exactly — `js_object_set_field_by_name` is a collection point
            // (`POLL_CAPABLE_RUNTIME`: it can run a user setter or a Proxy trap,
            // and the closure it just installed is reachable from the object, so
            // an evacuating minor MOVES it). The register queued above was then
            // stale, and pushing a stale pointer into a slot the collector scans
            // is strictly worse than not rooting at all: the patch loop below
            // read it back and wrote the receiver into abandoned from-space, so
            // the surviving copy's method ran with `this` unset.
            //
            // This arm was unreachable from TypeScript between #809 and #8793 —
            // every source-level literal with a method went through the IIFE
            // builder — which is why the ordering survived the #7192 sweep. See
            // `by_name_method_closure_tests` below, whose HIR-level fixture is
            // the only coverage it had.
            let mut rest: Vec<(String, u32)> = Vec::new();
            let closure_value = rooting::with_rooted_accumulator(
                ctx,
                Repr::Boxed,
                &v,
                protect,
                |ctx, closure| {
                    let key_raw = emit_interned_key_raw(ctx, &key_handle_global);
                    obj.call_void(
                        ctx,
                        "js_object_set_field_by_name",
                        &[Arg::Plain(I64, &key_raw), closure.as_arg()],
                    );
                    rest = lower_by_name_props(ctx, obj, props, i + 1, protect)?;
                    Ok(())
                },
                |_ctx, closure_value| Ok(closure_value.to_string()),
            )?;
            rest.push((closure_value, this_idx));
            return Ok(rest);
        }

        let v = lower_expr(ctx, value_expr)?;
        let key_raw = emit_interned_key_raw(ctx, &key_handle_global);
        obj.call_void(
            ctx,
            "js_object_set_field_by_name",
            &[Arg::Plain(I64, &key_raw), Arg::Plain(DOUBLE, &v)],
        );
    }
    Ok(Vec::new())
}

fn is_generator_iterator_object_literal(props: &[(String, Expr)]) -> bool {
    if props.len() != 3 {
        return false;
    }
    let expected = [
        ("next", "__val"),
        ("return", "__ret_val"),
        ("throw", "__throw_val"),
    ];
    props
        .iter()
        .zip(expected)
        .all(|((key, value), (expected_key, expected_param))| {
            if key != expected_key {
                return false;
            }
            matches!(
                value,
                Expr::Closure {
                    params,
                    captures_this: true,
                    ..
                } if params.len() == 1 && params[0].name == expected_param
            )
        })
}

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
    expected_ty: Option<&HirType>,
) -> Result<String> {
    // #6951: the object handle is allocated BEFORE the property values are
    // lowered and lives in an SSA register across all of them. `{ a: s, b: f() }`
    // therefore had its half-built object swept by `f`'s collection, and the
    // remaining field stores landed in recycled memory. Root the handle when any
    // initializer can collect; literals of plain locals emit no extra IR.
    let protect_handle = rooting::any_operand_may_collect(ctx, props.iter().map(|(_, v)| v));
    let field_count = props.len() as u32;
    let zero_str = "0".to_string();
    let n_str = field_count.to_string();
    let typed_layout = typed_object_literal_layout(ctx, props, expected_ty);
    let generator_iterator_object = is_generator_iterator_object_literal(props);

    // Fast path: no closure-with-`this` props. Use the shape-cache allocator
    // and write fields by INDEX — this skips the per-field linear key-search
    // done by `js_object_set_field_by_name`. Cuts ~10ns per field on the hot
    // path (and saves the keys_array realloc when `getDetailedIdType`-style
    // returns are evaluated 10k×/round). Closure-with-`this` props still
    // need the by-name path because `this_patches` populates them post-build
    // via `js_closure_set_capture_bits`, which assumes the key is already in
    // keys_array — fine here since the shape allocator pre-populates it.
    // A `captures_this` method used to force the WHOLE literal onto the
    // by-name path (all-or-nothing gate): `this_patches` needs the closure
    // value after every later initializer has run, and the by-name arm keeps
    // it alive through nested rooted accumulators. But the shape allocator
    // pre-populates keys_array, so by-INDEX stores are just as valid for
    // method props — and the patch loop can simply RE-READ each method
    // closure from its own field slot at patch time (`js_object_get_field`):
    // the rooted object keeps the closure alive, and the re-read observes
    // any evacuation that moved it, with no per-closure rooting at all. So
    // the gate is now only a kill switch (PERRY_OBJECT_LITERAL_SHAPE_METHODS=0
    // restores the old routing for A/B).
    let any_method_closure = !generator_iterator_object
        && props.iter().any(|(_, v)| {
            matches!(
                v,
                Expr::Closure {
                    captures_this: true,
                    ..
                }
            )
        });
    // Read per literal, deliberately uncached: this is a compile-time
    // decision (nanoseconds against a whole literal's codegen), and the
    // by-name tests below pin the fallback arm by flipping the env under a
    // module-local mutex — an OnceLock would freeze whichever state ran
    // first and silently un-pin the arm the tests exist to exercise.
    let shape_path_methods_enabled = !matches!(
        std::env::var("PERRY_OBJECT_LITERAL_SHAPE_METHODS").as_deref(),
        Ok("0") | Ok("off") | Ok("false")
    );

    if (!any_method_closure || shape_path_methods_enabled) && field_count > 0 {
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

        // `(field_idx, reserved_this_slot_idx)` for each `captures_this`
        // method: consumed in `finish`, where each closure is re-read from
        // its own field slot (see the gate comment above).
        let shape_this_patches: std::cell::RefCell<Vec<(u32, u32)>> =
            std::cell::RefCell::new(Vec::new());
        return rooting::with_rooted_accumulator(
            ctx,
            Repr::Ptr,
            &obj_handle,
            protect_handle,
            |ctx, obj| {
                for (i, (_, value_expr)) in props.iter().enumerate() {
                    if let Expr::Closure {
                        params: cparams,
                        body: cbody,
                        captures: ccaps,
                        captures_this: true,
                        ..
                    } = value_expr
                    {
                        let auto_caps = compute_auto_captures(ctx, cparams, cbody, ccaps);
                        shape_this_patches
                            .borrow_mut()
                            .push((i as u32, auto_caps.len() as u32));
                    }
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
                    let v_bits = ctx.block().bitcast_double_to_i64(&v);
                    obj.call_void(
                        ctx,
                        "js_object_set_field",
                        &[Arg::Plain(I32, &idx_str), Arg::Plain(I64, &v_bits)],
                    );
                }
                Ok(())
            },
            |ctx, obj_handle| {
                // Patch each method closure's reserved `this` slot AFTER all
                // fields are set, reading the closure back from the object so
                // the pointer is post-evacuation fresh (the by-name arm's
                // nested-rooting dance is unnecessary here).
                let patches = shape_this_patches.borrow();
                if !patches.is_empty() {
                    let blk = ctx.block();
                    let obj_tagged_bits = blk.or(I64, obj_handle, crate::nanbox::POINTER_TAG_I64);
                    for (field_idx, this_idx) in patches.iter() {
                        let fld = field_idx.to_string();
                        let bits = blk.call(
                            I64,
                            "js_object_get_field",
                            &[(I64, obj_handle), (I32, &fld)],
                        );
                        let closure_handle = blk.and(I64, &bits, POINTER_MASK_I64);
                        let idx_str = this_idx.to_string();
                        blk.call_void(
                            "js_closure_set_capture_bits",
                            &[
                                (I64, &closure_handle),
                                (I32, &idx_str),
                                (I64, &obj_tagged_bits),
                            ],
                        );
                    }
                }
                if let Some(layout) = typed_layout.as_ref() {
                    emit_object_typed_shape_init(ctx, obj_handle, layout);
                }
                Ok(nanbox_pointer_inline(ctx.block(), obj_handle))
            },
        );
    }

    let obj_handle = ctx
        .block()
        .call(I64, "js_object_alloc", &[(I32, &zero_str), (I32, &n_str)]);

    // `(closure_value_double, reserved_this_slot_idx)` for each method closure
    // that needs `this` patched after the object is fully built. Enables
    // `calc.add(n) { this.value = ... }`.
    //
    // A `RefCell` rather than a plain `&mut`: `build` fills it and `finish`
    // reads it, and the two closures are handed to `with_rooted_accumulator`
    // together — a `&mut` in one and a `&` in the other is `E0499`. Nothing
    // here is re-entrant, so the borrow discipline is trivially satisfied.
    let this_patches: std::cell::RefCell<Vec<(String, u32)>> = std::cell::RefCell::new(Vec::new());

    rooting::with_rooted_accumulator(
        ctx,
        Repr::Ptr,
        &obj_handle,
        protect_handle,
        |ctx, obj| {
            let mut patches = lower_by_name_props(ctx, obj, props, 0, protect_handle)?;
            // `lower_by_name_props` returns innermost-first, i.e. the LAST
            // method closure in source order first, because each nesting
            // appends its own value after the suffix it wrapped.
            patches.reverse();
            *this_patches.borrow_mut() = patches;
            Ok(())
        },
        |ctx, obj_handle| {
            // Patch each method closure's reserved `this` slot with the object
            // pointer (NaN-boxed). Done AFTER all fields are set so every
            // method sees the fully-initialized object. Every closure value
            // reaching here was read in its own `finish`, i.e. below the last
            // initializer that could have relocated it.
            let this_patches = this_patches.borrow();
            if !this_patches.is_empty() {
                let blk = ctx.block();
                let obj_tagged = {
                    let tagged = blk.or(I64, obj_handle, crate::nanbox::POINTER_TAG_I64);
                    blk.bitcast_i64_to_double(&tagged)
                };
                for (closure_val, this_idx) in this_patches.iter() {
                    let bits = blk.bitcast_double_to_i64(closure_val);
                    let closure_handle = blk.and(I64, &bits, POINTER_MASK_I64);
                    let idx_str = this_idx.to_string();
                    let obj_bits = blk.bitcast_double_to_i64(&obj_tagged);
                    blk.call_void(
                        "js_closure_set_capture_bits",
                        &[(I64, &closure_handle), (I32, &idx_str), (I64, &obj_bits)],
                    );
                }
            }

            if let Some(layout) = typed_layout.as_ref() {
                emit_object_typed_shape_init(ctx, obj_handle, layout);
            }

            Ok(nanbox_pointer_inline(ctx.block(), obj_handle))
        },
    )
}

#[cfg(test)]
mod by_name_method_closure_tests {
    use perry_hir::types::Type;
    use perry_hir::{Expr, Function, Module as HirModule, Param, Stmt};

    /// `{ add(k) {…}, tag: {}, scale(k) {…}, last: {} }` — an object literal
    /// carrying two `this`-capturing method closures. That selects
    /// `lower_object_literal`'s BY-NAME path and with it the deferred
    /// `this`-patch machinery this module's nested accumulators root.
    ///
    /// Built from HIR rather than from TypeScript on purpose — originally
    /// because the branch was unreachable from source, and now because this is
    /// the only place its *shape* is asserted rather than sampled.
    ///
    /// **That unreachability lapsed, and it cost a shipped rooting bug.** From
    /// #809 to #8793 every source-level object literal containing a
    /// `Prop::Method` lowered to a source-ordered IIFE over `{}`
    /// (`js_object_set_method_by_name`), so this path never ran on real code:
    /// over the whole `gc_root_dominance_corpus.sh` corpus every emitted
    /// `js_object_alloc` was `(i32 0, i32 0)`. #8793 routes a static-key method
    /// literal straight to `Expr::Object`, which lands here — and the late root
    /// this arm had carried unexercised since #6951 became seven live
    /// `--moving-only` violations the next morning (#8809). A branch no corpus
    /// reaches is a branch that keeps whatever bug it has until something
    /// reaches it, so treat "not reachable from TypeScript" as a note about
    /// today's front end, never as a reason a hazard here is theoretical.
    ///
    /// The two closures deliberately reserve DIFFERENT `this` slots — `add`
    /// captures nothing so its slot index is 0, `scale` captures `base` so its
    /// index is 1 — which is what lets
    /// [`the_patches_are_applied_in_source_order`] tell the two patch calls
    /// apart.
    /// Serializes the routing-env pin across this module's tests. The env
    /// var only affects literals that carry a `captures_this` method, and
    /// this module owns every such literal in the test corpus, so pinning
    /// under this mutex cannot perturb any concurrently running test.
    static ROUTING_PIN: std::sync::Mutex<()> = std::sync::Mutex::new(());

    pub(super) struct RoutingPin {
        prev: Option<std::ffi::OsString>,
        _guard: std::sync::MutexGuard<'static, ()>,
    }
    impl Drop for RoutingPin {
        fn drop(&mut self) {
            match self.prev.take() {
                Some(v) => std::env::set_var("PERRY_OBJECT_LITERAL_SHAPE_METHODS", v),
                None => std::env::remove_var("PERRY_OBJECT_LITERAL_SHAPE_METHODS"),
            }
        }
    }
    pub(super) fn pin_routing(value: &str) -> RoutingPin {
        let guard = ROUTING_PIN
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let prev = std::env::var_os("PERRY_OBJECT_LITERAL_SHAPE_METHODS");
        std::env::set_var("PERRY_OBJECT_LITERAL_SHAPE_METHODS", value);
        RoutingPin { prev, _guard: guard }
    }

    pub(super) fn method_literal_ir() -> String {
        let closure = |func_id: u32, captures: Vec<u32>| {
            let param_id = 10 + func_id;
            let mut body = vec![Stmt::Return(Some(Expr::LocalGet(param_id)))];
            if !captures.is_empty() {
                body.insert(0, Stmt::Expr(Expr::LocalGet(captures[0])));
            }
            Expr::Closure {
                func_id,
                params: vec![Param {
                    id: param_id,
                    name: "k".to_string(),
                    ty: Type::Any,
                    default: None,
                    decorators: Vec::new(),
                    is_rest: false,
                    arguments_object: None,
                }],
                return_type: Type::Any,
                body,
                captures,
                mutable_captures: Vec::new(),
                captures_this: true,
                captures_new_target: false,
                enclosing_class: None,
                is_arrow: false,
                is_async: false,
                is_generator: false,
                is_strict: true,
            }
        };
        let mut hir = HirModule::new("objlit_by_name_test");
        hir.functions.push(Function {
            id: 0,
            name: "build".to_string(),
            type_params: Vec::new(),
            params: Vec::new(),
            return_type: Type::Any,
            body: vec![
                Stmt::Let {
                    id: 5,
                    name: "base".to_string(),
                    ty: Type::Number,
                    mutable: false,
                    init: Some(Expr::Number(1.0)),
                },
                Stmt::Return(Some(Expr::Object(vec![
                    ("add".to_string(), closure(1, Vec::new())),
                    // Allocating initializers AFTER each closure: that is what
                    // makes `protect_handle` true, i.e. what puts the closure
                    // values in a window at all.
                    ("tag".to_string(), Expr::Object(Vec::new())),
                    ("scale".to_string(), closure(2, vec![5])),
                    ("last".to_string(), Expr::Object(Vec::new())),
                ]))),
            ],
            is_async: false,
            is_generator: false,
            is_strict: true,
            is_exported: false,
            captures: Vec::new(),
            decorators: Vec::new(),
            was_plain_async: false,
            was_unrolled: false,
        });
        let opts = crate::CompileOptions {
            emit_ir_only: true,
            ..Default::default()
        };
        let bytes = crate::compile_module(&hir, opts).expect("test module compiles");
        String::from_utf8(bytes).expect("LLVM IR is UTF-8")
    }

    /// The `build` function's body, and only it — the closure bodies are
    /// separate `define`s and contain none of what is asserted below.
    pub(super) fn build_fn(ir: &str) -> String {
        let mut body = Vec::new();
        let mut inside = false;
        for line in ir.lines() {
            if line.starts_with("define ") {
                inside = line.contains("__build(");
                continue;
            }
            if inside {
                if line == "}" {
                    break;
                }
                body.push(line.trim());
            }
        }
        assert!(
            !body.is_empty(),
            "no `build` function in the emitted IR:\n{ir}"
        );
        body.join("\n")
    }

#[cfg(test)]
mod shape_method_literal_tests {
    //! Shape-path twins of `by_name_method_closure_tests`: the DEFAULT
    //! routing for a `captures_this` method literal since the all-or-nothing
    //! gate became a kill switch. Same HIR fixture, same hazards pinned —
    //! deferred patches must run below every store, in source order — plus
    //! the shape path's own discipline: each patched closure is RE-READ from
    //! its field slot (`js_object_get_field`) instead of being kept alive in
    //! a rooted slot, so a patch that reads anything else regresses to the
    //! moved-from-address bug the by-name arm needed #8809 to fix.
    use super::{build_fn, method_literal_ir, pin_routing};

    #[test]
    fn a_this_capturing_method_selects_the_shape_path() {
        let _pin = pin_routing("1");
        let ir = build_fn(&method_literal_ir());
        assert!(
            ir.contains("@js_object_alloc_with_shape("),
            "the method literal must allocate through the shape cache:\n{ir}"
        );
        assert_eq!(
            ir.matches("@js_object_set_field(").count(),
            4,
            "every property is stored by INDEX on this path:\n{ir}"
        );
        assert_eq!(
            ir.matches("@js_object_set_field_by_name(").count(),
            0,
            "no by-name stores may remain:\n{ir}"
        );
    }

    #[test]
    fn the_patches_run_below_every_store_and_reread_their_closure() {
        let _pin = pin_routing("1");
        let ir = build_fn(&method_literal_ir());
        let last_store = ir
            .rfind("@js_object_set_field(")
            .expect("an indexed store");
        let tail = &ir[last_store..];
        assert_eq!(
            tail.matches("@js_closure_set_capture_bits(").count(),
            2,
            "one patch per `this`-capturing method, all below the last store:\n{tail}"
        );
        assert_eq!(
            tail.matches("@js_object_get_field(").count(),
            2,
            "each patch must RE-READ its closure from the object's field slot \
             (a kept register is stale after an evacuating initializer):\n{tail}"
        );
    }

    #[test]
    fn the_patches_are_applied_in_source_order() {
        let _pin = pin_routing("1");
        let ir = build_fn(&method_literal_ir());
        let last_store = ir
            .rfind("@js_object_set_field(")
            .expect("an indexed store");
        let tail = &ir[last_store..];
        // `add` sits at field index 0, `scale` at field index 2; the re-reads
        // name the field, so source order is readable off the `i32 <idx>`
        // argument of each `js_object_get_field`.
        let idxs: Vec<&str> = tail
            .lines()
            .filter(|l| l.contains("@js_object_get_field("))
            .map(|l| {
                l.split("i32 ")
                    .nth(1)
                    .and_then(|v| v.split(')').next())
                    .expect("the field index")
            })
            .collect();
        assert_eq!(
            idxs,
            vec!["0", "2"],
            "`add` (field 0) must be patched before `scale` (field 2):\n{tail}"
        );
    }
}


    /// Everything the lowering emits after the last property store: the
    /// deferred closure re-reads, the patch loop, and the releases.
    fn tail_after_last_property_store(ir: &str) -> String {
        let last_store = ir
            .rfind("@js_object_set_field_by_name(")
            .expect("a by-name store");
        ir[last_store..].to_string()
    }

    /// With the shape routing pinned OFF, the literal must take the BY-NAME
    /// path, or every assertion below is about a branch that did not run.
    /// (Default routing sends `captures_this` literals through the shape
    /// path — see `shape_method_literal_tests` for that arm's pins.)
    #[test]
    fn a_this_capturing_method_selects_the_by_name_path() {
        let _pin = pin_routing("0");
        let ir = build_fn(&method_literal_ir());
        assert!(
            ir.contains("@js_object_alloc(i32 0, i32 4)"),
            "the four-property literal must allocate through the by-name path:\n{ir}"
        );
        assert_eq!(
            ir.matches("@js_object_set_field_by_name(").count(),
            4,
            "every property is stored by name on this path:\n{ir}"
        );
    }

    /// Each `this` patch must be emitted BELOW the last property store — the
    /// ordering the deferral exists for ("every method sees the fully
    /// initialized object"), and what forces the closure values to be rooted in
    /// the first place.
    ///
    /// Counting patch calls over the WHOLE function would be vacuous: closure
    /// construction emits `js_closure_set_capture_bits` too (that is how the
    /// reserved `this` slot is seeded from `js_implicit_this_get`), so the
    /// count only means something in the tail.
    #[test]
    fn the_this_patches_run_below_every_property_store() {
        let _pin = pin_routing("0");
        let ir = build_fn(&method_literal_ir());
        let tail = tail_after_last_property_store(&ir);
        assert_eq!(
            tail.matches("@js_closure_set_capture_bits(").count(),
            2,
            "one patch per `this`-capturing method, all below the last store:\n{tail}"
        );
    }

    /// The patches must be applied in SOURCE order. The nested form fills its
    /// list innermost-first, so a dropped `reverse()` swaps the two — invisible
    /// while both closures reserve the same `this` slot index, and a live
    /// miscompile the moment they do not. Here `add` captures nothing (slot 0)
    /// and `scale` captures `base` (slot 1), so the order is readable off the
    /// `i32 <idx>` argument.
    #[test]
    fn the_patches_are_applied_in_source_order() {
        let _pin = pin_routing("0");
        let ir = build_fn(&method_literal_ir());
        let tail = tail_after_last_property_store(&ir);
        let idxs: Vec<&str> = tail
            .lines()
            .filter(|l| l.contains("@js_closure_set_capture_bits("))
            .map(|l| {
                l.split("i32 ")
                    .nth(1)
                    .and_then(|s| s.split(',').next())
                    .expect("the reserved this-slot index")
            })
            .collect();
        assert_eq!(
            idxs,
            vec!["0", "1"],
            "`add` (slot 0) must be patched before `scale` (slot 1) — reverse order \
             means the nested accumulators' list was not re-reversed:\n{tail}"
        );
    }

    /// #8809: each method closure's root store must DOMINATE the
    /// `js_object_set_field_by_name` that installs it, not follow it.
    ///
    /// The installer is a collection point — it can run a user setter or a
    /// Proxy trap, and by the time it returns the closure is reachable from the
    /// object, so an evacuating minor MOVES it. Rooting afterwards publishes
    /// the pre-move register into a slot the collector scans, and the deferred
    /// patch loop then writes the receiver into abandoned from-space: the
    /// surviving copy's method runs with `this` unset. That was the shipped
    /// state until this test existed.
    ///
    /// Asserted positionally over the emitted lines rather than by counting
    /// calls: the count is identical in both orderings, which is exactly why
    /// #7192's sweep did not catch this one.
    #[test]
    fn a_method_closure_is_rooted_before_it_is_installed() {
        let _pin = pin_routing("0");
        let ir = build_fn(&method_literal_ir());
        let lines: Vec<&str> = ir.lines().collect();
        let closure_allocs: Vec<usize> = lines
            .iter()
            .enumerate()
            .filter(|(_, l)| l.contains("@js_closure_alloc"))
            .map(|(i, _)| i)
            .collect();
        assert_eq!(
            closure_allocs.len(),
            2,
            "the fixture's two method closures must each be allocated here:\n{ir}"
        );
        for alloc in closure_allocs {
            let root = lines[alloc..]
                .iter()
                .position(|l| {
                    l.starts_with("store ptr addrspace(1) %") || l.starts_with("store i64 %")
                })
                .unwrap_or_else(|| {
                    panic!("no root store below the closure allocation on line {alloc}:\n{ir}")
                });
            let install = lines[alloc..]
                .iter()
                .position(|l| l.contains("@js_object_set_field_by_name("))
                .unwrap_or_else(|| {
                    panic!("no by-name install below the closure allocation on line {alloc}:\n{ir}")
                });
            assert!(
                root < install,
                "the closure allocated on line {alloc} is installed before its root store \
                 (root +{root}, install +{install}) — that is #7192's shape and it publishes \
                 a moved-from address:\n{ir}"
            );
        }
    }

    /// The rooting shape, read off the IR. Three GC values are live across the
    /// literal — the object handle and both deferred closure values — so the
    /// lowering must own three rooted slots and must give each back, innermost
    /// first, with the object handle released LAST (it is still needed by the
    /// patch loop).
    #[test]
    fn three_slots_are_rooted_and_released_innermost_first() {
        let _pin = pin_routing("0");
        let full = method_literal_ir();
        let ir = build_fn(&full);
        let tail = tail_after_last_property_store(&ir);
        // Line INDEXES, not `str::find`: the slot registers are `%r3`, `%r16`
        // and `%r38`, and `%r3` is a prefix of `%r38`, so a substring search for
        // the object's release finds the first closure's instead — and the
        // assertion then reads backwards while passing.
        let releases: Vec<usize> = tail
            .lines()
            .enumerate()
            .filter(|(_, l)| l.starts_with("store ptr addrspace(1) null, ptr %"))
            .map(|(i, _)| i)
            .collect();
        assert_eq!(
            releases.len(),
            3,
            "the object handle and both closure values must each be released \
             exactly once below the last store:\n{tail}"
        );
        let release_pos = releases[2];
        let patches: Vec<usize> = tail
            .lines()
            .enumerate()
            .filter(|(_, l)| l.contains("@js_closure_set_capture_bits("))
            .map(|(i, _)| i)
            .collect();
        let patch_pos = *patches.last().expect("a patch call");
        assert!(
            release_pos > patch_pos,
            "the object handle's root must outlive the patch loop that reads it — the \
             release is a stack CUT, so giving it back early drops every closure slot \
             above it too:\n{tail}"
        );
    }
}
