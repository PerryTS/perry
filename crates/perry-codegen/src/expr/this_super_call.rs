//! This / SuperCall.
//!
//! Extracted from `expr/mod.rs` to keep that file under the 2000-line cap.
//! Pure mechanical move — match arm bodies are verbatim copies, called from
//! `lower_expr`'s outer dispatch.

use anyhow::{anyhow, bail, Result};
#[allow(unused_imports)]
use perry_hir::{BinaryOp, CompareOp, Expr, UnaryOp, UpdateOp};
#[allow(unused_imports)]
use perry_types::Type as HirType;

#[allow(unused_imports)]
use crate::lower_call::{lower_call, lower_native_method_call, lower_new};
#[allow(unused_imports)]
use crate::lower_conditional::{lower_conditional, lower_logical, lower_truthy};
#[allow(unused_imports)]
use crate::lower_string_method::{
    flatten_string_add_chain, lower_string_coerce_concat, lower_string_concat,
    lower_string_concat_chain, lower_string_self_append,
};
#[allow(unused_imports)]
use crate::nanbox::{double_literal, POINTER_MASK_I64};
#[allow(unused_imports)]
use crate::type_analysis::{
    compute_auto_captures, is_array_expr, is_bigint_expr, is_bool_expr, is_map_expr,
    is_numeric_expr, is_set_expr, is_string_expr, is_url_search_params_expr, receiver_class_name,
};
#[allow(unused_imports)]
use crate::types::{DOUBLE, I1, I32, I64, I8, PTR};

#[allow(unused_imports)]
use super::{
    buffer_alias_metadata_suffix, can_lower_expr_as_i32, emit_layout_note_slot_on_block,
    emit_shadow_slot_clear, emit_shadow_slot_update_for_expr, emit_string_literal_global,
    emit_v8_export_call, emit_v8_member_method_call, emit_write_barrier,
    emit_write_barrier_slot_on_block, expr_is_known_non_pointer_shadow_value,
    extract_array_of_object_shape, i32_bool_to_nanbox, import_origin_suffix,
    is_global_this_builtin_function_name, is_global_this_builtin_name, is_known_finite,
    lower_array_literal, lower_channel_reduction, lower_expr, lower_expr_as_i32,
    lower_index_set_fast, lower_js_args_array, lower_object_literal, lower_stream_super_init,
    lower_url_string_getter, nanbox_bigint_inline, nanbox_pointer_inline,
    nanbox_pointer_inline_pub, nanbox_string_inline, proxy_build_args_array, try_flat_const_2d_int,
    try_lower_flat_const_index_get, try_match_channel_reduction, try_static_class_name,
    unbox_str_handle, unbox_to_i64, variant_name, ChannelReduction, FlatConstInfo, FnCtx,
    I18nLowerCtx,
};

