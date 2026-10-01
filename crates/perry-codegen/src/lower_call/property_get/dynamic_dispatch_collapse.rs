//! The full-outline collapse of the instance method-dispatch tower (#5391
//! path 4), split out of `dynamic_dispatch.rs` for the 2000-line cap.

use super::*;

/// Whether to collapse the instance method-dispatch tower in full-outline mode.
/// On by default whenever full-outline is active; `PERRY_OUTLINE_METHOD_DISPATCH=0`
/// / `off` / `false` keeps the inline class-id switch tower (escape hatch /
/// differential-test isolation).
pub(super) fn method_dispatch_collapse_enabled() -> bool {
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
pub(super) fn emit_collapsed_instance_dispatch(
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

    // Override arm: call the stored function value with the receiver as
    // `this` (#632 — a class-field non-arrow function reads `this`).
    ctx.current_block = override_idx;
    let this_bits = ctx.block().bitcast_double_to_i64(recv_box);
    let v_override = ctx.block().call(
        DOUBLE,
        "js_native_call_value",
        &[
            (DOUBLE, &own_method),
            (I64, &this_bits),
            (crate::types::PTR, &args_ptr),
            (I64, &args_len),
        ],
    );
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
