//! Proxy / Reflect metaprogramming.
//!
//! Extracted from `expr/mod.rs` to keep that file under the 2000-line cap.
//! Pure mechanical move — match arm bodies are verbatim copies, called from
//! `lower_expr`'s outer dispatch.

use anyhow::Result;
use perry_hir::Expr;

use crate::nanbox::{double_literal, POINTER_MASK_I64};
use crate::native_value::MaterializationReason;
use crate::type_analysis::{is_array_expr, is_numeric_expr, is_string_expr, receiver_class_name};
use crate::types::{DOUBLE, I1, I16, I32, I64, I8, PTR};

use super::{
    downgrade_buffer_aliases_in_expr, emit_jsvalue_slot_store_scalar_aware_on_block,
    expr_produces_non_pointer_bits_by_construction, lower_expr, nanbox_pointer_inline,
    proxy_build_args_array, unbox_str_handle, unbox_to_i64, FnCtx,
};

fn downgrade_unknown_call_expr(ctx: &mut FnCtx<'_>, expr: &Expr) {
    downgrade_buffer_aliases_in_expr(ctx, expr, MaterializationReason::UnknownCallEscape);
}

fn downgrade_unknown_call_args(ctx: &mut FnCtx<'_>, args: &[Expr]) {
    for arg in args {
        downgrade_unknown_call_expr(ctx, arg);
    }
}

/// `p.call(thisArg, ...rest)` / `p.apply(thisArg, argsArray)` where `p` is a
/// Proxy (#3656). The HIR lowers the callee to `ProxyGet(p, "call"|"apply")`,
/// which would otherwise read `.call`/`.apply` off the *target* and invoke the
/// target directly. Per `Function.prototype.{call,apply}` semantics the `this`
/// of the invocation is the proxy, so the call must route through the proxy's
/// `[[Call]]` (the `apply` trap) with `thisArg` bound. Returns `None` when the
/// callee isn't a proxy `.call`/`.apply` so the normal dispatch proceeds.
pub(crate) fn try_lower_proxy_fn_call_apply(
    ctx: &mut FnCtx<'_>,
    callee: &Expr,
    args: &[Expr],
) -> Result<Option<String>> {
    let Expr::ProxyGet { proxy, key } = callee else {
        return Ok(None);
    };
    let is_apply = match key.as_ref() {
        Expr::String(s) if s == "apply" => true,
        Expr::String(s) if s == "call" => false,
        _ => return Ok(None),
    };
    downgrade_unknown_call_expr(ctx, proxy);
    downgrade_unknown_call_args(ctx, args);
    let p = lower_expr(ctx, proxy)?;
    let this_arg = match args.first() {
        Some(a) => lower_expr(ctx, a)?,
        None => double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED)),
    };
    let arr_box = if is_apply {
        // 2nd arg is the already-built argument array (a JSValue). When absent,
        // synthesize an empty array so the trap receives a real argArray.
        match args.get(1) {
            Some(a) => lower_expr(ctx, a)?,
            None => {
                let arr_handle = proxy_build_args_array(ctx, &[])?;
                let blk = ctx.block();
                nanbox_pointer_inline(blk, &arr_handle)
            }
        }
    } else {
        let rest: Vec<Expr> = args.iter().skip(1).cloned().collect();
        let arr_handle = proxy_build_args_array(ctx, &rest)?;
        let blk = ctx.block();
        nanbox_pointer_inline(blk, &arr_handle)
    };
    Ok(Some(ctx.block().call(
        DOUBLE,
        "js_proxy_apply",
        &[(DOUBLE, &p), (DOUBLE, &this_arg), (DOUBLE, &arr_box)],
    )))
}

/// `proxy.method(args)` for a method name other than `call`/`apply` — the
/// *fused* member-call form whose callee the HIR lowered to
/// `ProxyGet(p, "method")` (#5196). Reading `.method` off the proxy and then
/// invoking it must bind `this` to the proxy itself, so `Array.prototype.map`
/// & friends iterate the proxy through its `get` trap. The plain closure-call
/// fallthrough loses that receiver (the method runs with `this = undefined`,
/// throwing `Cannot convert undefined or null to object`). Route the call
/// through `js_native_call_method`, whose Proxy arm performs the spec
/// `Get(proxy, "method")` then `Call(method, proxy, args)`. Returns `None`
/// when the callee isn't a proxy member-call so normal dispatch proceeds.
pub(crate) fn try_lower_proxy_method_call(
    ctx: &mut FnCtx<'_>,
    callee: &Expr,
    args: &[Expr],
) -> Result<Option<String>> {
    let Expr::ProxyGet { proxy, key } = callee else {
        return Ok(None);
    };
    let Expr::String(method_name) = key.as_ref() else {
        return Ok(None);
    };
    // `.call`/`.apply` route through the proxy's [[Call]] (apply trap) and are
    // handled by `try_lower_proxy_fn_call_apply`, which runs first.
    if method_name == "call" || method_name == "apply" {
        return Ok(None);
    }
    downgrade_unknown_call_expr(ctx, proxy);
    downgrade_unknown_call_args(ctx, args);
    let recv_box = lower_expr(ctx, proxy)?;
    let mut lowered_args: Vec<String> = Vec::with_capacity(args.len());
    for a in args {
        lowered_args.push(lower_expr(ctx, a)?);
    }
    let (args_ptr, args_len) = if lowered_args.is_empty() {
        ("null".to_string(), "0".to_string())
    } else {
        let n = lowered_args.len();
        let buf = ctx.func.alloca_entry_array(DOUBLE, n);
        {
            let blk = ctx.block();
            for (i, value) in lowered_args.iter().enumerate() {
                let slot = blk.gep(DOUBLE, &buf, &[(I64, &i.to_string())]);
                blk.store(DOUBLE, value, &slot);
            }
        }
        (buf, n.to_string())
    };
    let method_idx = ctx.strings.intern(method_name);
    let entry = ctx.strings.entry(method_idx);
    let bytes_global = format!("@{}", entry.bytes_global);
    let name_len = entry.byte_len.to_string();
    Ok(Some(ctx.block().call(
        DOUBLE,
        "js_native_call_method",
        &[
            (DOUBLE, &recv_box),
            (PTR, &bytes_global),
            (I64, &name_len),
            (PTR, &args_ptr),
            (I64, &args_len),
        ],
    )))
}

