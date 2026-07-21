//! SuperMethodCall / SuperPropertyGet / FsReadFileBinary.
//!
//! Extracted from `expr/mod.rs` to keep that file under the 2000-line cap.
//! Pure mechanical move — match arm bodies are verbatim copies, called from
//! `lower_expr`'s outer dispatch.

use anyhow::{anyhow, Result};
use perry_hir::Expr;

use crate::nanbox::double_literal;
use crate::types::{DOUBLE, I32, I64, PTR};

use super::{emit_string_literal_global, lower_expr, nanbox_pointer_inline, FnCtx};

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
                // Compile-time resolution failed (the parent has no INSTANCE
                // method of this name). This happens for (1) a DYNAMIC parent
                // (`class X extends _mod.default` — the interop ESM default base,
                // wall 38/42) whose `extends_name` never resolves to a known
                // class, and (2) a `super.m()` inside a `static` method, where
                // the target is the parent's STATIC method (not in the instance
                // tables walked above). Both are handled by the runtime helper,
                // which walks the registered parent edge and — when `this` is a
                // ClassRef — resolves the parent's static method. Routing here
                // beats the bogus numeric `0.0` ("value is not a function").
                let cid = ctx.class_ids.get(&current_class_name).copied().unwrap_or(0);
                if cid != 0 {
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

        // -------- super.method(...spread) --------
        // The plain `SuperMethodCall` arm passes each argument positionally, so
        // a `...rest` spread would be delivered as ONE array argument. When the
        // resolved super target is a NATIVE base (EventEmitter.prototype.emit
        // and friends) forwarding a rest param via `super.emit(event, ...args)`
        // delivered `[payload]` to the listener instead of `payload`. Flatten
        // every argument (regular + spread-expanded) into a single JS array,
        // then dispatch through `js_super_method_call_dynamic_apply` — the
        // runtime helper materialises the array into a flat f64 buffer and
        // forwards to the same parent-chain resolution used by the non-spread
        // dynamic path (which handles both native-prototype and user-class
        // JS parents).
        Expr::SuperMethodCallSpread { method, args } => {
            use perry_hir::CallArg;
            let Some(current_class_name) = ctx.class_stack.last().cloned() else {
                for a in args {
                    match a {
                        CallArg::Expr(e) | CallArg::Spread(e) => {
                            let _ = lower_expr(ctx, e)?;
                        }
                    }
                }
                return Ok(double_literal(0.0));
            };
            let cid = ctx.class_ids.get(&current_class_name).copied().unwrap_or(0);
            if cid == 0 {
                for a in args {
                    match a {
                        CallArg::Expr(e) | CallArg::Spread(e) => {
                            let _ = lower_expr(ctx, e)?;
                        }
                    }
                }
                return Ok(double_literal(0.0));
            }
            let this_box = match ctx.this_stack.last().cloned() {
                Some(slot) => ctx.block().load(DOUBLE, &slot),
                None => double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED)),
            };
            // Build a single args array containing every argument in source
            // order, expanding spreads via array-like-to-array + concat (the
            // same machinery the CallSpread method-apply path uses).
            let mut acc_handle = ctx.block().call(I64, "js_array_alloc", &[(I32, "0")]);
            for a in args {
                match a {
                    CallArg::Expr(e) => {
                        let v = lower_expr(ctx, e)?;
                        acc_handle = ctx.block().call(
                            I64,
                            "js_array_push_f64",
                            &[(I64, &acc_handle), (DOUBLE, &v)],
                        );
                    }
                    CallArg::Spread(e) => {
                        let part_box = lower_expr(ctx, e)?;
                        let part_handle =
                            ctx.block()
                                .call(I64, "js_array_like_to_array", &[(DOUBLE, &part_box)]);
                        acc_handle = ctx.block().call(
                            I64,
                            "js_array_concat",
                            &[(I64, &acc_handle), (I64, &part_handle)],
                        );
                    }
                }
            }
            let args_array = nanbox_pointer_inline(ctx.block(), &acc_handle);
            let name_global = emit_string_literal_global(ctx, method);
            Ok(ctx.block().call(
                DOUBLE,
                "js_super_method_call_dynamic_apply",
                &[
                    (I32, &cid.to_string()),
                    (PTR, &name_global),
                    (I64, &method.len().to_string()),
                    (DOUBLE, &this_box),
                    (DOUBLE, &args_array),
                ],
            ))
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
