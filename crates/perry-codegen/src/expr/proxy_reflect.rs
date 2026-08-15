//! Proxy / Reflect metaprogramming.
//!
//! Extracted from `expr/mod.rs` to keep that file under the 2000-line cap.
//!
//! # Layer 1 rooting (#7615 slice 7)
//!
//! Before this slice exactly one arm in the file made a rooting decision — the
//! `PutValueSet` write-IC, which #7201 fixed — and the other **twenty-eight**
//! made none. Every `Proxy.*` and `Reflect.*` entry point is the same shape:
//! two to four operands, each an arbitrary user expression, lowered in order
//! and then handed together to one runtime helper. `Reflect.has(target, key)`
//! lowered `target`, then lowered `key` — which can run a `Symbol.toPrimitive`,
//! a getter, or any other JS — and then passed the pre-collection `target`
//! register to `js_reflect_has`. That is #7280 taxonomy (c), operand-to-operand,
//! and `root_reload` cannot repair it.
//!
//! The arms are now `crate::rooting::with_operands_rooted` and read as an
//! operand list plus a consuming call. Four shapes needed more than that and
//! are commented where they sit:
//!
//! * `Proxy.apply` / `Proxy.construct` and the two proxy-callee lowerers build
//!   an **argument array** while lowering the arguments, so they take a
//!   [`crate::rooting::RootedGroup`] that holds the receiver and the
//!   accumulator in one scope (the #7154 shape — `proxy_build_args_array`
//!   threaded a raw `*mut ArrayHeader` through its push loop in a bare SSA
//!   register, and is deleted here in favour of the group);
//! * `process.env[k] = v` stripped its key to a **raw** `StringHeader*` above
//!   the value's lowering, which is taxonomy (a);
//! * the eight `reflect-metadata` arms had eight copies of one body and now
//!   share [`lower_reflect_metadata`];
//! * `Reflect.construct`'s capture write-back reads the call's own result, so
//!   it needs no operand root — it is below every lowering.

use anyhow::Result;
use perry_hir::Expr;

use crate::nanbox::{double_literal, POINTER_MASK_I64};
use crate::native_value::MaterializationReason;
use crate::rooting::{any_operand_may_collect, with_operands_rooted, with_rooted_group, Repr};
use crate::type_analysis::{is_array_expr, is_numeric_expr, is_string_expr, receiver_class_name};
use crate::types::{LlvmType, DOUBLE, I1, I16, I32, I64, I8, PTR};

use super::{
    downgrade_buffer_aliases_in_expr, emit_jsvalue_slot_store_scalar_aware_on_block,
    expr_produces_non_pointer_bits_by_construction, lower_expr, nanbox_pointer_inline,
    unbox_str_handle, unbox_to_i64, FnCtx,
};

/// Runtime write-PIC flags that force the miss path. Class-vs-instance kind is
/// encoded by the authoritative ShapeId and therefore owns no header flag.
const WRITE_PIC_BLOCKING_FLAGS: u16 = 0x1907;

/// #8098: `GcHeader::_reserved` bit 9 — the runtime birth-marked this
/// class-less receiver an ORDINARY plain object (`JSON.parse` output), so it is
/// eligible for the write PIC exactly like a class instance. MUST equal
/// `perry_runtime::gc::OBJ_FLAG_PLAIN_ORDINARY`; the runtime pins the value in
/// `proxy::tests::plain_ordinary_object_flag_matches_the_emitted_write_pic_literal`.
/// It is deliberately NOT in `WRITE_PIC_BLOCKING_FLAGS` — this bit ADMITS a
/// receiver, the blocking mask REJECTS one.
const PLAIN_ORDINARY_OBJ_FLAG: u16 = 0x200;

/// The NaN-boxed `undefined` literal, for an absent optional operand.
fn undefined_literal() -> String {
    double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED))
}

/// Borrow a re-read operand list as call arguments, every one a `double`.
///
/// Every helper in this file takes NaN-boxed operands and nothing else, so the
/// argument vector is always this and the per-arm code is the callee name plus
/// the operand list.
fn boxed_args<'v>(values: &'v [String]) -> Vec<(LlvmType, &'v str)> {
    values.iter().map(|v| (DOUBLE, v.as_str())).collect()
}

/// The whole shape of most of this file: lower `operands` in order, keep each
/// one valid across the ones after it, and hand the re-read list to `callee`.
///
/// Sixteen arms are exactly this and were sixteen copies of the unrooted form.
fn lower_boxed_operand_call(
    ctx: &mut FnCtx<'_>,
    callee: &str,
    operands: &[&Expr],
) -> Result<String> {
    with_operands_rooted(ctx, operands, |ctx, values| {
        Ok(ctx.block().call(DOUBLE, callee, &boxed_args(values)))
    })
}

/// Where the argument array sits in a trap helper's parameter list.
enum TrapArgOrder {
    /// `js_proxy_apply(proxy, thisArg, argArray)`.
    ThisFirst,
    /// `js_proxy_construct(proxy, argArray, newTarget)`.
    ArrayFirst,
}

/// `Proxy.apply` / `Proxy.construct`: the proxy, plus an argument array built
/// out of the call's own arguments.
///
/// One group holds both. The proxy is live across `js_array_alloc` and across
/// every argument's lowering; the array is live across every argument after the
/// first. Before this slice neither was rooted.
fn lower_proxy_trap_with_args(
    ctx: &mut FnCtx<'_>,
    callee: &str,
    proxy: &Expr,
    args: &[Expr],
    order: TrapArgOrder,
) -> Result<String> {
    let arg_exprs: Vec<&Expr> = args.iter().collect();
    with_rooted_group(ctx, 1, |ctx, g| {
        // Unconditional: the array build emits `js_array_alloc` plus one
        // `js_array_push_f64` per argument even when the arguments themselves
        // are inert, so there is no argument list for which this window is
        // empty.
        let p = g.lower(ctx, proxy, true)?;
        let arr = build_args_array(ctx, g, &arg_exprs)?;
        let arr_box = nanbox_pointer_inline(ctx.block(), &arr);
        let p = g.reread(ctx, p)?;
        let undef = undefined_literal();
        let call_args = match order {
            TrapArgOrder::ThisFirst => [
                (DOUBLE, p.as_str()),
                (DOUBLE, undef.as_str()),
                (DOUBLE, arr_box.as_str()),
            ],
            TrapArgOrder::ArrayFirst => [
                (DOUBLE, p.as_str()),
                (DOUBLE, arr_box.as_str()),
                (DOUBLE, undef.as_str()),
            ],
        };
        Ok(ctx.block().call(DOUBLE, callee, &call_args))
    })
}