fn put_value_static_property_fast_path(
    ctx: &FnCtx<'_>,
    target: &Expr,
    key: &Expr,
    receiver: &Expr,
    strict: bool,
) -> Option<String> {
    let Expr::String(property) = key else {
        return None;
    };
    // #6542: this fast path lowers to `js_object_set_field_by_name`, which has
    // no `strict` parameter and throws unconditionally when the field is
    // non-writable (frozen/sealed object, `writable: false` descriptor). That
    // matches spec `[[Set]]`+`PutValue` only in STRICT mode; a SLOPPY store to
    // a non-writable property must be a silent no-op (`OrdinarySet` returns
    // `false`, sloppy `PutValue` ignores it). So a heap object instance in
    // sloppy code must stay on the strict-aware `js_put_value_set` path (which
    // honors `strict = 0`). This gate only affects the class-instance arms
    // below: a POD-layout / scalar-replaced object never escapes, so it can
    // never have been passed to `Object.freeze` and can never be frozen —
    // those keep the fast path in both modes (and diverting them to the
    // pointer-taking `js_put_value_set` would break their fieldless storage).
    match (target, receiver) {
        (Expr::LocalGet(id), Expr::LocalGet(receiver_id)) if id == receiver_id => {
            let pod_field = ctx.pod_records.get(id).is_some_and(|local| {
                local
                    .layout
                    .fields
                    .iter()
                    .any(|field| field.name == *property)
            });
            let scalar_field = ctx
                .scalar_replaced
                .get(id)
                .is_some_and(|fields| fields.contains_key(property));
            if pod_field || scalar_field {
                return Some(property.clone());
            }
            if !strict {
                return None;
            }
            receiver_class_name(ctx, target)
                .and_then(|class_name| {
                    crate::type_analysis::class_field_global_index(ctx, &class_name, property)
                })
                .map(|_| property.clone())
        }
        (Expr::This, Expr::This) => {
            if ctx
                .scalar_ctor_target
                .last()
                .and_then(|tid| ctx.scalar_replaced.get(tid))
                .is_some_and(|fields| fields.contains_key(property))
            {
                return Some(property.clone());
            }
            if !strict {
                return None;
            }
            receiver_class_name(ctx, target)
                .and_then(|class_name| {
                    crate::type_analysis::class_field_global_index(ctx, &class_name, property)
                })
                .map(|_| property.clone())
        }
        _ if same_side_effect_free_receiver(target, receiver) => {
            if !strict {
                return None;
            }
            let class_name = receiver_class_name(ctx, target)?;
            crate::type_analysis::class_field_global_index(ctx, &class_name, property)
                .map(|_| property.clone())
        }
        _ => None,
    }
}

