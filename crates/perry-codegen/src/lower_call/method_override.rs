//! Issue #620 own-method-override runtime check.
//!
//! Extracted from `lower_call.rs` (#1099, part of #1097) — pure move,
//! no behavior change. `emit_own_method_override_check` emits a runtime
//! guard before a static class-method dispatch so a `this.method = X`
//! own-property override (or `class X { method = fn; }`) is honored.

use crate::expr::{
    emit_typed_feedback_register_site, i32_bool_to_nanbox, i32_to_nanbox, FnCtx,
    TypedFeedbackContract, TypedFeedbackKind,
};
use crate::nanbox::double_literal;
use crate::native_value::LoweredValue;
use crate::types::{DOUBLE, I1, I32, I64};

fn typed_i1_method_signature_note(reps: &[crate::codegen::TypedParamRep]) -> String {
    let first = reps.first().map(|rep| rep.label()).unwrap_or("void");
    if reps.len() <= 1 {
        format!("typed_signature=i1({first})->i1")
    } else {
        format!("typed_signature=i1({first}, ...)->i1")
    }
}

fn typed_i32_method_signature_note(arg_count: usize) -> String {
    if arg_count <= 1 {
        "typed_signature=i32(i32)->i32".to_string()
    } else {
        "typed_signature=i32(i32, ...)->i32".to_string()
    }
}

fn typed_string_method_signature_note(arg_count: usize) -> String {
    if arg_count <= 1 {
        "typed_signature=string(string)->string".to_string()
    } else {
        "typed_signature=string(string, ...)->string".to_string()
    }
}

fn typed_method_signature_note(ret: &str, reps: &[crate::codegen::TypedParamRep]) -> String {
    let first = reps.first().map(|rep| rep.label()).unwrap_or("void");
    if reps.len() <= 1 {
        format!("typed_signature={ret}({first})->{ret}")
    } else {
        format!("typed_signature={ret}({first}, ...)->{ret}")
    }
}

/// Issue #620: emit a runtime check before the static class-method dispatch.
/// If the receiver has an own-property override at `property` (set via
/// `this.method = X`), invoke the stored closure via `js_native_call_value`;
/// otherwise call the static method body directly. Returns the LLVM register
/// holding the unified result (phi over the two branches).
/// `override_user_args` are the FLAT (un-rest-bundled) user arguments — i.e.
/// the source-level call arguments WITHOUT the leading `this` and WITHOUT the
/// trailing rest array the static ABI bundles. The override branch dispatches a
/// dynamic value (an arrow / bound function / native method) via
/// `js_native_call_value`, which performs its own arity/rest handling from a
/// flat positional buffer — so it must receive the spread-out args, not the
/// rest array as one positional. (`super.emit(event, ...args)` forwarding to a
/// native EventEmitter override otherwise delivered `[payload]` to listeners.)
/// The static branch keeps `fallback_arg_slices` (rest-bundled) unchanged.
pub(super) fn emit_own_method_override_check(
    ctx: &mut FnCtx<'_>,
    recv_box: &str,
    property: &str,
    fallback_fn: &str,
    fallback_arg_slices: &[(crate::types::LlvmType, &str)],
    this_box: &str,
    override_user_args: &[String],
) -> String {
    // Intern the property name so we can pass (ptr, len) directly to the
    // override probe — saves an allocation vs synthesizing a StringHeader.
    let key_idx = ctx.strings.intern(property);
    let entry = ctx.strings.entry(key_idx);
    let bytes_global = format!("@{}", entry.bytes_global);
    let name_len_str = entry.byte_len.to_string();

    let blk = ctx.block();
    let own_method = blk.call(
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

    let override_idx = ctx.new_block("ovrcheck.override");
    let static_idx = ctx.new_block("ovrcheck.static");
    let merge_idx = ctx.new_block("ovrcheck.merge");
    let override_label = ctx.block_label(override_idx);
    let static_label = ctx.block_label(static_idx);
    let merge_label = ctx.block_label(merge_idx);

    ctx.block()
        .cond_br(&is_undef, &static_label, &override_label);

    // Override path: spill the user args (skip lowered_args[0] which is
    // `this`) into a fresh alloca and call js_native_call_value. The
    // override may be an arrow / `.bind(...)`-bound function whose
    // `this` is captured/bound — but it can also be a regular function
    // assigned via `this.method = fn` or `class X { method = fn; }`
    // (hono's RegExpRouter uses this exact shape — `match = match;`
    // assigns the imported standalone `match` function as an instance
    // own-property; its body reads `this.buildAllMatchers()`). Bind
    // `IMPLICIT_THIS` to the receiver around the call so non-arrow
    // function bodies see the right `this` (issue #632 / #519 pattern).
    ctx.current_block = override_idx;
    let user_arg_count = override_user_args.len();
    let (args_ptr, args_len) = if user_arg_count == 0 {
        ("null".to_string(), "0".to_string())
    } else {
        let buf_reg = ctx.func.alloca_entry_array(DOUBLE, user_arg_count);
        for (i, a_val) in override_user_args.iter().enumerate() {
            let slot = ctx
                .block()
                .gep(DOUBLE, &buf_reg, &[(I64, &format!("{}", i))]);
            ctx.block().store(DOUBLE, a_val, &slot);
        }
        let ptr_reg = ctx.block().next_reg();
        ctx.block().emit_raw(format!(
            "{} = getelementptr [{} x double], ptr {}, i64 0, i64 0",
            ptr_reg, user_arg_count, buf_reg
        ));
        (ptr_reg, user_arg_count.to_string())
    };
    let recv_for_this = if this_box.is_empty() {
        double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED))
    } else {
        this_box.to_string()
    };
    let prev_this = ctx
        .block()
        .call(DOUBLE, "js_implicit_this_set", &[(DOUBLE, &recv_for_this)]);
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

    // Static path: original direct call to fallback_fn.
    ctx.current_block = static_idx;
    let v_static = ctx.block().call(DOUBLE, fallback_fn, fallback_arg_slices);
    let after_static = ctx.block().label.clone();
    if !ctx.block().is_terminated() {
        ctx.block().br(&merge_label);
    }

    ctx.current_block = merge_idx;
    ctx.block().phi(
        DOUBLE,
        &[
            (v_override.as_str(), after_override.as_str()),
            (v_static.as_str(), after_static.as_str()),
        ],
    )
}

