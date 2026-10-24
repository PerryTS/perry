pub fn try_lower_class_static_accessor_call(
    ctx: &mut FnCtx<'_>,
    cls_name: &str,
    property: &str,
    callee: &Expr,
    args: &[Expr],
) -> Result<Option<String>> {
    let mut cur = Some(cls_name.to_string());
    while let Some(c) = cur {
        let Some(ci) = ctx.classes.get(&c) else {
            break;
        };
        if ci.static_accessor_names.iter().any(|n| n == property) {
            return try_lower_closure_call_fallthrough(ctx, callee, args);
        }
        cur = ci.extends_name.clone();
    }
    Ok(None)
}

/// A method-call receiver that must be lowered exactly once because
/// re-evaluating it would re-run side effects (or re-construct a fresh
/// value). The closure-call fallthrough otherwise lowers the receiver
/// twice — once for the `this` binding and once inside the full callee
/// PropertyGet — which double-runs these.
fn receiver_must_eval_once(e: &Expr) -> bool {
    matches!(
        e,
        Expr::Call { .. }
            | Expr::CallSpread { .. }
            | Expr::NativeMethodCall { .. }
            | Expr::StaticMethodCall { .. }
            | Expr::New { .. }
            | Expr::NewDynamic { .. }
            | Expr::Await(_)
    )
}