/// Monomorphic inline cache for a static-name `PutValue` whose target and
/// receiver are the same expression.
///
/// Sloppy script writes cannot reuse `PropertySet` because its fallback throws
/// on rejected writes. This diamond keeps the strict-aware runtime on every
/// miss, then turns a settled existing-own-data store into a keys-token compare
/// plus a direct slot write. Mutable semantic state (freeze/descriptor flags)
/// is rechecked on every hit.
fn lower_put_value_static_write_ic(
    ctx: &mut FnCtx<'_>,
    target: &Expr,
    key: &Expr,
    value: &Expr,
    receiver: &Expr,
    strict: bool,
) -> Result<Option<String>> {
    let Some(property) = static_write_key(ctx, key) else {
        return Ok(None);
    };
    if !same_put_value_receiver_expr(target, receiver) || crate::codegen::full_outline_ic_enabled()
    {
        return Ok(None);
    }
    // The assignment reference (target + static key) is evaluated before the
    // RHS. Until PutValue reference temporaries have dedicated GC roots, an
    // allocating/calling RHS could move the already-evaluated target while its
    // SSA value remains stale. Keep the inline PIC to call-free expressions;
    // the existing runtime lowering handles every other RHS.
    if !put_value_rhs_is_safepoint_free(ctx, value) {
        return Ok(None);
    }

    downgrade_unknown_call_expr(ctx, target);
    // An immutable `const key = "x"` has no observable work at this use site;
    // resolve it to the interned literal global instead of retaining a
    // movable runtime string pointer in the cache. Mutable locals and all
    // other computed keys stay on the ordinary dynamic PropertyKey path.
    let static_key = Expr::String(property);
    downgrade_unknown_call_expr(ctx, &static_key);
    downgrade_unknown_call_expr(ctx, value);
    downgrade_unknown_call_expr(ctx, receiver);
    let target_value = lower_expr(ctx, target)?;
    let key_value = lower_expr(ctx, &static_key)?;
    let stored_value = lower_expr(ctx, value)?;

    let target_bits = ctx.block().bitcast_double_to_i64(&target_value);
    let key_bits = ctx.block().bitcast_double_to_i64(&key_value);
    let key_handle = ctx.block().and(I64, &key_bits, POINTER_MASK_I64);
    let target_handle = ctx.block().and(I64, &target_bits, POINTER_MASK_I64);

    let site_id = ctx.ic_site_counter;
    ctx.ic_site_counter += 1;
    let cache_name = format!("perry_ic_{}", site_id);
    ctx.pending_declares
        .push((format!("__ic_decl_{}", site_id), DOUBLE, vec![]));
    ctx.ic_globals.push(cache_name.clone());
    let cache_ref = format!("@{}", cache_name);

    // Branch before the first header load so primitives, forged non-pointer
    // bit patterns, and native handle ids can never be dereferenced by the
    // inline checks.
    let target_tag = ctx.block().lshr(I64, &target_bits, "48");
    let pointer_tag = ctx.block().icmp_eq(I64, &target_tag, "32765"); // 0x7FFD
    let above_handles = ctx.block().icmp_ugt(I64, &target_handle, "1048575"); // 0x100000
    let heap_candidate = ctx.block().and(I1, &pointer_tag, &above_handles);
    let guard_idx = ctx.new_block("put.pic.guard");
    let guard2_idx = ctx.new_block("put.pic.guard2");
    let guard3_idx = ctx.new_block("put.pic.guard3");
    let guard4_idx = ctx.new_block("put.pic.guard4");
    let fallback_idx = ctx.new_block("put.pic.fallback");
    let dispatch3_idx = ctx.new_block("put.pic.dispatch3");
    let dispatch4_idx = ctx.new_block("put.pic.dispatch4");
    let hit_idx = ctx.new_block("put.pic.hit");
    let miss_idx = ctx.new_block("put.pic.miss");
    let miss2_idx = ctx.new_block("put.pic.miss2");
    let miss3_idx = ctx.new_block("put.pic.miss3");
    let miss4_idx = ctx.new_block("put.pic.miss4");
    let merge_idx = ctx.new_block("put.pic.merge");
    let guard_label = ctx.block_label(guard_idx);
    let guard2_label = ctx.block_label(guard2_idx);
    let guard3_label = ctx.block_label(guard3_idx);
    let guard4_label = ctx.block_label(guard4_idx);
    let fallback_label = ctx.block_label(fallback_idx);
    let dispatch3_label = ctx.block_label(dispatch3_idx);
    let dispatch4_label = ctx.block_label(dispatch4_idx);
    let hit_label = ctx.block_label(hit_idx);
    let miss_label = ctx.block_label(miss_idx);
    let miss2_label = ctx.block_label(miss2_idx);
    let miss3_label = ctx.block_label(miss3_idx);
    let miss4_label = ctx.block_label(miss4_idx);
    let merge_label = ctx.block_label(merge_idx);
    ctx.block()
        .cond_br(&heap_candidate, &guard_label, &miss_label);

    ctx.current_block = guard_idx;
    let safe_target = target_handle.clone();

    let gc_type_addr = ctx.block().sub(I64, &safe_target, "8");
    let gc_type_ptr = ctx.block().inttoptr(I64, &gc_type_addr);
    let gc_type = ctx.block().load(I8, &gc_type_ptr);
    let gc_object = ctx.block().icmp_eq(I8, &gc_type, "2");
    let gc_flags_addr = ctx.block().sub(I64, &safe_target, "7");
    let gc_flags_ptr = ctx.block().inttoptr(I64, &gc_flags_addr);
    let gc_flags = ctx.block().load(I8, &gc_flags_ptr);
    let forwarded = ctx.block().and(I8, &gc_flags, "128");
    let not_forwarded = ctx.block().icmp_eq(I8, &forwarded, "0");

    // Existing-own overwrite guards. Bit 12 is the per-object typed-layout
    // intact bit: the runtime miss downgrades it before priming this cache, so
    // same-shape siblings take one miss each before direct stores are allowed.
    const BLOCKING_FLAGS: u16 = 0x1907; // frozen/sealed/noextend/TA-proto/descriptors/typed-intact
    let reserved_addr = ctx.block().sub(I64, &safe_target, "6");
    let reserved_ptr = ctx.block().inttoptr(I64, &reserved_addr);
    let reserved = ctx.block().load(I16, &reserved_ptr);
    let blocked = ctx.block().and(I16, &reserved, &BLOCKING_FLAGS.to_string());
    let flags_clear = ctx.block().icmp_eq(I16, &blocked, "0");

    let object_type_ptr = ctx.block().inttoptr(I64, &safe_target);
    let object_type = ctx.block().load(I32, &object_type_ptr);
    let regular = ctx.block().icmp_eq(I32, &object_type, "1");
    let class_addr = ctx.block().add(I64, &safe_target, "4");
    let class_ptr = ctx.block().inttoptr(I64, &class_addr);
    let class_id = ctx.block().load(I32, &class_ptr);
    let class_nonzero = ctx.block().icmp_ne(I32, &class_id, "0");
    let not_native_module = ctx.block().icmp_ne(I32, &class_id, "-2");

    let keys_addr = ctx.block().add(I64, &safe_target, "16");
    let keys_ptr = ctx.block().inttoptr(I64, &keys_addr);
    let keys = ctx.block().load(I64, &keys_ptr);

    // Mirror the read PIC's #6804 discriminated shape token. Plain objects
    // carrying a never-reused runtime ShapeId compare by that stable id,
    // lifted above the pointer range with bit 62. Class instances and
    // unstamped receivers compare by their shared keys pointer. The runtime
    // miss publishes the same token representation.
    let parent_class_addr = ctx.block().add(I64, &safe_target, "8");
    let parent_class_ptr = ctx.block().inttoptr(I64, &parent_class_addr);
    let parent_class_id = ctx.block().load(I32, &parent_class_ptr);
    let shape_id_rel = ctx.block().add(I32, &parent_class_id, "-2147483648");
    let has_shape_id = ctx.block().icmp_ult(I32, &shape_id_rel, "1073741824");
    let shape_id64 = ctx.block().zext(I32, &parent_class_id, I64);
    let shape_id_token = ctx.block().or(I64, &shape_id64, "4611686018427387904");
    let shape_token = ctx
        .block()
        .select(I1, &has_shape_id, I64, &shape_id_token, &keys);
    let cached_token_ptr = ctx.block().gep(I64, &cache_ref, &[(I64, "0")]);
    let cached_token = ctx.block().load(I64, &cached_token_ptr);
    let token_match = ctx.block().icmp_eq(I64, &shape_token, &cached_token);
    let token_nonzero = ctx.block().icmp_ne(I64, &shape_token, "0");

    let cached_slot_ptr = ctx.block().gep(I64, &cache_ref, &[(I64, "1")]);
    let slot = ctx.block().load(I64, &cached_slot_ptr);
    let field_count_addr = ctx.block().add(I64, &safe_target, "12");
    let field_count_ptr = ctx.block().inttoptr(I64, &field_count_addr);
    let field_count = ctx.block().load(I32, &field_count_ptr);
    let field_count64 = ctx.block().zext(I32, &field_count, I64);
    let below_floor = ctx.block().icmp_ult(I64, &field_count64, "4");
    let inline_limit = ctx
        .block()
        .select(I1, &below_floor, I64, "4", &field_count64);
    let slot_in_bounds = ctx.block().icmp_ult(I64, &slot, &inline_limit);

    let mut hit = ctx.block().and(I1, &heap_candidate, &gc_object);
    hit = ctx.block().and(I1, &hit, &not_forwarded);
    hit = ctx.block().and(I1, &hit, &flags_clear);
    hit = ctx.block().and(I1, &hit, &regular);
    hit = ctx.block().and(I1, &hit, &class_nonzero);
    hit = ctx.block().and(I1, &hit, &not_native_module);
    hit = ctx.block().and(I1, &hit, &token_match);
    hit = ctx.block().and(I1, &hit, &token_nonzero);
    hit = ctx.block().and(I1, &hit, &slot_in_bounds);

    ctx.block().cond_br(&hit, &hit_label, &fallback_label);

    // A second bounded cache entry handles stable polymorphism without
    // changing the miss ABI. The first entry is filled initially; only after
    // it contains a different shape do we consult/prime the second entry.
    ctx.current_block = fallback_idx;
    let first_empty = ctx.block().icmp_eq(I64, &cached_token, "0");
    ctx.block()
        .cond_br(&first_empty, &miss_label, &guard2_label);

    ctx.current_block = guard2_idx;
    let cached2_token_ptr = ctx.block().gep(I64, &cache_ref, &[(I64, "2")]);
    let cached2_token = ctx.block().load(I64, &cached2_token_ptr);
    let cached2_slot_ptr = ctx.block().gep(I64, &cache_ref, &[(I64, "3")]);
    let slot2 = ctx.block().load(I64, &cached2_slot_ptr);
    let token2_match = ctx.block().icmp_eq(I64, &shape_token, &cached2_token);
    let token2_nonzero = ctx.block().icmp_ne(I64, &shape_token, "0");
    let slot2_in_bounds = ctx.block().icmp_ult(I64, &slot2, &inline_limit);
    let mut hit2 = ctx.block().and(I1, &heap_candidate, &gc_object);
    hit2 = ctx.block().and(I1, &hit2, &not_forwarded);
    hit2 = ctx.block().and(I1, &hit2, &flags_clear);
    hit2 = ctx.block().and(I1, &hit2, &regular);
    hit2 = ctx.block().and(I1, &hit2, &class_nonzero);
    hit2 = ctx.block().and(I1, &hit2, &not_native_module);
    hit2 = ctx.block().and(I1, &hit2, &token2_match);
    hit2 = ctx.block().and(I1, &hit2, &token2_nonzero);
    hit2 = ctx.block().and(I1, &hit2, &slot2_in_bounds);
    ctx.block().cond_br(&hit2, &hit_label, &dispatch3_label);

    ctx.current_block = dispatch3_idx;
    let second_empty = ctx.block().icmp_eq(I64, &cached2_token, "0");
    ctx.block()
        .cond_br(&second_empty, &miss2_label, &guard3_label);

    ctx.current_block = guard3_idx;
    let cached3_token_ptr = ctx.block().gep(I64, &cache_ref, &[(I64, "4")]);
    let cached3_token = ctx.block().load(I64, &cached3_token_ptr);
    let cached3_slot_ptr = ctx.block().gep(I64, &cache_ref, &[(I64, "5")]);
    let slot3 = ctx.block().load(I64, &cached3_slot_ptr);
    let token3_match = ctx.block().icmp_eq(I64, &shape_token, &cached3_token);
    let token3_nonzero = ctx.block().icmp_ne(I64, &shape_token, "0");
    let slot3_in_bounds = ctx.block().icmp_ult(I64, &slot3, &inline_limit);
    let mut hit3 = ctx.block().and(I1, &heap_candidate, &gc_object);
    hit3 = ctx.block().and(I1, &hit3, &not_forwarded);
    hit3 = ctx.block().and(I1, &hit3, &flags_clear);
    hit3 = ctx.block().and(I1, &hit3, &regular);
    hit3 = ctx.block().and(I1, &hit3, &class_nonzero);
    hit3 = ctx.block().and(I1, &hit3, &not_native_module);
    hit3 = ctx.block().and(I1, &hit3, &token3_match);
    hit3 = ctx.block().and(I1, &hit3, &token3_nonzero);
    hit3 = ctx.block().and(I1, &hit3, &slot3_in_bounds);
    ctx.block().cond_br(&hit3, &hit_label, &dispatch4_label);

    ctx.current_block = dispatch4_idx;
    let third_empty = ctx.block().icmp_eq(I64, &cached3_token, "0");
    ctx.block()
        .cond_br(&third_empty, &miss3_label, &guard4_label);

    ctx.current_block = guard4_idx;
    let cached4_token_ptr = ctx.block().gep(I64, &cache_ref, &[(I64, "6")]);
    let cached4_token = ctx.block().load(I64, &cached4_token_ptr);
    let cached4_slot_ptr = ctx.block().gep(I64, &cache_ref, &[(I64, "7")]);
    let slot4 = ctx.block().load(I64, &cached4_slot_ptr);
    let token4_match = ctx.block().icmp_eq(I64, &shape_token, &cached4_token);
    let token4_nonzero = ctx.block().icmp_ne(I64, &shape_token, "0");
    let slot4_in_bounds = ctx.block().icmp_ult(I64, &slot4, &inline_limit);
    let mut hit4 = ctx.block().and(I1, &heap_candidate, &gc_object);
    hit4 = ctx.block().and(I1, &hit4, &not_forwarded);
    hit4 = ctx.block().and(I1, &hit4, &flags_clear);
    hit4 = ctx.block().and(I1, &hit4, &regular);
    hit4 = ctx.block().and(I1, &hit4, &class_nonzero);
    hit4 = ctx.block().and(I1, &hit4, &not_native_module);
    hit4 = ctx.block().and(I1, &hit4, &token4_match);
    hit4 = ctx.block().and(I1, &hit4, &token4_nonzero);
    hit4 = ctx.block().and(I1, &hit4, &slot4_in_bounds);
    ctx.block().cond_br(&hit4, &hit_label, &miss4_label);

    ctx.current_block = hit_idx;
    let selected_slot = ctx.block().phi(
        I64,
        &[
            (&slot, &guard_label),
            (&slot2, &guard2_label),
            (&slot3, &guard3_label),
            (&slot4, &guard4_label),
        ],
    );

    let pointer_possible = !(is_numeric_expr(ctx, value)
        || expr_produces_non_pointer_bits_by_construction(ctx, value));
    {
        let header_size =
            crate::target_layout::object_header_size_bytes(ctx.target_triple).to_string();
        let blk = ctx.block();
        let slot_offset = blk.shl(I64, &selected_slot, "3");
        let fields_base = blk.add(I64, &target_handle, &header_size);
        let field_addr = blk.add(I64, &fields_base, &slot_offset);
        let field_ptr = blk.inttoptr(I64, &field_addr);
        if pointer_possible {
            let slot_i32 = blk.trunc(I64, &selected_slot, I32);
            emit_jsvalue_slot_store_scalar_aware_on_block(
                blk,
                &field_ptr,
                &stored_value,
                &target_handle,
                &slot_i32,
                true,
                &target_bits,
                &field_addr,
                true,
            );
        } else {
            // A non-pointer overwrite cannot create a young edge or make a GC
            // pointer layout less conservative. The per-object typed layout
            // bit was already cleared on the miss that primed this cache.
            // GC_STORE_AUDIT(POINTER_FREE): this branch only stores a value
            // proven unable to contain GC pointer bits.
            blk.store(DOUBLE, &stored_value, &field_ptr);
        }
        blk.br(&merge_label);
    }
    let hit_end_label = ctx.block().label.clone();

    ctx.current_block = miss_idx;
    let strict_i32 = if strict { "1" } else { "0" };
    let miss_value = ctx.block().call(
        DOUBLE,
        "js_put_value_set_ic_miss",
        &[
            (DOUBLE, &target_value),
            (I64, &key_handle),
            (DOUBLE, &stored_value),
            (I32, strict_i32),
            (PTR, &cache_ref),
        ],
    );
    let miss_end_label = ctx.block().label.clone();
    ctx.block().br(&merge_label);

    ctx.current_block = miss2_idx;
    let miss2_value = ctx.block().call(
        DOUBLE,
        "js_put_value_set_ic_miss",
        &[
            (DOUBLE, &target_value),
            (I64, &key_handle),
            (DOUBLE, &stored_value),
            (I32, strict_i32),
            (PTR, &cached2_token_ptr),
        ],
    );
    let miss2_end_label = ctx.block().label.clone();
    ctx.block().br(&merge_label);

    ctx.current_block = miss3_idx;
    let miss3_value = ctx.block().call(
        DOUBLE,
        "js_put_value_set_ic_miss",
        &[
            (DOUBLE, &target_value),
            (I64, &key_handle),
            (DOUBLE, &stored_value),
            (I32, strict_i32),
            (PTR, &cached3_token_ptr),
        ],
    );
    let miss3_end_label = ctx.block().label.clone();
    ctx.block().br(&merge_label);

    ctx.current_block = miss4_idx;
    let miss4_value = ctx.block().call(
        DOUBLE,
        "js_put_value_set_ic_miss",
        &[
            (DOUBLE, &target_value),
            (I64, &key_handle),
            (DOUBLE, &stored_value),
            (I32, strict_i32),
            (PTR, &cached4_token_ptr),
        ],
    );
    let miss4_end_label = ctx.block().label.clone();
    ctx.block().br(&merge_label);

    ctx.current_block = merge_idx;
    let result = ctx.block().phi(
        DOUBLE,
        &[
            (&stored_value, &hit_end_label),
            (&miss_value, &miss_end_label),
            (&miss2_value, &miss2_end_label),
            (&miss3_value, &miss3_end_label),
            (&miss4_value, &miss4_end_label),
        ],
    );
    Ok(Some(result))
}

