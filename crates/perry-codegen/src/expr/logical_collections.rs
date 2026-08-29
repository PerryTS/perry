//! Logical..SetNewFromArray.
//!
//! Extracted from `expr/mod.rs` to keep that file under the 2000-line cap.
//! Pure mechanical move — match arm bodies are verbatim copies, called from
//! `lower_expr`'s outer dispatch.
//!
//! # Layer 1 migrated module (#7615, slice 2)
//!
//! Nothing here names `expr::temp_root`. Multi-operand arms go through
//! [`crate::rooting::with_operands_rooted`]; the two arms that keep writing
//! into a half-built container while they lower more user code go through
//! [`crate::rooting::with_rooted_accumulator`].
//! `crate::rooting::migration_ledger` fails the build if this module reaches
//! back into the raw API. Single-operand arms keep their plain `lower_expr`,
//! as the template module (`expr/url_main.rs`, #7617) does: with nothing
//! lowered after the operand there is no window and `operand_protection` would
//! answer `Reuse`.
//!
//! ## What the migration found
//!
//! **Callback receivers.** `arr.filter(cb)`, `arr.some(cb)` and `arr.every(cb)`
//! lowered the array, then lowered the callback — and a callback literal is a
//! `js_closure_new`. That is #7620's `find*` finding in three more arms that
//! were missed because they live in a different file.
//!
//! **Two unrooted accumulators.** `Math.min`/`Math.max` with three or more
//! arguments allocate a scratch array and then thread a **raw `ArrayHeader*`**
//! through `js_array_push_f64` — across each remaining argument's own lowering.
//! `Math.min(f(), g(), h())` therefore pushed into a pre-move address. The
//! static-headers path of `fetch(url, { headers: { k: f() } })` has the same
//! shape with `js_object_alloc`. Both are #7154's `ObjectSpread` bug, and
//! because what is stale is a raw `i64` derived above the window rather than a
//! NaN-boxed double, #7280's `root_reload` structurally cannot repair either
//! (slice 1b's finding 2).
//!
//! **`fetch`'s three string operands.** `url`, `method` and `body` sat in
//! registers across the whole headers construction *and* across
//! `js_fetch_headers_to_json`, which enumerates the own properties of a
//! program-supplied value and so can re-enter user code through an accessor —
//! the argument `Expr::ObjectSpread` already makes for `js_object_copy_own_fields`
//! below. That step is emitted rather than lowered, so the window is stated
//! rather than derived: see `with_operands_rooted_across_call`.

use anyhow::{bail, Result};
use perry_hir::types::Type as HirType;
use perry_hir::Expr;

use crate::lower_conditional::lower_logical;
use crate::nanbox::{double_literal, POINTER_MASK_I64, TAG_UNDEFINED};
use crate::rooting::{self, Arg, Repr};
use crate::type_analysis::{map_static_type_args, string_value_is_runtime_guaranteed};
use crate::types::{DOUBLE, I32, I64, PTR};

use super::{
    emit_string_literal_global, i32_bool_to_nanbox, lower_expr, nanbox_pointer_inline,
    nanbox_string_inline, record_collection_number_key_fallback,
    record_collection_number_key_selected, record_collection_string_key_fallback,
    record_collection_string_key_selected, unbox_str_handle, unbox_to_i64, FnCtx,
};

fn is_static_string_key_map(ctx: &FnCtx<'_>, map: &Expr) -> bool {
    matches!(
        map_static_type_args(ctx, map),
        Some([HirType::String | HirType::StringLiteral(_), _])
    )
}

fn is_static_number_key_map(ctx: &FnCtx<'_>, map: &Expr) -> bool {
    matches!(
        map_static_type_args(ctx, map),
        Some([HirType::Number | HirType::Int32, _])
    )
}