pub(crate) fn lower(ctx: &mut FnCtx<'_>, expr: &Expr) -> Result<String> {
    match expr {
        Expr::This => {
            if let Some(slot) = ctx.this_stack.last().cloned() {
                Ok(ctx.block().load(DOUBLE, &slot))
            } else {
                Ok(ctx.block().call(DOUBLE, "js_implicit_this_get", &[]))
            }
        }

        // `super(args…)` — Phase C.2 inheritance. Look up the current
        // class's parent and inline the parent's constructor body
        // with the SAME `this` (so parent fields end up on the same
        // object). Parent's parameters get fresh slots populated with
        // the lowered super-call args.
        //
        // The current class is the topmost entry in `class_stack`. The
        // parent is `current_class.extends_name` (Perry uses the string
        // form for cross-module/late-resolved cases) or
        // `current_class.extends.and_then(class_id_to_name)`. For Phase
        // C.2 we use `extends_name` which is always populated when
        // there's a parent.
        Expr::SuperCall(super_args) => {
            // Soft fallback for super() outside a class context: lower
            // args and return undefined.
            let Some(current_class_name) = ctx.class_stack.last().cloned() else {
                for a in super_args {
                    let _ = lower_expr(ctx, a)?;
                }
                return Ok(double_literal(0.0));
            };
            let current_class = match ctx.classes.get(&current_class_name).copied() {
                Some(c) => c,
                None => {
                    for a in super_args {
                        let _ = lower_expr(ctx, a)?;
                    }
                    return Ok(double_literal(0.0));
                }
            };
            let Some(parent_name) = current_class.extends_name.as_deref().map(|s| s.to_string())
            else {
                for a in super_args {
                    let _ = lower_expr(ctx, a)?;
                }
                return Ok(double_literal(0.0));
            };
            let parent_class = match ctx.classes.get(&parent_name).copied() {
                Some(c) => c,
                None => {
                    // Issue #562: `class X extends WritableStream/ReadableStream/TransformStream`
                    // — `super({ ... })` allocates an underlying stream registry handle and
                    // stashes it on `this` under `__perry_stream_handle__`. Inherited methods
                    // (`pipeTo`, `getWriter`, etc.) and arguments to `pipeTo`/`pipeThrough`
                    // route the receiver through `js_stream_unwrap_handle` at the FFI site
                    // so a subclass instance dispatches to the same FFIs a bare handle does.
                    let stream_kind = match parent_name.as_str() {
                        "ReadableStream" => Some("readable"),
                        "WritableStream" => Some("writable"),
                        "TransformStream" => Some("transform"),
                        _ => None,
                    };
                    if let Some(kind) = stream_kind {
                        let result = lower_stream_super_init(ctx, kind, super_args)?;
                        // Per JS spec field initializers run AFTER super()
                        // returns. Without this, `this.foo = []` declared
                        // on the subclass never executes — instance reads
                        // see uninitialized slots. Mirrors the equivalent
                        // call in the user-class super branch below
                        // (line ~4521). Refs #562.
                        let current_class_name =
                            ctx.class_stack.last().cloned().unwrap_or_default();
                        crate::lower_call::apply_field_initializers_recursive(
                            ctx,
                            &current_class_name,
                            crate::lower_call::FieldInitMode::SelfOnly,
                        )?;
                        return Ok(result);
                    }
                    // Built-in parent (Error, TypeError, RangeError, etc.)
                    // — user classes extending them need `super(message)` to
                    // assign `this.message = args[0]` and `this.name = parent_name`
                    // so downstream `err.message` / `err.name` access works.
                    // `instanceof Error` walking the extends chain is handled
                    // elsewhere; this just makes `err.message` non-undefined.
                    let is_error_like = matches!(
                        parent_name.as_str(),
                        "Error"
                            | "TypeError"
                            | "RangeError"
                            | "ReferenceError"
                            | "SyntaxError"
                            | "URIError"
                            | "EvalError"
                            | "AggregateError"
                    );
                    // Lower args — at most 1 (message) for Error-like.
                    let mut lowered_args: Vec<String> = Vec::with_capacity(super_args.len());
                    for a in super_args {
                        lowered_args.push(lower_expr(ctx, a)?);
                    }
                    if is_error_like {
                        // Need the `this` pointer to set fields on.
                        let this_slot = ctx.this_stack.last().cloned();
                        if let Some(this_slot) = this_slot {
                            let blk = ctx.block();
                            let this_box = blk.load(DOUBLE, &this_slot);
                            let this_bits = blk.bitcast_double_to_i64(&this_box);
                            let this_handle = blk.and(I64, &this_bits, POINTER_MASK_I64);
                            // this.message = args[0] (if provided)
                            if let Some(msg_val) = lowered_args.first() {
                                let key_idx = ctx.strings.intern("message");
                                let key_handle_global =
                                    format!("@{}", ctx.strings.entry(key_idx).handle_global);
                                let blk = ctx.block();
                                let key_box = blk.load(DOUBLE, &key_handle_global);
                                let key_bits = blk.bitcast_double_to_i64(&key_box);
                                let key_raw = blk.and(I64, &key_bits, POINTER_MASK_I64);
                                blk.call_void(
                                    "js_object_set_field_by_name",
                                    &[(I64, &this_handle), (I64, &key_raw), (DOUBLE, msg_val)],
                                );
                            }
                            // this.name = <parent_name> as default (can be
                            // overridden by the subclass constructor body).
                            let name_idx = ctx.strings.intern("name");
                            let name_handle_global =
                                format!("@{}", ctx.strings.entry(name_idx).handle_global);
                            let name_val_idx = ctx.strings.intern(&parent_name);
                            let name_val_global =
                                format!("@{}", ctx.strings.entry(name_val_idx).handle_global);
                            let blk = ctx.block();
                            let name_key_box = blk.load(DOUBLE, &name_handle_global);
                            let name_key_bits = blk.bitcast_double_to_i64(&name_key_box);
                            let name_key_raw = blk.and(I64, &name_key_bits, POINTER_MASK_I64);
                            let name_val_box = blk.load(DOUBLE, &name_val_global);
                            blk.call_void(
                                "js_object_set_field_by_name",
                                &[
                                    (I64, &this_handle),
                                    (I64, &name_key_raw),
                                    (DOUBLE, &name_val_box),
                                ],
                            );
                        }
                    }
                    return Ok(double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED)));
                }
            };

            // Lower the super-call args.
            let mut lowered_args: Vec<String> = Vec::with_capacity(super_args.len());
            for a in super_args {
                lowered_args.push(lower_expr(ctx, a)?);
            }

            // Inline the parent constructor with the SAME this and a
            // fresh param scope for the parent's params.
            //
            // Walk the parent chain when the IMMEDIATE parent has no
            // constructor of its own — JS spec: an empty class implicitly
            // forwards args to its super, so `class Mid extends Base {}`
            // followed by `class Leaf extends Mid {}` calling `super(...)`
            // must reach Base's constructor body. Without this walk,
            // perry's super() produced a no-op when Mid had no ctor, and
            // Base's `this.config = {...}` never ran. Refs #420 (drizzle
            // PgSerialBuilder → PgColumnBuilder → ColumnBuilder chain
            // where only ColumnBuilder has a ctor body).
            // Walk up the parent chain to find the first class with a
            // local constructor body OR a cross-module ctor stub WITH
            // declared params. JS spec requires `class Mid extends Base {}`
            // followed by `class Leaf extends Mid` calling `super(...)` to
            // reach Base's ctor body (Mid has no ctor → implicit forward).
            // Refs #420 (drizzle's PgSerialBuilder → PgColumnBuilder →
            // ColumnBuilder where only ColumnBuilder has a body).
            //
            // We must skip past imported ctors with param_count=0 too —
            // those represent empty-bodied derived classes whose imported
            // standalone ctor would otherwise eat the incoming args
            // without forwarding. Walking past them and dispatching
            // directly to the ancestor-with-real-params standalone ctor
            // preserves the args end-to-end.
            let mut effective_parent_name = parent_name.clone();
            let mut effective_parent_class = parent_class;
            loop {
                let has_local_body = effective_parent_class.constructor.is_some();
                let has_real_imported_ctor = ctx
                    .imported_class_ctors
                    .get(&effective_parent_name)
                    .map(|(_, n)| *n > 0)
                    .unwrap_or(false);
                if has_local_body || has_real_imported_ctor {
                    break;
                }
                let Some(grandparent_name) = effective_parent_class
                    .extends_name
                    .as_deref()
                    .map(|s| s.to_string())
                else {
                    break;
                };
                let Some(gp_class) = ctx.classes.get(&grandparent_name).copied() else {
                    break;
                };
                effective_parent_name = grandparent_name;
                effective_parent_class = gp_class;
            }

            if let Some(parent_ctor) = &effective_parent_class.constructor {
                let saved_locals = ctx.locals.clone();
                let saved_local_types = ctx.local_types.clone();

                for (param, arg_val) in parent_ctor.params.iter().zip(lowered_args.iter()) {
                    // Parent ctor params become ctx.locals for the
                    // inlined body; a closure inside the parent ctor
                    // may capture them, so hoist to the entry block
                    // for dominance safety.
                    let slot = ctx.func.alloca_entry(DOUBLE);
                    ctx.block().store(DOUBLE, arg_val, &slot);
                    ctx.locals.insert(param.id, slot);
                    ctx.local_types.insert(param.id, param.ty.clone());
                }

                ctx.class_stack.push(effective_parent_name.clone());
                crate::stmt::lower_stmts(ctx, &parent_ctor.body)?;
                ctx.class_stack.pop();

                ctx.locals = saved_locals;
                ctx.local_types = saved_local_types;
            } else if let Some(error_kind) = {
                // Issue #573: walk the chain from `effective_parent_class`
                // upward; if it terminates at an Error-like built-in,
                // emit the same Error init the no-parent-class branch
                // does (sets this.message + this.name). Without this,
                // `class C extends Error {}; class D extends C { ctor(m){
                // super(m); } }` reaches here with `effective_parent_class
                // = C` (no own ctor) and a parent of "Error" (not in
                // ctx.classes), so neither inline nor cross-module-ctor
                // path fires and `super(msg)` becomes a no-op.
                let mut found: Option<String> = None;
                let mut cur = Some(effective_parent_name.clone());
                let mut depth = 0usize;
                while let Some(pname) = cur {
                    if matches!(
                        pname.as_str(),
                        "Error"
                            | "TypeError"
                            | "RangeError"
                            | "ReferenceError"
                            | "SyntaxError"
                            | "URIError"
                            | "EvalError"
                            | "AggregateError"
                    ) {
                        found = Some(pname);
                        break;
                    }
                    cur = ctx
                        .classes
                        .get(pname.as_str())
                        .and_then(|c| c.extends_name.clone());
                    depth += 1;
                    if depth > 32 {
                        break;
                    }
                }
                found
            } {
                let this_slot = ctx.this_stack.last().cloned();
                if let Some(this_slot) = this_slot {
                    let blk = ctx.block();
                    let this_box = blk.load(DOUBLE, &this_slot);
                    let this_bits = blk.bitcast_double_to_i64(&this_box);
                    let this_handle = blk.and(I64, &this_bits, POINTER_MASK_I64);
                    if let Some(msg_val) = lowered_args.first() {
                        let key_idx = ctx.strings.intern("message");
                        let key_handle_global =
                            format!("@{}", ctx.strings.entry(key_idx).handle_global);
                        let blk = ctx.block();
                        let key_box = blk.load(DOUBLE, &key_handle_global);
                        let key_bits = blk.bitcast_double_to_i64(&key_box);
                        let key_raw = blk.and(I64, &key_bits, POINTER_MASK_I64);
                        blk.call_void(
                            "js_object_set_field_by_name",
                            &[(I64, &this_handle), (I64, &key_raw), (DOUBLE, msg_val)],
                        );
                    }
                    let name_idx = ctx.strings.intern("name");
                    let name_handle_global =
                        format!("@{}", ctx.strings.entry(name_idx).handle_global);
                    let name_val_idx = ctx.strings.intern(&error_kind);
                    let name_val_global =
                        format!("@{}", ctx.strings.entry(name_val_idx).handle_global);
                    let blk = ctx.block();
                    let name_key_box = blk.load(DOUBLE, &name_handle_global);
                    let name_key_bits = blk.bitcast_double_to_i64(&name_key_box);
                    let name_key_raw = blk.and(I64, &name_key_bits, POINTER_MASK_I64);
                    let name_val_box = blk.load(DOUBLE, &name_val_global);
                    blk.call_void(
                        "js_object_set_field_by_name",
                        &[
                            (I64, &this_handle),
                            (I64, &name_key_raw),
                            (DOUBLE, &name_val_box),
                        ],
                    );
                }
            } else if let Some((ctor_name, param_count)) = ctx
                .imported_class_ctors
                .get(&effective_parent_name)
                .cloned()
            {
                // Issue #485: parent class is imported (stub with `constructor: None`)
                // and has no inlineable body in this module. Call the cross-module
                // standalone constructor symbol — it exists per-class in the source
                // module (compile_method emits `<source_prefix>__<class>_constructor`)
                // and itself runs `apply_field_initializers_recursive_pub`, so calling
                // it from `super()` inherits the parent's arrow-class-field
                // initializers (e.g. HonoBase's `request = (...) => ...`,
                // `fetch = (...) => ...`) onto `this`. Without this branch, perry
                // silently drops `super(...)` for imported parents and the subclass
                // ends up with only its own fields, breaking hono-base inheritance.
                let undef_lit = double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED));
                while lowered_args.len() < param_count {
                    lowered_args.push(undef_lit.clone());
                }
                let this_slot = ctx.this_stack.last().cloned();
                let this_box = if let Some(slot) = this_slot {
                    ctx.block().load(DOUBLE, &slot)
                } else {
                    undef_lit.clone()
                };
                let ctor_param_types: Vec<crate::types::LlvmType> = std::iter::once(DOUBLE)
                    .chain(lowered_args.iter().map(|_| DOUBLE))
                    .collect();
                let mut ctor_args: Vec<(crate::types::LlvmType, &str)> =
                    Vec::with_capacity(1 + lowered_args.len());
                ctor_args.push((DOUBLE, &this_box));
                for la in &lowered_args {
                    ctor_args.push((DOUBLE, la.as_str()));
                }
                ctx.pending_declares.push((
                    ctor_name.clone(),
                    crate::types::VOID,
                    ctor_param_types,
                ));
                ctx.block().call_void(&ctor_name, &ctor_args);
            }

            // After the parent body has run (which may have set `this.config`
            // etc.), apply field initializers for each class between
            // `effective_parent_name` (exclusive) and `current_class_name`
            // (inclusive). Per JS spec each default-ctor class's field
            // inits run immediately after that class's super() returns.
            // For drizzle's `SQLiteInteger ← SQLiteBaseInteger ← SQLiteColumn`,
            // walking up from SuperCall in SQLiteInteger finds the
            // inherited ctor at SQLiteColumn (effective_parent_name);
            // SQLiteBaseInteger (intermediate, no ctor) has fields
            // `autoIncrement = this.config.autoIncrement` that must run
            // after SQLiteColumn's body sets `this.config`. Refs #631.
            //
            // Walk parent → ... → effective_parent_name (exclusive),
            // collect intermediate names. Apply SelfOnly for each in
            // root-most-first order, then for current_class_name.
            let mut intermediates: Vec<String> = Vec::new();
            let mut walker = current_class.extends_name.as_deref().map(|s| s.to_string());
            while let Some(pname) = walker {
                if pname == effective_parent_name {
                    break;
                }
                intermediates.push(pname.clone());
                walker = ctx
                    .classes
                    .get(&pname)
                    .and_then(|c| c.extends_name.as_deref().map(|s| s.to_string()));
            }
            // Root-most intermediate first (reverse insertion order).
            intermediates.reverse();
            for inter in &intermediates {
                crate::lower_call::apply_field_initializers_recursive(
                    ctx,
                    inter,
                    crate::lower_call::FieldInitMode::SelfOnly,
                )?;
            }
            crate::lower_call::apply_field_initializers_recursive(
                ctx,
                &current_class_name,
                crate::lower_call::FieldInitMode::SelfOnly,
            )?;

            // super() evaluates to undefined in JS.
            Ok(double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED)))
        }

        // -------- isNaN(x) — global, coerces via ToNumber --------
        // Per ECMA-262 §19.2.3, the global `isNaN` first coerces its
        // argument via ToNumber and then checks if the result is NaN.
        // The pre-fix inline `fcmp uno x, x` idiom checked the raw bit
        // pattern, but every NaN-boxed value (strings, pointers, etc.)
        // has a NaN bit pattern — `isNaN("1")` returned true (correct
        // is false because "1" coerces to 1). Route to `js_is_nan` which
        // implements the ToNumber-then-check sequence. `Number.isNaN`
        // (strict, no coercion) goes through `Expr::NumberIsNaN` and
        // already calls `js_number_is_nan`.
        _ => unreachable!("expr/mod.rs dispatched a variant not handled by this submodule"),
    }
}