/// The `reflect-metadata` family: two or three required operands plus an
/// optional `propertyKey`, all NaN-boxed, all fed to one runtime helper with
/// `undefined` filling an absent key.
///
/// Eight arms carried eight copies of this body, and therefore eight copies of
/// the same window: `key` live across `target`'s lowering and `propertyKey`'s,
/// `target` live across `propertyKey`'s, each of them arbitrary user code.
fn lower_reflect_metadata(
    ctx: &mut FnCtx<'_>,
    callee: &str,
    required: &[&Expr],
    property_key: Option<&Expr>,
) -> Result<String> {
    for operand in required {
        downgrade_unknown_call_expr(ctx, operand);
    }
    if let Some(property_key) = property_key {
        downgrade_unknown_call_expr(ctx, property_key);
    }
    let mut operands: Vec<&Expr> = required.to_vec();
    operands.extend(property_key);
    let fill_undefined = property_key.is_none();
    with_operands_rooted(ctx, &operands, |ctx, values| {
        let undef = undefined_literal();
        let mut args = boxed_args(values);
        if fill_undefined {
            args.push((DOUBLE, undef.as_str()));
        }
        Ok(ctx.block().call(DOUBLE, callee, &args))
    })
}

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
    // The proxy is live across `thisArg`'s lowering AND across the argument
    // array's construction, and `thisArg` across the latter; the array is the
    // #7154 accumulator on top of that. One group holds all three.
    let rest: Vec<&Expr> = if is_apply {
        Vec::new()
    } else {
        args.iter().skip(1).collect()
    };
    let this_expr = args.first();
    let apply_array_expr = if is_apply { args.get(1) } else { None };
    let result = with_rooted_group(ctx, 2, |ctx, g| {
        let p = g.lower(ctx, proxy, true)?;
        let this_slot = match this_expr {
            Some(a) => Some(g.lower(ctx, a, true)?),
            None => None,
        };
        let arr_box = match apply_array_expr {
            // 2nd arg is the already-built argument array (a JSValue), and it
            // is the LAST lowering in the arm — only the two group re-reads
            // below it, which are loads.
            Some(a) => lower_expr(ctx, a)?,
            None => {
                let arr = build_args_array(ctx, g, &rest)?;
                nanbox_pointer_inline(ctx.block(), &arr)
            }
        };
        let p = g.reread(ctx, p)?;
        let this_arg = match this_slot {
            Some(i) => g.reread(ctx, i)?,
            None => undefined_literal(),
        };
        Ok(ctx.block().call(
            DOUBLE,
            "js_proxy_apply",
            &[(DOUBLE, &p), (DOUBLE, &this_arg), (DOUBLE, &arr_box)],
        ))
    })?;
    Ok(Some(result))
}

/// Build a NaN-boxed argument array inside `group`'s scope.
///
/// Replaces `expr::helpers::proxy_build_args_array`, which threaded the array's
/// raw `*mut ArrayHeader` through its push loop in a bare SSA register while
/// each element — arbitrary user code — was lowered. That is #7154's canonical
/// accumulator bug: the register held the ONLY reference to everything pushed
/// so far, and every `js_array_push_f64` allocates. The array now lives in the
/// group, so the push re-reads it and publishes the possibly-reallocated
/// pointer back.
///
/// Returns the raw handle, read below the last push and above the consuming
/// call — the group is still holding it, and is released by the caller.
fn build_args_array(
    ctx: &mut FnCtx<'_>,
    group: &mut crate::rooting::RootedGroup<'_>,
    args: &[&Expr],
) -> Result<String> {
    let cap = args.len().to_string();
    let acc = group.begin_array(ctx, &cap);
    for a in args {
        let v = lower_expr(ctx, a)?;
        group.push_array(ctx, acc, &v);
    }
    Ok(group.read_array(ctx, acc))
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
    // The receiver is live across EVERY argument's lowering, and argument `i`
    // across every argument after it. The stack buffer below is not a root —
    // `alloca_entry_array` is the plain-alloca shape `--unrooted-allocas`
    // reports (#7210) — so the values must be current when they are stored into
    // it, which means the whole list is re-read below the last lowering.
    let operands: Vec<&Expr> = std::iter::once(proxy.as_ref()).chain(args.iter()).collect();
    let result = with_operands_rooted(ctx, &operands, |ctx, values| {
        let (recv_box, lowered_args) = values.split_first().expect("receiver is always present");
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
        Ok(ctx.block().call(
            DOUBLE,
            "js_native_call_method",
            &[
                (DOUBLE, recv_box.as_str()),
                (PTR, &bytes_global),
                (I64, &name_len),
                (PTR, &args_ptr),
                (I64, &args_len),
            ],
        ))
    })?;
    Ok(Some(result))
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
                .or_else(|| guarded_declared_class_property_candidate(ctx, target))
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
                .or_else(|| guarded_declared_class_property_candidate(ctx, target))
                .and_then(|class_name| {
                    crate::type_analysis::class_field_global_index(ctx, &class_name, property)
                })
                .map(|_| property.clone())
        }
        _ if same_side_effect_free_receiver(target, receiver) => {
            if !strict {
                return None;
            }
            let class_name = receiver_class_name(ctx, target)
                .or_else(|| guarded_declared_class_property_candidate(ctx, target))?;
            crate::type_analysis::class_field_global_index(ctx, &class_name, property)
                .map(|_| property.clone())
        }
        _ => None,
    }
}