/// Return the compiled body symbol for an inline arrow whose function object
/// cannot be observed by `Array.prototype.some` and whose body cannot inspect
/// a closure environment. The runtime may then invoke the code pointer
/// directly without allocating/looking up a singleton ClosureHeader.
/// `arr.some(capturelessArrow)` as an inline loop, with `js_array_some_captureless`
/// as the fallback for every receiver the loop does not admit.
///
/// The runtime helper decides the receiver ONCE — a plain `GC_TYPE_ARRAY`
/// head, no indexed descriptors, pristine `Array.prototype` /
/// `Object.prototype` index state, `length <= capacity` — and then runs the
/// element loop with one rooted re-resolution per element, a NaN-boxed
/// receiver per call and an indirect call through the function pointer. The
/// loop emitted here makes the same one-time decision on the same live bits
/// (the sticky `PERRY_ARRAY_INDEX_FAST_PATH_INVALIDATED` byte is the
/// prototype half), then per element: re-reads the head from its root (the
/// callback may have collected, or grown the array — a forwarded head goes
/// through `js_array_live_head`), skips indices past the live length and
/// holes, calls the arrow's body symbol directly with as many of
/// `(element, index, receiver)` as it declares, and decides `true` / `false`
/// results inline with `js_is_truthy` for anything else. Same contract as
/// the helper: the bound is the length at entry, holes are skipped, an
/// exotic or non-array receiver takes the helper.
fn lower_captureless_some_inline(
    ctx: &mut FnCtx<'_>,
    array: &Expr,
    callback_func: &str,
    param_count: usize,
) -> Result<String> {
    use crate::nanbox::{POINTER_TAG_TOP16_I64, TAG_HOLE_I64};
    use crate::types::{I1, I16, I8};
    const TAG_TRUE_I64: &str = "9222246136947933188"; // 0x7FFC_0000_0000_0004
    const TAG_FALSE_I64: &str = "9222246136947933187"; // 0x7FFC_0000_0000_0003
    rooting::with_rooted_group(ctx, 1, |ctx, group| {
        let arr_idx = group.lower(ctx, array, true)?;
        let arr_box0 = group.reread(ctx, arr_idx)?;
        let admit_idx = ctx.new_block("some.inline.admit");
        let loop_idx = ctx.new_block("some.inline.loop");
        let body_idx = ctx.new_block("some.inline.body");
        let resolve_idx = ctx.new_block("some.inline.resolve");
        let live_idx = ctx.new_block("some.inline.live");
        let elem_idx = ctx.new_block("some.inline.elem");
        let call_idx = ctx.new_block("some.inline.call");
        let slow_idx = ctx.new_block("some.inline.slow");
        let truthy_idx = ctx.new_block("some.inline.truthy");
        let next_idx = ctx.new_block("some.inline.next");
        let found_idx = ctx.new_block("some.inline.found");
        let fallback_idx = ctx.new_block("some.inline.fallback");
        let merge_idx = ctx.new_block("some.inline.merge");
        let admit_l = ctx.block_label(admit_idx);
        let loop_l = ctx.block_label(loop_idx);
        let body_l = ctx.block_label(body_idx);
        let resolve_l = ctx.block_label(resolve_idx);
        let live_l = ctx.block_label(live_idx);
        let elem_l = ctx.block_label(elem_idx);
        let call_l = ctx.block_label(call_idx);
        let slow_l = ctx.block_label(slow_idx);
        let truthy_l = ctx.block_label(truthy_idx);
        let next_l = ctx.block_label(next_idx);
        let found_l = ctx.block_label(found_idx);
        let fallback_l = ctx.block_label(fallback_idx);
        let merge_l = ctx.block_label(merge_idx);
        let counter = ctx.func.alloca_entry(I32);

        // A heap pointer, before any header is read.
        {
            let blk = ctx.block();
            let bits = blk.bitcast_double_to_i64(&arr_box0);
            let top16 = blk.lshr(I64, &bits, "48");
            let is_pointer = blk.icmp_eq(I64, &top16, POINTER_TAG_TOP16_I64);
            blk.cond_br(&is_pointer, &admit_l, &fallback_l);
        }
        // Admission: the helper's one-time decision, on the live bits.
        ctx.current_block = admit_idx;
        let len0 = {
            let blk = ctx.block();
            let raw = unbox_to_i64(blk, &arr_box0);
            let type_addr = blk.sub(I64, &raw, "8");
            let type_ptr = blk.inttoptr(I64, &type_addr);
            let obj_type = blk.load(I8, &type_ptr);
            let is_array = blk.icmp_eq(I8, &obj_type, "1"); // GC_TYPE_ARRAY
            let flags_addr = blk.sub(I64, &raw, "7");
            let flags_ptr = blk.inttoptr(I64, &flags_addr);
            let gc_flags = blk.load(I8, &flags_ptr);
            let forwarded = blk.and(I8, &gc_flags, "128"); // GC_FLAG_FORWARDED
            let not_forwarded = blk.icmp_eq(I8, &forwarded, "0");
            let reserved_addr = blk.sub(I64, &raw, "6");
            let reserved_ptr = blk.inttoptr(I64, &reserved_addr);
            let reserved = blk.load(I16, &reserved_ptr);
            let descriptors = blk.and(I16, &reserved, "1024"); // OBJ_FLAG_ARRAY_DESCRIPTORS
            let no_descriptors = blk.icmp_eq(I16, &descriptors, "0");
            let invalidated = blk.load_volatile(I8, "@PERRY_ARRAY_INDEX_FAST_PATH_INVALIDATED");
            let prototype_clean = blk.icmp_eq(I8, &invalidated, "0");
            let len_ptr = blk.inttoptr(I64, &raw);
            let length = blk.load(I32, &len_ptr);
            let cap_addr = blk.add(I64, &raw, "4");
            let cap_ptr = blk.inttoptr(I64, &cap_addr);
            let capacity = blk.load(I32, &cap_ptr);
            let dense = blk.icmp_ule(I32, &length, &capacity);
            let a = blk.and(I1, &is_array, &not_forwarded);
            let b = blk.and(I1, &a, &no_descriptors);
            let c = blk.and(I1, &b, &prototype_clean);
            let admitted = blk.and(I1, &c, &dense);
            blk.store(I32, "0", &counter);
            blk.cond_br(&admitted, &loop_l, &fallback_l);
            length
        };
        // loop: i < len0 ? (the bound is the length at entry)
        ctx.current_block = loop_idx;
        let false_box = {
            let blk = ctx.block();
            let i = blk.load(I32, &counter);
            let more = blk.icmp_ult(I32, &i, &len0);
            // The merge phi's operands are materialised in the predecessors:
            // a phi must lead its block.
            let false_box = blk.bitcast_i64_to_double(TAG_FALSE_I64);
            blk.cond_br(&more, &body_l, &merge_l);
            false_box
        };
        // body: re-read the head from its root; a forwarded head resolves.
        ctx.current_block = body_idx;
        let arr_box = group.reread(ctx, arr_idx)?;
        let raw_reread = {
            let blk = ctx.block();
            let raw = unbox_to_i64(blk, &arr_box);
            let flags_addr = blk.sub(I64, &raw, "7");
            let flags_ptr = blk.inttoptr(I64, &flags_addr);
            let gc_flags = blk.load(I8, &flags_ptr);
            let forwarded = blk.and(I8, &gc_flags, "128");
            let is_forwarded = blk.icmp_ne(I8, &forwarded, "0");
            blk.cond_br(&is_forwarded, &resolve_l, &live_l);
            raw
        };
        ctx.current_block = resolve_idx;
        let resolved = ctx
            .block()
            .call(I64, "js_array_live_head", &[(I64, &raw_reread)]);
        ctx.block().br(&live_l);
        // live: bounds against the live length, then the element.
        ctx.current_block = live_idx;
        let raw = ctx
            .block()
            .phi(I64, &[(&raw_reread, &body_l), (&resolved, &resolve_l)]);
        let i = {
            let blk = ctx.block();
            let i = blk.load(I32, &counter);
            let len_ptr = blk.inttoptr(I64, &raw);
            let live_len = blk.load(I32, &len_ptr);
            let in_range = blk.icmp_ult(I32, &i, &live_len);
            blk.cond_br(&in_range, &elem_l, &next_l);
            i
        };
        ctx.current_block = elem_idx;
        let elem_bits = {
            let blk = ctx.block();
            let i64_i = blk.zext(I32, &i, I64);
            let byte_offset = blk.shl(I64, &i64_i, "3");
            let with_header = blk.add(I64, &byte_offset, "8");
            let elem_addr = blk.add(I64, &raw, &with_header);
            let elem_ptr = blk.inttoptr(I64, &elem_addr);
            let bits = blk.load(I64, &elem_ptr);
            let is_hole = blk.icmp_eq(I64, &bits, TAG_HOLE_I64);
            blk.cond_br(&is_hole, &next_l, &call_l);
            bits
        };
        ctx.current_block = call_idx;
        let result = {
            let blk = ctx.block();
            let elem = blk.bitcast_i64_to_double(&elem_bits);
            let i_double = blk.uitofp(I32, &i, DOUBLE);
            let recv = nanbox_pointer_inline(blk, &raw);
            let mut args: Vec<(crate::types::LlvmType, &str)> =
                vec![(I64, "0"), (DOUBLE, elem.as_str())];
            if param_count >= 2 {
                args.push((DOUBLE, i_double.as_str()));
            }
            if param_count >= 3 {
                args.push((DOUBLE, recv.as_str()));
            }
            let result = blk.call(DOUBLE, callback_func.trim_start_matches('@'), &args);
            let bits = blk.bitcast_double_to_i64(&result);
            let is_true = blk.icmp_eq(I64, &bits, TAG_TRUE_I64);
            blk.cond_br(&is_true, &found_l, &slow_l);
            result
        };
        ctx.current_block = slow_idx;
        {
            let blk = ctx.block();
            let bits = blk.bitcast_double_to_i64(&result);
            let is_false = blk.icmp_eq(I64, &bits, TAG_FALSE_I64);
            blk.cond_br(&is_false, &next_l, &truthy_l);
        }
        ctx.current_block = truthy_idx;
        {
            let blk = ctx.block();
            let truthy = blk.call(I32, "js_is_truthy", &[(DOUBLE, &result)]);
            let nonzero = blk.icmp_ne(I32, &truthy, "0");
            blk.cond_br(&nonzero, &found_l, &next_l);
        }
        ctx.current_block = next_idx;
        {
            let blk = ctx.block();
            let i = blk.load(I32, &counter);
            let inc = blk.add(I32, &i, "1");
            blk.store(I32, &inc, &counter);
            blk.br(&loop_l);
        }
        ctx.current_block = found_idx;
        let true_box = {
            let blk = ctx.block();
            let true_box = blk.bitcast_i64_to_double(TAG_TRUE_I64);
            blk.br(&merge_l);
            true_box
        };
        ctx.current_block = fallback_idx;
        let fallback_value = {
            let blk = ctx.block();
            let arr_handle = unbox_to_i64(blk, &arr_box0);
            let value = blk.call(
                DOUBLE,
                "js_array_some_captureless",
                &[(I64, &arr_handle), (PTR, callback_func)],
            );
            blk.br(&merge_l);
            value
        };
        ctx.current_block = merge_idx;
        let blk = ctx.block();
        Ok(blk.phi(
            DOUBLE,
            &[
                (&false_box, &loop_l),
                (&true_box, &found_l),
                (&fallback_value, &fallback_l),
            ],
        ))
    })
}

fn captureless_some_callback(ctx: &FnCtx<'_>, callback: &Expr) -> Option<String> {
    let Expr::Closure {
        func_id,
        params,
        body,
        captures,
        captures_this,
        captures_new_target,
        is_arrow,
        is_async,
        is_generator,
        ..
    } = callback
    else {
        return None;
    };
    if !is_arrow
        || *is_async
        || *is_generator
        || *captures_this
        || *captures_new_target
        || params.len() > 3
        || params
            .iter()
            .any(|param| param.is_rest || param.arguments_object.is_some())
        || !crate::type_analysis::compute_auto_captures(ctx, params, body, captures).is_empty()
    {
        return None;
    }
    Some(format!(
        "@perry_closure_{}__{}",
        ctx.strings.module_prefix(),
        func_id
    ))
}