fn static_write_key(ctx: &FnCtx<'_>, key: &Expr) -> Option<String> {
    match key {
        Expr::String(property) => Some(property.clone()),
        Expr::LocalGet(id) => ctx.const_string_locals.get(id).cloned(),
        _ => None,
    }
}

fn put_value_rhs_is_safepoint_free(ctx: &FnCtx<'_>, expr: &Expr) -> bool {
    match expr {
        Expr::LocalGet(_)
        | Expr::Number(_)
        | Expr::Integer(_)
        | Expr::Bool(_)
        | Expr::Null
        | Expr::Undefined
        | Expr::String(_) => true,
        Expr::Binary { left, right, .. } if is_numeric_expr(ctx, expr) => {
            put_value_rhs_is_safepoint_free(ctx, left)
                && put_value_rhs_is_safepoint_free(ctx, right)
        }
        _ => false,
    }
}

fn same_side_effect_free_receiver(target: &Expr, receiver: &Expr) -> bool {
    match (target, receiver) {
        (Expr::LocalGet(id), Expr::LocalGet(receiver_id)) => id == receiver_id,
        (Expr::This, Expr::This) => true,
        (
            Expr::PropertyGet {
                object, property, ..
            },
            Expr::PropertyGet {
                object: receiver_object,
                property: receiver_property,
                ..
            },
        ) => {
            property == receiver_property
                && same_side_effect_free_receiver(object.as_ref(), receiver_object.as_ref())
        }
        _ => false,
    }
}

