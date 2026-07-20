//! StaticFieldGet..NativeModuleRef (class meta + getters).
//!
//! Extracted from `expr/mod.rs` to keep that file under the 2000-line cap.
//! Pure mechanical move — match arm bodies are verbatim copies, called from
//! `lower_expr`'s outer dispatch.

use anyhow::Result;
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
    emit_root_nanbox_store_on_block, emit_shadow_slot_clear, emit_shadow_slot_update_for_expr,
    emit_string_literal_global, emit_v8_export_call, emit_v8_member_method_call,
    emit_write_barrier, emit_write_barrier_slot_on_block, expr_is_known_non_pointer_shadow_value,
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
        Expr::StaticFieldGet {
            class_name,
            field_name,
        } => {
            let key = (class_name.clone(), field_name.clone());
            if let Some(global_name) = ctx.static_field_globals.get(&key).cloned() {
                let g_ref = format!("@{}", global_name);
                Ok(ctx.block().load(DOUBLE, &g_ref))
            } else {
                Ok(double_literal(0.0))
            }
        }
        Expr::StaticFieldSet {
            class_name,
            field_name,
            value,
        } => {
            let v = lower_expr(ctx, value)?;
            let key = (class_name.clone(), field_name.clone());
            if let Some(global_name) = ctx.static_field_globals.get(&key).cloned() {
                let g_ref = format!("@{}", global_name);
                // GC_STORE_AUDIT(ROOT): static field global slot is registered as a mutable GC root
                // (register_module_globals_as_gc_roots walks ctx.static_field_globals since the
                // 2026-07-02 audit fix; before that this comment was aspirational and the slot
                // was unrooted).
                emit_root_nanbox_store_on_block(ctx.block(), &v, &g_ref);
            }
            // v0.5.747: also register the static field in the runtime
            // CLASS_DYNAMIC_PROPS side-table so dynamic-dispatch reads
            // (when the class ref is in an Any-typed local) find it.
            // Refs #420 / #618 followup.
            if let Some(&class_id) = ctx.class_ids.get(class_name) {
                let idx = ctx.strings.intern(field_name);
                let entry = ctx.strings.entry(idx);
                let bytes_ref = format!("@{}", entry.bytes_global);
                let len_str = entry.byte_len.to_string();
                let cid_str = class_id.to_string();
                ctx.block().call_void(
                    "js_class_register_static_field",
                    &[
                        (crate::types::I32, &cid_str),
                        (crate::types::PTR, &bytes_ref),
                        (crate::types::I64, &len_str),
                        (DOUBLE, &v),
                    ],
                );
            }
            Ok(v)
        }
        // Issue #711: dynamic parent-class registration for `class X
        // extends fn(...)` shapes. Evaluate the extends expression and
        // call `js_register_class_parent_dynamic(child_cid, value)`,
        // which extracts a class_id from the value (ClassRef payload
        // for INT32-tagged, ObjectHeader.class_id for POINTER-tagged)
        // and wires the parent edge into CLASS_REGISTRY. No-op if the
        // value carries no class_id — preserves the parentless
        // baseline rather than throwing during module init.
        Expr::RegisterClassParentDynamic {
            class_name,
            parent_expr,
        } => {
            let val = lower_expr(ctx, parent_expr)?;
            if let Some(&class_id) = ctx.class_ids.get(class_name) {
                if class_id != 0 {
                    let cid_str = class_id.to_string();
                    ctx.block().call_void(
                        "js_register_class_parent_dynamic",
                        &[(crate::types::I32, &cid_str), (DOUBLE, &val)],
                    );
                }
            }
            // Yield undefined — this expression is always wrapped in
            // `Stmt::Expr` for its side effect; the return value isn't
            // observable to user code.
            Ok(double_literal(f64::from_bits(0x7FFC_0000_0000_0001)))
        }
        // Snapshot a function-nested class's captured outer locals into the
        // runtime CLASS_CAPTURE_VALUES table at the decl site, so DYNAMIC
        // construction of the class value (`exports.C = C; new mod.C()` —
        // the webpack/zod bundle pattern) can fill the synthesized
        // `__perry_cap_<id>` ctor params. Mirrors RegisterClassParentDynamic
        // placement; static `new C()` sites pass captures inline and never
        // consult the table.
        Expr::RegisterClassCaptures {
            class_name,
            captures,
        } => {
            // #6052: the snapshot refresh emitted after EACH captured var's
            // assignment (#6037) can legally read a SIBLING capture whose
            // `let`/`const` has not run yet (`const _fs = ..; <refresh>;
            // const _path = ..` — the SWC CJS interop shape). Those loads are
            // Perry-internal materialization, not user reads: bracket them in
            // a TDZ-suppression window so a dead-zone box snapshots as
            // `undefined` (a later refresh fixes it up) instead of throwing
            // the #6044 ReferenceError. The window holds only these
            // side-effect-free capture loads — no user code runs inside it.
            ctx.block().call_void("js_tdz_suppress_begin", &[]);
            let mut lowered: Vec<String> = Vec::with_capacity(captures.len());
            for c in captures {
                lowered.push(lower_expr(ctx, c)?);
            }
            ctx.block().call_void("js_tdz_suppress_end", &[]);
            if let Some(&class_id) = ctx.class_ids.get(class_name) {
                if class_id != 0 && !lowered.is_empty() {
                    let n = lowered.len();
                    let buf = ctx.func.alloca_entry_array(DOUBLE, n);
                    for (i, v) in lowered.iter().enumerate() {
                        let slot =
                            ctx.block()
                                .gep(DOUBLE, &buf, &[(crate::types::I64, &i.to_string())]);
                        ctx.block().store(DOUBLE, v, &slot);
                    }
                    let ptr_reg = ctx.block().next_reg();
                    ctx.block().emit_raw(format!(
                        "{} = getelementptr [{} x double], ptr {}, i64 0, i64 0",
                        ptr_reg, n, buf
                    ));
                    let cid_str = class_id.to_string();
                    let len_str = n.to_string();
                    ctx.block().call_void(
                        "js_class_register_capture_values",
                        &[
                            (crate::types::I32, &cid_str),
                            (crate::types::PTR, &ptr_reg),
                            (crate::types::I64, &len_str),
                        ],
                    );
                }
            }
            Ok(double_literal(f64::from_bits(0x7FFC_0000_0000_0001)))
        }
        // Read slot `index` of the class's decl-site capture snapshot —
        // STATIC method prologue rebinds (no instance to carry the
        // `__perry_cap_*` fields).
        Expr::ClassCaptureValue {
            class_name,
            index,
            fallback,
            prefer_fallback,
        } => {
            // The fallback (a synthesized `__perry_cap_*` ctor param) must be
            // lowered FIRST so its value is available regardless of whether the
            // class id resolves — and so its side-effect-free read happens in
            // program order.
            let fallback_v = match fallback {
                Some(fb) => Some(lower_expr(ctx, fb)?),
                None => None,
            };
            if let Some(&class_id) = ctx.class_ids.get(class_name) {
                let cid_str = class_id.to_string();
                let idx_str = index.to_string();
                return Ok(match (&fallback_v, *prefer_fallback) {
                    // #5437 (param-first): the LIVE param (the `new`-site cap
                    // arg) wins whenever present; the decl-site snapshot is
                    // consulted ONLY when the param is `undefined` (the
                    // cross-module construct path drops the cap arg). Used by the
                    // synthesized constructor's capture rebind so a SAME-module
                    // `new C(...)`'s current (possibly mutated) outer is NOT
                    // overridden by a stale snapshot.
                    (Some(fb), true) => ctx.block().call(
                        DOUBLE,
                        "js_param_or_class_capture_value",
                        &[
                            (DOUBLE, fb),
                            (crate::types::I32, &cid_str),
                            (crate::types::I32, &idx_str),
                        ],
                    ),
                    // #5437: snapshot-or-fallback (snapshot-first). The decl-site
                    // snapshot wins when it holds a real value (W6: the appended
                    // cap arg may be a mis-boxed multi-level capture, or —
                    // cross-module — absent entirely); otherwise the fallback is
                    // used.
                    (Some(fb), false) => ctx.block().call(
                        DOUBLE,
                        "js_class_capture_value_or",
                        &[
                            (crate::types::I32, &cid_str),
                            (crate::types::I32, &idx_str),
                            (DOUBLE, fb),
                        ],
                    ),
                    (None, _) => ctx.block().call(
                        DOUBLE,
                        "js_class_capture_value",
                        &[(crate::types::I32, &cid_str), (crate::types::I32, &idx_str)],
                    ),
                });
            }
            // Class id unknown in this module: keep the fallback if we have one
            // (the cap param the construct supplied), else `undefined`.
            Ok(fallback_v.unwrap_or_else(|| double_literal(f64::from_bits(0x7FFC_0000_0000_0001))))
        }
        // Issue #894: `static [Symbol.for("k")] = init` inside a
        // class expression returned from a factory function. Emitted
        // by HIR lowering as a `Sequence([…, RegisterClassStaticSymbol,
        // ClassRef])` so each factory invocation re-registers the
        // (class_id, sym_key) → value entry. Without this, the
        // registration would only happen at module-init time when
        // referenced free variables may not yet be assigned, and
        // `isSchema(C)` (which checks `TypeId in C`) returns false on
        // a freshly-returned class.
        Expr::RegisterClassStaticSymbol {
            class_name,
            key_expr,
            value_expr,
        } => {
            let key_v = lower_expr(ctx, key_expr)?;
            let val_v = lower_expr(ctx, value_expr)?;
            if let Some(&class_id) = ctx.class_ids.get(class_name) {
                if class_id != 0 {
                    let cid_str = class_id.to_string();
                    ctx.block().call_void(
                        "js_class_register_static_symbol",
                        &[
                            (crate::types::I32, &cid_str),
                            (DOUBLE, &key_v),
                            (DOUBLE, &val_v),
                        ],
                    );
                }
            }
            Ok(double_literal(f64::from_bits(0x7FFC_0000_0000_0001)))
        }
        Expr::RegisterClassComputedMethod {
            class_name,
            key_expr,
            method_name,
            is_static,
            param_count,
            has_rest,
        } => {
            let key_v = lower_expr(ctx, key_expr)?;
            if let Some(&class_id) = ctx.class_ids.get(class_name) {
                if class_id != 0 {
                    let registry_name = if *is_static {
                        crate::codegen::static_method_registry_key(method_name)
                    } else {
                        method_name.clone()
                    };
                    if let Some(llvm_name) = ctx.methods.get(&(class_name.clone(), registry_name)) {
                        let func_ref = format!("@{}", llvm_name);
                        let func_i64 = ctx.block().ptrtoint(&func_ref, I64);
                        let cid_str = class_id.to_string();
                        let param_count_str = param_count.to_string();
                        let is_static_str = (*is_static as i64).to_string();
                        let has_rest_str = (*has_rest as i64).to_string();
                        ctx.block().call_void(
                            "js_register_class_computed_method",
                            &[
                                (I64, &cid_str),
                                (DOUBLE, &key_v),
                                (I64, &func_i64),
                                (I64, &param_count_str),
                                (I64, &is_static_str),
                                (I64, &has_rest_str),
                            ],
                        );
                    }
                }
            }
            Ok(double_literal(f64::from_bits(0x7FFC_0000_0000_0001)))
        }
        Expr::RegisterClassComputedAccessor {
            class_name,
            key_expr,
            getter_name,
            setter_name,
            is_static,
        } => {
            let key_v = lower_expr(ctx, key_expr)?;
            if let Some(&class_id) = ctx.class_ids.get(class_name) {
                if class_id != 0 {
                    let getter_i64 = getter_name
                        .as_ref()
                        .and_then(|name| {
                            let registry_name = if *is_static {
                                crate::codegen::static_method_registry_key(name)
                            } else {
                                name.clone()
                            };
                            ctx.methods.get(&(class_name.clone(), registry_name))
                        })
                        .map(|llvm_name| {
                            let func_ref = format!("@{}", llvm_name);
                            ctx.block().ptrtoint(&func_ref, I64)
                        })
                        .unwrap_or_else(|| "0".to_string());
                    let setter_i64 = setter_name
                        .as_ref()
                        .and_then(|name| {
                            let registry_name = if *is_static {
                                crate::codegen::static_method_registry_key(name)
                            } else {
                                name.clone()
                            };
                            ctx.methods.get(&(class_name.clone(), registry_name))
                        })
                        .map(|llvm_name| {
                            let func_ref = format!("@{}", llvm_name);
                            ctx.block().ptrtoint(&func_ref, I64)
                        })
                        .unwrap_or_else(|| "0".to_string());
                    let cid_str = class_id.to_string();
                    let is_static_str = (*is_static as i64).to_string();
                    ctx.block().call_void(
                        "js_register_class_computed_accessor",
                        &[
                            (I64, &cid_str),
                            (DOUBLE, &key_v),
                            (I64, &getter_i64),
                            (I64, &setter_i64),
                            (I64, &is_static_str),
                        ],
                    );
                }
            }
            Ok(double_literal(f64::from_bits(0x7FFC_0000_0000_0001)))
        }
        // Issue #1772: per-evaluation class identity for a class expression.
        // Each evaluation allocates a real heap "class object" — a regular
        // object stamped with the compile-time template's `class_id` (so
        // static methods / `new` / instanceof dispatch through the existing
        // class_id machinery, no fresh hop, no method regression) and
        // carrying the per-evaluation static fields as its own properties.
        // So `make(a) !== make(b)` (distinct pointers), `make(a).ast` is an
        // own field, `make(a).pipe()` dispatches via class_id=template, and
        // the object is GC-traced + collected when unreachable (no leak).
        Expr::ClassExprFresh {
            template,
            named_statics,
            symbol_statics,
            captured_args,
        } => {
            let template_cid = ctx.class_ids.get(template).copied().unwrap_or(0);
            let tcid_str = template_cid.to_string();
            let nfields = named_statics.len().to_string();
            // Allocate with class_id = template; set_field_by_name below
            // performs the keys-array transition for the named statics.
            let obj =
                ctx.block()
                    .call(I64, "js_object_alloc", &[(I32, &tcid_str), (I32, &nfields)]);
            // #1789: mark it as a class object (object_type = OBJECT_TYPE_CLASS)
            // so `typeof` reports "function" and `new`/`instanceof` read the
            // class_id from this object rather than treating it as an instance.
            ctx.block()
                .call_void("js_object_mark_class", &[(I64, &obj)]);
            // #6438: pin THIS evaluation's parent onto the object. The lowering
            // sequences `RegisterClassParentDynamic` immediately ahead of this
            // node, so `CLASS_DYNAMIC_PARENT_VALUE[template]` still holds this
            // evaluation's parent; later evaluations overwrite it, but each
            // object keeps its own edge. Without this, a factory invoked more
            // than once (effect's `class DeclareClass extends make(ast) { … }`)
            // has every instance walk to the LAST parent — reading that
            // evaluation's `static ast` instead of its own. No-op when the class
            // expression has no heritage.
            ctx.block().call_void(
                "js_class_object_pin_parent",
                &[(I64, &obj), (I32, &tcid_str)],
            );
            for (name, init) in named_statics {
                let key_idx = ctx.strings.intern(name);
                let key_handle_global = format!("@{}", ctx.strings.entry(key_idx).handle_global);
                let v = lower_expr(ctx, init)?;
                let blk = ctx.block();
                let key_box = blk.load(DOUBLE, &key_handle_global);
                let key_bits = blk.bitcast_double_to_i64(&key_box);
                let key_raw = blk.and(I64, &key_bits, crate::nanbox::POINTER_MASK_I64);
                blk.call_void(
                    "js_object_set_field_by_name",
                    &[(I64, &obj), (I64, &key_raw), (DOUBLE, &v)],
                );
            }
            // #1787: snapshot the captured outer-scope values onto the class
            // object as the `__perry_ctor_caps` own array (in the constructor's
            // capture-param order). `new <thisClassObjectValue>()` reads it back
            // in `js_new_function_construct` and replays the constructor with the
            // right captured environment — which the static `new ClassName()`
            // inlining can't do once the class escapes its defining scope.
            if !captured_args.is_empty() {
                let cap_len = captured_args.len().to_string();
                let mut lowered_caps: Vec<String> = Vec::with_capacity(captured_args.len());
                let mut caps_arr = ctx.block().call(I64, "js_array_alloc", &[(I32, &cap_len)]);
                // #6523: these capture loads are Perry-internal materialization
                // at the class's DEFINITION site, same as the
                // `RegisterClassCaptures` snapshot loads above (#6052). A
                // captured `const` declared AFTER the class (bundled semver's
                // `class Comparator` + trailing debug/require consts) is still
                // in its dead zone here — legal JS, since TDZ applies at
                // method-call time. Without the suppression window the checked
                // box read threw "Cannot access undefined before
                // initialization" while merely DEFINING the class. Suppressed
                // loads snapshot `undefined`; the #6037 refresh statements
                // re-register the live values right after each captured
                // binding's initializer runs.
                ctx.block().call_void("js_tdz_suppress_begin", &[]);
                for arg in captured_args {
                    let v = lower_expr(ctx, arg)?;
                    caps_arr = ctx.block().call(
                        I64,
                        "js_array_push_f64",
                        &[(I64, &caps_arr), (DOUBLE, &v)],
                    );
                    lowered_caps.push(v);
                }
                ctx.block().call_void("js_tdz_suppress_end", &[]);
                let caps_box = nanbox_pointer_inline(ctx.block(), &caps_arr);
                let key_idx = ctx.strings.intern("__perry_ctor_caps");
                let key_handle_global = format!("@{}", ctx.strings.entry(key_idx).handle_global);
                let blk = ctx.block();
                let key_box = blk.load(DOUBLE, &key_handle_global);
                let key_bits = blk.bitcast_double_to_i64(&key_box);
                let key_raw = blk.and(I64, &key_bits, crate::nanbox::POINTER_MASK_I64);
                blk.call_void(
                    "js_object_set_field_by_name",
                    &[(I64, &obj), (I64, &key_raw), (DOUBLE, &caps_box)],
                );
                // #685: ALSO register this evaluation's captures as the
                // template's CLASS_CAPTURE_VALUES snapshot. The static blocks
                // invoked below run compiled static-method bodies whose
                // enclosing-scope reads resolve through
                // `js_param_or_class_capture_value` — i.e. the name-keyed
                // snapshot — not `__perry_ctor_caps` (that array only feeds
                // constructor replay). Same write-right-before-use pattern
                // (and same documented per-evaluation overwrite limitation)
                // as the shared-template path's `RegisterClassCaptures`.
                if template_cid != 0 {
                    let n = lowered_caps.len();
                    let buf = ctx.func.alloca_entry_array(DOUBLE, n);
                    for (i, v) in lowered_caps.iter().enumerate() {
                        let slot =
                            ctx.block()
                                .gep(DOUBLE, &buf, &[(crate::types::I64, &i.to_string())]);
                        ctx.block().store(DOUBLE, v, &slot);
                    }
                    let ptr_reg = ctx.block().next_reg();
                    ctx.block().emit_raw(format!(
                        "{} = getelementptr [{} x double], ptr {}, i64 0, i64 0",
                        ptr_reg, n, buf
                    ));
                    let len_str = n.to_string();
                    ctx.block().call_void(
                        "js_class_register_capture_values",
                        &[
                            (crate::types::I32, &tcid_str),
                            (crate::types::PTR, &ptr_reg),
                            (crate::types::I64, &len_str),
                        ],
                    );
                }
            }
            let obj_box = nanbox_pointer_inline(ctx.block(), &obj);
            for (key, init) in symbol_statics {
                let k = lower_expr(ctx, key)?;
                let v = lower_expr(ctx, init)?;
                ctx.block().call(
                    DOUBLE,
                    "js_object_set_symbol_property",
                    &[(DOUBLE, &obj_box), (DOUBLE, &k), (DOUBLE, &v)],
                );
            }
            // #685: run the class's `static { … }` blocks NOW — at the class
            // expression's evaluation, with `this` = THIS fresh class object.
            // The `ClassExprFresh` fast path previously never invoked them
            // (they are also skipped by the module-init fallback when another
            // evaluation site invokes them inline), so `return class { static
            // { this.viaBlock = tag } }` factories produced objects whose
            // blocks simply never ran. Arm the one-shot static-`this`
            // override before each call so the compiled body's
            // `js_static_this_resolve` prologue binds `this` to the fresh
            // object (writes land as own properties of this evaluation's
            // object, not the shared template). Blocks run after the named
            // static fields above — the source interleaving of fields and
            // blocks is not reproduced on this path (pre-existing limitation).
            let block_fns: Vec<String> = ctx
                .classes
                .get(template)
                .map(|c| {
                    c.static_methods
                        .iter()
                        .filter(|m| m.name.starts_with("__perry_static_init_"))
                        .filter_map(|m| {
                            ctx.methods
                                .get(&(
                                    template.clone(),
                                    crate::codegen::static_method_registry_key(&m.name),
                                ))
                                .cloned()
                        })
                        .collect()
                })
                .unwrap_or_default();
            for fn_name in block_fns {
                ctx.block()
                    .call_void("js_static_this_arm_value", &[(DOUBLE, &obj_box)]);
                ctx.block().call(DOUBLE, &fn_name, &[]);
            }
            Ok(obj_box)
        }
        // Issue #711 part 2: `<expr>.prototype = <expr>` pattern.
        // Calls `js_set_function_prototype(func, proto)`, which (when
        // func is a closure and proto is an object) allocates a
        // synthetic class id and binds the proto object as that
        // class's vtable source. Method dispatch later consults
        // CLASS_PROTOTYPE_OBJECTS to resolve methods.
        Expr::SetFunctionPrototype { func, proto } => {
            let func_val = lower_expr(ctx, func)?;
            let proto_val = lower_expr(ctx, proto)?;
            // Discard the returned synthetic class id — it's stored in
            // the runtime side-table keyed by func_val and consulted
            // later by `js_register_class_parent_dynamic`. User code
            // gets the assigned value (proto_val) as the expression
            // result, matching JS semantics for `x.foo = bar`.
            let _ = ctx.block().call(
                crate::types::I32,
                "js_set_function_prototype",
                &[(DOUBLE, &func_val), (DOUBLE, &proto_val)],
            );
            Ok(proto_val)
        }
        // Link a generator/async-generator instance into the spec prototype
        // chain. Closure bodies can use their own closure pointer to preserve
        // `Object.getPrototypeOf(g()) === g.prototype`; direct function bodies
        // fall back to the generic two-hop intrinsic linker and call sites may
        // relink with a concrete closure when they have one.
        Expr::LinkGeneratorPrototype { obj, is_async } => {
            let obj_val = lower_expr(ctx, obj)?;
            if let Some(closure_ptr) = ctx.current_closure_ptr.clone() {
                return Ok(ctx.block().call(
                    DOUBLE,
                    "js_generator_attach_closure_prototype",
                    &[(DOUBLE, &obj_val), (crate::types::I64, &closure_ptr)],
                ));
            }
            let is_async_str = if *is_async { "1" } else { "0" };
            Ok(ctx.block().call(
                DOUBLE,
                "js_generator_attach_prototype",
                &[(DOUBLE, &obj_val), (crate::types::I32, is_async_str)],
            ))
        }
        // Issue #838: `<Class>.prototype.<method> = <fn>` and the
        // aliased `let p = <Class>.prototype; p.<method> = <fn>`
        // shape. HIR recognises the assignment pattern and lowers it
        // here; codegen emits `js_register_prototype_method(class_id,
        // name_ptr, name_len, value)` so the runtime stores the
        // closure into a per-class side-table consulted at dispatch
        // time. The expression yields the closure value to match
        // JS-spec `x.foo = bar`. If the class isn't in
        // `ctx.class_ids` (cross-module imported class) we fall back
        // to a generic field-set on `<Class>.prototype` so the value
        // at least lands on the prototype proxy — the importer side
        // typically owns the registration anyway.
        Expr::RegisterPrototypeMethod {
            class_name,
            method_name,
            value,
        } => {
            let val_double = lower_expr(ctx, value)?;
            if let Some(&class_id) = ctx.class_ids.get(class_name) {
                let key_idx = ctx.strings.intern(method_name);
                let key_bytes_global = format!("@{}", ctx.strings.entry(key_idx).bytes_global);
                let key_len = ctx.strings.entry(key_idx).byte_len.to_string();
                ctx.block().call_void(
                    "js_register_prototype_method",
                    &[
                        (crate::types::I32, &class_id.to_string()),
                        (PTR, &key_bytes_global),
                        (I64, &key_len),
                        (DOUBLE, &val_double),
                    ],
                );
            }
            Ok(val_double)
        }
        // Issue #838 followup (b): the prototype's owner is a function
        // declaration (Babel's `var Foo = function(){ function Foo(){…};
        // Foo.prototype.x = fn; return Foo; }()`, also dayjs's minified
        // form). Hand both the closure value and the method name to the
        // runtime helper — it allocates (or reuses) a synthetic class id
        // keyed by the closure's NaN-boxed bits and stores the method
        // on `CLASS_PROTOTYPE_METHODS[synthetic_cid]`. The paired
        // `new <FuncRef>(args)` lowering below stamps the same id on
        // the instance so dispatch finds the method via the regular
        // `(*obj).class_id` walk.
        Expr::RegisterFunctionPrototypeMethod {
            func,
            method_name,
            value,
        } => {
            let func_double = lower_expr(ctx, func)?;
            let val_double = lower_expr(ctx, value)?;
            let key_idx = ctx.strings.intern(method_name);
            let key_bytes_global = format!("@{}", ctx.strings.entry(key_idx).bytes_global);
            let key_len = ctx.strings.entry(key_idx).byte_len.to_string();
            let _ = ctx.block().call(
                crate::types::I32,
                "js_register_function_prototype_method",
                &[
                    (DOUBLE, &func_double),
                    (PTR, &key_bytes_global),
                    (I64, &key_len),
                    (DOUBLE, &val_double),
                ],
            );
            Ok(val_double)
        }
        // Read side of #838 followup (b): `<funcDecl>.prototype.<name>`
        // (Ident or computed-string-literal form) lowered into a direct
        // lookup of the prototype-method side-table. Returns the closure
        // value stored at registration time, or `undefined` if no method
        // by that name was registered. Pre-fix this would fall through
        // to a generic PropertyGet on a `Function.prototype` object that
        // never materialised (so the read was always `undefined`,
        // making `typeof Foo.prototype.method` come back `'undefined'`
        // even though `(new Foo()).method` correctly reached the
        // registered closure via the dispatch path).
        Expr::GetFunctionPrototypeMethod { func, method_name } => {
            let func_double = lower_expr(ctx, func)?;
            let key_idx = ctx.strings.intern(method_name);
            let key_bytes_global = format!("@{}", ctx.strings.entry(key_idx).bytes_global);
            let key_len = ctx.strings.entry(key_idx).byte_len.to_string();
            Ok(ctx.block().call(
                DOUBLE,
                "js_get_function_prototype_method",
                &[
                    (DOUBLE, &func_double),
                    (PTR, &key_bytes_global),
                    (I64, &key_len),
                ],
            ))
        }
        // `static [Symbol.for("k")] = "v"` — register in the runtime's
        // class-static-symbol side table. Refs #420 (drizzle).
        Expr::ClassStaticSymbolSet {
            class_name,
            key,
            value,
        } => {
            let key_v = lower_expr(ctx, key)?;
            let val_v = lower_expr(ctx, value)?;
            if let Some(&class_id) = ctx.class_ids.get(class_name) {
                let cid_str = class_id.to_string();
                ctx.block().call_void(
                    "js_class_register_static_symbol",
                    &[
                        (crate::types::I32, &cid_str),
                        (DOUBLE, &key_v),
                        (DOUBLE, &val_v),
                    ],
                );
            }
            Ok(val_v)
        }
        // Issue #894: when `NativeModuleRef` reaches this fallback path
        // (i.e. its parent isn't one of the dedicated fast-paths above —
        // for example it's the *return value* of a CJS-wrap synthesized
        // `require()` call, then stashed in a local and member-accessed
        // later: `const { EventEmitter } = require('node:events')` →
        // `Let { id: 6, init: Call(require, "node:events") }` followed by
        // `Let { id: 7, init: PropertyGet { LocalGet(6), "EventEmitter" } }`),
        // pre-fix the value lowered to the literal `0.0`. `0.0` is plain
        // f64 zero (not the NaN-boxed `undefined` tag), so a subsequent
        // `PropertyGet { LocalGet(6), "X" }` slow-pathed into
        // `js_object_get_field_by_name` with a null receiver and returned
        // `undefined` — and a then-chained `PropertyGet { LocalGet(7),
        // "prototype" }` on that `undefined` tripped the spec
        // "Cannot read properties of undefined (reading 'prototype')"
        // throw (pino's `lib/proto.js` exact shape).
        //
        // Materialize a real NATIVE_MODULE_CLASS_ID-tagged ObjectHeader
        // here so the downstream property-by-name path routes through the
        // namespace's NATIVE_MODULE_CLASS_ID arm in
        // `js_object_get_field_by_name` — that consults
        // `get_native_module_constant` and `is_native_module_callable_export`
        // exactly as the direct-NativeModuleRef fast path does. The two
        // paths now converge: `import * as fs from "node:fs"; fs.constants`
        // (direct AST shape, fast-path at line 3615) and `const fs =
        // require("node:fs"); fs.constants` (call-result shape, fallback
        // path here) both produce a real namespace object.
        Expr::NativeModuleRef(name) => {
            let mod_idx = ctx.strings.intern(name);
            let mod_bytes_global = format!("@{}", ctx.strings.entry(mod_idx).bytes_global);
            let mod_len_str = name.len().to_string();
            // Devirt: register this module's dispatch bucket before the namespace
            // exists, so method calls route to it. Sole compile-time ref to the
            // bucket's handlers — unimported modules dead-strip.
            let install_sym = crate::nm_install::nm_install_symbol(name);
            let blk = ctx.block();
            if let Some(s) = install_sym {
                blk.call_void(s, &[]);
            }
            Ok(blk.call(
                DOUBLE,
                "js_create_native_module_namespace",
                &[(PTR, &mod_bytes_global), (I64, &mod_len_str)],
            ))
        }

        // ObjectRest is the `...rest` capture in destructuring:
        // `const { a, b, ...rest } = obj` — `rest` must be a clone of
        // `obj` with keys `a`/`b` stripped. We build an exclude-keys
        // array of NaN-boxed strings and call `js_object_rest`, which
        // returns a fresh object pointer that we re-NaN-box.
        _ => unreachable!("expr/mod.rs dispatched a variant not handled by this submodule"),
    }
}
