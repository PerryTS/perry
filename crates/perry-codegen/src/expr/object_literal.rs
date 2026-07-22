//! Object-literal lowering (extracted from `expr.rs`, issue #1098).
//! Pure move — no logic changes.

use anyhow::Result;
use perry_hir::types::Type as HirType;
use perry_hir::Expr;

use super::{lower_expr, nanbox_pointer_inline, FnCtx};
use crate::nanbox::POINTER_MASK_I64;
use crate::type_analysis::{compute_auto_captures, is_numeric_expr};
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

fn unboxed_object_fields_enabled() -> bool {
    matches!(
        std::env::var("PERRY_UNBOXED_OBJECT_FIELDS").as_deref(),
        Ok("1")
    )
}

fn is_number_type(ty: &HirType) -> bool {
    matches!(ty, HirType::Number)
}

fn object_type_is_exact_xy_number(ty: &perry_hir::types::ObjectType) -> bool {
    ty.index_signature.is_none()
        && ty.properties.len() == 2
        && ty
            .properties
            .get("x")
            .map(|prop| is_number_type(&prop.ty))
            .unwrap_or(false)
        && ty
            .properties
            .get("y")
            .map(|prop| is_number_type(&prop.ty))
            .unwrap_or(false)
}

fn interface_is_exact_xy_number(iface: &perry_hir::Interface) -> bool {
    iface.extends.is_empty()
        && iface.methods.is_empty()
        && iface.properties.len() == 2
        && iface
            .properties
            .iter()
            .any(|prop| prop.name == "x" && is_number_type(&prop.ty))
        && iface
            .properties
            .iter()
            .any(|prop| prop.name == "y" && is_number_type(&prop.ty))
}

fn expected_type_is_exact_xy_number(ctx: &FnCtx<'_>, expected_ty: &HirType, depth: usize) -> bool {
    if depth > 32 {
        return false;
    }
    match expected_ty {
        HirType::Object(obj) => object_type_is_exact_xy_number(obj),
        HirType::Named(name) => {
            if let Some(alias) = ctx.type_aliases.get(name) {
                if expected_type_is_exact_xy_number(ctx, alias, depth + 1) {
                    return true;
                }
            }
            ctx.interfaces
                .get(name)
                .map(interface_is_exact_xy_number)
                .unwrap_or(false)
        }
        _ => false,
    }
}

fn unboxed_xy_object_literal(
    ctx: &FnCtx<'_>,
    props: &[(String, Expr)],
    expected_ty: Option<&HirType>,
) -> bool {
    if !unboxed_object_fields_enabled() {
        return false;
    }
    if props.len() != 2 || props[0].0 != "x" || props[1].0 != "y" {
        return false;
    }
    let Some(expected_ty) = expected_ty else {
        return false;
    };
    expected_type_is_exact_xy_number(ctx, expected_ty, 0)
        && props
            .iter()
            .all(|(_, value_expr)| is_numeric_expr(ctx, value_expr))
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

fn emit_unboxed_object_layout_init(ctx: &mut FnCtx<'_>, obj_handle: &str) {
    ctx.block().call_void(
        "js_gc_init_unboxed_object_layout",
        &[(I64, obj_handle), (I32, "2"), (I64, "3"), (I64, "0")],
    );
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

    if !any_method_closure && unboxed_xy_object_literal(ctx, props, expected_ty) {
        let mut packed_keys = String::new();
        for (k, _) in props {
            packed_keys.push_str(k);
            packed_keys.push('\0');
        }
        let keys_idx = ctx.strings.intern(&packed_keys);
        let keys_entry = ctx.strings.entry(keys_idx);
        let keys_global = format!("@{}", keys_entry.bytes_global);
        let keys_len_str = keys_entry.byte_len.to_string();

        let mut shape_id: u32 = 0x811c9dc5;
        for b in packed_keys.as_bytes() {
            shape_id ^= *b as u32;
            shape_id = shape_id.wrapping_mul(0x01000193);
        }
        if shape_id == 0 {
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
            ctx.block().call_void(
                "js_object_set_unboxed_f64_field",
                &[(I64, &obj_handle), (I32, &idx_str), (DOUBLE, &v)],
            );
        }
        emit_unboxed_object_layout_init(ctx, &obj_handle);
        return Ok(nanbox_pointer_inline(ctx.block(), &obj_handle));
    }

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
        if let Some(layout) = typed_layout.as_ref() {
            emit_object_typed_shape_init(ctx, &obj_handle, layout);
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
            let obj_bits = blk.bitcast_double_to_i64(&obj_tagged);
            blk.call_void(
                "js_closure_set_capture_bits",
                &[(I64, &closure_handle), (I32, &idx_str), (I64, &obj_bits)],
            );
        }
    }

    if let Some(layout) = typed_layout.as_ref() {
        emit_object_typed_shape_init(ctx, &obj_handle, layout);
    }

    Ok(nanbox_pointer_inline(ctx.block(), &obj_handle))
}