fn same_put_value_receiver_expr(target: &Expr, receiver: &Expr) -> bool {
    match (target, receiver) {
        (Expr::Undefined, Expr::Undefined)
        | (Expr::Null, Expr::Null)
        | (Expr::This, Expr::This) => true,
        (Expr::Bool(a), Expr::Bool(b)) => a == b,
        (Expr::Number(a), Expr::Number(b)) => a.to_bits() == b.to_bits(),
        (Expr::Integer(a), Expr::Integer(b)) => a == b,
        (Expr::BigInt(a), Expr::BigInt(b))
        | (Expr::String(a), Expr::String(b))
        | (Expr::NativeModuleRef(a), Expr::NativeModuleRef(b)) => a == b,
        (Expr::LocalGet(a), Expr::LocalGet(b)) => a == b,
        (Expr::GlobalGet(a), Expr::GlobalGet(b)) => a == b,
        (Expr::FuncRef(a), Expr::FuncRef(b)) => a == b,
        (
            Expr::ExternFuncRef {
                name: a_name,
                param_types: a_params,
                return_type: a_return,
            },
            Expr::ExternFuncRef {
                name: b_name,
                param_types: b_params,
                return_type: b_return,
            },
        ) => a_name == b_name && a_params == b_params && a_return == b_return,
        (
            Expr::Call {
                callee: a_callee,
                args: a_args,
                type_args: a_type_args,
                ..
            },
            Expr::Call {
                callee: b_callee,
                args: b_args,
                type_args: b_type_args,
                ..
            },
        ) => {
            a_type_args == b_type_args
                && same_put_value_receiver_expr(a_callee, b_callee)
                && a_args.len() == b_args.len()
                && a_args
                    .iter()
                    .zip(b_args.iter())
                    .all(|(a, b)| same_put_value_receiver_expr(a, b))
        }
        (
            Expr::NativeMethodCall {
                module: a_module,
                class_name: a_class,
                object: a_object,
                method: a_method,
                args: a_args,
            },
            Expr::NativeMethodCall {
                module: b_module,
                class_name: b_class,
                object: b_object,
                method: b_method,
                args: b_args,
            },
        ) => {
            a_module == b_module
                && a_class == b_class
                && a_method == b_method
                && match (a_object, b_object) {
                    (Some(a), Some(b)) => same_put_value_receiver_expr(a, b),
                    (None, None) => true,
                    _ => false,
                }
                && a_args.len() == b_args.len()
                && a_args
                    .iter()
                    .zip(b_args.iter())
                    .all(|(a, b)| same_put_value_receiver_expr(a, b))
        }
        (
            Expr::PropertyGet {
                object: a_object,
                property: a_property,
                ..
            },
            Expr::PropertyGet {
                object: b_object,
                property: b_property,
                ..
            },
        ) => a_property == b_property && same_put_value_receiver_expr(a_object, b_object),
        (
            Expr::IndexGet {
                object: a_object,
                index: a_index,
            },
            Expr::IndexGet {
                object: b_object,
                index: b_index,
            },
        ) => {
            same_put_value_receiver_expr(a_object, b_object)
                && same_put_value_receiver_expr(a_index, b_index)
        }
        _ => false,
    }
}

fn is_numeric_string_key(key: &str) -> bool {
    !key.is_empty()
        && key.chars().all(|c| c.is_ascii_digit())
        && !(key.len() > 1 && key.starts_with('0'))
}

fn put_value_index_fast_path(ctx: &FnCtx<'_>, target: &Expr, key: &Expr, receiver: &Expr) -> bool {
    if !same_side_effect_free_receiver(target, receiver) {
        return false;
    }
    if is_array_expr(ctx, target) {
        return match key {
            Expr::String(key) => is_numeric_string_key(key),
            _ => true,
        };
    }
    // #5525: `P[i] = v` where `P` is an *untyped* (`Type::Any`/`Type::Unknown`)
    // receiver. The desugared `PutValueSet` otherwise falls through to the
    // generic `js_put_value_set` ([[Set]] via `ordinary_set_with_receiver` →
    // stringify-the-index object dispatch), which dominated the bcrypt write
    // profile. Routing it to `index_set::lower` instead reaches that file's
    // matching `recv_unknown` arm, which emits `js_dyn_index_set` — carrying the
    // #5525 process-global typed-array kind cache + inline
    // `typed_array_fast_index_set` fast path. This is the write counterpart of
    // the IndexGet `recv_unknown → js_dyn_index_get` route that bcryptjs's
    // Blowfish `Int32Array` P/S boxes (reached through untyped `Array.<number>`
    // params) need. `js_dyn_index_set` carries the full spec dispatch (typed-
    // array per-kind store, plain-array extend, object by-name set, symbol side-
    // table) for the cases the fast path defers, so the only keys we keep off it
    // are statically-known string-literals / symbols (their interned-handle /
    // symbol-side-table routes below are already optimal). Every statically-
    // typed receiver is unaffected — `recv_unknown` is false for them.
    let recv_unknown = matches!(
        crate::type_analysis::static_type_of(ctx, target),
        None | Some(perry_hir::types::Type::Any) | Some(perry_hir::types::Type::Unknown)
    );
    // Mirror `index_set::lower`'s `recv_unknown` arm: keep statically-known
    // string-literal / symbol keys on their dedicated routes; route everything
    // else (numeric, runtime-string, or an unknown-typed index like bcryptjs's
    // `off + 1` where `off` is an `any` param) to `index_set::lower`, which emits
    // the `js_dyn_index_set` fast path. The earlier `is_numeric_expr(key)` gate
    // missed `off + 1` and those ~4M hot `lr[...]` writes stayed on
    // `js_put_value_set`.
    let key_is_static_string_or_symbol = matches!(
        key,
        Expr::String(_) | Expr::WtfString(_) | Expr::SymbolFor(_)
    ) || is_string_expr(ctx, key);
    recv_unknown && !key_is_static_string_or_symbol
}

fn try_lower_process_env_put_value_set(
    ctx: &mut FnCtx<'_>,
    target: &Expr,
    key: &Expr,
    value: &Expr,
    receiver: &Expr,
) -> Result<Option<String>> {
    if !matches!(target, Expr::ProcessEnv) || !matches!(receiver, Expr::ProcessEnv) {
        return Ok(None);
    }

    let key_handle = match key {
        Expr::String(property) => {
            let key_idx = ctx.strings.intern(property);
            let key_handle_global = format!("@{}", ctx.strings.entry(key_idx).handle_global);
            let blk = ctx.block();
            let key_box = blk.load(DOUBLE, &key_handle_global);
            unbox_to_i64(blk, &key_box)
        }
        _ => {
            let key_box = lower_expr(ctx, key)?;
            let blk = ctx.block();
            let property_key = blk.call(DOUBLE, "js_to_property_key", &[(DOUBLE, &key_box)]);
            unbox_str_handle(blk, &property_key)
        }
    };
    let val_double = lower_expr(ctx, value)?;
    ctx.block()
        .call_void("js_setenv", &[(I64, &key_handle), (DOUBLE, &val_double)]);
    Ok(Some(val_double))
}

