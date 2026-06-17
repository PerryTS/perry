//! SuperMethodCall / SuperPropertyGet / FsReadFileBinary.
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
        Expr::SuperMethodCall { method, args } => {
            // Find the current class from the class_stack.
            let Some(current_class_name) = ctx.class_stack.last().cloned() else {
                // No enclosing class — fall back to stub.
                for a in args {
                    let _ = lower_expr(ctx, a)?;
                }
                return Ok(double_literal(0.0));
            };
            // Walk parent chain starting from extends_name.
            let mut parent = ctx
                .classes
                .get(&current_class_name)
                .and_then(|c| c.extends_name.clone());
            let mut resolved_fn: Option<String> = None;
            while let Some(p) = parent {
                let key = (p.clone(), method.clone());
                if let Some(fname) = ctx.methods.get(&key).cloned() {
                    resolved_fn = Some(fname);
                    break;
                }
                parent = ctx.classes.get(&p).and_then(|c| c.extends_name.clone());
            }
            let Some(fn_name) = resolved_fn else {
                // Static resolution failed. For a class with a DYNAMIC parent
                // (`class X extends _mod.default` — the interop ESM
                // default-export base, wall 38/42), `extends_name` is "default"
                // and never resolves to a compile-time class, so the chain walk
                // above finds nothing. Dispatch `super.method(...)` at runtime
                // via the registered parent edge instead of returning the bogus
                // numeric `0.0` (which made `super.getRequestHandler()` in
                // Next.js's `NextNodeServer.makeRequestHandler` yield a number,
                // and the handler it built threw "value is not a function").
                let has_dyn_parent = ctx
                    .classes
                    .get(&current_class_name)
                    .map(|c| c.extends_expr.is_some())
                    .unwrap_or(false);
                let cid = ctx.class_ids.get(&current_class_name).copied().unwrap_or(0);
                if has_dyn_parent && cid != 0 {
                    let this_box = match ctx.this_stack.last().cloned() {
                        Some(slot) => ctx.block().load(DOUBLE, &slot),
                        None => double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED)),
                    };
                    let mut lowered_args: Vec<String> = Vec::with_capacity(args.len());
                    for a in args {
                        lowered_args.push(lower_expr(ctx, a)?);
                    }
                    let (args_ptr, args_len) = if lowered_args.is_empty() {
                        ("null".to_string(), "0".to_string())
                    } else {
                        let n = lowered_args.len();
                        let buf = ctx.func.alloca_entry_array(DOUBLE, n);
                        for (i, v) in lowered_args.iter().enumerate() {
                            let slot = ctx.block().gep(DOUBLE, &buf, &[(I64, &i.to_string())]);
                            ctx.block().store(DOUBLE, v, &slot);
                        }
                        let ptr_reg = ctx.block().next_reg();
                        ctx.block().emit_raw(format!(
                            "{} = getelementptr [{} x double], ptr {}, i64 0, i64 0",
                            ptr_reg, n, buf
                        ));
                        (ptr_reg, n.to_string())
                    };
                    let name_global = emit_string_literal_global(ctx, method);
                    return Ok(ctx.block().call(
                        DOUBLE,
                        "js_super_method_call_dynamic",
                        &[
                            (I32, &cid.to_string()),
                            (PTR, &name_global),
                            (I64, &method.len().to_string()),
                            (DOUBLE, &this_box),
                            (PTR, &args_ptr),
                            (I64, &args_len),
                        ],
                    ));
                }
                for a in args {
                    let _ = lower_expr(ctx, a)?;
                }
                return Ok(double_literal(0.0));
            };
            // Lower `this` (from this_stack) + args.
            let this_slot = ctx
                .this_stack
                .last()
                .cloned()
                .ok_or_else(|| anyhow!("super.{}() outside any method body", method))?;
            let this_box = ctx.block().load(DOUBLE, &this_slot);
            let mut lowered: Vec<String> = Vec::with_capacity(args.len() + 1);
            lowered.push(this_box);
            for a in args {
                lowered.push(lower_expr(ctx, a)?);
            }
            let arg_slices: Vec<(crate::types::LlvmType, &str)> =
                lowered.iter().map(|s| (DOUBLE, s.as_str())).collect();
            Ok(ctx.block().call(DOUBLE, &fn_name, &arg_slices))
        }

        // -------- super.<prop> as a value (issue #774) --------
        // Walk the parent-class chain. If a parent declares a method
        // with the requested name, materialize it as a closure value
        // via the singleton wrapper (mirroring `Expr::FuncRef`).
        // Otherwise return `undefined` — which is the strict-JS
        // result for instance-field shadows like:
        //
        //     class A { foo = "A"; }
        //     class B extends A { foo = "B"; m() { return super.foo; } }
        //
        // The previous lowering rewrote `super.foo` to `this.foo`, so
        // it silently returned the child override ("B") instead of
        // `undefined`. See issue #774 / PR #774 follow-up.
        //
        // Call-form `super.method(...)` never reaches this arm — it
        // is lowered to `Expr::SuperMethodCall` in lower_call.rs.
        Expr::SuperPropertyGet { property } => {
            let undef = double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED));
            let Some(current_class_name) = ctx.class_stack.last().cloned() else {
                return Ok(undef);
            };
            let immediate_parent = ctx
                .classes
                .get(&current_class_name)
                .and_then(|c| c.extends_name.clone());
            let mut parent = immediate_parent.clone();
            let mut resolved_fn: Option<String> = None;
            while let Some(p) = parent {
                let key = (p.clone(), property.clone());
                if let Some(fname) = ctx.methods.get(&key).cloned() {
                    resolved_fn = Some(fname);
                    break;
                }
                parent = ctx.classes.get(&p).and_then(|c| c.extends_name.clone());
            }
            let Some(fn_name) = resolved_fn else {
                // Not a method on the parent chain — `super.prop` then reads the
                // property off the parent prototype with `this` as receiver:
                // an accessor (getter) is INVOKED, a data property
                // (`B.prototype.x = 42`) is returned. Route to the runtime,
                // which walks the parent class chain. Refs
                // class/super/in-{constructor,getter,methods,setter}.
                let parent_cid = immediate_parent
                    .as_ref()
                    .and_then(|p| ctx.class_ids.get(p))
                    .copied()
                    .unwrap_or(0);
                if parent_cid == 0 {
                    return Ok(undef);
                }
                let recv_v = if let Some(this_slot) = ctx.this_stack.last().cloned() {
                    ctx.block().load(DOUBLE, &this_slot)
                } else {
                    let helper = if ctx.is_strict_fn {
                        "js_implicit_this_get"
                    } else {
                        "js_implicit_this_get_sloppy"
                    };
                    ctx.block().call(DOUBLE, helper, &[])
                };
                let key_idx = ctx.strings.intern(property);
                let key_handle_global = format!("@{}", ctx.strings.entry(key_idx).handle_global);
                let key_box = ctx.block().load(DOUBLE, &key_handle_global);
                let parent_cid_s = parent_cid.to_string();
                return Ok(ctx.block().call(
                    DOUBLE,
                    "js_super_accessor_get",
                    &[(I32, &parent_cid_s), (DOUBLE, &key_box), (DOUBLE, &recv_v)],
                ));
            };
            // Mirror Expr::FuncRef: route through the singleton wrapper
            // so callers can invoke via the closure-call ABI. The
            // `__perry_wrap_<fn>` symbol is emitted by compile_module
            // for every user function.
            //
            // #1126: when the resolved parent method lives in a DIFFERENT
            // module (typical with cross-module class inheritance —
            // rxjs's `OperatorSubscriber extends Subscriber` where
            // `super.complete` / `super._complete` refer to Subscriber.ts's
            // methods), the wrapper's `define` lives in the source TU but
            // is referenced from this consumer TU. LLVM per-TU IR
            // validation rejects the reference unless we forward-declare
            // the symbol. Push into `pending_declares` — `declare_function`
            // dedupes against any later same-TU `define` (`module.rs:67-69`
            // comment) so this is safe for the same-module case too. The
            // signature is informational only (runtime dispatches via
            // ClosureHeader's func_ptr); use the same `(i64)` + 0 doubles
            // shape the imported-function-ref site at `expr/mod.rs:12331`
            // uses for unknown-arity imports.
            let wrap_name = format!("__perry_wrap_{}", fn_name);
            ctx.pending_declares
                .push((wrap_name.clone(), DOUBLE, vec![I64]));
            let blk = ctx.block();
            let wrap_ptr = format!("@{}", wrap_name);
            let closure_handle = blk.call(I64, "js_closure_alloc_singleton", &[(PTR, &wrap_ptr)]);
            Ok(nanbox_pointer_inline(blk, &closure_handle))
        }

        Expr::SuperPropertySet {
            parent_class_id,
            parent_class_name,
            key,
            value,
        } => {
            let parent_cid = if *parent_class_id != 0 {
                *parent_class_id
            } else if let Some(parent_name) = parent_class_name {
                ctx.class_ids.get(parent_name).copied().unwrap_or(0)
            } else {
                let Some(current_class_name) = ctx.class_stack.last().cloned() else {
                    return Err(anyhow!("super property assignment outside any method body"));
                };
                ctx.classes
                    .get(&current_class_name)
                    .and_then(|c| c.extends_name.as_ref())
                    .and_then(|parent| ctx.class_ids.get(parent))
                    .copied()
                    .unwrap_or(0)
            };
            let recv_v = if let Some(this_slot) = ctx.this_stack.last().cloned() {
                ctx.block().load(DOUBLE, &this_slot)
            } else {
                let helper = if ctx.is_strict_fn {
                    "js_implicit_this_get"
                } else {
                    "js_implicit_this_get_sloppy"
                };
                ctx.block().call(DOUBLE, helper, &[])
            };
            let key_v = lower_expr(ctx, key)?;
            let value_v = lower_expr(ctx, value)?;
            let parent_cid_s = parent_cid.to_string();
            Ok(ctx.block().call(
                DOUBLE,
                "js_super_put_value_set",
                &[
                    (I32, &parent_cid_s),
                    (DOUBLE, &key_v),
                    (DOUBLE, &value_v),
                    (DOUBLE, &recv_v),
                    (I32, "1"),
                ],
            ))
        }

        Expr::ObjectSuperPropertyGet {
            home,
            key,
            receiver,
        } => {
            let home_v = lower_expr(ctx, home)?;
            let key_v = lower_expr(ctx, key)?;
            let recv_v = lower_expr(ctx, receiver)?;
            Ok(ctx.block().call(
                DOUBLE,
                "js_object_super_get",
                &[(DOUBLE, &home_v), (DOUBLE, &key_v), (DOUBLE, &recv_v)],
            ))
        }

        Expr::ObjectSuperPropertySet {
            home,
            key,
            value,
            receiver,
        } => {
            let home_v = lower_expr(ctx, home)?;
            let key_v = lower_expr(ctx, key)?;
            let value_v = lower_expr(ctx, value)?;
            let recv_v = lower_expr(ctx, receiver)?;
            Ok(ctx.block().call(
                DOUBLE,
                "js_object_super_put_value_set",
                &[
                    (DOUBLE, &home_v),
                    (DOUBLE, &key_v),
                    (DOUBLE, &value_v),
                    (DOUBLE, &recv_v),
                    (I32, "1"),
                ],
            ))
        }

        Expr::ObjectSuperMethodCall {
            home,
            key,
            receiver,
            args,
        } => {
            let home_v = lower_expr(ctx, home)?;
            let key_v = lower_expr(ctx, key)?;
            let recv_v = lower_expr(ctx, receiver)?;
            let mut lowered_args = Vec::with_capacity(args.len());
            for arg in args {
                lowered_args.push(lower_expr(ctx, arg)?);
            }
            let (args_ptr, args_len) = if lowered_args.is_empty() {
                ("null".to_string(), "0".to_string())
            } else {
                let buf = ctx.func.alloca_entry_array(DOUBLE, lowered_args.len());
                for (i, val) in lowered_args.iter().enumerate() {
                    let slot = ctx.block().gep(DOUBLE, &buf, &[(I64, &i.to_string())]);
                    ctx.block().store(DOUBLE, val, &slot);
                }
                let ptr_reg = ctx.block().next_reg();
                ctx.block().emit_raw(format!(
                    "{} = getelementptr [{} x double], ptr {}, i64 0, i64 0",
                    ptr_reg,
                    lowered_args.len(),
                    buf
                ));
                (ptr_reg, lowered_args.len().to_string())
            };
            Ok(ctx.block().call(
                DOUBLE,
                "js_object_super_call",
                &[
                    (DOUBLE, &home_v),
                    (DOUBLE, &key_v),
                    (DOUBLE, &recv_v),
                    (PTR, &args_ptr),
                    (I64, &args_len),
                ],
            ))
        }

        // -------- fs.readFileSync(path) -> Buffer (no encoding) --------
        // Node returns a Buffer when no encoding is supplied; mirror that.
        // js_fs_read_file_binary returns a raw *mut BufferHeader registered
        // in BUFFER_REGISTRY; NaN-box with POINTER_TAG so downstream
        // console.log / .toString / .length / .[i] dispatch consult the
        // registry and format the value as `<Buffer xx xx ...>` (or the
        // appropriate Buffer behaviour for each method).
        Expr::FsReadFileBinary(path) => {
            let path_box = lower_expr(ctx, path)?;
            let blk = ctx.block();
            let buf_handle = blk.call(I64, "js_fs_read_file_binary", &[(DOUBLE, &path_box)]);
            Ok(nanbox_pointer_inline(blk, &buf_handle))
        }

        // -------- instanceof --------
        // Look up the target class's id and call js_instanceof. The
        // runtime walks the object's class chain and returns a
        // NaN-tagged TAG_TRUE/TAG_FALSE double directly — no
        // conversion needed.
        _ => unreachable!("expr/mod.rs dispatched a variant not handled by this submodule"),
    }
}