pub fn try_lower_closure_call_fallthrough(
    ctx: &mut FnCtx<'_>,
    callee: &Expr,
    args: &[Expr],
) -> Result<Option<String>> {
    // Fallthrough: assume the callee evaluates to a closure value at
    // runtime and dispatch through `js_closure_call<N>`. This catches:
    //   - LocalGet of an `: any`-typed local that the static check missed
    //   - Nested calls like `curry(1)(2)(3)` where the callee is itself
    //     a Call returning a function
    //   - PropertyGet on a class instance whose property is a closure
    //
    // The runtime checks the closure header on its own — if the value
    // isn't actually a closure, js_closure_call<N> handles the error.
    // Issue #519: when the callee shape is `recv.method(args)` (a
    // PropertyGet) — i.e. a method-style invocation — pass the receiver as
    // the body's `this` (`js_closure_call{N}(closure, this, ..)`). Without it, hono's
    // `RegExpRouter.match = match` (where `match` is an imported function
    // declaration whose body does `this.buildAllMatchers()`) sees
    // `this = undefined` and TypeErrors out at the first chained method call.
    //
    // We evaluate `object` once into a fresh slot so that
    // (a) it's only side-effect-evaluated once, and
    // (b) the lowered `callee` (which re-reads `object` to get the
    //     property) and the receiver passed as `this` are the same
    //     value.
    //
    // The `this`-binding / closure-unbox setup below is arity-independent;
    // only the final dispatch differs. Arities 0..=16 use the per-arity
    // `js_closure_call{N}` register helpers (fast path); arities > 16 (no
    // `js_closure_call17`+ exists) marshal the args through a stack buffer
    // and dispatch via `js_closure_call_array`. Refs #3527 (qs's recursive
    // `stringify` self-calls with 18 args).
    // When the receiver is itself side-effecting (a call / construct /
    // await — e.g. a chained `C.staticMethod(g()).next()`), it MUST be
    // evaluated exactly once. The straightforward shape below lowers
    // `object` for the `this` binding AND lowers the full `callee`
    // PropertyGet for the closure value, which re-lowers `object` and so
    // re-runs its side effects. That double-stepped `g()` in test262
    // `language/{statements,expressions}/class/dstr/async-gen-meth-static-*`
    // (`C.method(g()).next()`). For a side-effecting receiver, evaluate it
    // once here and read the method off that value directly.
    //
    // #5247: capture this call's source byte offset NOW, before the receiver
    // and arguments are lowered (a nested call in either would overwrite the
    // shared pending offset). The `js_closure_unbox_callee_checked` dispatch
    // below throws `TypeError: value is not a function` for a non-callable
    // callee (`const f: any = 5; f()`, nanoid/yup's failure shape), and that
    // throw renders `CURRENT_CALL_LOCATION` via `make_stack`. Replaying the
    // offset as `js_set_call_location` right before the dispatch gives the
    // throw an `at <file>:<line>` frame under `--debug-symbols`. `0` (and the
    // default build) → no emission, unchanged `<anonymous>` frame.
    let call_byte_offset = ctx.strings.pending_call_offset();

    // #7154: this lowering held THREE classes of GC value in bare SSA
    // registers across work that can collect, and a bare register is not a GC
    // root. Under `PERRY_GC_MOVING_LOOP_POLLS=1` a back-edge poll inside any of
    // that work runs an evacuating minor: each value SURVIVES (the capture
    // cell, shadow slot or module global it was read from is a root) and
    // therefore MOVES, the collector rewrites that location, and the register
    // keeps naming from-space.
    //
    //   * the CALLEE, held across the whole argument list. The checked unbox
    //     then masks a pre-move address and `js_closure_callN` reads a closure
    //     header out of abandoned memory — `TypeError: value is not a
    //     function`, the failure shape #7154 has worn since #7184.
    //   * the `this` RECEIVER, held across the read of the callee off it AND
    //     the argument list. #7206 fixed this operand on the sibling
    //     `js_native_call_method_by_id` dispatch; this is the same operand on
    //     the generic one.
    //   * each already-lowered ARGUMENT, held across the arguments that follow
    //     it and across the rebind unbox below.
    //
    // The three windows are NOT the same, which is why they are computed
    // separately rather than protected as one block:
    //
    //   receiver | the callee read + every argument
    //   callee   | every argument
    //   argument | the arguments after it + the rebind unbox
    //
    // The rebind unbox is a collection point that only exists on the
    // member-shaped path: `js_closure_unbox_callee_checked_rebind` calls
    // `clone_closure_rebind_this`, which allocates a fresh closure
    // (`closure/dynamic_props.rs:1040`) when the callee captures `this`. It
    // sits BELOW the last argument and ABOVE `js_closure_callN`, so the
    // arguments are re-read after it, not before — see
    // `RootedOperands::reread_one`. The receiverless path takes
    // `js_closure_unbox_callee_checked`, which is a tag check and a mask and
    // allocates nothing, so `f(x, y)` on inert operands emits exactly the IR it
    // emitted before this change.
    //
    // Temp roots, not re-lowering: re-lowering the callee or the receiver would
    // observe an assignment made by an argument, which is a miscompile rather
    // than a rooting fix.
    //
    // This is the lowering that named the missing combinator. It consumes the
    // group in TWO instructions with an allocating step between them, so it
    // needs a re-read at two points, and every `with_operands_rooted*` form has
    // exactly one. The scope now belongs to `with_rooted_group`; the body is a
    // separate function only so that owning the release costs a wrapper rather
    // than a 260-line reindentation.
    with_rooted_group(ctx, args.len() + 2, |ctx, group| {
        lower_closure_call_rooted(ctx, group, callee, args, call_byte_offset)
    })
}