pub(crate) fn lower(ctx: &mut FnCtx<'_>, expr: &Expr) -> Result<String> {
    match expr {
        Expr::ProxyNew { target, handler } => {
            downgrade_unknown_call_expr(ctx, target);
            downgrade_unknown_call_expr(ctx, handler);
            let t = lower_expr(ctx, target)?;
            let h = lower_expr(ctx, handler)?;
            Ok(ctx
                .block()
                .call(DOUBLE, "js_proxy_new", &[(DOUBLE, &t), (DOUBLE, &h)]))
        }
        Expr::ProxyGet { proxy, key } => {
            downgrade_unknown_call_expr(ctx, proxy);
            downgrade_unknown_call_expr(ctx, key);
            let p = lower_expr(ctx, proxy)?;
            let k = lower_expr(ctx, key)?;
            Ok(ctx
                .block()
                .call(DOUBLE, "js_proxy_get", &[(DOUBLE, &p), (DOUBLE, &k)]))
        }
        Expr::ProxySet { proxy, key, value } => {
            downgrade_unknown_call_expr(ctx, proxy);
            downgrade_unknown_call_expr(ctx, key);
            downgrade_unknown_call_expr(ctx, value);
            let p = lower_expr(ctx, proxy)?;
            let k = lower_expr(ctx, key)?;
            let v = lower_expr(ctx, value)?;
            let _ = ctx.block().call(
                DOUBLE,
                "js_proxy_set",
                &[(DOUBLE, &p), (DOUBLE, &k), (DOUBLE, &v)],
            );
            Ok(v)
        }
        Expr::ProxyHas { proxy, key } => {
            downgrade_unknown_call_expr(ctx, proxy);
            downgrade_unknown_call_expr(ctx, key);
            let p = lower_expr(ctx, proxy)?;
            let k = lower_expr(ctx, key)?;
            Ok(ctx
                .block()
                .call(DOUBLE, "js_proxy_has", &[(DOUBLE, &p), (DOUBLE, &k)]))
        }
        Expr::ProxyDelete { proxy, key } => {
            downgrade_unknown_call_expr(ctx, proxy);
            downgrade_unknown_call_expr(ctx, key);
            let strict = if ctx.is_strict_fn { "1" } else { "0" };
            let p = lower_expr(ctx, proxy)?;
            let k = lower_expr(ctx, key)?;
            let blk = ctx.block();
            // `js_proxy_delete` reports the `[[Delete]]` boolean; a strict-mode
            // `delete proxy.key` that resolves to `false` (non-configurable
            // property, forwarded through the trap chain) must throw a TypeError
            // just like the ordinary member-delete path. Route the boolean
            // through `js_delete_result` so both modes match spec (test262
            // Proxy/deleteProperty/*-target-is-proxy `delete funcProxy.prototype`
            // under "use strict").
            let deleted_box = blk.call(DOUBLE, "js_proxy_delete", &[(DOUBLE, &p), (DOUBLE, &k)]);
            let deleted_i32 = blk.call(I32, "js_is_truthy", &[(DOUBLE, &deleted_box)]);
            Ok(blk.call(
                DOUBLE,
                "js_delete_result",
                &[(I32, &deleted_i32), (I32, strict)],
            ))
        }
        Expr::ProxyApply { proxy, args } => {
            downgrade_unknown_call_expr(ctx, proxy);
            downgrade_unknown_call_args(ctx, args);
            let p = lower_expr(ctx, proxy)?;
            let arr_handle = proxy_build_args_array(ctx, args)?;
            let blk = ctx.block();
            let arr_box = nanbox_pointer_inline(blk, &arr_handle);
            let undef = double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED));
            Ok(ctx.block().call(
                DOUBLE,
                "js_proxy_apply",
                &[(DOUBLE, &p), (DOUBLE, &undef), (DOUBLE, &arr_box)],
            ))
        }
        Expr::ProxyConstruct { proxy, args } => {
            downgrade_unknown_call_expr(ctx, proxy);
            downgrade_unknown_call_args(ctx, args);
            let p = lower_expr(ctx, proxy)?;
            let arr_handle = proxy_build_args_array(ctx, args)?;
            let blk = ctx.block();
            let arr_box = nanbox_pointer_inline(blk, &arr_handle);
            let undef = double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED));
            Ok(ctx.block().call(
                DOUBLE,
                "js_proxy_construct",
                &[(DOUBLE, &p), (DOUBLE, &arr_box), (DOUBLE, &undef)],
            ))
        }
        Expr::ProxyRevocable { target, handler } => {
            // #2846: return a real `{ proxy, revoke }` record so `typeof
            // rec.revoke === "function"`, `rec.proxy.a` forwards, and the
            // revoke function survives aliasing/storage.
            downgrade_unknown_call_expr(ctx, target);
            downgrade_unknown_call_expr(ctx, handler);
            let t = lower_expr(ctx, target)?;
            let h = lower_expr(ctx, handler)?;
            Ok(ctx
                .block()
                .call(DOUBLE, "js_proxy_revocable", &[(DOUBLE, &t), (DOUBLE, &h)]))
        }
        Expr::ProxyRevoke(proxy) => {
            downgrade_unknown_call_expr(ctx, proxy);
            let p = lower_expr(ctx, proxy)?;
            ctx.block().call_void("js_proxy_revoke", &[(DOUBLE, &p)]);
            Ok(double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED)))
        }
        Expr::ReflectGet {
            target,
            key,
            receiver,
        } => {
            // #2766: pass the optional receiver through; the runtime defaults
            // an `undefined` receiver to the target and binds it as `this` for
            // accessor getters.
            downgrade_unknown_call_expr(ctx, target);
            downgrade_unknown_call_expr(ctx, key);
            downgrade_unknown_call_expr(ctx, receiver);
            let t = lower_expr(ctx, target)?;
            let k = lower_expr(ctx, key)?;
            let r = lower_expr(ctx, receiver)?;
            Ok(ctx.block().call(
                DOUBLE,
                "js_reflect_get",
                &[(DOUBLE, &t), (DOUBLE, &k), (DOUBLE, &r)],
            ))
        }
        Expr::ReflectSet {
            target,
            key,
            value,
            receiver,
        } => {
            // Pass the optional receiver through; the runtime defaults an
            // `undefined` receiver to the target. A receiver distinct from an
            // Integer-Indexed target redirects the write to the receiver per
            // OrdinarySet (test262 internals/Set/key-is-valid-index-reflect-set).
            downgrade_unknown_call_expr(ctx, target);
            downgrade_unknown_call_expr(ctx, key);
            downgrade_unknown_call_expr(ctx, value);
            downgrade_unknown_call_expr(ctx, receiver);
            let t = lower_expr(ctx, target)?;
            let k = lower_expr(ctx, key)?;
            let v = lower_expr(ctx, value)?;
            let r = lower_expr(ctx, receiver)?;
            Ok(ctx.block().call(
                DOUBLE,
                "js_reflect_set",
                &[(DOUBLE, &t), (DOUBLE, &k), (DOUBLE, &v), (DOUBLE, &r)],
            ))
        }
        Expr::PutValueSet {
            target,
            key,
            value,
            receiver,
            strict,
        } => {
            if let Some(value) =
                try_lower_process_env_put_value_set(ctx, target, key, value, receiver)?
            {
                return Ok(value);
            }
            if let Expr::String(property) = key.as_ref() {
                if matches!(property.as_str(), "caller" | "arguments")
                    && same_side_effect_free_receiver(target, receiver)
                {
                    return super::property_set::lower(
                        ctx,
                        &Expr::PropertySet {
                            object: target.clone(),
                            property: property.clone(),
                            value: value.clone(),
                        },
                    );
                }
            }
            if let Some(property) =
                put_value_static_property_fast_path(ctx, target, key, receiver, *strict)
            {
                return super::property_set::lower(
                    ctx,
                    &Expr::PropertySet {
                        object: target.clone(),
                        property,
                        value: value.clone(),
                    },
                );
            }
            if put_value_index_fast_path(ctx, target, key, receiver) {
                return super::index_set::lower(
                    ctx,
                    &Expr::IndexSet {
                        object: target.clone(),
                        index: key.clone(),
                        value: value.clone(),
                    },
                );
            }
            if let Some(result) =
                lower_put_value_static_write_ic(ctx, target, key, value, receiver, *strict)?
            {
                return Ok(result);
            }
            downgrade_unknown_call_expr(ctx, target);
            downgrade_unknown_call_expr(ctx, key);
            downgrade_unknown_call_expr(ctx, value);
            downgrade_unknown_call_expr(ctx, receiver);
            let t = lower_expr(ctx, target)?;
            let k = lower_expr(ctx, key)?;
            let v = lower_expr(ctx, value)?;
            let r = if same_put_value_receiver_expr(target, receiver) {
                t.clone()
            } else {
                lower_expr(ctx, receiver)?
            };
            let strict_i32 = if *strict { "1" } else { "0" };
            Ok(ctx.block().call(
                DOUBLE,
                "js_put_value_set",
                &[
                    (DOUBLE, &t),
                    (DOUBLE, &k),
                    (DOUBLE, &v),
                    (DOUBLE, &r),
                    (I32, strict_i32),
                ],
            ))
        }
        Expr::ReflectHas { target, key } => {
            downgrade_unknown_call_expr(ctx, target);
            downgrade_unknown_call_expr(ctx, key);
            let t = lower_expr(ctx, target)?;
            let k = lower_expr(ctx, key)?;
            Ok(ctx
                .block()
                .call(DOUBLE, "js_reflect_has", &[(DOUBLE, &t), (DOUBLE, &k)]))
        }
        Expr::ReflectDelete { target, key } => {
            downgrade_unknown_call_expr(ctx, target);
            downgrade_unknown_call_expr(ctx, key);
            let t = lower_expr(ctx, target)?;
            let k = lower_expr(ctx, key)?;
            Ok(ctx
                .block()
                .call(DOUBLE, "js_reflect_delete", &[(DOUBLE, &t), (DOUBLE, &k)]))
        }
        Expr::ReflectOwnKeys(target) => {
            downgrade_unknown_call_expr(ctx, target);
            let t = lower_expr(ctx, target)?;
            Ok(ctx
                .block()
                .call(DOUBLE, "js_reflect_own_keys", &[(DOUBLE, &t)]))
        }
        Expr::ReflectApply {
            func,
            this_arg,
            args,
        } => {
            downgrade_unknown_call_expr(ctx, func);
            downgrade_unknown_call_expr(ctx, this_arg);
            downgrade_unknown_call_expr(ctx, args);
            let f = lower_expr(ctx, func)?;
            let ta = lower_expr(ctx, this_arg)?;
            let a = lower_expr(ctx, args)?;
            Ok(ctx.block().call(
                DOUBLE,
                "js_reflect_apply",
                &[(DOUBLE, &f), (DOUBLE, &ta), (DOUBLE, &a)],
            ))
        }
        Expr::ReflectConstruct {
            target,
            args,
            new_target,
        } => {
            downgrade_unknown_call_expr(ctx, target);
            downgrade_unknown_call_expr(ctx, args);
            downgrade_unknown_call_expr(ctx, new_target);
            let t = lower_expr(ctx, target)?;
            let a = lower_expr(ctx, args)?;
            let nt = lower_expr(ctx, new_target)?;
            let result = ctx.block().call(
                DOUBLE,
                "js_reflect_construct",
                &[(DOUBLE, &t), (DOUBLE, &a), (DOUBLE, &nt)],
            );
            // Write-back captured outer locals: when `target` is a
            // statically-known user class, the constructor body stores
            // mutations to `this.__perry_cap_*` but can't reach the
            // caller's outer alloca slots. Read the fields back here
            // (e.g. `++called` in a subclass constructor is visible
            // after `Reflect.construct(Sub, args)` returns).
            let class_name: Option<String> = match target.as_ref() {
                Expr::ClassRef(cn) => Some(cn.clone()),
                Expr::LocalGet(id) => ctx
                    .local_id_to_name
                    .get(id)
                    .and_then(|name| ctx.local_class_aliases.get(name))
                    .cloned(),
                _ => None,
            };
            if let Some(cn) = class_name {
                if let Some(class) = ctx.classes.get(cn.as_str()).copied() {
                    let bits = ctx.block().bitcast_double_to_i64(&result);
                    let inst_handle = ctx.block().and(I64, &bits, POINTER_MASK_I64);
                    crate::lower_call::emit_class_capture_writeback(ctx, class, &inst_handle, &[]);
                }
            }
            Ok(result)
        }
        Expr::ReflectDefineProperty {
            target,
            key,
            descriptor,
        } => {
            downgrade_unknown_call_expr(ctx, target);
            downgrade_unknown_call_expr(ctx, key);
            downgrade_unknown_call_expr(ctx, descriptor);
            let t = lower_expr(ctx, target)?;
            let k = lower_expr(ctx, key)?;
            let d = lower_expr(ctx, descriptor)?;
            Ok(ctx.block().call(
                DOUBLE,
                "js_reflect_define_property",
                &[(DOUBLE, &t), (DOUBLE, &k), (DOUBLE, &d)],
            ))
        }
        Expr::ReflectGetOwnPropertyDescriptor { target, key } => {
            downgrade_unknown_call_expr(ctx, target);
            downgrade_unknown_call_expr(ctx, key);
            let t = lower_expr(ctx, target)?;
            let k = lower_expr(ctx, key)?;
            Ok(ctx.block().call(
                DOUBLE,
                "js_reflect_get_own_property_descriptor",
                &[(DOUBLE, &t), (DOUBLE, &k)],
            ))
        }
        Expr::ReflectSetPrototypeOf { target, proto } => {
            // #2761: Reflect-specific boolean result (false on rejected change)
            // + TypeError on bad args, distinct from Object.setPrototypeOf.
            downgrade_unknown_call_expr(ctx, target);
            downgrade_unknown_call_expr(ctx, proto);
            let t = lower_expr(ctx, target)?;
            let p = lower_expr(ctx, proto)?;
            Ok(ctx.block().call(
                DOUBLE,
                "js_reflect_set_prototype_of",
                &[(DOUBLE, &t), (DOUBLE, &p)],
            ))
        }
        Expr::ReflectGetPrototypeOf(target) => {
            // #2757: return the actual [[Prototype]] (shared with
            // Object.getPrototypeOf), not the target object itself. The
            // `=== Class.prototype` comparison is still folded to a constant
            // bool at lowering time (lower_expr.rs); this path handles every
            // other (value-returning) use.
            downgrade_unknown_call_expr(ctx, target);
            let t = lower_expr(ctx, target)?;
            Ok(ctx
                .block()
                .call(DOUBLE, "js_reflect_get_prototype_of", &[(DOUBLE, &t)]))
        }
        Expr::ReflectIsExtensible(target) => {
            // #2762: Reflect-specific — boolean result + TypeError on
            // non-object, distinct from Object.isExtensible.
            downgrade_unknown_call_expr(ctx, target);
            let t = lower_expr(ctx, target)?;
            Ok(ctx
                .block()
                .call(DOUBLE, "js_reflect_is_extensible", &[(DOUBLE, &t)]))
        }
        Expr::ReflectPreventExtensions(target) => {
            // #2762: Reflect-specific — boolean result + TypeError on
            // non-object, distinct from Object.preventExtensions (which
            // returns the object).
            downgrade_unknown_call_expr(ctx, target);
            let t = lower_expr(ctx, target)?;
            Ok(ctx
                .block()
                .call(DOUBLE, "js_reflect_prevent_extensions", &[(DOUBLE, &t)]))
        }
        Expr::ReflectDefineMetadata {
            key,
            value,
            target,
            property_key,
        } => {
            downgrade_unknown_call_expr(ctx, key);
            downgrade_unknown_call_expr(ctx, value);
            downgrade_unknown_call_expr(ctx, target);
            if let Some(property_key) = property_key {
                downgrade_unknown_call_expr(ctx, property_key);
            }
            let k = lower_expr(ctx, key)?;
            let v = lower_expr(ctx, value)?;
            let t = lower_expr(ctx, target)?;
            let p = property_key
                .as_ref()
                .map(|p| lower_expr(ctx, p))
                .transpose()?
                .unwrap_or_else(|| double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED)));
            Ok(ctx.block().call(
                DOUBLE,
                "js_reflect_define_metadata",
                &[(DOUBLE, &k), (DOUBLE, &v), (DOUBLE, &t), (DOUBLE, &p)],
            ))
        }
        Expr::ReflectGetMetadata {
            key,
            target,
            property_key,
        } => {
            downgrade_unknown_call_expr(ctx, key);
            downgrade_unknown_call_expr(ctx, target);
            if let Some(property_key) = property_key {
                downgrade_unknown_call_expr(ctx, property_key);
            }
            let k = lower_expr(ctx, key)?;
            let t = lower_expr(ctx, target)?;
            let p = property_key
                .as_ref()
                .map(|p| lower_expr(ctx, p))
                .transpose()?
                .unwrap_or_else(|| double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED)));
            Ok(ctx.block().call(
                DOUBLE,
                "js_reflect_get_metadata",
                &[(DOUBLE, &k), (DOUBLE, &t), (DOUBLE, &p)],
            ))
        }
        Expr::ReflectGetOwnMetadata {
            key,
            target,
            property_key,
        } => {
            downgrade_unknown_call_expr(ctx, key);
            downgrade_unknown_call_expr(ctx, target);
            if let Some(property_key) = property_key {
                downgrade_unknown_call_expr(ctx, property_key);
            }
            let k = lower_expr(ctx, key)?;
            let t = lower_expr(ctx, target)?;
            let p = property_key
                .as_ref()
                .map(|p| lower_expr(ctx, p))
                .transpose()?
                .unwrap_or_else(|| double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED)));
            Ok(ctx.block().call(
                DOUBLE,
                "js_reflect_get_own_metadata",
                &[(DOUBLE, &k), (DOUBLE, &t), (DOUBLE, &p)],
            ))
        }
        Expr::ReflectHasMetadata {
            key,
            target,
            property_key,
        } => {
            downgrade_unknown_call_expr(ctx, key);
            downgrade_unknown_call_expr(ctx, target);
            if let Some(property_key) = property_key {
                downgrade_unknown_call_expr(ctx, property_key);
            }
            let k = lower_expr(ctx, key)?;
            let t = lower_expr(ctx, target)?;
            let p = property_key
                .as_ref()
                .map(|p| lower_expr(ctx, p))
                .transpose()?
                .unwrap_or_else(|| double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED)));
            Ok(ctx.block().call(
                DOUBLE,
                "js_reflect_has_metadata",
                &[(DOUBLE, &k), (DOUBLE, &t), (DOUBLE, &p)],
            ))
        }
        Expr::ReflectHasOwnMetadata {
            key,
            target,
            property_key,
        } => {
            downgrade_unknown_call_expr(ctx, key);
            downgrade_unknown_call_expr(ctx, target);
            if let Some(property_key) = property_key {
                downgrade_unknown_call_expr(ctx, property_key);
            }
            let k = lower_expr(ctx, key)?;
            let t = lower_expr(ctx, target)?;
            let p = property_key
                .as_ref()
                .map(|p| lower_expr(ctx, p))
                .transpose()?
                .unwrap_or_else(|| double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED)));
            Ok(ctx.block().call(
                DOUBLE,
                "js_reflect_has_own_metadata",
                &[(DOUBLE, &k), (DOUBLE, &t), (DOUBLE, &p)],
            ))
        }
        Expr::ReflectGetMetadataKeys {
            target,
            property_key,
        } => {
            downgrade_unknown_call_expr(ctx, target);
            if let Some(property_key) = property_key {
                downgrade_unknown_call_expr(ctx, property_key);
            }
            let t = lower_expr(ctx, target)?;
            let p = property_key
                .as_ref()
                .map(|p| lower_expr(ctx, p))
                .transpose()?
                .unwrap_or_else(|| double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED)));
            Ok(ctx.block().call(
                DOUBLE,
                "js_reflect_get_metadata_keys",
                &[(DOUBLE, &t), (DOUBLE, &p)],
            ))
        }
        Expr::ReflectGetOwnMetadataKeys {
            target,
            property_key,
        } => {
            downgrade_unknown_call_expr(ctx, target);
            if let Some(property_key) = property_key {
                downgrade_unknown_call_expr(ctx, property_key);
            }
            let t = lower_expr(ctx, target)?;
            let p = property_key
                .as_ref()
                .map(|p| lower_expr(ctx, p))
                .transpose()?
                .unwrap_or_else(|| double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED)));
            Ok(ctx.block().call(
                DOUBLE,
                "js_reflect_get_own_metadata_keys",
                &[(DOUBLE, &t), (DOUBLE, &p)],
            ))
        }
        Expr::ReflectDeleteMetadata {
            key,
            target,
            property_key,
        } => {
            downgrade_unknown_call_expr(ctx, key);
            downgrade_unknown_call_expr(ctx, target);
            if let Some(property_key) = property_key {
                downgrade_unknown_call_expr(ctx, property_key);
            }
            let k = lower_expr(ctx, key)?;
            let t = lower_expr(ctx, target)?;
            let p = property_key
                .as_ref()
                .map(|p| lower_expr(ctx, p))
                .transpose()?
                .unwrap_or_else(|| double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED)));
            Ok(ctx.block().call(
                DOUBLE,
                "js_reflect_delete_metadata",
                &[(DOUBLE, &k), (DOUBLE, &t), (DOUBLE, &p)],
            ))
        }

        // Issue #100: compile-time-resolved dynamic `import()`.
        //
        // The resolver in `collect_modules` already registered each
        // target path as a regular import edge (marked `is_dynamic`),
        // so the target's `__perry_init_<prefix>` runs as part of the
        // eager init chain BEFORE this dispatch site fires. The
        // populator at the end of that init has built the target's
        // `@__perry_ns_<prefix>` global; we just load it here, wrap in
        // a resolved Promise, and return.
        //
        // Single-path: emit a static load + `js_promise_resolved`.
        // Multi-path: evaluate the runtime path string, compare against
        // each compile-time constant via `js_string_equals`, and
        // dispatch to that target's namespace global. Falls through to
        // `js_promise_rejected(TypeError)` on no-match.
        _ => unreachable!("expr/mod.rs dispatched a variant not handled by this submodule"),
    }
}
