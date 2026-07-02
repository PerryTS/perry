//! Class instance method dispatch — the interface/dynamic dispatch tower and
//! the static-fallback + virtual-override tower.
//! Pure code move from `property_get.rs` — no behavior change.

use super::*;

use anyhow::Result;
use perry_hir::Expr;

use crate::expr::{lower_expr, nanbox_pointer_inline, nanbox_string_inline, unbox_to_i64, FnCtx};
use crate::lower_array_method::lower_array_method;
use crate::lower_string_method::{is_known_string_method_name, lower_string_method};
use crate::nanbox::double_literal;
use crate::type_analysis::{
    is_array_expr, is_global_constructor_expr, is_map_expr, is_native_module_dynamic_index,
    is_promise_expr, is_set_expr, is_string_expr, is_url_search_params_expr, receiver_class_name,
};
use crate::types::{DOUBLE, I32, I64};

// Reach the override-emit helpers (`pub(super)` of `lower_call`) by their
// canonical crate-relative path.
use crate::lower_call::method_override::{
    emit_guarded_direct_method_call, emit_own_method_override_check,
};

/// Interface / dynamic dispatch fallback: when the static class is unknown OR
/// resolves to an interface name not in the class registry, BUT the property
/// name corresponds to a method defined on at least one class in the registry,
/// emit a switch on class_id over all classes that have that method. Then the
/// static-fallback + virtual-override tower for typed-instance receivers.
///
/// `call_byte_offset` is this call's captured source offset (for the #5247
/// `js_set_call_location` emission before the runtime dispatch fallback).
pub(crate) fn try_lower_instance_method_call(
    ctx: &mut FnCtx<'_>,
    object: &Expr,
    property: &str,
    args: &[Expr],
    call_byte_offset: u32,
) -> Result<Option<String>> {
    // Skip dynamic dispatch when the receiver is GlobalGet (e.g.
    // `console.log`). GlobalGet is a module-level global object
    // (console, Math, JSON, etc.), not a class instance. Without
    // this guard, `console.log()` gets hijacked by the interface
    // dispatch tower when a user class happens to have a method
    // with the same name (like `SimpleLogger.log()`).
    let is_global = matches!(object, Expr::GlobalGet(_));
    // If the receiver's static type is a well-known built-in with its own
    // runtime method family (Buffer byte readers, Array, Map, Set, …),
    // don't enter the user-class dispatch tower. Otherwise an imported
    // user class that happens to declare the same method name (e.g. a
    // BufferCursor with `readUInt8`) would be enumerated as an
    // implementor and `buf.readUInt8(i)` would fall through to the
    // default 0.0 case when the Buffer's class id doesn't match any
    // tower entry.
    let is_builtin_receiver = match receiver_class_name(ctx, object) {
        Some(name) => matches!(
            name.as_str(),
            "Buffer"
                | "Uint8Array"
                | "Uint8ClampedArray"
                | "Int8Array"
                | "Int16Array"
                | "Uint16Array"
                | "Int32Array"
                | "Uint32Array"
                | "Float16Array"
                | "Float32Array"
                | "Float64Array"
                | "BigInt64Array"
                | "BigUint64Array"
                | "Array"
                | "ReadonlyArray"
                | "Map"
                | "ReadonlyMap"
                | "Set"
                | "ReadonlySet"
                | "WeakMap"
                | "WeakSet"
                | "Promise"
                | "RegExp"
                | "Date"
        ),
        None => false,
    };
    let needs_dynamic_dispatch = !is_global
        && !is_builtin_receiver
        && match receiver_class_name(ctx, object) {
            None => true,
            Some(name) => !ctx.classes.contains_key(&name),
        };
    if needs_dynamic_dispatch {
        // Find all (class_id → fn_name) for `property` — including
        // INHERITED methods. Per JS spec, `subInstance.method()` for a
        // method defined on a parent dispatches to the parent's
        // implementation. perry's previous walk only added classes that
        // DIRECTLY declared `property`; subclasses that inherited the
        // method weren't represented in the dispatch tower, so the
        // icmp_eq vs class_id missed and the call fell through to the
        // runtime's js_native_call_method fallback (which returns an
        // empty object for unknown receiver class+method combos).
        // Refs #420 — drizzle's `serial("id").primaryKey()` where
        // primaryKey is on ColumnBuilder (grandparent) but the
        // receiver is a PgSerialBuilder (grandchild).
        //
        // Algorithm: walk every class C in `class_ids`. For each, walk
        // C's parent chain and find the FIRST class that has `property`
        // in `ctx.methods`. Register (C's id → that ancestor's fn_name).
        let mut implementors: Vec<(u32, String)> = Vec::new();
        // #5437: (has_rest, decl_param_count) per implementor, aligned 1:1 with
        // `implementors`, so each case block can build its own per-arity args
        // without rescanning `ctx.methods`.
        let mut impl_meta: Vec<(bool, usize)> = Vec::new();
        let mut seen_pairs: std::collections::HashSet<(u32, String)> =
            std::collections::HashSet::new();
        for (start_cls, &start_cid) in ctx.class_ids.iter() {
            let mut cur: Option<String> = Some(start_cls.clone());
            while let Some(c) = cur {
                let key = (c.clone(), property.to_string());
                if let Some(fname) = ctx.methods.get(&key).cloned() {
                    if seen_pairs.insert((start_cid, fname.clone())) {
                        // `key` is the exact (defining-class, property) where the
                        // method resolved, so its arity metadata is available now.
                        let has_rest = matches!(ctx.method_has_rest.get(&key), Some(&true));
                        let decl = ctx.method_param_counts.get(&key).copied().unwrap_or(0);
                        implementors.push((start_cid, fname));
                        impl_meta.push((has_rest, decl));
                    }
                    break;
                }
                cur = ctx.classes.get(&c).and_then(|cc| cc.extends_name.clone());
            }
        }
        if !implementors.is_empty() {
            let recv_box = lower_expr(ctx, object)?;
            // #1758 / epic #1785: the raw user args (no `this`, no issue-#235
            // padding, no rest-bundling) drive every concrete callee below. A
            // `perry_static_*` implementor (a class-object value reaching this
            // instance-method tower — e.g. `class X extends
            // (make(...)).annotations(y) {}`) must dispatch through
            // `js_class_static_method_call`, which binds `this` and applies
            // static arity/rest semantics; the instance-style `fname(recv,
            // args…)` direct call would pass recv as arg0 and never set
            // IMPLICIT_THIS (the #1787 broken-tower bug).
            let mut static_user_args: Vec<String> = Vec::with_capacity(args.len());
            for a in args {
                static_user_args.push(lower_expr(ctx, a)?);
            }
            // #5391 path 4: oversized modules full-outline the class-id switch
            // tower. The tower emits one icmp + case block per class implementing
            // `property` (scaling __text with implementor count) whose default arm
            // is already `js_native_call_method`; collapse the whole switch to that
            // same by-name runtime dispatch (which resolves the user method via its
            // (class_id, name) vtable registry). The own-property override probe is
            // preserved inside the collapsed helper. Skipped when any implementor is
            // a `perry_static_*` class-object-static method, which needs
            // `js_class_static_method_call` and is NOT reproduced by the by-name
            // dispatcher. Mirrors the GET/SET/array-literal full-outline paths.
            if crate::codegen::full_outline_ic_enabled()
                && method_dispatch_collapse_enabled()
                && implementors
                    .iter()
                    .all(|(_, f)| !f.starts_with("perry_static_"))
            {
                let v = emit_collapsed_instance_dispatch(
                    ctx,
                    &recv_box,
                    property,
                    &static_user_args,
                    call_byte_offset,
                    /* with_override_probe */ true,
                )?;
                return Ok(Some(v));
            }
            // #5437: each implementor of `property` has its OWN declared arity
            // and rest-ness. The rest-bundle (and default-param padding) MUST be
            // applied per-implementor, not once globally — otherwise a single
            // rest-bearing implementor forces EVERY case (including non-rest
            // ones with more positional params) to receive a single bundled rest
            // array, dropping the real positional args. That was the Next.js
            // `f.get(r,u,context)` bug: `get` has rest- and non-rest impls
            // (`LRUCache.get`/`CacheHandler.get`/`ResponseCache.get`, arities
            // 1/2/3), so the global rest-bundle truncated `nh.get`'s 3 args into
            // one array passed as arg0 → `context` (the 3rd param) read 0.0.
            //
            // (has_rest, decl_param_count) per implementor was built in the
            // discovery loop above (`impl_meta`, aligned 1:1 with `implementors`);
            // each case block builds its own per-arity args below.
            let undefined_lit = double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED));

            // Issue #628 followup (#620 in dynamic-dispatch shape): probe
            // own-property override BEFORE the class-id switch tower. The
            // tower hard-codes the static method body for each known
            // class id; when a user mutates `this.method = X` inside
            // a method body (hono's SmartRouter rebinds itself on first
            // call), the second call's dispatch must invoke the stored
            // override, not the original method. The static-class fast
            // path got this in v0.5.716 (#620). The dynamic-dispatch
            // path needs the parallel fix.
            let key_idx_probe = ctx.strings.intern(property);
            let probe_entry = ctx.strings.entry(key_idx_probe);
            let probe_bytes_global = format!("@{}", probe_entry.bytes_global);
            let probe_name_len_str = probe_entry.byte_len.to_string();
            let own_method_probe = ctx.block().call(
                DOUBLE,
                "js_object_get_own_field_or_undef",
                &[
                    (DOUBLE, &recv_box),
                    (crate::types::PTR, &probe_bytes_global),
                    (I64, &probe_name_len_str),
                ],
            );
            let own_bits_probe = ctx.block().bitcast_double_to_i64(&own_method_probe);
            let undef_bits_str = format!("{}", crate::nanbox::TAG_UNDEFINED as i64);
            let is_undef_probe = ctx.block().icmp_eq(I64, &own_bits_probe, &undef_bits_str);
            let probe_override_idx = ctx.new_block("idisp.override");
            let probe_dispatch_idx = ctx.new_block("idisp.dispatch");
            let probe_outer_merge_idx = ctx.new_block("idisp.outer_merge");
            let probe_override_label = ctx.block_label(probe_override_idx);
            let probe_dispatch_label = ctx.block_label(probe_dispatch_idx);
            let probe_outer_merge_label = ctx.block_label(probe_outer_merge_idx);
            ctx.block().cond_br(
                &is_undef_probe,
                &probe_dispatch_label,
                &probe_override_label,
            );

            // Override path: pack user args (skip recv at slot 0) and
            // invoke via js_native_call_value. The stored value is
            // typically an arrow function or `.bind()` closure whose
            // `this` is captured/bound, so we don't pass the receiver
            // as an extra arg — matches the static-class fast path's
            // contract.
            //
            // Use `static_user_args` (the raw user args captured before
            // rest-bundling / issue-#235 padding mutated `lowered_args`).
            // The override target runs its own rest-bundling at call time
            // (via `js_native_call_value` → closure-call dispatch), so it
            // must receive the un-bundled args — the same fix as the
            // default branch below for #321 / regression from #2162.
            ctx.current_block = probe_override_idx;
            let user_arg_count_probe = static_user_args.len();
            let (probe_args_ptr, probe_args_len_str) = if user_arg_count_probe == 0 {
                ("null".to_string(), "0".to_string())
            } else {
                let buf_reg = ctx.func.alloca_entry_array(DOUBLE, user_arg_count_probe);
                for (i, a_val) in static_user_args.iter().enumerate() {
                    let slot = ctx
                        .block()
                        .gep(DOUBLE, &buf_reg, &[(I64, &format!("{}", i))]);
                    ctx.block().store(DOUBLE, a_val, &slot);
                }
                let ptr_reg = ctx.block().next_reg();
                ctx.block().emit_raw(format!(
                    "{} = getelementptr [{} x double], ptr {}, i64 0, i64 0",
                    ptr_reg, user_arg_count_probe, buf_reg
                ));
                (ptr_reg, user_arg_count_probe.to_string())
            };
            // Issue #632: bind IMPLICIT_THIS to the receiver around
            // the override call. The stored function may be a class
            // field assigning a non-arrow function (`class X { match
            // = match; }` — hono RegExpRouter — where the imported
            // `match` body reads `this.buildAllMatchers()`). Without
            // the bind, the body sees stale IMPLICIT_THIS and reads
            // garbage. Mirrors `lower_call.rs:2607` for the closure-
            // call fallthrough pattern (#519).
            let recv_for_this_probe = recv_box.clone();
            let prev_this_probe = ctx.block().call(
                DOUBLE,
                "js_implicit_this_set",
                &[(DOUBLE, &recv_for_this_probe)],
            );
            let v_override_probe = ctx.block().call(
                DOUBLE,
                "js_native_call_value",
                &[
                    (DOUBLE, &own_method_probe),
                    (crate::types::PTR, &probe_args_ptr),
                    (I64, &probe_args_len_str),
                ],
            );
            ctx.block().call(
                DOUBLE,
                "js_implicit_this_set",
                &[(DOUBLE, &prev_this_probe)],
            );
            let after_override_probe = ctx.block().label.clone();
            if !ctx.block().is_terminated() {
                ctx.block().br(&probe_outer_merge_label);
            }

            // Dispatch path: existing class-id switch tower.
            // Pre-create the tower blocks first so the POINTER_TAG guard below
            // can branch the non-pointer (primitive) case to the default
            // (runtime-dispatch) fallback.
            let mut case_idxs: Vec<usize> = Vec::with_capacity(implementors.len());
            for (i, _) in implementors.iter().enumerate() {
                case_idxs.push(ctx.new_block(&format!("idispatch.case{}", i)));
            }
            let default_idx = ctx.new_block("idispatch.default");
            let merge_idx = ctx.new_block("idispatch.merge");
            let merge_label = ctx.block_label(merge_idx);

            // A class-id switch is only valid for a real heap object
            // (POINTER_TAG = 0x7FFD). For a non-pointer receiver — a string
            // (STRING_TAG 0x7FFF / SSO), number, boolean — `unbox_to_i64` would
            // mask garbage low-48 bits that `js_object_get_class_id` ->
            // `is_valid_obj_ptr` can false-positive and dereference, crashing
            // with EXC_BAD_ACCESS. Gate the tower on a POINTER_TAG check and send
            // any non-pointer receiver to `idispatch.default`, whose
            // `js_native_call_method` path dispatches primitives via the runtime.
            // Idiom mirrors `expr/class_field_inline_guard.rs` (lshr 48; icmp eq
            // 0x7FFD).
            ctx.current_block = probe_dispatch_idx;
            let tower_idx = ctx.new_block("idispatch.tower");
            let tower_label = ctx.block_label(tower_idx);
            let default_label_guard = ctx.block_label(default_idx);
            {
                let blk = ctx.block();
                let recv_bits = blk.bitcast_double_to_i64(&recv_box);
                let recv_tag = blk.lshr(I64, &recv_bits, "48");
                let is_ptr = blk.icmp_eq(I64, &recv_tag, "32765"); // 0x7FFD POINTER_TAG
                blk.cond_br(&is_ptr, &tower_label, &default_label_guard);
            }

            // Tower of icmp+br: each implementor's case calls
            // its concrete method, default returns 0.0 (the
            // closure-call fallback would also handle this but
            // returning a sentinel is cheaper).
            ctx.current_block = tower_idx;
            let blk = ctx.block();
            let recv_handle = unbox_to_i64(blk, &recv_box);
            let cid = blk.call(I32, "js_object_get_class_id", &[(I64, &recv_handle)]);

            for (i, (case_cid, _)) in implementors.iter().enumerate() {
                let case_label = ctx.block_label(case_idxs[i]);
                let cmp = ctx.block().icmp_eq(I32, &cid, &case_cid.to_string());
                if i + 1 < implementors.len() {
                    let next_idx = ctx.new_block(&format!("idispatch.test{}", i + 1));
                    let next_lbl = ctx.block_label(next_idx);
                    ctx.block().cond_br(&cmp, &case_label, &next_lbl);
                    ctx.current_block = next_idx;
                } else {
                    let default_label = ctx.block_label(default_idx);
                    ctx.block().cond_br(&cmp, &case_label, &default_label);
                }
            }

            let mut phi_inputs: Vec<(String, String)> = Vec::new();
            for (((_, fname), &case_idx), &(impl_has_rest, impl_decl_count)) in implementors
                .iter()
                .zip(case_idxs.iter())
                .zip(impl_meta.iter())
            {
                ctx.current_block = case_idx;
                // #1758: a `perry_static_*` implementor is a STATIC method on a
                // class-object receiver. Route it through the runtime
                // `js_class_static_method_call` (binds `this`, walks the
                // class_id parent chain, applies static arity/rest) instead of
                // the instance-style direct call, which would pass recv as
                // arg0 and leave `this` unset (#1787 broken-tower behavior).
                let v = if fname.starts_with("perry_static_") {
                    let n = static_user_args.len();
                    let (sa_ptr, sa_len) = if n == 0 {
                        ("null".to_string(), "0".to_string())
                    } else {
                        let buf_reg = ctx.func.alloca_entry_array(DOUBLE, n);
                        for (i, a_val) in static_user_args.iter().enumerate() {
                            let slot =
                                ctx.block()
                                    .gep(DOUBLE, &buf_reg, &[(I64, &format!("{}", i))]);
                            ctx.block().store(DOUBLE, a_val, &slot);
                        }
                        let ptr_reg = ctx.block().next_reg();
                        ctx.block().emit_raw(format!(
                            "{} = getelementptr [{} x double], ptr {}, i64 0, i64 0",
                            ptr_reg, n, buf_reg
                        ));
                        (ptr_reg, n.to_string())
                    };
                    let name_ptr_i64 = ctx.block().ptrtoint(&probe_bytes_global, I64);
                    ctx.block().call(
                        DOUBLE,
                        "js_class_static_method_call",
                        &[
                            (DOUBLE, &recv_box),
                            (I64, &name_ptr_i64),
                            (I64, &probe_name_len_str),
                            (crate::types::PTR, &sa_ptr),
                            (I64, &sa_len),
                        ],
                    )
                } else {
                    // #5437: build THIS implementor's args from the raw user
                    // args (`static_user_args`), applying its own declared arity
                    // + rest-ness. A non-rest callee gets its positional params
                    // padded with `undefined`; a rest callee gets the trailing
                    // args bundled into a single array at its rest slot. This is
                    // per-case so one rest-bearing sibling can't force the others
                    // to receive a bundled array in place of positional params.
                    let mut case_args: Vec<String> = Vec::with_capacity(impl_decl_count + 1);
                    case_args.push(recv_box.clone());
                    if impl_has_rest {
                        let fixed_user = impl_decl_count.saturating_sub(1);
                        for i in 0..fixed_user {
                            case_args.push(
                                static_user_args
                                    .get(i)
                                    .cloned()
                                    .unwrap_or_else(|| undefined_lit.clone()),
                            );
                        }
                        let rest_count = static_user_args.len().saturating_sub(fixed_user);
                        let cap = (rest_count as u32).to_string();
                        let mut rest_arr = ctx.block().call(I64, "js_array_alloc", &[(I32, &cap)]);
                        for v in static_user_args.iter().skip(fixed_user) {
                            let blk = ctx.block();
                            rest_arr = blk.call(
                                I64,
                                "js_array_push_f64",
                                &[(I64, &rest_arr), (DOUBLE, v)],
                            );
                        }
                        let rest_box = nanbox_pointer_inline(ctx.block(), &rest_arr);
                        case_args.push(rest_box);
                    } else {
                        for v in &static_user_args {
                            case_args.push(v.clone());
                        }
                        // Issue #235: pad to the declared arity so the callee's
                        // default-param desugaring fires for skipped trailing
                        // params instead of reading an uninitialized arg slot.
                        while case_args.len() < impl_decl_count + 1 {
                            case_args.push(undefined_lit.clone());
                        }
                    }
                    let case_arg_slices: Vec<(crate::types::LlvmType, &str)> =
                        case_args.iter().map(|s| (DOUBLE, s.as_str())).collect();
                    ctx.block().call(DOUBLE, fname, &case_arg_slices)
                };
                let after_label = ctx.block().label.clone();
                if !ctx.block().is_terminated() {
                    ctx.block().br(&merge_label);
                }
                phi_inputs.push((v, after_label));
            }
            // Default branch: receiver's class id didn't match any user
            // class implementing `property`. Rather than returning 0.0,
            // fall through to the runtime's `js_native_call_method` so
            // same-named built-in methods (Buffer.readUInt8, Array.push,
            // Map.get, …) still reach their native dispatch. Without
            // this, a `buf.readUInt8(i)` call site ends up in the
            // default branch and returns 0, silently corrupting reads
            // any time a user class in scope happens to declare a
            // method of the same name.
            ctx.current_block = default_idx;
            let key_idx = ctx.strings.intern(property);
            let entry = ctx.strings.entry(key_idx);
            let key_handle_global = format!("@{}", entry.handle_global);
            let key_box = ctx.block().load(DOUBLE, &key_handle_global);
            let key_bits = ctx.block().bitcast_double_to_i64(&key_box);
            let method_id = ctx
                .block()
                .and(I64, &key_bits, crate::nanbox::POINTER_MASK_I64);
            let (fb_args_ptr, fb_args_len) = if static_user_args.is_empty() {
                ("null".to_string(), "0".to_string())
            } else {
                // Hoist the args-array alloca to the function entry
                // block — see issue #167 and `alloca_entry_array` doc.
                //
                // Use `static_user_args` (the raw user-provided args captured
                // before rest-bundling / issue-#235 padding mutated
                // `lowered_args`). The `js_native_call_method` fallback path
                // performs its own rest-bundling at runtime, so it must
                // receive the un-bundled args. Pre-fix this read from the
                // post-bundling `lowered_args`, which on a rest-bearing
                // dispatch (e.g. `obj.pipe(c1, c2, c3)` post-#2162 where
                // `pipe()` now has a synthesized `...arguments` rest) had
                // already been truncated+rest_box'd to `[recv, rest_arr]`.
                // The old code then alloca'd `[args.len() x double]`, stored
                // only the rest_arr into slot 0, and told the runtime to
                // read `args.len()` doubles — slots 1..N-1 were uninit
                // garbage that landed in pipeArguments's `arguments[i]`,
                // tripping `value is not a function` (#321 regression from
                // #2162; effect-barrel-init crash).
                let n = static_user_args.len();
                let buf_reg = ctx.func.alloca_entry_array(DOUBLE, n);
                for (i, a_val) in static_user_args.iter().enumerate() {
                    let slot = ctx
                        .block()
                        .gep(DOUBLE, &buf_reg, &[(I64, &format!("{}", i))]);
                    ctx.block().store(DOUBLE, a_val, &slot);
                }
                let ptr_reg = ctx.block().next_reg();
                ctx.block().emit_raw(format!(
                    "{} = getelementptr [{} x double], ptr {}, i64 0, i64 0",
                    ptr_reg, n, buf_reg
                ));
                (ptr_reg, n.to_string())
            };
            // #5247: record the source location of this call right before the
            // dynamic dispatch, so the runtime "X is not a function" /
            // "(kind).method is not a function" TypeError this fallback may
            // throw carries `at <file>:<line>`. Args are already lowered, so a
            // nested-call argument's location no longer shadows this one.
            crate::expr::calls::emit_call_location_at(ctx, call_byte_offset);
            let v_def = ctx.block().call(
                DOUBLE,
                "js_native_call_method_by_id",
                &[
                    (DOUBLE, &recv_box),
                    (I64, &method_id),
                    (crate::types::PTR, &fb_args_ptr),
                    (I64, &fb_args_len),
                ],
            );
            let def_label = ctx.block().label.clone();
            ctx.block().br(&merge_label);
            phi_inputs.push((v_def, def_label));

            ctx.current_block = merge_idx;
            let phi_args: Vec<(&str, &str)> = phi_inputs
                .iter()
                .map(|(v, l)| (v.as_str(), l.as_str()))
                .collect();
            let v_dispatch_phi = ctx.block().phi(DOUBLE, &phi_args);
            let after_dispatch_phi = ctx.block().label.clone();
            if !ctx.block().is_terminated() {
                ctx.block().br(&probe_outer_merge_label);
            }

            // Outer merge: phi over override and dispatch values.
            ctx.current_block = probe_outer_merge_idx;
            return Ok(Some(ctx.block().phi(
                DOUBLE,
                &[
                    (v_override_probe.as_str(), after_override_probe.as_str()),
                    (v_dispatch_phi.as_str(), after_dispatch_phi.as_str()),
                ],
            )));
        }
    }

    if let Some(class_name) = receiver_class_name(ctx, object) {
        // Step 1: walk parent chain for the static method name.
        let mut static_fn: Option<String> = None;
        let mut current_class = Some(class_name.clone());
        while let Some(cur) = current_class {
            let key = (cur.clone(), property.to_string());
            if let Some(fname) = ctx.methods.get(&key).cloned() {
                static_fn = Some(fname);
                break;
            }
            current_class = ctx.classes.get(&cur).and_then(|c| c.extends_name.clone());
        }

        if let Some(fallback_fn) = static_fn {
            // Step 2: collect overriding subclasses. For each
            // subclass C transitively extending class_name, look
            // up which method C uses for `property` (walking C's
            // parent chain). If that resolves to a different
            // function than the static fallback, C needs an
            // explicit case in the dispatch table.
            let mut overrides: Vec<(u32, String)> = Vec::new();
            for (sub_name, &sub_id) in ctx.class_ids.iter() {
                if *sub_name == class_name {
                    continue;
                }
                // Is sub_name transitively a subclass of class_name?
                let mut parent = ctx
                    .classes
                    .get(sub_name)
                    .and_then(|c| c.extends_name.clone());
                let mut is_subclass = false;
                while let Some(p) = parent {
                    if p == class_name {
                        is_subclass = true;
                        break;
                    }
                    parent = ctx.classes.get(&p).and_then(|c| c.extends_name.clone());
                }
                if !is_subclass {
                    continue;
                }
                // Resolve the method for sub_name by walking its
                // own parent chain (NOT class_name's chain).
                let mut cur = Some(sub_name.clone());
                let mut sub_fn: Option<String> = None;
                while let Some(c) = cur {
                    let key = (c.clone(), property.to_string());
                    if let Some(fname) = ctx.methods.get(&key).cloned() {
                        sub_fn = Some(fname);
                        break;
                    }
                    cur = ctx.classes.get(&c).and_then(|c| c.extends_name.clone());
                }
                if let Some(sub_fn) = sub_fn {
                    if sub_fn != fallback_fn {
                        overrides.push((sub_id, sub_fn));
                    }
                }
            }

            let recv_box = lower_expr(ctx, object)?;
            let mut fallback_user_args: Vec<String> = Vec::with_capacity(args.len());
            for a in args {
                fallback_user_args.push(lower_expr(ctx, a)?);
            }
            // #5391 path 4 (virtual tower): eligibility to collapse the
            // per-overriding-subclass class-id switch to a single by-name
            // dispatch (see the two return sites below). Requires overrides
            // (the empty case is the compact override-check path) and no
            // `perry_static_*` implementor (those need
            // `js_class_static_method_call`). Computed up front so the
            // rest-bearing case can return BEFORE the rest-array bundling below,
            // which would otherwise materialize a dead js_array for the collapsed
            // path (the by-name dispatch takes the raw, un-bundled args).
            let can_collapse_virtual = crate::codegen::full_outline_ic_enabled()
                && method_dispatch_collapse_enabled()
                && !overrides.is_empty()
                && !fallback_fn.starts_with("perry_static_")
                && overrides
                    .iter()
                    .all(|(_, f)| !f.starts_with("perry_static_"));
            let mut lowered_args: Vec<String> = Vec::with_capacity(fallback_user_args.len() + 1);
            lowered_args.push(recv_box.clone());
            lowered_args.extend(fallback_user_args.iter().cloned());
            // Issue #235: pad lowered_args with TAG_UNDEFINED so the
            // callee's default-param desugaring fires when the call site
            // passed fewer args than the method declares. Same approach
            // and reasoning as the dynamic-dispatch branch above —
            // applied here for the static-dispatch + virtual-override
            // case (receiver class IS in `ctx.classes`).
            //
            // Walk the parent chain `static_fn` was resolved through to
            // find the fallback's arity; take max across all overrides
            // so the unified arg_slices works for every concrete callee.
            let mut max_explicit_arity: usize = 0;
            let mut walk = Some(class_name.clone());
            while let Some(cur) = walk {
                let key = (cur.clone(), property.to_string());
                if let Some(&n) = ctx.method_param_counts.get(&key) {
                    if n > max_explicit_arity {
                        max_explicit_arity = n;
                    }
                    break;
                }
                walk = ctx.classes.get(&cur).and_then(|c| c.extends_name.clone());
            }
            for (sub_id, _) in &overrides {
                for (sub_name, &id) in ctx.class_ids.iter() {
                    if id == *sub_id {
                        if let Some(&n) = ctx
                            .method_param_counts
                            .get(&(sub_name.clone(), property.to_string()))
                        {
                            if n > max_explicit_arity {
                                max_explicit_arity = n;
                            }
                        }
                        break;
                    }
                }
            }
            // Closes #484: bundle trailing user args into a rest
            // array when the method has a `...rest` parameter.
            // Walk the same parent chain to find has_rest. Same
            // structural shape as the freestanding-function rest
            // bundling at lower_call.rs:444 — but operates on
            // `lowered_args` after the receiver was prepended.
            let mut method_has_rest = false;
            let mut method_decl_count = max_explicit_arity;
            let mut rest_walk = Some(class_name.clone());
            while let Some(cur) = rest_walk {
                let key = (cur.clone(), property.to_string());
                if let Some(&true) = ctx.method_has_rest.get(&key) {
                    method_has_rest = true;
                    method_decl_count = ctx
                        .method_param_counts
                        .get(&key)
                        .copied()
                        .unwrap_or(max_explicit_arity);
                    break;
                }
                rest_walk = ctx.classes.get(&cur).and_then(|c| c.extends_name.clone());
            }
            // Collapse a rest-bearing virtual dispatch HERE, before the rest
            // array is materialized below — the by-name dispatch takes the raw
            // `fallback_user_args` and does its own rest-bundling, so the bundle
            // would be dead. (The non-rest collapse happens at the vdispatch
            // site below, where there is no array to skip.)
            if method_has_rest && can_collapse_virtual {
                return Ok(Some(emit_collapsed_instance_dispatch(
                    ctx,
                    &recv_box,
                    property,
                    &fallback_user_args,
                    call_byte_offset,
                    /* with_override_probe */ false,
                )?));
            }
            let undefined_lit = double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED));
            if method_has_rest {
                // user-visible fixed param count = decl - 1 (the
                // last param is the rest). lowered_args[0] is
                // `this`, [1..] are user args.
                let fixed_user = method_decl_count.saturating_sub(1);
                // Pad missing fixed args first.
                while lowered_args.len() - 1 < fixed_user {
                    lowered_args.push(undefined_lit.clone());
                }
                // Bundle remaining trailing args into a fresh
                // js_array. Index in lowered_args: 1 + fixed_user.
                let split_at = 1 + fixed_user;
                let rest_count = lowered_args.len().saturating_sub(split_at);
                let cap = (rest_count as u32).to_string();
                let mut rest_arr = ctx.block().call(I64, "js_array_alloc", &[(I32, &cap)]);
                for v in &lowered_args[split_at..] {
                    let blk = ctx.block();
                    rest_arr = blk.call(I64, "js_array_push_f64", &[(I64, &rest_arr), (DOUBLE, v)]);
                }
                let rest_box = nanbox_pointer_inline(ctx.block(), &rest_arr);
                lowered_args.truncate(split_at);
                lowered_args.push(rest_box);
            } else {
                let target_total = max_explicit_arity + 1; // +1 for `this`
                while lowered_args.len() < target_total {
                    lowered_args.push(undefined_lit.clone());
                }
            }
            let arg_slices: Vec<(crate::types::LlvmType, &str)> =
                lowered_args.iter().map(|s| (DOUBLE, s.as_str())).collect();

            if !method_has_rest {
                let typed_method_key = (class_name.clone(), property.to_string());
                let typed_formal_count = ctx
                    .method_param_counts
                    .get(&typed_method_key)
                    .copied()
                    .unwrap_or(max_explicit_arity);
                let typed_receiver_info = ctx.classes.get(&class_name).and_then(|class| {
                    let class = *class;
                    class
                        .methods
                        .iter()
                        .find(|method| method.name.as_str() == property)
                        .and_then(|method| {
                            crate::codegen::typed_f64_receiver_method_info(class, method)
                        })
                });
                let typed_receiver_direct_name = if typed_receiver_info.is_some()
                    && ctx
                        .methods
                        .get(&typed_method_key)
                        .is_some_and(|name| name == &fallback_fn)
                    && args.len() == typed_formal_count
                    && args
                        .iter()
                        .all(|arg| crate::type_analysis::is_numeric_expr(ctx, arg))
                {
                    Some(crate::codegen::typed_f64_receiver_method_name(&fallback_fn))
                } else {
                    None
                };
                let shape_only_guard = typed_receiver_direct_name.is_none()
                    && !class_chain_has_field_named(ctx, &class_name, property);
                let typed_direct_name = if ctx.typed_f64_methods.contains(&typed_method_key)
                    && ctx
                        .methods
                        .get(&typed_method_key)
                        .is_some_and(|name| name == &fallback_fn)
                    && ctx
                        .typed_i1_method_param_reps
                        .get(&typed_method_key)
                        .is_some_and(|reps| {
                            crate::codegen::typed_param_reps_match_args(ctx, reps, args)
                        }) {
                    Some(crate::codegen::typed_f64_method_name(&fallback_fn))
                } else {
                    None
                };
                let typed_i32_direct_name = if ctx.typed_i32_methods.contains(&typed_method_key)
                    && ctx
                        .methods
                        .get(&typed_method_key)
                        .is_some_and(|name| name == &fallback_fn)
                    && ctx
                        .typed_i1_method_param_reps
                        .get(&typed_method_key)
                        .is_some_and(|reps| {
                            crate::codegen::typed_param_reps_match_args(ctx, reps, args)
                        }) {
                    Some(crate::codegen::typed_i32_method_name(&fallback_fn))
                } else {
                    None
                };
                let typed_i1_direct_name = if ctx.typed_i1_methods.contains(&typed_method_key)
                    && ctx
                        .methods
                        .get(&typed_method_key)
                        .is_some_and(|name| name == &fallback_fn)
                    && ctx
                        .typed_i1_method_param_reps
                        .get(&typed_method_key)
                        .is_some_and(|reps| {
                            crate::codegen::typed_param_reps_match_args(ctx, reps, args)
                        }) {
                    Some(crate::codegen::typed_i1_method_name(&fallback_fn))
                } else {
                    None
                };
                let typed_string_direct_name = if ctx
                    .typed_string_methods
                    .contains(&typed_method_key)
                    && ctx
                        .methods
                        .get(&typed_method_key)
                        .is_some_and(|name| name == &fallback_fn)
                    && ctx
                        .typed_i1_method_param_reps
                        .get(&typed_method_key)
                        .is_some_and(|reps| {
                            crate::codegen::typed_param_reps_match_args(ctx, reps, args)
                        }) {
                    Some(crate::codegen::typed_string_method_name(&fallback_fn))
                } else {
                    None
                };
                let typed_direct = typed_direct_name.as_ref().and_then(|name| {
                    ctx.typed_i1_method_param_reps
                        .get(&typed_method_key)
                        .cloned()
                        .map(|reps| (name.as_str(), reps))
                });
                let typed_receiver_direct = match (
                    typed_receiver_direct_name.as_ref(),
                    typed_receiver_info.as_ref(),
                ) {
                    (Some(name), Some(info)) => Some((name.as_str(), typed_formal_count, info)),
                    _ => None,
                };
                let typed_i32_direct = typed_i32_direct_name.as_ref().and_then(|name| {
                    ctx.typed_i1_method_param_reps
                        .get(&typed_method_key)
                        .cloned()
                        .map(|reps| (name.as_str(), reps))
                });
                let typed_i1_direct = typed_i1_direct_name.as_ref().and_then(|name| {
                    ctx.typed_i1_method_param_reps
                        .get(&typed_method_key)
                        .cloned()
                        .map(|reps| (name.as_str(), reps))
                });
                let typed_string_direct = typed_string_direct_name.as_ref().and_then(|name| {
                    ctx.typed_i1_method_param_reps
                        .get(&typed_method_key)
                        .cloned()
                        .map(|reps| (name.as_str(), reps))
                });
                if let Some(guarded) = emit_guarded_direct_method_call(
                    ctx,
                    &recv_box,
                    &class_name,
                    property,
                    &fallback_fn,
                    &arg_slices,
                    &fallback_user_args,
                    typed_direct,
                    typed_receiver_direct,
                    typed_i32_direct,
                    typed_i1_direct,
                    typed_string_direct,
                    shape_only_guard,
                ) {
                    return Ok(Some(guarded));
                }
            }

            if overrides.is_empty() {
                // Issue #620: before falling through to the static method,
                // check whether the receiver has an own-property override
                // for `property` (set via `this.method = X` inside the
                // class). Hono's SmartRouter rebinds `this.match` on the
                // first call so subsequent calls go through the bound
                // fast-path closure instead of the original method.
                // The override branch dispatches a dynamic value (arrow / bound
                // / native method) via `js_native_call_value`, which does its
                // own arity/rest handling from a FLAT positional buffer. Pass
                // the un-rest-bundled user args (`fallback_user_args`) — not the
                // rest-bundled `lowered_args[1..]`, which would deliver the rest
                // array as one positional argument and break a native override
                // such as `super.emit(event, ...args)` forwarding to
                // EventEmitter (#620 / rest-spread-to-native-override).
                return Ok(Some(emit_own_method_override_check(
                    ctx,
                    &recv_box,
                    property,
                    &fallback_fn,
                    &arg_slices,
                    &recv_box,
                    &fallback_user_args,
                )));
            }

            // #5391 path 4 (virtual tower): collapse the per-overriding-subclass
            // class-id switch below to a single by-name dispatch, which resolves
            // the same override through the runtime's (class_id, name) vtable
            // registry. This switch — unlike the dynamic tower — has no
            // own-property override probe, so the collapse is a bare
            // `js_native_call_method` (with_override_probe = false) to stay
            // behavior-identical. The rest-bearing case already collapsed above
            // (before the rest bundling); this handles the non-rest case.
            if can_collapse_virtual {
                return Ok(Some(emit_collapsed_instance_dispatch(
                    ctx,
                    &recv_box,
                    property,
                    &fallback_user_args,
                    call_byte_offset,
                    /* with_override_probe */ false,
                )?));
            }

            // Step 4: virtual dispatch via class_id switch.
            // Read class_id from the object header, then branch
            // to the right concrete method block.
            // Pre-create blocks: one per override + default + merge.
            let mut case_idxs: Vec<usize> = Vec::with_capacity(overrides.len());
            for (i, _) in overrides.iter().enumerate() {
                case_idxs.push(ctx.new_block(&format!("vdispatch.case{}", i)));
            }
            let default_idx = ctx.new_block("vdispatch.default");
            let merge_idx = ctx.new_block("vdispatch.merge");

            // POINTER_TAG (0x7FFD) guard: a receiver statically typed as a class
            // can still hold a primitive at runtime (TS type-vs-value drift). For
            // a non-pointer receiver, masking + `js_object_get_class_id` derefs
            // garbage and crashes (EXC_BAD_ACCESS). Route non-pointers to the
            // static fallback (`vdispatch.default`). Same idiom as
            // `expr/class_field_inline_guard.rs` (lshr 48; icmp eq 0x7FFD).
            let tower_idx = ctx.new_block("vdispatch.tower");
            let tower_label = ctx.block_label(tower_idx);
            let default_label_guard = ctx.block_label(default_idx);
            {
                let blk = ctx.block();
                let recv_bits = blk.bitcast_double_to_i64(&recv_box);
                let recv_tag = blk.lshr(I64, &recv_bits, "48");
                let is_ptr = blk.icmp_eq(I64, &recv_tag, "32765"); // 0x7FFD POINTER_TAG
                blk.cond_br(&is_ptr, &tower_label, &default_label_guard);
            }

            ctx.current_block = tower_idx;
            let blk = ctx.block();
            let recv_handle = unbox_to_i64(blk, &recv_box);
            let cid = blk.call(I32, "js_object_get_class_id", &[(I64, &recv_handle)]);

            // Default → fallback. We use a tower of icmp+br rather
            // than the LLVM `switch` instruction (which the IR
            // builder doesn't expose generically) — same shape,
            // slightly more verbose.
            let mut current_label = ctx.block().label.clone();
            for (i, (case_cid, _)) in overrides.iter().enumerate() {
                let next_label = if i + 1 < overrides.len() {
                    // We'll start the next test in this same block
                    // — actually use a fresh block for the test.
                    format!("vdispatch.test{}", i + 1)
                } else {
                    ctx.block_label(default_idx)
                };
                let case_label = ctx.block_label(case_idxs[i]);
                // Make sure ctx.current_block points at the
                // current test block.
                let _ = current_label;
                let cmp = ctx.block().icmp_eq(I32, &cid, &case_cid.to_string());
                if i + 1 < overrides.len() {
                    // Create the next test block as a fresh block
                    // and branch into it on the false arm.
                    let next_idx = ctx.new_block(&format!("vdispatch.test{}", i + 1));
                    let next_lbl = ctx.block_label(next_idx);
                    ctx.block().cond_br(&cmp, &case_label, &next_lbl);
                    ctx.current_block = next_idx;
                    current_label = next_lbl;
                } else {
                    ctx.block().cond_br(&cmp, &case_label, &next_label);
                }
            }

            // Each case block: call the override and branch to merge.
            let merge_label = ctx.block_label(merge_idx);
            let mut phi_inputs: Vec<(String, String)> = Vec::new();
            for ((_, fname), &case_idx) in overrides.iter().zip(case_idxs.iter()) {
                ctx.current_block = case_idx;
                let v = ctx.block().call(DOUBLE, fname, &arg_slices);
                let after_label = ctx.block().label.clone();
                if !ctx.block().is_terminated() {
                    ctx.block().br(&merge_label);
                }
                phi_inputs.push((v, after_label));
            }

            // Default block: call the static fallback.
            ctx.current_block = default_idx;
            let v_def = ctx.block().call(DOUBLE, &fallback_fn, &arg_slices);
            let def_label = ctx.block().label.clone();
            if !ctx.block().is_terminated() {
                ctx.block().br(&merge_label);
            }
            phi_inputs.push((v_def, def_label));

            // Merge: phi over all incoming case results.
            ctx.current_block = merge_idx;
            let phi_args: Vec<(&str, &str)> = phi_inputs
                .iter()
                .map(|(v, l)| (v.as_str(), l.as_str()))
                .collect();
            return Ok(Some(ctx.block().phi(DOUBLE, &phi_args)));
        }
    }
    Ok(None)
}