/// A source declaration may select the guarded class-field route, but it may
/// never authorize a raw slot access itself. `PropertySet` re-checks the live
/// receiver against the selected class/shape before touching the slot and
/// retains the ordinary runtime fallback on guard failure.
fn guarded_declared_class_property_candidate(ctx: &FnCtx<'_>, target: &Expr) -> Option<String> {
    let Expr::LocalGet(id) = target else {
        return None;
    };
    let perry_hir::types::Type::Named(name) = ctx.local_type_hint(id)? else {
        return None;
    };
    ctx.classes.contains_key(name).then(|| name.clone())
}

/// Bounded polymorphic inline cache for a static-name `PutValue` whose target
/// and receiver are the same expression.
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
    // Keep the first four ways inline. Shapes 5–8 use a separate cache in a
    // compact outlined helper, avoiding four more copies of the generated
    // receiver guards while preventing the fourth inline way from thrashing.
    let tail_cache_name = format!("perry_ic_{}_poly_tail", site_id);
    ctx.ic_globals.push(tail_cache_name.clone());
    let tail_cache_ref = format!("@{}", tail_cache_name);

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
    let dispatch5_idx = ctx.new_block("put.pic.dispatch5");
    let hit_idx = ctx.new_block("put.pic.hit");
    let miss_idx = ctx.new_block("put.pic.miss");
    let miss2_idx = ctx.new_block("put.pic.miss2");
    let miss3_idx = ctx.new_block("put.pic.miss3");
    let miss4_idx = ctx.new_block("put.pic.miss4");
    let tail_idx = ctx.new_block("put.pic.tail");
    let merge_idx = ctx.new_block("put.pic.merge");
    let guard_label = ctx.block_label(guard_idx);
    let guard2_label = ctx.block_label(guard2_idx);
    let guard3_label = ctx.block_label(guard3_idx);
    let guard4_label = ctx.block_label(guard4_idx);
    let fallback_label = ctx.block_label(fallback_idx);
    let dispatch3_label = ctx.block_label(dispatch3_idx);
    let dispatch4_label = ctx.block_label(dispatch4_idx);
    let dispatch5_label = ctx.block_label(dispatch5_idx);
    let hit_label = ctx.block_label(hit_idx);
    let miss_label = ctx.block_label(miss_idx);
    let miss2_label = ctx.block_label(miss2_idx);
    let miss3_label = ctx.block_label(miss3_idx);
    let miss4_label = ctx.block_label(miss4_idx);
    let tail_label = ctx.block_label(tail_idx);
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
    let reserved_addr = ctx.block().sub(I64, &safe_target, "6");
    let reserved_ptr = ctx.block().inttoptr(I64, &reserved_addr);
    let reserved = ctx.block().load(I16, &reserved_ptr);
    let blocked = ctx
        .block()
        .and(I16, &reserved, &WRITE_PIC_BLOCKING_FLAGS.to_string());
    let flags_clear = ctx.block().icmp_eq(I16, &blocked, "0");

    let class_addr = ctx.block().add(I64, &safe_target, "4");
    let class_ptr = ctx.block().inttoptr(I64, &class_addr);
    let class_id = ctx.block().load(I32, &class_ptr);
    let has_class = ctx.block().icmp_ne(I32, &class_id, "0");
    let not_native_module = ctx.block().icmp_ne(I32, &class_id, "-2");
    // #8098: a class-less receiver qualifies when the runtime birth-marked it
    // an ordinary plain object. `reserved` is already loaded above for the
    // blocking-flag test, so this costs one `and` + `icmp` + `or`, computed
    // once here and reused by all four ways (this block dominates them).
    let plain_ordinary_bits = ctx
        .block()
        .and(I16, &reserved, &PLAIN_ORDINARY_OBJ_FLAG.to_string());
    let plain_ordinary = ctx.block().icmp_ne(I16, &plain_ordinary_bits, "0");
    let receiver_kind_ok = ctx.block().or(I1, &has_class, &plain_ordinary);

    // The write PIC uses the same single ShapeId token domain as the read PIC.
    let shape_id_addr = ctx.block().add(I64, &safe_target, "8");
    let shape_id_ptr = ctx.block().inttoptr(I64, &shape_id_addr);
    let raw_shape_id = ctx.block().load(I32, &shape_id_ptr);
    let shape_id_rel = ctx.block().add(I32, &raw_shape_id, "-2147483648");
    let has_shape_id = ctx.block().icmp_ult(I32, &shape_id_rel, "1073741824");
    let shape_id64 = ctx.block().zext(I32, &raw_shape_id, I64);
    let shape_id_token = ctx.block().or(I64, &shape_id64, "4611686018427387904");
    let shape_token = ctx
        .block()
        .select(I1, &has_shape_id, I64, &shape_id_token, "0");
    let cached_token_ptr = ctx.block().gep(I64, &cache_ref, &[(I64, "0")]);
    let cached_token = ctx.block().load(I64, &cached_token_ptr);
    let token_match = ctx.block().icmp_eq(I64, &shape_token, &cached_token);
    let token_nonzero = ctx.block().icmp_ne(I64, &shape_token, "0");

    let cached_slot_ptr = ctx.block().gep(I64, &cache_ref, &[(I64, "1")]);
    let slot = ctx.block().load(I64, &cached_slot_ptr);
    let mut hit = ctx.block().and(I1, &heap_candidate, &gc_object);
    hit = ctx.block().and(I1, &hit, &not_forwarded);
    hit = ctx.block().and(I1, &hit, &flags_clear);
    hit = ctx.block().and(I1, &hit, &receiver_kind_ok);
    hit = ctx.block().and(I1, &hit, &not_native_module);
    hit = ctx.block().and(I1, &hit, &token_match);
    hit = ctx.block().and(I1, &hit, &token_nonzero);

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
    let mut hit2 = ctx.block().and(I1, &heap_candidate, &gc_object);
    hit2 = ctx.block().and(I1, &hit2, &not_forwarded);
    hit2 = ctx.block().and(I1, &hit2, &flags_clear);
    hit2 = ctx.block().and(I1, &hit2, &receiver_kind_ok);
    hit2 = ctx.block().and(I1, &hit2, &not_native_module);
    hit2 = ctx.block().and(I1, &hit2, &token2_match);
    hit2 = ctx.block().and(I1, &hit2, &token_nonzero);
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
    let mut hit3 = ctx.block().and(I1, &heap_candidate, &gc_object);
    hit3 = ctx.block().and(I1, &hit3, &not_forwarded);
    hit3 = ctx.block().and(I1, &hit3, &flags_clear);
    hit3 = ctx.block().and(I1, &hit3, &receiver_kind_ok);
    hit3 = ctx.block().and(I1, &hit3, &not_native_module);
    hit3 = ctx.block().and(I1, &hit3, &token3_match);
    hit3 = ctx.block().and(I1, &hit3, &token_nonzero);
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
    let mut hit4 = ctx.block().and(I1, &heap_candidate, &gc_object);
    hit4 = ctx.block().and(I1, &hit4, &not_forwarded);
    hit4 = ctx.block().and(I1, &hit4, &flags_clear);
    hit4 = ctx.block().and(I1, &hit4, &receiver_kind_ok);
    hit4 = ctx.block().and(I1, &hit4, &not_native_module);
    hit4 = ctx.block().and(I1, &hit4, &token4_match);
    hit4 = ctx.block().and(I1, &hit4, &token_nonzero);
    ctx.block().cond_br(&hit4, &hit_label, &dispatch5_label);

    ctx.current_block = dispatch5_idx;
    let fourth_empty = ctx.block().icmp_eq(I64, &cached4_token, "0");
    ctx.block()
        .cond_br(&fourth_empty, &miss4_label, &tail_label);

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

    ctx.current_block = tail_idx;
    let tail_value = ctx.block().call(
        DOUBLE,
        "js_put_value_set_ic_poly_tail",
        &[
            (PTR, &tail_cache_ref),
            (DOUBLE, &target_value),
            (I64, &key_handle),
            (DOUBLE, &stored_value),
            (I32, strict_i32),
        ],
    );
    let tail_end_label = ctx.block().label.clone();
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
            (&tail_value, &tail_end_label),
        ],
    );
    Ok(Some(result))
}