fn guarded_map_number_key_delete(ctx: &mut FnCtx<'_>, map_handle: &str, key_box: &str) -> String {
    let guard_raw = ctx
        .block()
        .call(I32, "js_typed_f64_arg_guard", &[(DOUBLE, key_box)]);
    let guard = ctx.block().icmp_ne(I32, &guard_raw, "0");
    let fast_idx = ctx.new_block("map_number_key.delete.fast");
    let fallback_idx = ctx.new_block("map_number_key.delete.fallback");
    let merge_idx = ctx.new_block("map_number_key.delete.merge");
    let fast_label = ctx.block_label(fast_idx);
    let fallback_label = ctx.block_label(fallback_idx);
    let merge_label = ctx.block_label(merge_idx);
    ctx.block().cond_br(&guard, &fast_label, &fallback_label);

    ctx.current_block = fast_idx;
    let key_raw = ctx
        .block()
        .call(DOUBLE, "js_typed_f64_arg_to_raw", &[(DOUBLE, key_box)]);
    let fast_value = ctx.block().call(
        I32,
        "js_map_delete_number_key",
        &[(I64, map_handle), (DOUBLE, &key_raw)],
    );
    record_collection_number_key_selected(
        ctx,
        "MapDelete",
        "collection_number_key.map_delete",
        &key_raw,
        "map",
        "number_key_helper",
        "js_map_delete_number_key",
        "key",
    );
    let after_fast = ctx.block().label.clone();
    if !ctx.block().is_terminated() {
        ctx.block().br(&merge_label);
    }

    ctx.current_block = fallback_idx;
    let fallback_value = ctx.block().call(
        I32,
        "js_map_delete",
        &[(I64, map_handle), (DOUBLE, key_box)],
    );
    record_collection_number_key_fallback(
        ctx,
        "MapDelete",
        "collection_number_key.map_delete_generic",
        key_box,
        "map",
        "number_key_helper",
        "js_map_delete",
        "runtime_key_guard_failed",
        "key",
    );
    let after_fallback = ctx.block().label.clone();
    if !ctx.block().is_terminated() {
        ctx.block().br(&merge_label);
    }

    ctx.current_block = merge_idx;
    ctx.block().phi(
        I32,
        &[
            (fast_value.as_str(), after_fast.as_str()),
            (fallback_value.as_str(), after_fallback.as_str()),
        ],
    )
}