/// Emit a typed-feedback runtime guard before a known class-method direct call.
///
/// The guard validates that the receiver still has the expected class shape,
/// has no own-property method replacement, and still resolves the method name
/// to the direct function pointer in the runtime vtable. Failures branch to the
/// existing dynamic method dispatcher and record a fallback once.
pub(super) fn emit_guarded_direct_method_call(
    ctx: &mut FnCtx<'_>,
    recv_box: &str,
    receiver_class_name: &str,
    property: &str,
    direct_fn: &str,
    direct_arg_slices: &[(crate::types::LlvmType, &str)],
    fallback_user_args: &[String],
    typed_direct_fn: Option<(&str, Vec<crate::codegen::TypedParamRep>)>,
    typed_f64_receiver_direct_fn: Option<(&str, usize, &crate::codegen::TypedReceiverMethodInfo)>,
    typed_i32_direct_fn: Option<(&str, Vec<crate::codegen::TypedParamRep>)>,
    typed_i1_direct_fn: Option<(&str, Vec<crate::codegen::TypedParamRep>)>,
    typed_string_direct_fn: Option<(&str, Vec<crate::codegen::TypedParamRep>)>,
    shape_only_guard: bool,
) -> Option<String> {
    let expected_class_id = *ctx.class_ids.get(receiver_class_name)?;
    let keys_global_name = ctx.class_keys_globals.get(receiver_class_name)?.clone();

    let expected_class_id_str = expected_class_id.to_string();
    let expected_keys_slot = ctx.func.entry_init_load_global(&keys_global_name, I64);
    let expected_keys = ctx.block().load(I64, &expected_keys_slot);

    let key_idx = ctx.strings.intern(property);
    let entry = ctx.strings.entry(key_idx);
    let bytes_global = format!("@{}", entry.bytes_global);
    let key_handle_global = format!("@{}", entry.handle_global);
    let name_len_str = entry.byte_len.to_string();
    let site_id = if shape_only_guard {
        None
    } else {
        Some(emit_typed_feedback_register_site(
            ctx,
            TypedFeedbackKind::MethodCall,
            property,
            TypedFeedbackContract::method_direct_call(),
        ))
    };

    let guard_idx = ctx.new_block("method_direct.guard");
    let fast_idx = ctx.new_block("method_direct.fast");
    let fallback_idx = ctx.new_block("method_direct.fallback");
    let merge_idx = ctx.new_block("method_direct.merge");
    let guard_label = ctx.block_label(guard_idx);
    let fast_label = ctx.block_label(fast_idx);
    let fallback_label = ctx.block_label(fallback_idx);
    let merge_label = ctx.block_label(merge_idx);
    ctx.block().br(&guard_label);

    ctx.current_block = guard_idx;
    let guard_ok = if shape_only_guard {
        ctx.block().call(
            I32,
            "js_method_direct_shape_guard",
            &[
                (DOUBLE, recv_box),
                (I32, &expected_class_id_str),
                (I64, &expected_keys),
            ],
        )
    } else {
        ctx.block().call(
            I32,
            "js_typed_feedback_method_direct_call_guard",
            &[
                (
                    I64,
                    site_id.as_deref().expect("typed-feedback method site id"),
                ),
                (DOUBLE, recv_box),
                (I32, &expected_class_id_str),
                (I64, &expected_keys),
                (crate::types::PTR, &bytes_global),
                (I64, &name_len_str),
                (crate::types::PTR, &format!("@{}", direct_fn)),
            ],
        )
    };
    let guard_pass = ctx.block().icmp_ne(I32, &guard_ok, "0");
    ctx.block()
        .cond_br(&guard_pass, &fast_label, &fallback_label);

    ctx.current_block = fast_idx;
    let fast_value = {
        if let Some((typed_fn, typed_formal_count, receiver_info)) = typed_f64_receiver_direct_fn {
            let generic_body_fn = crate::codegen::generic_method_body_name(direct_fn);
            let formal_args: Vec<&str> = direct_arg_slices
                .iter()
                .skip(1)
                .take(typed_formal_count)
                .map(|(_, value)| *value)
                .collect();
            let mut guard: Option<String> = None;
            for value in &formal_args {
                let raw = ctx
                    .block()
                    .call(I32, "js_typed_f64_arg_guard", &[(DOUBLE, *value)]);
                let ok = ctx.block().icmp_ne(I32, &raw, "0");
                guard = Some(match guard {
                    Some(prev) => ctx.block().and(I1, &prev, &ok),
                    None => ok,
                });
            }
            for field in &receiver_info.fields {
                let site_id = emit_typed_feedback_register_site(
                    ctx,
                    TypedFeedbackKind::PropertyGet,
                    &field.name,
                    TypedFeedbackContract::class_field_get(),
                );
                let key_idx = ctx.strings.intern(&field.name);
                let key_handle_global = format!("@{}", ctx.strings.entry(key_idx).handle_global);
                let key_box = ctx.block().load(DOUBLE, &key_handle_global);
                let key_bits = ctx.block().bitcast_double_to_i64(&key_box);
                let key_raw = ctx
                    .block()
                    .and(I64, &key_bits, crate::nanbox::POINTER_MASK_I64);
                let field_index_str = field.index.to_string();
                let raw_guard = ctx.block().call(
                    I32,
                    "js_typed_feedback_class_field_get_guard",
                    &[
                        (I64, &site_id),
                        (DOUBLE, recv_box),
                        (I32, &expected_class_id_str),
                        (I64, &expected_keys),
                        (I64, &key_raw),
                        (I32, &field_index_str),
                        (I32, "1"),
                    ],
                );
                let ok = ctx.block().icmp_ne(I32, &raw_guard, "0");
                guard = Some(match guard {
                    Some(prev) => ctx.block().and(I1, &prev, &ok),
                    None => ok,
                });
            }

            let typed_idx = ctx.new_block("typed_f64_recv_method.fast");
            let generic_idx = ctx.new_block("typed_f64_recv_method.generic");
            let typed_merge_idx = ctx.new_block("typed_f64_recv_method.merge");
            let typed_label = ctx.block_label(typed_idx);
            let generic_label = ctx.block_label(generic_idx);
            let typed_merge_label = ctx.block_label(typed_merge_idx);
            if let Some(guard) = guard {
                ctx.block().cond_br(&guard, &typed_label, &generic_label);
            } else {
                ctx.block().br(&typed_label);
            }

            ctx.current_block = typed_idx;
            let recv_bits = ctx.block().bitcast_double_to_i64(recv_box);
            let recv_handle = ctx
                .block()
                .and(I64, &recv_bits, crate::nanbox::POINTER_MASK_I64);
            let mut typed_args_storage: Vec<String> = Vec::with_capacity(formal_args.len());
            for value in &formal_args {
                typed_args_storage.push(ctx.block().call(
                    DOUBLE,
                    "js_typed_f64_arg_to_raw",
                    &[(DOUBLE, *value)],
                ));
            }
            let mut typed_args: Vec<(crate::types::LlvmType, &str)> =
                Vec::with_capacity(typed_args_storage.len() + 1);
            typed_args.push((I64, recv_handle.as_str()));
            for value in &typed_args_storage {
                typed_args.push((DOUBLE, value.as_str()));
            }
            let typed_value = ctx.block().call(DOUBLE, typed_fn, &typed_args);
            let after_typed = ctx.block().label.clone();
            if !ctx.block().is_terminated() {
                ctx.block().br(&typed_merge_label);
            }

            ctx.current_block = generic_idx;
            let generic_value = ctx
                .block()
                .call(DOUBLE, &generic_body_fn, direct_arg_slices);
            let after_generic = ctx.block().label.clone();
            if !ctx.block().is_terminated() {
                ctx.block().br(&typed_merge_label);
            }

            ctx.current_block = typed_merge_idx;
            let result = ctx.block().phi(
                DOUBLE,
                &[
                    (typed_value.as_str(), after_typed.as_str()),
                    (generic_value.as_str(), after_generic.as_str()),
                ],
            );
            ctx.record_lowered_value(
                "MethodCall",
                None,
                "typed_f64_receiver_method_direct_call",
                &LoweredValue::f64(result.clone()),
                None,
                None,
                None,
                false,
                false,
                vec![
                    format!("typed_clone={typed_fn}"),
                    format!("generic_method={generic_body_fn}"),
                    format!("receiver_class={receiver_class_name}"),
                    format!("method={property}"),
                    "receiver_arg=i64".to_string(),
                    "raw_f64_field_guard=required".to_string(),
                ],
            );
            result
        } else if let Some((typed_fn, typed_param_reps)) = typed_direct_fn {
            let generic_body_fn = crate::codegen::generic_method_body_name(direct_fn);
            let formal_args: Vec<&str> = direct_arg_slices
                .iter()
                .skip(1)
                .take(typed_param_reps.len())
                .map(|(_, value)| *value)
                .collect();
            let mut guard: Option<String> = None;
            for (value, rep) in formal_args.iter().zip(typed_param_reps.iter()) {
                let ok = crate::codegen::emit_typed_arg_guard(ctx.block(), *rep, value);
                guard = Some(match guard {
                    Some(prev) => ctx.block().and(I1, &prev, &ok),
                    None => ok,
                });
            }

            let typed_idx = ctx.new_block("typed_f64_method.fast");
            let generic_idx = ctx.new_block("typed_f64_method.generic");
            let typed_merge_idx = ctx.new_block("typed_f64_method.merge");
            let typed_label = ctx.block_label(typed_idx);
            let generic_label = ctx.block_label(generic_idx);
            let typed_merge_label = ctx.block_label(typed_merge_idx);
            if let Some(guard) = guard {
                ctx.block().cond_br(&guard, &typed_label, &generic_label);
            } else {
                ctx.block().br(&typed_label);
            }

            ctx.current_block = typed_idx;
            let mut typed_args_storage: Vec<String> = Vec::with_capacity(formal_args.len());
            for (value, rep) in formal_args.iter().zip(typed_param_reps.iter()) {
                typed_args_storage.push(crate::codegen::emit_typed_arg_to_raw(
                    ctx.block(),
                    *rep,
                    value,
                ));
            }
            let typed_args: Vec<(crate::types::LlvmType, &str)> = typed_args_storage
                .iter()
                .zip(typed_param_reps.iter())
                .map(|(value, rep)| (rep.llvm_ty(), value.as_str()))
                .collect();
            let typed_value = ctx.block().call(DOUBLE, typed_fn, &typed_args);
            let after_typed = ctx.block().label.clone();
            if !ctx.block().is_terminated() {
                ctx.block().br(&typed_merge_label);
            }

            ctx.current_block = generic_idx;
            let generic_value = ctx
                .block()
                .call(DOUBLE, &generic_body_fn, direct_arg_slices);
            let after_generic = ctx.block().label.clone();
            if !ctx.block().is_terminated() {
                ctx.block().br(&typed_merge_label);
            }

            ctx.current_block = typed_merge_idx;
            let result = ctx.block().phi(
                DOUBLE,
                &[
                    (typed_value.as_str(), after_typed.as_str()),
                    (generic_value.as_str(), after_generic.as_str()),
                ],
            );
            ctx.record_lowered_value(
                "MethodCall",
                None,
                "typed_f64_method_direct_call",
                &LoweredValue::f64(result.clone()),
                None,
                None,
                None,
                false,
                false,
                vec![
                    format!("typed_clone={typed_fn}"),
                    format!("generic_method={generic_body_fn}"),
                    format!("receiver_class={receiver_class_name}"),
                    format!("method={property}"),
                    typed_method_signature_note("f64", &typed_param_reps),
                ],
            );
            result
        } else if let Some((typed_fn, typed_param_reps)) = typed_i32_direct_fn {
            let generic_body_fn = crate::codegen::generic_method_body_name(direct_fn);
            let formal_args: Vec<&str> = direct_arg_slices
                .iter()
                .skip(1)
                .take(typed_param_reps.len())
                .map(|(_, value)| *value)
                .collect();
            let mut guard: Option<String> = None;
            for (value, rep) in formal_args.iter().zip(typed_param_reps.iter()) {
                let ok = crate::codegen::emit_typed_arg_guard(ctx.block(), *rep, value);
                guard = Some(match guard {
                    Some(prev) => ctx.block().and(I1, &prev, &ok),
                    None => ok,
                });
            }

            let typed_idx = ctx.new_block("typed_i32_method.fast");
            let generic_idx = ctx.new_block("typed_i32_method.generic");
            let typed_merge_idx = ctx.new_block("typed_i32_method.merge");
            let typed_label = ctx.block_label(typed_idx);
            let generic_label = ctx.block_label(generic_idx);
            let typed_merge_label = ctx.block_label(typed_merge_idx);
            if let Some(guard) = guard {
                ctx.block().cond_br(&guard, &typed_label, &generic_label);
            } else {
                ctx.block().br(&typed_label);
            }

            ctx.current_block = typed_idx;
            let mut typed_args_storage: Vec<String> = Vec::with_capacity(formal_args.len());
            for (value, rep) in formal_args.iter().zip(typed_param_reps.iter()) {
                typed_args_storage.push(crate::codegen::emit_typed_arg_to_raw(
                    ctx.block(),
                    *rep,
                    value,
                ));
            }
            let typed_args: Vec<(crate::types::LlvmType, &str)> = typed_args_storage
                .iter()
                .zip(typed_param_reps.iter())
                .map(|(value, rep)| (rep.llvm_ty(), value.as_str()))
                .collect();
            let raw_i32 = ctx.block().call(I32, typed_fn, &typed_args);
            let typed_value = i32_to_nanbox(ctx.block(), &raw_i32);
            let after_typed = ctx.block().label.clone();
            if !ctx.block().is_terminated() {
                ctx.block().br(&typed_merge_label);
            }

            ctx.current_block = generic_idx;
            let generic_value = ctx
                .block()
                .call(DOUBLE, &generic_body_fn, direct_arg_slices);
            let after_generic = ctx.block().label.clone();
            if !ctx.block().is_terminated() {
                ctx.block().br(&typed_merge_label);
            }

            ctx.current_block = typed_merge_idx;
            let result = ctx.block().phi(
                DOUBLE,
                &[
                    (typed_value.as_str(), after_typed.as_str()),
                    (generic_value.as_str(), after_generic.as_str()),
                ],
            );
            ctx.record_lowered_value(
                "MethodCall",
                None,
                "typed_i32_method_direct_call",
                &LoweredValue::js_value(result.clone()),
                None,
                None,
                None,
                false,
                false,
                vec![
                    format!("typed_clone={typed_fn}"),
                    format!("generic_method={generic_body_fn}"),
                    format!("receiver_class={receiver_class_name}"),
                    format!("method={property}"),
                    typed_method_signature_note("i32", &typed_param_reps),
                    "boxed_result_at=direct_call_boundary".to_string(),
                ],
            );
            result
        } else if let Some((typed_fn, typed_param_reps)) = typed_i1_direct_fn {
            let generic_body_fn = crate::codegen::generic_method_body_name(direct_fn);
            let formal_args: Vec<&str> = direct_arg_slices
                .iter()
                .skip(1)
                .take(typed_param_reps.len())
                .map(|(_, value)| *value)
                .collect();
            let mut guard: Option<String> = None;
            for (value, rep) in formal_args.iter().zip(typed_param_reps.iter()) {
                let raw = ctx.block().call(I32, rep.guard_fn(), &[(DOUBLE, *value)]);
                let ok = ctx.block().icmp_ne(I32, &raw, "0");
                guard = Some(match guard {
                    Some(prev) => ctx.block().and(I1, &prev, &ok),
                    None => ok,
                });
            }

            let typed_idx = ctx.new_block("typed_i1_method.fast");
            let generic_idx = ctx.new_block("typed_i1_method.generic");
            let typed_merge_idx = ctx.new_block("typed_i1_method.merge");
            let typed_label = ctx.block_label(typed_idx);
            let generic_label = ctx.block_label(generic_idx);
            let typed_merge_label = ctx.block_label(typed_merge_idx);
            if let Some(guard) = guard {
                ctx.block().cond_br(&guard, &typed_label, &generic_label);
            } else {
                ctx.block().br(&typed_label);
            }

            ctx.current_block = typed_idx;
            let mut typed_args_storage: Vec<String> = Vec::with_capacity(formal_args.len());
            for (value, rep) in formal_args.iter().zip(typed_param_reps.iter()) {
                typed_args_storage.push(match rep {
                    crate::codegen::TypedParamRep::F64 => {
                        ctx.block()
                            .call(DOUBLE, rep.unbox_fn(), &[(DOUBLE, *value)])
                    }
                    crate::codegen::TypedParamRep::I32 => {
                        ctx.block().call(I32, rep.unbox_fn(), &[(DOUBLE, *value)])
                    }
                    crate::codegen::TypedParamRep::I1 => {
                        let raw_i32 = ctx.block().call(I32, rep.unbox_fn(), &[(DOUBLE, *value)]);
                        ctx.block().icmp_ne(I32, &raw_i32, "0")
                    }
                    crate::codegen::TypedParamRep::StringRef => {
                        ctx.block().call(I64, rep.unbox_fn(), &[(DOUBLE, *value)])
                    }
                });
            }
            let typed_args: Vec<(crate::types::LlvmType, &str)> = typed_args_storage
                .iter()
                .zip(typed_param_reps.iter())
                .map(|(value, rep)| (rep.llvm_ty(), value.as_str()))
                .collect();
            let typed_i1 = ctx.block().call(I1, typed_fn, &typed_args);
            let typed_i32 = ctx.block().zext(I1, &typed_i1, I32);
            let typed_value = i32_bool_to_nanbox(ctx.block(), &typed_i32);
            let after_typed = ctx.block().label.clone();
            if !ctx.block().is_terminated() {
                ctx.block().br(&typed_merge_label);
            }

            ctx.current_block = generic_idx;
            let generic_value = ctx
                .block()
                .call(DOUBLE, &generic_body_fn, direct_arg_slices);
            let after_generic = ctx.block().label.clone();
            if !ctx.block().is_terminated() {
                ctx.block().br(&typed_merge_label);
            }

            ctx.current_block = typed_merge_idx;
            let result = ctx.block().phi(
                DOUBLE,
                &[
                    (typed_value.as_str(), after_typed.as_str()),
                    (generic_value.as_str(), after_generic.as_str()),
                ],
            );
            ctx.record_lowered_value(
                "MethodCall",
                None,
                "typed_i1_method_direct_call",
                &LoweredValue::js_value(result.clone()),
                None,
                None,
                None,
                false,
                false,
                vec![
                    format!("typed_clone={typed_fn}"),
                    format!("generic_method={generic_body_fn}"),
                    format!("receiver_class={receiver_class_name}"),
                    format!("method={property}"),
                    typed_i1_method_signature_note(&typed_param_reps),
                    "boxed_result_at=direct_call_boundary".to_string(),
                ],
            );
            result
        } else if let Some((typed_fn, typed_param_reps)) = typed_string_direct_fn {
            let generic_body_fn = crate::codegen::generic_method_body_name(direct_fn);
            let formal_args: Vec<&str> = direct_arg_slices
                .iter()
                .skip(1)
                .take(typed_param_reps.len())
                .map(|(_, value)| *value)
                .collect();
            let mut guard: Option<String> = None;
            for (value, rep) in formal_args.iter().zip(typed_param_reps.iter()) {
                let ok = crate::codegen::emit_typed_arg_guard(ctx.block(), *rep, value);
                guard = Some(match guard {
                    Some(prev) => ctx.block().and(I1, &prev, &ok),
                    None => ok,
                });
            }

            let typed_idx = ctx.new_block("typed_string_method.fast");
            let generic_idx = ctx.new_block("typed_string_method.generic");
            let typed_merge_idx = ctx.new_block("typed_string_method.merge");
            let typed_label = ctx.block_label(typed_idx);
            let generic_label = ctx.block_label(generic_idx);
            let typed_merge_label = ctx.block_label(typed_merge_idx);
            if let Some(guard) = guard {
                ctx.block().cond_br(&guard, &typed_label, &generic_label);
            } else {
                ctx.block().br(&typed_label);
            }

            ctx.current_block = typed_idx;
            let mut typed_args_storage: Vec<String> = Vec::with_capacity(formal_args.len());
            for (value, rep) in formal_args.iter().zip(typed_param_reps.iter()) {
                typed_args_storage.push(crate::codegen::emit_typed_arg_to_raw(
                    ctx.block(),
                    *rep,
                    value,
                ));
            }
            let typed_args: Vec<(crate::types::LlvmType, &str)> = typed_args_storage
                .iter()
                .zip(typed_param_reps.iter())
                .map(|(value, rep)| (rep.llvm_ty(), value.as_str()))
                .collect();
            let raw_string = ctx.block().call(I64, typed_fn, &typed_args);
            let typed_value = ctx
                .block()
                .call(DOUBLE, "js_nanbox_string", &[(I64, &raw_string)]);
            let after_typed = ctx.block().label.clone();
            if !ctx.block().is_terminated() {
                ctx.block().br(&typed_merge_label);
            }

            ctx.current_block = generic_idx;
            let generic_value = ctx
                .block()
                .call(DOUBLE, &generic_body_fn, direct_arg_slices);
            let after_generic = ctx.block().label.clone();
            if !ctx.block().is_terminated() {
                ctx.block().br(&typed_merge_label);
            }

            ctx.current_block = typed_merge_idx;
            let result = ctx.block().phi(
                DOUBLE,
                &[
                    (typed_value.as_str(), after_typed.as_str()),
                    (generic_value.as_str(), after_generic.as_str()),
                ],
            );
            ctx.record_lowered_value(
                "MethodCall",
                None,
                "typed_string_method_direct_call",
                &LoweredValue::js_value(result.clone()),
                None,
                None,
                None,
                false,
                false,
                vec![
                    format!("typed_clone={typed_fn}"),
                    format!("generic_method={generic_body_fn}"),
                    format!("receiver_class={receiver_class_name}"),
                    format!("method={property}"),
                    typed_method_signature_note("string", &typed_param_reps),
                    "boxed_result_at=direct_call_boundary".to_string(),
                ],
            );
            result
        } else {
            ctx.block().call(DOUBLE, direct_fn, direct_arg_slices)
        }
    };
    let after_fast = ctx.block().label.clone();
    if !ctx.block().is_terminated() {
        ctx.block().br(&merge_label);
    }

    ctx.current_block = fallback_idx;
    let (args_ptr, args_len) = if fallback_user_args.is_empty() {
        ("null".to_string(), "0".to_string())
    } else {
        let n = fallback_user_args.len();
        let buf_reg = ctx.func.alloca_entry_array(DOUBLE, n);
        for (i, a_val) in fallback_user_args.iter().enumerate() {
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
    if let Some(site_id) = site_id {
        ctx.block()
            .call_void("js_typed_feedback_record_fallback_call", &[(I64, &site_id)]);
    }
    let key_box = ctx.block().load(DOUBLE, &key_handle_global);
    let key_bits = ctx.block().bitcast_double_to_i64(&key_box);
    let method_id = ctx
        .block()
        .and(I64, &key_bits, crate::nanbox::POINTER_MASK_I64);
    let fallback_value = ctx.block().call(
        DOUBLE,
        "js_native_call_method_by_id",
        &[
            (DOUBLE, recv_box),
            (I64, &method_id),
            (crate::types::PTR, &args_ptr),
            (I64, &args_len),
        ],
    );
    let after_fallback = ctx.block().label.clone();
    if !ctx.block().is_terminated() {
        ctx.block().br(&merge_label);
    }

    ctx.current_block = merge_idx;
    Some(ctx.block().phi(
        DOUBLE,
        &[
            (fast_value.as_str(), after_fast.as_str()),
            (fallback_value.as_str(), after_fallback.as_str()),
        ],
    ))
}