/// #6812 (w12): inline hit path for the 3-way dynamic-key write IC.
/// Registers arrive in k → v → t evaluation order (see the call site); from
/// the target register onward the path is call-free until the store or the
/// outlined slow call. Guards are byte-for-byte the static write PIC's
/// (GcHeader -8/-7/-6 with BLOCKING 0x1907 incl. typed-intact, ObjectHeader
/// regular/class/token via the #6804 discriminated shape-token select).
/// The raw store fires only for non-reference VALUE tags (not pointer/
/// string/bigint), so it needs no barrier and no layout note; every other
/// case — and every miss — takes the outlined helper, which bottoms out at
/// full `[[Set]]` + re-prime.
fn lower_put_value_dyn_ic_inline(
    ctx: &mut FnCtx<'_>,
    t: &str,
    k: &str,
    v: &str,
    strict_i32: &str,
) -> Result<String> {
    let site_id = ctx.ic_site_counter;
    ctx.ic_site_counter += 1;
    let cache_name = format!("perry_ic_{}", site_id);
    ctx.ic_globals.push(cache_name.clone());
    let cache_ref = format!("@{}", cache_name);

    let k_bits = ctx.block().bitcast_double_to_i64(k);
    let v_bits = ctx.block().bitcast_double_to_i64(v);
    let t_bits = ctx.block().bitcast_double_to_i64(t);
    let t_handle = ctx.block().and(I64, &t_bits, POINTER_MASK_I64);
    let t_tag = ctx.block().lshr(I64, &t_bits, "48");
    let is_ptr = ctx.block().icmp_eq(I64, &t_tag, "32765");
    let above = ctx.block().icmp_ugt(I64, &t_handle, "1048575");
    // Value tag: reference-creating stores (pointer 0x7FFD, string 0x7FFF,
    // bigint 0x7FFA) leave the inline path before any store.
    let v_tag = ctx.block().lshr(I64, &v_bits, "48");
    let v_not_obj = ctx.block().icmp_ne(I64, &v_tag, "32765");
    let v_not_str = ctx.block().icmp_ne(I64, &v_tag, "32767");
    let v_not_big = ctx.block().icmp_ne(I64, &v_tag, "32762");
    // Zero key bits are the empty-way sentinel (and the JS number 0):
    // they must never reach the way compares.
    let k_nonzero = ctx.block().icmp_ne(I64, &k_bits, "0");
    let mut entry_ok = ctx.block().and(I1, &is_ptr, &above);
    entry_ok = ctx.block().and(I1, &entry_ok, &v_not_obj);
    entry_ok = ctx.block().and(I1, &entry_ok, &v_not_str);
    entry_ok = ctx.block().and(I1, &entry_ok, &v_not_big);
    entry_ok = ctx.block().and(I1, &entry_ok, &k_nonzero);

    let guard_idx = ctx.new_block("put.dynic.guard");
    let ways_idx = ctx.new_block("put.dynic.ways");
    let way1_idx = ctx.new_block("put.dynic.way1");
    let way2_idx = ctx.new_block("put.dynic.way2");
    let store_idx = ctx.new_block("put.dynic.store");
    let slow_idx = ctx.new_block("put.dynic.slow");
    let merge_idx = ctx.new_block("put.dynic.merge");
    let guard_label = ctx.block_label(guard_idx);
    let ways_label = ctx.block_label(ways_idx);
    let way1_label = ctx.block_label(way1_idx);
    let way2_label = ctx.block_label(way2_idx);
    let store_label = ctx.block_label(store_idx);
    let slow_label = ctx.block_label(slow_idx);
    let merge_label = ctx.block_label(merge_idx);
    ctx.block().cond_br(&entry_ok, &guard_label, &slow_label);

    ctx.current_block = guard_idx;
    let gc_type_addr = ctx.block().sub(I64, &t_handle, "8");
    let gc_type_ptr = ctx.block().inttoptr(I64, &gc_type_addr);
    let gc_type = ctx.block().load(I8, &gc_type_ptr);
    let gc_object = ctx.block().icmp_eq(I8, &gc_type, "2");
    let gc_flags_addr = ctx.block().sub(I64, &t_handle, "7");
    let gc_flags_ptr = ctx.block().inttoptr(I64, &gc_flags_addr);
    let gc_flags = ctx.block().load(I8, &gc_flags_ptr);
    let forwarded = ctx.block().and(I8, &gc_flags, "128");
    let not_forwarded = ctx.block().icmp_eq(I8, &forwarded, "0");
    let reserved_addr = ctx.block().sub(I64, &t_handle, "6");
    let reserved_ptr = ctx.block().inttoptr(I64, &reserved_addr);
    let reserved = ctx.block().load(I16, &reserved_ptr);
    let blocked = ctx
        .block()
        .and(I16, &reserved, &WRITE_PIC_BLOCKING_FLAGS.to_string());
    let flags_clear = ctx.block().icmp_eq(I16, &blocked, "0");
    let class_addr = ctx.block().add(I64, &t_handle, "4");
    let class_ptr = ctx.block().inttoptr(I64, &class_addr);
    let class_id = ctx.block().load(I32, &class_ptr);
    let has_class = ctx.block().icmp_ne(I32, &class_id, "0");
    let not_native_module = ctx.block().icmp_ne(I32, &class_id, "-2");
    // #8098: see the static-key PIC above.
    let plain_ordinary_bits = ctx
        .block()
        .and(I16, &reserved, &PLAIN_ORDINARY_OBJ_FLAG.to_string());
    let plain_ordinary = ctx.block().icmp_ne(I16, &plain_ordinary_bits, "0");
    let receiver_kind_ok = ctx.block().or(I1, &has_class, &plain_ordinary);
    let shape_id_addr = ctx.block().add(I64, &t_handle, "8");
    let shape_id_ptr = ctx.block().inttoptr(I64, &shape_id_addr);
    let raw_shape_id = ctx.block().load(I32, &shape_id_ptr);
    let shape_id_rel = ctx.block().add(I32, &raw_shape_id, "-2147483648");
    let has_shape_id = ctx.block().icmp_ult(I32, &shape_id_rel, "1073741824");
    let shape_id64 = ctx.block().zext(I32, &raw_shape_id, I64);
    let shape_id_token = ctx.block().or(I64, &shape_id64, "4611686018427387904");
    let shape_token = ctx
        .block()
        .select(I1, &has_shape_id, I64, &shape_id_token, "0");
    let cached_token_ptr = ctx.block().gep(I64, &cache_ref, &[(I64, "0")]);
    let cached_token = ctx.block().load(I64, &cached_token_ptr);
    let token_match = ctx.block().icmp_eq(I64, &shape_token, &cached_token);
    let token_nonzero = ctx.block().icmp_ne(I64, &shape_token, "0");
    let mut ok = ctx.block().and(I1, &gc_object, &not_forwarded);
    ok = ctx.block().and(I1, &ok, &flags_clear);
    ok = ctx.block().and(I1, &ok, &receiver_kind_ok);
    ok = ctx.block().and(I1, &ok, &not_native_module);
    ok = ctx.block().and(I1, &ok, &token_match);
    ok = ctx.block().and(I1, &ok, &token_nonzero);
    ctx.block().cond_br(&ok, &ways_label, &slow_label);

    ctx.current_block = ways_idx;
    let k0_ptr = ctx.block().gep(I64, &cache_ref, &[(I64, "1")]);
    let k0 = ctx.block().load(I64, &k0_ptr);
    let s0_ptr = ctx.block().gep(I64, &cache_ref, &[(I64, "2")]);
    let s0 = ctx.block().load(I64, &s0_ptr);
    let hit0 = ctx.block().icmp_eq(I64, &k_bits, &k0);
    ctx.block().cond_br(&hit0, &store_label, &way1_label);
    ctx.current_block = way1_idx;
    let k1_ptr = ctx.block().gep(I64, &cache_ref, &[(I64, "3")]);
    let k1 = ctx.block().load(I64, &k1_ptr);
    let s1_ptr = ctx.block().gep(I64, &cache_ref, &[(I64, "4")]);
    let s1 = ctx.block().load(I64, &s1_ptr);
    let hit1 = ctx.block().icmp_eq(I64, &k_bits, &k1);
    ctx.block().cond_br(&hit1, &store_label, &way2_label);
    ctx.current_block = way2_idx;
    let k2_ptr = ctx.block().gep(I64, &cache_ref, &[(I64, "5")]);
    let k2 = ctx.block().load(I64, &k2_ptr);
    let s2_ptr = ctx.block().gep(I64, &cache_ref, &[(I64, "6")]);
    let s2 = ctx.block().load(I64, &s2_ptr);
    let hit2 = ctx.block().icmp_eq(I64, &k_bits, &k2);
    ctx.block().cond_br(&hit2, &store_label, &slow_label);

    ctx.current_block = store_idx;
    let slot = ctx.block().phi(
        I64,
        &[(&s0, &ways_label), (&s1, &way1_label), (&s2, &way2_label)],
    );
    let header_words =
        (crate::target_layout::object_header_size_bytes(ctx.target_triple) / 8).to_string();
    let slot_word = ctx.block().add(I64, &slot, &header_words);
    let obj_ptr = ctx.block().inttoptr(I64, &t_handle);
    let slot_ptr = ctx
        .block()
        .gep_inbounds(I64, &obj_ptr, &[(I64, &slot_word)]);
    // GC_STORE_AUDIT(POINTER_FREE): the entry tag test proved the value is
    // not pointer/string/bigint — non-reference bits need no barrier.
    ctx.block().store(DOUBLE, v, &slot_ptr);
    ctx.block().br(&merge_label);

    ctx.current_block = slow_idx;
    let slow_result = ctx.block().call(
        DOUBLE,
        "js_put_value_set_dyn_ic",
        &[
            (crate::types::PTR, &cache_ref),
            (DOUBLE, t),
            (DOUBLE, k),
            (DOUBLE, v),
            (I32, strict_i32),
        ],
    );
    ctx.block().br(&merge_label);

    ctx.current_block = merge_idx;
    let result = ctx
        .block()
        .phi(DOUBLE, &[(v, &store_label), (&slow_result, &slow_label)]);
    Ok(result)
}