pub(crate) fn lower(ctx: &mut FnCtx<'_>, expr: &Expr) -> Result<String> {
    match expr {
        Expr::Logical { op, left, right } => lower_logical(ctx, *op, left, right),

        // -------- arr.filter(callback) --------
        // Mirrors ArrayMap: takes a closure header pointer, returns
        // a new array.
        Expr::ArrayFilter { array, callback } => {
            // #7615 slice 2: the array is live across the callback's lowering,
            // and a callback literal is a `js_closure_new`. Same window #7620
            // closed in the four `find*` arms.
            rooting::with_operands_rooted(ctx, &[array, callback], |ctx, vals| {
                let blk = ctx.block();
                let arr_handle = unbox_to_i64(blk, &vals[0]);
                // #4091: throw TypeError for a non-callable callback before iterating.
                let cb_handle = blk.call(I64, "js_validate_array_callback", &[(DOUBLE, &vals[1])]);
                let result = blk.call(
                    I64,
                    "js_array_filter",
                    &[(I64, &arr_handle), (I64, &cb_handle)],
                );
                Ok(nanbox_pointer_inline(blk, &result))
            })
        }

        // -------- fetch(url, { method, body, headers }) --------
        // Build a runtime headers object from the static (key, dynamic-value)
        // pairs, JSON-stringify it, and pass everything to
        // `js_fetch_with_options(url, method, body, headers_json)` which
        // returns a `*mut Promise`. The result is NaN-boxed with POINTER_TAG
        // so the rest of the await/then machinery sees a normal Promise.
        Expr::FetchWithOptions {
            url,
            method,
            body,
            headers,
            headers_dynamic,
            signal,
        } => {
            // Lower `init.signal` (if any) with the other operands so it can be
            // stashed for `js_fetch_with_options` right before the call below.
            let mut operands: Vec<&Expr> = vec![url, method, body];
            if let Some(s) = signal {
                operands.push(s);
            }
            // The window is STATED, not derived (`with_operands_rooted_across_call`):
            // the across step ends in `js_fetch_headers_to_json`, which enumerates
            // the own properties of a program-supplied value and therefore can run
            // a user accessor — the same argument `Expr::ObjectSpread` makes for
            // `js_object_copy_own_fields` below. No property of the *headers
            // expression* can rule that out, and `fetch` is about to do I/O, so
            // the three string operands pay for slots unconditionally.
            rooting::with_operands_rooted_across_call(
                ctx,
                &operands,
                |ctx| {
                    // Obtain the headers as a NaN-boxed object value, then
                    // JSON-stringify it. Two cases:
                    //   * `headers_dynamic` — the headers value was a variable, a
                    //     spread literal, or a call (`Object.assign`/`new
                    //     Headers`/`JSON.parse`). Lower it directly;
                    //     `js_json_stringify` enumerates its own properties at
                    //     runtime (#4932).
                    //   * otherwise — statically-extracted `{ "k": v, ... }`
                    //     pairs, which we build into a fresh object field-by-field.
                    let headers_obj_box = if let Some(hexpr) = headers_dynamic {
                        lower_expr(ctx, hexpr)?
                    } else {
                        // Build the headers object: js_object_alloc(0, N) followed
                        // by js_object_set_field_by_name for each (key, value).
                        let n_str = (headers.len() as u32).to_string();
                        let zero_str = "0".to_string();
                        let headers_handle = ctx.block().call(
                            I64,
                            "js_object_alloc",
                            &[(I32, &zero_str), (I32, &n_str)],
                        );
                        // #7615 slice 2: the half-built headers object was a raw
                        // `i64` across every value's lowering — #7154's
                        // `ObjectSpread` bug in a second arm.
                        let protect =
                            rooting::any_operand_may_collect(ctx, headers.iter().map(|(_, v)| v));
                        rooting::with_rooted_accumulator(
                            ctx,
                            Repr::Ptr,
                            &headers_handle,
                            protect,
                            |ctx, acc| {
                                for (key, val_expr) in headers {
                                    let key_idx = ctx.strings.intern(key);
                                    let key_handle_global =
                                        format!("@{}", ctx.strings.entry(key_idx).handle_global);
                                    let v_box = lower_expr(ctx, val_expr)?;
                                    let key_raw = {
                                        let blk = ctx.block();
                                        let key_box = blk.load(DOUBLE, &key_handle_global);
                                        let key_bits = blk.bitcast_double_to_i64(&key_box);
                                        blk.and(I64, &key_bits, POINTER_MASK_I64)
                                    };
                                    acc.call_void(
                                        ctx,
                                        "js_object_set_field_by_name",
                                        &[Arg::Plain(I64, &key_raw), Arg::Plain(DOUBLE, &v_box)],
                                    );
                                }
                                Ok(())
                            },
                            |ctx, headers_handle| {
                                let blk = ctx.block();
                                Ok(nanbox_pointer_inline(blk, headers_handle))
                            },
                        )?
                    };
                    // Stringify the headers value into the flat `{name:value}`
                    // JSON that `js_fetch_with_options` parses. Routed through
                    // `js_fetch_headers_to_json` (not the generic
                    // `js_json_stringify`) so a `Headers` instance — a fetch-band
                    // registry handle, e.g. `headers: new Headers(h)` — is read
                    // from its registry instead of being dereferenced as a heap
                    // pointer (the `js_json_stringify`-on-handle SIGSEGV; same
                    // #5559/#5560 handle-band family).
                    let blk = ctx.block();
                    Ok(blk.call(
                        I64,
                        "js_fetch_headers_to_json",
                        &[(DOUBLE, &headers_obj_box)],
                    ))
                },
                |ctx, vals, headers_str| {
                    let blk = ctx.block();
                    // The runtime takes raw StringHeader pointers (i64). Unbox
                    // each input string. `body` may be undefined → unbox produces
                    // 0 which the runtime treats as "no body" via
                    // string_from_header(). The unbox happens BELOW the re-read,
                    // which is the only place it can be correct (slice 1b's
                    // `BufferSlice` finding).
                    let url_handle = unbox_to_i64(blk, &vals[0]);
                    let method_handle = unbox_to_i64(blk, &vals[1]);
                    let body_handle = unbox_to_i64(blk, &vals[2]);
                    // Stash the AbortSignal so `js_fetch_with_options` can cancel
                    // the request when it aborts (`controller.abort()` /
                    // `AbortSignal.timeout`).
                    if let Some(sig) = vals.get(3) {
                        blk.call_void("js_fetch_set_pending_signal", &[(DOUBLE, sig)]);
                    }
                    let promise = blk.call(
                        I64,
                        "js_fetch_with_options",
                        &[
                            (I64, &url_handle),
                            (I64, &method_handle),
                            (I64, &body_handle),
                            (I64, &headers_str),
                        ],
                    );
                    Ok(nanbox_pointer_inline(blk, &promise))
                },
            )
        }

        // -------- arr.some(callback) -> boolean --------
        // js_array_some returns a NaN-tagged TAG_TRUE/TAG_FALSE as f64,
        // so we forward it directly without conversion.
        Expr::ArraySome { array, callback } => {
            if let Some(callback_func) = captureless_some_callback(ctx, callback) {
                let Expr::Closure { params, .. } = callback.as_ref() else {
                    unreachable!("captureless_some_callback matched a closure");
                };
                return lower_captureless_some_inline(ctx, array, &callback_func, params.len());
            }
            // #7615 slice 2: same callback window as `ArrayFilter` above.
            rooting::with_operands_rooted(ctx, &[array, callback], |ctx, vals| {
                let blk = ctx.block();
                let arr_handle = unbox_to_i64(blk, &vals[0]);
                // #4091: throw TypeError for a non-callable callback before iterating.
                let cb_handle = blk.call(I64, "js_validate_array_callback", &[(DOUBLE, &vals[1])]);
                Ok(blk.call(
                    DOUBLE,
                    "js_array_some",
                    &[(I64, &arr_handle), (I64, &cb_handle)],
                ))
            })
        }

        // -------- arr.every(callback) -> boolean --------
        Expr::ArrayEvery { array, callback } => {
            // #7615 slice 2: same callback window as `ArrayFilter` above.
            rooting::with_operands_rooted(ctx, &[array, callback], |ctx, vals| {
                let blk = ctx.block();
                let arr_handle = unbox_to_i64(blk, &vals[0]);
                // #4091: throw TypeError for a non-callable callback before iterating.
                let cb_handle = blk.call(I64, "js_validate_array_callback", &[(DOUBLE, &vals[1])]);
                Ok(blk.call(
                    DOUBLE,
                    "js_array_every",
                    &[(I64, &arr_handle), (I64, &cb_handle)],
                ))
            })
        }

        // -------- arr.join(separator?) -> string --------
        // The runtime wrapper applies Array.join separator semantics:
        // omitted/undefined means comma; every other value is ToString.
        Expr::ArrayJoin { array, separator } => {
            let mut operands: Vec<&Expr> = vec![array];
            if let Some(sep_expr) = separator {
                operands.push(sep_expr);
            }
            rooting::with_operands_rooted(ctx, &operands, |ctx, vals| {
                let sep_box = vals
                    .get(1)
                    .cloned()
                    .unwrap_or_else(|| double_literal(f64::from_bits(TAG_UNDEFINED)));
                let blk = ctx.block();
                let arr_handle = unbox_to_i64(blk, &vals[0]);
                let result = blk.call(
                    I64,
                    "js_array_join_value",
                    &[(I64, &arr_handle), (DOUBLE, &sep_box)],
                );
                Ok(nanbox_string_inline(blk, &result))
            })
        }

        // -------- Array.prototype.<m>.call/apply(arrayLike, ...) (#4597) --------
        // Generic over an array-like receiver: the runtime `js_arraylike_*`
        // entry points take the *original* receiver value (NaN-boxed `f64`) so
        // they apply ToObject + LengthOfArrayLike + indexed Get/HasProperty and
        // pass the original receiver as the callback's 3rd argument. The result
        // is already a NaN-boxed JS value (number / boolean / pointer / string),
        // so it is returned directly with no re-boxing.
        Expr::ArrayLikeMethod {
            method,
            receiver,
            args,
        } => {
            // #7615 slice 2: the receiver sat in a register across every
            // argument's lowering, and each argument across the ones after it.
            let mut operands: Vec<&Expr> = Vec::with_capacity(args.len() + 1);
            operands.push(receiver);
            operands.extend(args.iter());
            rooting::with_operands_rooted(ctx, &operands, |ctx, vals| {
                let recv_box = vals[0].clone();
                let arg_boxes: Vec<String> = vals[1..].to_vec();
                let undef = || double_literal(f64::from_bits(TAG_UNDEFINED));
                let nth = |i: usize| arg_boxes.get(i).cloned();
                let blk = ctx.block();
                let result = match method.as_str() {
                    // Callback iterators: (recv, callback, thisArg).
                    "forEach" | "map" | "filter" | "some" | "every" | "find" | "findIndex"
                    | "findLast" | "findLastIndex" => {
                        let cb = nth(0).unwrap_or_else(undef);
                        let this_arg = nth(1).unwrap_or_else(undef);
                        let fname = match method.as_str() {
                            "forEach" => "js_arraylike_forEach",
                            "map" => "js_arraylike_map",
                            "filter" => "js_arraylike_filter",
                            "some" => "js_arraylike_some",
                            "every" => "js_arraylike_every",
                            "find" => "js_arraylike_find",
                            "findIndex" => "js_arraylike_findIndex",
                            "findLast" => "js_arraylike_findLast",
                            _ => "js_arraylike_findLastIndex",
                        };
                        blk.call(
                            DOUBLE,
                            fname,
                            &[(DOUBLE, &recv_box), (DOUBLE, &cb), (DOUBLE, &this_arg)],
                        )
                    }
                    // Reducers: (recv, callback, has_init, init).
                    "reduce" | "reduceRight" => {
                        let cb = nth(0).unwrap_or_else(undef);
                        let (has_init, init) = match nth(1) {
                            Some(i) => ("1".to_string(), i),
                            None => ("0".to_string(), undef()),
                        };
                        let fname = if method == "reduce" {
                            "js_arraylike_reduce"
                        } else {
                            "js_arraylike_reduceRight"
                        };
                        blk.call(
                            DOUBLE,
                            fname,
                            &[
                                (DOUBLE, &recv_box),
                                (DOUBLE, &cb),
                                (I32, &has_init),
                                (DOUBLE, &init),
                            ],
                        )
                    }
                    // Search: (recv, value, fromIndex, has_from).
                    "indexOf" | "lastIndexOf" | "includes" => {
                        let value = nth(0).unwrap_or_else(undef);
                        let (has_from, from) = match nth(1) {
                            Some(f) => ("1".to_string(), f),
                            None => ("0".to_string(), undef()),
                        };
                        let fname = match method.as_str() {
                            "indexOf" => "js_arraylike_indexOf",
                            "lastIndexOf" => "js_arraylike_lastIndexOf",
                            _ => "js_arraylike_includes",
                        };
                        blk.call(
                            DOUBLE,
                            fname,
                            &[
                                (DOUBLE, &recv_box),
                                (DOUBLE, &value),
                                (DOUBLE, &from),
                                (I32, &has_from),
                            ],
                        )
                    }
                    // at(index): ToIntegerOrInfinity(undefined) === 0 when omitted.
                    "at" => {
                        let idx = nth(0).unwrap_or_else(undef);
                        blk.call(
                            DOUBLE,
                            "js_arraylike_at",
                            &[(DOUBLE, &recv_box), (DOUBLE, &idx)],
                        )
                    }
                    // join(separator?): undefined separator → comma.
                    "join" => {
                        let sep = nth(0).unwrap_or_else(undef);
                        blk.call(
                            DOUBLE,
                            "js_arraylike_join",
                            &[(DOUBLE, &recv_box), (DOUBLE, &sep)],
                        )
                    }
                    // flat(depth?): undefined selects the default depth 1.
                    "flat" => {
                        let depth = nth(0).unwrap_or_else(undef);
                        blk.call(
                            DOUBLE,
                            "js_arraylike_flat",
                            &[(DOUBLE, &recv_box), (DOUBLE, &depth)],
                        )
                    }
                    // slice(start?, end?): has-flags distinguish omitted from undefined.
                    "slice" => {
                        let (has_start, start) = match nth(0) {
                            Some(s) => ("1".to_string(), s),
                            None => ("0".to_string(), undef()),
                        };
                        let (has_end, end) = match nth(1) {
                            Some(e) => ("1".to_string(), e),
                            None => ("0".to_string(), undef()),
                        };
                        blk.call(
                            DOUBLE,
                            "js_arraylike_slice",
                            &[
                                (DOUBLE, &recv_box),
                                (DOUBLE, &start),
                                (I32, &has_start),
                                (DOUBLE, &end),
                                (I32, &has_end),
                            ],
                        )
                    }
                    // sort(comparator?): validated + run by the runtime engine.
                    "sort" => {
                        let cmp = nth(0).unwrap_or_else(undef);
                        blk.call(
                            DOUBLE,
                            "js_arraylike_sort",
                            &[(DOUBLE, &recv_box), (DOUBLE, &cmp)],
                        )
                    }
                    // splice(...) / concat(...): variadic — pass an alloca buffer
                    // of raw NaN-boxed doubles + count (mirrors the dense
                    // `js_array_concat_variadic` lowering).
                    "splice" | "concat" => {
                        let n = arg_boxes.len();
                        let (buf_reg, count_str) = if n == 0 {
                            ("null".to_string(), "0".to_string())
                        } else {
                            let buf_reg = blk.next_reg();
                            blk.emit_raw(format!("{} = alloca [{} x double]", buf_reg, n));
                            for (i, val) in arg_boxes.iter().enumerate() {
                                let slot = blk.gep(DOUBLE, &buf_reg, &[(I64, &format!("{}", i))]);
                                blk.store(DOUBLE, val, &slot);
                            }
                            (buf_reg, format!("{}", n))
                        };
                        let fname = if method == "splice" {
                            "js_arraylike_splice"
                        } else {
                            "js_arraylike_concat"
                        };
                        blk.call(
                            DOUBLE,
                            fname,
                            &[(DOUBLE, &recv_box), (PTR, &buf_reg), (I32, &count_str)],
                        )
                    }
                    // pop() / shift(): no args, generic over a value receiver.
                    "pop" | "shift" => {
                        let fname = if method == "pop" {
                            "js_arraylike_pop"
                        } else {
                            "js_arraylike_shift"
                        };
                        blk.call(DOUBLE, fname, &[(DOUBLE, &recv_box)])
                    }
                    // push(...) / unshift(...): variadic — pass an alloca buffer of
                    // raw NaN-boxed doubles + count (mirrors splice/concat above).
                    "push" | "unshift" => {
                        let n = arg_boxes.len();
                        let (buf_reg, count_str) = if n == 0 {
                            ("null".to_string(), "0".to_string())
                        } else {
                            let buf_reg = blk.next_reg();
                            blk.emit_raw(format!("{} = alloca [{} x double]", buf_reg, n));
                            for (i, val) in arg_boxes.iter().enumerate() {
                                let slot = blk.gep(DOUBLE, &buf_reg, &[(I64, &format!("{}", i))]);
                                blk.store(DOUBLE, val, &slot);
                            }
                            (buf_reg, format!("{}", n))
                        };
                        let fname = if method == "push" {
                            "js_arraylike_push"
                        } else {
                            "js_arraylike_unshift"
                        };
                        blk.call(
                            DOUBLE,
                            fname,
                            &[(DOUBLE, &recv_box), (PTR, &buf_reg), (I32, &count_str)],
                        )
                    }
                    other => bail!("unsupported generic array-like method '{other}'"),
                };
                Ok(result)
            })
        }

        // -------- map.delete(key) -> boolean --------
        Expr::MapDelete { map, key } => {
            let use_string_key_map =
                is_static_string_key_map(ctx, map) && string_value_is_runtime_guaranteed(ctx, key);
            let use_number_key_map = !use_string_key_map
                && is_static_number_key_map(ctx, map)
                && crate::codegen::typed_arg_is_guard_candidate(
                    ctx,
                    crate::codegen::TypedParamRep::F64,
                    key,
                );
            // #7615 slice 2: the map is live across the key's lowering.
            rooting::with_operands_rooted(ctx, &[map, key], |ctx, vals| {
                let (m_box, k_box) = (vals[0].clone(), vals[1].clone());
                let m_handle = {
                    let blk = ctx.block();
                    unbox_to_i64(blk, &m_box)
                };
                let i32_v = if use_string_key_map {
                    let (k_handle, i32_v) = {
                        let blk = ctx.block();
                        let k_handle = unbox_str_handle(blk, &k_box);
                        let i32_v = blk.call(
                            I32,
                            "js_map_delete_string_key",
                            &[(I64, &m_handle), (I64, &k_handle)],
                        );
                        (k_handle, i32_v)
                    };
                    record_collection_string_key_selected(
                        ctx,
                        "MapDelete",
                        "collection_string_key.map_delete",
                        &k_handle,
                        "map",
                        "js_map_delete_string_key",
                    );
                    i32_v
                } else if use_number_key_map {
                    guarded_map_number_key_delete(ctx, &m_handle, &k_box)
                } else {
                    let i32_v = {
                        let blk = ctx.block();
                        blk.call(I32, "js_map_delete", &[(I64, &m_handle), (DOUBLE, &k_box)])
                    };
                    record_collection_string_key_fallback(
                        ctx,
                        "MapDelete",
                        "collection_string_key.map_delete_generic",
                        &k_box,
                        "map",
                        "js_map_delete",
                        "receiver_or_key_not_static_string",
                    );
                    i32_v
                };
                let blk = ctx.block();
                let bit = blk.icmp_ne(I32, &i32_v, "0");
                let tagged = blk.select(
                    crate::types::I1,
                    &bit,
                    I64,
                    crate::nanbox::TAG_TRUE_I64,
                    crate::nanbox::TAG_FALSE_I64,
                );
                Ok(blk.bitcast_i64_to_double(&tagged))
            })
        }

        // -------- Object.keys(obj) -> string[] --------
        Expr::ObjectKeys(obj) => {
            let obj_box = lower_expr(ctx, obj)?;
            let blk = ctx.block();
            // Pass the NaN-boxed value (not an unboxed pointer) so the runtime
            // can dispatch on its tag — a string receiver yields index keys and
            // a primitive yields [], instead of crashing on a bad deref.
            let arr_handle = blk.call(I64, "js_object_keys_value", &[(DOUBLE, &obj_box)]);
            Ok(nanbox_pointer_inline(blk, &arr_handle))
        }

        // -------- for (key in obj) enumeration keys -> string[] --------
        // The guarded runtime entry reuses a stable one-key shape's immutable
        // key array without allocation, then falls back to the complete
        // nullish/prototype-aware enumerator for every other receiver.
        Expr::ForInKeys(obj) => {
            let obj_box = lower_expr(ctx, obj)?;
            let blk = ctx.block();
            let arr_handle = blk.call(I64, "js_for_in_keys_stable_value", &[(DOUBLE, &obj_box)]);
            Ok(nanbox_pointer_inline(blk, &arr_handle))
        }

        // -------- isFinite(x) — global, coerces to Number first --------
        // The runtime's js_is_finite returns NaN-tagged TAG_TRUE/TAG_FALSE
        // (not a raw 0.0/1.0), so we return the result directly. No fcmp
        // conversion needed — TAG_TRUE is itself a NaN payload and
        // fcmp("one", NaN, 0.0) always returns false.
        Expr::IsFinite(operand) => {
            let v = lower_expr(ctx, operand)?;
            Ok(ctx.block().call(DOUBLE, "js_is_finite", &[(DOUBLE, &v)]))
        }

        // -------- Number.isFinite(x) — strict, no coercion --------
        // Per ECMA-262 §21.1.2.2, returns false for any non-Number value
        // (`"1"`, `true`, `null`, etc.) — distinct from the global
        // `isFinite` which coerces via ToNumber. Pre-fix the codegen
        // routed both forms to `js_is_finite` (the coercing variant),
        // so `Number.isFinite("1")` returned true; correct value is
        // false.
        Expr::NumberIsFinite(operand) => {
            let v = lower_expr(ctx, operand)?;
            Ok(ctx
                .block()
                .call(DOUBLE, "js_number_is_finite", &[(DOUBLE, &v)]))
        }

        // -------- internal: is value === undefined OR a bare-NaN double --------
        Expr::IsUndefinedOrBareNan(operand) => {
            let v = lower_expr(ctx, operand)?;
            let blk = ctx.block();
            let i32_v = blk.call(I32, "js_is_undefined_or_bare_nan", &[(DOUBLE, &v)]);
            Ok(i32_bool_to_nanbox(blk, &i32_v))
        }

        // -------- Math.min(...args) --------
        // Two HIR shapes: variadic (Vec<Expr>) and spread-from-array
        // (single Expr that is an array). Both build/use an array and
        // call js_math_min_array. The variadic form materializes a
        // temporary fixed-size array via js_array_alloc + push.
        Expr::MathMin(values) => {
            if values.len() == 2 {
                return rooting::with_operands_rooted(
                    ctx,
                    &[&values[0], &values[1]],
                    |ctx, vals| {
                        let blk = ctx.block();
                        Ok(blk.call(
                            DOUBLE,
                            "js_math_min2",
                            &[(DOUBLE, &vals[0]), (DOUBLE, &vals[1])],
                        ))
                    },
                );
            }
            let cap = (values.len() as u32).to_string();
            let arr_handle_v = ctx.block().call(I64, "js_array_alloc", &[(I32, &cap)]);
            // Push each value. push_f64 may realloc, so the returned pointer is
            // threaded through — and #7615 slice 2: it was threaded through a
            // RAW SSA register that stayed live across the NEXT argument's
            // lowering, so `Math.min(f(), g(), h())` pushed into a pre-move
            // address. The accumulator is the fix; being an `i64` derived above
            // the window is exactly what #7280 cannot repair.
            let protect = rooting::any_operand_may_collect(ctx, values.iter());
            rooting::with_rooted_accumulator(
                ctx,
                Repr::Ptr,
                &arr_handle_v,
                protect,
                |ctx, acc| {
                    for v_expr in values {
                        let v_box = lower_expr(ctx, v_expr)?;
                        acc.advance(ctx, "js_array_push_f64", &[Arg::Plain(DOUBLE, &v_box)]);
                    }
                    Ok(())
                },
                |ctx, current| {
                    let blk = ctx.block();
                    Ok(blk.call(DOUBLE, "js_math_min_array", &[(I64, current)]))
                },
            )
        }
        Expr::MathMinSpread(arr_expr) => {
            let arr_box = lower_expr(ctx, arr_expr)?;
            let blk = ctx.block();
            let arr_handle = blk.call(I64, "js_array_like_to_array", &[(DOUBLE, &arr_box)]);
            Ok(blk.call(DOUBLE, "js_math_min_array", &[(I64, &arr_handle)]))
        }

        // -------- Math.max(...args) — same shape as Math.min --------
        Expr::MathMax(values) => {
            if values.len() == 2 {
                return rooting::with_operands_rooted(
                    ctx,
                    &[&values[0], &values[1]],
                    |ctx, vals| {
                        let blk = ctx.block();
                        Ok(blk.call(
                            DOUBLE,
                            "js_math_max2",
                            &[(DOUBLE, &vals[0]), (DOUBLE, &vals[1])],
                        ))
                    },
                );
            }
            let cap = (values.len() as u32).to_string();
            let arr_handle_v = ctx.block().call(I64, "js_array_alloc", &[(I32, &cap)]);
            // Same raw-accumulator window as `MathMin` above (#7615 slice 2).
            let protect = rooting::any_operand_may_collect(ctx, values.iter());
            rooting::with_rooted_accumulator(
                ctx,
                Repr::Ptr,
                &arr_handle_v,
                protect,
                |ctx, acc| {
                    for v_expr in values {
                        let v_box = lower_expr(ctx, v_expr)?;
                        acc.advance(ctx, "js_array_push_f64", &[Arg::Plain(DOUBLE, &v_box)]);
                    }
                    Ok(())
                },
                |ctx, current| {
                    let blk = ctx.block();
                    Ok(blk.call(DOUBLE, "js_math_max_array", &[(I64, current)]))
                },
            )
        }
        Expr::MathMaxSpread(arr_expr) => {
            let arr_box = lower_expr(ctx, arr_expr)?;
            let blk = ctx.block();
            let arr_handle = blk.call(I64, "js_array_like_to_array", &[(DOUBLE, &arr_box)]);
            Ok(blk.call(DOUBLE, "js_math_max_array", &[(I64, &arr_handle)]))
        }

        // -------- String(value) coercion --------
        Expr::StringCoerce(operand) => {
            let v = lower_expr(ctx, operand)?;
            let blk = ctx.block();
            let handle = blk.call(I64, "js_string_coerce", &[(DOUBLE, &v)]);
            Ok(nanbox_string_inline(blk, &handle))
        }

        // -------- Object(value) coercion (#3149) --------
        // js_object_coerce takes and returns a NaN-boxed JSValue (DOUBLE):
        // nullish/primitive -> fresh {}, existing object passes through.
        Expr::ObjectCoerce(operand) => {
            let v = lower_expr(ctx, operand)?;
            let blk = ctx.block();
            Ok(blk.call(DOUBLE, "js_object_coerce", &[(DOUBLE, &v)]))
        }

        // -------- Boolean(value) coercion --------
        // js_is_truthy is exactly the JS Boolean(value) coercion: it
        // returns 1 for truthy, 0 for falsy. We convert the i32 to
        // a NaN-tagged TAG_TRUE/TAG_FALSE so console.log prints
        // "true"/"false" via the runtime's NaN-tag dispatch.
        Expr::BooleanCoerce(operand) => {
            let (_v, bit) = crate::lower_conditional::lower_expr_with_truthy(ctx, operand)?;
            let blk = ctx.block();
            let tagged = blk.select(
                crate::types::I1,
                &bit,
                I64,
                crate::nanbox::TAG_TRUE_I64,
                crate::nanbox::TAG_FALSE_I64,
            );
            Ok(blk.bitcast_i64_to_double(&tagged))
        }

        // -------- arr.slice(start, end?) -- new array slice --------
        Expr::ArraySlice { array, start, end } => {
            let mut operands: Vec<&Expr> = vec![array, start];
            if let Some(end_expr) = end {
                operands.push(end_expr);
            }
            rooting::with_operands_rooted(ctx, &operands, |ctx, vals| {
                let end_d = vals.get(2).cloned().unwrap_or_else(|| {
                    double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED))
                });
                let blk = ctx.block();
                let arr_handle = unbox_to_i64(blk, &vals[0]);
                let result = blk.call(
                    I64,
                    "js_array_slice_values",
                    &[(I64, &arr_handle), (DOUBLE, &vals[1]), (DOUBLE, &end_d)],
                );
                Ok(nanbox_pointer_inline(blk, &result))
            })
        }

        // -------- arr.shift() (HIR variant takes a LocalId) --------
        Expr::ArrayShift(array_id) => {
            let arr_box = lower_expr(ctx, &Expr::LocalGet(*array_id))?;
            let blk = ctx.block();
            let arr_handle = unbox_to_i64(blk, &arr_box);
            Ok(blk.call(DOUBLE, "js_array_shift_f64", &[(I64, &arr_handle)]))
        }

        // -------- new Set() / new Set(arr) --------
        Expr::SetNew => {
            let cap = "8".to_string();
            let handle = ctx.block().call(I64, "js_set_alloc", &[(I32, &cap)]);
            Ok(nanbox_pointer_inline(ctx.block(), &handle))
        }

        // -------- "key" in obj --------
        // js_in_operator takes two NaN-boxed doubles and returns a NaN-boxed
        // boolean (1.0/0.0 already in our ABI). Unlike the bare
        // js_object_has_property helper (used internally by Reflect.has / proxy
        // traps / `with` / rest-destructuring), the `in`-operator entry point
        // first enforces ECMA-262 13.10.1 step 5: a non-Object right operand
        // (`"x" in 5`, `... in null`, `... in Symbol()`, …) throws a TypeError.
        Expr::In { property, object } => {
            rooting::with_operands_rooted(ctx, &[property, object], |ctx, vals| {
                Ok(ctx.block().call(
                    DOUBLE,
                    "js_in_operator",
                    &[(DOUBLE, &vals[1]), (DOUBLE, &vals[0])],
                ))
            })
        }
        Expr::PrivateBrandCheck {
            class_name,
            class_id: declaring_class_id,
            field_name,
            kind,
            is_static,
            receiver_is_brand_owner,
            object,
        } => {
            let obj = lower_expr(ctx, object)?;
            // The rooting window between these two operands is empty:
            // lowering `this` only reads the current binding and cannot GC.
            let brand_owner = if *receiver_is_brand_owner {
                obj.clone()
            } else {
                lower_expr(ctx, &Expr::This)?
            };
            let class_id = if *declaring_class_id != 0 {
                *declaring_class_id
            } else {
                ctx.class_ids.get(class_name).copied().unwrap_or(0)
            };
            let key_label = emit_string_literal_global(ctx, field_name);
            Ok(ctx.block().call(
                DOUBLE,
                "js_private_brand_check",
                &[
                    (DOUBLE, &obj),
                    (DOUBLE, &brand_owner),
                    (I32, &class_id.to_string()),
                    (PTR, &key_label),
                    (I32, &field_name.len().to_string()),
                    (I32, &kind.to_string()),
                    (I32, if *is_static { "1" } else { "0" }),
                ],
            ))
        }
        Expr::PrivateGuard {
            class_name,
            class_id: declaring_class_id,
            field_name,
            kind,
            op,
            receiver_is_brand_owner,
            object,
        } => {
            // Evaluate the receiver once, brand+kind check it, and return it
            // unchanged (or throw TypeError). The enclosing PropertyGet /
            // PropertySet / method-call lowering then operates on the result.
            let obj = lower_expr(ctx, object)?;
            // The rooting window between these two operands is empty:
            // lowering `this` only reads the current binding and cannot GC.
            let brand_owner = if *receiver_is_brand_owner {
                obj.clone()
            } else {
                lower_expr(ctx, &Expr::This)?
            };
            // Prefer the declaring class's unique HIR id carried on the node.
            // Resolving `class_name` through `class_ids` is ambiguous: that map
            // is keyed by name (last-writer-wins), so a minified bundle that
            // reuses a class name would bind the brand to the wrong same-named
            // class and reject a legal `this.#x`. Fall back to the name lookup
            // only when the id is absent (0 = unresolved → no-op guard).
            let class_id = if *declaring_class_id != 0 {
                *declaring_class_id
            } else {
                ctx.class_ids.get(class_name).copied().unwrap_or(0)
            };
            let key_label = emit_string_literal_global(ctx, field_name);
            Ok(ctx.block().call(
                DOUBLE,
                "js_private_guard",
                &[
                    (DOUBLE, &obj),
                    (DOUBLE, &brand_owner),
                    (I32, &class_id.to_string()),
                    (PTR, &key_label),
                    (I32, &field_name.len().to_string()),
                    (I32, &kind.to_string()),
                    (I32, &op.to_string()),
                ],
            ))
        }

        // -------- fs.writeFileSync(path, content) --------
        // The runtime takes both args as NaN-boxed doubles directly.
        // Returns i32 (1=success); we drop the result and return 0.0
        // since the HIR-level fs.writeFileSync is void in JS.
        // -------- parseInt(string, radix?) -> number --------
        Expr::ParseInt { string, radix } => {
            let mut operands: Vec<&Expr> = vec![string];
            if let Some(r_expr) = radix {
                operands.push(r_expr);
            }
            rooting::with_operands_rooted(ctx, &operands, |ctx, vals| {
                let r_d = vals.get(1).cloned().unwrap_or_else(|| "0.0".to_string());
                let blk = ctx.block();
                let s_handle = blk.call(I64, "js_string_coerce", &[(DOUBLE, &vals[0])]);
                Ok(blk.call(DOUBLE, "js_parse_int", &[(I64, &s_handle), (DOUBLE, &r_d)]))
            })
        }
        Expr::ParseFloat(string) => {
            let s_box = lower_expr(ctx, string)?;
            let blk = ctx.block();
            let s_handle = blk.call(I64, "js_string_coerce", &[(DOUBLE, &s_box)]);
            Ok(blk.call(DOUBLE, "js_parse_float", &[(I64, &s_handle)]))
        }

        // -------- RegExp literal: /pattern/flags --------
        // Constructs a RegExpHeader at compile time. Both pattern
        // and flags are interned in the StringPool so the runtime
        // sees stable handles.
        Expr::RegExp { pattern, flags } => {
            let pattern_idx = ctx.strings.intern(pattern);
            let flags_idx = ctx.strings.intern(flags);
            let pattern_global = format!("@{}", ctx.strings.entry(pattern_idx).handle_global);
            let flags_global = format!("@{}", ctx.strings.entry(flags_idx).handle_global);
            let blk = ctx.block();
            let pattern_box = blk.load(DOUBLE, &pattern_global);
            let flags_box = blk.load(DOUBLE, &flags_global);
            let pattern_handle = unbox_to_i64(blk, &pattern_box);
            let flags_handle = unbox_to_i64(blk, &flags_box);
            let result = blk.call(
                I64,
                "js_regexp_new",
                &[(I64, &pattern_handle), (I64, &flags_handle)],
            );
            Ok(nanbox_pointer_inline(blk, &result))
        }

        // `RegExp(<dynExpr>)` / `RegExp(<dynExpr>, <dynFlagsExpr>)` /
        // `new RegExp(<non-literal>)`. Folded at HIR (lower/expr_call.rs +
        // lower/expr_new.rs) from any callsite where the pattern (or
        // flags) come in as runtime values rather than string literals.
        // Both `pattern` and `flags` are NaN-boxed strings; missing
        // flags fall back to interning an empty string at codegen so
        // `js_regexp_new` always sees a real `StringHeader*`. Followup
        // to #957 / PR #959.
        Expr::RegExpDynamic {
            pattern,
            flags,
            is_call,
        } => {
            // Route through the full ECMAScript constructor: it handles a RegExp
            // pattern (copy / flag override), an `undefined`/`null` pattern
            // (`ToString` → `""`/`"null"`), an object pattern, and ToString-
            // coerced flags (an object flags → `"[object Object]"` → SyntaxError).
            // Passing the NaN-boxed values verbatim (NOT `unbox_str_handle`,
            // which mis-reads a non-string pattern as a StringHeader → garbage).
            //
            // The function-call form `RegExp(re)` (is_call) routes through
            // `js_regexp_construct_call`, which applies the ECMA-262 22.2.4.1
            // identity shortcut (a RegExp pattern + undefined flags returns the
            // argument unchanged) before falling back to the same constructor.
            // `new RegExp(re)` keeps `js_regexp_construct` so it always copies.
            let mut operands: Vec<&Expr> = vec![pattern];
            if let Some(flags_expr) = flags {
                operands.push(flags_expr);
            }
            rooting::with_operands_rooted(ctx, &operands, |ctx, vals| {
                let flags_box = vals.get(1).cloned().unwrap_or_else(|| {
                    double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED))
                });
                let ctor = if *is_call {
                    "js_regexp_construct_call"
                } else {
                    "js_regexp_construct"
                };
                let blk = ctx.block();
                let result = blk.call(I64, ctor, &[(DOUBLE, &vals[0]), (DOUBLE, &flags_box)]);
                Ok(nanbox_pointer_inline(blk, &result))
            })
        }

        // -------- ObjectSpread literal --------
        // `{ ...a, key: val, ...b }`. The HIR carries an ordered
        // Vec<(Option<String>, Expr)>. Static props use the same
        // js_object_set_field_by_name path as `Expr::Object`. For
        // spread sources we'd need a runtime helper to copy fields
        // — for now we just allocate the object and set the static
        // props, ignoring spreads. Wrong for `...src` but unblocks
        // compilation.
        Expr::ObjectSpread { parts } => {
            // `{ ...a, x: 1, ...b, y: 2 }` — allocate an empty object,
            // then process `parts` in source order: static keys call
            // `js_object_set_field_by_name`, spreads call the runtime
            // `js_object_copy_own_fields(dst, src)` which walks the
            // source's `keys_array` and copies each field via the same
            // setter (so later parts override earlier ones, matching JS
            // semantics).
            let static_count = parts.iter().filter(|(k, _)| k.is_some()).count() as u32;
            let class_id = "0".to_string();
            let count_str = static_count.to_string();
            let obj_handle = ctx.block().call(
                I64,
                "js_object_alloc",
                &[(I32, &class_id), (I32, &count_str)],
            );
            // #7154: the half-built object is a raw SSA register while every
            // part is lowered, and a part's initializer allocates (zod's
            // `classic/schemas.ts` builds a 269-key spread whose values are
            // `$ZodAny()` etc. — full JS calls). An evacuating minor inside one
            // of those relocates the object, and every later
            // `js_object_set_field_by_name` then writes into abandoned
            // from-space memory, so the fields silently vanish from the copy
            // the caller receives. This is the same rooting contract
            // `Expr::Object` has used since #6951; `ObjectSpread` never got it.
            //
            // A spread part forces protection on its own, independently of
            // whether the spread *expression* collects. `js_object_copy_own_fields`
            // reads every own key of the source, so a source carrying an accessor
            // runs arbitrary user code inside the helper — `{ ...a }` over a plain
            // `LocalGet` answers `false` to `any_may_trigger_gc` and is still a
            // collection point.
            //
            // A plain `js_object_set_field_by_name` on an inert value is NOT one,
            // which is why the rest of the predicate stays byte-identical to
            // `Expr::Object`'s: an allocation inside a runtime helper can never
            // *initiate* a moving collection. `gc_check_trigger`'s minor arm
            // defers to the loop safepoint under `PERRY_GC_MOVING_LOOP_POLLS=1`
            // (`gc/policy.rs`, `GC_SAFEPOINT_PENDING`) and is conservative-scanned
            // or budgeted-non-moving otherwise, so the register stays valid.
            let protect_handle = parts.iter().any(|(k, _)| k.is_none())
                || rooting::any_operand_may_collect(ctx, parts.iter().map(|(_, v)| v));
            rooting::with_rooted_accumulator(
                ctx,
                Repr::Ptr,
                &obj_handle,
                protect_handle,
                |ctx, acc| {
                    for (key_opt, value_expr) in parts {
                        if let Some(key) = key_opt {
                            // Static key:value pair.
                            let v = lower_expr(ctx, value_expr)?;
                            let key_idx = ctx.strings.intern(key);
                            let key_handle_global =
                                format!("@{}", ctx.strings.entry(key_idx).handle_global);
                            let key_raw = {
                                let blk = ctx.block();
                                let key_box = blk.load(DOUBLE, &key_handle_global);
                                let key_bits = blk.bitcast_double_to_i64(&key_box);
                                blk.and(I64, &key_bits, POINTER_MASK_I64)
                            };
                            acc.call_void(
                                ctx,
                                "js_object_set_field_by_name",
                                &[Arg::Plain(I64, &key_raw), Arg::Plain(DOUBLE, &v)],
                            );
                        } else {
                            // `...expr` spread — copy all own fields from the
                            // source object into the accumulator.
                            let src_box = lower_expr(ctx, value_expr)?;
                            acc.call_void(
                                ctx,
                                "js_object_copy_own_fields",
                                &[Arg::Plain(DOUBLE, &src_box)],
                            );
                        }
                    }
                    Ok(())
                },
                |ctx, obj_handle| Ok(nanbox_pointer_inline(ctx.block(), obj_handle)),
            )
        }

        // -------- Object.assign(target, ...sources) --------
        // Per ECMAScript spec, Object.assign mutates `target` by copying each
        // source's own enumerable string- and Symbol-keyed properties, and
        // returns `target` (same identity, class_id, and side-table state
        // preserved). The runtime helper `js_object_assign_one(t, s)` does
        // both copies for one source and returns t. We chain the calls so
        // `target` is evaluated exactly once and threaded through each source.
        // Refs #590.
        Expr::ObjectAssign { target, sources } => {
            let target_box = lower_expr(ctx, target)?;
            let acc = ctx.block().call(
                DOUBLE,
                "js_object_assign_validate_target",
                &[(DOUBLE, &target_box)],
            );
            // Stash target in a temp slot if there are multiple sources, so
            // each helper call uses the same SSA value (defensive: helper
            // returns target_f64 unchanged, but the chain is clearer when we
            // pass target_box explicitly each time — and side-step any LLVM
            // SSA reordering quirks). With zero sources, we still want to
            // return target itself (matching `Object.assign(t)` which is a
            // valid no-op-and-return-target form).
            if sources.is_empty() {
                return Ok(acc);
            }
            // #7200: `acc` is a live object handle across every remaining
            // source's lowering AND across every `js_object_assign_one` call —
            // that helper reads every own key of the source, so an accessor
            // there runs arbitrary user code inside the helper. `Expr::Object`
            // has rooted its accumulator since #6951; this arm never copied it.
            //
            // Unconditional whenever there is a source: the user-code re-entry
            // is inside the callee, so no property of the *source expression*
            // can rule it out. (#7198 declined the "a helper's own allocation
            // initiates a moving collection" argument on evidence; this is the
            // route it accepted instead.)
            rooting::with_rooted_accumulator(
                ctx,
                Repr::Boxed,
                &acc,
                true,
                |ctx, acc| {
                    for src in sources {
                        let src_box = lower_expr(ctx, src)?;
                        // `advance`: the helper returns the post-collection
                        // target address, so publish that back into the root
                        // rather than keeping the pre-call one —
                        // `Object.assign(t, a, b)` threads it into `b`'s link,
                        // and the caller receives it.
                        acc.advance(ctx, "js_object_assign_one", &[Arg::Plain(DOUBLE, &src_box)]);
                    }
                    Ok(())
                },
                |_ctx, acc| Ok(acc.to_string()),
            )
        }

        // -------- new Set(iter) --------
        // Fix #421 (v0.5.574): route through js_set_from_iterable so
        // string inputs (`new Set("abc")`) iterate codepoints instead of
        // segfaulting on a bad ArrayHeader cast. The runtime function
        // takes the NaN-boxed value directly and dispatches by tag.
        Expr::SetNewFromArray(arr_expr) => {
            let arr_box = lower_expr(ctx, arr_expr)?;
            let blk = ctx.block();
            let handle = blk.call(I64, "js_set_from_iterable", &[(DOUBLE, &arr_box)]);
            Ok(nanbox_pointer_inline(blk, &handle))
        }

        // -------- StaticMethodCall --------
        // `MyClass.staticMethod(args)` — look up the synthesized
        // `perry_method_<modprefix>__<class>__<method>` in the methods
        // registry and emit a direct call. Static methods don't take
        // a `this` parameter (unlike instance methods).
        _ => unreachable!("expr/mod.rs dispatched a variant not handled by this submodule"),
    }
}