/// The rooted body of [`try_lower_closure_call_fallthrough`].
///
/// Everything it protects lives in `group`, which its caller releases on every
/// path out — including this function's `?`.
fn lower_closure_call_rooted<'a>(
    ctx: &mut FnCtx<'_>,
    group: &mut crate::rooting::RootedGroup<'a>,
    callee: &'a Expr,
    args: &'a [Expr],
    call_byte_offset: u32,
) -> Result<Option<String>> {
    let arg_collects: Vec<bool> = args.iter().map(|a| operand_may_collect(ctx, a)).collect();
    let any_arg_collects = arg_collects.iter().any(|&c| c);
    // Reading the callee off the receiver: a by-name property get walks a
    // prototype chain and can run an accessor, so it is a collection point in
    // the receiver's window (and only in the receiver's).
    let callee_read_collects = operand_may_collect(ctx, callee);

    // Operands are recorded in the order their values are produced — receiver,
    // callee, then arguments — because the group roots each one BEFORE the next
    // is lowered. Rooting a finished list afterwards is worse than doing
    // nothing: by then an earlier operand may already have been swept and the
    // push publishes a dangling pointer into a slot the collector scans.
    let prelowered_recv: Option<(String, String)> =
        if let Expr::PropertyGet {
            object, property, ..
        } = callee
        {
            if receiver_must_eval_once(object.as_ref()) {
                Some((lower_expr(ctx, object)?, property.clone()))
            } else {
                None
            }
        } else {
            None
        };

    // The receiver expression, when this call binds one. Skip the
    // method-binding when the receiver is a global, namespace import, or
    // NativeModuleRef — those aren't user objects and shouldn't influence
    // `this`. (`receiver_must_eval_once` never matches those forms, so the
    // prelowered arm cannot disagree with this test.)
    let method_recv_expr: Option<&Expr> = match callee {
        Expr::PropertyGet { object, .. } => {
            if prelowered_recv.is_some() {
                Some(object.as_ref())
            } else if matches!(
                object.as_ref(),
                Expr::GlobalGet(_) | Expr::NativeModuleRef(_) | Expr::ExternFuncRef { .. }
            ) {
                None
            } else {
                Some(object.as_ref())
            }
        }
        _ => None,
    };

    let method_recv: Option<String> = match method_recv_expr {
        Some(obj_expr) => {
            let v = match prelowered_recv {
                Some((ref obj_v, _)) => obj_v.clone(),
                None => lower_expr(ctx, obj_expr)?,
            };
            // Rooted here, before the callee read below: nothing has collected
            // between the lowering above and this push. `adopt` rather than
            // `lower`, because the prelowered arm produced this value itself
            // and the receiver must be evaluated exactly once.
            group.adopt(ctx, obj_expr, &v, callee_read_collects || any_arg_collects);
            Some(v)
        }
        None => None,
    };
    let recv_slot = method_recv.as_ref().map(|_| 0usize);
    let callee_slot = if recv_slot.is_some() { 1 } else { 0 };

    let recv_box = if let Some((ref obj_v, ref property)) = prelowered_recv {
        // Read `property` off the once-lowered receiver value via the
        // generic by-name getter (walks the prototype chain, so
        // `gen.next` etc. resolve). Mirrors the dynamic by-name read in
        // `expr::property_get::lower`.
        let key_idx = ctx.strings.intern(property);
        let key_handle_global = format!("@{}", ctx.strings.entry(key_idx).handle_global);
        let blk = ctx.block();
        let obj_bits = blk.bitcast_double_to_i64(obj_v);
        let key_box = blk.load(DOUBLE, &key_handle_global);
        let key_bits = blk.bitcast_double_to_i64(&key_box);
        let key_handle = blk.and(I64, &key_bits, POINTER_MASK_I64);
        blk.call(
            DOUBLE,
            "js_object_get_field_by_name_f64",
            &[(I64, &obj_bits), (I64, &key_handle)],
        )
    } else {
        lower_expr(ctx, callee)?
    };
    // `adopt`: on the prelowered arm the callee's value is the hand-emitted
    // by-name read above, not `lower_expr(callee)`.
    group.adopt(ctx, callee, &recv_box, any_arg_collects);

    // The rebind unbox allocates (see the header above), and it sits between
    // the last argument and `js_closure_callN`, so every argument's window
    // includes it. Receiverless calls take the non-allocating unbox and keep
    // their old IR.
    let rebind_allocates = method_recv.is_some();
    for (i, a) in args.iter().enumerate() {
        let collects = rebind_allocates || arg_collects[i + 1..].iter().any(|&c| c);
        group.lower(ctx, a, collects)?;
    }

    // Re-read the receiver and the callee HERE: below every argument, above the
    // unbox that consumes them. Mandatory, not defensive — the temp-root slot
    // is a MUTABLE root, so an evacuating cycle rewrites it and the register
    // pushed beforehand is stale.
    let method_recv: Option<String> = match recv_slot {
        Some(i) => Some(group.reread(ctx, i)?),
        None => None,
    };
    let recv_box = group.reread(ctx, callee_slot)?;

    // #5247: record the source location right before the throw-capable
    // dispatch — after the receiver/args are lowered, so a nested-call
    // argument's location no longer shadows this one. Applies to both arity
    // branches below (the checked unbox throws in either). No-op default build.
    crate::expr::calls::emit_call_location_at(ctx, call_byte_offset);

    // #5504: tag-check the callee before masking to a closure pointer.
    // A non-callable value (number/string/bool/null/undefined) whose
    // low-48 bits form an in-range address would otherwise be handed to
    // `js_closure_callN` as a wild `*const ClosureHeader` and SIGSEGV on
    // the header read. The checked unbox throws `TypeError: value is not
    // a function` for any non-`POINTER_TAG` value.
    // #6475: a member-shaped call (`o.m(args)`) must rebind an
    // object-literal method's baked `this` capture slot to the receiver —
    // the slot wins over the `this` parameter passed below, so a method
    // inherited via `Object.setPrototypeOf(obj, proto)` otherwise runs
    // with `this` bound to the proto literal (effect's Pipeable
    // `TagClass.pipe(...)` composed against the wrong `this` and
    // HttpApiBuilder.group returned a curried function instead of a
    // Layer). The rebind variant is a no-op for closures that don't
    // capture `this`, so plain functions and arrows are untouched;
    // receiverless calls keep the plain checked unbox.
    //
    // #7154: this is also the collection point that sits between the arguments
    // and the dispatch. `clone_closure_rebind_this` ALLOCATES a replacement
    // closure when the callee captures `this`, so the arguments are re-read
    // below it — hoisting the unbox above the argument list instead is not an
    // option, because its throw is observable and the spec evaluates arguments
    // before it.
    let closure_handle = match method_recv {
        Some(ref this_val) => ctx.block().call(
            I64,
            "js_closure_unbox_callee_checked_rebind",
            &[(DOUBLE, &recv_box), (DOUBLE, this_val)],
        ),
        // S2: the POINTER_TAG hit is inline; only a non-callable value calls
        // (and throws from) the checked unbox. See `ic_fast_split.rs`.
        None => crate::expr::ic_fast_split::emit_checked_callee_unbox(ctx, &recv_box),
    };

    // Re-read the arguments BELOW the unbox. `closure_handle` itself is a raw
    // pointer in a register, but nothing between here and the dispatch can
    // collect, so it needs no protection of its own.
    let arg_base = callee_slot + 1;
    let mut lowered_args: Vec<String> = Vec::with_capacity(args.len());
    for i in 0..args.len() {
        lowered_args.push(group.reread(ctx, arg_base + i)?);
    }

    let result = if recv_slot.is_some() {
        // A member-shaped call binds its receiver: re-read it from its root
        // below the (allocating) rebind unbox and pass it as the body's
        // `this`.
        let this_val = group.reread(ctx, 0)?;
        let this_bits = ctx.block().bitcast_double_to_i64(&this_val);
        super::emit_closure_handle_call_this(ctx, &closure_handle, &this_bits, &lowered_args)
    } else {
        // A plain call: `js_closure_call{N}` hands the body `undefined`
        // (OrdinaryCallBindThis; a sloppy body coerces it itself).
        // #3527: > 16 args marshal into an entry-block `[N x double]` buffer and
        // dispatch through the variadic `js_closure_call_array`.
        //
        // #7154: the stores happen below the unbox now. A stack buffer is not a
        // GC root, so filling it above an allocating rebind would freeze
        // pre-move addresses into it — the same staleness one indirection
        // further out. The stores have no observable effect, so moving them
        // below the throw-capable unbox changes nothing else.
        super::emit_closure_handle_call(ctx, &closure_handle, &lowered_args)
    };

    // The group is released by `with_rooted_group` AFTER this returns, which is
    // after the dispatch — the dispatcher allocates while it reads these values.
    Ok(Some(result))
}