fn static_write_key(ctx: &FnCtx<'_>, key: &Expr) -> Option<String> {
    match key {
        Expr::String(property) => Some(property.clone()),
        Expr::LocalGet(id) => ctx.const_string_locals.get(id).cloned(),
        // #6812 (w13): `o[7] = v` — a constant integer key is the canonical
        // numeric-string property key ("7"; i64 formatting is canonical for
        // every integer, including negatives). Real arrays never take the IC
        // hit path: the miss handler and the emitted guards validate the
        // receiver as a REGULAR heap object, so array receivers fall to the
        // generic write, which performs the element store.
        Expr::Integer(n) => Some(n.to_string()),
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
    // Representation-selection Phase 2: a receiver statically typed as a
    // numeric typed array also belongs on `index_set::lower`'s typed-array
    // arm — that file carries the proven-view / checked-native element-store
    // tiers plus the same dynamic-key dispatcher this path would reach, so
    // element writes on typed receivers (the spec-ABI `lr[off] = …` shape)
    // stop routing through the generic write-IC.
    let recv_typed_array = matches!(
        crate::type_analysis::receiver_class_name(ctx, target).as_deref(),
        Some(
            "Int8Array"
                | "Uint8Array"
                | "Uint8ClampedArray"
                | "Int16Array"
                | "Uint16Array"
                | "Int32Array"
                | "Uint32Array"
                | "Float32Array"
                | "Float64Array"
        )
    );
    (recv_unknown || recv_typed_array) && !key_is_static_string_or_symbol
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

    // #7615 slice 7. `key_handle` used to be stripped to a RAW `StringHeader*`
    // here and then held across `value`'s lowering — arbitrary user code —
    // before `js_setenv` dereferenced it. That is #7280 taxonomy (a): once a
    // pointer has left the NaN-boxed representation, no re-read of a rooted
    // slot can repair it, because the slot holds the box and the register holds
    // the address.
    //
    // Both branches were exposed and for different reasons. The literal branch
    // loads the key's `__perry_init_strings_*` handle global, which is a
    // registered root that evacuation REWRITES — #7114 exactly, one operand
    // over from the `PutValueSet` key that #7201 fixed. The computed branch is
    // worse: `js_to_property_key` returns a FRESH string with no other root at
    // all, so a sweep between here and `js_setenv` frees it.
    //
    // The fix is to keep the key NaN-boxed across the value's lowering and take
    // the raw pointer below it. The literal branch is `Expr::String`, which
    // `operand_protection` answers with `Reload` — no runtime slot, just the
    // load emitted again below the window.
    if let Expr::String(property) = key {
        // Literal key. Its `__perry_init_strings_*` handle global is a
        // registered root, so nothing needs a slot — but the global is one the
        // collector REWRITES, so the load has to sit below `value`'s lowering
        // or the strip reads a pre-move address. That is #7114, and #7201 fixed
        // the same shape one operand over in `PutValueSet`.
        //
        // The load stays an explicit `handle_global` read rather than becoming
        // `lower_expr(key)`: a short string literal lowers to an inline
        // SHORT_STRING_TAG immediate, and `unbox_to_i64` is documented garbage
        // for those.
        let val_double = lower_expr(ctx, value)?;
        let key_idx = ctx.strings.intern(property);
        let key_handle_global = format!("@{}", ctx.strings.entry(key_idx).handle_global);
        let blk = ctx.block();
        let key_box = blk.load(DOUBLE, &key_handle_global);
        let key_handle = unbox_to_i64(blk, &key_box);
        blk.call_void("js_setenv", &[(I64, &key_handle), (DOUBLE, &val_double)]);
        return Ok(Some(val_double));
    }
    // Computed key. `js_to_property_key` must run ABOVE the value's evaluation
    // — ES2022 moved `ToPropertyKey` before the RHS — so the value that has to
    // survive that evaluation is the COERCED key, a fresh heap string produced
    // by an emitted call rather than by lowering an expression. That is what
    // `RootedGroup::adopt_emitted` is for.
    with_rooted_group(ctx, 1, |ctx, g| {
        let key_box = lower_expr(ctx, key)?;
        let property_key = ctx
            .block()
            .call(DOUBLE, "js_to_property_key", &[(DOUBLE, &key_box)]);
        let key_slot = g.adopt_emitted(ctx, Repr::Boxed, &property_key, true);
        let val_double = lower_expr(ctx, value)?;
        // The strip happens BELOW the window, never above it.
        let key_box = g.reread_emitted(ctx, key_slot);
        let key_handle = unbox_str_handle(ctx.block(), &key_box);
        ctx.block()
            .call_void("js_setenv", &[(I64, &key_handle), (DOUBLE, &val_double)]);
        Ok(Some(val_double))
    })
}

pub(crate) fn lower(ctx: &mut FnCtx<'_>, expr: &Expr) -> Result<String> {
    match expr {
        Expr::ProxyNew { target, handler } => {
            downgrade_unknown_call_expr(ctx, target);
            downgrade_unknown_call_expr(ctx, handler);
            lower_boxed_operand_call(ctx, "js_proxy_new", &[target, handler])
        }
        Expr::ProxyGet { proxy, key } => {
            downgrade_unknown_call_expr(ctx, proxy);
            downgrade_unknown_call_expr(ctx, key);
            lower_boxed_operand_call(ctx, "js_proxy_get", &[proxy, key])
        }
        Expr::ProxySet { proxy, key, value } => {
            downgrade_unknown_call_expr(ctx, proxy);
            downgrade_unknown_call_expr(ctx, key);
            downgrade_unknown_call_expr(ctx, value);
            with_operands_rooted(ctx, &[proxy, key, value], |ctx, v| {
                let _ = ctx.block().call(DOUBLE, "js_proxy_set", &boxed_args(v));
                // The assignment's value is `value` as the trap OBSERVED it, so
                // it is the re-read register rather than the pre-call one.
                Ok(v[2].clone())
            })
        }
        Expr::ProxyHas { proxy, key } => {
            downgrade_unknown_call_expr(ctx, proxy);
            downgrade_unknown_call_expr(ctx, key);
            lower_boxed_operand_call(ctx, "js_proxy_has", &[proxy, key])
        }
        Expr::ProxyDelete { proxy, key } => {
            downgrade_unknown_call_expr(ctx, proxy);
            downgrade_unknown_call_expr(ctx, key);
            let strict = if ctx.is_strict_fn { "1" } else { "0" };
            with_operands_rooted(ctx, &[proxy, key], |ctx, v| {
                let blk = ctx.block();
                // `js_proxy_delete` reports the `[[Delete]]` boolean; a strict-mode
                // `delete proxy.key` that resolves to `false` (non-configurable
                // property, forwarded through the trap chain) must throw a TypeError
                // just like the ordinary member-delete path. Route the boolean
                // through `js_delete_result` so both modes match spec (test262
                // Proxy/deleteProperty/*-target-is-proxy `delete funcProxy.prototype`
                // under "use strict").
                let deleted_box = blk.call(
                    DOUBLE,
                    "js_proxy_delete",
                    &[(DOUBLE, &v[0]), (DOUBLE, &v[1])],
                );
                let deleted_i32 = blk.call(I32, "js_is_truthy", &[(DOUBLE, &deleted_box)]);
                Ok(blk.call(
                    DOUBLE,
                    "js_delete_result",
                    &[(I32, &deleted_i32), (I32, strict)],
                ))
            })
        }
        Expr::ProxyApply { proxy, args } => {
            downgrade_unknown_call_expr(ctx, proxy);
            downgrade_unknown_call_args(ctx, args);
            lower_proxy_trap_with_args(ctx, "js_proxy_apply", proxy, args, TrapArgOrder::ThisFirst)
        }
        Expr::ProxyConstruct { proxy, args } => {
            downgrade_unknown_call_expr(ctx, proxy);
            downgrade_unknown_call_args(ctx, args);
            lower_proxy_trap_with_args(
                ctx,
                "js_proxy_construct",
                proxy,
                args,
                TrapArgOrder::ArrayFirst,
            )
        }
        Expr::ProxyRevocable { target, handler } => {
            // #2846: return a real `{ proxy, revoke }` record so `typeof
            // rec.revoke === "function"`, `rec.proxy.a` forwards, and the
            // revoke function survives aliasing/storage.
            downgrade_unknown_call_expr(ctx, target);
            downgrade_unknown_call_expr(ctx, handler);
            lower_boxed_operand_call(ctx, "js_proxy_revocable", &[target, handler])
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
            lower_boxed_operand_call(ctx, "js_reflect_get", &[target, key, receiver])
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
            lower_boxed_operand_call(ctx, "js_reflect_set", &[target, key, value, receiver])
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
            // #7288: sloppy code is barred from the class-field route above
            // because that route's fallback throws on a rejected write. The
            // FAST arm is mode-independent (its precheck rejects frozen /
            // descriptor-bearing receivers and non-number values), so emit it
            // here with a sloppy-correct miss path instead of surrendering the
            // whole optimization. #5094/P1 extends it to boxed slots. See
            // `property_set::try_lower_sloppy_class_field_store`.
            if !*strict {
                if let Expr::String(property) = key.as_ref() {
                    if same_put_value_receiver_expr(target, receiver)
                        && matches!(target.as_ref(), Expr::LocalGet(_) | Expr::This)
                    {
                        if let Some(result) =
                            super::property_set::try_lower_sloppy_class_field_store(
                                ctx, target, property, value,
                            )?
                        {
                            return Ok(result);
                        }
                    }
                }
            }
            if put_value_index_fast_path(ctx, target, key, receiver) {
                return super::index_set::lower(
                    ctx,
                    &Expr::IndexSet {
                        object: target.clone(),
                        index: key.clone(),
                        value: value.clone(),
                    },
                    // #7590: a `PutValue` routed through the index-set fast
                    // path returns the assigned value to ITS caller, which may
                    // well consume it. Never the discarded form.
                    false,
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
            let strict_i32 = if *strict { "1" } else { "0" };
            // #6812 (w12) inline path: evaluation order k → v → t. The
            // target is a pure local read, so hoisting key/value evaluation
            // above its REGISTER materialization is unobservable, and that
            // ordering is what keeps the TARGET safe — the pointer is
            // materialized below everything that can collect.
            //
            // #7201: the KEY is not covered by that argument, and the comment
            // that used to sit here claimed it was — "a moved key merely misses
            // by stale bits (identity compare — false negatives only)". That is
            // true of the three way-compares and false of everything below
            // them: the miss falls through to `put.dynic.slow`, which hands the
            // SAME register to `js_put_value_set_dyn_ic`, which DEREFERENCES it
            // as a `StringHeader*`. `this.viaBlock = churn()` inside a
            // `static { … }` block is the shipped shape (#7201): the key
            // literal's `__perry_init_strings_*` handle is a registered root
            // that evacuation REWRITES, so the register loaded above `churn()`
            // names from-space and the slow path reads a relocated string
            // header — SIGSEGV, or a property under a garbage name.
            //
            // An identity-compare-only use is not the only use. Re-derive the
            // key below the value, exactly as every other store operand does.
            let dyn_inline = same_put_value_receiver_expr(target, receiver)
                && matches!(target.as_ref(), Expr::LocalGet(_) | Expr::This);
            if dyn_inline {
                // Migrated onto `RootedGroup` without moving an emission: the
                // key's window is still derived from `value` alone, and the
                // re-read still sits above `target`'s lowering, which the
                // comment above argues is safe because `target` is a
                // `LocalGet` / `This`.
                return with_rooted_group(ctx, 1, |ctx, g| {
                    let collects = any_operand_may_collect(ctx, std::iter::once(value.as_ref()));
                    let key_slot = g.lower(ctx, key, collects)?;
                    let v = lower_expr(ctx, value)?;
                    let k = g.reread(ctx, key_slot)?;
                    let t = lower_expr(ctx, target)?;
                    // Released by the group below the store: the outlined
                    // helper allocates while reading the key (interning,
                    // keys-array growth, shape transition).
                    lower_put_value_dyn_ic_inline(ctx, &t, &k, &v, strict_i32)
                });
            }
            // #7201, outlined arms: `t` is lowered FIRST here, so it is live
            // across both `k`'s and `v`'s lowering, and `k` across `v`'s. Both
            // are consumed by helpers that dereference them.
            //
            // The two operands go into ONE group rather than two nested guards.
            // A group release is a single truncate at the LOWEST slot, which is
            // the same stack cut the nested pair performed — with the ordering
            // no longer expressible wrongly, since the caller never holds
            // either index.
            with_rooted_group(ctx, 2, |ctx, g| {
                // The receiver's window covers BOTH the key's lowering and the
                // value's, so its `collects` is the disjunction — `o[f()] = 1`
                // has an inert value and a collecting key.
                let recv_collects =
                    any_operand_may_collect(ctx, [key.as_ref(), value.as_ref(), receiver.as_ref()]);
                let recv_slot = g.lower(ctx, target, recv_collects)?;
                let key_collects =
                    any_operand_may_collect(ctx, [value.as_ref(), receiver.as_ref()]);
                let key_slot = g.lower(ctx, key, key_collects)?;
                let v = lower_expr(ctx, value)?;
                // #6812 (w12): same-receiver dynamic-key stores that failed the
                // inline gate (computed target expressions) still take the
                // outlined 3-way IC helper.
                if same_put_value_receiver_expr(target, receiver) {
                    let k = g.reread(ctx, key_slot)?;
                    let t = g.reread(ctx, recv_slot)?;
                    let site_id = ctx.ic_site_counter;
                    ctx.ic_site_counter += 1;
                    let cache_name = format!("perry_ic_{}", site_id);
                    ctx.ic_globals.push(cache_name.clone());
                    let cache_ref = format!("@{}", cache_name);
                    Ok(ctx.block().call(
                        DOUBLE,
                        "js_put_value_set_dyn_ic",
                        &[
                            (crate::types::PTR, &cache_ref),
                            (DOUBLE, &t),
                            (DOUBLE, &k),
                            (DOUBLE, &v),
                            (I32, strict_i32),
                        ],
                    ))
                } else {
                    // The explicit-receiver form lowers a FOURTH operand, so the
                    // re-reads have to sit below it, not above.
                    let r = lower_expr(ctx, receiver)?;
                    let k = g.reread(ctx, key_slot)?;
                    let t = g.reread(ctx, recv_slot)?;
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
            })
        }
        Expr::ReflectHas { target, key } => {
            downgrade_unknown_call_expr(ctx, target);
            downgrade_unknown_call_expr(ctx, key);
            lower_boxed_operand_call(ctx, "js_reflect_has", &[target, key])
        }
        Expr::ReflectDelete { target, key } => {
            downgrade_unknown_call_expr(ctx, target);
            downgrade_unknown_call_expr(ctx, key);
            lower_boxed_operand_call(ctx, "js_reflect_delete", &[target, key])
        }
        Expr::ReflectOwnKeys(target) => {
            // One operand, consumed by the very next emission: no window.
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
            lower_boxed_operand_call(ctx, "js_reflect_apply", &[func, this_arg, args])
        }
        Expr::ReflectConstruct {
            target,
            args,
            new_target,
        } => {
            downgrade_unknown_call_expr(ctx, target);
            downgrade_unknown_call_expr(ctx, args);
            downgrade_unknown_call_expr(ctx, new_target);
            let result =
                lower_boxed_operand_call(ctx, "js_reflect_construct", &[target, args, new_target])?;
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
            lower_boxed_operand_call(
                ctx,
                "js_reflect_define_property",
                &[target, key, descriptor],
            )
        }
        Expr::ReflectGetOwnPropertyDescriptor { target, key } => {
            downgrade_unknown_call_expr(ctx, target);
            downgrade_unknown_call_expr(ctx, key);
            lower_boxed_operand_call(
                ctx,
                "js_reflect_get_own_property_descriptor",
                &[target, key],
            )
        }
        Expr::ReflectSetPrototypeOf { target, proto } => {
            // #2761: Reflect-specific boolean result (false on rejected change)
            // + TypeError on bad args, distinct from Object.setPrototypeOf.
            downgrade_unknown_call_expr(ctx, target);
            downgrade_unknown_call_expr(ctx, proto);
            lower_boxed_operand_call(ctx, "js_reflect_set_prototype_of", &[target, proto])
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
        } => lower_reflect_metadata(
            ctx,
            "js_reflect_define_metadata",
            &[key, value, target],
            property_key.as_deref(),
        ),
        Expr::ReflectGetMetadata {
            key,
            target,
            property_key,
        } => lower_reflect_metadata(
            ctx,
            "js_reflect_get_metadata",
            &[key, target],
            property_key.as_deref(),
        ),
        Expr::ReflectGetOwnMetadata {
            key,
            target,
            property_key,
        } => lower_reflect_metadata(
            ctx,
            "js_reflect_get_own_metadata",
            &[key, target],
            property_key.as_deref(),
        ),
        Expr::ReflectHasMetadata {
            key,
            target,
            property_key,
        } => lower_reflect_metadata(
            ctx,
            "js_reflect_has_metadata",
            &[key, target],
            property_key.as_deref(),
        ),
        Expr::ReflectHasOwnMetadata {
            key,
            target,
            property_key,
        } => lower_reflect_metadata(
            ctx,
            "js_reflect_has_own_metadata",
            &[key, target],
            property_key.as_deref(),
        ),
        Expr::ReflectGetMetadataKeys {
            target,
            property_key,
        } => lower_reflect_metadata(
            ctx,
            "js_reflect_get_metadata_keys",
            &[target],
            property_key.as_deref(),
        ),
        Expr::ReflectGetOwnMetadataKeys {
            target,
            property_key,
        } => lower_reflect_metadata(
            ctx,
            "js_reflect_get_own_metadata_keys",
            &[target],
            property_key.as_deref(),
        ),
        Expr::ReflectDeleteMetadata {
            key,
            target,
            property_key,
        } => lower_reflect_metadata(
            ctx,
            "js_reflect_delete_metadata",
            &[key, target],
            property_key.as_deref(),
        ),

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