/// Whether to collapse the instance method-dispatch tower in full-outline mode.
/// On by default whenever full-outline is active; `PERRY_OUTLINE_METHOD_DISPATCH=0`
/// / `off` / `false` keeps the inline class-id switch tower (escape hatch /
/// differential-test isolation).
fn method_dispatch_collapse_enabled() -> bool {
    !matches!(
        std::env::var("PERRY_OUTLINE_METHOD_DISPATCH").as_deref(),
        Ok("0") | Ok("off") | Ok("false")
    )
}

/// #5391 path 4: full-outlined collapse of the instance method-dispatch tower
/// (see the call site in `try_lower_instance_method_call`).
///
/// Emits the own-property override probe (unchanged semantics) and, on the
/// non-override path, a SINGLE by-name `js_native_call_method` instead of the
/// per-implementor class-id switch tower. `js_native_call_method` is the tower's
/// own default arm and resolves the user-class method through its (class_id,
/// name) vtable registry, so behavior is preserved while the per-site IR shrinks
/// from ~6 + N blocks (N = implementor count) to a fixed 3 blocks. The raw user
/// args are marshalled once into an entry-block array and shared by both arms;
/// `js_native_call_method` / `js_native_call_value` apply their own arity / rest
/// adaptation at runtime (the same contract the tower's default + override arms
/// already rely on).
fn emit_collapsed_instance_dispatch(
    ctx: &mut FnCtx<'_>,
    recv_box: &str,
    property: &str,
    static_user_args: &[String],
    call_byte_offset: u32,
    with_override_probe: bool,
) -> Result<String> {
    let key_idx = ctx.strings.intern(property);
    let entry = ctx.strings.entry(key_idx);
    let bytes_global = format!("@{}", entry.bytes_global);
    let name_len_str = entry.byte_len.to_string();

    // Marshal the raw user args into an entry-block array once; both arms pass
    // the same flat (ptr, len). `js_native_call_value` (override) and
    // `js_native_call_method` (dispatch) each do their own rest-bundling /
    // arity padding at runtime, so the un-bundled args are correct for both.
    let n = static_user_args.len();
    let (args_ptr, args_len) = if n == 0 {
        ("null".to_string(), "0".to_string())
    } else {
        let buf = ctx.func.alloca_entry_array(DOUBLE, n);
        for (i, a) in static_user_args.iter().enumerate() {
            let slot = ctx.block().gep(DOUBLE, &buf, &[(I64, &format!("{}", i))]);
            ctx.block().store(DOUBLE, a, &slot);
        }
        let ptr = ctx.block().next_reg();
        ctx.block().emit_raw(format!(
            "{} = getelementptr [{} x double], ptr {}, i64 0, i64 0",
            ptr, n, buf
        ));
        (ptr, n.to_string())
    };

    // The virtual-dispatch switch this collapses (overriding-subclass case) has
    // NO own-property override probe, so its collapse must be a bare by-name
    // dispatch to stay behavior-identical; the dynamic-dispatch tower DOES probe
    // first, so its collapse keeps the probe. `with_override_probe` selects.
    // `js_native_call_method` handles a non-pointer (primitive) receiver at
    // runtime, so no codegen POINTER_TAG guard is needed on this path.
    if !with_override_probe {
        crate::expr::calls::emit_call_location_at(ctx, call_byte_offset);
        return Ok(ctx.block().call(
            DOUBLE,
            "js_native_call_method",
            &[
                (DOUBLE, recv_box),
                (crate::types::PTR, &bytes_global),
                (I64, &name_len_str),
                (crate::types::PTR, &args_ptr),
                (I64, &args_len),
            ],
        ));
    }

    // Override probe: an own-property method override (e.g. hono SmartRouter
    // rebinding `this.method = X`) wins over the class method.
    let own_method = ctx.block().call(
        DOUBLE,
        "js_object_get_own_field_or_undef",
        &[
            (DOUBLE, recv_box),
            (crate::types::PTR, &bytes_global),
            (I64, &name_len_str),
        ],
    );
    let own_bits = ctx.block().bitcast_double_to_i64(&own_method);
    let undef_bits_str = format!("{}", crate::nanbox::TAG_UNDEFINED as i64);
    let is_undef = ctx.block().icmp_eq(I64, &own_bits, &undef_bits_str);
    let override_idx = ctx.new_block("idispc.override");
    let dispatch_idx = ctx.new_block("idispc.dispatch");
    let merge_idx = ctx.new_block("idispc.merge");
    let override_label = ctx.block_label(override_idx);
    let dispatch_label = ctx.block_label(dispatch_idx);
    let merge_label = ctx.block_label(merge_idx);
    ctx.block()
        .cond_br(&is_undef, &dispatch_label, &override_label);

    // Override arm: bind IMPLICIT_THIS to the receiver and call the stored
    // function value (#632 — a class-field non-arrow function reads `this`).
    ctx.current_block = override_idx;
    let prev_this = ctx
        .block()
        .call(DOUBLE, "js_implicit_this_set", &[(DOUBLE, recv_box)]);
    let v_override = ctx.block().call(
        DOUBLE,
        "js_native_call_value",
        &[
            (DOUBLE, &own_method),
            (crate::types::PTR, &args_ptr),
            (I64, &args_len),
        ],
    );
    ctx.block()
        .call(DOUBLE, "js_implicit_this_set", &[(DOUBLE, &prev_this)]);
    let after_override = ctx.block().label.clone();
    if !ctx.block().is_terminated() {
        ctx.block().br(&merge_label);
    }

    // Dispatch arm: one by-name runtime dispatch (replaces the class-id tower).
    ctx.current_block = dispatch_idx;
    // #5247: record the call location so a runtime "X is not a function" carries
    // `at <file>:<line>`.
    crate::expr::calls::emit_call_location_at(ctx, call_byte_offset);
    let v_dispatch = ctx.block().call(
        DOUBLE,
        "js_native_call_method",
        &[
            (DOUBLE, recv_box),
            (crate::types::PTR, &bytes_global),
            (I64, &name_len_str),
            (crate::types::PTR, &args_ptr),
            (I64, &args_len),
        ],
    );
    let after_dispatch = ctx.block().label.clone();
    if !ctx.block().is_terminated() {
        ctx.block().br(&merge_label);
    }

    ctx.current_block = merge_idx;
    Ok(ctx.block().phi(
        DOUBLE,
        &[
            (v_override.as_str(), after_override.as_str()),
            (v_dispatch.as_str(), after_dispatch.as_str()),
        ],
    ))
}
